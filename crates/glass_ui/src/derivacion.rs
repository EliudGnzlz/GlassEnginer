//! Derivación escena → documento(s). Reglas (diseño de la fase,
//! defecto 10 de la revisión cruzada):
//! - Un nodo por cada entidad con `rect_transform` bajo un Canvas.
//! - Un Canvas raíz abre documento; un Canvas anidado es un nodo más
//!   del documento padre (hereda rect y escala; solo aporta frontera
//!   de invalidación y override sorting).
//! - Como máximo un Graphic por nodo: imagen y texto a la vez →
//!   manda la imagen y se anota el aviso (la migración separa el
//!   texto en un hijo sintético antes de derivar).
//! - Inactivos: la entidad o cualquier ancestro UI inactivo marca el
//!   nodo `activo = false`.
//! - El subárbol de un Canvas se recorre por la jerarquía de la
//!   escena (parent/children); las entidades sin rect_transform no
//!   son nodos pero SÍ se atraviesan (sus hijos UI cuelgan del
//!   ancestro UI más cercano).

use std::collections::{HashMap, HashSet};

use glass_core::scene::{EntityData, RectTransformData, SceneState, UIInputFieldData, UITextData};

/// Texto visible de un InputField (V4, Q1): el valor si no está vacío;
/// si lo está, el placeholder con el alfa a la mitad. El resto sale del
/// campo (tamaño, color, fuente por defecto, alineación izquierda).
fn texto_de_input(inp: &UIInputFieldData) -> UITextData {
    let (texto, color) = if inp.text.is_empty() {
        let mut c = inp.color;
        c[3] *= 0.5;
        (inp.placeholder.clone(), c)
    } else {
        (inp.text.clone(), inp.color)
    };
    UITextData {
        text: texto,
        font_size: inp.font_size,
        color,
        font: "default".into(),
        align: "Left".into(),
        style_extra: String::new(),
        raycast_target: true,
    }
}

use crate::documento::{Documento, NodoDocumento, TipoGraphic};

pub fn derivar_documentos(scene: &SceneState) -> Vec<Documento> {
    let by_id: HashMap<u64, &EntityData> = scene.entities.iter().map(|e| (e.id, e)).collect();

    // Orden de documentos = orden de las raíces en `scene.entities`.
    scene
        .entities
        .iter()
        .filter(|e| e.canvas.is_some() && !tiene_ancestro_canvas(e, &by_id))
        .map(|raiz| derivar_documento(raiz, &by_id))
        .collect()
}

/// Sube por la cadena `parent` buscando un Canvas. Un ciclo o un padre
/// que no existe en la escena cortan la búsqueda (sin ancestro Canvas).
fn tiene_ancestro_canvas(ent: &EntityData, by_id: &HashMap<u64, &EntityData>) -> bool {
    let mut visitados = HashSet::new();
    visitados.insert(ent.id);
    let mut actual = ent.parent;
    while let Some(id) = actual {
        if !visitados.insert(id) {
            return false;
        }
        let Some(padre) = by_id.get(&id) else {
            return false;
        };
        if padre.canvas.is_some() {
            return true;
        }
        actual = padre.parent;
    }
    false
}

