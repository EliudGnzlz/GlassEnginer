use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::thread;

/// SHA-256 del contenido ya exportado por ruta (Fase 6): si un
/// guardado no cambia el contenido, no se re-exporta.
static HASHES_EXPORTADOS: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

/// SHA-256 hexadecimal de un archivo (None si no se puede leer).
fn sha256_de_archivo(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Some(format!("{:x}", h.finalize()))
}

/// Consulta pura (no registra nada):
/// - `None`: el hash coincide con el último exportado con éxito.
/// - `Some(Some(hash))`: el contenido difiere; `hash` es el actual.
/// - `Some(None)`: no se pudo leer; se intenta la exportación igualmente.
fn contenido_cambio(path: &Path) -> Option<Option<String>> {
    let Some(hash) = sha256_de_archivo(path) else {
        return Some(None);
    };
    let mut guard = HASHES_EXPORTADOS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    if map.get(path) == Some(&hash) {
        None
    } else {
        Some(Some(hash))
    }
}

/// Registra `hash` como último contenido exportado con éxito de `path`.
fn registrar_exportado(path: &Path, hash: String) {
    let mut guard = HASHES_EXPORTADOS.lock().unwrap();
    guard
        .get_or_insert_with(HashMap::new)
        .insert(path.to_path_buf(), hash);
}

/// Fuentes (.blend/.fbx) con exportación en curso.
static IN_FLIGHT: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

/// Máximo de rondas de exportación si la fuente cambia durante la exportación.
const MAX_RONDAS_EXPORTACION: u32 = 3;

/// Marca de exportación en vuelo; se retira al soltarse (incluye errores y pánicos).
struct EnVuelo(PathBuf);

impl EnVuelo {
    /// Inserta `path` en IN_FLIGHT; `None` si ya estaba en vuelo.
    fn intentar(path: &Path) -> Option<Self> {
        let mut guard = IN_FLIGHT.lock().unwrap();
        let set = guard.get_or_insert_with(HashSet::new);
        if set.insert(path.to_path_buf()) {
            Some(EnVuelo(path.to_path_buf()))
        } else {
            None
        }
    }
}

impl Drop for EnVuelo {
    fn drop(&mut self) {
        if let Ok(mut guard) = IN_FLIGHT.lock() {
            if let Some(set) = guard.as_mut() {
                set.remove(&self.0);
            }
        }
    }
}

/// true si el .glb es la salida de una exportación en vuelo.
fn fuente_en_vuelo(glb: &Path) -> bool {
    let guard = IN_FLIGHT.lock().unwrap();
    match guard.as_ref() {
        Some(set) => {
            set.contains(&glb.with_extension("blend")) || set.contains(&glb.with_extension("fbx"))
        }
        None => false,
    }
}

/// Resumen del resultado del bridge: última línea GLASS_BRIDGE_RESULT o
/// los últimos 300 caracteres de stdout.
fn resumen_bridge(stdout: &str) -> String {
    if let Some(linea) = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("GLASS_BRIDGE_RESULT"))
    {
        return linea.trim().to_string();
    }
    let n = stdout.chars().count();
    stdout.chars().skip(n.saturating_sub(300)).collect()
}

/// Ejecuta el bridge una vez; true si tuvo éxito.
fn ejecutar_bridge(bridge_script: &Path, input_path: &Path, glb_path: &Path) -> bool {
    let output = Command::new("python3")
        .args([
            bridge_script.to_string_lossy().into_owned(),
            input_path.to_string_lossy().into_owned(),
            glb_path.to_string_lossy().into_owned(),
        ])
        .output();

    match output {
        Ok(res) if res.status.success() => {
            log::info!(
                "Conversión headless exitosa: {:?} -> {:?}",
                input_path,
                glb_path
            );
            true
        }
        Ok(res) => {
            let stdout = String::from_utf8_lossy(&res.stdout);
            log::error!(
                "Fallo en conversión Blender ({:?}): {}\nstderr:\n{}",
                input_path,
                resumen_bridge(&stdout),
                String::from_utf8_lossy(&res.stderr)
            );
            false
        }
        Err(e) => {
            log::error!("Fallo al invocar python bridge: {}", e);
            false
        }
    }
}

