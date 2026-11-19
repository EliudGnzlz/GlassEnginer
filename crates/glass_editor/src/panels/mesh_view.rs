//! Overlay e hit-test de la edición de malla en el viewport de la escena.
//!
//! Geometría pura sobre matrices/proyectores: el llamador aporta funciones
//! que proyectan puntos LOCALES de la malla a pantalla, de modo que este
//! módulo no depende de ninguna cámara concreta.

use std::collections::{BTreeSet, HashSet};

use egui::{Color32, Pos2, Shape, Stroke};

/// Radio de captura (px) de las flechas del gizmo.
pub const GIZMO_PICK_PX: f32 = 12.0;

/// Ejes del gizmo de malla: dirección local y color.
pub const GIZMO_AXES: [([f32; 3], egui::Color32); 3] = [
    ([1.0, 0.0, 0.0], egui::Color32::from_rgb(220, 60, 60)),
    ([0.0, 1.0, 0.0], egui::Color32::from_rgb(60, 200, 60)),
    ([0.0, 0.0, 1.0], egui::Color32::from_rgb(60, 120, 230)),
];

/// Estado de interacción con el viewport durante una sesión de malla.
/// Los deltas se guardan en espacio de MUNDO y sin snap; el panel aplica
/// el snap y la conversión a local al confirmar.
#[derive(Debug, Default)]
pub struct MeshViewState {
    pub gizmo_axis: Option<usize>,
    /// Posición absoluta de pantalla donde empezó el arrastre del gizmo.
    pub gizmo_start: [f32; 2],
    pub drag_delta_world: [f32; 3],
    pub grab: Option<GrabView>,
    pub grab_requested: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct GrabView {
    pub centroid_world: [f32; 3],
    pub normal_world: [f32; 3],
    pub start_world: [f32; 3],
    pub delta_world: [f32; 3],
}

// Colores del overlay.
const FACE_FILL: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 0); // marcador; ver face_fill()
const EDGE_GRAY: u8 = 200;
const VERT_COLOR: Color32 = Color32::from_rgb(255, 180, 60);
const SEL_FACE_STROKE_COLOR: Color32 = Color32::from_rgb(255, 180, 60);
const SEL_VERT_COLOR: Color32 = Color32::from_rgb(255, 140, 0);

const VERT_RADIUS: f32 = 2.5;
const SEL_VERT_RADIUS: f32 = 5.0;

/// Relleno translúcido de una cara normal.
fn face_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(120, 160, 255, 36)
}

/// Relleno de una cara seleccionada.
fn sel_face_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(255, 180, 60, 60)
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// Longitud del gizmo de malla: 0.6 * mayor extensión del bbox (mínimo 0.5 de extensión).
pub fn gizmo_len(verts: &[[f32; 3]]) -> f32 {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for v in verts {
        for k in 0..3 {
            min[k] = min[k].min(v[k]);
            max[k] = max[k].max(v[k]);
        }
    }
    let extent = if verts.is_empty() {
        0.0
    } else {
        (0..3).map(|k| max[k] - min[k]).fold(0.0_f32, f32::max)
    };
    extent.max(0.5) * 0.6
}

/// Distancia de un punto 2D al segmento a-b.
pub fn point_segment_dist(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if len2 <= f32::EPSILON {
        0.0
    } else {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / len2).clamp(0.0, 1.0)
    };
    let c = [a[0] + ab[0] * t - p[0], a[1] + ab[1] * t - p[1]];
    (c[0] * c[0] + c[1] * c[1]).sqrt()
}

