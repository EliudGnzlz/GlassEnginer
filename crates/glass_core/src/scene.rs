use serde::{Deserialize, Serialize};

/// Profundidad máxima de jerarquía que el motor recorre al componer matrices de
/// mundo. `reparent` ya rechaza ciclos, así que este tope es una red de
/// seguridad contra escenas manipuladas a mano o corrompidas: sin él, una
/// cadena cíclica colgaría el hilo de renderizado.
pub const MAX_HIERARCHY_DEPTH: usize = 256;


/// Transform de una entidad, con la orientación almacenada de forma
/// redundante: un cuaternión autoritativo y una vista Euler en grados.
///
/// Ambas representaciones son **privadas** a propósito. Si fueran campos
/// públicos, cualquiera podría escribir el Euler y dejar el cuaternión
/// desactualizado (o al revés), y el motor tendría dos verdades contradictorias
/// sobre la orientación. Todo acceso pasa por los métodos, que mantienen la
/// invariante: al tocar una representación se recalcula la otra.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformData {
    pub position: [f32; 3],
    pub scale: [f32; 3],

    /// Vista Euler en grados. Derivada; se refresca en cada escritura.
    rotation: [f32; 3],

    /// Cuaternión `[i, j, k, w]` autoritativo de la orientación.
    ///
    /// Es la fuente de verdad porque la física (Rapier3D) y los scripts C#
    /// trabajan en cuaterniones: convertir a Euler en cada ida y vuelta perdía
    /// precisión y saltaba de rama en gimbal lock. `#[serde(default)]` mantiene
    /// compatibilidad con las escenas `.scene.json` escritas antes de este campo.
    #[serde(default)]
    rotation_quat: [f32; 4],
}

impl Default for TransformData {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            rotation: [0.0, 0.0, 0.0],
            rotation_quat: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

impl TransformData {
    /// Vista Euler en grados, para el Inspector y los gizmos.
    pub fn rotation(&self) -> [f32; 3] {
        self.rotation
    }

    /// Cuaternión autoritativo `[i, j, k, w]`.
    ///
    /// Si la escena vino de disco sin el campo (versión anterior), se deriva del
    /// Euler en lugar de devolver el cuaternión cero, que no es una rotación.
    pub fn rotation_quat(&self) -> [f32; 4] {
        if self.rotation_quat == [0.0, 0.0, 0.0, 0.0] {
            crate::math::euler_deg_to_quat(self.rotation)
        } else {
            self.rotation_quat
        }
    }

    /// Fija la orientación desde un cuaternión y refresca la vista Euler.
    pub fn set_rotation_quat(&mut self, quat: [f32; 4]) {
        let q = crate::math::quat_normalize(quat);
        self.rotation_quat = q;
        self.rotation = crate::math::quat_to_euler_deg(q);
    }

    /// Fija la orientación desde la vista Euler y refresca el cuaternión.
    ///
    /// Es la ruta del Inspector y de los gizmos: el usuario razona en grados.
    pub fn set_rotation_euler(&mut self, euler_deg: [f32; 3]) {
        self.rotation = euler_deg;
        self.rotation_quat = crate::math::euler_deg_to_quat(euler_deg);
    }

