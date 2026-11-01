use egui::{DragValue, Ui};

#[derive(Debug, Clone)]
pub struct TransformComponent {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for TransformComponent {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

pub struct InspectorPanel {
    pub entity_name: String,
    pub is_active: bool,
    pub transform: TransformComponent,
    pub mesh_color: [f32; 3],
    pub wireframe: bool,
    pub script_name: String,
}

impl Default for InspectorPanel {
    fn default() -> Self {
        Self {
            entity_name: "Vulkan Viewport Triangle".into(),
            is_active: true,
            transform: TransformComponent::default(),
            mesh_color: [0.0, 0.8, 0.9],
            wireframe: false,
            script_name: "PlayerMovement.cs".into(),
        }
    }
}

impl InspectorPanel {
    pub fn render(&mut self, ui: &mut Ui) {
        ui.heading("🔍 Inspector");
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Cabecera de la Entidad
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.is_active, "");
                ui.text_edit_singleline(&mut self.entity_name);
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
                            ui.add(DragValue::new(&mut self.transform.position[0]).speed(0.1));
                            ui.label("Y");
                            ui.add(DragValue::new(&mut self.transform.position[1]).speed(0.1));
                            ui.label("Z");
                            ui.add(DragValue::new(&mut self.transform.position[2]).speed(0.1));
                        });
                        ui.end_row();

                        ui.label("Rotación");
                        ui.horizontal(|ui| {
                            ui.label("X");
                            ui.add(DragValue::new(&mut self.transform.rotation[0]).speed(0.5));
                            ui.label("Y");
                            ui.add(DragValue::new(&mut self.transform.rotation[1]).speed(0.5));
                            ui.label("Z");
                            ui.add(DragValue::new(&mut self.transform.rotation[2]).speed(0.5));
                        });
                        ui.end_row();

                        ui.label("Escala");
                        ui.horizontal(|ui| {
                            ui.label("X");
                            ui.add(DragValue::new(&mut self.transform.scale[0]).speed(0.05));
                            ui.label("Y");
                            ui.add(DragValue::new(&mut self.transform.scale[1]).speed(0.05));
                            ui.label("Z");
                            ui.add(DragValue::new(&mut self.transform.scale[2]).speed(0.05));
                        });
                        ui.end_row();
                    });
            });

            ui.separator();

            // Componente Renderizador de Malla (Mesh Renderer)
            ui.collapsing("🎨 Mesh Renderer & Material", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Color Albedo:");
                    ui.color_edit_button_rgb(&mut self.mesh_color);
                });
                ui.checkbox(&mut self.wireframe, "Modo Wireframe");
                ui.label("Shader: triangle.vert.spv / triangle.frag.spv");
            });

            ui.separator();

            // Componente de Scripting C# (.NET)
            ui.collapsing("📜 Script C# (.NET CoreCLR)", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Script:");
                    ui.text_edit_singleline(&mut self.script_name);
                });
                ui.label("Estado de Compilación: ✅ Compilado (.NET 10.0)");
            });

            ui.separator();

            ui.vertical_centered(|ui| {
                if ui.button("➕ Añadir Componente").clicked() {
                    // Selector de componentes futuros
                }
            });
        });
    }
}
