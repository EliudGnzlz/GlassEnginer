#[derive(Default)]
pub struct HierarchyPanel {
    pub selected_entity: Option<u64>,
}

impl HierarchyPanel {
    pub fn render(&mut self, ui: &mut egui::Ui) {
        ui.heading("Jerarquía de Escena");
        ui.separator();
        ui.label("• Escena Principal");
        ui.label("  ↳ Cámara Principal");
        ui.label("  ↳ Luz Direccional");
    }
}
