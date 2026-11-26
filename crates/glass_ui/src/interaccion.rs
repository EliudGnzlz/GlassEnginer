use crate::api::{DestinoSoltar, DocumentoColocado, SesionArrastre};
use crate::documento::Documento;

/// Devuelve los índices de `doc.nodos` en orden de pintado:
/// primero los nodos normales en orden DFS (0, 1, 2, …),
/// luego los de capa superior ordenados por (sort_order, posición DFS).
pub fn orden_pintado(_doc: &Documento, colocado: &DocumentoColocado) -> Vec<usize> {
    let mut normales: Vec<usize> = Vec::new();
    let mut superiores: Vec<(i32, usize, usize)> = Vec::new(); // (sort_order, dfs_pos, idx)

    for (dfs_pos, nc) in colocado.nodos.iter().enumerate() {
        let idx = nc.nodo;
        let es_superior = nc.estilos.override_sorting == Some(true);
        if es_superior {
            let so = nc.estilos.sort_order.unwrap_or(0);
            superiores.push((so, dfs_pos, idx));
        } else {
            normales.push(idx);
        }
    }

    // Los superiores se ordenan por (sort_order, posición DFS)
    superiores.sort_by_key(|&(so, dfs, _)| (so, dfs));

    let mut resultado = normales;
    for (_, _, idx) in superiores {
        resultado.push(idx);
    }
    resultado
}

/// Devuelve la entidad del nodo más al frente (último en orden de pintado)
/// que contenga el punto y sea raycast target válido.
///
/// Reglas de exclusión:
/// - Nodo inactivo
/// - `es_canvas_raiz == true`
/// - `raycast_target == false`
/// - Si tiene botón y `interactable == false`
/// - Si `doc.nodos.len() != colocado.nodos.len()` → None
pub fn hit_test(
    doc: &Documento,
    colocado: &DocumentoColocado,
    x: f32,
    y: f32,
) -> Option<u64> {
    if doc.nodos.len() != colocado.nodos.len() {
        return None;
    }

    let orden = orden_pintado(doc, colocado);

    // Recorremos al revés: el último pintado es el que está más arriba
    for &idx in orden.iter().rev() {
        let nodo = &doc.nodos[idx];
        let nc = &colocado.nodos[idx];

        if !nodo.activo {
            continue;
        }
        if nodo.es_canvas_raiz {
            continue;
        }
        if !nodo.raycast_target {
            continue;
        }
        if !nc.rect.contiene(x, y) {
            continue;
        }
        // Si tiene botón, debe ser interactable
        if let Some(ref btn) = nodo.boton {
            if !btn.interactable {
                continue;
            }
        }
        return Some(nodo.entidad);
    }
    None
}

/// Inicia un arrastre: busca el nodo draggable más al frente que contenga el punto.
///
/// Los nodos con `drag` presente pero `draggable == false` se saltan (no bloquean).
///
///
/// Campos de agarre:
/// - `agarre`: `[x - rect.x, y - rect.y]`
/// - `anclado_inicial`: `anchored_position` del `rect_transform` (o `[0.0, 0.0]`)
/// - `origen_inicial`: `[rect.x, rect.y]`
pub fn arrastre_pulsar(
    doc: &Documento,
    colocado: &DocumentoColocado,
    x: f32,
    y: f32,
) -> Option<SesionArrastre> {
    if doc.nodos.len() != colocado.nodos.len() {
        return None;
    }

    let orden = orden_pintado(doc, colocado);

    for &idx in orden.iter().rev() {
        let nodo = &doc.nodos[idx];
        let nc = &colocado.nodos[idx];

        if !nodo.activo {
            continue;
        }
        let drag = match &nodo.drag {
            Some(d) => d,
            None => continue,
        };
        if !drag.draggable {
            continue;
        }
        if !nc.rect.contiene(x, y) {
            continue;
        }

        let agarre = [x - nc.rect.x, y - nc.rect.y];
        let anclado_inicial = nodo.rect_transform.anchored_position;
        let origen_inicial = [nc.rect.x, nc.rect.y];

        return Some(SesionArrastre {
            entidad: nodo.entidad,
            nombre: nodo.nombre.clone(),
            accion_inicio: drag.on_begin_drag.clone(),
            accion_fin: drag.on_drag_end.clone(),
            agarre,
            anclado_inicial,
            origen_inicial,
        });
    }
    None
}

