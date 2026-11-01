use egui::{DragValue, Ui};
use glass_mcp::{MeshData, SceneState};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct InspectorPanel;

impl InspectorPanel {
    pub fn new() -> Self {
        Self
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        selected_id: Option<u64>,
        scene_state: &Arc<Mutex<SceneState>>,
    ) {
        ui.heading("🔍 Inspector");
        ui.separator();

        let mut scene = match scene_state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let entity = match selected_id.and_then(|id| scene.entities.iter_mut().find(|e| e.id == id)) {
            Some(e) => e,
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("Ninguna entidad seleccionada.");
                });
                return;
            }
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Nombre de la Entidad
            ui.horizontal(|ui| {
                ui.label(format!("ID: #{}", entity.id));
                ui.text_edit_singleline(&mut entity.name);
            });

            ui.separator();

            // Componente Transform
            ui.collapsing("📐 Transform", |ui| {
                egui::Grid::new("transform_grid")
                    .num_columns(4)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Posición");
                        ui.horizontal(|ui| {
                            ui.label("X");
                            ui.add(DragValue::new(&mut entity.transform.position[0]).speed(0.1));
                            ui.label("Y");
                            ui.add(DragValue::new(&mut entity.transform.position[1]).speed(0.1));
                            ui.label("Z");
                            ui.add(DragValue::new(&mut entity.transform.position[2]).speed(0.1));
                        });
                        ui.end_row();

                        ui.label("Rotación");
                        ui.horizontal(|ui| {
                            ui.label("X");
                            ui.add(DragValue::new(&mut entity.transform.rotation[0]).speed(0.5));
                            ui.label("Y");
                            ui.add(DragValue::new(&mut entity.transform.rotation[1]).speed(0.5));
                            ui.label("Z");
                            ui.add(DragValue::new(&mut entity.transform.rotation[2]).speed(0.5));
                        });
                        ui.end_row();

                        ui.label("Escala");
                        ui.horizontal(|ui| {
                            ui.label("X");
                            ui.add(DragValue::new(&mut entity.transform.scale[0]).speed(0.05));
                            ui.label("Y");
                            ui.add(DragValue::new(&mut entity.transform.scale[1]).speed(0.05));
                            ui.label("Z");
                            ui.add(DragValue::new(&mut entity.transform.scale[2]).speed(0.05));
                        });
                        ui.end_row();
                    });
            });

            ui.separator();

            // Componente Mesh Renderer & Material si existe
            let mut add_mesh = false;
            if let Some(ref mut mesh) = entity.mesh {
                ui.collapsing("🎨 Mesh Renderer & Material", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Color Albedo:");
                        ui.color_edit_button_rgb(&mut mesh.color);
                    });
                    ui.checkbox(&mut mesh.wireframe, "Modo Wireframe");
                    ui.label(format!("Shader: {}", mesh.shader));
                });
            } else {
                ui.horizontal(|ui| {
                    if ui.button("➕ Añadir MeshRenderer").clicked() {
                        add_mesh = true;
                    }
                });
            }
            if add_mesh {
                entity.mesh = Some(MeshData::default());
                if !entity.components.contains(&"MeshRenderer".to_string()) {
                    entity.components.push("MeshRenderer".into());
                }
            }

            ui.separator();

            // Componente Script C# si existe
            if let Some(ref mut script) = entity.script {
                ui.collapsing("📜 Script C# (.NET CoreCLR)", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Script:");
                        ui.text_edit_singleline(script);
                    });
                    ui.label("Estado: ✅ Sincronizado con MCP");
                });
            } else {
                if ui.button("➕ Añadir Script C#").clicked() {
                    entity.script = Some("PlayerMovement.cs".into());
                    if !entity.components.contains(&"Script".to_string()) {
                        entity.components.push("Script".into());
                    }
                }
            }
        });
    }
}