/// Lanza la conversión .blend/.fbx -> .glb hermano en un hilo propio y
/// emite `MeshReloaded` al terminar cada ronda con éxito.
/// Cerrojo global de conversión: como mucho UN proceso de Blender a la
/// vez en todo el motor. Sin él, una ráfaga de peticiones (muchos
/// archivos guardados a la vez, varias solicitudes de la escena) lanzaba
/// un Blender por archivo y agotaba la memoria del proceso.
static CERROJO_CONVERSION: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lanzar_conversion(path: &Path, tx: &Sender<AssetReloadEvent>) {
    // Una sola exportación en vuelo por ruta: la que
    // está en curso re-comprueba el hash al terminar.
    let Some(en_vuelo) = EnVuelo::intentar(path) else {
        log::info!(
            "Exportación ya en curso para {:?}: se ignora el evento \
             (se re-comprueba al terminar)",
            path
        );
        return;
    };
    // Fase 6: sin cambio de contenido no hay
    // re-exportación (los guardados en ráfaga de
    // Blender reescriben el archivo sin tocar la
    // escena).
    let hash_inicial = match contenido_cambio(path) {
        None => {
            log::info!(
                "Sin cambios de contenido (SHA-256 idéntico), no se re-exporta: {:?}",
                path
            );
            return; // `en_vuelo` se suelta y se retira.
        }
        Some(h) => h,
    };
    let glb_path = path.with_extension("glb");
    let input_path = path.to_path_buf();
    let tx_clone = tx.clone();

    // Ejecutar conversión desatendida en un hilo separado para no bloquear
    thread::spawn(move || {
        let _en_vuelo = en_vuelo;
        // Esperar el turno: un solo Blender a la vez (ver CERROJO_CONVERSION).
        let _turno = CERROJO_CONVERSION
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let bridge_script = bridge_path();
        if !bridge_script.exists() {
            // Fase 6: nada de fallos silenciosos.
            log::error!(
                "Bridge de Blender no encontrado en '{}': no se exporta {:?}. \
                 El .glb queda como estaba.",
                bridge_script.display(),
                input_path
            );
            return;
        }
        let mut hash_previo = hash_inicial;
        for ronda in 1..=MAX_RONDAS_EXPORTACION {
            if !ejecutar_bridge(&bridge_script, &input_path, &glb_path) {
                // Sin registro: el próximo guardado reintenta.
                break;
            }
            if let Some(h) = &hash_previo {
                registrar_exportado(&input_path, h.clone());
            }
            let _ = tx_clone.send(AssetReloadEvent::MeshReloaded(glb_path.clone()));
            let actual = sha256_de_archivo(&input_path);
            if actual.is_none() || actual == hash_previo {
                break;
            }
            if ronda == MAX_RONDAS_EXPORTACION {
                log::warn!(
                    "{:?} siguió cambiando tras {} rondas de exportación; \
                     el próximo guardado reintentará",
                    input_path,
                    MAX_RONDAS_EXPORTACION
                );
                break;
            }
            log::info!(
                "{:?} cambió durante la exportación: se repite (ronda {})",
                input_path,
                ronda + 1
            );
            hash_previo = actual;
        }
    });
}

/// true si el .glb hermano no existe o es más antiguo que la fuente.
fn glb_desactualizado(fuente: &Path) -> bool {
    let glb = fuente.with_extension("glb");
    let Ok(meta_glb) = std::fs::metadata(&glb) else {
        return true;
    };
    let (Ok(meta_fuente), Ok(mtime_glb)) = (std::fs::metadata(fuente), meta_glb.modified()) else {
        return true;
    };
    match meta_fuente.modified() {
        Ok(mtime_fuente) => mtime_fuente > mtime_glb,
        Err(_) => true,
    }
}

fn es_fuente_modelo(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("blend") | Some("fbx")
    )
}

/// Recorre el árbol registrando los modelos YA convertidos: por cada
/// fuente .blend/.fbx con un .glb al día emite `MeshReloaded`, para que
/// el modelo entre en el caché aunque el editor acabe de arrancar (la
/// precarga solo cubre `assets/models/`).
///
/// El escaneo NO convierte: convertir aquí lanzaría un Blender por cada
/// fuente sin .glb del árbol (cientos en paquetes de assets) y tumbaría
/// el proceso por memoria. Las fuentes sin convertir se convierten al
/// soltarlas en la escena (`AssetWatcher::solicitar_conversion`) o al
/// guardarse con el editor en marcha (el camino de eventos de siempre).
///
/// Con `incluir_modelos = false` se salta la carpeta `models/` de la
/// raíz: es el territorio de la precarga diferida, y emitir sus modelos
/// aquí los sube a GPU dos veces durante el arranque (la segunda subida
/// pisa los buffers de la primera y el proceso muere en el
/// rasterizador). El vigía inicial usa `false`; los de proyecto, `true`
/// (ninguna precarga cubre los modelos de un proyecto abierto).
fn escanear_fuentes(
    dir: &Path,
    tx: &Sender<AssetReloadEvent>,
    es_raiz: bool,
    incluir_modelos: bool,
) {
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return;
    };
    for entrada in entradas.flatten() {
        let path = entrada.path();
        if path.is_dir() {
            if es_raiz
                && !incluir_modelos
                && path
                    .file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("models"))
            {
                continue;
            }
            escanear_fuentes(&path, tx, false, incluir_modelos);
        } else if es_fuente_modelo(&path) && !glb_desactualizado(&path) {
            let _ = tx.send(AssetReloadEvent::MeshReloaded(
                path.with_extension("glb"),
            ));
        }
    }
}

