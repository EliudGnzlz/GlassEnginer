use std::path::Path;
use serde::{Deserialize, Serialize};

/// Configuración global del proyecto GlassEngine (`project.json`), paridad con ProjectSettings de Unity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub name: String,
    pub version: String,
    pub company_name: String,
    pub default_scene: String,
    pub scenes_in_build: Vec<String>,
    pub window_width: u32,
    pub window_height: u32,
    pub fullscreen: bool,
    pub target_fps: u32,
    pub vsync: bool,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            name: "GlassEngine Project".into(),
            version: "0.1.0-beta".into(),
            company_name: "GlassEngine Studio".into(),
            default_scene: "assets/scenes/SampleScene.scene.json".into(),
            scenes_in_build: vec!["assets/scenes/SampleScene.scene.json".into()],
            window_width: 1280,
            window_height: 720,
            fullscreen: false,
            target_fps: 60,
            vsync: true,
        }
    }
}

impl ProjectSettings {
    /// Carga el archivo `project.json` o devuelve los valores por defecto si no existe o falla el parseo.
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Self {
        if let Ok(content) = std::fs::read_to_string(path) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Guarda la configuración en disco en formato JSON.
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)?;
        std::fs::write(p, json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_settings_serialization() {
        let settings = ProjectSettings {
            name: "MyAwesomeGame".into(),
            scenes_in_build: vec![
                "assets/scenes/Level1.scene.json".into(),
                "assets/scenes/Level2.scene.json".into(),
            ],
            ..Default::default()
        };

        let json = serde_json::to_string(&settings).unwrap();
        let loaded: ProjectSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.name, "MyAwesomeGame");
        assert_eq!(loaded.scenes_in_build.len(), 2);
    }
}
