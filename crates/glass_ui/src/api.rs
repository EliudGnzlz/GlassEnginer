//! Contrato público de V2 (P0): las firmas que el núcleo va a
//! exponer. Todo es stub (`todo!()`) hasta que P1–P4 lo implementen;
//! los tests portados de `canvas_ui` se escriben contra este contrato
//! y quedan en rojo hasta entonces (puerta G1).
//!
//! Reglas del contrato:
//! - `DocumentoColocado` va en el mismo orden DFS que
//!   `Documento::nodos`; `NodoColocado::nodo` es el índice en ambos.
//! - Los nodos inactivos salen con rect (0, 0, 0, 0) y quedan fuera
//!   de hit-test y de arrastre.
//! - Hit-test: bordes izquierdo y superior dentro, derecho e inferior
//!   fuera (la semántica actual de `canvas_ui`).
//! - `resolver_estilos` aplica la cascada defecto del tipo → campos
//!   del componente → `style_extra`, con la pseudo-clase del estado.

use crate::documento::{Documento, TipoGraphic};

pub use crate::interaccion::RastreadorClic;
use crate::estilos::StyleProps;

/// Tamaño del viewport en píxeles físicos (1280×720 en los tests).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub ancho: f32,
    pub alto: f32,
}

/// Rect colocado, en píxeles físicos, origen arriba a la izquierda.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RectColocado {
    pub x: f32,
    pub y: f32,
    pub ancho: f32,
    pub alto: f32,
}

impl RectColocado {
    pub fn contiene(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.ancho && y >= self.y && y < self.y + self.alto
    }
}


// Añadir DESPUÉS de la definición de DocumentoColocado (o junto a los otros tipos públicos):

/// Celda de avance de un carácter en coordenadas de documento.
#[derive(Debug, Clone, PartialEq)]
pub struct CeldaCaracter {
    /// Índice del carácter en `texto.chars()`, empezando en 0.
    pub indice: usize,
    /// El carácter.
    pub caracter: char,
    /// `[x, y, ancho_avance, alto_línea]` en coordenadas de documento (píxeles físicos).
    pub celda: [f32; 4],
}

/// Mapa carácter→posición para un nodo de texto colocado.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MapaTexto {
    /// Una celda por cada carácter visible colocado.
    pub celdas: Vec<CeldaCaracter>,
    /// Texto que se ve tras aplicar el modo de desbordamiento.
    pub texto_visible: String,
}

/// Un nodo con su rect y sus estilos ya resueltos.
#[derive(Debug, Clone, PartialEq)]
pub struct NodoColocado {
    pub nodo: usize,
    pub entidad: u64,
    pub rect: RectColocado,
    pub estilos: StyleProps,
    /// Mapa carácter→posición del texto (vacío si no es texto).
    pub mapa_texto: MapaTexto,
}

/// El documento colocado: la salida del layout (P2).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocumentoColocado {
    pub nodos: Vec<NodoColocado>,
}

/// Estado de pseudo-clase de un nodo (la aplicación en runtime es P4;
/// P1 resuelve con un estado sintético).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EstadoPseudo {
    pub hover: bool,
    pub active: bool,
    pub disabled: bool,
    pub focus: bool,
    pub selected: bool,
}

/// El `style_extra` ya parseado: base + una capa por pseudo-clase.
/// Declaraciones con error se ignoran y se anotan en `avisos`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EstilosExtra {
    pub base: StyleProps,
    pub hover: StyleProps,
    pub active: StyleProps,
    pub disabled: StyleProps,
    pub focus: StyleProps,
    pub selected: StyleProps,
    pub avisos: Vec<String>,
}

/// Sesión de arrastre activa (el `nombre` viene del documento).
#[derive(Clone, Debug, Default)]
pub struct SesionArrastre {
    pub entidad: u64,
    pub nombre: String,
    pub accion_inicio: String,
    pub accion_fin: String,
    /// Puntero - esquina del rect en el momento de pulsar.
    pub agarre: [f32; 2],
    /// `anchored_position` del rect_transform al pulsar.
    pub anclado_inicial: [f32; 2],
    /// Esquina (x, y) del rect al pulsar.
    pub origen_inicial: [f32; 2],
}

impl SesionArrastre {
    /// Posición anclada que corresponde al puntero (x, y) durante el arrastre.
    pub fn follow(&self, x: f32, y: f32) -> [f32; 2] {
        let origen_deseado = [x - self.agarre[0], y - self.agarre[1]];
        [
            self.anclado_inicial[0] + (origen_deseado[0] - self.origen_inicial[0]),
            self.anclado_inicial[1] + (origen_deseado[1] - self.origen_inicial[1]),
        ]
    }

    /// Devuelve el `anchored_position` capturado al pulsar.
    pub fn anclado_inicial(&self) -> [f32; 2] {
        self.anclado_inicial
    }
}

/// Destino válido para soltar un arrastre (el `nombre` viene del documento).
#[derive(Clone, Debug, Default)]
pub struct DestinoSoltar {
    pub entidad: u64,
    pub nombre: String,
    /// `anchored_position` del destino (o [0,0] si no tiene rect_transform).
    pub anclado: [f32; 2],
    /// Acción `on_drop` del destino.
    pub accion_soltar: String,
}

/// Medida de texto: ancho máximo → tamaño (ancho, alto). P2 lo
/// implementa con el rasterizador provisional; V3 lo reimplementa
/// sin tocar el layout.
pub trait MedidaTexto {
    fn medir(&self, texto: &str, tamano: f32, ancho_max: f32, letter_spacing: f32) -> (f32, f32);
}