/// Eje del gizmo (0,1,2) cuya flecha pasa a menos de GIZMO_PICK_PX de `point_rel`.
/// `project` mapea un punto LOCAL de la malla a píxeles RELATIVOS al origen del viewport.
pub fn hit_gizmo_axis(
    centroid_local: [f32; 3],
    len: f32,
    point_rel: [f32; 2],
    project: &dyn Fn([f32; 3]) -> Option<[f32; 2]>,
) -> Option<usize> {
    let origin = project(centroid_local)?;
    let mut best: Option<(usize, f32)> = None;
    for (i, (dir, _)) in GIZMO_AXES.iter().enumerate() {
        let Some(tip) = project(add3(centroid_local, scale3(*dir, len))) else {
            continue;
        };
        let d = point_segment_dist(point_rel, origin, tip);
        if d < GIZMO_PICK_PX && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// Formas del overlay de edición, en orden de pintado:
/// 1) todas las caras rellenas translúcidas ordenadas de lejos a cerca (painter),
/// 2) aristas únicas (por índices) en gris claro,
/// 3) puntos de todos los vértices (ámbar, r=2.5),
/// 4) caras seleccionadas rellenas ámbar con contorno,
/// 5) vértices seleccionados (naranja, r=5),
/// 6) gizmo de 3 ejes en el centroide (línea + círculo en la punta; más grueso si es el eje activo).
/// `offset_local` desplaza SOLO los vértices seleccionados (preview de arrastre/grab).
/// `project`: punto local -> Pos2 ABSOLUTO de pantalla (None = detrás de cámara; la cara/arista se salta).
/// `depth`: punto local -> distancia a la cámara (para ordenar las caras).
pub fn overlay_shapes(
    verts: &[[f32; 3]],
    faces: &[[u32; 3]],
    selected_faces: &HashSet<u32>,
    sel_set: &HashSet<u32>,
    offset_local: [f32; 3],
    centroid_local: Option<[f32; 3]>,
    len: f32,
    active_axis: Option<usize>,
    project: &dyn Fn([f32; 3]) -> Option<Pos2>,
    depth: &dyn Fn([f32; 3]) -> f32,
) -> Vec<Shape> {
    let _ = FACE_FILL;
    let mut shapes = Vec::new();

    // Posiciones con el desplazamiento de preview aplicado a la selección
    // (la EditMesh está soldada: basta con mirar el índice).
    let moved: Vec<[f32; 3]> = verts
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if sel_set.contains(&(i as u32)) {
                add3(*v, offset_local)
            } else {
                *v
            }
        })
        .collect();
    let screen: Vec<Option<Pos2>> = moved.iter().map(|v| project(*v)).collect();

    // Puntos proyectados de una cara; None si algún índice es inválido o
    // algún vértice queda detrás de la cámara.
    let face_points = |f: &[u32; 3]| -> Option<Vec<Pos2>> {
        f.iter()
            .map(|&i| screen.get(i as usize).copied().flatten())
            .collect()
    };

    // 1) Caras translúcidas, de lejos a cerca.
    let mut sorted: Vec<(f32, Vec<Pos2>)> = faces
        .iter()
        .filter_map(|f| {
            let pts = face_points(f)?;
            let d = f.iter().map(|&i| depth(moved[i as usize])).sum::<f32>() / 3.0;
            Some((d, pts))
        })
        .collect();
    sorted.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, pts) in sorted {
        shapes.push(Shape::convex_polygon(pts, face_fill(), Stroke::NONE));
    }

    // 2) Aristas únicas por índices (orden determinista).
    let edges: BTreeSet<(u32, u32)> = faces
        .iter()
        .flat_map(|f| [(f[0], f[1]), (f[1], f[2]), (f[2], f[0])])
        .map(|(a, b)| (a.min(b), a.max(b)))
        .collect();
    let edge_stroke = Stroke::new(1.0, Color32::from_gray(EDGE_GRAY));
    for (a, b) in edges {
        let pa = screen.get(a as usize).copied().flatten();
        let pb = screen.get(b as usize).copied().flatten();
        if let (Some(pa), Some(pb)) = (pa, pb) {
            shapes.push(Shape::line_segment([pa, pb], edge_stroke));
        }
    }

    // 3) Puntos de todos los vértices.
    for p in screen.iter().flatten() {
        shapes.push(Shape::circle_filled(*p, VERT_RADIUS, VERT_COLOR));
    }

    // 4) Caras seleccionadas, siempre encima.
    let sel_stroke = Stroke::new(2.0, SEL_FACE_STROKE_COLOR);
    for (fi, f) in faces.iter().enumerate() {
        if !selected_faces.contains(&(fi as u32)) {
            continue;
        }
        if let Some(pts) = face_points(f) {
            shapes.push(Shape::convex_polygon(pts, sel_face_fill(), sel_stroke));
        }
    }

    // 5) Vértices seleccionados.
    for (i, p) in screen.iter().enumerate() {
        if let Some(p) = p {
            if sel_set.contains(&(i as u32)) {
                shapes.push(Shape::circle_filled(*p, SEL_VERT_RADIUS, SEL_VERT_COLOR));
            }
        }
    }

    // 6) Gizmo de 3 ejes en el centroide.
    if let Some(c) = centroid_local {
        if let Some(origin) = project(c) {
            for (i, (dir, color)) in GIZMO_AXES.iter().enumerate() {
                let Some(tip) = project(add3(c, scale3(*dir, len))) else {
                    continue;
                };
                let active = active_axis == Some(i);
                let width = if active { 3.0 } else { 2.0 };
                let radius = if active { 6.0 } else { 4.0 };
                shapes.push(Shape::line_segment([origin, tip], Stroke::new(width, *color)));
                shapes.push(Shape::circle_filled(tip, radius, *color));
            }
        }
    }

    shapes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proj_rel(p: [f32; 3]) -> Option<[f32; 2]> {
        Some([p[0] * 100.0 + 400.0, 300.0 - p[1] * 100.0])
    }

    fn proj_abs(p: [f32; 3]) -> Option<Pos2> {
        // Ligera inclinación con z para que las caras laterales no degeneren.
        Some(Pos2::new(
            p[0] * 100.0 + p[2] * 30.0 + 400.0,
            300.0 - p[1] * 100.0 - p[2] * 20.0,
        ))
    }

    fn depth_z(p: [f32; 3]) -> f32 {
        10.0 - p[2]
    }

    /// Cubo de lado 2 centrado en el origen: 8 vértices, 12 triángulos.
    fn cube() -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let verts = vec![
            [-1.0, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ];
        let faces = vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [3, 7, 6],
            [3, 6, 2],
            [0, 4, 7],
            [0, 7, 3],
            [1, 2, 6],
            [1, 6, 5],
        ];
        (verts, faces)
    }

    #[test]
    fn hit_gizmo_acierta_eje_x() {
        // Eje X proyecta de (400,300) a (500,300).
        assert_eq!(hit_gizmo_axis([0.0; 3], 1.0, [450.0, 303.0], &proj_rel), Some(0));
        // Eje Y proyecta de (400,300) a (400,200).
        assert_eq!(hit_gizmo_axis([0.0; 3], 1.0, [404.0, 250.0], &proj_rel), Some(1));
    }

    #[test]
    fn hit_gizmo_none_lejos() {
        assert_eq!(hit_gizmo_axis([0.0; 3], 1.0, [700.0, 700.0], &proj_rel), None);
        assert_eq!(hit_gizmo_axis([0.0; 3], 1.0, [450.0, 330.0], &proj_rel), None);
    }

    #[test]
    fn gizmo_len_cubo() {
        let (verts, _) = cube();
        assert!((gizmo_len(&verts) - 1.2).abs() < 1e-6);
    }

    #[test]
    fn gizmo_len_minimo() {
        assert!((gizmo_len(&[[0.0; 3]]) - 0.3).abs() < 1e-6);
        assert!((gizmo_len(&[]) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn point_segment_dist_basicos() {
        let a = [0.0, 0.0];
        let b = [10.0, 0.0];
        assert!((point_segment_dist([5.0, 3.0], a, b) - 3.0).abs() < 1e-6);
        assert!((point_segment_dist([-3.0, 4.0], a, b) - 5.0).abs() < 1e-6);
        assert!((point_segment_dist([13.0, 4.0], a, b) - 5.0).abs() < 1e-6);
        assert!(point_segment_dist([7.0, 0.0], a, b).abs() < 1e-6);
        // Segmento degenerado: distancia al punto.
        assert!((point_segment_dist([3.0, 4.0], a, a) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn overlay_cubo_sin_y_con_seleccion() {
        let (verts, faces) = cube();
        let vacio = HashSet::new();
        let base = overlay_shapes(
            &verts,
            &faces,
            &vacio,
            &vacio,
            [0.0; 3],
            Some([0.0; 3]),
            gizmo_len(&verts),
            None,
            &proj_abs,
            &depth_z,
        );
        assert!(!base.is_empty());

        let sel_faces: HashSet<u32> = [0].into_iter().collect();
        let con_sel = overlay_shapes(
            &verts,
            &faces,
            &sel_faces,
            &vacio,
            [0.0; 3],
            Some([0.0; 3]),
            gizmo_len(&verts),
            Some(0),
            &proj_abs,
            &depth_z,
        );
        assert!(con_sel.len() > base.len());
    }

    #[test]
    fn overlay_salta_vertices_detras_de_camara() {
        let (verts, faces) = cube();
        let vacio = HashSet::new();
        let nada = |_: [f32; 3]| -> Option<Pos2> { None };
        let shapes = overlay_shapes(
            &verts, &faces, &vacio, &vacio, [0.0; 3], Some([0.0; 3]), 1.0, None, &nada, &depth_z,
        );
        assert!(shapes.is_empty());
    }
}
