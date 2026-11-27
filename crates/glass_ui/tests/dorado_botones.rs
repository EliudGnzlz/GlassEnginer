//! G-AUTOR1: la pantalla de los 5 botones reconstruida SOLO con el
//! editor debe derivar el mismo documento que la escena escrita a
//! mano (`EscenaBotones.scene.json`). La identidad que se compara es
//! el NOMBRE de la entidad (el editor asigna ids frescos al crear;
//! el autor fija los nombres en el Inspector).
//!
//! El esperado es un JSON dorado versionado en el repo
//! (`fixtures/dorado_botones_1280x720.json`): por entidad, rect
//! colocado a 1280x720 y estilos resueltos (bajo la convención Q4:
//! el parser convierte sRGB a lineal y el ángulo de degradado sigue
//! CSS). El comparador hace diff campo a campo: rects con tolerancia
//! 0.5 px, estilos con tolerancia 1e-3 (absorbe 1 ULP entre la
//! conversión del generador y la del Inspector).
//!
//! Si el dorado no existe, el test lo genera desde la escena de
//! referencia y lo escribe en `target/dorado_botones_candidata.json`
//! (copiarlo a `fixtures/` para fijarlo). La escena rehecha con el
//! editor vive en `fixtures/EscenaBotonesEditor.scene.json`.

use glass_core::scene::{parsear_escena, SceneState};
use glass_ui::api::{colocar_documento, Viewport};
use glass_ui::derivacion::derivar_documentos;
use glass_ui::documento::TipoGraphic;
use glass_ui::estilos::{Dimension, Display, StyleProps, TamanoColumna, TextOverflow};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const VP: Viewport = Viewport {
    ancho: 1280.0,
    alto: 720.0,
};

const TOL_RECT: f32 = 0.5;
const TOL_ESTILO: f32 = 1e-3;

fn ruta_fixture(nombre: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(nombre)
}

fn escena_desde_json(ruta: &PathBuf) -> SceneState {
    let texto = std::fs::read_to_string(ruta)
        .unwrap_or_else(|e| panic!("no se pudo leer {}: {}", ruta.display(), e));
    let (_version, entidades) =
        parsear_escena(&texto).unwrap_or_else(|e| panic!("escena inválida {}: {}", ruta.display(), e));
    let mut escena = SceneState::default();
    escena.replace_entities(entidades);
    escena
}

fn cerca(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn cerca_lista(a: &[f32; 4], b: &[f32; 4], tol: f32) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| cerca(*x, *y, tol))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DimensionDorado {
    #[serde(rename = "tipo")]
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    valor: Option<f32>,
}

fn dimension_d(d: &Dimension) -> DimensionDorado {
    match d {
        Dimension::Px(v) => DimensionDorado {
            kind: "Px".to_string(),
            valor: Some(*v),
        },
        Dimension::Porcentaje(v) => DimensionDorado {
            kind: "Porcentaje".to_string(),
            valor: Some(*v),
        },
        Dimension::Auto => DimensionDorado {
            kind: "Auto".to_string(),
            valor: None,
        },
    }
}

