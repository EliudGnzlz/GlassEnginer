use std::sync::{OnceLock, RwLock};
use crate::bindings::TransformFfi;

/// Motivo por el que el motor rechazó una petición de carga de escena.
///
/// Es un tipo y no un `String` para que los puntos de entrada FFI puedan mapear
/// el fallo a un código numérico estable sin inspeccionar mensajes de texto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneLoadRejection {
    /// La escena pedida no existe en el proyecto.
    NotFound(String),
    /// El nombre o la ruta resuelta escapan de la raíz del proyecto.
    Invalid(String),
    /// El motor todavía no tiene escena cargable (sin proyecto, sin VFS).
    Unavailable(String),
}

impl SceneLoadRejection {
    pub fn message(&self) -> &str {
        match self {
            Self::NotFound(m) | Self::Invalid(m) | Self::Unavailable(m) => m,
        }
    }
}

impl std::fmt::Display for SceneLoadRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// Sin componente de física: la entidad spawneada es solo visual.
pub const SPAWN_PHYSICS_NONE: i32 = 0;
/// Cuerpo estático (Rapier `Fixed`): suelo, muros, piezas de una sala.
pub const SPAWN_PHYSICS_STATIC: i32 = 1;
/// Cuerpo dinámico: cae, choca y su transform lo escribe la física.
pub const SPAWN_PHYSICS_DYNAMIC: i32 = 2;
/// Cuerpo cinemático: lo mueve el script, empuja a los dinámicos.
pub const SPAWN_PHYSICS_KINEMATIC: i32 = 3;

/// Especificación de una entidad a crear en la escena viva a petición de
/// un script (bloque de instanciación). Es el tipo ya saneado que cruza
/// del FFI (`bindings::SpawnSpecFfi`) al motor: cadenas como `String`
/// (vacía = ausente), cuaternión de rotación y el tipo de física como
/// una de las constantes `SPAWN_PHYSICS_*`.
///
/// La transformada es la LOCAL de la entidad (relativa a `parent_id` si
/// lo hay; con 0, la raíz, local == mundo). El motor deriva la de mundo
/// para crear el cuerpo físico, igual que hace la reconstrucción de
/// escena con las entidades autoradas.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    /// Nombre de la entidad; vacío -> "Entity".
    pub name: String,
    /// Nombre del modelo en el AssetCache ("cube", "sphere", "capsule",
    /// "cylinder", "plane" o un .glb importado). Vacío = el cubo por
    /// defecto del render, como una entidad autorada sin modelo.
    pub model: String,
    /// Clase GlassBehaviour a adjuntar; vacía = sin script. La instancia
    /// NO se crea dentro de esta llamada: el motor la encola y la app la
    /// crea antes del siguiente `update_all` (ver la reconciliación en
    /// glass_app), para no instanciar C# en mitad de un callback FFI.
    pub script: String,
    /// Entidad padre; 0 = raíz de la escena.
    pub parent_id: u64,
    pub position: [f32; 3],
    /// Cuaternión `[x, y, z, w]`; el FFI lo sustituye por la identidad
    /// si llega degenerado (longitud ~0).
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// Color plano del material; solo se aplica si `has_color`.
    pub color: [f32; 3],
    pub has_color: bool,
    /// Una de las constantes `SPAWN_PHYSICS_*`.
    pub physics: i32,
}

