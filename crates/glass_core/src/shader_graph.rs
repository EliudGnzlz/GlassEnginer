//! Grafo de shaders propio del motor (estilo Shader Graph, sin copiarlo):
//! un modelo de datos serializable de nodos y conexiones que compila a un
//! fragment shader GLSL. La librería de prefabricados (`presets`) son grafos
//! normales guardados como JSON: se aprende y se personaliza editándolos,
//! igual que un material duplicado en Unity.
//!
//! Alcance: el modelo, el compilador a GLSL con detección de ciclos y
//! coerción de tipos, los prefabricados y el uso como material en runtime.
//! El shader generado comparte el bloque de push constants del pipeline de
//! mallas del motor (208 bytes, el mismo del PBR): la cámara sale de
//! `camera_pos` y el TIEMPO viaja en `light_color.w`, el canal que el PBR
//! tiene reservado y no lee — así un material de grafo no necesita ni un
//! pipeline layout distinto ni buffers uniformes propios. En el editor, una
//! entidad usa un grafo poniendo su `MeshData.shader` a `graph:<nombre>` y
//! el motor carga `assets/shaders/graph_<nombre>.frag.spv` al arrancar.
//!
//! Etapa de vértices (opcional): si el grafo declara un nodo
//! `VertexOutput` con su entrada "offset" conectada, `compile_vertex`
//! genera además un vertex shader (`graph_<nombre>.vert.spv`) que suma ese
//! offset —calculado por una sub-red con `Position` y `Time`— a la
//! posición del vértice en espacio de objeto. Sin `VertexOutput` no hay
//! etapa de vértices y el runtime usa el vértice estándar: los grafos
//! anteriores no cambian de comportamiento. Límite v1: las normales no se
//! recalculan tras el desplazamiento (viajan las originales).
//!
//! Canal global (bloques «punto de interacción» y «huella con forma»):
//! ambas plantillas declaran siempre un UBO en el set 1, `GraphGlobals`,
//! que SOLO existe en los pipelines de grafo (el PBR y el de piel no lo
//! tienen en su layout). Son DOS vec4 globales para todo el motor:
//! `interaction_point` —posición de mundo del intruso que toca la
//! superficie y su radio en el `w`— e `interaction_shape` —la FORMA de
//! su huella en planta: semiejes X y Z de la caja, su rotación Y en
//! radianes y un discriminador en el `w` (0 = esfera, 1 = caja)—. Un
//! script C# los alimenta cada frame vía `Shader.SetInteractionPoint`
//! (huella esférica) o `Shader.SetInteractionBox` (huella de caja: la
//! del collider en planta del objeto, p. ej. el casco de una barca). Es
//! el patrón del tutorial de Game Slave —un global con el mismo
//! significado en el shader y en el código— extendido para que la
//! huella tenga la forma del objeto y no siempre un círculo. El nodo
//! `InteractionPoint` los lee en las dos etapas. El valor inactivo
//! ([`INTERACTION_GLOBALS_INACTIVE`]) deja el punto tan lejos que
//! cualquier máscara de distancia da cero: un grafo que usa el nodo no
//! necesita que nadie lo alimente para verse bien.
//!
//! Grafos transparentes (bloque «estela suave»): un grafo puede
//! declararse `transparent` en su JSON. El alfa del fragmento ya salía
//! del Output (la plantilla escribe el vec4 completo), pero TODOS los
//! pipelines del motor nacían opacos, así que el alfa se ignoraba. Al
//! regenerar el .spv de un grafo transparente, junto a él se deja el
//! marcador `graph_<nombre>.transparent` —el mismo patrón que el
//! `.vert.spv` hermano— y el cargador de materiales crea su pipeline
//! con mezcla alfa y SIN escritura de profundidad (el orden de dibujo
//! de la escena pone el agua justo antes que la espuma, así que los
//! transparentes se componen sobre ella como calcomanías). Sin el
//! marcador, un grafo es opaco bit a bit como siempre. El nodo
//! [`NodeKind::RadialUv`] es la pieza que faltaba para construir el
//! alfa: el set de nodos no tiene Dot/Length ni resta, así que —como
//! las distancias de `InteractionPoint`— la distancia radial a la UV
//! va horneada en el nodo.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Valor del punto de interacción cuando nadie lo está alimentando: un
/// punto muy por debajo del mundo con radio cero. Cualquier distancia
/// (3D o XZ) desde una superficie real hasta él es enorme, así que las
/// máscaras construidas con `InteractionPoint` valen cero y el grafo se
/// ve exactamente como si el nodo no existiera. Es el valor con el que
/// arranca el motor y al que vuelve el canal al limpiar o al cambiar de
/// escena (el publicador desaparece con su escena).
pub const INTERACTION_POINT_INACTIVE: [f32; 4] = [0.0, -10000.0, 0.0, 0.0];

