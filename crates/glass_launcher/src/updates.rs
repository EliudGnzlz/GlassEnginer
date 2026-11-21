//! Actualizaciones del Launcher desde GitHub Releases.
//!
//! El Launcher consulta las releases públicas de GitHub, compara versiones
//! semánticas, selecciona el activo correspondiente a la plataforma actual,
//! descarga el paquete y verifica obligatoriamente su SHA-256 antes de
//! instalarlo en la carpeta de versiones y registrarlo en el store local.
//!
//! No se usan tokens: solo se soportan repositorios públicos. Un error 404
//! debe interpretarse como repositorio privado o inexistente.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::Digest;

pub const ENGINE_VERSION: &str = "0.10.20.9-beta";
/// Identidad propia del Hub (la cáscara fija). Solo cambia cuando cambia
/// el Hub; hoy coincide con la del motor.
pub const HUB_VERSION: &str = "0.2.3";
pub const DEFAULT_REPO: &str = "EliudGnzlz/GlassEnginer";
pub const DEFAULT_API: &str = "https://api.github.com";

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AssetInfo {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    pub browser_download_url: String,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<AssetInfo>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Installation {
    pub version: String,
    pub path: String,
    pub installed_unix: u64,
}

/// Proyecto conocido por el Launcher (creado o abierto desde el Hub),
/// viva donde viva en disco.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectEntry {
    pub name: String,
    pub path: String,
    pub last_opened_unix: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LauncherStore {
    #[serde(default = "default_repo")]
    pub repo: String,
    #[serde(default)]
    pub installations: Vec<Installation>,
    /// Registro persistente de proyectos. `serde(default)` para que los
    /// launcher.json anteriores (sin este campo) sigan cargando.
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
    /// Rutas canonicalizadas de proyectos quitados de la lista a mano:
    /// el descubrimiento del Hub las filtra en las tres fuentes
    /// (registro, escaneo y directorio de trabajo).
    #[serde(default)]
    pub ocultos: Vec<String>,
    /// Intervalo de autoguardado del editor en minutos (0 = desactivado).
    #[serde(default = "default_autosave_minutes")]
    pub autosave_minutes: u32,
}

fn default_autosave_minutes() -> u32 {
    5
}

fn default_repo() -> String {
    DEFAULT_REPO.to_string()
}

impl Default for LauncherStore {
    fn default() -> Self {
        Self {
            repo: default_repo(),
            installations: Vec::new(),
            projects: Vec::new(),
            ocultos: Vec::new(),
            autosave_minutes: default_autosave_minutes(),
        }
    }
}

pub fn api_base() -> String {
    let base = std::env::var("GLASS_UPDATE_API")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_API.to_string());
    base.trim_end_matches('/').to_string()
}

pub fn data_root() -> PathBuf {
    if let Ok(home) = std::env::var("GLASS_LAUNCHER_HOME") {
        if !home.is_empty() {
            return PathBuf::from(home);
        }
    }

    let base = std::env::var("HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok().filter(|v| !v.is_empty()))
        .unwrap_or_else(|| ".".to_string());

    PathBuf::from(base).join("GlassEnginer")
}

/// ¿Es `p` un ejecutable regular? En Unix exige el bit de
/// ejecución: un archivo que solo supera `is_file()` (una copia a
/// medias, un resto de descarga) no debe pasar por Hub.
fn es_ejecutable(p: &std::path::Path) -> bool {
    if !p.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(p) {
            Ok(m) => m.permissions().mode() & 0o111 != 0,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Ruta al ejecutable del Hub separado (separación verdadera V3).
///
/// Orden de búsqueda, del ancla más fuerte a la más débil:
/// 1. La raíz de ESTA instalación derivada del propio ejecutable:
///    el motor instalado vive en `<raíz>/engines/<versión>/`, y el
///    Hub en `<raíz>/`. Es la única que no puede cruzar a otra
///    instalación que conviva en el mismo equipo.
/// 2. La raíz del lanzador (`data_root()`: GLASS_LAUNCHER_HOME o
///    ~/GlassEnginer), que es la que el Hub en curso fijó al
///    lanzar este motor.
/// 3. Junto al ejecutable en curso: las builds de desarrollo,
///    donde glass_hub y glass_app comparten carpeta.
pub fn ruta_hub() -> Option<PathBuf> {
    let candidatos: &[&str] = if cfg!(windows) {
        &["GlassHub.exe", "glass_hub.exe"]
    } else {
        &["glass_hub", "GlassHub"]
    };
    let buscar_en = |dir: &std::path::Path| -> Option<PathBuf> {
        candidatos
            .iter()
            .map(|n| dir.join(n))
            .find(|p| es_ejecutable(p))
    };

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir_version) = exe.parent() {
            if let Some(dir_engines) = dir_version.parent() {
                if dir_engines.file_name().is_some_and(|n| n == "engines") {
                    if let Some(raiz) = dir_engines.parent() {
                        if let Some(hub) = buscar_en(raiz) {
                            return Some(hub);
                        }
                    }
                }
            }
            if let Some(hub) = buscar_en(&data_root()) {
                return Some(hub);
            }
            return buscar_en(dir_version);
        }
    }
    buscar_en(&data_root())
}

pub fn store_path() -> PathBuf {
    data_root().join("launcher.json")
}

pub fn versions_dir() -> PathBuf {
    data_root().join("versions")
}

/// Carpeta de los motores gestionados por el Hub: data_root()/engines.
pub fn engines_dir() -> PathBuf {
    data_root().join("engines")
}

pub fn engines_dir_in(root: &std::path::Path) -> PathBuf {
    root.join("engines")
}

/// Ejecutable del motor instalado: <root>/engines/<version>/glass_app[.exe].
pub fn motor_exe_in(root: &std::path::Path, version: &str) -> PathBuf {
    let exe = if cfg!(windows) { "glass_app.exe" } else { "glass_app" };
    engines_dir_in(root).join(version).join(exe)
}

impl LauncherStore {
    pub fn load() -> LauncherStore {
        std::fs::read_to_string(store_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let root = data_root();
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let ruta = store_path();
        let tmp = ruta.with_extension("tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &ruta).map_err(|e| e.to_string())
    }

    /// Registra (o refresca) un proyecto. Las rutas se comparan
    /// canonicalizadas, con fallback a la ruta tal cual si no existe.
    pub fn register_project(&mut self, name: &str, path: &std::path::Path) {
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let now = now_unix();
        if let Some(entry) = self.projects.iter_mut().find(|e| {
            let p = std::path::Path::new(&e.path);
            std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()) == canon
        }) {
            entry.name = name.to_string();
            entry.last_opened_unix = now;
        } else {
            self.projects.push(ProjectEntry {
                name: name.to_string(),
                path: canon.to_string_lossy().into_owned(),
                last_opened_unix: now,
            });
        }
    }

    /// Ruta canonicalizada como texto, con fallback a la ruta tal cual.
    fn canon_texto(path: &std::path::Path) -> String {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned()
    }

    /// ¿Está esta ruta en la lista de ocultos? (comparación canonicalizada)
    pub fn esta_oculto(&self, path: &std::path::Path) -> bool {
        let canon = Self::canon_texto(path);
        self.ocultos.contains(&canon)
    }

    /// Añade la ruta a la lista de ocultos (sin duplicados).
    pub fn ocultar_proyecto(&mut self, path: &std::path::Path) {
        let canon = Self::canon_texto(path);
        if !self.ocultos.contains(&canon) {
            self.ocultos.push(canon);
        }
    }

    /// Quita la ruta del registro de proyectos (sin tocar los ocultos).
    pub fn desregistrar_proyecto(&mut self, path: &std::path::Path) {
        let canon = Self::canon_texto(path);
        self.projects.retain(|e| Self::canon_texto(std::path::Path::new(&e.path)) != canon);
    }

    /// «Quitar de la lista»: desregistra y oculta, para que el proyecto
    /// no reaparezca ni por el escaneo ni como proyecto del directorio
    /// de trabajo.
    pub fn quitar_de_la_lista(&mut self, path: &std::path::Path) {
        self.desregistrar_proyecto(path);
        self.ocultar_proyecto(path);
    }
}

/// Registro del motor gestionado (engines.json). Solo lo escribe el Hub.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct EngineStore {
    #[serde(default)]
    pub en_uso: Option<String>,
    #[serde(default)]
    pub conservada: Option<String>,
    #[serde(default)]
    pub pendientes_borrar: Vec<String>,
}

pub fn engines_store_path() -> PathBuf {
    engines_store_path_in(&data_root())
}

pub fn engines_store_path_in(root: &std::path::Path) -> PathBuf {
    root.join("engines.json")
}

impl EngineStore {
    pub fn load() -> EngineStore {
        Self::load_in(&data_root())
    }

    /// Archivo ausente o ilegible -> registro por defecto.
    pub fn load_in(root: &std::path::Path) -> EngineStore {
        std::fs::read_to_string(engines_store_path_in(root))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_in(&data_root())
    }

    /// Escritura atómica: temporal hermano (.tmp) y rename al destino.
    pub fn save_in(&self, root: &std::path::Path) -> Result<(), String> {
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let dest = engines_store_path_in(root);
        let tmp = dest.with_extension("tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &dest).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }
}

pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Clave de versión del proyecto: 3 o 4 componentes numéricos más el
/// prerelease opcional. El cuarto componente (revisión) participa en el
/// orden: 0.10.14.1 > 0.10.14 > 0.10.0. El semver estricto no admite el
/// cuarto componente, por eso el parser es propio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionKey {
    pub core: [u64; 4],
    pub pre: String, // "" = sin prerelease (vale MÁS que cualquier prerelease)
}

impl PartialOrd for VersionKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for VersionKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let core_order = self.core.cmp(&other.core);
        if core_order != std::cmp::Ordering::Equal {
            return core_order;
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => cmp_prerelease(&self.pre, &other.pre),
        }
    }
}

/// Compara dos cadenas de prerelease identificador por identificador,
/// siguiendo la regla de semver: los identificadores numéricos comparan
/// como números y valen menos que los alfanuméricos; si un prerelease es
/// prefijo del otro, el más corto vale menos.
fn cmp_prerelease(a: &str, b: &str) -> std::cmp::Ordering {
    let mut ia = a.split('.');
    let mut ib = b.split('.');
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(xa), Some(xb)) => {
                let na = xa.parse::<u64>().ok();
                let nb = xb.parse::<u64>().ok();
                let order = match (na, nb) {
                    (Some(va), Some(vb)) => va.cmp(&vb),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => xa.cmp(xb),
                };
                if order != std::cmp::Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// La version del sello "engine" de un project.glass: None si el campo
/// no esta o no parsea como version (los sellos viejos tipo
/// "GlassEngine 0.1.0" cuentan como sin sello).
pub fn version_de_sello(meta: &serde_json::Value) -> Option<VersionKey> {
    meta.get("engine")
        .and_then(|v| v.as_str())
        .and_then(|s| tag_version(s).ok())
}

/// Sella un proyecto con la version del motor indicada: reescribe el
/// campo "engine" de su project.glass SOLO si el sello nuevo es MAYOR
/// que el actual (o no hay sello legible); un sello nunca rebaja.
/// Devuelve true si escribio. Preserva el resto de campos del JSON.
pub fn sellar_proyecto(dir: &std::path::Path, version: &str) -> Result<bool, String> {
    let ruta = dir.join("project.glass");
    let texto = std::fs::read_to_string(&ruta)
        .map_err(|e| format!("No se pudo leer {}: {}", ruta.display(), e))?;
    let mut meta: serde_json::Value = serde_json::from_str(&texto)
        .map_err(|e| format!("project.glass ilegible en {}: {}", ruta.display(), e))?;
    let nueva = tag_version(version)
        .map_err(|e| format!("Version de sello invalida '{}': {}", version, e))?;
    if let Some(actual) = version_de_sello(&meta) {
        if actual >= nueva {
            return Ok(false);
        }
    }
    let obj = meta
        .as_object_mut()
        .ok_or_else(|| "project.glass no es un objeto JSON".to_string())?;
    obj.insert("engine".to_string(), serde_json::Value::String(version.to_string()));
    let salida = serde_json::to_string_pretty(&meta)
        .map_err(|e| format!("No se pudo serializar project.glass: {}", e))?;
    // Escritura atomica (temporal + renombrado): un corte a mitad de la
    // escritura no debe dejar el project.glass truncado.
    let tmp = dir.join("project.glass.tmp");
    std::fs::write(&tmp, salida)
        .map_err(|e| format!("No se pudo escribir {}: {}", tmp.display(), e))?;
    std::fs::rename(&tmp, &ruta)
        .map_err(|e| format!("No se pudo renombrar {}: {}", tmp.display(), e))?;
    Ok(true)
}

pub fn tag_version(tag: &str) -> Result<VersionKey, String> {
    let raw = tag.strip_prefix('v').unwrap_or(tag);
    let (core_str, pre) = match raw.split_once('-') {
        Some((c, p)) => (c, p.to_string()),
        None => (raw, String::new()),
    };
    let parts: Vec<&str> = core_str.split('.').collect();
    if parts.len() != 3 && parts.len() != 4 {
        return Err(format!(
            "versión inválida '{}': se esperaban 3 o 4 componentes numéricos",
            tag
        ));
    }
    let mut core = [0u64; 4];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            return Err(format!("versión inválida '{}': componente vacío", tag));
        }
        core[i] = part
            .parse::<u64>()
            .map_err(|_| format!("versión inválida '{}': componente no numérico '{}'", tag, part))?;
    }
    Ok(VersionKey { core, pre })
}

pub fn is_newer(tag: &str, current: &str) -> bool {
    let release = match tag_version(tag) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let current_version = match tag_version(current) {
        Ok(v) => v,
        Err(_) => return false,
    };
    release > current_version
}

