//! I/O de .glb: lectura con el crate `gltf`, escritura mínima a mano.

use std::path::Path;

use serde_json::{json, Map, Value};

use crate::mesh_data::{MeshData, MeshEditError};

const GLB_MAGIC: u32 = 0x4654_6C67;
const GLB_VERSION: u32 = 2;
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;
const COMPONENT_FLOAT: u32 = 5126;
const COMPONENT_UINT: u32 = 5125;
const TARGET_ARRAY_BUFFER: u32 = 34962;
const TARGET_ELEMENT_ARRAY_BUFFER: u32 = 34963;

fn gltf_err(msg: impl Into<String>) -> MeshEditError {
    MeshEditError::Gltf(msg.into())
}

/// Lee la primera primitiva de la primera malla de un .glb/.gltf.
pub fn read_glb(path: &Path) -> Result<MeshData, MeshEditError> {
    let (doc, buffers, _images) =
        gltf::import(path).map_err(|e| gltf_err(format!("{}: {e}", path.display())))?;
    let mesh = doc
        .meshes()
        .next()
        .ok_or_else(|| gltf_err("el archivo no tiene mallas"))?;
    let prim = mesh
        .primitives()
        .next()
        .ok_or_else(|| gltf_err("la malla no tiene primitivas"))?;
    let reader = prim.reader(|b| buffers.get(b.index()).map(|d| d.0.as_slice()));

    let positions: Vec<[f32; 3]> = reader
        .read_positions()
        .ok_or_else(|| gltf_err("la primitiva no tiene POSITION"))?
        .collect();
    let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
    let uvs: Option<Vec<[f32; 2]>> = reader.read_tex_coords(0).map(|t| t.into_f32().collect());
    let indices: Vec<u32> = match reader.read_indices() {
        Some(i) => i.into_u32().collect(),
        None => (0..positions.len() as u32).collect(),
    };

    let data = MeshData {
        positions,
        normals,
        uvs,
        indices,
    };
    data.validate_shape()?;
    Ok(data)
}

/// Escribe `mesh` como .glb (una escena, un nodo, una malla, una primitiva).
pub fn write_glb(mesh: &MeshData, path: &Path) -> Result<(), MeshEditError> {
    if mesh.positions.is_empty() || mesh.indices.is_empty() {
        return Err(gltf_err("malla vacía"));
    }
    mesh.validate_shape()?;

    let flat;
    let normals: &[[f32; 3]] = match &mesh.normals {
        Some(n) => n,
        None => {
            flat = flat_normals(mesh);
            &flat
        }
    };

    let mut b = BinBuilder::default();
    let mut attributes = Map::new();

    let (min, max) = bounds(&mesh.positions);
    let pos = b.add(
        &le_bytes_f32(&mesh.positions),
        TARGET_ARRAY_BUFFER,
        json!({
            "componentType": COMPONENT_FLOAT,
            "count": mesh.positions.len(),
            "type": "VEC3",
            "min": min,
            "max": max,
        }),
    );
    attributes.insert("POSITION".to_string(), json!(pos));

    let nor = b.add(
        &le_bytes_f32(normals),
        TARGET_ARRAY_BUFFER,
        json!({
            "componentType": COMPONENT_FLOAT,
            "count": normals.len(),
            "type": "VEC3",
        }),
    );
    attributes.insert("NORMAL".to_string(), json!(nor));

    if let Some(uvs) = &mesh.uvs {
        let uv = b.add(
            &le_bytes_f32(uvs),
            TARGET_ARRAY_BUFFER,
            json!({
                "componentType": COMPONENT_FLOAT,
                "count": uvs.len(),
                "type": "VEC2",
            }),
        );
        attributes.insert("TEXCOORD_0".to_string(), json!(uv));
    }

    let index_bytes: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
    let idx = b.add(
        &index_bytes,
        TARGET_ELEMENT_ARRAY_BUFFER,
        json!({
            "componentType": COMPONENT_UINT,
            "count": mesh.indices.len(),
            "type": "SCALAR",
        }),
    );

    let BinBuilder {
        mut bin,
        views,
        accessors,
    } = b;
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }

    let doc = json!({
        "asset": {"version": "2.0", "generator": "glass_mesh_edit"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{"attributes": attributes, "indices": idx, "mode": 4}]}],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": views,
        "accessors": accessors,
    });
    let mut json_bytes = serde_json::to_vec(&doc).map_err(|e| gltf_err(e.to_string()))?;
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }

    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let total_u32 = u32::try_from(total).map_err(|_| gltf_err("el .glb supera 4 GiB"))?;
    let mut out = Vec::with_capacity(total);
    for word in [
        GLB_MAGIC,
        GLB_VERSION,
        total_u32,
        json_bytes.len() as u32,
        CHUNK_JSON,
    ] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
    out.extend_from_slice(&bin);

    std::fs::write(path, out).map_err(|e| gltf_err(format!("{}: {e}", path.display())))
}

