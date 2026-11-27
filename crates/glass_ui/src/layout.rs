//! Layout absoluto (P2a): colocación de nodos UI por anclas y pivote,
//! replicando el algoritmo del viejo `resolve_scene_ui_rects`.
//!
//! - Canvas raíz → rect de referencia [0, 0, viewport/s, viewport/s].
//! - Cada hijo se resuelve con `resolve_ui_rect_in` contra su padre UI.
//! - Al final, todos los rects se escalan por `s` para pasar a píxeles.
//! - Nodos inactivos → rect (0,0,0,0); sus hijos también.

use crate::api::{
    CeldaCaracter, DocumentoColocado, EstadoPseudo, MapaTexto, MedidaTexto, NodoColocado,
    RectColocado, Viewport,
};
use crate::documento::{Documento, TipoGraphic};
use crate::estilos::{Dimension, Display, StyleProps, TamanoColumna, TextOverflow};
use crate::parser::resolver_estilos;
use glass_core::scene::{canvas_scale_factor, layout_group_rects, resolve_ui_rect_in};
use std::sync::OnceLock;

// ── MedidorTexto con ab_glyph ──────────────────────────────────────

/// Fuente incrustada (DejaVu Sans).
static FONT: OnceLock<ab_glyph::FontVec> = OnceLock::new();

fn fuente_global() -> &'static ab_glyph::FontVec {
    FONT.get_or_init(|| {
        let bytes = include_bytes!("../../../assets/fonts/DejaVuSans.ttf");
        ab_glyph::FontVec::try_from_vec(bytes.to_vec())
            .expect("No se pudo cargar DejaVuSans.ttf")
    })
}

/// Medidor de texto basado en ab_glyph. Se construye sin argumentos.
pub struct MedidorTexto;

impl MedidorTexto {
    pub fn new() -> Self {
        // Fuerza la carga de la fuente al construir.
        let _ = fuente_global();
        MedidorTexto
    }
}

impl Default for MedidorTexto {
    fn default() -> Self {
        Self::new()
    }
}

/// Calcula el avance (con kerning) de cada carácter del texto.
/// Cada par devuelto es (carácter, avance_incluyendo_kern_con_anterior).
/// Avances por carácter con kerning de ab_glyph. `espaciado` es el
/// letter-spacing en px de documento (Q3): se suma a CADA avance,
/// incluido el del último carácter, como hace CSS — mapa, `medir` y
/// el pintor usan la misma convención, así que coinciden.
fn avances_con_kern(texto: &str, tamano: f32, espaciado: f32) -> Vec<(char, f32)> {
    use ab_glyph::{Font, ScaleFont};
    let font = fuente_global();
    let scale = ab_glyph::PxScale::from(tamano);
    let scaled = font.as_scaled(scale);
    let mut resultado = Vec::with_capacity(texto.len());
    let mut prev: Option<ab_glyph::GlyphId> = None;
    for c in texto.chars() {
        let gid = scaled.glyph_id(c);
        let mut avance = scaled.h_advance(gid);
        if let Some(p) = prev {
            avance += scaled.kern(p, gid);
        }
        avance += espaciado;
        resultado.push((c, avance));
        prev = Some(gid);
    }
    resultado
}

/// Resultado de la partición greedy de una línea de texto en palabras.
/// Cada línea es un slice de los avances originales: (inicio, fin) como
/// índices en el Vec de avances, más el ancho de la línea colocada.
struct LineaPartida {
    inicio: usize,
    fin: usize,       // exclusivo
    ancho: f32,
}

/// Partición greedy por palabras (misma regla que `medir`).
/// `avances` son los avances de un párrafo (sin '\n').
/// Primera palabra siempre en la primera línea; espacio + palabra solo si cabe.
/// Devuelve las líneas resultantes.
fn partir_en_lineas(avances: &[(char, f32)], ancho_max: f32) -> Vec<LineaPartida> {
    if avances.is_empty() {
        return vec![LineaPartida { inicio: 0, fin: 0, ancho: 0.0 }];
    }
    let mut lineas: Vec<LineaPartida> = Vec::new();
    // Dividir en palabras por espacios, conservando los índices.
    let mut inicio_linea = 0usize;
    let mut ancho_linea = 0.0f32;
    let mut inicio_palabra = 0usize;
    let mut i = 0usize;
    let mut primera_palabra = true;

    while i <= avances.len() {
        // Detectar fin de palabra (espacio o fin de texto).
        let es_espacio = i < avances.len() && avances[i].0 == ' ';
        let es_fin = i == avances.len();

        if (es_espacio || es_fin) && i > inicio_palabra {
            // Tenemos una palabra [inicio_palabra..i)
            let ancho_palabra: f32 = avances[inicio_palabra..i].iter().map(|a| a.1).sum();
            if primera_palabra {
                ancho_linea = ancho_palabra;
                primera_palabra = false;
            } else {
                // El espacio antes de la palabra (está en avances[inicio_palabra - 1])
                let ancho_espacio = avances[inicio_palabra - 1].1;
                let con_espacio = ancho_linea + ancho_espacio + ancho_palabra;
                if con_espacio <= ancho_max {
                    ancho_linea = con_espacio;
                } else {
                    // Cerrar línea anterior (sin el espacio de separación).
                    lineas.push(LineaPartida {
                        inicio: inicio_linea,
                        fin: inicio_palabra - 1, // excluye el espacio que provocó el salto
                        ancho: ancho_linea,
                    });
                    inicio_linea = inicio_palabra;
                    ancho_linea = ancho_palabra;
                }
            }
        }

        if es_espacio && i == inicio_palabra {
            // Espacio suelto: avanzar inicio de siguiente palabra
            inicio_palabra = i + 1;
            i += 1;
            continue;
        }
        if es_espacio {
            // Espacio después de palabra: la siguiente palabra empieza en i+1
            inicio_palabra = i + 1;
        }
        if es_fin {
            break;
        }
        if !es_espacio && i == inicio_palabra {
            // Dentro de una palabra, solo avanzar
        }
        i += 1;
    }
    // Cerrar última línea.
    lineas.push(LineaPartida {
        inicio: inicio_linea,
        fin: avances.len(),
        ancho: ancho_linea,
    });
    lineas
}

impl MedidaTexto for MedidorTexto {
    fn medir(&self, texto: &str, tamano: f32, ancho_max: f32, letter_spacing: f32) -> (f32, f32) {
        use ab_glyph::{Font, ScaleFont};
        if texto.is_empty() {
            return (0.0, 0.0);
        }
        let font = fuente_global();
        let scale = ab_glyph::PxScale::from(tamano);
        let scaled = font.as_scaled(scale);
        let line_height = scaled.ascent() - scaled.descent() + scaled.line_gap();

        // '\n' es salto forzado: cada párrafo se mide por separado, igual
        // que el mapa (antes, medir contaba '\n' como un carácter normal
        // y devolvía una sola línea de alto; el mapa producía varias).
        let mut ancho = 0.0f32;
        let mut alto = 0.0f32;
        for parrafo in texto.split('\n') {
            let avances = avances_con_kern(parrafo, tamano, letter_spacing);
            let ancho_total: f32 = avances.iter().map(|a| a.1).sum();
            if !ancho_max.is_finite() || ancho_total <= ancho_max {
                ancho = ancho.max(ancho_total);
                alto += line_height;
            } else {
                let lineas = partir_en_lineas(&avances, ancho_max);
                alto += line_height * lineas.len() as f32;
                ancho = ancho.max(
                    lineas.iter().map(|l| l.ancho).fold(0.0f32, f32::max),
                );
            }
        }
        (ancho, alto)
    }
}


