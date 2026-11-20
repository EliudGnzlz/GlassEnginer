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
    if map.get(&sin_prefijo_verbatim(path)) == Some(&hash) {
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
        .insert(sin_prefijo_verbatim(path), hash);
}

/// Quita el prefijo verbatim de Windows (`\\?\`, `\\?\UNC\`) de una ruta.
///
/// `std::fs::canonicalize` devuelve en Windows rutas verbatim
/// (`\\?\C:\...`): el lado Rust las maneja sin problema y protegen
/// las rutas largas, pero el puente de Blender no las tolera — el
/// exportador glTF crea el directorio de salida con `os.makedirs`
/// sobre el directorio verbatim con una '/' final y muere con
/// WinError 123 (defecto P4, diagnosticado con el `glass.log` de un
/// usuario real). La normalización se aplica SOLO donde una ruta va
/// a salir del proceso hacia Blender/Python (los argumentos de
/// `ejecutar_bridge_con_blender`) y a las claves de deduplicación
/// (`EnVuelo`, `HASHES_EXPORTADOS`), para que la forma verbatim y
/// la plana del mismo archivo sean la misma clave. Todo lo demás
/// sigue en verbatim. Sin prefijo, devuelve la ruta tal cual.
pub fn sin_prefijo_verbatim(path: &Path) -> PathBuf {
    let texto = path.to_string_lossy();
    let (resto, es_unc) = if let Some(r) = texto.strip_prefix(r"\\?\UNC\") {
        (r, true)
    } else if let Some(r) = texto.strip_prefix(r"\\?\") {
        // Solo se quita si detrás viene una unidad (`C:`): es la
        // forma que produce `canonicalize` para rutas de disco.
        let mut chars = r.chars();
        match (chars.next(), chars.next()) {
            (Some(letra), Some(':')) if letra.is_ascii_alphabetic() => (r, false),
            _ => return path.to_path_buf(),
        }
    } else {
        return path.to_path_buf();
    };
    // Seguridad: en la forma verbatim, una '/' dentro de la ruta,
    // los componentes '.'/'..' y los puntos o espacios finales de
    // un componente son literales; en la forma plana, Win32 los
    // reinterpretaría y la ruta pasaría a apuntar a otro archivo.
    // Ante cualquiera de esos casos se deja la ruta como está:
    // fallará en el exportador como hasta hoy, pero nunca se
    // convertirá OTRO archivo por error.
    let inseguro = resto.contains('/')
        || resto
            .split('\\')
            .any(|c| c == "." || c == ".." || c.ends_with('.') || c.ends_with(' '));
    if inseguro {
        return path.to_path_buf();
    }
    if es_unc {
        PathBuf::from(format!(r"\\{resto}"))
    } else {
        PathBuf::from(resto)
    }
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
        // Clave normalizada: la forma verbatim y la plana del mismo
        // archivo dedupen juntas (P4).
        let clave = sin_prefijo_verbatim(path);
        if set.insert(clave.clone()) {
            Some(EnVuelo(clave))
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
    // Las claves de IN_FLIGHT están normalizadas (sin verbatim): la
    // consulta también, para comparar la misma forma (P4).
    let glb = sin_prefijo_verbatim(glb);
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

/// Salida de una ejecución del bridge bajo Blender.
pub struct SalidaBridge {
    pub exito: bool,
    pub stdout: String,
    pub stderr: String,
}

const MENSAJE_SIN_BLENDER: &str =
    "No se encontró el ejecutable de Blender: instala Blender, define GLASS_BLENDER con su ruta, o descarga el Blender portable desde el Hub (Instalaciones → «Descargar Blender portable para el motor»).";

/// Resuelve el ejecutable de Blender: `GLASS_BLENDER` (si es un
/// archivo) → búsqueda en el PATH → rutas de instalación habituales
/// de la plataforma → copia portable descargada por el Hub en la
/// carpeta del motor (último recurso).
/// `None` si no hay Blender localizable.
pub fn resolver_blender() -> Option<std::path::PathBuf> {
    let env_val = std::env::var("GLASS_BLENDER").ok();
    let dirs_path: Vec<std::path::PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let mut candidatos = candidatos_instalacion();
    if let Some(local) = candidato_blender_local() {
        candidatos.push(local);
    }
    resolver_blender_desde(env_val.as_deref(), &dirs_path, &candidatos)
}

/// Núcleo puro de `resolver_blender`: env → PATH → candidatos.
/// Un valor solo cuenta si apunta a un archivo existente.
fn resolver_blender_desde(
    env_val: Option<&str>,
    dirs_path: &[std::path::PathBuf],
    candidatos: &[std::path::PathBuf],
) -> Option<std::path::PathBuf> {
    if let Some(v) = env_val {
        let p = std::path::PathBuf::from(v);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(p) = buscar_blender_en_path(dirs_path) {
        return Some(p);
    }
    candidatos.iter().find(|c| c.is_file()).cloned()
}

/// Devuelve la ruta esperada del Blender portable dentro de la raíz
/// del motor: `<raiz>/tools/blender/blender.exe` en Windows y
/// `<raiz>/tools/blender/blender` en el resto de plataformas.
pub fn ruta_blender_en_raiz(raiz: &std::path::Path) -> std::path::PathBuf {
    let nombre = if cfg!(windows) { "blender.exe" } else { "blender" };
    raiz.join("tools").join("blender").join(nombre)
}

/// Ruta del Blender portable que el Hub descarga junto al motor:
/// se compone a partir del directorio padre del ejecutable actual.
fn candidato_blender_local() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let raiz = exe.parent()?;
    Some(ruta_blender_en_raiz(raiz))
}

/// Busca `blender` (y en Windows también `blender.exe`) en los directorios dados.
fn buscar_blender_en_path(dirs_path: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    let nombres: &[&str] = if cfg!(windows) {
        &["blender.exe", "blender"]
    } else {
        &["blender"]
    };
    dirs_path
        .iter()
        .flat_map(|d| nombres.iter().map(move |n| d.join(n)))
        .find(|p| p.is_file())
}

/// Componentes numéricos de "Blender X.Y" para ordenar por versión (4.10 > 4.2).
fn clave_version_blender(nombre: &str) -> Vec<u64> {
    nombre
        .trim_start_matches("Blender ")
        .split('.')
        .map(|s| s.trim().parse().unwrap_or(0))
        .collect()
}

/// Candidatos de instalación en Windows a partir de las bases de Program Files.
fn candidatos_windows(bases: &[std::path::PathBuf]) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for base in bases {
        let mut subs: Vec<(String, std::path::PathBuf)> =
            std::fs::read_dir(base.join("Blender Foundation"))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter_map(|e| {
                    let nombre = e.file_name().to_string_lossy().into_owned();
                    nombre.starts_with("Blender ").then(|| (nombre, e.path()))
                })
                .collect();
        subs.sort_by(|(a, _), (b, _)| {
            clave_version_blender(b)
                .cmp(&clave_version_blender(a))
                .then_with(|| b.cmp(a))
        });
        out.extend(subs.into_iter().map(|(_, sub)| sub.join("blender.exe")));
        out.push(
            base.join("Steam")
                .join("steamapps")
                .join("common")
                .join("Blender")
                .join("blender.exe"),
        );
    }
    out
}

/// Token de un fichero VDF de Valve: cadena entre comillas (sin desescapar)
/// o llave de apertura/cierre de bloque.
enum TokenVdf {
    Cadena(String),
    Llave,
}

/// Tokeniza un VDF. Dentro de una cadena, `\` escapa el carácter siguiente
/// (así `\"` no la cierra); el contenido se devuelve tal cual, sin desescapar.
fn tokens_vdf(contenido: &str) -> Vec<TokenVdf> {
    let mut tokens = Vec::new();
    let mut chars = contenido.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let mut cadena = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => {
                            cadena.push('\\');
                            if let Some(siguiente) = chars.next() {
                                cadena.push(siguiente);
                            }
                        }
                        _ => cadena.push(c),
                    }
                }
                tokens.push(TokenVdf::Cadena(cadena));
            }
            '{' | '}' => tokens.push(TokenVdf::Llave),
            '/' if chars.peek() == Some(&'/') => {
                // Comentario hasta el final de la línea.
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    tokens
}