/// El canal global completo —los dos vec4 del UBO `GraphGlobals`,
/// seguidos, tal como viven en el buffer y en el estado CPU— en su valor
/// inactivo: el punto inactivo de siempre y la forma a cero, que el nodo
/// lee como la forma esférica (con el radio del punto: cero). Es el
/// estado con el que arranca el Renderer y al que vuelve el canal al
/// limpiar o al cambiar de escena.
/// GLSL del canal global que ambas plantillas (vértices y fragmento)
/// emiten tras sus declaraciones: el UBO `GraphGlobals` y las dos
/// funciones de máscara que consume el nodo `InteractionPoint`. Las
/// distancias viven aquí —y no construidas a piezas por el grafo—
/// porque el set de nodos no tiene Dot/Length ni resta de vectores: una
/// longitud (y menos aún la SDF de una caja rotada) no se puede montar
/// con los nodos actuales.
const NOISE_GLSL: &str = r#"// Hash de Morgan McGuire: el seno es solo el mezclador entero/fract del
// hash, no una onda periódica del patrón.
float graph_hash21(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}
float graph_value_noise(vec2 uv, float scale) {
    vec2 p = uv * scale;
    vec2 i = floor(p);
    vec2 f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    float a = graph_hash21(i);
    float b = graph_hash21(i + vec2(1.0, 0.0));
    float c = graph_hash21(i + vec2(0.0, 1.0));
    float d = graph_hash21(i + vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
vec2 graph_noise_gradient(vec2 p) {
    float a = graph_hash21(p) * 6.28318530718;
    return vec2(cos(a), sin(a));
}
float graph_gradient_noise(vec2 uv, float scale) {
    vec2 p = uv * scale;
    vec2 i = floor(p);
    vec2 f = fract(p);
    vec2 u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    float n00 = dot(graph_noise_gradient(i), f);
    float n10 = dot(graph_noise_gradient(i + vec2(1.0, 0.0)), f - vec2(1.0, 0.0));
    float n01 = dot(graph_noise_gradient(i + vec2(0.0, 1.0)), f - vec2(0.0, 1.0));
    float n11 = dot(graph_noise_gradient(i + vec2(1.0, 1.0)), f - vec2(1.0, 1.0));
    float n = mix(mix(n00, n10, u.x), mix(n01, n11, u.x), u.y);
    return clamp(n * 0.5 + 0.5, 0.0, 1.0);
}
float graph_fbm_noise(vec2 uv, float scale, int octaves, float lacunarity, float gain) {
    vec2 p = uv * scale;
    float sum = 0.0;
    float amp = 1.0;
    float total = 0.0;
    for (int i = 0; i < 6; ++i) {
        if (i >= octaves) break;
        sum += graph_value_noise(p, 1.0) * amp;
        total += amp;
        p *= lacunarity;
        amp *= gain;
    }
    return total > 0.0 ? sum / total : 0.0;
}
"#;

const GRAPH_GLOBALS_GLSL: &str = "\
layout(set = 1, binding = 0) uniform GraphGlobals {\n\
\x20   vec4 interaction_point;\n\
\x20   vec4 interaction_shape;\n\
} u_globals;\n\
// Distancia con signo a la huella de caja en planta (XZ): el offset de\n\
// mundo se lleva al marco local de la caja deshaciendo su rotación Y y\n\
// se aplica la SDF clásica de caja 2D (negativa dentro, cero en el\n\
// borde). Los semiejes van en el marco local: shape.x a lo largo del\n\
// eje X del objeto y shape.y a lo largo de su eje Z.\n\
float graph_sdf_box_xz(vec2 d) {\n\
\x20   float c = cos(u_globals.interaction_shape.z);\n\
\x20   float s = sin(u_globals.interaction_shape.z);\n\
\x20   vec2 local = vec2(d.x * c - d.y * s, d.x * s + d.y * c);\n\
\x20   vec2 q = abs(local) - u_globals.interaction_shape.xy;\n\
\x20   return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0);\n\
}\n\
// Máscara cenital (solo XZ) contra la forma activa: el disco del radio\n\
// si la forma es esférica; la huella de caja orientada si es caja.\n\
float graph_interaction_distance_xz(vec3 world_pos) {\n\
\x20   vec2 d = world_pos.xz - u_globals.interaction_point.xz;\n\
\x20   if (u_globals.interaction_shape.w > 0.5) {\n\
\x20       return graph_sdf_box_xz(d);\n\
\x20   }\n\
\x20   return length(d) - u_globals.interaction_point.w;\n\
}\n\
// Máscara 3D contra la forma activa: la esfera de verdad si la forma\n\
// es esférica; si es caja, la huella es un prisma (la caja del canal\n\
// no tiene altura), así que la distancia es la cenital.\n\
float graph_interaction_distance(vec3 world_pos) {\n\
\x20   if (u_globals.interaction_shape.w > 0.5) {\n\
\x20       return graph_interaction_distance_xz(world_pos);\n\
\x20   }\n\
\x20   return distance(world_pos, u_globals.interaction_point.xyz) - u_globals.interaction_point.w;\n\
}\n";

pub const INTERACTION_GLOBALS_INACTIVE: [f32; 8] = [
    INTERACTION_POINT_INACTIVE[0],
    INTERACTION_POINT_INACTIVE[1],
    INTERACTION_POINT_INACTIVE[2],
    INTERACTION_POINT_INACTIVE[3],
    0.0,
    0.0,
    0.0,
    0.0,
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum NodeKind {
    /// Constante de color RGBA.
    Color { rgba: [f32; 4] },
    /// Constante numérica.
    Float { value: f32 },
    /// Tiempo en segundos (uniforme del motor).
    Time,
    /// Coordenadas UV del fragmento.
    Uv,
    /// Normal del fragmento en espacio de mundo.
    Normal,
    /// Suma componente a componente (a + b).
    Add,
    /// Producto componente a componente (a * b).
    Multiply,
    /// Interpolación: mix(a, b, t).
    Mix,
    /// sin(x).
    Sine,
    /// abs(x).
    Abs,
    /// 1 - x.
    OneMinus,
    /// smoothstep(edge0, edge1, x) con los bordes como propiedades.
    Smoothstep { edge0: f32, edge1: f32 },
    /// Máscara radial desde el centro de la UV del quad: emite un
    /// vec4 `(1, 1, 1, alfa)` con
    /// `alfa = 1 - smoothstep(edge0, edge1, length(uv - 0,5) * 2)` —
    /// 1 en el centro, 0 al llegar al círculo inscrito (distancia 1)
    /// y 0 en las esquinas (distancia √2). Su sitio natural es
    /// multiplicar el color camino del `Output` de un grafo
    /// transparente: el RGB queda intacto (multiplica por 1) y el alfa
    /// pasa a ser la máscara, así el quad se lee como un disco suave
    /// y las esquinas desaparecen. Va horneada en el nodo porque el
    /// set de nodos no tiene Dot/Length ni resta (el mismo motivo que
    /// las distancias de `InteractionPoint`). Vale en las dos etapas:
    /// en vértices lee `in_uv`; simetría con `Uv`.
    RadialUv { edge0: f32, edge1: f32 },
    /// Término de Fresnel con la normal y la dirección de vista:
    /// pow(1 - max(dot(N, V), 0), power).
    Fresnel { power: f32 },
    /// Nodo final: su entrada "color" es el color del fragmento.
    Output,
    /// Posición del vértice en espacio de objeto. SOLO etapa de vértices:
    /// en la de fragmento es un error de compilación (`VertexOnlyNode`) —
    /// la posición en fragmento, si algún día existe, será un nodo
    /// distinto, no un alias silencioso de `v_world_pos`. El campo `axis`
    /// da acceso por componentes (la coerción vec→float del compilador
    /// toma `.x`, así que sin ejes no se podría construir un campo
    /// escalar sobre Z, p. ej. olas cruzadas sin(x) + sin(z)).
    Position {
        #[serde(default)]
        axis: PositionAxis,
    },
    /// Nodo final de la sub-red de vértices: su entrada "offset" (vec3)
    /// se suma a la posición del vértice en espacio de objeto, antes de
    /// las matrices de modelo y MVP. Declararlo es opcional: un grafo sin
    /// `VertexOutput` no tiene etapa de vértices.
    VertexOutput,
    /// El punto de interacción global del motor (ver la doc del módulo):
    /// los dos vec4 del canal (posición de mundo del intruso + radio, y
    /// la forma de su huella) que un script alimenta cada frame. Vale en
    /// AMBAS etapas —es un uniforme, no un atributo— y el campo
    /// `component` elige qué se lee de él, con el mismo patrón que
    /// `Position { axis }`. Las variantes `Distance`/`DistanceXZ` NO son
    /// la distancia euclídea pura: son la distancia **con signo a la
    /// superficie de la forma activa** —la esfera del intruso (o su
    /// cilindro cenital) si la forma es esférica, su huella de caja
    /// orientada en planta si es caja—, negativas dentro, cero en el
    /// borde, positivas fuera. Son las dos máscaras del tutorial de
    /// referencia, generalizadas a la forma real del objeto. Se hornean
    /// en el nodo (vía las funciones de `GRAPH_GLOBALS_GLSL`) porque el
    /// set de nodos no tiene Dot/Length ni resta, y sin ellas la
    /// distancia no se puede construir a piezas.
    InteractionPoint {
        #[serde(default)]
        component: InteractionComponent,
    },
    /// Ruido de valor clásico, determinista y sin texturas. `scale` es
    /// propiedad (como los bordes de Smoothstep/RadialUv): el nodo solo
    /// expone la entrada `uv`. Salida float en [0,1].
    ValueNoise { scale: f32 },
    /// Perlin 2D mejorado, determinista y sin texturas. `scale` es
    /// propiedad. Salida float re-mapeada a [0,1].
    GradientNoise { scale: f32 },
    /// FBM de ruido de VALOR (reutiliza la función de `ValueNoise`):
    /// suma de octavas normalizada por la suma de amplitudes. `octaves`
    /// se acota a 1..=6; `scale`, `lacunarity` y `gain` son propiedades.
    /// Salida float en [0,1].
    FbmNoise {
        scale: f32,
        octaves: u32,
        lacunarity: f32,
        gain: f32,
    },
    /// Resta componente a componente (a - b).
    Subtract,
    /// División componente a componente (a / b).
    Divide,
    /// Canal rojo de la entrada: `x.r` con relleno seguro para entradas
    /// de menor dimensión (mismo espíritu que las coerciones del archivo).
    ChannelR,
    /// Canal verde de la entrada: `x.g` con relleno seguro.
    ChannelG,
    /// Canal azul de la entrada: `x.b`; un vec2 devuelve 0.0.
    ChannelB,
    /// Canal alfa de la entrada: `x.a`; un vec3 devuelve 1.0.
    ChannelA,
    /// Construye un vec4 desde cuatro flotantes; alfa 1.0 si no se conecta.
    Combine,
    /// Producto escalar: dot(a, b).
    Dot,
    /// Longitud de un vector: length(x).
    Length,
    /// Distancia entre dos vectores: distance(a, b).
    Distance,
    /// Normaliza la entrada y conserva su tipo.
    Normalize,
    /// clamp(x, min, max) con los límites como propiedades.
    Clamp { min: f32, max: f32 },
    /// pow(x, exponent) con el exponente como propiedad.
    Power { exponent: f32 },
    /// step(edge, x): 0 si x < edge, 1 en caso contrario.
    Step { edge: f32 },
    /// mod(x, y) componente a componente.
    Modulo,
}

/// Qué lee un nodo `InteractionPoint` de los vec4 globales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum InteractionComponent {
    /// El vec4 crudo (posición de mundo xyz, radio en w).
    #[default]
    All,
    /// Componentes sueltas del vec4.
    X,
    Y,
    Z,
    /// El radio (componente w).
    Radius,
    /// Distancia con signo a la forma activa del intruso: a su esfera
    /// (`distance(posición_de_mundo, punto.xyz) - radio`) si la forma es
    /// esférica; a su huella de caja en planta —un prisma sin altura—
    /// si es caja.
    Distance,
    /// Distancia con signo a la huella en planta (solo XZ): al disco del
    /// radio si la forma es esférica; a la caja orientada (SDF de caja
    /// 2D rotada por su rotación Y) si es caja.
    DistanceXZ,
}

/// Componente que expone un nodo `Position`: un eje suelto (float) o la
/// posición completa (vec3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PositionAxis {
    X,
    Y,
    Z,
    #[default]
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: u32,
    pub kind: NodeKind,
    /// Posición en el editor visual (cuando exista); el compilador la ignora.
    #[serde(default)]
    pub position: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from_node: u32,
    pub to_node: u32,
    /// Nombre de la entrada destino: "a", "b", "t", "x" o "color".
    pub to_input: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderGraph {
    pub name: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// Si el material de este grafo se dibuja con mezcla alfa (y sin
    /// escritura de profundidad) en vez de opaco. Declaración de
    /// intenciones del autor: el runtime no lee el JSON, lee el
    /// marcador `graph_<nombre>.transparent` que acompaña al .spv
    /// regenerado (ver la doc del módulo). Sin el campo, opaco.
    #[serde(default)]
    pub transparent: bool,
}

#[derive(Debug, Error, PartialEq)]
pub enum GraphError {
    #[error("el grafo no tiene nodo Output")]
    MissingOutput,
    #[error("ciclo detectado en el grafo al llegar al nodo {0}")]
    Cycle(u32),
    #[error("la arista referencia un nodo inexistente: {0}")]
    UnknownNode(u32),
    #[error("el nodo {0} no tiene una entrada llamada '{1}'")]
    UnknownInput(u32, String),
    #[error("hay dos nodos con el mismo id: {0}")]
    DuplicateId(u32),
    #[error("el nodo {0} (Position) solo existe en la sub-red de vértices: es la posición en espacio de objeto y no tiene equivalente en la etapa de fragmento")]
    VertexOnlyNode(u32),
    #[error("el nodo {0} no está soportado en la etapa de vértices (v1)")]
    StageUnsupported(u32),
}

/// Tipo GLSL de la salida de un nodo, para coercionar al conectar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum GlslType {
    Float,
    Vec2,
    Vec3,
    Vec4,
}

impl GlslType {
    fn name(self) -> &'static str {
        match self {
            GlslType::Float => "float",
            GlslType::Vec2 => "vec2",
            GlslType::Vec3 => "vec3",
            GlslType::Vec4 => "vec4",
        }
    }
}

/// Expresión ya emitida para un nodo: su código y su tipo.
struct Emitted {
    expr: String,
    ty: GlslType,
}

/// Convierte `expr` del tipo `from` al tipo `to` con las reglas usuales:
/// un escalar se replica; subir de dimensión rellena (vec3→vec4 con alfa 1).
fn coerce(expr: &str, from: GlslType, to: GlslType) -> String {
    if from == to {
        return expr.to_string();
    }
    match (from, to) {
        (GlslType::Float, GlslType::Vec2) => format!("vec2({})", expr),
        (GlslType::Float, GlslType::Vec3) => format!("vec3({})", expr),
        (GlslType::Float, GlslType::Vec4) => format!("vec4(vec3({}), 1.0)", expr),
        (GlslType::Vec2, GlslType::Vec3) => format!("vec3({}, 0.0)", expr),
        (GlslType::Vec2, GlslType::Vec4) => format!("vec4({}, 0.0, 1.0)", expr),
        (GlslType::Vec3, GlslType::Vec4) => format!("vec4({}, 1.0)", expr),
        // Bajar de dimensión: se toma el primer componente (o rgb).
        (_, GlslType::Float) => format!("({}).x", expr),
        (GlslType::Vec3, GlslType::Vec2) => format!("({}).xy", expr),
        (GlslType::Vec4, GlslType::Vec2) => format!("({}).xy", expr),
        (GlslType::Vec4, GlslType::Vec3) => format!("({}).rgb", expr),
        // Tipos iguales: ya resuelto por el retorno temprano de arriba.
        _ => expr.to_string(),
    }
}

