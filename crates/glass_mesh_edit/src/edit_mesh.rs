//! Malla topológica de sesión: soldadura, adyacencia, validación,
//! estadísticas y horneado a malla de render.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::mesh_data::{MeshData, MeshEditError};

/// Área por debajo de la cual una cara se considera degenerada.
const DEGENERATE_AREA_EPS: f32 = 1e-12;
/// Extensión de bbox por debajo de la cual un eje se trata como plano.
const FLAT_EXTENT_EPS: f32 = 1e-12;

/// Resultado de la soldadura de la última construcción.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeldReport {
    /// Vértices de la malla de origen.
    pub source_verts: usize,
    /// Vértices lógicos tras soldar.
    pub welded_verts: usize,
    /// Caras descartadas por quedar con índices repetidos tras soldar.
    pub dropped_faces: usize,
}

/// Informe de validación topológica.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifoldReport {
    pub faces: usize,
    /// Aristas únicas.
    pub edges: usize,
    /// Aristas con 1 sola cara incidente.
    pub boundary_edges: usize,
    /// Aristas con 3 o más caras incidentes.
    pub non_manifold_edges: usize,
    /// Caras con área ~0, índices repetidos o índices fuera de rango.
    pub degenerate_faces: usize,
    /// `boundary == 0 && non_manifold == 0 && degenerate == 0`.
    pub is_closed_manifold: bool,
}

/// Estadísticas de la malla de edición.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MeshStats {
    pub verts: usize,
    pub faces: usize,
    pub edges: usize,
    pub bbox_min: [f32; 3],
    pub bbox_max: [f32; 3],
    pub surface_area: f32,
}

/// Malla topológica de SESIÓN: vértices soldados por posición y caras
/// sobre esos vértices lógicos. Es la forma editable; la de render se
/// obtiene con `bake()`.
///
/// Si se mutan `verts` o `faces` directamente, hay que llamar a
/// [`EditMesh::mark_dirty`] para que la adyacencia se reconstruya.
#[derive(Debug, Clone)]
pub struct EditMesh {
    /// Vértices lógicos (soldados).
    pub verts: Vec<[f32; 3]>,
    /// Triángulos sobre `verts`.
    pub faces: Vec<[u32; 3]>,
    /// UV heredada por vértice lógico (la primera vista).
    pub vert_uv: Vec<Option<[f32; 2]>>,
    /// Informe de la soldadura de la última construcción.
    pub last_weld: WeldReport,
    /// Arista (menor, mayor) → caras incidentes.
    edge_faces: HashMap<(u32, u32), Vec<u32>>,
    /// Vértice → caras incidentes.
    vert_faces: Vec<Vec<u32>>,
    adjacency_dirty: bool,
}

