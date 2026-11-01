use crate::panels::{ConsolePanel, HierarchyPanel, InspectorPanel, ViewportPanel};
use egui::Panel;

pub struct EditorDockingLayout {
    pub hierarchy: HierarchyPanel,
    pub viewport: ViewportPanel,
    pub inspector: InspectorPanel,
    pub console: ConsolePanel,
}

impl Default for EditorDockingLayout {
    fn default() -> Self {
        Self {
            hierarchy: HierarchyPanel::default(),
            viewport: ViewportPanel::default(),
            inspector: InspectorPanel::default(),
            console: ConsolePanel::default(),
        }
    }
}

impl EditorDockingLayout {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn render(&mut self, ui: &mut egui::Ui, viewport_texture_id: Option<egui::TextureId>) {
        Panel::left("hierarchy_panel")
            .resizable(true)
            .default_size(240.0)
            .min_size(160.0)
            .max_size(450.0)
            .show(ui, |ui| {
                self.hierarchy.render(ui);
            });

        Panel::right("inspector_panel")
            .resizable(true)
            .default_size(280.0)
            .min_size(200.0)
            .max_size(500.0)
            .show(ui, |ui| {
                self.inspector.render(ui);
            });

        Panel::bottom("console_panel")
            .resizable(true)
            .default_size(180.0)
            .min_size(100.0)
            .max_size(400.0)
            .show(ui, |ui| {
                self.console.render(ui);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            self.viewport.render(ui, viewport_texture_id);
        });
    }
}
