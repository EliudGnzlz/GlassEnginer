use egui::load::SizedTexture;
use egui::{vec2, Color32, Frame, Image, RichText, TextureId, Ui};
use glass_core::SceneState;
use std::sync::{Arc, Mutex};

use super::gizmo_3d::{self, GizmoAxis};

// Velocidad angular de órbita y mirada en rad por punto (la de Unity).
const ROT_SPEED: f32 = 0.003;
// Pitch casi cenital. El modelo esférico (look_at con up fijo [0,1,0])
// degenera en el polo, así que no se cruza (limitación aceptada).
const PITCH_LIMIT: f32 = 1.54;

/// Pivote y distancia para encuadrar un AABB en mundo con el FOV del editor
/// (45°). El radio es la mitad de la diagonal, así cabe sea cual sea su forma.
fn frame_aabb(min: [f32; 3], max: [f32; 3]) -> ([f32; 3], f32) {
    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let d = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    let radius = 0.5 * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let dist = (radius / 22.5_f32.to_radians().tan()).clamp(2.0, 100.0);
    (center, dist)
}

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

/// Formatos de aspecto y resolución para la ventana de Game estilo Unity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameResolution {
    FreeAspect,
    Aspect16_9,
    Aspect16_10,
    Aspect4_3,
    Res720p,
    Res1080p,
    Res1440p,
    Res4K,
}

impl GameResolution {
    pub fn name(self) -> &'static str {
        match self {
            Self::FreeAspect => "Free Aspect (Adaptable)",
            Self::Aspect16_9 => "16:9 Aspect",
            Self::Aspect16_10 => "16:10 Aspect",
            Self::Aspect4_3 => "4:3 Aspect",
            Self::Res720p => "1280x720 (720p)",
            Self::Res1080p => "1920x1080 (1080p)",
            Self::Res1440p => "2560x1440 (2K)",
            Self::Res4K => "3840x2160 (4K)",
        }
    }

    pub fn aspect_ratio(self) -> Option<f32> {
        match self {
            Self::FreeAspect => None,
            Self::Aspect16_9 | Self::Res720p | Self::Res1080p | Self::Res1440p | Self::Res4K => Some(16.0 / 9.0),
            Self::Aspect16_10 => Some(16.0 / 10.0),
            Self::Aspect4_3 => Some(4.0 / 3.0),
        }
    }
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
    GameView,  // Vista de juego con aspect ratio y cámara de jugador
}

pub struct ViewportPanel {
    pub width: f32, // Para compatibilidad
    pub height: f32,
    pub scene_width: f32,
    pub scene_height: f32,
    pub game_width: f32,
    pub game_height: f32,
    pub game_resolution: GameResolution,
    pub play_state: PlayState,
    pub gizmo_mode: GizmoMode,
    pub pivot_mode: PivotMode,
    pub coord_space: CoordSpace,
    pub active_tab: ViewportTab,
    // Cámara de editor
    pub editor_cam_yaw: f32,
    pub editor_cam_pitch: f32,
    pub editor_cam_dist: f32,
    pub editor_cam_center: [f32; 3],
    // Cámara activa seleccionada para Game View (multi-cámara)
    pub selected_camera_id: Option<u64>,
    // Eje activo actualmente arrastrado en el Gizmo 3D
    pub active_drag_axis: GizmoAxis,
    // Rect [x, y, ancho, alto] en puntos egui donde se pinta el juego en la
    // pestaña Game; None si aún no se ha dibujado.
    pub game_view_rect: Option<[f32; 4]>,
    // Velocidad de vuelo (u/s), independiente de la distancia al pivote,
    // como en Unity. Se ajusta con la rueda durante el vuelo.
    pub fly_speed: f32,
    // Vuelo en curso: derecho pulsado sobre el viewport y aún no soltado.
    // Sobrevive a que el cursor salga del rect.
    pub fly_active: bool,
    // Cotas locales de las mallas (clave → (min, max)) que la app
    // refresca desde el AssetCache antes de cada render: las usan el
    // picking y la caja de selección para trabajar con la geometría
    // real en vez de un cubo unidad supuesto.
    pub mesh_bounds: std::collections::HashMap<String, ([f32; 3], [f32; 3])>,
}

impl Default for ViewportPanel {
    fn default() -> Self {
        Self {
            width: 1280.0,
            height: 720.0,
            scene_width: 1280.0,
            scene_height: 720.0,
            game_width: 1280.0,
            game_height: 720.0,
            game_resolution: GameResolution::Aspect16_9,
            play_state: PlayState::Edit,
            gizmo_mode: GizmoMode::Translate,
            pivot_mode: PivotMode::Pivot,
            coord_space: CoordSpace::Global,
            active_tab: ViewportTab::SceneView,
            editor_cam_yaw: 0.0,
            editor_cam_pitch: 0.25,
            editor_cam_dist: 10.0,
            editor_cam_center: [3.5, 1.5, 0.0],
            selected_camera_id: None,
            active_drag_axis: GizmoAxis::None,
            game_view_rect: None,
            fly_speed: 10.0,
            fly_active: false,
            mesh_bounds: std::collections::HashMap::new(),
        }
    }
}

impl ViewportPanel {
    pub fn is_playing(&self) -> bool {
        self.play_state == PlayState::Playing
    }