/// Extrae el canal `ch` (0=r, 1=g, 2=b, 3=a) de `expr` con las
/// coerciones seguras del archivo: un float se replica en rgb y su alfa
/// vale 1; un vec2 rellena b=0 y alfa=1; un vec3 rellena alfa=1.
fn channel_expr(expr: &str, ty: GlslType, ch: usize) -> String {
    match (ty, ch) {
        (GlslType::Float, 0 | 1 | 2) => expr.to_string(),
        (GlslType::Float, _) => "1.0".to_string(),
        (GlslType::Vec2, 0) => format!("({}).x", expr),
        (GlslType::Vec2, 1) => format!("({}).y", expr),
        (GlslType::Vec2, 2) => "0.0".to_string(),
        (GlslType::Vec2, _) => "1.0".to_string(),
        (GlslType::Vec3, 0) => format!("({}).x", expr),
        (GlslType::Vec3, 1) => format!("({}).y", expr),
        (GlslType::Vec3, 2) => format!("({}).z", expr),
        (GlslType::Vec3, _) => "1.0".to_string(),
        (GlslType::Vec4, 0) => format!("({}).x", expr),
        (GlslType::Vec4, 1) => format!("({}).y", expr),
        (GlslType::Vec4, 2) => format!("({}).z", expr),
        (GlslType::Vec4, _) => format!("({}).w", expr),
    }
}

fn fmt_f32(v: f32) -> String {
    if v == v.trunc() && v.abs() < 1e6 {
        format!("{:.1}", v)
    } else {
        format!("{:.6}", v)
    }
}

/// Etapa de compilación: el mismo grafo produce el fragmento y, si
/// declara `VertexOutput`, también el vértice. La emisión de los nodos
/// fuente (Time/Uv/Normal/Position/Fresnel) depende de la etapa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Fragment,
    Vertex,
}

struct Compiler<'a> {
    nodes: HashMap<u32, &'a GraphNode>,
    /// Entradas conectadas: (nodo destino, nombre de entrada) -> nodo origen.
    inputs: HashMap<(u32, &'a str), u32>,
    emitted: HashMap<u32, Emitted>,
    visiting: Vec<u32>,
    body: String,
    stage: Stage,
}

impl<'a> Compiler<'a> {
    /// Entradas válidas de cada tipo de nodo (además de las propiedades).
    fn valid_inputs(kind: &NodeKind) -> &'static [&'static str] {
        match kind {
            NodeKind::Add | NodeKind::Multiply => &["a", "b"],
            NodeKind::Mix => &["a", "b", "t"],
            NodeKind::Sine | NodeKind::Abs | NodeKind::OneMinus | NodeKind::Smoothstep { .. } => &["x"],
            NodeKind::Output => &["color"],
            NodeKind::VertexOutput => &["offset"],
            NodeKind::ValueNoise { .. } | NodeKind::GradientNoise { .. } | NodeKind::FbmNoise { .. } => &["uv"],
        NodeKind::Subtract | NodeKind::Divide => &["a", "b"],
        NodeKind::ChannelR | NodeKind::ChannelG | NodeKind::ChannelB | NodeKind::ChannelA => &["x"],
        NodeKind::Combine => &["r", "g", "b", "a"],
        NodeKind::Dot | NodeKind::Distance => &["a", "b"],
        NodeKind::Length | NodeKind::Normalize | NodeKind::Clamp { .. } | NodeKind::Power { .. } | NodeKind::Step { .. } => &["x"],
        NodeKind::Modulo => &["x", "y"],
            _ => &[],
        }
    }

    fn input_or(&mut self, node: u32, input: &str, default: Emitted) -> Result<Emitted, GraphError> {
        if let Some(&src) = self.inputs.get(&(node, input)) {
            let e = self.emit(src)?;
            Ok(Emitted { expr: e.expr.clone(), ty: e.ty })
        } else {
            Ok(default)
        }
    }

    fn emit(&mut self, id: u32) -> Result<&Emitted, GraphError> {
        if self.emitted.contains_key(&id) {
            return Ok(&self.emitted[&id]);
        }
        if self.visiting.contains(&id) {
            return Err(GraphError::Cycle(id));
        }
        let node = match self.nodes.get(&id) {
            // `nodes` es una referencia compartida: get devuelve doble
            // referencia y hay que desenvolverla para clonar el nodo.
            Some(n) => (**n).clone(),
            None => return Err(GraphError::UnknownNode(id)),
        };
        self.visiting.push(id);
        let result = self.emit_node(&node);
        self.visiting.pop();
        let emitted = result?;
        // Los nodos con coste se materializan en una variable para no
        // repetir su cálculo si alimentan varias entradas.
        let var = format!("n{}", id);
        self.body
            .push_str(&format!("    {} {} = {};\n", emitted.ty.name(), var, emitted.expr));
        self.emitted.insert(id, Emitted { expr: var, ty: emitted.ty });
        Ok(&self.emitted[&id])
    }

    fn emit_node(&mut self, node: &GraphNode) -> Result<Emitted, GraphError> {
        let id = node.id;
        let scalar = |v: f32| Emitted { expr: fmt_f32(v), ty: GlslType::Float };
        match &node.kind {
            NodeKind::Color { rgba } => Ok(Emitted {
                expr: format!(
                    "vec4({}, {}, {}, {})",
                    fmt_f32(rgba[0]),
                    fmt_f32(rgba[1]),
                    fmt_f32(rgba[2]),
                    fmt_f32(rgba[3])
                ),
                ty: GlslType::Vec4,
            }),
            NodeKind::Float { value } => Ok(scalar(*value)),
            // El tiempo viaja en light_color.w (canal reservado del PBR),
            // y el rango de push constants del pipeline de mallas ya cubre
            // las etapas de vértice y fragmento: el mismo acceso vale en
            // las dos plantillas.
            NodeKind::Time => Ok(Emitted { expr: "u_push.light_color.w".into(), ty: GlslType::Float }),
            NodeKind::Uv => Ok(match self.stage {
                Stage::Fragment => Emitted { expr: "v_uv".into(), ty: GlslType::Vec2 },
                Stage::Vertex => Emitted { expr: "in_uv".into(), ty: GlslType::Vec2 },
            }),
            NodeKind::Normal => Ok(match self.stage {
                Stage::Fragment => Emitted { expr: "normalize(v_normal)".into(), ty: GlslType::Vec3 },
                // En vértices es el atributo crudo, SIN normalizar (a
                // diferencia del fragmento): un nodo que lo consuma en la
                // sub-red de vértices recibe el vector tal cual viene de
                // la malla.
                Stage::Vertex => Emitted { expr: "in_normal".into(), ty: GlslType::Vec3 },
            }),
            NodeKind::Position { axis } => match self.stage {
                Stage::Fragment => Err(GraphError::VertexOnlyNode(id)),
                Stage::Vertex => Ok(match axis {
                    PositionAxis::X => Emitted { expr: "in_position.x".into(), ty: GlslType::Float },
                    PositionAxis::Y => Emitted { expr: "in_position.y".into(), ty: GlslType::Float },
                    PositionAxis::Z => Emitted { expr: "in_position.z".into(), ty: GlslType::Float },
                    PositionAxis::All => Emitted { expr: "in_position".into(), ty: GlslType::Vec3 },
                }),
            },
            NodeKind::InteractionPoint { component } => {
                // La posición de mundo contra la que se miden las
                // distancias: en fragmento es el varying interpolado; en
                // vértices se calcula desde la posición SIN desplazar
                // (la estable: medir contra la posición ya desplazada
                // haría la máscara dependiente de su propio resultado).
                let world_pos = match self.stage {
                    Stage::Fragment => "v_world_pos",
                    Stage::Vertex => "(u_push.model * vec4(in_position, 1.0)).xyz",
                };
                let p = "u_globals.interaction_point";
                Ok(match component {
                    InteractionComponent::All => {
                        Emitted { expr: p.into(), ty: GlslType::Vec4 }
                    }
                    InteractionComponent::X => {
                        Emitted { expr: format!("{}.x", p), ty: GlslType::Float }
                    }
                    InteractionComponent::Y => {
                        Emitted { expr: format!("{}.y", p), ty: GlslType::Float }
                    }
                    InteractionComponent::Z => {
                        Emitted { expr: format!("{}.z", p), ty: GlslType::Float }
                    }
                    InteractionComponent::Radius => {
                        Emitted { expr: format!("{}.w", p), ty: GlslType::Float }
                    }
                    // Las distancias llaman a las funciones del canal
                    // global (GRAPH_GLOBALS_GLSL): ellas eligen la forma
                    // activa —esfera o huella de caja— y devuelven la
                    // distancia con signo a su superficie.
                    InteractionComponent::Distance => Emitted {
                        expr: format!("graph_interaction_distance({})", world_pos),
                        ty: GlslType::Float,
                    },
                    InteractionComponent::DistanceXZ => Emitted {
                        expr: format!("graph_interaction_distance_xz({})", world_pos),
                        ty: GlslType::Float,
                    },
                })
            }
            NodeKind::Subtract | NodeKind::Divide => {
                let is_sub = matches!(node.kind, NodeKind::Subtract);
                let (adef, bdef) = if is_sub { (0.0, 0.0) } else { (0.0, 1.0) };
                let a = self.input_or(id, "a", scalar(adef))?;
                let b = self.input_or(id, "b", scalar(bdef))?;
                let ty = a.ty.max(b.ty);
                let a = coerce(&a.expr, a.ty, ty);
                let b = coerce(&b.expr, b.ty, ty);
                let op = if is_sub { "-" } else { "/" };
                Ok(Emitted { expr: format!("({} {} {})", a, op, b), ty })
            }
            NodeKind::ChannelR | NodeKind::ChannelG | NodeKind::ChannelB | NodeKind::ChannelA => {
                let x = self.input_or(id, "x", scalar(0.0))?;
                let ch = match node.kind {
                    NodeKind::ChannelR => 0,
                    NodeKind::ChannelG => 1,
                    NodeKind::ChannelB => 2,
                    _ => 3,
                };
                Ok(Emitted { expr: channel_expr(&x.expr, x.ty, ch), ty: GlslType::Float })
            }
            NodeKind::Combine => {
                let r = self.input_or(id, "r", scalar(0.0))?;
                let g = self.input_or(id, "g", scalar(0.0))?;
                let b = self.input_or(id, "b", scalar(0.0))?;
                let a = self.input_or(id, "a", scalar(1.0))?;
                let r = coerce(&r.expr, r.ty, GlslType::Float);
                let g = coerce(&g.expr, g.ty, GlslType::Float);
                let b = coerce(&b.expr, b.ty, GlslType::Float);
                let a = coerce(&a.expr, a.ty, GlslType::Float);
                Ok(Emitted {
                    expr: format!("vec4({}, {}, {}, {})", r, g, b, a),
                    ty: GlslType::Vec4,
                })
            }
            NodeKind::Dot => {
                let a = self.input_or(id, "a", Emitted { expr: "vec2(0.0)".into(), ty: GlslType::Vec2 })?;
                let b = self.input_or(id, "b", Emitted { expr: "vec2(0.0)".into(), ty: GlslType::Vec2 })?;
                let ty = a.ty.max(b.ty).max(GlslType::Vec2);
                let a = coerce(&a.expr, a.ty, ty);
                let b = coerce(&b.expr, b.ty, ty);
                Ok(Emitted { expr: format!("dot({}, {})", a, b), ty: GlslType::Float })
            }
            NodeKind::Length => {
                let x = self.input_or(id, "x", Emitted { expr: "vec2(0.0)".into(), ty: GlslType::Vec2 })?;
                let ty = x.ty.max(GlslType::Vec2);
                let x = coerce(&x.expr, x.ty, ty);
                Ok(Emitted { expr: format!("length({})", x), ty: GlslType::Float })
            }
            NodeKind::Distance => {
                let a = self.input_or(id, "a", Emitted { expr: "vec2(0.0)".into(), ty: GlslType::Vec2 })?;
                let b = self.input_or(id, "b", Emitted { expr: "vec2(0.0)".into(), ty: GlslType::Vec2 })?;
                let ty = a.ty.max(b.ty).max(GlslType::Vec2);
                let a = coerce(&a.expr, a.ty, ty);
                let b = coerce(&b.expr, b.ty, ty);
                Ok(Emitted { expr: format!("distance({}, {})", a, b), ty: GlslType::Float })
            }
            NodeKind::Normalize => {
                let x = self.input_or(id, "x", Emitted { expr: "vec2(0.0)".into(), ty: GlslType::Vec2 })?;
                // normalize() no existe para float en GLSL: un escalar
                // «normalizado» es él mismo (su dirección es su signo y
                // el grafo decide); los vectores sí se normalizan.
                if x.ty == GlslType::Float {
                    Ok(Emitted { expr: x.expr.clone(), ty: x.ty })
                } else {
                    Ok(Emitted { expr: format!("normalize({})", x.expr), ty: x.ty })
                }
            }
            NodeKind::Clamp { min, max } => {
                let x = self.input_or(id, "x", scalar(0.0))?;
                Ok(Emitted {
                    expr: format!("clamp({}, {}, {})", x.expr, fmt_f32(*min), fmt_f32(*max)),
                    ty: x.ty,
                })
            }
            NodeKind::Power { exponent } => {
                let x = self.input_or(id, "x", scalar(1.0))?;
                let e = fmt_f32(*exponent);
                let expr = match x.ty {
                    GlslType::Float => format!("pow({}, {})", x.expr, e),
                    GlslType::Vec2 => format!("pow({}, vec2({}))", x.expr, e),
                    GlslType::Vec3 => format!("pow({}, vec3({}))", x.expr, e),
                    GlslType::Vec4 => format!("pow({}, vec4({}))", x.expr, e),
                };
                Ok(Emitted { expr, ty: x.ty })
            }
            NodeKind::Step { edge } => {
                let x = self.input_or(id, "x", scalar(0.0))?;
                Ok(Emitted { expr: format!("step({}, {})", fmt_f32(*edge), x.expr), ty: x.ty })
            }
            NodeKind::Modulo => {
                let x = self.input_or(id, "x", scalar(0.0))?;
                let y = self.input_or(id, "y", scalar(1.0))?;
                let ty = x.ty.max(y.ty);
                let x = coerce(&x.expr, x.ty, ty);
                let y = coerce(&y.expr, y.ty, ty);
                Ok(Emitted { expr: format!("mod({}, {})", x, y), ty })
            }
            NodeKind::ValueNoise { scale } => {
                let uv_default = match self.stage {
                    Stage::Fragment => "v_uv",
                    Stage::Vertex => "in_uv",
                };
                let uv = self.input_or(id, "uv", Emitted { expr: uv_default.into(), ty: GlslType::Vec2 })?;
                let uv = coerce(&uv.expr, uv.ty, GlslType::Vec2);
                Ok(Emitted {
                    expr: format!("graph_value_noise({}, {})", uv, fmt_f32(*scale)),
                    ty: GlslType::Float,
                })
            }
            NodeKind::GradientNoise { scale } => {
                let uv_default = match self.stage {
                    Stage::Fragment => "v_uv",
                    Stage::Vertex => "in_uv",
                };
                let uv = self.input_or(id, "uv", Emitted { expr: uv_default.into(), ty: GlslType::Vec2 })?;
                let uv = coerce(&uv.expr, uv.ty, GlslType::Vec2);
                Ok(Emitted {
                    expr: format!("graph_gradient_noise({}, {})", uv, fmt_f32(*scale)),
                    ty: GlslType::Float,
                })
            }
            NodeKind::FbmNoise { scale, octaves, lacunarity, gain } => {
                let uv_default = match self.stage {
                    Stage::Fragment => "v_uv",
                    Stage::Vertex => "in_uv",
                };
                let uv = self.input_or(id, "uv", Emitted { expr: uv_default.into(), ty: GlslType::Vec2 })?;
                let uv = coerce(&uv.expr, uv.ty, GlslType::Vec2);
                let octaves = (*octaves).clamp(1, 6);
                Ok(Emitted {
                    expr: format!(
                        "graph_fbm_noise({}, {}, {}, {}, {})",
                        uv,
                        fmt_f32(*scale),
                        octaves,
                        fmt_f32(*lacunarity),
                        fmt_f32(*gain)
                    ),
                    ty: GlslType::Float,
                })
            }
            NodeKind::Add | NodeKind::Multiply => {
                let is_add = matches!(node.kind, NodeKind::Add);
                let def = if is_add { 0.0 } else { 1.0 };
                let a = self.input_or(id, "a", scalar(def))?;
                let b = self.input_or(id, "b", scalar(def))?;
                let ty = a.ty.max(b.ty);
                let a = coerce(&a.expr, a.ty, ty);
                let b = coerce(&b.expr, b.ty, ty);
                let op = if is_add { "+" } else { "*" };
                Ok(Emitted { expr: format!("({} {} {})", a, op, b), ty })
            }
            NodeKind::Mix => {
                let a = self.input_or(id, "a", Emitted { expr: "vec4(0.0)".into(), ty: GlslType::Vec4 })?;
                let b = self.input_or(id, "b", Emitted { expr: "vec4(1.0)".into(), ty: GlslType::Vec4 })?;
                let t = self.input_or(id, "t", scalar(0.5))?;
                let ty = a.ty.max(b.ty);
                let a = coerce(&a.expr, a.ty, ty);
                let b = coerce(&b.expr, b.ty, ty);
                let t = coerce(&t.expr, t.ty, ty);
                Ok(Emitted { expr: format!("mix({}, {}, {})", a, b, t), ty })
            }
            NodeKind::Sine | NodeKind::Abs | NodeKind::OneMinus => {
                let x = self.input_or(id, "x", scalar(0.0))?;
                let expr = match node.kind {
                    NodeKind::Sine => format!("sin({})", x.expr),
                    NodeKind::Abs => format!("abs({})", x.expr),
                    _ => format!("(1.0 - {})", x.expr),
                };
                Ok(Emitted { expr, ty: x.ty })
            }
            NodeKind::Smoothstep { edge0, edge1 } => {
                let x = self.input_or(id, "x", scalar(0.0))?;
                let x = coerce(&x.expr, x.ty, GlslType::Float);
                Ok(Emitted {
                    expr: format!("smoothstep({}, {}, {})", fmt_f32(*edge0), fmt_f32(*edge1), x),
                    ty: GlslType::Float,
                })
            }
            NodeKind::RadialUv { edge0, edge1 } => {
                // La UV de la etapa: en fragmento es el varying
                // interpolado; en vértices, el atributo crudo (el
                // mismo espejo que el nodo Uv). La distancia va del
                // centro del quad (0) al círculo inscrito (1); las
                // esquinas quedan a √2, fuera de la máscara.
                let uv = match self.stage {
                    Stage::Fragment => "v_uv",
                    Stage::Vertex => "in_uv",
                };
                Ok(Emitted {
                    expr: format!(
                        "vec4(1.0, 1.0, 1.0, 1.0 - smoothstep({}, {}, length({} - vec2(0.5)) * 2.0))",
                        fmt_f32(*edge0),
                        fmt_f32(*edge1),
                        uv
                    ),
                    ty: GlslType::Vec4,
                })
            }
            NodeKind::Fresnel { power } => match self.stage {
                Stage::Fragment => Ok(Emitted {
                    expr: format!(
                        "pow(clamp(1.0 - max(dot(normalize(v_normal), normalize(u_push.camera_pos.xyz - v_world_pos)), 0.0), 0.0, 1.0), {})",
                        fmt_f32(*power)
                    ),
                    ty: GlslType::Float,
                }),
                // El Fresnel necesita la dirección de vista por fragmento;
                // en la etapa de vértices no se ofrece en esta v1.
                Stage::Vertex => Err(GraphError::StageUnsupported(id)),
            },
            // Los dos nodos terminales nunca se emiten como expresión: el
            // compilador de cada etapa arranca desde su entrada.
            NodeKind::Output | NodeKind::VertexOutput => Err(GraphError::UnknownInput(id, "output".into())),
        }
    }
}