/// Construye el mapa carácter→posición para un nodo de texto.
///
/// Usa `fuente_global()` directamente; el medidor inyectable de
/// `colocar_documento_con_medidor` solo gobierna (ancho, alto) del rect,
/// no el mapa.
pub fn construir_mapa_texto(
    texto: &str,
    tamano: f32,
    ancho_max: f32,
    overflow: TextOverflow,
    align: &str,
    origen: [f32; 2],
    letter_spacing: f32,
) -> MapaTexto {
    use ab_glyph::{Font, ScaleFont};

    if texto.is_empty() {
        return MapaTexto { celdas: Vec::new(), texto_visible: String::new() };
    }

    let font = fuente_global();
    let scale = ab_glyph::PxScale::from(tamano);
    let scaled = font.as_scaled(scale);
    let alto_linea = scaled.ascent() - scaled.descent() + scaled.line_gap();
    let sin_restriccion = !ancho_max.is_finite();

    // Calcular desplazamiento de alineación para una línea.
    let despl_align = |ancho_linea: f32| -> f32 {
        if sin_restriccion { return 0.0; }
        match align {
            "Center" => (ancho_max - ancho_linea) / 2.0,
            "Right" => ancho_max - ancho_linea,
            _ => 0.0,
        }
    };

    match overflow {
        TextOverflow::Truncar if !sin_restriccion => {
            // Una sola línea: '\n' termina el texto visible (antes, los
            // avances se calculaban sobre todo el texto incluido '\n').
            let primera_linea = texto.split('\n').next().unwrap_or("");
            let avances = avances_con_kern(primera_linea, tamano, letter_spacing);
            let mut celdas = Vec::new();
            let mut texto_visible = String::new();
            let mut x_acum = 0.0f32;
            for (idx, &(c, avance)) in avances.iter().enumerate() {
                if x_acum + avance > ancho_max {
                    break;
                }
                celdas.push(CeldaCaracter {
                    indice: idx,
                    caracter: c,
                    celda: [origen[0] + x_acum, origen[1], avance, alto_linea],
                });
                texto_visible.push(c);
                x_acum += avance;
            }
            let da = despl_align(x_acum);
            if da != 0.0 {
                for celda in &mut celdas {
                    celda.celda[0] += da;
                }
            }
            MapaTexto { celdas, texto_visible }
        }
        TextOverflow::PuntosSuspensivos if !sin_restriccion => {
            // Una sola línea: '\n' termina el texto visible.
            let primera_linea = texto.split('\n').next().unwrap_or("");
            let avances = avances_con_kern(primera_linea, tamano, letter_spacing);
            // Medir el carácter '…' (con su letter-spacing, como un carácter más).
            let ellipsis_gid = scaled.glyph_id('…');
            let ellipsis_w = scaled.h_advance(ellipsis_gid) + letter_spacing;
            if ellipsis_w > ancho_max {
                return MapaTexto { celdas: Vec::new(), texto_visible: String::new() };
            }
            // ¿Cabe todo el texto en una línea?
            let ancho_total: f32 = avances.iter().map(|a| a.1).sum();
            if ancho_total <= ancho_max {
                // No hace falta truncar.
                let mut celdas = Vec::new();
                let mut x_acum = 0.0f32;
                let da = despl_align(ancho_total);
                for (idx, &(c, avance)) in avances.iter().enumerate() {
                    celdas.push(CeldaCaracter {
                        indice: idx,
                        caracter: c,
                        celda: [origen[0] + da + x_acum, origen[1], avance, alto_linea],
                    });
                    x_acum += avance;
                }
                return MapaTexto { celdas, texto_visible: primera_linea.to_string() };
            }
            let mut celdas = Vec::new();
            let mut texto_visible = String::new();
            let mut x_acum = 0.0f32;
            for (idx, &(c, avance)) in avances.iter().enumerate() {
                if x_acum + avance + ellipsis_w > ancho_max {
                    break;
                }
                celdas.push(CeldaCaracter {
                    indice: idx,
                    caracter: c,
                    celda: [origen[0] + x_acum, origen[1], avance, alto_linea],
                });
                texto_visible.push(c);
                x_acum += avance;
            }
            // Kern entre último carácter colocado y '…'
            let ellipsis_avance = if let Some(last) = celdas.last() {
                let last_gid = scaled.glyph_id(last.caracter);
                scaled.h_advance(ellipsis_gid) + scaled.kern(last_gid, ellipsis_gid)
            } else {
                ellipsis_w
            };
            celdas.push(CeldaCaracter {
                indice: avances.len(),
                caracter: '…',
                celda: [origen[0] + x_acum, origen[1], ellipsis_avance, alto_linea],
            });
            texto_visible.push('…');
            let ancho_final = x_acum + ellipsis_avance;
            let da = despl_align(ancho_final);
            if da != 0.0 {
                for celda in &mut celdas {
                    celda.celda[0] += da;
                }
            }
            MapaTexto { celdas, texto_visible }
        }
        // Envolver (o Truncar/PuntosSuspensivos sin restricción → línea única completa)
        _ => {
            // Partir por '\n' en párrafos; envolver cada párrafo.
            let parrafos: Vec<&str> = texto.split('\n').collect();
            let mut celdas = Vec::new();
            let mut texto_visible = String::new();
            let mut linea_actual = 0usize;
            let mut indice_global = 0usize; // índice sobre texto.chars()

            for (ip, parrafo) in parrafos.iter().enumerate() {
                if ip > 0 {
                    // El '\n' cuenta en indice_global pero no tiene celda.
                    indice_global += 1;
                    texto_visible.push('\n');
                }
                if parrafo.is_empty() {
                    // Párrafo vacío: avanzar una línea.
                    linea_actual += 1;
                    continue;
                }
                let avances = avances_con_kern(parrafo, tamano, letter_spacing);
                if sin_restriccion {
                    // Línea única sin restricción.
                    let ancho_linea: f32 = avances.iter().map(|a| a.1).sum();
                    let da = despl_align(ancho_linea);
                    let mut x_acum = 0.0f32;
                    for (i, &(c, avance)) in avances.iter().enumerate() {
                        celdas.push(CeldaCaracter {
                            indice: indice_global + i,
                            caracter: c,
                            celda: [
                                origen[0] + da + x_acum,
                                origen[1] + linea_actual as f32 * alto_linea,
                                avance,
                                alto_linea,
                            ],
                        });
                        texto_visible.push(c);
                        x_acum += avance;
                    }
                    indice_global += avances.len();
                    linea_actual += 1;
                } else {
                    let lineas = partir_en_lineas(&avances, ancho_max);
                    for linea in &lineas {
                        let da = despl_align(linea.ancho);
                        let mut x_acum = 0.0f32;
                        let inicio_local = linea.inicio;
                        let fin_local = linea.fin;
                        for (k, &(c, avance)) in
                            avances[inicio_local..fin_local].iter().enumerate()
                        {
                            let i = inicio_local + k;
                            // El espacio que provocó un salto: está fuera del rango
                            // [inicio..fin) de la línea, así que no se coloca aquí.
                            celdas.push(CeldaCaracter {
                                indice: indice_global + i,
                                caracter: c,
                                celda: [
                                    origen[0] + da + x_acum,
                                    origen[1] + linea_actual as f32 * alto_linea,
                                    avance,
                                    alto_linea,
                                ],
                            });
                            texto_visible.push(c);
                            x_acum += avance;
                        }
                        // Si hay siguiente línea en este párrafo, el espacio de
                        // separación (entre fin_local y siguiente inicio) no se coloca.
                        linea_actual += 1;
                    }
                    // Añadir los espacios intermedios al texto_visible
                    // (ya se colocaron los caracteres de cada línea; los espacios
                    // de salto no tienen celda pero sí deben estar en texto_visible
                    // para Envolver — realmente texto_visible conserva todo).
                    // Corrección: reconstruir texto_visible para este párrafo
                    // incluyendo los espacios entre líneas.
                    // La lógica de arriba ya pushea los chars de cada línea.
                    // Necesitamos también los espacios de separación entre líneas
                    // en texto_visible.
                    // Volvemos a construir: quitar lo último y rehacerlo bien.
                    let chars_parrafo_insertados: usize = lineas.iter()
                        .map(|l| l.fin - l.inicio)
                        .sum();
                    // Eliminar los caracteres del párrafo que acabamos de insertar.
                    for _ in 0..chars_parrafo_insertados {
                        texto_visible.pop();
                    }
                    // Reinsertar el párrafo completo (texto_visible conserva todo).
                    texto_visible.push_str(parrafo);
                    indice_global += avances.len();
                }
            }
            MapaTexto { celdas, texto_visible }
        }
    }
}

// ── Layout absoluto ────────────────────────────────────────────────

/// Coloca todos los nodos del documento en el viewport dado.
/// Réplica del viejo `resolve_scene_ui_rects`.
pub fn colocar_documento(doc: &Documento, viewport: Viewport) -> DocumentoColocado {
    let medidor = MedidorTexto::new();
    colocar_documento_con_medidor(doc, viewport, &medidor)
}

/// Versión que acepta un medidor inyectable (útil para tests).
pub fn colocar_documento_con_medidor(
    doc: &Documento,
    viewport: Viewport,
    medidor: &dyn MedidaTexto,
) -> DocumentoColocado {
    let n = doc.nodos.len();
    // rects en unidades de referencia (antes de escalar)
    let mut rects_ref: Vec<[f32; 4]> = vec![[0.0, 0.0, 0.0, 0.0]; n];
    let mut escala: Vec<f32> = vec![1.0; n];

    // Primer paso: calcular rects en unidades de referencia (DFS).
    for idx in 0..n {
        let nodo = &doc.nodos[idx];

        // Escala del canvas activo más cercano.
        let s = if nodo.es_canvas_raiz {
            if let Some(ref cd) = nodo.canvas {
                canvas_scale_factor(cd, viewport.ancho, viewport.alto)
            } else {
                1.0
            }
        } else if let Some(pidx) = nodo.padre {
            escala[pidx]
        } else {
            1.0
        };
        escala[idx] = s;

        // Nodo inactivo → rect cero, y sus descendientes también.
        if !nodo.activo {
            rects_ref[idx] = [0.0, 0.0, 0.0, 0.0];
            continue;
        }
        // Padre inactivo → heredar cero.
        if let Some(pidx) = nodo.padre {
            if rects_ref[pidx] == [0.0, 0.0, 0.0, 0.0] && !doc.nodos[pidx].activo {
                rects_ref[idx] = [0.0, 0.0, 0.0, 0.0];
                continue;
            }
        }

        if nodo.es_canvas_raiz {
            // Canvas raíz: rect = [0, 0, viewport/s, viewport/s]
            rects_ref[idx] = [0.0, 0.0, viewport.ancho / s, viewport.alto / s];
        } else {
            // Rect del padre UI más cercano.
            let parent_rect = match nodo.padre {
                Some(pidx) => rects_ref[pidx],
                None => [0.0, 0.0, viewport.ancho / s, viewport.alto / s],
            };
            rects_ref[idx] = resolve_ui_rect_in(&nodo.rect_transform, parent_rect);
        }
    }

    // P2b: flex — recoloca los hijos de los padres flex en rects_ref
    // (unidades de referencia); el paso 2 aplica estilos y escala.
    aplicar_flex(doc, &mut rects_ref);
    aplicar_grid(doc, &mut rects_ref);

    // Segundo paso: aplicar estilos y convertir a píxeles.
    let estado = EstadoPseudo::default();
    let mut nodos_colocados = Vec::with_capacity(n);

    for idx in 0..n {
        let nodo = &doc.nodos[idx];
        let estilos = resolver_estilos(nodo, &estado);
        let s = escala[idx];

        if !nodo.activo || (nodo.padre.is_some()
            && !doc.nodos[nodo.padre.unwrap()].activo)
        {
            // Comprobar si algún ancestro está inactivo.
            let mut ancestro_inactivo = !nodo.activo;
            if !ancestro_inactivo {
                let mut p = nodo.padre;
                while let Some(pidx) = p {
                    if !doc.nodos[pidx].activo {
                        ancestro_inactivo = true;
                        break;
                    }
                    p = doc.nodos[pidx].padre;
                }
            }
            if ancestro_inactivo {
                nodos_colocados.push(NodoColocado {
                    nodo: idx,
                    entidad: nodo.entidad,
                    rect: RectColocado::default(),
                    estilos,
                    mapa_texto: MapaTexto::default(),
                });
                continue;
            }
        }

        let r = rects_ref[idx];
        let mut ancho_ref = r[2];
        let mut alto_ref = r[3];

        // Ancho del padre para porcentajes.
        let padre_ref = match nodo.padre {
            Some(pidx) => rects_ref[pidx],
            None => r,
        };

        // Despacho por display del padre (para P2b/P2c).
        let display_padre = if let Some(pidx) = nodo.padre {
            let est_padre = resolver_estilos(&doc.nodos[pidx], &estado);
            est_padre.display
        } else {
            None
        };

        match display_padre {
            // Flex y grid se resuelven en la pasada previa (aplicar_flex
            // / P2c); aquí el rect ya viene impuesto en rects_ref.
            Some(Display::Flex) | Some(Display::Grid) => {}
            _ => {}
        }

        // Aplicar estilos de ancho.
        if let Some(ref dim) = estilos.width {
            match dim {
                Dimension::Px(v) => ancho_ref = *v,
                Dimension::Porcentaje(p) => ancho_ref = p / 100.0 * padre_ref[2],
                Dimension::Auto => {
                    // Para nodos de texto (y campos de entrada, que
                    // llevan el texto sintetizado en `nodo.texto`): medir.
                    if nodo.graphic == TipoGraphic::Texto
                        || nodo.graphic == TipoGraphic::InputField
                    {
                        if let Some(ref txt) = nodo.texto {
                            let tam = estilos.font_size.unwrap_or(14.0);
                            let ls = estilos.letter_spacing.unwrap_or(0.0);
                            let (tw, _) = medidor.medir(&txt.text, tam, f32::INFINITY, ls);
                            ancho_ref = tw;
                        }
                    }
                    // En otro caso, se queda con el valor de resolve_ui_rect_in.
                }
            }
        }

        // Aplicar estilos de alto.
        if let Some(ref dim) = estilos.height {
            match dim {
                Dimension::Px(v) => alto_ref = *v,
                Dimension::Porcentaje(p) => alto_ref = p / 100.0 * padre_ref[3],
                Dimension::Auto => {
                    if nodo.graphic == TipoGraphic::Texto
                        || nodo.graphic == TipoGraphic::InputField
                    {
                        if let Some(ref txt) = nodo.texto {
                            let tam = estilos.font_size.unwrap_or(14.0);
                            let ls = estilos.letter_spacing.unwrap_or(0.0);
                            let (_, th) = medidor.medir(&txt.text, tam, ancho_ref, ls);
                            alto_ref = th;
                        }
                    }
                }
            }
        }

        // P2c: aspect-ratio y límites min/max (el recorte por overflow
        // es del pintor, P3, no del layout).
        let (ancho_ref, alto_ref) = limitar_tamano(
            &estilos,
            ancho_ref,
            alto_ref,
            padre_ref[2],
            padre_ref[3],
            estilos.width.is_some(),
            estilos.height.is_some(),
        );

        // Escalar a píxeles.
        let rect = RectColocado {
            x: r[0] * s,
            y: r[1] * s,
            ancho: ancho_ref * s,
            alto: alto_ref * s,
        };

        // V3c: mapa carácter→posición para nodos de texto (y campos de
        // entrada, que llevan el texto sintetizado en `nodo.texto`).
        let mapa_texto = if nodo.graphic == TipoGraphic::Texto
            || nodo.graphic == TipoGraphic::InputField
        {
            if let Some(ref txt) = nodo.texto {
                let fs = estilos.font_size.unwrap_or(txt.font_size);
                construir_mapa_texto(
                    &txt.text,
                    fs,
                    rect.ancho,
                    estilos.text_overflow.unwrap_or(TextOverflow::Envolver),
                    &txt.align,
                    [rect.x, rect.y],
                    estilos.letter_spacing.unwrap_or(0.0),
                )
            } else {
                MapaTexto::default()
            }
        } else {
            MapaTexto::default()
        };

        nodos_colocados.push(NodoColocado {
            nodo: idx,
            entidad: nodo.entidad,
            rect,
            estilos,
            mapa_texto,
        });
    }

    DocumentoColocado {
        nodos: nodos_colocados,
    }
}