    /// Escribe una sola componente de la rotación Euler, recalculando el
    /// cuaternión. Evita que un `rotation[1] += delta` deje el cuaternión viejo.
    pub fn add_rotation_euler(&mut self, delta: [f32; 3]) {
        self.set_rotation_euler([
            self.rotation[0] + delta[0],
            self.rotation[1] + delta[1],
            self.rotation[2] + delta[2],
        ]);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshData {
    pub color: [f32; 3],
    pub wireframe: bool,
    pub shader: String,
    /// Nombre del modelo en el AssetCache (la clave es el nombre del .glb
    /// sin extensión, p. ej. "cube"). Vacío = la malla por defecto ("cube").
    /// Antes este campo no existía: el bucle de render dibujaba el cubo para
    /// TODAS las entidades y los modelos importados nunca llegaban a pantalla.
    #[serde(default)]
    pub model: String,
/// Nombre del activo de material en `assets/materials/`
    /// (sin extensión; "" = sin material asignado: mandan el
    /// color de la entidad y los valores históricos).
    #[serde(default)]
    pub material: String,
    /// Valores de las propiedades del shader de grafo de esta entidad
    /// (solo tienen efecto si `shader` es "graph:<nombre>" y el grafo
    /// declara esas propiedades; ver glass_core::shader_graph).
    #[serde(default)]
    pub shader_properties: std::collections::HashMap<String, serde_json::Value>,
}

impl Default for MeshData {
    fn default() -> Self {
        Self {
            color: [0.0, 0.8, 0.9],
            wireframe: false,
            model: String::new(),
            shader: "triangle.vert.spv".into(),
            material: String::new(),
            shader_properties: Default::default(),
        }
    }
}

/// Default de `PhysicsData::collision_mask`: todas las capas.
fn default_collision_mask() -> u32 {
    u32::MAX
}

/// Un collider primitivo compuesto de la caja de herramientas (Fase 1.5):
/// una entidad puede llevar varios, cada uno con su desplazamiento local
/// respecto al cuerpo. La capa y la máscara salen del PhysicsData de la
/// entidad; el trigger es por collider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColliderSpec {
    /// "Box" | "Sphere" | "Capsule" | "Cylinder" (otro valor = Box).
    pub shape: String,
    /// Box: half_extents · Sphere: [radio, 0, 0] ·
    /// Capsule/Cylinder: [half_height, radio, 0].
    pub size: [f32; 3],
    /// Desplazamiento local del collider respecto al origen del cuerpo.
    #[serde(default)]
    pub offset: [f32; 3],
    /// true = este collider es sensor (los demás pueden ser sólidos).
    #[serde(default)]
    pub is_trigger: bool,
}

/// Modo de collider de malla (Fase 1.5): None = camino histórico (primitiva
/// inferida o lista `colliders`); Convex = casco convexo de la geometría
/// del modelo; TriMesh = malla triangular exacta (SOLO estáticos: en un
/// cuerpo no estático la app lo degrada a Convex, como en Unity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MeshColliderMode {
    #[default]
    None,
    Convex,
    TriMesh,
}

// Dentro de PhysicsData, tras is_trigger:

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsData {
    pub body_type: String, // "Dynamic", "Static", "Kinematic"
    pub mass: f32,
    pub friction: f32,
    pub restitution: f32,
    pub box_extents: [f32; 3],
    pub linear_velocity: [f32; 3],
    /// Capa de colisión (0..31): el collider pertenece a esta capa y
    /// colisiona según su máscara. Fase 1.
    #[serde(default)]
    pub layer: u8,
    /// Máscara de colisión: capas contra las que colisiona (bit a bit).
    /// Default u32::MAX (todas) vía función porque el default del tipo es 0.
    #[serde(default = "default_collision_mask")]
    pub collision_mask: u32,
    /// Si es true, el collider es sensor (trigger): detecta sin bloquear.
    #[serde(default)]
    pub is_trigger: bool,
    /// Colliders primitivos compuestos (Fase 1.5). Vacío = camino
    /// histórico de un solo collider inferido de la malla.
    #[serde(default)]
    pub colliders: Vec<ColliderSpec>,
    /// Collider de malla sobre la geometría del modelo (Fase 1.5).
    #[serde(default)]
    pub mesh_collider: MeshColliderMode,
}

impl Default for PhysicsData {
    fn default() -> Self {
        Self {
            body_type: "Dynamic".into(),
            mass: 1.0,
            friction: 0.7,
            restitution: 0.05,
            box_extents: [0.5, 0.5, 0.5],
            linear_velocity: [0.0, 0.0, 0.0],
            layer: 0,
            collision_mask: u32::MAX,
            is_trigger: false,
            colliders: Vec::new(),
            mesh_collider: MeshColliderMode::None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptComponentData {
    pub class_name: String,
    /// Con `#[serde(default)]`: una escena cuyo script_data solo trae
    /// class_name debe cargar igual (antes tumbaba la escena entera).
    #[serde(default = "default_move_speed")]
    pub move_speed: f32,
    #[serde(default = "default_jump_velocity")]
    pub jump_velocity: f32,
    #[serde(default)]
    pub auto_demo: bool,
}

fn default_move_speed() -> f32 {
    3.8
}

fn default_jump_velocity() -> f32 {
    7.2
}

impl Default for ScriptComponentData {
    fn default() -> Self {
        Self {
            class_name: "GlassEngine.Core.Scripts.PlayerController".into(),
            move_speed: 3.8,
            jump_velocity: 7.2,
            auto_demo: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSourceData {
    pub clip_name: String,
    pub volume: f32,
    pub pitch: f32,
    pub is_playing: bool,
    /// Bus de audio de la fuente (Fase 3): "master" | "music" | "sfx".
    #[serde(default = "default_audio_bus")]
    pub bus: String,
}

fn default_audio_bus() -> String {
    "sfx".to_string()
}

impl Default for AudioSourceData {
    fn default() -> Self {
        Self {
            clip_name: "beep_440".into(),
            volume: 1.0,
            pitch: 1.0,
            is_playing: false,
            bus: "sfx".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraData {
    pub fov: f32,
    pub near_clip: f32,
    pub far_clip: f32,
    pub is_main: bool,
}

impl Default for CameraData {
    fn default() -> Self {
        Self {
            fov: 60.0,
            near_clip: 0.1,
            far_clip: 1000.0,
            is_main: true,
        }
    }
}

/// Niebla analítica lineal de la escena (Fase 2): vive en una entidad
/// (normalmente la cámara o una entidad «Environment»); la primera
/// entidad con este bloque manda. Presencia = niebla activa.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FogData {
    /// Color de la niebla en RGB lineal.
    pub color: [f32; 3],
    /// Distancia (m) a la que empieza la niebla.
    pub near: f32,
    /// Distancia (m) a la que la niebla es total.
    pub far: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightData {
    pub light_type: String, // "Directional", "Point", "Spot"
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    /// Si esta luz puntual proyecta sombra (Fase 2): el motor le asigna
    /// uno de los 4 cubos del atlas compartido por orden de cercanía.
    #[serde(default)]
    pub cast_shadow: bool,
}

impl Default for LightData {
    fn default() -> Self {
        Self {
            light_type: "Directional".into(),
            color: [1.0, 0.98, 0.92],
            intensity: 1.0,
            range: 10.0,
            cast_shadow: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasData {
    pub render_mode: String, // "ScreenSpaceOverlay", "ScreenSpaceCamera", "WorldSpace"
    pub reference_resolution: [f32; 2],
    pub match_width_or_height: f32,
}

impl Default for CanvasData {
    fn default() -> Self {
        Self {
            render_mode: "ScreenSpaceOverlay".into(),
            reference_resolution: [1920.0, 1080.0],
            match_width_or_height: 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RectTransformData {
    pub anchored_position: [f32; 2],
    pub size_delta: [f32; 2],
    pub anchor_min: [f32; 2],
    pub anchor_max: [f32; 2],
    pub pivot: [f32; 2],
}

/// Grupo de layout (Fase 5): recoloca a los hijos UI directos de la
/// entidad en fila o columna dentro de su rect, estilo Unity
/// (Horizontal/VerticalLayoutGroup reducido: sin Control Size — cada
/// hijo conserva su tamaño — y con padding y alineación del bloque).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutGroupData {
    /// "horizontal" o "vertical".
    #[serde(default = "default_layout_direction")]
    pub direction: String,
    /// Separación entre hijos (px de referencia).
    #[serde(default)]
    pub spacing: f32,
    /// Márgenes interiores [izquierda, arriba, derecha, abajo].
    #[serde(default)]
    pub padding: [f32; 4],
    /// Alineación del bloque: "start", "center" o "end".
    #[serde(default = "default_layout_alignment")]
    pub alignment: String,
}

fn default_layout_direction() -> String {
    "horizontal".into()
}

fn default_layout_alignment() -> String {
    "start".into()
}

impl Default for LayoutGroupData {
    fn default() -> Self {
        Self {
            direction: "horizontal".into(),
            spacing: 8.0,
            padding: [0.0, 0.0, 0.0, 0.0],
            alignment: "start".into(),
        }
    }
}

/// Recoloca los rects de los hijos dentro del rect del grupo
/// (Fase 5): eje principal en orden, con el espaciado y el bloque
/// alineado; eje cruzado alineado por hijo. Los tamaños no cambian.
/// Función pura.
pub fn layout_group_rects(
    group_rect: [f32; 4],
    group: &LayoutGroupData,
    children: &[[f32; 4]],
) -> Vec<[f32; 4]> {
    if children.is_empty() {
        return Vec::new();
    }
    let inner = [
        group_rect[0] + group.padding[0],
        group_rect[1] + group.padding[1],
        (group_rect[2] - group.padding[0] - group.padding[2]).max(0.0),
        (group_rect[3] - group.padding[1] - group.padding[3]).max(0.0),
    ];
    let horizontal = group.direction != "vertical";
    let main = |r: &[f32; 4]| if horizontal { r[2] } else { r[3] };
    let total: f32 = children.iter().map(main).sum::<f32>()
        + group.spacing * (children.len() as f32 - 1.0);
    let inner_main = if horizontal { inner[2] } else { inner[3] };
    let bloque = match group.alignment.as_str() {
        "center" => (inner_main - total) * 0.5,
        "end" => inner_main - total,
        _ => 0.0,
    };
    let mut cursor = (if horizontal { inner[0] } else { inner[1] }) + bloque;
    let mut out = Vec::with_capacity(children.len());
    for r in children {
        let cross_size = if horizontal { r[3] } else { r[2] };
        let inner_cross = if horizontal { inner[3] } else { inner[2] };
        let off_cross = match group.alignment.as_str() {
            "center" => (inner_cross - cross_size) * 0.5,
            "end" => inner_cross - cross_size,
            _ => 0.0,
        };
        let mut nr = *r;
        if horizontal {
            nr[0] = cursor;
            nr[1] = inner[1] + off_cross;
            cursor += r[2] + group.spacing;
        } else {
            nr[1] = cursor;
            nr[0] = inner[0] + off_cross;
            cursor += r[3] + group.spacing;
        }
        out.push(nr);
    }
    out
}

impl Default for RectTransformData {
    fn default() -> Self {
        Self {
            anchored_position: [0.0, 0.0],
            size_delta: [160.0, 40.0],
            anchor_min: [0.5, 0.5],
            anchor_max: [0.5, 0.5],
            pivot: [0.5, 0.5],
        }
    }
}

/// Fuente por defecto de un texto de UI (el DejaVu incrustado).
fn default_ui_font() -> String {
    "default".into()
}

/// Alineación por defecto de un texto de UI.
fn default_ui_align() -> String {
    "Left".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UITextData {
    pub text: String,
    pub font_size: f32,
    pub color: [f32; 4],
    /// Nombre de la fuente: un .ttf de assets/fonts/ sin extensión,
    /// o "default" (el DejaVu incrustado). Las escenas antiguas no
    /// traen el campo: el default serde lo rellena.
    #[serde(default = "default_ui_font")]
    pub font: String,
    /// "Left" | "Center" | "Right".
    #[serde(default = "default_ui_align")]
    pub align: String,
}

impl Default for UITextData {
    fn default() -> Self {
        Self {
            text: "Texto UI".into(),
            font_size: 18.0,
            color: [1.0, 1.0, 1.0, 1.0],
            font: "default".into(),
            align: "Left".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIButtonData {
    pub interactable: bool,
    pub target_graphic_color: [f32; 4],
    pub on_click_action: String,
}

impl Default for UIButtonData {
    fn default() -> Self {
        Self {
            interactable: true,
            target_graphic_color: [0.25, 0.55, 0.85, 1.0],
            on_click_action: "".into(),
        }
    }
}

/// Elemento Imagen del Canvas (v2): muestra un asset de imagen
/// dentro de su RectTransform, con tinte multiplicativo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIImageData {
    /// Ruta del asset relativa a assets/, p. ej.
    /// "textures/icono.png"; vacío = sin imagen (no se dibuja).
    pub sprite: String,
    pub color: [f32; 4],
    /// true = encajar la imagen conservando su proporción dentro
    /// del rect (letterbox); false = estirar al rect completo.
    pub preserve_aspect: bool,
}

impl Default for UIImageData {
    fn default() -> Self {
        Self {
            sprite: String::new(),
            color: [1.0, 1.0, 1.0, 1.0],
            preserve_aspect: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIDragData {
    /// true = el elemento se puede arrastrar con el ratón.
    pub draggable: bool,
    /// true = el elemento recibe sueltas (casillas de inventario).
    pub accepts_drop: bool,
    /// Método del script de ESTA entidad al empezar el arrastre.
    pub on_begin_drag: String,
    /// Método del script de ESTA entidad al terminar SU arrastre
    /// (se haya soltado sobre un destino o no).
    pub on_drag_end: String,
    /// Método del script de ESTA entidad cuando algo se suelta
    /// sobre ella (solo si accepts_drop).
    pub on_drop: String,
}

impl Default for UIDragData {
    fn default() -> Self {
        Self {
            draggable: false,
            accepts_drop: false,
            on_begin_drag: String::new(),
            on_drag_end: String::new(),
            on_drop: String::new(),
        }
    }
}

/// Activo de material estilo Unity (Canvas no: materiales 3D,
/// bloque «Materiales v1»): vive en
/// `assets/materials/<nombre>.material.json` y una entidad lo
/// referencia con `MeshData.material`. Sustituye por completo a
/// los parámetros por defecto (y a los del glTF) de la entidad.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialData {
    /// Color albedo (rgba); la componente a es la opacidad.
    #[serde(default = "default_material_albedo")]
    pub albedo: [f32; 4],
    /// Textura base: ruta relativa a `assets/` ("" = sin textura,
    /// se muestrea el blanco 1×1).
    #[serde(default)]
    pub texture: String,
    #[serde(default)]
    pub metallic: f32,
    /// 0.45 es el histórico del motor cuando no hay material.
    #[serde(default = "default_material_roughness")]
    pub roughness: f32,
    /// Color emisivo (rgb); se suma tal cual × intensidad (HDR:
    /// con intensidad > 1 alimenta el bloom del post-procesado).
    #[serde(default)]
    pub emissive: [f32; 3],
    #[serde(default)]
    pub emissive_intensity: f32,
    /// Repeticiones de la UV (tiling de Unity).
    #[serde(default = "default_material_tiling")]
    pub tiling: [f32; 2],
    /// Desplazamiento de la UV.
    #[serde(default)]
    pub offset: [f32; 2],
    /// true = pipeline transparente (mezcla por alfa, sin
    /// escritura de profundidad).
    #[serde(default)]
    pub transparent: bool,
}

/// Albedo por defecto: blanco opaco.
fn default_material_albedo() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

/// Rugosidad histórica del motor (0.45).
fn default_material_roughness() -> f32 {
    0.45
}

/// Tiling por defecto: sin repetición.
fn default_material_tiling() -> [f32; 2] {
    [1.0, 1.0]
}

impl Default for MaterialData {
    fn default() -> Self {
        // Coherente con las funciones `default_*`: no usar
        // `#[derive(Default)]` porque `roughness` y `tiling`
        // no son ceros.
        Self {
            albedo: default_material_albedo(),
            texture: String::new(),
            metallic: 0.0,
            roughness: default_material_roughness(),
            emissive: [0.0, 0.0, 0.0],
            emissive_intensity: 0.0,
            tiling: default_material_tiling(),
            offset: [0.0, 0.0],
            transparent: false,
        }
    }
}

/// Componente de animación de una entidad con modelo con piel (skinning).
/// Es deliberadamente mínimo: qué clip suena, a qué velocidad y si está
/// sonando. El modelo sale de `MeshData.model` y los clips, del .glb
/// importado; el tiempo del clip único NO vive aquí (es estado de runtime
/// de la app), pero la máquina de estados sí guarda el suyo en campos
/// `#[serde(skip)]` (nunca se serializan). Un `clip` vacío significa
/// «el primer clip del modelo». Los scripts lo controlan con
/// `Animator.Play("nombre")` (puente `animator_play`), como en Unity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimatorData {
    pub clip: String,
    pub speed: f32,
    pub playing: bool,

    /// Nombre del controlador en `assets/animators/`. Vacío = sin controlador
    /// (comportamiento clásico de un único clip; camino intacto).
    #[serde(default)]
    pub controller: String,

    // --- Estado de runtime de la máquina (NO se serializa) ---
    /// Estado actual del controlador.
    #[serde(skip)]
    pub current_state: String,
    /// Tiempo acumulado (s) dentro del estado actual.
    #[serde(skip)]
    pub state_time: f32,
    /// Estado origen de un fundido en curso, si lo hay.
    #[serde(skip)]
    pub fade_from_state: Option<String>,
    /// Tiempo del estado origen durante el fundido.
    #[serde(skip)]
    pub fade_from_time: f32,
    /// Duración total del fundido (s).
    #[serde(skip)]
    pub fade_duration: f32,
    /// Tiempo transcurrido del fundido (s).
    #[serde(skip)]
    pub fade_elapsed: f32,
    /// Valores de los parámetros (bool/trigger como 0..1).
    #[serde(skip)]
    pub params: std::collections::HashMap<String, f32>,
    /// Petición pendiente de salto de estado (Play/CrossFade con controlador):
    /// (nombre_estado, duración_fundido). La consume `advance`.
    #[serde(skip)]
    pub pending_play: Option<(String, f32)>,
    /// Petición pendiente de CrossFade libre sin controlador:
    /// (clip_destino, duración). La consume el bucle de dibujo.
    #[serde(skip)]
    pub pending_free_crossfade: Option<(String, f32)>,
    /// Relojes por clip de los hijos del blend tree del estado actual
    /// (Fase 4): clave = nombre de clip.
    #[serde(skip)]
    pub blend_times: std::collections::HashMap<String, f32>,
    /// Relojes de los hijos del blend tree del estado origen durante
    /// un fundido (Fase 4).
    #[serde(skip)]
    pub fade_blend_times: std::collections::HashMap<String, f32>,
}

impl Default for AnimatorData {
    fn default() -> Self {
        Self {
            clip: String::new(),
            speed: 1.0,
            playing: true,
            controller: String::new(),
            current_state: String::new(),
            state_time: 0.0,
            fade_from_state: None,
            fade_from_time: 0.0,
            fade_duration: 0.0,
            fade_elapsed: 0.0,
            params: std::collections::HashMap::new(),
            pending_play: None,
            pending_free_crossfade: None,
            blend_times: std::collections::HashMap::new(),
            fade_blend_times: std::collections::HashMap::new(),
        }
    }
}

/// Resuelve un `RectTransformData` a píxeles de pantalla: devuelve
/// `[x, y, ancho, alto]` con el origen arriba a la izquierda e Y hacia
/// abajo, para una pantalla de `screen_w` × `screen_h` píxeles.
///
/// Es LA MISMA fórmula que usa la vista de juego del editor (egui) en
/// `render_in_game_canvas_ui`, para que el Canvas se vea idéntico dentro
/// del editor y en el juego standalone: el ancla se interpreta en
/// convención Unity (Y hacia arriba, de ahí el `1.0 - anchor_min[1]`),
/// la posición usa el `size_delta` sin clamar y el tamaño dibujado se
/// clampa a un mínimo de 10 px por eje, igual que allí.
///
/// No compone jerarquías: cada entidad se resuelve contra la pantalla
/// completa, que es también lo que hace hoy el editor.
/// Resuelve el rectángulo de un RectTransform dentro de un rect
/// PADRE (en píxeles, origen arriba-izquierda, Y hacia abajo), con
/// anclas completas (Fase 5): el span entre anchor_min y anchor_max
/// se estira con el padre y `size_delta` se suma al span. La
/// posición se calcula con el tamaño sin clamar y el tamaño de
/// salida se clampa a ≥10 px, como en la fórmula histórica.
pub fn resolve_ui_rect_in(rt: &RectTransformData, parent: [f32; 4]) -> [f32; 4] {
    let ax0 = parent[0] + parent[2] * rt.anchor_min[0];
    let ax1 = parent[0] + parent[2] * rt.anchor_max[0];
    let ay0 = parent[1] + parent[3] * (1.0 - rt.anchor_max[1]);
    let ay1 = parent[1] + parent[3] * (1.0 - rt.anchor_min[1]);
    let w = (ax1 - ax0) + rt.size_delta[0];
    let h = (ay1 - ay0) + rt.size_delta[1];
    let px = ax0 + (ax1 - ax0) * rt.pivot[0] + rt.anchored_position[0];
    let py = ay0 + (ay1 - ay0) * rt.pivot[1] - rt.anchored_position[1];
    let x = px - w * rt.pivot[0];
    let y = py - h * (1.0 - rt.pivot[1]);
    [x, y, w.max(10.0), h.max(10.0)]
}

/// Resuelve contra la pantalla completa (la fórmula histórica: un
/// envoltorio de `resolve_ui_rect_in` con el padre = pantalla; con
/// anclas puntuales da exactamente lo mismo que siempre).
pub fn resolve_ui_rect(rt: &RectTransformData, screen_w: f32, screen_h: f32) -> [f32; 4] {
    resolve_ui_rect_in(rt, [0.0, 0.0, screen_w, screen_h])
}

/// Resuelve los rectángulos EN PÍXELES de todas las entidades UI de
/// la escena (Fase 5): aplica el Canvas Scaler del primer Canvas,
/// compone la jerarquía (cada hijo contra el rect de su ancestro UI
/// más cercano) y aplica los layout groups por profundidad
/// ascendente. Las entidades sin `rect_transform` no aparecen.
pub fn resolve_scene_ui_rects(
    scene: &SceneState,
    screen_w: f32,
    screen_h: f32,
) -> std::collections::HashMap<u64, [f32; 4]> {
    let scale = scene
        .entities
        .iter()
        .find_map(|e| e.canvas.as_ref())
        .map(|c| canvas_scale_factor(c, screen_w, screen_h))
        .unwrap_or(1.0);
    let root: [f32; 4] = [0.0, 0.0, screen_w / scale, screen_h / scale];
    let by_id: std::collections::HashMap<u64, &EntityData> =
        scene.entities.iter().map(|e| (e.id, e)).collect();

    fn resolver(
        ent: &EntityData,
        by_id: &std::collections::HashMap<u64, &EntityData>,
        root: [f32; 4],
        memo: &mut std::collections::HashMap<u64, [f32; 4]>,
        visitando: &mut Vec<u64>,
    ) -> [f32; 4] {
        if let Some(r) = memo.get(&ent.id) {
            return *r;
        }
        // El Canvas ES el espacio raíz (como en Unity: su
        // RectTransform es la pantalla en unidades de referencia,
        // no su rect autorado).
        if ent.canvas.is_some() {
            memo.insert(ent.id, root);
            return root;
        }
        if visitando.contains(&ent.id) {
            return root; // ciclo en la jerarquía: se corta contra la raíz
        }
        visitando.push(ent.id);
        // Padre: el ancestro UI más cercano (con rect_transform o Canvas).
        let mut padre_rect = root;
        let mut cur = ent.parent;
        while let Some(pid) = cur {
            let Some(p) = by_id.get(&pid) else { break };
            if p.rect_transform.is_some() || p.canvas.is_some() {
                if p.rect_transform.is_some() {
                    padre_rect = resolver(p, by_id, root, memo, visitando);
                }
                break;
            }
            cur = p.parent;
        }
        let r = resolve_ui_rect_in(ent.rect_transform.as_ref().unwrap(), padre_rect);
        visitando.pop();
        memo.insert(ent.id, r);
        r
    }

    let mut memo: std::collections::HashMap<u64, [f32; 4]> =
        std::collections::HashMap::new();
    for ent in scene
        .entities
        .iter()
        .filter(|e| e.rect_transform.is_some())
    {
        let mut visitando = Vec::new();
        resolver(ent, &by_id, root, &mut memo, &mut visitando);
    }

    // Layout groups por profundidad ascendente (los exteriores primero).
    fn profundidad(ent: &EntityData, by_id: &std::collections::HashMap<u64, &EntityData>) -> u32 {
        let mut d = 0;
        let mut cur = ent.parent;
        while let Some(pid) = cur {
            d += 1;
            match by_id.get(&pid) {
                Some(p) => cur = p.parent,
                None => break,
            }
            if d > 64 {
                break;
            }
        }
        d
    }
    let mut grupos: Vec<&EntityData> = scene
        .entities
        .iter()
        .filter(|e| e.layout_group.is_some() && memo.contains_key(&e.id))
        .collect();
    grupos.sort_by_key(|e| profundidad(e, &by_id));
    for g in grupos {
        let grect = memo[&g.id];
        let hijos: Vec<u64> = scene
            .entities
            .iter()
            .filter(|e| e.parent == Some(g.id) && memo.contains_key(&e.id))
            .map(|e| e.id)
            .collect();
        if hijos.is_empty() {
            continue;
        }
        let rects: Vec<[f32; 4]> = hijos.iter().map(|id| memo[id]).collect();
        let nuevos = layout_group_rects(grect, g.layout_group.as_ref().unwrap(), &rects);
        for (id, nr) in hijos.iter().zip(nuevos) {
            memo.insert(*id, nr);
        }
    }

    // A píxeles.
    memo.values_mut().for_each(|r| {
        r[0] *= scale;
        r[1] *= scale;
        r[2] *= scale;
        r[3] *= scale;
    });
    memo
}

/// Factor de escala del Canvas Scaler (Fase 5, modo «Scale With
/// Screen Size» de Unity): interpola logarítmicamente entre el
/// factor por ancho y el factor por alto según
/// `match_width_or_height` (0 = manda el ancho, 1 = manda el alto).
/// Referencia o pantalla inválidas → 1.
pub fn canvas_scale_factor(canvas: &CanvasData, screen_w: f32, screen_h: f32) -> f32 {
    let ref_w = canvas.reference_resolution[0];
    let ref_h = canvas.reference_resolution[1];
    if ref_w <= 0.0 || ref_h <= 0.0 || screen_w <= 0.0 || screen_h <= 0.0 {
        return 1.0;
    }
    let m = canvas.match_width_or_height.clamp(0.0, 1.0);
    let lw = (screen_w / ref_w).ln();
    let lh = (screen_h / ref_h).ln();
    (lw + (lh - lw) * m).exp()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityData {
    pub id: u64,
    pub active: bool,
    pub name: String,
    pub tag: String,
    pub layer: String,
    pub is_static: bool,
    pub components: Vec<String>,
    pub transform: TransformData,
    pub mesh: Option<MeshData>,
    pub script: Option<String>,
    pub script_data: Option<ScriptComponentData>,
    pub physics: Option<PhysicsData>,
    pub audio: Option<AudioSourceData>,
    pub camera: Option<CameraData>,
    pub light: Option<LightData>,
    /// Bloque de niebla de escena (Fase 2). `#[serde(default)]`: las
    /// escenas anteriores no traen el campo.
    #[serde(default)]
    pub fog: Option<FogData>,
    pub canvas: Option<CanvasData>,
    pub rect_transform: Option<RectTransformData>,
    pub ui_text: Option<UITextData>,
    pub ui_button: Option<UIButtonData>,
    /// Elemento Imagen del Canvas (v2). `#[serde(default)]`: las
    /// escenas anteriores a la UI v2 no traen el campo.
    #[serde(default)]
    pub ui_image: Option<UIImageData>,

    /// Grupo de layout del Canvas (Fase 5). `#[serde(default)]`: las
    /// escenas anteriores no traen el campo.
    #[serde(default)]
    pub layout_group: Option<LayoutGroupData>,
/// Drag & drop de UI (Canvas v2). `#[serde(default)]`: las
    /// escenas anteriores no traen el campo.
    #[serde(default)]
    pub ui_drag: Option<UIDragData>,
    /// Animación de la entidad (skinning). `#[serde(default)]` mantiene
    /// cargables las escenas escritas antes de este campo: sin él, deserializar
    /// un .scene.json antiguo fallaría por campo ausente.
    #[serde(default)]
    pub animator: Option<AnimatorData>,
    /// Portal (bloque Portales): la entidad es la boca de un
    /// portal enlazado con la otra del mismo `pair`.
    /// `#[serde(default)]`: las escenas anteriores no lo traen.
    #[serde(default)]
    pub portal: Option<crate::portal::PortalData>,
    pub parent: Option<u64>,
    pub children: Vec<u64>,
}

impl EntityData {
    pub fn new(id: u64, name: String, components: Vec<String>) -> Self {
        let is_static = components.iter().any(|c| c == "StaticCollider");
        let mesh = if components.iter().any(|c| c == "MeshRenderer") {
            Some(MeshData::default())
        } else {
            None
        };
        let script = if components.iter().any(|c| c == "Script") {
            Some("GlassEngine.Core.Scripts.PlayerController".into())
        } else {
            None
        };
        let script_data = if script.is_some() {
            Some(ScriptComponentData::default())
        } else {
            None
        };
        let physics = if components.iter().any(|c| c == "RigidBody" || c == "Collider" || c == "StaticCollider") {
            let body_type = if is_static { "Static".into() } else { "Dynamic".into() };
            Some(PhysicsData {
                body_type,
                mass: 1.0,
                friction: 0.7,
                restitution: 0.05,
                box_extents: [0.5, 0.5, 0.5],
                linear_velocity: [0.0, 0.0, 0.0],
                layer: 0,
                collision_mask: u32::MAX,
                is_trigger: false,
                colliders: Vec::new(),
                mesh_collider: MeshColliderMode::None,
            })
        } else {
            None
        };
        let audio = if components.iter().any(|c| c == "AudioSource") {
            Some(AudioSourceData::default())
        } else {
            None
        };
        let camera = if components.iter().any(|c| c == "Camera") {
            Some(CameraData::default())
        } else {
            None
        };
        let light = if components.iter().any(|c| c == "Light") {
            Some(LightData::default())
        } else {
            None
        };
        let canvas = if components.iter().any(|c| c == "Canvas") {
            Some(CanvasData::default())
        } else {
            None
        };
        let rect_transform = if components.iter().any(|c| c == "RectTransform" || c == "Canvas" || c == "Text" || c == "Button") {
            Some(RectTransformData::default())
        } else {
            None
        };
        let ui_text = if components.iter().any(|c| c == "Text") {
            Some(UITextData::default())
        } else {
            None
        };
        let ui_button = if components.iter().any(|c| c == "Button") {
            Some(UIButtonData::default())
        } else {
            None
        };
        let animator = if components.iter().any(|c| c == "Animator") {
            Some(AnimatorData::default())
        } else {
            None
        };
        let layout_group = if components.iter().any(|c| c == "LayoutGroup") {
            Some(LayoutGroupData::default())
        } else {
            None
        };

        let tag = if name.contains("Player") {
            "Player".into()
        } else if name.contains("Camera") {
            "MainCamera".into()
        } else {
            "Untagged".into()
        };

        Self {
            id,
            active: true,
            name,
            tag,
            layer: "Default".into(),
            is_static,
            components,
            transform: TransformData::default(),
            mesh,
            script,
            script_data,
            physics,
            audio,
            camera,
            light,
            fog: None,
            canvas,
            rect_transform,
            ui_text,
            ui_button,
            ui_image: None,
            ui_drag: None,
            layout_group,
            portal: None,
            animator,
            parent: None,
            children: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SceneState {
    pub entities: Vec<EntityData>,
    pub next_entity_id: u64,
    pub viewport_snapshot_png: Option<Vec<u8>>,
    pub shaders_reloaded_flag: bool,
    /// Caché SoA de transforms (Fase 7): la resolución de mundos
    /// trabaja sobre arrays contiguos; la fuente autoral sigue
    /// siendo `EntityData.transform`.
    pub transform_store: std::cell::RefCell<crate::transforms::TransformStore>,
}

impl Default for SceneState {
    fn default() -> Self {
        Self {
            entities: vec![
                EntityData::new(1, "Main Camera".into(), vec!["Transform".into(), "Camera".into()]),
                EntityData::new(2, "Directional Light".into(), vec!["Transform".into(), "Light".into()]),
                EntityData::new(3, "Vulkan Viewport Triangle".into(), vec!["Transform".into(), "MeshRenderer".into(), "Material".into()]),
                EntityData::new(4, "Player Entity".into(), vec!["Transform".into(), "Script".into()]),
            ],
            next_entity_id: 5,
            viewport_snapshot_png: None,
            shaders_reloaded_flag: false,
            transform_store: std::cell::RefCell::new(crate::transforms::TransformStore::new()),
        }
    }
}

impl SceneState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_entity(&mut self, name: String, components: Vec<String>) -> u64 {
        let id = self.next_entity_id;
        self.next_entity_id += 1;
        self.entities.push(EntityData::new(id, name, components));
        id
    }

    pub fn delete_entity(&mut self, id: u64) -> bool {
        // Quitar de los hijos del padre si tenía
        let parent = self.get_entity(id).and_then(|e| e.parent);
        if let Some(pid) = parent {
            if let Some(p) = self.get_entity_mut(pid) {
                p.children.retain(|&c| c != id);
            }
        }
        // Desemparentar a sus hijos (moverlos al abuelo o raíz)
        let children = self.get_entity(id).map(|e| e.children.clone()).unwrap_or_default();
        for cid in children {
            if let Some(c) = self.get_entity_mut(cid) {
                c.parent = parent;
            }
            if let Some(pid) = parent {
                if let Some(p) = self.get_entity_mut(pid) {
                    p.children.push(cid);
                }
            }
        }

        if let Some(pos) = self.entities.iter().position(|e| e.id == id) {
            self.entities.remove(pos);
            true
        } else {
            false
        }
    }

    /// Comprueba si `candidate_ancestor` es antecesor de `descendant_id` para evitar ciclos
    pub fn is_descendant_of(&self, candidate_ancestor: u64, descendant_id: u64) -> bool {
        let mut curr = Some(candidate_ancestor);
        while let Some(id) = curr {
            if id == descendant_id {
                return true;
            }
            curr = self.get_entity(id).and_then(|e| e.parent);
        }
        false
    }

    /// Emparenta `child_id` bajo `new_parent_id` (o `None` para raíz)
    pub fn reparent(&mut self, child_id: u64, new_parent_id: Option<u64>) -> bool {
        if let Some(pid) = new_parent_id {
            if pid == child_id || self.is_descendant_of(pid, child_id) {
                return false; // Evita auto-emparentamiento o ciclos circulares
            }
        }

        // Quitar de los hijos del padre anterior si lo tenía
        let old_parent = self.get_entity(child_id).and_then(|e| e.parent);
        if let Some(old_pid) = old_parent {
            if let Some(old_p) = self.get_entity_mut(old_pid) {
                old_p.children.retain(|&c| c != child_id);
            }
        }

        // Agregar al nuevo padre si existe
        if let Some(new_pid) = new_parent_id {
            if let Some(new_p) = self.get_entity_mut(new_pid) {
                if !new_p.children.contains(&child_id) {
                    new_p.children.push(child_id);
                }
            }
        }

        // Actualizar el puntero parent del hijo
        if let Some(child) = self.get_entity_mut(child_id) {
            child.parent = new_parent_id;
            true
        } else {
            false
        }
    }

    /// Mueve una entidad hacia arriba en el orden visual de la jerarquía
    pub fn move_entity_up(&mut self, id: u64) -> bool {
        let parent = self.get_entity(id).and_then(|e| e.parent);
        if let Some(pid) = parent {
            if let Some(p) = self.get_entity_mut(pid) {
                if let Some(idx) = p.children.iter().position(|&c| c == id) {
                    if idx > 0 {
                        p.children.swap(idx, idx - 1);
                        return true;
                    }
                }
            }
        } else {
            // Entidad raíz: mover dentro del vector principal entities
            if let Some(idx) = self.entities.iter().position(|e| e.id == id) {
                if idx > 0 {
                    self.entities.swap(idx, idx - 1);
                    return true;
                }
            }
        }
        false
    }

    /// Mueve una entidad hacia abajo en el orden visual de la jerarquía
    pub fn move_entity_down(&mut self, id: u64) -> bool {
        let parent = self.get_entity(id).and_then(|e| e.parent);
        if let Some(pid) = parent {
            if let Some(p) = self.get_entity_mut(pid) {
                if let Some(idx) = p.children.iter().position(|&c| c == id) {
                    if idx + 1 < p.children.len() {
                        p.children.swap(idx, idx + 1);
                        return true;
                    }
                }
            }
        } else {
            // Entidad raíz: mover dentro del vector principal entities
            if let Some(idx) = self.entities.iter().position(|e| e.id == id) {
                if idx + 1 < self.entities.len() {
                    self.entities.swap(idx, idx + 1);
                    return true;
                }
            }
        }
        false
    }

    /// Inserta una entidad inmediatamente antes o después de otra (Drag & Drop en la jerarquía)
    pub fn insert_relative(&mut self, source_id: u64, target_id: u64, before: bool) -> bool {
        if source_id == target_id {
            return false;
        }
        // Prevenir ciclos: target_id no puede ser descendiente de source_id
        if self.is_descendant_of(target_id, source_id) {
            return false;
        }

        let target_parent = self.get_entity(target_id).and_then(|e| e.parent);
        let source_parent = self.get_entity(source_id).and_then(|e| e.parent);

        // Si los padres difieren, desenlazar de su padre anterior
        if source_parent != target_parent {
            if let Some(old_pid) = source_parent {
                if let Some(old_p) = self.get_entity_mut(old_pid) {
                    old_p.children.retain(|&c| c != source_id);
                }
            }
            if let Some(s) = self.get_entity_mut(source_id) {
                s.parent = target_parent;
            }
        }

        if let Some(pid) = target_parent {
            // Reordenar dentro de la lista de hijos del padre
            if let Some(p) = self.get_entity_mut(pid) {
                p.children.retain(|&c| c != source_id);
                if let Some(t_idx) = p.children.iter().position(|&c| c == target_id) {
                    let insert_idx = if before { t_idx } else { t_idx + 1 };
                    let clamped = insert_idx.min(p.children.len());
                    p.children.insert(clamped, source_id);
                    return true;
                } else {
                    p.children.push(source_id);
                    return true;
                }
            }
        } else {
            // Ambas son entidades raíz en self.entities
            if let Some(s_idx) = self.entities.iter().position(|e| e.id == source_id) {
                let ent = self.entities.remove(s_idx);
                if let Some(t_idx) = self.entities.iter().position(|e| e.id == target_id) {
                    let insert_idx = if before { t_idx } else { t_idx + 1 };
                    let clamped = insert_idx.min(self.entities.len());
                    self.entities.insert(clamped, ent);
                    return true;
                } else {
                    self.entities.push(ent);
                    return true;
                }
            }
        }
        false
    }

    /// Agrupa una o más entidades bajo un nuevo GameObject padre
    pub fn group_entities(&mut self, entity_ids: &[u64], group_name: &str) -> u64 {
        let group_id = self.create_entity(group_name.into(), vec!["Transform".into()]);
        for &eid in entity_ids {
            self.reparent(eid, Some(group_id));
        }
        group_id
    }

    /// Retorna la posición acumulada en espacio de mundo recorriendo la jerarquía.
    ///
    /// Composición real: `world = parent_world * local`, de modo que la rotación
    /// y la escala de los ancestros afectan al hijo. Véase
    /// [`SceneState::compute_world_matrices`] para la explicación del defecto que
    /// esta función tuvo (sumaba componentes ignorando rotación y escala).
    ///
    /// Los ciclos no pueden existir (`reparent` los rechaza), pero el recorrido
    /// lleva un tope de profundidad como red de seguridad para que una escena
    /// manipulada a mano nunca pueda colgar el motor.
    pub fn get_world_position(&self, id: u64) -> [f32; 3] {
        let m = self.compute_world_matrix(id);
        crate::math::mat4_translation(m)
    }

    /// Matriz de mundo de una entidad, componiendo la cadena de ancestros.
    ///
    /// Cacula la cadena raíz->hoja y compone de arriba abajo, que es el orden
    /// correcto: `T_world = T_root * ... * T_parent * T_local`.
    pub fn compute_world_matrix(&self, id: u64) -> [f32; 16] {
        // 1) Recolectar la cadena de ancestros, de la entidad hacia la raíz.
        let mut chain: Vec<&EntityData> = Vec::new();
        let mut curr = Some(id);
        let mut guard = 0;

        while let Some(eid) = curr {
            guard += 1;
            if guard > MAX_HIERARCHY_DEPTH {
                log::warn!(
                    "Jerarquía de más de {MAX_HIERARCHY_DEPTH} niveles en la entidad {id}; \
                     se trunca la composición de la matriz de mundo."
                );
                break;
            }
            match self.get_entity(eid) {
                Some(ent) => {
                    chain.push(ent);
                    curr = ent.parent;
                }
                None => {
                    // Padre inexistente: se trata como raíz para que la entidad
                    // siga siendo visible en lugar de desaparecer.
                    log::warn!("Entidad {id} referencia a un padre inexistente; se trata como raíz.");
                    break;
                }
            }
        }

        // 2) Componer desde la raíz (último de la cadena) hasta la entidad.
        let mut world = crate::math::mat4_identity();
        for ent in chain.iter().rev() {
            // Se compone con el cuaternión autoritativo, no con la vista Euler:
            // así una orientación acumulada por física o scripting se respeta
            // exactamente y no salta en gimbal lock.
            let local = crate::math::mat4_from_trs_quat(
                ent.transform.position,
                ent.transform.rotation_quat(),
                ent.transform.scale,
            );
            world = crate::math::mat4_mul(world, local);
        }

        world
    }

    /// Escribe una transformación en **espacio de mundo** sobre una entidad,
    /// convirtiéndola al transform LOCAL que la escena almacena.
    ///
    /// Es la ruta obligatoria para todo lo que produce resultados en mundo:
    /// el solucionador de Rapier3D y cualquier futura cinemática o IK. Escribir
    /// la posición de mundo directamente en `transform.position` corrompe a
    /// cualquier entidad con padre, porque esa componente es local.
    ///
    /// Devuelve `false` si la entidad no existe.
    pub fn set_world_transform(
        &mut self,
        id: u64,
        world_pos: [f32; 3],
        world_rot_quat: [f32; 4],
    ) -> bool {
        let parent_world = match self.get_entity(id).and_then(|e| e.parent) {
            Some(pid) => self.compute_world_matrix(pid),
            None => crate::math::mat4_identity(),
        };

        // local = parent^-1 * world  (sólo para la posición).
        let local_pos = match crate::math::mat4_inverse_affine(parent_world) {
            Some(inv) => crate::math::mat4_transform_point(inv, world_pos),
            None => {
                // Padre con escala cero en algún eje: no existe inversa. La
                // posición de mundo es la única información válida que se puede
                // conservar sin inventar valores.
                log::warn!(
                    "No se pudo convertir a local la entidad {id}: su padre tiene escala degenerada."
                );
                world_pos
            }
        };

        // local_rot = conj(parent_rot) * world_rot
        let parent_rot = crate::math::mat4_to_quat(parent_world);
        let parent_inv = [-parent_rot[0], -parent_rot[1], -parent_rot[2], parent_rot[3]];
        let local_rot = crate::math::quat_normalize(crate::math::quat_mul(parent_inv, world_rot_quat));

        if let Some(ent) = self.get_entity_mut(id) {
            ent.transform.position = local_pos;
            // El cuaternión local es la verdad; el Euler se deriva para el
            // Inspector. Guardar sólo el Euler descartaba precisión en cada
            // tick de física.
            ent.transform.set_rotation_quat(local_rot);
            true
        } else {
            false
        }
    }

    /// Matrices de mundo de **todas** las entidades, calculadas en una sola
    /// pasada topológica (padres antes que hijos).
    ///
    /// Es la ruta que debe usar el renderizado por frame: resolver cada entidad
    /// por separado con [`SceneState::compute_world_matrix`] es correcto pero
    /// cuesta `O(n * profundidad)`, mientras que esto es `O(n)`.
    ///
    /// Entidades cuyo padre no existe en la escena se tratan como raíces.
    pub fn compute_world_matrices(&self) -> std::collections::HashMap<u64, [f32; 16]> {
        let inicio = std::time::Instant::now();
        let mut store = self.transform_store.borrow_mut();
        if store.structure_differs(self) {
            store.rebuild(self);
        } else {
            store.refresh_locals(self);
        }
        let hechas = store.compute_world();
        let mapa = store.world_map();
        crate::transform_stats::record_pass(
            store.len() as u32,
            hechas as u32,
            store.max_depth,
            inicio.elapsed().as_micros() as u64,
        );
        mapa
    }
    pub fn get_entity(&self, id: u64) -> Option<&EntityData> {
        self.entities.iter().find(|e| e.id == id)
    }

    pub fn get_entity_mut(&mut self, id: u64) -> Option<&mut EntityData> {
        self.entities.iter_mut().find(|e| e.id == id)
    }

    pub fn modify_transform(
        &mut self,
        id: u64,
        position: Option<[f32; 3]>,
        rotation: Option<[f32; 3]>,
        scale: Option<[f32; 3]>,
    ) -> bool {
        if let Some(entity) = self.get_entity_mut(id) {
            if let Some(pos) = position {
                entity.transform.position = pos;
            }
            if let Some(rot) = rotation {
                entity.transform.set_rotation_euler(rot);
            }
            if let Some(s) = scale {
                entity.transform.scale = s;
            }
            true
        } else {
            false
        }
    }

    pub fn modify_material(&mut self, id: u64, color: [f32; 3], wireframe: Option<bool>) -> bool {
        if let Some(entity) = self.get_entity_mut(id) {
            if let Some(ref mut mesh) = entity.mesh {
                mesh.color = color;
                if let Some(wf) = wireframe {
                    mesh.wireframe = wf;
                }
            } else {
                entity.mesh = Some(MeshData {
                    color,
                    wireframe: wireframe.unwrap_or(false),
                    shader: "triangle.vert.spv".into(),
                    model: String::new(),
                    material: String::new(),
                    shader_properties: Default::default(),
                });
            }
            true
        } else {
            false
        }
    }

    pub fn modify_script(&mut self, id: u64, script_name: Option<String>) -> bool {
        if let Some(entity) = self.get_entity_mut(id) {
            entity.script = script_name;
            if entity.script.is_some() && !entity.components.iter().any(|c| c == "Script") {
                entity.components.push("Script".to_string());
            }
            true
        } else {
            false
        }
    }

    pub fn update_viewport_snapshot(&mut self, png_bytes: Vec<u8>) {
        self.viewport_snapshot_png = Some(png_bytes);
    }

    /// Retorna la primera cámara activa disponible en la escena
    pub fn active_camera(&self) -> Option<&EntityData> {
        self.entities.iter().find(|e| e.active && e.camera.is_some())
    }

    /// Retorna la luz direccional o principal activa en la escena
    pub fn main_light(&self) -> Option<&EntityData> {
        self.entities.iter().find(|e| e.active && e.light.is_some())
    }

    /// Guarda el estado de la escena en un archivo JSON en disco
    pub fn save_to_json(&self, path: &str) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self.entities)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)
    }

    /// Sustituye por completo el conjunto de entidades de la escena.
    ///
    /// ITERACIÓN 5 — es el **único** punto donde el mundo se reemplaza de golpe.
    /// Tanto `load_from_json` como el `SceneManager` pasan por aquí, de modo que
    /// la invariante de `next_entity_id` (siempre mayor que cualquier id vivo,
    /// para que una entidad nueva no colisione con una cargada de disco) se
    /// mantiene en un solo sitio y no puede divergir entre ambos caminos.
    /// Ajustes de niebla de la escena (Fase 2): el FogData de la primera
    /// entidad que lo lleve, si alguna.
    pub fn fog_settings(&self) -> Option<FogData> {
        self.entities.iter().find_map(|e| e.fog)
    }

    pub fn replace_entities(&mut self, entities: Vec<EntityData>) {
        self.entities = entities;
        self.next_entity_id = self.entities.iter().map(|e| e.id).max().unwrap_or(0) + 1;
    }

    /// Carga entidades desde un archivo JSON reemplazando la escena actual
    pub fn load_from_json(&mut self, path: &str) -> std::io::Result<()> {
        let json = std::fs::read_to_string(path)?;
        let entities: Vec<EntityData> = serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.replace_entities(entities);
        Ok(())
    }

    /// Instancia un Prefab dentro de la escena, reasignando IDs únicos y emparentando correctamente.
    pub fn instantiate_prefab(
        &mut self,
        prefab: &crate::prefab::Prefab,
        parent_id: Option<u64>,
        world_pos: Option<[f32; 3]>,
    ) -> u64 {
        use std::collections::HashMap;
        let mut id_map: HashMap<u64, u64> = HashMap::new();

        let new_root_id = self.next_entity_id;
        self.next_entity_id += 1;
        id_map.insert(prefab.root_entity.id, new_root_id);

        for desc in &prefab.descendant_entities {
            let new_id = self.next_entity_id;
            self.next_entity_id += 1;
            id_map.insert(desc.id, new_id);
        }

        let mut root_clone = prefab.root_entity.clone();
        root_clone.id = new_root_id;
        root_clone.parent = parent_id;
        if let Some(pos) = world_pos {
            root_clone.transform.position = pos;
        }
        root_clone.children = root_clone
            .children
            .iter()
            .filter_map(|cid| id_map.get(cid).copied())
            .collect();

        // Si se emparentó a un padre existente, añadir el nuevo id a sus hijos
        if let Some(pid) = parent_id {
            if let Some(p) = self.get_entity_mut(pid) {
                if !p.children.contains(&new_root_id) {
                    p.children.push(new_root_id);
                }
            }
        }

        self.entities.push(root_clone);

        for desc in &prefab.descendant_entities {
            let mut desc_clone = desc.clone();
            let new_id = id_map[&desc.id];
            desc_clone.id = new_id;
            desc_clone.parent = desc.parent.and_then(|old_pid| id_map.get(&old_pid).copied());
            desc_clone.children = desc_clone
                .children
                .iter()
                .filter_map(|cid| id_map.get(cid).copied())
                .collect();
            self.entities.push(desc_clone);
        }

        new_root_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_ui_rect_esquina_inferior_izquierda_sample_scene() {
        // El «Score HUD Text» de SampleScene/Nivel2: ancla y pivote en la
        // esquina inferior izquierda (convención Unity), desplazamiento
        // [20, 20] y tamaño [250, 40]. En pantalla de 1280×720 debe quedar
        // a 20 px del borde izquierdo y a 20 px del borde inferior.
        let rt = RectTransformData {
            anchored_position: [20.0, 20.0],
            size_delta: [250.0, 40.0],
            anchor_min: [0.0, 0.0],
            anchor_max: [0.0, 0.0],
            pivot: [0.0, 0.0],
        };
        let r = resolve_ui_rect(&rt, 1280.0, 720.0);
        assert_eq!(r, [20.0, 660.0, 250.0, 40.0]);
    }

    #[test]
    fn resolve_ui_rect_centrado_por_defecto() {
        // El RectTransform por defecto (ancla y pivote al centro,
        // posición [0,0], tamaño [160,40]) queda centrado en pantalla.
        let rt = RectTransformData::default();
        let r = resolve_ui_rect(&rt, 1280.0, 720.0);
        assert_eq!(r, [560.0, 340.0, 160.0, 40.0]);
    }

    #[test]
    fn ui_text_antiguo_sin_fuente_ni_alineacion_deserializa() {
        // Las escenas previas a la UI v2 no traen font/align.
        let t: UITextData = serde_json::from_str(
            r#"{"text":"Hola","font_size":20.0,"color":[1.0,1.0,1.0,1.0]}"#,
        )
        .unwrap();
        assert_eq!(t.font, "default");
        assert_eq!(t.align, "Left");
    }

    #[test]
    fn entidad_antigua_sin_ui_image_y_roundtrip_imagen() {
        let mut e = EntityData::new(9, "Icono".into(), vec!["RectTransform".into()]);
        let v = serde_json::to_value(&e).unwrap();
        let mut obj = v.as_object().unwrap().clone();
        obj.remove("ui_image");
        let old: EntityData =
            serde_json::from_value(serde_json::Value::Object(obj)).unwrap();
        assert!(old.ui_image.is_none());
        e.ui_image = Some(UIImageData {
            sprite: "textures/icono.png".into(),
            ..Default::default()
        });
        let back: EntityData =
            serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(
            back.ui_image.unwrap().sprite,
            "textures/icono.png"
        );
    }

    #[test]
    fn ancla_estirada_ocupa_el_span_del_padre() {
        // Anclas de esquina a esquina (0,0)-(1,1) con size_delta 0:
        // el rect ES el padre entero.
        let rt = RectTransformData {
            anchored_position: [0.0, 0.0],
            size_delta: [0.0, 0.0],
            anchor_min: [0.0, 0.0],
            anchor_max: [1.0, 1.0],
            pivot: [0.5, 0.5],
        };
        let r = resolve_ui_rect_in(&rt, [10.0, 20.0, 400.0, 300.0]);
        assert_eq!(r, [10.0, 20.0, 400.0, 300.0]);
        // Banda superior: anclas (0,1)-(1,1), alto fijo de 50 por size_delta.
        let rt = RectTransformData {
            anchored_position: [0.0, 0.0],
            size_delta: [0.0, 50.0],
            anchor_min: [0.0, 1.0],
            anchor_max: [1.0, 1.0],
            pivot: [0.5, 1.0],
        };
        let r = resolve_ui_rect_in(&rt, [0.0, 0.0, 1280.0, 720.0]);
        assert_eq!(r, [0.0, 0.0, 1280.0, 50.0]);
    }

    #[test]
    fn canvas_scale_factor_interpolado() {
        let c = CanvasData::default(); // 1920×1080, match 0.5
        // Misma resolución que la referencia: factor 1.
        assert!((canvas_scale_factor(&c, 1920.0, 1080.0) - 1.0).abs() < 1e-5);
        // Mitad en ambas: 0.5 con cualquier match.
        assert!((canvas_scale_factor(&c, 960.0, 540.0) - 0.5).abs() < 1e-5);
        // 1280×720 frente a 1920×1080: 2/3 en ambos ejes.
        assert!((canvas_scale_factor(&c, 1280.0, 720.0) - 2.0 / 3.0).abs() < 1e-4);
    }

    #[test]
    fn layout_horizontal_recoloca_en_fila() {
        let g = LayoutGroupData {
            direction: "horizontal".into(),
            spacing: 10.0,
            padding: [5.0, 5.0, 5.0, 5.0],
            alignment: "start".into(),
        };
        let hijos = [[0.0, 0.0, 100.0, 40.0], [0.0, 0.0, 50.0, 40.0]];
        let out = layout_group_rects([0.0, 0.0, 300.0, 100.0], &g, &hijos);
        assert_eq!(out[0], [5.0, 5.0, 100.0, 40.0]);
        assert_eq!(out[1], [115.0, 5.0, 50.0, 40.0]);
    }

    #[test]
    fn escena_ui_jerarquia_y_layout() {
        // Canvas + panel hijo + dos botones dentro de un grupo.
        let mut scene = SceneState::new();
        let canvas_id = scene.create_entity("Canvas".into(), vec!["Canvas".into()]);
        let panel_id = scene.create_entity("Panel".into(), vec!["RectTransform".into(), "LayoutGroup".into()]);
        let b1 = scene.create_entity("B1".into(), vec!["Button".into()]);
        let b2 = scene.create_entity("B2".into(), vec!["Button".into()]);
        let mut fijar = |id: u64, f: &dyn Fn(&mut EntityData)| {
            let e = scene.entities.iter_mut().find(|e| e.id == id).unwrap();
            f(e);
        };
        fijar(canvas_id, &|e: &mut EntityData| {
            e.canvas = Some(CanvasData {
                render_mode: "ScreenSpaceOverlay".into(),
                reference_resolution: [1280.0, 720.0],
                match_width_or_height: 0.5,
            });
        });
        fijar(panel_id, &|e: &mut EntityData| {
            e.parent = Some(canvas_id);
            e.layout_group = Some(LayoutGroupData {
                direction: "horizontal".into(),
                spacing: 10.0,
                padding: [0.0, 0.0, 0.0, 0.0],
                alignment: "start".into(),
            });
            e.rect_transform = Some(RectTransformData {
                anchored_position: [0.0, 0.0],
                size_delta: [400.0, 60.0],
                anchor_min: [0.0, 0.0],
                anchor_max: [0.0, 0.0],
                pivot: [0.0, 0.0],
            });
        });
        for b in [b1, b2] {
            fijar(b, &|e: &mut EntityData| {
                e.parent = Some(panel_id);
                e.rect_transform = Some(RectTransformData {
                    anchored_position: [0.0, 0.0],
                    size_delta: [100.0, 40.0],
                    anchor_min: [0.0, 0.0],
                    anchor_max: [0.0, 0.0],
                    pivot: [0.0, 0.0],
                });
            });
        }
        let rects = resolve_scene_ui_rects(&scene, 1280.0, 720.0);
        // Referencia = pantalla: factor 1. El panel va a la esquina
        // inferior izquierda (ancla 0,0, pivote 0,0): y = 720-60.
        assert_eq!(rects[&panel_id], [0.0, 660.0, 400.0, 60.0]);
        // Los botones, en fila dentro del panel por el layout.
        assert_eq!(rects[&b1], [0.0, 660.0, 100.0, 40.0]);
        assert_eq!(rects[&b2], [110.0, 660.0, 100.0, 40.0]);
    }

    #[test]
    fn resolve_ui_rect_tamano_minimo_y_ancla_superior() {
        // Tamaños por debajo de 10 px se claman como en el editor, y la
        // posición se calcula con el tamaño SIN clamar (paridad exacta).
        let rt = RectTransformData {
            anchored_position: [20.0, 20.0],
            size_delta: [4.0, 4.0],
            anchor_min: [0.0, 1.0],
            anchor_max: [0.0, 1.0],
            pivot: [0.0, 1.0],
        };
        let r = resolve_ui_rect(&rt, 1280.0, 720.0);
        assert_eq!(r, [20.0, -20.0, 10.0, 10.0]);
    }

    #[test]
    fn test_reparenting_and_hierarchy() {
        let mut scene = SceneState::new();
        let parent_id = scene.create_entity("Parent".into(), vec!["Transform".into()]);
        let child_id = scene.create_entity("Child".into(), vec!["Transform".into()]);

        assert!(scene.reparent(child_id, Some(parent_id)));
        assert_eq!(scene.get_entity(child_id).unwrap().parent, Some(parent_id));
        assert!(scene.get_entity(parent_id).unwrap().children.contains(&child_id));

        // Desemparentar
        assert!(scene.reparent(child_id, None));
        assert_eq!(scene.get_entity(child_id).unwrap().parent, None);
        assert!(!scene.get_entity(parent_id).unwrap().children.contains(&child_id));
    }

    #[test]
    fn test_cycle_prevention() {
        let mut scene = SceneState::new();
        let a = scene.create_entity("A".into(), vec!["Transform".into()]);
        let b = scene.create_entity("B".into(), vec!["Transform".into()]);
        let c = scene.create_entity("C".into(), vec!["Transform".into()]);

        // A -> B -> C
        assert!(scene.reparent(b, Some(a)));
        assert!(scene.reparent(c, Some(b)));

        // Intentar hacer que A sea hijo de C (crearía un ciclo) debe fallar
        assert!(!scene.reparent(a, Some(c)));
        assert_eq!(scene.get_entity(a).unwrap().parent, None);
    }

    #[test]
    fn test_move_up_and_down() {
        let mut scene = SceneState::new();
        let e1 = scene.create_entity("E1".into(), vec!["Transform".into()]);
        let e2 = scene.create_entity("E2".into(), vec!["Transform".into()]);

        let n = scene.entities.len();
        assert_eq!(scene.entities[n - 2].id, e1);
        assert_eq!(scene.entities[n - 1].id, e2);

        // Mover e2 arriba (intercambia con e1)
        assert!(scene.move_entity_up(e2));
        assert_eq!(scene.entities[n - 2].id, e2);
        assert_eq!(scene.entities[n - 1].id, e1);

        // Mover e2 abajo (regresa al final)
        assert!(scene.move_entity_down(e2));
        assert_eq!(scene.entities[n - 2].id, e1);
        assert_eq!(scene.entities[n - 1].id, e2);

        // Probar reordenación entre hermanos hijos
        let p = scene.create_entity("Parent".into(), vec!["Transform".into()]);
        let c1 = scene.create_entity("Child 1".into(), vec!["Transform".into()]);
        let c2 = scene.create_entity("Child 2".into(), vec!["Transform".into()]);
        scene.reparent(c1, Some(p));
        scene.reparent(c2, Some(p));

        assert_eq!(scene.get_entity(p).unwrap().children, vec![c1, c2]);
        assert!(scene.move_entity_up(c2));
        assert_eq!(scene.get_entity(p).unwrap().children, vec![c2, c1]);
        assert!(scene.move_entity_down(c2));
        assert_eq!(scene.get_entity(p).unwrap().children, vec![c1, c2]);
    }

    #[test]
    fn test_group_entities() {
        let mut scene = SceneState::new();
        let c1 = scene.create_entity("Cube 1".into(), vec!["Transform".into()]);
        let c2 = scene.create_entity("Cube 2".into(), vec!["Transform".into()]);

        let group_id = scene.group_entities(&[c1, c2], "Cubes Group");
        let group_ent = scene.get_entity(group_id).unwrap();
        assert_eq!(group_ent.name, "Cubes Group");
        assert_eq!(group_ent.children.len(), 2);
        assert!(group_ent.children.contains(&c1));
        assert!(group_ent.children.contains(&c2));

        assert_eq!(scene.get_entity(c1).unwrap().parent, Some(group_id));
        assert_eq!(scene.get_entity(c2).unwrap().parent, Some(group_id));
    }

    #[test]
    fn test_delete_entity_cleans_hierarchy() {
        let mut scene = SceneState::new();
        let p = scene.create_entity("Parent".into(), vec!["Transform".into()]);
        let c = scene.create_entity("Child".into(), vec!["Transform".into()]);
        scene.reparent(c, Some(p));

        scene.delete_entity(p);
        assert!(scene.get_entity(p).is_none());
        // El hijo debe quedar desemparentado a la raíz
        let child = scene.get_entity(c).unwrap();
        assert_eq!(child.parent, None);
    }

    // ---- Jerarquía: composición de matrices de mundo ------------------------

    #[test]
    fn test_hierarchy_rotation_is_inherited_in_world_matrix() {
        // Defecto corregido en ITERACIÓN 4: el hijo debe heredar la ROTACIÓN del
        // padre. Antes `get_world_position` sólo sumaba posiciones, de modo que
        // un hijo a +1 en X de un padre rotado 90 en Y reportaba [1,0,0] en vez
        // de [0,0,-1].
        let mut scene = SceneState::new();
        let p = scene.create_entity("Parent".into(), vec!["Transform".into()]);
        let c = scene.create_entity("Child".into(), vec!["Transform".into()]);

        scene.modify_transform(p, Some([0.0, 0.0, 0.0]), Some([0.0, 90.0, 0.0]), None);
        scene.modify_transform(c, Some([1.0, 0.0, 0.0]), None, None);
        scene.reparent(c, Some(p));

        let wp = scene.get_world_position(c);
        assert!(wp[0].abs() < 1.0e-4, "x = {} (debería ser 0)", wp[0]);
        assert!(wp[1].abs() < 1.0e-4, "y = {}", wp[1]);
        assert!((wp[2] + 1.0).abs() < 1.0e-4, "z = {} (debería ser -1)", wp[2]);
    }

    #[test]
    fn test_hierarchy_scale_is_inherited_in_world_matrix() {
        let mut scene = SceneState::new();
        let p = scene.create_entity("Parent".into(), vec!["Transform".into()]);
        let c = scene.create_entity("Child".into(), vec!["Transform".into()]);

        scene.modify_transform(p, Some([0.0, 0.0, 0.0]), None, Some([2.0, 2.0, 2.0]));
        scene.modify_transform(c, Some([3.0, 0.0, 0.0]), None, None);
        scene.reparent(c, Some(p));

        let wp = scene.get_world_position(c);
        assert!((wp[0] - 6.0).abs() < 1.0e-4, "x = {} (debería ser 6)", wp[0]);
    }

    #[test]
    fn test_hierarchy_composes_three_levels() {
        let mut scene = SceneState::new();
        let a = scene.create_entity("A".into(), vec!["Transform".into()]);
        let b = scene.create_entity("B".into(), vec!["Transform".into()]);
        let c = scene.create_entity("C".into(), vec!["Transform".into()]);

        scene.modify_transform(a, Some([10.0, 0.0, 0.0]), None, None);
        scene.modify_transform(b, Some([0.0, 5.0, 0.0]), None, None);
        scene.modify_transform(c, Some([0.0, 0.0, 2.0]), None, None);

        scene.reparent(b, Some(a));
        scene.reparent(c, Some(b));

        let wp = scene.get_world_position(c);
        assert!((wp[0] - 10.0).abs() < 1.0e-4, "x = {}", wp[0]);
        assert!((wp[1] - 5.0).abs() < 1.0e-4, "y = {}", wp[1]);
        assert!((wp[2] - 2.0).abs() < 1.0e-4, "z = {}", wp[2]);

        // La matriz de mundo del hijo debe coincidir con la ruta de la cadena.
        let m_chain = scene.compute_world_matrix(c);
        let all = scene.compute_world_matrices();
        let m_batch = all[&c];
        for i in 0..16 {
            assert!(
                (m_chain[i] - m_batch[i]).abs() < 1.0e-4,
                "la ruta por lote difiere de la ruta por cadena en el índice {i}"
            );
        }
    }

    #[test]
    fn test_world_matrices_batch_covers_every_entity() {
        let mut scene = SceneState::new();
        // La escena por defecto ya trae 4 entidades sin jerarquía.
        let p = scene.create_entity("P".into(), vec!["Transform".into()]);
        let c = scene.create_entity("C".into(), vec!["Transform".into()]);
        scene.reparent(c, Some(p));

        let all = scene.compute_world_matrices();
        assert_eq!(
            all.len(),
            scene.entities.len(),
            "toda entidad debe tener matriz de mundo resuelta"
        );
        for ent in &scene.entities {
            assert!(all.contains_key(&ent.id), "falta la entidad {}", ent.id);
        }
    }

    #[test]
    fn test_orphan_entity_is_treated_as_root_not_dropped() {
        // Un padre inexistente no debe hacer desaparecer al hijo de la escena.
        let mut scene = SceneState::new();
        let c = scene.create_entity("Huerfano".into(), vec!["Transform".into()]);
        scene.modify_transform(c, Some([7.0, 0.0, 0.0]), None, None);
        scene.get_entity_mut(c).unwrap().parent = Some(999_999);

        let all = scene.compute_world_matrices();
        assert!(all.contains_key(&c), "el huérfano debe seguir resolviéndose");
        let wp = scene.get_world_position(c);
        assert!((wp[0] - 7.0).abs() < 1.0e-4, "x = {}", wp[0]);
    }

    #[test]
    fn test_cyclic_hierarchy_does_not_hang() {
        // `reparent` rechaza ciclos, así que se fuerza uno a mano para probar la
        // red de seguridad: la resolución debe terminar, no colgarse.
        let mut scene = SceneState::new();
        let a = scene.create_entity("A".into(), vec!["Transform".into()]);
        let b = scene.create_entity("B".into(), vec!["Transform".into()]);
        scene.get_entity_mut(a).unwrap().parent = Some(b);
        scene.get_entity_mut(b).unwrap().parent = Some(a);

        let all = scene.compute_world_matrices();
        assert_eq!(all.len(), scene.entities.len());
        // Y la ruta por cadena también debe acotarse por MAX_HIERARCHY_DEPTH.
        let _ = scene.compute_world_matrix(a);
    }

    #[test]
    fn test_root_world_matrix_equals_local_matrix() {
        let mut scene = SceneState::new();
        let e = scene.create_entity("Raiz".into(), vec!["Transform".into()]);
        scene.modify_transform(e, Some([1.0, 2.0, 3.0]), Some([0.0, 45.0, 0.0]), Some([2.0, 2.0, 2.0]));

        let ent = scene.get_entity(e).unwrap();
        let expected = crate::math::mat4_from_trs_euler(
            ent.transform.position,
            ent.transform.rotation(),
            ent.transform.scale,
        );
        let actual = scene.compute_world_matrix(e);
        for i in 0..16 {
            assert!((expected[i] - actual[i]).abs() < 1.0e-5, "índice {i}");
        }
    }

    #[test]
    fn test_world_position_hierarchy() {
        let mut scene = SceneState::new();
        let p = scene.create_entity("Parent".into(), vec!["Transform".into()]);
        let c = scene.create_entity("Child".into(), vec!["Transform".into()]);
        scene.modify_transform(p, Some([10.0, 5.0, 0.0]), None, None);
        scene.modify_transform(c, Some([2.0, 1.0, 0.0]), None, None);
        scene.reparent(c, Some(p));

        let world_pos = scene.get_world_position(c);
        assert_eq!(world_pos, [12.0, 6.0, 0.0]);
    }

    #[test]
    fn test_insert_relative_drag_and_drop() {
        let mut scene = SceneState::new();
        let p = scene.create_entity("Parent".into(), vec!["Transform".into()]);
        let c1 = scene.create_entity("C1".into(), vec!["Transform".into()]);
        let c2 = scene.create_entity("C2".into(), vec!["Transform".into()]);
        let c3 = scene.create_entity("C3".into(), vec!["Transform".into()]);
        scene.reparent(c1, Some(p));
        scene.reparent(c2, Some(p));
        scene.reparent(c3, Some(p));

        assert_eq!(scene.get_entity(p).unwrap().children, vec![c1, c2, c3]);

        // Insertar c3 antes de c1 (subirlo al inicio de la lista de hermanos)
        assert!(scene.insert_relative(c3, c1, true));
        assert_eq!(scene.get_entity(p).unwrap().children, vec![c3, c1, c2]);

        // Insertar c3 después de c1 (bajarlo después de c1)
        assert!(scene.insert_relative(c3, c1, false));
        assert_eq!(scene.get_entity(p).unwrap().children, vec![c1, c3, c2]);

        // Prevención de ciclo: no se puede insertar un padre dentro de su hijo
        assert!(!scene.insert_relative(p, c1, true));
    }

    #[test]
    fn test_scene_serialization_and_canvas_ui() {
        let mut scene = SceneState::new();
        let canvas_id = scene.create_entity(
            "Main Canvas".into(),
            vec!["Transform".into(), "RectTransform".into(), "Canvas".into()],
        );
        let text_id = scene.create_entity(
            "Score Text".into(),
            vec!["Transform".into(), "RectTransform".into(), "Text".into()],
        );
        scene.reparent(text_id, Some(canvas_id));

        assert!(scene.get_entity(canvas_id).unwrap().canvas.is_some());
        assert!(scene.get_entity(text_id).unwrap().ui_text.is_some());

        let temp_path = std::env::temp_dir().join("test_scene.json");
        let temp_str = temp_path.to_str().unwrap();

        assert!(scene.save_to_json(temp_str).is_ok());

        let mut loaded_scene = SceneState::new();
        assert!(loaded_scene.load_from_json(temp_str).is_ok());

        assert_eq!(loaded_scene.entities.len(), scene.entities.len());
        let loaded_text = loaded_scene.get_entity(text_id).unwrap();
        assert_eq!(loaded_text.name, "Score Text");
        assert!(loaded_text.ui_text.is_some());
        assert_eq!(loaded_text.parent, Some(canvas_id));

        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn set_world_transform_converts_to_local_for_child() {
        // El padre está rotado 90 grados en Y y escalado x2. Escribir la
        // posición de mundo [0,0,-2] sobre el hijo debe guardar el offset LOCAL
        // [1,0,0], no el valor de mundo (que era el defecto original).
        let mut scene = SceneState::new();
        let p = scene.create_entity("P".into(), vec!["Transform".into()]);
        let c = scene.create_entity("C".into(), vec!["Transform".into()]);
        scene.modify_transform(p, Some([0.0, 0.0, 0.0]), Some([0.0, 90.0, 0.0]), Some([2.0, 2.0, 2.0]));
        scene.reparent(c, Some(p));

        let world_rot = crate::math::euler_deg_to_quat([0.0, 90.0, 0.0]);
        assert!(scene.set_world_transform(c, [0.0, 0.0, -2.0], world_rot));

        let local = scene.get_entity(c).unwrap().transform.position;
        assert!(
            (local[0] - 1.0).abs() < 1.0e-4,
            "x = {} (local esperado 1)",
            local[0]
        );
        assert!(local[1].abs() < 1.0e-4, "y = {}", local[1]);
        assert!(local[2].abs() < 1.0e-4, "z = {}", local[2]);

        // Y al recomponer el mundo se recupera exactamente lo escrito.
        let back = scene.get_world_position(c);
        assert!(back[0].abs() < 1.0e-4, "world x = {}", back[0]);
        assert!(back[1].abs() < 1.0e-4, "world y = {}", back[1]);
        assert!((back[2] + 2.0).abs() < 1.0e-4, "world z = {}", back[2]);
    }

    #[test]
    fn set_world_transform_on_root_equals_local() {
        let mut scene = SceneState::new();
        let e = scene.create_entity("Raiz".into(), vec!["Transform".into()]);

        let rot = crate::math::euler_deg_to_quat([10.0, 20.0, 30.0]);
        assert!(scene.set_world_transform(e, [4.0, 5.0, 6.0], rot));

        let t = &scene.get_entity(e).unwrap().transform;
        assert!(t.position[0] - 4.0 < 1.0e-4 && 4.0 - t.position[0] < 1.0e-4);
        assert!(t.position[1] - 5.0 < 1.0e-4 && 5.0 - t.position[1] < 1.0e-4);
        assert!(t.position[2] - 6.0 < 1.0e-4 && 6.0 - t.position[2] < 1.0e-4);
    }

    #[test]
    fn rotation_quat_survives_a_script_round_trip() {
        // Contrato FFI: un script C# escribe un cuaternión y lo vuelve a leer.
        // Antes el puente convertía a Euler al escribir y de vuelta a cuaternión
        // al leer, así que una orientación compuesta acumulaba error y podía
        // saltar de rama en gimbal lock.
        let mut scene = SceneState::new();
        let e = scene.create_entity("Script".into(), vec!["Transform".into()]);

        // Cuaternión no trivial, ya normalizado.
        let q = crate::math::euler_deg_to_quat([23.0, -71.0, 44.0]);
        scene.set_world_transform(e, [1.0, 2.0, 3.0], q);

        let stored = scene.get_entity(e).unwrap().transform.rotation_quat();
        let dot = stored[0] * q[0] + stored[1] * q[1] + stored[2] * q[2] + stored[3] * q[3];
        assert!(
            (dot.abs() - 1.0).abs() < 1.0e-6,
            "el cuaternión no sobrevivió la ida y vuelta: dot = {dot}"
        );

        // Diez vueltas más no deben degradar la orientación (esto era lo que
        // fallaba con el rodeo por Euler).
        for _ in 0..10 {
            let cur = scene.get_entity(e).unwrap().transform.rotation_quat();
            scene.set_world_transform(e, [1.0, 2.0, 3.0], cur);
        }
        let after = scene.get_entity(e).unwrap().transform.rotation_quat();
        let drift = after[0] * q[0] + after[1] * q[1] + after[2] * q[2] + after[3] * q[3];
        assert!(
            (drift.abs() - 1.0).abs() < 1.0e-5,
            "la orientación derivó tras 10 vueltas: dot = {drift}"
        );
    }

    #[test]
    fn euler_setter_keeps_quaternion_in_sync() {
        let mut scene = SceneState::new();
        let e = scene.create_entity("Inspector".into(), vec!["Transform".into()]);

        // El Inspector escribe Euler; el cuaternión debe quedar coherente.
        scene.modify_transform(e, None, Some([0.0, 90.0, 0.0]), None);
        let q = scene.get_entity(e).unwrap().transform.rotation_quat();
        let expected = crate::math::euler_deg_to_quat([0.0, 90.0, 0.0]);
        let dot = q[0] * expected[0] + q[1] * expected[1] + q[2] * expected[2] + q[3] * expected[3];
        assert!((dot.abs() - 1.0).abs() < 1.0e-6, "dot = {dot}");

        // Y la vista Euler sigue siendo la que escribió el usuario.
        let view = scene.get_entity(e).unwrap().transform.rotation();
        assert!((view[1] - 90.0).abs() < 1.0e-3, "y = {}", view[1]);
    }

    #[test]
    fn transform_json_without_rotation_quat_still_loads() {
        // Compatibilidad: un `.scene.json` guardado antes de que existiera el
        // campo autoritativo no debe fallar al cargar, y no debe quedar con la
        // orientación en el cuaternión cero (que no es una rotación).
        let legacy = r#"{
            "position": [1.0, 2.0, 3.0],
            "rotation": [0.0, 45.0, 0.0],
            "scale": [1.0, 1.0, 1.0]
        }"#;

        let t: TransformData = serde_json::from_str(legacy).expect("transform antiguo debe cargar");

        let q = t.rotation_quat();
        assert!(
            q != [0.0, 0.0, 0.0, 0.0],
            "un cuaternión cero no es una rotación válida"
        );

        let expected = crate::math::euler_deg_to_quat([0.0, 45.0, 0.0]);
        let dot = q[0] * expected[0] + q[1] * expected[1] + q[2] * expected[2] + q[3] * expected[3];
        assert!((dot.abs() - 1.0).abs() < 1.0e-5, "dot = {dot}");
    }

    #[test]
    fn transform_json_round_trips_both_representations() {
        // Lo que se guarda debe poder recuperarse: si el cuaternión no se
        // serializa, la orientación autoritativa se pierde al guardar.
        let mut t = TransformData::default();
        t.set_rotation_quat(crate::math::euler_deg_to_quat([15.0, 160.0, -80.0]));

        let json = serde_json::to_string(&t).expect("debe serializar");
        assert!(
            json.contains("rotation_quat"),
            "el cuaternión autoritativo debe persistir: {json}"
        );

        let back: TransformData = serde_json::from_str(&json).expect("debe deserializar");
        let a = t.rotation_quat();
        let b = back.rotation_quat();
        let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
        assert!((dot.abs() - 1.0).abs() < 1.0e-6, "dot = {dot}");
    }
}

#[cfg(test)]
mod tests_material {
    use super::*;

    /// 1) JSON parcial: los campos ausentes caen en sus defaults.
    #[test]
    fn serde_parcial_usa_defaults() {
        let json = r#"{"albedo":[1.0,0.5,0.25,1.0],"metallic":1.0}"#;
        let m: MaterialData = serde_json::from_str(json).unwrap();

        assert_eq!(m.albedo, [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(m.metallic, 1.0);
        // Campos no informados: defaults del motor.
        assert_eq!(m.roughness, 0.45);
        assert_eq!(m.tiling, [1.0, 1.0]);
        assert!(!m.transparent);
        assert_eq!(m.texture, "");
        assert_eq!(m.emissive, [0.0, 0.0, 0.0]);
        assert_eq!(m.emissive_intensity, 0.0);
        assert_eq!(m.offset, [0.0, 0.0]);
    }

    /// 2) Roundtrip completo con todos los campos no-default.
    #[test]
    fn roundtrip_completo() {
        let original = MaterialData {
            albedo: [0.2, 0.4, 0.6, 0.8],
            texture: "materials/madera.png".to_string(),
            metallic: 0.9,
            roughness: 0.15,
            emissive: [0.1, 0.2, 0.3],
            emissive_intensity: 2.5,
            tiling: [3.0, 4.0],
            offset: [0.25, 0.75],
            transparent: true,
        };

        let json = serde_json::to_string(&original).unwrap();
        let recuperado: MaterialData = serde_json::from_str(&json).unwrap();

        assert_eq!(recuperado.albedo, original.albedo);
        assert_eq!(recuperado.texture, original.texture);
        assert_eq!(recuperado.metallic, original.metallic);
        assert_eq!(recuperado.roughness, original.roughness);
        assert_eq!(recuperado.emissive, original.emissive);
        assert_eq!(recuperado.emissive_intensity, original.emissive_intensity);
        assert_eq!(recuperado.tiling, original.tiling);
        assert_eq!(recuperado.offset, original.offset);
        assert!(recuperado.transparent);
    }

    /// 3) Escena antigua: `MeshData` sin el campo `material`
    /// debe deserializar con `material == ""`.
    #[test]
    fn meshdata_antiguo_sin_material() {
        // `color`, `wireframe` y `shader` no tienen default, así
        // que deben aparecer; `model` y `material` sí lo tienen.
        let json = r#"{
            "color": [1.0, 0.0, 0.0],
            "wireframe": false,
            "shader": "standard"
        }"#;
        let mesh: MeshData = serde_json::from_str(json).unwrap();

        assert_eq!(mesh.material, "");
        assert_eq!(mesh.model, "");
        assert_eq!(mesh.shader, "standard");
        assert_eq!(mesh.color, [1.0, 0.0, 0.0]);
        assert!(!mesh.wireframe);
    }
}