impl EditMesh {
    /// Construye la malla de sesión soldando vértices a distancia
    /// `<= weld_tolerance`. Con tolerancia `<= 0` (o no finita) la soldadura
    /// es exacta por bits. Las caras que degeneran por la soldadura se
    /// descartan y se cuentan en [`EditMesh::last_weld`].
    pub fn from_mesh_data(src: &MeshData, weld_tolerance: f32) -> Result<EditMesh, MeshEditError> {
        src.validate_shape()?;

        let mut verts: Vec<[f32; 3]> = Vec::new();
        let mut vert_uv: Vec<Option<[f32; 2]>> = Vec::new();
        let mut remap: Vec<u32> = Vec::with_capacity(src.positions.len());

        if weld_tolerance > 0.0 && weld_tolerance.is_finite() {
            let tol_sq = weld_tolerance * weld_tolerance;
            let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
            for (i, p) in src.positions.iter().enumerate() {
                let cell = cell_of(p, weld_tolerance);
                let logical = match find_in_neighbourhood(&grid, &verts, cell, p, tol_sq) {
                    Some(l) => l,
                    None => {
                        let l = push_logical(&mut verts, &mut vert_uv, *p, uv_at(src, i));
                        grid.entry(cell).or_default().push(l);
                        l
                    }
                };
                remap.push(logical);
            }
        } else {
            let mut exact: HashMap<[u32; 3], u32> = HashMap::new();
            for (i, p) in src.positions.iter().enumerate() {
                let key = p.map(f32::to_bits);
                let logical = *exact
                    .entry(key)
                    .or_insert_with(|| push_logical(&mut verts, &mut vert_uv, *p, uv_at(src, i)));
                remap.push(logical);
            }
        }

        let mut faces = Vec::with_capacity(src.indices.len() / 3);
        let mut dropped_faces = 0usize;
        for tri in src.indices.as_chunks::<3>().0 {
            let f = [
                remap[tri[0] as usize],
                remap[tri[1] as usize],
                remap[tri[2] as usize],
            ];
            if has_repeated_indices(&f) {
                dropped_faces += 1;
            } else {
                faces.push(f);
            }
        }

        let last_weld = WeldReport {
            source_verts: src.positions.len(),
            welded_verts: verts.len(),
            dropped_faces,
        };
        log::debug!(
            "EditMesh: {} vértices → {} soldados, {} caras, {} descartadas",
            last_weld.source_verts,
            last_weld.welded_verts,
            faces.len(),
            last_weld.dropped_faces
        );
        if dropped_faces > 0 {
            log::warn!("EditMesh: {dropped_faces} caras degeneradas descartadas al soldar");
        }

        Ok(EditMesh {
            verts,
            faces,
            vert_uv,
            last_weld,
            edge_faces: HashMap::new(),
            vert_faces: Vec::new(),
            adjacency_dirty: true,
        })
    }

    /// Marca la adyacencia como sucia; se reconstruirá en la próxima consulta.
    pub fn mark_dirty(&mut self) {
        self.adjacency_dirty = true;
    }

    /// Añade un vértice lógico y devuelve su índice.
    pub fn push_vert(&mut self, position: [f32; 3], uv: Option<[f32; 2]>) -> u32 {
        let index = push_logical(&mut self.verts, &mut self.vert_uv, position, uv);
        self.mark_dirty();
        index
    }

    /// Mueve un vértice lógico.
    pub fn set_vert(&mut self, index: u32, position: [f32; 3]) -> Result<(), MeshEditError> {
        let verts = self.verts.len();
        let slot = self
            .verts
            .get_mut(index as usize)
            .ok_or(MeshEditError::IndexOutOfRange { index, verts })?;
        *slot = position;
        self.mark_dirty();
        Ok(())
    }

    /// Añade una cara sobre vértices existentes y devuelve su índice.
    pub fn push_face(&mut self, face: [u32; 3]) -> Result<u32, MeshEditError> {
        let verts = self.verts.len();
        if let Some(&index) = face.iter().find(|&&i| i as usize >= verts) {
            return Err(MeshEditError::IndexOutOfRange { index, verts });
        }
        self.faces.push(face);
        self.mark_dirty();
        Ok((self.faces.len() - 1) as u32)
    }

    /// Reconstruye los mapas arista→caras y vértice→caras.
    pub fn rebuild_adjacency(&mut self) {
        self.edge_faces.clear();
        self.vert_faces = vec![Vec::new(); self.verts.len()];

        for (fi, f) in self.faces.iter().enumerate() {
            let fi = fi as u32;
            for k in 0..3 {
                let a = f[k];
                let b = f[(k + 1) % 3];
                if a != b {
                    self.edge_faces.entry(edge_key(a, b)).or_default().push(fi);
                }
                let repeated_earlier = f[..k].contains(&a);
                if !repeated_earlier {
                    if let Some(list) = self.vert_faces.get_mut(a as usize) {
                        list.push(fi);
                    }
                }
            }
        }
        self.adjacency_dirty = false;
    }

    /// Reconstruye la adyacencia solo si está sucia.
    pub fn ensure_adjacency(&mut self) {
        if self.adjacency_dirty {
            self.rebuild_adjacency();
        }
    }

