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

/// Formatea un conteo con puntos de miles (1.234.567), como se lee en español.
pub fn fmt_miles(n: u64) -> String {
    let digitos = n.to_string();
    let mut resultado = String::with_capacity(digitos.len() + digitos.len() / 3);
    for (indice, caracter) in digitos.chars().enumerate() {
        if indice > 0 && (digitos.len() - indice).is_multiple_of(3) {
            resultado.push('.');
        }
        resultado.push(caracter);
    }
    resultado
}

/// Contenido del desplegable «⏱ … ms ▾» de la vista de Escena (P8): los
/// tiempos del frame y la memoria que antes vivían en la pestaña
/// Frametime, más las estadísticas de render del último frame
/// recolectado, con los polígonos por vista.
fn menu_estadisticas_frame(ui: &mut Ui) {
    const MIB: f64 = 1024.0 * 1024.0;

    ui.set_min_width(240.0);

    let snap = glass_core::profiling::frame_timing_snapshot();
    let total_ms = snap.total.avg_us / 1000.0;
    let fps = if snap.total.avg_us > 0.0 {
        1_000_000.0 / snap.total.avg_us
    } else {
        0.0
    };
    ui.monospace(format!("FPS {fps:.0} · Total {total_ms:.2} ms"));

    for (nombre, avg_us) in [
        ("Update", snap.update.avg_us),
        ("Physics", snap.physics.avg_us),
        ("Render", snap.render.avg_us),
    ] {
        let avg_ms = avg_us / 1000.0;
        ui.monospace(format!("{nombre:<8} {avg_ms:6.2} ms"));
    }

    if snap.gpu_samples > 0 {
        let gpu_ms = snap.gpu_render.avg_us / 1000.0;
        let nombre = "GPU";
        ui.monospace(format!("{nombre:<8} {gpu_ms:6.2} ms"));
    } else {
        ui.monospace("GPU      sin datos");
    }

    let mem = glass_core::profiling::process_memory_snapshot();
    let mut linea = match mem.rss_bytes {
        Some(bytes) => {
            let rss_mib = bytes as f64 / MIB;
            format!("RSS {rss_mib:.1} MiB")
        }
        None => "RSS n/d".to_owned(),
    };
    if let Some(gpu) = glass_core::profiling::gpu_memory_snapshot() {
        let usado = gpu.allocated_bytes as f64 / MIB;
        let capacidad = gpu.capacity_bytes as f64 / MIB;
        linea.push_str(&format!(" · GPU {usado:.0}/{capacidad:.0} MiB"));
    }
    ui.monospace(linea);

    ui.separator();

    let stats = glass_core::render_stats::render_stats_snapshot();
    if stats.entities_considered == 0 {
        ui.monospace("Render sin datos (aún sin frames)");
        return;
    }

    ui.label(RichText::new("Render").strong());

    let dibujadas = stats
        .entities_considered
        .saturating_sub(stats.entities_culled);
    ui.monospace(format!(
        "Entidades {}/{} (descartadas {})",
        fmt_miles(u64::from(dibujadas)),
        fmt_miles(u64::from(stats.entities_considered)),
        fmt_miles(u64::from(stats.entities_culled)),
    ));

    let sombras = u64::from(stats.shadow_draw_calls) + u64::from(stats.graph_shadow_draw_calls);
    ui.monospace("Draw calls");
    ui.monospace(format!(
        "  Escena   {}",
        fmt_miles(u64::from(stats.scene_draw_calls))
    ));
    ui.monospace(format!(
        "  Juego    {}",
        fmt_miles(u64::from(stats.game_draw_calls))
    ));
    ui.monospace(format!("  Sombras  {}", fmt_miles(sombras)));
    ui.monospace(format!(
        "  Contorno {}",
        fmt_miles(u64::from(stats.outline_draw_calls))
    ));

    ui.monospace(format!(
        "Polígonos Escena {}",
        fmt_miles(stats.scene_triangles)
    ));
    ui.monospace(format!(
        "Polígonos Juego  {}",
        fmt_miles(stats.game_triangles)
    ));
    ui.monospace(format!(
        "Pipeline binds {}",
        fmt_miles(u64::from(stats.pipeline_binds))
    ));
    ui.monospace(format!(
        "Descriptor binds {}",
        fmt_miles(u64::from(stats.descriptor_binds))
    ));
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
    /// Preferencias del menú Gizmos, persistidas entre sesiones.
    pub gizmos: crate::gizmo_prefs::GizmoPrefs,
    // Cámara activa seleccionada para Game View (multi-cámara)
    pub selected_camera_id: Option<u64>,
    // Eje activo actualmente arrastrado en el Gizmo 3D
    pub active_drag_axis: GizmoAxis,
    // Eje bajo el puntero en el momento de PULSAR el botón primario: la
    // captura del arrastre se decide ahí, no cuando egui declara el inicio
    // del arrastre (con un eje muy escorzado, un primer movimiento brusco
    // saca el puntero del segmento antes de ese frame y la captura se
    // perdía en silencio).
    pub press_axis: GizmoAxis,
    // Rect [x, y, ancho, alto] en puntos egui donde se pinta el juego en la
    // pestaña Game; None si aún no se ha dibujado.
    pub game_view_rect: Option<[f32; 4]>,
    // Velocidad de vuelo (u/s), independiente de la distancia al pivote,
    // como en Unity. Se ajusta con la rueda durante el vuelo.
    pub fly_speed: f32,
    // Vuelo en curso: derecho pulsado sobre el viewport y aún no soltado.
    // Sobrevive a que el cursor salga del rect.
    pub fly_active: bool,
    // El puntero está sobre la vista de escena.
    pub scene_hovered: bool,
    /// Rect de la vista de escena en el último frame (contexto de
    /// Supr por clic en el docking).
    pub rect_escena: egui::Rect,
    // Cotas locales de las mallas (clave → (min, max)) que la app
    // refresca desde el AssetCache antes de cada render: las usan el
    // picking y la caja de selección para trabajar con la geometría
    // real en vez de un cubo unidad supuesto.
    pub mesh_bounds: std::collections::HashMap<String, ([f32; 3], [f32; 3])>,
    // Fuente .blend/.fbx soltada cuyo .glb falta o está desactualizado:
    // la app pide su conversión al vigía que corresponda y la vacía.
    pub pending_conversion: Option<std::path::PathBuf>,
    // .glb de un asset soltado que no necesita conversión (el propio
    // .glb o el gemelo ya al día de una fuente): la app lo carga en la
    // caché y la vacía. Sin esto, la entidad nacía con el cubo de
    // respaldo y nadie cargaba su modelo hasta recargar la escena.
    pub pending_carga: Option<std::path::PathBuf>,
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
            gizmos: crate::gizmo_prefs::GizmoPrefs::load(),
            selected_camera_id: None,
            active_drag_axis: GizmoAxis::None,
            press_axis: GizmoAxis::None,
            game_view_rect: None,
            fly_speed: 10.0,
            fly_active: false,
            scene_hovered: false,
            rect_escena: egui::Rect::NOTHING,
            mesh_bounds: std::collections::HashMap::new(),
            pending_conversion: None,
            pending_carga: None,
        }
    }
}

