//! Port de los tests de `glass_app::canvas_ui` a la API de V2
//! (`glass_ui::api`). Todo está en rojo: las funciones del contrato
//! son stubs `todo!()` hasta P1–P4 (puerta G1).
//!
//! Reglas del port:
//! - Lote de quads → documento colocado: se comprueba que el nodo
//!   existe y su rect es el esperado (los conteos exactos de vértices
//!   y los segmentos por atlas son del pintor, P3; ver
//!   [FALTA EN CONTRATO]).
//! - `ButtonClickTracker` (pulsar/soltar con estado) → par de
//!   hit-tests (punto de pulsación y punto de suelta) más la acción
//!   leída del nodo; la máquina de estados es [FALTA EN CONTRATO].
//! - Las escenas se construyen a mano: la derivación solo produce
//!   documentos bajo un Canvas raíz, así que cada botón/texto cuelga
//!   de un Canvas creado en el test.

use glass_core::scene::{
    CanvasData, EntityData, RectTransformData, SceneState, UIButtonData, UIDragData, UIImageData,
    UITextData,
};
use glass_ui::api::{
    arrastre_pulsar, arrastre_soltar, colocar_documento, hit_test, DocumentoColocado, Viewport,
};
use glass_ui::derivacion::derivar_documentos;
use glass_ui::documento::Documento;

const VP: Viewport = Viewport {
    ancho: 1280.0,
    alto: 720.0,
};

fn canvas_raiz(id: u64) -> EntityData {
    let mut e = EntityData::new(id, "Canvas".into(), vec![]);
    e.active = true;
    // Referencia = viewport de los tests (1280×720): el Canvas Scaler
    // da factor 1 y los rects esperados quedan en píxeles directos.
    // (Con la referencia por defecto 1920×1080 el factor sería 2/3 y
    // todos los rects saldrían escalados, como manda el diseño.)
    e.canvas = Some(CanvasData {
        render_mode: "ScreenSpaceOverlay".into(),
        reference_resolution: [1280.0, 720.0],
        match_width_or_height: 0.5,
     ..Default::default() });
    e.rect_transform = Some(RectTransformData {
        anchored_position: [0.0, 0.0],
        size_delta: [0.0, 0.0],
        anchor_min: [0.0, 0.0],
        anchor_max: [1.0, 1.0],
        pivot: [0.5, 0.5],
    });
    e
}

fn rect_centrado(anchored: [f32; 2], size: [f32; 2]) -> RectTransformData {
    RectTransformData {
        anchored_position: anchored,
        size_delta: size,
        anchor_min: [0.5, 0.5],
        anchor_max: [0.5, 0.5],
        pivot: [0.5, 0.5],
    }
}

fn boton(id: u64, nombre: &str, accion: &str, size: [f32; 2]) -> EntityData {
    let mut e = EntityData::new(id, nombre.into(), vec![]);
    e.active = true;
    e.rect_transform = Some(rect_centrado([0.0, 0.0], size));
    e.ui_button = Some(UIButtonData {
        interactable: true,
        on_click_action: accion.into(),
        ..Default::default()
    });
    e
}

fn enlazar(ents: &mut [EntityData], padre: u64, hijo: u64) {
    for e in ents.iter_mut() {
        if e.id == padre {
            e.children.push(hijo);
        }
        if e.id == hijo {
            e.parent = Some(padre);
        }
    }
}

fn escena(ents: Vec<EntityData>) -> SceneState {
    let mut s = SceneState::default();
    s.replace_entities(ents);
    s
}

/// Escena con Canvas (id 1) y un botón centrado 220×56 (id 2):
/// rect [530, 332, 220, 56], centro (640, 360).
fn escena_boton() -> (SceneState, u64) {
    let mut ents = vec![canvas_raiz(1), boton(2, "Play", "StartGame", [220.0, 56.0])];
    enlazar(&mut ents, 1, 2);
    (escena(ents), 2)
}

fn colocar(scene: &SceneState) -> DocumentoColocado {
    colocar_con_doc(scene).1
}

