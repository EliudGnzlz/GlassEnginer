//! Operadores de edición de B-A3: espejo, subdivisión, array, soldadura,
//! borrado de vértices y booleana sobre `EditMesh`.
//!
//! Todos validan sus entradas antes de mutar la malla, de modo que
//! `apply_command` sigue siendo atómico (el historial hace snapshot antes).

use std::collections::HashMap;

use crate::boolean::{boolean, BooleanOp};
use crate::commands::MirrorAxis;
use crate::edit_mesh::EditMesh;
use crate::mesh_data::{MeshData, MeshEditError};

const MAX_SUBDIVISION_LEVELS: u32 = 4;
const MAX_ARRAY_COUNT: u32 = 64;
const BOOLEAN_WELD_TOLERANCE: f32 = 1e-4;

/// Refleja la malla respecto al plano del eje dado.
///
/// Niega la coordenada del eje e invierte el winding de cada triángulo para
/// que las normales sigan apuntando hacia fuera. Las UV no cambian.
pub fn mirror(mesh: &mut EditMesh, axis: MirrorAxis) {
    let a = axis.index();
    for v in &mut mesh.verts {
        v[a] = -v[a];
    }
    for f in &mut mesh.faces {
        f.swap(1, 2);
    }
    mesh.mark_dirty();
}

/// Subdivide cada triángulo en 4 por los puntos medios de sus aristas,
/// `levels` veces. Los puntos medios se comparten por arista, así que la
/// malla sigue soldada.
pub fn subdivide(mesh: &mut EditMesh, levels: u32) -> Result<(), MeshEditError> {
    if levels > MAX_SUBDIVISION_LEVELS {
        return Err(MeshEditError::LimitExceeded {
            what: "subdivision levels",
            limit: MAX_SUBDIVISION_LEVELS as usize,
            asked: levels as usize,
        });
    }
    if levels == 0 {
        return Ok(());
    }
    for _ in 0..levels {
        subdivide_once(mesh);
    }
    mesh.mark_dirty();
    Ok(())
}