/// true si `path` es una fuente .blend/.fbx cuyo .glb hermano falta o
/// está desactualizado: para ver el modelo real hay que convertirla.
pub fn necesita_conversion(path: &Path) -> bool {
    es_fuente_modelo(path) && glb_desactualizado(path)
}

#[derive(Debug, Clone)]
pub enum AssetReloadEvent {
    MeshReloaded(PathBuf),
    ShaderReloaded(PathBuf),
}

pub struct AssetWatcher {
    _watcher: RecommendedWatcher,
    pub receiver: Receiver<AssetReloadEvent>,
    sender: Sender<AssetReloadEvent>,
}

/// Ruta absoluta del bridge de Blender, fijada al crear el vigía a
/// partir de la raíz del proyecto (el padre de `assets/`). La cadena
/// de recarga nunca trabaja con rutas relativas: algún eslabón
/// resolvía contra el `$PWD` del lanzador y la exportación fallaba
/// al abrir la app desde otro directorio (defecto de la Fase 10).
static BRIDGE_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn bridge_path() -> PathBuf {
    BRIDGE_PATH
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("tools/headless_blender_bridge.py"))
}

/// Ruta del bridge de Blender bajo una raíz dada
/// (`<raiz>/tools/headless_blender_bridge.py`) si el archivo existe;
/// `None` en otro caso. Función pura: la decisión es comprobable sin
/// tocar el estado global.
pub fn ruta_bridge_en_raiz(raiz: &Path) -> Option<PathBuf> {
    let candidato = raiz.join("tools").join("headless_blender_bridge.py");
    candidato.is_file().then_some(candidato)
}

/// Fija la ruta global del bridge a partir de una raíz (la del motor o
/// la del proyecto vigilado): solo si el bridge EXISTE ahí — una raíz
/// sin `tools/` no debe secuestrar la ruta. La primera raíz válida
/// manda (OnceLock). Sin esta fijación desde la raíz del motor, en la
/// build instalada el bridge se buscaba relativo al directorio de
/// trabajo del proceso hijo y la conversión .blend/.fbx nunca ocurría
/// (regresión de la separación Hub/motor, 2026-11-18).
pub fn fijar_bridge_desde_raiz(raiz: &Path) {
    if let Some(candidato) = ruta_bridge_en_raiz(raiz) {
        let _ = BRIDGE_PATH.set(candidato);
    }
}

impl AssetWatcher {
    pub fn new(
        assets_dir: impl AsRef<Path>,
        incluir_modelos: bool,
    ) -> Result<Self, String> {
        let (tx, rx) = channel();
        let tx_scan = tx.clone();
        let sender = tx.clone();
        // Canonizar: los eventos del vigía heredan el prefijo con el
        // que se registra la carpeta; si es absoluta, todas las rutas
        // de la recarga lo son.
        let path = std::fs::canonicalize(assets_dir.as_ref())
            .unwrap_or_else(|_| assets_dir.as_ref().to_path_buf());
        if let Some(raiz) = path.parent() {
            // Solo fijar la ruta si el bridge EXISTE ahí: un vigía creado
            // para la carpeta de un proyecto de usuario (que no trae
            // `tools/`) no debe secuestrar la ruta del bridge del motor.
            fijar_bridge_desde_raiz(raiz);
        }

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| match res {
                Ok(event) => {
                    Self::handle_event(event, &tx);
                }
                Err(e) => {
                    log::warn!("Error en vigía de assets: {:?}", e);
                }
            },
            Config::default(),
        )
        .map_err(|e| format!("Fallo al inicializar vigía de assets: {}", e))?;

        watcher
            .watch(&path, RecursiveMode::Recursive)
            .map_err(|e| format!("Fallo al observar directorio {:?}: {}", path, e))?;

        log::info!("AssetWatcher iniciado en {:?}", path);

        // Los modelos ya convertidos que ya estaban en la carpeta no
        // generan eventos: se registran aquí (sin convertir nada).
        escanear_fuentes(&path, &tx_scan, true, incluir_modelos);