    /// Caras incidentes a la arista `a`–`b` (el orden es irrelevante).
    pub fn edge_faces(&mut self, a: u32, b: u32) -> &[u32] {
        self.ensure_adjacency();
        match self.edge_faces.get(&edge_key(a, b)) {
            Some(list) => list,
            None => &[],
        }
    }

    /// Caras incidentes al vértice `v`.
    pub fn vert_faces(&mut self, v: u32) -> &[u32] {
        self.ensure_adjacency();
        match self.vert_faces.get(v as usize) {
            Some(list) => list,
            None => &[],
        }
    }

    /// Valida la topología: aristas de borde, aristas no manifold y caras
    /// degeneradas.
    pub fn validate_manifold(&mut self) -> ManifoldReport {
        self.ensure_adjacency();

        let mut boundary_edges = 0;
        let mut non_manifold_edges = 0;
        for list in self.edge_faces.values() {
            match list.len() {
                1 => boundary_edges += 1,
                n if n >= 3 => non_manifold_edges += 1,
                _ => {}
            }
        }

        let degenerate_faces = self
            .faces
            .iter()
            .filter(|f| {
                has_repeated_indices(f)
                    || self
                        .face_area(f)
                        .is_none_or(|area| area <= DEGENERATE_AREA_EPS)
            })
            .count();

        ManifoldReport {
            faces: self.faces.len(),
            edges: self.edge_faces.len(),
            boundary_edges,
            non_manifold_edges,
            degenerate_faces,
            is_closed_manifold: boundary_edges == 0
                && non_manifold_edges == 0
                && degenerate_faces == 0,
        }
    }

    /// Estadísticas: conteos, bbox y área total.
    pub fn stats(&mut self) -> MeshStats {
        self.ensure_adjacency();
        let (bbox_min, bbox_max) = self.bounds();
        let surface_area = self.faces.iter().filter_map(|f| self.face_area(f)).sum();
        MeshStats {
            verts: self.verts.len(),
            faces: self.faces.len(),
            edges: self.edge_faces.len(),
            bbox_min,
            bbox_max,
            surface_area,
        }
    }

    /// Hornea a malla de render con normales planas: un vértice por esquina,
    /// normal de cara en las tres esquinas y uv heredada o proyectada por el
    /// eje dominante de la normal.
    pub fn bake(&self) -> MeshData {
        let (bmin, bmax) = self.bounds();
        let extent = [0usize, 1, 2].map(|k| {
            let e = bmax[k] - bmin[k];
            if e > FLAT_EXTENT_EPS {
                e
            } else {
                1.0
            }
        });

        let corners = self.faces.len() * 3;
        let mut positions = Vec::with_capacity(corners);
        let mut normals = Vec::with_capacity(corners);
        let mut uvs = Vec::with_capacity(corners);
        let mut indices = Vec::with_capacity(corners);
        let mut skipped = 0usize;

        for f in &self.faces {
            let Some(tri) = self.face_positions(f) else {
                skipped += 1;
                continue;
            };
            let normal = face_normal(&tri);
            let (ua, va) = projection_axes(&normal);

            for (corner, p) in f.iter().zip(tri.iter()) {
                let uv = match self.vert_uv.get(*corner as usize) {
                    Some(Some(uv)) => *uv,
                    _ => [
                        (p[ua] - bmin[ua]) / extent[ua],
                        (p[va] - bmin[va]) / extent[va],
                    ],
                };
                indices.push(positions.len() as u32);
                positions.push(*p);
                normals.push(normal);
                uvs.push(uv);
            }
        }

        if skipped > 0 {
            log::warn!("bake: {skipped} caras con índices fuera de rango omitidas");
        }

        MeshData {
            positions,
            normals: Some(normals),
            uvs: Some(uvs),
            indices,
        }
    }

    /// Bbox de los vértices lógicos; `([0;3], [0;3])` si no hay vértices.
    fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let Some(first) = self.verts.first() else {
            return ([0.0; 3], [0.0; 3]);
        };
        let mut min = *first;
        let mut max = *first;
        for p in &self.verts[1..] {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        (min, max)
    }