/// Layout completo del documento en el viewport (P2).
pub fn colocar_documento(doc: &Documento, viewport: Viewport) -> DocumentoColocado {
    crate::layout::colocar_documento(doc, viewport)
}

/// Devuelve la entidad bajo (x, y) según orden de pintado. Necesita `doc`
/// porque `raycast_target`, `interactable` y `drag` no viajan en el colocado.
pub fn hit_test(
    doc: &Documento,
    colocado: &DocumentoColocado,
    x: f32,
    y: f32,
) -> Option<u64> {
    crate::interaccion::hit_test(doc, colocado, x, y)
}

/// Asciende desde la entidad indicada hasta el ancestro más cercano
/// con componente UI propio (botón, imagen o InputField): ese es el
/// CONTROL que se selecciona y se arrastra en el editor. Un hijo de
/// texto (solo `ui_text`) no es control: se sube a su padre. Si ningún
/// ancestro tiene componente de control (p. ej. un texto suelto bajo
/// el Canvas), se devuelve la entidad original. Si la entidad no está
/// en el documento, también se devuelve tal cual.
pub fn ascender_a_control(doc: &Documento, entidad: u64) -> u64 {
    let mut idx = doc.nodos.iter().position(|n| n.entidad == entidad);
    while let Some(i) = idx {
        let nodo = &doc.nodos[i];
        if nodo.boton.is_some()
            || nodo.imagen.is_some()
            || nodo.graphic == TipoGraphic::InputField
        {
            return nodo.entidad;
        }
        idx = nodo.padre;
    }
    entidad
}

/// Inicia un arrastre bajo (x, y).
pub fn arrastre_pulsar(
    doc: &Documento,
    colocado: &DocumentoColocado,
    x: f32,
    y: f32,
) -> Option<SesionArrastre> {
    crate::interaccion::arrastre_pulsar(doc, colocado, x, y)
}

/// Rects colocados por entidad: el mismo mapa que devolvía el viejo
/// `glass_core::resolve_scene_ui_rects` (`[x, y, ancho, alto]`), pero
/// leído del documento colocado V2. Puente para MCP y editor.
pub fn rects_colocados(
    doc: &Documento,
    colocado: &DocumentoColocado,
) -> std::collections::HashMap<u64, [f32; 4]> {
    doc.nodos
        .iter()
        .zip(colocado.nodos.iter())
        .map(|(n, nc)| (n.entidad, [nc.rect.x, nc.rect.y, nc.rect.ancho, nc.rect.alto]))
        .collect()
}

/// Busca destino de soltar para la entidad `arrastrada`.
pub fn arrastre_soltar(
    doc: &Documento,
    colocado: &DocumentoColocado,
    arrastrada: u64,
    x: f32,
    y: f32,
) -> Option<DestinoSoltar> {
    crate::interaccion::arrastre_soltar(doc, colocado, arrastrada, x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derivacion::derivar_documentos;
    use glass_core::scene::{
        CanvasData, EntityData, RectTransformData, SceneState, UIButtonData, UIInputFieldData,
        UIImageData, UITextData,
    };

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

    fn documento(ents: Vec<EntityData>) -> Documento {
        let docs = derivar_documentos(&escena(ents));
        assert_eq!(docs.len(), 1, "se esperaba un solo documento");
        docs.into_iter().next().unwrap()
    }

    fn boton(id: u64) -> EntityData {
        let mut e = entidad(id, "Botón");
        e.rect_transform = Some(rect());
        e.ui_button = Some(UIButtonData::default());
        e
    }

    fn texto(id: u64) -> EntityData {
        let mut e = entidad(id, "Texto");
        e.rect_transform = Some(rect());
        e.ui_text = Some(UITextData::default());
        e
    }

    fn panel(id: u64) -> EntityData {
        let mut e = entidad(id, "Panel");
        e.rect_transform = Some(rect());
        e.ui_image = Some(UIImageData::default());
        e
    }

    #[test]
    fn hijo_de_texto_asciende_al_boton() {
        let mut ents = vec![canvas_raiz(10), boton(12), texto(14)];
        enlazar(&mut ents, 10, 12);
        enlazar(&mut ents, 12, 14);
        let doc = documento(ents);
        assert_eq!(ascender_a_control(&doc, 14), 12);
        assert_eq!(ascender_a_control(&doc, 12), 12);
    }

    #[test]
    fn texto_suelto_se_queda() {
        let mut ents = vec![canvas_raiz(10), texto(20)];
        enlazar(&mut ents, 10, 20);
        let doc = documento(ents);
        assert_eq!(ascender_a_control(&doc, 20), 20);
    }

    #[test]
    fn hijo_de_texto_asciende_al_panel() {
        let mut ents = vec![canvas_raiz(10), panel(30), texto(31)];
        enlazar(&mut ents, 10, 30);
        enlazar(&mut ents, 30, 31);
        let doc = documento(ents);
        assert_eq!(ascender_a_control(&doc, 31), 30);
        assert_eq!(ascender_a_control(&doc, 30), 30);
    }

    #[test]
    fn input_field_es_control() {
        let mut e = entidad(40, "Campo");
        e.rect_transform = Some(rect());
        e.ui_input_field = Some(UIInputFieldData::default());
        let mut ents = vec![canvas_raiz(10), e];
        enlazar(&mut ents, 10, 40);
        let doc = documento(ents);
        assert_eq!(ascender_a_control(&doc, 40), 40);
    }

    #[test]
    fn entidad_ausente_devuelve_la_misma() {
        let doc = documento(vec![canvas_raiz(10)]);
        assert_eq!(ascender_a_control(&doc, 999), 999);
    }
}