impl ShaderGraph {
    /// Valida el grafo y construye los mapas que consume el compilador:
    /// nodos por id y entradas conectadas. Compartido por las dos etapas.
    fn validated_maps<'a>(
        &'a self,
    ) -> Result<(HashMap<u32, &'a GraphNode>, HashMap<(u32, &'a str), u32>), GraphError> {
        let mut nodes: HashMap<u32, &GraphNode> = HashMap::new();
        for n in &self.nodes {
            if nodes.insert(n.id, n).is_some() {
                return Err(GraphError::DuplicateId(n.id));
            }
        }
        let mut inputs: HashMap<(u32, &str), u32> = HashMap::new();
        for e in &self.edges {
            let dst = nodes.get(&e.to_node).ok_or(GraphError::UnknownNode(e.to_node))?;
            if !nodes.contains_key(&e.from_node) {
                return Err(GraphError::UnknownNode(e.from_node));
            }
            if !Compiler::valid_inputs(&dst.kind).contains(&e.to_input.as_str()) {
                return Err(GraphError::UnknownInput(e.to_node, e.to_input.clone()));
            }
            inputs.insert((e.to_node, e.to_input.as_str()), e.from_node);
        }
        Ok((nodes, inputs))
    }

    /// Compila el grafo a un fragment shader GLSL 450 completo, listo para
    /// glslangValidator. La plantilla declara las mismas entradas que el
    /// pipeline de mallas del motor (posición, normal, UV).
    pub fn compile_fragment(&self) -> Result<String, GraphError> {
        let (nodes, inputs) = self.validated_maps()?;
        let output = self
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Output))
            .ok_or(GraphError::MissingOutput)?;

        let mut compiler = Compiler {
            nodes,
            inputs,
            emitted: HashMap::new(),
            visiting: Vec::new(),
            body: String::new(),
            stage: Stage::Fragment,
        };

        // El color final: la entrada "color" del Output (negro si no hay).
        let final_expr = match compiler.inputs.get(&(output.id, "color")) {
            Some(&src) => {
                let e = compiler.emit(src)?;
                coerce(&e.expr.clone(), e.ty, GlslType::Vec4)
            }
            None => "vec4(0.0, 0.0, 0.0, 1.0)".to_string(),
        };

        Ok(format!(
            "#version 450\n\
             // Generado por el grafo de shaders «{name}» de GlassEngine.\n\
             layout(location = 0) in vec3 v_world_pos;\n\
             layout(location = 1) in vec3 v_normal;\n\
             layout(location = 2) in vec2 v_uv;\n\
             layout(location = 0) out vec4 out_color;\n\
             layout(set = 0, binding = 0) uniform sampler2D base_color_tex;\n\
             layout(push_constant) uniform PushConstants {{\n\
             \x20   mat4 mvp;\n\
             \x20   mat4 model;\n\
             \x20   vec4 light_dir;\n\
             \x20   vec4 light_color;\n\
             \x20   vec4 material_params;\n\
             \x20   vec4 object_color;\n\
             \x20   vec4 camera_pos;\n\
             }} u_push;\n\
             {globals}\
             {noise}\
             void main() {{\n{body}\
             \x20   out_color = {final_expr};\n\
             }}\n",
            name = self.name,
            globals = GRAPH_GLOBALS_GLSL,
            noise = NOISE_GLSL,
            body = compiler.body,
            final_expr = final_expr
        ))
    }

    /// Compila la sub-red de vértices a un vertex shader GLSL 450, espejo
    /// del vértice PBR del motor (mismos atributos, varyings y bloque de
    /// push constants — que debe permanecer idéntico byte a byte al del
    /// fragmento, porque ambos comparten el mismo rango de memoria).
    /// Devuelve `Ok(None)` si el grafo no declara desplazamiento: sin nodo
    /// `VertexOutput`, o con su entrada "offset" sin conectar. En ese caso
    /// el runtime usa el vértice estándar y el grafo se comporta como
    /// antes de que existiera esta etapa.
    ///
    /// El offset se suma a la posición en espacio de objeto ANTES de las
    /// matrices de modelo y MVP (el patrón de Shader Graph), así que lo
    /// escala el transform de la entidad. Las normales NO se recalculan:
    /// viajan las originales (límite documentado de la v1).
    pub fn compile_vertex(&self) -> Result<Option<String>, GraphError> {
        let Some(vertex_output) = self
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::VertexOutput))
        else {
            return Ok(None);
        };
        let (nodes, inputs) = self.validated_maps()?;
        let Some(&src) = inputs.get(&(vertex_output.id, "offset")) else {
            return Ok(None);
        };

        let mut compiler = Compiler {
            nodes,
            inputs,
            emitted: HashMap::new(),
            visiting: Vec::new(),
            body: String::new(),
            stage: Stage::Vertex,
        };
        let e = compiler.emit(src)?;
        let offset_expr = coerce(&e.expr.clone(), e.ty, GlslType::Vec3);

        Ok(Some(format!(
            "#version 450\n\
             // Generado por el grafo de shaders «{name}» de GlassEngine (etapa de vértices).\n\
             layout(location = 0) in vec3 in_position;\n\
             layout(location = 1) in vec3 in_normal;\n\
             layout(location = 2) in vec2 in_uv;\n\
             layout(location = 0) out vec3 v_world_pos;\n\
             layout(location = 1) out vec3 v_normal;\n\
             layout(location = 2) out vec2 v_uv;\n\
             layout(push_constant) uniform PushConstants {{\n\
             \x20   mat4 mvp;\n\
             \x20   mat4 model;\n\
             \x20   vec4 light_dir;\n\
             \x20   vec4 light_color;\n\
             \x20   vec4 material_params;\n\
             \x20   vec4 object_color;\n\
             \x20   vec4 camera_pos;\n\
             }} u_push;\n\
             {globals}\
             {noise}\
             void main() {{\n{body}\
             \x20   vec3 displaced = in_position + {offset};\n\
             \x20   v_world_pos = (u_push.model * vec4(displaced, 1.0)).xyz;\n\
             \x20   v_normal = transpose(inverse(mat3(u_push.model))) * in_normal;\n\
             \x20   v_uv = in_uv;\n\
             \x20   gl_Position = u_push.mvp * vec4(displaced, 1.0);\n\
             }}\n",
            name = self.name,
            globals = GRAPH_GLOBALS_GLSL,
            noise = NOISE_GLSL,
            body = compiler.body,
            offset = offset_expr
        )))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