fn compara_dimension(a: &DimensionDorado, b: &DimensionDorado, donde: &str, fallos: &mut Vec<String>) {
    if a.kind != b.kind {
        fallos.push(format!("{donde}: Dimension {a:?} != {b:?}"));
        return;
    }
    match (a.valor, b.valor) {
        (Some(x), Some(y)) => {
            if !cerca(x, y, TOL_ESTILO) {
                fallos.push(format!("{donde}: Dimension {} {x} != {y}", a.kind));
            }
        }
        (None, None) => {}
        _ => fallos.push(format!("{donde}: Dimension {a:?} != {b:?}")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SombraDorado {
    dx: f32,
    dy: f32,
    desenfoque: f32,
    color: [f32; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TamanoColumnaDorado {
    #[serde(rename = "tipo")]
    kind: String,
    valor: f32,
}

fn columna_d(c: &TamanoColumna) -> TamanoColumnaDorado {
    match c {
        TamanoColumna::Fija(v) => TamanoColumnaDorado {
            kind: "Fija".to_string(),
            valor: *v,
        },
        TamanoColumna::Fraccion(v) => TamanoColumnaDorado {
            kind: "Fraccion".to_string(),
            valor: *v,
        },
        TamanoColumna::Relleno(v) => TamanoColumnaDorado {
            kind: "Relleno".to_string(),
            valor: *v,
        },
    }
}

/// Proyección serializable de StyleProps (todos sus campos).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EstilosDorado {
    #[serde(skip_serializing_if = "Option::is_none")]
    display: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    flex_direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gap: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    padding: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    justify_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    align_items: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<DimensionDorado>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<DimensionDorado>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_width: Option<DimensionDorado>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_width: Option<DimensionDorado>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_height: Option<DimensionDorado>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_height: Option<DimensionDorado>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aspect_ratio: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    background_color: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    background_gradient: Option<(f32, Vec<[f32; 4]>)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    border_radius: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    border: Option<(f32, [f32; 4])>,
    #[serde(skip_serializing_if = "Option::is_none")]
    letter_spacing: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    box_shadow: Option<SombraDorado>,
    #[serde(skip_serializing_if = "Option::is_none")]
    opacity: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    overflow: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    font_size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    grid_template_columns: Option<Vec<TamanoColumnaDorado>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sort_order: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    override_sorting: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_overflow: Option<String>,
}

fn estilos_d(e: &StyleProps) -> EstilosDorado {
    EstilosDorado {
        display: e.display.map(|d| match d {
            Display::Absoluto => "Absoluto".to_string(),
            Display::Flex => "Flex".to_string(),
            Display::Grid => "Grid".to_string(),
            Display::Ninguno => "Ninguno".to_string(),
        }),
        flex_direction: e.flex_direction.clone(),
        gap: e.gap,
        padding: e.padding,
        justify_content: e.justify_content.clone(),
        align_items: e.align_items.clone(),
        width: e.width.as_ref().map(dimension_d),
        height: e.height.as_ref().map(dimension_d),
        min_width: e.min_width.as_ref().map(dimension_d),
        max_width: e.max_width.as_ref().map(dimension_d),
        min_height: e.min_height.as_ref().map(dimension_d),
        max_height: e.max_height.as_ref().map(dimension_d),
        aspect_ratio: e.aspect_ratio,
        background_color: e.background_color,
        background_gradient: e.background_gradient.clone(),
        border_radius: e.border_radius,
        border: e.border,
        letter_spacing: e.letter_spacing,
        box_shadow: e.box_shadow.map(|s| SombraDorado {
            dx: s.dx,
            dy: s.dy,
            desenfoque: s.desenfoque,
            color: s.color,
        }),
        opacity: e.opacity,
        overflow: e.overflow.clone(),
        color: e.color,
        font_size: e.font_size,
        grid_template_columns: e.grid_template_columns.as_ref().map(|v| {
            v.iter().map(columna_d).collect()
        }),
        sort_order: e.sort_order,
        override_sorting: e.override_sorting,
        text_overflow: e.text_overflow.map(|t| match t {
            TextOverflow::Envolver => "Envolver".to_string(),
            TextOverflow::Truncar => "Truncar".to_string(),
            TextOverflow::PuntosSuspensivos => "PuntosSuspensivos".to_string(),
        }),
    }
}

fn compara_opcion_f32(
    a: &Option<f32>,
    b: &Option<f32>,
    donde: &str,
    fallos: &mut Vec<String>,
) {
    match (a, b) {
        (Some(x), Some(y)) => {
            if !cerca(*x, *y, TOL_ESTILO) {
                fallos.push(format!("{donde}: {x} != {y}"));
            }
        }
        (None, None) => {}
        _ => fallos.push(format!("{donde}: {a:?} != {b:?}")),
    }
}

fn compara_opcion_color(
    a: &Option<[f32; 4]>,
    b: &Option<[f32; 4]>,
    donde: &str,
    fallos: &mut Vec<String>,
) {
    match (a, b) {
        (Some(x), Some(y)) => {
            if !cerca_lista(x, y, TOL_ESTILO) {
                fallos.push(format!("{donde}: {x:?} != {y:?}"));
            }
        }
        (None, None) => {}
        _ => fallos.push(format!("{donde}: {a:?} != {b:?}")),
    }
}

fn compara_estilos(a: &EstilosDorado, b: &EstilosDorado, donde: &str, fallos: &mut Vec<String>) {
    macro_rules! campo_str {
        ($campo:ident) => {
            if a.$campo != b.$campo {
                fallos.push(format!(
                    "{donde}: {} {:?} != {:?}",
                    stringify!($campo),
                    a.$campo,
                    b.$campo
                ));
            }
        };
    }
    macro_rules! campo_f32 {
        ($campo:ident) => {
            compara_opcion_f32(&a.$campo, &b.$campo, &format!("{donde}: {}", stringify!($campo)), fallos);
        };
    }
    macro_rules! campo_color {
        ($campo:ident) => {
            compara_opcion_color(
                &a.$campo,
                &b.$campo,
                &format!("{donde}: {}", stringify!($campo)),
                fallos,
            );
        };
    }
    macro_rules! campo_dimension {
        ($campo:ident) => {
            match (&a.$campo, &b.$campo) {
                (Some(x), Some(y)) => compara_dimension(
                    x,
                    y,
                    &format!("{donde}: {}", stringify!($campo)),
                    fallos,
                ),
                (None, None) => {}
                _ => fallos.push(format!(
                    "{donde}: {} {:?} != {:?}",
                    stringify!($campo),
                    a.$campo,
                    b.$campo
                )),
            }
        };
    }
    campo_str!(display);
    campo_str!(flex_direction);
    campo_f32!(gap);
    match (&a.padding, &b.padding) {
        (Some(x), Some(y)) => {
            if !cerca_lista(x, y, TOL_ESTILO) {
                fallos.push(format!("{donde}: padding {x:?} != {y:?}"));
            }
        }
        (None, None) => {}
        _ => fallos.push(format!("{donde}: padding {:?} != {:?}", a.padding, b.padding)),
    }
    campo_str!(justify_content);
    campo_str!(align_items);
    campo_dimension!(width);
    campo_dimension!(height);
    campo_dimension!(min_width);
    campo_dimension!(max_width);
    campo_dimension!(min_height);
    campo_dimension!(max_height);
    campo_f32!(aspect_ratio);
    campo_color!(background_color);
    match (&a.background_gradient, &b.background_gradient) {
        (Some((ag, cg)), Some((bg, dg))) => {
            if !cerca(*ag, *bg, TOL_ESTILO) {
                fallos.push(format!("{donde}: ángulo degradado {ag} != {bg}"));
            }
            if cg.len() != dg.len() {
                fallos.push(format!(
                    "{donde}: paradas degradado {} != {}",
                    cg.len(),
                    dg.len()
                ));
            } else {
                for (i, (c, d)) in cg.iter().zip(dg.iter()).enumerate() {
                    if !cerca_lista(c, d, TOL_ESTILO) {
                        fallos.push(format!("{donde}: parada {i} {c:?} != {d:?}"));
                    }
                }
            }
        }
        (None, None) => {}
        _ => fallos.push(format!(
            "{donde}: background_gradient {:?} != {:?}",
            a.background_gradient, b.background_gradient
        )),
    }
    match (&a.border_radius, &b.border_radius) {
        (Some(x), Some(y)) => {
            if !cerca_lista(x, y, TOL_ESTILO) {
                fallos.push(format!("{donde}: border_radius {x:?} != {y:?}"));
            }
        }
        (None, None) => {}
        _ => fallos.push(format!(
            "{donde}: border_radius {:?} != {:?}",
            a.border_radius, b.border_radius
        )),
    }
    match (&a.border, &b.border) {
        (Some((aw, ac)), Some((bw, bc))) => {
            if !cerca(*aw, *bw, TOL_ESTILO) {
                fallos.push(format!("{donde}: border ancho {aw} != {bw}"));
            }
            if !cerca_lista(ac, bc, TOL_ESTILO) {
                fallos.push(format!("{donde}: border color {ac:?} != {bc:?}"));
            }
        }
        (None, None) => {}
        _ => fallos.push(format!("{donde}: border {:?} != {:?}", a.border, b.border)),
    }
    campo_f32!(letter_spacing);
    match (&a.box_shadow, &b.box_shadow) {
        (Some(x), Some(y)) => {
            if !cerca(x.dx, y.dx, TOL_ESTILO)
                || !cerca(x.dy, y.dy, TOL_ESTILO)
                || !cerca(x.desenfoque, y.desenfoque, TOL_ESTILO)
                || !cerca_lista(&x.color, &y.color, TOL_ESTILO)
            {
                fallos.push(format!("{donde}: box_shadow {x:?} != {y:?}"));
            }
        }
        (None, None) => {}
        _ => fallos.push(format!(
            "{donde}: box_shadow {:?} != {:?}",
            a.box_shadow, b.box_shadow
        )),
    }
    campo_f32!(opacity);
    campo_str!(overflow);
    campo_color!(color);
    campo_f32!(font_size);
    match (&a.grid_template_columns, &b.grid_template_columns) {
        (Some(x), Some(y)) => {
            if x.len() != y.len() {
                fallos.push(format!(
                    "{donde}: columnas {} != {}",
                    x.len(),
                    y.len()
                ));
            } else {
                for (i, (c, d)) in x.iter().zip(y.iter()).enumerate() {
                    if c.kind != d.kind || !cerca(c.valor, d.valor, TOL_ESTILO) {
                        fallos.push(format!("{donde}: columna {i} {c:?} != {d:?}"));
                    }
                }
            }
        }
        (None, None) => {}
        _ => fallos.push(format!(
            "{donde}: grid_template_columns {:?} != {:?}",
            a.grid_template_columns, b.grid_template_columns
        )),
    }
    campo_str!(sort_order);
    campo_str!(override_sorting);
    campo_str!(text_overflow);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodoDorado {
    entidad: u64,
    nombre: String,
    graphic: String,
    rect: [f32; 4],
    estilos: EstilosDorado,
    texto_visible: String,
}

fn documento_dorado(ruta_escena: &PathBuf) -> Vec<NodoDorado> {
    let escena = escena_desde_json(ruta_escena);
    let docs = derivar_documentos(&escena);
    assert_eq!(
        docs.len(),
        1,
        "la escena {} debe derivar un solo documento (un Canvas)",
        ruta_escena.display()
    );
    let doc = &docs[0];
    let colocado = colocar_documento(doc, VP);
    let mut nodos: Vec<NodoDorado> = doc
        .nodos
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let c = &colocado.nodos[i];
            NodoDorado {
                entidad: n.entidad,
                nombre: n.nombre.clone(),
                graphic: match n.graphic {
                    TipoGraphic::Ninguno => "Ninguno".to_string(),
                    TipoGraphic::Imagen => "Imagen".to_string(),
                    TipoGraphic::Texto => "Texto".to_string(),
                    TipoGraphic::InputField => "InputField".to_string(),
                },
                rect: [c.rect.x, c.rect.y, c.rect.ancho, c.rect.alto],
                estilos: estilos_d(&c.estilos),
                texto_visible: c.mapa_texto.texto_visible.clone(),
            }
        })
        .collect();
    // Orden estable por NOMBRE para que el diff sea legible: la
    // identidad que compara el dorado es el nombre (lo que el autor
    // fija en el editor); los ids los asigna el editor al crear y no
    // tienen por qué coincidir con los de la escena escrita a mano.
    nodos.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    nodos
}

fn compara_dorados(esperado: &[NodoDorado], obtenido: &[NodoDorado], donde: &str) {
    // Ambas listas se ordenan por nombre antes de emparejar: el
    // dorado guardado puede venir ordenado por id.
    let mut esperado: Vec<&NodoDorado> = esperado.iter().collect();
    let mut obtenido: Vec<&NodoDorado> = obtenido.iter().collect();
    esperado.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    obtenido.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    let mut fallos: Vec<String> = Vec::new();
    if esperado.len() != obtenido.len() {
        panic!(
            "{donde}: nº de nodos {} != {} (esperado {:?}, obtenido {:?})",
            esperado.len(),
            obtenido.len(),
            esperado.iter().map(|n| n.entidad).collect::<Vec<_>>(),
            obtenido.iter().map(|n| n.entidad).collect::<Vec<_>>(),
        );
    }
    for (e, o) in esperado.iter().zip(obtenido.iter()) {
        let base = format!("{donde} «{}»", e.nombre);
        if e.nombre != o.nombre {
            fallos.push(format!(
                "{base}: nombre {:?} != {:?} (id dorado {} / id obtenido {})",
                e.nombre, o.nombre, e.entidad, o.entidad
            ));
            continue;
        }
        if e.graphic != o.graphic {
            fallos.push(format!("{base}: graphic {} != {}", e.graphic, o.graphic));
        }
        if !cerca_lista(&e.rect, &o.rect, TOL_RECT) {
            fallos.push(format!("{base}: rect {:?} != {:?}", e.rect, o.rect));
        }
        if e.texto_visible != o.texto_visible {
            fallos.push(format!(
                "{base}: texto {:?} != {:?}",
                e.texto_visible, o.texto_visible
            ));
        }
        compara_estilos(&e.estilos, &o.estilos, &base, &mut fallos);
    }
    assert!(
        fallos.is_empty(),
        "{donde}: {} diferencias:\n{}",
        fallos.len(),
        fallos.join("\n")
    );
}

#[test]
fn dorado_desde_referencia() {
    let ruta_dorado = ruta_fixture("dorado_botones_1280x720.json");
    let obtenido = documento_dorado(&ruta_fixture("EscenaBotones.scene.json"));
    if !ruta_dorado.exists() {
        let candidata = ruta_fixture("dorado_botones_candidata.json");
        std::fs::write(
            &candidata,
            serde_json::to_string_pretty(&obtenido).unwrap(),
        )
        .unwrap();
        panic!(
            "dorado ausente: generado desde la referencia en {} ({} nodos). Cópialo a {} y vuelve a correr.",
            candidata.display(),
            obtenido.len(),
            ruta_dorado.display()
        );
    }
    let texto = std::fs::read_to_string(&ruta_dorado).unwrap();
    let esperado: Vec<NodoDorado> = serde_json::from_str(&texto).unwrap();
    compara_dorados(&esperado, &obtenido, "referencia");
}

/// Nombres en orden de pintado (DFS pre-orden de `children`), sin ordenar
/// por nombre: es el orden en que el pintor emite los nodos.
/// H3 (peces V4): el golden por nombre no veía el orden y los halos de
/// Rosa/Rojo se pintaban ENCIMA de sus botones sin que el test lo cazara.
fn orden_pintado(ruta_escena: &PathBuf) -> Vec<String> {
    let escena = escena_desde_json(ruta_escena);
    let docs = derivar_documentos(&escena);
    assert_eq!(
        docs.len(),
        1,
        "la escena {} debe derivar un solo documento (un Canvas)",
        ruta_escena.display()
    );
    docs[0].nodos.iter().map(|n| n.nombre.clone()).collect()
}

#[test]
fn orden_pintado_igual_y_nombres_unicos() {
    let referencia = orden_pintado(&ruta_fixture("EscenaBotones.scene.json"));
    let editor = orden_pintado(&ruta_fixture("EscenaBotonesEditor.scene.json"));
    // Nombres únicos en cada escena: si no, la clave por nombre colisiona.
    let mut unicos = referencia.clone();
    unicos.sort();
    unicos.dedup();
    assert_eq!(
        unicos.len(),
        referencia.len(),
        "la referencia tiene nombres duplicados"
    );
    let mut unicos_e = editor.clone();
    unicos_e.sort();
    unicos_e.dedup();
    assert_eq!(
        unicos_e.len(),
        editor.len(),
        "la escena del editor tiene nombres duplicados"
    );
    assert_eq!(
        editor, referencia,
        "el orden de pintado del editor debe igualar la referencia"
    );
}

#[test]
fn editor_igual_a_dorado() {
    let ruta_rehecha = ruta_fixture("EscenaBotonesEditor.scene.json");
    assert!(
        ruta_rehecha.exists(),
        "falta la escena rehecha con el editor: {} (V4e: reconstruir la pantalla solo con el editor y guardarla ahí)",
        ruta_rehecha.display()
    );
    let ruta_dorado = ruta_fixture("dorado_botones_1280x720.json");
    let texto = std::fs::read_to_string(&ruta_dorado)
        .unwrap_or_else(|_| panic!("falta el dorado: corre primero dorado_desde_referencia"));
    let esperado: Vec<NodoDorado> = serde_json::from_str(&texto).unwrap();
    let obtenido = documento_dorado(&ruta_rehecha);
    compara_dorados(&esperado, &obtenido, "editor");
}