/// Contrato entre el runtime de scripting (C#) y la escena viva del motor.
///
/// Los métodos de escena tienen implementación por defecto para que un
/// `SceneBridge` que sólo quiera transformadas siga compilando, y para que
/// `NullBridge` no tenga que enumerarlos.
pub trait SceneBridge: Send + Sync + 'static {
    fn get_transform(&self, entity_id: u64) -> Option<TransformFfi>;
    fn set_transform(&self, entity_id: u64, transform: TransformFfi) -> bool;

    /// Encola una carga de escena para el final del frame actual, siguiendo la
    /// semántica diferida de `SceneManager.LoadScene` de Unity: el llamante
    /// retorna de inmediato y la escena cambia cuando el frame termina, nunca a
    /// mitad de frame con la GPU y la física leyendo el mundo.
    fn request_scene_load(&self, name: &str) -> Result<(), SceneLoadRejection> {
        Err(SceneLoadRejection::Unavailable(format!(
            "el motor no soporta carga de escenas en este puente (pedido: '{name}')"
        )))
    }

    /// Nombre corto de la escena activa, o `None` si todavía no hay proyecto.
    fn active_scene_name(&self) -> Option<String> {
        None
    }

    /// Escenas disponibles en el proyecto, ordenadas.
    fn available_scenes(&self) -> Vec<String> {
        Vec::new()
    }

    /// Pide que la entidad reproduzca un clip de animación (bloque de
    /// skinning): el motor crea o actualiza su componente `Animator` con
    /// ese clip y `playing = true`. Devuelve `true` si la entidad existe.
    /// Por defecto no hace nada: un puente sin escena animable (o el
    /// inerte) responde que la entidad no está, igual que `set_transform`.
    fn animator_play(&self, _entity_id: u64, _clip: &str) -> bool {
        false
    }

    /// Fija un parámetro del controlador de animación de la entidad
    /// (bloque FSM): `kind` 0 = bool, 1 = float, 2 = trigger. Por
    /// defecto no hace nada, como `animator_play`.
    fn animator_set_param(&self, _entity_id: u64, _param: &str, _kind: i32, _value: f32) -> bool {
        false
    }

    /// Estado actual de la máquina de la entidad (bloque FSM): con
    /// controlador, el nombre del estado; sin controlador, el clip.
    fn animator_current_state(&self, _entity_id: u64) -> String {
        String::new()
    }

    /// Salto con fundido (bloque FSM): a un estado si la entidad tiene
    /// controlador, o crossfade libre entre clips si no lo tiene.
    fn animator_crossfade(&self, _entity_id: u64, _name: &str, _fade: f32) -> bool {
        false
    }
    /// Color de la luz de la entidad (bloque de luces): muta
    /// `light.color` del componente. El motor ya lee
    /// `light.color * light.intensity` cada frame, así que basta con
    /// mutar `LightData`. Por defecto no hace nada, como
    /// `animator_set_param`.
    fn light_set_color(&self, _entity_id: u64, _r: f32, _g: f32, _b: f32) -> bool {
        false
    }

    /// Intensidad de la luz de la entidad (bloque de luces): muta
    /// `light.intensity` del componente (el motor la acota a >= 0.0).
    /// Por defecto no hace nada, como `animator_set_param`.
    fn light_set_intensity(&self, _entity_id: u64, _intensity: f32) -> bool {
        false
    }

    /// Lee la intensidad de la luz de la entidad (bloque de luces): es una
    /// lectura REAL del componente del motor, no del campo local de C#.
    /// Por defecto devuelve `None`, igual que `light_set_intensity` da
    /// `false` cuando no hay puente.
    fn light_get_intensity(&self, _entity_id: u64) -> Option<f32> {
        None
    }

    /// Tipo de la luz de la entidad (bloque de luces): muta
    /// `light.light_type` del componente. 0 = Directional, 1 = Point,
    /// 2 = Spot. Por defecto no hace nada, como `animator_set_param`.
    fn light_set_type(&self, _entity_id: u64, _tipo: i32) -> bool {
        false
    }

    /// Rango de la luz de la entidad (bloque de luces): muta
    /// `light.range` del componente (el motor la acota a >= 0.0).
    /// Por defecto no hace nada, como `animator_set_param`.
    fn light_set_range(&self, _entity_id: u64, _range: f32) -> bool {
        false
    }

    /// Ángulos del cono Spot de la luz de la entidad (bloque de luces):
    /// muta los ángulos exterior e interior (en grados) del componente.
    /// Por defecto no hace nada, como `animator_set_param`.
    fn light_set_spot_angles(&self, _entity_id: u64, _outer_deg: f32, _inner_deg: f32) -> bool {
        false
    }
    /// Muta el campo `ui_text.text` de la entidad indicada.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_text(&self, _entity_id: u64, _text: &str) -> bool {
        false
    }

    /// Muta el campo `ui_text.font_size` de la entidad indicada.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_font_size(&self, _entity_id: u64, _size: f32) -> bool {
        false
    }

    /// Muta el campo `ui_text.color` de la entidad indicada.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_text_color(&self, _entity_id: u64, _r: f32, _g: f32, _b: f32, _a: f32) -> bool {
        false
    }

    /// Muta el campo `ui_image.sprite` de la entidad indicada.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_image_sprite(&self, _entity_id: u64, _sprite: &str) -> bool {
        false
    }

    /// Muta el campo `ui_image.color` de la entidad indicada.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_image_color(&self, _entity_id: u64, _r: f32, _g: f32, _b: f32, _a: f32) -> bool {
        false
    }

    /// Muta el campo `ui_button.target_graphic_color` de la entidad indicada.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_button_color(&self, _entity_id: u64, _r: f32, _g: f32, _b: f32, _a: f32) -> bool {
        false
    }

    /// Muta el campo `rect_transform.anchored_position` de la entidad indicada.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_anchored_pos(&self, _entity_id: u64, _x: f32, _y: f32) -> bool {
        false
    }

    /// Muta el campo `rect_transform.size_delta` de la entidad indicada.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_size_delta(&self, _entity_id: u64, _w: f32, _h: f32) -> bool {
        false
    }

    /// Muta la declaración `background-color` del style_extra de la entidad.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_background_color(&self, _entity_id: u64, _r: f32, _g: f32, _b: f32, _a: f32) -> bool {
        false
    }

    /// Muta la declaración `border-radius` del style_extra de la entidad.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_border_radius(&self, _entity_id: u64, _tl: f32, _tr: f32, _br: f32, _bl: f32) -> bool {
        false
    }

    /// Muta la declaración `opacity` del style_extra de la entidad.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_opacity(&self, _entity_id: u64, _opacity: f32) -> bool {
        false
    }

    /// Muta la declaración `overflow` del style_extra de la entidad.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_overflow(&self, _entity_id: u64, _overflow: &str) -> bool {
        false
    }

    /// Muta la declaración `letter-spacing` del style_extra de la entidad.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_letter_spacing(&self, _entity_id: u64, _px: f32) -> bool {
        false
    }

    /// Muta la declaración `border` del style_extra de la entidad.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_border(&self, _entity_id: u64, _width: f32, _r: f32, _g: f32, _b: f32, _a: f32) -> bool {
        false
    }

    /// Muta `rect_transform.anchor_min` de la entidad indicada.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_anchor_min(&self, _entity_id: u64, _x: f32, _y: f32) -> bool {
        false
    }

    /// Muta `rect_transform.anchor_max` de la entidad indicada.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_anchor_max(&self, _entity_id: u64, _x: f32, _y: f32) -> bool {
        false
    }

    /// Muta `rect_transform.pivot` de la entidad indicada.
    /// Devuelve `false` si el puente no está vinculado o la entidad no existe.
    fn ui_set_pivot(&self, _entity_id: u64, _x: f32, _y: f32) -> bool {
        false
    }
    /// Entidad del último arrastre del Canvas (UI v2, drag &
    /// drop): la lee C# como `DragDrop.DraggedEntityId`.
    fn ui_dragged_id(&self) -> u64 {
        0
    }

    /// Casilla que recibió la última suelta (0 = en vacío): la lee
    /// C# como `DragDrop.DropTargetEntityId`.
    fn ui_drop_target_id(&self) -> u64 {
        0
    }

    /// Crea una entidad NUEVA en la escena viva (bloque de
    /// instanciación): malla, transformada, física inferida de la malla
    /// y, si el spec trae clase, el componente de script declarado (la
    /// instancia C# la crea la app en su reconciliación, no aquí).
    /// Devuelve el id asignado, o `None` si el puente no crea entidades
    /// o el spec es inaceptable (p. ej. un padre que no existe).
    ///
    /// La aplicación es INMEDIATA, no diferida como la carga de escenas:
    /// quien llama es un script en su Update/Awake y ningún Mutex de
    /// escena o física está retenido en ese punto del frame, así que el
    /// id devuelto ya es utilizable por el propio script en la misma
    /// llamada (semántica de `Instantiate` de Unity).
    fn spawn_entity(&self, _spec: &SpawnSpec) -> Option<u64> {
        None
    }

    /// Instancia un Prefab en la escena viva (bloque de instanciación
    /// desde `.prefab.json`): el motor ya resolvió la ruta y cargó el
    /// archivo, así que este método solo recibe la ruta final (por si
    /// el puente quiere registrar de dónde vino) y la posición de
    /// mundo de la raíz. Devuelve el id de la entidad raíz creada, o
    /// `None` si el puente no soporta prefabs o la carga/instanciación
    /// falló (el motivo va al log del lado que la implementa). Por
    /// defecto no hace nada, igual que `spawn_entity`.
    fn instantiate_prefab(&self, _path: &str, _position: [f32; 3]) -> Option<u64> {
        None
    }

    /// Destruye una entidad de la escena viva: sale del grafo (sus
    /// hijos se reemparentan, la semántica de borrado del editor) y su
    /// cuerpo y colliders salen del mundo de Rapier. Las instancias de
    /// script que tuviera las dispone la reconciliación de la app en el
    /// frame en curso (su `OnDestroy` corre ahí, nunca dentro de esta
    /// llamada). Devuelve `true` si la entidad existía.
    fn destroy_entity(&self, _entity_id: u64) -> bool {
        false
    }

    /// Activa o desactiva una entidad (la base de los pools): una
    /// entidad inactiva no se dibuja, y su cuerpo sale del mundo físico
    /// al desactivarla y vuelve a crearse —desde su `PhysicsData` y la
    /// forma inferida de su malla— al reactivarla. Devuelve `true` si
    /// la entidad existe.
    fn set_entity_active(&self, _entity_id: u64, _active: bool) -> bool {
        false
    }

    /// Fija la niebla de la escena en vivo (color RGB lineal, cerca y
    /// lejos): escribe el `FogData` de la primera entidad que lo
    /// tenga (la app lo lee cada frame). Pensado para sistemas de
    /// clima en scripts. Devuelve `true` si había niebla que fijar.
    fn set_fog(&self, _color: [f32; 3], _near: f32, _far: f32) -> bool {
        false
    }

    /// Fija el FOV (grados) de la cámara principal en vivo: escribe
    /// el `CameraData` de la cámara main (la app lo lee cada frame).
    /// Pensado para miras con zoom en scripts. Devuelve `true` si
    /// había cámara principal que ajustar.
    fn set_camera_fov(&self, _fov: f32) -> bool {
        false
    }

    /// Número de entidades de la escena viva, o `None` sin puente.
    fn entity_count(&self) -> Option<usize> {
        None
    }

    /// Devuelve TODOS los pares (entity_id, transform) de la escena
    /// en una sola lectura.
    ///
    /// El host lo usa para cruzar la frontera FFI una única vez por
    /// frame en lugar de N veces (ver `get_transforms_batch`). El
    /// default vacío es deliberado: un puente que no soporte lotes
    /// simplemente reporta N = 0 y C# cae al camino de una entidad
    /// por llamada, sin romper nada.
    fn all_transforms(&self) -> Vec<(u64, TransformFfi)> {
        Vec::new()
    }

    /// (cuerpos, colliders) del mundo físico vivo, o `None` sin puente.
    /// Son los conteos del propio Rapier (`RigidBodySet::len` /
    /// `ColliderSet::len` a través de `PhysicsWorld`), no una
    /// contabilidad paralela: es la cifra que un generador usa para
    /// verificar que tras borrar una sala todo volvió a la base.
    fn physics_counts(&self) -> Option<(usize, usize)> {
        None
    }
}

