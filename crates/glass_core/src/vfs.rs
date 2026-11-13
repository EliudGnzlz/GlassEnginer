//! Sistema de archivos virtuales (VFS) mínimo.
//!
//! ITERACIÓN 5 — prerequisito de contenido.
//!
//! Hasta la Iteración 4 todas las rutas de assets del motor eran literales de
//! cadena relativos al directorio de trabajo (`"assets/scenes/SampleScene.scene.json"`,
//! `"assets/shaders/mesh.frag.spv"`, `"project.json"`). Eso funcionaba sólo si el
//! proceso se lanzaba desde la raíz del proyecto, y no ofrecía ninguna defensa
//! frente a rutas que escapan de `assets/` (un `..` en el nombre de una escena
//! pedida por un script de C# leía cualquier archivo del disco).
//!
//! Este módulo resuelve las dos cosas sin introducir un sistema de paquetes:
//!
//! * **Raíz descubierta, no asumida**: `AssetVfs::locate()` sube por los
//!   directorios padres buscando el marcador `project.json`, de modo que el
//!   motor encuentra sus assets aunque se lance desde `build/` o desde un
//!   acceso directo.
//! * **Rutas relativas selladas**: `normalize_asset_path` rechaza rutas
//!   absolutas y cualquier componente `..`; `resolve` garantiza además que el
//!   resultado quede físicamente dentro de la raíz.
//!
//! Deliberadamente **no** implementa montajes, paquetes comprimidos ni
//! prioridades de búsqueda: es la abstracción mínima que texturas, shaders y
//! builds standalone necesitan, y nada más.

use std::path::{Component, Path, PathBuf};

/// Archivo cuyo hallazgo define la raíz de assets.
pub const ROOT_MARKER: &str = "project.json";

/// Directorio de escenas, relativo a la raíz de assets.
pub const SCENES_DIR: &str = "assets/scenes";

/// Errores de resolución de rutas de assets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VfsError {
    /// La ruta intenta salir de la raíz de assets (`..` o absoluta).
    EscapesRoot(String),
    /// La ruta está vacía o sólo contiene separadores.
    Empty,
}

impl std::fmt::Display for VfsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VfsError::EscapesRoot(p) => write!(
                f,
                "la ruta '{p}' escapa de la raíz de assets o es absoluta"
            ),
            VfsError::Empty => write!(f, "la ruta de asset está vacía"),
        }
    }
}

impl std::error::Error for VfsError {}

/// Normaliza una ruta de asset relativa.
///
/// Acepta separadores `/` y `\`, ignora segmentos vacíos y `.`, y **rechaza**
/// cualquier `..` o ruta absoluta. El resultado usa siempre `/` como separador
/// lógico para que las claves de asset sean idénticas en Linux y Windows.
pub fn normalize_asset_path(relative: &str) -> Result<String, VfsError> {
    let trimmed = relative.trim();
    if trimmed.is_empty() {
        return Err(VfsError::Empty);
    }

    // Una ruta absoluta nunca es un asset: ni POSIX (`/etc/passwd`) ni Windows
    // (`C:\...`), que además `Path` no reconocería como absoluta en Linux.
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(VfsError::EscapesRoot(trimmed.to_string()));
    }
    if trimmed.len() >= 2 && trimmed.as_bytes()[1] == b':' {
        return Err(VfsError::EscapesRoot(trimmed.to_string()));
    }

    let mut parts: Vec<&str> = Vec::new();
    for segment in trimmed.split(['/', '\\']) {
        match segment {
            "" | "." => continue,
            ".." => return Err(VfsError::EscapesRoot(trimmed.to_string())),
            s => parts.push(s),
        }
    }

    if parts.is_empty() {
        return Err(VfsError::Empty);
    }

    Ok(parts.join("/"))
}

/// Entrada del explorador de assets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

/// Sistema de archivos virtual anclado a una raíz de proyecto.
#[derive(Debug, Clone)]
pub struct AssetVfs {
    root: PathBuf,
}

impl AssetVfs {
    /// Crea un VFS anclado explícitamente a `root`.
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// Descubre la raíz subiendo por los padres del directorio actual hasta
    /// encontrar `project.json`. Devuelve `None` si no hay proyecto.
    pub fn locate() -> Option<Self> {
        Self::locate_from(std::env::current_dir().ok()?)
    }

