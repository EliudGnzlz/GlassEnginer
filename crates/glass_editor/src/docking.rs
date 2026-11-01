use crate::panels::{ConsolePanel, HierarchyPanel, InspectorPanel, ViewportPanel};

#[derive(Default)]
pub struct EditorDockingLayout {
    pub hierarchy: HierarchyPanel,
    pub viewport: ViewportPanel,
    pub inspector: InspectorPanel,
    pub console: ConsolePanel,
}

impl EditorDockingLayout {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn render(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("hierarchy_panel").show(ctx, |ui| {
            self.hierarchy.render(ui);
        });

        egui::SidePanel::right("inspector_panel").show(ctx, |ui| {
            self.inspector.render(ui);
        });

        egui::TopBottomPanel::bottom("console_panel").show(ctx, |ui| {
            self.console.render(ui);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.viewport.render(ui);
        });
    }
}
