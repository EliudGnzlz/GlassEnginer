use egui::load::SizedTexture;
use egui::{vec2, Color32, Frame, Image, RichText, TextureId, Ui};
use glass_core::SceneState;
use std::sync::{Arc, Mutex};

use super::gizmo_3d::{self, GizmoAxis};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoMode {
    View,      // [Q] Mano (Pan / Órbita de cámara)
    Translate, // [W] Mover traslación (La cruz de Unity)
    Rotate,    // [E] Rotar (Anillos de rotación de Unity)
    Scale,     // [R] Escalar (Ejes con cubos de Unity)
    Rect,      // [T] Rect Tool
    Transform, // [Y] Universal Transform Tool
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PivotMode {
    Pivot,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordSpace {
    Global,
    Local,
}

/// Estado formal de ejecución del editor (Unity-style Play/Pause/Stop)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Edit,    // Modo edición (física y scripts en reposo de spawn)
    Playing, // Modo ejecución (simulación activa a 60 Hz y controles habilitados)
    Paused,  // Modo pausa (congelado en el frame actual para inspección)
}

/// Pestaña activa del viewport — desacopla la vista de edición Scene de la de gameplay Game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewportTab {
    SceneView, // Vista libre de edición con gizmos
    GameView,  // Vista de juego con aspect ratio 16:9 y cámara de jugador
}

pub struct ViewportPanel {
    pub width: f32,
    pub height: f32,
    pub play_state: PlayState,
    pub gizmo_mode: GizmoMode,
    pub pivot_mode: PivotMode,
    pub coord_space: CoordSpace,
    pub active_tab: ViewportTab,
    pub request_reset_to_spawn: bool,
    // Cámara de editor
    pub editor_cam_yaw: f32,
    pub editor_cam_pitch: f32,
    pub editor_cam_dist: f32,
    pub editor_cam_center: [f32; 3],
    // Eje activo actualmente arrastrado en el Gizmo 3D
    pub active_drag_axis: GizmoAxis,
}

impl Default for ViewportPanel {
    fn default() -> Self {
        Self {
            width: 1280.0,
            height: 720.0,
            play_state: PlayState::Edit,
            gizmo_mode: GizmoMode::Translate,
            pivot_mode: PivotMode::Pivot,
            coord_space: CoordSpace::Global,
            active_tab: ViewportTab::SceneView,
            request_reset_to_spawn: false,
            editor_cam_yaw: 0.0,
            editor_cam_pitch: 0.25,
            editor_cam_dist: 10.0,
            editor_cam_center: [3.5, 1.5, 0.0],
            active_drag_axis: GizmoAxis::None,
        }
    }
}

