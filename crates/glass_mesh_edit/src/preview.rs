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