/// Busca un destino de soltar bajo el punto, excluyendo la entidad arrastrada.
pub fn arrastre_soltar(
    doc: &Documento,
    colocado: &DocumentoColocado,
    arrastrada: u64,
    x: f32,
    y: f32,
) -> Option<DestinoSoltar> {
    if doc.nodos.len() != colocado.nodos.len() {
        return None;
    }

    let orden = orden_pintado(doc, colocado);

    for &idx in orden.iter().rev() {
        let nodo = &doc.nodos[idx];
        let nc = &colocado.nodos[idx];

        if !nodo.activo {
            continue;
        }
        if nodo.entidad == arrastrada {
            continue;
        }
        let drag = match &nodo.drag {
            Some(d) => d,
            None => continue,
        };
        if !drag.accepts_drop {
            continue;
        }
        if !nc.rect.contiene(x, y) {
            continue;
        }

        let anclado = nodo.rect_transform.anchored_position;

        return Some(DestinoSoltar {
            entidad: nodo.entidad,
            nombre: nodo.nombre.clone(),
            anclado,
            accion_soltar: drag.on_drop.clone(),
        });
    }
    None
}

/// Máquina de estados para detectar clics en botones.
///
/// Al pulsar registra la entidad bajo el puntero. Al soltar, si la suelta
/// cae en la misma entidad (o en un descendiente con botón), devuelve la
/// `on_click_action` del botón. Si la entidad pulsada no tiene botón,
/// burbujea por los ancestros hasta encontrar uno interactable.
pub struct RastreadorClic {
    pulsado: Option<u64>,
}

impl RastreadorClic {
    /// Crea un rastreador sin pulsación activa.
    pub fn new() -> Self {
        Self { pulsado: None }
    }

    /// Registra la pulsación. Devuelve la entidad bajo el puntero, o None.
    pub fn pulsar(
        &mut self,
        doc: &Documento,
        colocado: &DocumentoColocado,
        x: f32,
        y: f32,
    ) -> Option<u64> {
        let ent = hit_test(doc, colocado, x, y);
        self.pulsado = ent;
        ent
    }

    /// Al soltar devuelve (entidad del botón, acción) si corresponde, y
    /// limpia el estado. La entidad es la del BOTÓN que aporta la acción
    /// (el propio nodo o su ancestro más cercano con botón interactable),
    /// no el nodo golpeado: si el clic cae sobre el texto hijo de un
    /// botón, la acción debe invocarse en el botón (ahí vive el script),
    /// no en la etiqueta.
    pub fn soltar(
        &mut self,
        doc: &Documento,
        colocado: &DocumentoColocado,
        x: f32,
        y: f32,
    ) -> Option<(u64, String)> {
        let ent_pulsada = self.pulsado.take()?;
        let ent_suelta = hit_test(doc, colocado, x, y)?;

        if ent_suelta != ent_pulsada {
            return None;
        }

        // Buscar el índice del nodo pulsado
        let idx = doc.nodos.iter().position(|n| n.entidad == ent_pulsada)?;

        // Intentar obtener la acción: primero del propio nodo, luego burbujeando
        Self::accion_boton(doc, idx)
    }

    /// Devuelve la entidad actualmente pulsada.
    pub fn pulsado(&self) -> Option<u64> {
        self.pulsado
    }

    /// Cancela un gesto a medias (p. ej. al cambiar de escena).
    pub fn cancelar(&mut self) {
        self.pulsado = None;
    }

