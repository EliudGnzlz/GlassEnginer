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
    SpotLight,
    AreaLight,
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
    /// Fuente .blend/.fbx soltada cuyo .glb falta o está desactualizado:
    /// la app pide su conversión al vigía que corresponda y la vacía.
    pub pending_conversion: Option<std::path::PathBuf>,
    /// .glb de un asset soltado que no necesita conversión (el propio
    /// .glb o el gemelo ya al día de una fuente): la app lo carga en la
    /// caché y la vacía, como en el viewport.
    pub pending_carga: Option<std::path::PathBuf>,
    /// Señal para el docking: se acaba de crear un Canvas y toca
    /// abrir la pestaña «Canvas / UI». El layout la consume y la
    /// devuelve a false.
    pub abrir_panel_canvas: bool,
    /// Rect del panel en el último frame renderizado (contexto de
    /// Supr por clic en el docking).
    pub rect_ultimo: egui::Rect,
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
            pending_conversion: None,
            pending_carga: None,
            abrir_panel_canvas: false,
            rect_ultimo: egui::Rect::NOTHING,
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
            if ui.button("⎚ Luz de Foco (Spot Light)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::SpotLight));
                ui.close();
            }
            if ui.button("▭ Luz de Área (Area Light)").clicked() {
                *pending_action = Some(HierarchyAction::Create(EntityPreset::AreaLight));
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
        // Rect del panel para detectar el soltado de assets del panel Proyecto.
        let panel_rect = ui.max_rect();
        self.rect_ultimo = panel_rect;
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
                        // La primera fila es la acción (criterio Unity:
                        // «Create Empty» abre el menú contextual de la
                        // jerarquía). Antes era un `ui.label` con pinta
                        // de ítem: al clicarlo el menú se cerraba sin
                        // crear nada (defecto reportado el 2026-11-18).
                        if ui.button("➕ Crear GameObject Vacío (Create Empty)").clicked() {
                            pending_action = Some(HierarchyAction::Create(EntityPreset::Empty));
                            ui.close();
                        }
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
                    self.nota_creacion(preset, new_id);
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
                    self.nota_creacion(preset, new_id);
                }
            }
        }

        // El arrastre interno de filas no lleva carga ArrastreAsset; soltar un
        // modelo/prefab sobre el panel lo instancia en el origen.
        let arrastre_asset =
            egui::DragAndDrop::payload::<super::project::ArrastreAsset>(ui.ctx());
        if let (Some(mp), Some(payload)) = (mouse_pos, arrastre_asset) {
            // Solo instanciables: un .physmat.json arrastrado va al campo
            // Material del Inspector, no a la jerarquía (E4).
            let nombre_payload = payload
                .ruta
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if panel_rect.contains(mp) && super::project::es_instanciable(nombre_payload, false) {
                ui.painter().rect_stroke(
                    panel_rect,
                    0.0,
                    egui::Stroke::new(2.0, Color32::from_rgb(80, 200, 120)),
                    egui::StrokeKind::Inside,
                );
                if is_pointer_released {
                    let ruta = payload.ruta.clone();
                    let _ = egui::DragAndDrop::take_payload::<super::project::ArrastreAsset>(
                        ui.ctx(),
                    );
                    match super::project::instanciar_asset(&mut scene, &ruta, [0.0, 0.0, 0.0]) {
                        Ok(id) => {
                            self.selected_entity = Some(id);
                            // Como en el viewport: si el .glb no está al
                            // día, se pide la conversión y el modelo real
                            // sustituye al cubo de respaldo al llegar.
                            if glass_renderer::watcher::necesita_conversion(&ruta) {
                                self.pending_conversion = Some(ruta.clone());
                            } else {
                                // Sin conversión pendiente: carga
                                // directa del .glb, como en el viewport.
                                let glb = if ruta
                                    .extension()
                                    .is_some_and(|e| e == "glb")
                                {
                                    ruta.clone()
                                } else {
                                    ruta.with_extension("glb")
                                };
                                if glb.exists() {
                                    self.pending_carga = Some(glb);
                                }
                            }
                            log::info!(
                                "Asset '{}' instanciado como entidad {} en el origen",
                                ruta.display(),
                                id
                            );
                        }
                        Err(e) => {
                            log::warn!("No se pudo instanciar '{}': {e}", ruta.display());
                        }
                    }
                }
            }
        }
    }

    /// Tras crear una entidad desde el menú: queda seleccionada y,
    /// si es un Canvas, se señala al docking que abra su pestaña.
    fn nota_creacion(&mut self, preset: EntityPreset, new_id: u64) {
        self.selected_entity = Some(new_id);
        if matches!(preset, EntityPreset::Canvas) {
            self.abrir_panel_canvas = true;
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
                    // 10×10 m como antes. El collider no depende de esta
                    // escala: su forma sale de box_extents (abajo).
                    ent.transform.scale = [10.0, 1.0, 10.0];
                    if let Some(m) = &mut ent.mesh {
                        m.model = "plane".into();
                        m.color = [0.5, 0.5, 0.5];
                    }
                    // Física: EntityData::new no crea PhysicsData sin
                    // "RigidBody" en los componentes, así que el
                    // StaticCollider del preset quedaba sin cuerpo: el
                    // suelo no colisionaba con nada. Se materializa
                    // aquí, estático y con las extensiones de la huella
                    // 10×10. El collider explícito va desplazado −0,5
                    // en Y para que su cara superior quede en y = 0,
                    // la superficie visible del plano: sin el
                    // desplazamiento la caja quedaría centrada en el
                    // origen y los objetos reposarían flotando 0,5 m
                    // por encima del suelo.
                    ent.physics = Some(glass_core::PhysicsData {
                        body_type: "Static".into(),
                        has_rigidbody: false,
                        has_collider: true,
                        box_extents: [5.0, 0.5, 5.0],
                        colliders: vec![glass_core::ColliderSpec {
                            shape: "Box".into(),
                            size: [5.0, 0.5, 5.0],
                            offset: [0.0, -0.5, 0.0],
                            is_trigger: false,
                            material: String::new(),
                            direction: "Y".to_string(),
                        }],
                        ..Default::default()
                    });
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
                    // El tipo hay que fijarlo: la entidad nace con
                    // LightData::default(), que es Directional.
                    if let Some(l) = ent.light.as_mut() {
                        l.light_type = "Point".into();
                    }
                }
                id
            }
            EntityPreset::SpotLight => {
                let id = scene.create_entity(
                    "Spot Light".into(),
                    vec!["Transform".into(), "Light".into()],
                );
                if let Some(ent) = scene.get_entity_mut(id) {
                    ent.transform.position = [0.0, 3.0, 0.0];
                    // El cono apunta a su forward (−Z); inclinado 45°
                    // hacia abajo ilumina el suelo frente a él.
                    ent.transform.set_rotation_euler([-45.0, 0.0, 0.0]);
                    if let Some(l) = ent.light.as_mut() {
                        l.light_type = "Spot".into();
                    }
                }
                id
            }
            EntityPreset::AreaLight => {
                let id = scene.create_entity(
                    "Area Light".into(),
                    vec!["Transform".into(), "Light".into()],
                );
                if let Some(ent) = scene.get_entity_mut(id) {
                    ent.transform.position = [0.0, 3.0, 0.0];
                    // El rectángulo emite por su forward (−Z); inclinado
                    // 45° hacia abajo, como el foco.
                    ent.transform.set_rotation_euler([-45.0, 0.0, 0.0]);
                    if let Some(l) = ent.light.as_mut() {
                        l.light_type = "Area".into();
                    }
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

#[cfg(test)]
mod preset_physics_tests {
    use super::*;

    #[test]
    fn primitiva_nueva_nace_solo_con_collider() {
        // Corrección de D3m3tr10 (2026-11-19): como en Unity, la
        // primitiva nace con Collider y SIN Rigidbody (estática); el
        // Rigidbody se añade a mano.
        let mut scene = glass_core::SceneState::default();
        let id = HierarchyPanel::instantiate_preset(&mut scene, EntityPreset::Cube);
        let ent = scene.get_entity(id).expect("la primitiva existe");
        assert!(!ent.components.iter().any(|c| c == "RigidBody"));
        assert!(ent.components.iter().any(|c| c == "Collider"));
        let phys = ent.physics.as_ref().expect("la primitiva trae física (collider)");
        assert!(!phys.has_rigidbody);
        assert!(phys.has_collider);
        assert_eq!(phys.body_type, "Static");
    }

    #[test]
    fn crear_canvas_levanta_la_senal_del_panel() {
        let mut panel = HierarchyPanel::default();
        panel.nota_creacion(EntityPreset::Canvas, 42);
        assert_eq!(panel.selected_entity, Some(42));
        assert!(panel.abrir_panel_canvas);
    }

    #[test]
    fn crear_otra_cosa_no_levanta_la_senal() {
        let mut panel = HierarchyPanel::default();
        panel.nota_creacion(EntityPreset::Cube, 7);
        assert_eq!(panel.selected_entity, Some(7));
        assert!(!panel.abrir_panel_canvas);
    }
}
