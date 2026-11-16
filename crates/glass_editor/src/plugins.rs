//! Complementos del editor: catálogo remoto, instalación verificada por SHA-256 y
//! activación/desactivación sin reinstalar.
//!
//! El código de cada complemento viaja dentro del binario del motor (extensión
//! integrada). Este módulo gestiona el paquete descargado (manifiesto, declaración
//! del módulo y datos) y la puerta de activación `plugin_enabled`, que el editor
//! consulta al arrancar.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::updates::{
    data_root, extract_zip, get_bytes, is_newer, now_unix, sha256_hex, ENGINE_VERSION,
};

/// Nombre del manifiesto dentro de cada paquete.
const MANIFEST_FILE: &str = "plugin.json";
/// Registro local de complementos instalados.
const STORE_FILE: &str = "plugins.json";
/// Subcarpeta de la raíz de datos donde se instalan los complementos.
const PLUGINS_SUBDIR: &str = "plugins";

/// Manifiesto de un complemento, tal como va en el catálogo y dentro del zip (plugin.json).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub min_engine: Option<String>,
    #[serde(default)]
    pub max_engine: Option<String>,
    pub sha256: String,
    pub url: String,
    pub file_name: String,
}

/// Catálogo oficial de complementos.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCatalog {
    #[serde(default)]
    pub plugins: Vec<PluginManifest>,
}

/// Registro de un complemento instalado en disco.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInstall {
    pub id: String,
    pub version: String,
    pub enabled: bool,
    pub path: String,
    pub installed_unix: u64,
}

/// Registro local de complementos instalados (plugins.json).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginStore {
    #[serde(default)]
    pub installs: Vec<PluginInstall>,
}

/// Carpeta donde se instalan los complementos: data_root()/plugins.
pub fn plugins_dir() -> PathBuf {
    plugins_dir_in(&data_root())
}

/// Carpeta de un complemento concreto: data_root()/plugins/<id>.
pub fn plugin_dir(id: &str) -> PathBuf {
    plugins_dir().join(id)
}

/// Ruta del registro de complementos: data_root()/plugins.json.
pub fn store_path() -> PathBuf {
    store_path_in(&data_root())
}

fn plugins_dir_in(root: &Path) -> PathBuf {
    root.join(PLUGINS_SUBDIR)
}

fn store_path_in(root: &Path) -> PathBuf {
    root.join(STORE_FILE)
}

impl PluginStore {
    /// Carga el registro desde la ruta real. Tolerante: si falta o está dañado, vacío.
    pub fn load() -> Self {
        Self::load_from(&store_path())
    }

    /// Carga el registro desde `path`. Tolerante: si falta o está dañado, vacío.
    pub fn load_from(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Guarda el registro en la ruta real (crea la carpeta de datos si hace falta).
    pub fn save(&self) -> Result<(), String> {
        self.save_to(&store_path())
    }

    /// Guarda el registro en `path` (crea la carpeta padre si hace falta).
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("No se pudo crear {}: {e}", parent.display()))?;
            }
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("No se pudo serializar el registro de complementos: {e}"))?;
        fs::write(path, json).map_err(|e| format!("No se pudo escribir {}: {e}", path.display()))
    }

    /// Busca el registro de un complemento por id.
    pub fn find(&self, id: &str) -> Option<&PluginInstall> {
        self.installs.iter().find(|i| i.id == id)
    }

    /// Activa o desactiva un complemento instalado. Devuelve false si no está instalado.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        match self.installs.iter_mut().find(|i| i.id == id) {
            Some(install) => {
                install.enabled = enabled;
                true
            }
            None => false,
        }
    }

    /// Inserta el registro o sustituye el existente con el mismo id.
    fn upsert(&mut self, install: PluginInstall) {
        match self.installs.iter_mut().find(|i| i.id == install.id) {
            Some(slot) => *slot = install,
            None => self.installs.push(install),
        }
    }
}

/// URL del catálogo: $GLASS_PLUGINS_CATALOG si no está vacía; si no, la del repo en GitHub.
pub fn catalog_url(repo: &str) -> String {
    std::env::var("GLASS_PLUGINS_CATALOG")
        .ok()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| format!("https://raw.githubusercontent.com/{repo}/main/plugins/catalog.json"))
}