    /// Posiciones de las tres esquinas; `None` si algún índice está fuera de rango.
    fn face_positions(&self, f: &[u32; 3]) -> Option<[[f32; 3]; 3]> {
        Some([
            *self.verts.get(f[0] as usize)?,
            *self.verts.get(f[1] as usize)?,
            *self.verts.get(f[2] as usize)?,
        ])
    }

    /// Área de la cara; `None` si algún índice está fuera de rango.
    fn face_area(&self, f: &[u32; 3]) -> Option<f32> {
        let tri = self.face_positions(f)?;
        Some(0.5 * length(cross(sub(tri[1], tri[0]), sub(tri[2], tri[0]))))
    }
}

fn push_logical(
    verts: &mut Vec<[f32; 3]>,
    vert_uv: &mut Vec<Option<[f32; 2]>>,
    position: [f32; 3],
    uv: Option<[f32; 2]>,
) -> u32 {
    let index = verts.len() as u32;
    verts.push(position);
    vert_uv.push(uv);
    index
}

fn uv_at(src: &MeshData, i: usize) -> Option<[f32; 2]> {
    src.uvs.as_ref().and_then(|uvs| uvs.get(i).copied())
}

fn cell_of(p: &[f32; 3], cell_size: f32) -> (i64, i64, i64) {
    let c = p.map(|x| (x / cell_size).floor() as i64);
    (c[0], c[1], c[2])
}

/// Busca un vértice lógico a distancia `<= tol` en la celda y sus 26 vecinas.
fn find_in_neighbourhood(
    grid: &HashMap<(i64, i64, i64), Vec<u32>>,
    verts: &[[f32; 3]],
    cell: (i64, i64, i64),
    p: &[f32; 3],
    tol_sq: f32,
) -> Option<u32> {
    for dx in -1i64..=1 {
        for dy in -1i64..=1 {
            for dz in -1i64..=1 {
                let key = (
                    cell.0.saturating_add(dx),
                    cell.1.saturating_add(dy),
                    cell.2.saturating_add(dz),
                );
                if let Some(list) = grid.get(&key) {
                    for &l in list {
                        let d = sub(verts[l as usize], *p);
                        if dot(d, d) <= tol_sq {
                            return Some(l);
                        }
                    }
                }
            }
        }
    }
    None
}

fn has_repeated_indices(f: &[u32; 3]) -> bool {
    f[0] == f[1] || f[1] == f[2] || f[0] == f[2]
}

fn edge_key(a: u32, b: u32) -> (u32, u32) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Normal normalizada de la cara; `[0, 1, 0]` si el área es ~0.
fn face_normal(tri: &[[f32; 3]; 3]) -> [f32; 3] {
    let n = cross(sub(tri[1], tri[0]), sub(tri[2], tri[0]));
    let len = length(n);
    if 0.5 * len <= DEGENERATE_AREA_EPS {
        [0.0, 1.0, 0.0]
    } else {
        n.map(|c| c / len)
    }
}

