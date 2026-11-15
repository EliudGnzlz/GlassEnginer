//! Sesión de edición de malla en proceso: malla editable, historial y receta.

use std::path::{Path, PathBuf};

use crate::commands::{CommandHistory, MeshCommand, Recipe};
use crate::edit_mesh::{EditMesh, ManifoldReport, MeshStats};
use crate::gltf_io::{read_glb, write_glb};
use crate::mesh_data::{MeshData, MeshEditError};

const WELD_TOLERANCE: f32 = 1e-4;
const MIN_EXTENT: f32 = 1e-6;

pub struct MeshSession {
    pub name: String,
    pub source_path: Option<PathBuf>,
    pub mesh: EditMesh,
    pub history: CommandHistory,
    pub recipe: Recipe,
}

impl MeshSession {
    /// Abre una sesión sobre `data` (soldada con 1e-4).
    pub fn enter(
        data: MeshData,
        name: &str,
        source_path: Option<PathBuf>,
    ) -> Result<Self, MeshEditError> {
        let mesh = EditMesh::from_mesh_data(&data, WELD_TOLERANCE)?;
        Ok(Self {
            name: name.to_string(),
            source_path,
            mesh,
            history: CommandHistory::new(),
            recipe: Recipe::new(name),
        })
    }

    /// Abre una sesión leyendo un .glb; el nombre es el `file_stem` de la ruta.
    pub fn enter_from_glb(path: &Path) -> Result<Self, MeshEditError> {
        let data = read_glb(path)?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mesh".to_string());
        Self::enter(data, &name, Some(path.to_path_buf()))
    }

    /// Ejecuta el comando y, si tiene éxito, lo registra en la receta.
    pub fn apply(&mut self, cmd: MeshCommand) -> Result<(), MeshEditError> {
        self.history.execute(&mut self.mesh, cmd.clone())?;
        self.recipe.push(cmd);
        Ok(())
    }

    /// Deshace en la sesión; la receta no cambia.
    pub fn undo(&mut self) -> Option<MeshCommand> {
        self.history.undo(&mut self.mesh)
    }

    /// Rehace en la sesión; la receta no cambia.
    pub fn redo(&mut self) -> Option<MeshCommand> {
        self.history.redo(&mut self.mesh)
    }

    /// Escala todos los vértices sobre el centro del bbox para que mida exactamente `size`.
    /// Los ejes con extensión < 1e-6 se dejan como están.
    pub fn set_dimensions(&mut self, size: [f32; 3]) -> Result<(), MeshEditError> {
        let s = self.mesh.stats();
        let center: [f32; 3] = std::array::from_fn(|k| 0.5 * (s.bbox_min[k] + s.bbox_max[k]));
        let factor: [f32; 3] = std::array::from_fn(|k| {
            let extent = s.bbox_max[k] - s.bbox_min[k];
            if extent < MIN_EXTENT {
                1.0
            } else {
                size[k] / extent
            }
        });
        let indices: Vec<u32> = (0..self.mesh.verts.len() as u32).collect();
        self.apply(MeshCommand::ScaleSelection {
            indices,
            center,
            factor,
        })
    }

    pub fn stats(&mut self) -> (MeshStats, ManifoldReport) {
        let stats = self.mesh.stats();
        let manifold = self.mesh.validate_manifold();
        (stats, manifold)
    }

    pub fn bake(&self) -> MeshData {
        self.mesh.bake()
    }

    /// Escribe el .glb horneado y la receta JSON, creando los directorios padre.
    pub fn save(&self, glb_path: &Path, recipe_path: &Path) -> Result<(), MeshEditError> {
        ensure_parent(glb_path)?;
        ensure_parent(recipe_path)?;
        write_glb(&self.bake(), glb_path)?;
        let json = self
            .recipe
            .to_json()
            .map_err(|e| MeshEditError::Gltf(e.to_string()))?;
        std::fs::write(recipe_path, json).map_err(io_err)
    }
}

fn io_err(e: std::io::Error) -> MeshEditError {
    MeshEditError::Gltf(e.to_string())
}

fn ensure_parent(path: &Path) -> Result<(), MeshEditError> {
    if let Some(parent) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(io_err)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cerca(a: [f32; 3], b: [f32; 3], tol: f32) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() <= tol)
    }

    #[test]
    fn sesion_aplica_y_deshace() {
        let mut s = MeshSession::enter(MeshData::cube(2.0), "cubo", None).unwrap();
        let original = s.mesh.verts[0];
        let movido = [original[0] + 1.0, original[1], original[2]];

        s.apply(MeshCommand::MoveVerts {
            indices: vec![0],
            delta: [1.0, 0.0, 0.0],
        })
        .unwrap();
        assert!(cerca(s.mesh.verts[0], movido, 1e-6));
        assert_eq!(s.recipe.commands.len(), 1);

        assert!(s.undo().is_some());
        assert!(cerca(s.mesh.verts[0], original, 1e-6));
        assert_eq!(s.recipe.commands.len(), 1);

        assert!(s.redo().is_some());
        assert!(cerca(s.mesh.verts[0], movido, 1e-6));
        assert_eq!(s.recipe.commands.len(), 1);
    }

    #[test]
    fn set_dimensions_exactas() {
        let mut s = MeshSession::enter(MeshData::cube(2.0), "cubo", None).unwrap();
        s.set_dimensions([4.0, 2.0, 6.0]).unwrap();
        let (st, _) = s.stats();
        assert!(cerca(st.bbox_min, [-2.0, -1.0, -3.0], 1e-5));
        assert!(cerca(st.bbox_max, [2.0, 1.0, 3.0], 1e-5));
    }

    #[test]
    fn save_escribe_glb_y_receta() {
        let mut s = MeshSession::enter(MeshData::cube(2.0), "cubo", None).unwrap();
        s.apply(MeshCommand::MoveVerts {
            indices: vec![0],
            delta: [0.5, 0.0, 0.0],
        })
        .unwrap();

        let dir = std::env::temp_dir();
        let glb = dir.join("glass_mesh_edit_test_sesion.glb");
        let receta = dir.join("glass_mesh_edit_test_sesion.mesh.json");
        s.save(&glb, &receta).unwrap();

        assert!(glb.exists());
        assert!(receta.exists());
        let leido = read_glb(&glb).unwrap();
        let texto = std::fs::read_to_string(&receta).unwrap();
        let _ = std::fs::remove_file(&glb);
        let _ = std::fs::remove_file(&receta);

        assert_eq!(leido.positions.len(), s.bake().positions.len());
        assert_eq!(Recipe::from_json(&texto).unwrap().commands.len(), 1);
    }
}