    /// Renderiza la pestaña de Scene View (Edición 3D, Gizmos, PiP Preview y Picking en Edit y Play)
    pub fn render_scene_tab(
        &mut self,
        ui: &mut Ui,
        texture_id: Option<TextureId>,
        game_texture_id: Option<TextureId>,
        selected_entity_id: &mut Option<u64>,
        scene_state: &Arc<Mutex<SceneState>>,
    ) {
        self.active_tab = ViewportTab::SceneView;

        // Atajos Q, W, E, R, T, Y habilitados en SceneView tanto en Edit como en Play.
        // Con el derecho pulsado W/E/Q son teclas de vuelo: no cambian la herramienta.
        if !ui.memory(|m| m.focused().is_some()) && !ui.input(|i| i.pointer.secondary_down()) {
            ui.input(|i| {
                if i.key_pressed(egui::Key::Q) { self.gizmo_mode = GizmoMode::View; }
                if i.key_pressed(egui::Key::W) { self.gizmo_mode = GizmoMode::Translate; }
                if i.key_pressed(egui::Key::E) { self.gizmo_mode = GizmoMode::Rotate; }
                if i.key_pressed(egui::Key::R) { self.gizmo_mode = GizmoMode::Scale; }
                if i.key_pressed(egui::Key::T) { self.gizmo_mode = GizmoMode::Rect; }
                if i.key_pressed(egui::Key::Y) { self.gizmo_mode = GizmoMode::Transform; }
            });
        }

        ui.horizontal(|ui| {
            let btn_size = vec2(26.0, 22.0);
            let mut tool_btn = |ui: &mut Ui, mode: GizmoMode, icon: &str, tip: &str| {
                let selected = self.gizmo_mode == mode;
                let resp = ui.add_sized(
                    btn_size,
                    egui::Button::new(RichText::new(icon).size(13.0)).selected(selected),
                );
                if resp.clicked() {
                    self.gizmo_mode = mode;
                }
                resp.on_hover_text(tip);
            };

            tool_btn(ui, GizmoMode::View, "✋", "Mano [Q] - Pan de cámara");
            tool_btn(ui, GizmoMode::Translate, "✥", "Mover [W] - Trasladar");
            tool_btn(ui, GizmoMode::Rotate, "🔄", "Rotar [E] - Rotar");
            tool_btn(ui, GizmoMode::Scale, "⤢", "Escalar [R] - Escalar");
            tool_btn(ui, GizmoMode::Rect, "⊡", "Rect [T] - Rectángulo 2D");
            tool_btn(ui, GizmoMode::Transform, "✦", "Transform [Y] - Universal");

            ui.separator();

            if ui.button("⌖ Centrar").on_hover_text("Centrar cámara de escena").clicked() {
                self.editor_cam_center = [3.5, 1.5, 0.0];
                self.editor_cam_dist = 10.0;
                self.editor_cam_pitch = 0.25;
                self.editor_cam_yaw = 0.0;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("⎚ {:.0}x{:.0}", self.scene_width, self.scene_height));
            });
        });

        ui.separator();

        let available = ui.available_size();
        self.scene_width = available.x.max(64.0);
        self.scene_height = available.y.max(64.0);
        self.width = self.scene_width;
        self.height = self.scene_height;

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
        // Far 1000: con dist hasta 200 el pivote no debe quedar recortado.
        let proj = gizmo_3d::perspective_matrix(45.0_f32.to_radians(), (self.scene_width / self.scene_height).max(0.1), 0.1, 1000.0);
        let view_proj = gizmo_3d::matmul(&proj, &view);

        Frame::canvas(ui.style()).show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(self.scene_width, self.scene_height), egui::Sense::click_and_drag());
            let mouse_pos = ui.input(|i| i.pointer.hover_pos());

            if let Some(id) = texture_id {
                let sized_tex = SizedTexture::new(id, vec2(self.scene_width, self.scene_height));
                Image::from_texture(sized_tex).paint_at(ui, rect);
            }

            let painter = ui.painter();

            // Dibujar Gizmos de Cámaras y Luces de la escena (estilo Unity Scene View).
            // Las matrices de mundo se resuelven una sola vez por frame: los
            // gizmos, el picking y el bounding box deben coincidir exactamente
            // con la geometría que el renderer dibuja, incluidos los hijos.
            let gizmo_world_matrices = scene_state
                .lock()
                .map(|s| s.compute_world_matrices())
                .unwrap_or_default();

            if let Ok(state) = scene_state.lock() {
                for ent in &state.entities {
                    if !ent.active {
                        continue;
                    }
                    let is_sel = *selected_entity_id == Some(ent.id);
                    let world = gizmo_world_matrices
                        .get(&ent.id)
                        .copied()
                        .unwrap_or_else(|| {
                            glass_core::mat4_from_trs_euler(
                                ent.transform.position,
                                ent.transform.rotation(),
                                ent.transform.scale,
                            )
                        });

                    if ent.camera.is_some() {
                        gizmo_3d::draw_camera_gizmo(painter, ent, &world, &view_proj, rect, is_sel);
                    }
                    if ent.light.is_some() {
                        gizmo_3d::draw_light_gizmo(painter, ent, &world, &view_proj, rect, is_sel);
                    }
                }

                // Dibujar Bounding Box 3D del objeto seleccionado (funciona tanto en Edit como en Play)
                if let Some(target_id) = *selected_entity_id {
                    if let Some(ent) = state.entities.iter().find(|e| e.id == target_id) {
                        let is_pure_cam_or_light = (ent.camera.is_some() || ent.light.is_some()) && ent.mesh.is_none();
                        if !is_pure_cam_or_light {
                            let world = gizmo_world_matrices
                                .get(&ent.id)
                                .copied()
                                .unwrap_or_else(|| {
                                    glass_core::mat4_from_trs_euler(
                                        ent.transform.position,
                                        ent.transform.rotation(),
                                        ent.transform.scale,
                                    )
                                });
                            gizmo_3d::draw_selection_bounding_box(
                                painter,
                                ent,
                                &world,
                                &view_proj,
                                rect,
                                &self.mesh_bounds,
                            );
                        }
                    }
                }
            }

            // Dibujar Gizmo 3D interactivo si hay un objeto seleccionado
            let mut hovered_gizmo_axis = GizmoAxis::None;
            if self.gizmo_mode != GizmoMode::View {
                let selected_pos = if let Some(target_id) = *selected_entity_id {
                    // El gizmo va donde el objeto está en el mundo; para un hijo,
                    // su `position` local lo dibujaría descolocado.
                    scene_state.lock().ok().and_then(|s| {
                        if let Some(w) = s.compute_world_matrices().get(&target_id) {
                            Some(glass_core::mat4_translation(*w))
                        } else {
                            s.entities.iter().find(|e| e.id == target_id).map(|e| e.transform.position)
                        }
                    })
                } else {
                    None
                };

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

            // Gestión reactiva del cursor del ratón
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

            // Estado de vuelo. Se activa al PULSAR el derecho sobre el viewport.
            // drag_started no sirve porque espera el umbral de arrastre y no
            // dejaría volar sin mover el ratón. Dura hasta soltar el botón,
            // aunque el cursor salga del rect.
            let (secondary_pressed, secondary_held) = ui.input(|i| {
                (
                    i.pointer.button_pressed(egui::PointerButton::Secondary),
                    i.pointer.secondary_down(),
                )
            });
            if secondary_pressed && response.hovered() {
                self.fly_active = true;
            }
            if !secondary_held {
                self.fly_active = false;
            }

            // Rueda. Durante el vuelo ajusta su velocidad (Unity). Sin Alt
            // acerca hacia el pivote sin moverlo (Unity 3D). Con Alt acerca
            // hacia el cursor desplazando el centro por su rayo (Alt+rueda de Unity).
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 && self.fly_active {
                self.fly_speed = (self.fly_speed * (1.0 + scroll * 0.002)).clamp(0.5, 200.0);
                ui.ctx().request_repaint();
            } else if scroll.abs() > 0.0 && response.hovered() {
                let old_dist = self.editor_cam_dist;
                let new_dist = (old_dist * (1.0 - scroll * 0.001)).clamp(0.5, 200.0);
                let is_alt = ui.input(|i| i.modifiers.alt);
                if let Some(mp) = mouse_pos.filter(|_| is_alt) {
                    let aspect = (self.scene_width / self.scene_height).max(0.1);
                    let tan_half = 22.5_f32.to_radians().tan();
                    let ndc_x = 2.0 * (mp.x - rect.min.x) / rect.width() - 1.0;
                    let ndc_y = 1.0 - 2.0 * (mp.y - rect.min.y) / rect.height();
                    let fwd = [-cy * cx, -sy, -cy * cz];
                    let off_x = ndc_x * tan_half * aspect;
                    let off_y = ndc_y * tan_half;
                    let mut ray = [
                        fwd[0] + cam_right[0] * off_x + cam_up[0] * off_y,
                        fwd[1] + cam_right[1] * off_x + cam_up[1] * off_y,
                        fwd[2] + cam_right[2] * off_x + cam_up[2] * off_y,
                    ];
                    let len = (ray[0] * ray[0] + ray[1] * ray[1] + ray[2] * ray[2]).sqrt().max(1e-6);
                    ray = [ray[0] / len, ray[1] / len, ray[2] / len];
                    let paso = old_dist - new_dist;
                    self.editor_cam_center[0] += ray[0] * paso;
                    self.editor_cam_center[1] += ray[1] * paso;
                    self.editor_cam_center[2] += ray[2] * paso;
                }
                self.editor_cam_dist = new_dist;
                ui.ctx().request_repaint();
            }

            // Selección por Raycasting al hacer clic en Scene View (¡HABILITADO EN EDIT Y PLAY!)
            if response.clicked() && hovered_gizmo_axis == GizmoAxis::None && self.gizmo_mode != GizmoMode::View && !ui.input(|i| i.modifiers.alt) {
                if let Some(pos) = mouse_pos {
                    if let Ok(state) = scene_state.lock() {
                        let world_matrices = state.compute_world_matrices();
                        if let Some(hit_id) = gizmo_3d::pick_entity_at_screen(pos, &state.entities, &world_matrices, &view_proj, rect, eye, &self.mesh_bounds) {
                            *selected_entity_id = Some(hit_id);
                            log::info!("Objeto seleccionado en Scene View por Raycasting: #{}", hit_id);
                            ui.ctx().request_repaint();
                        } else {
                            // Clic en vacío: deselecciona, como Unity.
                            *selected_entity_id = None;
                            ui.ctx().request_repaint();
                        }
                    }
                }
            }

            // Arrastre e interactividad
            // Solo el izquierdo captura el gizmo: un pan o look que empieza
            // sobre él debe navegar, no manipular el objeto.
            if response.drag_started()
                && ui.input(|i| i.pointer.primary_down())
                && self.gizmo_mode != GizmoMode::View && hovered_gizmo_axis != GizmoAxis::None
                && !ui.input(|i| i.modifiers.alt) {
                    self.active_drag_axis = hovered_gizmo_axis;
                }

            if response.drag_stopped() {
                self.active_drag_axis = GizmoAxis::None;
            }

            if response.dragged() {
                let delta = response.drag_delta();
                let is_shift = ui.input(|i| i.modifiers.shift);
                let primary_down = ui.input(|i| i.pointer.primary_down());
                let secondary_down = ui.input(|i| i.pointer.secondary_down());
                let middle_down = ui.input(|i| i.pointer.middle_down());

                if self.active_drag_axis != GizmoAxis::None {
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
                                            GizmoAxis::X => ent.transform.add_rotation_euler([delta.y * 0.6, 0.0, 0.0]),
                                            GizmoAxis::Y => ent.transform.add_rotation_euler([0.0, delta.x * 0.6, 0.0]),
                                            GizmoAxis::Z => ent.transform.add_rotation_euler([0.0, 0.0, (delta.x - delta.y) * 0.6]),
                                            _ => ent.transform.add_rotation_euler([0.0, delta.x * 0.5, 0.0]),
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
                                            _ => ent.transform.add_rotation_euler([0.0, delta.x * 0.5, 0.0]),
                                        }
                                    }
                                    GizmoMode::View => {}
                                }
                            }
                        }
                    }
                    ui.ctx().request_repaint();
                } else {
                    let is_alt = ui.input(|i| i.modifiers.alt);
                    if is_alt && primary_down {
                        // Alt + izquierdo: orbitar alrededor del centro. Con
                        // ∂eye/∂yaw = +right, yaw -= dx hace que el objeto siga
                        // al cursor; pitch += dy sube el ojo con el ratón abajo.
                        self.editor_cam_yaw -= delta.x * ROT_SPEED;
                        self.editor_cam_pitch = (self.editor_cam_pitch + delta.y * ROT_SPEED).clamp(-PITCH_LIMIT, PITCH_LIMIT);
                        ui.ctx().request_repaint();
                    } else if is_alt && secondary_down {
                        // Alt + derecho: dolly por el eje dominante (niceMouseDelta
                        // de Unity). Derecha o abajo acercan. Shift ×3.
                        let v = if delta.x.abs() >= delta.y.abs() { delta.x } else { delta.y };
                        let boost = if is_shift { 3.0 } else { 1.0 };
                        self.editor_cam_dist = (self.editor_cam_dist * (1.0 - v * 0.005 * boost)).clamp(0.5, 200.0);
                        ui.ctx().request_repaint();
                    } else if middle_down || (primary_down && !is_alt && self.gizmo_mode == GizmoMode::View) {
                        // Central en cualquier modo, o izquierdo sin Alt con la herramienta Mano: pan.
                        // Mundo por punto = 2·d·tan(fov/2)/alto, así el plano del
                        // pivote sigue exactamente al cursor. Shift ×4.
                        let wpp = 2.0 * self.editor_cam_dist * 22.5_f32.to_radians().tan() / rect.height();
                        let pan = wpp * if is_shift { 4.0 } else { 1.0 };
                        self.editor_cam_center[0] += (-cam_right[0] * delta.x + cam_up[0] * delta.y) * pan;
                        self.editor_cam_center[1] += (-cam_right[1] * delta.x + cam_up[1] * delta.y) * pan;
                        self.editor_cam_center[2] += (-cam_right[2] * delta.x + cam_up[2] * delta.y) * pan;
                        ui.ctx().request_repaint();
                    } else if secondary_down && !is_alt {
                        // Derecho sin Alt: mirar desde el ojo sin moverlo (free look)
                        let old_yaw = self.editor_cam_yaw;
                        let old_pitch = self.editor_cam_pitch;
                        let dist = self.editor_cam_dist;
                        let eye = [
                            self.editor_cam_center[0] + dist * old_pitch.cos() * old_yaw.sin(),
                            self.editor_cam_center[1] + dist * old_pitch.sin(),
                            self.editor_cam_center[2] + dist * old_pitch.cos() * old_yaw.cos(),
                        ];
                        // Con fwd = −o, yaw -= dx mira a la derecha y pitch += dy mira abajo.
                        self.editor_cam_yaw -= delta.x * ROT_SPEED;
                        self.editor_cam_pitch = (self.editor_cam_pitch + delta.y * ROT_SPEED).clamp(-PITCH_LIMIT, PITCH_LIMIT);
                        let new_yaw = self.editor_cam_yaw;
                        let new_pitch = self.editor_cam_pitch;
                        self.editor_cam_center = [
                            eye[0] - dist * new_pitch.cos() * new_yaw.sin(),
                            eye[1] - dist * new_pitch.sin(),
                            eye[2] - dist * new_pitch.cos() * new_yaw.cos(),
                        ];
                        ui.ctx().request_repaint();
                    }
                }
            }

            // Vuelo estilo Unity: con el botón derecho mantenido, WASD/QE
            // mueven la cámara (Shift = rápido). Vive fuera del bloque de
            // arrastre: basta mantener el botón, sin mover el ratón.
            // Velocidad propia (fly_speed), no ligada a la distancia al pivote.
            if self.fly_active {
                let is_fast = ui.input(|i| i.modifiers.shift);
                let dt = ui.input(|i| i.stable_dt);
                let speed = self.fly_speed * if is_fast { 3.0 } else { 1.0 };
                let fwd = [-cy * cx, -sy, -cy * cz];
                let (w, s, a, d, e, q) = ui.input(|i| {
                    (
                        i.key_down(egui::Key::W) as i32 as f32,
                        i.key_down(egui::Key::S) as i32 as f32,
                        i.key_down(egui::Key::A) as i32 as f32,
                        i.key_down(egui::Key::D) as i32 as f32,
                        i.key_down(egui::Key::E) as i32 as f32,
                        i.key_down(egui::Key::Q) as i32 as f32,
                    )
                });
                let move_fwd = w - s;
                let move_right = d - a;
                let move_up = e - q;
                // fwd/right/up son ortonormales: normalizar la entrada iguala
                // la velocidad en diagonales. Q/E van por el up de la cámara.
                let len = (move_fwd * move_fwd + move_right * move_right + move_up * move_up).sqrt();
                if len > 0.0 {
                    let step = speed * dt / len;
                    self.editor_cam_center[0] += (fwd[0] * move_fwd + cam_right[0] * move_right + cam_up[0] * move_up) * step;
                    self.editor_cam_center[1] += (fwd[1] * move_fwd + cam_right[1] * move_right + cam_up[1] * move_up) * step;
                    self.editor_cam_center[2] += (fwd[2] * move_fwd + cam_right[2] * move_right + cam_up[2] * move_up) * step;
                    ui.ctx().request_repaint();
                }
            }

            // ── Menú contextual del Scene View (estilo Unity) ──────────────
            // En Unity, el clic derecho sobre la rejilla abre el menú de
            // creación y el objeto nace **bajo el cursor**, no en el origen.
            // Aquí se respeta ese comportamiento cuando el rayo corta el
            // plano Y=0; si no lo corta (cámara mirando al cielo) se cae al
            // origen para no perder la acción.
            let mut spawn_point: Option<[f32; 3]> = None;
            let mut spawn_request: Option<super::hierarchy::EntityPreset> = None;
            let mut frame_selected_request = false;
            // Tecla F: enfocar la entidad seleccionada (estilo Unity).
            if response.hovered()
                && selected_entity_id.is_some()
                && ui.input(|i| i.key_pressed(egui::Key::F))
            {
                frame_selected_request = true;
            }

            response.context_menu(|ui| {
                if let Some(pos) = mouse_pos {
                    spawn_point = gizmo_3d::screen_to_world_on_plane(pos, &view_proj, rect, 0.0);
                }
                if let Some(p) = spawn_point {
                    ui.label(
                        egui::RichText::new(format!("⌖ Crear en ({:.2}, {:.2}, {:.2})", p[0], p[1], p[2]))
                            .small()
                            .weak(),
                    );
                } else {
                    ui.label(egui::RichText::new("⌖ Crear en el origen").small().weak());
                }
                ui.separator();

                ui.menu_button("⬡ Objeto 3D", |ui| {
                    if ui.button("■ Cubo").clicked() {
                        spawn_request = Some(super::hierarchy::EntityPreset::Cube);
                        ui.close();
                    }
                    if ui.button("● Esfera").clicked() {
                        spawn_request = Some(super::hierarchy::EntityPreset::Sphere);
                        ui.close();
                    }
                    if ui.button("⬯ Cápsula").clicked() {
                        spawn_request = Some(super::hierarchy::EntityPreset::Capsule);
                        ui.close();
                    }
                    if ui.button("▭ Plano").clicked() {
                        spawn_request = Some(super::hierarchy::EntityPreset::Plane);
                        ui.close();
                    }
                });
                ui.menu_button("☼ Luz", |ui| {
                    if ui.button("☀ Direccional").clicked() {
                        spawn_request = Some(super::hierarchy::EntityPreset::DirectionalLight);
                        ui.close();
                    }
                    if ui.button("💡 Puntual").clicked() {
                        spawn_request = Some(super::hierarchy::EntityPreset::PointLight);
                        ui.close();
                    }
                });
                ui.menu_button("🖼 UI", |ui| {
                    if ui.button("▭ Canvas").clicked() {
                        spawn_request = Some(super::hierarchy::EntityPreset::Canvas);
                        ui.close();
                    }
                    if ui.button("🅰 Texto").clicked() {
                        spawn_request = Some(super::hierarchy::EntityPreset::UIText);
                        ui.close();
                    }
                    if ui.button("🔘 Botón").clicked() {
                        spawn_request = Some(super::hierarchy::EntityPreset::UIButton);
                        ui.close();
                    }
                });
                if ui.button("📦 Vacío").clicked() {
                    spawn_request = Some(super::hierarchy::EntityPreset::Empty);
                    ui.close();
                }

                ui.separator();
                if ui.button("🎯 Enfocar seleccionado [F]").clicked() {
                    frame_selected_request = true;
                    ui.close();
                }
                if ui.button("📷 Crear Main Camera").clicked() {
                    spawn_request = Some(super::hierarchy::EntityPreset::Camera);
                    ui.close();
                }
            });

            // Ejecutar la creación fuera del cierre del menú para no retener
            // el préstamo mutable de `scene_state` mientras egui dibuja.
            if let Some(preset) = spawn_request {
                if let Ok(mut state) = scene_state.lock() {
                    let new_id = super::hierarchy::HierarchyPanel::instantiate_preset(&mut state, preset);
                    if let Some(ent) = state.entities.iter_mut().find(|e| e.id == new_id) {
                        if let Some(p) = spawn_point {
                            // El plano se apoya en el suelo; el resto se eleva
                            // media unidad para no nacer enterrado.
                            let y = if matches!(preset, super::hierarchy::EntityPreset::Plane) {
                                p[1]
                            } else {
                                p[1] + 0.5
                            };
                            ent.transform.position = [p[0], y, p[2]];
                        }
                    }
                    *selected_entity_id = Some(new_id);
                    log::info!("Creado por menú contextual del Scene View: #{}", new_id);
                }
                ui.ctx().request_repaint();
            }

            // Soltado de assets desde el panel Proyecto; solo en Edit (en Play la escena es transitoria).
            if self.play_state == PlayState::Edit {
                if let Some(payload) =
                    response.dnd_hover_payload::<super::project::ArrastreAsset>()
                {
                    let nombre = payload
                        .ruta
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("asset");
                    painter.rect_stroke(
                        rect,
                        0.0,
                        egui::Stroke::new(2.0, Color32::from_rgb(80, 200, 120)),
                        egui::StrokeKind::Inside,
                    );
                    painter.text(
                        rect.min + egui::vec2(12.0, 16.0),
                        egui::Align2::LEFT_TOP,
                        format!("Soltar para instanciar: {nombre}"),
                        egui::FontId::proportional(13.0),
                        Color32::from_rgb(160, 255, 190),
                    );
                }

                if let Some(payload) =
                    response.dnd_release_payload::<super::project::ArrastreAsset>()
                {
                    let ruta = payload.ruta.clone();
                    let pos = ui
                        .input(|i| i.pointer.interact_pos())
                        .unwrap_or(rect.center());
                    let destino = gizmo_3d::screen_to_world_on_plane(pos, &view_proj, rect, 0.0)
                        .unwrap_or(self.editor_cam_center);
                    match scene_state.lock() {
                        Ok(mut state) => {
                            match super::project::instanciar_asset(&mut state, &ruta, destino) {
                                Ok(id) => {
                                    *selected_entity_id = Some(id);
                                    log::info!(
                                        "Asset '{}' instanciado como entidad {} en [{:.2}, {:.2}, {:.2}]",
                                        ruta.display(),
                                        id,
                                        destino[0],
                                        destino[1],
                                        destino[2]
                                    );
                                }
                                Err(e) => {
                                    log::warn!(
                                        "No se pudo instanciar '{}': {e}",
                                        ruta.display()
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!(
                                "No se pudo bloquear la escena para soltar '{}': {e}",
                                ruta.display()
                            );
                        }
                    }
                    ui.ctx().request_repaint();
                }
            }

            if frame_selected_request {
                if let Ok(state) = scene_state.lock() {
                    if let Some(id) = *selected_entity_id {
                        let world = state.compute_world_matrices();
                        if let Some(w) = world.get(&id) {
                            // Pivote al centro de los bounds en mundo y distancia
                            // según su tamaño. Los extents salen de la misma
                            // cuenta que draw_selection_bounding_box: medias
                            // extensiones = |escala de mundo| · 0.5 y las 8
                            // esquinas locales transformadas por la matriz.
                            let scale = glass_core::mat4_scale(*w);
                            let hx = (scale[0].abs() * 0.5).max(0.05);
                            let hy = (scale[1].abs() * 0.5).max(0.05);
                            let hz = (scale[2].abs() * 0.5).max(0.05);
                            let mut bmin = [f32::INFINITY; 3];
                            let mut bmax = [f32::NEG_INFINITY; 3];
                            for cx in [-hx, hx] {
                                for cy in [-hy, hy] {
                                    for cz in [-hz, hz] {
                                        let pw = glass_core::mat4_transform_point(*w, [cx, cy, cz]);
                                        for i in 0..3 {
                                            bmin[i] = bmin[i].min(pw[i]);
                                            bmax[i] = bmax[i].max(pw[i]);
                                        }
                                    }
                                }
                            }
                            let (center, dist) = frame_aabb(bmin, bmax);
                            self.editor_cam_center = center;
                            self.editor_cam_dist = dist;
                            log::info!("Cámara de escena enfocada en #{}", id);
                        }
                    }
                }
                ui.ctx().request_repaint();
            }

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
                    format!("⊡ [T] Rect: Modifica dimensiones 2D sobre '{}'", ent_name),
                    Color32::from_rgb(255, 180, 100),
                ),
                GizmoMode::Transform => (
                    format!("✦ [Y] Transform: Mover, rotar y escalar sobre '{}'", ent_name),
                    Color32::from_rgb(200, 130, 255),
                ),
                GizmoMode::View => (
                    "✋ [Q] Mano: Clic Izquierdo arrastra la vista libremente (Pan) | Clic Derecho: mirar (arrastrar) y volar con WASD/QE".to_string(),
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

            // PiP Preview flotante en la esquina inferior derecha si el GameObject seleccionado tiene Camera (estilo Unity Camera Preview)
            if let Some(sel_id) = *selected_entity_id {
                let cam_opt = scene_state.lock().ok().and_then(|s| {
                    s.entities.iter().find(|e| e.id == sel_id && e.camera.is_some()).cloned()
                });
                if let Some(cam_ent) = cam_opt {
                    let pip_size = vec2(220.0, 130.0);
                    let pip_pos = rect.right_bottom() - pip_size - vec2(14.0, 48.0);
                    let pip_rect = egui::Rect::from_min_size(pip_pos, pip_size);

                    painter.rect_filled(pip_rect.expand(2.0), 4.0, Color32::from_black_alpha(220));
                    painter.rect_stroke(
                        pip_rect.expand(2.0),
                        4.0,
                        egui::Stroke::new(1.5, Color32::from_rgb(80, 210, 255)),
                        egui::StrokeKind::Outside,
                    );

                    if let Some(gid) = game_texture_id {
                        let sized_tex = SizedTexture::new(gid, pip_size);
                        Image::from_texture(sized_tex).paint_at(ui, pip_rect);
                    }

                    // Encabezado de la previsualización PiP estilo Unity
                    let hdr_rect = egui::Rect::from_min_size(pip_rect.left_top(), vec2(pip_size.x, 22.0));
                    painter.rect_filled(hdr_rect, 3.0, Color32::from_rgba_unmultiplied(20, 25, 35, 230));
                    let cam_fov = cam_ent.camera.as_ref().map(|c| c.fov).unwrap_or(60.0);
                    let hdr_text = format!("🎥 Camera Preview: {} ({:.0}°)", cam_ent.name, cam_fov);
                    painter.text(
                        hdr_rect.left_top() + vec2(6.0, 3.0),
                        egui::Align2::LEFT_TOP,
                        hdr_text,
                        egui::FontId::proportional(11.0),
                        Color32::from_rgb(180, 230, 255),
                    );
                }
            }
        });
    }

    /// Renderiza la pestaña de Game View (Aspect Ratios de Unity, Letterboxing/Pillarboxing centrado)
    pub fn render_game_tab(
        &mut self,
        ui: &mut Ui,
        texture_id: Option<TextureId>,
        scene_state: &Arc<Mutex<SceneState>>,
    ) {
        self.active_tab = ViewportTab::GameView;

        ui.horizontal(|ui| {
            ui.label("Aspect:");
            ui.menu_button(self.game_resolution.name(), |ui| {
                let presets = [
                    GameResolution::FreeAspect,
                    GameResolution::Aspect16_9,
                    GameResolution::Aspect16_10,
                    GameResolution::Aspect4_3,
                    GameResolution::Res720p,
                    GameResolution::Res1080p,
                    GameResolution::Res1440p,
                    GameResolution::Res4K,
                ];
                for p in presets {
                    if ui.selectable_label(self.game_resolution == p, p.name()).clicked() {
                        self.game_resolution = p;
                        ui.close();
                    }
                }
            });

            // Selector de Cámara Activa en Game View (soporte de múltiples cámaras)
            if let Ok(state) = scene_state.lock() {
                let cameras: Vec<(u64, String)> = state.entities.iter()
                    .filter(|e| e.active && e.camera.is_some())
                    .map(|e| (e.id, e.name.clone()))
                    .collect();
                if cameras.len() > 1 {
                    ui.separator();
                    let current_label = self.selected_camera_id
                        .and_then(|id| cameras.iter().find(|(cid, _)| *cid == id).map(|(_, n)| format!("🎥 {}", n)))
                        .unwrap_or_else(|| "🎥 (Principal)".into());
                    ui.menu_button(current_label, |ui| {
                        for (cid, cname) in &cameras {
                            if ui.selectable_label(self.selected_camera_id == Some(*cid), format!("🎥 {}", cname)).clicked() {
                                self.selected_camera_id = Some(*cid);
                                ui.close();
                            }
                        }
                    });
                }
            }

            ui.separator();

            let controls_hint = RichText::new("🕹️ [W][A][S][D] / Flechas: Mover | [Espacio]: Saltar")
                .color(Color32::from_rgb(100, 200, 255));
            ui.label(controls_hint);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("▶ {:.0}x{:.0}", self.game_width, self.game_height));
            });
        });

        ui.separator();

        let container_size = ui.available_size();
        self.game_width = container_size.x.max(64.0);
        self.game_height = container_size.y.max(64.0);

        // Cálculo de letterbox / pillarbox centrado
        let (draw_size, offset) = if let Some(target_ratio) = self.game_resolution.aspect_ratio() {
            let draw_w = (container_size.y * target_ratio).min(container_size.x);
            let draw_h = draw_w / target_ratio;
            let offset_x = (container_size.x - draw_w) * 0.5;
            let offset_y = (container_size.y - draw_h) * 0.5;
            (vec2(draw_w, draw_h), vec2(offset_x, offset_y))
        } else {
            (container_size, vec2(0.0, 0.0))
        };

        let has_camera = scene_state
            .lock()
            .ok()
            .and_then(|s| s.active_camera().cloned())
            .is_some();

        Frame::canvas(ui.style()).show(ui, |ui| {
            let (rect, _response) = ui.allocate_exact_size(container_size, egui::Sense::hover());

            // Fondo para las barras negras de letterbox
            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(12, 12, 15));

            // Rectángulo centrado para el juego
            let game_rect = egui::Rect::from_min_size(rect.min + offset, draw_size);
            self.game_view_rect = Some([
                game_rect.min.x,
                game_rect.min.y,
                game_rect.width(),
                game_rect.height(),
            ]);

            if has_camera {
                if let Some(id) = texture_id {
                    let sized_tex = SizedTexture::new(id, draw_size);
                    Image::from_texture(sized_tex).paint_at(ui, game_rect);
                }
                // Renderizar Canvas UI sobre la pantalla de juego (Textos, Botones, etc.)
                Self::render_in_game_canvas_ui(ui, game_rect, scene_state, self.play_state == PlayState::Playing);
            } else {
                // Fondo oscuro cuando no hay cámara activa (Estilo Unity)
                ui.painter().rect_filled(game_rect, 0.0, Color32::from_rgb(20, 20, 26));

                // Cartel de advertencia centrado estilo Unity: "No cameras rendering"
                ui.painter().text(
                    game_rect.center() - vec2(0.0, 40.0),
                    egui::Align2::CENTER_CENTER,
                    "⚠️",
                    egui::FontId::proportional(34.0),
                    Color32::from_rgb(255, 195, 40),
                );
                ui.painter().text(
                    game_rect.center() - vec2(0.0, 8.0),
                    egui::Align2::CENTER_CENTER,
                    "No cameras rendering",
                    egui::FontId::proportional(18.0),
                    Color32::WHITE,
                );
                ui.painter().text(
                    game_rect.center() + vec2(0.0, 16.0),
                    egui::Align2::CENTER_CENTER,
                    "There are no cameras rendering to this display",
                    egui::FontId::proportional(12.0),
                    Color32::from_rgb(160, 160, 175),
                );

                // Botón interactivo para añadir una cámara principal de inmediato
                let btn_rect = egui::Rect::from_center_size(game_rect.center() + vec2(0.0, 52.0), vec2(210.0, 28.0));
                let resp = ui.put(
                    btn_rect,
                    egui::Button::new(RichText::new("➕ Crear Main Camera").strong().color(Color32::from_rgb(80, 220, 140))),
                );
                if resp.clicked() {
                    if let Ok(mut state) = scene_state.lock() {
                        let id = state.create_entity(
                            "Main Camera".into(),
                            vec!["Transform".into(), "Camera".into()],
                        );
                        if let Some(cam) = state.get_entity_mut(id) {
                            cam.transform.position = [3.5, 3.2, 9.5];
                            cam.transform.set_rotation_euler([10.0, 0.0, 0.0]);
                        }
                    }
                    ui.ctx().request_repaint();
                }
            }

            // Borde fino del viewport del juego
            ui.painter().rect_stroke(game_rect, 0.0, egui::Stroke::new(1.0, Color32::from_rgb(45, 45, 55)), egui::StrokeKind::Outside);
        });
    }

    /// Render unificado para compatibilidad
    pub fn render(
        &mut self,
        ui: &mut Ui,
        texture_id: Option<TextureId>,
        selected_entity_id: &mut Option<u64>,
        scene_state: &Arc<Mutex<SceneState>>,
    ) {
        match self.active_tab {
            ViewportTab::SceneView => self.render_scene_tab(ui, texture_id, None, selected_entity_id, scene_state),
            ViewportTab::GameView => self.render_game_tab(ui, texture_id, scene_state),
        }
    }

    /// Renderiza elementos de Canvas UI superpuestos en la vista de juego (Screen Space Overlay)
    fn render_in_game_canvas_ui(ui: &mut Ui, game_rect: egui::Rect, scene_state: &Arc<Mutex<SceneState>>, playing: bool) {
        if let Ok(scene) = scene_state.lock() {
            // Auditoría C7: la vista previa usa el MISMO resolvedor
            // que el runtime (`resolve_scene_ui_rects` con el tamaño
            // del viewport como pantalla): jerarquía, anclas con
            // estirado, layout groups y escala de canvas coinciden
            // con lo que se verá en juego. Antes había una fórmula
            // propia simplificada (solo anchor_min, sin padres) que
            // podía enseñar otra cosa.
            let rects = glass_core::resolve_scene_ui_rects(&scene, game_rect.width(), game_rect.height());
            for ent in &scene.entities {
                if !ent.active { continue; }
                if ent.rect_transform.is_some() {
                    let Some(r) = rects.get(&ent.id) else { continue };
                    let elem_rect = egui::Rect::from_min_size(
                        egui::pos2(game_rect.left() + r[0], game_rect.top() + r[1]),
                        egui::vec2(r[2], r[3]),
                    );

                    if let Some(btn) = &ent.ui_button {
                        let col = Color32::from_rgba_unmultiplied(
                            (btn.target_graphic_color[0] * 255.0) as u8,
                            (btn.target_graphic_color[1] * 255.0) as u8,
                            (btn.target_graphic_color[2] * 255.0) as u8,
                            (btn.target_graphic_color[3] * 255.0) as u8,
                        );
                        let label = ent.ui_text.as_ref().map(|t| t.text.as_str()).unwrap_or("Button");
                        if playing {
                            // En Play el clic lo despacha la ruta runtime del Canvas;
                            // aquí solo se pinta para no consumir el evento en egui.
                            let painter = ui.painter();
                            painter.rect_filled(elem_rect, 0.0, col);
                            painter.text(
                                elem_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                label,
                                egui::FontId::proportional(16.0),
                                Color32::WHITE,
                            );
                        } else {
                            let b = egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(col);
                            if ui.put(elem_rect, b).clicked() {
                                log::info!("[Canvas UI] Botón '{}' clickeado en GameView!", ent.name);
                            }
                        }
                    } else if let Some(txt) = &ent.ui_text {
                        let col = Color32::from_rgba_unmultiplied(
                            (txt.color[0] * 255.0) as u8,
                            (txt.color[1] * 255.0) as u8,
                            (txt.color[2] * 255.0) as u8,
                            (txt.color[3] * 255.0) as u8,
                        );
                        ui.painter().text(
                            elem_rect.min,
                            egui::Align2::LEFT_TOP,
                            &txt.text,
                            egui::FontId::proportional(txt.font_size.max(10.0)),
                            col,
                        );
                    }
                }
            }
        }
    }
}
