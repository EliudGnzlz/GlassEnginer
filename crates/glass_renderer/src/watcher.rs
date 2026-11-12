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

#[derive(Debug, Clone)]
pub enum AssetReloadEvent {
    MeshReloaded(PathBuf),
    ShaderReloaded(PathBuf),
}

pub struct AssetWatcher {
    _watcher: RecommendedWatcher,
    pub receiver: Receiver<AssetReloadEvent>,
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

impl AssetWatcher {
    pub fn new(assets_dir: impl AsRef<Path>) -> Result<Self, String> {
        let (tx, rx) = channel();
        // Canonizar: los eventos del vigía heredan el prefijo con el
        // que se registra la carpeta; si es absoluta, todas las rutas
        // de la recarga lo son.
        let path = std::fs::canonicalize(assets_dir.as_ref())
            .unwrap_or_else(|_| assets_dir.as_ref().to_path_buf());
        if let Some(raiz) = path.parent() {
            let _ = BRIDGE_PATH.set(raiz.join("tools").join("headless_blender_bridge.py"));
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

        Ok(Self {
            _watcher: watcher,
            receiver: rx,
        })
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
                        // Una sola exportación en vuelo por ruta: la que
                        // está en curso re-comprueba el hash al terminar.
                        let Some(en_vuelo) = EnVuelo::intentar(&path) else {
                            log::info!(
                                "Exportación ya en curso para {:?}: se ignora el evento \
                                 (se re-comprueba al terminar)",
                                path
                            );
                            continue;
                        };
                        // Fase 6: sin cambio de contenido no hay
                        // re-exportación (los guardados en ráfaga de
                        // Blender reescriben el archivo sin tocar la
                        // escena).
                        let hash_inicial = match contenido_cambio(&path) {
                            None => {
                                log::info!(
                                    "Sin cambios de contenido (SHA-256 idéntico), no se re-exporta: {:?}",
                                    path
                                );
                                continue; // `en_vuelo` se suelta y se retira.
                            }
                            Some(h) => h,
                        };
                        let glb_path = path.with_extension("glb");
                        let input_path = path.clone();
                        let tx_clone = tx.clone();

                        // Ejecutar conversión desatendida en un hilo separado para no bloquear
                        thread::spawn(move || {
                            let _en_vuelo = en_vuelo;
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
                                let _ = tx_clone
                                    .send(AssetReloadEvent::MeshReloaded(glb_path.clone()));
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
