//! Preferencias de la Herramienta 3D (esquema de atajos de edición de malla, avisos),
//! persistidas como JSON en la carpeta de datos del editor.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Esquema de atajos de teclado para la edición de malla.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShortcutScheme {
    Blender,
    Unity,
}

/// Preferencias persistentes de la Herramienta 3D.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolPrefs {
    /// Esquema de atajos elegido. `None` = todavía no se ha preguntado.
    #[serde(default)]
    pub scheme: Option<ShortcutScheme>,
    /// No volver a preguntar por el collider (lo usa B-E2).
    #[serde(default)]
    pub collider_no_ask: bool,
}

/// Ruta del archivo de preferencias: `<data_root>/tool_prefs.json`.
pub fn prefs_path() -> PathBuf {
    crate::updates::data_root().join("tool_prefs.json")
}

impl ToolPrefs {
    /// Carga desde la ruta por defecto.
    pub fn load() -> Self {
        Self::load_from(&prefs_path())
    }

    /// Carga desde `path`. Si no existe o no parsea, devuelve los valores por defecto.
    pub fn load_from(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|texto| serde_json::from_str(&texto).ok())
            .unwrap_or_default()
    }

    /// Guarda en la ruta por defecto.
    pub fn save(&self) -> Result<(), String> {
        self.save_to(&prefs_path())
    }

    /// Guarda en `path` como JSON legible; crea el directorio padre si hace falta.
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(padre) = path.parent() {
            if !padre.as_os_str().is_empty() {
                fs::create_dir_all(padre)
                    .map_err(|e| format!("no se pudo crear {}: {e}", padre.display()))?;
            }
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("no se pudo serializar las preferencias: {e}"))?;
        fs::write(path, json).map_err(|e| format!("no se pudo escribir {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Directorio temporal único por proceso y por test.
    fn dir_temporal(sufijo: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "glass_tool_prefs_{}_{sufijo}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn roundtrip_blender_con_collider() {
        let dir = dir_temporal("roundtrip_blender");
        let ruta = dir.join("anidado").join("tool_prefs.json");
        let prefs = ToolPrefs {
            scheme: Some(ShortcutScheme::Blender),
            collider_no_ask: true,
        };
        prefs.save_to(&ruta).expect("guardar");
        assert_eq!(ToolPrefs::load_from(&ruta), prefs);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_vacio_da_defaults() {
        let dir = dir_temporal("json_vacio");
        fs::create_dir_all(&dir).expect("crear dir");
        let ruta = dir.join("tool_prefs.json");
        fs::write(&ruta, "{}").expect("escribir");
        let prefs = ToolPrefs::load_from(&ruta);
        assert_eq!(prefs, ToolPrefs::default());
        assert_eq!(prefs.scheme, None);
        assert!(!prefs.collider_no_ask);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_parcial_conserva_lo_presente() {
        let dir = dir_temporal("json_parcial");
        fs::create_dir_all(&dir).expect("crear dir");
        let ruta = dir.join("tool_prefs.json");
        fs::write(&ruta, r#"{"collider_no_ask": true}"#).expect("escribir");
        let prefs = ToolPrefs::load_from(&ruta);
        assert_eq!(prefs.scheme, None);
        assert!(prefs.collider_no_ask);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archivo_inexistente_da_defaults() {
        let dir = dir_temporal("inexistente");
        let ruta = dir.join("no_existe.json");
        assert_eq!(ToolPrefs::load_from(&ruta), ToolPrefs::default());
    }

    #[test]
    fn roundtrip_unity() {
        let dir = dir_temporal("roundtrip_unity");
        let ruta = dir.join("tool_prefs.json");
        let prefs = ToolPrefs {
            scheme: Some(ShortcutScheme::Unity),
            collider_no_ask: false,
        };
        prefs.save_to(&ruta).expect("guardar");
        assert_eq!(ToolPrefs::load_from(&ruta), prefs);
        let _ = fs::remove_dir_all(&dir);
    }
}