/// Puente inerte: usado antes de que exista una escena real y en tests.
pub struct NullBridge;
impl SceneBridge for NullBridge {
    fn get_transform(&self, _: u64) -> Option<TransformFfi> {
        None
    }
    fn set_transform(&self, _: u64, _: TransformFfi) -> bool {
        false
    }
}

static SCENE_BRIDGE: OnceLock<RwLock<Box<dyn SceneBridge>>> = OnceLock::new();

fn get_bridge_cell() -> &'static RwLock<Box<dyn SceneBridge>> {
    SCENE_BRIDGE.get_or_init(|| RwLock::new(Box::new(NullBridge)))
}

/// Registers the engine scene bridge connecting C# runtime calls to the actual scene entities.
pub fn register_scene_bridge(bridge: Box<dyn SceneBridge>) {
    if let Ok(mut cell) = get_bridge_cell().write() {
        *cell = bridge;
    }
}

pub(crate) fn read_entity_transform(entity_id: u64) -> Option<TransformFfi> {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.get_transform(entity_id)
    } else {
        None
    }
}

pub(crate) fn write_entity_transform(entity_id: u64, transform: TransformFfi) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.set_transform(entity_id, transform)
    } else {
        false
    }
}

/// Lee el lote completo de transforms de la escena activa.
///
/// Análoga a `read_entity_transform`, pero devuelve todos los pares
/// con una sola toma del candado del puente. Si el puente no está
/// registrado o el candado está envenenado se devuelve un vector
/// vacío: el lote vacío es un estado válido, no un error (el
/// contrato FFI sólo distingue tamaños, no fallos de lectura).
pub(crate) fn read_all_transforms() -> Vec<(u64, TransformFfi)> {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.all_transforms()
    } else {
        Vec::new()
    }
}