    /// Igual que `locate`, partiendo de un directorio concreto.
    pub fn locate_from(start: impl AsRef<Path>) -> Option<Self> {
        let mut dir: Option<&Path> = Some(start.as_ref());
        while let Some(current) = dir {
            if current.join(ROOT_MARKER).is_file() {
                return Some(Self::new(current));
            }
            dir = current.parent();
        }
        None
    }

    /// Raíz del proyecto.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Directorio absoluto del explorador de assets (`<raíz>/assets`).
    pub fn assets_dir(&self) -> PathBuf {
        self.root.join("assets")
    }

    /// Resuelve una ruta relativa a una ruta absoluta, garantizando que quede
    /// dentro de la raíz.
    pub fn resolve(&self, relative: &str) -> Result<PathBuf, VfsError> {
        let normal = normalize_asset_path(relative)?;
        let candidate = self.root.join(&normal);

        // Segunda barrera: aunque `normalize` ya descartó `..`, se comprueba
        // que el resultado siga colgando de la raíz. Defensa en profundidad por
        // si en el futuro se relaja la normalización.
        if !candidate.starts_with(&self.root) {
            return Err(VfsError::EscapesRoot(relative.to_string()));
        }

        Ok(candidate)
    }

    /// `true` si la ruta relativa existe dentro de la raíz.
    pub fn exists(&self, relative: &str) -> bool {
        self.resolve(relative).map(|p| p.exists()).unwrap_or(false)
    }

    /// Lee un asset de texto (UTF-8).
    pub fn read_string(&self, relative: &str) -> std::io::Result<String> {
        let path = self
            .resolve(relative)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        std::fs::read_to_string(path)
    }

    /// Escribe un asset de texto, creando los directorios intermedios.
    pub fn write_string(&self, relative: &str, contents: &str) -> std::io::Result<()> {
        let path = self
            .resolve(relative)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)
    }

    /// Lista el contenido de un directorio de assets, ordenado con carpetas
    /// primero y alfabético dentro de cada grupo.
    pub fn list(&self, relative_dir: &str) -> std::io::Result<Vec<AssetEntry>> {
        let dir = self
            .resolve(relative_dir)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

        let mut entries: Vec<AssetEntry> = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            // Los archivos ocultos se muestran igual que en un explorador de
            // archivos, pero se omiten los artefactos de directorio.
            if name == "." || name == ".." {
                continue;
            }
            let meta = entry.metadata()?;
            entries.push(AssetEntry {
                name,
                is_dir: meta.is_dir(),
                size: if meta.is_file() { meta.len() } else { 0 },
            });
        }

        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(entries)
    }

    /// Lista los nombres de escena disponibles (`assets/scenes/*.scene.json`),
    /// sin la extensión doble, ordenados alfabéticamente.
    pub fn list_scenes(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let Ok(entries) = self.list(SCENES_DIR) else {
            return names;
        };
        for entry in entries {
            if entry.is_dir {
                continue;
            }
            if let Some(stem) = entry.name.strip_suffix(".scene.json") {
                names.push(stem.to_string());
            }
        }
        names.sort();
        names
    }

    /// Resuelve el nombre de una escena a su ruta relativa de asset.
    ///
    /// Reglas, en orden:
    /// 1. Nombre vacío o `"<default>"` → escena por defecto de `project.json`.
    /// 2. Un nombre con separadores o terminado en `.json` se trata como ruta
    ///    de asset explícita (permite `assets/scenes/Otra.scene.json`).
    /// 3. `assets/scenes/<nombre>.scene.json` si existe.
    /// 4. `assets/scenes/<nombre>` si existe tal cual.
    ///
    /// Devuelve `None` si ninguna regla encuentra un archivo real, para que el
    /// llamante pueda reportar "escena no encontrada" en lugar de cargar nada.
    pub fn resolve_scene(&self, name: &str) -> Option<String> {
        let requested = name.trim();

        if requested.is_empty() || requested == "<default>" {
            let default = crate::ProjectSettings::load_or_default(self.root.join(ROOT_MARKER));
            let candidate = default.default_scene;
            return self.exists(&candidate).then_some(candidate);
        }

        if requested.contains('/') || requested.contains('\\') || requested.ends_with(".json") {
            return self.exists(requested).then(|| requested.to_string());
        }

        let with_ext = format!("{SCENES_DIR}/{requested}.scene.json");
        if self.exists(&with_ext) {
            return Some(with_ext);
        }

        let bare = format!("{SCENES_DIR}/{requested}");
        self.exists(&bare).then_some(bare)
    }
}