// === P2b: layout flex (uGUI LayoutGroup + flex CSS mínimo) ===
// Todo se calcula en UNIDADES DE REFERENCIA; la escala se aplica en el paso 2.

/// Desplazamiento inicial del bloque de hijos según justify_content.
/// Devuelve el offset a lo largo del eje principal.
fn offset_justify(justify: Option<&str>, espacio_libre: f32) -> f32 {
    match justify {
        Some("center") => espacio_libre * 0.5,
        Some("end") | Some("flex-end") => espacio_libre,
        // "start" / "flex-start" / None: sin desplazamiento
        _ => 0.0,
    }
}

/// Posición en el eje transversal para un hijo, dado el hueco interior
/// [inicio, tamaño] y el tamaño del hijo.
fn pos_align(align: Option<&str>, inicio: f32, tam_interior: f32, tam_hijo: f32) -> f32 {
    match align {
        Some("center") => inicio + (tam_interior - tam_hijo) * 0.5,
        Some("end") | Some("flex-end") => inicio + (tam_interior - tam_hijo),
        // "stretch" se gestiona aparte (cambia el tamaño, no solo la posición)
        _ => inicio,
    }
}

/// Aplica el layout de un padre flex CSS puro (sin LayoutGroup) sobre sus hijos.
/// `rects_hijos` son los rects actuales de los hijos directos activos (paso 1).
/// Devuelve los nuevos rects en el mismo orden.
fn flex_css_hijos(
    estilos_padre: &StyleProps,
    rect_padre: [f32; 4],
    rects_hijos: &[[f32; 4]],
) -> Vec<[f32; 4]> {
    let horizontal = match estilos_padre.flex_direction.as_deref() {
        Some("column") => false,
        _ => true, // "row" por defecto
    };
    let pad = estilos_padre.padding.unwrap_or([0.0; 4]);
    let gap = estilos_padre.gap.unwrap_or(0.0);

    // Área interior (unidades de referencia).
    let inter_x = rect_padre[0] + pad[0];
    let inter_y = rect_padre[1] + pad[1];
    let inter_w = (rect_padre[2] - pad[0] - pad[2]).max(0.0);
    let inter_h = (rect_padre[3] - pad[1] - pad[3]).max(0.0);

    let n = rects_hijos.len();
    if n == 0 {
        return Vec::new();
    }

    // Eje principal / transversal según dirección.
    let (tam_interior_princ, tam_interior_trans) = if horizontal {
        (inter_w, inter_h)
    } else {
        (inter_h, inter_w)
    };

    // Suma de tamaños de hijos en el eje principal + gaps.
    let suma_hijos: f32 = rects_hijos
        .iter()
        .map(|r| if horizontal { r[2] } else { r[3] })
        .sum();
    let suma_gaps = gap * (n.saturating_sub(1)) as f32;
    let espacio_libre = (tam_interior_princ - suma_hijos - suma_gaps).max(0.0);

    let cursor_inicial = offset_justify(
        estilos_padre.justify_content.as_deref(),
        espacio_libre,
    );

    let stretch = matches!(estilos_padre.align_items.as_deref(), Some("stretch"));

    let mut salida = Vec::with_capacity(n);
    // Cursor a lo largo del eje principal (relativo al inicio interior).
    let mut cursor = cursor_inicial;

    for r in rects_hijos {
        let (tam_princ_hijo, tam_trans_hijo) = if horizontal { (r[2], r[3]) } else { (r[3], r[2]) };

        // Tamaño transversal: stretch ocupa todo el interior; si no, el propio hijo.
        let tam_trans_final = if stretch { tam_interior_trans } else { tam_trans_hijo };

        // Posición transversal.
        let pos_trans = if stretch {
            if horizontal { inter_y } else { inter_x }
        } else {
            pos_align(
                estilos_padre.align_items.as_deref(),
                if horizontal { inter_y } else { inter_x },
                tam_interior_trans,
                tam_trans_hijo,
            )
        };

        // Posición principal.
        let pos_princ = (if horizontal { inter_x } else { inter_y }) + cursor;

        // Reconstruir rect en orden (x, y, ancho, alto).
        let nuevo = if horizontal {
            [pos_princ, pos_trans, tam_princ_hijo, tam_trans_final]
        } else {
            [pos_trans, pos_princ, tam_trans_final, tam_princ_hijo]
        };
        salida.push(nuevo);

        cursor += tam_princ_hijo + gap;
    }

    salida
}

/// Aplica el layout flex a todo el documento, en orden DFS (padres antes que hijos).
/// Se llama entre el paso 1 y el paso 2 de `colocar_documento_con_medidor`.
pub fn aplicar_flex(doc: &Documento, rects_ref: &mut [[f32; 4]]) {
    // `padre`/`hijos` son ÍNDICES en `doc.nodos` (no ids de entidad).

    for (i_padre, nodo_padre) in doc.nodos.iter().enumerate() {
        if !nodo_padre.activo || nodo_padre.hijos.is_empty() {
            continue;
        }

        let estilos = resolver_estilos(nodo_padre, &EstadoPseudo::default());

        // ¿Es un padre flex? Por display resuelto o por layout_group (que implica Flex por P1).
        let es_flex = matches!(estilos.display, Some(Display::Flex))
            || nodo_padre.layout_group.is_some();
        if !es_flex {
            continue;
        }

        // Índices de hijos directos activos.
        let hijos_idx: Vec<usize> = nodo_padre
            .hijos
            .iter()
            .copied()
            .filter(|&j| j < doc.nodos.len() && doc.nodos[j].activo)
            .collect();
        if hijos_idx.is_empty() {
            continue;
        }

        let rect_padre = rects_ref[i_padre];

        if let Some(grupo) = &nodo_padre.layout_group {
            // uGUI: reparto exacto de layout_group_rects.
            let rects_hijos: Vec<[f32; 4]> = hijos_idx.iter().map(|&j| rects_ref[j]).collect();
            let nuevos = layout_group_rects(rect_padre, grupo, &rects_hijos);
            for (k, &j) in hijos_idx.iter().enumerate() {
                if let Some(nuevo) = nuevos.get(k) {
                    rects_ref[j] = *nuevo;
                }
            }
        } else {
            // Flex CSS mínimo por estilo.
            let rects_hijos: Vec<[f32; 4]> = hijos_idx.iter().map(|&j| rects_ref[j]).collect();
            let nuevos = flex_css_hijos(&estilos, rect_padre, &rects_hijos);
            for (k, &j) in hijos_idx.iter().enumerate() {
                if let Some(nuevo) = nuevos.get(k) {
                    rects_ref[j] = *nuevo;
                }
            }
        }
    }
}