/// Acumula el chunk BIN, los bufferViews y los accessors.
#[derive(Default)]
struct BinBuilder {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl BinBuilder {
    /// Añade `bytes` como bufferView nuevo y un accessor sobre él; devuelve el índice del accessor.
    fn add(&mut self, bytes: &[u8], target: u32, mut accessor: Value) -> usize {
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        self.views.push(json!({
            "buffer": 0,
            "byteOffset": offset,
            "byteLength": bytes.len(),
            "target": target,
        }));
        accessor["bufferView"] = json!(self.views.len() - 1);
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }
}

fn le_bytes_f32<const N: usize>(v: &[[f32; N]]) -> Vec<u8> {
    v.iter().flatten().flat_map(|c| c.to_le_bytes()).collect()
}

fn bounds(p: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    p.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), v| {
        (
            std::array::from_fn(|k| lo[k].min(v[k])),
            std::array::from_fn(|k| hi[k].max(v[k])),
        )
    })
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

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();
    if len > 1e-12 {
        v.map(|c| c / len)
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Normales por cara asignadas a los vértices de cada triángulo (si un vértice se comparte, se promedian).
fn flat_normals(mesh: &MeshData) -> Vec<[f32; 3]> {
    let mut acc = vec![[0.0f32; 3]; mesh.positions.len()];
    for tri in mesh.indices.as_chunks::<3>().0 {
        let [ia, ib, ic] = *tri;
        let a = mesh.positions[ia as usize];
        let b = mesh.positions[ib as usize];
        let c = mesh.positions[ic as usize];
        let n = cross(sub(b, a), sub(c, a));
        for i in [ia, ib, ic] {
            let v = &mut acc[i as usize];
            v[0] += n[0];
            v[1] += n[1];
            v[2] += n[2];
        }
    }
    acc.into_iter().map(normalize).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volumen(m: &MeshData) -> f32 {
        let suma: f32 = m
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let a = m.positions[t[0] as usize];
                let b = m.positions[t[1] as usize];
                let c = m.positions[t[2] as usize];
                dot(a, cross(b, c))
            })
            .sum();
        (suma / 6.0).abs()
    }

    #[test]
    fn glb_roundtrip_cubo() {
        let cubo = MeshData::cube(2.0);
        let ruta = std::env::temp_dir().join("glass_mesh_edit_test_cubo.glb");
        write_glb(&cubo, &ruta).unwrap();
        let leido = read_glb(&ruta).unwrap();
        let _ = std::fs::remove_file(&ruta);

        assert_eq!(leido.positions.len(), 24);
        assert_eq!(leido.indices.len(), 36);
        assert!((volumen(&leido) - volumen(&cubo)).abs() < 1e-4);
        assert!((volumen(&leido) - 8.0).abs() < 1e-4);
    }

    #[test]
    fn glb_sin_normales_escribe_planas() {
        let cubo = MeshData::cube(2.0);
        let malla = MeshData {
            positions: cubo.positions,
            normals: None,
            uvs: None,
            indices: cubo.indices,
        };
        let ruta = std::env::temp_dir().join("glass_mesh_edit_test_sin_normales.glb");
        write_glb(&malla, &ruta).unwrap();
        let leido = read_glb(&ruta).unwrap();
        let _ = std::fs::remove_file(&ruta);

        assert_eq!(leido.normals.map(|n| n.len()), Some(24));
        assert!(leido.uvs.is_none());
    }

    #[test]
    fn glb_malla_vacia_da_error() {
        let vacia = MeshData {
            positions: Vec::new(),
            normals: None,
            uvs: None,
            indices: Vec::new(),
        };
        let ruta = std::env::temp_dir().join("glass_mesh_edit_test_vacia.glb");
        assert!(matches!(
            write_glb(&vacia, &ruta),
            Err(MeshEditError::Gltf(_))
        ));
    }
}