fn derivar_documento(raiz: &EntityData, by_id: &HashMap<u64, &EntityData>) -> Documento {
    let mut nodos: Vec<NodoDocumento> = Vec::new();
    let mut avisos: Vec<String> = Vec::new();
    let mut visitados: HashSet<u64> = HashSet::new();
    visitados.insert(raiz.id);

    // La raíz siempre es nodo; sin RectTransform se sintetiza uno a
    // pantalla completa.
    let rect_raiz = match &raiz.rect_transform {
        Some(rect) => rect.clone(),
        None => {
            avisos.push(format!(
                "Canvas raíz '{}' (id {}) sin RectTransform: se sintetiza uno a pantalla completa",
                raiz.name, raiz.id
            ));
            RectTransformData {
                anchored_position: [0.0, 0.0],
                size_delta: [0.0, 0.0],
                anchor_min: [0.0, 0.0],
                anchor_max: [1.0, 1.0],
                pivot: [0.5, 0.5],
            }
        }
    };
    let canvas_raiz = nodos.len();
    nodos.push(construir_nodo(raiz, None, raiz.active, rect_raiz, true, &mut avisos));

    // Pila del DFS en pre-orden: (entidad, nodo ancestro UI más
    // cercano, todos los ancestros recorridos activos). Los hijos se
    // apilan invertidos para respetar el orden guardado en `children`.
    let mut pila: Vec<(u64, usize, bool)> = raiz
        .children
        .iter()
        .rev()
        .map(|&h| (h, canvas_raiz, raiz.active))
        .collect();

    while let Some((id, ancestro_ui, ancestros_activos)) = pila.pop() {
        // Ciclos o hijos repetidos: cada entidad se procesa una vez.
        if !visitados.insert(id) {
            continue;
        }
        // Hijo que no existe en la escena: se ignora.
        let Some(ent) = by_id.get(&id).copied() else {
            continue;
        };

        let activo = ancestros_activos && ent.active;
        let mut ancestro_hijos = ancestro_ui;

        if let Some(rect) = &ent.rect_transform {
            let indice = nodos.len();
            nodos.push(construir_nodo(
                ent,
                Some(ancestro_ui),
                activo,
                rect.clone(),
                false,
                &mut avisos,
            ));
            nodos[ancestro_ui].hijos.push(indice);
            ancestro_hijos = indice;
        }
        // Sin rect: se atraviesa y sus hijos cuelgan del ancestro UI.

        pila.extend(
            ent.children
                .iter()
                .rev()
                .map(|&h| (h, ancestro_hijos, activo)),
        );
    }

    Documento {
        canvas_raiz,
        nodos,
        avisos,
    }
}

