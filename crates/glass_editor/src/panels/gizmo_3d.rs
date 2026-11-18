use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};
use glass_core::EntityData;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoAxis {
    None,
    Center,
    X,
    Y,
    Z,
}

pub fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let f = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
    let f_len = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt().max(0.0001);
    let f = [f[0] / f_len, f[1] / f_len, f[2] / f_len];

    let r = [
        f[1] * up[2] - f[2] * up[1],
        f[2] * up[0] - f[0] * up[2],
        f[0] * up[1] - f[1] * up[0],
    ];
    let r_len = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt().max(0.0001);
    let r = [r[0] / r_len, r[1] / r_len, r[2] / r_len];

    let u = [
        r[1] * f[2] - r[2] * f[1],
        r[2] * f[0] - r[0] * f[2],
        r[0] * f[1] - r[1] * f[0],
    ];

    [
        r[0], u[0], -f[0], 0.0,
        r[1], u[1], -f[1], 0.0,
        r[2], u[2], -f[2], 0.0,
        -(r[0] * eye[0] + r[1] * eye[1] + r[2] * eye[2]),
        -(u[0] * eye[0] + u[1] * eye[1] + u[2] * eye[2]),
        f[0] * eye[0] + f[1] * eye[1] + f[2] * eye[2],
        1.0,
    ]
}

pub fn perspective_matrix(fov_rad: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (fov_rad / 2.0).tan();
    [
        f / aspect, 0.0, 0.0, 0.0,
        0.0, -f, 0.0, 0.0, // Invertir Y para Vulkan
        0.0, 0.0, far / (near - far), -1.0,
        0.0, 0.0, (near * far) / (near - far), 0.0,
    ]
}

pub fn matmul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut res = [0.0; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            res[col * 4 + row] = sum;
        }
    }
    res
}

/// Longitud en mundo de cada eje del gizmo de transformación (la usan el
/// dibujo, el hit-test y el arrastre: tienen que coincidir).
pub const GIZMO_EJE_LEN: f32 = 1.4;

/// Avance en unidades de mundo a lo largo de un eje del gizmo cuando el ratón
/// se desplaza `delta` píxeles en pantalla: la proyección del delta sobre la
/// dirección EN PANTALLA del eje, a la escala (mundo/píxel) de ese eje a esa
/// profundidad. Arrastrar hacia la punta avanza en positivo y el objeto sigue
/// al cursor 1:1 sobre la línea del eje, desde cualquier punto de vista.
///
/// Existe porque el arrastre usaba un mapeo fijo pantalla→eje
/// (`X += delta.x`, `Y -= delta.y`, `Z += delta.x + delta.y`) que solo
/// coincide con la vista por defecto del editor: al orbitar, los ejes se
/// proyectan en otras direcciones y el objeto se movía AL CONTRARIO de la
/// flecha arrastrada (reporte de D3m3tr10, 2026-11-17). Si el eje apunta a la
/// cámara (proyección casi nula) no hay avance fiable y devuelve 0.
pub fn avance_arrastre_eje(
    origen: [f32; 3],
    dir: [f32; 3],
    vp: &[f32; 16],
    rect: Rect,
    delta: egui::Vec2,
) -> f32 {
    let (Some(p0), Some(p1)) = (
        world_to_screen(origen, vp, rect),
        world_to_screen(
            [
                origen[0] + dir[0] * GIZMO_EJE_LEN,
                origen[1] + dir[1] * GIZMO_EJE_LEN,
                origen[2] + dir[2] * GIZMO_EJE_LEN,
            ],
            vp,
            rect,
        ),
    ) else {
        return 0.0;
    };
    let v = p1 - p0;
    let len = v.length();
    if len < 1.0 {
        return 0.0;
    }
    let firmado_px = (delta.x * v.x + delta.y * v.y) / len;
    firmado_px * (GIZMO_EJE_LEN / len)
}

/// Proyecta una coordenada de mundo 3D a coordenadas 2D en pantalla dentro del rectángulo del Viewport.
pub fn world_to_screen(pos: [f32; 3], vp: &[f32; 16], rect: Rect) -> Option<Pos2> {
    let x = pos[0] * vp[0] + pos[1] * vp[4] + pos[2] * vp[8] + vp[12];
    let y = pos[0] * vp[1] + pos[1] * vp[5] + pos[2] * vp[9] + vp[13];
    let _z = pos[0] * vp[2] + pos[1] * vp[6] + pos[2] * vp[10] + vp[14];
    let w = pos[0] * vp[3] + pos[1] * vp[7] + pos[2] * vp[11] + vp[15];

    if w <= 0.05 {
        return None;
    }

    let ndc_x = x / w;
    let ndc_y = y / w;

    let screen_x = rect.min.x + (ndc_x * 0.5 + 0.5) * rect.width();
    let screen_y = rect.min.y + (ndc_y * 0.5 + 0.5) * rect.height();

    Some(pos2(screen_x, screen_y))
}

/// Distancia euclídea entre un punto 2D y un segmento de línea.
fn point_to_segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let ap = p - a;
    let ab_len_sq = ab.length_sq();
    if ab_len_sq < 1e-6 {
        return (p - a).length();
    }
    let t = (ap.dot(ab) / ab_len_sq).clamp(0.0, 1.0);
    let proj = a + ab * t;
    (p - proj).length()
}

/// Dibuja una flecha estilizada 3D con punta triangular en el canvas de egui.
fn draw_arrow(painter: &Painter, start: Pos2, end: Pos2, color: Color32, thickness: f32) {
    let dir = (end - start).normalized();
    let normal = vec2(-dir.y, dir.x);

    // Línea del eje
    painter.line_segment([start, end], Stroke::new(thickness, color));

    // Punta de flecha triangular
    let tip = end + dir * 4.0;
    let p1 = end - dir * 12.0 + normal * 5.5;
    let p2 = end - dir * 12.0 - normal * 5.5;
    painter.add(Shape::convex_polygon(vec![tip, p1, p2], color, Stroke::NONE));
}

/// Dibuja un eje de escala con un cubo sólido en el extremo.
fn draw_scale_axis(painter: &Painter, start: Pos2, end: Pos2, color: Color32, thickness: f32) {
    painter.line_segment([start, end], Stroke::new(thickness, color));
    let cube_rect = Rect::from_center_size(end, vec2(10.0, 10.0));
    painter.rect_filled(cube_rect, 1.5, color);
}