/// Ejes (u, v) de proyección según el eje dominante de la normal.
fn projection_axes(n: &[f32; 3]) -> (usize, usize) {
    let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
    if ax >= ay && ax >= az {
        (2, 1)
    } else if ay >= az {
        (0, 2)
    } else {
        (0, 1)
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    fn approx3(a: [f32; 3], b: [f32; 3]) -> bool {
        approx(a[0], b[0]) && approx(a[1], b[1]) && approx(a[2], b[2])
    }

    #[test]
    fn cubo_soldado_es_manifold_cerrado() {
        let mut em = EditMesh::from_mesh_data(&MeshData::cube(2.0), 1e-4).expect("cubo válido");
        assert_eq!(em.verts.len(), 8);
        assert_eq!(em.faces.len(), 12);
        assert_eq!(em.last_weld.source_verts, 24);
        assert_eq!(em.last_weld.welded_verts, 8);
        assert_eq!(em.last_weld.dropped_faces, 0);

        let report = em.validate_manifold();
        assert!(report.is_closed_manifold);
        assert_eq!(report.faces, 12);
        assert_eq!(report.edges, 18);
        assert_eq!(report.boundary_edges, 0);
        assert_eq!(report.non_manifold_edges, 0);
        assert_eq!(report.degenerate_faces, 0);
    }

    #[test]
    fn aristas_del_cubo_tienen_dos_caras() {
        let mut em = EditMesh::from_mesh_data(&MeshData::cube(2.0), 1e-4).expect("cubo válido");
        let [a, b, _] = em.faces[0];
        assert_eq!(em.edge_faces(a, b).len(), 2);
        assert_eq!(em.edge_faces(b, a).len(), 2);
        assert!(!em.vert_faces(a).is_empty());
    }

    #[test]
    fn plano_es_abierto() {
        let mut em = EditMesh::from_mesh_data(&MeshData::plane(1.0), 1e-4).expect("plano válido");
        assert_eq!(em.verts.len(), 4);
        assert_eq!(em.faces.len(), 2);

        let report = em.validate_manifold();
        assert_eq!(report.boundary_edges, 4);
        assert_eq!(report.edges, 5);
        assert!(!report.is_closed_manifold);
    }

    #[test]
    fn bake_del_cubo_con_normales_planas() {
        let em = EditMesh::from_mesh_data(&MeshData::cube(2.0), 1e-4).expect("cubo válido");
        let baked = em.bake();
        assert!(baked.validate_shape().is_ok());
        assert_eq!(baked.positions.len(), 36);
        assert_eq!(baked.indices.len(), 36);

        let normals = baked.normals.as_ref().expect("normales horneadas");
        let mut top_triangles = 0;
        for tri in baked.indices.as_chunks::<3>().0 {
            let n0 = normals[tri[0] as usize];
            assert!(approx3(n0, normals[tri[1] as usize]));
            assert!(approx3(n0, normals[tri[2] as usize]));

            let is_top = tri.iter().all(|&i| approx(baked.positions[i as usize][1], 1.0));
            if is_top {
                top_triangles += 1;
                assert!(approx3(n0, [0.0, 1.0, 0.0]));
            }
        }
        assert_eq!(top_triangles, 2);

        for p in &baked.positions {
            assert!(p.iter().all(|c| c.abs() <= 1.0 + 1e-6));
        }
    }

    #[test]
    fn soldadura_exacta_del_cubo() {
        let em = EditMesh::from_mesh_data(&MeshData::cube(2.0), 0.0).expect("cubo válido");
        assert_eq!(em.verts.len(), 8);
        assert_eq!(em.faces.len(), 12);
    }

    #[test]
    fn estadisticas_del_cubo() {
        let mut em = EditMesh::from_mesh_data(&MeshData::cube(2.0), 1e-4).expect("cubo válido");
        let stats = em.stats();
        assert_eq!(stats.verts, 8);
        assert_eq!(stats.faces, 12);
        assert_eq!(stats.edges, 18);
        assert!(approx3(stats.bbox_min, [-1.0, -1.0, -1.0]));
        assert!(approx3(stats.bbox_max, [1.0, 1.0, 1.0]));
        assert!(approx(stats.surface_area, 24.0));
    }

    #[test]
    fn indice_fuera_de_rango() {
        let mesh = MeshData {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: None,
            uvs: None,
            indices: vec![0, 1, 5],
        };
        let result = EditMesh::from_mesh_data(&mesh, 1e-4);
        assert!(matches!(
            result,
            Err(MeshEditError::IndexOutOfRange { index: 5, verts: 3 })
        ));
    }

    #[test]
    fn mutacion_marca_adyacencia_sucia() {
        let mut em = EditMesh::from_mesh_data(&MeshData::plane(1.0), 1e-4).expect("plano válido");
        assert_eq!(em.validate_manifold().boundary_edges, 4);
        let v = em.push_vert([0.0, 1.0, 0.0], None);
        em.push_face([0, 1, v]).expect("cara válida");
        let report = em.validate_manifold();
        assert_eq!(report.faces, 3);
        assert_eq!(report.edges, 7);
    }
}
