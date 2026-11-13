use egui::{Color32, Ui};
use glass_core::EntityData;
use glass_mcp::SceneState;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy)]
pub enum EntityPreset {
    Group,
    Empty,
    Cube,
    Sphere,
    Capsule,
    Cylinder,
    Plane,
    DirectionalLight,
    PointLight,
    AudioSource,
    Camera,
    Canvas,
    UIText,
    UIButton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPosition {
    Before,
    Inside,
    After,
}

enum HierarchyAction {
    Select(u64),
    Delete(u64),
    Duplicate(u64),
    Create(EntityPreset),
    CreateChild(u64, EntityPreset),
    MoveUp(u64),
    MoveDown(u64),
    Reparent(u64, Option<u64>),
    InsertRelative(u64, u64, bool),
    Group(Vec<u64>),
}

pub struct HierarchyPanel {
    pub selected_entity: Option<u64>,
    pub search_filter: String,
    pub collapsed_nodes: HashSet<u64>,
    pub dragging_entity: Option<u64>,
    /// Desplazamiento puntero−esquina de la fila al empezar el
    /// arrastre: el fantasma se dibuja con ese mismo agarre
    /// para que la fila quede PEGADA a la punta del ratón.
    pub drag_grab_offset: egui::Vec2,
    /// Ancho de la fila agarrada (el fantasma lo conserva).
    pub drag_row_width: f32,
    pub hovered_drop: Option<(u64, DropPosition)>,
}

impl Default for HierarchyPanel {
    fn default() -> Self {
        Self {
            selected_entity: Some(3), // Por defecto el objeto principal
            search_filter: String::new(),
            collapsed_nodes: HashSet::new(),
            dragging_entity: None,
            drag_grab_offset: egui::Vec2::ZERO,
            drag_row_width: 0.0,
            hovered_drop: None,
        }
    }
}

impl HierarchyPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Aplana el árbol jerárquico respetando raíces, hijos y estado colapsado
    fn flatten_tree(
        entities: &[EntityData],
        collapsed: &HashSet<u64>,
        filter: &str,
    ) -> Vec<(u64, usize, bool)> {
        let mut out = Vec::new();
        if !filter.is_empty() {
            for e in entities {
                if e.name.to_lowercase().contains(filter) {
                    out.push((e.id, 0, !e.children.is_empty()));
                }
            }
            return out;
        }

        // Obtener todas las entidades raíz (parent == None o padre no existente)
        let root_ids: Vec<u64> = entities
            .iter()
            .filter(|e| e.parent.is_none() || !entities.iter().any(|p| Some(p.id) == e.parent))
            .map(|e| e.id)
            .collect();

        fn recurse(
            id: u64,
            depth: usize,
            entities: &[EntityData],
            collapsed: &HashSet<u64>,
            out: &mut Vec<(u64, usize, bool)>,
        ) {
            let ent = match entities.iter().find(|e| e.id == id) {
                Some(e) => e,
                None => return,
            };
            let has_children = !ent.children.is_empty();
            out.push((id, depth, has_children));

            if has_children && !collapsed.contains(&id) {
                for &child_id in &ent.children {
                    recurse(child_id, depth + 1, entities, collapsed, out);
                }
            }
        }

        for r in root_ids {
            recurse(r, 0, entities, collapsed, &mut out);
        }

        out
    }