/// Convierte `\\` en `\`; una barra solitaria se conserva.
fn desescapar_vdf(crudo: &str) -> String {
    let mut out = String::with_capacity(crudo.len());
    let mut chars = crudo.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'\\') {
            chars.next();
        }
        out.push(c);
    }
    out
}

/// Indica si el valor parece una ruta absoluta de Windows (unidad o UNC).
fn parece_ruta_windows(valor: &str) -> bool {
    valor.contains(":\\") || valor.contains(":/") || valor.starts_with("\\\\")
}

/// Añade la ruta al final de la lista si aún no está.
fn anadir_sin_duplicar(lista: &mut Vec<std::path::PathBuf>, ruta: std::path::PathBuf) {
    if !lista.contains(&ruta) {
        lista.push(ruta);
    }
}

/// Rutas de biblioteca declaradas en un `libraryfolders.vdf`, en orden de
/// aparición y sin duplicados. Admite el formato moderno (clave "path") y el
/// antiguo (clave numérica seguida directamente de la ruta).
fn bibliotecas_de_vdf(contenido: &str) -> Vec<std::path::PathBuf> {
    let tokens = tokens_vdf(contenido);
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let (TokenVdf::Cadena(clave), Some(TokenVdf::Cadena(valor))) =
            (&tokens[i], tokens.get(i + 1))
        else {
            // Llave o clave de bloque: no forma par clave/valor.
            i += 1;
            continue;
        };
        let ruta = desescapar_vdf(valor);
        let es_path = clave.eq_ignore_ascii_case("path");
        let es_antigua = !clave.is_empty()
            && clave.bytes().all(|b| b.is_ascii_digit())
            && parece_ruta_windows(&ruta);
        if (es_path || es_antigua) && !ruta.is_empty() {
            anadir_sin_duplicar(&mut out, std::path::PathBuf::from(ruta));
        }
        i += 2;
    }
    out
}

