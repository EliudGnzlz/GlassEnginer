use crate::api::{DocumentoColocado, NodoColocado, RectColocado};
use crate::documento::{Documento, NodoDocumento, TipoGraphic};
use crate::estilos::StyleProps;

/// Tipo de primitiva de pintado.
#[derive(Debug, Clone, PartialEq)]
pub enum ModoPintado {
    Color,
    Degradado,
    Imagen,
    Texto,
    Sombra,
}

/// Primitiva de pintado lista para rasterizar (solo CPU).
#[derive(Debug, Clone)]
pub struct Segmento {
    pub entidad: u64,
    pub modo: ModoPintado,
    /// Rect redondeado a píxel físico.
    pub rect: RectColocado,
    /// Color RGBA con alfa premultiplicado.
    pub color: [f32; 4],
    /// (ángulo, t0, t1, c0, c1) — solo para Degradado.
    pub degradado: Option<(f32, f32, f32, [f32; 4], [f32; 4])>,
    /// Border radius (4 esquinas), 0 si no hay.
    pub radios: [f32; 4],
    /// (rect de clip [x,y,w,h], softness en px); None = sin recorte.
    pub clip: Option<([f32; 4], f32)>,
    /// (texto, font_size, align) — solo para Texto.
    pub texto: Option<(String, f32, String)>,
    /// Ruta del sprite — solo para Imagen.
    pub sprite: Option<String>,
    /// Pertenece a capa superior (override_sorting).
    pub capa_superior: bool,
    /// Posición final en la lista de pintado (0 = primero).
    pub orden: usize,
    /// Desenfoque en px (>0 solo en Sombra).
    pub desenfoque: f32,
}

/// Trait para consultar el tamaño original de un sprite.
pub trait MedidaImagen {
    fn tamano(&self, sprite: &str) -> Option<(f32, f32)>;
}

/// Implementación nula: siempre devuelve None.
pub struct MedidaImagenNula;
impl MedidaImagen for MedidaImagenNula {
    fn tamano(&self, _sprite: &str) -> Option<(f32, f32)> {
        None
    }
}

/// Redondea un rect a píxeles físicos; ancho/alto negativos → 0.
fn redondear_rect(r: &RectColocado) -> RectColocado {
    let ancho = r.ancho.round();
    let alto = r.alto.round();
    RectColocado {
        x: r.x.round(),
        y: r.y.round(),
        ancho: if ancho < 0.0 { 0.0 } else { ancho },
        alto: if alto < 0.0 { 0.0 } else { alto },
    }
}