/// Puntos de un anillo de rotación de radio `radio` centrado en `origin`, en el plano
/// perpendicular al eje `eje` (0=X → plano YZ, 1=Y → plano XZ, 2=Z → plano XY).
/// Empieza en el primer eje del plano y no repite el primer punto al final.
pub fn anillo_rotacion_puntos(origin: [f32; 3], eje: usize, radio: f32, segmentos: usize) -> Vec<[f32; 3]> {
    let (a, b) = match eje {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    (0..segmentos)
        .map(|i| {
            let t = i as f32 / segmentos as f32 * std::f32::consts::TAU;
            let mut p = origin;
            p[a] += radio * t.cos();
            p[b] += radio * t.sin();
            p
        })
        .collect()
}

/// Dibuja el Gizmo 3D interactivo estilo Unity sobre la posición proyectada del objeto.
/// Retorna el eje que el ratón está apuntando (hover).
pub fn render_3d_transform_gizmo(
    painter: &Painter,
    origin_3d: [f32; 3],
    vp: &[f32; 16],
    rect: Rect,
    mouse_pos: Option<Pos2>,
    mode: crate::panels::viewport::GizmoMode,
    active_drag_axis: GizmoAxis,
) -> GizmoAxis {
    let Some(center_2d) = world_to_screen(origin_3d, vp, rect) else {
        return GizmoAxis::None;
    };

    let axis_len_3d = GIZMO_EJE_LEN;

    // Calcular puntas de los 3 ejes en espacio de mundo
    let p_x_world = [origin_3d[0] + axis_len_3d, origin_3d[1], origin_3d[2]];
    let p_y_world = [origin_3d[0], origin_3d[1] + axis_len_3d, origin_3d[2]];
    let p_z_world = [origin_3d[0], origin_3d[1], origin_3d[2] + axis_len_3d];

    let p_x_2d = world_to_screen(p_x_world, vp, rect);
    let p_y_2d = world_to_screen(p_y_world, vp, rect);
    let p_z_2d = world_to_screen(p_z_world, vp, rect);

    let (Some(tip_x), Some(tip_y), Some(tip_z)) = (p_x_2d, p_y_2d, p_z_2d) else {
        return GizmoAxis::None;
    };

    // Colores Unity
    let col_red = Color32::from_rgb(235, 75, 75);
    let col_green = Color32::from_rgb(75, 205, 100);
    let col_blue = Color32::from_rgb(70, 140, 245);
    let col_yellow = Color32::from_rgb(255, 230, 40);

    // Anillos de rotación 3D reales (solo en modo Rotate), radio en pantalla ≈ 45 px
    let anillos_2d: Option<[Vec<Option<Pos2>>; 3]> = if matches!(mode, crate::panels::viewport::GizmoMode::Rotate) {
        let mut px_por_unidad = [
            (tip_x - center_2d).length() / GIZMO_EJE_LEN,
            (tip_y - center_2d).length() / GIZMO_EJE_LEN,
            (tip_z - center_2d).length() / GIZMO_EJE_LEN,
        ];
        px_por_unidad.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mediana = px_por_unidad[1];
        let radio_mundo = if mediana.is_finite() && mediana > 1e-3 { 45.0 / mediana } else { 1.4 };
        Some([0usize, 1, 2].map(|eje| {
            anillo_rotacion_puntos(origin_3d, eje, radio_mundo, 64)
                .into_iter()
                .map(|p| world_to_screen(p, vp, rect))
                .collect()
        }))
    } else {
        None
    };

    // Hit-testing con el cursor del ratón
    let mut hovered_axis = GizmoAxis::None;

    if let Some(mouse) = mouse_pos {
        if active_drag_axis != GizmoAxis::None {
            hovered_axis = active_drag_axis;
        } else {
            match mode {
                crate::panels::viewport::GizmoMode::Rotate => {
                    // Distancia mínima del ratón a la polilínea proyectada de cada anillo
                    if let Some(anillos) = &anillos_2d {
                        let mut mejor_d = f32::INFINITY;
                        let mut mejor_eje = usize::MAX;
                        for (i, anillo) in anillos.iter().enumerate() {
                            let n = anillo.len();
                            for k in 0..n {
                                if let (Some(a), Some(b)) = (anillo[k], anillo[(k + 1) % n]) {
                                    let d = point_to_segment_distance(mouse, a, b);
                                    if d < mejor_d {
                                        mejor_d = d;
                                        mejor_eje = i;
                                    }
                                }
                            }
                        }
                        if mejor_d < 7.0 {
                            hovered_axis = match mejor_eje {
                                0 => GizmoAxis::X,
                                1 => GizmoAxis::Y,
                                2 => GizmoAxis::Z,
                                _ => GizmoAxis::None,
                            };
                        }
                    }
                }
                _ => {
                    let dist_center = (mouse - center_2d).length();
                    let dist_x = point_to_segment_distance(mouse, center_2d, tip_x);
                    let dist_y = point_to_segment_distance(mouse, center_2d, tip_y);
                    let dist_z = point_to_segment_distance(mouse, center_2d, tip_z);

                    if dist_center < 10.0 {
                        hovered_axis = GizmoAxis::Center;
                    } else if dist_x < 12.0 && dist_x <= dist_y && dist_x <= dist_z {
                        hovered_axis = GizmoAxis::X;
                    } else if dist_y < 12.0 && dist_y <= dist_x && dist_y <= dist_z {
                        hovered_axis = GizmoAxis::Y;
                    } else if dist_z < 12.0 {
                        hovered_axis = GizmoAxis::Z;
                    }
                }
            }
        }
    }

    use crate::panels::viewport::GizmoMode;

    match mode {
        GizmoMode::Translate => {
            // --- Eje X (Rojo) ---
            let is_x = hovered_axis == GizmoAxis::X;
            let c_x = if is_x { col_yellow } else { col_red };
            let w_x = if is_x { 4.0 } else { 2.5 };
            draw_arrow(painter, center_2d, tip_x, c_x, w_x);
            painter.text(tip_x + vec2(8.0, -4.0), egui::Align2::LEFT_TOP, "X", egui::FontId::proportional(12.0), c_x);

            // --- Eje Y (Verde) ---
            let is_y = hovered_axis == GizmoAxis::Y;
            let c_y = if is_y { col_yellow } else { col_green };
            let w_y = if is_y { 4.0 } else { 2.5 };
            draw_arrow(painter, center_2d, tip_y, c_y, w_y);
            painter.text(tip_y + vec2(-4.0, -18.0), egui::Align2::LEFT_TOP, "Y", egui::FontId::proportional(12.0), c_y);

            // --- Eje Z (Azul) ---
            let is_z = hovered_axis == GizmoAxis::Z;
            let c_z = if is_z { col_yellow } else { col_blue };
            let w_z = if is_z { 4.0 } else { 2.5 };
            draw_arrow(painter, center_2d, tip_z, c_z, w_z);
            painter.text(tip_z + vec2(-14.0, 6.0), egui::Align2::LEFT_TOP, "Z", egui::FontId::proportional(12.0), c_z);

            // --- Centro del Gizmo (Mover libremente) ---
            let is_c = hovered_axis == GizmoAxis::Center;
            let c_center = if is_c { col_yellow } else { Color32::WHITE };
            painter.circle_filled(center_2d, if is_c { 6.5 } else { 4.5 }, c_center);
            painter.circle_stroke(center_2d, 5.0, Stroke::new(1.0, Color32::BLACK));
        }

        GizmoMode::Rotate => {
            // Anillos de rotación Euler estilo Unity alrededor del objeto
            let r = 45.0_f32;
            let is_x = hovered_axis == GizmoAxis::X;
            let is_y = hovered_axis == GizmoAxis::Y;
            let is_z = hovered_axis == GizmoAxis::Z;

            // Anillos 3D reales: X en plano YZ (rojo), Y en plano XZ (verde), Z en plano XY (azul).
            // Amarillo y más grueso al hover.
            if let Some(anillos) = &anillos_2d {
                for (i, anillo) in anillos.iter().enumerate() {
                    let (hover, col) = match i {
                        0 => (is_x, col_red),
                        1 => (is_y, col_green),
                        _ => (is_z, col_blue),
                    };
                    let stroke = Stroke::new(if hover { 4.0 } else { 2.0 }, if hover { col_yellow } else { col });
                    let n = anillo.len();
                    for k in 0..n {
                        if let (Some(a), Some(b)) = (anillo[k], anillo[(k + 1) % n]) {
                            painter.line_segment([a, b], stroke);
                        }
                    }
                }
            }

            let label = match hovered_axis {
                GizmoAxis::X => "⟳ Rotar X (Pitch)",
                GizmoAxis::Y => "⟳ Rotar Y (Yaw)",
                GizmoAxis::Z => "⟳ Rotar Z (Roll)",
                _ => "⟳ Rotar [E]",
            };
            painter.text(
                center_2d + vec2(r * 1.15 + 8.0, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(12.0),
                if hovered_axis != GizmoAxis::None { col_yellow } else { Color32::from_rgb(220, 220, 230) },
            );
        }

        GizmoMode::Scale => {
            // Ejes de escala terminados en cubos
            let is_x = hovered_axis == GizmoAxis::X;
            let c_x = if is_x { col_yellow } else { col_red };
            draw_scale_axis(painter, center_2d, tip_x, c_x, if is_x { 4.0 } else { 2.5 });

            let is_y = hovered_axis == GizmoAxis::Y;
            let c_y = if is_y { col_yellow } else { col_green };
            draw_scale_axis(painter, center_2d, tip_y, c_y, if is_y { 4.0 } else { 2.5 });

            let is_z = hovered_axis == GizmoAxis::Z;
            let c_z = if is_z { col_yellow } else { col_blue };
            draw_scale_axis(painter, center_2d, tip_z, c_z, if is_z { 4.0 } else { 2.5 });

            let is_c = hovered_axis == GizmoAxis::Center;
            let c_center = if is_c { col_yellow } else { Color32::WHITE };
            painter.rect_filled(Rect::from_center_size(center_2d, vec2(8.0, 8.0)), 1.0, c_center);
        }

        GizmoMode::Rect => {
            let is_x = hovered_axis == GizmoAxis::X;
            let is_y = hovered_axis == GizmoAxis::Y;
            let c_x = if is_x { col_yellow } else { col_red };
            let c_y = if is_y { col_yellow } else { col_green };
            draw_arrow(painter, center_2d, tip_x, c_x, if is_x { 3.5 } else { 2.0 });
            draw_arrow(painter, center_2d, tip_y, c_y, if is_y { 3.5 } else { 2.0 });
            let rect_box = Rect::from_min_max(center_2d, tip_x + (tip_y - center_2d));
            painter.rect_stroke(rect_box, 1.0, Stroke::new(1.0, Color32::from_rgb(200, 200, 255)), egui::StrokeKind::Outside);
        }

        GizmoMode::Transform => {
            let is_x = hovered_axis == GizmoAxis::X;
            let is_y = hovered_axis == GizmoAxis::Y;
            let is_z = hovered_axis == GizmoAxis::Z;
            let c_x = if is_x { col_yellow } else { col_red };
            let c_y = if is_y { col_yellow } else { col_green };
            let c_z = if is_z { col_yellow } else { col_blue };

            draw_arrow(painter, center_2d, tip_x, c_x, if is_x { 4.0 } else { 2.5 });
            draw_arrow(painter, center_2d, tip_y, c_y, if is_y { 4.0 } else { 2.5 });
            draw_arrow(painter, center_2d, tip_z, c_z, if is_z { 4.0 } else { 2.5 });
            painter.circle_stroke(center_2d, 35.0, Stroke::new(1.5, Color32::from_rgb(255, 255, 100)));
        }

        GizmoMode::View => {}
    }

    hovered_axis
}

/// Cotas locales (min, max) de la geometría de una entidad: las reales de
/// su malla según el mapa que la app llena desde el AssetCache. Con
/// `model == ""` el render usa la malla "cube" de respaldo, así que se
/// consultan sus cotas. Sin malla conocida, el cubo unidad de siempre.
pub fn entity_local_bounds(
    ent: &EntityData,
    bounds: &std::collections::HashMap<String, ([f32; 3], [f32; 3])>,
) -> ([f32; 3], [f32; 3]) {
    if let Some(mesh) = &ent.mesh {
        let key = if mesh.model.is_empty() { "cube" } else { mesh.model.as_str() };
        if let Some(b) = bounds.get(key) {
            return *b;
        }
    }
    ([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5])
}

/// Inversa general 4×4 (adjunta), en el mismo orden column-major que el
/// resto del motor. `glass_core::mat4_inverse_affine` NO sirve para un
/// view-projection: solo invierte la parte afín 3×3 y un VP con
/// perspectiva queda mal invertido (el picking salía desviado).
fn mat4_inverse_full(m: [f32; 16]) -> Option<[f32; 16]> {
    let mut inv = [0.0f32; 16];
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
        + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
        - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
        + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
        - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
        - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
        + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
        - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
        + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
        + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
        - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
        + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
        - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
        - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
        + m[4] * m[3] * m[10] + m[12] * m[2] * m[7] - m[12] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
        - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
        + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
    let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    if det.abs() < 1.0e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    for v in &mut inv {
        *v *= inv_det;
    }
    Some(inv)
}

/// Rayo de mundo que sale de un punto de pantalla: (origen, dirección
/// normalizada). Es la inversa exacta de `world_to_screen` sobre el
/// mismo `vp`; devuelve None si el vp no es invertible o el rayo es
/// degenerado. La desproyección es homogénea (divide por w) con la
/// inversa general: la inversa afín de glass_core no invierte la
/// perspectiva. El NDC de esta proyección usa z ∈ [0, 1].
pub fn screen_ray(
    pos: Pos2,
    vp: &[f32; 16],
    rect: Rect,
) -> Option<([f32; 3], [f32; 3])> {
    let inv = mat4_inverse_full(*vp)?;
    // La Y sigue la convención de `world_to_screen` (la proyección lleva
    // la Y invertida para Vulkan): ndc_y crece hacia ABAJO en pantalla.
    let nx = ((pos.x - rect.min.x) / rect.width()) * 2.0 - 1.0;
    let ny = ((pos.y - rect.min.y) / rect.height()) * 2.0 - 1.0;
    let desproyectar = |nz: f32| -> Option<[f32; 3]> {
        let v = [nx, ny, nz, 1.0];
        let mut o = [0.0f32; 4];
        for (row, out) in o.iter_mut().enumerate() {
            *out = inv[row] * v[0] + inv[4 + row] * v[1] + inv[8 + row] * v[2] + inv[12 + row] * v[3];
        }
        if o[3].abs() < 1.0e-9 {
            return None;
        }
        Some([o[0] / o[3], o[1] / o[3], o[2] / o[3]])
    };
    let p0 = desproyectar(0.0)?;
    let p1 = desproyectar(1.0)?;
    let dir = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if len < 1.0e-9 {
        return None;
    }
    Some((p0, [dir[0] / len, dir[1] / len, dir[2] / len]))
}

/// Raycasting 3D sobre la escena: Encuentra la entidad más cercana al rayo cuando el usuario hace clic.
/// Convierte un punto de pantalla en un punto del mundo sobre el plano
/// horizontal `plane_y` (por defecto el suelo Y=0).
///
/// Se usa para el menú contextual del Scene View: en Unity, "Create > Cube"
/// con el ratón sobre la rejilla coloca el objeto **bajo el cursor**, no en el
/// origen. Devuelve `None` si el rayo es paralelo al plano o va hacia arriba.
pub fn screen_to_world_on_plane(
    pos: Pos2,
    vp: &[f32; 16],
    rect: Rect,
    plane_y: f32,
) -> Option<[f32; 3]> {
    let (origin, dir) = screen_ray(pos, vp, rect)?;
    if dir[1].abs() < 1.0e-6 {
        return None;
    }
    let t = (plane_y - origin[1]) / dir[1];
    if t < 0.0 {
        return None;
    }
    Some([origin[0] + dir[0] * t, plane_y, origin[2] + dir[2] * t])
}

/// Las 8 esquinas de una caja (min, max) en el orden que esperan las
/// aristas de la caja de selección: cara min-Z (0..4) y cara max-Z (4..8),
/// ambas recorridas (min,min) → (max,min) → (max,max) → (min,max) en XY.
fn aabb_corners(lo: [f32; 3], hi: [f32; 3]) -> [[f32; 3]; 8] {
    [
        [lo[0], lo[1], lo[2]],
        [hi[0], lo[1], lo[2]],
        [hi[0], hi[1], lo[2]],
        [lo[0], hi[1], lo[2]],
        [lo[0], lo[1], hi[2]],
        [hi[0], lo[1], hi[2]],
        [hi[0], hi[1], hi[2]],
        [lo[0], hi[1], hi[2]],
    ]
}

/// Intersección rayo-AABB por slabs. Devuelve el t ≥ 0 de entrada (0 si el
/// origen ya está dentro) o None si no hay impacto delante del origen.
fn ray_aabb_hit(origin: [f32; 3], dir: [f32; 3], min: [f32; 3], max: [f32; 3]) -> Option<f32> {
    let mut t_near = f32::NEG_INFINITY;
    let mut t_far = f32::INFINITY;
    for a in 0..3 {
        if dir[a].abs() < 1.0e-9 {
            // Paralelo a este eje: solo hay impacto si el origen está
            // dentro del slab.
            if origin[a] < min[a] || origin[a] > max[a] {
                return None;
            }
        } else {
            let inv = 1.0 / dir[a];
            let mut t0 = (min[a] - origin[a]) * inv;
            let mut t1 = (max[a] - origin[a]) * inv;
            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
            }
            t_near = t_near.max(t0);
            t_far = t_far.min(t1);
            if t_near > t_far {
                return None;
            }
        }
    }
    if t_far < 0.0 {
        return None;
    }
    Some(t_near.max(0.0))
}

/// Devuelve la entidad que el usuario clica en el Scene View.
///
/// Primero los iconos de cámara y luz: si el clic cae a ≤ 24 px de la
/// posición proyectada de alguno, gana el más cercano a `cam_pos`. Si no,
/// se lanza un rayo desde el punto de pantalla contra la AABB de mundo de
/// la geometría real de cada entidad (cotas locales de su malla en
/// `bounds`, llevadas a mundo con su matriz completa) y gana el impacto
/// más cercano al origen del rayo.
pub fn pick_entity_at_screen(
    click_pos: Pos2,
    entities: &[EntityData],
    world_matrices: &std::collections::HashMap<u64, [f32; 16]>,
    vp: &[f32; 16],
    rect: Rect,
    cam_pos: [f32; 3],
    bounds: &std::collections::HashMap<String, ([f32; 3], [f32; 3])>,
) -> Option<u64> {
    let (origin, dir) = screen_ray(click_pos, vp, rect)?;

    // El picking debe ocurrir donde la geometría se dibuja de verdad: en
    // espacio de mundo. Sobre un hijo, el local ignora el transform del
    // padre y el clic no cae donde el ojo ve la malla.
    let world_of = |ent: &EntityData| -> [f32; 16] {
        world_matrices
            .get(&ent.id)
            .copied()
            .unwrap_or_else(|| glass_core::mat4_from_trs_euler(
                ent.transform.position,
                ent.transform.rotation(),
                ent.transform.scale,
            ))
    };

    // Pasada de iconos: cámaras y luces tienen prioridad.
    let mut best_icon = None;
    let mut best_icon_dist = f32::MAX;
    for ent in entities {
        if !ent.active || (ent.camera.is_none() && ent.light.is_none()) {
            continue;
        }
        let pos = glass_core::mat4_translation(world_of(ent));
        let Some(center) = world_to_screen(pos, vp, rect) else {
            continue;
        };
        if (click_pos - center).length() > 24.0 {
            continue;
        }
        let dx = pos[0] - cam_pos[0];
        let dy = pos[1] - cam_pos[1];
        let dz = pos[2] - cam_pos[2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if dist < best_icon_dist {
            best_icon_dist = dist;
            best_icon = Some(ent.id);
        }
    }
    if best_icon.is_some() {
        return best_icon;
    }

    // Pasada de mallas: rayo contra la AABB de mundo de la geometría real.
    let mut best_id = None;
    let mut best_t = f32::MAX;
    for ent in entities {
        if !ent.active {
            continue;
        }
        let world = world_of(ent);
        let (lo, hi) = entity_local_bounds(ent, bounds);
        let mut wmin = [f32::INFINITY; 3];
        let mut wmax = [f32::NEG_INFINITY; 3];
        for c in aabb_corners(lo, hi) {
            let w = glass_core::mat4_transform_point(world, c);
            for a in 0..3 {
                wmin[a] = wmin[a].min(w[a]);
                wmax[a] = wmax[a].max(w[a]);
            }
        }
        if let Some(t) = ray_aabb_hit(origin, dir, wmin, wmax) {
            if t < best_t {
                best_t = t;
                best_id = Some(ent.id);
            }
        }
    }

    best_id
}

/// Punto donde un rayo golpea la AABB de mundo más cercana entre las
/// entidades CON malla (la misma pasada de mallas que
/// `pick_entity_at_screen`, sin iconos). Sirve para aterrizar un asset
/// soltado sobre la superficie que el usuario señala, en vez de sobre el
/// plano y=0.
pub fn punto_aterrizaje(
    entities: &[EntityData],
    world_matrices: &std::collections::HashMap<u64, [f32; 16]>,
    ray_origin: [f32; 3],
    ray_dir: [f32; 3],
    bounds: &std::collections::HashMap<String, ([f32; 3], [f32; 3])>,
) -> Option<[f32; 3]> {
    let world_of = |ent: &EntityData| -> [f32; 16] {
        world_matrices
            .get(&ent.id)
            .copied()
            .unwrap_or_else(|| {
                glass_core::mat4_from_trs_euler(
                    ent.transform.position,
                    ent.transform.rotation(),
                    ent.transform.scale,
                )
            })
    };
    let mut best_t = f32::MAX;
    for ent in entities {
        if !ent.active || ent.mesh.is_none() {
            continue;
        }
        let world = world_of(ent);
        let (lo, hi) = entity_local_bounds(ent, bounds);
        let mut wmin = [f32::INFINITY; 3];
        let mut wmax = [f32::NEG_INFINITY; 3];
        for c in aabb_corners(lo, hi) {
            let w = glass_core::mat4_transform_point(world, c);
            for a in 0..3 {
                wmin[a] = wmin[a].min(w[a]);
                wmax[a] = wmax[a].max(w[a]);
            }
        }
        if let Some(t) = ray_aabb_hit(ray_origin, ray_dir, wmin, wmax) {
            if t < best_t {
                best_t = t;
            }
        }
    }
    if best_t == f32::MAX {
        None
    } else {
        Some([
            ray_origin[0] + ray_dir[0] * best_t,
            ray_origin[1] + ray_dir[1] * best_t,
            ray_origin[2] + ray_dir[2] * best_t,
        ])
    }
}

/// Clase de una línea de la rejilla del editor. Decide color, alfa base y grosor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaseLineaRejilla {
    Menor,
    Mayor,
    EjeX,
    EjeZ,
}

/// Genera las líneas de la rejilla del suelo (plano y = 0) que cubren el cuadrado
/// [centro.x - radio, centro.x + radio] x [centro.z - radio, centro.z + radio].
///
/// Hay una línea de x constante por cada entero k en [ceil(centro.x - radio), floor(centro.x + radio)]
/// y una de z constante por cada entero k en [ceil(centro.z - radio), floor(centro.z + radio)].
/// Los extremos se extienden a floor/ceil del borde del cuadrado.
/// La línea z = 0 es `EjeX`, la línea x = 0 es `EjeZ`, los múltiplos de 10 son `Mayor`
/// y el resto `Menor`.
pub fn lineas_rejilla(
    centro: [f32; 3],
    radio: f32,
) -> Vec<([f32; 3], [f32; 3], ClaseLineaRejilla)> {
    let (x_lo, x_hi) = (centro[0] - radio, centro[0] + radio);
    let (z_lo, z_hi) = (centro[2] - radio, centro[2] + radio);
    let (x_min, x_max) = (x_lo.floor(), x_hi.ceil());
    let (z_min, z_max) = (z_lo.floor(), z_hi.ceil());

    let (kx0, kx1) = (x_lo.ceil() as i32, x_hi.floor() as i32);
    let (kz0, kz1) = (z_lo.ceil() as i32, z_hi.floor() as i32);

    let n_x = (kx1 - kx0 + 1).max(0) as usize;
    let n_z = (kz1 - kz0 + 1).max(0) as usize;
    let mut lineas = Vec::with_capacity(n_x + n_z);

    // Líneas de x constante: corren a lo largo de Z. La de x = 0 es el eje Z.
    for k in kx0..=kx1 {
        let clase = if k == 0 {
            ClaseLineaRejilla::EjeZ
        } else if k % 10 == 0 {
            ClaseLineaRejilla::Mayor
        } else {
            ClaseLineaRejilla::Menor
        };
        let kf = k as f32;
        lineas.push(([kf, 0.0, z_min], [kf, 0.0, z_max], clase));
    }

    // Líneas de z constante: corren a lo largo de X. La de z = 0 es el eje X.
    for k in kz0..=kz1 {
        let clase = if k == 0 {
            ClaseLineaRejilla::EjeX
        } else if k % 10 == 0 {
            ClaseLineaRejilla::Mayor
        } else {
            ClaseLineaRejilla::Menor
        };
        let kf = k as f32;
        lineas.push(([x_min, 0.0, kf], [x_max, 0.0, kf], clase));
    }

    lineas
}

/// Vértices de la rejilla del suelo (y = 0) para el pase 3D del editor:
/// las líneas de `lineas_rejilla` troceadas en tramos de 2 m, con el
/// color de su clase y el alfa desvanecido por la distancia al centro
/// EN CADA VÉRTICE (el troceo es lo que hace posible el degradado: los
/// extremos de una línea entera quedan siempre fuera del radio). El
/// renderer la dibuja con depth test: los modelos la tapan, como en
/// Unity. Sustituye al antiguo dibujo 2D con el Painter de egui, que
/// quedaba sobre los modelos al no tener profundidad.
pub fn vertices_rejilla(
    centro: [f32; 3],
    distancia_cam: f32,
) -> Vec<glass_renderer::EditorLineVertex> {
    // Un tramo se descarta solo si ambos extremos caen por debajo de 1/255.
    const ALFA_MINIMO: f32 = 1.0 / 255.0;
    const LARGO_TRAMO: f32 = 2.0;
    let radio = (distancia_cam * 2.5).clamp(10.0, 100.0);
    let factor_menor = (1.0 - (distancia_cam - 25.0) / 35.0).clamp(0.0, 1.0);
    // Atenuación por distancia en XZ al centro, acotada a [0, 1].
    let atenuacion = |p: [f32; 3]| -> f32 {
        let dx = p[0] - centro[0];
        let dz = p[2] - centro[2];
        (1.0 - (dx * dx + dz * dz).sqrt() / radio).clamp(0.0, 1.0)
    };
    let mut vertices = Vec::new();
    for (a, b, clase) in lineas_rejilla(centro, radio) {
        // `base` en escala 0-255; sin grosor: las líneas 3D son de 1 px.
        let (base, (r, g, bl)) = match clase {
            ClaseLineaRejilla::Menor => (60.0 * factor_menor, (200, 205, 215)),
            ClaseLineaRejilla::Mayor => (110.0, (200, 205, 215)),
            ClaseLineaRejilla::EjeX => (200.0, (225, 90, 90)),
            ClaseLineaRejilla::EjeZ => (200.0, (90, 130, 235)),
        };
        if base <= 0.0 {
            continue;
        }
        let color = |alfa: f32| {
            [
                r as f32 / 255.0,
                g as f32 / 255.0,
                bl as f32 / 255.0,
                alfa,
            ]
        };
        // Trocear la línea: cada tramo aporta sus dos vértices con el
        // alfa de su propio extremo.
        let dx = b[0] - a[0];
        let dz = b[2] - a[2];
        let largo = (dx * dx + dz * dz).sqrt();
        if largo <= f32::EPSILON {
            continue;
        }
        let mut s = 0.0;
        while s < largo {
            let e = (s + LARGO_TRAMO).min(largo);
            let (t0, t1) = (s / largo, e / largo);
            let q0 = [a[0] + dx * t0, 0.0, a[2] + dz * t0];
            let q1 = [a[0] + dx * t1, 0.0, a[2] + dz * t1];
            let alfa0 = base * atenuacion(q0) / 255.0;
            let alfa1 = base * atenuacion(q1) / 255.0;
            if alfa0 >= ALFA_MINIMO || alfa1 >= ALFA_MINIMO {
                vertices.push(glass_renderer::EditorLineVertex {
                    position: q0,
                    color: color(alfa0),
                });
                vertices.push(glass_renderer::EditorLineVertex {
                    position: q1,
                    color: color(alfa1),
                });
            }
            s = e;
        }
    }
    vertices
}

/// Dibuja la caja delimitadora 3D orientada (12 aristas con halo y esquinas) estilo Unity
/// para el GameObject seleccionado en la ventana de escena.
pub fn draw_selection_bounding_box(
    painter: &Painter,
    entity: &EntityData,
    world: &[f32; 16],
    vp: &[f32; 16],
    rect: Rect,
    bounds: &std::collections::HashMap<String, ([f32; 3], [f32; 3])>,
) {
    // La caja envuelve la geometría real de la malla (sus cotas locales).
    // Al transformarla por `world` hereda la rotación, la escala y la
    // traslación del padre, igual que la malla que envuelve.
    let (lo, hi) = entity_local_bounds(entity, bounds);

    // 8 esquinas en espacio local
    let corners_local = aabb_corners(lo, hi);

    // Transformar cada esquina a coordenadas de mundo con la matriz ya
    // resuelta (incluye la rotación del cuaternión autoritativo y la escala
    // heredada del padre).
    let mut corners_world = [[0.0f32; 3]; 8];
    for (i, c) in corners_local.iter().enumerate() {
        corners_world[i] = glass_core::mat4_transform_point(*world, *c);
    }

    // Proyectar las 8 esquinas a pantalla
    let mut corners_screen: [Option<Pos2>; 8] = [None; 8];
    for (i, p) in corners_world.iter().enumerate() {
        corners_screen[i] = world_to_screen(*p, vp, rect);
    }

    // 12 aristas del paralelepípedo
    const EDGES: [(usize, usize); 12] = [
        (0, 1), (1, 2), (2, 3), (3, 0), // Base
        (4, 5), (5, 6), (6, 7), (7, 4), // Techo
        (0, 4), (1, 5), (2, 6), (3, 7), // Pilares verticales
    ];

    // Colores de la selección: blanco a petición del usuario (en lugar del naranja de Unity), líneas un 25% más finas
    let halo_stroke = Stroke::new(2.6, Color32::from_rgba_unmultiplied(255, 255, 255, 120));
    let main_stroke = Stroke::new(1.5, Color32::WHITE);
    let corner_fill = Color32::WHITE;

    // Dibujar las 12 aristas
    for &(a, b) in &EDGES {
        if let (Some(pa), Some(pb)) = (corners_screen[a], corners_screen[b]) {
            painter.line_segment([pa, pb], halo_stroke);
            painter.line_segment([pa, pb], main_stroke);
        }
    }

    // Dibujar pequeños vértices ancla en las esquinas visibles
    for opt_pt in &corners_screen {
        if let Some(pt) = *opt_pt {
            painter.circle_filled(pt, 2.25, corner_fill);
            painter.circle_stroke(pt, 2.25, Stroke::new(0.75, Color32::BLACK));
        }
    }

    // Etiqueta flotante con el nombre del GameObject sobre el objeto
    let mut top_screen_pos: Option<Pos2> = None;
    for opt_pt in &corners_screen {
        if let Some(pt) = *opt_pt {
            top_screen_pos = match top_screen_pos {
                None => Some(pt),
                Some(cur) => {
                    if pt.y < cur.y {
                        Some(pt)
                    } else {
                        Some(cur)
                    }
                }
            };
        }
    }

    if let Some(top_pt) = top_screen_pos {
        let badge_pos = top_pt + vec2(0.0, -16.0);
        let badge_text = format!("📦 {} (#{})", entity.name, entity.id);

        let font_id = egui::FontId::proportional(11.0);
        let text_color = Color32::from_rgb(255, 245, 220);
        let bg_color = Color32::from_rgba_unmultiplied(20, 20, 25, 220);

        let galley = painter.layout_no_wrap(badge_text, font_id, text_color);
        let text_size = galley.size();
        let badge_rect = Rect::from_center_size(badge_pos, text_size + vec2(12.0, 6.0));

        painter.rect(
            badge_rect,
            4.0,
            bg_color,
            Stroke::new(1.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
        painter.galley(badge_pos - text_size * 0.5, galley, text_color);
    }
}

/// Base de una cámara para el dibujo del frustum: posición y ejes de mundo.
pub struct BaseFrustum {
    pub pos: [f32; 3],
    pub fwd: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
}

/// Esquina de un plano del frustum: `pos + fwd*d + right*(sr*w) + up*(su*h)`,
/// calculada componente a componente con un único bucle indexado.
///
/// Existe como helper porque la versión escrita a mano, componente a
/// componente, llevaba dos índices cambiados en la Y de las esquinas derechas
/// del plano lejano (`right[0]`/`up[0]` en vez de `right[1]`/`up[1]`): con la
/// cámara mirando a −Z, la Y de esas esquinas recibía `+w_far` y el rectángulo
/// lejano salía torcido hacia arriba a la derecha (el «frustum deformado» de
/// B7, reabierto el 2026-11-17). Con el bucle, esa clase de error no puede
/// volver a colarse en una sola componente.
pub fn esquina_frustum(base: &BaseFrustum, d: f32, w: f32, h: f32, sr: f32, su: f32) -> [f32; 3] {
    let mut p = [0.0f32; 3];
    for i in 0..3 {
        p[i] = base.pos[i] + base.fwd[i] * d + base.right[i] * (sr * w) + base.up[i] * (su * h);
    }
    p
}

/// Dibuja el Gizmo 3D de una Cámara: Icono 2D flotante, Frustum piramidal wireframe y vector de visión
pub fn draw_camera_gizmo(
    painter: &Painter,
    cam_entity: &EntityData,
    world: &[f32; 16],
    vp: &[f32; 16],
    rect: Rect,
    is_selected: bool,
) -> Option<Rect> {
    // Posición y orientación en mundo: un gizmo de cámara hijo debe dibujarse
    // donde la cámara está realmente, no donde su transform local dice.
    let pos = glass_core::mat4_translation(*world);

    // Base de la cámara leída directamente de las columnas de su matriz de
    // mundo: es la misma orientación que usan el renderer, el gizmo de
    // transform y el bounding box, así el frustum no puede divergir de la
    // entidad. Antes se reconstruía desde los ángulos Euler con las fórmulas
    // de `camera_view_matrix` (glass_app), cuyos signos de pitch/yaw son los
    // opuestos a la convención del motor (R = Rz*Ry*Rx en glass_core::math):
    // con yaw de 90° el frustum apuntaba 180° al revés y con pitch+yaw
    // combinados quedaba torcido respecto a la entidad (la "deformación").
    // Las columnas se normalizan para descartar la escala del transform.
    fn eje_normalizado(v: [f32; 3], respaldo: [f32; 3]) -> [f32; 3] {
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len > 1.0e-8 {
            [v[0] / len, v[1] / len, v[2] / len]
        } else {
            respaldo
        }
    }
    let right = eje_normalizado([world[0], world[1], world[2]], [1.0, 0.0, 0.0]);
    let up = eje_normalizado([world[4], world[5], world[6]], [0.0, 1.0, 0.0]);
    let back = eje_normalizado([world[8], world[9], world[10]], [0.0, 0.0, 1.0]);
    let fwd = [-back[0], -back[1], -back[2]];

    let fov_rad = cam_entity.camera.as_ref().map(|c| c.fov).unwrap_or(60.0).to_radians();
    let aspect = 16.0 / 9.0;

    // Dimensiones del frustum para representación visual en el editor
    let d_near = 0.5;
    let d_far = cam_entity.camera.as_ref().map(|c| c.far_clip).unwrap_or(15.0).clamp(10.0, 25.0);
    let h_near = d_near * (fov_rad * 0.5).tan();
    let w_near = h_near * aspect;
    let h_far = d_far * (fov_rad * 0.5).tan();
    let w_far = h_far * aspect;

    // Esquinas de los planos cercano y lejano, calculadas con el helper
    // componente a componente (ver `esquina_frustum`).
    let base = BaseFrustum { pos, fwd, right, up };
    let n_tl = esquina_frustum(&base, d_near, w_near, h_near, -1.0, 1.0);
    let n_tr = esquina_frustum(&base, d_near, w_near, h_near, 1.0, 1.0);
    let n_br = esquina_frustum(&base, d_near, w_near, h_near, 1.0, -1.0);
    let n_bl = esquina_frustum(&base, d_near, w_near, h_near, -1.0, -1.0);

    let f_tl = esquina_frustum(&base, d_far, w_far, h_far, -1.0, 1.0);
    let f_tr = esquina_frustum(&base, d_far, w_far, h_far, 1.0, 1.0);
    let f_br = esquina_frustum(&base, d_far, w_far, h_far, 1.0, -1.0);
    let f_bl = esquina_frustum(&base, d_far, w_far, h_far, -1.0, -1.0);

    // Centros de los planos para la línea de mira central (Eje óptico)
    let c_near = [pos[0] + fwd[0]*d_near, pos[1] + fwd[1]*d_near, pos[2] + fwd[2]*d_near];
    let c_far = [pos[0] + fwd[0]*d_far, pos[1] + fwd[1]*d_far, pos[2] + fwd[2]*d_far];

    // Proyecciones a pantalla
    let p_cam = world_to_screen(pos, vp, rect);
    let pn_tl = world_to_screen(n_tl, vp, rect);
    let pn_tr = world_to_screen(n_tr, vp, rect);
    let pn_br = world_to_screen(n_br, vp, rect);
    let pn_bl = world_to_screen(n_bl, vp, rect);

    let pf_tl = world_to_screen(f_tl, vp, rect);
    let pf_tr = world_to_screen(f_tr, vp, rect);
    let pf_br = world_to_screen(f_br, vp, rect);
    let pf_bl = world_to_screen(f_bl, vp, rect);

    let p_cn = world_to_screen(c_near, vp, rect);
    let p_cf = world_to_screen(c_far, vp, rect);

    // Dibujar Frustum wireframe estilo Unity
    let stroke_frustum = if is_selected {
        Stroke::new(1.8, Color32::from_rgb(80, 220, 255))
    } else {
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(80, 220, 255, 70))
    };

    if is_selected {
        // 1. Rayos desde el vértice de la cámara (Apex) hacia las 4 esquinas del plano cercano
        let stroke_apex = Stroke::new(1.2, Color32::from_rgba_unmultiplied(80, 220, 255, 170));
        if let Some(pc) = p_cam {
            if let Some(p) = pn_tl { painter.line_segment([pc, p], stroke_apex); }
            if let Some(p) = pn_tr { painter.line_segment([pc, p], stroke_apex); }
            if let Some(p) = pn_br { painter.line_segment([pc, p], stroke_apex); }
            if let Some(p) = pn_bl { painter.line_segment([pc, p], stroke_apex); }
        }

        // 2. Rectángulo del plano cercano (Near Plane)
        if let (Some(a), Some(b)) = (pn_tl, pn_tr) { painter.line_segment([a, b], stroke_frustum); }
        if let (Some(a), Some(b)) = (pn_tr, pn_br) { painter.line_segment([a, b], stroke_frustum); }
        if let (Some(a), Some(b)) = (pn_br, pn_bl) { painter.line_segment([a, b], stroke_frustum); }
        if let (Some(a), Some(b)) = (pn_bl, pn_tl) { painter.line_segment([a, b], stroke_frustum); }

        // 3. Aristas laterales de unión del Frustum (Near -> Far)
        if let (Some(a), Some(b)) = (pn_tl, pf_tl) { painter.line_segment([a, b], stroke_frustum); }
        if let (Some(a), Some(b)) = (pn_tr, pf_tr) { painter.line_segment([a, b], stroke_frustum); }
        if let (Some(a), Some(b)) = (pn_br, pf_br) { painter.line_segment([a, b], stroke_frustum); }
        if let (Some(a), Some(b)) = (pn_bl, pf_bl) { painter.line_segment([a, b], stroke_frustum); }

        // 4. Rectángulo del plano lejano (Far Plane)
        if let (Some(a), Some(b)) = (pf_tl, pf_tr) { painter.line_segment([a, b], stroke_frustum); }
        if let (Some(a), Some(b)) = (pf_tr, pf_br) { painter.line_segment([a, b], stroke_frustum); }
        if let (Some(a), Some(b)) = (pf_br, pf_bl) { painter.line_segment([a, b], stroke_frustum); }
        if let (Some(a), Some(b)) = (pf_bl, pf_tl) { painter.line_segment([a, b], stroke_frustum); }

        // 5. Línea central de mira / eje óptico
        if let (Some(cn), Some(cf)) = (p_cn, p_cf) {
            let stroke_sight = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 220, 100, 140));
            painter.line_segment([cn, cf], stroke_sight);
        }

        // 6. Triángulo indicador "UP" en el borde superior del plano cercano
        let up_apex = [
            c_near[0] + up[0] * (h_near * 1.35),
            c_near[1] + up[1] * (h_near * 1.35),
            c_near[2] + up[2] * (h_near * 1.35),
        ];
        let up_bl = [
            c_near[0] + up[0] * h_near - right[0] * (w_near * 0.2),
            c_near[1] + up[1] * h_near - right[1] * (w_near * 0.2),
            c_near[2] + up[2] * h_near - right[2] * (w_near * 0.2),
        ];
        let up_br = [
            c_near[0] + up[0] * h_near + right[0] * (w_near * 0.2),
            c_near[1] + up[1] * h_near + right[1] * (w_near * 0.2),
            c_near[2] + up[2] * h_near + right[2] * (w_near * 0.2),
        ];
        if let (Some(p_apex), Some(p_l), Some(p_r)) = (
            world_to_screen(up_apex, vp, rect),
            world_to_screen(up_bl, vp, rect),
            world_to_screen(up_br, vp, rect),
        ) {
            let up_stroke = Stroke::new(1.8, Color32::from_rgb(255, 215, 0));
            painter.line_segment([p_l, p_apex], up_stroke);
            painter.line_segment([p_apex, p_r], up_stroke);
        }
    }

    // Dibujar icono billboard simple y elegante (24px)
    if let Some(screen_pos) = p_cam {
        let badge_rect = Rect::from_center_size(screen_pos, vec2(24.0, 24.0));
        let bg_fill = if is_selected {
            Color32::from_rgb(0, 100, 170)
        } else {
            Color32::from_rgba_unmultiplied(20, 25, 35, 200)
        };
        let border_stroke = if is_selected {
            Stroke::new(1.5, Color32::from_rgb(80, 210, 255))
        } else {
            Stroke::new(1.0, Color32::from_rgb(60, 90, 120))
        };

        painter.circle(screen_pos, 12.0, bg_fill, border_stroke);
        painter.text(
            screen_pos,
            egui::Align2::CENTER_CENTER,
            "🎥",
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );

        painter.text(
            screen_pos + vec2(0.0, 16.0),
            egui::Align2::CENTER_TOP,
            &cam_entity.name,
            egui::FontId::proportional(10.0),
            if is_selected { Color32::from_rgb(80, 210, 255) } else { Color32::from_rgb(180, 180, 180) },
        );

        Some(badge_rect)
    } else {
        None
    }
}