/// Cambio pesado = difiere major o minor. Si alguna versión no parsea,
/// se trata como pesado (ante la duda se conserva, no se borra).
pub fn es_cambio_pesado(a: &str, b: &str) -> bool {
    match (tag_version(a), tag_version(b)) {
        (Ok(va), Ok(vb)) => va.core[0] != vb.core[0] || va.core[1] != vb.core[1],
        _ => true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRetirada {
    pub borrar: Vec<String>,
    pub conservada: Option<String>,
}

/// Decide qué versiones del motor retirar al instalar `nueva`.
/// Nunca propone borrar `nueva` ni la versión que queda conservada.
pub fn plan_retirada(en_uso: Option<&str>, conservada: Option<&str>, nueva: &str) -> PlanRetirada {
    let sin_cambios = PlanRetirada {
        borrar: Vec::new(),
        conservada: conservada.map(str::to_string),
    };
    let actual = match en_uso {
        Some(v) => v,
        None => return sin_cambios,
    };
    if actual == nueva {
        return sin_cambios;
    }
    if es_cambio_pesado(actual, nueva) {
        let borrar = conservada
            .filter(|c| *c != actual && *c != nueva)
            .map(str::to_string)
            .into_iter()
            .collect();
        PlanRetirada {
            borrar,
            conservada: Some(actual.to_string()),
        }
    } else {
        let borrar = if conservada == Some(actual) {
            Vec::new()
        } else {
            vec![actual.to_string()]
        };
        PlanRetirada {
            borrar,
            conservada: conservada.map(str::to_string),
        }
    }
}

pub fn platform_slug() -> &'static str {
    use std::env::consts::{ARCH, OS};

    match (OS, ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("linux", "x86_64") => "linux-x64",
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x64",
        _ => "unknown",
    }
}

pub fn pick_asset(release: &ReleaseInfo) -> Option<&AssetInfo> {
    let candidates: Vec<&AssetInfo> = release
        .assets
        .iter()
        .filter(|a| a.name.ends_with(".zip"))
        .collect();

    let slug = platform_slug();
    if let Some(asset) = candidates.iter().find(|a| a.name.contains(slug)) {
        return Some(*asset);
    }

    None
}

/// Selecciona el paquete del motor gestionado para esta plataforma.
/// El contrato de nombre es `GlassEnginer-Engine-<version>-<slug>.zip`
/// (lo produce release.yml). Seleccion ESTRICTA: un .zip cuyo nombre en
/// minusculas contenga "-engine-" y el slug de la plataforma. Sin
/// fallback de ningun tipo.
pub fn pick_engine_asset(release: &ReleaseInfo) -> Option<&AssetInfo> {
    let slug = platform_slug().to_lowercase();
    release.assets.iter().find(|a| {
        let name = a.name.to_lowercase();
        name.ends_with(".zip") && name.contains("-engine-") && name.contains(slug.as_str())
    })
}

pub fn digest_sha256(asset: &AssetInfo) -> Option<String> {
    let digest = asset.digest.as_ref()?;
    let normalized = digest.to_ascii_lowercase();
    let hex = normalized.strip_prefix("sha256:").unwrap_or(&normalized);

    if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(hex.to_string())
    } else {
        None
    }
}

pub fn parse_checksums(text: &str, file_name: &str) -> Option<String> {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let hex = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');

        if name == file_name {
            return Some(hex.to_ascii_lowercase());
        }
    }

    None
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Tope de lectura para las respuestas que se cargan en memoria (el
/// JSON de releases, los checksums, los manifiestos de complementos).
///
/// ureq capa `read_to_vec`/`read_to_string` a 10 MiB por defecto: el
/// tope se hace explícito y amplio para no depender de ese defecto.
/// Los PAQUETES no pasan por aquí: se descargan en streaming a disco
/// con `get_to_file` (defecto 8c — con el tope por defecto de ureq,
/// toda descarga de actualización moría con «the response body is
/// larger than request limit: 10485760», porque el paquete del motor
/// pesa ~26 MB y hasta el binario solo ocupa 13,2 MB comprimido).
const LIMITE_LECTURA_HTTP: u64 = 512 * 1024 * 1024;

fn peticion_get(url: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(300)))
        .build()
        .into();
    match agent
        .get(url)
        .header("User-Agent", "GlassEnginer-Launcher")
        .call()
    {
        Ok(r) => Ok(r),
        Err(ureq::Error::StatusCode(404)) => {
            Err("HTTP 404: el recurso no existe o no es público.".to_string())
        }
        Err(ureq::Error::StatusCode(n)) => Err(format!("HTTP {n}")),
        Err(e) => Err(e.to_string()),
    }
}

pub(crate) fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let mut resp = peticion_get(url)?;
    resp.body_mut()
        .with_config()
        .limit(LIMITE_LECTURA_HTTP)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

/// Descarga `url` a `dest` en streaming: el cuerpo se copia por
/// trozos a disco, sin cargarlo entero en memoria y sin ningún tope
/// de tamaño (`as_reader()` no impone el límite de 10 MiB de
/// `read_to_vec`). Devuelve los bytes escritos. Es la vía de los
/// paquetes de actualización (defecto 8c).
pub(crate) fn get_to_file(url: &str, dest: &std::path::Path) -> Result<u64, String> {
    let mut resp = peticion_get(url)?;
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    std::io::copy(&mut resp.body_mut().as_reader(), &mut file).map_err(|e| e.to_string())
}

/// SHA-256 de un archivo, leído por trozos (para verificar un paquete
/// descargado en streaming sin cargarlo en memoria).
pub fn sha256_hex_file(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Devuelve la release del motor más reciente publicada en `repo`.
///
/// No usa `GET /repos/{repo}/releases/latest`: GitHub reserva ese
/// endpoint a la última release NO-prerelease, y las del motor se
/// publican como prerelease. Además, en el repo conviven releases de
/// complementos (p. ej. `herramienta-3d-v1.0.1`), que «latest» podría
/// devolver. Por eso pide la lista (`/releases?per_page=100`) y elige
/// con `select_latest_engine_release`.
pub fn fetch_latest(api_base: &str, repo: &str) -> Result<ReleaseInfo, String> {
    let url = format!("{api_base}/repos/{repo}/releases?per_page=100");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(300)))
        .build()
        .into();
    let mut resp = match agent
        .get(&url)
        .header("User-Agent", "GlassEnginer-Launcher")
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => {
            return Err(format!(
                "El repositorio '{repo}' no responde (404): puede ser privado o no existir. El Launcher no usa tokens: solo puede leer repositorios públicos."
            ));
        }
        Err(e) => return Err(e.to_string()),
    };
    let body = resp
        .body_mut()
        .with_config()
        .limit(LIMITE_LECTURA_HTTP)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let releases: Vec<ReleaseInfo> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    select_latest_engine_release(&releases)
        .ok_or_else(|| format!("No hay ninguna release del motor publicada en '{repo}'."))
}

/// ¿Es `tag` un tag de release DEL MOTOR? Los del motor son
/// `v<versión>` ('v' seguida de dígito y parseable por tag_version);
/// los de complementos llevan nombre (p. ej. `herramienta-3d-v1.0.1`)
/// y quedan fuera.
pub fn is_engine_tag(tag: &str) -> bool {
    let mut chars = tag.chars();
    chars.next() == Some('v')
        && matches!(chars.next(), Some(c) if c.is_ascii_digit())
        && tag_version(tag).is_ok()
}

/// Elige la release del motor más reciente de una lista (la de
/// `GET /repos/{repo}/releases`): descarta borradores, se queda con
/// los tags del motor (las prerelease SÍ cuentan: las del motor se
/// publican como prerelease) y devuelve la mayor según VersionKey.
pub fn select_latest_engine_release(releases: &[ReleaseInfo]) -> Option<ReleaseInfo> {
    releases
        .iter()
        .filter(|r| !r.draft && is_engine_tag(&r.tag_name))
        .filter_map(|r| tag_version(&r.tag_name).ok().map(|k| (k, r)))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, r)| r.clone())
}

/// ¿Es `tag` un tag de release DEL HUB? Los del Hub son
/// `hub-v<versión>` ('hub-v' seguida de dígito y parseable por
/// tag_version); los del motor y los complementos quedan fuera.
pub fn is_hub_tag(tag: &str) -> bool {
    let Some(rest) = tag.strip_prefix("hub-v") else {
        return false;
    };
    matches!(rest.chars().next(), Some(c) if c.is_ascii_digit())
        && tag_version(rest).is_ok()
}

/// La VersionKey de un tag de release del Hub (`hub-v1.2.3[-pre]`).
/// Devuelve error si el tag no tiene la forma de un tag del Hub.
pub fn hub_tag_version(tag: &str) -> Result<VersionKey, String> {
    let Some(rest) = tag.strip_prefix("hub-v") else {
        return Err(format!(
            "'{tag}' no es un tag de release del Hub (falta el prefijo 'hub-v')."
        ));
    };
    if !matches!(rest.chars().next(), Some(c) if c.is_ascii_digit()) {
        return Err(format!(
            "'{tag}' no es un tag de release del Hub (la versión no empieza por dígito)."
        ));
    }
    tag_version(rest)
}

/// Elige la release del Hub más reciente de una lista (la de
/// `GET /repos/{repo}/releases`): descarta borradores, se queda con
/// los tags del Hub (las prerelease SÍ cuentan) y devuelve la mayor
/// según VersionKey.
pub fn select_latest_hub_release(releases: &[ReleaseInfo]) -> Option<ReleaseInfo> {
    releases
        .iter()
        .filter(|r| !r.draft && is_hub_tag(&r.tag_name))
        .filter_map(|r| hub_tag_version(&r.tag_name).ok().map(|k| (k, r)))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, r)| r.clone())
}

/// Devuelve la release del Hub más reciente publicada en `repo`.
/// Espejo de `fetch_latest` con el selector del Hub (S9): misma
/// petición a la lista de releases y mismos errores de transporte.
pub fn fetch_latest_hub(api_base: &str, repo: &str) -> Result<ReleaseInfo, String> {
    let url = format!("{api_base}/repos/{repo}/releases?per_page=100");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(300)))
        .build()
        .into();
    let mut resp = match agent
        .get(&url)
        .header("User-Agent", "GlassEnginer-Launcher")
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => {
            return Err(format!(
                "El repositorio '{repo}' no responde (404): puede ser privado o no existir. El Launcher no usa tokens: solo puede leer repositorios públicos."
            ));
        }
        Err(e) => return Err(e.to_string()),
    };
    let body = resp
        .body_mut()
        .with_config()
        .limit(LIMITE_LECTURA_HTTP)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let releases: Vec<ReleaseInfo> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    select_latest_hub_release(&releases)
        .ok_or_else(|| format!("No hay ninguna release del Hub publicada en '{repo}'."))
}

/// Comprobación de actualización del propio Hub contra `HUB_VERSION`.
/// La comparación se hace entre VersionKey: `is_newer` no sirve aquí
/// porque `tag_version` no sabe desnudar el prefijo `hub-v`.
pub fn check_hub_update(api_base: &str, repo: &str) -> Result<UpdateCheck, String> {
    let release = fetch_latest_hub(api_base, repo)?;
    let version_release = hub_tag_version(&release.tag_name)?;
    let version_actual = tag_version(HUB_VERSION)?;
    if version_release > version_actual {
        Ok(UpdateCheck::Available(release))
    } else {
        Ok(UpdateCheck::UpToDate)
    }
}

/// El asset del Hub para ESTA plataforma: en Linux el AppImage, en
/// Windows el setup .exe; en otras plataformas no hay paquete del Hub.
pub fn pick_hub_asset(release: &ReleaseInfo) -> Option<&AssetInfo> {
    // Selección estricta (auditoría S9): el paquete del Hub es el del
    // Launcher para esta plataforma — en Linux el AppImage, en Windows
    // el setup — y debe ser el ÚNICO candidato; con varios, la elección
    // sería ambigua y no se instala nada.
    let slug = platform_slug();
    let mut candidatos = release.assets.iter().filter(|a| {
        let n = a.name.to_lowercase();
        n.contains(slug) && n.contains("launcher")
            && if cfg!(target_os = "linux") {
                n.ends_with(".appimage")
            } else if cfg!(target_os = "windows") {
                n.ends_with(".exe") && n.contains("setup")
            } else {
                false
            }
    });
    match (candidatos.next(), candidatos.next()) {
        (Some(unico), None) => Some(unico),
        _ => None,
    }
}

pub fn expected_sha256(release: &ReleaseInfo, asset: &AssetInfo) -> Result<String, String> {
    if let Some(hash) = digest_sha256(asset) {
        return Ok(hash);
    }

    if let Some(checksums) = release
        .assets
        .iter()
        .find(|a| a.name == "checksums.txt" || a.name == "SHA256SUMS.txt")
    {
        let data = get_bytes(&checksums.browser_download_url)?;
        let texto = String::from_utf8_lossy(&data);
        if let Some(hash) = parse_checksums(&texto, &asset.name) {
            return Ok(hash);
        }
    }

    let sha_name = format!("{}.sha256", asset.name);
    if let Some(sha_asset) = release.assets.iter().find(|a| a.name == sha_name) {
        let data = get_bytes(&sha_asset.browser_download_url)?;
        let texto = String::from_utf8_lossy(&data);
        if let Some(token) = texto.split_whitespace().next() {
            if token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit()) {
                return Ok(token.to_ascii_lowercase());
            }
        }
    }

    Err("El release no publica SHA-256 (ni digest ni archivo de checksums): por seguridad no se instala sin hash.".to_string())
}

#[derive(Debug, Clone)]
pub enum UpdateCheck {
    UpToDate,
    Available(ReleaseInfo),
}

pub fn check_update_against(api_base: &str, repo: &str, version_actual: &str) -> Result<UpdateCheck, String> {
    let release = fetch_latest(api_base, repo)?;
    if is_newer(&release.tag_name, version_actual) {
        Ok(UpdateCheck::Available(release))
    } else {
        Ok(UpdateCheck::UpToDate)
    }
}

pub fn check_update(api_base: &str, repo: &str) -> Result<UpdateCheck, String> {
    check_update_against(api_base, repo, ENGINE_VERSION)
}

pub fn extract_zip(data: &[u8], dest: &std::path::Path) -> Result<usize, String> {
    extract_zip_reader(std::io::Cursor::new(data), dest)
}

/// Extrae el .zip que hay en `path` (la vía de los paquetes
/// descargados en streaming: el archivo no se carga en memoria).
pub fn extract_zip_file(path: &std::path::Path, dest: &std::path::Path) -> Result<usize, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    extract_zip_reader(file, dest)
}

fn extract_zip_reader<R: std::io::Read + std::io::Seek>(
    reader: R,
    dest: &std::path::Path,
) -> Result<usize, String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;

    let mut ar = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;

    let mut count = 0usize;

    for i in 0..ar.len() {
        let mut f = ar.by_index(i).map_err(|e| e.to_string())?;

        let rel = match f.enclosed_name() {
            Some(p) => p,
            None => continue,
        };

        let ruta = dest.join(rel);

        if f.is_dir() {
            std::fs::create_dir_all(&ruta).map_err(|e| e.to_string())?;
            continue;
        }

        if let Some(parent) = ruta.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let mut out = std::fs::File::create(&ruta).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, &mut out).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(modo) = f.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&ruta, std::fs::Permissions::from_mode(modo));
        }
        count += 1;
    }

    if count == 0 {
        Err("El .zip no contiene archivos.".to_string())
    } else {
        Ok(count)
    }
}

/// Resumen de una instalacion del motor gestionado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumenInstalacionMotor {
    pub version: String,
    pub proyectos_migrados: usize,
    pub retiradas: Vec<String>,
    pub diferidas: Vec<String>,
    pub conservada: Option<String>,
}

