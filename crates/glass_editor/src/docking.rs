use crate::panels::{ConsolePanel, HierarchyPanel, InspectorPanel, ViewportPanel};
use egui::Panel;
use glass_mcp::SceneState;
use std::sync::{Arc, Mutex};

pub struct EditorDockingLayout {
    pub hierarchy: HierarchyPanel,
    pub viewport: ViewportPanel,
    pub inspector: InspectorPanel,
    pub console: ConsolePanel,
}

impl Default for EditorDockingLayout {
    fn default() -> Self {
        Self {
            hierarchy: HierarchyPanel::new(),
            viewport: ViewportPanel::default(),
            inspector: InspectorPanel::new(),
            console: ConsolePanel::default(),
        }
    }
}

impl EditorDockingLayout {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        viewport_texture_id: Option<egui::TextureId>,
        scene_state: &Arc<Mutex<SceneState>>,
    ) {
        Panel::left("hierarchy_panel")
            .resizable(true)
            .default_size(240.0)
            .min_size(160.0)
            .max_size(450.0)
            .show(ui, |ui| {
                self.hierarchy.render(ui, scene_state);
            });

        let mut selected_id = self.hierarchy.selected_entity;

        Panel::right("inspector_panel")
            .resizable(true)
            .default_size(280.0)
            .min_size(200.0)
            .max_size(500.0)
            .show(ui, |ui| {
                self.inspector.render(ui, selected_id, scene_state);
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
            self.viewport.render(ui, viewport_texture_id, &mut selected_id, scene_state);
        });

        // Sincronizar selección del viewport (3D raycasting click) de vuelta a la jerarquía
        if selected_id != self.hierarchy.selected_entity {
            self.hierarchy.selected_entity = selected_id;
            ui.ctx().request_repaint();
        }
    }
}