/// Dibuja el Gizmo 3D de una Luz: Icono simple, rayos/orientación de sol (Directional) o esfera de alcance (Point Light)
pub fn draw_light_gizmo(
    painter: &Painter,
    light_entity: &EntityData,
    world: &[f32; 16],
    vp: &[f32; 16],
    rect: Rect,
    is_selected: bool,
) -> Option<Rect> {
    let pos = glass_core::mat4_translation(*world);

    // Dirección de emisión = forward de la entidad (-columna 2 de su matriz
    // de mundo), la misma convención que el sombreado y que el gizmo de
    // cámara. Antes se reconstruía desde Euler con los signos de pitch/yaw
    // opuestos a la convención del motor y los rayos no coincidían con la
    // luz real de la escena.
    let back = [world[8], world[9], world[10]];
    let bl = (back[0]*back[0] + back[1]*back[1] + back[2]*back[2]).sqrt().max(0.001);
    let ray_dir = [-back[0] / bl, -back[1] / bl, -back[2] / bl];
    let len = (ray_dir[0]*ray_dir[0] + ray_dir[1]*ray_dir[1] + ray_dir[2]*ray_dir[2]).sqrt().max(0.001);
    let l_dir = [ray_dir[0] / len, ray_dir[1] / len, ray_dir[2] / len];

    let ltype = light_entity.light.as_ref().map(|l| l.light_type.as_str()).unwrap_or("Directional");
    let range = light_entity.light.as_ref().map(|l| l.range).unwrap_or(10.0).max(0.1);

    let p_light = world_to_screen(pos, vp, rect);

    if is_selected {
        if ltype == "Directional" {
            // Dirección solar: dibujar disco emisor y 5 rayos paralelos que viajan en dirección de la luz
            let up_approx = if l_dir[1].abs() > 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
            let r_x = l_dir[1]*up_approx[2] - l_dir[2]*up_approx[1];
            let r_y = l_dir[2]*up_approx[0] - l_dir[0]*up_approx[2];
            let r_z = l_dir[0]*up_approx[1] - l_dir[1]*up_approx[0];
            let r_len = (r_x*r_x + r_y*r_y + r_z*r_z).sqrt().max(0.001);
            let u_vec = [r_x / r_len, r_y / r_len, r_z / r_len];
            let v_vec = [
                l_dir[1]*u_vec[2] - l_dir[2]*u_vec[1],
                l_dir[2]*u_vec[0] - l_dir[0]*u_vec[2],
                l_dir[0]*u_vec[1] - l_dir[1]*u_vec[0],
            ];

            let ray_len = 3.5;
            let disk_rad = 0.7;
            let ray_color = Color32::from_rgb(255, 220, 60);
            let ray_stroke = Stroke::new(1.6, ray_color);

            // Anillo/disco emisor en 3D
            const DISK_STEPS: usize = 16;
            let mut prev_pt: Option<Pos2> = None;
            for i in 0..=DISK_STEPS {
                let theta = (i as f32 / DISK_STEPS as f32) * std::f32::consts::TAU;
                let (st, ct) = (theta.sin(), theta.cos());
                let p_disk = [
                    pos[0] + (u_vec[0]*ct + v_vec[0]*st) * disk_rad,
                    pos[1] + (u_vec[1]*ct + v_vec[1]*st) * disk_rad,
                    pos[2] + (u_vec[2]*ct + v_vec[2]*st) * disk_rad,
                ];
                if let Some(sc_pt) = world_to_screen(p_disk, vp, rect) {
                    if let Some(prev) = prev_pt {
                        painter.line_segment([prev, sc_pt], Stroke::new(1.2, ray_color));
                    }
                    prev_pt = Some(sc_pt);
                }
            }

            // Rayo central y 4 rayos perimetrales con flechas
            let offsets = [
                [0.0, 0.0],
                [disk_rad * 0.65, 0.0],
                [-disk_rad * 0.65, 0.0],
                [0.0, disk_rad * 0.65],
                [0.0, -disk_rad * 0.65],
            ];
            for off in offsets {
                let origin = [
                    pos[0] + u_vec[0]*off[0] + v_vec[0]*off[1],
                    pos[1] + u_vec[1]*off[0] + v_vec[1]*off[1],
                    pos[2] + u_vec[2]*off[0] + v_vec[2]*off[1],
                ];
                let tip = [
                    origin[0] + l_dir[0] * ray_len,
                    origin[1] + l_dir[1] * ray_len,
                    origin[2] + l_dir[2] * ray_len,
                ];
                if let (Some(po), Some(pt)) = (world_to_screen(origin, vp, rect), world_to_screen(tip, vp, rect)) {
                    painter.line_segment([po, pt], ray_stroke);
                    let arrow_dir = (pt - po).normalized();
                    let arrow_side = egui::vec2(-arrow_dir.y, arrow_dir.x);
                    painter.line_segment([pt, pt - arrow_dir * 7.0 + arrow_side * 3.5], ray_stroke);
                    painter.line_segment([pt, pt - arrow_dir * 7.0 - arrow_side * 3.5], ray_stroke);
                }
            }
        } else {
            // Point Light: esfera wireframe 3D limpia
            let sphere_stroke = Stroke::new(1.4, Color32::from_rgba_unmultiplied(255, 215, 50, 180));
            const CIRCLE_SEGS: usize = 32;

            // 1. Círculo plano XZ (Ecuador horizontal)
            let mut prev_pt: Option<Pos2> = None;
            for i in 0..=CIRCLE_SEGS {
                let theta = (i as f32 / CIRCLE_SEGS as f32) * std::f32::consts::TAU;
                let pt3d = [pos[0] + range * theta.cos(), pos[1], pos[2] + range * theta.sin()];
                if let Some(sc_pt) = world_to_screen(pt3d, vp, rect) {
                    if let Some(prev) = prev_pt {
                        painter.line_segment([prev, sc_pt], sphere_stroke);
                    }
                    prev_pt = Some(sc_pt);
                } else {
                    prev_pt = None;
                }
            }

            // 2. Círculo plano XY (Meridiano vertical 1)
            let mut prev_pt: Option<Pos2> = None;
            for i in 0..=CIRCLE_SEGS {
                let theta = (i as f32 / CIRCLE_SEGS as f32) * std::f32::consts::TAU;
                let pt3d = [pos[0] + range * theta.cos(), pos[1] + range * theta.sin(), pos[2]];
                if let Some(sc_pt) = world_to_screen(pt3d, vp, rect) {
                    if let Some(prev) = prev_pt {
                        painter.line_segment([prev, sc_pt], sphere_stroke);
                    }
                    prev_pt = Some(sc_pt);
                } else {
                    prev_pt = None;
                }
            }

            // 3. Círculo plano YZ (Meridiano vertical 2)
            let mut prev_pt: Option<Pos2> = None;
            for i in 0..=CIRCLE_SEGS {
                let theta = (i as f32 / CIRCLE_SEGS as f32) * std::f32::consts::TAU;
                let pt3d = [pos[0], pos[1] + range * theta.sin(), pos[2] + range * theta.cos()];
                if let Some(sc_pt) = world_to_screen(pt3d, vp, rect) {
                    if let Some(prev) = prev_pt {
                        painter.line_segment([prev, sc_pt], sphere_stroke);
                    }
                    prev_pt = Some(sc_pt);
                } else {
                    prev_pt = None;
                }
            }

            // Etiqueta limpia
            let label_pos = [pos[0], pos[1] + range + 0.25, pos[2]];
            if let Some(lp) = world_to_screen(label_pos, vp, rect) {
                painter.text(
                    lp,
                    egui::Align2::CENTER_BOTTOM,
                    format!("☼ Alcance: {:.1} m", range),
                    egui::FontId::proportional(11.0),
                    Color32::from_rgb(255, 220, 70),
                );
            }
        }
    }

    // Dibujar icono billboard simple y elegante (24px)
    if let Some(screen_pos) = p_light {
        let badge_rect = Rect::from_center_size(screen_pos, vec2(24.0, 24.0));
        let icon_str = if ltype == "Directional" { "☼" } else { "💡" };

        let bg_fill = if is_selected {
            Color32::from_rgb(160, 120, 15)
        } else {
            Color32::from_rgba_unmultiplied(35, 30, 15, 200)
        };
        let border_stroke = if is_selected {
            Stroke::new(1.5, Color32::from_rgb(255, 220, 60))
        } else {
            Stroke::new(1.0, Color32::from_rgb(130, 105, 35))
        };

        painter.circle(screen_pos, 12.0, bg_fill, border_stroke);
        painter.text(
            screen_pos,
            egui::Align2::CENTER_CENTER,
            icon_str,
            egui::FontId::proportional(13.0),
            Color32::WHITE,
        );

        painter.text(
            screen_pos + vec2(0.0, 16.0),
            egui::Align2::CENTER_TOP,
            &light_entity.name,
            egui::FontId::proportional(10.0),
            if is_selected { Color32::from_rgb(255, 220, 60) } else { Color32::from_rgb(180, 180, 180) },
        );

        Some(badge_rect)
    } else {
        None
    }
}