/// Instala el motor `version` desde un .zip ya descargado (y con el
/// SHA-256 ya verificado por el llamante) bajo la raiz de datos
/// `root`: extrae a engines/.staging-<version>, valida que el paquete
/// trae el ejecutable en su raiz, lo renombra a engines/<version> y
/// delega en `registrar_motor_en_uso` (registro, retirada y sellado).
/// Devuelve el resumen.
pub fn instalar_motor_desde_zip(
    root: &std::path::Path,
    zip: &std::path::Path,
    version: &str,
    proyectos: &[std::path::PathBuf],
) -> Result<ResumenInstalacionMotor, String> {
    let engines = engines_dir_in(root);
    std::fs::create_dir_all(&engines).map_err(|e| e.to_string())?;
    let staging = engines.join(format!(".staging-{}", version));
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    let resultado = (|| -> Result<(), String> {
        extract_zip_file(zip, &staging)?;
        let exe = if cfg!(windows) { "glass_app.exe" } else { "glass_app" };
        if !staging.join(exe).is_file() {
            return Err(format!(
                "El paquete de la versión {} no contiene {} en su raíz: no es un paquete del motor.",
                version, exe
            ));
        }
        let destino = engines.join(version);
        if destino.exists() {
            std::fs::remove_dir_all(&destino).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&staging, &destino).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = resultado {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    registrar_motor_en_uso(root, version, proyectos)
}

/// Registra en `engines.json` (bajo `root`) la versión `version`, cuya
/// carpeta ya está en engines/<version>, como el motor en uso. Orden
/// seguro: primero guarda el registro (desde ese momento el registro
/// manda y apunta a una carpeta que existe); después borra las
/// versiones retiradas sin sesión activa (las que no se puedan borrar
/// quedan en pendientes_borrar, sin fallar) y por último sella los
/// proyectos dados con la versión nueva. Devuelve el resumen.
pub fn registrar_motor_en_uso(
    root: &std::path::Path,
    version: &str,
    proyectos: &[std::path::PathBuf],
) -> Result<ResumenInstalacionMotor, String> {
    registrar_motor_en_uso_int(root, version, proyectos, false)
}

/// Como `registrar_motor_en_uso`; con `conservar_previa` (la vía de
/// adopción) la versión que estaba en uso NUNCA se borra: queda como
/// conservada, para no destruir la última versión sana conocida si
/// la carpeta adoptada resultara estar dañada.
fn registrar_motor_en_uso_int(
    root: &std::path::Path,
    version: &str,
    proyectos: &[std::path::PathBuf],
    conservar_previa: bool,
) -> Result<ResumenInstalacionMotor, String> {
    // El registro nunca apunta a una carpeta que no está en disco:
    // esa fue la mitad del defecto original.
    if !exe_en_disco_in(root, version) {
        return Err(format!(
            "La versión {} no está en disco: no se puede registrar como motor en uso.",
            version
        ));
    }
    let engines = engines_dir_in(root);
    let mut store = EngineStore::load_in(root);
    let mut plan = plan_retirada(store.en_uso.as_deref(), store.conservada.as_deref(), version);
    if conservar_previa {
        if let Some(prev) = store.en_uso.clone() {
            if prev != version {
                plan.borrar.retain(|v| v != &prev);
                plan.conservada = Some(prev);
            }
        }
    }
    let mut diferidas = Vec::new();
    let mut por_borrar = Vec::new();
    for v in &plan.borrar {
        if version_con_sesion_activa_en(root, v) {
            diferidas.push(v.clone());
            if !store.pendientes_borrar.contains(v) {
                store.pendientes_borrar.push(v.clone());
            }
        } else {
            por_borrar.push(v.clone());
        }
    }
    store.en_uso = Some(version.to_string());
    store.conservada = plan.conservada.clone();
    let conservada_final = store.conservada.clone();
    store.pendientes_borrar.retain(|v| {
        v != version && Some(v.as_str()) != conservada_final.as_deref()
    });
    store.save_in(root)?;
    let mut retiradas = Vec::new();
    let mut pendientes_cambiadas = false;
    for v in por_borrar {
        match std::fs::remove_dir_all(engines.join(&v)) {
            Ok(()) => retiradas.push(v),
            // Si la carpeta ya no está (borrada a mano o por un
            // proceso anterior), el objetivo está cumplido: cuenta
            // como retirada, no como diferida.
            Err(_) if !engines.join(&v).exists() => retiradas.push(v),
            Err(_) => {
                if !store.pendientes_borrar.contains(&v) {
                    store.pendientes_borrar.push(v.clone());
                    pendientes_cambiadas = true;
                }
                diferidas.push(v);
            }
        }
    }
    if pendientes_cambiadas {
        // Best-effort: la activación ya está guardada; si este segundo
        // guardado falla, las pendientes se reconcilian al arrancar.
        // Devolver Err aquí diría «la activación falló» cuando el
        // motor en uso ya cambió: un error engañoso.
        if let Err(e) = store.save_in(root) {
            log::warn!("No se pudo guardar engines.json tras retirar versiones: {e}");
        }
    }
    let mut migrados = 0;
    for dir in proyectos {
        if let Ok(true) = sellar_proyecto(dir, version) {
            migrados += 1;
        }
    }
    Ok(ResumenInstalacionMotor {
        version: version.to_string(),
        proyectos_migrados: migrados,
        retiradas,
        diferidas,
        conservada: plan.conservada,
    })
}

/// Completa una instalación que quedó a medias: la carpeta
/// engines/<version> ya se renombró pero el registro no llegó a
/// actualizarse. La carpeta renombrada es íntegra por construcción,
/// porque el renombrado solo ocurre tras extraer el zip completo y
/// comprobar el ejecutable. Si el ejecutable de `version` no está en
/// disco bajo `root`, devuelve error; si está, delega en
/// `registrar_motor_en_uso`.
pub fn completar_instalacion_en_disco(
    root: &std::path::Path,
    version: &str,
    proyectos: &[std::path::PathBuf],
) -> Result<ResumenInstalacionMotor, String> {
    if !exe_en_disco_in(root, version) {
        return Err(format!(
            "La versión {} no está descargada en este equipo.",
            version
        ));
    }
    // Integridad mínima de la carpeta adoptada: el paquete del motor
    // trae siempre managed/ (el C#) y tools/ junto al ejecutable. Si
    // faltan, la carpeta está dañada (borrado parcial posterior) y
    // adoptarla dejaría al equipo con un motor que no arranca: se
    // rechaza y el panel vuelve a ofrecer la descarga completa.
    let dir = engines_dir_in(root).join(version);
    if !dir.join("managed").is_dir() || !dir.join("tools").is_dir() {
        return Err(format!(
            "La copia descargada de la versión {} está incompleta (faltan managed/ o tools/): no se activó. Descárgala de nuevo.",
            version
        ));
    }
    registrar_motor_en_uso_int(root, version, proyectos, true)
}

/// El ejecutable del motor `version` está en disco bajo `root`.
fn exe_en_disco_in(root: &std::path::Path, version: &str) -> bool {
    motor_exe_in(root, version).is_file()
}

/// Barre los residuos de instalaciones interrumpidas bajo `root`:
/// temporales de descarga (`.descarga-*`) y carpetas de staging
/// (`.staging-*`) dentro de engines/. Lo que no se pueda borrar se
/// deja: barrer nunca impide instalar.
/// Lock de sesion de un motor gestionado: mientras un proceso lo
/// sostiene (lock compartido sobre engines/<version>/.sesion.lock),
/// el Hub sabe que esa version esta viva y no la borra. El guarda
/// conserva el archivo abierto; al soltarse (Drop) el lock se libera.
pub struct LockSesion {
    _archivo: std::fs::File,
}

/// Guarda del lock exclusivo de instancia única del Hub: mientras
/// vive, ningún otro Hub puede arrancar en esta raíz de datos (una
/// segunda instancia pide despertar a la primera y sale). El lock
/// se libera al soltar la guarda (Drop), es decir, al salir el
/// proceso.
pub struct BloqueoHub {
    _archivo: std::fs::File,
}

/// Ruta del archivo de lock de instancia única del Hub.
pub fn ruta_bloqueo_hub_en(root: &Path) -> PathBuf {
    root.join("hub.lock")
}

/// Intenta tomar el lock exclusivo del Hub bajo `root`, sin
/// esperar. Ante cualquier error (ocupado o de E/S) devuelve None:
/// fail-closed, se asume que ya hay otro Hub.
pub fn tomar_bloqueo_hub_en(root: &Path) -> Option<BloqueoHub> {
    std::fs::create_dir_all(root).ok()?;
    let archivo = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(ruta_bloqueo_hub_en(root))
        .ok()?;
    archivo.try_lock().ok()?;
    Some(BloqueoHub { _archivo: archivo })
}

/// Intenta tomar el lock exclusivo del Hub en la raíz de datos.
pub fn tomar_bloqueo_hub() -> Option<BloqueoHub> {
    tomar_bloqueo_hub_en(&data_root())
}

/// Objetivo de la sonda de GPU: el motor GESTIONADO en uso bajo
/// `root` — su ejecutable y su version. None si no hay motor en uso
/// o su ejecutable no esta en disco (el llamante sondea entonces el
/// propio binario, como en el modo combinado).
/// Borra la version conservada bajo `root` y limpia el puntero.
/// Con sesion activa en ella devuelve Err (que la cierre primero):
/// un borrado a clic no se difiere en silencio. Ok(None) si no
/// habia conservada.
pub fn borrar_conservada_en(root: &std::path::Path) -> Result<Option<String>, String> {
    let mut store = EngineStore::load_in(root);
    let Some(version) = store.conservada.clone() else {
        return Ok(None);
    };
    if version_con_sesion_activa_en(root, &version) {
        return Err(format!("la version conservada v{version} tiene una sesion abierta: cierrala antes de borrarla"));
    }
    let dir = engines_dir_in(root).join(&version);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("no se pudo borrar engines/{version}: {e}"))?;
    }
    store.conservada = None;
    store.save_in(root)?;
    Ok(Some(version))
}

/// Nombres de las carpetas que queden en el `versions/` heredado
/// del mecanismo viejo (vacias si no hay carpeta o esta vacia).
pub fn restos_versions_en(root: &std::path::Path) -> Vec<String> {
    let dir = root.join("versions");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut nombres: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    nombres.sort();
    nombres
}

/// Borra entero el `versions/` heredado bajo `root`.
pub fn borrar_versions_en(root: &std::path::Path) -> Result<(), String> {
    let dir = root.join("versions");
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("no se pudo borrar versions/: {e}"))?;
    }
    Ok(())
}

pub fn objetivo_sonda_en(root: &std::path::Path) -> Option<(std::path::PathBuf, String)> {
    let store = EngineStore::load_in(root);
    let version = store.en_uso?;
    let exe = motor_exe_in(root, &version);
    exe.is_file().then_some((exe, version))
}

/// Objetivo de la sonda de GPU en la raiz de datos real.
pub fn objetivo_sonda() -> Option<(std::path::PathBuf, String)> {
    objetivo_sonda_en(&data_root())
}

/// Ruta del archivo de lock de sesion de una version.
pub fn lock_sesion_path_in(root: &std::path::Path, version: &str) -> std::path::PathBuf {
    engines_dir_in(root).join(version).join(".sesion.lock")
}

/// Toma el lock de sesion (compartido) de `version` bajo `root`:
/// varios editores de la misma version lo comparten.
pub fn bloquear_sesion_en(root: &std::path::Path, version: &str) -> Result<LockSesion, String> {
    let dir = engines_dir_in(root).join(version);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let archivo = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock_sesion_path_in(root, version))
        .map_err(|e| e.to_string())?;
    archivo.lock_shared().map_err(|e| e.to_string())?;
    Ok(LockSesion { _archivo: archivo })
}

/// Toma el lock de sesion de `version` en la raiz de datos real.
pub fn bloquear_sesion(version: &str) -> Result<LockSesion, String> {
    bloquear_sesion_en(&data_root(), version)
}

/// ¿Tiene `version` una sesion viva bajo `root`? Sonda con un lock
/// exclusivo sin esperar: si algun editor sostiene el compartido, la
/// sonda no puede tomar el exclusivo.
pub fn version_con_sesion_activa_en(root: &std::path::Path, version: &str) -> bool {
    let ruta = lock_sesion_path_in(root, version);
    if !ruta.is_file() {
        return false;
    }
    let Ok(archivo) = std::fs::OpenOptions::new().read(true).open(&ruta) else {
        return false;
    };
    match archivo.try_lock() {
        Ok(()) => {
            let _ = archivo.unlock();
            false
        }
        // WouldBlock = sesion viva. Cualquier otro error = indeterminado:
        // la sonda es fail-closed y ante la duda NO se borra.
        Err(_) => true,
    }
}

/// ¿Tiene `version` una sesion viva en la raiz de datos real?
pub fn version_con_sesion_activa(version: &str) -> bool {
    version_con_sesion_activa_en(&data_root(), version)
}

/// Ruta de la bandera con la que el Hub solicita el cierre del
/// motor con `pid` (Bug A): el motor la sondea y se cierra con su
/// propia regla de guardado.
pub fn ruta_solicitud_cierre_en(root: &Path, pid: u32) -> PathBuf {
    root.join(format!("cerrar-{pid}.flag"))
}

/// Solicita el cierre del motor con `pid` dejando su bandera en la
/// raíz de datos. Devuelve false si no se pudo escribir.
pub fn solicitar_cierre_motor(pid: u32) -> bool {
    let root = data_root();
    if std::fs::create_dir_all(&root).is_err() {
        return false;
    }
    std::fs::write(ruta_solicitud_cierre_en(&root, pid), pid.to_string()).is_ok()
}

/// ¿Hay una solicitud de cierre para ESTE proceso?
pub fn solicitud_cierre_propia() -> bool {
    ruta_solicitud_cierre_en(&data_root(), std::process::id()).is_file()
}

/// Retira la solicitud de cierre de este proceso, si existe.
pub fn retirar_solicitud_cierre_propia() {
    let _ = std::fs::remove_file(ruta_solicitud_cierre_en(
        &data_root(),
        std::process::id(),
    ));
}

/// Ruta de la bandera con la que una segunda instancia del Hub
/// pide a la primera (en segundo plano) que se muestre.
pub fn ruta_despertar_hub_en(root: &Path) -> PathBuf {
    root.join("hub-despierta.flag")
}

/// Pide despertar al Hub en segundo plano. Devuelve false si no
/// se pudo escribir la bandera.
pub fn solicitar_despertar_hub() -> bool {
    let root = data_root();
    if std::fs::create_dir_all(&root).is_err() {
        return false;
    }
    std::fs::write(ruta_despertar_hub_en(&root), "").is_ok()
}

/// Consume la bandera de despertar bajo `root`: si existía y pudo
/// borrarse, devuelve true (una sola vez).
pub fn consumir_despertar_hub_en(root: &Path) -> bool {
    std::fs::remove_file(ruta_despertar_hub_en(root)).is_ok()
}

/// Consume la bandera de despertar de la raíz de datos.
pub fn consumir_despertar_hub() -> bool {
    consumir_despertar_hub_en(&data_root())
}

/// Si este proceso ES un motor gestionado (su ejecutable vive en
/// engines/<version>/ de la raiz de datos), toma el lock de sesion
/// de su version y lo devuelve; si no, None.
pub fn bloquear_sesion_si_gestionado() -> Option<LockSesion> {
    let exe = std::env::current_exe().ok()?;
    let engines = engines_dir();
    let dentro = exe.strip_prefix(&engines).ok()?;
    let version = dentro.components().next()?.as_os_str().to_str()?.to_string();
    match bloquear_sesion(&version) {
        Ok(lock) => {
            log::info!("Lock de sesion del motor gestionado v{} tomado.", version);
            Some(lock)
        }
        Err(e) => {
            log::warn!("No se pudo tomar el lock de sesion de la v{}: {}", version, e);
            None
        }
    }
}

/// ¿Es ESTE proceso un motor gestionado (su ejecutable vive en
/// engines/<version>/ de la raíz de datos)? No toma ningún lock:
/// es la mitad de solo lectura de `bloquear_sesion_si_gestionado`.
pub fn es_gestionado() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Ok(dentro) = exe.strip_prefix(engines_dir()) else {
        return false;
    };
    dentro.components().next().is_some()
}

