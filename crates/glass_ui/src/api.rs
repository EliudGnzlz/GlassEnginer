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

use crate::documento::{Documento, NodoDocumento};
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

/// Un nodo con su rect y sus estilos ya resueltos.
#[derive(Debug, Clone, PartialEq)]
pub struct NodoColocado {
    pub nodo: usize,
    pub entidad: u64,
    pub rect: RectColocado,
    pub estilos: StyleProps,
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

/// Sesión de arrastre (espejo de `DragSession` de `canvas_ui`).
#[derive(Debug, Clone, PartialEq)]
pub struct SesionArrastre {
    pub entidad: u64,
    pub nombre: String,
    pub accion_inicio: String,
    pub accion_fin: String,
}

/// Destino de soltado: la entidad bajo el puntero al soltar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DestinoSoltar {
    pub entidad: u64,
}

/// Medida de texto: ancho máximo → tamaño (ancho, alto). P2 lo
/// implementa con el rasterizador provisional; V3 lo reimplementa
/// sin tocar el layout.
pub trait MedidaTexto {
    fn medir(&self, texto: &str, tamano: f32, ancho_max: f32) -> (f32, f32);
}

/// Layout completo del documento en el viewport (P2).
pub fn colocar_documento(_doc: &Documento, _viewport: Viewport) -> DocumentoColocado {
    todo!("V2 P2: layout")
}

/// Hit-test: la entidad del nodo superior con `raycast_target` bajo
/// el punto, o `None` (P4).
pub fn hit_test(_colocado: &DocumentoColocado, _x: f32, _y: f32) -> Option<u64> {
    todo!("V2 P4: hit-test")
}

/// Inicio de arrastre sobre un nodo con `UIDragData` (P4).
pub fn arrastre_pulsar(_colocado: &DocumentoColocado, _x: f32, _y: f32) -> Option<SesionArrastre> {
    todo!("V2 P4: arrastre")
}

/// Destino de soltado (P4).
pub fn arrastre_soltar(
    _colocado: &DocumentoColocado,
    _arrastrada: u64,
    _x: f32,
    _y: f32,
) -> Option<DestinoSoltar> {
    todo!("V2 P4: arrastre")
}

/// Parser del `style_extra` (P1): declaraciones + bloques de
/// pseudo-clase, sin selectores.
pub fn parsear_estilo_extra(_texto: &str) -> EstilosExtra {
    todo!("V2 P1: parser CSS")
}

/// Cascada completa de un nodo con un estado dado (P1 + P2).
pub fn resolver_estilos(_nodo: &NodoDocumento, _estado: &EstadoPseudo) -> StyleProps {
    todo!("V2 P1: cascada")
}
