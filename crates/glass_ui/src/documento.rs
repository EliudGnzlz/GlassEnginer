//! Tipos del documento derivado: un nodo por entidad con
//! RectTransform bajo un Canvas. El documento es una vista calculada
//! de la escena, no un segundo formato guardado.

use glass_core::scene::{
    CanvasData, ContentSizeFitterData, GridLayoutGroupData, LayoutElementData, LayoutGroupData,
    RectMask2DData, RectTransformData, UIButtonData, UIDragData, UIImageData, UITextData,
};

/// El único Graphic de un nodo (regla de Unity: un Graphic por
/// GameObject). Si la entidad trae imagen y texto a la vez, manda la
/// imagen y la derivación lo anota como aviso (la migración separa el
/// texto en un hijo sintético antes). El InputField es la excepción
/// (V4, Q1): un solo nodo con fondo (imagen) + texto sintetizado del
/// campo, porque el campo ES el texto que se ve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipoGraphic {
    Ninguno,
    Imagen,
    Texto,
    InputField,
}

/// Un nodo del documento. Los índices (`padre`, `hijos`) son
/// posiciones dentro de `Documento::nodos`.
#[derive(Debug, Clone)]
pub struct NodoDocumento {
    /// Id de la entidad de escena de la que deriva.
    pub entidad: u64,
    /// Nombre de la entidad: viaja con el documento para que las
    /// sesiones de arrastre y los destinos de suelta lo lleven consigo.
    pub nombre: String,
    pub padre: Option<usize>,
    pub hijos: Vec<usize>,
    pub graphic: TipoGraphic,
    /// La entidad trae CanvasData (raíz o anidado).
    pub es_canvas: bool,
    /// Canvas sin ningún Canvas ancestro: abre documento propio.
    pub es_canvas_raiz: bool,
    /// Entidad activa y todos sus ancestros UI activos. Los nodos
    /// inactivos se quedan en el árbol pero fuera de layout, pintado
    /// y hit-test.
    pub activo: bool,
    pub rect_transform: RectTransformData,
    pub canvas: Option<CanvasData>,
    pub imagen: Option<UIImageData>,
    pub texto: Option<UITextData>,
    pub boton: Option<UIButtonData>,
    pub drag: Option<UIDragData>,
    pub layout_group: Option<LayoutGroupData>,
    /// Tamaños de layout del hijo (V5b-1, G7-mínimo).
    pub layout_element: Option<LayoutElementData>,
    /// Ajuste del grupo a su contenido (V5b-2, G6).
    pub content_size_fitter: Option<ContentSizeFitterData>,
    /// Rejilla de celdas de tamaño fijo (V5b-3, G5).
    pub grid_layout_group: Option<GridLayoutGroupData>,
    /// Máscara rectangular (V5c, M1).
    pub rect_mask2d: Option<RectMask2DData>,
    /// El `style_extra` vigente del nodo: el del Graphic si lo hay;
    /// si no, el del botón; si no, vacío.
    pub style_extra: String,
    /// Hit-test: el del Graphic; sin Graphic, false salvo que el
    /// nodo tenga botón (el botón recibe puntero por su Graphic
    /// objetivo, que puede ser él mismo sin Graphic: en ese caso
    /// manda el defecto true del botón implícito de Unity... en
    /// nuestro modelo, un botón sin Graphic usa raycast_target=true
    /// solo si su entidad es el propio target; ver derivación).
    pub raycast_target: bool,
}

/// Un documento por cada Canvas raíz de la escena.
#[derive(Debug, Clone)]
pub struct Documento {
    /// Índice del nodo del Canvas raíz en `nodos`.
    pub canvas_raiz: usize,
    /// Nodos en orden de jerarquía (DFS desde la raíz, hijos en el
    /// orden de la escena).
    pub nodos: Vec<NodoDocumento>,
    /// Avisos de la derivación (gráficos duplicados, referencias
    /// rotas, etc.), ya agregados y legibles.
    pub avisos: Vec<String>,
}
