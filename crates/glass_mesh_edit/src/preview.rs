//! Matemática de preview wireframe: cámara orbital y proyección perspectiva.
//! Sin egui ni Vulkan; testeable sin ventana.

use std::collections::HashSet;

use crate::mesh_data::MeshData;

/// Paso de cuantización para deduplicar posiciones (bake() expande vértices).
const QUANT: f32 = 1e-4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub center: [f32; 3],
}

impl OrbitCamera {
    /// Cámara con yaw 0.6, pitch 0.35 y distancia fija 5.0 mirando a `center`.
    /// Quien conozca el bbox puede ajustar `dist` a 2.5 * max(extensión, 1.0).
    pub fn new(center: [f32; 3]) -> Self {
        Self {
            yaw: 0.6,
            pitch: 0.35,
            dist: 5.0,
            center,
        }
    }

    /// center + dist * (cos(pitch)*sin(yaw), sin(pitch), cos(pitch)*cos(yaw))
    pub fn eye(&self) -> [f32; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        [
            self.center[0] + self.dist * cp * sy,
            self.center[1] + self.dist * sp,
            self.center[2] + self.dist * cp * cy,
        ]
    }
}

struct CameraBasis {
    eye: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    forward: [f32; 3],
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();
    if len > f32::EPSILON {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        v
    }
}

fn basis(camera: &OrbitCamera) -> CameraBasis {
    let eye = camera.eye();
    let forward = normalize(sub(camera.center, eye));
    let world_up = if dot(forward, [0.0, 1.0, 0.0]).abs() > 0.999 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let right = normalize(cross(forward, world_up));
    let up = cross(right, forward);
    CameraBasis {
        eye,
        right,
        up,
        forward,
    }
}

fn focal(height: f32, fov_deg: f32) -> f32 {
    (height * 0.5) / (fov_deg.to_radians() * 0.5).tan()
}

fn project_with(
    b: &CameraBasis,
    point: [f32; 3],
    width: f32,
    height: f32,
    f: f32,
) -> Option<[f32; 2]> {
    let d = sub(point, b.eye);
    let z = dot(d, b.forward);
    if z <= 0.01 {
        return None;
    }
    let x = dot(d, b.right);
    let y = dot(d, b.up);
    Some([width * 0.5 + f * x / z, height * 0.5 - f * y / z])
}

fn quantize(p: [f32; 3]) -> [i64; 3] {
    [
        (p[0] / QUANT).round() as i64,
        (p[1] / QUANT).round() as i64,
        (p[2] / QUANT).round() as i64,
    ]
}

/// Proyecta un punto con cámara perspectiva simple (fov en grados).
/// Devuelve None si el punto queda detrás de la cámara (z_cam <= 0.01).
/// Coordenadas de pantalla en píxeles, origen arriba a la izquierda, y hacia abajo.
pub fn project_point(
    point: [f32; 3],
    camera: &OrbitCamera,
    width: f32,
    height: f32,
    fov_deg: f32,
) -> Option<[f32; 2]> {
    project_with(&basis(camera), point, width, height, focal(height, fov_deg))
}

/// Aristas únicas de la malla horneada, proyectadas: cada arista es un par de
/// puntos de pantalla. Deduplica por par de posiciones (ida y vuelta) con
/// cuantización a 1e-4, porque bake() expande un vértice por esquina.
/// Las aristas con algún extremo detrás de la cámara se descartan.
/// Las diagonales interiores de cada cara triangulada cuentan como aristas.
pub fn project_edges(
    mesh: &MeshData,
    camera: &OrbitCamera,
    width: f32,
    height: f32,
    fov_deg: f32,
) -> Vec<([f32; 2], [f32; 2])> {
    let b = basis(camera);
    let f = focal(height, fov_deg);
    let mut seen: HashSet<([i64; 3], [i64; 3])> = HashSet::new();
    let mut out = Vec::new();
    for tri in mesh.indices.chunks(3) {
        if tri.len() != 3 {
            continue;
        }
        for (ia, ib) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            let (Some(&pa), Some(&pb)) = (
                mesh.positions.get(ia as usize),
                mesh.positions.get(ib as usize),
            ) else {
                continue;
            };
            let (qa, qb) = (quantize(pa), quantize(pb));
            if qa == qb {
                continue;
            }
            let key = if qa <= qb { (qa, qb) } else { (qb, qa) };
            if !seen.insert(key) {
                continue;
            }
            if let (Some(sa), Some(sb)) = (
                project_with(&b, pa, width, height, f),
                project_with(&b, pb, width, height, f),
            ) {
                out.push((sa, sb));
            }
        }
    }
    out
}

