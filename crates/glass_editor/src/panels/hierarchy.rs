use egui::Ui;

#[derive(Debug, Clone)]
pub struct EntityNode {
    pub id: u64,
    pub name: String,
    pub icon: &'static str,
    pub children: Vec<EntityNode>,
}

pub struct HierarchyPanel {
    pub selected_entity: Option<u64>,
    pub entities: Vec<EntityNode>,
    pub next_id: u64,
}

impl Default for HierarchyPanel {
    fn default() -> Self {
        Self {
            selected_entity: Some(1),
            next_id: 5,
            entities: vec![
                EntityNode {
                    id: 1,
                    name: "Main Camera".into(),
                    icon: "🎥",
                    children: Vec::new(),
                },
                EntityNode {
                    id: 2,
                    name: "Directional Light".into(),
                    icon: "💡",
                    children: Vec::new(),
                },
                EntityNode {
                    id: 3,
                    name: "Vulkan Viewport Triangle".into(),
                    icon: "📐",
                    children: Vec::new(),
                },
                EntityNode {
                    id: 4,
                    name: "Player Entity".into(),
                    icon: "👾",
                    children: Vec::new(),
                },
            ],
        }
    }
}

impl HierarchyPanel {
    pub fn render(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("🌿 Jerarquía");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("➕ Crear").clicked() {
                    let id = self.next_id;
                    self.next_id += 1;
                    self.entities.push(EntityNode {
                        id,
                        name: format!("GameObject_{}", id),
                        icon: "📦",
                        children: Vec::new(),
                    });
                    self.selected_entity = Some(id);
                }
            });
        });

        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut to_delete = None;
            for (idx, entity) in self.entities.iter().enumerate() {
                let is_selected = self.selected_entity == Some(entity.id);
                let label_text = format!("{} {}", entity.icon, entity.name);

                ui.horizontal(|ui| {
                    if ui.selectable_label(is_selected, &label_text).clicked() {
                        self.selected_entity = Some(entity.id);
                    }

                    if is_selected {
                        if ui.small_button("🗑").clicked() {
                            to_delete = Some(idx);
                        }
                    }
                });
            }

            if let Some(idx) = to_delete {
                self.entities.remove(idx);
                self.selected_entity = self.entities.first().map(|e| e.id);
            }
        });
    }
}