/// Completa los borrados diferidos bajo `root`: de las versiones en
/// pendientes_borrar del EngineStore, borra las que ya no tienen
/// sesion viva (una carpeta ya ausente cuenta como borrada). Guarda
/// el registro solo si cambio. Devuelve las versiones retiradas.
/// Añade a `pendientes_borrar` las carpetas de versión de `engines/` que el store
/// no conoce. No borra nada: el borrado posterior respeta las sesiones activas.
pub fn reconciliar_huerfanas_en(root: &std::path::Path) -> Vec<String> {
    let mut store = EngineStore::load_in(root);
    // Defensa (auditoría S10): la versión en uso y la conservada NUNCA
    // son pendientes de borrar, vengan de donde vengan (un rollback
    // puede devolver a en uso una versión que quedó pendiente con una
    // sesión viva). Se purgan aquí y también en procesar.
    let mut cambio = false;
    let en_uso = store.en_uso.clone();
    let conservada = store.conservada.clone();
    store.pendientes_borrar.retain(|v| {
        let protegida = Some(v.as_str()) == en_uso.as_deref()
            || Some(v.as_str()) == conservada.as_deref();
        if protegida {
            cambio = true;
        }
        !protegida
    });
    let dir = engines_dir_in(root);
    let entradas = match std::fs::read_dir(&dir) {
        Ok(entradas) => entradas,
        Err(_) => return Vec::new(),
    };

    let mut candidatas = Vec::new();
    for entrada in entradas.flatten() {
        let nombre = match entrada.file_name().into_string() {
            Ok(nombre) => nombre,
            Err(_) => continue,
        };
        if !entrada.path().is_dir() {
            continue;
        }
        if tag_version(&nombre).is_err() {
            continue;
        }
        if store.en_uso.as_deref() == Some(nombre.as_str()) {
            continue;
        }
        if store.conservada.as_deref() == Some(nombre.as_str()) {
            continue;
        }
        if store.pendientes_borrar.iter().any(|p| p == &nombre) {
            continue;
        }
        // Una carpeta con el ejecutable del motor dentro es una
        // instalación COMPLETA cuyo registro pudo quedar a medias
        // (la adopción del panel la completa): no es una huérfana
        // para borrar. Solo se reconcilian como basura las carpetas
        // de versión sin ejecutable (restos sin motor dentro).
        if exe_en_disco_in(root, &nombre) {
            continue;
        }
        candidatas.push(nombre);
    }

    candidatas.sort();
    let mut anadidas = Vec::new();
    for nombre in candidatas {
        store.pendientes_borrar.push(nombre.clone());
        anadidas.push(nombre);
    }
    if !anadidas.is_empty() || cambio {
        // Si falla el guardado, se tolera (el arranque no debe
        // bloquearse) pero se deja rastro: sin persistir, procesar no
        // verá estas pendientes (auditoría S10).
        if let Err(e) = store.save_in(root) {
            log::warn!("No se pudo guardar engines.json tras reconciliar huérfanas: {e}");
        }
    }
    anadidas
}

/// Envoltorio de `reconciliar_huerfanas_en` sobre `data_root()`.
pub fn reconciliar_huerfanas() -> Vec<String> {
    reconciliar_huerfanas_en(&data_root())
}

pub fn procesar_pendientes_borrar_en(root: &std::path::Path) -> Vec<String> {
    let mut store = EngineStore::load_in(root);
    // Defensa (auditoría S10): en uso y conservada jamás se borran,
    // aunque una lista de pendientes vieja las nombre; se purgan de la
    // lista y el purgado se persiste con el resto de cambios.
    let antes = store.pendientes_borrar.len();
    let en_uso = store.en_uso.clone();
    let conservada = store.conservada.clone();
    store.pendientes_borrar.retain(|v| {
        Some(v.as_str()) != en_uso.as_deref() && Some(v.as_str()) != conservada.as_deref()
    });
    let purgadas = antes - store.pendientes_borrar.len();
    if store.pendientes_borrar.is_empty() {
        if purgadas > 0 {
            let _ = store.save_in(root);
        }
        return Vec::new();
    }
    let engines = engines_dir_in(root);
    let mut borradas = Vec::new();
    let pendientes_antes = store.pendientes_borrar.len();
    store.pendientes_borrar.retain(|v| {
        if version_con_sesion_activa_en(root, v) {
            return true;
        }
        // Firma de instalación varada (carpeta completa, registro a
        // medias): tiene ejecutable y es más nueva que la en uso — o
        // no hay versión en uso con la que comparar (registro ausente
        // o ilegible). Ante la duda no se destruye la única copia
        // completa: SALE de pendientes sin borrarse (deja de ser una
        // retirada diferida) y el panel la ofrecerá en adopción si
        // vuelve a ser la versión buscada.
        if exe_en_disco_in(root, v) {
            let protegida = match en_uso.as_deref() {
                None => true,
                Some(uso) => match (tag_version(v), tag_version(uso)) {
                    (Ok(clave_v), Ok(clave_uso)) => clave_v > clave_uso,
                    _ => true,
                },
            };
            if protegida {
                return false;
            }
        }
        match std::fs::remove_dir_all(engines.join(v)) {
            Ok(()) => {
                borradas.push(v.clone());
                false
            }
            Err(_) if !engines.join(v).exists() => {
                borradas.push(v.clone());
                false
            }
            Err(_) => true,
        }
    });
    // Se guarda si cambió algo: borradas, purgadas o salidas de la
    // lista sin borrado (la varada que se protege deja de ser una
    // retirada diferida: sin este guardado seguiría en el archivo).
    if !borradas.is_empty() || purgadas > 0 || store.pendientes_borrar.len() != pendientes_antes {
        let _ = store.save_in(root);
    }
    borradas
}

/// Completa los borrados diferidos en la raiz de datos real.
pub fn procesar_pendientes_borrar() -> Vec<String> {
    procesar_pendientes_borrar_en(&data_root())
}

pub fn limpiar_residuos_instalacion(root: &std::path::Path) {
    let engines = engines_dir_in(root);
    let Ok(entradas) = std::fs::read_dir(&engines) else {
        return;
    };
    for entrada in entradas.flatten() {
        let nombre = entrada.file_name();
        let nombre = nombre.to_string_lossy();
        if nombre.starts_with(".descarga-") || nombre.starts_with(".staging-") {
            let ruta = entrada.path();
            if ruta.is_dir() {
                let _ = std::fs::remove_dir_all(&ruta);
            } else {
                let _ = std::fs::remove_file(&ruta);
            }
        }
    }
}

pub fn install_release(
    release: &ReleaseInfo,
    store: &mut LauncherStore,
) -> Result<ResumenInstalacionMotor, String> {
    limpiar_residuos_instalacion(&data_root());
    let asset = pick_asset(release).ok_or_else(|| {
        format!(
            "El release {} no trae un .zip para esta plataforma ({}).",
            release.tag_name,
            platform_slug()
        )
    })?;

    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        .to_string();

    // Descarga en streaming a un temporal DENTRO de engines/ (mismo
    // disco que el destino: la instalacion renombra, no copia). El
    // SHA-256 se verifica sobre el archivo ANTES de extraer nada. El
    // temporal se borra siempre al salir.
    let root = data_root();
    let engines = engines_dir_in(&root);
    std::fs::create_dir_all(&engines).map_err(|e| e.to_string())?;
    let tmp = engines.join(format!(
        ".descarga-{}-{}.zip",
        version,
        std::process::id()
    ));
    let resultado = (|| -> Result<ResumenInstalacionMotor, String> {
        get_to_file(&asset.browser_download_url, &tmp)?;
        let esperado = expected_sha256(release, asset)?;
        if sha256_hex_file(&tmp)? != esperado {
            return Err("El SHA-256 no coincide: la descarga no es íntegra o no es el archivo publicado. No se instaló nada.".to_string());
        }
        let proyectos: Vec<std::path::PathBuf> = store
            .projects
            .iter()
            .map(|p| std::path::PathBuf::from(&p.path))
            .collect();
        instalar_motor_desde_zip(&root, &tmp, &version, &proyectos)
    })();
    let _ = std::fs::remove_file(&tmp);
    resultado
}

/// La carpeta de esta versión está en disco; puede no estar registrada
/// ni en uso — el registro manda en `engines.json`.
pub fn motor_en_disco(tag: &str) -> bool {
    let version = tag.strip_prefix('v').unwrap_or(tag);
    exe_en_disco_in(&data_root(), version)
}

// ---------------------------------------------------------------------------
// Blender portable oficial (conversión de .blend sin Blender instalado)
// ---------------------------------------------------------------------------

/// Versión de Blender portable que descarga el launcher. Está fijada (LTS)
/// para que el SHA-256 sea verificable y la conversión de .blend no cambie
/// de comportamiento según el día en que el usuario descargue.
pub const BLENDER_PORTABLE_VERSION: &str = "4.2.3";

/// (url, nombre_archivo, sha256) del portable oficial para Windows x64.
const BLENDER_PORTABLE_WINDOWS: (&str, &str, &str) = (
    "https://download.blender.org/release/Blender4.2/blender-4.2.3-windows-x64.zip",
    "blender-4.2.3-windows-x64.zip",
    "7f8be9b1dc904e7689b3112659791c29cc9ed9d46ce57c4ea5b31a0621489b3a",
);

/// (url, nombre_archivo, sha256) del portable oficial para Linux x64.
const BLENDER_PORTABLE_LINUX: (&str, &str, &str) = (
    "https://download.blender.org/release/Blender4.2/blender-4.2.3-linux-x64.tar.xz",
    "blender-4.2.3-linux-x64.tar.xz",
    "3a64efd1982465395abab4259b4091d5c8c56054c7267e9633e4f702a71ea3f4",
);

/// Temporales dentro de `<dir_motor>/tools/`: al estar en el mismo sistema
/// de archivos que el destino, el aplanado puede usar `rename` (rápido y sin
/// copiar los ~1 GB de Blender).
const NOMBRE_TMP_BLENDER: &str = ".blender-portable.tmp";
const NOMBRE_EXTRACCION_BLENDER: &str = ".blender-extraccion";

/// Paquete oficial de Blender portable para la plataforma actual:
/// `(url, nombre_archivo, sha256)`. `None` si no hay portable oficial.
pub fn paquete_blender_portable() -> Option<(&'static str, &'static str, &'static str)> {
    if cfg!(windows) {
        Some(BLENDER_PORTABLE_WINDOWS)
    } else if cfg!(target_os = "linux") {
        Some(BLENDER_PORTABLE_LINUX)
    } else {
        None
    }
}

/// Nombre del ejecutable de Blender dentro del portable.
fn nombre_ejecutable_blender() -> &'static str {
    if cfg!(windows) {
        "blender.exe"
    } else {
        "blender"
    }
}

/// Directorio donde queda instalado el portable: `<dir_motor>/tools/blender/`.
pub fn dir_blender_portable(dir_motor: &std::path::Path) -> std::path::PathBuf {
    dir_motor.join("tools").join("blender")
}

/// Ruta del ejecutable del portable dentro de `dir_blender_portable`.
pub fn ruta_blender_portable(dir_motor: &std::path::Path) -> std::path::PathBuf {
    dir_blender_portable(dir_motor).join(nombre_ejecutable_blender())
}

/// `true` si el ejecutable del portable ya está en su sitio.
pub fn blender_portable_instalado(dir_motor: &std::path::Path) -> bool {
    ruta_blender_portable(dir_motor).is_file()
}

/// Descarga, verifica e instala el Blender portable oficial en
/// `<dir_motor>/tools/blender/`. Devuelve la ruta del ejecutable.
///
/// El paquete se instala tal cual lo publica Blender (sin modificar su
/// contenido); solo se aplana la carpeta raíz del archivo comprimido.
pub fn descargar_blender_portable(
    dir_motor: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let paquete = paquete_blender_portable().ok_or_else(|| {
        "No hay Blender portable oficial para esta plataforma (solo Windows x64 y Linux x64). \
         Instala Blender manualmente para poder convertir archivos .blend."
            .to_string()
    })?;

    let herramientas = dir_motor.join("tools");
    std::fs::create_dir_all(&herramientas)
        .map_err(|e| format!("No se pudo crear {}: {e}", herramientas.display()))?;
    let tmp = herramientas.join(NOMBRE_TMP_BLENDER);
    let extraccion = herramientas.join(NOMBRE_EXTRACCION_BLENDER);

    // Restos de un intento anterior interrumpido (cierre del launcher, corte de red…).
    limpiar_temporales_blender(&tmp, &extraccion);
    let resultado = instalar_blender_portable(dir_motor, paquete, &tmp, &extraccion);
    // Se limpia siempre, haya ido bien o mal: los temporales pesan cientos de MB.
    limpiar_temporales_blender(&tmp, &extraccion);
    resultado
}

/// Borra el archivo descargado y el directorio de extracción. Los errores se
/// ignoran a propósito: lo normal es que alguno de los dos no exista.
fn limpiar_temporales_blender(tmp: &std::path::Path, extraccion: &std::path::Path) {
    let _ = std::fs::remove_file(tmp);
    let _ = std::fs::remove_dir_all(extraccion);
}

/// Pasos 2–6 de `descargar_blender_portable`, separados para que la
/// limpieza de temporales se haga en un único sitio.
fn instalar_blender_portable(
    dir_motor: &std::path::Path,
    (url, nombre, esperado): (&str, &str, &str),
    tmp: &std::path::Path,
    extraccion: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    // En streaming a disco: el paquete pesa cientos de MB (defecto 8c).
    get_to_file(url, tmp).map_err(|e| {
        format!("No se pudo descargar Blender {BLENDER_PORTABLE_VERSION} desde {url}: {e}")
    })?;

    let calculado = sha256_hex_file(tmp)
        .map_err(|e| format!("No se pudo leer el paquete descargado {}: {e}", tmp.display()))?;
    if !calculado.eq_ignore_ascii_case(esperado) {
        let _ = std::fs::remove_file(tmp);
        return Err(format!(
            "El SHA-256 de {nombre} no coincide (esperado {esperado}, obtenido {calculado}). \
             No se ha instalado nada."
        ));
    }

    std::fs::create_dir_all(extraccion)
        .map_err(|e| format!("No se pudo crear {}: {e}", extraccion.display()))?;
    // El formato se decide por el nombre del paquete fijado, no por la
    // plataforma, para que ambas ramas compilen y se comprueben en todas.
    if nombre.ends_with(".zip") {
        extract_zip_file(tmp, extraccion)
            .map_err(|e| format!("No se pudo descomprimir {nombre}: {e}"))?;
    } else {
        extraer_tar_xz(tmp, extraccion)?;
    }

    aplanar_extraccion_blender(extraccion, &dir_blender_portable(dir_motor))?;

    let ejecutable = ruta_blender_portable(dir_motor);
    if !ejecutable.is_file() {
        return Err(format!(
            "El paquete de Blender se descomprimió, pero no contiene el ejecutable esperado ({}).",
            ejecutable.display()
        ));
    }

    // `rename` conserva los permisos del tar, pero se fuerzan por si el
    // sistema de archivos o un umask restrictivo los perdieron.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&ejecutable, std::fs::Permissions::from_mode(0o755)).map_err(
            |e| {
                format!(
                    "No se pudo dar permiso de ejecución a {}: {e}",
                    ejecutable.display()
                )
            },
        )?;
    }

    Ok(ejecutable)
}