    /// Busca (entidad del botón, acción) del nodo o de su ancestro más
    /// cercano con botón interactable.
    fn accion_boton(doc: &Documento, mut idx: usize) -> Option<(u64, String)> {
        loop {
            let nodo = &doc.nodos[idx];
            if let Some(ref btn) = nodo.boton {
                if btn.interactable {
                    return Some((nodo.entidad, btn.on_click_action.clone()));
                } else {
                    // Botón no interactable: no burbujea más
                    return None;
                }
            }
            // Subir al padre
            idx = nodo.padre?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{DocumentoColocado, NodoColocado, RectColocado};
    use crate::documento::{Documento, NodoDocumento, TipoGraphic};
    use crate::estilos::StyleProps;
    use glass_core::scene::{RectTransformData, UIButtonData, UIDragData};

    /// Helper: documento con canvas raíz + un botón.
    fn doc_boton(interactable: bool) -> (Documento, DocumentoColocado) {
        let nodos = vec![
            NodoDocumento {
                entidad: 1,
                nombre: "Canvas".into(),
                padre: None,
                hijos: vec![1],
                graphic: TipoGraphic::Ninguno,
                es_canvas: true,
                es_canvas_raiz: true,
                activo: true,
                rect_transform: RectTransformData {
                anchored_position: [0.0, 0.0],
                size_delta: [0.0, 0.0],
                anchor_min: [0.5, 0.5],
                anchor_max: [0.5, 0.5],
                pivot: [0.5, 0.5],
            },
                canvas: None,
                imagen: None,
                texto: None,
                boton: None,
                drag: None,
                layout_group: None,
                style_extra: String::new(),
                raycast_target: false,
            },
            NodoDocumento {
                entidad: 2,
                nombre: "Play".into(),
                padre: Some(0),
                hijos: vec![],
                graphic: TipoGraphic::Ninguno,
                es_canvas: false,
                es_canvas_raiz: false,
                activo: true,
                rect_transform: RectTransformData {
                anchored_position: [0.0, 0.0],
                size_delta: [0.0, 0.0],
                anchor_min: [0.5, 0.5],
                anchor_max: [0.5, 0.5],
                pivot: [0.5, 0.5],
            },
                canvas: None,
                imagen: None,
                texto: None,
                boton: Some(UIButtonData {
                    interactable,
                    target_graphic_color: [1.0, 1.0, 1.0, 1.0],
                    on_click_action: "StartGame".to_string(),
                    style_extra: String::new(),
                }),
                drag: None,
                layout_group: None,
                style_extra: String::new(),
                raycast_target: true,
            },
        ];
        let doc = Documento {
            canvas_raiz: 0,
            nodos,
            avisos: vec![],
        };
        let colocado = DocumentoColocado {
            nodos: vec![
                NodoColocado {
                    nodo: 0,
                    entidad: 1,
                    rect: RectColocado { x: 0.0, y: 0.0, ancho: 1280.0, alto: 720.0 },
                    estilos: StyleProps::default(),
                },
                NodoColocado {
                    nodo: 1,
                    entidad: 2,
                    rect: RectColocado { x: 530.0, y: 332.0, ancho: 220.0, alto: 56.0 },
                    estilos: StyleProps::default(),
                },
            ],
        };
        (doc, colocado)
    }

    fn nodo_base(entidad: u64, padre: Option<usize>, hijos: Vec<usize>) -> NodoDocumento {
        NodoDocumento {
            entidad,
            nombre: format!("nodo-{entidad}"),
            padre,
            hijos,
            graphic: TipoGraphic::Ninguno,
            es_canvas: false,
            es_canvas_raiz: false,
            activo: true,
            rect_transform: RectTransformData {
                anchored_position: [0.0, 0.0],
                size_delta: [0.0, 0.0],
                anchor_min: [0.5, 0.5],
                anchor_max: [0.5, 0.5],
                pivot: [0.5, 0.5],
            },
            canvas: None,
            imagen: None,
            texto: None,
            boton: None,
            drag: None,
            layout_group: None,
            style_extra: String::new(),
            raycast_target: true,
        }
    }

    fn canvas_raiz(hijos: Vec<usize>) -> NodoDocumento {
        let mut n = nodo_base(1, None, hijos);
        n.es_canvas = true;
        n.es_canvas_raiz = true;
        n.raycast_target = false;
        n
    }

    fn nc(nodo: usize, entidad: u64, rect: RectColocado) -> NodoColocado {
        NodoColocado { nodo, entidad, rect, estilos: StyleProps::default() }
    }

    #[test]
    fn hit_test_gana_el_ultimo_y_respeta_bordes() {
        // Dos botones solapados en el mismo rect; el nodo 2 (entidad 3) va después en DFS
        let mut n1 = nodo_base(2, Some(0), vec![]);
        n1.boton = Some(UIButtonData {
            interactable: true,
            target_graphic_color: [1.0; 4],
            on_click_action: "A".to_string(),
            style_extra: String::new(),
        });
        let mut n2 = nodo_base(3, Some(0), vec![]);
        n2.boton = Some(UIButtonData {
            interactable: true,
            target_graphic_color: [1.0; 4],
            on_click_action: "B".to_string(),
            style_extra: String::new(),
        });

        let doc = Documento {
            canvas_raiz: 0,
            nodos: vec![canvas_raiz(vec![1, 2]), n1, n2],
            avisos: vec![],
        };
        let r = RectColocado { x: 530.0, y: 332.0, ancho: 220.0, alto: 56.0 };
        let colocado = DocumentoColocado {
            nodos: vec![
                nc(0, 1, RectColocado { x: 0.0, y: 0.0, ancho: 1280.0, alto: 720.0 }),
                nc(1, 2, r),
                nc(2, 3, r),
            ],
        };

        // Centro: gana el último en DFS (entidad 3)
        assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), Some(3));
        // Borde superior-izquierdo: dentro
        assert_eq!(hit_test(&doc, &colocado, 530.0, 332.0), Some(3));
        // Borde derecho: fuera (x = 530 + 220 = 750)
        assert_eq!(hit_test(&doc, &colocado, 750.0, 360.0), None);
        // Borde inferior: fuera (y = 332 + 56 = 388)
        assert_eq!(hit_test(&doc, &colocado, 640.0, 388.0), None);
    }