/// Descarga y parsea el catálogo oficial de complementos.
pub fn fetch_catalog(repo: &str) -> Result<PluginCatalog, String> {
    let url = catalog_url(repo);
    let bytes = get_bytes(&url)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| format!("El catálogo de complementos ({url}) no es un JSON válido: {e}"))
}

/// Requisito de motor que no se cumple (p. ej. ">= 0.11.0"), o None si es compatible.
pub fn engine_incompatibility(m: &PluginManifest) -> Option<String> {
    if let Some(min) = m.min_engine.as_deref() {
        // El motor es más viejo que el mínimo.
        if is_newer(min, ENGINE_VERSION) {
            return Some(format!(">= {min}"));
        }
    }
    if let Some(max) = m.max_engine.as_deref() {
        // El motor es más nuevo que el máximo.
        if is_newer(ENGINE_VERSION, max) {
            return Some(format!("<= {max}"));
        }
    }
    None
}

/// true si la versión del motor está dentro de [min_engine, max_engine].
pub fn engine_compatible(m: &PluginManifest) -> bool {
    engine_incompatibility(m).is_none()
}

/// true si el complemento está instalado y el catálogo ofrece una versión más nueva.
pub fn update_available(m: &PluginManifest, store: &PluginStore) -> bool {
    store
        .find(&m.id)
        .is_some_and(|installed| is_newer(&m.version, &installed.version))
}

/// Descarga, verifica e instala un complemento en data_root()/plugins/<id>.
pub fn install_plugin(m: &PluginManifest) -> Result<PluginInstall, String> {
    // Comprobaciones baratas antes de descargar nada.
    validate_id(&m.id)?;
    check_engine(m)?;
    let bytes = get_bytes(&m.url)?;
    install_plugin_from_bytes(m, &bytes, &data_root())
}

/// Núcleo de la instalación sin red: verifica `bytes` e instala en `root`/plugins/<id>,
/// registrando el complemento en `root`/plugins.json.
pub fn install_plugin_from_bytes(
    m: &PluginManifest,
    bytes: &[u8],
    root: &Path,
) -> Result<PluginInstall, String> {
    validate_id(&m.id)?;
    check_engine(m)?;

    // El hash se comprueba ANTES de tocar el disco.
    let expected = m.sha256.trim().to_ascii_lowercase();
    let got = sha256_hex(bytes);
    if got != expected {
        return Err(format!(
            "El SHA-256 de {} no coincide con el del catálogo (esperado {expected}, obtenido {got}). No se ha instalado nada.",
            m.file_name
        ));
    }

    // Se extrae a una carpeta provisional y solo se sustituye la instalación anterior
    // cuando el paquete es válido. Los id no pueden empezar por '.', así que no colisiona.
    let base = plugins_dir_in(root);
    let staging = base.join(format!(".{}.part", m.id));
    let dir = base.join(&m.id);

    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|e| format!("No se pudo limpiar {}: {e}", staging.display()))?;
    }
    fs::create_dir_all(&staging)
        .map_err(|e| format!("No se pudo crear {}: {e}", staging.display()))?;

    if let Err(e) = extract_and_check(m, bytes, &staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }

    if dir.exists() {
        if let Err(e) = fs::remove_dir_all(&dir) {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("No se pudo borrar la versión anterior en {}: {e}", dir.display()));
        }
    }
    if let Err(e) = fs::rename(&staging, &dir) {
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("No se pudo mover el complemento a {}: {e}", dir.display()));
    }

    let install = PluginInstall {
        id: m.id.clone(),
        version: m.version.clone(),
        enabled: true,
        path: dir.to_string_lossy().into_owned(),
        installed_unix: now_unix(),
    };
    let store_file = store_path_in(root);
    let mut store = PluginStore::load_from(&store_file);
    store.upsert(install.clone());
    store.save_to(&store_file)?;
    Ok(install)
}

/// Puerta de activación: true solo si el complemento está instalado y activado.
pub fn plugin_enabled(id: &str) -> bool {
    PluginStore::load().find(id).is_some_and(|i| i.enabled)
}

/// El id viene de la red y se usa como nombre de carpeta: solo [A-Za-z0-9._-], sin '.' inicial.
fn validate_id(id: &str) -> Result<(), String> {
    let valid = !id.is_empty()
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        Err(format!("Identificador de complemento no válido: «{id}»."))
    }
}

fn check_engine(m: &PluginManifest) -> Result<(), String> {
    match engine_incompatibility(m) {
        Some(req) => Err(format!(
            "El complemento {} requiere motor {req}; este es {ENGINE_VERSION}.",
            m.name
        )),
        None => Ok(()),
    }
}