/// La librería de prefabricados: grafos abiertos y editables, pensados para
/// aprender modificándolos. Cada uno es un `ShaderGraph` normal que se puede
/// guardar como JSON en `assets/shader_graphs/`.
pub mod presets {
    use super::*;

    fn node(id: u32, kind: NodeKind) -> GraphNode {
        GraphNode { id, kind, position: [0.0, 0.0] }
    }

    fn edge(from_node: u32, to_node: u32, to_input: &str) -> GraphEdge {
        GraphEdge { from_node, to_node, to_input: to_input.to_string() }
    }

    /// Agua: mezcla de azul profundo y azul claro movida por dos ondas.
    pub fn agua() -> ShaderGraph {
        // onda = sin((uv.x + uv.y) * 8 + tiempo * 1.5) * 0.5 + 0.5
        ShaderGraph {
            transparent: false,
            name: "agua".into(),
            nodes: vec![
                node(1, NodeKind::Uv),
                node(2, NodeKind::Float { value: 8.0 }),
                node(3, NodeKind::Multiply), // uv * 8 (vec2)
                node(4, NodeKind::Time),
                node(5, NodeKind::Float { value: 1.5 }),
                node(6, NodeKind::Multiply), // tiempo * 1.5
                node(7, NodeKind::Add),      // (uv*8) + (t*1.5) -> vec2
                node(8, NodeKind::Sine),     // sin(...) vec2
                node(9, NodeKind::Color { rgba: [0.02, 0.15, 0.35, 1.0] }),
                node(10, NodeKind::Color { rgba: [0.15, 0.55, 0.85, 1.0] }),
                node(11, NodeKind::Mix),
                node(12, NodeKind::Output),
            ],
            edges: vec![
                edge(1, 3, "a"),
                edge(2, 3, "b"),
                edge(4, 6, "a"),
                edge(5, 6, "b"),
                edge(3, 7, "a"),
                edge(6, 7, "b"),
                edge(7, 8, "x"),
                edge(9, 11, "a"),
                edge(10, 11, "b"),
                edge(8, 11, "t"),
                edge(11, 12, "color"),
            ],
        }
    }

    /// Espuma: bandas blancas suaves que avanzan con el tiempo.
    pub fn espuma() -> ShaderGraph {
        // banda = smoothstep(0.6, 0.95, sin(uv.y * 30 - tiempo * 2))
        // color = mix(azul, blanco, banda)
        // Salida: color * RadialUv — el patrón de siempre con el alfa
        // recortado a un disco suave (grafo transparente): sobre un
        // quad plano se lee como espuma redonda, sin esquinas.
        ShaderGraph {
            transparent: true,
            name: "espuma".into(),
            nodes: vec![
                node(1, NodeKind::Uv),
                node(2, NodeKind::Float { value: 30.0 }),
                node(3, NodeKind::Multiply),
                node(4, NodeKind::Time),
                node(5, NodeKind::Float { value: -2.0 }),
                node(6, NodeKind::Multiply),
                node(7, NodeKind::Add),
                node(8, NodeKind::Sine),
                node(9, NodeKind::Smoothstep { edge0: 0.6, edge1: 0.95 }),
                node(10, NodeKind::Color { rgba: [0.05, 0.25, 0.5, 1.0] }),
                node(11, NodeKind::Color { rgba: [1.0, 1.0, 1.0, 1.0] }),
                node(12, NodeKind::Mix),
                node(13, NodeKind::Output),
                node(14, NodeKind::RadialUv { edge0: 0.7, edge1: 1.0 }),
                node(15, NodeKind::Multiply),
            ],
            edges: vec![
                edge(1, 3, "a"),
                edge(2, 3, "b"),
                edge(4, 6, "a"),
                edge(5, 6, "b"),
                edge(3, 7, "a"),
                edge(6, 7, "b"),
                edge(7, 8, "x"),
                edge(8, 9, "x"),
                edge(10, 12, "a"),
                edge(11, 12, "b"),
                edge(9, 12, "t"),
                edge(12, 15, "a"),
                edge(14, 15, "b"),
                edge(15, 13, "color"),
            ],
        }
    }

    /// Fragmentos: destellos cristalinos fríos con borde de Fresnel.
    pub fn fragmentos() -> ShaderGraph {
        // destello = sin(uv.x*40) * sin(uv.y*40); color = mix(base, blanco, fresnel * destello)
        ShaderGraph {
            transparent: false,
            name: "fragmentos".into(),
            nodes: vec![
                node(1, NodeKind::Uv),
                node(2, NodeKind::Float { value: 40.0 }),
                node(3, NodeKind::Multiply),
                node(4, NodeKind::Sine),
                node(5, NodeKind::Fresnel { power: 2.0 }),
                node(6, NodeKind::Multiply), // fresnel * destello
                node(7, NodeKind::Color { rgba: [0.1, 0.12, 0.2, 1.0] }),
                node(8, NodeKind::Color { rgba: [0.85, 0.95, 1.0, 1.0] }),
                node(9, NodeKind::Mix),
                node(10, NodeKind::Output),
            ],
            edges: vec![
                edge(1, 3, "a"),
                edge(2, 3, "b"),
                edge(3, 4, "x"),
                edge(5, 6, "a"),
                edge(4, 6, "b"),
                edge(7, 9, "a"),
                edge(8, 9, "b"),
                edge(6, 9, "t"),
                edge(9, 10, "color"),
            ],
        }
    }

    /// Láser: núcleo brillante vertical que se desvanece hacia los lados.
    pub fn laser() -> ShaderGraph {
        // nucleo = 1 - smoothstep(0.0, 0.5, abs(uv.x - 0.5)); color = rojo * nucleo + nucleo
        ShaderGraph {
            transparent: false,
            name: "laser".into(),
            nodes: vec![
                node(1, NodeKind::Uv),
                node(3, NodeKind::Add), // uv + (-0.5) por el puerto b con Float -0.5
                node(4, NodeKind::Float { value: -0.5 }),
                node(5, NodeKind::Abs),
                node(6, NodeKind::Smoothstep { edge0: 0.0, edge1: 0.5 }),
                node(7, NodeKind::OneMinus),
                node(8, NodeKind::Color { rgba: [1.0, 0.1, 0.1, 1.0] }),
                node(9, NodeKind::Multiply),
                node(10, NodeKind::Output),
            ],
            edges: vec![
                edge(1, 3, "a"),
                edge(4, 3, "b"),
                edge(3, 5, "x"),
                edge(5, 6, "x"),
                edge(6, 7, "x"),
                edge(8, 9, "a"),
                edge(7, 9, "b"),
                edge(9, 10, "color"),
            ],
        }
    }

