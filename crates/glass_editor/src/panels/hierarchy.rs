use egui::Ui;
use glass_mcp::SceneState;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct HierarchyPanel {
    pub selected_entity: Option<u64>,
}

impl HierarchyPanel {
    pub fn new() -> Self {
        Self {
            selected_entity: Some(3), // Por defecto el triángulo
        }
    }

    pub fn render(&mut self, ui: &mut Ui, scene_state: &Arc<Mutex<SceneState>>) {
        let mut scene = match scene_state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        ui.horizontal(|ui| {
            ui.heading("🌿 Jerarquía");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("➕ Crear").clicked() {
                    let id = scene.create_entity("GameObject".into(), vec!["Transform".into()]);
                    self.selected_entity = Some(id);
                }
            });
        });

        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut to_delete = None;
            for entity in &scene.entities {
                let is_selected = self.selected_entity == Some(entity.id);
                let icon = if entity.components.iter().any(|c| c == "Camera") {
                    "🎥"
                } else if entity.components.iter().any(|c| c == "Light") {
                    "💡"
                } else if entity.components.iter().any(|c| c == "MeshRenderer") {
                    "📐"
                } else {
                    "📦"
                };

                let label_text = format!("{} #{} {}", icon, entity.id, entity.name);

                ui.horizontal(|ui| {
                    if ui.selectable_label(is_selected, &label_text).clicked() {
                        self.selected_entity = Some(entity.id);
                    }

                    if is_selected {
                        if ui.small_button("🗑").clicked() {
                            to_delete = Some(entity.id);
                        }
                    }
                });
            }

            if let Some(id) = to_delete {
                scene.delete_entity(id);
                if self.selected_entity == Some(id) {
                    self.selected_entity = scene.entities.first().map(|e| e.id);
                }
            }
        });
    }
}