fn construir_nodo(
    ent: &EntityData,
    padre: Option<usize>,
    activo: bool,
    rect_transform: RectTransformData,
    es_canvas_raiz: bool,
    avisos: &mut Vec<String>,
) -> NodoDocumento {
    let imagen = ent.ui_image.clone();
    let texto_real = ent.ui_text.clone();
    let boton = ent.ui_button.clone();
    let input = ent.ui_input_field.clone();

    // InputField (V4, Q1): el texto que se ve se sintetiza del campo —
    // el valor si no está vacío, el placeholder atenuado si lo está — y
    // el fondo lo pone la imagen del nodo. Sin ui_text en la entidad.
    let texto = if let Some(ref inp) = input {
        Some(texto_de_input(inp))
    } else {
        texto_real.clone()
    };

    let graphic = if input.is_some() {
        TipoGraphic::InputField
    } else if imagen.is_some() {
        if texto_real.is_some() {
            avisos.push(format!(
                "Entidad '{}' (id {}) tiene UIImage y UIText: manda la imagen; el texto queda fuera hasta que la migración lo separe en un hijo",
                ent.name, ent.id
            ));
        }
        TipoGraphic::Imagen
    } else if texto.is_some() {
        TipoGraphic::Texto
    } else {
        TipoGraphic::Ninguno
    };

    let style_extra = if let Some(i) = &imagen {
        i.style_extra.clone()
    } else if let Some(t) = &texto {
        t.style_extra.clone()
    } else if let Some(b) = &boton {
        b.style_extra.clone()
    } else {
        String::new()
    };

    // Un botón sin Graphic sigue recibiendo eventos.
    let raycast_target = if let Some(i) = &imagen {
        i.raycast_target
    } else if let Some(t) = &texto {
        t.raycast_target
    } else {
        boton.is_some()
    };

    NodoDocumento {
        entidad: ent.id,
        nombre: ent.name.clone(),
        padre,
        hijos: Vec::new(),
        graphic,
        es_canvas: ent.canvas.is_some(),
        es_canvas_raiz,
        activo,
        rect_transform,
        canvas: ent.canvas.clone(),
        imagen,
        texto,
        boton,
        drag: ent.ui_drag.clone(),
        layout_group: ent.layout_group.clone(),
        layout_element: ent.layout_element.clone(),
        content_size_fitter: ent.content_size_fitter.clone(),
        style_extra,
        raycast_target,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glass_core::scene::{parsear_escena, CanvasData, UIInputFieldData, UIImageData, UITextData};

    fn entidad(id: u64, nombre: &str) -> EntityData {
        let mut e = EntityData::new(id, nombre.to_string(), Vec::new());
        e.active = true;
        e
    }

    fn rect() -> RectTransformData {
        RectTransformData {
            anchored_position: [0.0, 0.0],
            size_delta: [100.0, 100.0],
            anchor_min: [0.5, 0.5],
            anchor_max: [0.5, 0.5],
            pivot: [0.5, 0.5],
        }
    }

    fn canvas() -> CanvasData {
        CanvasData {
            render_mode: "ScreenSpaceOverlay".to_string(),
            reference_resolution: [1920.0, 1080.0],
            match_width_or_height: 0.5,
        }
    }

    // Se asume `Default` en los datos de Graphic (campos con serde default).
    fn imagen(style: &str) -> UIImageData {
        UIImageData {
            style_extra: style.to_string(),
            raycast_target: true,
            ..Default::default()
        }
    }

    fn texto(style: &str) -> UITextData {
        UITextData {
            style_extra: style.to_string(),
            raycast_target: false,
            ..Default::default()
        }
    }

    fn canvas_raiz(id: u64) -> EntityData {
        let mut e = entidad(id, "Canvas");
        e.canvas = Some(canvas());
        e.rect_transform = Some(rect());
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

    fn nodo_de(doc: &Documento, id: u64) -> &NodoDocumento {
        doc.nodos
            .iter()
            .find(|n| n.entidad == id)
            .expect("nodo de la entidad")
    }

    fn comprobar_consistencia(doc: &Documento) {
        for (i, n) in doc.nodos.iter().enumerate() {
            for &h in &n.hijos {
                assert_eq!(doc.nodos[h].padre, Some(i));
            }
            if let Some(p) = n.padre {
                assert!(doc.nodos[p].hijos.contains(&i));
            }
        }
    }

    #[test]
    fn sin_canvas_no_hay_documentos() {
        let mut a = entidad(1, "A");
        a.rect_transform = Some(rect());
        let mut ents = vec![a, entidad(2, "B")];
        enlazar(&mut ents, 1, 2);
        assert!(derivar_documentos(&escena(ents)).is_empty());
    }

    #[test]
    fn input_field_vacio_muestra_placeholder_atenuado() {
        let mut campo = entidad(2, "Campo");
        campo.rect_transform = Some(rect());
        campo.ui_image = Some(imagen(""));
        campo.ui_input_field = Some(UIInputFieldData {
            text: String::new(),
            placeholder: "Escribe...".into(),
            font_size: 20.0,
            color: [1.0, 1.0, 1.0, 1.0],
        });
        let mut ents = vec![canvas_raiz(1), campo];
        enlazar(&mut ents, 1, 2);

        let docs = derivar_documentos(&escena(ents));
        let n = nodo_de(&docs[0], 2);
        assert_eq!(n.graphic, TipoGraphic::InputField);
        let txt = n.texto.as_ref().expect("texto sintetizado");
        assert_eq!(txt.text, "Escribe...");
        assert_eq!(txt.font_size, 20.0);
        assert_eq!(txt.color[3], 0.5);
    }

    #[test]
    fn input_field_con_valor_muestra_el_valor() {
        let mut campo = entidad(2, "Campo");
        campo.rect_transform = Some(rect());
        campo.ui_image = Some(imagen(""));
        campo.ui_input_field = Some(UIInputFieldData {
            text: "hola".into(),
            placeholder: "Escribe...".into(),
            font_size: 20.0,
            color: [1.0, 1.0, 1.0, 1.0],
        });
        let mut ents = vec![canvas_raiz(1), campo];
        enlazar(&mut ents, 1, 2);

        let docs = derivar_documentos(&escena(ents));
        let n = nodo_de(&docs[0], 2);
        assert_eq!(n.graphic, TipoGraphic::InputField);
        let txt = n.texto.as_ref().expect("texto sintetizado");
        assert_eq!(txt.text, "hola");
        assert_eq!(txt.color[3], 1.0);
    }

    #[test]
    fn jerarquia_con_intermedio_sin_rect() {
        let mut nieto = entidad(3, "Nieto");
        nieto.rect_transform = Some(rect());
        nieto.ui_image = Some(imagen(""));
        let mut ents = vec![canvas_raiz(1), entidad(2, "Intermedio"), nieto];
        enlazar(&mut ents, 1, 2);
        enlazar(&mut ents, 2, 3);

        let docs = derivar_documentos(&escena(ents));
        assert_eq!(docs.len(), 1);
        let doc = &docs[0];
        assert_eq!(doc.nodos.len(), 2);
        let n = nodo_de(doc, 3);
        assert_eq!(n.padre, Some(0));
        assert_eq!(n.graphic, TipoGraphic::Imagen);
        assert!(doc.nodos[0].hijos.contains(&1));
        comprobar_consistencia(doc);
    }

    #[test]
    fn imagen_y_texto_avisa_y_manda_imagen() {
        let mut hijo = entidad(2, "Ambos");
        hijo.rect_transform = Some(rect());
        hijo.ui_image = Some(imagen("color: red;"));
        hijo.ui_text = Some(texto("color: blue;"));
        let mut ents = vec![canvas_raiz(1), hijo];
        enlazar(&mut ents, 1, 2);

        let docs = derivar_documentos(&escena(ents));
        let doc = &docs[0];
        let n = nodo_de(doc, 2);
        assert_eq!(n.graphic, TipoGraphic::Imagen);
        assert_eq!(n.style_extra, "color: red;");
        assert!(!doc.avisos.is_empty());
    }

    #[test]
    fn inactivo_se_propaga() {
        let mut intermedio = entidad(2, "Intermedio");
        intermedio.active = false;
        let mut hoja = entidad(3, "Hoja");
        hoja.rect_transform = Some(rect());
        let mut ents = vec![canvas_raiz(1), intermedio, hoja];
        enlazar(&mut ents, 1, 2);
        enlazar(&mut ents, 2, 3);

        let docs = derivar_documentos(&escena(ents));
        let doc = &docs[0];
        assert!(doc.nodos[doc.canvas_raiz].activo);
        assert!(!nodo_de(doc, 3).activo);
    }

    #[test]
    fn canvas_anidado_mismo_documento() {
        let mut anidado = canvas_raiz(2);
        anidado.name = "Anidado".to_string();
        let mut hoja = entidad(3, "Hoja");
        hoja.rect_transform = Some(rect());
        let mut ents = vec![canvas_raiz(1), anidado, hoja];
        enlazar(&mut ents, 1, 2);
        enlazar(&mut ents, 2, 3);

        let docs = derivar_documentos(&escena(ents));
        assert_eq!(docs.len(), 1);
        let doc = &docs[0];
        let n = nodo_de(doc, 2);
        assert!(n.es_canvas && !n.es_canvas_raiz);
        let h = nodo_de(doc, 3);
        assert_eq!(h.padre.map(|p| doc.nodos[p].entidad), Some(2));
        comprobar_consistencia(doc);
    }

    #[test]
    fn dos_raices_dos_documentos() {
        let ents = vec![canvas_raiz(1), canvas_raiz(2)];
        let docs = derivar_documentos(&escena(ents));
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].nodos[docs[0].canvas_raiz].entidad, 1);
        assert_eq!(docs[1].nodos[docs[1].canvas_raiz].entidad, 2);
    }

    #[test]
    fn escena_real_canvas_ui_test() {
        let json = include_str!("../../../assets/scenes/CanvasUiTest.scene.json");
        let (_version, entidades) = parsear_escena(json).expect("CanvasUiTest.scene.json parsea");
        let scene = escena(entidades);

        let docs = derivar_documentos(&scene);
        assert_eq!(docs.len(), 1);
        let doc = &docs[0];
        assert!(doc.nodos[doc.canvas_raiz].es_canvas_raiz);

        let mut ids = HashSet::new();
        for n in &doc.nodos {
            assert!(ids.insert(n.entidad), "entidad {} repetida", n.entidad);
        }
        comprobar_consistencia(doc);
    }
}
