//! Preferencias del menú Gizmos de la vista de escena, persistidas como JSON
//! en la carpeta de datos del editor.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Valor por defecto de los interruptores que nacen activados: casi todo el
/// overlay de la escena se muestra al abrir el editor por primera vez.
fn default_true() -> bool {
    true
}

/// Escala por defecto de los iconos 3D: 1.0 = tamaño "natural" del icono.
fn default_icon_scale() -> f32 {
    1.0
}

/// Preferencias persistentes de los gizmos de la vista de escena.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GizmoPrefs {
    /// Rejilla del viewport. Activada por defecto; se apaga en escenas grandes
    /// donde el ruido visual estorba.
    #[serde(default = "default_true")]
    pub show_grid: bool,
    /// Etiquetas con el nombre de la entidad sobre cada objeto.
    #[serde(default = "default_true")]
    pub show_nameplates: bool,
    /// Iconos 3D (cámara, luz, etc.) dibujados en el espacio de la escena.
    #[serde(default = "default_true")]
    pub icons_3d: bool,
    /// Escala global de los iconos 3D. Vive aquí y no en cada entidad para que
    /// ajustar la legibilidad sea un único cambio persistente.
    #[serde(default = "default_icon_scale")]
    pub icon_scale: f32,
    /// Contorno de resaltado del objeto seleccionado.
    #[serde(default = "default_true")]
    pub selection_outline: bool,
    /// Dibujar colliders siempre (no solo al seleccionar). Apagado por defecto:
    /// en escenas con física densa tapa la geometría real.
    #[serde(default)]
    pub colliders_always: bool,
    /// Icono 3D de las entidades de cámara.
    #[serde(default = "default_true")]
    pub icon_camera: bool,
    /// Gizmo (ejes/frustum) de las entidades de cámara.
    #[serde(default = "default_true")]
    pub gizmo_camera: bool,
    /// Icono 3D de las entidades de luz.
    #[serde(default = "default_true")]
    pub icon_light: bool,
    /// Gizmo (dirección/cono) de las entidades de luz.
    #[serde(default = "default_true")]
    pub gizmo_light: bool,
    /// Gizmo de los colliders de física.
    #[serde(default = "default_true")]
    pub gizmo_collider: bool,
    /// Gizmo de los personajes (cápsula/controlador).
    #[serde(default = "default_true")]
    pub gizmo_character: bool,
}

impl Default for GizmoPrefs {
    /// Implementación manual y no `derive`: los defectos del contrato son
    /// `true` en casi todos los bool y `1.0` en la escala, no `false`/`0.0`.
    fn default() -> Self {
        Self {
            show_grid: true,
            show_nameplates: true,
            icons_3d: true,
            icon_scale: 1.0,
            selection_outline: true,
            colliders_always: false,
            icon_camera: true,
            gizmo_camera: true,
            icon_light: true,
            gizmo_light: true,
            gizmo_collider: true,
            gizmo_character: true,
        }
    }
}

/// Ruta del archivo de preferencias: `<data_root>/gizmo_prefs.json`.
pub fn prefs_path() -> PathBuf {
    crate::updates::data_root().join("gizmo_prefs.json")
}

impl GizmoPrefs {
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
            .map_err(|e| format!("no se pudo serializar las preferencias de gizmos: {e}"))?;
        fs::write(path, json).map_err(|e| format!("no se pudo escribir {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Directorio temporal único por proceso y por test.
    fn dir_temporal(sufijo: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "glass_gizmo_prefs_{}_{sufijo}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn roundtrip_completo() {
        let dir = dir_temporal("roundtrip_completo");
        let ruta = dir.join("anidado").join("gizmo_prefs.json");
        let prefs = GizmoPrefs {
            show_grid: false,
            show_nameplates: false,
            icons_3d: false,
            icon_scale: 2.5,
            selection_outline: false,
            colliders_always: true,
            icon_camera: false,
            gizmo_camera: false,
            icon_light: false,
            gizmo_light: false,
            gizmo_collider: false,
            gizmo_character: false,
        };
        prefs.save_to(&ruta).expect("guardar");
        assert_eq!(GizmoPrefs::load_from(&ruta), prefs);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_vacio_da_defaults() {
        let dir = dir_temporal("json_vacio");
        fs::create_dir_all(&dir).expect("crear dir");
        let ruta = dir.join("gizmo_prefs.json");
        fs::write(&ruta, "{}").expect("escribir");
        let prefs = GizmoPrefs::load_from(&ruta);
        assert_eq!(prefs, GizmoPrefs::default());
        assert!(prefs.show_grid);
        assert!(prefs.show_nameplates);
        assert!(prefs.icons_3d);
        assert_eq!(prefs.icon_scale, 1.0);
        assert!(prefs.selection_outline);
        assert!(!prefs.colliders_always);
        assert!(prefs.icon_camera);
        assert!(prefs.gizmo_camera);
        assert!(prefs.icon_light);
        assert!(prefs.gizmo_light);
        assert!(prefs.gizmo_collider);
        assert!(prefs.gizmo_character);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_parcial_conserva_lo_presente() {
        let dir = dir_temporal("json_parcial");
        fs::create_dir_all(&dir).expect("crear dir");
        let ruta = dir.join("gizmo_prefs.json");
        fs::write(&ruta, r#"{"gizmo_collider": false, "icon_scale": 2.0}"#).expect("escribir");
        let prefs = GizmoPrefs::load_from(&ruta);
        assert!(!prefs.gizmo_collider);
        assert_eq!(prefs.icon_scale, 2.0);
        // El resto sale por defecto, no a false/0.0.
        assert!(prefs.show_grid);
        assert!(prefs.show_nameplates);
        assert!(prefs.icons_3d);
        assert!(prefs.selection_outline);
        assert!(!prefs.colliders_always);
        assert!(prefs.icon_camera);
        assert!(prefs.gizmo_camera);
        assert!(prefs.icon_light);
        assert!(prefs.gizmo_light);
        assert!(prefs.gizmo_character);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archivo_inexistente_da_defaults() {
        let dir = dir_temporal("inexistente");
        let ruta = dir.join("no_existe.json");
        assert_eq!(GizmoPrefs::load_from(&ruta), GizmoPrefs::default());
    }
}