    fn render_creation_menu(ui: &mut Ui, pending_action: &mut Option<HierarchyAction>) {
        if ui.button("📁 Crear Grupo Vacío (Create Group)").clicked() {
            *pending_action = Some(HierarchyAction::Create(EntityPreset::Group));
            ui.close();
        }
        if ui.button("📦 Crear Vacío (Create Empty)").clicked() {
            *pending_action = Some(HierarchyAction::Create(EntityPreset::Empty));
            ui.close();
        }
        ui.separator();
        ui.menu_button("⬡ Objeto 3D (3D Object)", |ui| {
            if ui.button("■ Cubo (Cube)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::Cube));
                ui.close();
            }
            if ui.button("● Esfera (Sphere)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::Sphere));
                ui.close();
            }
            if ui.button("⬯ Cápsula (Capsule)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::Capsule));
                ui.close();
            }
            if ui.button("⬒ Cilindro (Cylinder)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::Cylinder));
                ui.close();
            }
            if ui.button("▬ Plano / Suelo (Plane)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::Plane));
                ui.close();
            }
        });
        ui.menu_button("☼ Luces (Light)", |ui| {
            if ui.button("☼ Luz Direccional (Directional Light)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::DirectionalLight));
                ui.close();
            }
            if ui.button("✦ Luz Puntual (Point Light)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::PointLight));
                ui.close();
            }
        });
        ui.menu_button("♫ Audio", |ui| {
            if ui.button("♫ Audio Source").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::AudioSource));
                ui.close();
            }
        });
        if ui.button("🎥 Cámara (Camera)").clicked() {
            *pending_action = Some(HierarchyAction::Create(EntityPreset::Camera));
            ui.close();
        }
        ui.menu_button("🖼 UI", |ui| {
            if ui.button("🖼 Canvas").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::Canvas));
                ui.close();
            }
            if ui.button("🔤 Texto (TMP / Text)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::UIText));
                ui.close();
            }
            if ui.button("🔘 Botón (Button)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::UIButton));
                ui.close();
            }
        });
    }

    pub fn render(&mut self, ui: &mut Ui, scene_state: &Arc<Mutex<SceneState>>) {
        let mut scene = match scene_state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let mut pending_action: Option<HierarchyAction> = None;
        let mouse_pos = ui.input(|i| i.pointer.hover_pos());
        let is_pointer_released = ui.input(|i| i.pointer.any_released());

        // La tecla Supr ya no se comprueba aquí: es un comando
        // global del marco del editor (docking), para que borre
        // la selección única sea cual sea el panel con el foco.

        // Barra superior estilo Unity
        ui.horizontal(|ui| {
            ui.heading("Hierarchy");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("+ Create ▾", |ui| {
                    Self::render_creation_menu(ui, &mut pending_action);
                });

                if let Some(sel_id) = self.selected_entity {
                    if ui.button("📁 Group").on_hover_text("Agrupar objeto seleccionado en un nuevo padre").clicked() {
                        pending_action = Some(HierarchyAction::Group(vec![sel_id]));
                    }
                }
            });
        });

        // Barra de búsqueda rápida
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.text_edit_singleline(&mut self.search_filter);
            if !self.search_filter.is_empty() && ui.small_button("✖").clicked() {
                self.search_filter.clear();
            }
        });

        ui.separator();

        let search_term = self.search_filter.trim().to_lowercase();
        let mut current_frame_drop: Option<(u64, DropPosition)> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let flat_tree = Self::flatten_tree(&scene.entities, &self.collapsed_nodes, &search_term);

                for (entity_id, depth, has_children) in flat_tree {
                    let entity = match scene.entities.iter().find(|e| e.id == entity_id) {
                        Some(e) => e,
                        None => continue,
                    };

                    let is_selected = self.selected_entity == Some(entity.id);
                    let is_collapsed = self.collapsed_nodes.contains(&entity.id);

                    // Iconos simples y minimalistas estilo Unity
                    let icon = if !entity.children.is_empty() && entity.mesh.is_none() {
                        "📁"
                    } else if entity.components.iter().any(|c| c == "Camera") {
                        "🎥"
                    } else if entity.components.iter().any(|c| c == "Light") {
                        "☼"
                    } else if entity.components.iter().any(|c| c == "MeshRenderer") {
                        "⬡"
                    } else if entity.components.iter().any(|c| c == "AudioSource") {
                        "♫"
                    } else {
                        "▣"
                    };

                    // Fila completa interactiva con soporte para clic y arrastre (Drag & Drop)
                    let row_height = 22.0;
                    let full_width = ui.available_width();
                    let (row_rect, row_resp) = ui.allocate_exact_size(
                        egui::vec2(full_width, row_height),
                        egui::Sense::click_and_drag(),
                    );

                    // Inicio del arrastre de la fila
                    if row_resp.drag_started() {
                        self.dragging_entity = Some(entity.id);
                        if let Some(mp) = mouse_pos {
                            self.drag_grab_offset = mp - row_rect.min;
                        }
                        self.drag_row_width = row_rect.width();
                    }

                    // Detección de zona de soltado (Drop Target) cuando se arrastra sobre esta fila
                    if let Some(drag_id) = self.dragging_entity {
                        if drag_id != entity.id && row_resp.hovered() {
                            if let Some(mp) = mouse_pos {
                                let rel_y = (mp.y - row_rect.top()) / row_rect.height();
                                let drop_pos = if rel_y < 0.28 {
                                    DropPosition::Before
                                } else if rel_y > 0.72 {
                                    DropPosition::After
                                } else {
                                    DropPosition::Inside
                                };
                                current_frame_drop = Some((entity.id, drop_pos));
                            }
                        }
                    }

                    if ui.is_rect_visible(row_rect) {
                        let bg = if is_selected {
                            ui.visuals().selection.bg_fill
                        } else if row_resp.hovered() && self.dragging_entity.is_none() {
                            ui.visuals().widgets.hovered.bg_fill
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(row_rect, 2.0, bg);

                        // Si esta fila es el objetivo activo de Drag & Drop, renderizar feedback visual estilo Unity
                        if let Some((target_id, dpos)) = current_frame_drop {
                            if target_id == entity.id {
                                match dpos {
                                    DropPosition::Before => {
                                        let y = row_rect.top();
                                        ui.painter().line_segment(
                                            [egui::pos2(row_rect.left(), y), egui::pos2(row_rect.right(), y)],
                                            egui::Stroke::new(2.5, Color32::from_rgb(0, 160, 255)),
                                        );
                                        ui.painter().circle_filled(
                                            egui::pos2(row_rect.left() + 4.0, y),
                                            3.5,
                                            Color32::from_rgb(0, 160, 255),
                                        );
                                    }
                                    DropPosition::After => {
                                        let y = row_rect.bottom();
                                        ui.painter().line_segment(
                                            [egui::pos2(row_rect.left(), y), egui::pos2(row_rect.right(), y)],
                                            egui::Stroke::new(2.5, Color32::from_rgb(0, 160, 255)),
                                        );
                                        ui.painter().circle_filled(
                                            egui::pos2(row_rect.left() + 4.0, y),
                                            3.5,
                                            Color32::from_rgb(0, 160, 255),
                                        );
                                    }
                                    DropPosition::Inside => {
                                        ui.painter().rect(
                                            row_rect,
                                            2.0,
                                            Color32::from_rgba_unmultiplied(0, 140, 255, 35),
                                            egui::Stroke::new(1.5, Color32::from_rgb(0, 160, 255)),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                }
                            }
                        }

                        let text_color = if is_selected {
                            ui.visuals().selection.stroke.color
                        } else {
                            ui.visuals().text_color()
                        };

                        // La fila que se está arrastrando queda
                        // atenuada en su sitio (el «original» se
                        // ve en el fantasma que sigue al ratón).
                        let text_color = if self.dragging_entity == Some(entity.id) {
                            text_color.gamma_multiply(0.4)
                        } else {
                            text_color
                        };

                        // Indentación según profundidad en la jerarquía
                        let indent_x = (depth as f32) * 16.0;

                        // Triángulo desplegar/colapsar
                        let mut content_x = row_rect.left() + 6.0 + indent_x;
                        if has_children {
                            let arrow_rect = egui::Rect::from_min_size(
                                egui::pos2(content_x, row_rect.top() + 2.0),
                                egui::vec2(14.0, 18.0),
                            );
                            let arrow_resp = ui.interact(
                                arrow_rect,
                                ui.id().with(entity.id).with("arrow"),
                                egui::Sense::click(),
                            );
                            let arrow_text = if is_collapsed { "▶" } else { "▼" };
                            ui.painter().text(
                                arrow_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                arrow_text,
                                egui::FontId::proportional(10.0),
                                if arrow_resp.hovered() { Color32::WHITE } else { Color32::GRAY },
                            );
                            if arrow_resp.clicked() {
                                if is_collapsed {
                                    self.collapsed_nodes.remove(&entity.id);
                                } else {
                                    self.collapsed_nodes.insert(entity.id);
                                }
                            }
                            content_x += 16.0;
                        } else {
                            content_x += 16.0;
                        }

                        // Etiqueta simple y limpia: icono y nombre del GameObject
                        let label_text = format!("{} {}", icon, entity.name);
                        let text_pos = egui::pos2(content_x, row_rect.center().y);
                        ui.painter().text(
                            text_pos,
                            egui::Align2::LEFT_CENTER,
                            &label_text,
                            egui::FontId::proportional(13.0),
                            text_color,
                        );
                    }

                    // Clic en la fila selecciona el objeto
                    if row_resp.clicked() {
                        pending_action = Some(HierarchyAction::Select(entity.id));
                    }

                    // Clic derecho abre el menú contextual
                    row_resp.context_menu(|ui| {
                        ui.label(egui::RichText::new(&entity.name).strong());
                        ui.separator();

                        if ui.button("▲ Mover Arriba").clicked() {
                            pending_action = Some(HierarchyAction::MoveUp(entity.id));
                            ui.close();
                        }
                        if ui.button("▼ Mover Abajo").clicked() {
                            pending_action = Some(HierarchyAction::MoveDown(entity.id));
                            ui.close();
                        }
                        ui.separator();

                        if ui.button("📁 Agrupar Selección").clicked() {
                            pending_action = Some(HierarchyAction::Group(vec![entity.id]));
                            ui.close();
                        }

                        if ui.button("➕ Crear Hijo Vacío").clicked() {
                            pending_action = Some(HierarchyAction::CreateChild(entity.id, EntityPreset::Empty));
                            ui.close();
                        }

                        if entity.parent.is_some()
                            && ui.button("⛓ Desemparentar (Mover a Raíz)").clicked() {
                                pending_action = Some(HierarchyAction::Reparent(entity.id, None));
                                ui.close();
                            }

                        // Submenú para emparentar con otro objeto
                        let candidate_parents: Vec<(u64, String)> = scene.entities
                            .iter()
                            .filter(|e| e.id != entity.id && !scene.is_descendant_of(e.id, entity.id))
                            .map(|e| (e.id, e.name.clone()))
                            .collect();

                        if !candidate_parents.is_empty() {
                            ui.menu_button("🔗 Emparentar con... ▾", |ui| {
                                for (pid, pname) in candidate_parents {
                                    if ui.button(pname.to_string()).clicked() {
                                        pending_action = Some(HierarchyAction::Reparent(entity.id, Some(pid)));
                                        ui.close();
                                    }
                                }
                            });
                        }

                        ui.separator();
                        if ui.button("📋 Duplicar").clicked() {
                            pending_action = Some(HierarchyAction::Duplicate(entity.id));
                            ui.close();
                        }
                        if ui.button("🗑 Eliminar").clicked() {
                            pending_action = Some(HierarchyAction::Delete(entity.id));
                            ui.close();
                        }
                        ui.separator();
                        ui.label("➕ Añadir:");
                        Self::render_creation_menu(ui, &mut pending_action);
                    });
                }

                // Espacio vacío restante del ScrollArea: arrastrar aquí desemparenta a la raíz
                let remaining = ui.available_size();
                if remaining.y > 10.0 {
                    let (_empty_rect, bg_resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), remaining.y),
                        egui::Sense::click_and_drag(),
                    );

                    // Si soltamos el arrastre en el fondo vacío, desemparentar el objeto a la raíz
                    if is_pointer_released && bg_resp.hovered() {
                        if let Some(drag_id) = self.dragging_entity.take() {
                            pending_action = Some(HierarchyAction::Reparent(drag_id, None));
                        }
                    }

                    bg_resp.context_menu(|ui| {
                        ui.label("➕ Crear GameObject");
                        ui.separator();
                        Self::render_creation_menu(ui, &mut pending_action);
                    });
                }
            });

        // Si se estaba arrastrando un objeto, gestionar el cursor y soltado
        if self.dragging_entity.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);

            // Dibujar el fantasma de la fila arrastrada PEGADO al
            // punto de agarre del ratón, en la capa superior del
            // contexto: el painter de panel recortaba el fantasma
            // al salir del panel y el desplazamiento fijo lo
            // «despegaba» de la punta del cursor (queja del
            // usuario el 2026-11-06).
            if let Some(drag_id) = self.dragging_entity {
                if let Some(mp) = mouse_pos {
                    if let Some(drag_ent) = scene.entities.iter().find(|e| e.id == drag_id) {
                        let painter = ui.ctx().layer_painter(egui::LayerId::new(
                            egui::Order::Foreground,
                            egui::Id::new("fantasma_jerarquia"),
                        ));
                        let ghost_rect = egui::Rect::from_min_size(
                            mp - self.drag_grab_offset,
                            egui::vec2(self.drag_row_width.max(80.0), 22.0),
                        );
                        let sel = ui.visuals().selection.bg_fill;
                        painter.rect_filled(
                            ghost_rect,
                            2.0,
                            Color32::from_rgba_unmultiplied(sel.r(), sel.g(), sel.b(), 225),
                        );
                        painter.rect_stroke(
                            ghost_rect,
                            2.0,
                            egui::Stroke::new(1.0, Color32::from_rgb(0, 160, 255)),
                            egui::StrokeKind::Inside,
                        );
                        painter.text(
                            ghost_rect.min + egui::vec2(10.0, 11.0),
                            egui::Align2::LEFT_CENTER,
                            format!("▣ {}", drag_ent.name),
                            egui::FontId::proportional(12.0),
                            Color32::WHITE,
                        );
                    }
                }
            }

            // Al soltar el botón del ratón, aplicar la acción de Drag & Drop
            if is_pointer_released {
                if let Some(drag_id) = self.dragging_entity.take() {
                    if let Some((target_id, dpos)) = current_frame_drop {
                        match dpos {
                            DropPosition::Inside => {
                                pending_action = Some(HierarchyAction::Reparent(drag_id, Some(target_id)));
                            }
                            DropPosition::Before => {
                                pending_action = Some(HierarchyAction::InsertRelative(drag_id, target_id, true));
                            }
                            DropPosition::After => {
                                pending_action = Some(HierarchyAction::InsertRelative(drag_id, target_id, false));
                            }
                        }
                    }
                }
            }
        }

        // Ejecución limpia de acciones sin colisiones de préstamos mutables
        if let Some(action) = pending_action {
            match action {
                HierarchyAction::Select(id) => {
                    self.selected_entity = Some(id);
                }
                HierarchyAction::MoveUp(id) => {
                    scene.move_entity_up(id);
                }
                HierarchyAction::MoveDown(id) => {
                    scene.move_entity_down(id);
                }
                HierarchyAction::InsertRelative(source_id, target_id, before) => {
                    scene.insert_relative(source_id, target_id, before);
                }
                HierarchyAction::Reparent(child_id, new_parent) => {
                    scene.reparent(child_id, new_parent);
                }
                HierarchyAction::Group(ids) => {
                    let gid = scene.group_entities(&ids, "GameObject Group");
                    self.selected_entity = Some(gid);
                }
                HierarchyAction::CreateChild(parent_id, preset) => {
                    let new_id = Self::instantiate_preset(&mut scene, preset);
                    scene.reparent(new_id, Some(parent_id));
                    self.selected_entity = Some(new_id);
                }
                HierarchyAction::Delete(id) => {
                    scene.delete_entity(id);
                    if self.selected_entity == Some(id) {
                        self.selected_entity = scene.entities.first().map(|e| e.id);
                    }
                }
                HierarchyAction::Duplicate(id) => {
                    if let Some(orig) = scene.get_entity(id).cloned() {
                        let new_id = scene.create_entity(
                            format!("{} (Clone)", orig.name),
                            orig.components.clone(),
                        );
                        if let Some(new_ent) = scene.get_entity_mut(new_id) {
                            new_ent.transform = orig.transform.clone();
                            new_ent.transform.position[0] += 1.0;
                            new_ent.mesh = orig.mesh.clone();
                            new_ent.script = orig.script.clone();
                            new_ent.script_data = orig.script_data.clone();
                            new_ent.physics = orig.physics.clone();
                            new_ent.audio = orig.audio.clone();
                            new_ent.camera = orig.camera.clone();
                            new_ent.light = orig.light.clone();
                            new_ent.parent = orig.parent;
                        }
                        if let Some(pid) = orig.parent {
                            if let Some(p) = scene.get_entity_mut(pid) {
                                p.children.push(new_id);
                            }
                        }
                        self.selected_entity = Some(new_id);
                    }
                }
                HierarchyAction::Create(preset) => {
                    let new_id = Self::instantiate_preset(&mut scene, preset);
                    self.selected_entity = Some(new_id);
                }
            }
        }
    }

    pub fn instantiate_preset(scene: &mut SceneState, preset: EntityPreset) -> u64 {
        match preset {
            EntityPreset::Group => {
                scene.create_entity("GameObject Group".into(), vec!["Transform".into()])
            }
            EntityPreset::Empty => {
                scene.create_entity("GameObject".into(), vec!["Transform".into()])
            }
            EntityPreset::Cube => {
                let id = scene.create_entity(
                    "Cube".into(),
                    vec![
                        "Transform".into(),
                        "MeshRenderer".into(),
                        "RigidBody".into(),
                        "Collider".into(),
                    ],
                );
                if let Some(ent) = scene.get_entity_mut(id) {
                    ent.transform.position = [0.0, 2.0, 0.0];
                    if let Some(m) = &mut ent.mesh {
                        m.color = [0.2, 0.7, 1.0];
                    }
                }
                id
            }
            EntityPreset::Sphere => {
                let id = scene.create_entity(
                    "Sphere".into(),
                    vec![
                        "Transform".into(),
                        "MeshRenderer".into(),
                        "RigidBody".into(),
                        "Collider".into(),
                    ],
                );
                if let Some(ent) = scene.get_entity_mut(id) {
                    ent.transform.position = [0.0, 2.0, 0.0];
                    if let Some(m) = &mut ent.mesh {
                        // La primitiva integrada (antes este preset
                        // dibujaba el cubo). Con el nombre puesto, la
                        // reconstrucción de física infiere Ball de la
                        // escala: collider acorde a la forma.
                        m.model = "sphere".into();
                        m.color = [0.9, 0.4, 0.2];
                    }
                }
                id
            }
            EntityPreset::Capsule => {
                let id = scene.create_entity(
                    "Capsule".into(),
                    vec![
                        "Transform".into(),
                        "MeshRenderer".into(),
                        "RigidBody".into(),
                        "Collider".into(),
                    ],
                );
                if let Some(ent) = scene.get_entity_mut(id) {
                    ent.transform.position = [0.0, 2.0, 0.0];
                    ent.transform.scale = [1.0, 2.0, 1.0];
                    if let Some(m) = &mut ent.mesh {
                        // Primitiva integrada; la escala [1,2,1] de
                        // siempre sobre la malla de alto 2 la deja en
                        // 4 m y la física infiere la cápsula acorde.
                        m.model = "capsule".into();
                        m.color = [0.3, 0.8, 0.4];
                    }
                }
                id
            }
            EntityPreset::Cylinder => {
                let id = scene.create_entity(
                    "Cylinder".into(),
                    vec![
                        "Transform".into(),
                        "MeshRenderer".into(),
                        "RigidBody".into(),
                        "Collider".into(),
                    ],
                );
                if let Some(ent) = scene.get_entity_mut(id) {
                    ent.transform.position = [0.0, 2.0, 0.0];
                    ent.transform.scale = [1.0, 2.0, 1.0];
                    if let Some(m) = &mut ent.mesh {
                        // Primitiva integrada; la escala [1,2,1] de
                        // siempre sobre la malla de alto 2 la deja en
                        // 4 m y la física infiere el cilindro acorde.
                        m.model = "cylinder".into();
                        m.color = [0.8, 0.8, 0.2];
                    }
                }
                id
            }
            EntityPreset::Plane => {
                let id = scene.create_entity(
                    "Plane".into(),
                    vec![
                        "Transform".into(),
                        "MeshRenderer".into(),
                        "StaticCollider".into(),
                    ],
                );
                if let Some(ent) = scene.get_entity_mut(id) {
                    ent.transform.position = [0.0, 0.0, 0.0];
                    // La malla "plane" es la primitiva integrada: rejilla
                    // 1×1 subdividida (sin grosor que escalar en Y, así
                    // que la escala Y queda en 1 y un desplazamiento de
                    // vértices futuro no se encoge). Huella de suelo
                    // 10×10 m como antes; el StaticCollider sale de
                    // box_extents y no depende de esta escala.
                    ent.transform.scale = [10.0, 1.0, 10.0];
                    if let Some(m) = &mut ent.mesh {
                        m.model = "plane".into();
                        m.color = [0.5, 0.5, 0.5];
                    }
                }
                id
            }
            EntityPreset::DirectionalLight => {
                let id = scene.create_entity(
                    "Directional Light".into(),
                    vec!["Transform".into(), "Light".into()],
                );
                if let Some(ent) = scene.get_entity_mut(id) {
                    ent.transform.position = [0.0, 10.0, 0.0];
                    // Signos migrados al unificar la convención de luz/cámara
                    // con la matriz de mundo: [-45, 30] produce hoy la misma
                    // dirección de sol que [45, -30] producía antes.
                    ent.transform.set_rotation_euler([-45.0, 30.0, 0.0]);
                }
                id
            }
            EntityPreset::PointLight => {
                let id = scene.create_entity(
                    "Point Light".into(),
                    vec!["Transform".into(), "Light".into()],
                );
                if let Some(ent) = scene.get_entity_mut(id) {
                    ent.transform.position = [0.0, 3.0, 0.0];
                }
                id
            }
            EntityPreset::AudioSource => {
                let id = scene.create_entity(
                    "Audio Source".into(),
                    vec!["Transform".into(), "AudioSource".into()],
                );
                if let Some(ent) = scene.get_entity_mut(id) {
                    ent.transform.position = [0.0, 1.0, 0.0];
                }
                id
            }
            EntityPreset::Camera => {
                let id = scene.create_entity(
                    "Camera".into(),
                    vec!["Transform".into(), "Camera".into()],
                );
                if let Some(ent) = scene.get_entity_mut(id) {
                    ent.transform.position = [0.0, 2.0, -5.0];
                }
                id
            }
            EntityPreset::Canvas => {
                scene.create_entity(
                    "Canvas".into(),
                    vec!["Transform".into(), "RectTransform".into(), "Canvas".into()],
                )
            }
            EntityPreset::UIText => {
                scene.create_entity(
                    "Text (TMP)".into(),
                    vec!["Transform".into(), "RectTransform".into(), "Text".into()],
                )
            }
            EntityPreset::UIButton => {
                let id = scene.create_entity(
                    "Button".into(),
                    vec!["Transform".into(), "RectTransform".into(), "Button".into(), "Text".into()],
                );
                if let Some(ent) = scene.get_entity_mut(id) {
                    if let Some(txt) = &mut ent.ui_text {
                        txt.text = "Button".into();
                    }
                }
                id
            }
        }
    }
}