        Ok(Self {
            _watcher: watcher,
            receiver: rx,
            sender,
        })
    }

    /// Pide la conversión de una fuente .blend/.fbx concreta (p. ej. la
    /// que el usuario acaba de soltar en la escena sin que su .glb
    /// existiera): la conversión va en su propio hilo y al terminar
    /// emite `MeshReloaded`, con lo que la entidad pasa a dibujar el
    /// modelo real.
    pub fn solicitar_conversion(&self, path: &Path) {
        if es_fuente_modelo(path) {
            lanzar_conversion(path, &self.sender);
        }
    }

    fn handle_event(event: Event, tx: &Sender<AssetReloadEvent>) {
        if !matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
            return;
        }

        for path in event.paths {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                match ext {
                    // El FBX entra por el mismo puente que el .blend: Blender
                    // lo importa de forma nativa y lo exporta a .glb, que es
                    // el formato que el motor carga. Así el FBX tiene recarga
                    // en caliente igual que el .blend, sin un importador
                    // binario propio que mantener.
                    "blend" | "fbx" => {
                        log::info!("Detectado cambio en archivo de modelo (blend/fbx): {:?}", path);
                        lanzar_conversion(&path, tx);
                    }
                    "glb" => {
                        if path.to_string_lossy().contains(".glass-tmp") || fuente_en_vuelo(&path) {
                            log::info!(
                                "GLB escrito por una exportación en vuelo, se ignora: {:?}",
                                path
                            );
                            continue;
                        }
                        log::info!("Detectado archivo glTF binario modificado: {:?}", path);
                        let _ = tx.send(AssetReloadEvent::MeshReloaded(path));
                    }
                    "vert" | "frag" | "spv" => {
                        log::info!("Detectado shader modificado: {:?}", path);
                        let _ = tx.send(AssetReloadEvent::ShaderReloaded(path));
                    }
                    _ => (),
                }
            }
        }
    }
}

/// Busca `<stem>.glb` recorriendo `raices` en orden y en profundidad
/// (subcarpetas incluidas, recorrido ordenado para ser determinista).
/// Devuelve la primera ruta cuyo file_stem sea exactamente `stem`.
pub fn resolver_glb_por_stem(raices: &[&std::path::Path], stem: &str) -> Option<std::path::PathBuf> {
    for raiz in raices {
        let mut pending: Vec<std::path::PathBuf> = vec![raiz.to_path_buf()];
        while let Some(dir) = pending.pop() {
            let lectura = match std::fs::read_dir(&dir) {
                Ok(l) => l,
                Err(_) => continue,
            };
            let mut entradas: Vec<std::path::PathBuf> =
                lectura.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            entradas.sort();
            for ruta in entradas {
                if ruta.is_dir() {
                    pending.push(ruta);
                } else if ruta.extension().and_then(|e| e.to_str()) == Some("glb")
                    && ruta.file_stem().and_then(|s| s.to_str()) == Some(stem)
                {
                    return Some(ruta);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{resolver_glb_por_stem, ruta_bridge_en_raiz};

    #[test]
    fn resolver_glb_encuentra_en_subcarpeta_y_respeta_orden() {
        let base = std::env::temp_dir()
            .join(format!("glass_resolver_glb_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let raiz1 = base.join("raiz1");
        let raiz2 = base.join("raiz2");
        std::fs::create_dir_all(&raiz1).unwrap();
        std::fs::create_dir_all(raiz2.join("sub")).unwrap();
        std::fs::write(raiz1.join("a.glb"), b"").unwrap();
        std::fs::write(raiz2.join("a.glb"), b"").unwrap();
        std::fs::write(raiz2.join("sub").join("b.glb"), b"").unwrap();

        let raices = [raiz1.as_path(), raiz2.as_path()];
        assert_eq!(
            resolver_glb_por_stem(&raices, "b"),
            Some(raiz2.join("sub").join("b.glb"))
        );
        assert_eq!(resolver_glb_por_stem(&raices, "a"), Some(raiz1.join("a.glb")));
        assert_eq!(resolver_glb_por_stem(&raices, "zz"), None);

        let _ = std::fs::remove_dir_all(&base);
    }
    use std::path::PathBuf;

    fn raiz_temporal(nombre: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ge_bridge_test_{}_{}",
            nombre,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sin_tools_no_hay_bridge() {
        let raiz = raiz_temporal("sin_tools");
        assert_eq!(ruta_bridge_en_raiz(&raiz), None);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn tools_sin_el_archivo_no_hay_bridge() {
        let raiz = raiz_temporal("tools_vacia");
        std::fs::create_dir_all(raiz.join("tools")).unwrap();
        assert_eq!(ruta_bridge_en_raiz(&raiz), None);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn con_el_archivo_devuelve_su_ruta() {
        let raiz = raiz_temporal("con_bridge");
        let tools = raiz.join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        let bridge = tools.join("headless_blender_bridge.py");
        std::fs::write(&bridge, b"# bridge").unwrap();
        assert_eq!(ruta_bridge_en_raiz(&raiz), Some(bridge));
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn un_directorio_con_el_nombre_no_cuenta() {
        let raiz = raiz_temporal("bridge_es_dir");
        std::fs::create_dir_all(
            raiz.join("tools").join("headless_blender_bridge.py"),
        )
        .unwrap();
        assert_eq!(ruta_bridge_en_raiz(&raiz), None);
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
