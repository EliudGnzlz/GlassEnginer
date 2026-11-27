//! Sesión de edición de malla en proceso: malla editable, historial y receta.
//!
//! La receta de la sesión se deriva siempre de los comandos en vigor del
//! historial: lo deshecho no forma parte de la receta y no resucita al
//! reproducirla.

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
    /// Receta sincronizada con los comandos en vigor del historial.
    /// `version` y `base_asset` se fijan al entrar y se conservan.
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
        let mut session = Self {
            name: name.to_string(),
            source_path,
            mesh,
            history: CommandHistory::new(),
            recipe: Recipe::new(name),
        };
        session.sync_recipe();
        Ok(session)
    }

    /// Abre una sesión sobre la malla base `data` (soldada con 1e-4) y reproduce
    /// `recipe` a través del historial, de modo que los pasos reproducidos se
    /// pueden deshacer. Si algún comando falla, devuelve el error y no se crea
    /// sesión. La receta de la sesión conserva `version` y `base_asset` de `recipe`.
    pub fn enter_with_recipe(
        data: MeshData,
        recipe: &Recipe,
        name: &str,
        source_path: Option<PathBuf>,
    ) -> Result<Self, MeshEditError> {
        let mut session = Self::enter(data, name, source_path)?;
        for cmd in &recipe.commands {
            session.history.execute(&mut session.mesh, cmd.clone())?;
        }
        session.recipe = Recipe {
            version: recipe.version,
            base_asset: recipe.base_asset.clone(),
            commands: Vec::new(),
        };
        session.sync_recipe();
        Ok(session)
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

    /// Ejecuta el comando y, si tiene éxito, la receta pasa a incluirlo.
    pub fn apply(&mut self, cmd: MeshCommand) -> Result<(), MeshEditError> {
        self.history.execute(&mut self.mesh, cmd)?;
        self.sync_recipe();
        Ok(())
    }

    /// Deshace en la sesión; el comando deshecho sale de la receta.
    pub fn undo(&mut self) -> Option<MeshCommand> {
        let cmd = self.history.undo(&mut self.mesh);
        self.sync_recipe();
        cmd
    }

    /// Rehace en la sesión; el comando rehecho vuelve a la receta.
    pub fn redo(&mut self) -> Option<MeshCommand> {
        let cmd = self.history.redo(&mut self.mesh);
        self.sync_recipe();
        cmd
    }

    /// Receta derivada de los comandos en vigor, con la `version` y el
    /// `base_asset` fijados al entrar en la sesión.
    pub fn live_recipe(&self) -> Recipe {
        Recipe {
            version: self.recipe.version,
            base_asset: self.recipe.base_asset.clone(),
            commands: self.history.live_commands(),
        }
    }

    /// Reconstruye `self.recipe.commands` desde el historial conservando
    /// `version` y `base_asset`.
    fn sync_recipe(&mut self) {
        self.recipe.commands = self.history.live_commands();
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

    /// Escribe el .glb horneado y la receta viva en JSON, creando los directorios padre.
    pub fn save(&self, glb_path: &Path, recipe_path: &Path) -> Result<(), MeshEditError> {
        ensure_parent(glb_path)?;
        ensure_parent(recipe_path)?;
        write_glb(&self.bake(), glb_path)?;
        let json = self
            .live_recipe()
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

    fn cmd_a() -> MeshCommand {
        MeshCommand::MoveVerts {
            indices: vec![0],
            delta: [1.0, 0.0, 0.0],
        }
    }

    fn cmd_b() -> MeshCommand {
        MeshCommand::MoveVerts {
            indices: vec![1, 2],
            delta: [0.0, 2.0, -0.5],
        }
    }

    type Geometria = (Vec<[f32; 3]>, Vec<[u32; 3]>, Vec<Option<[f32; 2]>>);

    fn geometria(s: &MeshSession) -> Geometria {
        (
            s.mesh.verts.clone(),
            s.mesh.faces.clone(),
            s.mesh.vert_uv.clone(),
        )
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
        assert_eq!(s.recipe.commands.len(), 0);

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
        assert_eq!(s.recipe.commands.len(), 1);
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

    #[test]
    fn receta_tras_undo_solo_contiene_comandos_en_vigor() {
        let mut s = MeshSession::enter(MeshData::cube(2.0), "cubo", None).unwrap();
        s.apply(cmd_a()).unwrap();
        s.apply(cmd_b()).unwrap();
        assert!(s.undo().is_some());

        assert_eq!(s.live_recipe().commands, vec![cmd_a()]);
        assert_eq!(s.recipe.commands, vec![cmd_a()]);
        assert_eq!(s.recipe.base_asset, "cubo");
        assert_eq!(s.live_recipe(), s.recipe);

        let dir = std::env::temp_dir();
        let glb = dir.join("glass_mesh_edit_test_receta_viva.glb");
        let receta = dir.join("glass_mesh_edit_test_receta_viva.mesh.json");
        s.save(&glb, &receta).unwrap();

        let texto = std::fs::read_to_string(&receta).unwrap();
        let _ = std::fs::remove_file(&glb);
        let _ = std::fs::remove_file(&receta);

        let guardada = Recipe::from_json(&texto).unwrap();
        assert_eq!(guardada.commands, vec![cmd_a()]);
        assert_eq!(guardada.base_asset, "cubo");
    }

    #[test]
    fn redo_devuelve_el_comando_a_la_receta() {
        let mut s = MeshSession::enter(MeshData::cube(2.0), "cubo", None).unwrap();
        s.apply(cmd_a()).unwrap();
        s.apply(cmd_b()).unwrap();
        assert!(s.undo().is_some());
        assert_eq!(s.recipe.commands, vec![cmd_a()]);

        assert!(s.redo().is_some());
        assert_eq!(s.recipe.commands, vec![cmd_a(), cmd_b()]);
        assert_eq!(s.live_recipe().commands, vec![cmd_a(), cmd_b()]);
    }

    #[test]
    fn reentrada_con_receta_ida_y_vuelta() {
        let base = MeshData::cube(2.0);

        let mut s1 = MeshSession::enter(base.clone(), "cubo", None).unwrap();
        let geo_base = geometria(&s1);
        s1.apply(cmd_a()).unwrap();
        let geo_tras_a = geometria(&s1);
        s1.apply(cmd_b()).unwrap();
        let horneado1 = s1.bake();
        let receta = s1.live_recipe();

        let mut s2 =
            MeshSession::enter_with_recipe(base.clone(), &receta, "cubo", None).unwrap();
        assert_eq!(s2.recipe, receta);
        assert_eq!(geometria(&s2), geometria(&s1));
        assert_eq!(s2.bake(), horneado1);
        assert_eq!(receta.replay(&base, WELD_TOLERANCE).unwrap(), s2.bake());

        assert_eq!(s2.undo(), Some(cmd_b()));
        assert_eq!(geometria(&s2), geo_tras_a);
        assert_eq!(s2.recipe.commands, vec![cmd_a()]);

        assert_eq!(s2.undo(), Some(cmd_a()));
        assert_eq!(geometria(&s2), geo_base);
        assert!(s2.recipe.commands.is_empty());

        assert!(s2.undo().is_none());
    }

    #[test]
    fn reentrada_conserva_base_asset_de_la_receta() {
        let mut receta = Recipe::new("cubo_base");
        receta.push(cmd_a());

        let mut s =
            MeshSession::enter_with_recipe(MeshData::cube(2.0), &receta, "cubo_editado", None)
                .unwrap();
        assert_eq!(s.recipe.base_asset, "cubo_base");

        s.apply(cmd_b()).unwrap();
        assert_eq!(s.recipe.base_asset, "cubo_base");
        assert!(s.undo().is_some());
        assert!(s.undo().is_some());
        assert_eq!(s.recipe.base_asset, "cubo_base");
        assert!(s.redo().is_some());
        assert_eq!(s.recipe.base_asset, "cubo_base");
        assert_eq!(s.live_recipe().base_asset, "cubo_base");
        assert_eq!(s.recipe.version, receta.version);
    }

    #[test]
    fn reentrada_con_comando_invalido_falla() {
        let mut receta = Recipe::new("cubo");
        receta.push(cmd_a());
        receta.push(MeshCommand::MoveVerts {
            indices: vec![9999],
            delta: [1.0, 0.0, 0.0],
        });

        let r = MeshSession::enter_with_recipe(MeshData::cube(2.0), &receta, "cubo", None);
        assert!(matches!(r, Err(MeshEditError::IndexOutOfRange { .. })));
    }
}