/// Aplica layout grid a los padres con `display: grid` (P2c).
/// Se llama tras `aplicar_flex`, entre el paso 1 (top-down) y el paso 2 (tamaños y escala).
/// Un padre es flex O grid, nunca los dos, así que el orden entre ambos no altera el resultado.
fn aplicar_grid(doc: &Documento, rects_ref: &mut [[f32; 4]]) {
    // Recorrido en orden DFS sobre los índices de `doc.nodos` (mismo orden que el paso 1).
    for idx in orden_dfs(doc) {
        let nodo = &doc.nodos[idx];
        if !nodo.activo {
            continue;
        }
        // Solo padres con hijos directos activos.
        let hijos_activos: Vec<usize> = nodo
            .hijos
            .iter()
            .copied()
            .filter(|&h| doc.nodos.get(h).map(|n| n.activo).unwrap_or(false))
            .collect();
        if hijos_activos.is_empty() {
            continue;
        }
        let estilos = resolver_estilos(nodo, &EstadoPseudo::default());
        if estilos.display != Some(Display::Grid) {
            continue;
        }
        let r = rects_ref[idx];
        // Área interior = rect del padre menos padding [izq, arriba, der, abajo].
        let pad = estilos.padding.unwrap_or([0.0; 4]);
        let interior_x = r[0] + pad[0];
        let interior_y = r[1] + pad[1];
        let interior_w = (r[2] - pad[0] - pad[2]).max(0.0);
        let gap = estilos.gap.unwrap_or(0.0).max(0.0);

        // Expansión de la plantilla de columnas.
        let anchos_col = expandir_columnas(
            estilos.grid_template_columns.as_deref(),
            interior_w,
            gap,
        );
        let ncols = anchos_col.len();
        if ncols == 0 {
            continue;
        }

        // Fila por hijo en orden row-major: columna k % ncols, fila k / ncols.
        // Los hijos inactivos ya fueron filtrados, así que no consumen celda.
        let mut alto_total_previo = 0.0_f32;
        let mut inicio = 0usize;
        while inicio < hijos_activos.len() {
            let fin = (inicio + ncols).min(hijos_activos.len());
            let fila = &hijos_activos[inicio..fin];
            // Alto de la fila = máximo alto (del paso 1) de los hijos de esa fila.
            let alto_fila = fila
                .iter()
                .map(|&h| rects_ref[h][3].max(0.0))
                .fold(0.0_f32, f32::max);
            // Posición x acumulada por columna dentro de la fila.
            let mut x_acum = interior_x;
            for (col, &h) in fila.iter().enumerate() {
                let ancho_hijo = rects_ref[h][2];
                let alto_hijo = rects_ref[h][3];
                rects_ref[h][0] = x_acum;
                rects_ref[h][1] = interior_y + alto_total_previo;
                // El hijo conserva su tamaño del paso 1; no se estira a la celda.
                rects_ref[h][2] = ancho_hijo;
                rects_ref[h][3] = alto_hijo;
                // Avanza el ancho de la columna más el gap.
                x_acum += anchos_col[col] + gap;
            }
            alto_total_previo += alto_fila + gap;
            inicio = fin;
        }
    }
}

/// Recorre los nodos en orden DFS partiendo del canvas raíz.
/// Devuelve la lista de índices de `doc.nodos` en ese orden.
fn orden_dfs(doc: &Documento) -> Vec<usize> {
    let mut orden = Vec::with_capacity(doc.nodos.len());
    let mut pila = vec![doc.canvas_raiz];
    while let Some(idx) = pila.pop() {
        if idx >= doc.nodos.len() {
            continue;
        }
        orden.push(idx);
        // Apila los hijos en orden inverso para que se visite el primero primero.
        for &h in doc.nodos[idx].hijos.iter().rev() {
            if h < doc.nodos.len() {
                pila.push(h);
            }
        }
    }
    orden
}

/// Expande la plantilla de columnas a anchos en unidades de referencia.
/// Si no hay plantilla, devuelve una sola columna del ancho interior.
fn expandir_columnas(
    plantilla: Option<&[TamanoColumna]>,
    interior_w: f32,
    gap: f32,
) -> Vec<f32> {
    let some = match plantilla {
        Some(p) if !p.is_empty() => p,
        _ => return vec![interior_w.max(0.0)],
    };
    // Primera pasada: sumar fijas y localizar un posible Relleno.
    let mut suma_fijas = 0.0_f32;
    let mut suma_frac = 0.0_f32;
    let mut relleno: Option<f32> = None;
    for col in some {
        match *col {
            TamanoColumna::Fija(w) => suma_fijas += w.max(0.0),
            TamanoColumna::Fraccion(f) => suma_frac += f.max(0.0),
            TamanoColumna::Relleno(c) => {
                if relleno.is_none() {
                    relleno = Some(c.max(0.0));
                }
            }
        }
    }
    // Relleno: ncols = max(1, floor((interior_w + gap) / (celda + gap))).
    if let Some(celda) = relleno {
        let denom = celda + gap;
        let n = if denom <= 0.0 {
            1
        } else {
            (((interior_w + gap) / denom).floor() as i64).max(1) as usize
        };
        return vec![celda; n];
    }
    // Fracciones: parte del espacio restante tras fijas y gaps.
    let ncols = some.len();
    let gaps_totales = gap * (ncols.saturating_sub(1) as f32);
    let restante = (interior_w - suma_fijas - gaps_totales).max(0.0);
    let mut anchos = Vec::with_capacity(ncols);
    for col in some {
        let w = match *col {
            TamanoColumna::Fija(w) => w.max(0.0),
            TamanoColumna::Fraccion(f) => {
                if suma_frac <= 0.0 {
                    0.0
                } else {
                    restante * (f.max(0.0) / suma_frac)
                }
            }
            TamanoColumna::Relleno(_) => 0.0, // ya tratado arriba
        };
        anchos.push(w);
    }
    anchos
}