/// Reenvía al motor una petición de carga de escena procedente de C#.
pub(crate) fn request_scene_load(name: &str) -> Result<(), SceneLoadRejection> {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.request_scene_load(name)
    } else {
        Err(SceneLoadRejection::Unavailable(
            "puente de escena bloqueado por un hilo en pánico".to_string(),
        ))
    }
}

pub(crate) fn read_active_scene_name() -> Option<String> {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.active_scene_name()
    } else {
        None
    }
}

pub(crate) fn read_available_scenes() -> Vec<String> {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.available_scenes()
    } else {
        Vec::new()
    }
}

/// Reenvía al motor un `Animator.Play` procedente de C#.
pub(crate) fn animator_play(entity_id: u64, clip: &str) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.animator_play(entity_id, clip)
    } else {
        false
    }
}

/// Fija un parámetro del controlador (bloque FSM) a través del puente
/// activo; sin puente, no hace nada.
pub(crate) fn animator_set_param(entity_id: u64, param: &str, kind: i32, value: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.animator_set_param(entity_id, param, kind, value)
    } else {
        false
    }
}

/// Estado actual de la máquina (bloque FSM) a través del puente activo.
pub(crate) fn animator_current_state(entity_id: u64) -> String {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.animator_current_state(entity_id)
    } else {
        String::new()
    }
}

