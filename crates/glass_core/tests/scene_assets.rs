//! Validación de los assets de escena reales del proyecto.
//!
//! `SceneState` no lleva `#[serde(default)]` en sus structs: un campo nuevo
//! olvidado en un `.scene.json` a mano rompe la carga en tiempo de ejecución, no
//! en tiempo de compilación. Estos tests recorren `assets/scenes/` y exigen que
//! **todos** los assets del repositorio parseen y sobrevivan a un round-trip de
//! serialización, de modo que un asset corrupto falle en CI y no al abrir el
//! editor.

use std::path::{Path, PathBuf};

use glass_core::scene::EntityData;

/// Raíz del workspace (`crates/glass_core` -> `../..`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR debe tener un abuelo")
        .to_path_buf()
}

fn scene_assets() -> Vec<PathBuf> {
    let dir = workspace_root().join("assets/scenes");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("no se pudo leer {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().and_then(|s| s.to_str()) == Some("json")
                && p.file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|n| n.ends_with(".scene.json"))
        })
        .collect();
    out.sort();
    out
}

#[test]
fn todos_los_assets_de_escena_parsean() {
    let assets = scene_assets();
    assert!(
        !assets.is_empty(),
        "no se encontró ningún *.scene.json en assets/scenes"
    );

    for path in &assets {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("no se pudo leer {}: {e}", path.display()));
        let entities: Vec<EntityData> = serde_json::from_str(&text).unwrap_or_else(|e| {
            panic!(
                "el asset {} no parsea como Vec<EntityData>: {e}",
                path.display()
            )
        });
        assert!(
            !entities.is_empty(),
            "el asset {} no declara ninguna entidad",
            path.display()
        );
    }
}

#[test]
fn los_ids_de_entidad_son_unicos_y_las_jerarquias_son_coherentes() {
    for path in scene_assets() {
        let text = std::fs::read_to_string(&path).unwrap();
        let entities: Vec<EntityData> = serde_json::from_str(&text).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        let mut ids: Vec<u64> = entities.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        let total = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), total, "ids duplicados en {name}");

        // `replace_entities` renumera a partir de max(id)+1: un id 0 o un hueco
        // enorme no rompe nada, pero un duplicado sí deja entidades inalcanzables.
        for ent in &entities {
            if let Some(parent) = ent.parent {
                assert!(
                    entities.iter().any(|p| p.id == parent),
                    "{}: '{}' apunta a un padre inexistente ({parent})",
                    name,
                    ent.name
                );
            }
            for child in &ent.children {
                let c = entities
                    .iter()
                    .find(|e| e.id == *child)
                    .unwrap_or_else(|| {
                        panic!("{name}: '{}' declara un hijo inexistente ({child})", ent.name)
                    });
                assert_eq!(
                    c.parent,
                    Some(ent.id),
                    "{name}: '{}' figura como hijo de '{}' pero su parent es {:?}",
                    c.name,
                    ent.name,
                    c.parent
                );
            }
        }
    }
}

#[test]
fn el_round_trip_de_serializacion_no_pierde_campos() {
    for path in scene_assets() {
        let text = std::fs::read_to_string(&path).unwrap();
        let entities: Vec<EntityData> = serde_json::from_str(&text).unwrap();
        let reencoded = serde_json::to_string(&entities).unwrap();
        let again: Vec<EntityData> = serde_json::from_str(&reencoded).unwrap();

        assert_eq!(entities.len(), again.len(), "{}", path.display());
        for (a, b) in entities.iter().zip(again.iter()) {
            assert_eq!(a.id, b.id, "{}", path.display());
            assert_eq!(a.name, b.name, "{}", path.display());
            assert_eq!(a.components, b.components, "{}", path.display());
            assert_eq!(a.parent, b.parent, "{}", path.display());
            assert_eq!(a.children, b.children, "{}", path.display());
            assert_eq!(
                a.transform.position, b.transform.position,
                "{}: '{}'",
                path.display(),
                a.name
            );
            assert_eq!(
                a.transform.scale, b.transform.scale,
                "{}: '{}'",
                path.display(),
                a.name
            );
        }
    }
}