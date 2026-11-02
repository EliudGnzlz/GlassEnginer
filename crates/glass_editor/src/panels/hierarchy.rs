use egui::Ui;
use glass_mcp::SceneState;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy)]
pub enum EntityPreset {
    Empty,
    Cube,
    Sphere,
    Capsule,
    Cylinder,
    Plane,
    DirectionalLight,
    PointLight,
    AudioSource,
    Camera,
}

enum HierarchyAction {
    Select(u64),
    Delete(u64),
    Duplicate(u64),
    Create(EntityPreset),
}

pub struct HierarchyPanel {
    pub selected_entity: Option<u64>,
    pub search_filter: String,
}

impl Default for HierarchyPanel {
    fn default() -> Self {
        Self {
            selected_entity: Some(3), // Por defecto el objeto principal
            search_filter: String::new(),
        }
    }
}

impl HierarchyPanel {
    pub fn new() -> Self {
        Self::default()
    }

    fn render_creation_menu(ui: &mut Ui, pending_action: &mut Option<HierarchyAction>) {
        if ui.button("📦 Crear Vacío (Create Empty)").clicked() {
            *pending_action = Some(HierarchyAction::Create(EntityPreset::Empty));
            ui.close();
        }
        ui.separator();
        ui.menu_button("🧊 Objeto 3D (3D Object)", |ui| {
            if ui.button("🧊 Cubo (Cube)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::Cube));
                ui.close();
            }
            if ui.button("⚽ Esfera (Sphere)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::Sphere));
                ui.close();
            }
            if ui.button("💊 Cápsula (Capsule)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::Capsule));
                ui.close();
            }
            if ui.button("🛢 Cilindro (Cylinder)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::Cylinder));
                ui.close();
            }
            if ui.button("🟫 Plano / Suelo (Plane)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::Plane));
                ui.close();
            }
        });
        ui.menu_button("💡 Luces (Light)", |ui| {
            if ui.button("💡 Luz Direccional (Directional Light)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::DirectionalLight));
                ui.close();
            }
            if ui.button("🔆 Luz Puntual (Point Light)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::PointLight));
                ui.close();
            }
        });
        ui.menu_button("🔊 Audio", |ui| {
            if ui.button("🔊 Audio Source").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::AudioSource));
                ui.close();
            }
        });
        if ui.button("🎥 Cámara (Camera)").clicked() {
            *pending_action = Some(HierarchyAction::Create(EntityPreset::Camera));
            ui.close();
        }
    }

    pub fn render(&mut self, ui: &mut Ui, scene_state: &Arc<Mutex<SceneState>>) {
        let mut scene = match scene_state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let mut pending_action: Option<HierarchyAction> = None;

        // Barra superior de la Jerarquía estilo Unity
        ui.horizontal(|ui| {
            ui.heading("🌿 Jerarquía");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("➕ Crear ▾", |ui| {
                    Self::render_creation_menu(ui, &mut pending_action);
                });
            });
        });

        // Barra de búsqueda rápida
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.text_edit_singleline(&mut self.search_filter);
            if !self.search_filter.is_empty() && ui.small_button("✖").clicked() {
                self.search_filter.clear();
            }
        });

        ui.separator();

        let search_term = self.search_filter.trim().to_lowercase();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entity in &scene.entities {
                    if !search_term.is_empty() && !entity.name.to_lowercase().contains(&search_term) {
                        continue;
                    }

                    let is_selected = self.selected_entity == Some(entity.id);
                    let icon = if entity.components.iter().any(|c| c == "Camera") {
                        "🎥"
                    } else if entity.components.iter().any(|c| c == "Light") {
                        "💡"
                    } else if entity.components.iter().any(|c| c == "MeshRenderer") {
                        "📐"
                    } else if entity.components.iter().any(|c| c == "AudioSource") {
                        "🔊"
                    } else {
                        "📦"
                    };

                    let label_text = format!("{} #{} {}", icon, entity.id, entity.name);

                    // Reservar fila completa interactiva de ancho total
                    let row_height = 22.0;
                    let full_width = ui.available_width();
                    let (row_rect, row_resp) = ui.allocate_exact_size(
                        egui::vec2(full_width, row_height),
                        egui::Sense::click(),
                    );

                    if ui.is_rect_visible(row_rect) {
                        let bg = if is_selected {
                            ui.visuals().selection.bg_fill
                        } else if row_resp.hovered() {
                            ui.visuals().widgets.hovered.bg_fill
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(row_rect, 3.0, bg);

                        let text_color = if is_selected {
                            ui.visuals().selection.stroke.color
                        } else {
                            ui.visuals().text_color()
                        };

                        let text_pos = row_rect.left_center() + egui::vec2(8.0, 0.0);
                        ui.painter().text(
                            text_pos,
                            egui::Align2::LEFT_CENTER,
                            &label_text,
                            egui::FontId::proportional(13.0),
                            text_color,
                        );

                        // Botón de eliminar en extremo derecho si está seleccionado
                        if is_selected {
                            let del_rect = egui::Rect::from_min_size(
                                egui::pos2(row_rect.right() - 24.0, row_rect.top() + 2.0),
                                egui::vec2(20.0, 18.0),
                            );
                            let del_resp = ui.interact(del_rect, ui.id().with(entity.id).with("del"), egui::Sense::click());
                            if del_resp.hovered() {
                                ui.painter().rect_filled(del_rect, 2.0, egui::Color32::from_rgb(180, 50, 50));
                            }
                            ui.painter().text(
                                del_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "🗑",
                                egui::FontId::proportional(11.0),
                                egui::Color32::WHITE,
                            );
                            if del_resp.clicked() {
                                pending_action = Some(HierarchyAction::Delete(entity.id));
                            }
                        }
                    }

                    // Clic izquierdo en la fila selecciona el objeto
                    if row_resp.clicked() {
                        pending_action = Some(HierarchyAction::Select(entity.id));
                    }

                    // Clic derecho en CUALQUIER punto de la fila abre el menú contextual
                    row_resp.context_menu(|ui| {
                        ui.label(format!("Objeto: {}", entity.name));
                        ui.separator();
                        if ui.button("📋 Duplicar (Duplicate)").clicked() {
                            pending_action = Some(HierarchyAction::Duplicate(entity.id));
                            ui.close();
                        }
                        if ui.button("🗑 Eliminar (Delete)").clicked() {
                            pending_action = Some(HierarchyAction::Delete(entity.id));
                            ui.close();
                        }
                        ui.separator();
                        ui.label("➕ Añadir GameObject:");
                        Self::render_creation_menu(ui, &mut pending_action);
                    });
                }

                // Espacio vacío restante del ScrollArea: clic derecho aquí abre el menú de creación
                let remaining = ui.available_size();
                if remaining.y > 10.0 {
                    let (_empty_rect, bg_resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), remaining.y),
                        egui::Sense::click(),
                    );
                    bg_resp.context_menu(|ui| {
                        ui.label("➕ Crear GameObject");
                        ui.separator();
                        Self::render_creation_menu(ui, &mut pending_action);
                    });
                }
            });

        // Ejecución limpia de acciones sin colisiones de préstamos mutables
        if let Some(action) = pending_action {
            match action {
                HierarchyAction::Select(id) => {
                    self.selected_entity = Some(id);
                }
                HierarchyAction::Delete(id) => {
                    scene.delete_entity(id);
                    if self.selected_entity == Some(id) {
                        self.selected_entity = scene.entities.first().map(|e| e.id);
                    }
                }
                HierarchyAction::Duplicate(id) => {
                    if let Some(orig) = scene.get_entity(id).cloned() {
                        let new_id = scene.create_entity(
                            format!("{} (Clone)", orig.name),
                            orig.components.clone(),
                        );
                        if let Some(new_ent) = scene.get_entity_mut(new_id) {
                            new_ent.transform = orig.transform.clone();
                            new_ent.transform.position[0] += 1.0;
                            new_ent.mesh = orig.mesh.clone();
                            new_ent.script = orig.script.clone();
                            new_ent.script_data = orig.script_data.clone();
                            new_ent.physics = orig.physics.clone();
                            new_ent.audio = orig.audio.clone();
                        }
                        self.selected_entity = Some(new_id);
                    }
                }
                HierarchyAction::Create(preset) => {
                    let new_id = match preset {
                        EntityPreset::Empty => {
                            scene.create_entity("GameObject".into(), vec!["Transform".into()])
                        }
                        EntityPreset::Cube => {
                            let id = scene.create_entity(
                                "Cube".into(),
                                vec![
                                    "Transform".into(),
                                    "MeshRenderer".into(),
                                    "RigidBody".into(),
                                    "Collider".into(),
                                ],
                            );
                            if let Some(ent) = scene.get_entity_mut(id) {
                                ent.transform.position = [0.0, 2.0, 0.0];
                                if let Some(m) = &mut ent.mesh {
                                    m.color = [0.2, 0.7, 1.0];
                                }
                            }
                            id
                        }
                        EntityPreset::Sphere => {
                            let id = scene.create_entity(
                                "Sphere".into(),
                                vec![
                                    "Transform".into(),
                                    "MeshRenderer".into(),
                                    "RigidBody".into(),
                                    "Collider".into(),
                                ],
                            );
                            if let Some(ent) = scene.get_entity_mut(id) {
                                ent.transform.position = [0.0, 2.0, 0.0];
                                if let Some(m) = &mut ent.mesh {
                                    m.color = [0.9, 0.4, 0.2];
                                }
                            }
                            id
                        }
                        EntityPreset::Capsule => {
                            let id = scene.create_entity(
                                "Capsule".into(),
                                vec![
                                    "Transform".into(),
                                    "MeshRenderer".into(),
                                    "RigidBody".into(),
                                    "Collider".into(),
                                ],
                            );
                            if let Some(ent) = scene.get_entity_mut(id) {
                                ent.transform.position = [0.0, 2.0, 0.0];
                                ent.transform.scale = [1.0, 2.0, 1.0];
                                if let Some(m) = &mut ent.mesh {
                                    m.color = [0.3, 0.8, 0.4];
                                }
                            }
                            id
                        }
                        EntityPreset::Cylinder => {
                            let id = scene.create_entity(
                                "Cylinder".into(),
                                vec![
                                    "Transform".into(),
                                    "MeshRenderer".into(),
                                    "RigidBody".into(),
                                    "Collider".into(),
                                ],
                            );
                            if let Some(ent) = scene.get_entity_mut(id) {
                                ent.transform.position = [0.0, 2.0, 0.0];
                                ent.transform.scale = [1.0, 2.0, 1.0];
                                if let Some(m) = &mut ent.mesh {
                                    m.color = [0.8, 0.8, 0.2];
                                }
                            }
                            id
                        }
                        EntityPreset::Plane => {
                            let id = scene.create_entity(
                                "Plane".into(),
                                vec![
                                    "Transform".into(),
                                    "MeshRenderer".into(),
                                    "StaticCollider".into(),
                                ],
                            );
                            if let Some(ent) = scene.get_entity_mut(id) {
                                ent.transform.position = [0.0, 0.0, 0.0];
                                ent.transform.scale = [10.0, 0.1, 10.0];
                                if let Some(m) = &mut ent.mesh {
                                    m.color = [0.5, 0.5, 0.5];
                                }
                            }
                            id
                        }
                        EntityPreset::DirectionalLight => {
                            let id = scene.create_entity(
                                "Directional Light".into(),
                                vec!["Transform".into(), "Light".into()],
                            );
                            if let Some(ent) = scene.get_entity_mut(id) {
                                ent.transform.position = [0.0, 10.0, 0.0];
                                ent.transform.rotation = [45.0, -30.0, 0.0];
                            }
                            id
                        }
                        EntityPreset::PointLight => {
                            let id = scene.create_entity(
                                "Point Light".into(),
                                vec!["Transform".into(), "Light".into()],
                            );
                            if let Some(ent) = scene.get_entity_mut(id) {
                                ent.transform.position = [0.0, 3.0, 0.0];
                            }
                            id
                        }
                        EntityPreset::AudioSource => {
                            let id = scene.create_entity(
                                "Audio Source".into(),
                                vec!["Transform".into(), "AudioSource".into()],
                            );
                            if let Some(ent) = scene.get_entity_mut(id) {
                                ent.transform.position = [0.0, 1.0, 0.0];
                            }
                            id
                        }
                        EntityPreset::Camera => {
                            let id = scene.create_entity(
                                "Camera".into(),
                                vec!["Transform".into(), "Camera".into()],
                            );
                            if let Some(ent) = scene.get_entity_mut(id) {
                                ent.transform.position = [0.0, 2.0, -5.0];
                            }
                            id
                        }
                    };
                    self.selected_entity = Some(new_id);
                }
            }
        }
    }
}
