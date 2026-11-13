use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::scene::{EntityData, SceneState};

/// Representa un activo de Prefab reutilizable, serializable a `.prefab.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefab {
    pub format_version: u32,
    pub name: String,
    pub root_entity: EntityData,
    pub descendant_entities: Vec<EntityData>,
}

impl Prefab {
    /// Crea un Prefab a partir de una entidad existente y todos sus descendientes en la escena.
    pub fn from_entity(scene: &SceneState, root_id: u64) -> Result<Self, String> {
        let root = scene
            .get_entity(root_id)
            .ok_or_else(|| format!("Entidad #{} no existe en la escena", root_id))?
            .clone();

        let mut descendants = Vec::new();
        let mut queue = root.children.clone();

        while let Some(child_id) = queue.pop() {
            if let Some(child) = scene.get_entity(child_id) {
                descendants.push(child.clone());
                queue.extend(child.children.clone());
            }
        }

        Ok(Self {
            format_version: 1,
            name: root.name.clone(),
            root_entity: root,
            descendant_entities: descendants,
        })
    }

    /// Guarda el Prefab en un archivo `.prefab.json`.
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)?;
        std::fs::write(p, json)
    }

    /// Carga un Prefab desde un archivo `.prefab.json`.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let prefab: Self = serde_json::from_str(&json)
            .map_err(std::io::Error::other)?;
        Ok(prefab)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefab_creation_and_instantiation() {
        let mut scene = SceneState::new();
        let parent_id = scene.create_entity("CoinGroup".into(), vec!["Transform".into()]);
        let child_id = scene.create_entity("CoinMesh".into(), vec!["Transform".into(), "MeshRenderer".into()]);
        assert!(scene.reparent(child_id, Some(parent_id)));

        // Crear prefab desde parent
        let prefab = Prefab::from_entity(&scene, parent_id).expect("Error creando prefab");
        assert_eq!(prefab.name, "CoinGroup");
        assert_eq!(prefab.descendant_entities.len(), 1);

        // Guardar y cargar prefab en memoria JSON
        let json = serde_json::to_string(&prefab).unwrap();
        let loaded_prefab: Prefab = serde_json::from_str(&json).unwrap();

        // Instanciar prefab dos veces en la escena
        let inst1_id = scene.instantiate_prefab(&loaded_prefab, None, Some([1.0, 2.0, 3.0]));
        let inst2_id = scene.instantiate_prefab(&loaded_prefab, None, Some([4.0, 5.0, 6.0]));

        assert_ne!(inst1_id, parent_id);
        assert_ne!(inst2_id, inst1_id);

        let inst1_root = scene.get_entity(inst1_id).unwrap();
        assert_eq!(inst1_root.transform.position, [1.0, 2.0, 3.0]);
        assert_eq!(inst1_root.children.len(), 1);

        let inst1_child = scene.get_entity(inst1_root.children[0]).unwrap();
        assert_eq!(inst1_child.parent, Some(inst1_id));
        assert_ne!(inst1_child.id, child_id);
    }
}
