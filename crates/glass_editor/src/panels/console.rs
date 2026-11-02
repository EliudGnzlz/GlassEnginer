use egui::Ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Console,
    Project,
}

pub struct ConsolePanel {
    pub active_tab: BottomTab,
    pub logs: Vec<(String, &'static str)>,
    pub filter_info: bool,
    pub filter_warn: bool,
    pub filter_err: bool,
}

impl Default for ConsolePanel {
    fn default() -> Self {
        Self {
            active_tab: BottomTab::Console,
            logs: vec![
                ("[Vulkan] Inicializado dispositivo y colas gráficas con éxito.".into(), "INFO"),
                ("[Renderer] Offscreen Framebuffer 1280x720 asignado en VRAM.".into(), "INFO"),
                ("[Editor] Panel Docking estilo Unity renderizado en egui.".into(), "INFO"),
                ("[Hot-Reload] Vigilante de recursos activo en assets/.".into(), "INFO"),
            ],
            filter_info: true,
            filter_warn: true,
            filter_err: true,
        }
    }
}

impl ConsolePanel {
    pub fn render_console_only(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("📋 Consola");
            ui.separator();
            ui.checkbox(&mut self.filter_info, "ℹ Info");
            ui.checkbox(&mut self.filter_warn, "⚠ Advertencias");
            ui.checkbox(&mut self.filter_err, "⛔ Errores");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🗑 Limpiar").clicked() {
                    self.logs.clear();
                }
            });
        });

        ui.separator();

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for (msg, level) in &self.logs {
                    let (badge, color) = match *level {
                        "ERROR" => ("⛔ [ERROR]", egui::Color32::from_rgb(255, 100, 100)),
                        "WARN" => ("⚠ [WARN]", egui::Color32::from_rgb(255, 200, 80)),
                        _ => ("ℹ [INFO]", egui::Color32::from_rgb(150, 200, 255)),
                    };

                    let show = match *level {
                        "ERROR" => self.filter_err,
                        "WARN" => self.filter_warn,
                        _ => self.filter_info,
                    };

                    if show {
                        ui.horizontal(|ui| {
                            ui.colored_label(color, badge);
                            ui.label(msg);
                        });
                    }
                }
            });
    }

    pub fn render_project_only(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("📁 Proyecto / Assets");
            ui.separator();
            ui.label("Ruta: assets/");
        });

        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let assets = [
                    ("📄 triangle.vert", "Vertex Shader SPIR-V"),
                    ("📄 triangle.frag", "Fragment Shader SPIR-V"),
                    ("🧊 cube.blend", "Blender 3D Asset"),
                    ("📜 PlayerMovement.cs", "C# Gameplay Script"),
                    ("🔊 soundtrack.ogg", "Audio Asset"),
                ];

                for (name, desc) in assets {
                    ui.group(|ui| {
                        ui.set_min_size(egui::vec2(140.0, 60.0));
                        ui.vertical(|ui| {
                            ui.strong(name);
                            ui.small(desc);
                        });
                    });
                }
            });
        });
    }

    pub fn render(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.active_tab, BottomTab::Console, "📋 Consola");
            ui.selectable_value(&mut self.active_tab, BottomTab::Project, "📁 Proyecto / Assets");
        });

        ui.separator();

        match self.active_tab {
            BottomTab::Console => self.render_console_only(ui),
            BottomTab::Project => self.render_project_only(ui),
        }
    }
}