/// Candidatos `steamapps/common/Blender/blender.exe` de cada biblioteca.
fn candidatos_desde_bibliotecas(bibliotecas: &[std::path::PathBuf]) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for bib in bibliotecas {
        anadir_sin_duplicar(
            &mut out,
            bib.join("steamapps")
                .join("common")
                .join("Blender")
                .join("blender.exe"),
        );
    }
    out
}

/// Bibliotecas de Steam: cada raíz cuenta como biblioteca, seguida de las que
/// declare su `steamapps/libraryfolders.vdf` si existe. Orden estable.
fn bibliotecas_steam_en_raices(raices: &[std::path::PathBuf]) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for raiz in raices {
        anadir_sin_duplicar(&mut out, raiz.clone());
        let vdf = raiz.join("steamapps").join("libraryfolders.vdf");
        if let Ok(contenido) = std::fs::read_to_string(&vdf) {
            for bib in bibliotecas_de_vdf(&contenido) {
                anadir_sin_duplicar(&mut out, bib);
            }
        }
    }
    out
}

/// Raíz del cliente de Steam según el registro (`HKCU\Software\Valve\Steam`).
#[cfg(target_os = "windows")]
fn ruta_steam_desde_registro() -> Option<std::path::PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let clave = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Valve\Steam")
        .ok()?;
    let ruta = clave.get_value::<String, _>("SteamPath").ok()?;
    let ruta = ruta.replace('/', "\\");
    (!ruta.is_empty()).then(|| std::path::PathBuf::from(ruta))
}

/// Fuera de Windows no hay registro: no hay raíz de Steam que consultar.
#[cfg(not(target_os = "windows"))]
fn ruta_steam_desde_registro() -> Option<std::path::PathBuf> {
    None
}

/// Rutas de instalación habituales según la plataforma.
fn candidatos_instalacion() -> Vec<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    if cfg!(windows) {
        let bases: Vec<std::path::PathBuf> = [
            ("ProgramFiles", r"C:\Program Files"),
            ("ProgramFiles(x86)", r"C:\Program Files (x86)"),
        ]
        .iter()
        .map(|(var, respaldo)| {
            std::env::var_os(var)
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from(respaldo))
        })
        .collect();
        let mut lista = candidatos_windows(&bases);
        // Bibliotecas de Steam fuera de la ruta por defecto (otros
        // discos): el registro da la raíz del cliente aunque Steam no
        // viva en Program Files, y el libraryfolders.vdf de cada raíz
        // declara las bibliotecas adicionales.
        let mut raices: Vec<std::path::PathBuf> =
            bases.iter().map(|b| b.join("Steam")).collect();
        if let Some(raiz) = ruta_steam_desde_registro() {
            raices.push(raiz);
        }
        let bibliotecas = bibliotecas_steam_en_raices(&raices);
        for candidato in candidatos_desde_bibliotecas(&bibliotecas) {
            if !lista.contains(&candidato) {
                lista.push(candidato);
            }
        }
        lista
    } else if cfg!(target_os = "macos") {
        let mut v = vec![std::path::PathBuf::from(
            "/Applications/Blender.app/Contents/MacOS/Blender",
        )];
        if let Some(h) = home {
            v.push(h.join("Applications/Blender.app/Contents/MacOS/Blender"));
        }
        v
    } else {
        let mut v = vec![
            std::path::PathBuf::from("/usr/bin/blender"),
            std::path::PathBuf::from("/usr/local/bin/blender"),
            std::path::PathBuf::from("/snap/bin/blender"),
        ];
        if let Some(h) = home {
            v.push(h.join(".local/bin/blender"));
        }
        v.push(std::path::PathBuf::from("/opt/blender/blender"));
        v
    }
}