impl ViewportPanel {
    pub fn is_playing(&self) -> bool {
        self.play_state == PlayState::Playing
    }

    /// Tamaño en píxeles del icono de un componente a `dist` de la cámara
    /// del editor: base 24 px por la escala del usuario; con Iconos 3D
    /// además escala con la distancia (perspectiva, referencia 10 m).
    fn tam_icono(&self, dist: f32) -> f32 {
        let base = 24.0 * self.gizmos.icon_scale;
        if self.gizmos.icons_3d {
            (base * (10.0 / dist.max(0.1))).clamp(8.0, 96.0)
        } else {
            base
        }
    }

    /// Punto de mundo donde aterriza un asset soltado en `pos`: la
    /// superficie de la geometría bajo el cursor si la hay.
    fn punto_soltado(
        &self,
        pos: egui::Pos2,
        view_proj: &[f32; 16],
        rect: egui::Rect,
        scene_state: &Arc<Mutex<SceneState>>,
    ) -> Option<[f32; 3]> {
        let (origin, dir) = gizmo_3d::screen_ray(pos, view_proj, rect)?;
        let state = scene_state.lock().ok()?;
        let matrices = state.compute_world_matrices();
        gizmo_3d::punto_aterrizaje(&state.entities, &matrices, origin, dir, &self.mesh_bounds)
    }

    /// Renderiza la pestaña de Scene View (Edición 3D, Gizmos, PiP Preview y Picking en Edit y Play)
    pub fn render_scene_tab(
        &mut self,
        ui: &mut Ui,
        texture_id: Option<TextureId>,
        game_texture_id: Option<TextureId>,
        selected_entity_id: &mut Option<u64>,
        scene_state: &Arc<Mutex<SceneState>>,
        mut mesh: Option<(&mut super::mesh_tool::MeshToolPanel, u64)>,
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

        // Sin barra propia: «Centrar» vive en la barra principal;
        // la rejilla y las placas se controlan desde el botón «Gizmos»
        // superpuesto en la esquina de esta vista (orden del usuario,
        // 2026-11-18). La vista gana la franja entera para la escena.

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
        let malla_activa = mesh.is_some();

        Frame::canvas(ui.style()).show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(self.scene_width, self.scene_height), egui::Sense::click_and_drag());
            self.rect_escena = rect;
            self.scene_hovered = response.hovered();
            let mouse_pos = ui.input(|i| i.pointer.hover_pos());

            if let Some(id) = texture_id {
                let sized_tex = SizedTexture::new(id, vec2(self.scene_width, self.scene_height));
                Image::from_texture(sized_tex).paint_at(ui, rect);
            }

