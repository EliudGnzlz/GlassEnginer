#[derive(Default)]
pub struct ConsolePanel {
    pub logs: Vec<String>,
}

impl ConsolePanel {
    pub fn render(&mut self, ui: &mut egui::Ui) {
        ui.heading("Consola de Mensajes");
        ui.separator();
        if self.logs.is_empty() {
            ui.label("[GlassEngine] Sistema iniciado correctamente.");
        } else {
            for log in &self.logs {
                ui.label(log);
            }
        }
    }
}
