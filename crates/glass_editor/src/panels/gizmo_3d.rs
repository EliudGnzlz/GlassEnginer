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
    let inv = glass_core::mat4_inverse_affine(*vp)?;

    let to_ndc = |p: Pos2, depth: f32| -> [f32; 4] {
        let nx = ((p.x - rect.min.x) / rect.width()) * 2.0 - 1.0;
        let ny = 1.0 - ((p.y - rect.min.y) / rect.height()) * 2.0;
        [nx, ny, depth, 1.0]
    };

    // Extremos del rayo en NDC (OpenGL: -1 cercano, +1 lejano).
    let near = to_ndc(pos, -1.0);
    let far = to_ndc(pos, 1.0);
    let p0 = glass_core::mat4_transform_point(inv, [near[0], near[1], near[2]]);
    let p1 = glass_core::mat4_transform_point(inv, [far[0], far[1], far[2]]);

    let dir = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    if dir[1].abs() < 1.0e-6 {
        return None;
    }
    let t = (plane_y - p0[1]) / dir[1];
    if t < 0.0 {
        return None;
    }
    Some([p0[0] + dir[0] * t, plane_y, p0[2] + dir[2] * t])
}

pub fn pick_entity_at_screen(
    click_pos: Pos2,
    entities: &[EntityData],
    world_matrices: &std::collections::HashMap<u64, [f32; 16]>,
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

        // El picking debe ocurrir donde la geometría se dibuja de verdad:
        // en espacio de mundo, no en el local de la entidad. Sobre un hijo,
        // el local ignora el transform del padre y el clic no cae donde el ojo
        // ve la caja.
        let world = world_matrices
            .get(&ent.id)
            .copied()
            .unwrap_or_else(|| glass_core::mat4_from_trs_euler(
                ent.transform.position,
                ent.transform.rotation(),
                ent.transform.scale,
            ));
        let pos = glass_core::mat4_translation(world);
        let scale = glass_core::mat4_scale(world);

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

        let mut min_sx = f32::MAX;
        let mut max_sx = f32::MIN;
        let mut min_sy = f32::MAX;
        let mut max_sy = f32::MIN;
        let mut valid_corners = 0;

        // Las esquinas se llevan a mundo con la matriz completa: hereda la
        // rotación del padre, que la recomposición Euler local ignoraba.
        for c in &corners_local {
            let wpos = glass_core::mat4_transform_point(world, *c);
            if let Some(spt) = world_to_screen(wpos, vp, rect) {
                valid_corners += 1;
                min_sx = min_sx.min(spt.x);
                max_sx = max_sx.max(spt.x);
                min_sy = min_sy.min(spt.y);
                max_sy = max_sy.max(spt.y);
            }
        }

        let center_2d = world_to_screen(pos, vp, rect);
        let icon_clicked = (ent.camera.is_some() || ent.light.is_some())
            && center_2d.map_or(false, |c| (click_pos - c).length() <= 24.0);

        let pad = 6.0_f32;
        let is_hit = icon_clicked || if valid_corners >= 2 {
            click_pos.x >= (min_sx - pad)
                && click_pos.x <= (max_sx + pad)
                && click_pos.y >= (min_sy - pad)
                && click_pos.y <= (max_sy + pad)
        } else if let Some(c) = center_2d {
            (click_pos - c).length() <= 35.0
        } else {
            false
        };

        // Si se hizo clic directo sobre el icono 2D de cámara o luz, le damos prioridad de picking
        let effective_dist = if icon_clicked { 0.0 } else { dist_cam };
        if is_hit && effective_dist < min_dist_to_cam {
            min_dist_to_cam = effective_dist;
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
    world: &[f32; 16],
    vp: &[f32; 16],
    rect: Rect,
) {
    // La caja vive en espacio de mundo: hereda rotación y escala del padre,
    // igual que la malla que envuelve.
    let scale = glass_core::mat4_scale(*world);

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

    // Esquinas del plano cercano
    let n_tl = [pos[0] + fwd[0]*d_near - right[0]*w_near + up[0]*h_near, pos[1] + fwd[1]*d_near - right[1]*w_near + up[1]*h_near, pos[2] + fwd[2]*d_near - right[2]*w_near + up[2]*h_near];
    let n_tr = [pos[0] + fwd[0]*d_near + right[0]*w_near + up[0]*h_near, pos[1] + fwd[1]*d_near + right[1]*w_near + up[1]*h_near, pos[2] + fwd[2]*d_near + right[2]*w_near + up[2]*h_near];
    let n_br = [pos[0] + fwd[0]*d_near + right[0]*w_near - up[0]*h_near, pos[1] + fwd[1]*d_near + right[1]*w_near - up[1]*h_near, pos[2] + fwd[2]*d_near + right[2]*w_near - up[2]*h_near];
    let n_bl = [pos[0] + fwd[0]*d_near - right[0]*w_near - up[0]*h_near, pos[1] + fwd[1]*d_near - right[1]*w_near - up[1]*h_near, pos[2] + fwd[2]*d_near - right[2]*w_near - up[2]*h_near];

    // Esquinas del plano lejano
    let f_tl = [pos[0] + fwd[0]*d_far - right[0]*w_far + up[0]*h_far, pos[1] + fwd[1]*d_far - right[1]*w_far + up[1]*h_far, pos[2] + fwd[2]*d_far - right[2]*w_far + up[2]*h_far];
    let f_tr = [pos[0] + fwd[0]*d_far + right[0]*w_far + up[0]*h_far, pos[1] + fwd[1]*d_far + right[0]*w_far + up[1]*h_far, pos[2] + fwd[2]*d_far + right[2]*w_far + up[2]*h_far];
    let f_br = [pos[0] + fwd[0]*d_far + right[0]*w_far - up[0]*h_far, pos[1] + fwd[1]*d_far + right[0]*w_far - up[0]*h_far, pos[2] + fwd[2]*d_far + right[2]*w_far - up[2]*h_far];
    let f_bl = [pos[0] + fwd[0]*d_far - right[0]*w_far - up[0]*h_far, pos[1] + fwd[1]*d_far - right[1]*w_far - up[1]*h_far, pos[2] + fwd[2]*d_far - right[2]*w_far - up[2]*h_far];

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