    #[test]
    fn hit_test_omite_inactivo_no_raycast_y_no_interactuable() {
        // Inactivo
        {
            let (mut doc, colocado) = doc_boton(true);
            doc.nodos[1].activo = false;
            assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), None);
        }
        // raycast_target false
        {
            let (mut doc, colocado) = doc_boton(true);
            doc.nodos[1].raycast_target = false;
            assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), None);
        }
        // interactable false
        {
            let (doc, colocado) = doc_boton(false);
            assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), None);
        }
    }

    #[test]
    fn hit_test_capa_superior_gana_aunque_vaya_antes_en_dfs() {
        // Panel superior (nodo 1, entidad 3) va ANTES en DFS que botón (nodo 2, entidad 2)
        let mut panel = nodo_base(3, Some(0), vec![]);
        panel.boton = None; // sin botón, solo raycast
        let mut boton = nodo_base(2, Some(0), vec![]);
        boton.boton = Some(UIButtonData {
            interactable: true,
            target_graphic_color: [1.0; 4],
            on_click_action: "X".to_string(),
            style_extra: String::new(),
        });

        let doc = Documento {
            canvas_raiz: 0,
            nodos: vec![canvas_raiz(vec![1, 2]), panel, boton.clone()],
            avisos: vec![],
        };
        let r = RectColocado { x: 530.0, y: 332.0, ancho: 220.0, alto: 56.0 };
        let colocado = DocumentoColocado {
            nodos: vec![
                nc(0, 1, RectColocado { x: 0.0, y: 0.0, ancho: 1280.0, alto: 720.0 }),
                NodoColocado {
                    nodo: 1,
                    entidad: 3,
                    rect: r,
                    estilos: StyleProps {
                        override_sorting: Some(true),
                        sort_order: Some(0),
                        ..StyleProps::default()
                    },
                },
                nc(2, 2, r),
            ],
        };

        // El panel superior gana aunque va antes en DFS
        assert_eq!(hit_test(&doc, &colocado, 640.0, 360.0), Some(3));

        // Dos superiores: gana el de mayor sort_order
        let mut panel2 = nodo_base(4, Some(0), vec![]);
        panel2.boton = None;
        let doc2 = Documento {
            canvas_raiz: 0,
            nodos: vec![canvas_raiz(vec![1, 2, 3]), {
                let mut p = nodo_base(3, Some(0), vec![]);
                p.boton = None;
                p
            }, {
                let mut p = nodo_base(4, Some(0), vec![]);
                p.boton = None;
                p
            }, boton.clone()],
            avisos: vec![],
        };
        let colocado2 = DocumentoColocado {
            nodos: vec![
                nc(0, 1, RectColocado { x: 0.0, y: 0.0, ancho: 1280.0, alto: 720.0 }),
                NodoColocado {
                    nodo: 1, entidad: 3, rect: r,
                    estilos: StyleProps {
                        override_sorting: Some(true),
                        sort_order: Some(0),
                        ..StyleProps::default()
                    },
                },
                NodoColocado {
                    nodo: 2, entidad: 4, rect: r,
                    estilos: StyleProps {
                        override_sorting: Some(true),
                        sort_order: Some(5),
                        ..StyleProps::default()
                    },
                },
                nc(3, 2, r),
            ],
        };
        // sort_order 5 (entidad 4) gana sobre sort_order 0 (entidad 3)
        assert_eq!(hit_test(&doc2, &colocado2, 640.0, 360.0), Some(4));
    }

    #[test]
    fn arrastre_pulsar_devuelve_sesion_con_agarre_y_follow() {

        let mut draggable_nodo = nodo_base(5, Some(0), vec![]);
        draggable_nodo.drag = Some(UIDragData {
            draggable: true,
            accepts_drop: false,
            on_begin_drag: "begin".to_string(),
            on_drag_end: "end".to_string(),
            on_drop: String::new(),
        });
        // Sin rect_transform → anclado_inicial = [0,0]

        let doc = Documento {
            canvas_raiz: 0,
            nodos: vec![canvas_raiz(vec![1]), draggable_nodo],
            avisos: vec![],
        };
        let r = RectColocado { x: 350.0, y: 250.0, ancho: 100.0, alto: 100.0 };
        let colocado = DocumentoColocado {
            nodos: vec![
                nc(0, 1, RectColocado { x: 0.0, y: 0.0, ancho: 1280.0, alto: 720.0 }),
                nc(1, 5, r),
            ],
        };

        let sesion = arrastre_pulsar(&doc, &colocado, 400.0, 300.0).unwrap();
        assert_eq!(sesion.entidad, 5);
        assert_eq!(sesion.accion_inicio, "begin");
        assert_eq!(sesion.accion_inicio, "begin");
        assert_eq!(sesion.accion_fin, "end");
        assert_eq!(sesion.agarre, [50.0, 50.0]);
        assert_eq!(sesion.follow(500.0, 400.0), [100.0, 100.0]);
        assert!(arrastre_pulsar(&doc, &colocado, 10.0, 10.0).is_none());
    }

    #[test]
    fn arrastre_soltar_excluye_arrastrado_y_devuelve_destino() {
        let r = RectColocado { x: 350.0, y: 250.0, ancho: 100.0, alto: 100.0 };
        let mut casilla = nodo_base(10, Some(0), vec![]);
        casilla.drag = Some(UIDragData {
            draggable: false,
            accepts_drop: true,
            on_begin_drag: String::new(),
            on_drag_end: String::new(),
            on_drop: "soltar".into(),
        });
        casilla.rect_transform = RectTransformData {
            anchored_position: [10.0, 10.0],
            size_delta: [100.0, 100.0],
            anchor_min: [0.5, 0.5],
            anchor_max: [0.5, 0.5],
            pivot: [0.5, 0.5],
        };
        let mut arrastrado = nodo_base(2, Some(0), vec![]);
        arrastrado.drag = Some(UIDragData {
            draggable: true,
            accepts_drop: false,
            on_begin_drag: "begin".into(),
            on_drag_end: "end".into(),
            on_drop: String::new(),
        });
        let doc = Documento {
            canvas_raiz: 0,
            nodos: vec![canvas_raiz(vec![10, 2]), casilla, arrastrado.clone()],
            avisos: vec![],
        };
        let colocado = DocumentoColocado {
            nodos: vec![
                nc(0, 1, RectColocado { x: 0.0, y: 0.0, ancho: 1280.0, alto: 720.0 }),
                nc(1, 10, r),
                nc(2, 2, r),
            ],
        };
        let destino = arrastre_soltar(&doc, &colocado, 2, 400.0, 300.0).expect("la casilla");
        assert_eq!(destino.entidad, 10);
        assert_eq!(destino.accion_soltar, "soltar");
        assert_eq!(destino.anclado, [10.0, 10.0]);
        // Sin casilla: solo el arrastrado bajo el punto -> None.
        let doc2 = Documento {
            canvas_raiz: 0,
            nodos: vec![canvas_raiz(vec![2]), arrastrado],
            avisos: vec![],
        };
        let colocado2 = DocumentoColocado {
            nodos: vec![
                nc(0, 1, RectColocado { x: 0.0, y: 0.0, ancho: 1280.0, alto: 720.0 }),
                nc(1, 2, r),
            ],
        };
        assert!(arrastre_soltar(&doc2, &colocado2, 2, 400.0, 300.0).is_none());
    }

    #[test]
    fn rastreador_clic_completo_y_cancelaciones() {
        let (doc, colocado) = doc_boton(true);
        let mut r = RastreadorClic::new();
        assert_eq!(r.pulsar(&doc, &colocado, 640.0, 360.0), Some(2));
        assert_eq!(r.soltar(&doc, &colocado, 645.0, 355.0), Some((2, "StartGame".to_string())));
        // Suelta fuera: se cancela.
        assert_eq!(r.pulsar(&doc, &colocado, 640.0, 360.0), Some(2));
        assert_eq!(r.soltar(&doc, &colocado, 20.0, 20.0), None);
        // Pulsación fuera: no hay clic aunque se suelte dentro.
        assert_eq!(r.pulsar(&doc, &colocado, 20.0, 20.0), None);
        assert_eq!(r.soltar(&doc, &colocado, 640.0, 360.0), None);
        // Botón no interactable: ni pulsar ni soltar dan acción.
        let (doc_no, colocado_no) = doc_boton(false);
        assert_eq!(r.pulsar(&doc_no, &colocado_no, 640.0, 360.0), None);
        assert_eq!(r.soltar(&doc_no, &colocado_no, 640.0, 360.0), None);
        // Burbuja: el hit es un hijo texto sin botón con ancestro botón.
        let mut hijo = nodo_base(3, Some(1), vec![]);
        hijo.graphic = TipoGraphic::Texto;
        hijo.raycast_target = true;
        let (mut doc_b, colocado_b) = doc_boton(true);
        doc_b.nodos[1].hijos = vec![2];
        doc_b.nodos.push(hijo);
        let mut colocado_b = colocado_b;
        colocado_b.nodos.push(nc(2, 3, RectColocado { x: 600.0, y: 340.0, ancho: 80.0, alto: 20.0 }));
        assert_eq!(r.pulsar(&doc_b, &colocado_b, 640.0, 350.0), Some(3));
        assert_eq!(r.soltar(&doc_b, &colocado_b, 640.0, 350.0), Some((2, "StartGame".to_string())));
    }
}