#[cfg(test)]
mod tests_picking {
    #[test]
    fn anillo_x_esta_en_el_plano_yz() {
        let origin = [1.0_f32, -2.0, 3.5];
        let radio = 2.0_f32;
        let pts = super::anillo_rotacion_puntos(origin, 0, radio, 64);
        for p in &pts {
            assert!((p[0] - origin[0]).abs() < 1e-5, "x fuera del plano YZ: {:?}", p);
            let d = ((p[0] - origin[0]).powi(2) + (p[1] - origin[1]).powi(2) + (p[2] - origin[2]).powi(2)).sqrt();
            assert!((d - radio).abs() < 1e-4, "radio incorrecto: {}", d);
        }
    }

    #[test]
    fn anillos_son_perpendiculares_entre_si() {
        let origin = [0.5_f32, 1.5, -3.0];
        for eje in 0..3 {
            let mut n = [0.0_f32; 3];
            n[eje] = 1.0;
            for p in super::anillo_rotacion_puntos(origin, eje, 1.7, 64) {
                let dot = (p[0] - origin[0]) * n[0] + (p[1] - origin[1]) * n[1] + (p[2] - origin[2]) * n[2];
                assert!(dot.abs() < 1e-5, "eje {} no perpendicular: dot={}", eje, dot);
            }
        }
    }