/// Deriva el documento y lo coloca, devolviendo ambos: el hit-test de
/// P4 necesita el `Documento` (raycast_target/interactable/drag no
/// viajan en el colocado).
fn colocar_con_doc(scene: &SceneState) -> (Documento, DocumentoColocado) {
    let docs = derivar_documentos(scene);
    let doc = docs.into_iter().next().expect("un documento");
    let colocado = colocar_documento(&doc, VP);
    (doc, colocado)
}

fn rect_de(colocado: &DocumentoColocado, entidad: u64) -> [f32; 4] {
    let n = colocado
        .nodos
        .iter()
        .find(|n| n.entidad == entidad)
        .expect("nodo colocado de la entidad");
    [n.rect.x, n.rect.y, n.rect.ancho, n.rect.alto]
}

fn cerca(a: [f32; 4], b: [f32; 4]) {
    for i in 0..4 {
        assert!((a[i] - b[i]).abs() < 0.5, "rect {:?} != esperado {:?}", a, b);
    }
}

// --- Lote → colocado -------------------------------------------------------

#[test]
fn texto_del_hud_genera_nodo_colocado() {
    let mut texto = EntityData::new(2, "Score HUD Text".into(), vec![]);
    texto.active = true;
    texto.rect_transform = Some(RectTransformData {
        anchored_position: [20.0, 20.0],
        size_delta: [250.0, 40.0],
        anchor_min: [0.0, 0.0],
        anchor_max: [0.0, 0.0],
        pivot: [0.0, 0.0],
    });
    texto.ui_text = Some(UITextData {
        text: "Score: 0 / 100 pts".into(),
        font_size: 22.0,
        ..Default::default()
    });
    let mut ents = vec![canvas_raiz(1), texto];
    enlazar(&mut ents, 1, 2);
    let colocado = colocar(&escena(ents));
    // Ancla [0,0] = esquina inferior-izquierda (convención Unity del
    // viejo resolver): y = 720 - 20 - 40 = 660. El snapshot congelado
    // dice lo mismo para la entidad 11, con este mismo RectTransform.
    cerca(rect_de(&colocado, 2), [20.0, 660.0, 250.0, 40.0]);
}

#[test]
fn boton_genera_nodo_y_etiqueta() {
    let mut b = boton(2, "Play Button", "StartGame", [220.0, 56.0]);
    b.ui_text = Some(UITextData {
        text: "Jugar".into(),
        ..Default::default()
    });
    let mut ents = vec![canvas_raiz(1), b];
    enlazar(&mut ents, 1, 2);
    let colocado = colocar(&escena(ents));
    cerca(rect_de(&colocado, 2), [530.0, 332.0, 220.0, 56.0]);
}

#[test]
fn escena_sin_ui_no_genera_documento() {
    let mut s = SceneState::default();
    s.replace_entities(vec![EntityData::new(1, "Cube".into(), vec![])]);
    assert!(derivar_documentos(&s).is_empty());
}

#[test]
fn escena_canvas_ui_test_genera_colocado() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/scenes/CanvasUiTest.scene.json"
    );
    let mut scene = SceneState::default();
    scene.load_from_json(path).expect("la escena de prueba carga");
    let colocado = colocar(&scene);
    // Canvas + Score + botón JUGAR + título.
    assert_eq!(colocado.nodos.len(), 4);
    for n in &colocado.nodos {
        assert!(n.rect.ancho > 0.0 && n.rect.alto > 0.0, "nodo {} sin rect", n.entidad);
    }
}

