//! Malla indexada en el formato del motor y errores del crate.

use serde::{Deserialize, Serialize};

/// Errores de construcción y validación de mallas.
#[derive(Debug, thiserror::Error)]
pub enum MeshEditError {
    /// El número de índices no es múltiplo de 3.
    #[error("número de índices inválido: {len} no es múltiplo de 3")]
    InvalidIndices { len: usize },
    /// Un índice apunta fuera del array de vértices.
    #[error("índice {index} fuera de rango (hay {verts} vértices)")]
    IndexOutOfRange { index: u32, verts: usize },
    /// Un atributo opcional no tiene la misma longitud que `positions`.
    #[error("longitud de '{attribute}' incorrecta: se esperaban {expected}, hay {found}")]
    LengthMismatch {
        attribute: &'static str,
        expected: usize,
        found: usize,
    },
    /// La booleana produjo una malla que no es manifold cerrada.
    #[error("resultado de booleana no manifold: {boundary} aristas borde, {non_manifold} no manifold, {degenerate} degeneradas")]
    NonManifoldResult {
        boundary: usize,
        non_manifold: usize,
        degenerate: usize,
    },
    #[error("comando no implementado todavía: {command}")]
    NotImplemented { command: &'static str },
    /// Un parámetro supera el presupuesto de la operación.
    #[error("{what} excede el límite: {asked} > {limit}")]
    LimitExceeded {
        what: &'static str,
        limit: usize,
        asked: usize,
    },
}

/// Malla indexada en el formato del motor (la que viaja a GPU y a .glb).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub uvs: Option<Vec<[f32; 2]>>,
    pub indices: Vec<u32>, // triángulos: len % 3 == 0
}

/// Esquinas de una cara cuadrada: (signo en u, signo en v, uv).
/// Orden antihorario visto desde el lado de la normal.
const QUAD_CORNERS: [(f32, f32, [f32; 2]); 4] = [
    (-1.0, -1.0, [0.0, 0.0]),
    (1.0, -1.0, [1.0, 0.0]),
    (1.0, 1.0, [1.0, 1.0]),
    (-1.0, 1.0, [0.0, 1.0]),
];

/// Caras del cubo: (normal, tangente u, bitangente v) con u × v = normal.
const CUBE_FACES: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
    ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
    ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
    ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
];

impl MeshData {
    /// Comprueba la forma de los buffers: índices múltiplo de 3 y dentro de
    /// rango, y atributos opcionales con la misma longitud que `positions`.
    pub fn validate_shape(&self) -> Result<(), MeshEditError> {
        if !self.indices.len().is_multiple_of(3) {
            return Err(MeshEditError::InvalidIndices {
                len: self.indices.len(),
            });
        }
        let verts = self.positions.len();
        if let Some(&index) = self.indices.iter().find(|&&i| i as usize >= verts) {
            return Err(MeshEditError::IndexOutOfRange { index, verts });
        }
        if let Some(normals) = &self.normals {
            if normals.len() != verts {
                return Err(MeshEditError::LengthMismatch {
                    attribute: "normals",
                    expected: verts,
                    found: normals.len(),
                });
            }
        }
        if let Some(uvs) = &self.uvs {
            if uvs.len() != verts {
                return Err(MeshEditError::LengthMismatch {
                    attribute: "uvs",
                    expected: verts,
                    found: uvs.len(),
                });
            }
        }
        Ok(())
    }

    /// Cubo centrado en el origen de lado `size`: 24 vértices (4 por cara,
    /// normal por cara, uv 0..1 por cara) y 36 índices.
    pub fn cube(size: f32) -> MeshData {
        let h = size * 0.5;
        let mut positions = Vec::with_capacity(24);
        let mut normals = Vec::with_capacity(24);
        let mut uvs = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);

        for (n, u, v) in &CUBE_FACES {
            let base = positions.len() as u32;
            for &(su, sv, uv) in &QUAD_CORNERS {
                // Cada eje recibe exactamente un término ±1, así las posiciones
                // son exactamente ±h (bits idénticos entre caras).
                let p = [0usize, 1, 2].map(|k| (n[k] + u[k] * su + v[k] * sv) * h);
                positions.push(p);
                normals.push(*n);
                uvs.push(uv);
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }

        MeshData {
            positions,
            normals: Some(normals),
            uvs: Some(uvs),
            indices,
        }
    }

    /// Plano XZ centrado de lado `size`: 4 vértices, 2 triángulos, normal +Y.
    /// Malla abierta usada como fixture de validación.
    pub fn plane(size: f32) -> MeshData {
        let h = size * 0.5;
        let positions = vec![[-h, 0.0, h], [h, 0.0, h], [h, 0.0, -h], [-h, 0.0, -h]];
        let uvs = QUAD_CORNERS.iter().map(|&(_, _, uv)| uv).collect();
        MeshData {
            positions,
            normals: Some(vec![[0.0, 1.0, 0.0]; 4]),
            uvs: Some(uvs),
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubo_tiene_forma_valida() {
        let cube = MeshData::cube(2.0);
        assert_eq!(cube.positions.len(), 24);
        assert_eq!(cube.indices.len(), 36);
        assert!(cube.validate_shape().is_ok());
    }

    #[test]
    fn plano_tiene_forma_valida() {
        let plane = MeshData::plane(1.0);
        assert_eq!(plane.positions.len(), 4);
        assert_eq!(plane.indices.len(), 6);
        assert!(plane.validate_shape().is_ok());
    }

    #[test]
    fn indices_no_multiplo_de_tres() {
        let mut mesh = MeshData::plane(1.0);
        mesh.indices.pop();
        assert!(matches!(
            mesh.validate_shape(),
            Err(MeshEditError::InvalidIndices { len: 5 })
        ));
    }

    #[test]
    fn longitud_de_uvs_incorrecta() {
        let mut mesh = MeshData::plane(1.0);
        mesh.uvs = Some(vec![[0.0, 0.0]; 3]);
        assert!(matches!(
            mesh.validate_shape(),
            Err(MeshEditError::LengthMismatch {
                attribute: "uvs",
                expected: 4,
                found: 3
            })
        ));
    }

    #[test]
    fn ida_y_vuelta_json() {
        let cube = MeshData::cube(2.0);
        let json = serde_json::to_string(&cube).expect("serializar");
        let back: MeshData = serde_json::from_str(&json).expect("deserializar");
        assert_eq!(cube, back);
    }
}
