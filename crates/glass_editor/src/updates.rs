//! Actualizaciones del Launcher desde GitHub Releases.
//!
//! El Launcher consulta las releases públicas de GitHub, compara versiones
//! semánticas, selecciona el activo correspondiente a la plataforma actual,
//! descarga el paquete y verifica obligatoriamente su SHA-256 antes de
//! instalarlo en la carpeta de versiones y registrarlo en el store local.
//!
//! No se usan tokens: solo se soportan repositorios públicos. Un error 404
//! debe interpretarse como repositorio privado o inexistente.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::Digest;

pub const ENGINE_VERSION: &str = "0.10.17.2-beta";
/// Identidad propia del Hub (la cáscara fija). Solo cambia cuando cambia
/// el Hub; hoy coincide con la del motor.
pub const HUB_VERSION: &str = "0.10.17.2-beta";
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

pub fn check_update(api_base: &str, repo: &str) -> Result<UpdateCheck, String> {
    let release = fetch_latest(api_base, repo)?;
    if is_newer(&release.tag_name, ENGINE_VERSION) {
        Ok(UpdateCheck::Available(release))
    } else {
        Ok(UpdateCheck::UpToDate)
    }
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

pub fn install_release(
    release: &ReleaseInfo,
    store: &mut LauncherStore,
) -> Result<Installation, String> {
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

    // Descarga en streaming a un archivo temporal (defecto 8c): el
    // paquete (~26 MB) se copia por trozos a disco, sin cargarlo en
    // RAM y sin el tope de 10 MiB de `read_to_vec`. El SHA-256 se
    // verifica sobre el archivo ANTES de extraer: si no coincide, no
    // se instala nada. El temporal se borra siempre al salir.
    let tmp = std::env::temp_dir().join(format!(
        "glassenginer_update_{}_{}.zip",
        version,
        std::process::id()
    ));
    let instalacion = descargar_e_instalar(release, asset, &version, &tmp, store);
    let _ = std::fs::remove_file(&tmp);
    instalacion
}

fn descargar_e_instalar(
    release: &ReleaseInfo,
    asset: &AssetInfo,
    version: &str,
    tmp: &std::path::Path,
    store: &mut LauncherStore,
) -> Result<Installation, String> {
    get_to_file(&asset.browser_download_url, tmp)?;
    let esperado = expected_sha256(release, asset)?;

    if sha256_hex_file(tmp)? != esperado {
        return Err("El SHA-256 no coincide: la descarga no es íntegra o no es el archivo publicado. No se instaló nada.".to_string());
    }

    let destino = versions_dir().join(version);

    if destino.exists() {
        std::fs::remove_dir_all(&destino).map_err(|e| e.to_string())?;
    }

    extract_zip_file(tmp, &destino)?;

    let installation = Installation {
        version: version.to_string(),
        path: destino.to_string_lossy().to_string(),
        installed_unix: now_unix(),
    };

    store.installations.retain(|i| i.version != version);
    store.installations.push(installation.clone());
    store.save()?;

    Ok(installation)
}

pub fn is_installed(store: &LauncherStore, tag: &str) -> bool {
    let version = tag.strip_prefix('v').unwrap_or(tag);
    store.installations.iter().any(|i| i.version == version)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