/// Extrae el paquete en `dest` y comprueba que su plugin.json coincide con el catálogo.
fn extract_and_check(m: &PluginManifest, bytes: &[u8], dest: &Path) -> Result<(), String> {
    extract_zip(bytes, dest)?;
    let manifest_path = dest.join(MANIFEST_FILE);
    let raw = fs::read(&manifest_path)
        .map_err(|_| format!("El paquete de {} no contiene {MANIFEST_FILE}.", m.name))?;
    let inner: PluginManifest = serde_json::from_slice(&raw)
        .map_err(|e| format!("El {MANIFEST_FILE} de {} no es válido: {e}", m.name))?;
    if inner.id != m.id || inner.version != m.version {
        return Err(format!(
            "El {MANIFEST_FILE} del paquete declara {} v{}, pero el catálogo anuncia {} v{}.",
            inner.id, inner.version, m.id, m.version
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    /// Carpeta temporal única por proceso y por test.
    fn temp_root(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glass_plugins_{}_{suffix}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn manifest(id: &str, version: &str) -> PluginManifest {
        PluginManifest {
            id: id.to_owned(),
            name: format!("Complemento {id}"),
            version: version.to_owned(),
            description: String::new(),
            min_engine: None,
            max_engine: None,
            sha256: String::new(),
            url: format!("https://example.invalid/{id}.zip"),
            file_name: format!("{id}-{version}.zip"),
        }
    }

    fn build_zip(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    /// Paquete con plugin.json (id/versión interiores) y un archivo extra.
    fn package(inner_id: &str, version: &str) -> Vec<u8> {
        let inner = serde_json::to_vec_pretty(&manifest(inner_id, version)).unwrap();
        build_zip(&[
            (MANIFEST_FILE, inner),
            ("modulo.json", br#"{"modulo":"herramienta-3d"}"#.to_vec()),
        ])
    }

    fn install(id: &str, version: &str, enabled: bool) -> PluginInstall {
        PluginInstall {
            id: id.to_owned(),
            version: version.to_owned(),
            enabled,
            path: format!("/tmp/{id}"),
            installed_unix: 1,
        }
    }

    #[test]
    fn store_roundtrip_set_enabled_y_find() {
        let root = temp_root("store");
        let path = store_path_in(&root);
        let mut store = PluginStore::default();
        store.upsert(install("a", "1.0.0", true));
        store.upsert(install("b", "2.0.0", false));
        store.save_to(&path).unwrap();

        let mut loaded = PluginStore::load_from(&path);
        assert_eq!(loaded, store);
        assert!(loaded.find("a").is_some_and(|i| i.enabled));
        assert!(loaded.find("b").is_some_and(|i| !i.enabled));
        assert!(loaded.find("c").is_none());

        assert!(loaded.set_enabled("a", false));
        assert!(!loaded.set_enabled("c", true));
        loaded.save_to(&path).unwrap();
        assert!(PluginStore::load_from(&path).find("a").is_some_and(|i| !i.enabled));

        // Tolerante: archivo inexistente o dañado → vacío.
        assert_eq!(PluginStore::load_from(&root.join("no_existe.json")), PluginStore::default());
        fs::write(&path, b"{ roto").unwrap();
        assert_eq!(PluginStore::load_from(&path), PluginStore::default());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compatibilidad_con_el_motor() {
        let mut m = manifest("x", "1.0.0");
        assert!(engine_compatible(&m));

        m.min_engine = Some("0.10.0".to_owned());
        assert!(engine_compatible(&m));
        m.min_engine = Some("99.0.0".to_owned());
        assert!(!engine_compatible(&m));
        assert_eq!(engine_incompatibility(&m).as_deref(), Some(">= 99.0.0"));

        m.min_engine = None;
        m.max_engine = Some("0.1.0".to_owned());
        assert!(!engine_compatible(&m));
        assert_eq!(engine_incompatibility(&m).as_deref(), Some("<= 0.1.0"));
        m.max_engine = Some("99.0.0".to_owned());
        assert!(engine_compatible(&m));
    }

    #[test]
    fn actualizacion_disponible() {
        let store = PluginStore { installs: vec![install("x", "1.0.0", true)] };
        assert!(update_available(&manifest("x", "1.1.0"), &store));
        assert!(!update_available(&manifest("x", "1.0.0"), &store));
        assert!(!update_available(&manifest("x", "0.9.0"), &store));
        assert!(!update_available(&manifest("y", "5.0.0"), &store));
    }

    #[test]
    fn instalar_desde_bytes() {
        let root = temp_root("install_ok");
        let bytes = package("herramienta-3d", "1.0.0");
        let mut m = manifest("herramienta-3d", "1.0.0");
        m.sha256 = sha256_hex(&bytes).to_ascii_uppercase();

        let inst = install_plugin_from_bytes(&m, &bytes, &root).unwrap();
        let dir = plugins_dir_in(&root).join("herramienta-3d");
        assert!(dir.join(MANIFEST_FILE).is_file());
        assert!(dir.join("modulo.json").is_file());
        assert!(!plugins_dir_in(&root).join(".herramienta-3d.part").exists());
        assert!(inst.enabled);

        let store = PluginStore::load_from(&store_path_in(&root));
        assert_eq!(store.find("herramienta-3d"), Some(&inst));
        assert!(store.find("herramienta-3d").is_some_and(|i| i.enabled));

        // Reinstalar una versión nueva sustituye la anterior sin duplicar el registro.
        let bytes2 = package("herramienta-3d", "1.1.0");
        let mut m2 = manifest("herramienta-3d", "1.1.0");
        m2.sha256 = sha256_hex(&bytes2);
        install_plugin_from_bytes(&m2, &bytes2, &root).unwrap();
        let store = PluginStore::load_from(&store_path_in(&root));
        assert_eq!(store.installs.len(), 1);
        assert_eq!(store.find("herramienta-3d").map(|i| i.version.as_str()), Some("1.1.0"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sha256_incorrecto_no_instala() {
        let root = temp_root("install_bad_hash");
        let bytes = package("herramienta-3d", "1.0.0");
        let mut m = manifest("herramienta-3d", "1.0.0");
        m.sha256 = "00".repeat(32);

        let err = install_plugin_from_bytes(&m, &bytes, &root).unwrap_err();
        assert!(err.contains("SHA-256"));
        assert!(!plugins_dir_in(&root).join("herramienta-3d").exists());
        assert!(PluginStore::load_from(&store_path_in(&root)).find("herramienta-3d").is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn manifiesto_interior_de_otro_id_falla() {
        let root = temp_root("install_wrong_id");
        let bytes = package("otro", "1.0.0");
        let mut m = manifest("herramienta-3d", "1.0.0");
        m.sha256 = sha256_hex(&bytes);

        assert!(install_plugin_from_bytes(&m, &bytes, &root).is_err());
        assert!(!plugins_dir_in(&root).join("herramienta-3d").exists());
        assert!(!plugins_dir_in(&root).join(".herramienta-3d.part").exists());
        assert!(PluginStore::load_from(&store_path_in(&root)).find("herramienta-3d").is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn id_no_valido_se_rechaza() {
        let root = temp_root("install_bad_id");
        let bytes = package("../fuera", "1.0.0");
        let mut m = manifest("../fuera", "1.0.0");
        m.sha256 = sha256_hex(&bytes);
        assert!(install_plugin_from_bytes(&m, &bytes, &root).is_err());
        assert!(validate_id(".oculto").is_err());
        assert!(validate_id("").is_err());
        assert!(validate_id("herramienta-3d").is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn parsea_catalogo() {
        let json = r#"{
            "plugins": [{
                "id": "herramienta-3d",
                "name": "MeshForge",
                "version": "1.0.0",
                "min_engine": "0.10.14",
                "sha256": "abc123",
                "url": "https://github.com/EliudGnzlz/GlassEnginer/releases/download/plugin-herramienta-3d-1.0.0/herramienta-3d-1.0.0.zip",
                "file_name": "herramienta-3d-1.0.0.zip"
            }]
        }"#;
        let catalog: PluginCatalog = serde_json::from_str(json).unwrap();
        assert_eq!(catalog.plugins.len(), 1);
        let p = &catalog.plugins[0];
        assert_eq!(p.id, "herramienta-3d");
        assert_eq!(p.description, "");
        assert_eq!(p.min_engine.as_deref(), Some("0.10.14"));
        assert_eq!(p.max_engine, None);

        let empty: PluginCatalog = serde_json::from_str("{}").unwrap();
        assert!(empty.plugins.is_empty());
    }
}
