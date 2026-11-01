#[derive(Default)]
pub struct InspectorPanel {}

impl InspectorPanel {
    pub fn render(&mut self, ui: &mut egui::Ui) {
        ui.heading("Inspector");
        ui.separator();
        ui.collapsing("Transform", |ui| {
            ui.label("Posición: [0.0, 0.0, 0.0]");
            ui.label("Rotación: [0.0, 0.0, 0.0]");
            ui.label("Escala:   [1.0, 1.0, 1.0]");
        });
    }
}