#[test]
fn entidad_inactiva_rect_cero() {
    let mut texto = EntityData::new(2, "Hidden Text".into(), vec![]);
    texto.active = false;
    texto.rect_transform = Some(rect_centrado([0.0, 0.0], [250.0, 40.0]));
    texto.ui_text = Some(UITextData {
        text: "oculto".into(),
        ..Default::default()
    });
    let mut ents = vec![canvas_raiz(1), texto];
    enlazar(&mut ents, 1, 2);
    let colocado = colocar(&escena(ents));
    cerca(rect_de(&colocado, 2), [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn imagen_y_luego_boton_orden_de_pintado() {
    let mut img = EntityData::new(2, "imagen".into(), vec![]);
    img.active = true;
    img.rect_transform = Some(rect_centrado([0.0, 0.0], [100.0, 100.0]));
    img.ui_image = Some(UIImageData {
        sprite: "sprite.png".into(),
        ..Default::default()
    });
    let mut ents = vec![canvas_raiz(1), img, boton(3, "boton", "OK", [100.0, 40.0])];
    enlazar(&mut ents, 1, 2);
    enlazar(&mut ents, 1, 3);
    let colocado = colocar(&escena(ents));
    let pos = |id| colocado.nodos.iter().position(|n| n.entidad == id).unwrap();
    assert!(pos(2) < pos(3), "la imagen se pinta antes que el botón");
}

#[test]
fn preserve_aspect_encaja_y_centra() {
    // Port parcial: el rect colocado existe y es no vacío. La medida
    // exacta (200×100 en 100×100 → 100×50) exige el tamaño del sprite,
    // que el contrato no recibe: ver [FALTA EN CONTRATO].
    let mut e = EntityData::new(2, "imagen".into(), vec![]);
    e.active = true;
    e.rect_transform = Some(rect_centrado([0.0, 0.0], [100.0, 100.0]));
    e.ui_image = Some(UIImageData {
        sprite: "sprite.png".into(),
        preserve_aspect: true,
        ..Default::default()
    });
    let mut ents = vec![canvas_raiz(1), e];
    enlazar(&mut ents, 1, 2);
    let colocado = colocar(&escena(ents));
    let r = rect_de(&colocado, 2);
    assert!(r[2] > 0.0 && r[3] > 0.0, "la imagen debe colocarse");
}

#[test]
fn sin_ui_devuelve_documento_vacio() {
    let scene = SceneState::default();
    assert!(derivar_documentos(&scene).is_empty());
}

// --- Hit-test --------------------------------------------------------------

#[test]
fn hit_test_acierta_el_centro_del_boton() {
    let (scene, id) = escena_boton();
    let (doc, colocado) = colocar_con_doc(&scene);
    assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), Some(id));
}

#[test]
fn hit_test_falla_fuera_del_boton() {
    let (scene, _) = escena_boton();
    let (doc, colocado) = colocar_con_doc(&scene);
    assert_eq!(hit_test(&doc, &colocado, 10.0, 10.0), None);
    assert_eq!(hit_test(&doc, &colocado, 640.0, 100.0), None);
    assert_eq!(hit_test(&doc, &colocado, 100.0, 360.0), None);
}

#[test]
fn hit_test_bordes_izquierdo_y_superior_dentro_derecho_e_inferior_fuera() {
    let (scene, id) = escena_boton();
    let (doc, colocado) = colocar_con_doc(&scene);
    // Rect: x ∈ [530, 750), y ∈ [332, 388).
    assert_eq!(hit_test(&doc, &colocado, 530.0, 332.0), Some(id));
    assert_eq!(hit_test(&doc, &colocado, 749.9, 387.9), Some(id));
    assert_eq!(hit_test(&doc, &colocado, 750.0, 360.0), None);
    assert_eq!(hit_test(&doc, &colocado, 640.0, 388.0), None);
    assert_eq!(hit_test(&doc, &colocado, 529.9, 360.0), None);
}

#[test]
fn hit_test_omite_inactivos_y_no_interactuables() {
    let (mut scene, _) = escena_boton();
    scene.entities.iter_mut().find(|e| e.id == 2).unwrap().active = false;
    let (doc, colocado) = colocar_con_doc(&scene);
    assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), None);

    let (mut scene, id) = escena_boton();
    scene
        .entities
        .iter_mut()
        .find(|e| e.id == id)
        .unwrap()
        .ui_button
        .as_mut()
        .unwrap()
        .interactable = false;
    let (doc, colocado) = colocar_con_doc(&scene);
    assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), None);
}

#[test]
fn hit_test_con_solape_gana_el_ultimo_de_la_escena() {
    let mut ents = vec![
        canvas_raiz(1),
        boton(2, "Debajo", "A", [220.0, 56.0]),
        boton(3, "Encima", "B", [220.0, 56.0]),
    ];
    enlazar(&mut ents, 1, 2);
    enlazar(&mut ents, 1, 3);
    let (doc, colocado) = colocar_con_doc(&escena(ents));
    assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), Some(3));
}