/// Ejecuta el bridge UNA vez con el Python del propio Blender
/// (`blender -b --python-exit-code 1 --python <bridge> -- <in> <out>`).
/// Err solo si Blender no se encuentra o no se puede lanzar; si el
/// proceso corrió, Ok con su salida y si salió bien.
pub fn ejecutar_bridge_con_blender(
    bridge_script: &Path,
    input_path: &Path,
    glb_path: &Path,
) -> Result<SalidaBridge, String> {
    // P4: a Blender/Python no puede llegar ninguna ruta verbatim
    // (`\\?\...`): su exportador glTF muere con WinError 123 al
    // crear el directorio de salida. Se normaliza AQUÍ, en el único
    // punto por el que pasa toda ejecución de Blender (vigía y tool
    // MCP `glass_asset_reload`, vía `convertir_fuente`); el resto
    // del motor sigue trabajando con las rutas tal cual las tenga.
    let bridge_script = sin_prefijo_verbatim(bridge_script);
    let input_path = sin_prefijo_verbatim(input_path);
    let glb_path = sin_prefijo_verbatim(glb_path);
    // Un solo proceso de Blender a la vez en TODO el motor: el
    // cerrojo vive aquí, en la única función que lanza Blender,
    // para que también cubra a la tool MCP `glass_asset_reload`
    // (que llama a `convertir_fuente` sin pasar por el vigía y
    // podía lanzar un segundo Blender sobre el mismo destino y
    // el mismo `.glass-tmp` que una exportación del vigía).
    let _turno = CERROJO_CONVERSION
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // La ruta del propio ejecutable también va normalizada: si el
    // resolver la trae canonizada (p. ej. la copia portable derivada
    // de un directorio verbatim), ni el proceso ni la variable
    // GLASS_BLENDER que lee el bridge deben llevar el prefijo.
    let blender = sin_prefijo_verbatim(
        &resolver_blender().ok_or_else(|| MENSAJE_SIN_BLENDER.to_string())?,
    );
    let mut comando = Command::new(&blender);
    // El bridge vuelve a resolver Blender desde Python (su lista no conoce
    // la copia portable del motor ni refleja la de Rust): se le pasa el ya
    // resuelto por GLASS_BLENDER, que es su primera fuente tras bpy, para
    // que ambas resoluciones coincidan siempre (defecto cazado en el test
    // físico del bloque Blender portable: el .blend fallaba dentro del
    // bridge aunque Rust sí había encontrado el portable).
    comando.env("GLASS_BLENDER", &blender);
    comando
        .args(["-b", "--python-exit-code", "1", "--python"])
        .arg(bridge_script)
        .arg("--")
        .arg(input_path)
        .arg(glb_path);
    glass_core::proceso::sin_consola(&mut comando);
    let res = comando
        .output()
        .map_err(|e| format!("No se pudo lanzar Blender ({}): {}", blender.display(), e))?;
    Ok(SalidaBridge {
        exito: res.status.success(),
        stdout: String::from_utf8_lossy(&res.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&res.stderr).into_owned(),
    })
}

/// Vía por la que se obtuvo el .glb. Se expone para que el vigía (y la UI)
/// puedan distinguir una conversión nativa de una que tiró de Blender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViaConversion {
    Nativa,
    Blender,
}

/// Resultado común a ambas vías: así el llamante no necesita saber qué
/// convertidor actuó para registrar, notificar o mostrar avisos.
#[derive(Debug, Clone)]
pub struct InformeFuente {
    pub conteos: glass_fbx::Conteos,
    pub avisos: Vec<String>,
    pub via: ViaConversion,
}