fn subdivide_once(mesh: &mut EditMesh) {
    let old_faces = std::mem::take(&mut mesh.faces);
    let mut cache: HashMap<(u32, u32), u32> = HashMap::with_capacity(old_faces.len() * 3 / 2);
    let mut new_faces = Vec::with_capacity(old_faces.len() * 4);
    for [a, b, c] in old_faces {
        let ab = edge_midpoint(mesh, &mut cache, a, b);
        let bc = edge_midpoint(mesh, &mut cache, b, c);
        let ca = edge_midpoint(mesh, &mut cache, c, a);
        new_faces.extend_from_slice(&[[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
    }
    mesh.faces = new_faces;
}

fn edge_midpoint(
    mesh: &mut EditMesh,
    cache: &mut HashMap<(u32, u32), u32>,
    a: u32,
    b: u32,
) -> u32 {
    let key = (a.min(b), a.max(b));
    *cache.entry(key).or_insert_with(|| {
        let pa = mesh.verts[a as usize];
        let pb = mesh.verts[b as usize];
        let position = [
            (pa[0] + pb[0]) * 0.5,
            (pa[1] + pb[1]) * 0.5,
            (pa[2] + pb[2]) * 0.5,
        ];
        let uv = match (mesh.vert_uv[a as usize], mesh.vert_uv[b as usize]) {
            (Some(ua), Some(ub)) => Some([(ua[0] + ub[0]) * 0.5, (ua[1] + ub[1]) * 0.5]),
            _ => None,
        };
        mesh.push_vert(position, uv)
    })
}

/// Replica la malla completa `count` veces; la copia `k` se desplaza
/// `k * offset`. `count == 1` es no-op.
pub fn array(mesh: &mut EditMesh, count: u32, offset: [f32; 3]) -> Result<(), MeshEditError> {
    if count == 0 || count > MAX_ARRAY_COUNT {
        return Err(MeshEditError::LimitExceeded {
            what: "array count",
            limit: MAX_ARRAY_COUNT as usize,
            asked: count as usize,
        });
    }
    if count == 1 {
        return Ok(());
    }
    let n = mesh.verts.len();
    let total = n * count as usize;
    let Ok(n32) = u32::try_from(n) else {
        return Err(MeshEditError::LimitExceeded {
            what: "array vertices",
            limit: u32::MAX as usize,
            asked: total,
        });
    };
    if u32::try_from(total).is_err() {
        return Err(MeshEditError::LimitExceeded {
            what: "array vertices",
            limit: u32::MAX as usize,
            asked: total,
        });
    }

    let base_verts = mesh.verts.clone();
    let base_uvs = mesh.vert_uv.clone();
    let base_faces = mesh.faces.clone();
    mesh.verts.reserve(total - n);
    mesh.vert_uv.reserve(total - n);
    mesh.faces.reserve(base_faces.len() * (count as usize - 1));

    for k in 1..count {
        let kf = k as f32;
        let d = [offset[0] * kf, offset[1] * kf, offset[2] * kf];
        mesh.verts
            .extend(base_verts.iter().map(|v| [v[0] + d[0], v[1] + d[1], v[2] + d[2]]));
        mesh.vert_uv.extend_from_slice(&base_uvs);
        let base = k * n32;
        mesh.faces
            .extend(base_faces.iter().map(|f| [f[0] + base, f[1] + base, f[2] + base]));
    }
    mesh.mark_dirty();
    Ok(())
}

/// Re-suelda la malla con la tolerancia dada usando la soldadura de
/// `EditMesh::from_mesh_data`.
///
/// Las caras que quedan degeneradas tras soldar se descartan (comportamiento
/// de `from_mesh_data`; el recuento queda en `last_weld.dropped_faces`).
/// Si algún vértice tiene UV, los vértices sin UV reciben `[0, 0]`.
pub fn weld(mesh: &mut EditMesh, tolerance: f32) -> Result<(), MeshEditError> {
    let tmp = to_mesh_data(mesh);
    let rebuilt = EditMesh::from_mesh_data(&tmp, tolerance)?;
    replace_with(mesh, rebuilt);
    Ok(())
}

/// Elimina los vértices indicados y todas las caras que los referencian,
/// reindexando los restantes. Los duplicados en `indices` se ignoran.
pub fn delete_verts(mesh: &mut EditMesh, indices: &[u32]) -> Result<(), MeshEditError> {
    let n = mesh.verts.len();
    if let Some(&bad) = indices.iter().find(|&&i| i as usize >= n) {
        return Err(MeshEditError::IndexOutOfRange {
            index: bad,
            verts: n,
        });
    }
    if indices.is_empty() {
        return Ok(());
    }

    let mut removed = vec![false; n];
    for &i in indices {
        removed[i as usize] = true;
    }

    let mut remap = vec![u32::MAX; n];
    let mut verts = Vec::with_capacity(n);
    let mut uvs = Vec::with_capacity(n);
    let mut next = 0u32;
    for (((slot, &gone), &pos), &uv) in remap
        .iter_mut()
        .zip(&removed)
        .zip(&mesh.verts)
        .zip(&mesh.vert_uv)
    {
        if !gone {
            *slot = next;
            next += 1;
            verts.push(pos);
            uvs.push(uv);
        }
    }

    let faces = mesh
        .faces
        .iter()
        .filter(|f| f.iter().all(|&i| !removed[i as usize]))
        .map(|f| f.map(|i| remap[i as usize]))
        .collect();

    mesh.verts = verts;
    mesh.vert_uv = uvs;
    mesh.faces = faces;
    mesh.mark_dirty();
    Ok(())
}

/// Aplica una booleana entre la malla actual y `other`, y reconstruye la
/// `EditMesh` desde el resultado (soldadura a 1e-4). Propaga el error de la
/// booleana tal cual.
pub fn apply_boolean(
    mesh: &mut EditMesh,
    op: BooleanOp,
    other: &MeshData,
) -> Result<(), MeshEditError> {
    let baked = mesh.bake();
    let result = boolean(&baked, other, op)?;
    let rebuilt = EditMesh::from_mesh_data(&result, BOOLEAN_WELD_TOLERANCE)?;
    replace_with(mesh, rebuilt);
    Ok(())
}

fn to_mesh_data(mesh: &EditMesh) -> MeshData {
    let uvs = mesh.vert_uv.iter().any(Option::is_some).then(|| {
        mesh.vert_uv
            .iter()
            .map(|uv| uv.unwrap_or([0.0, 0.0]))
            .collect()
    });
    MeshData {
        positions: mesh.verts.clone(),
        normals: None,
        uvs,
        indices: mesh.faces.iter().flatten().copied().collect(),
    }
}

fn replace_with(mesh: &mut EditMesh, rebuilt: EditMesh) {
    mesh.verts = rebuilt.verts;
    mesh.faces = rebuilt.faces;
    mesh.vert_uv = rebuilt.vert_uv;
    mesh.last_weld = rebuilt.last_weld;
    mesh.mark_dirty();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> EditMesh {
        EditMesh::from_mesh_data(&MeshData::cube(2.0), 1e-4).expect("cubo válido")
    }

    fn mesh_volume(m: &MeshData) -> f32 {
        let p = &m.positions;
        let mut v = 0.0f32;
        for t in m.indices.as_chunks::<3>().0 {
            let a = p[t[0] as usize];
            let b = p[t[1] as usize];
            let c = p[t[2] as usize];
            v += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]);
        }
        (v / 6.0).abs()
    }

    #[test]
    fn mirror_conserva_manifold() {
        let mut mesh = cube();
        let original = mesh.verts.clone();
        mirror(&mut mesh, MirrorAxis::X);
        assert!(mesh.validate_manifold().is_closed_manifold);
        for (o, n) in original.iter().zip(&mesh.verts) {
            assert_eq!(*n, [-o[0], o[1], o[2]]);
        }
        assert!(mesh.verts.contains(&[1.0, -1.0, -1.0]));
    }

    #[test]
    fn subdivide_un_nivel() {
        let mut mesh = cube();
        subdivide(&mut mesh, 1).unwrap();
        assert_eq!(mesh.faces.len(), 48);
        assert_eq!(mesh.verts.len(), 26);
        assert!(mesh.validate_manifold().is_closed_manifold);
    }

    #[test]
    fn subdivide_cero_y_limite() {
        let mut mesh = cube();
        let verts = mesh.verts.clone();
        let faces = mesh.faces.clone();
        subdivide(&mut mesh, 0).unwrap();
        assert_eq!(mesh.verts, verts);
        assert_eq!(mesh.faces, faces);
        assert!(matches!(
            subdivide(&mut mesh, 5),
            Err(MeshEditError::LimitExceeded { .. })
        ));
        assert_eq!(mesh.verts, verts);
        assert_eq!(mesh.faces, faces);
    }

    #[test]
    fn array_tres_copias() {
        let mut mesh = cube();
        array(&mut mesh, 3, [3.0, 0.0, 0.0]).unwrap();
        assert_eq!(mesh.verts.len(), 24);
        assert_eq!(mesh.faces.len(), 36);
        assert!(mesh.verts.iter().any(|v| (v[0] - 5.0).abs() < 1e-6));
    }

    #[test]
    fn weld_fusiona_duplicados() {
        let mut mesh = cube();
        assert_eq!(mesh.verts.len(), 8);
        let p0 = mesh.verts[0];
        let p1 = mesh.verts[1];
        let p2 = mesh.verts[2];
        let n = mesh.push_vert(p0, None);
        mesh.push_face([n, 1, 2]).unwrap();
        weld(&mut mesh, 1e-4).unwrap();
        assert_eq!(mesh.verts.len(), 8);
        assert_eq!(mesh.faces.len(), 13);
        assert!(mesh.faces.iter().flatten().all(|&i| (i as usize) < 8));
        assert!(mesh
            .faces
            .iter()
            .any(|f| f.map(|i| mesh.verts[i as usize]) == [p0, p1, p2]));
    }

    #[test]
    fn delete_verts_reindexa() {
        let mut mesh = cube();
        let p0 = mesh.verts[0];
        delete_verts(&mut mesh, &[0]).unwrap();
        assert!(!mesh.verts.contains(&p0));
        assert_eq!(mesh.verts.len(), 7);
        assert!(mesh.faces.len() < 12);
        assert!(mesh.bake().validate_shape().is_ok());

        let verts = mesh.verts.clone();
        let faces = mesh.faces.clone();
        assert!(matches!(
            delete_verts(&mut mesh, &[9999]),
            Err(MeshEditError::IndexOutOfRange { .. })
        ));
        assert_eq!(mesh.verts, verts);
        assert_eq!(mesh.faces, faces);
    }

    #[test]
    fn boolean_por_comando() {
        let mut mesh = cube();
        let mut other = MeshData::cube(2.0);
        for p in &mut other.positions {
            p[0] += 1.0;
        }
        apply_boolean(&mut mesh, BooleanOp::Union, &other).unwrap();
        let vol = mesh_volume(&mesh.bake());
        assert!((vol - 12.0).abs() < 1e-3, "volumen {vol}");
        assert!(mesh.validate_manifold().is_closed_manifold);
    }
}
