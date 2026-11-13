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

pub const ENGINE_VERSION: &str = "0.1.0-alpha";
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LauncherStore {
    #[serde(default = "default_repo")]
    pub repo: String,
    #[serde(default)]
    pub installations: Vec<Installation>,
}

fn default_repo() -> String {
    DEFAULT_REPO.to_string()
}

impl Default for LauncherStore {
    fn default() -> Self {
        Self {
            repo: default_repo(),
            installations: Vec::new(),
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
        std::fs::write(store_path(), text).map_err(|e| e.to_string())
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn tag_version(tag: &str) -> Result<semver::Version, String> {
    let raw = tag.strip_prefix('v').unwrap_or(tag);
    semver::Version::parse(raw).map_err(|e| e.to_string())
}

pub fn is_newer(tag: &str, current: &str) -> bool {
    let release = match tag_version(tag) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let current_raw = current.strip_prefix('v').unwrap_or(current);
    let current_version = match semver::Version::parse(current_raw) {
        Ok(v) => v,
        Err(_) => return false,
    };
    release > current_version
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

    if candidates.len() == 1 {
        return candidates.first().copied();
    }

    None
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

fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(300)))
        .build()
        .into();
    let mut resp = match agent
        .get(url)
        .header("User-Agent", "GlassEnginer-Launcher")
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => {
            return Err("HTTP 404: el recurso no existe o no es público.".to_string());
        }
        Err(ureq::Error::StatusCode(n)) => return Err(format!("HTTP {n}")),
        Err(e) => return Err(e.to_string()),
    };
    resp.body_mut().read_to_vec().map_err(|e| e.to_string())
}

pub fn fetch_latest(api_base: &str, repo: &str) -> Result<ReleaseInfo, String> {
    let url = format!("{api_base}/repos/{repo}/releases/latest");
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
        .read_to_string()
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
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
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;

    let mut ar = zip::ZipArchive::new(std::io::Cursor::new(data.to_vec()))
        .map_err(|e| e.to_string())?;

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

    let data = get_bytes(&asset.browser_download_url)?;
    let esperado = expected_sha256(release, asset)?;

    if sha256_hex(&data) != esperado {
        return Err("El SHA-256 no coincide: la descarga no es íntegra o no es el archivo publicado. No se instaló nada.".to_string());
    }

    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        .to_string();

    let destino = versions_dir().join(&version);

    if destino.exists() {
        std::fs::remove_dir_all(&destino).map_err(|e| e.to_string())?;
    }

    extract_zip(&data, &destino)?;

    let installation = Installation {
        version: version.clone(),
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

    #[test]
    fn versiones_y_novedad() {
        assert!(is_newer("v0.2.0", "0.1.0-alpha"));
        assert!(!is_newer("v0.1.0-alpha", "0.1.0-alpha"));
        assert!(is_newer("0.1.0", "0.1.0-alpha"));
        assert!(tag_version("v1.2.3").is_ok());
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
    fn pick_asset_prefiere_plataforma() {
        let bueno = format!("glass-{}.zip", platform_slug());
        let malo = "glass-otra-plataforma.zip".to_string();

        let release = ReleaseInfo {
            tag_name: "v0.2.0".to_string(),
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
            name: "Release".to_string(),
            html_url: "https://example.com".to_string(),
            assets: vec![AssetInfo {
                name: "engine.zip".to_string(),
                size: 10,
                browser_download_url: "https://example.com/engine.zip".to_string(),
                digest: None,
            }],
        };

        assert_eq!(pick_asset(&unico).map(|a| a.name.as_str()), Some("engine.zip"));

        let vacio = ReleaseInfo {
            tag_name: "v0.2.0".to_string(),
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
}