/// Salto con fundido (bloque FSM) a través del puente activo.
pub(crate) fn animator_crossfade(entity_id: u64, name: &str, fade: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.animator_crossfade(entity_id, name, fade)
    } else {
        false
    }
}

/// Fija el color de la luz de la entidad a través del puente activo;
/// sin puente, no hace nada.
pub(crate) fn light_set_color(entity_id: u64, r: f32, g: f32, b: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.light_set_color(entity_id, r, g, b)
    } else {
        false
    }
}

/// Fija la intensidad de la luz de la entidad a través del puente activo;
/// sin puente, no hace nada.
pub(crate) fn light_set_intensity(entity_id: u64, intensity: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.light_set_intensity(entity_id, intensity)
    } else {
        false
    }
}

/// Lee la intensidad de la luz de la entidad a través del puente activo;
/// sin puente, devuelve `None`.
pub(crate) fn light_get_intensity(entity_id: u64) -> Option<f32> {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.light_get_intensity(entity_id)
    } else {
        None
    }
}

/// Fija el tipo de la luz de la entidad a través del puente activo;
/// sin puente, no hace nada.
pub(crate) fn light_set_type(entity_id: u64, tipo: i32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.light_set_type(entity_id, tipo)
    } else {
        false
    }
}

/// Fija el rango de la luz de la entidad a través del puente activo;
/// sin puente, no hace nada.
pub(crate) fn light_set_range(entity_id: u64, range: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.light_set_range(entity_id, range)
    } else {
        false
    }
}

/// Fija los ángulos del cono Spot de la luz de la entidad a través del
/// puente activo; sin puente, no hace nada.
pub(crate) fn light_set_spot_angles(entity_id: u64, outer_deg: f32, inner_deg: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.light_set_spot_angles(entity_id, outer_deg, inner_deg)
    } else {
        false
    }
}

/// Despacha la mutación de `ui_text.text` al puente activo.
pub(crate) fn ui_set_text(entity_id: u64, text: &str) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_text(entity_id, text)
    } else {
        false
    }
}

/// Despacha la mutación de `ui_text.font_size` al puente activo.
pub(crate) fn ui_set_font_size(entity_id: u64, size: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_font_size(entity_id, size)
    } else {
        false
    }
}

/// Despacha la mutación de `ui_text.color` al puente activo.
pub(crate) fn ui_set_text_color(entity_id: u64, r: f32, g: f32, b: f32, a: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_text_color(entity_id, r, g, b, a)
    } else {
        false
    }
}

/// Despacha la mutación de `ui_image.sprite` al puente activo.
pub(crate) fn ui_set_image_sprite(entity_id: u64, sprite: &str) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_image_sprite(entity_id, sprite)
    } else {
        false
    }
}

/// Despacha la mutación de `ui_image.color` al puente activo.
pub(crate) fn ui_set_image_color(entity_id: u64, r: f32, g: f32, b: f32, a: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_image_color(entity_id, r, g, b, a)
    } else {
        false
    }
}

/// Despacha la mutación de `ui_button.target_graphic_color` al puente activo.
pub(crate) fn ui_set_button_color(entity_id: u64, r: f32, g: f32, b: f32, a: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_button_color(entity_id, r, g, b, a)
    } else {
        false
    }
}