    /// Brisa: gradiente suave que respira con el tiempo (hojas/césped).
    pub fn brisa() -> ShaderGraph {
        // t = sin(uv.x * 3 + tiempo * 0.8); color = mix(verde oscuro, verde claro, t)
        ShaderGraph {
            transparent: false,
            name: "brisa".into(),
            nodes: vec![
                node(1, NodeKind::Uv),
                node(2, NodeKind::Float { value: 3.0 }),
                node(3, NodeKind::Multiply),
                node(4, NodeKind::Time),
                node(5, NodeKind::Float { value: 0.8 }),
                node(6, NodeKind::Multiply),
                node(7, NodeKind::Add),
                node(8, NodeKind::Sine),
                node(9, NodeKind::Color { rgba: [0.08, 0.3, 0.08, 1.0] }),
                node(10, NodeKind::Color { rgba: [0.45, 0.75, 0.3, 1.0] }),
                node(11, NodeKind::Mix),
                node(12, NodeKind::Output),
            ],
            edges: vec![
                edge(1, 3, "a"),
                edge(2, 3, "b"),
                edge(4, 6, "a"),
                edge(5, 6, "b"),
                edge(3, 7, "a"),
                edge(6, 7, "b"),
                edge(7, 8, "x"),
                edge(9, 11, "a"),
                edge(10, 11, "b"),
                edge(8, 11, "t"),
                edge(11, 12, "color"),
            ],
        }
    }

    /// Tormenta de arena: capas de ondas cálidas que corren con el viento.
    pub fn tormenta_arena() -> ShaderGraph {
        // capas = sin(uv.x*12 + tiempo*3) * sin(uv.y*7 - tiempo*1.7)
        // color = mix(arena oscura, arena clara, capas)
        ShaderGraph {
            transparent: false,
            name: "tormenta_arena".into(),
            nodes: vec![
                node(1, NodeKind::Uv),
                node(2, NodeKind::Float { value: 12.0 }),
                node(3, NodeKind::Multiply),
                node(4, NodeKind::Time),
                node(5, NodeKind::Float { value: 3.0 }),
                node(6, NodeKind::Multiply),
                node(7, NodeKind::Add),
                node(8, NodeKind::Sine),
                node(9, NodeKind::Float { value: 7.0 }),
                node(10, NodeKind::Multiply), // uv * 7 (segunda capa)
                node(11, NodeKind::Float { value: -1.7 }),
                node(12, NodeKind::Multiply), // tiempo * -1.7
                node(13, NodeKind::Add),
                node(14, NodeKind::Sine),
                node(15, NodeKind::Multiply), // capa1 * capa2
                node(16, NodeKind::Color { rgba: [0.45, 0.32, 0.15, 1.0] }),
                node(17, NodeKind::Color { rgba: [0.9, 0.75, 0.45, 1.0] }),
                node(18, NodeKind::Mix),
                node(19, NodeKind::Output),
            ],
            edges: vec![
                edge(1, 3, "a"),
                edge(2, 3, "b"),
                edge(4, 6, "a"),
                edge(5, 6, "b"),
                edge(3, 7, "a"),
                edge(6, 7, "b"),
                edge(7, 8, "x"),
                edge(1, 10, "a"),
                edge(9, 10, "b"),
                edge(4, 12, "a"),
                edge(11, 12, "b"),
                edge(10, 13, "a"),
                edge(12, 13, "b"),
                edge(13, 14, "x"),
                edge(8, 15, "a"),
                edge(14, 15, "b"),
                edge(16, 18, "a"),
                edge(17, 18, "b"),
                edge(15, 18, "t"),
                edge(18, 19, "color"),
            ],
        }
    }