/// Convierte una fuente .blend/.fbx a .glb eligiendo la vía adecuada.
///
/// Un .fbx va primero por el convertidor nativo, que no depende de Blender.
/// Blender solo se usa como respaldo si está instalado. Un .blend solo puede
/// abrirlo Blender, así que va directo al bridge.
pub fn convertir_fuente(fuente: &Path, destino_glb: &Path) -> Result<InformeFuente, String> {
    let extension = fuente
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    match extension.as_deref() {
        Some("fbx") => match glass_fbx::convertir_fbx(fuente, destino_glb) {
            Ok(informe) => Ok(InformeFuente {
                conteos: informe.conteos,
                avisos: informe.avisos,
                via: ViaConversion::Nativa,
            }),
            Err(e) => {
                // Sin Blender no hay respaldo posible. Se devuelve el error nativo
                // a secas: para un .fbx Blender no es requisito, y mencionarlo
                // haría creer al usuario que debe instalarlo.
                if resolver_blender().is_none() {
                    return Err(e.to_string());
                }
                match convertir_con_bridge(fuente, destino_glb) {
                    Ok(mut informe) => {
                        // Se deja constancia del fallo nativo: el .glb existe,
                        // pero el convertidor nativo tiene un caso pendiente.
                        informe.avisos.push(format!(
                            "el convertidor nativo falló ({e}); se usó Blender como respaldo"
                        ));
                        Ok(informe)
                    }
                    Err(m) => Err(format!(
                        "No se pudo convertir «{}»: el convertidor nativo falló ({e}) y el respaldo con Blender también ({m})",
                        fuente.display()
                    )),
                }
            }
        },
        // Los errores de esta vía ya mencionan Blender cuando corresponde.
        Some("blend") => convertir_con_bridge(fuente, destino_glb),
        _ => {
            let nombre = fuente
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_else(|| fuente.to_string_lossy());
            Err(format!(
                "Solo se convierten fuentes .blend/.fbx (recibido «{nombre}»)"
            ))
        }
    }
}

/// Vía Blender: lanza el bridge headless y traduce su salida a InformeFuente.
fn convertir_con_bridge(fuente: &Path, destino_glb: &Path) -> Result<InformeFuente, String> {
    // Se comprueba aquí y no en el vigía: la vía nativa no necesita el script.
    let bridge = bridge_path();
    if !bridge.exists() {
        return Err(format!(
            "Bridge de Blender no encontrado en '{}': no se exporta {:?}.",
            bridge.display(),
            fuente
        ));
    }

    // El Err (sin Blender o fallo al lanzar) ya trae un mensaje adecuado.
    let salida = ejecutar_bridge_con_blender(&bridge, fuente, destino_glb)?;

    if !salida.exito {
        let resumen = resumen_bridge(&salida.stdout);
        let stderr = salida.stderr.trim();
        return Err(if stderr.is_empty() {
            resumen
        } else {
            format!("{resumen} | stderr: {stderr}")
        });
    }

    // Si el bridge dice acertar pero no informa de conteos, no se inventan:
    // un informe con ceros parecería una exportación vacía válida.
    let conteos = conteos_desde_resultado_bridge(&salida.stdout)
        .ok_or_else(|| "El bridge no devolvió resultado".to_string())?;

    Ok(InformeFuente {
        conteos,
        avisos: Vec::new(),
        via: ViaConversion::Blender,
    })
}

/// Extrae los conteos de la línea `GLASS_BRIDGE_RESULT {json}` del bridge.
///
/// Se toma la última línea con el prefijo porque Blender y sus addons pueden
/// imprimir ruido antes, y la línea definitiva del bridge va al final.
pub fn conteos_desde_resultado_bridge(stdout: &str) -> Option<glass_fbx::Conteos> {
    const PREFIJO: &str = "GLASS_BRIDGE_RESULT ";

    let carga = stdout.lines().rev().find_map(|l| l.strip_prefix(PREFIJO))?;
    let valor: serde_json::Value = serde_json::from_str(carga.trim()).ok()?;

    if valor.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return None;
    }

    let counts = valor.get("counts")?.as_object()?;

    // Campo ausente = 0 (versiones antiguas del bridge no informan de todos).
    // Un campo presente pero no entero es un resultado corrupto, así que da None.
    let campo = |nombre: &str| -> Option<usize> {
        match counts.get(nombre) {
            None => Some(0),
            Some(v) => usize::try_from(v.as_u64()?).ok(),
        }
    };

    Some(glass_fbx::Conteos {
        meshes: campo("meshes")?,
        nodes: campo("nodes")?,
        materials: campo("materials")?,
        animations: campo("animations")?,
        skins: campo("skins")?,
        bytes: campo("bytes")?,
    })
}