// --- Clic (pulsar/soltar, sin máquina de estados: ver [FALTA EN CONTRATO]) ---

#[test]
fn clic_completo_pulsar_y_soltar_dentro() {
    let (scene, id) = escena_boton();
    let (doc, colocado) = colocar_con_doc(&scene);
    assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), Some(id));
    assert_eq!(hit_test(&doc, &colocado, 645.0, 355.0), Some(id));
    let ent = scene.entities.iter().find(|e| e.id == id).unwrap();
    assert_eq!(
        ent.ui_button.as_ref().unwrap().on_click_action, "StartGame",
        "la acción del botón es StartGame"
    );
}

#[test]
fn clic_se_cancela_al_soltar_fuera() {
    let (scene, id) = escena_boton();
    let (doc, colocado) = colocar_con_doc(&scene);
    assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), Some(id));
    assert_eq!(hit_test(&doc, &colocado, 20.0, 20.0), None);
}

#[test]
fn sin_clic_si_la_pulsacion_fue_fuera() {
    let (scene, id) = escena_boton();
    let (doc, colocado) = colocar_con_doc(&scene);
    assert_eq!(hit_test(&doc, &colocado, 20.0, 20.0), None);
    assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), Some(id));
}

#[test]
fn clic_no_salta_de_un_boton_a_otro() {
    let mut ents = vec![
        canvas_raiz(1),
        boton(2, "Debajo", "A", [220.0, 56.0]),
        boton(3, "Encima", "B", [220.0, 56.0]),
    ];
    enlazar(&mut ents, 1, 2);
    enlazar(&mut ents, 1, 3);
    let mut scene = escena(ents);
    let (doc, colocado) = colocar_con_doc(&scene);
    assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), Some(3));
    scene.entities.iter_mut().find(|e| e.id == 3).unwrap().active = false;
    let (doc, colocado) = colocar_con_doc(&scene);
    assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), Some(2));
}

#[test]
fn escena_real_canvas_ui_test_el_boton_jugar_recibe_el_clic() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/scenes/CanvasUiTest.scene.json"
    );
    let mut scene = SceneState::default();
    scene.load_from_json(path).expect("la escena de prueba carga");
    let (doc, colocado) = colocar_con_doc(&scene);
    let hit = hit_test(&doc, &colocado, 640.0, 360.0).expect("el centro cae en el botón JUGAR");
    let ent = scene.entities.iter().find(|e| e.id == hit).unwrap();
    assert_eq!(ent.name, "Start Button Test");
    assert_eq!(ent.ui_button.as_ref().unwrap().on_click_action, "StartGame");
}

// --- Drag & drop -----------------------------------------------------------

fn ui_arrastre(draggable: bool, accepts_drop: bool, begin: &str, end: &str, drop: &str) -> UIDragData {
    UIDragData {
        draggable,
        accepts_drop,
        on_begin_drag: begin.into(),
        on_drag_end: end.into(),
        on_drop: drop.into(),
    }
}

fn arrastrable(id: u64, nombre: &str, anchored: [f32; 2]) -> EntityData {
    let mut e = EntityData::new(id, nombre.into(), vec![]);
    e.active = true;
    e.rect_transform = Some(rect_centrado(anchored, [100.0, 100.0]));
    e.ui_drag = Some(ui_arrastre(true, false, "begin", "end", ""));
    e
}

#[test]
fn entity_new_deja_active_true_y_ui_none() {
    let e = EntityData::new(7, "nodo".into(), vec![]);
    assert!(e.active);
    assert!(e.ui_drag.is_none());
    assert!(e.rect_transform.is_none());
}

