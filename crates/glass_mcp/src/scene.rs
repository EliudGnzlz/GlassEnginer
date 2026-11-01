use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformData {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for TransformData {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshData {
    pub color: [f32; 3],
    pub wireframe: bool,
    pub shader: String,
}

impl Default for MeshData {
    fn default() -> Self {
        Self {
            color: [0.0, 0.8, 0.9],
            wireframe: false,
            shader: "triangle.vert.spv".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityData {
    pub id: u64,
    pub name: String,
    pub components: Vec<String>,
    pub transform: TransformData,
    pub mesh: Option<MeshData>,
    pub script: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SceneState {
    pub entities: Vec<EntityData>,
    pub next_entity_id: u64,
    pub viewport_snapshot_png: Option<Vec<u8>>,
    pub shaders_reloaded_flag: bool,
}

impl Default for SceneState {
    fn default() -> Self {
        Self {
            entities: vec![
                EntityData {
                    id: 1,
                    name: "Main Camera".into(),
                    components: vec!["Transform".into(), "Camera".into()],
                    transform: TransformData {
                        position: [0.0, 1.5, -5.0],
                        rotation: [0.0, 0.0, 0.0],
                        scale: [1.0, 1.0, 1.0],
                    },
                    mesh: None,
                    script: None,
                },
                EntityData {
                    id: 2,
                    name: "Directional Light".into(),
                    components: vec!["Transform".into(), "Light".into()],
                    transform: TransformData {
                        position: [2.0, 10.0, 5.0],
                        rotation: [-45.0, 30.0, 0.0],
                        scale: [1.0, 1.0, 1.0],
                    },
                    mesh: None,
                    script: None,
                },
                EntityData {
                    id: 3,
                    name: "Vulkan Viewport Triangle".into(),
                    components: vec!["Transform".into(), "MeshRenderer".into(), "Material".into()],
                    transform: TransformData {
                        position: [0.0, 0.0, 0.0],
                        rotation: [0.0, 0.0, 0.0],
                        scale: [1.0, 1.0, 1.0],
                    },
                    mesh: Some(MeshData::default()),
                    script: None,
                },
                EntityData {
                    id: 4,
                    name: "Player Entity".into(),
                    components: vec!["Transform".into(), "Script".into()],
                    transform: TransformData {
                        position: [0.0, 0.0, 2.0],
                        rotation: [0.0, 0.0, 0.0],
                        scale: [1.0, 1.0, 1.0],
                    },
                    mesh: None,
                    script: Some("PlayerMovement.cs".into()),
                },
            ],
            next_entity_id: 5,
            viewport_snapshot_png: None,
            shaders_reloaded_flag: false,
        }
    }
}

impl SceneState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_entity(&mut self, name: String, components: Vec<String>) -> u64 {
        let id = self.next_entity_id;
        self.next_entity_id += 1;
        self.entities.push(EntityData {
            id,
            name,
            components,
            transform: TransformData::default(),
            mesh: None,
            script: None,
        });
        id
    }

    pub fn delete_entity(&mut self, id: u64) -> bool {
        if let Some(pos) = self.entities.iter().position(|e| e.id == id) {
            self.entities.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn get_entity(&self, id: u64) -> Option<&EntityData> {
        self.entities.iter().find(|e| e.id == id)
    }

    pub fn get_entity_mut(&mut self, id: u64) -> Option<&mut EntityData> {
        self.entities.iter_mut().find(|e| e.id == id)
    }

    pub fn modify_transform(
        &mut self,
        id: u64,
        position: Option<[f32; 3]>,
        rotation: Option<[f32; 3]>,
        scale: Option<[f32; 3]>,
    ) -> bool {
        if let Some(entity) = self.get_entity_mut(id) {
            if let Some(pos) = position {
                entity.transform.position = pos;
            }
            if let Some(rot) = rotation {
                entity.transform.rotation = rot;
            }
            if let Some(s) = scale {
                entity.transform.scale = s;
            }
            true
        } else {
            false
        }
    }

    pub fn modify_material(&mut self, id: u64, color: [f32; 3], wireframe: Option<bool>) -> bool {
        if let Some(entity) = self.get_entity_mut(id) {
            if let Some(ref mut mesh) = entity.mesh {
                mesh.color = color;
                if let Some(wf) = wireframe {
                    mesh.wireframe = wf;
                }
            } else {
                entity.mesh = Some(MeshData {
                    color,
                    wireframe: wireframe.unwrap_or(false),
                    shader: "triangle.vert.spv".into(),
                });
            }
            true
        } else {
            false
        }
    }

    pub fn update_viewport_snapshot(&mut self, png_bytes: Vec<u8>) {
        self.viewport_snapshot_png = Some(png_bytes);
    }
}
