//! Nacimiento de sesiones de MeshForge desde la entidad seleccionada en la escena.
//! Modelo ProBuilder: el usuario edita el modelo de SU objeto, nunca elige archivo ni formato.

use std::path::{Path, PathBuf};

use glass_mesh_edit::{MeshData, MeshSession};
use glass_renderer::Vertex;

use crate::panels::project::subcarpeta;

/// El nombre vacío equivale a "cube" (primitiva por defecto).
pub fn normalize_model(model: &str) -> &str {
    if model.is_empty() {
        "cube"
    } else {
        model
    }
}

/// Primitivas generadas por el motor (sobre el nombre normalizado).
pub fn es_primitiva(modelo: &str) -> bool {
    matches!(
        normalize_model(modelo),
        "sphere" | "capsule" | "cylinder" | "plane"
    )
}

/// Malla exacta de una primitiva generada; `None` si no es primitiva.
pub fn mesh_data_primitiva(modelo: &str) -> Option<MeshData> {
    let (verts, indices) = match normalize_model(modelo) {
        "sphere" => glass_renderer::generate_sphere(),
        "capsule" => glass_renderer::generate_capsule(),
        "cylinder" => glass_renderer::generate_cylinder(),
        "plane" => glass_renderer::generate_plane(glass_renderer::PLANE_SEGMENTS),
        _ => return None,
    };
    Some(mesh_data_desde_vertices(verts, indices))
}

/// Convierte los vértices del renderer a `MeshData` editables.
fn mesh_data_desde_vertices(verts: Vec<Vertex>, indices: Vec<u32>) -> MeshData {
    let positions = verts.iter().map(|v| v.position).collect();
    let normals = verts.iter().map(|v| v.normal).collect();
    let uvs = verts.iter().map(|v| v.uv).collect();
    MeshData {
        positions,
        normals: Some(normals),
        uvs: Some(uvs),
        indices,
    }
}

/// Indica si el modelo puede abrirse en MeshForge y, si no, por qué.
#[derive(Debug, Clone, PartialEq)]
pub enum Editabilidad {
    Editable,
    NoEditable(String),
}

/// Motivo uniforme cuando el modelo no se halla como .glb.
fn motivo_no_encontrado(normalizado: &str) -> String {
    format!(
        "el modelo «{}» no se encuentra como .glb en las carpetas de modelos",
        normalizado
    )
}

/// Una primitiva es editable; un .glb resoluble por stem también.
pub fn editabilidad(modelo: &str, raices: &[&Path]) -> Editabilidad {
    let n = normalize_model(modelo);
    if es_primitiva(n) {
        return Editabilidad::Editable;
    }
    match glass_renderer::watcher::resolver_glb_por_stem(raices, n) {
        Some(_) => Editabilidad::Editable,
        None => Editabilidad::NoEditable(motivo_no_encontrado(n)),
    }
}

/// Abre una sesión de edición sobre la malla del modelo indicado.
pub fn abrir_sesion(modelo: &str, raices: &[&Path]) -> Result<MeshSession, String> {
    let n = normalize_model(modelo);
    if es_primitiva(n) {
        let data = mesh_data_primitiva(n).ok_or_else(|| motivo_no_encontrado(n))?;
        return MeshSession::enter(data, n, None).map_err(|e| e.to_string());
    }
    match glass_renderer::watcher::resolver_glb_por_stem(raices, n) {
        Some(ruta) => MeshSession::enter_from_glb(&ruta).map_err(|e| e.to_string()),
        None => Err(motivo_no_encontrado(n)),
    }
}

/// Raíces de búsqueda de modelos, en el mismo orden que usa el cargador del motor.
pub fn raices_modelos(assets_root: &Path) -> Vec<PathBuf> {
    vec![
        assets_root.to_path_buf(),
        PathBuf::from("assets/models"),
        PathBuf::from("assets"),
    ]
}

/// Directorio de modelos DEL PROYECTO.
pub fn dir_horneado(assets_root: &Path) -> PathBuf {
    subcarpeta(assets_root, "models", "Models")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(nombre: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "glass_editor_mesh_birth_{}_{}",
            std::process::id(),
            nombre
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn normaliza_modelo() {
        assert_eq!(normalize_model(""), "cube");
        assert_eq!(normalize_model("robot"), "robot");
    }

    #[test]
    fn reconoce_primitivas() {
        assert!(es_primitiva("sphere"));
        assert!(es_primitiva("capsule"));
        assert!(es_primitiva("cylinder"));
        assert!(es_primitiva("plane"));
        assert!(!es_primitiva("cube"));
        assert!(!es_primitiva("robot"));
        assert!(!es_primitiva(""));
    }

    #[test]
    fn malla_de_esfera() {
        let d = mesh_data_primitiva("sphere").expect("esfera");
        assert!(!d.positions.is_empty());
        assert_eq!(d.indices.len() % 3, 0);
        let normals = d.normals.expect("normales");
        assert_eq!(normals.len(), d.positions.len());
        assert!(mesh_data_primitiva("cube").is_none());
        assert!(mesh_data_primitiva("robot").is_none());
    }

    #[test]
    fn editabilidad_y_resolver_por_stem() {
        let dir = temp_dir("editabilidad");
        std::fs::write(dir.join("robot.glb"), b"").unwrap();
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("tank.glb"), b"").unwrap();

        let raices = [dir.as_path()];
        assert_eq!(editabilidad("robot", &raices), Editabilidad::Editable);
        assert_eq!(editabilidad("tank", &raices), Editabilidad::Editable);
        match editabilidad("avion", &raices) {
            Editabilidad::NoEditable(m) => assert!(m.contains("avion")),
            Editabilidad::Editable => panic!("avion no debería ser editable"),
        }
        assert_eq!(editabilidad("sphere", &[]), Editabilidad::Editable);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn abre_primitiva() {
        let s = abrir_sesion("sphere", &[]).expect("esfera");
        assert_eq!(s.name, "sphere");
        assert!(!s.mesh.verts.is_empty());
    }

    #[test]
    fn abre_modelo_inexistente() {
        let dir = temp_dir("vacio");
        let err = abrir_sesion("avion", &[dir.as_path()])
            .err()
            .expect("avion no debería abrirse");
        assert!(err.contains("avion"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn abre_glb_real() {
        let dir = temp_dir("glb");
        let malla = MeshData {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            normals: Some(vec![[0.0, 0.0, 1.0]; 4]),
            uvs: Some(vec![[0.0, 0.0]; 4]),
            indices: vec![0, 1, 2, 0, 2, 3, 0, 3, 1, 1, 3, 2],
        };
        let ruta = dir.join("tetra.glb");
        glass_mesh_edit::write_glb(&malla, &ruta).expect("escribir glb");

        let s = abrir_sesion("tetra", &[dir.as_path()]).expect("tetra");
        assert_eq!(s.name, "tetra");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn raices_del_modelo() {
        let root = Path::new("/proyecto/assets");
        let r = raices_modelos(root);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].as_path(), root);
    }

    #[test]
    fn direccion_de_horneado() {
        let dir = temp_dir("horneado");
        assert!(dir_horneado(&dir).ends_with("models"));
        std::fs::create_dir_all(dir.join("Models")).unwrap();
        assert!(dir_horneado(&dir).ends_with("Models"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
