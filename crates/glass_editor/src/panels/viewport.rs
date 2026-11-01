#[derive(Default)]
pub struct ViewportPanel {
    pub width: f32,
    pub height: f32,
}

impl ViewportPanel {
    pub fn render(&mut self, ui: &mut egui::Ui) {
        ui.heading("Viewport 3D");
        let available = ui.available_size();
        self.width = available.x;
        self.height = available.y;
        ui.label(format!("Resolución de Render: {:.0}x{:.0}", self.width, self.height));
        ui.label("(Textura offscreen Vulkan enlazada aquí)");
    }
}