/// Premultiplica un color RGBA por una opacidad adicional.
fn premultiplicar(color: [f32; 4], opacity: f32) -> [f32; 4] {
    let a = color[3] * opacity;
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// Intersección de dos rects dados como [x, y, w, h]. Devuelve None si es vacía.
fn intersectar_rects(a: [f32; 4], b: [f32; 4]) -> Option<[f32; 4]> {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = (a[0] + a[2]).min(b[0] + b[2]);
    let y1 = (a[1] + a[3]).min(b[1] + b[3]);
    let w = x1 - x0;
    let h = y1 - y0;
    if w <= 0.0 || h <= 0.0 {
        None
    } else {
        Some([x0, y0, w, h])
    }
}

/// Determina si un nodo es de capa superior.
fn es_capa_superior(estilos: &StyleProps) -> bool {
    estilos.override_sorting == Some(true)
}

/// Pinta un documento colocado y devuelve la lista de segmentos ordenada.
pub fn pintar_documento(
    doc: &Documento,
    colocado: &DocumentoColocado,
    imagenes: &dyn MedidaImagen,
) -> Vec<Segmento> {
    // Validación: las longitudes deben coincidir.
    if doc.nodos.len() != colocado.nodos.len() {
        return Vec::new();
    }

    let nodos_doc = &doc.nodos;
    let nodos_col = &colocado.nodos;

    // Pre-calcular clips heredados por overflow:hidden.
    // Para cada nodo colocado (en orden DFS), almacenamos el clip activo.
    let mut clips: Vec<Option<[f32; 4]>> = vec![None; nodos_col.len()];

    for (i, nc) in nodos_col.iter().enumerate() {
        let idx = nc.nodo;
        let nd = &nodos_doc[idx];

        // Si este nodo es de capa superior, no hereda clip.
        if es_capa_superior(&nc.estilos) {
            clips[i] = None;
        } else {
            // Buscar el clip del padre (por la cadena de ancestros en el documento).
            if nd.padre.is_some() {
                clips[i] = calcular_clip_para_nodo(idx, nodos_doc, nodos_col);
            }
        }

        // Si este nodo tiene overflow hidden y NO es capa superior,
        // actualizar el clip para sus hijos (lo haremos cuando los hijos busquen).
        // No hace falta almacenar aquí, se calcula al buscar ancestro.
    }

    // Recolectar segmentos separados en normales y de capa superior.
    let mut normales: Vec<Segmento> = Vec::new();
    let mut superiores: Vec<(i32, usize, Vec<Segmento>)> = Vec::new(); // (sort_order, pos_dfs, segs)

    for (pos_dfs, nc) in nodos_col.iter().enumerate() {
        let idx = nc.nodo;
        let nd = &nodos_doc[idx];

        // Saltar nodos inactivos.
        if !nd.activo {
            continue;
        }

        let estilos = &nc.estilos;
        let opacity = estilos.opacity.unwrap_or(1.0);

        // Si opacity es 0, no emitir nada.
        if opacity == 0.0 {
            continue;
        }

        let rect = redondear_rect(&nc.rect);
        let radios = estilos.border_radius.unwrap_or([0.0; 4]);
        let sup = es_capa_superior(estilos);

        // Calcular clip para este nodo.
        let clip_rect = if sup {
            None
        } else {
            clips[pos_dfs]
        };

        // Si el clip resulta en intersección vacía con el rect del nodo, no emitir.
        let clip_val = clip_rect.map(|cr| {
            (cr, 1.0_f32)
        });

        // Verificar si el clip hace invisible al nodo.
        if let Some(cr) = clip_rect {
            let r_arr = [rect.x, rect.y, rect.ancho, rect.alto];
            if intersectar_rects(r_arr, cr).is_none() {
                continue;
            }
        }

        let mut segs_nodo: Vec<Segmento> = Vec::new();

        // --- Sombra ---
        if let Some(ref sombra) = estilos.box_shadow {
            let sr = RectColocado {
                x: rect.x + sombra.dx,
                y: rect.y + sombra.dy,
                ancho: rect.ancho,
                alto: rect.alto,
            };
            let sc = premultiplicar(sombra.color, opacity);
            // La sombra también se recorta si hay clip.
            if let Some(cr) = clip_rect {
                let sr_arr = [sr.x, sr.y, sr.ancho, sr.alto];
                if intersectar_rects(sr_arr, cr).is_none() {
                    // Sombra fuera de clip, no emitir sombra pero seguir con el nodo.
                } else {
                    segs_nodo.push(Segmento {
                        entidad: nc.entidad,
                        modo: ModoPintado::Sombra,
                        rect: sr,
                        color: sc,
                        degradado: None,
                        radios,
                        clip: clip_val,
                        texto: None,
                        sprite: None,
                        capa_superior: sup,
                        orden: 0, // se asigna después
                        desenfoque: sombra.desenfoque,
                    });
                }
            } else {
                segs_nodo.push(Segmento {
                    entidad: nc.entidad,
                    modo: ModoPintado::Sombra,
                    rect: sr,
                    color: sc,
                    degradado: None,
                    radios,
                    clip: clip_val,
                    texto: None,
                    sprite: None,
                    capa_superior: sup,
                    orden: 0,
                    desenfoque: sombra.desenfoque,
                });
            }
        }

        // --- Fondo: degradado o color ---
        let mut tiene_fondo = false;
        if let Some((angulo, ref paradas)) = estilos.background_gradient {
            if paradas.len() >= 2 {
                tiene_fondo = true;
                let n = paradas.len();
                for i in 0..n - 1 {
                    let t0 = i as f32 / (n - 1) as f32;
                    let t1 = (i + 1) as f32 / (n - 1) as f32;
                    let c0 = premultiplicar(paradas[i], opacity);
                    let c1 = premultiplicar(paradas[i + 1], opacity);
                    segs_nodo.push(Segmento {
                        entidad: nc.entidad,
                        modo: ModoPintado::Degradado,
                        rect: rect.clone(),
                        color: c0,
                        degradado: Some((angulo, t0, t1, c0, c1)),
                        radios,
                        clip: clip_val,
                        texto: None,
                        sprite: None,
                        capa_superior: sup,
                        orden: 0,
                        desenfoque: 0.0,
                    });
                }
            } else if paradas.len() == 1 {
                tiene_fondo = true;
                let c = premultiplicar(paradas[0], opacity);
                segs_nodo.push(Segmento {
                    entidad: nc.entidad,
                    modo: ModoPintado::Color,
                    rect: rect.clone(),
                    color: c,
                    degradado: None,
                    radios,
                    clip: clip_val,
                    texto: None,
                    sprite: None,
                    capa_superior: sup,
                    orden: 0,
                    desenfoque: 0.0,
                });
            }
        }
        if !tiene_fondo {
            if let Some(bg) = estilos.background_color {
                tiene_fondo = true;
                let c = premultiplicar(bg, opacity);
                segs_nodo.push(Segmento {
                    entidad: nc.entidad,
                    modo: ModoPintado::Color,
                    rect: rect.clone(),
                    color: c,
                    degradado: None,
                    radios,
                    clip: clip_val,
                    texto: None,
                    sprite: None,
                    capa_superior: sup,
                    orden: 0,
                    desenfoque: 0.0,
                });
            }
        }

        // --- Imagen ---
        let mut tiene_imagen = false;
        if nd.graphic == TipoGraphic::Imagen {
            if let Some(ref img) = nd.imagen {
                if !img.sprite.is_empty() {
                    tiene_imagen = true;
                    let tint = premultiplicar(img.color, opacity);
                    let mut ir = rect.clone();
                    if img.preserve_aspect {
                        if let Some((sw, sh)) = imagenes.tamano(&img.sprite) {
                            if sw > 0.0 && sh > 0.0 {
                                let escala_w = rect.ancho / sw;
                                let escala_h = rect.alto / sh;
                                let escala = escala_w.min(escala_h);
                                let nw = sw * escala;
                                let nh = sh * escala;
                                ir = RectColocado {
                                    x: rect.x + (rect.ancho - nw) / 2.0,
                                    y: rect.y + (rect.alto - nh) / 2.0,
                                    ancho: nw,
                                    alto: nh,
                                };
                            }
                        }
                    }
                    segs_nodo.push(Segmento {
                        entidad: nc.entidad,
                        modo: ModoPintado::Imagen,
                        rect: ir,
                        color: tint,
                        degradado: None,
                        radios,
                        clip: clip_val,
                        texto: None,
                        sprite: Some(img.sprite.clone()),
                        capa_superior: sup,
                        orden: 0,
                        desenfoque: 0.0,
                    });
                }
            }
        }

        // --- Texto ---
        let mut tiene_texto = false;
        if nd.graphic == TipoGraphic::Texto {
            if let Some(ref txt) = nd.texto {
                tiene_texto = true;
                let base_color = estilos.color.unwrap_or(txt.color);
                let tc = premultiplicar(base_color, opacity);
                let fs = estilos.font_size.unwrap_or(txt.font_size);
                segs_nodo.push(Segmento {
                    entidad: nc.entidad,
                    modo: ModoPintado::Texto,
                    rect: rect.clone(),
                    color: tc,
                    degradado: None,
                    radios,
                    clip: clip_val,
                    texto: Some((txt.text.clone(), fs, txt.align.clone())),
                    sprite: None,
                    capa_superior: sup,
                    orden: 0,
                    desenfoque: 0.0,
                });
            }
        }

        // --- Botón sin graphic propio ---
        if nd.boton.is_some() && !tiene_fondo && !tiene_imagen && !tiene_texto {
            let boton = nd.boton.as_ref().unwrap();
            let bc = premultiplicar(boton.target_graphic_color, opacity);
            segs_nodo.push(Segmento {
                entidad: nc.entidad,
                modo: ModoPintado::Color,
                rect: rect.clone(),
                color: bc,
                degradado: None,
                radios,
                clip: clip_val,
                texto: None,
                sprite: None,
                capa_superior: sup,
                orden: 0,
                desenfoque: 0.0,
            });
        }

        if sup {
            let so = estilos.sort_order.unwrap_or(0);
            superiores.push((so, pos_dfs, segs_nodo));
        } else {
            normales.extend(segs_nodo);
        }
    }

    // Ordenar superiores por sort_order ascendente, luego por pos_dfs.
    superiores.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    // Unir: normales primero, luego superiores.
    let mut resultado: Vec<Segmento> = normales;
    for (_, _, segs) in superiores {
        resultado.extend(segs);
    }

    // Asignar índice de orden final.
    for (i, seg) in resultado.iter_mut().enumerate() {
        seg.orden = i;
    }

    resultado
}

/// Busca el clip heredado recorriendo ancestros en el documento.
fn calcular_clip_para_nodo(
    doc_idx: usize,
    nodos_doc: &[NodoDocumento],
    nodos_col: &[NodoColocado],
) -> Option<[f32; 4]> {
    let mut clip_actual: Option<[f32; 4]> = None;

    // Recopilar cadena de ancestros (sin incluir el nodo mismo).
    let mut cadena = Vec::new();
    let mut cur = nodos_doc[doc_idx].padre;
    while let Some(c) = cur {
        cadena.push(c);
        cur = nodos_doc[c].padre;
    }

    // Recorrer de raíz a padre directo.
    for &anc_idx in cadena.iter().rev() {
        let nc_opt = nodos_col.iter().find(|nc| nc.nodo == anc_idx);
        if let Some(nc) = nc_opt {
            if es_capa_superior(&nc.estilos) {
                // Capa superior rompe la cadena de clip.
                clip_actual = None;
                continue;
            }
            if nc.estilos.overflow.as_deref() == Some("hidden") {
                let r = redondear_rect(&nc.rect);
                let r_arr = [r.x, r.y, r.ancho, r.alto];
                clip_actual = match clip_actual {
                    Some(prev) => intersectar_rects(prev, r_arr),
                    None => Some(r_arr),
                };
            }
        }
    }

    clip_actual
}

/// Gestor de invalidación para un documento/sub-Canvas.
pub struct Pintor {
    sucio: bool,
}

impl Pintor {
    /// Crea un pintor nuevo (sucio = true para la primera pasada).
    pub fn new() -> Self {
        Self { sucio: true }
    }

    /// Marca el pintor como sucio (necesita repintado).
    pub fn marcar_sucio(&mut self) {
        self.sucio = true;
    }

    /// Consulta si necesita repintado.
    pub fn esta_sucio(&self) -> bool {
        self.sucio
    }

    /// Si está sucio pinta y devuelve los segmentos; si no, devuelve None.
    pub fn pintar_si_sucio(
        &mut self,
        doc: &Documento,
        colocado: &DocumentoColocado,
        imagenes: &dyn MedidaImagen,
    ) -> Option<Vec<Segmento>> {
        if !self.sucio {
            return None;
        }
        let segs = pintar_documento(doc, colocado, imagenes);
        self.sucio = false;
        Some(segs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{DocumentoColocado, NodoColocado, RectColocado};
    use crate::documento::{Documento, NodoDocumento, TipoGraphic};
    use crate::estilos::{StyleProps, Sombra};
    use glass_core::scene::UIImageData;

    /// Helper: crea un NodoDocumento mínimo activo sin graphic.
    fn nodo_base(entidad: u64, padre: Option<usize>, hijos: Vec<usize>) -> NodoDocumento {
        NodoDocumento {
            entidad,
            padre,
            hijos,
            graphic: TipoGraphic::Ninguno,
            es_canvas: false,
            es_canvas_raiz: false,
            activo: true,
            rect_transform: glass_core::scene::RectTransformData {
                anchored_position: [0.0; 2],
                size_delta: [0.0; 2],
                anchor_min: [0.0; 2],
                anchor_max: [0.0; 2],
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
        }
    }

    #[test]
    fn pintor_ordena_dfs_y_redondea() {
        // 2 nodos con fondo, comprobar redondeo y orden DFS.
        let doc = Documento {
            canvas_raiz: 0,
            nodos: vec![
                nodo_base(1, None, vec![]),
                nodo_base(2, None, vec![]),
            ],
            avisos: vec![],
        };
        let colocado = DocumentoColocado {
            nodos: vec![
                NodoColocado {
                    nodo: 0, entidad: 1,
                    rect: RectColocado { x: 10.4, y: 20.6, ancho: 100.2, alto: 40.8 },
                    estilos: StyleProps {
                        background_color: Some([1.0, 1.0, 1.0, 1.0]),
                        ..Default::default()
                    },
                },
                NodoColocado {
                    nodo: 1, entidad: 2,
                    rect: RectColocado { x: 5.1, y: 5.9, ancho: 50.0, alto: 30.0 },
                    estilos: StyleProps {
                        background_color: Some([1.0, 0.0, 0.0, 1.0]),
                        ..Default::default()
                    },
                },
            ],
        };
        let segs = pintar_documento(&doc, &colocado, &MedidaImagenNula);
        assert_eq!(segs.len(), 2);
        // Primer segmento: rect redondeado de nodo 1.
        assert_eq!(segs[0].rect.x, 10.0);
        assert_eq!(segs[0].rect.y, 21.0);
        assert_eq!(segs[0].rect.ancho, 100.0);
        assert_eq!(segs[0].rect.alto, 41.0);
        assert_eq!(segs[0].orden, 0);
        // Segundo segmento: rect redondeado de nodo 2.
        assert_eq!(segs[1].rect.x, 5.0);
        assert_eq!(segs[1].rect.y, 6.0);
        assert_eq!(segs[1].rect.ancho, 50.0);
        assert_eq!(segs[1].rect.alto, 30.0);
        assert_eq!(segs[1].orden, 1);
    }

    #[test]
    fn pintor_degradado_tres_paradas_subdivide() {
        let doc = Documento {
            canvas_raiz: 0,
            nodos: vec![nodo_base(10, None, vec![])],
            avisos: vec![],
        };
        let colocado = DocumentoColocado {
            nodos: vec![NodoColocado {
                nodo: 0, entidad: 10,
                rect: RectColocado { x: 0.0, y: 0.0, ancho: 100.0, alto: 50.0 },
                estilos: StyleProps {
                    background_gradient: Some((90.0, vec![
                        [1.0, 0.0, 0.0, 1.0],
                        [0.0, 1.0, 0.0, 1.0],
                        [0.0, 0.0, 1.0, 1.0],
                    ])),
                    ..Default::default()
                },
            }],
        };
        let segs = pintar_documento(&doc, &colocado, &MedidaImagenNula);
        assert_eq!(segs.len(), 2);
        // Banda 0: t0=0, t1=0.5, rojo → verde.
        assert_eq!(segs[0].modo, ModoPintado::Degradado);
        let d0 = segs[0].degradado.unwrap();
        assert!((d0.0 - 90.0).abs() < 0.001); // ángulo
        assert!((d0.1 - 0.0).abs() < 0.001);  // t0
        assert!((d0.2 - 0.5).abs() < 0.001);  // t1
        assert_eq!(d0.3, [1.0, 0.0, 0.0, 1.0]); // c0 rojo premul (a=1)
        assert_eq!(d0.4, [0.0, 1.0, 0.0, 1.0]); // c1 verde
        // Banda 1: t0=0.5, t1=1.0, verde → azul.
        let d1 = segs[1].degradado.unwrap();
        assert!((d1.1 - 0.5).abs() < 0.001);
        assert!((d1.2 - 1.0).abs() < 0.001);
        assert_eq!(d1.3, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(d1.4, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn pintor_sombra_antes_que_fondo_y_opacity_cero_no_emite() {
        let n0 = nodo_base(20, None, vec![]);
        // Nodo con sombra + fondo.
        let n1 = nodo_base(21, None, vec![]);
        // Nodo con opacity 0.
        let doc = Documento {
            canvas_raiz: 0,
            nodos: vec![n0, n1],
            avisos: vec![],
        };
        let colocado = DocumentoColocado {
            nodos: vec![
                NodoColocado {
                    nodo: 0, entidad: 20,
                    rect: RectColocado { x: 10.0, y: 10.0, ancho: 80.0, alto: 60.0 },
                    estilos: StyleProps {
                        background_color: Some([1.0, 1.0, 1.0, 1.0]),
                        box_shadow: Some(Sombra {
                            dx: 4.0, dy: 4.0, desenfoque: 8.0,
                            color: [0.0, 0.0, 0.0, 0.5],
                        }),
                        ..Default::default()
                    },
                },
                NodoColocado {
                    nodo: 1, entidad: 21,
                    rect: RectColocado { x: 0.0, y: 0.0, ancho: 50.0, alto: 50.0 },
                    estilos: StyleProps {
                        background_color: Some([1.0, 0.0, 0.0, 1.0]),
                        opacity: Some(0.0),
                        ..Default::default()
                    },
                },
            ],
        };
        let segs = pintar_documento(&doc, &colocado, &MedidaImagenNula);
        // Solo el nodo 20 emite: sombra + fondo = 2 segmentos.
        // El nodo 21 con opacity 0 no emite nada.
        assert_eq!(segs.len(), 2);
        // Segmento 0 = Sombra.
        assert_eq!(segs[0].modo, ModoPintado::Sombra);
        assert_eq!(segs[0].rect.x, 14.0); // 10 + 4
        assert_eq!(segs[0].rect.y, 14.0); // 10 + 4
        assert_eq!(segs[0].desenfoque, 8.0);
        // Color premultiplicado: [0*0.5, 0*0.5, 0*0.5, 0.5]
        assert_eq!(segs[0].color, [0.0, 0.0, 0.0, 0.5]);
        // Segmento 1 = Color blanco.
        assert_eq!(segs[1].modo, ModoPintado::Color);
        assert_eq!(segs[1].color, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn pintor_capa_superior_al_final_sin_clip() {
        // Canvas raíz (idx 0) con 3 hijos: A (idx 1), B (idx 2 hijo de A), C (idx 3).
        // A tiene overflow hidden; B es hijo normal de A; C tiene override_sorting.
        let doc = Documento {
            canvas_raiz: 0,
            nodos: vec![
                nodo_base(100, None, vec![1, 3]),       // raíz
                {
                    let n = nodo_base(101, Some(0), vec![2]); // A
                    n
                },
                nodo_base(102, Some(1), vec![]),         // B hijo de A
                nodo_base(103, Some(0), vec![]),         // C capa superior
            ],
            avisos: vec![],
        };
        let colocado = DocumentoColocado {
            nodos: vec![
                NodoColocado {
                    nodo: 0, entidad: 100,
                    rect: RectColocado { x: 0.0, y: 0.0, ancho: 500.0, alto: 500.0 },
                    estilos: StyleProps { ..Default::default() },
                },
                NodoColocado {
                    nodo: 1, entidad: 101,
                    rect: RectColocado { x: 10.0, y: 10.0, ancho: 200.0, alto: 200.0 },
                    estilos: StyleProps {
                        overflow: Some("hidden".to_string()),
                        background_color: Some([0.5, 0.5, 0.5, 1.0]),
                        ..Default::default()
                    },
                },
                NodoColocado {
                    nodo: 2, entidad: 102,
                    rect: RectColocado { x: 20.0, y: 20.0, ancho: 50.0, alto: 50.0 },
                    estilos: StyleProps {
                        background_color: Some([0.0, 1.0, 0.0, 1.0]),
                        ..Default::default()
                    },
                },
                NodoColocado {
                    nodo: 3, entidad: 103,
                    rect: RectColocado { x: 15.0, y: 15.0, ancho: 60.0, alto: 60.0 },
                    estilos: StyleProps {
                        override_sorting: Some(true),
                        sort_order: Some(5),
                        background_color: Some([0.0, 0.0, 1.0, 1.0]),
                        ..Default::default()
                    },
                },
            ],
        };
        let segs = pintar_documento(&doc, &colocado, &MedidaImagenNula);
        // A emite fondo, B emite fondo con clip, C emite fondo al final.
        // Raíz no tiene fondo → 0 segmentos de raíz.
        assert_eq!(segs.len(), 3);
        // B (entidad 102) debe tener clip = rect de A.
        let seg_b = segs.iter().find(|s| s.entidad == 102).unwrap();
        assert!(seg_b.clip.is_some());
        let (cr, _soft) = seg_b.clip.unwrap();
        assert_eq!(cr, [10.0, 10.0, 200.0, 200.0]);
        // C (entidad 103) es el último segmento y sin clip.
        let ultimo = &segs[segs.len() - 1];
        assert_eq!(ultimo.entidad, 103);
        assert!(ultimo.clip.is_none());
        assert!(ultimo.capa_superior);
    }

    #[test]
    fn pintor_preserve_aspect_encaja_sin_deformar() {
        // Medidor de prueba que devuelve 400×400 para el sprite dado.
        struct MedidaPrueba;
        impl MedidaImagen for MedidaPrueba {
            fn tamano(&self, sprite: &str) -> Option<(f32, f32)> {
                if sprite == "textures/cuadro.png" {
                    Some((400.0, 400.0))
                } else {
                    None
                }
            }
        }

        let mut nd = nodo_base(50, None, vec![]);
        nd.graphic = TipoGraphic::Imagen;
        nd.imagen = Some(UIImageData {
            sprite: "textures/cuadro.png".to_string(),
            color: [1.0, 1.0, 1.0, 1.0],
            preserve_aspect: true,
            style_extra: String::new(),
            raycast_target: false,
        });

        let doc = Documento {
            canvas_raiz: 0,
            nodos: vec![nd.clone()],
            avisos: vec![],
        };
        let colocado = DocumentoColocado {
            nodos: vec![NodoColocado {
                nodo: 0, entidad: 50,
                rect: RectColocado { x: 0.0, y: 0.0, ancho: 200.0, alto: 100.0 },
                estilos: StyleProps { ..Default::default() },
            }],
        };

        // Con medidor que devuelve tamaño → letterbox.
        let segs = pintar_documento(&doc, &colocado, &MedidaPrueba);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].modo, ModoPintado::Imagen);
        // escala = min(200/400, 100/400) = 0.25; tamaño = 100×100; centrado x=50.
        assert_eq!(segs[0].rect.x, 50.0);
        assert_eq!(segs[0].rect.y, 0.0);
        assert_eq!(segs[0].rect.ancho, 100.0);
        assert_eq!(segs[0].rect.alto, 100.0);

        // Con MedidaImagenNula → rect completo.
        let segs2 = pintar_documento(&doc, &colocado, &MedidaImagenNula);
        assert_eq!(segs2.len(), 1);
        assert_eq!(segs2[0].rect.ancho, 200.0);
        assert_eq!(segs2[0].rect.alto, 100.0);
    }
}