impl ViewportPanel {
    pub fn is_playing(&self) -> bool {
        self.play_state == PlayState::Playing
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        texture_id: Option<TextureId>,
        selected_entity_id: &mut Option<u64>,
        scene_state: &Arc<Mutex<SceneState>>,
    ) {
        // --- 0. Captura de Atajos Q, W, E, R, T, Y estilo Unity ---
        if self.active_tab == ViewportTab::SceneView && self.play_state == PlayState::Edit {
            if !ui.memory(|m| m.focused().is_some()) {
                ui.input(|i| {
                    if i.key_pressed(egui::Key::Q) {
                        self.gizmo_mode = GizmoMode::View;
                    }
                    if i.key_pressed(egui::Key::W) {
                        self.gizmo_mode = GizmoMode::Translate;
                    }
                    if i.key_pressed(egui::Key::E) {
                        self.gizmo_mode = GizmoMode::Rotate;
                    }
                    if i.key_pressed(egui::Key::R) {
                        self.gizmo_mode = GizmoMode::Scale;
                    }
                    if i.key_pressed(egui::Key::T) {
                        self.gizmo_mode = GizmoMode::Rect;
                    }
                    if i.key_pressed(egui::Key::Y) {
                        self.gizmo_mode = GizmoMode::Transform;
                    }
                });
            }
        }

        ui.horizontal(|ui| {
            // --- 1. Pestañas de Vista Estilo Unity (Scene vs Game) ---
            let scene_btn = ui.selectable_label(self.active_tab == ViewportTab::SceneView, "🌐 Escena (Scene View)");
            if scene_btn.clicked() {
                self.active_tab = ViewportTab::SceneView;
            }

            let game_btn = ui.selectable_label(self.active_tab == ViewportTab::GameView, "🎮 Juego (Game View)");
            if game_btn.clicked() {
                self.active_tab = ViewportTab::GameView;
            }

            ui.separator();

            // --- 2. Barra de Play / Pause / Stop Autoritativa ---
            let is_playing = self.play_state == PlayState::Playing;
            let is_paused = self.play_state == PlayState::Paused;
            let is_edit = self.play_state == PlayState::Edit;

            let play_text = if is_playing {
                RichText::new("▶ JUGANDO").color(Color32::from_rgb(20, 240, 100)).strong()
            } else {
                RichText::new("▶ Jugar")
            };
            if ui.selectable_label(is_playing, play_text).clicked() {
                self.play_state = PlayState::Playing;
                self.active_tab = ViewportTab::GameView;
            }

            let pause_text = if is_paused {
                RichText::new("⏸ PAUSADO").color(Color32::from_rgb(255, 200, 40)).strong()
            } else {
                RichText::new("⏸ Pausar")
            };
            if ui.selectable_label(is_paused, pause_text).clicked() {
                if is_playing {
                    self.play_state = PlayState::Paused;
                } else if is_paused {
                    self.play_state = PlayState::Playing;
                }
            }

            let stop_text = if is_edit {
                RichText::new("⏹ Detenido").color(Color32::GRAY)
            } else {
                RichText::new("⏹ Detener").color(Color32::LIGHT_RED).strong()
            };
            if ui.button(stop_text).clicked() {
                self.play_state = PlayState::Edit;
                self.request_reset_to_spawn = true;
            }

            ui.separator();

            // --- 3. Herramientas contextuales en iconos estilo Unity ---
            match self.active_tab {
                ViewportTab::SceneView => {
                    let btn_size = vec2(28.0, 22.0);
                    let mut tool_btn = |ui: &mut Ui, mode: GizmoMode, icon: &str, tip: &str| {
                        let selected = self.gizmo_mode == mode;
                        let resp = ui.add_sized(
                            btn_size,
                            egui::Button::new(RichText::new(icon).size(14.0)).selected(selected),
                        );
                        if resp.clicked() {
                            self.gizmo_mode = mode;
                        }
                        resp.on_hover_text(tip);
                    };

                    tool_btn(ui, GizmoMode::View, "✋", "Herramienta Mano [Q] - Pan de cámara");
                    tool_btn(ui, GizmoMode::Translate, "✥", "Herramienta Mover [W] - Trasladar");
                    tool_btn(ui, GizmoMode::Rotate, "🔄", "Herramienta Rotar [E] - Rotar");
                    tool_btn(ui, GizmoMode::Scale, "⤢", "Herramienta Escalar [R] - Escalar");
                    tool_btn(ui, GizmoMode::Rect, "⊡", "Herramienta Rect [T] - Rectángulo 2D/UI");
                    tool_btn(ui, GizmoMode::Transform, "✦", "Herramienta Transform [Y] - Universal");

                    ui.separator();

                    if ui.button("⌖ Centrar Vista").clicked() {
                        self.editor_cam_center = [3.5, 1.5, 0.0];
                        self.editor_cam_dist = 10.0;
                        self.editor_cam_pitch = 0.25;
                        self.editor_cam_yaw = 0.0;
                    }
                }
                ViewportTab::GameView => {
                    let controls_hint = RichText::new("🕹️ Controles: [W][A][S][D] / Flechas: Mover | [Espacio]: Saltar")
                        .color(Color32::from_rgb(100, 200, 255));
                    ui.label(controls_hint);
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("📐 {:.0}x{:.0}", self.width, self.height));
            });
        });

        ui.separator();

        let available = ui.available_size();
        self.width = available.x.max(64.0);
        self.height = available.y.max(64.0);

        // Calcular matrices y vectores directores de la cámara libre del editor
        let cy = self.editor_cam_pitch.cos();
        let sy = self.editor_cam_pitch.sin();
        let cx = self.editor_cam_yaw.sin();
        let cz = self.editor_cam_yaw.cos();

