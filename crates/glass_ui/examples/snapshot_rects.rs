//! Herramienta de V2 (fase UI/Canvas): vuelca los rects que produce
//! el layout nuevo (derivar + colocar) sobre la escena de ejemplo,
//! para compararlos contra el fixture congelado en V1
//! (`tests/fixtures/rects_canvasuitest_1280x720.json`, paridad ≤0,5 px).
//!
//! Uso: cargo run -p glass_ui --example snapshot_rects > fixture.json

use glass_core::scene::{parsear_escena, SceneState};
use glass_ui::api::{colocar_documento, rects_colocados, Viewport};
use glass_ui::derivacion::derivar_documentos;

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

    let docs = derivar_documentos(&scene);
    let doc = docs.into_iter().next().expect("la escena tiene un Canvas");
    let colocado = colocar_documento(&doc, Viewport { ancho: 1280.0, alto: 720.0 });
    let rects = rects_colocados(&doc, &colocado);
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