/// Descomprime un .tar.xz con el `tar` del sistema (sin dependencias nuevas:
/// el crate `zip` no lee xz). Se pasan rutas como argumentos, sin shell.
fn extraer_tar_xz(archivo: &std::path::Path, destino: &std::path::Path) -> Result<(), String> {
    let salida = std::process::Command::new("tar")
        .arg("-xJf")
        .arg(archivo)
        .arg("-C")
        .arg(destino)
        .output()
        .map_err(|e| {
            format!(
                "No se pudo ejecutar `tar` para descomprimir Blender ({e}). \
                 Instala `tar` y `xz` (xz-utils) e inténtalo de nuevo."
            )
        })?;
    if !salida.status.success() {
        let stderr = String::from_utf8_lossy(&salida.stderr);
        return Err(format!(
            "`tar` no pudo descomprimir Blender ({}): {}. Comprueba que `xz` está instalado.",
            salida.status,
            stderr.trim()
        ));
    }
    Ok(())
}

/// Mueve el CONTENIDO de la única carpeta raíz `blender-*` de `extraccion`
/// a `destino`. Si ya había un portable en `destino`, se borra antes; esto se
/// hace después de localizar la raíz para no perder el anterior si el
/// paquete nuevo viene mal formado.
fn aplanar_extraccion_blender(
    extraccion: &std::path::Path,
    destino: &std::path::Path,
) -> Result<(), String> {
    let entradas = std::fs::read_dir(extraccion)
        .map_err(|e| format!("No se pudo leer {}: {e}", extraccion.display()))?;
    let mut raices = Vec::new();
    for entrada in entradas {
        let entrada = entrada
            .map_err(|e| format!("No se pudo leer {}: {e}", extraccion.display()))?;
        let es_dir = entrada.file_type().is_ok_and(|t| t.is_dir());
        if es_dir && entrada.file_name().to_string_lossy().starts_with("blender-") {
            raices.push(entrada.path());
        }
    }
    let raiz = match raices.as_slice() {
        [unica] => unica.clone(),
        [] => {
            return Err(
                "El paquete de Blender no contiene la carpeta raíz esperada (blender-…)."
                    .to_string(),
            )
        }
        _ => {
            return Err(
                "El paquete de Blender contiene varias carpetas raíz blender-…; no se sabe cuál instalar."
                    .to_string(),
            )
        }
    };

    if destino.exists() {
        std::fs::remove_dir_all(destino).map_err(|e| {
            format!(
                "No se pudo borrar el Blender portable anterior en {}: {e}",
                destino.display()
            )
        })?;
    }
    std::fs::create_dir_all(destino)
        .map_err(|e| format!("No se pudo crear {}: {e}", destino.display()))?;

    let contenido = std::fs::read_dir(&raiz)
        .map_err(|e| format!("No se pudo leer {}: {e}", raiz.display()))?;
    for entrada in contenido {
        let entrada =
            entrada.map_err(|e| format!("No se pudo leer {}: {e}", raiz.display()))?;
        let objetivo = destino.join(entrada.file_name());
        std::fs::rename(entrada.path(), &objetivo).map_err(|e| {
            format!(
                "No se pudo mover {} a {}: {e}",
                entrada.path().display(),
                objetivo.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests_completar_instalacion {
    use super::*;

    fn raiz_temporal(sufijo: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir()
            .join(format!("glass_hubfix_{}_{}", std::process::id(), sufijo));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn registrar_en_uso(root: &std::path::Path, version: &str) {
        std::fs::create_dir_all(engines_dir_in(root).join(version)).unwrap();
        let mut store = EngineStore::load_in(root);
        store.en_uso = Some(version.to_string());
        store.save_in(root).unwrap();
    }

    #[test]
    fn completar_instalacion_activa_la_version_en_disco() {
        let root = raiz_temporal("activa");
        registrar_en_uso(&root, "0.10.17.1-beta");
        let exe = motor_exe_in(&root, "0.10.18.2-beta");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // La carpeta adoptada íntegra trae managed/ y tools/.
        let dir = engines_dir_in(&root).join("0.10.18.2-beta");
        std::fs::create_dir_all(dir.join("managed")).unwrap();
        std::fs::create_dir_all(dir.join("tools")).unwrap();

        let resumen = completar_instalacion_en_disco(&root, "0.10.18.2-beta", &[]).unwrap();

        assert_eq!(resumen.version, "0.10.18.2-beta");
        let store = EngineStore::load_in(&root);
        assert_eq!(store.en_uso.as_deref(), Some("0.10.18.2-beta"));
        // La adopción nunca destruye la última versión sana: la
        // previa queda conservada y su carpeta intacta.
        assert_eq!(store.conservada.as_deref(), Some("0.10.17.1-beta"));
        assert!(engines_dir_in(&root).join("0.10.17.1-beta").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reconciliar_no_marca_la_instalacion_varada() {
        // La varada tiene ejecutable: es adoptable, no basura. La
        // carpeta de versión SIN ejecutable sí se reconcilia.
        let root = raiz_temporal("reconciliar");
        registrar_en_uso(&root, "0.10.17.1-beta");
        let exe = motor_exe_in(&root, "0.10.18.2-beta");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        std::fs::create_dir_all(engines_dir_in(&root).join("0.10.17.0-beta")).unwrap();

        let anadidas = reconciliar_huerfanas_en(&root);

        assert_eq!(anadidas, vec!["0.10.17.0-beta".to_string()]);
        let store = EngineStore::load_in(&root);
        assert!(!store.pendientes_borrar.iter().any(|v| v == "0.10.18.2-beta"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn procesar_no_borra_la_varada_aunque_este_en_pendientes() {
        // Pendientes heredada de un código anterior que sí la marcó:
        // siendo más nueva que la en uso y con ejecutable, sobrevive.
        let root = raiz_temporal("procesar");
        registrar_en_uso(&root, "0.10.17.1-beta");
        let exe = motor_exe_in(&root, "0.10.18.2-beta");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        let mut store = EngineStore::load_in(&root);
        store.pendientes_borrar.push("0.10.18.2-beta".to_string());
        store.save_in(&root).unwrap();

        let borradas = procesar_pendientes_borrar_en(&root);

        assert!(borradas.is_empty());
        assert!(engines_dir_in(&root).join("0.10.18.2-beta").exists());
        // Y sale de la lista: ya no es una retirada diferida.
        let store = EngineStore::load_in(&root);
        assert!(!store.pendientes_borrar.iter().any(|v| v == "0.10.18.2-beta"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn completar_carpeta_incompleta_es_error() {
        // Ejecutable presente pero sin managed/: carpeta dañada por
        // un borrado parcial. Adoptarla dejaría un motor que no
        // arranca y destruiría la previa: se rechaza.
        let root = raiz_temporal("incompleta");
        registrar_en_uso(&root, "0.10.17.1-beta");
        let exe = motor_exe_in(&root, "0.10.18.2-beta");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();

        assert!(completar_instalacion_en_disco(&root, "0.10.18.2-beta", &[]).is_err());
        let store = EngineStore::load_in(&root);
        assert_eq!(store.en_uso.as_deref(), Some("0.10.17.1-beta"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn registrar_sin_carpeta_en_disco_es_error() {
        // El registro nunca apunta a una carpeta inexistente.
        let root = raiz_temporal("registrar");
        registrar_en_uso(&root, "0.10.17.1-beta");

        assert!(registrar_motor_en_uso(&root, "0.10.18.2-beta", &[]).is_err());
        let store = EngineStore::load_in(&root);
        assert_eq!(store.en_uso.as_deref(), Some("0.10.17.1-beta"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn completar_sin_carpeta_es_error() {
        let root = raiz_temporal("sin_carpeta");
        registrar_en_uso(&root, "0.10.17.1-beta");

        let r = completar_instalacion_en_disco(&root, "0.10.18.2-beta", &[]);

        assert!(r.is_err());
        let store = EngineStore::load_in(&root);
        assert_eq!(store.en_uso.as_deref(), Some("0.10.17.1-beta"));
        assert!(engines_dir_in(&root).join("0.10.17.1-beta").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// Qué debe hacer el Hub tras aplicar su propia actualización (lo ejecuta main.rs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccionHub {
    /// Linux/AppImage (o ejecutable suelto): el archivo en `programa`
    /// ya está reemplazado; hay que lanzarlo con `--hub` y
    /// `--tras-actualizar-hub` y salir.
    Relanzar { programa: std::path::PathBuf },
    /// Windows: hay que ejecutar el instalador descargado y salir.
    LanzarInstalador { setup: std::path::PathBuf },
}

/// Ejecutable actual del Hub. Dentro de un AppImage `current_exe` apunta al
/// montaje temporal, no al archivo, así que manda `APPIMAGE` si está fijada.
fn ejecutable_hub() -> Result<std::path::PathBuf, String> {
    if let Some(a) = std::env::var_os("APPIMAGE") {
        if !a.is_empty() {
            return Ok(std::path::PathBuf::from(a));
        }
    }
    std::env::current_exe().map_err(|e| format!("No se pudo localizar el ejecutable del Hub: {e}"))
}

fn ruta_respaldo_hub(destino: &std::path::Path) -> std::path::PathBuf {
    let mut s = destino.as_os_str().to_os_string();
    s.push(".viejo");
    std::path::PathBuf::from(s)
}

/// Descarga el paquete del Hub de `release` para esta plataforma y verifica su
/// SHA-256. Solo se llama tras el clic del usuario en «Descargar e instalar».
pub fn descargar_actualizacion_hub(release: &ReleaseInfo) -> Result<std::path::PathBuf, String> {
    let asset = pick_hub_asset(release).ok_or_else(|| {
        format!("La versión {} del Hub no trae paquete para esta plataforma.", release.tag_name)
    })?;
    // El nombre viene de la API remota: solo se acepta el último componente.
    let nombre = std::path::Path::new(&asset.name)
        .file_name()
        .map(|n| n.to_os_string())
        .ok_or_else(|| format!("Nombre de paquete del Hub no válido: {}", asset.name))?;

    let esperado = expected_sha256(release, asset)?;
    let esperado = esperado.trim();
    if esperado.is_empty() {
        return Err(format!("La versión {} del Hub no publica SHA-256: no se instala.", release.tag_name));
    }

    let dir = data_root().join("hub-update");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    let ruta = dir.join(nombre);

    let bytes = get_bytes(&asset.browser_download_url)?;
    std::fs::write(&ruta, &bytes)
        .map_err(|e| format!("No se pudo guardar {}: {e}", ruta.display()))?;

    let calculado = sha256_hex(&bytes);
    if !calculado.eq_ignore_ascii_case(esperado) {
        let _ = std::fs::remove_file(&ruta);
        return Err(format!(
            "El SHA-256 del Hub descargado no coincide (esperado {esperado}, obtenido {calculado}). No se ha instalado nada."
        ));
    }
    Ok(ruta)
}

/// Aplica el paquete ya verificado. En Windows solo devuelve el instalador a
/// lanzar; en el resto reemplaza el ejecutable del Hub (nunca el del editor).
pub fn aplicar_actualizacion_hub(descargado: &std::path::Path) -> Result<AccionHub, String> {
    if !descargado.is_file() {
        return Err(format!("No existe el paquete descargado: {}", descargado.display()));
    }
    if cfg!(windows) {
        return Ok(AccionHub::LanzarInstalador { setup: descargado.to_path_buf() });
    }
    let destino = ejecutable_hub()?;
    aplicar_reemplazo_en(&destino, descargado)?;
    let _ = std::fs::remove_file(descargado);
    Ok(AccionHub::Relanzar { programa: destino })
}

/// Reemplazo seguro: `destino` → `destino.viejo`, copia del nuevo, 0o755.
/// Si la copia falla, intenta dejar el ejecutable anterior en su sitio.
pub fn aplicar_reemplazo_en(destino: &std::path::Path, descargado: &std::path::Path) -> Result<(), String> {
    let viejo = ruta_respaldo_hub(destino);
    if viejo.exists() {
        std::fs::remove_file(&viejo)
            .map_err(|e| format!("No se pudo borrar el respaldo anterior {}: {e}", viejo.display()))?;
    }
    std::fs::rename(destino, &viejo)
        .map_err(|e| format!("No se pudo apartar {}: {e}", destino.display()))?;

    if let Err(e) = copiar_ejecutable(descargado, destino) {
        let _ = std::fs::remove_file(destino);
        return Err(match std::fs::rename(&viejo, destino) {
            Ok(()) => format!("No se pudo instalar el Hub nuevo ({e}); se ha restaurado la versión anterior."),
            Err(e2) => format!(
                "No se pudo instalar el Hub nuevo ({e}) ni restaurar la anterior ({e2}); la anterior está en {}.",
                viejo.display()
            ),
        });
    }
    Ok(())
}

fn copiar_ejecutable(origen: &std::path::Path, destino: &std::path::Path) -> std::io::Result<()> {
    std::fs::copy(origen, destino)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(destino, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// Al arrancar el Hub: borra `<ejecutable>.viejo` si quedó de una actualización.
/// Nunca falla ni impide arrancar.
pub fn limpiar_respaldo_hub() {
    if let Ok(destino) = ejecutable_hub() {
        limpiar_respaldo_en(&destino);
    }
}

pub fn limpiar_respaldo_en(destino: &std::path::Path) {
    let viejo = ruta_respaldo_hub(destino);
    if viejo.is_file() {
        let _ = std::fs::remove_file(&viejo);
    }
}

#[cfg(test)]
mod tests {
    fn dir_prueba_hub(nombre: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("glass_hub_{nombre}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn aplicar_reemplazo_hub_deja_nuevo_y_respaldo() {
        let d = dir_prueba_hub("reemplazo");
        let destino = d.join("GlassEnginer-Hub.AppImage");
        let descargado = d.join("descargado.AppImage");
        std::fs::write(&destino, b"viejo").unwrap();
        std::fs::write(&descargado, b"nuevo").unwrap();

        super::aplicar_reemplazo_en(&destino, &descargado).unwrap();

        assert_eq!(std::fs::read(&destino).unwrap(), b"nuevo");
        assert_eq!(std::fs::read(super::ruta_respaldo_hub(&destino)).unwrap(), b"viejo");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let modo = std::fs::metadata(&destino).unwrap().permissions().mode();
            assert_eq!(modo & 0o777, 0o755);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn aplicar_reemplazo_hub_sustituye_respaldo_previo() {
        let d = dir_prueba_hub("respaldo_previo");
        let destino = d.join("hub");
        let descargado = d.join("hub-nuevo");
        std::fs::write(&destino, b"v2").unwrap();
        std::fs::write(super::ruta_respaldo_hub(&destino), b"v1").unwrap();
        std::fs::write(&descargado, b"v3").unwrap();

        super::aplicar_reemplazo_en(&destino, &descargado).unwrap();

        assert_eq!(std::fs::read(&destino).unwrap(), b"v3");
        assert_eq!(std::fs::read(super::ruta_respaldo_hub(&destino)).unwrap(), b"v2");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn limpiar_respaldo_hub_borra_y_tolera_ausencia() {
        let d = dir_prueba_hub("limpiar");
        let destino = d.join("hub");
        std::fs::write(&destino, b"actual").unwrap();
        let viejo = super::ruta_respaldo_hub(&destino);
        std::fs::write(&viejo, b"viejo").unwrap();

        super::limpiar_respaldo_en(&destino);
        assert!(!viejo.exists());
        assert!(destino.exists());

        // Sin respaldo: no debe fallar.
        super::limpiar_respaldo_en(&destino);
        assert!(destino.exists());
        let _ = std::fs::remove_dir_all(&d);
    }
    use super::*;

    /// ReleaseInfo con el tag dado (el helper `release_de_prueba` fija
    /// el tag a v0.10.18 y no sirve para probar los selectores).
    fn rel_con_tag(tag: &str, draft: bool) -> ReleaseInfo {
        ReleaseInfo {
            tag_name: tag.to_string(),
            draft,
            prerelease: true,
            name: "Release".to_string(),
            html_url: "https://example.com".to_string(),
            assets: Vec::new(),
        }
    }

    #[test]
    fn reconcilia_huerfanas_sin_borrar_y_es_idempotente() {
        let d = std::env::temp_dir().join(format!(
            "glass_s3_reconcilia_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(super::engines_dir_in(&d)).unwrap();

        for nombre in [
            "0.10.99",
            "0.10.98",
            "0.10.97",
            "0.10.96",
            "notas",
            ".staging-0.10.95",
        ] {
            std::fs::create_dir_all(super::engines_dir_in(&d).join(nombre)).unwrap();
        }
        std::fs::write(super::engines_dir_in(&d).join("0.10.94"), b"archivo").unwrap();

        let mut store = super::EngineStore::load_in(&d);
        store.en_uso = Some("0.10.99".to_string());
        store.conservada = Some("0.10.98".to_string());
        store.pendientes_borrar = vec!["0.10.97".to_string()];
        store.save_in(&d).unwrap();

        assert_eq!(
            super::reconciliar_huerfanas_en(&d),
            vec!["0.10.96".to_string()]
        );
        let recargado = super::EngineStore::load_in(&d);
        assert!(recargado
            .pendientes_borrar
            .contains(&"0.10.96".to_string()));
        assert!(super::reconciliar_huerfanas_en(&d).is_empty());

        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn procesar_nunca_borra_la_en_uso_ni_la_conservada() {
        // Escenario de la auditoría S10: una lista de pendientes vieja
        // nombra a la versión que volvió a estar en uso tras un
        // rollback; procesar debe purgarla, no borrarla.
        let d = std::env::temp_dir().join(format!(
            "glass_s10_protegidas_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        for v in ["0.10.99", "0.10.98", "0.10.97"] {
            std::fs::create_dir_all(engines_dir_in(&d).join(v)).unwrap();
        }
        let mut store = EngineStore::load_in(&d);
        store.en_uso = Some("0.10.99".to_string());
        store.conservada = Some("0.10.98".to_string());
        store.pendientes_borrar = vec![
            "0.10.99".to_string(),
            "0.10.98".to_string(),
            "0.10.97".to_string(),
        ];
        store.save_in(&d).unwrap();

        let borradas = procesar_pendientes_borrar_en(&d);
        assert_eq!(borradas, vec!["0.10.97".to_string()]);
        assert!(engines_dir_in(&d).join("0.10.99").is_dir());
        assert!(engines_dir_in(&d).join("0.10.98").is_dir());
        assert!(!engines_dir_in(&d).join("0.10.97").exists());
        assert!(EngineStore::load_in(&d).pendientes_borrar.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reconciliar_y_procesar_respetan_una_sesion_viva() {
        // Integración reconciliar→procesar (auditoría S10): la
        // huérfana con sesión viva pasa a pendiente pero NO se borra
        // hasta que la sesión muere.
        let d = std::env::temp_dir().join(format!(
            "glass_s10_sesion_viva_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(engines_dir_in(&d).join("0.10.96")).unwrap();
        let mut store = EngineStore::load_in(&d);
        store.en_uso = Some("0.10.99".to_string());
        store.save_in(&d).unwrap();

        let guarda = bloquear_sesion_en(&d, "0.10.96").unwrap();
        assert_eq!(
            reconciliar_huerfanas_en(&d),
            vec!["0.10.96".to_string()]
        );
        assert!(procesar_pendientes_borrar_en(&d).is_empty());
        assert!(engines_dir_in(&d).join("0.10.96").is_dir());
        assert!(EngineStore::load_in(&d)
            .pendientes_borrar
            .contains(&"0.10.96".to_string()));

        drop(guarda);
        assert_eq!(
            procesar_pendientes_borrar_en(&d),
            vec!["0.10.96".to_string()]
        );
        assert!(!engines_dir_in(&d).join("0.10.96").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reconciliar_sin_engines_devuelve_vacio_y_no_crea_nada() {
        let d = std::env::temp_dir().join(format!(
            "glass_s3_reconcilia_sin_engines_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();

        assert!(super::reconciliar_huerfanas_en(&d).is_empty());
        assert!(!super::engines_dir_in(&d).exists());

        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn los_selectores_no_se_contaminan_entre_motor_y_hub() {
        let releases = vec![
            rel_con_tag("v0.10.99", false),
            rel_con_tag("hub-v0.10.5", false),
            rel_con_tag("hub-v0.11.0", false),
            rel_con_tag("herramienta-3d-v1.0.1", false),
            rel_con_tag("hub-v0.12.0", true),
        ];

        assert_eq!(
            select_latest_hub_release(&releases).map(|r| r.tag_name),
            Some("hub-v0.11.0".to_string())
        );
        assert_eq!(
            select_latest_engine_release(&releases).map(|r| r.tag_name),
            Some("v0.10.99".to_string())
        );
    }

    #[test]
    fn is_hub_tag_reconoce_solo_los_tags_del_hub() {
        assert!(is_hub_tag("hub-v0.11.0"));
        assert!(is_hub_tag("hub-v0.11.0-beta"));
        assert!(!is_hub_tag("v0.11.0"));
        assert!(!is_hub_tag("herramienta-3d-v1.0.1"));
        assert!(!is_hub_tag("hub-vx"));
        assert!(!is_hub_tag("hub-0.11.0"));
    }

    #[test]
    fn hub_tag_version_desnuda_el_prefijo_y_falla_con_otros_tags() {
        assert!(hub_tag_version("hub-v0.11.0").is_ok());
        assert!(hub_tag_version("hub-v0.11.0-beta").is_ok());
        assert!(hub_tag_version("v0.11.0").is_err());
        assert!(hub_tag_version("hub-vx").is_err());
        assert_eq!(
            hub_tag_version("hub-v0.11.0").unwrap(),
            tag_version("0.11.0").unwrap()
        );
    }

    #[test]
    fn pick_hub_asset_elige_el_paquete_de_esta_plataforma() {
        let mut rel = rel_con_tag("hub-v0.11.0", false);
        rel.assets = vec![
            asset_de_prueba("GlassEnginer-Launcher-0.11.0-windows-x64-setup.exe".into()),
            asset_de_prueba("GlassEnginer-Launcher-0.11.0-linux-x64.AppImage".into()),
            asset_de_prueba("GlassEnginer-Engine-0.11.0-linux-x64.zip".into()),
            asset_de_prueba("checksums.txt".into()),
        ];
        let elegido = pick_hub_asset(&rel).map(|a| a.name.as_str());
        match platform_slug() {
            "linux-x64" => {
                assert_eq!(
                    elegido,
                    Some("GlassEnginer-Launcher-0.11.0-linux-x64.AppImage")
                )
            }
            "windows-x64" => {
                assert_eq!(
                    elegido,
                    Some("GlassEnginer-Launcher-0.11.0-windows-x64-setup.exe")
                )
            }
            _ => assert_eq!(elegido, None),
        }
    }

    fn rel(tag: &str) -> ReleaseInfo {
        ReleaseInfo {
            tag_name: tag.to_string(),
            draft: false,
            prerelease: false,
            name: String::new(),
            html_url: String::new(),
            assets: Vec::new(),
        }
    }

    fn rel_pre(tag: &str) -> ReleaseInfo {
        ReleaseInfo {
            prerelease: true,
            ..rel(tag)
        }
    }

    fn rel_draft(tag: &str) -> ReleaseInfo {
        ReleaseInfo {
            draft: true,
            ..rel(tag)
        }
    }

    fn tag_elegido(releases: &[ReleaseInfo]) -> Option<String> {
        select_latest_engine_release(releases).map(|r| r.tag_name)
    }

    #[test]
    fn seleccion_ignora_complementos_y_orden_por_version() {
        let lista = [
            rel("herramienta-3d-v1.0.1"),
            rel_pre("v0.10.14.1-beta"),
            rel_pre("v0.10.16.1-beta"),
            rel_pre("v0.10.15.1-beta"),
        ];
        assert_eq!(tag_elegido(&lista).as_deref(), Some("v0.10.16.1-beta"));
    }

    #[test]
    fn seleccion_incluye_prereleases_y_excluye_borradores() {
        let lista = [rel_pre("v0.10.16.1-beta"), rel_draft("v9.9.9")];
        assert_eq!(tag_elegido(&lista).as_deref(), Some("v0.10.16.1-beta"));
    }

    #[test]
    fn seleccion_solo_complementos_devuelve_none() {
        let lista = [rel("herramienta-3d-v1.0.1"), rel("herramienta-3d-v1.0.0")];
        assert_eq!(tag_elegido(&lista), None);
        assert_eq!(tag_elegido(&[]), None);
    }

    #[test]
    fn seleccion_respeta_el_orden_de_versionkey() {
        let lista = [rel("v0.10.16.1"), rel_pre("v0.10.16.1-beta")];
        assert_eq!(tag_elegido(&lista).as_deref(), Some("v0.10.16.1"));

        let lista = [rel_pre("v0.10.16.2-beta"), rel("v0.10.16.1")];
        assert_eq!(tag_elegido(&lista).as_deref(), Some("v0.10.16.2-beta"));
    }

    #[test]
    fn lista_json_de_github_parsea_y_selecciona() {
        let json = r#"[
            {
                "tag_name": "herramienta-3d-v1.0.1",
                "name": "MeshForge 1.0.1",
                "html_url": "https://github.com/x/y/releases/tag/herramienta-3d-v1.0.1",
                "draft": false,
                "prerelease": false,
                "assets": []
            },
            {
                "tag_name": "v0.10.15.1-beta",
                "name": "GlassEnginer 0.10.15.1-beta",
                "html_url": "https://github.com/x/y/releases/tag/v0.10.15.1-beta",
                "draft": false,
                "prerelease": true,
                "assets": []
            },
            {
                "tag_name": "v0.10.16.1-beta",
                "name": "GlassEnginer 0.10.16.1-beta",
                "html_url": "https://github.com/x/y/releases/tag/v0.10.16.1-beta",
                "draft": false,
                "prerelease": true,
                "assets": []
            }
        ]"#;
        let lista = serde_json::from_str::<Vec<ReleaseInfo>>(json).expect("JSON válido");
        assert_eq!(lista.len(), 3);
        assert!(!lista[0].prerelease);
        assert!(lista[1].prerelease && lista[2].prerelease);
        assert!(lista.iter().all(|r| !r.draft));

        let elegida = select_latest_engine_release(&lista).expect("hay release del motor");
        assert_eq!(elegida.tag_name, "v0.10.16.1-beta");
        assert_eq!(elegida.name, "GlassEnginer 0.10.16.1-beta");
    }

    #[test]
    fn tags_de_motor_y_de_complemento() {
        assert!(is_engine_tag("v0.10.16.1-beta"));
        assert!(!is_engine_tag("herramienta-3d-v1.0.1"));
        assert!(!is_engine_tag("v1.2"));
        assert!(!is_engine_tag(""));
    }

    #[test]
    fn versiones_y_novedad() {
        assert!(is_newer("v0.2.0", "0.1.0-alpha"));
        assert!(!is_newer("v0.1.0-alpha", "0.1.0-alpha"));
        assert!(is_newer("0.1.0", "0.1.0-alpha"));
        assert!(tag_version("v1.2.3").is_ok());
        assert!(is_newer("v0.10.14.1-beta", "0.10.0-beta"));
        assert!(is_newer("v0.10.14.1", "0.10.14"));
        assert!(is_newer("v0.10.14", "0.10.0"));
        assert!(!is_newer("v0.10.14.1-beta", "0.10.14.1-beta"));
        assert!(is_newer("v0.10.14.1", "0.10.14.1-beta"));
        assert!(tag_version("v0.10.14.1-beta").is_ok());
        assert!(tag_version("v1.2").is_err());
        assert!(tag_version("v1.2.3.4.5").is_err());
    }

    #[test]
    fn sha256_conocido() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn digest_con_y_sin_prefijo() {
        let upper = "AA".repeat(32);
        let asset = AssetInfo {
            name: "glass.zip".to_string(),
            size: 10,
            browser_download_url: "https://example.com/glass.zip".to_string(),
            digest: Some(format!("sha256:{}", upper)),
        };

        assert_eq!(digest_sha256(&asset), Some("aa".repeat(32)));

        let asset_sin_digest = AssetInfo {
            name: "glass.zip".to_string(),
            size: 10,
            browser_download_url: "https://example.com/glass.zip".to_string(),
            digest: None,
        };

        assert_eq!(digest_sha256(&asset_sin_digest), None);
    }

    #[test]
    fn checksums_parsea() {
        let text = "\
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA  otro.zip
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb *glass.zip
";

        assert_eq!(
            parse_checksums(text, "glass.zip"),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string())
        );
    }

    #[test]
    fn es_cambio_pesado_clasifica() {
        assert!(!es_cambio_pesado("0.10.17.2-beta", "0.10.18"));
        assert!(es_cambio_pesado("0.10.17.2-beta", "0.11.0"));
        assert!(es_cambio_pesado("0.10.17.2", "1.0.0"));
        assert!(es_cambio_pesado("basura", "0.10.18"));
    }

    #[test]
    fn plan_retirada_ligero_borra_anterior_y_mantiene_conservada() {
        let plan = plan_retirada(Some("0.10.17.2-beta"), Some("0.9.0"), "0.10.18");
        assert_eq!(
            plan,
            PlanRetirada {
                borrar: vec!["0.10.17.2-beta".to_string()],
                conservada: Some("0.9.0".to_string()),
            }
        );
    }

    #[test]
    fn plan_retirada_pesado_sin_conservada_previa() {
        let plan = plan_retirada(Some("0.10.18"), None, "0.11.0");
        assert_eq!(
            plan,
            PlanRetirada {
                borrar: Vec::new(),
                conservada: Some("0.10.18".to_string()),
            }
        );
    }

    #[test]
    fn plan_retirada_pesado_con_conservada_previa() {
        let plan = plan_retirada(Some("0.10.18"), Some("0.9.0"), "0.11.0");
        assert_eq!(
            plan,
            PlanRetirada {
                borrar: vec!["0.9.0".to_string()],
                conservada: Some("0.10.18".to_string()),
            }
        );
    }

    #[test]
    fn plan_retirada_reinstalar_en_uso_no_cambia_nada() {
        let plan = plan_retirada(Some("0.10.18"), Some("0.9.0"), "0.10.18");
        assert_eq!(
            plan,
            PlanRetirada {
                borrar: Vec::new(),
                conservada: Some("0.9.0".to_string()),
            }
        );
    }

    #[test]
    fn plan_retirada_primera_instalacion() {
        let plan = plan_retirada(None, None, "0.10.18");
        assert_eq!(
            plan,
            PlanRetirada {
                borrar: Vec::new(),
                conservada: None,
            }
        );
    }

    fn engine_store_dir_temporal(sufijo: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "glass_engine_store_{}_{}",
            sufijo,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn engine_store_roundtrip() {
        let root = engine_store_dir_temporal("roundtrip");
        let store = EngineStore {
            en_uso: Some("0.11.0".to_string()),
            conservada: Some("0.10.18".to_string()),
            pendientes_borrar: vec!["0.9.0".to_string()],
        };
        store.save_in(&root).expect("save_in");
        let cargado = EngineStore::load_in(&root);
        assert_eq!(cargado.en_uso, store.en_uso);
        assert_eq!(cargado.conservada, store.conservada);
        assert_eq!(cargado.pendientes_borrar, store.pendientes_borrar);
        assert!(!engines_store_path_in(&root).with_extension("tmp").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn engine_store_root_vacio_da_default() {
        let root = engine_store_dir_temporal("vacio");
        let cargado = EngineStore::load_in(&root);
        assert_eq!(cargado.en_uso, None);
        assert_eq!(cargado.conservada, None);
        assert!(cargado.pendientes_borrar.is_empty());
    }

    #[test]
    fn rutas_del_motor() {
        let root = std::path::Path::new("raiz");
        assert_eq!(engines_dir_in(root), root.join("engines"));
        let exe = if cfg!(windows) { "glass_app.exe" } else { "glass_app" };
        assert_eq!(
            motor_exe_in(root, "0.10.18"),
            root.join("engines").join("0.10.18").join(exe)
        );
    }

    fn asset_de_prueba(name: String) -> AssetInfo {
        AssetInfo {
            name,
            size: 10,
            browser_download_url: "https://example.com/x.zip".to_string(),
            digest: None,
        }
    }

    fn release_de_prueba(assets: Vec<AssetInfo>) -> ReleaseInfo {
        ReleaseInfo {
            tag_name: "v0.10.18".to_string(),
            draft: false,
            prerelease: false,
            name: "Release".to_string(),
            html_url: "https://example.com".to_string(),
            assets,
        }
    }

    fn otro_slug() -> &'static str {
        if platform_slug() == "windows-x64" {
            "linux-x64"
        } else {
            "windows-x64"
        }
    }

    #[test]
    fn pick_asset_un_solo_zip_de_otra_plataforma_devuelve_none() {
        // Regresion R1: el fallback de «un solo zip» instalaba el paquete
        // de otra plataforma.
        let release = release_de_prueba(vec![asset_de_prueba(format!(
            "GlassEnginer-Launcher-0.10.17.2-beta-{}.zip",
            otro_slug()
        ))]);
        assert!(pick_asset(&release).is_none());
    }

    #[test]
    fn pick_engine_asset_elige_el_del_motor() {
        let slug = platform_slug();
        let release = release_de_prueba(vec![
            asset_de_prueba(format!("GlassEnginer-Launcher-0.10.17.2-beta-{slug}.zip")),
            asset_de_prueba(format!("GlassEnginer-Engine-0.10.18-{slug}.zip")),
        ]);
        let picked = pick_engine_asset(&release).expect("debe elegir el paquete del motor");
        assert!(picked.name.contains("-Engine-"));
    }

    #[test]
    fn pick_engine_asset_sin_motor_devuelve_none() {
        let slug = platform_slug();
        let release = release_de_prueba(vec![asset_de_prueba(format!(
            "GlassEnginer-Launcher-0.10.17.2-beta-{slug}.zip"
        ))]);
        assert!(pick_engine_asset(&release).is_none());
    }

    #[test]
    fn pick_engine_asset_de_otra_plataforma_devuelve_none() {
        let release = release_de_prueba(vec![asset_de_prueba(format!(
            "GlassEnginer-Engine-0.10.18-{}.zip",
            otro_slug()
        ))]);
        assert!(pick_engine_asset(&release).is_none());
    }

    fn dir_prueba_s3(nombre: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("glass_s3_{}_{}", nombre, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn zip_con_entrada(dir: &std::path::Path, archivo: &str, entrada: &str) -> std::path::PathBuf {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let ruta = dir.join(archivo);
        let f = std::fs::File::create(&ruta).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        zw.start_file(entrada, SimpleFileOptions::default()).unwrap();
        zw.write_all(b"bin").unwrap();
        zw.finish().unwrap();
        ruta
    }

    fn zip_con_exe(dir: &std::path::Path) -> std::path::PathBuf {
        let exe = if cfg!(windows) { "glass_app.exe" } else { "glass_app" };
        zip_con_entrada(dir, "motor.zip", exe)
    }

    fn zip_sin_exe(dir: &std::path::Path) -> std::path::PathBuf {
        zip_con_entrada(dir, "sin_exe.zip", "leeme.txt")
    }

    fn sin_staging_residual(root: &std::path::Path) -> bool {
        std::fs::read_dir(engines_dir_in(root))
            .map(|rd| {
                rd.flatten()
                    .all(|e| !e.file_name().to_string_lossy().starts_with(".staging-"))
            })
            .unwrap_or(true)
    }

    #[test]
    fn borrar_conservada_ok() {
        let dir = dir_prueba_s3("borrar_conservada_ok");
        let root = dir.join("datos");
        std::fs::create_dir_all(engines_dir_in(&root).join("0.9.0")).unwrap();
        std::fs::write(engines_dir_in(&root).join("0.9.0").join("f"), b"x").unwrap();
        let mut store = EngineStore::load_in(&root);
        store.conservada = Some("0.9.0".to_string());
        store.save_in(&root).unwrap();
        let r = borrar_conservada_en(&root).unwrap();
        assert_eq!(r, Some("0.9.0".to_string()));
        assert!(!engines_dir_in(&root).join("0.9.0").exists());
        assert!(EngineStore::load_in(&root).conservada.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn borrar_conservada_sin_conservada() {
        let dir = dir_prueba_s3("borrar_conservada_sin_conservada");
        let root = dir.join("datos");
        let r = borrar_conservada_en(&root).unwrap();
        assert_eq!(r, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn borrar_conservada_con_sesion() {
        let dir = dir_prueba_s3("borrar_conservada_con_sesion");
        let root = dir.join("datos");
        std::fs::create_dir_all(engines_dir_in(&root).join("0.9.0")).unwrap();
        let mut store = EngineStore::load_in(&root);
        store.conservada = Some("0.9.0".to_string());
        store.save_in(&root).unwrap();
        let _lock = bloquear_sesion_en(&root, "0.9.0").unwrap();
        let r = borrar_conservada_en(&root);
        assert!(r.is_err());
        assert!(engines_dir_in(&root).join("0.9.0").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restos_y_borrado_versions() {
        let dir = dir_prueba_s3("restos_y_borrado_versions");
        let root = dir.join("datos");
        std::fs::create_dir_all(root.join("versions").join("a")).unwrap();
        std::fs::create_dir_all(root.join("versions").join("b")).unwrap();
        std::fs::write(root.join("versions").join("a").join("f"), b"x").unwrap();
        assert_eq!(restos_versions_en(&root), vec!["a".to_string(), "b".to_string()]);
        borrar_versions_en(&root).unwrap();
        assert!(!root.join("versions").exists());
        assert!(restos_versions_en(&root).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn objetivo_sonda_sin_motor_en_uso() {
        let dir = dir_prueba_s3("objetivo_sonda_sin_motor");
        let root = dir.join("datos");
        assert!(objetivo_sonda_en(&root).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn objetivo_sonda_en_uso_sin_exe() {
        let dir = dir_prueba_s3("objetivo_sonda_sin_exe");
        let root = dir.join("datos");
        let store = EngineStore {
            en_uso: Some("0.10.0".into()),
            ..Default::default()
        };
        store.save_in(&root).unwrap();
        assert!(objetivo_sonda_en(&root).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn objetivo_sonda_completo() {
        let dir = dir_prueba_s3("objetivo_sonda_completo");
        let root = dir.join("datos");
        let store = EngineStore {
            en_uso: Some("0.10.0".into()),
            ..Default::default()
        };
        store.save_in(&root).unwrap();
        let exe = motor_exe_in(&root, "0.10.0");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::File::create(&exe).unwrap();
        assert_eq!(objetivo_sonda_en(&root), Some((exe, "0.10.0".to_string())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sesion_activa_se_detecta_y_se_libera() {
        let dir = dir_prueba_s3("sesion");
        let root = dir.join("datos");
        assert!(!version_con_sesion_activa_en(&root, "0.10.0"));
        let lock = bloquear_sesion_en(&root, "0.10.0").unwrap();
        assert!(version_con_sesion_activa_en(&root, "0.10.0"));
        drop(lock);
        assert!(!version_con_sesion_activa_en(&root, "0.10.0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pendientes_se_borran_sin_sesion_y_esperan_con_sesion() {
        let dir = dir_prueba_s3("pendientes");
        let root = dir.join("datos");
        std::fs::create_dir_all(engines_dir_in(&root).join("0.9.0")).unwrap();
        std::fs::create_dir_all(engines_dir_in(&root).join("0.9.1")).unwrap();
        let mut store = EngineStore::load_in(&root);
        store.pendientes_borrar = vec!["0.9.0".to_string(), "0.9.1".to_string()];
        store.save_in(&root).unwrap();
        let _lock = bloquear_sesion_en(&root, "0.9.1").unwrap();
        let borradas = procesar_pendientes_borrar_en(&root);
        assert_eq!(borradas, vec!["0.9.0".to_string()]);
        assert!(!engines_dir_in(&root).join("0.9.0").exists());
        assert!(engines_dir_in(&root).join("0.9.1").exists());
        let store = EngineStore::load_in(&root);
        assert_eq!(store.pendientes_borrar, vec!["0.9.1".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn instalar_con_sesion_activa_difiere_la_retirada() {
        let dir = dir_prueba_s3("diferida");
        let root = dir.join("datos");
        let zip = zip_con_exe(&dir);
        instalar_motor_desde_zip(&root, &zip, "0.10.0", &[]).unwrap();
        let _lock = bloquear_sesion_en(&root, "0.10.0").unwrap();
        let resumen = instalar_motor_desde_zip(&root, &zip, "0.10.1", &[]).unwrap();
        assert_eq!(resumen.diferidas, vec!["0.10.0".to_string()]);
        assert!(resumen.retiradas.is_empty());
        assert!(engines_dir_in(&root).join("0.10.0").is_dir());
        let store = EngineStore::load_in(&root);
        assert!(store.pendientes_borrar.contains(&"0.10.0".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn instala_en_vacio_y_queda_en_uso() {
        let dir = dir_prueba_s3("vacio");
        let root = dir.join("datos");
        let zip = zip_con_exe(&dir);

        let r = instalar_motor_desde_zip(&root, &zip, "0.10.0", &[]).expect("instalacion ok");
        assert_eq!(r.version, "0.10.0");
        assert!(motor_exe_in(&root, "0.10.0").is_file());
        assert_eq!(EngineStore::load_in(&root).en_uso.as_deref(), Some("0.10.0"));
        assert!(sin_staging_residual(&root));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cambio_ligero_retira_la_anterior() {
        let dir = dir_prueba_s3("ligero");
        let root = dir.join("datos");
        let zip = zip_con_exe(&dir);

        instalar_motor_desde_zip(&root, &zip, "0.10.0", &[]).expect("primera ok");
        let r = instalar_motor_desde_zip(&root, &zip, "0.10.1", &[]).expect("segunda ok");
        assert!(!engines_dir_in(&root).join("0.10.0").exists());
        assert_eq!(r.retiradas, vec!["0.10.0".to_string()]);
        let s = EngineStore::load_in(&root);
        assert_eq!(s.en_uso.as_deref(), Some("0.10.1"));
        assert_eq!(s.conservada, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cambio_pesado_conserva_la_anterior() {
        let dir = dir_prueba_s3("pesado");
        let root = dir.join("datos");
        let zip = zip_con_exe(&dir);

        instalar_motor_desde_zip(&root, &zip, "0.10.9", &[]).expect("primera ok");
        let r = instalar_motor_desde_zip(&root, &zip, "0.11.0", &[]).expect("segunda ok");
        assert!(engines_dir_in(&root).join("0.10.9").is_dir());
        assert_eq!(r.conservada.as_deref(), Some("0.10.9"));
        assert!(r.retiradas.is_empty());
        assert_eq!(EngineStore::load_in(&root).conservada.as_deref(), Some("0.10.9"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reinstalar_la_conservada_la_pasa_a_en_uso() {
        let dir = dir_prueba_s3("conservada");
        let root = dir.join("datos");
        let zip = zip_con_exe(&dir);
        instalar_motor_desde_zip(&root, &zip, "0.10.9", &[]).expect("primera");
        instalar_motor_desde_zip(&root, &zip, "0.11.0", &[]).expect("pesada");
        // Estado: en uso 0.11.0, conservada 0.10.9. Reinstalar la
        // conservada la devuelve a en uso y conserva la otra: el plan
        // no borra ninguna de las dos carpetas.
        let r = instalar_motor_desde_zip(&root, &zip, "0.10.9", &[]).expect("reinstalacion");
        let store = EngineStore::load_in(&root);
        assert_eq!(store.en_uso.as_deref(), Some("0.10.9"));
        assert_eq!(store.conservada.as_deref(), Some("0.11.0"));
        assert_eq!(r.conservada.as_deref(), Some("0.11.0"));
        assert!(r.retiradas.is_empty());
        assert!(motor_exe_in(&root, "0.10.9").is_file());
        assert!(motor_exe_in(&root, "0.11.0").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn barrido_de_residuos_de_instalacion() {
        let dir = dir_prueba_s3("barrido");
        let root = dir.join("datos");
        let engines = engines_dir_in(&root);
        std::fs::create_dir_all(engines.join(".staging-0.9.0")).unwrap();
        std::fs::write(engines.join(".descarga-0.9.0-123.zip"), b"x").unwrap();
        std::fs::create_dir_all(engines.join("0.10.0")).unwrap();
        limpiar_residuos_instalacion(&root);
        assert!(!engines.join(".staging-0.9.0").exists());
        assert!(!engines.join(".descarga-0.9.0-123.zip").exists());
        assert!(engines.join("0.10.0").is_dir(), "una version real no se toca");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paquete_sin_ejecutable_no_instala_nada() {
        let dir = dir_prueba_s3("sin_exe");
        let root = dir.join("datos");
        let zip = zip_sin_exe(&dir);

        assert!(instalar_motor_desde_zip(&root, &zip, "0.10.0", &[]).is_err());
        assert!(!engines_dir_in(&root).join("0.10.0").exists());
        assert!(sin_staging_residual(&root));
        assert!(EngineStore::load_in(&root).en_uso.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sella_los_proyectos_al_instalar() {
        let dir = dir_prueba_s3("sellado");
        let root = dir.join("datos");
        let zip = zip_con_exe(&dir);

        let viejo = dir.join("proyecto_viejo");
        let mayor = dir.join("proyecto_mayor");
        std::fs::create_dir_all(&viejo).unwrap();
        std::fs::create_dir_all(&mayor).unwrap();
        std::fs::write(viejo.join("project.glass"), r#"{"name": "X", "engine": "0.9.0"}"#).unwrap();
        std::fs::write(mayor.join("project.glass"), r#"{"name": "X", "engine": "9.9.9"}"#).unwrap();

        let r = instalar_motor_desde_zip(&root, &zip, "0.10.0", &[viejo.clone(), mayor.clone()])
            .expect("instalacion ok");
        assert_eq!(r.proyectos_migrados, 1);

        let txt = std::fs::read_to_string(mayor.join("project.glass")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&txt).unwrap();
        assert_eq!(v["engine"].as_str(), Some("9.9.9"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn extraccion_conserva_el_bit_de_ejecucion() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default().unix_permissions(0o100755);
            zw.start_file("glass_app", opts).unwrap();
            zw.write_all(b"#!/bin/sh\necho hola\n").unwrap();
            zw.finish().unwrap();
        }
        let bytes = buf.into_inner();

        let dir = std::env::temp_dir().join(format!("glass_exec_bit_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        extract_zip_reader(std::io::Cursor::new(bytes), &dir).expect("extraccion ok");

        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(dir.join("glass_app")).unwrap();
        assert!(meta.permissions().mode() & 0o111 != 0, "falta el bit de ejecucion");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn version_de_sello_parsea_y_rechaza() {
        let con_version: serde_json::Value =
            serde_json::json!({"engine": "0.10.17.2-beta"});
        assert!(version_de_sello(&con_version).is_some());
        let sello_viejo: serde_json::Value =
            serde_json::json!({"engine": "GlassEngine 0.1.0"});
        assert!(version_de_sello(&sello_viejo).is_none());
        let sin_campo: serde_json::Value = serde_json::json!({"name": "X"});
        assert!(version_de_sello(&sin_campo).is_none());
    }

    #[test]
    fn sellar_proyecto_sube_y_no_rebaja() {
        let dir = std::env::temp_dir()
            .join(format!("glass_sellar_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("project.glass");

        std::fs::write(&ruta, r#"{"name": "X", "engine": "0.10.0"}"#).unwrap();
        assert!(sellar_proyecto(&dir, "0.10.17.2-beta").unwrap());
        let meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&ruta).unwrap()).unwrap();
        assert_eq!(
            meta.get("engine").and_then(|v| v.as_str()),
            Some("0.10.17.2-beta")
        );
        assert_eq!(meta.get("name").and_then(|v| v.as_str()), Some("X"));

        assert!(!sellar_proyecto(&dir, "0.9.0").unwrap());
        let meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&ruta).unwrap()).unwrap();
        assert_eq!(
            meta.get("engine").and_then(|v| v.as_str()),
            Some("0.10.17.2-beta")
        );

        std::fs::write(&ruta, r#"{"name": "Y", "engine": "GlassEngine 0.1.0"}"#).unwrap();
        assert!(sellar_proyecto(&dir, "0.1.0").unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pick_asset_prefiere_plataforma() {
        let bueno = format!("glass-{}.zip", platform_slug());
        let malo = "glass-otra-plataforma.zip".to_string();

        let release = ReleaseInfo {
            tag_name: "v0.2.0".to_string(),
            draft: false,
            prerelease: false,
            name: "Release".to_string(),
            html_url: "https://example.com".to_string(),
            assets: vec![
                AssetInfo {
                    name: bueno.clone(),
                    size: 10,
                    browser_download_url: "https://example.com/bueno.zip".to_string(),
                    digest: None,
                },
                AssetInfo {
                    name: malo,
                    size: 10,
                    browser_download_url: "https://example.com/malo.zip".to_string(),
                    digest: None,
                },
            ],
        };

        assert_eq!(pick_asset(&release).map(|a| a.name.as_str()), Some(bueno.as_str()));

        let unico = ReleaseInfo {
            tag_name: "v0.2.0".to_string(),
            draft: false,
            prerelease: false,
            name: "Release".to_string(),
            html_url: "https://example.com".to_string(),
            assets: vec![AssetInfo {
                name: "engine.zip".to_string(),
                size: 10,
                browser_download_url: "https://example.com/engine.zip".to_string(),
                digest: None,
            }],
        };

        // S0 (separación): sin fallback de «un solo zip» (defecto R1). Un
        // único zip sin el slug de la plataforma ya NO se elige: antes se
        // instalaba el paquete de otra plataforma.
        assert!(pick_asset(&unico).is_none());

        let vacio = ReleaseInfo {
            tag_name: "v0.2.0".to_string(),
            draft: false,
            prerelease: false,
            name: "Release".to_string(),
            html_url: "https://example.com".to_string(),
            assets: Vec::new(),
        };

        assert!(pick_asset(&vacio).is_none());
    }

    #[test]
    fn store_roundtrip_json() {
        let store = LauncherStore::default();
        assert_eq!(store.repo, DEFAULT_REPO);
        assert!(store.installations.is_empty());

        let text = serde_json::to_string(&store).unwrap();
        let back: LauncherStore = serde_json::from_str(&text).unwrap();
        assert_eq!(back.repo, DEFAULT_REPO);
        assert!(back.installations.is_empty());

        let sin_repo: LauncherStore = serde_json::from_str(r#"{"installations":[]}"#).unwrap();
        assert_eq!(sin_repo.repo, DEFAULT_REPO);
    }

    /// Servidor HTTP mínimo en localhost que responde `cuerpo` a la
    /// primera petición y termina. Devuelve (puerto, hilo).
    fn servidor_local(cuerpo: Vec<u8>) -> (u16, std::thread::JoinHandle<()>) {
        use std::io::{Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let puerto = listener.local_addr().unwrap().port();
        let hilo = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let mut leido = Vec::new();
            while !leido.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = sock.read(&mut buf).unwrap();
                assert!(n > 0, "el cliente cerró antes de pedir");
                leido.extend_from_slice(&buf[..n]);
            }
            let cabecera = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                cuerpo.len()
            );
            sock.write_all(cabecera.as_bytes()).unwrap();
            sock.write_all(&cuerpo).unwrap();
        });
        (puerto, hilo)
    }

    /// Regresión del defecto 8c: ureq capa la lectura del cuerpo a
    /// 10 MiB por defecto y el paquete del motor pesa ~26 MB, así que
    /// las lecturas en memoria deben llevar el límite explícito. Con
    /// el tope por defecto de ureq este test muere con «the response
    /// body is larger than request limit: 10485760».
    #[test]
    fn get_bytes_descarga_mas_de_10_mib() {
        let cuerpo = vec![0xABu8; 11 * 1024 * 1024];
        let esperado = cuerpo.len();
        let (puerto, servidor) = servidor_local(cuerpo);

        let datos = get_bytes(&format!("http://127.0.0.1:{puerto}/paquete.zip")).unwrap();
        servidor.join().unwrap();
        assert_eq!(datos.len(), esperado);
        assert!(datos.iter().all(|&b| b == 0xAB));
    }

    /// Regresión del defecto 8c por el camino REAL de instalación: la
    /// descarga de paquetes va en streaming a archivo (`get_to_file`)
    /// y su SHA-256 se calcula sobre el archivo (`sha256_hex_file`).
    /// Un cuerpo de 11 MiB —más que el tope de ureq— debe llegar
    /// entero y con el hash correcto.
    #[test]
    fn get_to_file_streaming_mas_de_10_mib_y_hash() {
        let cuerpo: Vec<u8> = (0..11 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let hash_esperado = sha256_hex(&cuerpo);
        let (puerto, servidor) = servidor_local(cuerpo);

        let tmp = std::env::temp_dir().join(format!(
            "glass_test_stream_{}.zip",
            std::process::id()
        ));
        let n = get_to_file(
            &format!("http://127.0.0.1:{puerto}/paquete.zip"),
            &tmp,
        )
        .unwrap();
        servidor.join().unwrap();
        assert_eq!(n, 11 * 1024 * 1024);
        assert_eq!(sha256_hex_file(&tmp).unwrap(), hash_esperado);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn extrae_zip_en_memoria() {
        use std::io::Write as _;
        let mut cur = std::io::Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut cur);
            let opts = zip::write::SimpleFileOptions::default();
            zw.start_file("glass_app", opts).unwrap();
            zw.write_all(b"BIN").unwrap();
            zw.start_file("assets/version.txt", opts).unwrap();
            zw.write_all(b"9.9.9").unwrap();
            zw.finish().unwrap();
        }
        let data = cur.into_inner();
        let dir = std::env::temp_dir().join(format!("glass_test_{}", std::process::id()));
        let n = extract_zip(&data, &dir).unwrap();
        assert_eq!(n, 2);
        assert_eq!(std::fs::read(dir.join("glass_app")).unwrap(), b"BIN");
        assert_eq!(std::fs::read(dir.join("assets/version.txt")).unwrap(), b"9.9.9");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registrar_proyecto_no_duplica_y_actualiza() {
        let dir = std::env::temp_dir().join(format!(
            "registrar_proyecto_no_duplica_y_actualiza_{}",
            std::process::id()
        ));
        let mut store = LauncherStore::default();
        store.register_project("Primero", &dir);
        store.register_project("Segundo", &dir);
        assert_eq!(store.projects.len(), 1);
        assert_eq!(store.projects[0].name, "Segundo");
        assert!(store.projects[0].last_opened_unix > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registrar_dos_proyectos_distintos() {
        let dir = std::env::temp_dir().join(format!(
            "registrar_dos_proyectos_distintos_{}",
            std::process::id()
        ));
        let mut store = LauncherStore::default();
        store.register_project("A", &dir.join("a"));
        store.register_project("B", &dir.join("b"));
        assert_eq!(store.projects.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// Directorio temporal único por proceso y por test: los tests corren en
    /// paralelo dentro del mismo proceso, así que el pid solo no basta.
    fn dir_temporal_blender_test(nombre: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "glass-blender-test-{}-{nombre}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn blender_portable_paquete_coherente() {
        if let Some((url, nombre, sha)) = super::paquete_blender_portable() {
            assert_eq!(sha.len(), 64);
            assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(url.ends_with(nombre));
            assert!(url.starts_with("https://download.blender.org/"));
            assert!(url.contains(super::BLENDER_PORTABLE_VERSION));
            assert!(nombre.contains(super::BLENDER_PORTABLE_VERSION));
        }
    }

    #[test]
    fn blender_portable_rutas() {
        let motor = std::path::Path::new("motor");
        let dir = super::dir_blender_portable(motor);
        assert_eq!(dir, motor.join("tools").join("blender"));
        let exe = if cfg!(windows) { "blender.exe" } else { "blender" };
        assert_eq!(super::ruta_blender_portable(motor), dir.join(exe));
    }

    #[test]
    fn blender_portable_aplanar_extraccion() {
        let base = dir_temporal_blender_test("aplanar");
        let extraccion = base.join("extraccion");
        let raiz = extraccion.join("blender-4.2.3-test-x64");
        let interior = raiz.join("4.2").join("datafiles");
        std::fs::create_dir_all(&interior).unwrap();
        let exe = super::nombre_ejecutable_blender();
        std::fs::write(raiz.join(exe), b"ejecutable").unwrap();
        std::fs::write(interior.join("ejemplo.txt"), b"contenido").unwrap();

        // Un portable anterior debe sustituirse por completo.
        let destino = base.join("destino");
        std::fs::create_dir_all(&destino).unwrap();
        std::fs::write(destino.join("viejo.txt"), b"viejo").unwrap();

        super::aplanar_extraccion_blender(&extraccion, &destino).unwrap();

        assert!(destino.join(exe).is_file());
        assert_eq!(
            std::fs::read(destino.join("4.2").join("datafiles").join("ejemplo.txt")).unwrap(),
            b"contenido"
        );
        assert!(!destino.join("viejo.txt").exists());
        assert!(!destino.join("blender-4.2.3-test-x64").exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn blender_portable_aplanar_sin_raiz_falla_y_conserva_destino() {
        let base = dir_temporal_blender_test("sin-raiz");
        let extraccion = base.join("extraccion");
        std::fs::create_dir_all(extraccion.join("otra-carpeta")).unwrap();
        let destino = base.join("destino");
        std::fs::create_dir_all(&destino).unwrap();
        std::fs::write(destino.join("previo.txt"), b"previo").unwrap();

        assert!(super::aplanar_extraccion_blender(&extraccion, &destino).is_err());
        assert!(destino.join("previo.txt").is_file());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn blender_portable_instalado_detecta_ejecutable() {
        let motor = dir_temporal_blender_test("instalado");
        assert!(!super::blender_portable_instalado(&motor));

        std::fs::create_dir_all(super::dir_blender_portable(&motor)).unwrap();
        std::fs::write(super::ruta_blender_portable(&motor), b"ejecutable").unwrap();
        assert!(super::blender_portable_instalado(&motor));

        let _ = std::fs::remove_dir_all(&motor);
    }
}

#[cfg(test)]
mod tests_banderas_hub {
    use super::*;

    struct RaizTemporal {
        ruta: PathBuf,
    }

    impl RaizTemporal {
        fn nueva(nombre: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};

            static CONTADOR: AtomicU64 = AtomicU64::new(0);
            let ruta = std::env::temp_dir().join(format!(
                "glass-launcher-{nombre}-{}-{}",
                std::process::id(),
                CONTADOR.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&ruta).unwrap();
            Self { ruta }
        }
    }

    impl Drop for RaizTemporal {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.ruta);
        }
    }

    #[test]
    fn solicitud_de_cierre_escribe_la_bandera_del_pid() {
        let raiz = RaizTemporal::nueva("cierre");
        let pid = 123;
        let ruta = ruta_solicitud_cierre_en(&raiz.ruta, pid);

        assert!(!ruta.exists());
        std::fs::write(&ruta, pid.to_string()).unwrap();
        assert!(ruta.is_file());
    }

    #[test]
    fn solicitud_de_cierre_propia_se_escribe_y_se_retira() {
        retirar_solicitud_cierre_propia();
        assert!(!solicitud_cierre_propia());

        assert!(solicitar_cierre_motor(std::process::id()));
        assert!(solicitud_cierre_propia());

        retirar_solicitud_cierre_propia();
        assert!(!solicitud_cierre_propia());
    }

    #[test]
    fn despertar_hub_se_consume_una_sola_vez() {
        let raiz = RaizTemporal::nueva("despertar");

        assert!(!consumir_despertar_hub_en(&raiz.ruta));
        std::fs::write(ruta_despertar_hub_en(&raiz.ruta), "").unwrap();
        assert!(consumir_despertar_hub_en(&raiz.ruta));
        assert!(!consumir_despertar_hub_en(&raiz.ruta));
    }

    #[test]
    fn bloqueo_hub_es_exclusivo_y_se_libera_al_soltar_la_guarda() {
        let raiz = RaizTemporal::nueva("bloqueo-hub");

        let bloqueo = tomar_bloqueo_hub_en(&raiz.ruta);
        assert!(bloqueo.is_some());
        assert!(tomar_bloqueo_hub_en(&raiz.ruta).is_none());

        drop(bloqueo);
        assert!(tomar_bloqueo_hub_en(&raiz.ruta).is_some());
    }

    #[test]
    fn rutas_de_banderas_y_bloqueo_usan_los_nombres_exactos() {
        let raiz = Path::new("raiz");

        assert_eq!(
            ruta_solicitud_cierre_en(raiz, 123),
            raiz.join("cerrar-123.flag")
        );
        assert_eq!(ruta_despertar_hub_en(raiz), raiz.join("hub-despierta.flag"));
        assert_eq!(ruta_bloqueo_hub_en(raiz), raiz.join("hub.lock"));
    }
}
