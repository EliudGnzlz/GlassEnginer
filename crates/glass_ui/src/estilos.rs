//! Estilos tipados del «núcleo bueno» de CSS v1. El texto CSS del
//! campo `style_extra` es solo una entrada: se parsea a este
//! `StyleProps` (el parser llega en V2) y el documento trabaja
//! siempre con la forma tipada. `None` en un campo = no fijado por
//! esta capa de la cascada.

/// Una dimensión de layout: píxeles de referencia (previos a la
/// escala del Canvas), porcentaje del padre o automática.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dimension {
    Px(f32),
    Porcentaje(f32),
    Auto,
}

/// Modo de colocación de los hijos de un nodo. El defecto del motor
/// es `Absoluto` (por anclas del RectTransform, como Unity sin
/// LayoutGroup); Flex/Grid solo con LayoutGroup o `display`
/// explícito en el estilo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Absoluto,
    Flex,
    Grid,
    Ninguno,
}

/// Sombra de caja sencilla (se interpreta en el pintor, V2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sombra {
    pub dx: f32,
    pub dy: f32,
    pub desenfoque: f32,
    pub color: [f32; 4],
}

/// El conjunto cerrado de propiedades del núcleo bueno. La cascada
/// (defecto del tipo → campos del componente → style_extra) se
/// resuelve campo a campo en V2; en V1 el tipo existe y la
/// derivación lo rellena desde los componentes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StyleProps {
    pub display: Option<Display>,
    /// "row" | "column" (flex).
    pub flex_direction: Option<String>,
    /// Reparto de uGUI cuando el flex viene de un LayoutGroup se
    /// calcula en el layout (V2/V5); aquí solo van los valores.
    pub gap: Option<f32>,
    /// [izquierda, arriba, derecha, abajo].
    pub padding: Option<[f32; 4]>,
    /// Alineación del bloque: "start" | "center" | "end" y las de 9
    /// posiciones de Unity se traducen en V2.
    pub justify_content: Option<String>,
    pub align_items: Option<String>,
    pub width: Option<Dimension>,
    pub height: Option<Dimension>,
    pub min_width: Option<Dimension>,
    pub min_height: Option<Dimension>,
    pub max_width: Option<Dimension>,
    pub max_height: Option<Dimension>,
    /// Relación ancho/alto forzada (aspect-ratio).
    pub aspect_ratio: Option<f32>,
    pub background_color: Option<[f32; 4]>,
    /// Degradado lineal: (ángulo en grados, paradas de color). Las
    /// paradas múltiples se pintan por subdivisión del quad (V2).
    pub background_gradient: Option<(f32, Vec<[f32; 4]>)>,
    /// Radios de esquina [sup-izq, sup-der, inf-der, inf-izq].
    pub border_radius: Option<[f32; 4]>,
    pub box_shadow: Option<Sombra>,
    /// Opacidad del elemento (multiplica el alfa, como uGUI).
    pub opacity: Option<f32>,
    /// "visible" | "hidden" (recorte al rect del nodo).
    pub overflow: Option<String>,
    /// Color del texto (en nodos con Graphic de texto).
    pub color: Option<[f32; 4]>,
    pub font_size: Option<f32>,
}

