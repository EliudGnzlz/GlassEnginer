//! Capa de comandos de edición: comandos como datos serializables, historial
//! undo/redo basado en instantáneas y receta reproducible.
//!
//! La receta se aplica SIEMPRE sobre una malla base y nunca pisa ediciones
//! manuales: regenerar la base equivale a volver a reproducir la receta sobre
//! la base nueva. El asset editado nace con nombre propio, distinto del de la
//! base.

use serde::{Deserialize, Serialize};

use crate::boolean::BooleanOp;
use crate::edit_mesh::EditMesh;
use crate::mesh_data::{MeshData, MeshEditError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirrorAxis {
    X,
    Y,
    Z,
}

impl MirrorAxis {
    pub fn index(&self) -> usize {
        match self {
            MirrorAxis::X => 0,
            MirrorAxis::Y => 1,
            MirrorAxis::Z => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MeshCommand {
    MoveVerts { indices: Vec<u32>, delta: [f32; 3] },
    SetVert { index: u32, position: [f32; 3] },
    ScaleSelection { indices: Vec<u32>, center: [f32; 3], factor: [f32; 3] },
    Mirror { axis: MirrorAxis },
    Subdivide { levels: u32 },
    Array { count: u32, offset: [f32; 3] },
    Weld { tolerance: f32 },
    DeleteVerts { indices: Vec<u32> },
    Boolean { op: BooleanOp, other: MeshData },
}

impl MeshCommand {
    /// Nombre corto para el historial.
    pub fn name(&self) -> &'static str {
        match self {
            MeshCommand::MoveVerts { .. } => "move_verts",
            MeshCommand::SetVert { .. } => "set_vert",
            MeshCommand::ScaleSelection { .. } => "scale_selection",
            MeshCommand::Mirror { .. } => "mirror",
            MeshCommand::Subdivide { .. } => "subdivide",
            MeshCommand::Array { .. } => "array",
            MeshCommand::Weld { .. } => "weld",
            MeshCommand::DeleteVerts { .. } => "delete_verts",
            MeshCommand::Boolean { .. } => "boolean",
        }
    }
}

/// Ejecuta un comando sobre la malla. Si devuelve error, la malla queda intacta.
pub fn apply_command(mesh: &mut EditMesh, cmd: &MeshCommand) -> Result<(), MeshEditError> {
    match cmd {
        MeshCommand::MoveVerts { indices, delta } => transform_verts(mesh, indices, |p| {
            [p[0] + delta[0], p[1] + delta[1], p[2] + delta[2]]
        }),
        MeshCommand::SetVert { index, position } => mesh.set_vert(*index, *position),
        MeshCommand::ScaleSelection {
            indices,
            center,
            factor,
        } => transform_verts(mesh, indices, |p| {
            std::array::from_fn(|k| center[k] + (p[k] - center[k]) * factor[k])
        }),
        MeshCommand::Mirror { axis } => {
            crate::ops::mirror(mesh, *axis);
            Ok(())
        }
        MeshCommand::Subdivide { levels } => crate::ops::subdivide(mesh, *levels),
        MeshCommand::Array { count, offset } => crate::ops::array(mesh, *count, *offset),
        MeshCommand::Weld { tolerance } => crate::ops::weld(mesh, *tolerance),
        MeshCommand::DeleteVerts { indices } => crate::ops::delete_verts(mesh, indices),
        MeshCommand::Boolean { op, other } => crate::ops::apply_boolean(mesh, *op, other),
    }
}

/// Valida todos los índices antes de tocar nada; cada índice se transforma una vez.
fn transform_verts(
    mesh: &mut EditMesh,
    indices: &[u32],
    f: impl Fn([f32; 3]) -> [f32; 3],
) -> Result<(), MeshEditError> {
    let mut unique = indices.to_vec();
    unique.sort_unstable();
    unique.dedup();

    let mut updates = Vec::with_capacity(unique.len());
    for &i in &unique {
        let p = mesh
            .verts
            .get(i as usize)
            .copied()
            .ok_or(MeshEditError::IndexOutOfRange {
                index: i,
                verts: mesh.verts.len(),
            })?;
        updates.push((i, f(p)));
    }
    for (i, p) in updates {
        mesh.set_vert(i, p)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
struct MeshSnapshot {
    verts: Vec<[f32; 3]>,
    faces: Vec<[u32; 3]>,
    vert_uv: Vec<Option<[f32; 2]>>,
}

impl MeshSnapshot {
    fn capture(mesh: &EditMesh) -> Self {
        Self {
            verts: mesh.verts.clone(),
            faces: mesh.faces.clone(),
            vert_uv: mesh.vert_uv.clone(),
        }
    }

    /// Restaura la geometría. `last_weld` no se toca: es un informe, no estado topológico.
    fn restore(self, mesh: &mut EditMesh) {
        mesh.verts = self.verts;
        mesh.faces = self.faces;
        mesh.vert_uv = self.vert_uv;
        mesh.mark_dirty();
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommandHistory {
    /// (comando, instantánea previa a aplicarlo)
    past: Vec<(MeshCommand, MeshSnapshot)>,
    /// (comando, instantánea posterior a aplicarlo)
    future: Vec<(MeshCommand, MeshSnapshot)>,
}

impl CommandHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn execute(&mut self, mesh: &mut EditMesh, cmd: MeshCommand) -> Result<(), MeshEditError> {
        let before = MeshSnapshot::capture(mesh);
        apply_command(mesh, &cmd)?;
        self.past.push((cmd, before));
        self.future.clear();
        Ok(())
    }

    pub fn undo(&mut self, mesh: &mut EditMesh) -> Option<MeshCommand> {
        let (cmd, before) = self.past.pop()?;
        let after = MeshSnapshot::capture(mesh);
        before.restore(mesh);
        self.future.push((cmd.clone(), after));
        Some(cmd)
    }

    pub fn redo(&mut self, mesh: &mut EditMesh) -> Option<MeshCommand> {
        let (cmd, after) = self.future.pop()?;
        let before = MeshSnapshot::capture(mesh);
        after.restore(mesh);
        self.past.push((cmd.clone(), before));
        Some(cmd)
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn len(&self) -> usize {
        self.past.len()
    }

    pub fn is_empty(&self) -> bool {
        self.past.is_empty()
    }

    pub fn last(&self) -> Option<&MeshCommand> {
        self.past.last().map(|(cmd, _)| cmd)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    pub version: u32,
    pub base_asset: String,
    pub commands: Vec<MeshCommand>,
}

impl Recipe {
    pub fn new(base_asset: &str) -> Self {
        Self {
            version: 1,
            base_asset: base_asset.to_string(),
            commands: Vec::new(),
        }
    }

    pub fn push(&mut self, cmd: MeshCommand) {
        self.commands.push(cmd);
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> Result<Recipe, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Reproduce la receta sobre la malla base: suelda con `weld_tolerance` y
    /// aplica los comandos en orden. Si un comando falla, devuelve el error y la
    /// malla queda en el estado previo a ese comando.
    pub fn replay(&self, base: &MeshData, weld_tolerance: f32) -> Result<MeshData, MeshEditError> {
        let mut mesh = EditMesh::from_mesh_data(base, weld_tolerance)?;
        for cmd in &self.commands {
            apply_command(&mut mesh, cmd)?;
        }
        Ok(mesh.bake())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f32 = 1e-5;

    fn cube_mesh() -> EditMesh {
        EditMesh::from_mesh_data(&MeshData::cube(2.0), TOL).unwrap()
    }

    #[test]
    fn move_verts_y_undo() {
        let mut mesh = cube_mesh();
        let mut history = CommandHistory::new();
        let original = mesh.verts[0];
        assert!(!history.can_undo());
        assert!(!history.can_redo());

        history
            .execute(
                &mut mesh,
                MeshCommand::MoveVerts {
                    indices: vec![0],
                    delta: [1.0, 0.0, 0.0],
                },
            )
            .unwrap();
        let moved = [original[0] + 1.0, original[1], original[2]];
        assert_eq!(mesh.verts[0], moved);
        assert!(history.can_undo());
        assert!(!history.can_redo());
        assert_eq!(history.len(), 1);
        assert_eq!(history.last().map(MeshCommand::name), Some("move_verts"));

        let undone = history.undo(&mut mesh).unwrap();
        assert_eq!(undone.name(), "move_verts");
        assert_eq!(mesh.verts[0], original);
        assert!(!history.can_undo());
        assert!(history.can_redo());

        let redone = history.redo(&mut mesh).unwrap();
        assert_eq!(redone.name(), "move_verts");
        assert_eq!(mesh.verts[0], moved);
        assert!(history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn comando_fallido_no_apila() {
        let mut mesh = cube_mesh();
        let mut history = CommandHistory::new();
        let before = mesh.verts.clone();

        let err = history
            .execute(
                &mut mesh,
                MeshCommand::MoveVerts {
                    indices: vec![0, 9999],
                    delta: [1.0, 0.0, 0.0],
                },
            )
            .unwrap_err();
        assert!(matches!(err, MeshEditError::IndexOutOfRange { .. }));
        assert_eq!(history.len(), 0);
        assert!(history.is_empty());
        assert_eq!(mesh.verts, before);
    }

    #[test]
    fn receta_ida_y_vuelta_json() {
        let mut recipe = Recipe::new("cube_2m");
        recipe.push(MeshCommand::MoveVerts {
            indices: vec![0, 1],
            delta: [1.0, 0.5, -0.25],
        });
        recipe.push(MeshCommand::ScaleSelection {
            indices: vec![2, 3],
            center: [0.0, 0.0, 0.0],
            factor: [2.0, 1.0, 1.0],
        });

        let json = recipe.to_json().unwrap();
        let back = Recipe::from_json(&json).unwrap();
        assert_eq!(back, recipe);
        assert_eq!(back.version, 1);
    }

    #[test]
    fn replay_aplica_los_comandos() {
        let base = MeshData::cube(2.0);
        let plain = Recipe::new("cube_2m").replay(&base, TOL).unwrap();

        let mut recipe = Recipe::new("cube_2m");
        recipe.push(MeshCommand::MoveVerts {
            indices: vec![0],
            delta: [1.0, 0.0, 0.0],
        });
        let edited = recipe.replay(&base, TOL).unwrap();

        assert_eq!(edited.positions.len(), plain.positions.len());
        assert_ne!(edited.positions, plain.positions);

        let mut manual = cube_mesh();
        apply_command(
            &mut manual,
            &MeshCommand::MoveVerts {
                indices: vec![0],
                delta: [1.0, 0.0, 0.0],
            },
        )
        .unwrap();
        assert_eq!(manual.bake().positions, edited.positions);
    }

    #[test]
    fn set_vert_y_scale() {
        let mut mesh = cube_mesh();

        apply_command(
            &mut mesh,
            &MeshCommand::SetVert {
                index: 1,
                position: [5.0, 6.0, 7.0],
            },
        )
        .unwrap();
        assert_eq!(mesh.verts[1], [5.0, 6.0, 7.0]);

        let before = mesh.verts.clone();
        apply_command(
            &mut mesh,
            &MeshCommand::ScaleSelection {
                indices: vec![2, 3],
                center: [0.0, 0.0, 0.0],
                factor: [2.0, 1.0, 1.0],
            },
        )
        .unwrap();
        for i in [2usize, 3] {
            assert_eq!(mesh.verts[i], [before[i][0] * 2.0, before[i][1], before[i][2]]);
        }
        assert_eq!(mesh.verts[1], before[1]);
    }
}