    #[test]
    fn anillo_tiene_los_segmentos_pedidos() {
        for seg in [3usize, 16, 64] {
            assert_eq!(super::anillo_rotacion_puntos([0.0; 3], 1, 1.0, seg).len(), seg);
        }
    }
    use super::*;
    use std::collections::HashMap;

    type Cotas = HashMap<String, ([f32; 3], [f32; 3])>;

    const EYE: [f32; 3] = [0.0, 6.0, 10.0];

    fn rect_prueba() -> Rect {
        Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 600.0))
    }

    fn vp_prueba(rect: Rect) -> [f32; 16] {
        let view = look_at(EYE, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let proj = perspective_matrix(
            45.0_f32.to_radians(),
            rect.width() / rect.height(),
            0.1,
            1000.0,
        );
        matmul(&proj, &view)
    }

    fn entidad_malla(id: u64, nombre: &str, modelo: &str, pos: [f32; 3], escala: [f32; 3]) -> EntityData {
        let mut ent = EntityData::new(
            id,
            nombre.into(),
            vec!["Transform".into(), "MeshRenderer".into()],
        );
        ent.transform.position = pos;
        ent.transform.scale = escala;
        ent.mesh.as_mut().unwrap().model = modelo.into();
        ent
    }

    fn plano() -> EntityData {
        entidad_malla(1, "Plane", "plane", [0.0, 0.0, 0.0], [10.0, 1.0, 10.0])
    }

    fn cotas_plano() -> Cotas {
        let mut m = Cotas::new();
        m.insert("plane".into(), ([-0.5, 0.0, -0.5], [0.5, 0.0, 0.5]));
        m
    }

    fn mundos(ents: &[EntityData]) -> HashMap<u64, [f32; 16]> {
        ents.iter()
            .map(|e| {
                (
                    e.id,
                    glass_core::mat4_from_trs_euler(
                        e.transform.position,
                        e.transform.rotation(),
                        e.transform.scale,
                    ),
                )
            })
            .collect()
    }

    fn clic(ents: &[EntityData], cotas: &Cotas, pos: Pos2) -> Option<u64> {
        let rect = rect_prueba();
        let vp = vp_prueba(rect);
        pick_entity_at_screen(pos, ents, &mundos(ents), &vp, rect, EYE, cotas)
    }

    fn proyectar(p: [f32; 3]) -> Pos2 {
        let rect = rect_prueba();
        let s = world_to_screen(p, &vp_prueba(rect), rect).expect("el punto debe proyectarse");
        assert!(rect.contains(s), "el punto proyectado {s:?} cae fuera del viewport");
        s
    }

    #[test]
    fn punto_aterrizaje_golpea_cara_del_cubo_y_none_si_falla() {
        let ents = vec![entidad_malla(1, "Cubo", "cubo", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0])];
        let mut cotas = Cotas::new();
        cotas.insert("cubo".into(), ([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]));
        let mats = mundos(&ents);

        let p = punto_aterrizaje(&ents, &mats, [0.0, 0.0, 10.0], [0.0, 0.0, -1.0], &cotas)
            .expect("el rayo debe golpear el cubo");
        assert!(p[0].abs() < 1e-4, "x = {}", p[0]);
        assert!(p[1].abs() < 1e-4, "y = {}", p[1]);
        assert!((p[2] - 1.0).abs() < 1e-4, "z = {}", p[2]);

        let fallo = punto_aterrizaje(&ents, &mats, [5.0, 0.0, 10.0], [0.0, 0.0, -1.0], &cotas);
        assert!(fallo.is_none(), "un rayo que no toca nada debe dar None: {fallo:?}");
    }

    #[test]
    fn plano_escalado_se_clica_en_su_superficie() {
        let ents = vec![plano()];
        let cotas = cotas_plano();
        assert_eq!(clic(&ents, &cotas, rect_prueba().center()), Some(1));
        assert_eq!(clic(&ents, &cotas, proyectar([3.0, 0.0, 3.0])), Some(1));
    }

    #[test]
    fn modelo_con_origen_en_los_pies_se_clica_en_el_torso() {
        let ents = vec![entidad_malla(2, "Hero", "hero", [0.0, 0.0, -6.0], [1.0, 1.0, 1.0])];
        let mut cotas = Cotas::new();
        cotas.insert("hero".into(), ([-0.4, 0.0, -0.3], [0.4, 1.8, 0.3]));
        assert_eq!(clic(&ents, &cotas, proyectar([0.0, 1.2, -6.0])), Some(2));
    }

    #[test]
    fn gana_el_mas_cercano_al_rayo() {
        let ents = vec![
            plano(),
            entidad_malla(2, "Cube", "", [0.0, 0.5, 0.0], [1.0, 1.0, 1.0]),
        ];
        let cotas = cotas_plano();
        assert_eq!(clic(&ents, &cotas, rect_prueba().center()), Some(2));
    }

    #[test]
    fn clic_en_vacio_no_selecciona() {
        let ents = vec![plano()];
        let cotas = cotas_plano();
        assert_eq!(clic(&ents, &cotas, Pos2::new(5.0, 5.0)), None);
    }

    #[test]
    fn screen_ray_pasa_por_el_punto_proyectado() {
        // Invariante: proyectar un punto de mundo y lanzar el rayo por el
        // píxel resultante debe dar un rayo que pasa por el punto.
        let rect = rect_prueba();
        let vp = vp_prueba(rect);
        for p in [[0.0, 1.2, -6.0], [3.0, 0.0, 3.0], [-2.0, 0.5, -2.0], [0.0, 0.0, 0.0]] {
            let s = world_to_screen(p, &vp, rect).expect("proyección");
            let (origin, dir) = screen_ray(s, &vp, rect).expect("rayo");
            let rel = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
            let t = rel[0] * dir[0] + rel[1] * dir[1] + rel[2] * dir[2];
            let cerca = [
                origin[0] + dir[0] * t,
                origin[1] + dir[1] * t,
                origin[2] + dir[2] * t,
            ];
            let dist = ((p[0] - cerca[0]).powi(2)
                + (p[1] - cerca[1]).powi(2)
                + (p[2] - cerca[2]).powi(2))
            .sqrt();
            assert!(dist < 0.05, "el rayo no pasa por {p:?}: dist {dist}");
        }
    }

    #[test]
    fn caja_de_seleccion_usa_las_cotas_reales() {
        let hero_cotas = ([-0.4, 0.0, -0.3], [0.4, 1.8, 0.3]);
        let mut cotas_hero = Cotas::new();
        cotas_hero.insert("hero".into(), hero_cotas);
        let hero = entidad_malla(1, "Hero", "hero", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        assert_eq!(entity_local_bounds(&hero, &cotas_hero), hero_cotas);

        let cube_cotas = ([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]);
        let mut cotas_cube = Cotas::new();
        cotas_cube.insert("cube".into(), cube_cotas);
        let cubo = entidad_malla(2, "Cube", "", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        assert_eq!(entity_local_bounds(&cubo, &cotas_cube), cube_cotas);

        let vacio = EntityData::new(3, "Empty".into(), vec!["Transform".into()]);
        assert_eq!(
            entity_local_bounds(&vacio, &cotas_cube),
            ([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5])
        );
    }

    /// Regresión del «frustum deformado» (B7 reabierto, 2026-11-17): la Y de
    /// las esquinas derechas del plano lejano usaba `right[0]`/`up[0]` en vez
    /// de `right[1]`/`up[1]`, y con la cámara mirando a −Z recibía `+w_far`
    /// (aquí daría y = 9.445 / 6.766 en vez de 4.679 / −0.679). Valores
    /// exactos del caso medido en vivo con la sonda de diagnóstico.
    #[test]
    fn esquinas_frustum_eje_alineado_valores_exactos() {
        let base = BaseFrustum {
            pos: [0.0, 2.0, 12.0],
            fwd: [0.0, 0.0, -1.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
        };
        let (d, w, h) = (10.0, 4.766, 2.679);
        let eps = 1e-3;
        let tl = esquina_frustum(&base, d, w, h, -1.0, 1.0);
        let tr = esquina_frustum(&base, d, w, h, 1.0, 1.0);
        let br = esquina_frustum(&base, d, w, h, 1.0, -1.0);
        let bl = esquina_frustum(&base, d, w, h, -1.0, -1.0);
        assert!((tl[1] - 4.679).abs() < eps && (tr[1] - 4.679).abs() < eps, "arriba: {tl:?} {tr:?}");
        assert!((br[1] + 0.679).abs() < eps && (bl[1] + 0.679).abs() < eps, "abajo: {br:?} {bl:?}");
        assert!((tl[0] + 4.766).abs() < eps && (tr[0] - 4.766).abs() < eps);
        assert!((br[0] - 4.766).abs() < eps && (bl[0] + 4.766).abs() < eps);
        for c in [tl, tr, br, bl] {
            assert!((c[2] - 2.0).abs() < eps, "todas en el plano z=2: {c:?}");
        }
    }

    /// Con una base rotada (yaw 90°: la cámara mira a −X), las cuatro esquinas
    /// de un plano deben quedar centradas en el eje óptico, en el plano
    /// perpendicular a `fwd` y a la distancia `sqrt(w² + h²)` del centro.
    #[test]
    fn esquinas_frustum_base_rotada_perpendiculares_y_centradas() {
        let base = BaseFrustum {
            pos: [1.0, 2.0, 3.0],
            fwd: [-1.0, 0.0, 0.0],
            right: [0.0, 0.0, -1.0],
            up: [0.0, 1.0, 0.0],
        };
        let (d, w, h): (f32, f32, f32) = (8.0, 3.0, 1.5);
        let centro = [
            base.pos[0] + base.fwd[0] * d,
            base.pos[1] + base.fwd[1] * d,
            base.pos[2] + base.fwd[2] * d,
        ];
        let dist_esperada = (w * w + h * h).sqrt();
        for (sr, su) in [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
            let c = esquina_frustum(&base, d, w, h, sr, su);
            let rel = [c[0] - centro[0], c[1] - centro[1], c[2] - centro[2]];
            let perp = rel[0] * base.fwd[0] + rel[1] * base.fwd[1] + rel[2] * base.fwd[2];
            assert!(perp.abs() < 1e-5, "la esquina no está en el plano: {c:?}");
            let dist = (rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]).sqrt();
            assert!((dist - dist_esperada).abs() < 1e-4, "distancia al centro: {dist}");
        }
    }

    fn vp_desde(eye: [f32; 3], rect: Rect) -> [f32; 16] {
        let view = look_at(eye, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let proj = perspective_matrix(
            45.0_f32.to_radians(),
            rect.width() / rect.height(),
            0.1,
            1000.0,
        );
        matmul(&proj, &view)
    }

    /// Vista por defecto (ojo en +Z): el eje X se proyecta a la derecha y un
    /// arrastre a la derecha avanza en +X siguiendo al cursor 1:1.
    #[test]
    fn avance_eje_x_vista_defecto_sigue_al_cursor() {
        let rect = rect_prueba();
        let vp = vp_desde([0.0, 0.0, 10.0], rect);
        let origen = [0.0, 0.0, 0.0];
        let delta = egui::Vec2::new(60.0, 0.0);
        let a = avance_arrastre_eje(origen, [1.0, 0.0, 0.0], &vp, rect, delta);
        assert!(a > 0.0, "arrastrar a la derecha debe avanzar en +X: {a}");
        let p0 = world_to_screen(origen, &vp, rect).unwrap();
        let p1 = world_to_screen([origen[0] + a, origen[1], origen[2]], &vp, rect).unwrap();
        assert!(
            (p1.x - p0.x - 60.0).abs() < 1.5,
            "el objeto debe seguir al cursor: {} px",
            p1.x - p0.x
        );
        assert!((p1.y - p0.y).abs() < 1.5, "sin deriva vertical: {}", p1.y - p0.y);
    }

    /// LA REGRESIÓN del arrastre invertido: orbitando 180° (ojo en −Z), el eje
    /// X se proyecta a la IZQUIERDA; arrastrar hacia la punta (a la izquierda)
    /// debe avanzar en +X, no al contrario.
    #[test]
    fn avance_eje_x_vista_orbitada_no_se_invierte() {
        let rect = rect_prueba();
        let vp = vp_desde([0.0, 0.0, -10.0], rect);
        let origen = [0.0, 0.0, 0.0];
        let hacia_la_punta = avance_arrastre_eje(
            origen,
            [1.0, 0.0, 0.0],
            &vp,
            rect,
            egui::Vec2::new(-60.0, 0.0),
        );
        assert!(
            hacia_la_punta > 0.0,
            "arrastrar hacia la punta debe avanzar en +X: {hacia_la_punta}"
        );
        let en_contra = avance_arrastre_eje(
            origen,
            [1.0, 0.0, 0.0],
            &vp,
            rect,
            egui::Vec2::new(60.0, 0.0),
        );
        assert!(en_contra < 0.0, "arrastrar en contra debe retroceder: {en_contra}");
    }

    /// El caso exacto de la reproducción física (ojo en +X, como al orbitar
    /// +90°): el eje Z se proyecta a la izquierda y arrastrar hacia su punta
    /// avanza en +Z (antes el Z caía 2,40 en negativo con ese mismo arrastre).
    #[test]
    fn avance_eje_z_desde_el_lado_avanza_hacia_la_punta() {
        let rect = rect_prueba();
        let vp = vp_desde([10.0, 2.0, 0.0], rect);
        let a = avance_arrastre_eje(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            &vp,
            rect,
            egui::Vec2::new(-80.0, 0.0),
        );
        assert!(a > 0.0, "arrastrar hacia la punta de Z debe avanzar en +Z: {a}");
    }

    /// Un eje que apunta a la cámara no tiene dirección de pantalla fiable:
    /// el avance es 0 y el objeto no salta.
    #[test]
    fn avance_eje_apuntando_a_la_camara_es_cero() {
        let rect = rect_prueba();
        let vp = vp_desde([0.0, 0.0, 10.0], rect);
        let a = avance_arrastre_eje(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            &vp,
            rect,
            egui::Vec2::new(120.0, 40.0),
        );
        assert!(a.abs() < 1e-6, "sin avance con el eje hacia la cámara: {a}");
    }

    // ---- Rejilla del editor ----

    type LineaRejilla = ([f32; 3], [f32; 3], super::ClaseLineaRejilla);

    /// Una línea de x constante tiene la misma x en ambos extremos.
    fn es_x_constante(l: &LineaRejilla) -> bool {
        l.0[0] == l.1[0]
    }

    /// ¿El color del vértice es el de la clase (tolerancia 0.01)?
    fn es_color(c: [f32; 4], r: u8, g: u8, b: u8) -> bool {
        (c[0] - r as f32 / 255.0).abs() < 0.01
            && (c[1] - g as f32 / 255.0).abs() < 0.01
            && (c[2] - b as f32 / 255.0).abs() < 0.01
    }

    #[test]
    fn vertices_rejilla_pares_sobre_el_suelo() {
        let v = super::vertices_rejilla([0.0, 0.0, 0.0], 10.0);
        assert!(!v.is_empty());
        assert_eq!(v.len() % 2, 0);
        assert!(v.iter().all(|vert| vert.position[1] == 0.0));
    }

    #[test]
    fn vertices_rejilla_colores_de_los_ejes() {
        let v = super::vertices_rejilla([0.0, 0.0, 0.0], 10.0);
        assert!(v.iter().any(|vert| es_color(vert.color, 225, 90, 90)));
        assert!(v.iter().any(|vert| es_color(vert.color, 90, 130, 235)));
    }

    #[test]
    fn vertices_rejilla_sin_menores_a_distancia_grande() {
        // La línea x = 1 es solo menor (no es múltiplo de 10 ni eje):
        // sus tramos son pares de vértices con x == 1 en ambos extremos.
        let tiene_linea_x1 = |dist: f32| {
            let v = super::vertices_rejilla([0.0, 0.0, 0.0], dist);
            v.as_chunks::<2>()
                .0
                .iter()
                .any(|par| par[0].position[0] == 1.0 && par[1].position[0] == 1.0)
        };
        assert!(tiene_linea_x1(10.0));
        assert!(!tiene_linea_x1(60.0));
    }

    #[test]
    fn vertices_rejilla_alfa_cae_con_la_distancia_en_el_eje_x() {
        let v = super::vertices_rejilla([0.0, 0.0, 0.0], 10.0);
        let rojos: Vec<_> = v
            .iter()
            .filter(|vert| es_color(vert.color, 225, 90, 90))
            .collect();
        let alfa_centro = rojos
            .iter()
            .filter(|vert| vert.position[0].abs() <= 2.0)
            .map(|vert| vert.color[3])
            .fold(0.0f32, f32::max);
        let alfa_lejos = rojos
            .iter()
            .filter(|vert| vert.position[0].abs() >= 20.0)
            .map(|vert| vert.color[3])
            .fold(f32::MAX, f32::min);
        assert!(alfa_centro > 0.5, "el eje junto al centro es casi opaco");
        assert!(alfa_lejos < alfa_centro, "y se desvanece al alejarse");
    }

    #[test]
    fn rejilla_centrada_radio_10_clases_y_conteo() {
        use super::ClaseLineaRejilla as C;
        let lineas = super::lineas_rejilla([0.0, 0.0, 0.0], 10.0);
        assert_eq!(lineas.len(), 42, "21 líneas por dirección");

        let n_x = lineas.iter().filter(|l| es_x_constante(l)).count();
        assert_eq!(n_x, 21);
        assert_eq!(lineas.len() - n_x, 21);

        let contar = |c: C| lineas.iter().filter(|l| l.2 == c).count();
        assert_eq!(contar(C::EjeX), 1);
        assert_eq!(contar(C::EjeZ), 1);
        assert_eq!(contar(C::Mayor), 4);
        assert_eq!(contar(C::Menor), 36);

        for l in &lineas {
            let k = if es_x_constante(l) { l.0[0] } else { l.0[2] };
            match l.2 {
                C::EjeX => {
                    assert!(!es_x_constante(l), "EjeX es la línea z = 0");
                    assert_eq!(k, 0.0);
                }
                C::EjeZ => {
                    assert!(es_x_constante(l), "EjeZ es la línea x = 0");
                    assert_eq!(k, 0.0);
                }
                C::Mayor => assert_eq!(k.abs(), 10.0, "mayor fuera de ±10: {k}"),
                C::Menor => assert!(k != 0.0 && k.abs() != 10.0, "menor con k = {k}"),
            }
        }

        let mayores_x = lineas
            .iter()
            .filter(|l| l.2 == C::Mayor && es_x_constante(l))
            .count();
        assert_eq!(mayores_x, 2, "k = -10 y k = 10 en x");
    }

    #[test]
    fn rejilla_descentrada_constantes_enteras_en_rango() {
        let lineas = super::lineas_rejilla([3.4, 0.0, -2.2], 5.0);

        let mut ks_x: Vec<i32> = Vec::new();
        let mut ks_z: Vec<i32> = Vec::new();
        for l in &lineas {
            let k = if es_x_constante(l) { l.0[0] } else { l.0[2] };
            assert_eq!(k.fract(), 0.0, "constante no entera: {k}");
            if es_x_constante(l) {
                assert!((-1.0..=8.0).contains(&k), "x fuera de rango: {k}");
                ks_x.push(k as i32);
            } else {
                assert!((-7.0..=2.0).contains(&k), "z fuera de rango: {k}");
                ks_z.push(k as i32);
            }
        }
        ks_x.sort_unstable();
        ks_z.sort_unstable();
        assert_eq!(ks_x, (-1..=8).collect::<Vec<_>>());
        assert_eq!(ks_z, (-7..=2).collect::<Vec<_>>());
    }

    #[test]
    fn rejilla_lineas_en_plano_y_cero_con_constante_compartida() {
        let casos = [
            ([0.0, 0.0, 0.0], 10.0),
            ([3.4, 0.0, -2.2], 5.0),
            ([-57.3, 4.0, 81.9], 37.5),
        ];
        for (centro, radio) in casos {
            for l in super::lineas_rejilla(centro, radio) {
                assert_eq!(l.0[1], 0.0, "extremo a fuera de y = 0: {l:?}");
                assert_eq!(l.1[1], 0.0, "extremo b fuera de y = 0: {l:?}");
                let x_igual = l.0[0] == l.1[0];
                let z_igual = l.0[2] == l.1[2];
                assert!(
                    x_igual ^ z_igual,
                    "los extremos deben compartir exactamente una coordenada: {l:?}"
                );
            }
        }
    }
}
