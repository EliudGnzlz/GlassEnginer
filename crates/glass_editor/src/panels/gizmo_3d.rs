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

    let axis_len_3d = 1.4_f32;

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

    // Hit-testing con el cursor del ratón
    let mut hovered_axis = GizmoAxis::None;

    if let Some(mouse) = mouse_pos {
        if active_drag_axis != GizmoAxis::None {
            hovered_axis = active_drag_axis;
        } else {
            match mode {
                crate::panels::viewport::GizmoMode::Rotate => {
                    let dist = (mouse - center_2d).length();
                    // Anillo X (radio ~38px), Anillo Y (radio ~45px), Anillo Z (radio ~52px)
                    if (dist - 38.0).abs() < 5.0 {
                        hovered_axis = GizmoAxis::X;
                    } else if (dist - 45.0).abs() < 5.0 {
                        hovered_axis = GizmoAxis::Y;
                    } else if (dist - 52.0).abs() < 5.0 {
                        hovered_axis = GizmoAxis::Z;
                    } else if dist >= 32.0 && dist <= 58.0 {
                        if dist < 41.5 {
                            hovered_axis = GizmoAxis::X;
                        } else if dist < 48.5 {
                            hovered_axis = GizmoAxis::Y;
                        } else {
                            hovered_axis = GizmoAxis::Z;
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

            // Anillo X (Rojo / Amarillo al hover)
            painter.circle_stroke(center_2d, r * 0.85, Stroke::new(if is_x { 4.0 } else { 2.0 }, if is_x { col_yellow } else { col_red }));
            // Anillo Y (Verde / Amarillo al hover)
            painter.circle_stroke(center_2d, r, Stroke::new(if is_y { 4.0 } else { 2.0 }, if is_y { col_yellow } else { col_green }));
            // Anillo Z (Azul / Amarillo al hover)
            painter.circle_stroke(center_2d, r * 1.15, Stroke::new(if is_z { 4.0 } else { 2.0 }, if is_z { col_yellow } else { col_blue }));

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

        GizmoMode::View => {}
    }

    hovered_axis
}

/// Raycasting 3D sobre la escena: Encuentra la entidad más cercana al rayo cuando el usuario hace clic.
pub fn pick_entity_at_screen(
    click_pos: Pos2,
    entities: &[EntityData],
    vp: &[f32; 16],
    rect: Rect,
    cam_pos: [f32; 3],
) -> Option<u64> {
    let mut best_id = None;
    let mut min_dist_to_cam = f32::MAX;

    for ent in entities {
        if !ent.active {
            continue;
        }

        let pos = ent.transform.position;
        let rot = ent.transform.rotation;
        let scale = ent.transform.scale;

        // Distancia euclídea del centro a la cámara
        let dx = pos[0] - cam_pos[0];
        let dy = pos[1] - cam_pos[1];
        let dz = pos[2] - cam_pos[2];
        let dist_cam = (dx * dx + dy * dy + dz * dz).sqrt().max(0.1);

        // Medias extensiones para el Bounding Box de picking
        let hx = (scale[0].abs() * 0.5).max(0.2);
        let hy = (scale[1].abs() * 0.5).max(0.2);
        let hz = (scale[2].abs() * 0.5).max(0.2);

        let corners_local = [
            [-hx, -hy, -hz],
            [ hx, -hy, -hz],
            [ hx,  hy, -hz],
            [-hx,  hy, -hz],
            [-hx, -hy,  hz],
            [ hx, -hy,  hz],
            [ hx,  hy,  hz],
            [-hx,  hy,  hz],
        ];

        let rx = rot[0].to_radians();
        let ry = rot[1].to_radians();
        let rz = rot[2].to_radians();

        let (sx, cx) = (rx.sin(), rx.cos());
        let (sy, cy) = (ry.sin(), ry.cos());
        let (sz, cz) = (rz.sin(), rz.cos());

        let mut min_sx = f32::MAX;
        let mut max_sx = f32::MIN;
        let mut min_sy = f32::MAX;
        let mut max_sy = f32::MIN;
        let mut valid_corners = 0;

        for c in &corners_local {
            let (x, y, z) = (c[0], c[1], c[2]);
            let y1 = y * cx - z * sx;
            let z1 = y * sx + z * cx;
            let x2 = x * cy + z1 * sy;
            let z2 = -x * sy + z1 * cy;
            let x3 = x2 * cz - y1 * sz;
            let y3 = x2 * sz + y1 * cz;

            let wpos = [pos[0] + x3, pos[1] + y3, pos[2] + z2];
            if let Some(spt) = world_to_screen(wpos, vp, rect) {
                valid_corners += 1;
                min_sx = min_sx.min(spt.x);
                max_sx = max_sx.max(spt.x);
                min_sy = min_sy.min(spt.y);
                max_sy = max_sy.max(spt.y);
            }
        }

        let pad = 6.0_f32;
        let is_hit = if valid_corners >= 2 {
            click_pos.x >= (min_sx - pad)
                && click_pos.x <= (max_sx + pad)
                && click_pos.y >= (min_sy - pad)
                && click_pos.y <= (max_sy + pad)
        } else if let Some(center_2d) = world_to_screen(pos, vp, rect) {
            (click_pos - center_2d).length() <= 35.0
        } else {
            false
        };

        if is_hit && dist_cam < min_dist_to_cam {
            min_dist_to_cam = dist_cam;
            best_id = Some(ent.id);
        }
    }

    best_id
}

/// Dibuja la caja delimitadora 3D orientada (12 aristas con halo y esquinas) estilo Unity
/// para el GameObject seleccionado en la ventana de escena.
pub fn draw_selection_bounding_box(
    painter: &Painter,
    entity: &EntityData,
    vp: &[f32; 16],
    rect: Rect,
) {
    let pos = entity.transform.position;
    let rot = entity.transform.rotation;
    let scale = entity.transform.scale;

    // Calcular las medias extensiones (half-extents) locales directamente adaptadas a la malla y escala del GameObject
    let hx = (scale[0].abs() * 0.5).max(0.05);
    let hy = (scale[1].abs() * 0.5).max(0.05);
    let hz = (scale[2].abs() * 0.5).max(0.05);

    // 8 esquinas en espacio local
    let corners_local = [
        [-hx, -hy, -hz],
        [ hx, -hy, -hz],
        [ hx,  hy, -hz],
        [-hx,  hy, -hz],
        [-hx, -hy,  hz],
        [ hx, -hy,  hz],
        [ hx,  hy,  hz],
        [-hx,  hy,  hz],
    ];

    // Convertir rotación Euler de grados a radianes
    let rx = rot[0].to_radians();
    let ry = rot[1].to_radians();
    let rz = rot[2].to_radians();

    let (sx, cx) = (rx.sin(), rx.cos());
    let (sy, cy) = (ry.sin(), ry.cos());
    let (sz, cz) = (rz.sin(), rz.cos());

    // Transformar cada esquina a coordenadas de mundo
    let mut corners_world = [[0.0f32; 3]; 8];
    for (i, c) in corners_local.iter().enumerate() {
        let (x, y, z) = (c[0], c[1], c[2]);

        // Rotación Euler (X luego Y luego Z)
        let y1 = y * cx - z * sx;
        let z1 = y * sx + z * cx;

        let x2 = x * cy + z1 * sy;
        let z2 = -x * sy + z1 * cy;

        let x3 = x2 * cz - y1 * sz;
        let y3 = x2 * sz + y1 * cz;

        corners_world[i] = [pos[0] + x3, pos[1] + y3, pos[2] + z2];
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

    // Colores estilo Unity para la selección (Línea de selección naranja)
    let halo_stroke = Stroke::new(3.5, Color32::from_rgba_unmultiplied(255, 255, 255, 120));
    let main_stroke = Stroke::new(2.0, Color32::from_rgb(255, 149, 0)); // Naranja brillante Unity
    let corner_fill = Color32::from_rgb(255, 215, 60);

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
            painter.circle_filled(pt, 3.0, corner_fill);
            painter.circle_stroke(pt, 3.0, Stroke::new(1.0, Color32::BLACK));
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
            Stroke::new(1.0, Color32::from_rgb(255, 149, 0)),
            egui::StrokeKind::Inside,
        );
        painter.galley(badge_pos - text_size * 0.5, galley, text_color);
    }
}