        let eye = [
            self.editor_cam_center[0] + self.editor_cam_dist * cy * cx,
            self.editor_cam_center[1] + self.editor_cam_dist * sy,
            self.editor_cam_center[2] + self.editor_cam_dist * cy * cz,
        ];
        let cam_right = [cz, 0.0, -cx];
        let cam_up = [-sy * cx, cy, -sy * cz];

        let view = gizmo_3d::look_at(eye, self.editor_cam_center, [0.0, 1.0, 0.0]);
        let proj = gizmo_3d::perspective_matrix(45.0_f32.to_radians(), (self.width / self.height).max(0.1), 0.1, 100.0);
        let view_proj = gizmo_3d::matmul(&proj, &view);

        Frame::canvas(ui.style()).show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(self.width, self.height), egui::Sense::click_and_drag());
            let mouse_pos = ui.input(|i| i.pointer.hover_pos());

            // Obtener posición del objeto seleccionado si existe
            let selected_pos = if let Some(target_id) = *selected_entity_id {
                scene_state.lock().ok().and_then(|s| {
                    s.entities.iter().find(|e| e.id == target_id).map(|e| e.transform.position)
                })
            } else {
                None
            };

            // Dibujar la textura Vulkan offscreen adaptada al 100% del área del Viewport
            if let Some(id) = texture_id {
                let sized_tex = SizedTexture::new(id, vec2(self.width, self.height));
                Image::from_texture(sized_tex).paint_at(ui, rect);
            }

            let painter = ui.painter();

            // En modo Edición y Scene View: Dibujar Bounding Box 3D del objeto seleccionado
            if self.active_tab == ViewportTab::SceneView && self.play_state == PlayState::Edit {
                if let Some(target_id) = *selected_entity_id {
                    if let Ok(state) = scene_state.lock() {
                        if let Some(ent) = state.entities.iter().find(|e| e.id == target_id) {
                            gizmo_3d::draw_selection_bounding_box(
                                painter,
                                ent,
                                &view_proj,
                                rect,
                            );
                        }
                    }
                }
            }

            // En modo Edición y Scene View: Dibujar Gizmo 3D interactivo si hay un objeto seleccionado
            let mut hovered_gizmo_axis = GizmoAxis::None;
            if self.active_tab == ViewportTab::SceneView && self.play_state == PlayState::Edit && self.gizmo_mode != GizmoMode::View {
                if let Some(pos) = selected_pos {
                    hovered_gizmo_axis = gizmo_3d::render_3d_transform_gizmo(
                        painter,
                        pos,
                        &view_proj,
                        rect,
                        mouse_pos,
                        self.gizmo_mode,
                        self.active_drag_axis,
                    );
                }
            }

            // Gestión reactiva del cursor del ratón según la herramienta activa
            if response.hovered() {
                if self.gizmo_mode == GizmoMode::View {
                    ui.ctx().set_cursor_icon(if response.dragged() {
                        egui::CursorIcon::Grabbing
                    } else {
                        egui::CursorIcon::Grab
                    });
                } else if hovered_gizmo_axis != GizmoAxis::None || self.active_drag_axis != GizmoAxis::None {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }

            // Zoom con rueda del ratón en Scene View
            if response.hovered() && self.active_tab == ViewportTab::SceneView {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll.abs() > 0.0 {
                    self.editor_cam_dist = (self.editor_cam_dist - scroll * 0.05).clamp(2.0, 50.0);
                    ui.ctx().request_repaint();
                }
            }

            // Arrastre e interactividad
            if response.drag_started() {
                if self.gizmo_mode != GizmoMode::View && hovered_gizmo_axis != GizmoAxis::None {
                    self.active_drag_axis = hovered_gizmo_axis;
                }
            }

            if response.drag_stopped() {
                self.active_drag_axis = GizmoAxis::None;
            }

            if response.clicked() && hovered_gizmo_axis == GizmoAxis::None && self.gizmo_mode != GizmoMode::View {
                if let Some(pos) = mouse_pos {
                    if let Ok(state) = scene_state.lock() {
                        if let Some(hit_id) = gizmo_3d::pick_entity_at_screen(pos, &state.entities, &view_proj, rect, eye) {
                            *selected_entity_id = Some(hit_id);
                            log::info!("Objeto seleccionado en Scene View por Raycasting: #{}", hit_id);
                            ui.ctx().request_repaint();
                        }
                    }
                }
            }

            if response.dragged() {
                let delta = response.drag_delta();
                let is_shift = ui.input(|i| i.modifiers.shift);
                let primary_down = ui.input(|i| i.pointer.primary_down());
                let secondary_down = ui.input(|i| i.pointer.secondary_down());
                let middle_down = ui.input(|i| i.pointer.middle_down());

                if self.active_drag_axis != GizmoAxis::None {
                    // Manipulación del Transform según el Gizmo activo
                    if let Some(target_id) = *selected_entity_id {
                        if let Ok(mut state) = scene_state.lock() {
                            if let Some(ent) = state.entities.iter_mut().find(|e| e.id == target_id) {
                                match self.gizmo_mode {
                                    GizmoMode::Translate => {
                                        match self.active_drag_axis {
                                            GizmoAxis::X => ent.transform.position[0] += delta.x * 0.02,
                                            GizmoAxis::Y => ent.transform.position[1] -= delta.y * 0.02,
                                            GizmoAxis::Z => ent.transform.position[2] += (delta.x + delta.y) * 0.02,
                                            GizmoAxis::Center => {
                                                if is_shift {
                                                    ent.transform.position[2] += delta.y * 0.02;
                                                } else {
                                                    ent.transform.position[0] += delta.x * 0.02;
                                                    ent.transform.position[1] -= delta.y * 0.02;
                                                }
                                            }
                                            GizmoAxis::None => {}
                                        }
                                    }
                                    GizmoMode::Rotate => {
                                        match self.active_drag_axis {
                                            GizmoAxis::X => ent.transform.rotation[0] += delta.y * 0.6,
                                            GizmoAxis::Y => ent.transform.rotation[1] += delta.x * 0.6,
                                            GizmoAxis::Z => ent.transform.rotation[2] += (delta.x - delta.y) * 0.6,
                                            _ => ent.transform.rotation[1] += delta.x * 0.5,
                                        }
                                    }
                                    GizmoMode::Scale => {
                                        match self.active_drag_axis {
                                            GizmoAxis::X => ent.transform.scale[0] = (ent.transform.scale[0] + delta.x * 0.02).max(0.1),
                                            GizmoAxis::Y => ent.transform.scale[1] = (ent.transform.scale[1] - delta.y * 0.02).max(0.1),
                                            GizmoAxis::Z => ent.transform.scale[2] = (ent.transform.scale[2] + delta.x * 0.02).max(0.1),
                                            GizmoAxis::Center => {
                                                let factor = (delta.x - delta.y) * 0.02;
                                                ent.transform.scale[0] = (ent.transform.scale[0] + factor).max(0.1);
                                                ent.transform.scale[1] = (ent.transform.scale[1] + factor).max(0.1);
                                                ent.transform.scale[2] = (ent.transform.scale[2] + factor).max(0.1);
                                            }
                                            GizmoAxis::None => {}
                                        }
                                    }
                                    GizmoMode::Rect => {
                                        match self.active_drag_axis {
                                            GizmoAxis::X => ent.transform.scale[0] = (ent.transform.scale[0] + delta.x * 0.02).max(0.1),
                                            GizmoAxis::Y => ent.transform.scale[1] = (ent.transform.scale[1] - delta.y * 0.02).max(0.1),
                                            _ => {}
                                        }
                                    }
                                    GizmoMode::Transform => {
                                        match self.active_drag_axis {
                                            GizmoAxis::X => ent.transform.position[0] += delta.x * 0.02,
                                            GizmoAxis::Y => ent.transform.position[1] -= delta.y * 0.02,
                                            GizmoAxis::Z => ent.transform.position[2] += (delta.x + delta.y) * 0.02,
                                            _ => ent.transform.rotation[1] += delta.x * 0.5,
                                        }
                                    }
                                    GizmoMode::View => {}
                                }
                            }
                        }
                    }
                    ui.ctx().request_repaint();
                } else if self.active_tab == ViewportTab::SceneView {
                    if self.gizmo_mode == GizmoMode::View {
                        if primary_down {
                            let pan_speed = self.editor_cam_dist * 0.0015;
                            self.editor_cam_center[0] -= (cam_right[0] * delta.x - cam_up[0] * delta.y) * pan_speed;
                            self.editor_cam_center[1] -= (cam_right[1] * delta.x - cam_up[1] * delta.y) * pan_speed;
                            self.editor_cam_center[2] -= (cam_right[2] * delta.x - cam_up[2] * delta.y) * pan_speed;
                            ui.ctx().request_repaint();
                        } else if secondary_down {
                            self.editor_cam_yaw += delta.x * 0.005;
                            self.editor_cam_pitch = (self.editor_cam_pitch - delta.y * 0.005).clamp(-1.4, 1.4);
                            ui.ctx().request_repaint();
                        }
                    } else {
                        if (primary_down && !is_shift) || secondary_down {
                            self.editor_cam_yaw += delta.x * 0.005;
                            self.editor_cam_pitch = (self.editor_cam_pitch - delta.y * 0.005).clamp(-1.4, 1.4);
                            ui.ctx().request_repaint();
                        } else if (primary_down && is_shift) || middle_down {
                            let pan_speed = self.editor_cam_dist * 0.0015;
                            self.editor_cam_center[0] -= (cam_right[0] * delta.x - cam_up[0] * delta.y) * pan_speed;
                            self.editor_cam_center[1] -= (cam_right[1] * delta.x - cam_up[1] * delta.y) * pan_speed;
                            self.editor_cam_center[2] -= (cam_right[2] * delta.x - cam_up[2] * delta.y) * pan_speed;
                            ui.ctx().request_repaint();
                        }
                    }
                }
            }

            // En Scene View: Mostrar banner de ayuda sobre la herramienta activa
            if self.active_tab == ViewportTab::SceneView && self.play_state == PlayState::Edit {
                let ent_name = selected_entity_id
                    .and_then(|id| {
                        scene_state.lock().ok().and_then(|s| {
                            s.entities.iter().find(|e| e.id == id).map(|e| e.name.clone())
                        })
                    })
                    .unwrap_or_else(|| "Ninguno".into());

                let (hint_text, hint_color) = match self.gizmo_mode {
                    GizmoMode::Translate => (
                        format!("✥ [W] Mover: Arrastra la cruz 3D sobre '{}' (Ejes: X Rojo, Y Verde, Z Azul)", ent_name),
                        Color32::from_rgb(255, 110, 110),
                    ),
                    GizmoMode::Rotate => (
                        format!("🔄 [E] Rotar: Arrastra los anillos de rotación sobre '{}'", ent_name),
                        Color32::from_rgb(110, 240, 130),
                    ),
                    GizmoMode::Scale => (
                        format!("⤢ [R] Escalar: Arrastra los cubos de escala sobre '{}'", ent_name),
                        Color32::from_rgb(110, 190, 255),
                    ),
                    GizmoMode::Rect => (
                        format!("⊡ [T] Rect: Modifica dimensiones 2D/UI sobre '{}'", ent_name),
                        Color32::from_rgb(255, 180, 100),
                    ),
                    GizmoMode::Transform => (
                        format!("✦ [Y] Transform: Mover, rotar y escalar sobre '{}'", ent_name),
                        Color32::from_rgb(200, 130, 255),
                    ),
                    GizmoMode::View => (
                        format!("✋ [Q] Mano: Clic Izquierdo arrastra la vista libremente (Pan) | Clic Derecho para orbitar"),
                        Color32::from_rgb(240, 220, 110),
                    ),
                };

                let banner_rect = egui::Rect::from_min_size(
                    rect.left_bottom() + vec2(12.0, -36.0),
                    vec2(720.0, 24.0),
                );
                painter.rect_filled(banner_rect, 4.0, Color32::from_black_alpha(200));
                painter.text(
                    banner_rect.min + vec2(8.0, 4.0),
                    egui::Align2::LEFT_TOP,
                    hint_text,
                    egui::FontId::proportional(12.0),
                    hint_color,
                );
            }
        });
    }
}
