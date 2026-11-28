//! Mínimo S22 (P4c): una instancia de prefab con UI se abre y coloca
//! sus rects con paridad ≤0,5 px contra valores calculados a mano.
//! La instancia llega ya expandida (como la deja la resolución de
//! Fase E): la entidad raíz lleva `prefab_source`/`prefab_hash`/
//! `prefab_local_id` y sus hijos de UI cuelgan de ella.

use glass_core::scene::{
    CanvasData, EntityData, RectTransformData, SceneState, UIButtonData, UIImageData,
};
use glass_ui::api::{colocar_documento, Viewport};
use glass_ui::derivacion::derivar_documentos;

fn rt(anclado: [f32; 2], tamano: [f32; 2]) -> RectTransformData {
    RectTransformData {
        anchored_position: anclado,
        size_delta: tamano,
        anchor_min: [0.5, 0.5],
        anchor_max: [0.5, 0.5],
        pivot: [0.5, 0.5],
    }
}

#[test]
fn instancia_prefab_con_ui_coloca_rects() {
    let mut scene = SceneState::default();
    let mut canvas = EntityData::new(1, "Canvas".into(), vec![]);
    canvas.active = true;
    canvas.canvas = Some(CanvasData {
        render_mode: "ScreenSpaceOverlay".into(),
        reference_resolution: [1920.0, 1080.0],
        match_width_or_height: 0.5,
     ..Default::default() });
    canvas.rect_transform = Some(rt([0.0, 0.0], [1920.0, 1080.0]));
    canvas.children = vec![2];

    // Raíz de la instancia (viene de un .prefab.json): lleva el vínculo
    // vivo con el asset, como lo deja la resolución de Fase E.
    let mut raiz = EntityData::new(2, "BotonPrefab".into(), vec![]);
    raiz.active = true;
    raiz.parent = Some(1);
    raiz.children = vec![3];
    raiz.prefab_source = Some("assets/prefabs/Boton.prefab.json".into());
    raiz.prefab_hash = Some("abc123".into());
    raiz.prefab_local_id = Some(7);
    raiz.rect_transform = Some(rt([0.0, 0.0], [200.0, 60.0]));
    raiz.ui_button = Some(UIButtonData {
        interactable: true,
        target_graphic_color: [0.25, 0.55, 0.85, 1.0],
        on_click_action: "StartGame".into(),
        style_extra: String::new(),
    });
    raiz.ui_image = Some(UIImageData {
        sprite: String::new(),
        color: [0.25, 0.55, 0.85, 1.0],
        preserve_aspect: false,
        style_extra: String::new(),
        raycast_target: true,
    });

    // Hijo de la instancia (también del asset): la etiqueta del botón.
    let mut etiqueta = EntityData::new(3, "Etiqueta".into(), vec![]);
    etiqueta.active = true;
    etiqueta.parent = Some(2);
    etiqueta.prefab_source = Some("assets/prefabs/Boton.prefab.json".into());
    etiqueta.prefab_hash = Some("abc123".into());
    etiqueta.prefab_local_id = Some(8);

    scene.replace_entities(vec![canvas, raiz, etiqueta]);
    let docs = derivar_documentos(&scene);
    assert_eq!(docs.len(), 1);
    let colocado = colocar_documento(
        &docs[0],
        Viewport {
            ancho: 1280.0,
            alto: 720.0,
        },
    );
    // A mano: factor 2/3; centro de pantalla (640, 360); botón 200×60
    // en canvas → 133,33×40 en pantalla.
    let boton = colocado
        .nodos
        .iter()
        .find(|n| n.entidad == 2)
        .expect("botón colocado");
    let esperado = [573.3333, 340.0, 133.3333, 40.0];
    let got = [
        boton.rect.x,
        boton.rect.y,
        boton.rect.ancho,
        boton.rect.alto,
    ];
    for i in 0..4 {
        assert!(
            (got[i] - esperado[i]).abs() < 0.5,
            "coord {i}: {got:?} vs {esperado:?}"
        );
    }
}