#[test]
fn press_devuelve_el_draggable_y_fuera_da_none() {
    let mut ents = vec![canvas_raiz(1), arrastrable(2, "arrastrable", [0.0, 0.0])];
    enlazar(&mut ents, 1, 2);
    // Viewport 800×600 en este test: el rect centrado es [350, 250, 100, 100].
    let docs = derivar_documentos(&escena(ents));
    let doc = docs.into_iter().next().expect("un documento");
    let colocado = colocar_documento(&doc, Viewport {
        ancho: 800.0,
        alto: 600.0,
    });
    let sesion = arrastre_pulsar(&doc, &colocado, 400.0, 300.0).expect("debe pulsar el draggable");
    assert_eq!(sesion.entidad, 2);
    assert_eq!(sesion.nombre, "arrastrable");
    assert_eq!(sesion.accion_inicio, "begin");
    assert_eq!(sesion.accion_fin, "end");
    assert!(arrastre_pulsar(&doc, &colocado, 10.0, 10.0).is_none());
}

#[test]
fn follow_conserva_el_agarre() {
    // Port parcial: solo la sesión al pulsar. `follow()` (nueva
    // anchored_position con el agarre) es [FALTA EN CONTRATO].
    let mut ents = vec![canvas_raiz(1), arrastrable(2, "arrastrable", [0.0, 0.0])];
    enlazar(&mut ents, 1, 2);
    let docs = derivar_documentos(&escena(ents));
    let doc = docs.into_iter().next().expect("un documento");
    let colocado = colocar_documento(&doc, Viewport {
        ancho: 800.0,
        alto: 600.0,
    });
    let sesion = arrastre_pulsar(&doc, &colocado, 400.0, 300.0).expect("debe pulsar el draggable");
    assert_eq!(sesion.entidad, 2);
}

#[test]
fn drop_target_excluye_al_arrastrado_y_da_none_sin_casilla() {
    let mut casilla = EntityData::new(10, "casilla".into(), vec![]);
    casilla.active = true;
    casilla.rect_transform = Some(rect_centrado([10.0, 10.0], [100.0, 100.0]));
    casilla.ui_drag = Some(ui_arrastre(false, true, "", "", "soltar"));
    let mut ents = vec![canvas_raiz(1), casilla, arrastrable(2, "arrastrado", [0.0, 0.0])];
    enlazar(&mut ents, 1, 10);
    enlazar(&mut ents, 1, 2);
    let docs = derivar_documentos(&escena(ents));
    let doc = docs.into_iter().next().expect("un documento");
    let colocado = colocar_documento(&doc, Viewport {
        ancho: 800.0,
        alto: 600.0,
    });
    let hit = arrastre_soltar(&doc, &colocado, 2, 400.0, 300.0).expect("debe encontrar la casilla");
    assert_eq!(hit.entidad, 10);

    let mut ents2 = vec![canvas_raiz(1), arrastrable(2, "arrastrado", [0.0, 0.0])];
    enlazar(&mut ents2, 1, 2);
    let docs2 = derivar_documentos(&escena(ents2));
    let doc2 = docs2.into_iter().next().expect("un documento");
    let colocado2 = colocar_documento(&doc2, Viewport {
        ancho: 800.0,
        alto: 600.0,
    });
    assert!(arrastre_soltar(&doc2, &colocado2, 2, 400.0, 300.0).is_none());
}

#[test]
fn un_no_draggable_encima_no_bloquea_al_draggable_de_debajo() {
    let mut encima = EntityData::new(3, "encima".into(), vec![]);
    encima.active = true;
    encima.rect_transform = Some(rect_centrado([0.0, 0.0], [100.0, 100.0]));
    encima.ui_drag = Some(ui_arrastre(false, false, "", "", ""));
    let mut ents = vec![canvas_raiz(1), arrastrable(2, "debajo", [0.0, 0.0]), encima];
    enlazar(&mut ents, 1, 2);
    enlazar(&mut ents, 1, 3);
    let docs = derivar_documentos(&escena(ents));
    let doc = docs.into_iter().next().expect("un documento");
    let colocado = colocar_documento(&doc, Viewport {
        ancho: 800.0,
        alto: 600.0,
    });
    let sesion = arrastre_pulsar(&doc, &colocado, 400.0, 300.0).expect("debe atravesar el no draggable");
    assert_eq!(sesion.entidad, 2);
    assert_eq!(sesion.nombre, "debajo");
}
