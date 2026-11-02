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
        // --- 0. Captura de Atajos Q, W, E, R estilo Unity (solo en SceneView y Edit) ---
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

            // Botón PLAY
            let play_text = if is_playing {
                RichText::new("▶ JUGANDO").color(Color32::from_rgb(20, 240, 100)).strong()
            } else {
                RichText::new("▶ Jugar")
            };
            if ui.selectable_label(is_playing, play_text).clicked() {
                self.play_state = PlayState::Playing;
                self.active_tab = ViewportTab::GameView;
            }

            // Botón PAUSE
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

            // Botón STOP
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

            // --- 3. Herramientas contextuales según la pestaña activa ---
            match self.active_tab {
                ViewportTab::SceneView => {
                    ui.label("Herramientas:");
                    ui.selectable_value(&mut self.gizmo_mode, GizmoMode::View, "✋ [Q] Mano");
                    ui.selectable_value(&mut self.gizmo_mode, GizmoMode::Translate, "✛ [W] Mover");
                    ui.selectable_value(&mut self.gizmo_mode, GizmoMode::Rotate, "⟳ [E] Rotar");
                    ui.selectable_value(&mut self.gizmo_mode, GizmoMode::Scale, "⤢ [R] Escalar");

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

        // Calcular matrices de la cámara libre del editor
        let eye = [
            self.editor_cam_center[0] + self.editor_cam_dist * self.editor_cam_pitch.cos() * self.editor_cam_yaw.sin(),
            self.editor_cam_center[1] + self.editor_cam_dist * self.editor_cam_pitch.sin(),
            self.editor_cam_center[2] + self.editor_cam_dist * self.editor_cam_pitch.cos() * self.editor_cam_yaw.cos(),
        ];
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

            // Dibujar la textura Vulkan offscreen en el centro
            if let Some(id) = texture_id {
                let aspect = 16.0 / 9.0;
                let mut display_w = self.width;
                let mut display_h = self.width / aspect;
                if display_h > self.height {
                    display_h = self.height;
                    display_w = self.height * aspect;
                }
                let x_pad = (self.width - display_w).max(0.0) / 2.0;
                let y_pad = (self.height - display_h).max(0.0) / 2.0;

                let image_rect = egui::Rect::from_min_size(
                    rect.min + vec2(x_pad, y_pad),
                    vec2(display_w, display_h),
                );

                let sized = SizedTexture::new(id, vec2(display_w, display_h));
                Image::new(sized).paint_at(ui, image_rect);
            }

            let painter = ui.painter_at(rect);

            // ==========================================
            // DIBUJAR GIZMO 3D ESTILO UNITY (LA CRUZ 3D)
            // ==========================================
            let mut hovered_gizmo_axis = GizmoAxis::None;

            if self.active_tab == ViewportTab::SceneView && self.play_state == PlayState::Edit {
                if let Some(pos_3d) = selected_pos {
                    hovered_gizmo_axis = gizmo_3d::render_3d_transform_gizmo(
                        &painter,
                        pos_3d,
                        &view_proj,
                        rect,
                        mouse_pos,
                        self.gizmo_mode,
                        self.active_drag_axis,
                    );
                }
            }

            // ==========================================
            // GESTIÓN DE EVENTOS DE RATÓN Y SELECCIÓN 3D
            // ==========================================
            if self.active_tab == ViewportTab::SceneView {
                // 1. Zoom con la rueda del ratón
                if response.hovered() {
                    let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll_delta.abs() > 0.0 {
                        self.editor_cam_dist = (self.editor_cam_dist - scroll_delta * 0.01).clamp(2.0, 50.0);
                    }
                }

                // 2. Paneo con botón central del ratón
                if response.dragged_by(egui::PointerButton::Middle) {
                    let delta = response.drag_delta();
                    self.editor_cam_center[0] -= delta.x * 0.01;
                    self.editor_cam_center[1] += delta.y * 0.01;
                }

                // 3. Órbita con botón secundario (derecho)
                if response.dragged_by(egui::PointerButton::Secondary) {
                    let delta = response.drag_delta();
                    self.editor_cam_yaw += delta.x * 0.005;
                    self.editor_cam_pitch = (self.editor_cam_pitch + delta.y * 0.005).clamp(-1.4, 1.4);
                }

                // 4. Clic primario: Seleccionar eje del Gizmo o Raycasting Picking sobre la escena 3D
                if response.drag_started_by(egui::PointerButton::Primary) {
                    if hovered_gizmo_axis != GizmoAxis::None {
                        self.active_drag_axis = hovered_gizmo_axis;
                    }
                }

                if response.clicked() && hovered_gizmo_axis == GizmoAxis::None {
                    // Clic sobre la escena 3D: Raycasting picking para seleccionar el objeto bajo el cursor
                    if let Some(click_pos) = response.interact_pointer_pos() {
                        if let Ok(scene) = scene_state.lock() {
                            if let Some(picked_id) = gizmo_3d::pick_entity_at_screen(
                                click_pos,
                                &scene.entities,
                                &view_proj,
                                rect,
                                eye,
                            ) {
                                *selected_entity_id = Some(picked_id);
                                log::info!("Objeto seleccionado en Scene View por Raycasting: #{}", picked_id);
                            }
                        }
                    }
                }

                // 5. Arrastre primario: Manipulación del objeto con la cruz de Unity
                if response.dragged_by(egui::PointerButton::Primary) {
                    let delta = response.drag_delta();
                    let is_shift = ui.input(|i| i.modifiers.shift);

                    if self.active_drag_axis != GizmoAxis::None {
                        if let Some(target_id) = *selected_entity_id {
                            if let Ok(mut scene) = scene_state.lock() {
                                if let Some(ent) = scene.entities.iter_mut().find(|e| e.id == target_id) {
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
                                            let factor = 1.0 + (delta.x - delta.y) * 0.01;
                                            match self.active_drag_axis {
                                                GizmoAxis::X => ent.transform.scale[0] = (ent.transform.scale[0] * (1.0 + delta.x * 0.01)).max(0.05),
                                                GizmoAxis::Y => ent.transform.scale[1] = (ent.transform.scale[1] * (1.0 - delta.y * 0.01)).max(0.05),
                                                GizmoAxis::Z => ent.transform.scale[2] = (ent.transform.scale[2] * (1.0 + delta.x * 0.01)).max(0.05),
                                                _ => {
                                                    ent.transform.scale[0] = (ent.transform.scale[0] * factor).max(0.05);
                                                    ent.transform.scale[1] = (ent.transform.scale[1] * factor).max(0.05);
                                                    ent.transform.scale[2] = (ent.transform.scale[2] * factor).max(0.05);
                                                }
                                            }
                                        }
                                        GizmoMode::View => {}
                                    }
                                }
                            }
                        }
                    } else if self.gizmo_mode == GizmoMode::View {
                        // En modo Mano [Q], el botón primario orbita la cámara
                        self.editor_cam_yaw += delta.x * 0.005;
                        self.editor_cam_pitch = (self.editor_cam_pitch + delta.y * 0.005).clamp(-1.4, 1.4);
                    }
                }

                // Al soltar el botón primario, liberar el eje del gizmo
                if !response.dragged() {
                    self.active_drag_axis = GizmoAxis::None;
                }
            }

            // ==========================================
            // OVERLAYS Y BADGES INFORMATIVOS EN PANTALLA
            // ==========================================
            match self.active_tab {
                ViewportTab::SceneView => {
                    let tag_text = "🌐 VISTA DE ESCENA (MODO EDICIÓN - RAYCASTING & GIZMOS ACTIVOS)";
                    painter.rect_filled(
                        egui::Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(460.0, 24.0)),
                        4.0,
                        Color32::from_black_alpha(190),
                    );
                    painter.text(
                        rect.min + vec2(20.0, 16.0),
                        egui::Align2::LEFT_TOP,
                        tag_text,
                        egui::FontId::proportional(13.0),
                        Color32::from_rgb(180, 220, 255),
                    );

                    // Banner inferior contextual
                    if let Some(ent_id) = *selected_entity_id {
                        let ent_name = scene_state.lock().ok()
                            .and_then(|s| s.entities.iter().find(|e| e.id == ent_id).map(|e| e.name.clone()))
                            .unwrap_or_else(|| format!("#{ent_id}"));

                        let (hint_text, hint_color) = match self.gizmo_mode {
                            GizmoMode::Translate => (
                                format!("✛ [W] Mover Activo: Arrastra la cruz 3D sobre '{}' (Ejes: X Rojo, Y Verde, Z Azul)", ent_name),
                                Color32::from_rgb(255, 110, 110),
                            ),
                            GizmoMode::Rotate => (
                                format!("⟳ [E] Rotar Activo: Arrastra los anillos de rotación sobre '{}'", ent_name),
                                Color32::from_rgb(110, 240, 130),
                            ),
                            GizmoMode::Scale => (
                                format!("⤢ [R] Escalar Activo: Arrastra los cubos de escala sobre '{}'", ent_name),
                                Color32::from_rgb(110, 190, 255),
                            ),
                            GizmoMode::View => (
                                format!("✋ [Q] Mano: Clic en objetos para seleccionarlos | Arrastre para orbitar cámara",),
                                Color32::from_rgb(240, 220, 110),
                            ),
                        };

                        let banner_rect = egui::Rect::from_min_size(
                            rect.left_bottom() + vec2(12.0, -36.0),
                            vec2(640.0, 24.0),
                        );
                        painter.rect_filled(banner_rect, 4.0, Color32::from_black_alpha(200));
                        painter.text(
                            banner_rect.min + vec2(8.0, 4.0),
                            egui::Align2::LEFT_TOP,
                            hint_text,
                            egui::FontId::proportional(12.5),
                            hint_color,
                        );
                    } else {
                        let hint_rect = egui::Rect::from_min_size(
                            rect.left_bottom() + vec2(12.0, -36.0),
                            vec2(440.0, 24.0),
                        );
                        painter.rect_filled(hint_rect, 4.0, Color32::from_black_alpha(190));
                        painter.text(
                            hint_rect.min + vec2(8.0, 4.0),
                            egui::Align2::LEFT_TOP,
                            "💡 Haz clic sobre cualquier objeto 3D en la escena para seleccionarlo e inspeccionarlo.",
                            egui::FontId::proportional(12.0),
                            Color32::from_rgb(220, 220, 230),
                        );
                    }
                }
                ViewportTab::GameView => {
                    let (tag_text, tag_color) = match self.play_state {
                        PlayState::Playing => ("🎮 VISTA DE JUEGO — EN EJECUCIÓN (WASD + ESPACIO ACTIVO)", Color32::from_rgb(50, 240, 120)),
                        PlayState::Paused => ("⏸ VISTA DE JUEGO — PAUSADO", Color32::from_rgb(255, 210, 40)),
                        PlayState::Edit => ("🎮 VISTA DE JUEGO (DETENIDO) — PULSA ▶ JUGAR PARA EMPEZAR", Color32::from_rgb(210, 215, 230)),
                    };

                    painter.rect_filled(
                        egui::Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(480.0, 24.0)),
                        4.0,
                        Color32::from_black_alpha(190),
                    );
                    painter.text(
                        rect.min + vec2(20.0, 16.0),
                        egui::Align2::LEFT_TOP,
                        tag_text,
                        egui::FontId::proportional(13.0),
                        tag_color,
                    );
                }
            }
        });
    }
}