const PICK_DET_EPS: f32 = 1e-8;
const PICK_T_MIN: f32 = 1e-4;
const PICK_TIE_EPS: f32 = 1e-4;

fn pk_add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn pk_sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn pk_scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn pk_dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn pk_cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn pk_normalize(v: [f32; 3]) -> [f32; 3] {
    let len = pk_dot(v, v).sqrt();
    if len <= f32::EPSILON {
        v
    } else {
        pk_scale(v, 1.0 / len)
    }
}

fn pk_focal(height: f32, fov_deg: f32) -> f32 {
    let half = (fov_deg.to_radians() * 0.5).tan();
    if half.abs() <= f32::EPSILON {
        height * 0.5
    } else {
        (height * 0.5) / half
    }
}

/// Base de cámara (forward, right, up) para picking. Duplicado mínimo de la base interna.
fn pk_camera_basis(camera: &OrbitCamera) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let forward = pk_normalize(pk_sub(camera.center, camera.eye()));
    let mut right = pk_cross(forward, [0.0, 1.0, 0.0]);
    if pk_dot(right, right) <= 1e-12 {
        right = [1.0, 0.0, 0.0];
    }
    let right = pk_normalize(right);
    let up = pk_cross(right, forward);
    (forward, right, up)
}

/// Rayo de mundo que pasa por el píxel (px, py) del viewport (origen arriba-izquierda).
///
/// La dirección se calibra con `project_point` sobre el plano a distancia 1 de la cámara,
/// así que es la inversa exacta de la proyección. Si la calibración falla, usa la
/// proyección pinhole analítica.
pub fn screen_ray(
    camera: &OrbitCamera,
    px: f32,
    py: f32,
    width: f32,
    height: f32,
    fov_deg: f32,
) -> ([f32; 3], [f32; 3]) {
    let eye = camera.eye();
    let (forward, right, up) = pk_camera_basis(camera);
    let plane_center = pk_add(eye, forward);
    let probe = |offset: [f32; 3]| {
        project_point(pk_add(plane_center, offset), camera, width, height, fov_deg)
    };
    let calibrated = match (probe([0.0; 3]), probe(right), probe(up)) {
        (Some(p0), Some(pr), Some(pu)) => {
            let (r0, r1) = (pr[0] - p0[0], pr[1] - p0[1]);
            let (u0, u1) = (pu[0] - p0[0], pu[1] - p0[1]);
            let det = r0 * u1 - u0 * r1;
            if det.abs() > 1e-6 {
                let (dx, dy) = (px - p0[0], py - p0[1]);
                Some(((dx * u1 - u0 * dy) / det, (r0 * dy - r1 * dx) / det))
            } else {
                None
            }
        }
        _ => None,
    };
    let (a, b) = calibrated.unwrap_or_else(|| {
        let f = pk_focal(height, fov_deg);
        ((px - width * 0.5) / f, (height * 0.5 - py) / f)
    });
    let dir = pk_normalize(pk_add(forward, pk_add(pk_scale(right, a), pk_scale(up, b))));
    (eye, dir)
}