            // Botón «Gizmos» superpuesto (esquina superior derecha de la
            // vista): controla la rejilla, las placas de nombre y la
            // visibilidad de los gizmos de cada componente integrado.
            // Botón «⏱ … ms ▾» superpuesto (a la izquierda de «Gizmos»):
            // el tiempo del frame como desplegable con todas las
            // estadísticas dentro, incluidos los polígonos (P8; sustituye
            // a la antigua pestaña Frametime).
            let total_ms_boton =
                glass_core::profiling::frame_timing_snapshot().total.avg_us / 1000.0;
            egui::Area::new(egui::Id::new("frame_stats_escena"))
                .fixed_pos(rect.right_top() + egui::vec2(-226.0, 6.0))
                .show(ui.ctx(), |ui| {
                    // La etiqueta lleva el valor («⏱ 3405.34 ms ▾» con el
                    // motor frío): sin esto, egui la parte en dos líneas.
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.menu_button(format!("⏱ {total_ms_boton:.2} ms ▾"), |ui| {
                        menu_estadisticas_frame(ui);
                    });
                });

            let gizmos_antes = self.gizmos.clone();
            egui::Area::new(egui::Id::new("gizmos_escena"))
                .fixed_pos(rect.right_top() + egui::vec2(-96.0, 6.0))
                .show(ui.ctx(), |ui| {
                    ui.menu_button("Gizmos ▾", |ui| {
                        ui.set_min_width(280.0);
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.gizmos.icons_3d, "Iconos 3D");
                            ui.add(
                                egui::Slider::new(&mut self.gizmos.icon_scale, 0.25..=3.0)
                                    .text("Tamaño"),
                            );
                        });
                        ui.checkbox(&mut self.gizmos.show_grid, "Rejilla");
                        ui.checkbox(
                            &mut self.gizmos.show_nameplates,
                            "Nombres de objetos",
                        );
                        ui.checkbox(
                            &mut self.gizmos.selection_outline,
                            "Contorno de selección",
                        );
                        ui.checkbox(
                            &mut self.gizmos.colliders_always,
                            "Contornos de collider siempre",
                        );
                        ui.separator();
                        ui.label(egui::RichText::new("Componentes integrados").strong());
                        egui::Grid::new("gizmos_componentes")
                            .num_columns(3)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("").small().strong());
                                ui.label(egui::RichText::new("Icono").small().strong());
                                ui.label(egui::RichText::new("Gizmo").small().strong());
                                ui.end_row();

                                ui.label("Cámara");
                                ui.checkbox(&mut self.gizmos.icon_camera, "");
                                ui.checkbox(&mut self.gizmos.gizmo_camera, "");
                                ui.end_row();

                                ui.label("Luz");
                                ui.checkbox(&mut self.gizmos.icon_light, "");
                                ui.checkbox(&mut self.gizmos.gizmo_light, "");
                                ui.end_row();

                                ui.label("Collider");
                                ui.label(egui::RichText::new("—").weak());
                                ui.checkbox(&mut self.gizmos.gizmo_collider, "");
                                ui.end_row();