#[cfg(test)]
mod tests_resolver_blender {
    use super::*;
    use std::path::{Path, PathBuf};

    fn dir_temporal(nombre: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "glass_blender_{}_{}",
            nombre,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn tocar(p: &Path) {
        if let Some(padre) = p.parent() {
            std::fs::create_dir_all(padre).unwrap();
        }
        std::fs::write(p, b"").unwrap();
    }

    #[test]
    fn prefiere_glass_blender_y_cae_al_path_si_no_existe() {
        let d = dir_temporal("env");
        let env_bin = d.join("mi_blender");
        tocar(&env_bin);
        let dir_path = d.join("path");
        let en_path = dir_path.join("blender");
        tocar(&en_path);
        let dirs = [dir_path.clone()];

        assert_eq!(
            resolver_blender_desde(Some(env_bin.to_str().unwrap()), &dirs, &[]),
            Some(env_bin.clone())
        );
        let inexistente = d.join("no_existe");
        assert_eq!(
            resolver_blender_desde(Some(inexistente.to_str().unwrap()), &dirs, &[]),
            Some(en_path.clone())
        );

        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn encuentra_blender_en_el_path() {
        let d = dir_temporal("path");
        let vacio = d.join("vacio");
        std::fs::create_dir_all(&vacio).unwrap();
        let con_blender = d.join("bin");
        let bin = con_blender.join("blender");
        tocar(&bin);

        assert_eq!(
            resolver_blender_desde(None, &[vacio, con_blender], &[]),
            Some(bin)
        );

        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sin_env_ni_path_usa_el_primer_candidato_existente() {
        let d = dir_temporal("candidatos");
        let falta = d.join("falta").join("blender");
        let primero = d.join("uno").join("blender");
        let segundo = d.join("dos").join("blender");
        tocar(&primero);
        tocar(&segundo);

        assert_eq!(
            resolver_blender_desde(None, &[], &[falta, primero.clone(), segundo]),
            Some(primero)
        );

        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn candidatos_windows_ordena_por_version_e_incluye_steam() {
        let base = dir_temporal("windows");
        let fundacion = base.join("Blender Foundation");
        let v42 = fundacion.join("Blender 4.2").join("blender.exe");
        let v36 = fundacion.join("Blender 3.6").join("blender.exe");
        tocar(&v42);
        tocar(&v36);
        std::fs::create_dir_all(fundacion.join("Otros")).unwrap();
        let steam = base
            .join("Steam")
            .join("steamapps")
            .join("common")
            .join("Blender")
            .join("blender.exe");

        let lista = candidatos_windows(std::slice::from_ref(&base));

        assert_eq!(lista.first(), Some(&v42));
        assert!(lista.contains(&v36));
        assert!(lista.contains(&steam));
        assert!(!lista.iter().any(|p| p.starts_with(fundacion.join("Otros"))));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn vdf_moderno_extrae_bibliotecas_en_orden_y_desescapa() {
        let vdf = r#""libraryfolders"
{
    "0"
    {
        "path"        "C:\\Program Files (x86)\\Steam"
        "label"        ""
        "contentid"        "1234567890"
        "totalsize"        "0"
        "apps"
        {
            "228980"        "123456"
        }
    }
    "1"
    {
        "path"        "D:\\SteamLibrary"
        "label"        ""
        "apps"
        {
            "365670"        "987654"
        }
    }
}
"#;
        assert_eq!(
            bibliotecas_de_vdf(vdf),
            vec![
                std::path::PathBuf::from(r"C:\Program Files (x86)\Steam"),
                std::path::PathBuf::from(r"D:\SteamLibrary"),
            ]
        );
    }

    #[test]
    fn vdf_antiguo_extrae_bibliotecas() {
        let vdf = r#""LibraryFolders"
{
    "TimeNextStatsReport"        "1600000000"
    "ContentStatsID"        "-123456789"
    "1"        "D:\\SteamLibrary"
    "2"        "E:\\Juegos\\Steam"
}
"#;
        assert_eq!(
            bibliotecas_de_vdf(vdf),
            vec![
                std::path::PathBuf::from(r"D:\SteamLibrary"),
                std::path::PathBuf::from(r"E:\Juegos\Steam"),
            ]
        );
    }

    #[test]
    fn vdf_ignora_valores_que_no_son_rutas() {
        let vdf = r#""LibraryFolders"
{
    "1"        "12345"
    "2"        "F:\\Steam"
}
"#;
        assert_eq!(
            bibliotecas_de_vdf(vdf),
            vec![std::path::PathBuf::from(r"F:\Steam")]
        );
    }

    #[test]
    fn bibliotecas_en_raices_resuelve_el_caso_del_probador() {
        let base = dir_temporal("steam_bibliotecas_probador");
        let raiz = base.join("cliente_steam");
        let biblioteca = base.join("otro_disco").join("SteamLibrary");
        let escapar = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
        let vdf = format!(
            "\"libraryfolders\"\n{{\n    \"0\"\n    {{\n        \"path\"        \"{}\"\n    }}\n    \"1\"\n    {{\n        \"path\"        \"{}\"\n    }}\n}}\n",
            escapar(&raiz),
            escapar(&biblioteca)
        );
        std::fs::create_dir_all(raiz.join("steamapps")).unwrap();
        std::fs::write(raiz.join("steamapps").join("libraryfolders.vdf"), vdf).unwrap();
        let exe = biblioteca
            .join("steamapps")
            .join("common")
            .join("Blender")
            .join("blender.exe");
        tocar(&exe);

        let candidatos =
            candidatos_desde_bibliotecas(&bibliotecas_steam_en_raices(std::slice::from_ref(&raiz)));
        assert!(candidatos.contains(&exe));
        assert_eq!(resolver_blender_desde(None, &[], &candidatos), Some(exe));

        let _ = std::fs::remove_dir_all(&base);
    }
}

/// Lanza la conversión .blend/.fbx -> .glb hermano en un hilo propio y
/// emite `MeshReloaded` al terminar cada ronda con éxito.
/// Cerrojo global de conversión: como mucho UN proceso de Blender a la
/// vez en todo el motor. Sin él, una ráfaga de peticiones (muchos
/// archivos guardados a la vez, varias solicitudes de la escena) lanzaba
/// un Blender por archivo y agotaba la memoria del proceso. Se toma
/// dentro de `ejecutar_bridge_con_blender` — la única función que
/// lanza Blender — para cubrir también a la tool MCP.
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
        // El turno de Blender (un solo proceso a la vez) se espera
        // dentro de `ejecutar_bridge_con_blender`, no aquí: así el
        // cerrojo cubre a todo el que lance Blender, MCP incluido.
        let mut hash_previo = hash_inicial;
        for ronda in 1..=MAX_RONDAS_EXPORTACION {
            // F2: convertir_fuente elige la vía (nativa para .fbx con
            // respaldo en Blender; bridge para .blend).
            let informe = match convertir_fuente(&input_path, &glb_path) {
                Ok(informe) => informe,
                Err(e) => {
                    // Sin registro: el próximo guardado reintenta.
                    log::error!("Fallo de conversión ({:?}): {}", input_path, e);
                    break;
                }
            };
            match informe.via {
                ViaConversion::Nativa => log::info!(
                    "Conversión nativa exitosa: {:?} -> {:?}",
                    input_path,
                    glb_path
                ),
                ViaConversion::Blender => log::info!(
                    "Conversión headless exitosa: {:?} -> {:?}",
                    input_path,
                    glb_path
                ),
            }
            for aviso in &informe.avisos {
                log::warn!("Aviso al convertir {:?}: {}", input_path, aviso);
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
    #[test]
    fn ruta_blender_en_raiz_compone_ruta_por_plataforma() {
        let raiz = std::path::Path::new("motor");
        let esperado = if cfg!(windows) {
            raiz.join("tools").join("blender").join("blender.exe")
        } else {
            raiz.join("tools").join("blender").join("blender")
        };
        assert_eq!(super::ruta_blender_en_raiz(raiz), esperado);
    }

    #[test]
    fn local_es_ultimo_recurso_frente_al_instalado() {
        let base = std::env::temp_dir().join(format!("glass_watcher_ultimo_{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let instalado = base.join("instalado_blender");
        let local = base.join("local_blender");
        std::fs::write(&instalado, b"").unwrap();
        std::fs::write(&local, b"").unwrap();

        let r = super::resolver_blender_desde(None, &[], &[instalado.clone(), local]);
        assert_eq!(r, Some(instalado));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn local_se_encuentra_si_es_el_unico_candidato() {
        let base = std::env::temp_dir().join(format!("glass_watcher_unico_{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let local = base.join("local_blender");
        std::fs::write(&local, b"").unwrap();

        let r = super::resolver_blender_desde(None, &[], std::slice::from_ref(&local));
        assert_eq!(r, Some(local));

        let _ = std::fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod tests_prefijo_verbatim {
    use super::*;

    #[test]
    fn quita_el_prefijo_de_disco() {
        let entrada = PathBuf::from(r"\\?\C:\Users\vic\Assets\a.blend");
        assert_eq!(
            sin_prefijo_verbatim(&entrada),
            PathBuf::from(r"C:\Users\vic\Assets\a.blend")
        );
    }

    #[test]
    fn convierte_unc_verbatim_a_unc_plana() {
        let entrada = PathBuf::from(r"\\?\UNC\srv\share\a.blend");
        assert_eq!(
            sin_prefijo_verbatim(&entrada),
            PathBuf::from(r"\\srv\share\a.blend")
        );
    }

    #[test]
    fn deja_intactas_las_rutas_sin_prefijo() {
        for ruta in [
            r"C:\Users\vic\Assets\a.blend",
            "/home/u/Assets/a.blend",
            "assets/a.blend",
            r"\\srv\share\a.blend",
        ] {
            let p = PathBuf::from(ruta);
            assert_eq!(sin_prefijo_verbatim(&p), p, "ruta: {ruta}");
        }
    }

    #[test]
    fn no_quita_un_prefijo_sin_unidad_detras() {
        // `\\?\` seguido de algo que no es una unidad (`C:`) no es
        // la forma verbatim de disco que produce `canonicalize`:
        // no se toca.
        let p = PathBuf::from(r"\\?\relativa\a.blend");
        assert_eq!(sin_prefijo_verbatim(&p), p);
    }

    #[test]
    fn no_quita_el_prefijo_si_la_forma_plana_apuntaria_a_otro_archivo() {
        // En verbatim estos componentes son literales; en plano,
        // Win32 los reinterpretaría. Se deja la ruta como está.
        for ruta in [
            r"\\?\C:\a/b.blend",       // '/' dentro de la ruta
            r"\\?\C:\carpeta.\a.blend", // componente con punto final
            r"\\?\C:\carpeta \a.blend", // componente con espacio final
            r"\\?\C:\a\..\a.blend",     // componente '..'
        ] {
            let p = PathBuf::from(ruta);
            assert_eq!(sin_prefijo_verbatim(&p), p, "ruta: {ruta}");
        }
    }

    #[test]
    fn en_vuelo_dedupe_entre_formas() {
        let verbatim = PathBuf::from(r"\\?\C:\proj\Assets\dedup_p4.blend");
        let plana = PathBuf::from(r"C:\proj\Assets\dedup_p4.blend");
        let guarda = EnVuelo::intentar(&verbatim).expect("la primera inserción entra");
        assert!(
            EnVuelo::intentar(&plana).is_none(),
            "la forma plana del mismo archivo debe dedupir contra la verbatim"
        );
        assert!(fuente_en_vuelo(&plana.with_extension("glb")));
        drop(guarda);
        let guarda2 = EnVuelo::intentar(&plana);
        assert!(guarda2.is_some(), "al soltar la guarda se puede volver a entrar");
        drop(guarda2);
    }

    #[test]
    fn hash_registrado_se_consulta_sin_cambio_y_detecta_cambio() {
        let dir = std::env::temp_dir().join(format!("glass_p4_hash_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("fuente_hash_p4.blend");
        std::fs::write(&real, b"contenido de prueba p4").unwrap();
        let hash = sha256_de_archivo(&real).expect("hash del temporal");
        registrar_exportado(&real, hash);
        assert!(
            contenido_cambio(&real).is_none(),
            "mismo contenido: no hay que re-exportar"
        );
        std::fs::write(&real, b"contenido DISTINTO p4").unwrap();
        assert!(
            contenido_cambio(&real).is_some_and(|c| c.is_some()),
            "contenido distinto: hay que re-exportar con el hash nuevo"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