/// Möller–Trumbore de doble cara. Devuelve (cara, t) del impacto más cercano con t > 1e-4.
pub fn pick_face(
    verts: &[[f32; 3]],
    faces: &[[u32; 3]],
    origin: [f32; 3],
    dir: [f32; 3],
) -> Option<(u32, f32)> {
    let mut best: Option<(u32, f32)> = None;
    for (i, face) in faces.iter().enumerate() {
        let Ok(face_index) = u32::try_from(i) else {
            break;
        };
        let (Some(a), Some(b), Some(c)) = (
            verts.get(face[0] as usize),
            verts.get(face[1] as usize),
            verts.get(face[2] as usize),
        ) else {
            continue;
        };
        let e1 = pk_sub(*b, *a);
        let e2 = pk_sub(*c, *a);
        let p = pk_cross(dir, e2);
        let det = pk_dot(e1, p);
        if det.abs() < PICK_DET_EPS {
            continue;
        }
        let inv = 1.0 / det;
        let s = pk_sub(origin, *a);
        let u = pk_dot(s, p) * inv;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = pk_cross(s, e1);
        let v = pk_dot(dir, q) * inv;
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let t = pk_dot(e2, q) * inv;
        if t <= PICK_T_MIN {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, best_t)) => t < best_t,
        };
        if better {
            best = Some((face_index, t));
        }
    }
    best
}

/// Vértice cuya proyección cae a menos de `max_dist_px` de `point`.
/// Gana la menor distancia de pantalla; en empate, la menor profundidad de cámara.
/// `point` y `size` son [x, y] y [ancho, alto] del viewport en píxeles.
pub fn pick_vertex(
    verts: &[[f32; 3]],
    camera: &OrbitCamera,
    point: [f32; 2],
    size: [f32; 2],
    fov_deg: f32,
    max_dist_px: f32,
) -> Option<u32> {
    let (px, py) = (point[0], point[1]);
    let (width, height) = (size[0], size[1]);
    let eye = camera.eye();
    let (forward, _, _) = pk_camera_basis(camera);
    let mut best: Option<(u32, f32, f32)> = None;
    for (i, v) in verts.iter().enumerate() {
        let Ok(index) = u32::try_from(i) else {
            break;
        };
        let Some(s) = project_point(*v, camera, width, height, fov_deg) else {
            continue;
        };
        let dist = ((s[0] - px).powi(2) + (s[1] - py).powi(2)).sqrt();
        if dist >= max_dist_px {
            continue;
        }
        let depth = pk_dot(pk_sub(*v, eye), forward);
        let better = match best {
            None => true,
            Some((_, best_dist, best_depth)) => {
                dist < best_dist - PICK_TIE_EPS
                    || ((dist - best_dist).abs() <= PICK_TIE_EPS && depth < best_depth)
            }
        };
        if better {
            best = Some((index, dist, depth));
        }
    }
    best.map(|(index, _, _)| index)
}

/// Como `pick_vertex`, pero la proyección la da el llamador: `project` mapea un
/// punto a (píxel relativo al viewport, profundidad de cámara) o None si queda
/// detrás. Gana la menor distancia de pantalla; en empate, la menor profundidad.
pub fn pick_vertex_with(
    verts: &[[f32; 3]],
    point: [f32; 2],
    max_dist_px: f32,
    project: &dyn Fn([f32; 3]) -> Option<([f32; 2], f32)>,
) -> Option<u32> {
    let (px, py) = (point[0], point[1]);
    let mut best: Option<(u32, f32, f32)> = None;
    for (i, v) in verts.iter().enumerate() {
        let Ok(index) = u32::try_from(i) else {
            break;
        };
        let Some((s, depth)) = project(*v) else {
            continue;
        };
        let dist = ((s[0] - px).powi(2) + (s[1] - py).powi(2)).sqrt();
        if dist >= max_dist_px {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, best_dist, best_depth)) => {
                dist < best_dist - PICK_TIE_EPS
                    || ((dist - best_dist).abs() <= PICK_TIE_EPS && depth < best_depth)
            }
        };
        if better {
            best = Some((index, dist, depth));
        }
    }
    best.map(|(index, _, _)| index)
}

/// Redondea al múltiplo de `step` más cercano. Con step <= 0 (o NaN) devuelve `value`.
pub fn snap_value(value: f32, step: f32) -> f32 {
    if step.is_nan() || step <= 0.0 {
        return value;
    }
    (value / step).round() * step
}

/// `snap_value` por componente.
pub fn snap_delta(delta: [f32; 3], step: f32) -> [f32; 3] {
    [
        snap_value(delta[0], step),
        snap_value(delta[1], step),
        snap_value(delta[2], step),
    ]
}