                                ui.label("Character Controller");
                                ui.label(egui::RichText::new("—").weak());
                                ui.checkbox(&mut self.gizmos.gizmo_character, "");
                                ui.end_row();
                            });
                    });
                });
            if self.gizmos != gizmos_antes {
                let _ = self.gizmos.save();
            }

            let painter = ui.painter();

            // La rejilla ya no se pinta aquí: la dibuja el renderer en
            // el pase 3D de la escena (con profundidad), subida por la
            // app con set_editor_grid_lines.

            // Dibujar Gizmos de Cámaras y Luces de la escena (estilo Unity Scene View).
            // Las matrices de mundo se resuelven una sola vez por frame: los
            // gizmos, el picking y el bounding box deben coincidir exactamente
            // con la geometría que el renderer dibuja, incluidos los hijos.
            let gizmo_world_matrices = scene_state
                .lock()
                .map(|s| s.compute_world_matrices())
                .unwrap_or_default();

            // Mundo e inversa de la entidad cuya malla se edita. Sin alguna
            // de las dos no hay overlay ni entrada de malla en este frame.
            let mesh_world_inv = mesh.as_ref().and_then(|(_, entity_id)| {
                gizmo_world_matrices
                    .get(entity_id)
                    .copied()
                    .or_else(|| {
                        scene_state.lock().ok().and_then(|s| {
                            s.entities.iter().find(|e| e.id == *entity_id).map(|ent| {
                                glass_core::mat4_from_trs_euler(
                                    ent.transform.position,
                                    ent.transform.rotation(),
                                    ent.transform.scale,
                                )
                            })
                        })
                    })
            }).and_then(|world| {
                glass_core::mat4_inverse_affine(world).map(|inv| (world, inv))
            });

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
                    let ent_pos = glass_core::mat4_translation(world);
                    let dist = ((eye[0] - ent_pos[0]).powi(2)
                        + (eye[1] - ent_pos[1]).powi(2)
                        + (eye[2] - ent_pos[2]).powi(2))
                    .sqrt();

                    if ent.camera.is_some()
                        && (self.gizmos.icon_camera
                            || (is_sel && self.gizmos.gizmo_camera))
                    {
                        gizmo_3d::draw_camera_gizmo(
                            painter,
                            ent,
                            &world,
                            &view_proj,
                            rect,
                            is_sel,
                            self.tam_icono(dist),
                            self.gizmos.icon_camera,
                            self.gizmos.gizmo_camera,
                        );
                    }
                    if ent.light.is_some()
                        && (self.gizmos.icon_light
                            || (is_sel && self.gizmos.gizmo_light))
                    {
                        gizmo_3d::draw_light_gizmo(
                            painter,
                            ent,
                            &world,
                            &view_proj,
                            rect,
                            is_sel,
                            self.tam_icono(dist),
                            self.gizmos.icon_light,
                            self.gizmos.gizmo_light,
                        );
                    }

                    // Los colliders se muestran siempre al seleccionar su
                    // entidad; la opción global permite inspeccionarlos todos.
                    if self.gizmos.gizmo_collider
                        && (is_sel || self.gizmos.colliders_always)
                    {
                        match gizmo_3d::collider_dibujo(
                            ent,
                            glass_core::mat4_scale(world),
                        ) {
                            gizmo_3d::ColliderDibujo::Formas(formas) => {
                                // Las formas ya vienen en tamaño de mundo
                                // (como las coloca la física): la pose de
                                // dibujo va SIN escala o se aplica dos veces.
                                gizmo_3d::draw_collider_gizmo(
                                    painter,
                                    &formas,
                                    &glass_core::mat4_sin_escala(world),
                                    &view_proj,
                                    rect,
                                    is_sel,
                                );
                            }
                            gizmo_3d::ColliderDibujo::Malla => {
                                let (lo, hi) =
                                    gizmo_3d::entity_local_bounds(ent, &self.mesh_bounds);
                                gizmo_3d::dibujar_caja(
                                    painter,
                                    lo,
                                    hi,
                                    &world,
                                    &view_proj,
                                    rect,
                                    egui::Color32::from_rgba_unmultiplied(
                                        90,
                                        255,
                                        130,
                                        if is_sel { 255 } else { 130 },
                                    ),
                                    if is_sel { 1.8 } else { 1.2 },
                                );
                            }
                            gizmo_3d::ColliderDibujo::MallaYFormas(formas) => {
                                // La caja de la malla (cotas locales, matriz
                                // completa) Y las formas de los specs (tamaño
                                // de mundo, pose sin escala), como la física.
                                let (lo, hi) =
                                    gizmo_3d::entity_local_bounds(ent, &self.mesh_bounds);
                                gizmo_3d::dibujar_caja(
                                    painter,
                                    lo,
                                    hi,
                                    &world,
                                    &view_proj,
                                    rect,
                                    egui::Color32::from_rgba_unmultiplied(
                                        90,
                                        255,
                                        130,
                                        if is_sel { 255 } else { 130 },
                                    ),
                                    if is_sel { 1.8 } else { 1.2 },
                                );
                                gizmo_3d::draw_collider_gizmo(
                                    painter,
                                    &formas,
                                    &glass_core::mat4_sin_escala(world),
                                    &view_proj,
                                    rect,
                                    is_sel,
                                );
                            }
                            gizmo_3d::ColliderDibujo::Ninguno => {}
                        }
                    }

                    if self.gizmos.gizmo_character
                        && (is_sel || self.gizmos.colliders_always)
                    {
                        if let Some(ch) = &ent.character {
                            gizmo_3d::draw_character_gizmo(
                                painter,
                                ch,
                                &glass_core::mat4_sin_escala(world),
                                &view_proj,
                                rect,
                                is_sel,
                            );
                        }
                    }
                }

                // Dibujar Bounding Box 3D del objeto seleccionado (funciona tanto en Edit como en Play)
                if self.gizmos.selection_outline {
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
                                gizmo_3d::draw_selection_outline(
                                    painter,
                                    ent,
                                    &world,
                                    &view_proj,
                                    rect,
                                    &self.mesh_bounds,
                                    self.gizmos.show_nameplates,
                                );
                            }
                        }
                    }
                }
            }

            // Overlay de la malla en edición, incluida la previsualización
            // del desplazamiento del gizmo o del agarre.
            if let (Some((panel, _)), Some((world, inv))) = (&mut mesh, mesh_world_inv) {
                if let Some(session) = panel.session.as_ref() {
                    let delta = if panel.view.gizmo_axis.is_some() {
                        panel.view.drag_delta_world
                    } else if let Some(g) = panel.view.grab.as_ref() {
                        g.delta_world
                    } else {
                        [0.0; 3]
                    };
                    let offset_local = if delta == [0.0; 3] {
                        [0.0; 3]
                    } else {
                        let origen_local = glass_core::mat4_transform_point(inv, [0.0; 3]);
                        let destino_local = glass_core::mat4_transform_point(inv, delta);
                        panel.snap_local([
                            destino_local[0] - origen_local[0],
                            destino_local[1] - origen_local[1],
                            destino_local[2] - origen_local[2],
                        ])
                    };
                    let sel_set: std::collections::HashSet<u32> =
                        panel.selection_vert_indices().into_iter().collect();
                    let project = |p_local| {
                        gizmo_3d::world_to_screen(
                            glass_core::mat4_transform_point(world, p_local),
                            &view_proj,
                            rect,
                        )
                    };
                    let depth = |p_local| {
                        let p = glass_core::mat4_transform_point(world, p_local);
                        let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
                        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                    };
                    painter.extend(super::mesh_view::overlay_shapes(
                        &session.mesh.verts,
                        &session.mesh.faces,
                        &panel.selected_faces,
                        &sel_set,
                        offset_local,
                        panel.centroide_seleccion(),
                        super::mesh_view::gizmo_len(&session.mesh.verts),
                        panel.view.gizmo_axis,
                        &project,
                        &depth,
                    ));
                }
            }

            // Dibujar Gizmo 3D interactivo si hay un objeto seleccionado.
            // La edición de malla suspende el gizmo de transform.
            let mut hovered_gizmo_axis = GizmoAxis::None;
            if !malla_activa && self.gizmo_mode != GizmoMode::View {
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

            // Entrada de la malla: agarre, gizmo propio y picking local.
            if let (Some((panel, _)), Some((world, inv))) = (&mut mesh, mesh_world_inv) {
                if panel.session.is_some() {
                    let (primary_pressed, primary_released, primary_down, is_alt, additive) =
                        ui.input(|i| {
                            (
                                i.pointer.button_pressed(egui::PointerButton::Primary),
                                i.pointer.button_released(egui::PointerButton::Primary),
                                i.pointer.primary_down(),
                                i.modifiers.alt,
                                i.modifiers.shift,
                            )
                        });
                    let normalizar = |v: [f32; 3]| {
                        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
                            .sqrt()
                            .max(1e-6);
                        [v[0] / len, v[1] / len, v[2] / len]
                    };
                    let a_local = |d: [f32; 3]| {
                        let cero = glass_core::mat4_transform_point(inv, [0.0; 3]);
                        let p = glass_core::mat4_transform_point(inv, d);
                        [p[0] - cero[0], p[1] - cero[1], p[2] - cero[2]]
                    };
                    let project_rel = |p_local| {
                        gizmo_3d::world_to_screen(
                            glass_core::mat4_transform_point(world, p_local),
                            &view_proj,
                            rect,
                        )
                        .map(|p| [p.x - rect.min.x, p.y - rect.min.y])
                    };

                    // Arranque del agarre sobre el plano perpendicular a la vista.
                    if panel.view.grab_requested && panel.view.grab.is_none() {
                        if let (Some(centroide_local), Some(pos)) =
                            (panel.centroide_seleccion(), mouse_pos)
                        {
                            let centroid_world =
                                glass_core::mat4_transform_point(world, centroide_local);
                            let normal = normalizar([
                                centroid_world[0] - eye[0],
                                centroid_world[1] - eye[1],
                                centroid_world[2] - eye[2],
                            ]);
                            if let Some((o_w, d_w)) =
                                gizmo_3d::screen_ray(pos, &view_proj, rect)
                            {
                                if let Some(start) = super::mesh_keymap::plane_point(
                                    o_w,
                                    d_w,
                                    centroid_world,
                                    normal,
                                ) {
                                    panel.view.grab = Some(super::mesh_view::GrabView {
                                        centroid_world,
                                        normal_world: normal,
                                        start_world: start,
                                        delta_world: [0.0; 3],
                                    });
                                    panel.view.grab_requested = false;
                                }
                            }
                        }
                    }

                    // Seguimiento del agarre sobre su plano original.
                    if let (Some(g), Some(pos)) = (panel.view.grab.as_mut(), mouse_pos) {
                        if let Some((o_w, d_w)) = gizmo_3d::screen_ray(pos, &view_proj, rect) {
                            if let Some(p) = super::mesh_keymap::plane_point(
                                o_w,
                                d_w,
                                g.centroid_world,
                                g.normal_world,
                            ) {
                                g.delta_world = [
                                    p[0] - g.start_world[0],
                                    p[1] - g.start_world[1],
                                    p[2] - g.start_world[2],
                                ];
                                ui.ctx().request_repaint();
                            }
                        }
                    }

                    // El eje del gizmo de malla se captura al pulsar.
                    if primary_pressed
                        && response.hovered()
                        && !is_alt
                        && panel.view.grab.is_none()
                    {
                        if let (Some(centroide_local), Some(pos), Some(session)) = (
                            panel.centroide_seleccion(),
                            mouse_pos,
                            panel.session.as_ref(),
                        ) {
                            if let Some(axis) = super::mesh_view::hit_gizmo_axis(
                                centroide_local,
                                super::mesh_view::gizmo_len(&session.mesh.verts),
                                [pos.x - rect.min.x, pos.y - rect.min.y],
                                &project_rel,
                            ) {
                                panel.view.gizmo_axis = Some(axis);
                                panel.view.gizmo_start = [pos.x, pos.y];
                                panel.view.drag_delta_world = [0.0; 3];
                            }
                        }
                    }

                    // Avance total sobre el eje transformado a mundo.
                    if let (Some(axis), Some(pos)) = (panel.view.gizmo_axis, mouse_pos) {
                        if primary_down {
                            if let Some(centroide_local) = panel.centroide_seleccion() {
                                let eje_local = match axis {
                                    0 => [1.0, 0.0, 0.0],
                                    1 => [0.0, 1.0, 0.0],
                                    _ => [0.0, 0.0, 1.0],
                                };
                                let origen_world =
                                    glass_core::mat4_transform_point(world, [0.0; 3]);
                                let extremo_world =
                                    glass_core::mat4_transform_point(world, eje_local);
                                let dir_world = normalizar([
                                    extremo_world[0] - origen_world[0],
                                    extremo_world[1] - origen_world[1],
                                    extremo_world[2] - origen_world[2],
                                ]);
                                let centroid_world =
                                    glass_core::mat4_transform_point(world, centroide_local);
                                let start = panel.view.gizmo_start;
                                let amount = gizmo_3d::avance_arrastre_eje(
                                    centroid_world,
                                    dir_world,
                                    &view_proj,
                                    rect,
                                    egui::vec2(pos.x - start[0], pos.y - start[1]),
                                );
                                panel.view.drag_delta_world = [
                                    dir_world[0] * amount,
                                    dir_world[1] * amount,
                                    dir_world[2] * amount,
                                ];
                                ui.ctx().request_repaint();
                            }
                        }
                    }

                    // Al soltar se consolida el arrastre en la sesión.
                    if primary_released && panel.view.gizmo_axis.is_some() {
                        panel.commit_drag_local(a_local(panel.view.drag_delta_world));
                        panel.view.gizmo_axis = None;
                        panel.view.drag_delta_world = [0.0; 3];
                        ui.ctx().request_repaint();
                    }

                    // Un clic confirma el agarre; sin agarre, selecciona malla.
                    if response.clicked() && !is_alt {
                        if let Some(g) = panel.view.grab.take() {
                            panel.commit_drag_local(a_local(g.delta_world));
                            ui.ctx().request_repaint();
                        } else if let Some(pos) = mouse_pos {
                            if let Some((o_w, d_w)) =
                                gizmo_3d::screen_ray(pos, &view_proj, rect)
                            {
                                let o_l = glass_core::mat4_transform_point(inv, o_w);
                                let fin_l = glass_core::mat4_transform_point(
                                    inv,
                                    [o_w[0] + d_w[0], o_w[1] + d_w[1], o_w[2] + d_w[2]],
                                );
                                let d_l = [
                                    fin_l[0] - o_l[0],
                                    fin_l[1] - o_l[1],
                                    fin_l[2] - o_l[2],
                                ];
                                let project_pick = |p_local| {
                                    let rel = project_rel(p_local)?;
                                    let p = glass_core::mat4_transform_point(world, p_local);
                                    let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
                                    let depth =
                                        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                                    Some((rel, depth))
                                };
                                panel.pick_viewport(
                                    o_l,
                                    d_l,
                                    [pos.x - rect.min.x, pos.y - rect.min.y],
                                    &project_pick,
                                    additive,
                                );
                                ui.ctx().request_repaint();
                            }
                        }
                    }
                }
            }

            // Selección por Raycasting al hacer clic en Scene View (¡HABILITADO EN EDIT Y PLAY!)
            if !malla_activa && response.clicked() && hovered_gizmo_axis == GizmoAxis::None && self.gizmo_mode != GizmoMode::View && !ui.input(|i| i.modifiers.alt) {
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
            // El eje se decide al PULSAR: si se espera a `drag_started()`,
            // un primer movimiento brusco puede haber sacado ya el puntero
            // de un eje muy escorzado (pocos píxeles en pantalla) y la
            // captura se pierde en silencio.
            if !malla_activa
                && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary))
                && !ui.input(|i| i.modifiers.alt)
            {
                self.press_axis = hovered_gizmo_axis;
            }
            if ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary)) {
                self.press_axis = GizmoAxis::None;
            }
            if !malla_activa
                && response.drag_started()
                && ui.input(|i| i.pointer.primary_down())
                && self.gizmo_mode != GizmoMode::View
                && !ui.input(|i| i.modifiers.alt) {
                    let eje = if hovered_gizmo_axis != GizmoAxis::None {
                        hovered_gizmo_axis
                    } else {
                        self.press_axis
                    };
                    if eje != GizmoAxis::None {
                        self.active_drag_axis = eje;
                    }
                }

            if response.drag_stopped() {
                self.active_drag_axis = GizmoAxis::None;
                self.press_axis = GizmoAxis::None;
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
                            // El gizmo se dibuja en el origen de MUNDO de la
                            // entidad y con los ejes de mundo: el arrastre se
                            // calcula en mundo (proyectando el delta de
                            // pantalla sobre la dirección en pantalla de cada
                            // eje, ver `gizmo_3d::avance_arrastre_eje`) y se
                            // convierte a la posición local al aplicar. El
                            // mapeo fijo anterior (X=delta.x, Y=-delta.y,
                            // Z=delta.x+delta.y) solo coincidía con la vista
                            // por defecto y al orbitar movía el objeto AL
                            // CONTRARIO de la flecha (reporte 2026-11-17).
                            let matrices_mundo = state.compute_world_matrices();
                            let pos_mundo = matrices_mundo
                                .get(&target_id)
                                .map(|m| glass_core::mat4_translation(*m));
                            let inv_padre = state
                                .entities
                                .iter()
                                .find(|e| e.id == target_id)
                                .and_then(|e| e.parent)
                                .and_then(|p| matrices_mundo.get(&p))
                                .and_then(|m| glass_core::mat4_inverse_affine(*m));
                            let avance = |dir: [f32; 3]| -> f32 {
                                match pos_mundo {
                                    Some(pm) => gizmo_3d::avance_arrastre_eje(
                                        pm,
                                        dir,
                                        &view_proj,
                                        rect,
                                        delta,
                                    ),
                                    None => 0.0,
                                }
                            };
                            let aplicar = |ent: &mut glass_core::EntityData, dm: [f32; 3]| {
                                match pos_mundo {
                                    Some(pm) => {
                                        let nuevo =
                                            [pm[0] + dm[0], pm[1] + dm[1], pm[2] + dm[2]];
                                        ent.transform.position = match inv_padre {
                                            Some(inv) => {
                                                glass_core::mat4_transform_point(inv, nuevo)
                                            }
                                            None => nuevo,
                                        };
                                    }
                                    None => {
                                        ent.transform.position[0] += dm[0];
                                        ent.transform.position[1] += dm[1];
                                        ent.transform.position[2] += dm[2];
                                    }
                                }
                            };
                            if let Some(ent) = state.entities.iter_mut().find(|e| e.id == target_id) {
                                match self.gizmo_mode {
                                    GizmoMode::Translate => {
                                        match self.active_drag_axis {
                                            GizmoAxis::X => {
                                                let a = avance([1.0, 0.0, 0.0]);
                                                aplicar(ent, [a, 0.0, 0.0]);
                                            }
                                            GizmoAxis::Y => {
                                                let a = avance([0.0, 1.0, 0.0]);
                                                aplicar(ent, [0.0, a, 0.0]);
                                            }
                                            GizmoAxis::Z => {
                                                let a = avance([0.0, 0.0, 1.0]);
                                                aplicar(ent, [0.0, 0.0, a]);
                                            }
                                            GizmoAxis::Center => {
                                                // Libre en el plano de la vista (con
                                                // Shift, en profundidad), a la escala
                                                // del objeto y no a la del pivote.
                                                let dist_obj = pos_mundo
                                                    .map(|pm| {
                                                        let d = [
                                                            pm[0] - eye[0],
                                                            pm[1] - eye[1],
                                                            pm[2] - eye[2],
                                                        ];
                                                        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
                                                            .sqrt()
                                                    })
                                                    .unwrap_or(self.editor_cam_dist);
                                                let wpp = 2.0
                                                    * dist_obj
                                                    * 22.5_f32.to_radians().tan()
                                                    / rect.height();
                                                let dm = if is_shift {
                                                    let mut fwd = [
                                                        self.editor_cam_center[0] - eye[0],
                                                        self.editor_cam_center[1] - eye[1],
                                                        self.editor_cam_center[2] - eye[2],
                                                    ];
                                                    let l = (fwd[0] * fwd[0]
                                                        + fwd[1] * fwd[1]
                                                        + fwd[2] * fwd[2])
                                                        .sqrt()
                                                        .max(1e-6);
                                                    fwd = [fwd[0] / l, fwd[1] / l, fwd[2] / l];
                                                    let k = -delta.y * wpp;
                                                    [fwd[0] * k, fwd[1] * k, fwd[2] * k]
                                                } else {
                                                    let kx = delta.x * wpp;
                                                    let ky = -delta.y * wpp;
                                                    [
                                                        cam_right[0] * kx + cam_up[0] * ky,
                                                        cam_right[1] * kx + cam_up[1] * ky,
                                                        cam_right[2] * kx + cam_up[2] * ky,
                                                    ]
                                                };
                                                aplicar(ent, dm);
                                            }
                                            GizmoAxis::None => {}
                                        }
                                    }
                                    GizmoMode::Rotate => {
                                        // El anillo persigue al cursor y el signo sale de la proyección
                                        // (el mapeo fijo anterior invertía el giro según el lado de la cámara; reporte 2026-11-18).
                                        let eje = match self.active_drag_axis {
                                            GizmoAxis::X => Some(0usize),
                                            GizmoAxis::Y => Some(1usize),
                                            GizmoAxis::Z => Some(2usize),
                                            _ => None,
                                        };
                                        match (eje, pos_mundo, mouse_pos) {
                                            (Some(eje), Some(origen), Some(raton)) => {
                                                let grados = gizmo_3d::angulo_arrastre_anillo(
                                                    origen,
                                                    eje,
                                                    &gizmo_3d::CamaraAnillo {
                                                        ojo: eye,
                                                        derecha: cam_right,
                                                        arriba: cam_up,
                                                        adelante: [-cy * cx, -sy, -cy * cz],
                                                        tan_mitad_fov: (22.5f32).to_radians().tan(),
                                                    },
                                                    rect,
                                                    raton,
                                                    delta,
                                                );
                                                let mut giro = [0.0f32; 3];
                                                giro[eje] = grados;
                                                ent.transform.add_rotation_euler(giro);
                                            }
                                            _ => ent.transform.add_rotation_euler([0.0, delta.x * 0.5, 0.0]),
                                        }
                                    }
                                    GizmoMode::Scale => {
                                        match self.active_drag_axis {
                                            GizmoAxis::X => ent.transform.scale[0] = (ent.transform.scale[0] + avance([1.0, 0.0, 0.0]) * 0.5).max(0.1),
                                            GizmoAxis::Y => ent.transform.scale[1] = (ent.transform.scale[1] + avance([0.0, 1.0, 0.0]) * 0.5).max(0.1),
                                            GizmoAxis::Z => ent.transform.scale[2] = (ent.transform.scale[2] + avance([0.0, 0.0, 1.0]) * 0.5).max(0.1),
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
                                            GizmoAxis::X => {
                                                let a = avance([1.0, 0.0, 0.0]);
                                                aplicar(ent, [a, 0.0, 0.0]);
                                            }
                                            GizmoAxis::Y => {
                                                let a = avance([0.0, 1.0, 0.0]);
                                                aplicar(ent, [0.0, a, 0.0]);
                                            }
                                            GizmoAxis::Z => {
                                                let a = avance([0.0, 0.0, 1.0]);
                                                aplicar(ent, [0.0, 0.0, a]);
                                            }
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
                    } else if middle_down || (primary_down && !is_alt && self.gizmo_mode == GizmoMode::View && !malla_activa) {
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
                    let destino = self
                        .punto_soltado(pos, &view_proj, rect, scene_state)
                        .unwrap_or_else(|| {
                            gizmo_3d::screen_to_world_on_plane(pos, &view_proj, rect, 0.0)
                                .unwrap_or(self.editor_cam_center)
                        });
                    match scene_state.lock() {
                        Ok(mut state) => {
                            match super::project::instanciar_asset(&mut state, &ruta, destino) {
                                Ok(id) => {
                                    *selected_entity_id = Some(id);
                                    // Si el .glb de la fuente no existe o está
                                    // desactualizado, la entidad nace con el
                                    // cubo de respaldo: se pide la conversión
                                    // y al llegar el modelo real la sustituye.
                                    if glass_renderer::watcher::necesita_conversion(&ruta) {
                                        self.pending_conversion = Some(ruta.clone());
                                    } else {
                                        // Sin conversión pendiente (el
                                        // asset ES el .glb o su gemelo ya
                                        // está al día): se pide la carga
                                        // directa de ese .glb, porque la
                                        // recarga bajo demanda de la app
                                        // solo cubre modelos expulsados y
                                        // la entidad se quedaría con el
                                        // cubo de respaldo.
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
                            // cuenta que draw_selection_outline: medias
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
            ViewportTab::SceneView => self.render_scene_tab(ui, texture_id, None, selected_entity_id, scene_state, None),
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

#[cfg(test)]
mod tests {
    use super::fmt_miles;

    #[test]
    fn fmt_miles_cero() {
        assert_eq!(fmt_miles(0), "0");
    }

    #[test]
    fn fmt_miles_sin_separador() {
        assert_eq!(fmt_miles(999), "999");
    }

    #[test]
    fn fmt_miles_con_separadores() {
        assert_eq!(fmt_miles(1_234_567), "1.234.567");
    }

    #[test]
    fn fmt_miles_mil() {
        assert_eq!(fmt_miles(1000), "1.000");
    }
}
