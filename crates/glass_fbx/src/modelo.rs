//! Modelo intermedio (IR) entre el lector FBX (`lector`) y el escritor
//! GLB (`escritor`). Es el contrato interno de la crate: el lector vuelca
//! la escena de ufbx aquí y el escritor emite exactamente lo que el
//! cargador del motor (`glass_renderer::mesh`) consume:
//!
//! - Árbol de nodos real (nombre, TRS local, jerarquía): el cargador
//!   aplana las globales en los modelos sin piel y usa el árbol tal cual
//!   para piel y animaciones.
//! - Geometría por primitiva con su material; en modelos con piel, los
//!   vértices quedan en espacio de geometría (sin hornear la global del
//!   nodo) y las inversas de unión hacen el resto.
//! - Una sola piel por modelo (límite declarado del motor,
//!   `skin::MAX_JOINTS` articulaciones como máximo).

/// Escena completa lista para escribir como GLB.
#[derive(Debug, Default)]
pub struct EscenaIr {
    /// Nombre del modelo (stem de la fuente).
    pub nombre: String,
    /// Nodos en orden de emisión; el índice en este vector es el índice
    /// de nodo del GLB.
    pub nodos: Vec<NodoIr>,
    pub mallas: Vec<MallaIr>,
    pub materiales: Vec<MaterialIr>,
    /// Texturas ya resueltas a bytes (embebidas en el FBX o leídas de
    /// disco); se incrustan en el GLB.
    pub texturas: Vec<TexturaIr>,
    /// La piel única del modelo, si alguna malla tiene pesos.
    pub piel: Option<PielIr>,
    pub animaciones: Vec<AnimIr>,
    /// Diferencias conocidas y decisiones tomadas al leer (textura no
    /// encontrada, malla vacía omitida…): se devuelven al llamante y se
    /// vuelcan en la consola del editor, no se esconden.
    pub avisos: Vec<String>,
}

#[derive(Debug)]
pub struct NodoIr {
    pub nombre: String,
    pub padre: Option<usize>,
    pub hijos: Vec<usize>,
    pub traslacion: [f32; 3],
    /// Cuaternión en orden xyzw.
    pub rotacion: [f32; 4],
    pub escala: [f32; 3],
    /// Índice en `EscenaIr::mallas`, si el nodo lleva malla.
    pub malla: Option<usize>,
    /// El nodo es articulación de la piel (su índice está en
    /// `PielIr::articulaciones`).
    pub es_articulacion: bool,
}

#[derive(Debug)]
pub struct MallaIr {
    pub nombre: String,
    /// Nunca vacío: el lector omite las mallas sin primitivas.
    pub primitivas: Vec<PrimitivaIr>,
    /// Si es `true`, cada primitiva trae `joints`/`weights` alineados
    /// con `posiciones` y el escritor le pone la piel al nodo.
    pub con_piel: bool,
}

#[derive(Debug)]
pub struct PrimitivaIr {
    pub posiciones: Vec<[f32; 3]>,
    pub normales: Vec<[f32; 3]>,
    /// Puede venir vacío (el escritor omite TEXCOORD_0 y el cargador
    /// pone [0, 0]); si viene, está alineado con `posiciones`.
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Índice en `EscenaIr::materiales`.
    pub material: Option<usize>,
    /// Índices DENTRO de `PielIr::articulaciones` (no de nodo),
    /// alineados con `posiciones`; vacío si la malla no tiene piel.
    pub joints: Vec<[u16; 4]>,
    /// Pesos alineados con `joints` (suman 1 por vértice).
    pub weights: Vec<[f32; 4]>,
}

#[derive(Debug)]
pub struct MaterialIr {
    pub nombre: String,
    /// Factor RGBA de color base (ya compuesto: color × factor).
    pub color_base: [f32; 4],
    /// Índice en `EscenaIr::texturas`.
    pub textura_base: Option<usize>,
    pub metalico: f32,
    pub rugosidad: f32,
    /// Color de emisión ya multiplicado por su factor.
    pub emision: [f32; 3],
    pub doble_cara: bool,
}

#[derive(Debug)]
pub struct TexturaIr {
    pub nombre: String,
    /// PNG o JPEG tal cual, para incrustar en el GLB.
    pub bytes: Vec<u8>,
    /// "image/png" o "image/jpeg".
    pub mime: &'static str,
}

#[derive(Debug)]
pub struct PielIr {
    /// Índices de nodo (en `EscenaIr::nodos`), en el orden de la piel.
    pub articulaciones: Vec<usize>,
    /// Matriz inversa de unión por articulación, column-major
    /// ([f32; 16]), alineada con `articulaciones`.
    pub inversas: Vec<[f32; 16]>,
}

#[derive(Debug)]
pub struct AnimIr {
    pub nombre: String,
    pub canales: Vec<CanalIr>,
}

#[derive(Debug)]
pub struct CanalIr {
    /// Índice en `EscenaIr::nodos`.
    pub nodo: usize,
    /// Tiempos en segundos desde 0, compartidos por los tres
    /// componentes presentes.
    pub tiempos: Vec<f32>,
    pub traslaciones: Option<Vec<[f32; 3]>>,
    /// Cuaterniones xyzw.
    pub rotaciones: Option<Vec<[f32; 4]>>,
    pub escalas: Option<Vec<[f32; 3]>>,
}
