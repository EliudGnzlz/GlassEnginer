use egui::load::SizedTexture;
use egui::{vec2, Color32, Frame, Image, RichText, TextureId, Ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoMode {
    Translate,
    Rotate,
    Scale,
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
        }
    }
}

impl ViewportPanel {
    pub fn is_playing(&self) -> bool {
        self.play_state == PlayState::Playing
    }

    pub fn render(&mut self, ui: &mut Ui, texture_id: Option<TextureId>) {
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
                self.active_tab = ViewportTab::GameView; // Al dar Play, enfocar la vista Game
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
                    ui.label("Gizmo:");
                    ui.selectable_value(&mut self.gizmo_mode, GizmoMode::Translate, "✛ Mover");
                    ui.selectable_value(&mut self.gizmo_mode, GizmoMode::Rotate, "⟳ Rotar");
                    ui.selectable_value(&mut self.gizmo_mode, GizmoMode::Scale, "⤢ Escalar");

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

        Frame::canvas(ui.style()).show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(self.width, self.height), egui::Sense::drag());

            // Navegación con ratón para la cámara libre de SceneView
            if self.active_tab == ViewportTab::SceneView && response.dragged_by(egui::PointerButton::Secondary) {
                let delta = response.drag_delta();
                self.editor_cam_yaw += delta.x * 0.005;
                self.editor_cam_pitch = (self.editor_cam_pitch + delta.y * 0.005).clamp(-1.4, 1.4);
            }
            if self.active_tab == ViewportTab::SceneView && response.dragged_by(egui::PointerButton::Middle) {
                let delta = response.drag_delta();
                self.editor_cam_center[0] -= delta.x * 0.01;
                self.editor_cam_center[1] += delta.y * 0.01;
            }

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

                // Overlay identificador de ventana
                let painter = ui.painter_at(rect);
                match self.active_tab {
                    ViewportTab::SceneView => {
                        let tag_text = "🌐 VISTA DE ESCENA (MODO EDICIÓN - CÁMARA LIBRE)";
                        painter.rect_filled(
                            egui::Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(380.0, 24.0)),
                            4.0,
                            Color32::from_black_alpha(180),
                        );
                        painter.text(
                            rect.min + vec2(20.0, 16.0),
                            egui::Align2::LEFT_TOP,
                            tag_text,
                            egui::FontId::proportional(13.0),
                            Color32::from_rgb(180, 220, 255),
                        );
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
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Esperando enlace de textura Vulkan offscreen...");
                });
            }
        });
    }
}