/// Media de las posiciones de los índices válidos. None si no hay ninguno válido.
pub fn selection_centroid(
    verts: &[[f32; 3]],
    indices: &std::collections::HashSet<u32>,
) -> Option<[f32; 3]> {
    let mut sorted: Vec<u32> = indices.iter().copied().collect();
    sorted.sort_unstable();
    let mut sum = [0.0f64; 3];
    let mut count = 0usize;
    for i in sorted {
        if let Some(p) = verts.get(i as usize) {
            sum[0] += f64::from(p[0]);
            sum[1] += f64::from(p[1]);
            sum[2] += f64::from(p[2]);
            count += 1;
        }
    }
    if count == 0 {
        return None;
    }
    let n = count as f64;
    Some([
        (sum[0] / n) as f32,
        (sum[1] / n) as f32,
        (sum[2] / n) as f32,
    ])
}

#[cfg(test)]
mod picking_tests {
    use super::*;
    use std::collections::HashSet;

    const W: f32 = 800.0;
    const H: f32 = 600.0;
    const FOV: f32 = 60.0;

    fn cube() -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let mesh = MeshData::cube(2.0);
        let faces = mesh.indices.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
        (mesh.positions, faces)
    }

    #[test]
    fn rayo_es_inverso_de_project_point() {
        let cam = OrbitCamera::new([0.0, 0.0, 0.0]);
        let p = [0.7, -0.4, 0.9];
        let s = project_point(p, &cam, W, H, FOV).expect("proyecta");
        let (o, d) = screen_ray(&cam, s[0], s[1], W, H, FOV);
        let (forward, _, _) = pk_camera_basis(&cam);
        let z = pk_dot(pk_sub(p, o), forward);
        let back = pk_add(o, pk_scale(d, z / pk_dot(d, forward)));
        for k in 0..3 {
            assert!((back[k] - p[k]).abs() < 1e-3, "{back:?} vs {p:?}");
        }
    }

    #[test]
    fn rayo_central_impacta_cubo() {
        let cam = OrbitCamera::new([0.0, 0.0, 0.0]);
        let (verts, faces) = cube();
        let (o, d) = screen_ray(&cam, W * 0.5, H * 0.5, W, H, FOV);
        let (_, t) = pick_face(&verts, &faces, o, d).expect("impacta");
        assert!(t > 0.0 && t < cam.dist, "t = {t}");
    }

    #[test]
    fn rayo_esquina_no_impacta() {
        let cam = OrbitCamera::new([0.0, 0.0, 0.0]);
        let (verts, faces) = cube();
        let (o, d) = screen_ray(&cam, 5.0, 5.0, W, H, FOV);
        assert!(pick_face(&verts, &faces, o, d).is_none());
    }

    #[test]
    fn pick_vertex_devuelve_vertice_proyectado() {
        let cam = OrbitCamera::new([0.0, 0.0, 0.0]);
        let (verts, _) = cube();
        let s = project_point(verts[0], &cam, W, H, FOV).expect("proyecta");
        let picked = pick_vertex(&verts, &cam, s, [W, H], FOV, 14.0).expect("vértice");
        let p = verts[picked as usize];
        for k in 0..3 {
            assert!((p[k] - verts[0][k]).abs() < 1e-6);
        }
    }

    #[test]
    fn pick_vertex_lejos_devuelve_none() {
        let cam = OrbitCamera::new([0.0, 0.0, 0.0]);
        let (verts, _) = cube();
        assert!(pick_vertex(&verts, &cam, [5.0, 5.0], [W, H], FOV, 14.0).is_none());
    }

    #[test]
    fn pick_vertex_with_usa_proyector_del_llamador() {
        let cam = OrbitCamera::new([0.0, 0.0, 0.0]);
        let (verts, _) = cube();
        let project = |p: [f32; 3]| project_point(p, &cam, W, H, FOV).map(|s| (s, p[2]));
        let s = project(verts[0]).expect("proyecta").0;
        let picked = pick_vertex_with(&verts, s, 14.0, &project).expect("vértice");
        let p = verts[picked as usize];
        for k in 0..3 {
            assert!((p[k] - verts[0][k]).abs() < 1e-6);
        }
    }

    #[test]
    fn pick_vertex_with_lejos_devuelve_none() {
        let cam = OrbitCamera::new([0.0, 0.0, 0.0]);
        let (verts, _) = cube();
        let project = |p: [f32; 3]| project_point(p, &cam, W, H, FOV).map(|s| (s, p[2]));
        assert!(pick_vertex_with(&verts, [5.0, 5.0], 14.0, &project).is_none());
    }

    #[test]
    fn snap_redondea_al_mas_cercano() {
        assert!((snap_value(1.13, 0.25) - 1.25).abs() < 1e-6);
        assert!((snap_value(1.1, 0.25) - 1.0).abs() < 1e-6);
        let d = snap_delta([1.13, -0.3, 0.0], 0.25);
        assert!((d[0] - 1.25).abs() < 1e-6);
        assert!((d[1] + 0.25).abs() < 1e-6);
        assert!(d[2].abs() < 1e-6);
    }

    #[test]
    fn snap_con_step_cero_no_cambia() {
        assert!((snap_value(1.13, 0.0) - 1.13).abs() < 1e-7);
        assert!((snap_value(1.13, -1.0) - 1.13).abs() < 1e-7);
        let d = snap_delta([0.26, 0.1, -0.7], 0.0);
        assert!((d[0] - 0.26).abs() < 1e-7);
        assert!((d[1] - 0.1).abs() < 1e-7);
        assert!((d[2] + 0.7).abs() < 1e-7);
    }

    #[test]
    fn centroide_del_cubo_es_origen() {
        let (verts, _) = cube();
        let all: HashSet<u32> = (0..verts.len())
            .filter_map(|i| u32::try_from(i).ok())
            .collect();
        let c = selection_centroid(&verts, &all).expect("centroide");
        for v in c {
            assert!(v.abs() < 1e-5);
        }
        assert!(selection_centroid(&verts, &HashSet::new()).is_none());
        let invalid: HashSet<u32> = [9_999].into_iter().collect();
        assert!(selection_centroid(&verts, &invalid).is_none());
    }

    #[test]
    fn triangulo_degenerado_no_impacta() {
        let verts = vec![[1.0, 1.0, 1.0]; 3];
        let faces = vec![[0, 1, 2]];
        assert!(pick_face(&verts, &faces, [1.0, 1.0, 5.0], [0.0, 0.0, -1.0]).is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proyeccion_centro_cubo() {
        let cam = OrbitCamera::new([0.0, 0.0, 0.0]);
        assert_eq!(cam.dist, 5.0);
        let p = project_point([0.0, 0.0, 0.0], &cam, 800.0, 600.0, 60.0).unwrap();
        assert!((p[0] - 400.0).abs() <= 1.0, "{p:?}");
        assert!((p[1] - 300.0).abs() <= 1.0, "{p:?}");
    }

    #[test]
    fn proyeccion_detras_de_camara() {
        let cam = OrbitCamera::new([0.0, 0.0, 0.0]);
        let eye = cam.eye();
        let forward = normalize(sub(cam.center, eye));
        let behind = sub(eye, forward);
        assert_eq!(project_point(behind, &cam, 800.0, 600.0, 60.0), None);
    }

    /// 12 aristas del cubo + 6 diagonales (una por cara triangulada).
    #[test]
    fn aristas_cubo_son_18() {
        let cam = OrbitCamera::new([0.0, 0.0, 0.0]);
        let edges = project_edges(&MeshData::cube(2.0), &cam, 800.0, 600.0, 60.0);
        assert_eq!(edges.len(), 18);
    }

    /// 4 aristas de contorno + la diagonal interior entre los 2 triángulos.
    #[test]
    fn aristas_plano_son_5() {
        let cam = OrbitCamera::new([0.0, 0.0, 0.0]);
        let edges = project_edges(&MeshData::plane(2.0), &cam, 800.0, 600.0, 60.0);
        assert_eq!(edges.len(), 5);
    }
}