/// Convierte un `PathBuf` absoluto dentro de la raíz en su clave de asset
/// relativa con separadores `/`. Devuelve `None` si está fuera de la raíz.
pub fn to_asset_key(root: &Path, absolute: &Path) -> Option<String> {
    let rel = absolute.strip_prefix(root).ok()?;
    let mut parts: Vec<String> = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(s) => parts.push(s.to_string_lossy().to_string()),
            // Cualquier `..` o raíz significa que la ruta escapa.
            _ => return None,
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza_separadores_y_puntos() {
        assert_eq!(normalize_asset_path("assets/scenes/A.json").unwrap(), "assets/scenes/A.json");
        assert_eq!(normalize_asset_path("./assets//scenes/./A.json").unwrap(), "assets/scenes/A.json");
        assert_eq!(normalize_asset_path("assets\\scenes\\A.json").unwrap(), "assets/scenes/A.json");
        assert_eq!(normalize_asset_path("  assets/A.json  ").unwrap(), "assets/A.json");
    }

    #[test]
    fn rechaza_escapes_de_la_raiz() {
        // El caso que motivó el módulo: un script de C# pidiendo "../secreto".
        assert_eq!(
            normalize_asset_path("../../etc/passwd"),
            Err(VfsError::EscapesRoot("../../etc/passwd".into()))
        );
        assert_eq!(
            normalize_asset_path("assets/../../fuera.json"),
            Err(VfsError::EscapesRoot("assets/../../fuera.json".into()))
        );
        assert!(normalize_asset_path("/etc/passwd").is_err());
        assert!(normalize_asset_path("C:\\Windows\\system32").is_err());
        assert_eq!(normalize_asset_path("   "), Err(VfsError::Empty));
        assert_eq!(normalize_asset_path(""), Err(VfsError::Empty));
    }

    #[test]
    fn resolve_mantiene_todo_dentro_de_la_raiz() {
        let vfs = AssetVfs::new("/tmp/glass_vfs_test");
        let resolved = vfs.resolve("assets/scenes/A.scene.json").unwrap();
        assert!(resolved.starts_with("/tmp/glass_vfs_test"));
        assert!(vfs.resolve("../fuera").is_err());
    }

    #[test]
    fn locate_encuentra_la_raiz_subiendo_por_los_padres() {
        let base = std::env::temp_dir().join(format!("glass_vfs_locate_{}", std::process::id()));
        let nested = base.join("build").join("standalone");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(base.join(ROOT_MARKER), "{}").unwrap();

        let found = AssetVfs::locate_from(&nested).expect("debe encontrar el marcador");
        assert_eq!(found.root(), base.as_path());

        // Y desde un árbol sin proyecto no debe inventarse una raíz.
        let orphan = std::env::temp_dir().join(format!("glass_vfs_orphan_{}", std::process::id()));
        std::fs::create_dir_all(&orphan).unwrap();
        assert!(AssetVfs::locate_from(orphan.join("nada")).is_none());

        std::fs::remove_dir_all(&base).ok();
        std::fs::remove_dir_all(&orphan).ok();
    }

    #[test]
    fn list_scenes_descubre_escenas_reales() {
        let base = std::env::temp_dir().join(format!("glass_vfs_scenes_{}", std::process::id()));
        std::fs::create_dir_all(base.join("assets/scenes")).unwrap();
        std::fs::write(base.join("assets/scenes/Zeta.scene.json"), "[]").unwrap();
        std::fs::write(base.join("assets/scenes/Alfa.scene.json"), "[]").unwrap();
        std::fs::write(base.join("assets/scenes/notas.txt"), "x").unwrap();

        let vfs = AssetVfs::new(&base);
        assert_eq!(vfs.list_scenes(), vec!["Alfa".to_string(), "Zeta".to_string()]);

        // La resolución acepta nombre corto, ruta explícita y nombre inexistente.
        assert_eq!(vfs.resolve_scene("Alfa").as_deref(), Some("assets/scenes/Alfa.scene.json"));
        assert_eq!(
            vfs.resolve_scene("assets/scenes/Zeta.scene.json").as_deref(),
            Some("assets/scenes/Zeta.scene.json")
        );
        assert_eq!(vfs.resolve_scene("NoExiste"), None);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn to_asset_key_rechaza_rutas_externas() {
        let root = Path::new("/tmp/glass_key");
        assert_eq!(
            to_asset_key(root, Path::new("/tmp/glass_key/assets/A.json")).as_deref(),
            Some("assets/A.json")
        );
        assert_eq!(to_asset_key(root, Path::new("/tmp/otro/A.json")), None);
    }
}
