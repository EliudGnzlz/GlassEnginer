use egui::load::SizedTexture;
use egui::{vec2, Frame, Image, TextureId, Ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoMode {
    Translate,
    Rotate,
    Scale,
}

pub struct ViewportPanel {
    pub width: f32,
    pub height: f32,
    pub play_mode: bool,
    pub gizmo_mode: GizmoMode,
}

impl Default for ViewportPanel {
    fn default() -> Self {
        Self {
            width: 1280.0,
            height: 720.0,
            play_mode: false,
            gizmo_mode: GizmoMode::Translate,
        }
    }
}

impl ViewportPanel {
    pub fn render(&mut self, ui: &mut Ui, texture_id: Option<TextureId>) {
        ui.horizontal(|ui| {
            ui.heading("🎬 Viewport 3D");
            ui.separator();

            if ui.selectable_label(!self.play_mode, "⏹ Detenido").clicked() {
                self.play_mode = false;
            }
            if ui.selectable_label(self.play_mode, "▶ Reproducir").clicked() {
                self.play_mode = true;
            }

            ui.separator();
            ui.label("Herramienta:");
            ui.selectable_value(&mut self.gizmo_mode, GizmoMode::Translate, "✛ Mover");
            ui.selectable_value(&mut self.gizmo_mode, GizmoMode::Rotate, "⟳ Rotar");
            ui.selectable_value(&mut self.gizmo_mode, GizmoMode::Scale, "⤢ Escalar");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("📐 Resol: {:.0}x{:.0}", self.width, self.height));
            });
        });

        ui.separator();

        let available = ui.available_size();
        self.width = available.x.max(64.0);
        self.height = available.y.max(64.0);

        Frame::canvas(ui.style()).show(ui, |ui| {
            if let Some(id) = texture_id {
                let aspect = 16.0 / 9.0;
                let mut display_w = self.width;
                let mut display_h = self.width / aspect;
                if display_h > self.height {
                    display_h = self.height;
                    display_w = self.height * aspect;
                }

                let y_pad = (self.height - display_h).max(0.0) / 2.0;

                ui.allocate_ui_with_layout(
                    egui::vec2(self.width, self.height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.add_space(y_pad);
                        let sized = SizedTexture::new(id, vec2(display_w, display_h));
                        ui.add(Image::new(sized));
                    },
                );
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Esperando enlace de textura Vulkan offscreen...");
                });
            }
        });
    }
}