/// Despacha la mutación de `rect_transform.anchored_position` al puente activo.
pub(crate) fn ui_set_anchored_pos(entity_id: u64, x: f32, y: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_anchored_pos(entity_id, x, y)
    } else {
        false
    }
}

/// Despacha la mutación de `rect_transform.size_delta` al puente activo.
pub(crate) fn ui_set_size_delta(entity_id: u64, w: f32, h: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_size_delta(entity_id, w, h)
    } else {
        false
    }
}

/// Despacha la mutación de `background-color` al puente activo.
pub(crate) fn ui_set_background_color(entity_id: u64, r: f32, g: f32, b: f32, a: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_background_color(entity_id, r, g, b, a)
    } else {
        false
    }
}

/// Despacha la mutación de `border-radius` al puente activo.
pub(crate) fn ui_set_border_radius(entity_id: u64, tl: f32, tr: f32, br: f32, bl: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_border_radius(entity_id, tl, tr, br, bl)
    } else {
        false
    }
}

/// Despacha la mutación de `opacity` al puente activo.
pub(crate) fn ui_set_opacity(entity_id: u64, opacity: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_opacity(entity_id, opacity)
    } else {
        false
    }
}

/// Despacha la mutación de `overflow` al puente activo.
pub(crate) fn ui_set_overflow(entity_id: u64, overflow: &str) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_overflow(entity_id, overflow)
    } else {
        false
    }
}

/// Despacha la mutación de `letter-spacing` al puente activo.
pub(crate) fn ui_set_letter_spacing(entity_id: u64, px: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_letter_spacing(entity_id, px)
    } else {
        false
    }
}

/// Despacha la mutación de `border` al puente activo.
pub(crate) fn ui_set_border(entity_id: u64, width: f32, r: f32, g: f32, b: f32, a: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_border(entity_id, width, r, g, b, a)
    } else {
        false
    }
}

/// Despacha la mutación de `anchor_min` al puente activo.
pub(crate) fn ui_set_anchor_min(entity_id: u64, x: f32, y: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_anchor_min(entity_id, x, y)
    } else {
        false
    }
}

/// Despacha la mutación de `anchor_max` al puente activo.
pub(crate) fn ui_set_anchor_max(entity_id: u64, x: f32, y: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_anchor_max(entity_id, x, y)
    } else {
        false
    }
}

/// Despacha la mutación de `pivot` al puente activo.
pub(crate) fn ui_set_pivot(entity_id: u64, x: f32, y: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_set_pivot(entity_id, x, y)
    } else {
        false
    }
}

/// Entidad del último arrastre del Canvas a través del puente activo.
pub(crate) fn ui_dragged_id() -> u64 {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_dragged_id()
    } else {
        0
    }
}

/// Casilla de la última suelta a través del puente activo.
pub(crate) fn ui_drop_target_id() -> u64 {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.ui_drop_target_id()
    } else {
        0
    }
}

/// Reenvía al motor un spawn de entidad procedente de C#.
pub(crate) fn spawn_entity(spec: &SpawnSpec) -> Option<u64> {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.spawn_entity(spec)
    } else {
        None
    }
}

/// Reenvía al motor una instanciación de prefab procedente de C#.
pub(crate) fn instantiate_prefab(path: &str, position: [f32; 3]) -> Option<u64> {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.instantiate_prefab(path, position)
    } else {
        None
    }
}


/// Reenvía al motor un destroy de entidad procedente de C#.
pub(crate) fn destroy_entity(entity_id: u64) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.destroy_entity(entity_id)
    } else {
        false
    }
}

/// Reenvía al motor un cambio de activo de entidad procedente de C#.
pub(crate) fn set_entity_active(entity_id: u64, active: bool) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.set_entity_active(entity_id, active)
    } else {
        false
    }
}

/// Reenvía al motor un cambio de niebla procedente de C#.
pub(crate) fn set_fog(color: [f32; 3], near: f32, far: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.set_fog(color, near, far)
    } else {
        false
    }
}

/// Reenvía al motor un cambio de FOV de la cámara principal.
pub(crate) fn set_camera_fov(fov: f32) -> bool {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.set_camera_fov(fov)
    } else {
        false
    }
}

/// Número de entidades vivas según el puente instalado.
pub(crate) fn entity_count() -> Option<usize> {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.entity_count()
    } else {
        None
    }
}

/// Conteos del mundo físico según el puente instalado.
pub(crate) fn physics_counts() -> Option<(usize, usize)> {
    if let Ok(cell) = get_bridge_cell().read() {
        cell.physics_counts()
    } else {
        None
    }
}