/// Aplica `aspect-ratio` y límites min/max sobre el tamaño en unidades de referencia.
/// `ancho_fijado`/`alto_fijado` = el estilo width/height estaba presente (Px, % o Auto).
/// Nunca devuelve NaN ni infinito: si un cálculo sale no finito, conserva la entrada.
fn limitar_tamano(
    estilos: &StyleProps,
    ancho: f32,
    alto: f32,
    padre_ancho: f32,
    padre_alto: f32,
    ancho_fijado: bool,
    alto_fijado: bool,
) -> (f32, f32) {
    let mut w = ancho;
    let mut h = alto;

    // 1) aspect-ratio: solo cuando se fija exactamente una de las dos dimensiones.
    if let Some(ratio) = estilos.aspect_ratio {
        if ratio.is_finite() && ratio > 0.0 {
            if ancho_fijado && !alto_fijado {
                let nh = w / ratio;
                if nh.is_finite() {
                    h = nh;
                }
            } else if alto_fijado && !ancho_fijado {
                let nw = h * ratio;
                if nw.is_finite() {
                    w = nw;
                }
            }
            // ambos fijados o ninguno: no se toca
        }
    }

    // 2) min/max: Px -> valor, Porcentaje -> % del padre, Auto -> sin efecto.
    // Si min > max, manda el mínimo (se aplica primero con max, luego min).
    let resolver = |d: Option<Dimension>, base: f32| -> Option<f32> {
        match d {
            Some(Dimension::Px(v)) if v.is_finite() => Some(v),
            Some(Dimension::Porcentaje(p)) if p.is_finite() => {
                let v = base * (p / 100.0);
                if v.is_finite() { Some(v) } else { None }
            }
            _ => None, // Auto o None
        }
    };

    let min_w = resolver(estilos.min_width, padre_ancho);
    let max_w = resolver(estilos.max_width, padre_ancho);
    let min_h = resolver(estilos.min_height, padre_alto);
    let max_h = resolver(estilos.max_height, padre_alto);

    if let Some(mx) = max_w {
        w = w.min(mx);
    }
    if let Some(mn) = min_w {
        w = w.max(mn);
    }
    if let Some(mx) = max_h {
        h = h.min(mx);
    }
    if let Some(mn) = min_h {
        h = h.max(mn);
    }

    // Nota: `overflow` no altera el layout; el recorte lo hace el pintor (P3).
    (w, h)
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Viewport;
    use crate::documento::{Documento, NodoDocumento};
    use glass_core::scene::{canvas_scale_factor, CanvasData, RectTransformData};

    /// Test 1: snapshot contra el fixture generado por el viejo resolver.
    #[test]
    fn snapshot_canvas_ui_test_dentro_de_medio_pixel() {
        let scene_path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/scenes/CanvasUiTest.scene.json");
        let fixture_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/rects_canvasuitest_1280x720.json"
        );

        let mut scene = glass_core::scene::SceneState::default();
        scene.load_from_json(scene_path).expect("la escena de prueba carga");
        let docs = crate::derivacion::derivar_documentos(&scene);
        assert!(!docs.is_empty(), "No se derivó ningún documento");

        let viewport = Viewport {
            ancho: 1280.0,
            alto: 720.0,
        };
        let colocado = colocar_documento(&docs[0], viewport);

        // Cargar fixture JSON.
        let fixture_str = std::fs::read_to_string(fixture_path)
            .expect("No se pudo leer el fixture de rects");
        let fixture: std::collections::HashMap<String, [f32; 4]> =
            serde_json::from_str(&fixture_str).expect("Fixture JSON inválido");

        for nc in &colocado.nodos {
            let clave = nc.entidad.to_string();
            if let Some(esperado) = fixture.get(&clave) {
                let r = &nc.rect;
                let diff_x = (r.x - esperado[0]).abs();
                let diff_y = (r.y - esperado[1]).abs();
                let diff_w = (r.ancho - esperado[2]).abs();
                let diff_h = (r.alto - esperado[3]).abs();
                assert!(
                    diff_x <= 0.5 && diff_y <= 0.5 && diff_w <= 0.5 && diff_h <= 0.5,
                    "Entidad {} difiere: got [{}, {}, {}, {}], expected {:?} (diffs: {}, {}, {}, {})",
                    nc.entidad, r.x, r.y, r.ancho, r.alto, esperado,
                    diff_x, diff_y, diff_w, diff_h
                );
            }
        }
    }

    /// Test 2: el medidor de texto coincide con ab_glyph directo (±1 px).
    #[test]
    fn medidor_texto_coincide_con_ab_glyph() {
        use ab_glyph::{Font, ScaleFont};

        let medidor = MedidorTexto::new();
        let (mw, mh) = medidor.medir("Hola mundo 123", 16.0, f32::INFINITY, 0.0);

        // Medida directa con ab_glyph.
        let font = fuente_global();
        let scale = ab_glyph::PxScale::from(16.0);
        let scaled = font.as_scaled(scale);

        let mut w = 0.0f32;
        let mut prev: Option<ab_glyph::GlyphId> = None;
        for c in "Hola mundo 123".chars() {
            let gid = scaled.glyph_id(c);
            if let Some(p) = prev {
                w += scaled.kern(p, gid);
            }
            w += scaled.h_advance(gid);
            prev = Some(gid);
        }
        let h = scaled.ascent() - scaled.descent() + scaled.line_gap();

        assert!(
            (mw - w).abs() <= 1.0,
            "Ancho difiere: medidor={mw}, directo={w}"
        );
        assert!(
            (mh - h).abs() <= 1.0,
            "Alto difiere: medidor={mh}, directo={h}"
        );
    }

    /// Test 3: la escala del canvas por defecto a 1280×720 es ≈ 2/3.
    #[test]
    fn escala_canvas_1280x720_es_dos_tercios() {
        let canvas = CanvasData::default();
        let s = canvas_scale_factor(&canvas, 1280.0, 720.0);
        let esperado = 2.0 / 3.0;
        assert!(
            (s - esperado).abs() < 0.01,
            "Escala esperada ≈ {esperado}, obtenida {s}"
        );
    }

    /// Test 4: nodo inactivo y sus hijos obtienen rect (0,0,0,0).
    #[test]
    fn nodo_inactivo_y_sus_hijos_rect_cero() {
        use crate::documento::{Documento, NodoDocumento};

        // Canvas raíz (idx 0), hijo inactivo (idx 1), nieto (idx 2).
        let canvas_data = CanvasData::default();
        let rt_identity = RectTransformData {
            anchored_position: [0.0, 0.0],
            size_delta: [100.0, 50.0],
            anchor_min: [0.5, 0.5],
            anchor_max: [0.5, 0.5],
            pivot: [0.5, 0.5],
        };

        let nodo_raiz = NodoDocumento {
            entidad: 1, nombre: String::new(),
            padre: None,
            hijos: vec![1],
            graphic: TipoGraphic::Ninguno,
            es_canvas: true,
            es_canvas_raiz: true,
            activo: true,
            rect_transform: RectTransformData {
                anchored_position: [0.0, 0.0],
                size_delta: [0.0, 0.0],
                anchor_min: [0.0, 0.0],
                anchor_max: [1.0, 1.0],
                pivot: [0.5, 0.5],
            },
            canvas: Some(canvas_data.clone()),
            imagen: None,
            texto: None,
            boton: None,
            drag: None,
            layout_group: None,
            style_extra: String::new(),
            raycast_target: false,
        };

        let nodo_inactivo = NodoDocumento {
            entidad: 2, nombre: String::new(),
            padre: Some(0),
            hijos: vec![2],
            graphic: TipoGraphic::Texto,
            es_canvas: false,
            es_canvas_raiz: false,
            activo: false, // ¡inactivo!
            rect_transform: rt_identity.clone(),
            canvas: None,
            imagen: None,
            texto: None,
            boton: None,
            drag: None,
            layout_group: None,
            style_extra: String::new(),
            raycast_target: false,
        };

        let nodo_hijo = NodoDocumento {
            entidad: 3, nombre: String::new(),
            padre: Some(1),
            hijos: vec![],
            graphic: TipoGraphic::Ninguno,
            es_canvas: false,
            es_canvas_raiz: false,
            activo: true, // activo, pero su padre no
            rect_transform: rt_identity,
            canvas: None,
            imagen: None,
            texto: None,
            boton: None,
            drag: None,
            layout_group: None,
            style_extra: String::new(),
            raycast_target: false,
        };

        let doc = Documento {
            canvas_raiz: 0,
            nodos: vec![nodo_raiz, nodo_inactivo, nodo_hijo],
            avisos: vec![],
        };

        let viewport = Viewport {
            ancho: 1280.0,
            alto: 720.0,
        };
        let colocado = colocar_documento(&doc, viewport);

        // El nodo inactivo (idx 1) y su hijo (idx 2) deben ser cero.
        let r1 = &colocado.nodos[1].rect;
        assert_eq!(
            (r1.x, r1.y, r1.ancho, r1.alto),
            (0.0, 0.0, 0.0, 0.0),
            "Nodo inactivo debe tener rect cero"
        );
        let r2 = &colocado.nodos[2].rect;
        assert_eq!(
            (r2.x, r2.y, r2.ancho, r2.alto),
            (0.0, 0.0, 0.0, 0.0),
            "Hijo de nodo inactivo debe tener rect cero"
        );
    }
    // --- P2b: flex (tests escritos a mano) ---

    /// Nodo simple para los tests de flex: rect centrado con el tamaño
    /// dado; el tamaño es lo que usa el flex (la posición la impone él).
    fn nodo_flex(
        entidad: u64,
        padre: Option<usize>,
        hijos: Vec<usize>,
        size: [f32; 2],
        extra: &str,
        grupo: Option<glass_core::scene::LayoutGroupData>,
    ) -> crate::documento::NodoDocumento {
        crate::documento::NodoDocumento {
            entidad, nombre: String::new(),
            padre,
            hijos,
            graphic: TipoGraphic::Ninguno,
            es_canvas: false,
            es_canvas_raiz: false,
            activo: true,
            rect_transform: RectTransformData {
                anchored_position: [0.0, 0.0],
                size_delta: size,
                anchor_min: [0.5, 0.5],
                anchor_max: [0.5, 0.5],
                pivot: [0.5, 0.5],
            },
            canvas: None,
            imagen: None,
            texto: None,
            boton: None,
            drag: None,
            layout_group: grupo,
            style_extra: extra.to_string(),
            raycast_target: false,
        }
    }

    /// Canvas raíz de referencia = viewport (escala 1).
    fn canvas_flex(entidad: u64, hijos: Vec<usize>, w: f32, h: f32) -> crate::documento::NodoDocumento {
        crate::documento::NodoDocumento {
            entidad, nombre: String::new(),
            padre: None,
            hijos,
            graphic: TipoGraphic::Ninguno,
            es_canvas: true,
            es_canvas_raiz: true,
            activo: true,
            rect_transform: RectTransformData {
                anchored_position: [0.0, 0.0],
                size_delta: [0.0, 0.0],
                anchor_min: [0.0, 0.0],
                anchor_max: [1.0, 1.0],
                pivot: [0.5, 0.5],
            },
            canvas: Some(CanvasData {
                render_mode: "ScreenSpaceOverlay".into(),
                reference_resolution: [w, h],
                match_width_or_height: 0.5,
            }),
            imagen: None,
            texto: None,
            boton: None,
            drag: None,
            layout_group: None,
            style_extra: String::new(),
            raycast_target: false,
        }
    }

    fn rect_de_colocado(colocado: &DocumentoColocado, entidad: u64) -> [f32; 4] {
        let n = colocado
            .nodos
            .iter()
            .find(|n| n.entidad == entidad)
            .expect("nodo colocado");
        [n.rect.x, n.rect.y, n.rect.ancho, n.rect.alto]
    }

    fn cerca_rect(a: [f32; 4], b: [f32; 4]) {
        for i in 0..4 {
            assert!((a[i] - b[i]).abs() < 0.5, "rect {:?} != esperado {:?}", a, b);
        }
    }

    #[test]
    fn flex_layout_group_horizontal_reparte_en_fila() {
        use glass_core::scene::LayoutGroupData;
        let grupo = LayoutGroupData {
            direction: "horizontal".into(),
            spacing: 10.0,
            padding: [5.0, 5.0, 5.0, 5.0],
            alignment: "start".into(),
        };
        let mut canvas = canvas_flex(1, vec![1, 2], 300.0, 100.0);
        canvas.layout_group = Some(grupo);
        let doc = crate::documento::Documento {
            canvas_raiz: 0,
            nodos: vec![
                canvas,
                nodo_flex(2, Some(0), vec![], [100.0, 40.0], "", None),
                nodo_flex(3, Some(0), vec![], [50.0, 40.0], "", None),
            ],
            avisos: vec![],
        };
        let colocado = colocar_documento(&doc, Viewport { ancho: 300.0, alto: 100.0 });
        cerca_rect(rect_de_colocado(&colocado, 2), [5.0, 5.0, 100.0, 40.0]);
        cerca_rect(rect_de_colocado(&colocado, 3), [115.0, 5.0, 50.0, 40.0]);
    }

    #[test]
    fn flex_layout_group_horizontal_centrado() {
        use glass_core::scene::LayoutGroupData;
        let grupo = LayoutGroupData {
            direction: "horizontal".into(),
            spacing: 10.0,
            padding: [5.0, 5.0, 5.0, 5.0],
            alignment: "center".into(),
        };
        let mut canvas = canvas_flex(1, vec![1, 2], 300.0, 100.0);
        canvas.layout_group = Some(grupo);
        let doc = crate::documento::Documento {
            canvas_raiz: 0,
            nodos: vec![
                canvas,
                nodo_flex(2, Some(0), vec![], [100.0, 40.0], "", None),
                nodo_flex(3, Some(0), vec![], [50.0, 40.0], "", None),
            ],
            avisos: vec![],
        };
        let colocado = colocar_documento(&doc, Viewport { ancho: 300.0, alto: 100.0 });
        // Interior 290, contenido 160 → bloque 65; x inicial 5 + 65 = 70.
        // La alineación "center" centra TAMBIÉN en el eje transversal
        // (comportamiento uGUI de layout_group_rects): y = 5 + (90−40)/2 = 30.
        cerca_rect(rect_de_colocado(&colocado, 2), [70.0, 30.0, 100.0, 40.0]);
        cerca_rect(rect_de_colocado(&colocado, 3), [180.0, 30.0, 50.0, 40.0]);
    }

    #[test]
    fn flex_css_fila_con_gap_y_justify_center() {
        let mut canvas = canvas_flex(1, vec![1, 2], 400.0, 80.0);
        canvas.style_extra =
            "display: flex; flex-direction: row; gap: 10px; justify-content: center;".into();
        let doc = crate::documento::Documento {
            canvas_raiz: 0,
            nodos: vec![
                canvas,
                nodo_flex(2, Some(0), vec![], [100.0, 40.0], "", None),
                nodo_flex(3, Some(0), vec![], [80.0, 40.0], "", None),
            ],
            avisos: vec![],
        };
        let colocado = colocar_documento(&doc, Viewport { ancho: 400.0, alto: 80.0 });
        // Libre 400 − 190 = 210; inicio 105.
        cerca_rect(rect_de_colocado(&colocado, 2), [105.0, 0.0, 100.0, 40.0]);
        cerca_rect(rect_de_colocado(&colocado, 3), [215.0, 0.0, 80.0, 40.0]);
    }

    #[test]
    fn flex_css_columna_con_align_stretch() {
        let mut canvas = canvas_flex(1, vec![1, 2], 200.0, 300.0);
        canvas.style_extra =
            "display: flex; flex-direction: column; align-items: stretch; gap: 5px;".into();
        let doc = crate::documento::Documento {
            canvas_raiz: 0,
            nodos: vec![
                canvas,
                nodo_flex(2, Some(0), vec![], [100.0, 40.0], "", None),
                nodo_flex(3, Some(0), vec![], [60.0, 30.0], "", None),
            ],
            avisos: vec![],
        };
        let colocado = colocar_documento(&doc, Viewport { ancho: 200.0, alto: 300.0 });
        cerca_rect(rect_de_colocado(&colocado, 2), [0.0, 0.0, 200.0, 40.0]);
        cerca_rect(rect_de_colocado(&colocado, 3), [0.0, 45.0, 200.0, 30.0]);
    }

    #[test]
    fn flex_anidado_dentro_de_absoluto() {
        let mut intermedio = nodo_flex(
            2,
            Some(0),
            vec![2, 3],
            [400.0, 200.0],
            "display: flex; flex-direction: row;",
            None,
        );
        intermedio.rect_transform.anchored_position = [0.0, 0.0];
        let doc = crate::documento::Documento {
            canvas_raiz: 0,
            nodos: vec![
                canvas_flex(1, vec![1], 800.0, 600.0),
                intermedio,
                nodo_flex(3, Some(1), vec![], [100.0, 40.0], "", None),
                nodo_flex(4, Some(1), vec![], [100.0, 40.0], "", None),
            ],
            avisos: vec![],
        };
        let colocado = colocar_documento(&doc, Viewport { ancho: 800.0, alto: 600.0 });
        // El padre flex queda centrado: [200, 200, 400, 200].
        let p = rect_de_colocado(&colocado, 2);
        cerca_rect(p, [200.0, 200.0, 400.0, 200.0]);
        let a = rect_de_colocado(&colocado, 3);
        let b = rect_de_colocado(&colocado, 4);
        cerca_rect(a, [200.0, 200.0, 100.0, 40.0]);
        cerca_rect(b, [300.0, 200.0, 100.0, 40.0]);
        assert!(a[0] >= p[0] && b[0] > a[0]);
        assert!(b[0] + b[2] <= p[0] + p[2] + 0.5);
    }
    // --- P2c: grid + límites (tests escritos a mano) ---

    #[test]
    fn grid_columnas_fijas_y_1fr() {
        let doc = Documento {
            canvas_raiz: 0,
            nodos: vec![
                NodoDocumento {
                    entidad: 1, nombre: String::new(), padre: None, hijos: vec![1, 2, 3],
                    graphic: TipoGraphic::Ninguno, es_canvas: true, es_canvas_raiz: true,
                    activo: true,
                    rect_transform: RectTransformData { anchored_position: [0.0, 0.0], size_delta: [400.0, 80.0], anchor_min: [0.0, 0.0], anchor_max: [0.0, 0.0], pivot: [0.0, 0.0] },
                    canvas: Some(CanvasData { render_mode: "ScreenSpaceOverlay".into(), reference_resolution: [400.0, 80.0], match_width_or_height: 0.0 }),
                    imagen: None, texto: None, boton: None, drag: None, layout_group: None,
                    style_extra: "display: grid; grid-template-columns: 100px 1fr; gap: 10px;".to_string(),
                    raycast_target: false,
                },
                NodoDocumento {
                    entidad: 2, nombre: String::new(), padre: Some(0), hijos: vec![],
                    graphic: TipoGraphic::Ninguno, es_canvas: false, es_canvas_raiz: false,
                    activo: true,
                    rect_transform: RectTransformData { anchored_position: [0.0, 0.0], size_delta: [100.0, 40.0], anchor_min: [0.5, 0.5], anchor_max: [0.5, 0.5], pivot: [0.5, 0.5] },
                    canvas: None, imagen: None, texto: None, boton: None, drag: None, layout_group: None,
                    style_extra: String::new(), raycast_target: false,
                },
                NodoDocumento {
                    entidad: 3, nombre: String::new(), padre: Some(0), hijos: vec![],
                    graphic: TipoGraphic::Ninguno, es_canvas: false, es_canvas_raiz: false,
                    activo: true,
                    rect_transform: RectTransformData { anchored_position: [0.0, 0.0], size_delta: [80.0, 40.0], anchor_min: [0.5, 0.5], anchor_max: [0.5, 0.5], pivot: [0.5, 0.5] },
                    canvas: None, imagen: None, texto: None, boton: None, drag: None, layout_group: None,
                    style_extra: String::new(), raycast_target: false,
                },
                NodoDocumento {
                    entidad: 4, nombre: String::new(), padre: Some(0), hijos: vec![],
                    graphic: TipoGraphic::Ninguno, es_canvas: false, es_canvas_raiz: false,
                    activo: true,
                    rect_transform: RectTransformData { anchored_position: [0.0, 0.0], size_delta: [60.0, 30.0], anchor_min: [0.5, 0.5], anchor_max: [0.5, 0.5], pivot: [0.5, 0.5] },
                    canvas: None, imagen: None, texto: None, boton: None, drag: None, layout_group: None,
                    style_extra: String::new(), raycast_target: false,
                },
            ],
            avisos: vec![],
        };
        let col = colocar_documento(&doc, Viewport { ancho: 400.0, alto: 80.0 });
        // Columnas [100, 290] (restante = 400 - 100 - 10 = 290).
        let h1 = &col.nodos[1];
        let h2 = &col.nodos[2];
        let h3 = &col.nodos[3];
        assert!((h1.rect.x - 0.0).abs() < 0.5 && (h1.rect.y - 0.0).abs() < 0.5, "h1 pos");
        assert!((h1.rect.ancho - 100.0).abs() < 0.5 && (h1.rect.alto - 40.0).abs() < 0.5, "h1 tam");
        assert!((h2.rect.x - 110.0).abs() < 0.5 && (h2.rect.y - 0.0).abs() < 0.5, "h2 pos");
        assert!((h2.rect.ancho - 80.0).abs() < 0.5 && (h2.rect.alto - 40.0).abs() < 0.5, "h2 tam");
        // Fila 2: y = alto fila 1 (40) + gap (10) = 50.
        assert!((h3.rect.x - 0.0).abs() < 0.5 && (h3.rect.y - 50.0).abs() < 0.5, "h3 pos");
        assert!((h3.rect.ancho - 60.0).abs() < 0.5 && (h3.rect.alto - 30.0).abs() < 0.5, "h3 tam");
    }

    #[test]
    fn grid_auto_fill_repite_celdas() {
        let doc = Documento {
            canvas_raiz: 0,
            nodos: vec![
                NodoDocumento {
                    entidad: 1, nombre: String::new(), padre: None, hijos: vec![1, 2, 3],
                    graphic: TipoGraphic::Ninguno, es_canvas: true, es_canvas_raiz: true,
                    activo: true,
                    rect_transform: RectTransformData { anchored_position: [0.0, 0.0], size_delta: [320.0, 80.0], anchor_min: [0.0, 0.0], anchor_max: [0.0, 0.0], pivot: [0.0, 0.0] },
                    canvas: Some(CanvasData { render_mode: "ScreenSpaceOverlay".into(), reference_resolution: [320.0, 80.0], match_width_or_height: 0.0 }),
                    imagen: None, texto: None, boton: None, drag: None, layout_group: None,
                    style_extra: "display: grid; grid-template-columns: repeat(auto-fill, 100px); gap: 10px;".to_string(),
                    raycast_target: false,
                },
                NodoDocumento {
                    entidad: 2, nombre: String::new(), padre: Some(0), hijos: vec![],
                    graphic: TipoGraphic::Ninguno, es_canvas: false, es_canvas_raiz: false,
                    activo: true,
                    rect_transform: RectTransformData { anchored_position: [0.0, 0.0], size_delta: [100.0, 40.0], anchor_min: [0.5, 0.5], anchor_max: [0.5, 0.5], pivot: [0.5, 0.5] },
                    canvas: None, imagen: None, texto: None, boton: None, drag: None, layout_group: None,
                    style_extra: String::new(), raycast_target: false,
                },
                NodoDocumento {
                    entidad: 3, nombre: String::new(), padre: Some(0), hijos: vec![],
                    graphic: TipoGraphic::Ninguno, es_canvas: false, es_canvas_raiz: false,
                    activo: true,
                    rect_transform: RectTransformData { anchored_position: [0.0, 0.0], size_delta: [100.0, 40.0], anchor_min: [0.5, 0.5], anchor_max: [0.5, 0.5], pivot: [0.5, 0.5] },
                    canvas: None, imagen: None, texto: None, boton: None, drag: None, layout_group: None,
                    style_extra: String::new(), raycast_target: false,
                },
                NodoDocumento {
                    entidad: 4, nombre: String::new(), padre: Some(0), hijos: vec![],
                    graphic: TipoGraphic::Ninguno, es_canvas: false, es_canvas_raiz: false,
                    activo: true,
                    rect_transform: RectTransformData { anchored_position: [0.0, 0.0], size_delta: [100.0, 40.0], anchor_min: [0.5, 0.5], anchor_max: [0.5, 0.5], pivot: [0.5, 0.5] },
                    canvas: None, imagen: None, texto: None, boton: None, drag: None, layout_group: None,
                    style_extra: String::new(), raycast_target: false,
                },
            ],
            avisos: vec![],
        };
        let col = colocar_documento(&doc, Viewport { ancho: 320.0, alto: 80.0 });
        // ncols = floor((320 + 10) / 110) = 3 -> x = 0, 110, 220.
        let esperados_x = [0.0_f32, 110.0, 220.0];
        for (i, exp_x) in esperados_x.iter().enumerate() {
            let n = &col.nodos[1 + i];
            assert!((n.rect.x - exp_x).abs() < 0.5, "x hijo {}", i);
            assert!((n.rect.y - 0.0).abs() < 0.5, "y hijo {}", i);
            assert!((n.rect.ancho - 100.0).abs() < 0.5 && (n.rect.alto - 40.0).abs() < 0.5, "tam hijo {}", i);
        }
    }

    #[test]
    fn limites_min_max_y_aspect_ratio() {
        // Helper local: nodo suelto bajo un canvas de 1000x1000 (escala 1).
        let nodo_suelto = |entidad: u64, y: f32, style_extra: &str| -> Documento {
            Documento {
                canvas_raiz: 0,
                nodos: vec![
                    NodoDocumento {
                        entidad: 1, nombre: String::new(), padre: None, hijos: vec![1],
                        graphic: TipoGraphic::Ninguno, es_canvas: true, es_canvas_raiz: true,
                        activo: true,
                        rect_transform: RectTransformData { anchored_position: [0.0, 0.0], size_delta: [1000.0, 1000.0], anchor_min: [0.0, 0.0], anchor_max: [0.0, 0.0], pivot: [0.0, 0.0] },
                        canvas: Some(CanvasData { render_mode: "ScreenSpaceOverlay".into(), reference_resolution: [1000.0, 1000.0], match_width_or_height: 0.0 }),
                        imagen: None, texto: None, boton: None, drag: None, layout_group: None,
                        style_extra: String::new(), raycast_target: false,
                    },
                    NodoDocumento {
                        entidad, nombre: String::new(), padre: Some(0), hijos: vec![],
                        graphic: TipoGraphic::Ninguno, es_canvas: false, es_canvas_raiz: false,
                        activo: true,
                        rect_transform: RectTransformData { anchored_position: [0.0, y], size_delta: [50.0, 50.0], anchor_min: [0.5, 0.5], anchor_max: [0.5, 0.5], pivot: [0.5, 0.5] },
                        canvas: None, imagen: None, texto: None, boton: None, drag: None, layout_group: None,
                        style_extra: style_extra.to_string(), raycast_target: false,
                    },
                ],
                avisos: vec![],
            }
        };
        let vp = Viewport { ancho: 1000.0, alto: 1000.0 };

        // (a) width 500px + max-width 200px -> ancho 200.
        let doc_a = nodo_suelto(2, 0.0, "width: 500px; max-width: 200px;");
        let col_a = colocar_documento(&doc_a, vp);
        assert!((col_a.nodos[1].rect.ancho - 200.0).abs() < 0.5, "max-width");

        // (b) width 100px + min-width 300px -> ancho 300.
        let doc_b = nodo_suelto(2, 0.0, "width: 100px; min-width: 300px;");
        let col_b = colocar_documento(&doc_b, vp);
        assert!((col_b.nodos[1].rect.ancho - 300.0).abs() < 0.5, "min-width");

        // (c) width 200px + aspect-ratio 2 con alto Auto -> alto 100.
        let doc_c = nodo_suelto(2, 0.0, "width: 200px; aspect-ratio: 2;");
        let col_c = colocar_documento(&doc_c, vp);
        assert!((col_c.nodos[1].rect.ancho - 200.0).abs() < 0.5, "aspect ancho");
            assert!((col_c.nodos[1].rect.alto - 100.0).abs() < 0.5, "aspect alto");
    }

    #[test]
    fn grid_anidado_dentro_de_flex() {
        // Canvas 800×600. Hijo flex (fila) centrado de 400×200 con un
        // hijo grid de 2 hijos; los hijos del grid quedan dentro de su
        // padre y en orden.
        let mut intermedio = nodo_flex(
            2,
            Some(0),
            vec![2],
            [400.0, 200.0],
            "display: flex; flex-direction: row;",
            None,
        );
        intermedio.rect_transform.anchored_position = [0.0, 0.0];
        let mut rejilla = nodo_flex(
            3,
            Some(1),
            vec![3, 4],
            [400.0, 200.0],
            "display: grid; grid-template-columns: 100px 1fr; gap: 10px;",
            None,
        );
        rejilla.rect_transform.anchored_position = [0.0, 0.0];
        let doc = crate::documento::Documento {
            canvas_raiz: 0,
            nodos: vec![
                canvas_flex(1, vec![1], 800.0, 600.0),
                intermedio,
                rejilla,
                nodo_flex(4, Some(2), vec![], [100.0, 40.0], "", None),
                nodo_flex(5, Some(2), vec![], [80.0, 40.0], "", None),
            ],
            avisos: vec![],
        };
        let colocado = colocar_documento(&doc, Viewport { ancho: 800.0, alto: 600.0 });
        // El padre flex queda centrado: [200, 200, 400, 200]; el grid
        // hereda ese rect como padre.
        let g = rect_de_colocado(&colocado, 3);
        cerca_rect(g, [200.0, 200.0, 400.0, 200.0]);
        let a = rect_de_colocado(&colocado, 4);
        let b = rect_de_colocado(&colocado, 5);
        cerca_rect(a, [200.0, 200.0, 100.0, 40.0]);
        cerca_rect(b, [310.0, 200.0, 80.0, 40.0]);
        assert!(a[0] >= g[0] && b[0] > a[0]);
        assert!(b[0] + b[2] <= g[0] + g[2] + 0.5);
    }


    use crate::estilos::TextOverflow;

    /// Avances esperados calculados directamente con ab_glyph, kern incluido.
    fn avances_ab_glyph(texto: &str, tamano: f32) -> Vec<f32> {
        use ab_glyph::{Font, ScaleFont};
        let font = fuente_global();
        let scaled = font.as_scaled(ab_glyph::PxScale::from(tamano));
        let mut out = Vec::new();
        let mut prev: Option<ab_glyph::GlyphId> = None;
        for c in texto.chars() {
            let gid = scaled.glyph_id(c);
            let mut w = scaled.h_advance(gid);
            if let Some(p) = prev {
                w += scaled.kern(p, gid);
            }
            out.push(w);
            prev = Some(gid);
        }
        out
    }

    fn alto_linea(tamano: f32) -> f32 {
        use ab_glyph::{Font, ScaleFont};
        let font = fuente_global();
        let scaled = font.as_scaled(ab_glyph::PxScale::from(tamano));
        scaled.ascent() - scaled.descent() + scaled.line_gap()
    }

    #[test]
    fn mapa_texto_avances_coinciden_con_ab_glyph() {
        let texto = "Hola mundo";
        let tam = 16.0;
        let mapa = construir_mapa_texto(
            texto,
            tam,
            f32::INFINITY,
            TextOverflow::Envolver,
            "Left",
            [0.0, 0.0],
            0.0,
        );
        let esperados = avances_ab_glyph(texto, tam);
        let alto = alto_linea(tam);

        assert_eq!(mapa.celdas.len(), esperados.len());
        assert_eq!(mapa.texto_visible, texto);

        let mut x_acum = 0.0f32;
        for (celda, (idx, &av)) in mapa.celdas.iter().zip(esperados.iter().enumerate()) {
            assert_eq!(celda.indice, idx);
            assert!((celda.celda[0] - x_acum).abs() <= 0.01, "x esperado {x_acum}, obtenido {}", celda.celda[0]);
            assert!((celda.celda[1] - 0.0).abs() <= 0.01, "y debe ser 0");
            assert!((celda.celda[2] - av).abs() <= 0.01, "ancho esperado {av}, obtenido {}", celda.celda[2]);
            assert!((celda.celda[3] - alto).abs() <= 0.01, "alto esperado {alto}, obtenido {}", celda.celda[3]);
            x_acum += av;
        }
    }


    use crate::api::MedidaTexto;

    #[test]
    fn mapa_texto_wrap_en_varias_lineas() {
        let texto = "aaa bbb ccc ddd";
        let tam = 16.0;
        // Elegir ancho para forzar al menos 2 líneas: medir una sola palabra
        // y dejar sitio para una palabra + espacio pero no dos.
        let medidor = MedidorTexto;
        let una_palabra = medidor.medir("aaa", tam, f32::INFINITY, 0.0).0;
        let ancho_max = una_palabra * 2.2;
        // Confirmar que el texto completo no cabe.
        let (ancho_una_linea, _) = medidor.medir(texto, tam, f32::INFINITY, 0.0);
        assert!(ancho_una_linea > ancho_max, "el ancho debe forzar wrap");

        let mapa = construir_mapa_texto(
            texto,
            tam,
            ancho_max,
            TextOverflow::Envolver,
            "Left",
            [0.0, 0.0],
            0.0,
        );
        let alto = alto_linea(tam);

        // Celdas en al menos 2 valores distintos de y.
        let mut ys: Vec<f32> = mapa.celdas.iter().map(|c| c.celda[1]).collect();
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        ys.dedup_by(|a, b| (*a - *b).abs() <= 0.01);
        assert!(ys.len() >= 2, "debe haber al menos 2 líneas, y={ys:?}");

        // y avanza exactamente alto_linea por línea.
        for (i, y) in ys.iter().enumerate() {
            let esperado = i as f32 * alto;
            assert!((y - esperado).abs() <= 0.01, "y de línea {i}: esperado {esperado}, obtenido {y}");
        }

        // Ninguna celda sale del ancho.
        for c in &mapa.celdas {
            assert!(c.celda[0] + c.celda[2] <= ancho_max + 0.01, "celda sobresale del ancho");
        }
    }

    #[test]
    fn mapa_texto_truncar_y_puntos() {
        let texto = "abcdefghijklmnopqrstuvwxyz abcdefghij";
        let tam = 16.0;
        let ancho_max = 50.0;

        let truncado = construir_mapa_texto(
            texto,
            tam,
            ancho_max,
            TextOverflow::Truncar,
            "Left",
            [0.0, 0.0],
            0.0,
        );
        assert!(texto.starts_with(&truncado.texto_visible), "truncado debe ser prefijo");
        assert!(!truncado.texto_visible.is_empty());
        for c in &truncado.celdas {
            assert!(c.celda[0] + c.celda[2] <= ancho_max + 0.01, "celda truncada fuera del ancho");
        }

        let puntos = construir_mapa_texto(
            texto,
            tam,
            ancho_max,
            TextOverflow::PuntosSuspensivos,
            "Left",
            [0.0, 0.0],
            0.0,
        );
        assert!(puntos.texto_visible.ends_with('…'), "debe terminar en puntos suspensivos");
        for c in &puntos.celdas {
            assert!(c.celda[0] + c.celda[2] <= ancho_max + 0.01, "celda con puntos fuera del ancho");
        }
        // La última celda es la de '…' con indice = chars del texto original.
        let ultima = puntos.celdas.last().expect("debe haber celdas");
        assert_eq!(ultima.caracter, '…');
        assert_eq!(ultima.indice, texto.chars().count());
    }

    #[test]
    fn mapa_texto_letter_spacing_ensancha() {
        let tam = 16.0;
        let base = construir_mapa_texto("AB", tam, f32::INFINITY, TextOverflow::Envolver, "Left", [0.0, 0.0], 0.0);
        let esp = construir_mapa_texto("AB", tam, f32::INFINITY, TextOverflow::Envolver, "Left", [0.0, 0.0], 10.0);
        assert_eq!(esp.celdas.len(), 2);
        for (i, celda) in esp.celdas.iter().enumerate() {
            assert!((celda.celda[2] - (base.celdas[i].celda[2] + 10.0)).abs() < 0.01);
        }
        let ancho_base: f32 = base.celdas.iter().map(|c| c.celda[2]).sum();
        let ancho_esp: f32 = esp.celdas.iter().map(|c| c.celda[2]).sum();
        assert!((ancho_esp - (ancho_base + 20.0)).abs() < 0.01);
    }

    #[test]
    fn medir_letter_spacing_ensancha() {
        let medidor = MedidorTexto::new();
        let tam = 16.0;
        let (a0, _) = medidor.medir("AB", tam, f32::INFINITY, 0.0);
        let (a1, _) = medidor.medir("AB", tam, f32::INFINITY, 10.0);
        assert!((a1 - (a0 + 20.0)).abs() < 0.01);
    }

    #[test]
    fn mapa_texto_nueva_linea_es_salto_forzado() {
        let texto = "AB\nCD";
        let tam = 16.0;
        let mapa = construir_mapa_texto(
            texto,
            tam,
            f32::INFINITY,
            TextOverflow::Envolver,
            "Left",
            [0.0, 0.0],
            0.0,
        );
        let alto = alto_linea(tam);

        let indices: Vec<usize> = mapa.celdas.iter().map(|c| c.indice).collect();
        for i in [0, 1, 3, 4] {
            assert!(indices.contains(&i), "falta índice {i}");
        }
        assert!(!indices.contains(&2), "el '\\n' no debe tener celda");

        // La 'C' (índice 3) está en la línea 1.
        let c = mapa.celdas.iter().find(|c| c.indice == 3).expect("debe existir C");
        assert_eq!(c.caracter, 'C');
        assert!((c.celda[1] - alto).abs() <= 0.01, "C debe estar en la línea 1 (y={})", c.celda[1]);

        // La 'A' está en la línea 0.
        let a = mapa.celdas.iter().find(|c| c.indice == 0).expect("debe existir A");
        assert!((a.celda[1] - 0.0).abs() <= 0.01, "A debe estar en la línea 0");
        assert_eq!(mapa.texto_visible, texto);
    }

    #[test]
    fn medir_con_salto_de_linea_cuenta_dos_lineas() {
        // Hallazgo pez 1/2 (fin de módulo V3): medir contaba '\n' como
        // un carácter normal y devolvía una sola línea de alto mientras
        // el mapa producía dos. Ahora medir y mapa coinciden.
        let medidor = MedidorTexto::new();
        let tam = 16.0;
        let (ancho, alto) = medidor.medir("AB\nCD", tam, f32::INFINITY, 0.0);
        assert!((alto - 2.0 * alto_linea(tam)).abs() <= 0.01, "alto={alto}");
        let (ancho_ab, _) = medidor.medir("AB", tam, f32::INFINITY, 0.0);
        let (ancho_cd, _) = medidor.medir("CD", tam, f32::INFINITY, 0.0);
        assert!((ancho - ancho_ab.max(ancho_cd)).abs() <= 0.01, "ancho={ancho}");
    }

    #[test]
    fn mapa_texto_truncar_y_puntos_se_detienen_en_salto_de_linea() {
        // Hallazgo pez 1/2: Truncar/PuntosSuspensivos calculaban avances
        // sobre todo el texto incluido '\n'. Una sola línea: el salto
        // termina el texto visible.
        let tam = 16.0;
        let mapa = construir_mapa_texto(
            "abc\ndef",
            tam,
            1000.0,
            TextOverflow::Truncar,
            "Left",
            [0.0, 0.0],
            0.0,
        );
        assert_eq!(mapa.texto_visible, "abc");
        assert!(mapa.celdas.iter().all(|c| c.indice < 3));

        let mapa = construir_mapa_texto(
            "abcdefgh\nxyz",
            tam,
            40.0,
            TextOverflow::PuntosSuspensivos,
            "Left",
            [0.0, 0.0],
            0.0,
        );
        assert!(!mapa.texto_visible.contains('\n'), "sin salto: {}", mapa.texto_visible);
        assert!(mapa.texto_visible.ends_with('…'), "con puntos: {}", mapa.texto_visible);
    }


    /// Nodo de texto bajo el canvas, centrado, con el tamaño y el
    /// style_extra dados.
    fn nodo_texto_escena(
        entidad: u64,
        texto: &str,
        size: [f32; 2],
        extra: &str,
    ) -> crate::documento::NodoDocumento {
        use glass_core::scene::UITextData;
        crate::documento::NodoDocumento {
            entidad, nombre: String::new(),
            padre: Some(0),
            hijos: vec![],
            graphic: TipoGraphic::Texto,
            es_canvas: false,
            es_canvas_raiz: false,
            activo: true,
            rect_transform: RectTransformData {
                anchored_position: [0.0, 0.0],
                size_delta: size,
                anchor_min: [0.5, 0.5],
                anchor_max: [0.5, 0.5],
                pivot: [0.5, 0.5],
            },
            canvas: None,
            imagen: None,
            texto: Some(UITextData {
                text: texto.to_string(),
                font_size: 16.0,
                color: [1.0, 1.0, 1.0, 1.0],
                font: "default".to_string(),
                align: "Left".to_string(),
                style_extra: String::new(),
                raycast_target: true,
            }),
            boton: None,
            drag: None,
            layout_group: None,
            style_extra: extra.to_string(),
            raycast_target: true,
        }
    }

    /// Escena de desbordamiento: canvas 400x100 (escala 1) con un nodo
    /// de texto de 100 px de ancho.
    fn doc_desbordamiento(texto: &str, extra: &str) -> Documento {
        Documento {
            canvas_raiz: 0,
            nodos: vec![
                canvas_flex(1, vec![1], 400.0, 100.0),
                nodo_texto_escena(2, texto, [100.0, 40.0], extra),
            ],
            avisos: vec![],
        }
    }

    fn nodo_texto_colocado(colocado: &DocumentoColocado) -> &NodoColocado {
        colocado.nodos.iter().find(|n| n.entidad == 2).expect("nodo de texto")
    }

    /// Ninguna celda sale del rect colocado del nodo.
    fn celdas_dentro_del_rect(n: &NodoColocado) {
        for c in &n.mapa_texto.celdas {
            assert!(
                c.celda[0] + c.celda[2] <= n.rect.x + n.rect.ancho + 0.01,
                "celda fuera del rect: {:?} rect {:?}",
                c.celda,
                n.rect
            );
        }
    }

    #[test]
    fn escena_ellipsis_recorta_con_puntos() {
        let doc = doc_desbordamiento(
            "abcdefghijklmnopqrstuvwxyz abcdefghij",
            "width: 100px; text-overflow: ellipsis;",
        );
        let colocado = colocar_documento(&doc, Viewport { ancho: 400.0, alto: 100.0 });
        let n = nodo_texto_colocado(&colocado);
        assert!(n.mapa_texto.texto_visible.ends_with('…'), "visible: {}", n.mapa_texto.texto_visible);
        celdas_dentro_del_rect(n);
    }

    #[test]
    fn escena_truncate_recorta_sin_puntos() {
        let texto = "abcdefghijklmnopqrstuvwxyz abcdefghij";
        let doc = doc_desbordamiento(texto, "width: 100px; text-overflow: truncate;");
        let colocado = colocar_documento(&doc, Viewport { ancho: 400.0, alto: 100.0 });
        let n = nodo_texto_colocado(&colocado);
        assert!(!n.mapa_texto.texto_visible.is_empty());
        assert!(texto.starts_with(&n.mapa_texto.texto_visible), "visible: {}", n.mapa_texto.texto_visible);
        assert!(!n.mapa_texto.texto_visible.ends_with('…'));
        celdas_dentro_del_rect(n);
    }

    #[test]
    fn escena_input_field_tiene_mapa_texto() {
        // V4b: el layout construía el mapa solo para graphic Texto; el
        // InputField quedaba con texto_visible vacío y el pintor no
        // emitía el segmento de texto (campo blanco sin letras).
        let mut doc = doc_desbordamiento("Escribe...", "");
        doc.nodos[1].graphic = TipoGraphic::InputField;
        let colocado = colocar_documento(&doc, Viewport { ancho: 400.0, alto: 100.0 });
        let n = nodo_texto_colocado(&colocado);
        assert_eq!(n.mapa_texto.texto_visible, "Escribe...");
        celdas_dentro_del_rect(n);
    }

    #[test]
    fn escena_wrap_parte_en_varias_lineas() {
        let doc = doc_desbordamiento("aaa bbb ccc ddd", "width: 100px; text-overflow: wrap;");
        let colocado = colocar_documento(&doc, Viewport { ancho: 400.0, alto: 100.0 });
        let n = nodo_texto_colocado(&colocado);
        let mut ys: Vec<f32> = n.mapa_texto.celdas.iter().map(|c| c.celda[1]).collect();
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        ys.dedup_by(|a, b| (*a - *b).abs() <= 0.01);
        assert!(ys.len() >= 2, "debe haber al menos 2 líneas, y={ys:?}");
        celdas_dentro_del_rect(n);
    }

}
