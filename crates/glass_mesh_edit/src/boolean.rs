//! Booleanas (unión, resta, intersección) sobre [`MeshData`] con el kernel Manifold.
//!
//! El kernel trabaja en `f32`. Las entradas deben ser mallas cerradas y manifold.
//! Si alguna no lo es, la booleana puede producir un resultado no manifold; el
//! cierre con validación lo señala con [`MeshEditError::NonManifoldResult`]. No se
//! intenta reparar. Las normales y uvs de entrada se ignoran: el resultado sale con
//! `normals: None, uvs: None` y el bake posterior las genera.

use manifold3d::{Manifold, MeshGL};

use crate::edit_mesh::{EditMesh, ManifoldReport};
use crate::mesh_data::{MeshData, MeshEditError};

/// Tolerancia de soldadura al validar el resultado.
const WELD_EPSILON: f32 = 1e-4;

/// Operación booleana entre dos mallas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    Union,
    Difference,
    Intersection,
}

/// Aplica `op` entre `a` y `b` y devuelve la malla resultante.
pub fn boolean(a: &MeshData, b: &MeshData, op: BooleanOp) -> Result<MeshData, MeshEditError> {
    boolean_with_report(a, b, op).map(|(mesh, _)| mesh)
}

/// Igual que [`boolean`], y además devuelve el informe de manifold del resultado.
pub fn boolean_with_report(
    a: &MeshData,
    b: &MeshData,
    op: BooleanOp,
) -> Result<(MeshData, ManifoldReport), MeshEditError> {
    a.validate_shape()?;
    b.validate_shape()?;

    let ma = to_manifold(a)?;
    let mb = to_manifold(b)?;

    let out = match op {
        BooleanOp::Union => ma.union(&mb),
        BooleanOp::Difference => ma.difference(&mb),
        BooleanOp::Intersection => ma.intersection(&mb),
    };

    let result = from_manifold(&out)?;

    let mut edited = EditMesh::from_mesh_data(&result, WELD_EPSILON)?;
    let report = edited.validate_manifold();
    if !report.is_closed_manifold {
        return Err(MeshEditError::NonManifoldResult {
            boundary: report.boundary_edges,
            non_manifold: report.non_manifold_edges,
            degenerate: report.degenerate_faces,
        });
    }

    Ok((result, report))
}

fn internal_error() -> MeshEditError {
    MeshEditError::NonManifoldResult {
        boundary: 0,
        non_manifold: 0,
        degenerate: 0,
    }
}

fn to_manifold(mesh: &MeshData) -> Result<Manifold, MeshEditError> {
    // Las MeshData de render duplican vértices por cara (costuras de
    // normal/UV): un cubo trae 24 vértices en 8 posiciones. El kernel exige
    // vértices compartidos, así que se suelda primero por la vía de la P1
    // (EditMesh) y se alimenta al kernel con los vértices lógicos.
    let edited = EditMesh::from_mesh_data(mesh, WELD_EPSILON)?;
    let flat: Vec<f32> = edited.verts.iter().flat_map(|p| *p).collect();
    let indices: Vec<u32> = edited.faces.iter().flat_map(|f| *f).collect();
    let gl = MeshGL::new(&flat, 3, &indices).map_err(|_| internal_error())?;
    Manifold::from_meshgl(&gl).map_err(|_| internal_error())
}

fn from_manifold(manifold: &Manifold) -> Result<MeshData, MeshEditError> {
    let (verts, stride, indices) = manifold.to_mesh_f32();
    if stride != 3 {
        return Err(internal_error());
    }
    let positions = verts.as_chunks::<3>().0.to_vec();
    Ok(MeshData {
        positions,
        normals: None,
        uvs: None,
        indices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f32 = 1e-3;

    fn translate(mut mesh: MeshData, offset: [f32; 3]) -> MeshData {
        for p in &mut mesh.positions {
            p[0] += offset[0];
            p[1] += offset[1];
            p[2] += offset[2];
        }
        mesh
    }

    fn mesh_volume(mesh: &MeshData) -> f32 {
        let mut vol = 0.0_f32;
        for tri in mesh.indices.as_chunks::<3>().0 {
            let v0 = mesh.positions[tri[0] as usize];
            let v1 = mesh.positions[tri[1] as usize];
            let v2 = mesh.positions[tri[2] as usize];
            let cross = [
                v1[1] * v2[2] - v1[2] * v2[1],
                v1[2] * v2[0] - v1[0] * v2[2],
                v1[0] * v2[1] - v1[1] * v2[0],
            ];
            vol += (v0[0] * cross[0] + v0[1] * cross[1] + v0[2] * cross[2]) / 6.0;
        }
        vol.abs()
    }

    fn cubos() -> (MeshData, MeshData) {
        let a = MeshData::cube(2.0);
        let b = translate(MeshData::cube(2.0), [1.0, 0.0, 0.0]);
        (a, b)
    }

    #[test]
    fn union_dos_cubos() {
        let (a, b) = cubos();
        let (mesh, report) = boolean_with_report(&a, &b, BooleanOp::Union).unwrap();
        assert!((mesh_volume(&mesh) - 12.0).abs() < TOL);
        assert!(report.is_closed_manifold);
    }

    #[test]
    fn resta_dos_cubos() {
        let (a, b) = cubos();
        let (mesh, report) = boolean_with_report(&a, &b, BooleanOp::Difference).unwrap();
        assert!((mesh_volume(&mesh) - 4.0).abs() < TOL);
        assert!(report.is_closed_manifold);
    }

    #[test]
    fn interseccion_dos_cubos() {
        let (a, b) = cubos();
        let (mesh, report) = boolean_with_report(&a, &b, BooleanOp::Intersection).unwrap();
        assert!((mesh_volume(&mesh) - 4.0).abs() < TOL);
        assert!(report.is_closed_manifold);
    }

    #[test]
    fn plano_abierto_da_error() {
        let plane = MeshData::plane(1.0);
        let cube = MeshData::cube(2.0);
        assert!(boolean(&plane, &cube, BooleanOp::Union).is_err());
    }

    #[test]
    fn entradas_con_indices_invalidos() {
        let mut bad = MeshData::cube(2.0);
        bad.indices.pop();
        assert!(!bad.indices.len().is_multiple_of(3));
        let cube = MeshData::cube(2.0);
        assert!(matches!(
            boolean(&bad, &cube, BooleanOp::Union),
            Err(MeshEditError::InvalidIndices { .. })
        ));
    }
}
