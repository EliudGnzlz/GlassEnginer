//! Herramienta de V1 (fase UI/Canvas): vuelca los rects que produce
//! HOY el resolutor viejo (`glass_core::resolve_scene_ui_rects`)
//! sobre la escena de ejemplo, para congelarlos como fixture. En V2
//! el layout nuevo se compara contra este archivo (paridad).
//!
//! Uso: cargo run -p glass_ui --example snapshot_rects > fixture.json

use glass_core::scene::{parsear_escena, resolve_scene_ui_rects, SceneState};

fn main() {
    let ruta = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/scenes/CanvasUiTest.scene.json"
    );
    let json = std::fs::read_to_string(ruta).expect("leer la escena de ejemplo");
    let (version, entities) = parsear_escena(&json).expect("parsear la escena");
    let mut scene = SceneState::default();
    scene.ui_schema_version = version;
    scene.replace_entities(entities);

    let rects = resolve_scene_ui_rects(&scene, 1280.0, 720.0);
    let mut pares: Vec<(u64, [f32; 4])> = rects.into_iter().collect();
    pares.sort_by_key(|(id, _)| *id);
    let obj: serde_json::Map<String, serde_json::Value> = pares
        .into_iter()
        .map(|(id, r)| {
            (
                id.to_string(),
                serde_json::json!([r[0], r[1], r[2], r[3]]),
            )
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap()
    );

    // Conteo de la escena para el contrato de derivación (a stderr).
    let n_canvas = scene.entities.iter().filter(|e| e.canvas.is_some()).count();
    let n_rect = scene
        .entities
        .iter()
        .filter(|e| e.rect_transform.is_some())
        .count();
    let n_ambos = scene
        .entities
        .iter()
        .filter(|e| e.ui_image.is_some() && e.ui_text.is_some())
        .count();
    eprintln!(
        "version={version} entidades={} con_canvas={n_canvas} con_rect={n_rect} imagen_y_texto={n_ambos}",
        scene.entities.len()
    );
}