    /// Toda la librería, en el orden en que la mostrará el editor.
    pub fn all() -> Vec<ShaderGraph> {
        vec![agua(), espuma(), fragmentos(), laser(), brisa(), tormenta_arena()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todos_los_prefabricados_compilan() {
        for g in presets::all() {
            let glsl = g.compile_fragment().unwrap_or_else(|e| panic!("{}: {}", g.name, e));
            assert!(glsl.contains("void main()"), "{}", g.name);
            assert!(glsl.contains("out_color ="), "{}", g.name);
        }
    }

    #[test]
    fn la_espuma_es_transparente_y_lleva_la_mascara_radial() {
        let g = presets::espuma();
        assert!(g.transparent, "la espuma se declara transparente");
        let glsl = g.compile_fragment().expect("la espuma compila");
        // La máscara radial horneada: distancia a la UV centrada y el
        // vec4 (1,1,1,alfa) que multiplica al color camino del Output.
        assert!(
            glsl.contains("length(v_uv - vec2(0.5)) * 2.0"),
            "el fragmento lleva la distancia radial:\n{glsl}"
        );
        assert!(
            glsl.contains("1.0 - smoothstep(0.700000, 1.0,"),
            "el alfa cae con smoothstep(0.7, 1.0):\n{glsl}"
        );
        // El resto de prefabricados sigue opaco.
        for g in presets::all() {
            if g.name != "espuma" {
                assert!(!g.transparent, "{} no debe ser transparente", g.name);
            }
        }
    }

    #[test]
    fn radial_uv_tambien_compila_en_la_etapa_de_vertices() {
        // Un grafo mínimo que usa RadialUv también como offset de
        // vértices: el nodo lee in_uv en esa etapa y no debe error.
        let g = ShaderGraph {
            transparent: false,
            name: "radial_vertices".into(),
            nodes: vec![
                GraphNode { id: 1, kind: NodeKind::Color { rgba: [1.0, 1.0, 1.0, 1.0] }, position: [0.0, 0.0] },
                GraphNode { id: 2, kind: NodeKind::RadialUv { edge0: 0.7, edge1: 1.0 }, position: [0.0, 0.0] },
                GraphNode { id: 3, kind: NodeKind::Multiply, position: [0.0, 0.0] },
                GraphNode { id: 4, kind: NodeKind::Output, position: [0.0, 0.0] },
                GraphNode { id: 5, kind: NodeKind::VertexOutput, position: [0.0, 0.0] },
            ],
            edges: vec![
                GraphEdge { from_node: 1, to_node: 3, to_input: "a".into() },
                GraphEdge { from_node: 2, to_node: 3, to_input: "b".into() },
                GraphEdge { from_node: 3, to_node: 4, to_input: "color".into() },
                GraphEdge { from_node: 2, to_node: 5, to_input: "offset".into() },
            ],
        };
        let frag = g.compile_fragment().expect("fragmento compila");
        assert!(frag.contains("length(v_uv - vec2(0.5))"));
        let vert = g.compile_vertex().expect("vértices compila").expect("hay etapa de vértices");
        assert!(vert.contains("length(in_uv - vec2(0.5))"), "en vértices lee in_uv:\n{vert}");
    }

    #[test]
    fn el_json_de_un_prefabricado_es_ida_y_vuelta() {
        let g = presets::agua();
        let text = g.to_json();
        let back = ShaderGraph::from_json(&text).expect("json válido");
        assert_eq!(back.name, "agua");
        assert_eq!(back.nodes.len(), g.nodes.len());
        assert_eq!(back.compile_fragment().unwrap(), g.compile_fragment().unwrap());
    }

    #[test]
    fn detecta_ciclos() {
        let g = ShaderGraph {
            transparent: false,
            name: "ciclo".into(),
            nodes: vec![
                GraphNode { id: 1, kind: NodeKind::Add, position: [0.0, 0.0] },
                GraphNode { id: 2, kind: NodeKind::Multiply, position: [0.0, 0.0] },
                GraphNode { id: 3, kind: NodeKind::Output, position: [0.0, 0.0] },
            ],
            edges: vec![
                GraphEdge { from_node: 2, to_node: 1, to_input: "a".into() },
                GraphEdge { from_node: 1, to_node: 2, to_input: "a".into() },
                GraphEdge { from_node: 1, to_node: 3, to_input: "color".into() },
            ],
        };
        assert!(matches!(g.compile_fragment(), Err(GraphError::Cycle(_))));
    }

    #[test]
    fn detecta_entrada_desconocida_y_falta_de_output() {
        let sin_output = ShaderGraph {
            transparent: false,
            name: "x".into(),
            nodes: vec![GraphNode { id: 1, kind: NodeKind::Time, position: [0.0, 0.0] }],
            edges: vec![],
        };
        assert_eq!(sin_output.compile_fragment(), Err(GraphError::MissingOutput));

        let mala = ShaderGraph {
            transparent: false,
            name: "x".into(),
            nodes: vec![
                GraphNode { id: 1, kind: NodeKind::Time, position: [0.0, 0.0] },
                GraphNode { id: 2, kind: NodeKind::Output, position: [0.0, 0.0] },
            ],
            edges: vec![GraphEdge { from_node: 1, to_node: 2, to_input: "brillo".into() }],
        };
        assert!(matches!(mala.compile_fragment(), Err(GraphError::UnknownInput(2, _))));
    }

    fn nodo(id: u32, kind: NodeKind) -> GraphNode {
        GraphNode { id, kind, position: [0.0, 0.0] }
    }

    fn arista(from_node: u32, to_node: u32, to_input: &str) -> GraphEdge {
        GraphEdge { from_node, to_node, to_input: to_input.into() }
    }

    /// Grafo con sub-red de vértices: h = 0,07·sin(37,7·Position.X + 1,3·t)
    /// + 0,05·sin(28,3·Position.Z + 0,9·t); offset = (0, h, 0) vía un color
    /// verde multiplicado por h. Y su fragmento trivial (un color fijo).
    fn grafo_con_olas() -> ShaderGraph {
        use NodeKind as K;
        use PositionAxis as A;
        ShaderGraph {
            transparent: false,
            name: "olas".into(),
            nodes: vec![
                nodo(1, K::Position { axis: A::X }),
                nodo(2, K::Float { value: 37.7 }),
                nodo(3, K::Multiply),
                nodo(4, K::Time),
                nodo(5, K::Float { value: 1.3 }),
                nodo(6, K::Multiply),
                nodo(7, K::Add),
                nodo(8, K::Sine),
                nodo(9, K::Float { value: 0.07 }),
                nodo(10, K::Multiply),
                nodo(11, K::Position { axis: A::Z }),
                nodo(12, K::Float { value: 28.3 }),
                nodo(13, K::Multiply),
                nodo(14, K::Float { value: 0.9 }),
                nodo(15, K::Multiply),
                nodo(16, K::Add),
                nodo(17, K::Sine),
                nodo(18, K::Float { value: 0.05 }),
                nodo(19, K::Multiply),
                nodo(20, K::Add), // h = onda1 + onda2
                nodo(21, K::Color { rgba: [0.0, 1.0, 0.0, 1.0] }),
                nodo(22, K::Multiply), // (0,h,0,h) -> offset
                nodo(23, K::VertexOutput),
                nodo(24, K::Color { rgba: [0.1, 0.3, 0.6, 1.0] }),
                nodo(25, K::Output),
            ],
            edges: vec![
                arista(1, 3, "a"),
                arista(2, 3, "b"),
                arista(4, 6, "a"),
                arista(5, 6, "b"),
                arista(3, 7, "a"),
                arista(6, 7, "b"),
                arista(7, 8, "x"),
                arista(8, 10, "a"),
                arista(9, 10, "b"),
                arista(11, 13, "a"),
                arista(12, 13, "b"),
                arista(4, 15, "a"),
                arista(14, 15, "b"),
                arista(13, 16, "a"),
                arista(15, 16, "b"),
                arista(16, 17, "x"),
                arista(17, 19, "a"),
                arista(18, 19, "b"),
                arista(10, 20, "a"),
                arista(19, 20, "b"),
                arista(21, 22, "a"),
                arista(20, 22, "b"),
                arista(22, 23, "offset"),
                arista(24, 25, "color"),
            ],
        }
    }

    #[test]
    fn sin_vertex_output_no_hay_etapa_de_vertices() {
        // Ningún prefabricado declara desplazamiento: todos devuelven None
        // (no-regresión: el runtime seguirá usando el vértice estándar).
        for g in presets::all() {
            assert_eq!(g.compile_vertex(), Ok(None), "{}", g.name);
        }
        // Y con VertexOutput presente pero sin conectar, también None.
        let mut g = grafo_con_olas();
        g.edges.retain(|e| e.to_input != "offset");
        assert_eq!(g.compile_vertex(), Ok(None));
    }

    #[test]
    fn compile_vertex_genera_el_desplazamiento_desde_in_position() {
        let g = grafo_con_olas();
        let glsl = g.compile_vertex().expect("compila").expect("hay vértice");
        // La plantilla es espejo del vértice PBR y desplaza antes de MVP.
        assert!(glsl.contains("vec3 displaced = in_position +"), "{}", glsl);
        assert!(glsl.contains("gl_Position = u_push.mvp * vec4(displaced, 1.0);"), "{}", glsl);
        // Los ejes se leen por componente: X como x y Z como z. Este es el
        // punto donde la coerción genérica (vec→float toma .x) podría
        // enmascarar un eje mal emitido: Z debe aparecer como in_position.z.
        assert!(glsl.contains("in_position.x"), "{}", glsl);
        assert!(glsl.contains("in_position.z"), "{}", glsl);
        // El tiempo llega por el mismo canal que en el fragmento.
        assert!(glsl.contains("u_push.light_color.w"), "{}", glsl);
        // El fragmento del mismo grafo sigue compilando igual que siempre.
        assert!(g.compile_fragment().is_ok());
    }

    #[test]
    fn position_en_fragmento_es_error_explicito() {
        let g = ShaderGraph {
            transparent: false,
            name: "x".into(),
            nodes: vec![
                nodo(1, NodeKind::Position { axis: PositionAxis::X }),
                nodo(2, NodeKind::Output),
            ],
            edges: vec![arista(1, 2, "color")],
        };
        assert_eq!(g.compile_fragment(), Err(GraphError::VertexOnlyNode(1)));
    }

    #[test]
    fn fresnel_en_vertices_es_error_explicito() {
        let g = ShaderGraph {
            transparent: false,
            name: "x".into(),
            nodes: vec![
                nodo(1, NodeKind::Fresnel { power: 2.0 }),
                nodo(2, NodeKind::VertexOutput),
                nodo(3, NodeKind::Color { rgba: [0.0, 0.0, 1.0, 1.0] }),
                nodo(4, NodeKind::Output),
            ],
            edges: vec![arista(1, 2, "offset"), arista(3, 4, "color")],
        };
        assert_eq!(g.compile_vertex(), Err(GraphError::StageUnsupported(1)));
    }

    /// Grafo con el punto de interacción en las DOS etapas: en vértices,
    /// una depresión (1 − smoothstep(−0,5..0,3, DistanceXZ)) como offset
    /// Y; en fragmento, una banda por Distance mezclando azul→blanco.
    fn grafo_con_interaccion() -> ShaderGraph {
        use InteractionComponent as C;
        use NodeKind as K;
        ShaderGraph {
            transparent: false,
            name: "interaccion".into(),
            nodes: vec![
                nodo(1, K::InteractionPoint { component: C::DistanceXZ }),
                nodo(2, K::Smoothstep { edge0: -0.5, edge1: 0.3 }),
                nodo(3, K::OneMinus),
                nodo(4, K::Color { rgba: [0.0, 1.0, 0.0, 1.0] }),
                nodo(5, K::Multiply),
                nodo(6, K::VertexOutput),
                nodo(7, K::InteractionPoint { component: C::Distance }),
                nodo(8, K::Smoothstep { edge0: 0.3, edge1: 0.9 }),
                nodo(9, K::Color { rgba: [1.0, 1.0, 1.0, 1.0] }),
                nodo(10, K::Color { rgba: [0.1, 0.3, 0.6, 1.0] }),
                nodo(11, K::Mix),
                nodo(12, K::Output),
            ],
            edges: vec![
                arista(1, 2, "x"),
                arista(2, 3, "x"),
                arista(3, 5, "a"),
                arista(4, 5, "b"),
                arista(5, 6, "offset"),
                arista(7, 8, "x"),
                arista(10, 11, "a"),
                arista(9, 11, "b"),
                arista(8, 11, "t"),
                arista(11, 12, "color"),
            ],
        }
    }

    #[test]
    fn interaction_point_compila_en_ambas_etapas_con_sus_mascaras() {
        let g = grafo_con_interaccion();

        let frag = g.compile_fragment().expect("fragmento compila");
        // La plantilla declara el canal global (los dos vec4) y las
        // funciones de máscara; el nodo llama a la función 3D con el
        // varying de mundo, y dentro de ella la rama esférica conserva
        // la fórmula de siempre: distancia menos radio (no es la
        // distancia pura: es la distancia con signo a la esfera).
        assert!(frag.contains("layout(set = 1, binding = 0) uniform GraphGlobals"), "{}", frag);
        assert!(frag.contains("vec4 interaction_shape;"), "{}", frag);
        assert!(frag.contains("graph_interaction_distance(v_world_pos)"), "{}", frag);
        assert!(
            frag.contains("distance(world_pos, u_globals.interaction_point.xyz) - u_globals.interaction_point.w"),
            "{}",
            frag
        );
        // La rama de caja: SDF de caja 2D con la rotación deshecha.
        assert!(frag.contains("float graph_sdf_box_xz(vec2 d)"), "{}", frag);
        assert!(frag.contains("abs(local) - u_globals.interaction_shape.xy"), "{}", frag);

        let vert = g
            .compile_vertex()
            .expect("vértices compila")
            .expect("hay etapa de vértices");
        // En vértices la posición de mundo sale de la matriz de modelo y
        // de la posición SIN desplazar, y la máscara cenital es la
        // función XZ del canal (que dentro trabaja con .xz).
        assert!(vert.contains("layout(set = 1, binding = 0) uniform GraphGlobals"), "{}", vert);
        assert!(vert.contains("vec4 interaction_shape;"), "{}", vert);
        assert!(
            vert.contains("graph_interaction_distance_xz((u_push.model * vec4(in_position, 1.0)).xyz)"),
            "{}",
            vert
        );
    }

    #[test]
    fn interaction_point_all_y_radius_emiten_el_vec4_y_su_w() {
        use InteractionComponent as C;
        use NodeKind as K;
        let g = ShaderGraph {
            transparent: false,
            name: "x".into(),
            nodes: vec![
                nodo(1, K::InteractionPoint { component: C::All }),
                nodo(2, K::Output),
                nodo(3, K::InteractionPoint { component: C::Radius }),
                nodo(4, K::VertexOutput),
            ],
            edges: vec![arista(1, 2, "color"), arista(3, 4, "offset")],
        };
        let frag = g.compile_fragment().unwrap();
        assert!(frag.contains("vec4 n1 = u_globals.interaction_point;"), "{}", frag);
        // Radius en la etapa de vértices: el w crudo, sin distancias.
        let vert = g.compile_vertex().unwrap().expect("hay vértice");
        assert!(vert.contains("u_globals.interaction_point.w"), "{}", vert);
    }

    #[test]
    fn sin_nodo_de_interaccion_los_grafos_no_cambian_de_comportamiento() {
        // No-regresión: ningún prefabricado usa el nodo y todos siguen
        // compilando el fragmento y devolviendo None en vértices. La
        // plantilla declara el UBO siempre, pero declararlo no consume el
        // canal: ningún prefabricado emite una lectura de u_globals.
        for g in presets::all() {
            let frag = g.compile_fragment().unwrap();
            assert!(!frag.contains("= u_globals.interaction_point"), "{}", g.name);
            assert_eq!(g.compile_vertex(), Ok(None), "{}", g.name);
        }
        // Y el valor inactivo es el acordado: muy lejos y sin radio.
        assert_eq!(INTERACTION_POINT_INACTIVE, [0.0, -10000.0, 0.0, 0.0]);
        // El canal completo inactivo: ese punto y la forma a cero
        // (esfera, por el discriminador 0).
        assert_eq!(
            INTERACTION_GLOBALS_INACTIVE,
            [0.0, -10000.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn el_json_con_interaccion_es_ida_y_vuelta() {
        let g = grafo_con_interaccion();
        let back = ShaderGraph::from_json(&g.to_json()).expect("json válido");
        assert_eq!(back.compile_fragment().unwrap(), g.compile_fragment().unwrap());
        assert_eq!(back.compile_vertex().unwrap(), g.compile_vertex().unwrap());
    }

    #[test]
    fn el_json_con_etapa_de_vertices_es_ida_y_vuelta() {
        let g = grafo_con_olas();
        let back = ShaderGraph::from_json(&g.to_json()).expect("json válido");
        assert_eq!(back.compile_vertex().unwrap(), g.compile_vertex().unwrap());
        assert_eq!(back.compile_fragment().unwrap(), g.compile_fragment().unwrap());
    }

    fn grafo_ruido(kind: NodeKind) -> ShaderGraph {
        ShaderGraph {
            transparent: false,
            name: "ruido".into(),
            nodes: vec![
                nodo(1, NodeKind::Uv),
                nodo(2, kind),
                nodo(3, NodeKind::Output),
            ],
            edges: vec![
                arista(1, 2, "uv"),
                arista(2, 3, "color"),
            ],
        }
    }

    #[test]
    fn value_noise_emite_la_llamada_y_la_funcion() {
        let g = grafo_ruido(NodeKind::ValueNoise { scale: 8.0 });
        let glsl = g.compile_fragment().expect("compila");
        assert!(glsl.contains("float graph_value_noise(vec2 uv, float scale)"), "{}", glsl);
        assert!(glsl.contains("graph_value_noise(n1, 8.0)"), "{}", glsl);
    }

    #[test]
    fn gradient_noise_emite_la_llamada_y_la_funcion() {
        let g = grafo_ruido(NodeKind::GradientNoise { scale: 5.0 });
        let glsl = g.compile_fragment().expect("compila");
        assert!(glsl.contains("float graph_gradient_noise(vec2 uv, float scale)"), "{}", glsl);
        assert!(glsl.contains("graph_gradient_noise(n1, 5.0)"), "{}", glsl);
    }

    #[test]
    fn fbm_noise_emite_la_llamada_y_la_funcion() {
        let g = grafo_ruido(NodeKind::FbmNoise {
            scale: 4.0,
            octaves: 3,
            lacunarity: 2.0,
            gain: 0.5,
        });
        let glsl = g.compile_fragment().expect("compila");
        assert!(
            glsl.contains("float graph_fbm_noise(vec2 uv, float scale, int octaves, float lacunarity, float gain)"),
            "{}",
            glsl
        );
        assert!(glsl.contains("graph_fbm_noise(n1, 4.0, 3, 2.0, 0.500000)"), "{}", glsl);
    }

    #[test]
    fn dos_nodos_de_ruido_no_duplican_la_funcion_auxiliar() {
        let g = ShaderGraph {
            transparent: false,
            name: "doble_ruido".into(),
            nodes: vec![
                nodo(1, NodeKind::ValueNoise { scale: 8.0 }),
                nodo(2, NodeKind::FbmNoise {
                    scale: 4.0,
                    octaves: 3,
                    lacunarity: 2.0,
                    gain: 0.5,
                }),
                nodo(3, NodeKind::Add),
                nodo(4, NodeKind::Output),
            ],
            edges: vec![
                arista(1, 3, "a"),
                arista(2, 3, "b"),
                arista(3, 4, "color"),
            ],
        };
        let glsl = g.compile_fragment().expect("compila");
        assert_eq!(glsl.matches("float graph_value_noise(").count(), 1, "{}", glsl);
        assert_eq!(glsl.matches("float graph_fbm_noise(").count(), 1, "{}", glsl);
        assert_eq!(glsl.matches("float graph_hash21(").count(), 1, "{}", glsl);
    }

    #[test]
    fn el_ruido_tambien_compila_en_la_etapa_de_vertices() {
        let g = ShaderGraph {
            transparent: false,
            name: "ruido_vertices".into(),
            nodes: vec![
                nodo(1, NodeKind::ValueNoise { scale: 8.0 }),
                nodo(2, NodeKind::VertexOutput),
            ],
            edges: vec![arista(1, 2, "offset")],
        };
        let vert = g.compile_vertex().expect("compila").expect("hay vértice");
        assert!(vert.contains("graph_value_noise(in_uv, 8.0)"), "{}", vert);
    }

    #[test]
    fn subtract_y_divide_emiten_sus_operadores() {
        let g = ShaderGraph {
            transparent: false,
            name: "aritmetica".into(),
            nodes: vec![
                nodo(1, NodeKind::Uv),
                nodo(2, NodeKind::Float { value: 0.5 }),
                nodo(3, NodeKind::Subtract),
                nodo(4, NodeKind::Divide),
                nodo(5, NodeKind::Float { value: 2.0 }),
                nodo(6, NodeKind::Output),
            ],
            edges: vec![
                arista(1, 3, "a"),
                arista(2, 3, "b"),
                arista(3, 4, "a"),
                arista(5, 4, "b"),
                arista(4, 6, "color"),
            ],
        };
        let glsl = g.compile_fragment().expect("compila");
        assert!(glsl.contains("vec2 n3 = (n1 - vec2(n2));"), "{glsl}");
        assert!(glsl.contains("vec2 n4 = (n3 / vec2(n5));"), "{glsl}");
    }

    #[test]
    fn los_canales_extraen_con_relleno_seguro() {
        let g = ShaderGraph {
            transparent: false,
            name: "canales".into(),
            nodes: vec![
                nodo(1, NodeKind::Uv),
                nodo(2, NodeKind::Normal),
                nodo(3, NodeKind::ChannelB),
                nodo(4, NodeKind::ChannelA),
                nodo(5, NodeKind::ChannelR),
                nodo(6, NodeKind::ChannelG),
                nodo(7, NodeKind::Combine),
                nodo(8, NodeKind::Output),
            ],
            edges: vec![
                arista(1, 3, "x"),
                arista(2, 4, "x"),
                arista(1, 5, "x"),
                arista(1, 6, "x"),
                arista(3, 7, "r"),
                arista(4, 7, "g"),
                arista(5, 7, "b"),
                arista(6, 7, "a"),
                arista(7, 8, "color"),
            ],
        };
        let glsl = g.compile_fragment().expect("compila");
        assert!(glsl.contains("float n3 = 0.0;"), "{glsl}");
        assert!(glsl.contains("float n4 = 1.0;"), "{glsl}");
        assert!(glsl.contains("float n5 = (n1).x;"), "{glsl}");
        assert!(glsl.contains("float n6 = (n1).y;"), "{glsl}");
    }

    #[test]
    fn combine_usa_alfa_uno_si_no_se_conecta() {
        let g = ShaderGraph {
            transparent: false,
            name: "combine".into(),
            nodes: vec![
                nodo(1, NodeKind::Float { value: 0.1 }),
                nodo(2, NodeKind::Float { value: 0.2 }),
                nodo(3, NodeKind::Float { value: 0.3 }),
                nodo(4, NodeKind::Combine),
                nodo(5, NodeKind::Output),
            ],
            edges: vec![
                arista(1, 4, "r"),
                arista(2, 4, "g"),
                arista(3, 4, "b"),
                arista(4, 5, "color"),
            ],
        };
        let glsl = g.compile_fragment().expect("compila");
        assert!(glsl.contains("vec4 n4 = vec4(n1, n2, n3, 1.0);"), "{glsl}");
    }

    #[test]
    fn dot_length_distance_y_normalize_compilan() {
        let g = ShaderGraph {
            transparent: false,
            name: "vectorial".into(),
            nodes: vec![
                nodo(1, NodeKind::Uv),
                nodo(2, NodeKind::Float { value: 0.5 }),
                nodo(3, NodeKind::Subtract),
                nodo(4, NodeKind::Length),
                nodo(5, NodeKind::Dot),
                nodo(6, NodeKind::Float { value: 0.0 }),
                nodo(7, NodeKind::Distance),
                nodo(8, NodeKind::Normalize),
                nodo(9, NodeKind::ChannelR),
                nodo(10, NodeKind::Combine),
                nodo(11, NodeKind::Output),
            ],
            edges: vec![
                arista(1, 3, "a"),
                arista(2, 3, "b"),
                arista(3, 4, "x"),
                arista(3, 5, "a"),
                arista(3, 5, "b"),
                arista(3, 7, "a"),
                arista(6, 7, "b"),
                arista(3, 8, "x"),
                arista(8, 9, "x"),
                arista(4, 10, "r"),
                arista(5, 10, "g"),
                arista(7, 10, "b"),
                arista(9, 10, "a"),
                arista(10, 11, "color"),
            ],
        };
        let glsl = g.compile_fragment().expect("compila");
        assert!(glsl.contains("float n4 = length(n3);"), "{glsl}");
        assert!(glsl.contains("float n5 = dot(n3, n3);"), "{glsl}");
        assert!(glsl.contains("float n7 = distance(n3, vec2(n6));"), "{glsl}");
        assert!(glsl.contains("vec2 n8 = normalize(n3);"), "{glsl}");
    }

    #[test]
    fn clamp_power_step_y_modulo_emiten_sus_funciones() {
        let g = ShaderGraph {
            transparent: false,
            name: "propiedades".into(),
            nodes: vec![
                nodo(1, NodeKind::Float { value: 2.0 }),
                nodo(2, NodeKind::Clamp { min: 0.0, max: 1.0 }),
                nodo(3, NodeKind::Power { exponent: 2.0 }),
                nodo(4, NodeKind::Step { edge: 0.5 }),
                nodo(5, NodeKind::Float { value: 1.5 }),
                nodo(6, NodeKind::Float { value: 1.0 }),
                nodo(7, NodeKind::Modulo),
                nodo(8, NodeKind::Combine),
                nodo(9, NodeKind::Output),
            ],
            edges: vec![
                arista(1, 2, "x"),
                arista(1, 3, "x"),
                arista(1, 4, "x"),
                arista(5, 7, "x"),
                arista(6, 7, "y"),
                arista(2, 8, "r"),
                arista(3, 8, "g"),
                arista(4, 8, "b"),
                arista(7, 8, "a"),
                arista(8, 9, "color"),
            ],
        };
        let glsl = g.compile_fragment().expect("compila");
        assert!(glsl.contains("float n2 = clamp(n1, 0.0, 1.0);"), "{glsl}");
        assert!(glsl.contains("float n3 = pow(n1, 2.0);"), "{glsl}");
        assert!(glsl.contains("float n4 = step(0.500000, n1);"), "{glsl}");
        assert!(glsl.contains("float n7 = mod(n5, n6);"), "{glsl}");
    }

    #[test]
    fn el_patron_radial_se_construye_con_uv_float_subtract_y_length() {
        let g = ShaderGraph {
            transparent: false,
            name: "radial_piezas".into(),
            nodes: vec![
                nodo(1, NodeKind::Uv),
                nodo(2, NodeKind::Float { value: 0.5 }),
                nodo(3, NodeKind::Subtract),
                nodo(4, NodeKind::Length),
                nodo(5, NodeKind::Output),
            ],
            edges: vec![
                arista(1, 3, "a"),
                arista(2, 3, "b"),
                arista(3, 4, "x"),
                arista(4, 5, "color"),
            ],
        };
        let glsl = g.compile_fragment().expect("compila");
        assert!(glsl.contains("vec2 n3 = (n1 - vec2(n2));"), "{glsl}");
        assert!(glsl.contains("float n4 = length(n3);"), "{glsl}");
        assert!(glsl.contains("out_color = vec4(vec3(n4), 1.0);"), "{glsl}");
    }
}
