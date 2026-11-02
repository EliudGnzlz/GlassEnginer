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
pub struct PhysicsData {
    pub body_type: String, // "Dynamic", "Static", "Kinematic"
    pub mass: f32,
    pub friction: f32,
    pub restitution: f32,
    pub box_extents: [f32; 3],
    pub linear_velocity: [f32; 3],
}

impl Default for PhysicsData {
    fn default() -> Self {
        Self {
            body_type: "Dynamic".into(),
            mass: 1.0,
            friction: 0.7,
            restitution: 0.05,
            box_extents: [0.5, 0.5, 0.5],
            linear_velocity: [0.0, 0.0, 0.0],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptComponentData {
    pub class_name: String,
    pub move_speed: f32,
    pub jump_velocity: f32,
    pub auto_demo: bool,
}

impl Default for ScriptComponentData {
    fn default() -> Self {
        Self {
            class_name: "GlassEngine.Core.Scripts.PlayerController".into(),
            move_speed: 3.8,
            jump_velocity: 7.2,
            auto_demo: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSourceData {
    pub clip_name: String,
    pub volume: f32,
    pub pitch: f32,
    pub is_playing: bool,
}

impl Default for AudioSourceData {
    fn default() -> Self {
        Self {
            clip_name: "beep_440".into(),
            volume: 1.0,
            pitch: 1.0,
            is_playing: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraData {
    pub fov: f32,
    pub near_clip: f32,
    pub far_clip: f32,
    pub is_main: bool,
}

impl Default for CameraData {
    fn default() -> Self {
        Self {
            fov: 60.0,
            near_clip: 0.1,
            far_clip: 1000.0,
            is_main: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightData {
    pub light_type: String, // "Directional", "Point", "Spot"
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
}

impl Default for LightData {
    fn default() -> Self {
        Self {
            light_type: "Directional".into(),
            color: [1.0, 0.98, 0.92],
            intensity: 1.0,
            range: 10.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasData {
    pub render_mode: String, // "ScreenSpaceOverlay", "ScreenSpaceCamera", "WorldSpace"
    pub reference_resolution: [f32; 2],
    pub match_width_or_height: f32,
}

impl Default for CanvasData {
    fn default() -> Self {
        Self {
            render_mode: "ScreenSpaceOverlay".into(),
            reference_resolution: [1920.0, 1080.0],
            match_width_or_height: 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RectTransformData {
    pub anchored_position: [f32; 2],
    pub size_delta: [f32; 2],
    pub anchor_min: [f32; 2],
    pub anchor_max: [f32; 2],
    pub pivot: [f32; 2],
}

impl Default for RectTransformData {
    fn default() -> Self {
        Self {
            anchored_position: [0.0, 0.0],
            size_delta: [160.0, 40.0],
            anchor_min: [0.5, 0.5],
            anchor_max: [0.5, 0.5],
            pivot: [0.5, 0.5],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UITextData {
    pub text: String,
    pub font_size: f32,
    pub color: [f32; 4],
}

impl Default for UITextData {
    fn default() -> Self {
        Self {
            text: "Texto UI".into(),
            font_size: 18.0,
            color: [1.0, 1.0, 1.0, 1.0],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIButtonData {
    pub interactable: bool,
    pub target_graphic_color: [f32; 4],
    pub on_click_action: String,
}

impl Default for UIButtonData {
    fn default() -> Self {
        Self {
            interactable: true,
            target_graphic_color: [0.25, 0.55, 0.85, 1.0],
            on_click_action: "".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityData {
    pub id: u64,
    pub active: bool,
    pub name: String,
    pub tag: String,
    pub layer: String,
    pub is_static: bool,
    pub components: Vec<String>,
    pub transform: TransformData,
    pub mesh: Option<MeshData>,
    pub script: Option<String>,
    pub script_data: Option<ScriptComponentData>,
    pub physics: Option<PhysicsData>,
    pub audio: Option<AudioSourceData>,
    pub camera: Option<CameraData>,
    pub light: Option<LightData>,
    pub canvas: Option<CanvasData>,
    pub rect_transform: Option<RectTransformData>,
    pub ui_text: Option<UITextData>,
    pub ui_button: Option<UIButtonData>,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
}

impl EntityData {
    pub fn new(id: u64, name: String, components: Vec<String>) -> Self {
        let is_static = components.iter().any(|c| c == "StaticCollider");
        let mesh = if components.iter().any(|c| c == "MeshRenderer") {
            Some(MeshData::default())
        } else {
            None
        };
        let script = if components.iter().any(|c| c == "Script") {
            Some("GlassEngine.Core.Scripts.PlayerController".into())
        } else {
            None
        };
        let script_data = if script.is_some() {
            Some(ScriptComponentData::default())
        } else {
            None
        };
        let physics = if components.iter().any(|c| c == "RigidBody" || c == "Collider" || c == "StaticCollider") {
            let body_type = if is_static { "Static".into() } else { "Dynamic".into() };
            Some(PhysicsData {
                body_type,
                mass: 1.0,
                friction: 0.7,
                restitution: 0.05,
                box_extents: [0.5, 0.5, 0.5],
                linear_velocity: [0.0, 0.0, 0.0],
            })
        } else {
            None
        };
        let audio = if components.iter().any(|c| c == "AudioSource") {
            Some(AudioSourceData::default())
        } else {
            None
        };
        let camera = if components.iter().any(|c| c == "Camera") {
            Some(CameraData::default())
        } else {
            None
        };
        let light = if components.iter().any(|c| c == "Light") {
            Some(LightData::default())
        } else {
            None
        };
        let canvas = if components.iter().any(|c| c == "Canvas") {
            Some(CanvasData::default())
        } else {
            None
        };
        let rect_transform = if components.iter().any(|c| c == "RectTransform" || c == "Canvas" || c == "Text" || c == "Button") {
            Some(RectTransformData::default())
        } else {
            None
        };
        let ui_text = if components.iter().any(|c| c == "Text") {
            Some(UITextData::default())
        } else {
            None
        };
        let ui_button = if components.iter().any(|c| c == "Button") {
            Some(UIButtonData::default())
        } else {
            None
        };
        let tag = if name.contains("Player") {
            "Player".into()
        } else if name.contains("Camera") {
            "MainCamera".into()
        } else {
            "Untagged".into()
        };

        Self {
            id,
            active: true,
            name,
            tag,
            layer: "Default".into(),
            is_static,
            components,
            transform: TransformData::default(),
            mesh,
            script,
            script_data,
            physics,
            audio,
            camera,
            light,
            canvas,
            rect_transform,
            ui_text,
            ui_button,
            parent: None,
            children: Vec::new(),
        }
    }
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
                EntityData::new(1, "Main Camera".into(), vec!["Transform".into(), "Camera".into()]),
                EntityData::new(2, "Directional Light".into(), vec!["Transform".into(), "Light".into()]),
                EntityData::new(3, "Vulkan Viewport Triangle".into(), vec!["Transform".into(), "MeshRenderer".into(), "Material".into()]),
                EntityData::new(4, "Player Entity".into(), vec!["Transform".into(), "Script".into()]),
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
        self.entities.push(EntityData::new(id, name, components));
        id
    }

    pub fn delete_entity(&mut self, id: u64) -> bool {
        // Quitar de los hijos del padre si tenía
        let parent = self.get_entity(id).and_then(|e| e.parent);
        if let Some(pid) = parent {
            if let Some(p) = self.get_entity_mut(pid) {
                p.children.retain(|&c| c != id);
            }
        }
        // Desemparentar a sus hijos (moverlos al abuelo o raíz)
        let children = self.get_entity(id).map(|e| e.children.clone()).unwrap_or_default();
        for cid in children {
            if let Some(c) = self.get_entity_mut(cid) {
                c.parent = parent;
            }
            if let Some(pid) = parent {
                if let Some(p) = self.get_entity_mut(pid) {
                    p.children.push(cid);
                }
            }
        }

        if let Some(pos) = self.entities.iter().position(|e| e.id == id) {
            self.entities.remove(pos);
            true
        } else {
            false
        }
    }

    /// Comprueba si `candidate_ancestor` es antecesor de `descendant_id` para evitar ciclos
    pub fn is_descendant_of(&self, candidate_ancestor: u64, descendant_id: u64) -> bool {
        let mut curr = Some(candidate_ancestor);
        while let Some(id) = curr {
            if id == descendant_id {
                return true;
            }
            curr = self.get_entity(id).and_then(|e| e.parent);
        }
        false
    }

    /// Emparenta `child_id` bajo `new_parent_id` (o `None` para raíz)
    pub fn reparent(&mut self, child_id: u64, new_parent_id: Option<u64>) -> bool {
        if let Some(pid) = new_parent_id {
            if pid == child_id || self.is_descendant_of(pid, child_id) {
                return false; // Evita auto-emparentamiento o ciclos circulares
            }
        }

        // Quitar de los hijos del padre anterior si lo tenía
        let old_parent = self.get_entity(child_id).and_then(|e| e.parent);
        if let Some(old_pid) = old_parent {
            if let Some(old_p) = self.get_entity_mut(old_pid) {
                old_p.children.retain(|&c| c != child_id);
            }
        }

        // Agregar al nuevo padre si existe
        if let Some(new_pid) = new_parent_id {
            if let Some(new_p) = self.get_entity_mut(new_pid) {
                if !new_p.children.contains(&child_id) {
                    new_p.children.push(child_id);
                }
            }
        }

        // Actualizar el puntero parent del hijo
        if let Some(child) = self.get_entity_mut(child_id) {
            child.parent = new_parent_id;
            true
        } else {
            false
        }
    }

    /// Mueve una entidad hacia arriba en el orden visual de la jerarquía
    pub fn move_entity_up(&mut self, id: u64) -> bool {
        let parent = self.get_entity(id).and_then(|e| e.parent);
        if let Some(pid) = parent {
            if let Some(p) = self.get_entity_mut(pid) {
                if let Some(idx) = p.children.iter().position(|&c| c == id) {
                    if idx > 0 {
                        p.children.swap(idx, idx - 1);
                        return true;
                    }
                }
            }
        } else {
            // Entidad raíz: mover dentro del vector principal entities
            if let Some(idx) = self.entities.iter().position(|e| e.id == id) {
                if idx > 0 {
                    self.entities.swap(idx, idx - 1);
                    return true;
                }
            }
        }
        false
    }

    /// Mueve una entidad hacia abajo en el orden visual de la jerarquía
    pub fn move_entity_down(&mut self, id: u64) -> bool {
        let parent = self.get_entity(id).and_then(|e| e.parent);
        if let Some(pid) = parent {
            if let Some(p) = self.get_entity_mut(pid) {
                if let Some(idx) = p.children.iter().position(|&c| c == id) {
                    if idx + 1 < p.children.len() {
                        p.children.swap(idx, idx + 1);
                        return true;
                    }
                }
            }
        } else {
            // Entidad raíz: mover dentro del vector principal entities
            if let Some(idx) = self.entities.iter().position(|e| e.id == id) {
                if idx + 1 < self.entities.len() {
                    self.entities.swap(idx, idx + 1);
                    return true;
                }
            }
        }
        false
    }

    /// Inserta una entidad inmediatamente antes o después de otra (Drag & Drop en la jerarquía)
    pub fn insert_relative(&mut self, source_id: u64, target_id: u64, before: bool) -> bool {
        if source_id == target_id {
            return false;
        }
        // Prevenir ciclos: target_id no puede ser descendiente de source_id
        if self.is_descendant_of(target_id, source_id) {
            return false;
        }

        let target_parent = self.get_entity(target_id).and_then(|e| e.parent);
        let source_parent = self.get_entity(source_id).and_then(|e| e.parent);

        // Si los padres difieren, desenlazar de su padre anterior
        if source_parent != target_parent {
            if let Some(old_pid) = source_parent {
                if let Some(old_p) = self.get_entity_mut(old_pid) {
                    old_p.children.retain(|&c| c != source_id);
                }
            }
            if let Some(s) = self.get_entity_mut(source_id) {
                s.parent = target_parent;
            }
        }

        if let Some(pid) = target_parent {
            // Reordenar dentro de la lista de hijos del padre
            if let Some(p) = self.get_entity_mut(pid) {
                p.children.retain(|&c| c != source_id);
                if let Some(t_idx) = p.children.iter().position(|&c| c == target_id) {
                    let insert_idx = if before { t_idx } else { t_idx + 1 };
                    let clamped = insert_idx.min(p.children.len());
                    p.children.insert(clamped, source_id);
                    return true;
                } else {
                    p.children.push(source_id);
                    return true;
                }
            }
        } else {
            // Ambas son entidades raíz en self.entities
            if let Some(s_idx) = self.entities.iter().position(|e| e.id == source_id) {
                let ent = self.entities.remove(s_idx);
                if let Some(t_idx) = self.entities.iter().position(|e| e.id == target_id) {
                    let insert_idx = if before { t_idx } else { t_idx + 1 };
                    let clamped = insert_idx.min(self.entities.len());
                    self.entities.insert(clamped, ent);
                    return true;
                } else {
                    self.entities.push(ent);
                    return true;
                }
            }
        }
        false
    }

    /// Agrupa una o más entidades bajo un nuevo GameObject padre
    pub fn group_entities(&mut self, entity_ids: &[u64], group_name: &str) -> u64 {
        let group_id = self.create_entity(group_name.into(), vec!["Transform".into()]);
        for &eid in entity_ids {
            self.reparent(eid, Some(group_id));
        }
        group_id
    }

    /// Retorna la posición acumulada en espacio de mundo recorriendo la jerarquía
    pub fn get_world_position(&self, id: u64) -> [f32; 3] {
        let mut pos = [0.0, 0.0, 0.0];
        let mut curr = Some(id);
        while let Some(eid) = curr {
            if let Some(ent) = self.get_entity(eid) {
                pos[0] += ent.transform.position[0];
                pos[1] += ent.transform.position[1];
                pos[2] += ent.transform.position[2];
                curr = ent.parent;
            } else {
                break;
            }
        }
        pos
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

    pub fn modify_script(&mut self, id: u64, script_name: Option<String>) -> bool {
        if let Some(entity) = self.get_entity_mut(id) {
            entity.script = script_name;
            if entity.script.is_some() && !entity.components.iter().any(|c| c == "Script") {
                entity.components.push("Script".to_string());
            }
            true
        } else {
            false
        }
    }

    pub fn update_viewport_snapshot(&mut self, png_bytes: Vec<u8>) {
        self.viewport_snapshot_png = Some(png_bytes);
    }

    /// Retorna la primera cámara activa disponible en la escena
    pub fn active_camera(&self) -> Option<&EntityData> {
        self.entities.iter().find(|e| e.active && e.camera.is_some())
    }

    /// Retorna la luz direccional o principal activa en la escena
    pub fn main_light(&self) -> Option<&EntityData> {
        self.entities.iter().find(|e| e.active && e.light.is_some())
    }

    /// Guarda el estado de la escena en un archivo JSON en disco
    pub fn save_to_json(&self, path: &str) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self.entities)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)
    }

    /// Carga entidades desde un archivo JSON reemplazando la escena actual
    pub fn load_from_json(&mut self, path: &str) -> std::io::Result<()> {
        let json = std::fs::read_to_string(path)?;
        let entities: Vec<EntityData> = serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.entities = entities;
        self.next_entity_id = self.entities.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reparenting_and_hierarchy() {
        let mut scene = SceneState::new();
        let parent_id = scene.create_entity("Parent".into(), vec!["Transform".into()]);
        let child_id = scene.create_entity("Child".into(), vec!["Transform".into()]);

        assert!(scene.reparent(child_id, Some(parent_id)));
        assert_eq!(scene.get_entity(child_id).unwrap().parent, Some(parent_id));
        assert!(scene.get_entity(parent_id).unwrap().children.contains(&child_id));

        // Desemparentar
        assert!(scene.reparent(child_id, None));
        assert_eq!(scene.get_entity(child_id).unwrap().parent, None);
        assert!(!scene.get_entity(parent_id).unwrap().children.contains(&child_id));
    }

    #[test]
    fn test_cycle_prevention() {
        let mut scene = SceneState::new();
        let a = scene.create_entity("A".into(), vec!["Transform".into()]);
        let b = scene.create_entity("B".into(), vec!["Transform".into()]);
        let c = scene.create_entity("C".into(), vec!["Transform".into()]);

        // A -> B -> C
        assert!(scene.reparent(b, Some(a)));
        assert!(scene.reparent(c, Some(b)));

        // Intentar hacer que A sea hijo de C (crearía un ciclo) debe fallar
        assert!(!scene.reparent(a, Some(c)));
        assert_eq!(scene.get_entity(a).unwrap().parent, None);
    }

    #[test]
    fn test_move_up_and_down() {
        let mut scene = SceneState::new();
        let e1 = scene.create_entity("E1".into(), vec!["Transform".into()]);
        let e2 = scene.create_entity("E2".into(), vec!["Transform".into()]);

        let n = scene.entities.len();
        assert_eq!(scene.entities[n - 2].id, e1);
        assert_eq!(scene.entities[n - 1].id, e2);

        // Mover e2 arriba (intercambia con e1)
        assert!(scene.move_entity_up(e2));
        assert_eq!(scene.entities[n - 2].id, e2);
        assert_eq!(scene.entities[n - 1].id, e1);

        // Mover e2 abajo (regresa al final)
        assert!(scene.move_entity_down(e2));
        assert_eq!(scene.entities[n - 2].id, e1);
        assert_eq!(scene.entities[n - 1].id, e2);

        // Probar reordenación entre hermanos hijos
        let p = scene.create_entity("Parent".into(), vec!["Transform".into()]);
        let c1 = scene.create_entity("Child 1".into(), vec!["Transform".into()]);
        let c2 = scene.create_entity("Child 2".into(), vec!["Transform".into()]);
        scene.reparent(c1, Some(p));
        scene.reparent(c2, Some(p));

        assert_eq!(scene.get_entity(p).unwrap().children, vec![c1, c2]);
        assert!(scene.move_entity_up(c2));
        assert_eq!(scene.get_entity(p).unwrap().children, vec![c2, c1]);
        assert!(scene.move_entity_down(c2));
        assert_eq!(scene.get_entity(p).unwrap().children, vec![c1, c2]);
    }

    #[test]
    fn test_group_entities() {
        let mut scene = SceneState::new();
        let c1 = scene.create_entity("Cube 1".into(), vec!["Transform".into()]);
        let c2 = scene.create_entity("Cube 2".into(), vec!["Transform".into()]);

        let group_id = scene.group_entities(&[c1, c2], "Cubes Group");
        let group_ent = scene.get_entity(group_id).unwrap();
        assert_eq!(group_ent.name, "Cubes Group");
        assert_eq!(group_ent.children.len(), 2);
        assert!(group_ent.children.contains(&c1));
        assert!(group_ent.children.contains(&c2));

        assert_eq!(scene.get_entity(c1).unwrap().parent, Some(group_id));
        assert_eq!(scene.get_entity(c2).unwrap().parent, Some(group_id));
    }

    #[test]
    fn test_delete_entity_cleans_hierarchy() {
        let mut scene = SceneState::new();
        let p = scene.create_entity("Parent".into(), vec!["Transform".into()]);
        let c = scene.create_entity("Child".into(), vec!["Transform".into()]);
        scene.reparent(c, Some(p));

        scene.delete_entity(p);
        assert!(scene.get_entity(p).is_none());
        // El hijo debe quedar desemparentado a la raíz
        let child = scene.get_entity(c).unwrap();
        assert_eq!(child.parent, None);
    }

    #[test]
    fn test_world_position_hierarchy() {
        let mut scene = SceneState::new();
        let p = scene.create_entity("Parent".into(), vec!["Transform".into()]);
        let c = scene.create_entity("Child".into(), vec!["Transform".into()]);
        scene.modify_transform(p, Some([10.0, 5.0, 0.0]), None, None);
        scene.modify_transform(c, Some([2.0, 1.0, 0.0]), None, None);
        scene.reparent(c, Some(p));

        let world_pos = scene.get_world_position(c);
        assert_eq!(world_pos, [12.0, 6.0, 0.0]);
    }

    #[test]
    fn test_insert_relative_drag_and_drop() {
        let mut scene = SceneState::new();
        let p = scene.create_entity("Parent".into(), vec!["Transform".into()]);
        let c1 = scene.create_entity("C1".into(), vec!["Transform".into()]);
        let c2 = scene.create_entity("C2".into(), vec!["Transform".into()]);
        let c3 = scene.create_entity("C3".into(), vec!["Transform".into()]);
        scene.reparent(c1, Some(p));
        scene.reparent(c2, Some(p));
        scene.reparent(c3, Some(p));

        assert_eq!(scene.get_entity(p).unwrap().children, vec![c1, c2, c3]);

        // Insertar c3 antes de c1 (subirlo al inicio de la lista de hermanos)
        assert!(scene.insert_relative(c3, c1, true));
        assert_eq!(scene.get_entity(p).unwrap().children, vec![c3, c1, c2]);

        // Insertar c3 después de c1 (bajarlo después de c1)
        assert!(scene.insert_relative(c3, c1, false));
        assert_eq!(scene.get_entity(p).unwrap().children, vec![c1, c3, c2]);

        // Prevención de ciclo: no se puede insertar un padre dentro de su hijo
        assert!(!scene.insert_relative(p, c1, true));
    }

    #[test]
    fn test_scene_serialization_and_canvas_ui() {
        let mut scene = SceneState::new();
        let canvas_id = scene.create_entity(
            "Main Canvas".into(),
            vec!["Transform".into(), "RectTransform".into(), "Canvas".into()],
        );
        let text_id = scene.create_entity(
            "Score Text".into(),
            vec!["Transform".into(), "RectTransform".into(), "Text".into()],
        );
        scene.reparent(text_id, Some(canvas_id));

        assert!(scene.get_entity(canvas_id).unwrap().canvas.is_some());
        assert!(scene.get_entity(text_id).unwrap().ui_text.is_some());

        let temp_path = std::env::temp_dir().join("test_scene.json");
        let temp_str = temp_path.to_str().unwrap();

        assert!(scene.save_to_json(temp_str).is_ok());

        let mut loaded_scene = SceneState::new();
        assert!(loaded_scene.load_from_json(temp_str).is_ok());

        assert_eq!(loaded_scene.entities.len(), scene.entities.len());
        let loaded_text = loaded_scene.get_entity(text_id).unwrap();
        assert_eq!(loaded_text.name, "Score Text");
        assert!(loaded_text.ui_text.is_some());
        assert_eq!(loaded_text.parent, Some(canvas_id));

        let _ = std::fs::remove_file(temp_path);
    }
}
