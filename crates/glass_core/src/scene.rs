use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// Objeto (nodo) del modelo al que se enlaza la entidad cuando el
    /// archivo trae varios: la entidad dibuja solo los rangos de ese
    /// objeto. Vacío = el modelo entero. Lo rellena el importador de
    /// Unity desde la tabla de sub-objetos del `.meta` (el nombre de
    /// entidad de Unity no siempre coincide con el nodo del FBX).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub object: String,
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
            object: String::new(),
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
    /// Ruta del `.physmat.json` relativa a Assets; "" = sin material (el
    /// collider usa `PhysicsData::body_fallback_material`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    /// "X" | "Y" | "Z": eje de la cápsula. Solo aplica a Capsule; "Y" es el
    /// valor histórico y no se serializa.
    #[serde(default = "default_direction_y", skip_serializing_if = "es_y")]
    pub direction: String,
}

/// Spec de collider con los valores por defecto de su forma, como el que
/// crea Unity al añadir el componente: medio metro en todo. Las formas
/// desconocidas se normalizan a `Box`.
pub fn spec_por_defecto(shape: &str) -> ColliderSpec {
    let (shape, size) = match shape {
        "Sphere" => ("Sphere", [0.5, 0.0, 0.0]),
        "Capsule" => ("Capsule", [0.5, 0.5, 0.0]),
        "Cylinder" => ("Cylinder", [0.5, 0.5, 0.0]),
        _ => ("Box", [0.5, 0.5, 0.5]),
    };
    ColliderSpec {
        shape: shape.to_string(),
        size,
        offset: [0.0, 0.0, 0.0],
        is_trigger: false,
        material: String::new(),
        direction: default_direction_y(),
    }
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
// Derive heredado de la línea de atributos anterior: #[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterData {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_cc_radius")]
    pub radius: f32,
    #[serde(default = "default_cc_height")]
    pub height: f32,
    #[serde(default = "default_cc_slope")]
    pub slope_limit_deg: f32,
    #[serde(default = "default_cc_step")]
    pub step_offset: f32,
    #[serde(default = "default_cc_skin")]
    pub skin_width: f32,
    #[serde(default = "default_cc_min_move")]
    pub min_move_distance: f32,
    #[serde(default)]
    pub center: [f32; 3],
    #[serde(default = "default_true")]
    pub detect_collisions: bool,
    #[serde(default = "default_cc_snap")]
    pub snap: f32,
}

fn default_cc_radius() -> f32 {
    0.5
}
fn default_cc_height() -> f32 {
    2.0
}
fn default_cc_slope() -> f32 {
    45.0
}
fn default_cc_step() -> f32 {
    0.3
}
fn default_cc_skin() -> f32 {
    0.08
}
fn default_cc_min_move() -> f32 {
    0.001
}
fn default_cc_snap() -> f32 {
    0.3
}

impl Default for CharacterData {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            radius: default_cc_radius(),
            height: default_cc_height(),
            slope_limit_deg: default_cc_slope(),
            step_offset: default_cc_step(),
            skin_width: default_cc_skin(),
            min_move_distance: default_cc_min_move(),
            center: [0.0; 3],
            detect_collisions: default_true(),
            snap: default_cc_snap(),
        }
    }
}

impl CharacterData {
    /// (radius, height, skin_width, step_offset, slope_limit_deg, snap) para construir el
    /// CharacterConfig de glass_physics en el llamador (glass_core no depende de glass_physics).
    pub fn config(&self) -> (f32, f32, f32, f32, f32, f32) {
        (
            self.radius,
            self.height,
            self.skin_width,
            self.step_offset,
            self.slope_limit_deg,
            self.snap,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsData {
    pub body_type: String, // "Dynamic", "Static", "Kinematic"
    /// Presencia del componente Rigidbody (separación RB/Collider
    /// estilo Unity, 2026-11-18). Sin Rigidbody, el Collider solo es
    /// estático: la construcción de física fuerza cuerpo fijo.
    #[serde(default = "default_true")]
    pub has_rigidbody: bool,
    /// Presencia del componente Collider. Sin Collider, el Rigidbody
    /// es un cuerpo sin forma: cae y no colisiona con nada.
    #[serde(default = "default_true")]
    pub has_collider: bool,
    pub mass: f32,
    /// Fricción heredada de escenas anteriores al Physics Material. La
    /// clave JSON sigue siendo "friction": la escena vieja carga `Some(..)`
    /// y la reescribe igual al guardar. `None` = sin legado (objeto nuevo).
    #[serde(rename = "friction", default, skip_serializing_if = "Option::is_none")]
    pub legacy_friction: Option<f32>,
    /// Restitución heredada; clave JSON "restitution". Ver `legacy_friction`.
    #[serde(rename = "restitution", default, skip_serializing_if = "Option::is_none")]
    pub legacy_restitution: Option<f32>,
    pub box_extents: [f32; 3],
    /// Centro (local) del collider en el camino histórico de lista vacía
    /// (`colliders` sin specs). Espejo de `ColliderSpec::offset`.
    #[serde(default)]
    pub box_offset: [f32; 3],
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
    /// Restricciones por ejes estilo Unity (bits de RigidBodyConstraints:
    /// 1/2/4 posición XYZ, 8/16/32 rotación XYZ). 0 = libre (defecto).
    #[serde(default)]
    pub constraints: u8,
    /// Colliders primitivos compuestos (Fase 1.5). Vacío = camino
    /// histórico de un solo collider inferido de la malla.
    #[serde(default)]
    pub colliders: Vec<ColliderSpec>,
    /// Collider de malla sobre la geometría del modelo (Fase 1.5).
    #[serde(default)]
    pub mesh_collider: MeshColliderMode,
    /// Ruta del `.physmat.json` del Mesh Collider, relativa a Assets;
    /// "" = sin material (usa `body_fallback_material`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mesh_collider_material: String,
    /// true = el Mesh Collider es sensor.
    #[serde(default)]
    pub mesh_collider_is_trigger: bool,
    /// Amortiguación lineal (freno de velocidad). Defecto histórico 1.5.
    #[serde(default = "default_linear_damping")]
    pub linear_damping: f32,
    /// Amortiguación angular (freno de rotación). Defecto 0.0, el de Rapier.
    #[serde(default = "default_angular_damping")]
    pub angular_damping: f32,
    /// Iteraciones TOTALES del solver para el cuerpo. Defecto 4, la base de Rapier.
    #[serde(default = "default_solver_iterations")]
    pub solver_iterations: u32,
    /// Velocidad máxima de despenetración (m/s). 0.0 = sin límite.
    #[serde(default = "default_max_depenetration_velocity")]
    pub max_depenetration_velocity: f32,
    /// solverVelocityIterations del cuerpo. Comparte contador con solver_iterations
    /// en Rapier: manda el máximo. Defecto 4.
    #[serde(default = "default_solver_velocity_iterations")]
    pub solver_velocity_iterations: u32,
    /// includeLayers por cuerpo (Unity): capas con las que SÍ colisiona. Distinto
    /// de collision_mask (máscara del collider). Defecto u32::MAX.
    #[serde(default = "default_include_layers")]
    pub include_layers: u32,
    /// excludeLayers por cuerpo (Unity): capas con las que NO colisiona. Defecto 0.
    #[serde(default)]
    pub exclude_layers: u32,
    /// automaticCenterOfMass (Unity): COM desde los colliders. Defecto true.
    #[serde(default = "default_true")]
    pub automatic_center_of_mass: bool,
    /// automaticInertiaTensor (Unity): inercia desde los colliders. Defecto true.
    #[serde(default = "default_true")]
    pub automatic_inertia_tensor: bool,
    /// Si el cuerpo responde a la gravedad del mundo.
    #[serde(default = "default_true")]
    pub use_gravity: bool,
    /// Detección continua de colisiones (CCD).
    #[serde(default)]
    pub ccd: bool,
    /// Interpolación de render entre pasos fijos (estilo Unity).
    #[serde(default)]
    pub interpolate: bool,
}

fn default_linear_damping() -> f32 {
    1.5
}

fn default_angular_damping() -> f32 {
    0.0
}

fn default_max_depenetration_velocity() -> f32 {
    0.0
}

fn default_solver_iterations() -> u32 {
    4
}

fn default_solver_velocity_iterations() -> u32 {
    4
}

fn default_include_layers() -> u32 {
    u32::MAX
}

fn default_true() -> bool {
    true
}

/// Eje histórico de las cápsulas: Y.
fn default_direction_y() -> String {
    "Y".to_string()
}

/// `skip_serializing_if` de `ColliderSpec::direction`: no se escribe el eje
/// histórico, así las escenas existentes no cambian al guardarse.
#[allow(clippy::ptr_arg)]
fn es_y(s: &String) -> bool {
    s == "Y"
}

impl Default for PhysicsData {
    fn default() -> Self {
        Self {
            body_type: "Dynamic".into(),
            // Corrección de D3m3tr10 (2026-11-19): un objeto nuevo nace
            // SOLO con Collider, como en Unity; el Rigidbody se añade a
            // mano. El Default de creación no trae Rigidbody (el default
            // de CARGA serde es otro: los .json viejos sin el campo
            // siguen cargando con RB, como fueron creados).
            has_rigidbody: false,
            has_collider: true,
            interpolate: false,
            mass: 1.0,
            // La creación no lleva legado: fricción 0.7 y rebote 0.05 salen
            // de PhysicsMaterial::PROJECT_DEFAULT vía body_fallback_material.
            legacy_friction: None,
            legacy_restitution: None,
            box_extents: [0.5, 0.5, 0.5],
            box_offset: [0.0, 0.0, 0.0],
            linear_velocity: [0.0, 0.0, 0.0],
            layer: 0,
            collision_mask: u32::MAX,
            is_trigger: false,
            constraints: 0,
            colliders: Vec::new(),
            solver_velocity_iterations: 4,
            include_layers: u32::MAX,
            exclude_layers: 0,
            automatic_center_of_mass: true,
            automatic_inertia_tensor: true,
            mesh_collider: MeshColliderMode::None,
            mesh_collider_material: String::new(),
            mesh_collider_is_trigger: false,
            linear_damping: 1.5,
            angular_damping: 0.0,
            solver_iterations: 4,
            max_depenetration_velocity: 0.0,
            use_gravity: true,
            ccd: false,
        }
    }
}

impl PhysicsData {
    /// Material que se aplica a un collider de este cuerpo sin `.physmat.json`
    /// asignado. Compone el legado sobre PROJECT_DEFAULT:
    /// - dynamic_friction = legacy_friction o el del proyecto;
    /// - static_friction = siempre el del proyecto (el legado no la guarda);
    /// - bounciness = legacy_restitution o el del proyecto;
    /// - combines = los del proyecto (Average).
    pub fn body_fallback_material(&self) -> crate::physics_material::PhysicsMaterial {
        use crate::physics_material::PhysicsMaterial;
        let base = PhysicsMaterial::PROJECT_DEFAULT;
        PhysicsMaterial {
            dynamic_friction: self.legacy_friction.unwrap_or(base.dynamic_friction),
            static_friction: base.static_friction,
            bounciness: self.legacy_restitution.unwrap_or(base.bounciness),
            friction_combine: base.friction_combine,
            bounce_combine: base.bounce_combine,
        }
    }

    /// Añade un collider primitivo a la lista y marca el componente
    /// Collider como presente (P3: cada spec es un componente).
    pub fn anadir_collider(&mut self, spec: ColliderSpec) {
        self.colliders.push(spec);
        self.has_collider = true;
    }

    /// Duplica el collider `idx` justo después del original. `false` si
    /// `idx` está fuera de rango (no toca nada).
    pub fn duplicar_collider(&mut self, idx: usize) -> bool {
        if idx >= self.colliders.len() {
            return false;
        }
        let copia = self.colliders[idx].clone();
        self.colliders.insert(idx + 1, copia);
        self.has_collider = true;
        true
    }

    /// Extrae el collider `idx`. Si la lista queda vacía y no hay Mesh
    /// Collider, apaga `has_collider`: la lista vacía sin mesh es el
    /// estado «inferido legacy» y resucitaría un collider que el usuario
    /// acaba de quitar.
    pub fn quitar_collider(&mut self, idx: usize) -> Option<ColliderSpec> {
        if idx >= self.colliders.len() {
            return None;
        }
        let quitado = self.colliders.remove(idx);
        if self.colliders.is_empty()
            && matches!(self.mesh_collider, MeshColliderMode::None)
        {
            self.has_collider = false;
        }
        Some(quitado)
    }

    /// Quita el Mesh Collider (modo a `None`). Si no quedan specs, apaga
    /// `has_collider` por la misma regla que `quitar_collider`.
    pub fn quitar_mesh_collider(&mut self) {
        self.mesh_collider = MeshColliderMode::None;
        if self.colliders.is_empty() {
            self.has_collider = false;
        }
    }

    /// Número de colliders efectivos de la entidad: los specs, más el de
    /// malla si lo hay, más el inferido legacy si la lista está vacía sin
    /// mesh. 0 si el componente Collider no está presente.
    pub fn total_colliders(&self) -> usize {
        if !self.has_collider {
            return 0;
        }
        let mut total = self.colliders.len();
        if !matches!(self.mesh_collider, MeshColliderMode::None) {
            total += 1;
        }
        if self.colliders.is_empty()
            && matches!(self.mesh_collider, MeshColliderMode::None)
        {
            total += 1;
        }
        total
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
    /// Campos serializados del script (C0b): nombre → valor JSON tal como
    /// venía en la escena de Unity (escalares, vectores como objetos
    /// `{x, y, z}`, listas anidadas). Las referencias a objetos de Unity
    /// no se transportan en v1. Vacío en escenas anteriores a C0b o sin
    /// campos; con `skip_serializing_if` las escenas viejas no cambian.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub campos: std::collections::BTreeMap<String, serde_json::Value>,
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
            campos: Default::default(),
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

/// Datos de un emisor de partículas (Fase 8P): espejo serializado
/// de `glass_particles::EmitterConfig` más el `playing` autorado.
/// La app lo convierte en el emisor de runtime; si el dato cambia
/// en vivo (MCP), el pool se reinicia con la configuración nueva.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParticleEmitterData {
    #[serde(default = "default_particle_max")]
    pub max_particles: usize,
    #[serde(default = "default_particle_rate")]
    pub rate: f32,
    #[serde(default = "default_particle_lifetime")]
    pub lifetime: [f32; 2],
    #[serde(default = "default_particle_speed")]
    pub start_speed: [f32; 2],
    #[serde(default = "default_particle_size")]
    pub start_size: [f32; 2],
    #[serde(default = "default_particle_gravity")]
    pub gravity: [f32; 3],
    #[serde(default = "default_particle_drag")]
    pub drag: f32,
    /// "point" | "sphere" | "box" | "cone".
    #[serde(default = "default_particle_shape")]
    pub shape: String,
    /// Radio de la esfera o del disco del cono.
    #[serde(default = "default_particle_radius")]
    pub shape_radius: f32,
    /// Semiejes de la caja.
    #[serde(default = "default_particle_extents")]
    pub shape_extents: [f32; 3],
    /// Apertura del cono, en grados.
    #[serde(default = "default_particle_angle")]
    pub cone_angle: f32,
    #[serde(default)]
    pub local_space: bool,
    #[serde(default = "default_particle_color_start")]
    pub color_start: [f32; 4],
    #[serde(default = "default_particle_color_end")]
    pub color_end: [f32; 4],
    #[serde(default = "default_particle_size_end")]
    pub size_end_factor: f32,
    #[serde(default = "default_particle_rotation_speed")]
    pub rotation_speed: [f32; 2],
    #[serde(default)]
    pub stretched: bool,
    #[serde(default = "default_particle_stretch")]
    pub stretch_factor: f32,
    #[serde(default = "default_particle_seed")]
    pub seed: u32,
    #[serde(default = "default_particle_playing")]
    pub playing: bool,
}

fn default_particle_max() -> usize { 2000 }
fn default_particle_rate() -> f32 { 700.0 }
fn default_particle_lifetime() -> [f32; 2] { [1.6, 2.8] }
fn default_particle_speed() -> [f32; 2] { [1.2, 2.4] }
fn default_particle_size() -> [f32; 2] { [0.35, 0.6] }
fn default_particle_gravity() -> [f32; 3] { [0.0, 1.2, 0.0] }
fn default_particle_drag() -> f32 { 0.6 }
fn default_particle_shape() -> String { "cone".to_string() }
fn default_particle_radius() -> f32 { 0.45 }
fn default_particle_extents() -> [f32; 3] { [0.5, 0.5, 0.5] }
fn default_particle_angle() -> f32 { 18.0 }
fn default_particle_color_start() -> [f32; 4] { [1.0, 0.55, 0.12, 0.85] }
fn default_particle_color_end() -> [f32; 4] { [0.75, 0.08, 0.02, 0.0] }
fn default_particle_size_end() -> f32 { 0.25 }
fn default_particle_rotation_speed() -> [f32; 2] { [-1.2, 1.2] }
fn default_particle_stretch() -> f32 { 0.08 }
fn default_particle_seed() -> u32 { 1 }
fn default_particle_playing() -> bool { true }

impl Default for ParticleEmitterData {
    fn default() -> Self {
        Self {
            max_particles: default_particle_max(),
            rate: default_particle_rate(),
            lifetime: default_particle_lifetime(),
            start_speed: default_particle_speed(),
            start_size: default_particle_size(),
            gravity: default_particle_gravity(),
            drag: default_particle_drag(),
            shape: default_particle_shape(),
            shape_radius: default_particle_radius(),
            shape_extents: default_particle_extents(),
            cone_angle: default_particle_angle(),
            local_space: false,
            color_start: default_particle_color_start(),
            color_end: default_particle_color_end(),
            size_end_factor: default_particle_size_end(),
            rotation_speed: default_particle_rotation_speed(),
            stretched: false,
            stretch_factor: default_particle_stretch(),
            seed: default_particle_seed(),
            playing: default_particle_playing(),
        }
    }
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

fn default_spot_outer_deg() -> f32 { 45.0 }
fn default_spot_inner_deg() -> f32 { 35.0 }
fn default_area_width() -> f32 { 5.0 }
fn default_area_height() -> f32 { 3.0 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightData {
    pub light_type: String, // "Directional", "Point", "Spot", "Area"
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    /// Si esta luz puntual proyecta sombra (Fase 2): el motor le asigna
    /// uno de los 4 cubos del atlas compartido por orden de cercanía.
    #[serde(default)]
    pub cast_shadow: bool,
    #[serde(default = "default_spot_outer_deg")]
    pub spot_outer_deg: f32,
    #[serde(default = "default_spot_inner_deg")]
    pub spot_inner_deg: f32,
    /// Ancho del rectángulo emisor de una luz Area, en metros.
    #[serde(default = "default_area_width")]
    pub area_width: f32,
    /// Alto del rectángulo emisor de una luz Area, en metros.
    #[serde(default = "default_area_height")]
    pub area_height: f32,
}

impl Default for LightData {
    fn default() -> Self {
        Self {
            light_type: "Directional".into(),
            color: [1.0, 0.98, 0.92],
            intensity: 1.0,
            range: 10.0,
            cast_shadow: false,
            spot_outer_deg: 45.0,
            spot_inner_deg: 35.0,
            area_width: 5.0,
            area_height: 3.0,
        }
    }
}

impl LightData {
    /// (cos_outer, cos_inner) del cono Spot con los ángulos saneados:
    /// outer se acota a [1°, 179°] e inner nunca supera a outer.
    pub fn spot_cone_cos(&self) -> (f32, f32) {
        let outer = self.spot_outer_deg.clamp(1.0, 179.0);
        let inner = self.spot_inner_deg.min(outer).max(0.0);
        (outer.to_radians().cos(), inner.to_radians().cos())
    }
}


// === G1: Ambiente de escena ===

/// Valores URP (Universal Render Pipeline) de referencia para el gradient
/// ambiente de 3 paradas (cielo / ecuador / suelo), en espacio lineal.
pub const URP_AMBIENT_SKY: [f32; 3] = [0.03697, 0.04215, 0.05455];
pub const URP_AMBIENT_EQUATOR: [f32; 3] = [0.01233, 0.01435, 0.01593];
pub const URP_AMBIENT_GROUND: [f32; 3] = [0.003671, 0.003335, 0.002709];
pub const URP_AMBIENT_INTENSITY: f32 = 1.0;

/// Valores que reproducen EXACTAMENTE la fórmula ambiente heredada
/// (hemisférica de dos paradas con factor 0,45–1,0) en las paradas
/// N.y ∈ {-1, 0, +1}; entre paradas el error es ≤ ~4 % del término
/// ambiente (dictamen G0/G1).
pub const LEGACY_AMBIENT_SKY: [f32; 3] = [0.82, 0.88, 1.0];
pub const LEGACY_AMBIENT_EQUATOR: [f32; 3] = [0.522, 0.52925, 0.561875];
pub const LEGACY_AMBIENT_GROUND: [f32; 3] = [0.279, 0.261, 0.2475];
pub const LEGACY_AMBIENT_INTENSITY_LIT: f32 = 0.25;
pub const LEGACY_AMBIENT_INTENSITY_UNLIT: f32 = 0.06;

/// Post de escena (G2): exposición previa al ACES, intensidad del
/// bloom y viñeta. Valores por defecto = URP (Post Exposure 0 EV).
/// El umbral y la rodilla del bloom NO son datos de escena: siguen
/// siendo constantes del motor (ver `crate::post` / glass_renderer).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PostComponent {
    pub exposure: f32,
    pub intensity: f32,
    pub vignette: f32,
}

impl Default for PostComponent {
    fn default() -> Self {
        Self { exposure: 1.0, intensity: 0.3, vignette: 0.22 }
    }
}

/// Valores heredados del motor (lo que hoy es constante/entorno):
/// la exposición 1,5 que vivía en el post; bloom y viñeta iguales
/// a los defectos actuales.
pub const LEGACY_POST_EXPOSURE: f32 = 1.5;
pub const LEGACY_POST_INTENSITY: f32 = 0.3;
pub const LEGACY_POST_VIGNETTE: f32 = 0.22;

/// Origen de los valores de post resueltos para una escena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostSource {
    /// Hay al menos una entidad activa con `PostComponent` explícito.
    Explicit,
    /// Ninguna entidad lo declara: valores heredados, look de siempre.
    Legacy,
}

/// Resultado de resolver el post efectivo de una escena.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedPost {
    pub exposure: f32,
    pub intensity: f32,
    pub vignette: f32,
    pub source: PostSource,
    /// Portadores activos adicionales ignorados (gana el primero).
    pub extra_carriers: usize,
}

impl ResolvedPost {
    /// El post heredado (sin portador): LEGACY_POST_*.
    pub fn legacy() -> Self {
        Self {
            exposure: LEGACY_POST_EXPOSURE,
            intensity: LEGACY_POST_INTENSITY,
            vignette: LEGACY_POST_VIGNETTE,
            source: PostSource::Legacy,
            extra_carriers: 0,
        }
    }
}

/// Resuelve el post efectivo de la escena: la primera entidad ACTIVA
/// con `post` gana; sus canales se sanean (no finito o negativo →
/// defecto del componente) y se acotan: exposición 0..=8, intensidad
/// 0..=4, viñeta 0..=1. Sin portador, modo Legacy.
pub fn resolve_post(entities: &[EntityData]) -> ResolvedPost {
    let carriers: Vec<&PostComponent> = entities
        .iter()
        .filter(|e| e.active)
        .filter_map(|e| e.post.as_ref())
        .collect();

    let Some(first) = carriers.first().copied() else {
        return ResolvedPost::legacy();
    };

    let default = PostComponent::default();
    let sanitize = |v: f32, fallback: f32, lo: f32, hi: f32| -> f32 {
        if !v.is_finite() || v < 0.0 {
            fallback
        } else {
            v.clamp(lo, hi)
        }
    };

    ResolvedPost {
        exposure: sanitize(first.exposure, default.exposure, 0.0, 8.0),
        intensity: sanitize(first.intensity, default.intensity, 0.0, 4.0),
        vignette: sanitize(first.vignette, default.vignette, 0.0, 1.0),
        source: PostSource::Explicit,
        extra_carriers: carriers.len() - 1,
    }
}


/// Componente de ambiente de escena (G1): gradient de 3 paradas en
/// espacio lineal — `sky` hacia arriba, `equator` en horizontal y
/// `ground` hacia abajo — modulado por `intensity` (0..=8, tipo URP).
/// Lo porta cualquier entidad; la resolución usa la primera activa.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AmbientComponent {
    pub sky: [f32; 3],
    pub equator: [f32; 3],
    pub ground: [f32; 3],
    pub intensity: f32,
}

impl Default for AmbientComponent {
    fn default() -> Self {
        Self {
            sky: URP_AMBIENT_SKY,
            equator: URP_AMBIENT_EQUATOR,
            ground: URP_AMBIENT_GROUND,
            intensity: URP_AMBIENT_INTENSITY,
        }
    }
}

/// Origen de los valores de ambiente resueltos para una escena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmbientSource {
    /// Hay al menos una entidad activa con `AmbientComponent` explícito.
    Explicit,
    /// Ninguna entidad lo declara: valores heredados, look de siempre.
    Legacy,
}

/// Resultado de resolver el ambiente efectivo de una escena.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedAmbient {
    pub sky: [f32; 3],
    pub equator: [f32; 3],
    pub ground: [f32; 3],
    /// `None` en modo Legacy: la rama consumidora conserva su literal
    /// de intensidad actual (0,25 con sol / 0,06 sin sol).
    pub intensity: Option<f32>,
    pub source: AmbientSource,
    /// Portadores activos adicionales ignorados (gana el primero).
    pub extra_carriers: usize,
}

impl ResolvedAmbient {
    /// El ambiente heredado (sin portador): colores LEGACY_* e
    /// intensidad None.
    pub fn legacy() -> Self {
        Self {
            sky: LEGACY_AMBIENT_SKY,
            equator: LEGACY_AMBIENT_EQUATOR,
            ground: LEGACY_AMBIENT_GROUND,
            intensity: None,
            source: AmbientSource::Legacy,
            extra_carriers: 0,
        }
    }
}

/// Resuelve el ambiente efectivo de la escena: la primera entidad
/// ACTIVA con `ambient` gana; sus canales se sanean (no finitos o
/// negativos → 0, sin tope superior) y la intensidad se acota a 0..=8
/// (no finita → 1,0). Sin portador, modo Legacy.
pub fn resolve_ambient(entities: &[EntityData]) -> ResolvedAmbient {
    let carriers: Vec<&AmbientComponent> = entities
        .iter()
        .filter(|e| e.active)
        .filter_map(|e| e.ambient.as_ref())
        .collect();

    let Some(first) = carriers.first().copied() else {
        return ResolvedAmbient::legacy();
    };

    let sanitize_channel = |v: f32| -> f32 {
        if !v.is_finite() || v < 0.0 { 0.0 } else { v }
    };
    let sanitize_rgb = |c: [f32; 3]| -> [f32; 3] {
        [sanitize_channel(c[0]), sanitize_channel(c[1]), sanitize_channel(c[2])]
    };
    let intensity = if !first.intensity.is_finite() {
        1.0
    } else {
        first.intensity.clamp(0.0, 8.0)
    };

    ResolvedAmbient {
        sky: sanitize_rgb(first.sky),
        equator: sanitize_rgb(first.equator),
        ground: sanitize_rgb(first.ground),
        intensity: Some(intensity),
        source: AmbientSource::Explicit,
        extra_carriers: carriers.len() - 1,
    }
}

/// Migración G1 de una escena: si ninguna entidad activa porta
/// ambiente, añade AL FINAL una entidad "Ambiente" con los valores
/// heredados equivalentes. La intensidad es 0,25 si la escena tiene
/// luz principal — el mismo predicado de `SceneState::main_light`
/// (entidad activa con luz, de cualquier tipo) — y 0,06 si no.
/// Devuelve la intensidad escrita, o None si ya había portador
/// (idempotente). `new_id` es el id para la entidad nueva.
pub fn migrate_scene_ambient(entities: &mut Vec<EntityData>, new_id: u64) -> Option<f32> {
    if resolve_ambient(entities).source == AmbientSource::Explicit {
        return None;
    }
    let has_main_light = entities.iter().any(|e| e.active && e.light.is_some());
    let intensity = if has_main_light {
        LEGACY_AMBIENT_INTENSITY_LIT
    } else {
        LEGACY_AMBIENT_INTENSITY_UNLIT
    };
    let mut carrier = EntityData::new(
        new_id,
        "Ambiente".into(),
        vec!["Transform".into(), "Ambiente".into()],
    );
    carrier.ambient = Some(AmbientComponent {
        sky: LEGACY_AMBIENT_SKY,
        equator: LEGACY_AMBIENT_EQUATOR,
        ground: LEGACY_AMBIENT_GROUND,
        intensity,
    });
    entities.push(carrier);
    Some(intensity)
}

/// Migración G2 de una escena: si ninguna entidad activa porta post,
/// añade AL FINAL una entidad "Post" con los valores heredados
/// equivalentes (exposición 1,5, bloom 0,3, viñeta 0,22). Devuelve
/// la exposición escrita, o None si ya había portador (idempotente).
/// `new_id` es el id para la entidad nueva.
pub fn migrate_scene_post(entities: &mut Vec<EntityData>, new_id: u64) -> Option<f32> {
    if resolve_post(entities).source == PostSource::Explicit {
        return None;
    }

    let mut nueva = EntityData::new(
        new_id,
        "Post".to_string(),
        vec!["Transform".to_string(), "Post".to_string()],
    );
    nueva.post = Some(PostComponent {
        exposure: LEGACY_POST_EXPOSURE,
        intensity: LEGACY_POST_INTENSITY,
        vignette: LEGACY_POST_VIGNETTE,
    });

    entities.push(nueva);
    Some(LEGACY_POST_EXPOSURE)
}

#[cfg(test)]
mod migrate_post_tests {
    use super::*;

    fn entidad_base(id: u64, nombre: &str) -> EntityData {
        EntityData::new(id, nombre.to_string(), vec!["Transform".to_string()])
    }

    #[test]
    fn migrate_adds_carrier() {
        let mut entidades = vec![entidad_base(1, "Main Camera"), entidad_base(2, "Directional Light")];
        let resultado = migrate_scene_post(&mut entidades, 3);

        assert_eq!(resultado, Some(LEGACY_POST_EXPOSURE));
        assert_eq!(entidades.len(), 3);

        let ultima = entidades.last().unwrap();
        assert_eq!(ultima.name, "Post");
        let post = ultima.post.as_ref().expect("la nueva entidad debe portar post");
        assert_eq!(post.exposure, LEGACY_POST_EXPOSURE);
        assert_eq!(post.intensity, LEGACY_POST_INTENSITY);
        assert_eq!(post.vignette, LEGACY_POST_VIGNETTE);
    }

    #[test]
    fn migrate_is_idempotent() {
        let mut entidades = vec![entidad_base(1, "Main Camera")];
        migrate_scene_post(&mut entidades, 2);
        let len_tras_primera = entidades.len();

        let resultado = migrate_scene_post(&mut entidades, 3);

        assert_eq!(resultado, None);
        assert_eq!(entidades.len(), len_tras_primera);
    }

    #[test]
    fn migrate_preserves_other_entities() {
        let mut entidades = vec![entidad_base(1, "Main Camera"), entidad_base(2, "Directional Light")];
        migrate_scene_post(&mut entidades, 3);

        assert_eq!(entidades[0].id, 1);
        assert_eq!(entidades[0].name, "Main Camera");
        assert_eq!(entidades[1].id, 2);
        assert_eq!(entidades[1].name, "Directional Light");
    }

    #[test]
    fn migrate_skips_when_carrier_exists() {
        let mut portador = entidad_base(1, "Post");
        portador.post = Some(PostComponent::default());
        let mut entidades = vec![portador];

        let resultado = migrate_scene_post(&mut entidades, 2);

        assert_eq!(resultado, None);
        assert_eq!(entidades.len(), 1);
    }
}
    /// Verificador G2: cada `*.scene.json` del directorio de migración
    /// carga con el cargador del motor y resuelve su post como Explicit
    /// con exactamente un portador y los valores heredados.
    /// `cargo test -p glass_core -- --ignored verify_scene_files_post`
    #[test]
    #[ignore]
    fn verify_scene_files_post() {
        let dir = std::env::var("GLASS_MIGRATE_DIR")
            .unwrap_or_else(|_| "../../assets/scenes".to_string());
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("no se pudo leer {dir}: {e}"))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().ends_with(".scene.json"))
                    .unwrap_or(false)
            })
            .collect();
        paths.sort();
        assert!(!paths.is_empty(), "sin escenas en {dir}");
        for path in &paths {
            let mut state = SceneState::new();
            state
                .load_from_json(path.to_str().unwrap())
                .unwrap_or_else(|e| panic!("carga {}: {e}", path.display()));
            let post = state.post_settings();
            assert_eq!(
                post.source,
                PostSource::Explicit,
                "{} sin portador explícito",
                path.display()
            );
            assert_eq!(post.extra_carriers, 0, "{} con portadores extra", path.display());
            assert_eq!(post.exposure, LEGACY_POST_EXPOSURE);
            assert_eq!(post.intensity, LEGACY_POST_INTENSITY);
            assert_eq!(post.vignette, LEGACY_POST_VIGNETTE);
        }
        println!("verificadas {} escenas", paths.len());
    }


fn default_tope_perfil() -> u8 { 4 }
fn default_prioridad_perfil() -> f32 { 1.0 }

/// Perfil de iluminación de una luz (integración de iluminación):
/// política que la asignación de luces por objeto usa para puntuar,
/// agrupar y limitar cuántas luces de este perfil alumbran a un
/// mismo objeto, y si la luz proyecta sombra según su prioridad.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LightProfileData {
    /// Nombre del preset aplicado ("Vela", "Antorcha", "LinternaArma",
    /// "Panteon" o "Custom" si los valores se editaron a mano).
    pub preset: String,
    /// Máximo de luces de ESTE grupo que alumbran a un mismo objeto.
    #[serde(default = "default_tope_perfil")]
    pub tope_por_objeto: u8,
    /// Grupo de asignación: los topes se aplican por grupo.
    #[serde(default)]
    pub grupo: u8,
    /// Peso en el score de asignación (más = se asigna antes).
    #[serde(default = "default_prioridad_perfil")]
    pub prioridad: f32,
    /// Si es true, la luz solo proyecta sombra cuando su score medio
    /// supera `umbral_sombra` (con 4 cubos globales no caben todas).
    #[serde(default)]
    pub sombra_por_prioridad: bool,
    #[serde(default)]
    pub umbral_sombra: f32,
}

impl Default for LightProfileData {
    fn default() -> Self {
        Self {
            preset: "Custom".into(),
            tope_por_objeto: 4,
            grupo: 0,
            prioridad: 1.0,
            sombra_por_prioridad: false,
            umbral_sombra: 0.0,
        }
    }
}

impl LightProfileData {
    /// Perfil de un preset conocido; `None` si el nombre no existe.
    pub fn preset(nombre: &str) -> Option<Self> {
        match nombre {
            "Vela" => Some(Self {
                preset: "Vela".into(),
                tope_por_objeto: 4,
                grupo: 0,
                prioridad: 0.3,
                sombra_por_prioridad: false,
                umbral_sombra: 0.0,
            }),
            "Antorcha" => Some(Self {
                preset: "Antorcha".into(),
                tope_por_objeto: 4,
                grupo: 0,
                prioridad: 0.6,
                sombra_por_prioridad: false,
                umbral_sombra: 0.0,
            }),
            "LinternaArma" => Some(Self {
                preset: "LinternaArma".into(),
                tope_por_objeto: 2,
                grupo: 1,
                prioridad: 1.5,
                sombra_por_prioridad: false,
                umbral_sombra: 0.0,
            }),
            "Panteon" => Some(Self {
                preset: "Panteon".into(),
                tope_por_objeto: 4,
                grupo: 0,
                prioridad: 1.0,
                sombra_por_prioridad: true,
                umbral_sombra: 1.0,
            }),
            _ => None,
        }
    }

    /// Perfil implícito de una luz sin componente de perfil.
    pub fn implicito() -> Self {
        Self {
            preset: "Custom".into(),
            tope_por_objeto: 8,
            grupo: 0,
            prioridad: 1.0,
            sombra_por_prioridad: false,
            umbral_sombra: 0.0,
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

/// `raycast_target` por defecto de un Graphic (fase UI/Canvas): los
/// elementos dibujables reciben puntero salvo que se desactive.
fn default_raycast_target() -> bool {
    true
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
    /// Estilo CSS extra del elemento (fase UI/Canvas): declaraciones y
    /// bloques de pseudo-clase que la cascada aplica sobre los campos
    /// del componente. Vacío = sin extra. Aditivo: las escenas antiguas
    /// no lo traen.
    #[serde(default)]
    pub style_extra: String,
    /// Si el elemento participa en el hit-test de puntero.
    #[serde(default = "default_raycast_target")]
    pub raycast_target: bool,
}

impl Default for UITextData {
    fn default() -> Self {
        Self {
            text: "Texto UI".into(),
            font_size: 18.0,
            color: [1.0, 1.0, 1.0, 1.0],
            font: "default".into(),
            align: "Left".into(),
            style_extra: String::new(),
            raycast_target: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIButtonData {
    pub interactable: bool,
    pub target_graphic_color: [f32; 4],
    pub on_click_action: String,
    /// Estilo CSS extra del elemento (fase UI/Canvas), como en
    /// `UITextData::style_extra`.
    #[serde(default)]
    pub style_extra: String,
}

impl Default for UIButtonData {
    fn default() -> Self {
        Self {
            interactable: true,
            target_graphic_color: [0.25, 0.55, 0.85, 1.0],
            on_click_action: "".into(),
            style_extra: String::new(),
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
    /// Estilo CSS extra del elemento (fase UI/Canvas), como en
    /// `UITextData::style_extra`.
    #[serde(default)]
    pub style_extra: String,
    /// Si el elemento participa en el hit-test de puntero.
    #[serde(default = "default_raycast_target")]
    pub raycast_target: bool,
}

impl Default for UIImageData {
    fn default() -> Self {
        Self {
            sprite: String::new(),
            color: [1.0, 1.0, 1.0, 1.0],
            preserve_aspect: false,
            style_extra: String::new(),
            raycast_target: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
/// la escena (Fase 5): cada subárbol usa el Canvas Scaler de su
/// Canvas activo más cercano (escala 1 sin Canvas), compone la
/// jerarquía (cada hijo contra el rect de su ancestro UI más
/// cercano) y aplica los layout groups por profundidad ascendente,
/// iterando hasta punto fijo (máx. 4 pasadas) para que los
/// descendientes de los hijos de layout se resuelvan contra el rect
/// impuesto. Las entidades sin `rect_transform` no aparecen.
pub fn resolve_scene_ui_rects(
    scene: &SceneState,
    screen_w: f32,
    screen_h: f32,
) -> std::collections::HashMap<u64, [f32; 4]> {
    use std::collections::HashMap;

    let by_id: HashMap<u64, &EntityData> =
        scene.entities.iter().map(|e| (e.id, e)).collect();

    // Escala de una entidad: la del Canvas ACTIVO más cercano (ella
    // misma incluida); los Canvas inactivos se saltan. Sin Canvas → 1.
    fn escala_de(
        ent: &EntityData,
        by_id: &HashMap<u64, &EntityData>,
        screen_w: f32,
        screen_h: f32,
    ) -> f32 {
        let mut cur: Option<&EntityData> = Some(ent);
        let mut pasos = 0u32;
        while let Some(e) = cur {
            if e.active {
                if let Some(c) = e.canvas.as_ref() {
                    return canvas_scale_factor(c, screen_w, screen_h);
                }
            }
            pasos += 1;
            if pasos > 64 {
                break;
            }
            cur = e.parent.and_then(|pid| by_id.get(&pid).copied());
        }
        1.0
    }
    let escalas: HashMap<u64, f32> = scene
        .entities
        .iter()
        .map(|e| (e.id, escala_de(e, &by_id, screen_w, screen_h)))
        .collect();

    fn resolver(
        ent: &EntityData,
        by_id: &HashMap<u64, &EntityData>,
        escalas: &HashMap<u64, f32>,
        pantalla: [f32; 2],
        impuestos: &HashMap<u64, [f32; 4]>,
        memo: &mut HashMap<u64, [f32; 4]>,
        visitando: &mut Vec<u64>,
    ) -> [f32; 4] {
        if let Some(r) = memo.get(&ent.id) {
            return *r;
        }
        let s = escalas.get(&ent.id).copied().unwrap_or(1.0);
        let root: [f32; 4] = [0.0, 0.0, pantalla[0] / s, pantalla[1] / s];
        // El Canvas ES el espacio raíz (como en Unity: su
        // RectTransform es la pantalla en unidades de referencia,
        // no su rect autorado).
        if ent.canvas.is_some() {
            memo.insert(ent.id, root);
            return root;
        }
        // Rect impuesto por un layout group en la pasada anterior.
        if let Some(r) = impuestos.get(&ent.id) {
            memo.insert(ent.id, *r);
            return *r;
        }
        if visitando.contains(&ent.id) {
            return root; // ciclo en la jerarquía: se corta contra la raíz
        }
        visitando.push(ent.id);
        // Padre: el ancestro UI más cercano (con rect_transform o Canvas).
        let mut padre_rect = root;
        let mut cur = ent.parent;
        let mut pasos = 0u32;
        while let Some(pid) = cur {
            pasos += 1;
            if pasos > 64 {
                padre_rect = root; // guarda de ciclos: se trata como raíz
                break;
            }
            let Some(p) = by_id.get(&pid) else {
                // Menor de UI de la auditoría: un padre ausente del
                // mapa se resolvía contra la pantalla EN SILENCIO.
                // Ahora avisa (una sola vez por entidad y proceso).
                static AVISADOS: std::sync::Mutex<Vec<u64>> =
                    std::sync::Mutex::new(Vec::new());
                if let Ok(mut v) = AVISADOS.lock() {
                    if !v.contains(&ent.id) {
                        v.push(ent.id);
                        log::warn!(
                            "UI: la entidad {} declara un padre {} que no existe en la escena; se resuelve contra la pantalla.",
                            ent.id,
                            pid
                        );
                    }
                }
                break;
            };
            if p.rect_transform.is_some() || p.canvas.is_some() {
                if p.rect_transform.is_some() {
                    padre_rect = resolver(
                        p, by_id, escalas, pantalla, impuestos, memo, visitando,
                    );
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

    // Layout groups por profundidad ascendente (los exteriores primero).
    fn profundidad(ent: &EntityData, by_id: &HashMap<u64, &EntityData>) -> u32 {
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
        .filter(|e| e.layout_group.is_some() && e.rect_transform.is_some())
        .collect();
    grupos.sort_by_key(|e| profundidad(e, &by_id));

    let pantalla = [screen_w, screen_h];
    let mut impuestos: HashMap<u64, [f32; 4]> = HashMap::new();
    let mut memo: HashMap<u64, [f32; 4]> = HashMap::new();
    for _ in 0..4 {
        memo = HashMap::new();
        for ent in scene
            .entities
            .iter()
            .filter(|e| e.rect_transform.is_some())
        {
            let mut visitando = Vec::new();
            resolver(
                ent, &by_id, &escalas, pantalla, &impuestos, &mut memo, &mut visitando,
            );
        }

        let mut cambio = false;
        for g in &grupos {
            let Some(grect) = memo.get(&g.id).copied() else { continue };
            let hijos: Vec<u64> = scene
                .entities
                .iter()
                .filter(|e| e.parent == Some(g.id) && e.active && memo.contains_key(&e.id))
                .map(|e| e.id)
                .collect();
            if hijos.is_empty() {
                continue;
            }
            let rects: Vec<[f32; 4]> = hijos.iter().map(|id| memo[id]).collect();
            let nuevos = layout_group_rects(grect, g.layout_group.as_ref().unwrap(), &rects);
            for (id, nr) in hijos.iter().zip(nuevos) {
                if impuestos.get(id) != Some(&nr) {
                    cambio = true;
                }
                impuestos.insert(*id, nr);
                memo.insert(*id, nr);
            }
        }
        if !cambio {
            break;
        }
    }

    // A píxeles, cada entidad con la escala de su Canvas.
    for (id, r) in memo.iter_mut() {
        let s = escalas.get(id).copied().unwrap_or(1.0);
        r[0] *= s;
        r[1] *= s;
        r[2] *= s;
        r[3] *= s;
    }
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
    /// Character Controller autorado en el editor (se crea en la física al entrar en Play).
    #[serde(default)]
    pub character: Option<CharacterData>,
    pub audio: Option<AudioSourceData>,
    /// Emisor de partículas (Fase 8P).
    #[serde(default)]
    pub particles: Option<ParticleEmitterData>,
    pub camera: Option<CameraData>,
    pub light: Option<LightData>,
    /// Perfil de iluminación (integración de iluminación).
    /// `#[serde(default)]`: las escenas anteriores no traen el campo.
    #[serde(default)]
    pub light_profile: Option<LightProfileData>,
    /// Bloque de niebla de escena (Fase 2). `#[serde(default)]`: las
    /// escenas anteriores no traen el campo.
    #[serde(default)]
    pub fog: Option<FogData>,
    /// Ambiente de escena (G1). `#[serde(default)]`: las escenas
    /// anteriores no traen el campo.
    #[serde(default)]
    pub ambient: Option<AmbientComponent>,
    /// Post de escena (G2). `#[serde(default)]`: las escenas
    /// anteriores no traen el campo.
    #[serde(default)]
    pub post: Option<PostComponent>,
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
    /// Prefab vivo (Fase A): ruta del `.prefab.json`, relativa al proyecto.
    /// Solo en la raíz de una instancia.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefab_source: Option<String>,
    /// Prefab vivo: huella de la instancia al instanciar/actualizar. Solo en la raíz.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefab_hash: Option<String>,
    /// Prefab vivo: id de esta entidad dentro del prefab. En todo el subárbol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefab_local_id: Option<u64>,
    /// Prefab vivo (Fase D): overrides de esta instancia frente a su asset.
    /// Solo en la raíz de la instancia.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefab_overrides: Option<crate::prefab::PrefabOverrides>,
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
        let tiene_rigidbody = components.iter().any(|c| c == "RigidBody");
        let tiene_collider = components.iter().any(|c| c == "Collider");
        let physics = if !tiene_rigidbody && tiene_collider {
            // Regla refinada de D3m3tr10 (2026-11-18): sin RigidBody ni
            // CharacterController no hay física DINÁMICA. Un Collider solo
            // es geometría estática, como en Unity. "StaticCollider" a secas
            // sigue sin crear física aquí (el spawn la materializa).
            Some(PhysicsData {
                body_type: "Static".into(),
                has_rigidbody: false,
                has_collider: true,
                ..PhysicsData::default()
            })
        } else if tiene_rigidbody {
            // Con RigidBody: cuerpo según is_static y collider inferido
            // incluido (el spawn por código depende de este contrato).
            let body_type = if is_static { "Static".into() } else { "Dynamic".into() };
            Some(PhysicsData {
                body_type,
                has_rigidbody: true,
                has_collider: true,
                mass: 1.0,
                // Sin legado: el comportamiento (0.7 / 0.05) sale de
                // PROJECT_DEFAULT vía body_fallback_material.
                legacy_friction: None,
                legacy_restitution: None,
                box_extents: [0.5, 0.5, 0.5],
                box_offset: [0.0, 0.0, 0.0],
                linear_velocity: [0.0, 0.0, 0.0],
                layer: 0,
                collision_mask: u32::MAX,
                is_trigger: false,
                constraints: 0,
                linear_damping: 1.5,
                solver_velocity_iterations: 4,
                include_layers: u32::MAX,
                exclude_layers: 0,
                automatic_center_of_mass: true,
                automatic_inertia_tensor: true,
                angular_damping: 0.0,
                solver_iterations: 4,
                max_depenetration_velocity: 0.0,
                use_gravity: true,
                ccd: false,
                interpolate: false,
                colliders: Vec::new(),
                mesh_collider: MeshColliderMode::None,
                mesh_collider_material: String::new(),
                mesh_collider_is_trigger: false,
            })
        } else {
            None
        };
        let audio = if components.iter().any(|c| c == "AudioSource") {
            Some(AudioSourceData::default())
        } else {
            None
        };
        let particles = if components.iter().any(|c| c == "ParticleEmitter") {
            Some(ParticleEmitterData::default())
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
        let light_profile = if components.iter().any(|c| c == "LightProfile") {
            Some(LightProfileData::default())
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
            character: None,
            audio,
            particles,
            camera,
            light,
            light_profile,
            fog: None,
            ambient: None,
            post: None,
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
            prefab_source: None,
            prefab_hash: None,
            prefab_local_id: None,
            prefab_overrides: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SceneState {
    pub entities: Vec<EntityData>,
    pub next_entity_id: u64,
    /// Versión del esquema de UI de la escena (fase UI/Canvas):
    /// 0 = formato anterior a la fase (raíz JSON = array de entidades);
    /// 1 = raíz objeto con `ui_schema_version`. La migración vive en
    /// el crate `glass_ui`; aquí solo se conserva el número al
    /// cargar/guardar.
    pub ui_schema_version: u32,
    pub viewport_snapshot_png: Option<Vec<u8>>,
    pub shaders_reloaded_flag: bool,
    /// Caché SoA de transforms (Fase 7): la resolución de mundos
    /// trabaja sobre arrays contiguos; la fuente autoral sigue
    /// siendo `EntityData.transform`.
    pub transform_store: std::cell::RefCell<crate::transforms::TransformStore>,
    /// Índice id -> posición en `entities`. No asumir que está
    /// siempre sincronizado: código externo puede mutar `entities`
    /// directamente (push/retain en tests de canvas_ui.rs). Usar
    /// siempre `ensure_index`/`index_lookup` antes de leerlo.
    id_index: std::cell::RefCell<HashMap<u64, usize>>,
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
            ui_schema_version: UI_SCHEMA_VERSION_ACTUAL,
            viewport_snapshot_png: None,
            shaders_reloaded_flag: false,
            transform_store: std::cell::RefCell::new(crate::transforms::TransformStore::new()),
            id_index: std::cell::RefCell::new(HashMap::new()),
        }
    }
}

/// Copia de seguridad de la versión anterior de una escena al
/// guardar (auditoría C9): `<ruta>.snapshots/<nombre>-<millis>.json`,
/// con poda a las 10 más recientes. Sin archivo previo, no hace
/// nada; cualquier fallo se ignora (guardar nunca debe fallar por
/// culpa de la copia).
fn snapshot_previous_version(path: &str) {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return;
    }
    let Some(stem) = p.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let dir = p.with_extension("snapshots");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let destino = dir.join(format!("{stem}-{millis}.json"));
    if std::fs::copy(p, &destino).is_err() {
        return;
    }
    // Poda: solo las 10 instantáneas más recientes de ESTA escena.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut propias: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|q| {
                q.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with(stem))
                    .unwrap_or(false)
            })
            .collect();
        propias.sort();
        while propias.len() > 10 {
            let vieja = propias.remove(0);
            let _ = std::fs::remove_file(vieja);
        }
    }
}

/// Versión actual del esquema de UI al guardar una escena nueva
/// (fase UI/Canvas). La versión 0 es el formato anterior a la fase:
/// la raíz del JSON es directamente el array de entidades.
pub const UI_SCHEMA_VERSION_ACTUAL: u32 = 1;

/// Parsea el JSON de una escena aceptando las dos formas de raíz:
/// el array desnudo de entidades (versión 0) y el objeto
/// `{"ui_schema_version": N, "entities": [...]}` (versión N).
/// Devuelve la versión declarada y las entidades. Es el único punto
/// de parseo de escenas: lo usan `SceneState::load_from_json` y el
/// `SceneManager`.
pub fn parsear_escena(json: &str) -> Result<(u32, Vec<EntityData>), String> {
    let valor: serde_json::Value =
        serde_json::from_str(json).map_err(|e| e.to_string())?;
    if valor.is_array() {
        let entities: Vec<EntityData> =
            serde_json::from_value(valor).map_err(|e| e.to_string())?;
        return Ok((0, entities));
    }
    let version = valor
        .get("ui_schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let entities: Vec<EntityData> = match valor.get("entities") {
        Some(e) => serde_json::from_value(e.clone()).map_err(|e| e.to_string())?,
        None => return Err("escena sin campo 'entities'".into()),
    };
    Ok((version, entities))
}

/// Serializa una escena en la forma objeto actual (con versión).
pub fn serializar_escena(version: u32, entities: &[EntityData]) -> Result<String, String> {
    let valor = serde_json::json!({
        "ui_schema_version": version,
        "entities": entities,
    });
    serde_json::to_string_pretty(&valor).map_err(|e| e.to_string())
}

impl SceneState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconstruye el índice completo id->posición. O(N).
    fn rebuild_index(&self) {
        let mut idx = self.id_index.borrow_mut();
        idx.clear();
        idx.reserve(self.entities.len());
        for (pos, e) in self.entities.iter().enumerate() {
            idx.insert(e.id, pos);
        }
    }

    /// Verificación barata + reparación puntual antes de cada lookup.
    /// - Longitud distinta (push/retain externo, carga de escena):
    ///   reconstrucción completa.
    /// - Longitud igual pero entrada de `id` desalineada: búsqueda
    ///   lineal puntual y reparación solo de esa entrada.
    fn ensure_index(&self, id: u64) {
        let needs_full_rebuild = {
            let idx = self.id_index.borrow();
            idx.len() != self.entities.len()
        };
        if needs_full_rebuild {
            self.rebuild_index();
            return;
        }
        let stale = {
            let idx = self.id_index.borrow();
            match idx.get(&id) {
                Some(&pos) => self.entities.get(pos).map(|e| e.id != id).unwrap_or(true),
                None => false,
            }
        };
        if stale {
            if let Some(pos) = self.entities.iter().position(|e| e.id == id) {
                self.id_index.borrow_mut().insert(id, pos);
            } else {
                self.id_index.borrow_mut().remove(&id);
            }
        }
    }

    /// Resuelve la posición de `id` en `entities`, O(1) amortizado.
    fn index_lookup(&self, id: u64) -> Option<usize> {
        self.ensure_index(id);
        {
            let idx = self.id_index.borrow();
            if let Some(&pos) = idx.get(&id) {
                if self.entities.get(pos).map(|e| e.id == id).unwrap_or(false) {
                    return Some(pos);
                }
            }
        }
        // Fallback defensivo: búsqueda lineal + reparación.
        if let Some(pos) = self.entities.iter().position(|e| e.id == id) {
            self.id_index.borrow_mut().insert(id, pos);
            return Some(pos);
        }
        None
    }

    pub fn create_entity(&mut self, name: String, components: Vec<String>) -> u64 {
        let id = self.next_entity_id;
        self.next_entity_id += 1;
        // Mantener el índice sincronizado incrementalmente: O(1).
        // Si estaba desincronizado (mutación externa previa), el
        // próximo lookup lo reconstruirá por diferencia de longitud.
        let pos = self.entities.len();
        self.entities.push(EntityData::new(id, name, components));
        self.id_index.borrow_mut().insert(id, pos);
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

        // NOTA de orden: swap_remove reemplaza al borrado por el
        // último elemento del Vec. Único efecto observable: el orden
        // relativo de las entidades restantes en `entities` ya no se
        // preserva tras un borrado (antes sí, con Vec::remove). La
        // jerarquía (parent/children) no vive en el orden del Vec.
        if let Some(pos) = self.index_lookup(id) {
            self.entities.swap_remove(pos);
            let mut idx = self.id_index.borrow_mut();
            idx.remove(&id);
            // La entidad que estaba al final ahora ocupa `pos`
            // (salvo que `pos` fuera ya la última posición).
            if let Some(moved) = self.entities.get(pos) {
                let moved_id = moved.id;
                idx.insert(moved_id, pos);
            }
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

    /// Duplica una entidad con todo su subárbol: ids nuevos,
    /// `parent`/`children` remapeados dentro de la copia y la raíz
    /// copia como hermana de la original (mismo padre, registrada
    /// en su lista de hijos). Devuelve el id de la raíz copia.
    pub fn duplicate_entity(&mut self, id: u64) -> Option<u64> {
        // Recorrido del subárbol, la raíz primero.
        let mut orden: Vec<u64> = Vec::new();
        let mut pila = vec![id];
        while let Some(actual) = pila.pop() {
            let Some(ent) = self.get_entity(actual) else {
                continue;
            };
            orden.push(actual);
            for hijo in ent.children.iter().rev() {
                pila.push(*hijo);
            }
        }
        if orden.is_empty() {
            return None;
        }
        let padre_raiz = self.get_entity(id).and_then(|e| e.parent);

        // Mapa de ids viejos a nuevos, en el orden del recorrido.
        let mut mapa: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        for viejo in &orden {
            let nuevo = self.next_entity_id;
            self.next_entity_id += 1;
            mapa.insert(*viejo, nuevo);
        }

        // Clonar con los punteros remapeados y empujar manteniendo
        // el índice de ids sincronizado, como hace create_entity.
        let clones: Vec<EntityData> = orden
            .iter()
            .filter_map(|viejo| {
                let mut copia = self.get_entity(*viejo)?.clone();
                copia.id = *mapa.get(viejo)?;
                copia.parent = if *viejo == id {
                    padre_raiz
                } else {
                    copia.parent.and_then(|p| mapa.get(&p).copied())
                };
                copia.children = copia
                    .children
                    .iter()
                    .filter_map(|h| mapa.get(h).copied())
                    .collect();
                Some(copia)
            })
            .collect();
        let raiz_nueva = *mapa.get(&id)?;
        for copia in clones {
            let pos = self.entities.len();
            let cid = copia.id;
            self.entities.push(copia);
            self.id_index.borrow_mut().insert(cid, pos);
        }
        if let Some(pid) = padre_raiz {
            if let Some(p) = self.get_entity_mut(pid) {
                if !p.children.contains(&raiz_nueva) {
                    p.children.push(raiz_nueva);
                }
            }
        }
        Some(raiz_nueva)
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
        let pos = self.index_lookup(id)?;
        self.entities.get(pos)
    }

    pub fn get_entity_mut(&mut self, id: u64) -> Option<&mut EntityData> {
        let pos = self.index_lookup(id)?;
        self.entities.get_mut(pos)
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
                    object: String::new(),
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

    /// Guarda el estado de la escena en un archivo JSON en disco.
    ///
    /// Auditoría C9 (mitigación ante la ausencia de undo/redo): si el
    /// archivo ya existe, su contenido anterior se copia primero a
    /// `<escena>.snapshots/<nombre>-<millis>.json`, conservando las
    /// 10 instantáneas más recientes por escena.
    pub fn save_to_json(&self, path: &str) -> std::io::Result<()> {
        let json = serializar_escena(self.ui_schema_version, &self.entities)
            .map_err(std::io::Error::other)?;
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        snapshot_previous_version(path);
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

    /// Ambiente efectivo de la escena (G1): ver `resolve_ambient`.
    pub fn ambient_settings(&self) -> ResolvedAmbient {
        resolve_ambient(&self.entities)
    }

    /// Nombre de la entidad que aporta el ambiente efectivo (la
    /// primera activa con el componente), si alguna lo declara.
    pub fn ambient_carrier_name(&self) -> Option<String> {
        self.entities
            .iter()
            .find(|e| e.active && e.ambient.is_some())
            .map(|e| e.name.clone())
    }

    /// Post efectivo de la escena (G2): ver `resolve_post`.
    pub fn post_settings(&self) -> ResolvedPost {
        resolve_post(&self.entities)
    }

    /// Nombre de la entidad que aporta el post efectivo (la primera
    /// activa con el componente), si alguna lo declara.
    pub fn post_carrier_name(&self) -> Option<String> {
        self.entities
            .iter()
            .find(|e| e.active && e.post.is_some())
            .map(|e| e.name.clone())
    }

    pub fn replace_entities(&mut self, entities: Vec<EntityData>) {
        self.entities = entities;
        self.next_entity_id = self.entities.iter().map(|e| e.id).max().unwrap_or(0) + 1;
    }

    /// Carga entidades desde un archivo JSON reemplazando la escena actual
    pub fn load_from_json(&mut self, path: &str) -> std::io::Result<()> {
        let json = std::fs::read_to_string(path)?;
        let (version, entities) =
            parsear_escena(&json).map_err(std::io::Error::other)?;
        self.ui_schema_version = version;
        self.replace_entities(entities);
        Ok(())
    }

    /// Instancia un Prefab dentro de la escena, reasignando IDs únicos y emparentando correctamente.
    pub fn instantiate_prefab(
        &mut self,
        prefab: &crate::prefab::Prefab,
        parent_id: Option<u64>,
        world_pos: Option<[f32; 3]>,
        fuente: Option<&str>,
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
        // Vínculo vivo: la raíz guarda fuente y huella; todos los clones, su id en el prefab.
        let vinculo = fuente.is_some();
        root_clone.prefab_source = fuente.map(str::to_owned);
        root_clone.prefab_hash = fuente.map(|_| prefab.huella());
        root_clone.prefab_local_id = vinculo.then_some(prefab.root_entity.id);
        root_clone.prefab_overrides = None;

        // Si se emparentó a un padre existente, añadir el nuevo id a sus hijos
        if let Some(pid) = parent_id {
            if let Some(p) = self.get_entity_mut(pid) {
                if !p.children.contains(&new_root_id) {
                    p.children.push(new_root_id);
                }
            }
        }

        self.entities.push(root_clone);

        // Ids del asset que cuelgan de una raíz anidada (sin contar la
        // propia raíz anidada): conservan el `prefab_local_id` que traen
        // del asset, que es su id en el asset interior.
        let mut dentro_de_anidada: std::collections::HashSet<u64> = std::collections::HashSet::new();
        for desc in &prefab.descendant_entities {
            if desc.prefab_source.is_some() {
                let sub = crate::prefab::recorrer_subarbol(
                    std::iter::once(&prefab.root_entity).chain(&prefab.descendant_entities),
                    desc.id,
                );
                for n in sub.iter().skip(1) {
                    dentro_de_anidada.insert(n.e.id);
                }
            }
        }

        for desc in &prefab.descendant_entities {
            let mut desc_clone = desc.clone();
            let new_id = id_map[&desc.id];
            desc_clone.id = new_id;
            desc_clone.parent = desc.parent.and_then(|old_pid| id_map.get(&old_pid).copied());
            if desc.prefab_source.is_some() {
                // Raíz anidada: conserva SU vínculo (fuente y huella de su
                // asset, que trae el clon). Si esta instancia va vinculada,
                // su local_id es su hueco en el asset exterior; si no, se
                // queda con el que trae del asset (su id en el interior).
                if vinculo {
                    desc_clone.prefab_local_id = Some(desc.id);
                }
                desc_clone.prefab_overrides = None;
            } else if dentro_de_anidada.contains(&desc.id) {
                // Miembro de una instancia anidada: su local_id es su id
                // en el asset interior y viene ya en el clon.
                desc_clone.prefab_source = None;
                desc_clone.prefab_hash = None;
            } else {
                desc_clone.prefab_source = None;
                desc_clone.prefab_hash = None;
                desc_clone.prefab_local_id = vinculo.then_some(desc.id);
            }
            desc_clone.children = desc_clone
                .children
                .iter()
                .filter_map(|cid| id_map.get(cid).copied())
                .collect();
            self.entities.push(desc_clone);
        }

        new_root_id
    }

    /// Prefabs vivos (Fase A): sincroniza cada instancia vinculada con su asset.
    ///
    /// Por cada raíz con `prefab_source`: si el asset no se resuelve, o si la
    /// instancia se editó en la escena (su huella difiere de `prefab_hash`), no
    /// se toca y se devuelve un aviso. Si la instancia está intacta y el asset
    /// cambió, se re-expande conservando los ids de escena y, en la raíz, el
    /// transform y el padre externo.
    pub fn refrescar_instancias_prefab(
        &mut self,
        resolver: &dyn Fn(&str) -> Option<crate::prefab::Prefab>,
    ) -> Vec<String> {
        let raices: Vec<(u64, String, Option<String>)> = self
            .entities
            .iter()
            .filter_map(|e| {
                e.prefab_source
                    .as_ref()
                    .map(|s| (e.id, s.clone(), e.prefab_hash.clone()))
            })
            .collect();

        let mut avisos = Vec::new();
        for (raiz_id, fuente, guardada) in raices {
            // Una raíz anidada puede haber desaparecido al re-expandir
            // la instancia exterior que la contenía en esta misma pasada.
            if self.get_entity(raiz_id).is_none() {
                continue;
            }
            let Some(prefab) = resolver(&fuente) else {
                avisos.push(format!("#{raiz_id} ({fuente}): vínculo roto"));
                continue;
            };
            // La raíz se clavea con su id en SU asset: una raíz anidada
            // guarda en `prefab_local_id` su hueco en el asset exterior,
            // y con esa clave su huella nunca coincidiría con la del asset.
            let id_raiz_asset = prefab.root_entity.id;
            let huella_escena = crate::prefab::huella_subarbol(&self.entities, raiz_id, |e| {
                if e.id == raiz_id {
                    id_raiz_asset
                } else {
                    e.prefab_local_id.unwrap_or(u64::MAX)
                }
            });
            let huella_asset = prefab.huella();
            if guardada.as_deref() == Some(huella_escena.as_str()) {
                // Intacta: solo se mueve si el asset cambió.
                if huella_asset != huella_escena {
                    self.reexpandir_instancia(raiz_id, &prefab);
                    if let Some(raiz) = self.get_entity_mut(raiz_id) {
                        raiz.prefab_hash = Some(huella_asset);
                        raiz.prefab_overrides = None;
                    }
                }
                continue;
            }
            // Modificada: sin diff guardado, comportamiento de la Fase A
            // (no se pisa jamás en silencio).
            let overrides = self
                .get_entity(raiz_id)
                .and_then(|r| r.prefab_overrides.clone());
            let Some(overrides) = overrides.filter(|o| !o.esta_vacia()) else {
                avisos.push(format!(
                    "#{raiz_id} ({fuente}): instancia modificada: no actualizada"
                ));
                continue;
            };
            // Con diff y sin deriva del asset, la instancia ya es
            // asset+diff: re-expandir sería idéntico, no se toca nada.
            if guardada.as_deref() == Some(huella_asset.as_str()) {
                continue;
            }
            self.reexpandir_con_overrides(raiz_id, &fuente, &prefab, overrides, &mut avisos);
        }
        avisos
    }

    /// Prefab vivo (Fase D): recalcula el `prefab_overrides` de cada
    /// instancia vinculada contra su asset actual. Lo llama el editor al
    /// guardar la escena: la instancia intacta queda sin diff y la
    /// modificada guarda el suyo, para que el refresco de la próxima
    /// carga reaplique los cambios en vez de descartarlos. Con el
    /// vínculo roto no se toca el diff guardado.
    pub fn recalcular_overrides_prefab(
        &mut self,
        resolver: &dyn Fn(&str) -> Option<crate::prefab::Prefab>,
    ) {
        let raices: Vec<(u64, String, Option<String>)> = self
            .entities
            .iter()
            .filter_map(|e| {
                e.prefab_source
                    .as_ref()
                    .map(|f| (e.id, f.clone(), e.prefab_hash.clone()))
            })
            .collect();
        for (raiz_id, fuente, guardada) in raices {
            let Some(prefab) = resolver(&fuente) else {
                continue;
            };
            let id_raiz_asset = prefab.root_entity.id;
            let huella = crate::prefab::huella_subarbol(&self.entities, raiz_id, |e| {
                if e.id == raiz_id {
                    id_raiz_asset
                } else {
                    e.prefab_local_id.unwrap_or(u64::MAX)
                }
            });
            let nuevo = if guardada.as_deref() == Some(huella.as_str()) {
                None
            } else {
                match self.calcular_diff_prefab(raiz_id, &prefab) {
                    Some(d) if !d.esta_vacia() => Some(d),
                    _ => None,
                }
            };
            if let Some(raiz) = self.get_entity_mut(raiz_id) {
                raiz.prefab_overrides = nuevo;
            }
        }
    }

    /// Id de escena de la entidad vinculada del subárbol de `raiz_id`
    /// con ese `prefab_local_id`. Los local_id solo son únicos dentro de
    /// su instancia, así que la búsqueda va acotada: no entra en las
    /// instancias anidadas (raíces con `prefab_source`, que son frontera)
    /// y el local_id de la raíz del asset (`local_raiz`) resuelve a la
    /// propia raíz de la escena, cuyo local_id puede faltar o diferir.
    fn id_en_subarbol_prefab(
        &self,
        raiz_id: u64,
        local_id: u64,
        local_raiz: u64,
    ) -> Option<u64> {
        if local_id == local_raiz {
            return Some(raiz_id);
        }
        use std::collections::HashMap;
        let por_id: HashMap<u64, &EntityData> =
            self.entities.iter().map(|e| (e.id, e)).collect();
        let raiz = por_id.get(&raiz_id)?;
        // Preorden en orden de documento (hijos al revés en la pila):
        // ante un local_id duplicado por copiar-pegar, el primer
        // emparejamiento es la entidad vinculada original, no la copia.
        let mut pila: Vec<&EntityData> = raiz
            .children
            .iter()
            .filter_map(|c| por_id.get(c).copied())
            .rev()
            .collect();
        while let Some(e) = pila.pop() {
            // Frontera: una instancia anidada no se entra ni se empareja.
            if e.prefab_source.is_some() {
                continue;
            }
            if e.prefab_local_id == Some(local_id) {
                return Some(e.id);
            }
            pila.extend(
                e.children
                    .iter()
                    .filter_map(|c| por_id.get(c).copied())
                    .rev(),
            );
        }
        None
    }

    /// Prefab vivo (Fase D): re-expande la instancia `raiz_id` desde
    /// `prefab` (que ha derivado respecto a la base del diff) reaplicando
    /// `overrides`. Las entidades añadidas se despegan antes de expandir y
    /// se re-pegan después conservando su id de escena; las quitadas se
    /// borran tras expandir; las propiedades se escriben por
    /// `prefab_local_id` y las que ya no encajan (la entidad ya no está en
    /// el asset o el valor no aplica) quedan marcadas huérfanas con aviso,
    /// sin aplicarse a ciegas. Al final la raíz guarda la huella del asset
    /// nuevo como base y el diff actualizado.
    fn reexpandir_con_overrides(
        &mut self,
        raiz_id: u64,
        fuente: &str,
        prefab: &crate::prefab::Prefab,
        mut overrides: crate::prefab::PrefabOverrides,
        avisos: &mut Vec<String>,
    ) {
        use std::collections::HashSet;

        // 1. Despegar las raíces añadidas con el mismo criterio que
        // `calcular_diff_prefab`: recorriendo solo la parte vinculada,
        // es añadida la entidad sin local_id, la raíz de otra instancia
        // o la de local_id repetido. Lo que cuelga de una añadida viaja
        // con ella (no se recorre). «Local desconocido en el asset» NO
        // es criterio: con el asset nuevo en la mano, esa entidad es un
        // override que quedará huérfano, no una añadida.
        let local_raiz = prefab.root_entity.id;
        let por_id: std::collections::HashMap<u64, &EntityData> =
            self.entities.iter().map(|e| (e.id, e)).collect();
        let asset_por_id: std::collections::HashMap<u64, &EntityData> =
            std::iter::once(&prefab.root_entity)
                .chain(&prefab.descendant_entities)
                .map(|e| (e.id, e))
                .collect();
        let mut locales_vistos: HashSet<u64> = HashSet::from([local_raiz]);
        let mut visitadas: HashSet<u64> = HashSet::from([raiz_id]);
        // (id de escena, id del padre, local_id del padre, nombre, índice entre hermanos)
        let mut despegadas: Vec<(u64, u64, Option<u64>, String, usize)> = Vec::new();
        if let Some(raiz) = por_id.get(&raiz_id).copied() {
            let mut pila: Vec<&EntityData> = vec![raiz];
            while let Some(e) = pila.pop() {
                let mut vinculados: Vec<&EntityData> = Vec::new();
                for (indice, hijo) in e
                    .children
                    .iter()
                    .filter_map(|c| por_id.get(c).copied())
                    .enumerate()
                {
                    if !visitadas.insert(hijo.id) {
                        continue;
                    }
                    // La anidada ALMACENADA en el asset (su entidad en el
                    // asset también es raíz de instancia) no es añadida
                    // ni se desciende en ella: la re-expansión la
                    // reproduce desde el asset y su subárbol es suyo.
                    let es_almacenada = hijo.prefab_source.is_some()
                        && hijo.prefab_local_id.is_some_and(|l| {
                            asset_por_id
                                .get(&l)
                                .is_some_and(|a| a.prefab_source.is_some())
                        });
                    if es_almacenada {
                        if let Some(l) = hijo.prefab_local_id {
                            locales_vistos.insert(l);
                        }
                        continue;
                    }
                    let es_anadida = match hijo.prefab_local_id {
                        None => true,
                        Some(_) if hijo.prefab_source.is_some() => true,
                        Some(l) => !locales_vistos.insert(l),
                    };
                    if es_anadida {
                        let padre_local = if e.id == raiz_id {
                            Some(local_raiz)
                        } else {
                            e.prefab_local_id
                        };
                        despegadas.push((
                            hijo.id,
                            e.id,
                            padre_local,
                            hijo.name.clone(),
                            indice,
                        ));
                    } else {
                        vinculados.push(hijo);
                    }
                }
                pila.extend(vinculados.into_iter().rev());
            }
        }
        for (id, padre_escena, _, _, _) in &despegadas {
            if let Some(p) = self.get_entity_mut(*padre_escena) {
                p.children.retain(|c| c != id);
            }
            if let Some(e) = self.get_entity_mut(*id) {
                e.parent = None;
            }
        }

        // 2. Re-expandir desde el asset nuevo (las añadidas despegadas ya
        // no están en el subárbol: sobreviven a la sustitución).
        self.reexpandir_instancia(raiz_id, prefab);

        // 3. Borrar las quitadas que el asset nuevo vuelve a traer.
        for local in &overrides.quitadas {
            let Some(objetivo) = self.id_en_subarbol_prefab(raiz_id, *local, local_raiz) else {
                continue;
            };
            let ids: HashSet<u64> = crate::prefab::recorrer_subarbol(&self.entities, objetivo)
                .iter()
                .map(|n| n.e.id)
                .collect();
            self.entities.retain(|e| !ids.contains(&e.id));
            for e in &mut self.entities {
                e.children.retain(|c| !ids.contains(c));
            }
        }

        // 4. Reaplicar las propiedades; la que no encaje queda huérfana.
        for entrada in &mut overrides.propiedades {
            let aplicada = self
                .id_en_subarbol_prefab(raiz_id, entrada.local_id, local_raiz)
                .and_then(|id| self.get_entity_mut(id))
                .map(|e| {
                    crate::prefab::escribir_path(e, &entrada.path, &entrada.valor).is_ok()
                })
                .unwrap_or(false);
            if aplicada {
                entrada.huerfana = false;
            } else {
                entrada.huerfana = true;
                avisos.push(format!(
                    "#{raiz_id} ({fuente}): override huérfano en local {} ({}): no aplicado",
                    entrada.local_id, entrada.path
                ));
            }
        }

        // 5. Re-pegar las añadidas bajo su padre vinculado, en su
        // posición entre hermanos si cabe; si el padre ya no está en el
        // asset, quedan sueltas con aviso.
        for (id, _, padre_local, nombre, indice) in &despegadas {
            let nuevo_padre =
                padre_local.and_then(|l| self.id_en_subarbol_prefab(raiz_id, l, local_raiz));
            match nuevo_padre {
                Some(p) => {
                    if let Some(e) = self.get_entity_mut(*id) {
                        e.parent = Some(p);
                    }
                    if let Some(padre) = self.get_entity_mut(p) {
                        if !padre.children.contains(id) {
                            let pos = (*indice).min(padre.children.len());
                            padre.children.insert(pos, *id);
                        }
                    }
                }
                None => {
                    avisos.push(format!(
                        "#{raiz_id} ({fuente}): entidad añadida «{nombre}» sin padre en el asset nuevo: queda suelta"
                    ));
                }
            }
        }

        // 6. La raíz guarda la nueva base y el diff actualizado. Las
        // quitadas que el asset nuevo ya no contiene se depuran: ya no
        // hay nada que quitar (las entradas de añadidas las regenera el
        // próximo cálculo del diff al guardar).
        overrides.quitadas.retain(|l| {
            prefab
                .descendant_entities
                .iter()
                .any(|e| e.id == *l)
        });
        if let Some(raiz) = self.get_entity_mut(raiz_id) {
            raiz.prefab_hash = Some(prefab.huella());
            raiz.prefab_overrides = Some(overrides);
        }
    }

    /// Prefab vivo (Fase D): overrides de la instancia `raiz_id` frente a
    /// `prefab`. None si la entidad no existe o no es raíz vinculada;
    /// Some(vacío) si la instancia está intacta. Las entidades se emparejan
    /// por `prefab_local_id`; la raíz siempre con la raíz del asset.
    pub fn calcular_diff_prefab(
        &self,
        raiz_id: u64,
        prefab: &crate::prefab::Prefab,
    ) -> Option<crate::prefab::PrefabOverrides> {
        use crate::prefab::{
            paths_comparables, valor_en_path, OverrideAnadida, OverridePropiedad, PrefabOverrides,
        };
        use std::collections::{HashMap, HashSet};

        let raiz = self.get_entity(raiz_id)?;
        raiz.prefab_source.as_ref()?;

        let por_id: HashMap<u64, &EntityData> =
            self.entities.iter().map(|e| (e.id, e)).collect();
        let asset_por_id: HashMap<u64, &EntityData> = std::iter::once(&prefab.root_entity)
            .chain(&prefab.descendant_entities)
            .map(|e| (e.id, e))
            .collect();
        let local_raiz = prefab.root_entity.id;

        // Entidades del asset bajo una raíz anidada almacenada: no son
        // miembros directos de esta instancia (el subárbol pertenece a
        // la anidada), así que nunca cuentan como quitadas de ésta.
        let bajo_anidada_asset: HashSet<u64> = {
            let nodos = crate::prefab::recorrer_subarbol(
                std::iter::once(&prefab.root_entity).chain(&prefab.descendant_entities),
                prefab.root_entity.id,
            );
            let mut excluidos = HashSet::new();
            for n in &nodos {
                if let Some(p) = n.padre {
                    if excluidos.contains(&p.id)
                        || (p.id != prefab.root_entity.id && p.prefab_source.is_some())
                    {
                        excluidos.insert(n.e.id);
                    }
                }
            }
            excluidos
        };

        // Preorden por la parte vinculada de la instancia. Un hijo sin
        // local_id (o con uno desconocido o repetido, o raíz de otra
        // instancia) es una añadida y no se recorre por dentro: lo que
        // cuelga de él pertenece a esa añadida.
        let mut parejas: Vec<(u64, &EntityData)> = Vec::new();
        let mut anadidas: Vec<OverrideAnadida> = Vec::new();
        let mut visitadas: HashSet<u64> = HashSet::from([raiz_id]);
        let mut locales_vistos: HashSet<u64> = HashSet::from([local_raiz]);
        let mut pila: Vec<(u64, &EntityData)> = vec![(local_raiz, raiz)];

        while let Some((local, e)) = pila.pop() {
            parejas.push((local, e));
            let mut indice = 0;
            let mut vinculados: Vec<(u64, &EntityData)> = Vec::new();
            for hijo in e.children.iter().filter_map(|c| por_id.get(c).copied()) {
                if !visitadas.insert(hijo.id) {
                    continue;
                }
                // Anidada ALMACENADA en el asset (su entidad en el asset
                // también es raíz de instancia): es pareja para comparar
                // sus propiedades como miembro, sin descender — su
                // subárbol es territorio de la instancia anidada.
                let es_almacenada = hijo.prefab_source.is_some()
                    && hijo.prefab_local_id.is_some_and(|l| {
                        asset_por_id
                            .get(&l)
                            .is_some_and(|a| a.prefab_source.is_some())
                    });
                if es_almacenada {
                    let l = hijo.prefab_local_id.unwrap();
                    locales_vistos.insert(l);
                    parejas.push((l, hijo));
                    continue;
                }
                // La raíz de una instancia anidada cuenta como añadida
                // (no se recorre), pero si su local_id existe en el asset
                // se marca visto: esa entidad del asset está presente en
                // la escena y no debe acabar en `quitadas` (el refresco
                // borraría la instancia anidada entera).
                if hijo.prefab_source.is_some() {
                    if let Some(l) = hijo.prefab_local_id {
                        if asset_por_id.contains_key(&l) {
                            locales_vistos.insert(l);
                        }
                    }
                }
                let vinculado = match hijo.prefab_local_id {
                    Some(l)
                        if hijo.prefab_source.is_none()
                            && asset_por_id.contains_key(&l)
                            && locales_vistos.insert(l) =>
                    {
                        Some(l)
                    }
                    _ => None,
                };
                match vinculado {
                    Some(l) => vinculados.push((l, hijo)),
                    None => {
                        anadidas.push(OverrideAnadida {
                            padre_local_id: Some(local),
                            nombre: hijo.name.clone(),
                            indice,
                            subarbol: None,
                        });
                        indice += 1;
                    }
                }
            }
            pila.extend(vinculados.into_iter().rev());
        }

        let mut propiedades: Vec<OverridePropiedad> = Vec::new();
        for &(local, e) in &parejas {
            let asset = asset_por_id[&local];
            for path in paths_comparables(e.id == raiz_id) {
                let en_escena = valor_en_path(e, path);
                if valor_en_path(asset, path) != en_escena {
                    propiedades.push(OverridePropiedad {
                        local_id: local,
                        path: path.to_string(),
                        valor: en_escena.unwrap_or(serde_json::Value::Null),
                        huerfana: false,
                    });
                }
            }
        }

        let quitadas: Vec<u64> = prefab
            .descendant_entities
            .iter()
            .map(|e| e.id)
            .filter(|id| !locales_vistos.contains(id) && !bajo_anidada_asset.contains(id))
            .collect();

        Some(PrefabOverrides {
            propiedades,
            anadidas,
            quitadas,
        })
    }

    /// Revierte una instancia vinculada a su asset (Fase C): re-expande
    /// desde `prefab` DESCARTANDO las modificaciones de la instancia
    /// (incluidas las entidades añadidas en la escena) y deja la huella
    /// guardada en la del asset, para que la instancia vuelva a contar
    /// como intacta. La raíz conserva su transform y su padre externo,
    /// como en el refresco. Error si la entidad no es raíz de instancia.
    pub fn revertir_instancia_prefab(
        &mut self,
        raiz_id: u64,
        prefab: &crate::prefab::Prefab,
    ) -> Result<(), String> {
        match self.get_entity(raiz_id) {
            Some(e) if e.prefab_source.is_some() => {}
            Some(_) => {
                return Err(format!(
                    "La entidad #{raiz_id} no es raíz de una instancia de prefab"
                ))
            }
            None => return Err(format!("La entidad #{raiz_id} no existe en la escena")),
        }
        self.reexpandir_instancia(raiz_id, prefab);
        let huella = prefab.huella();
        if let Some(raiz) = self.get_entity_mut(raiz_id) {
            raiz.prefab_hash = Some(huella);
            raiz.prefab_overrides = None;
        }
        Ok(())
    }

    /// Desempaqueta una instancia (Fase C): corta el vínculo de todo el
    /// subárbol y deja las entidades como normales, horneando su estado
    /// actual (no se toca ningún otro dato). Error si la entidad no es
    /// raíz de instancia.
    pub fn desempaquetar_instancia(&mut self, raiz_id: u64) -> Result<(), String> {
        match self.get_entity(raiz_id) {
            Some(e) if e.prefab_source.is_some() => {}
            Some(_) => {
                return Err(format!(
                    "La entidad #{raiz_id} no es raíz de una instancia de prefab"
                ))
            }
            None => return Err(format!("La entidad #{raiz_id} no existe en la escena")),
        }
        let ids: Vec<u64> = crate::prefab::recorrer_subarbol(&self.entities, raiz_id)
            .iter()
            .map(|n| n.e.id)
            .collect();
        for id in ids {
            if let Some(e) = self.get_entity_mut(id) {
                e.prefab_local_id = None;
                e.prefab_overrides = None;
                if id == raiz_id {
                    e.prefab_source = None;
                    e.prefab_hash = None;
                }
            }
        }
        Ok(())
    }

    /// Convierte la instancia en entidades normales TAMBIÉN en sus
    /// instancias anidadas (el «Unpack Completely» de Unity): desempaqueta
    /// la raíz y después cada raíz anidada de su subárbol, de fuera hacia
    /// dentro.
    pub fn desempaquetar_instancia_completa(&mut self, raiz_id: u64) -> Result<(), String> {
        match self.get_entity(raiz_id) {
            Some(e) if e.prefab_source.is_some() => {}
            Some(_) => {
                return Err(format!(
                    "La entidad #{raiz_id} no es raíz de una instancia de prefab"
                ))
            }
            None => return Err(format!("La entidad #{raiz_id} no existe en la escena")),
        }
        let anidadas: Vec<u64> = crate::prefab::recorrer_subarbol(&self.entities, raiz_id)
            .iter()
            .filter(|n| n.e.id != raiz_id && n.e.prefab_source.is_some())
            .map(|n| n.e.id)
            .collect();
        self.desempaquetar_instancia(raiz_id)?;
        for id in anidadas {
            self.desempaquetar_instancia(id)?;
        }
        Ok(())
    }

    /// Sustituye el subárbol de la instancia `raiz_id` por el contenido de
    /// `prefab`, emparejando entidades por `prefab_local_id`. Conserva el id
    /// de escena de las que siguen en el asset, crea las nuevas y elimina las
    /// que ya no están. La raíz conserva transform, padre externo y vínculo.
    fn reexpandir_instancia(&mut self, raiz_id: u64, prefab: &crate::prefab::Prefab) {
        use crate::prefab::recorrer_subarbol;
        use std::collections::{HashMap, HashSet};

        let (transform, parent_externo, fuente, huella, local_raiz) = match self.get_entity(raiz_id) {
            Some(r) => (
                r.transform.clone(),
                r.parent,
                r.prefab_source.clone(),
                r.prefab_hash.clone(),
                r.prefab_local_id,
            ),
            None => return,
        };

        // Entidades actuales de la instancia: (id de escena, local_id).
        let actuales: Vec<(u64, Option<u64>)> = recorrer_subarbol(&self.entities, raiz_id)
            .iter()
            .map(|n| (n.e.id, n.e.prefab_local_id))
            .collect();
        let mut escena_por_local: HashMap<u64, u64> = actuales
            .iter()
            .filter(|(id, _)| *id != raiz_id)
            .filter_map(|(id, local)| local.map(|l| (l, *id)))
            .collect();
        // La raíz sigue siendo la raíz aunque su id en el asset haya cambiado.
        escena_por_local.insert(prefab.root_entity.id, raiz_id);

        let nodos = recorrer_subarbol(
            std::iter::once(&prefab.root_entity).chain(&prefab.descendant_entities),
            prefab.root_entity.id,
        );
        // Categoría de cada entidad del asset respecto a las anidadas
        // ALMACENADAS: la raíz de una anidada y su subárbol se copian
        // del asset tal cual (su vínculo y sus ids locales interiores
        // son suyos), no como miembros de esta instancia. El recorrido
        // trae al padre siempre antes que al hijo.
        let mut es_raiz_anidada: HashSet<u64> = HashSet::new();
        let mut bajo_anidada: HashSet<u64> = HashSet::new();
        for n in &nodos {
            if let Some(p) = n.padre {
                if es_raiz_anidada.contains(&p.id) || bajo_anidada.contains(&p.id) {
                    bajo_anidada.insert(n.e.id);
                } else if n.e.prefab_source.is_some() {
                    es_raiz_anidada.insert(n.e.id);
                }
            }
        }
        let mut asset_a_escena: HashMap<u64, u64> = HashMap::new();
        for n in &nodos {
            // Clave de emparejado con la escena: la raíz y los miembros
            // por su id en el asset (la anidada, por su hueco); lo que
            // cuelga de una anidada, por su local ALMACENADO (su id en
            // el asset interior), que es el que la escena conserva.
            let clave = if n.padre.is_none() {
                Some(prefab.root_entity.id)
            } else if bajo_anidada.contains(&n.e.id) {
                n.e.prefab_local_id
            } else {
                Some(n.e.id)
            };
            let id = match clave.and_then(|c| escena_por_local.get(&c)) {
                Some(&id) => id,
                None => {
                    let id = self.next_entity_id;
                    self.next_entity_id += 1;
                    id
                }
            };
            asset_a_escena.insert(n.e.id, id);
        }

        let mut nuevas: Vec<EntityData> = Vec::with_capacity(nodos.len());
        for n in &nodos {
            let mut e = n.e.clone();
            e.id = asset_a_escena[&n.e.id];
            if n.padre.is_none() {
                // La raíz conserva el local que traía en la escena: si es
                // una raíz ANIDADA, ese local es su hueco en el asset
                // exterior, no su id en el suyo — pisarlo con el id del
                // asset la desvincula de la instancia exterior (su diff
                // la contaría como añadida y a su hueco como quitada).
                e.prefab_local_id = Some(local_raiz.unwrap_or(n.e.id));
            } else if es_raiz_anidada.contains(&n.e.id) {
                // Raíz de la anidada almacenada: el clon ya trae su
                // vínculo del asset; su local pasa a ser su hueco aquí.
                e.prefab_local_id = Some(n.e.id);
            } else if bajo_anidada.contains(&n.e.id) {
                // Dentro de la anidada: copia literal del asset (local,
                // fuente y hash almacenados son los suyos).
                e.prefab_local_id = n.e.prefab_local_id;
                e.prefab_source = n.e.prefab_source.clone();
                e.prefab_hash = n.e.prefab_hash.clone();
            } else {
                e.prefab_local_id = Some(n.e.id);
                e.prefab_source = None;
                e.prefab_hash = None;
            }
            e.children = n
                .e
                .children
                .iter()
                .filter_map(|c| asset_a_escena.get(c).copied())
                .collect();
            match n.padre {
                Some(p) => e.parent = Some(asset_a_escena[&p.id]),
                None => {
                    e.transform = transform.clone();
                    e.parent = parent_externo;
                    e.prefab_source = fuente.clone();
                    e.prefab_hash = huella.clone();
                }
            }
            nuevas.push(e);
        }

        let conservadas: HashSet<u64> = nuevas.iter().map(|e| e.id).collect();
        let eliminadas: HashSet<u64> = actuales
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| !conservadas.contains(id))
            .collect();
        self.entities.retain(|e| !eliminadas.contains(&e.id));
        for e in &mut self.entities {
            e.children.retain(|c| !eliminadas.contains(c));
        }
        for nueva in nuevas {
            match self.entities.iter().position(|e| e.id == nueva.id) {
                Some(i) => self.entities[i] = nueva,
                None => self.entities.push(nueva),
            }
        }
    }
}

/// Todas las referencias no vacías a `.physmat.json` de la escena: el
/// `material` de cada ColliderSpec y el `mesh_collider_material` de cada
/// entidad con física. Sin duplicados y ordenadas. Las instancias de prefab
/// son entidades de la escena con su PhysicsData, así que entran por aquí.
pub fn physmat_refs(scene: &SceneState) -> Vec<String> {
    let mut refs = std::collections::BTreeSet::new();
    for physics in scene.entities.iter().filter_map(|e| e.physics.as_ref()) {
        for spec in &physics.colliders {
            if !spec.material.is_empty() {
                refs.insert(spec.material.clone());
            }
        }
        if !physics.mesh_collider_material.is_empty() {
            refs.insert(physics.mesh_collider_material.clone());
        }
    }
    refs.into_iter().collect()
}

#[cfg(test)]
mod regression_physics_tests {
    use super::*;

    #[test]
    fn collider_sin_rigidbody_es_estatico() {
        // Regla refinada de D3m3tr10 (2026-11-18): un Collider sin
        // RigidBody es geometría estática, como en Unity.
        let entity = EntityData::new(
            1,
            "Cube".into(),
            vec!["Transform".into(), "MeshRenderer".into(), "Collider".into()],
        );
        let phys = entity.physics.as_ref().expect("Collider solo debe crear física estática");
        assert!(!phys.has_rigidbody);
        assert!(phys.has_collider);
        assert_eq!(phys.body_type, "Static");
    }

    #[test]
    fn default_de_creacion_sin_rigidbody() {
        // Corrección de D3m3tr10 (2026-11-19): el Default con el que se
        // CREAN entidades no trae Rigidbody; sí Collider.
        let phys = PhysicsData::default();
        assert!(!phys.has_rigidbody);
        assert!(phys.has_collider);
    }

    #[test]
    fn rigidbody_a_secas_trae_collider() {
        let entity = EntityData::new(1, "Cube".into(), vec!["RigidBody".into()]);
        let phys = entity.physics.as_ref().expect("RigidBody debe crear física");
        assert!(phys.has_rigidbody);
        assert!(phys.has_collider);
    }

    #[test]
    fn rigidbody_con_collider_es_dinamico() {
        let entity = EntityData::new(
            2,
            "Cube".into(),
            vec![
                "Transform".into(),
                "MeshRenderer".into(),
                "RigidBody".into(),
                "Collider".into(),
            ],
        );
        assert!(entity.physics.is_some());
        assert_eq!(entity.physics.as_ref().unwrap().body_type, "Dynamic");
    }

    #[test]
    fn static_collider_sin_rigidbody_no_crea_fisica() {
        let entity = EntityData::new(
            3,
            "Plane".into(),
            vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
        );
        assert!(entity.physics.is_none());
    }

    #[test]
    fn sin_componentes_de_fisica_no_hay_physics() {
        let entity = EntityData::new(4, "Vacio".into(), vec!["Transform".into()]);
        assert!(entity.physics.is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_entity_copia_el_subarbol_con_ids_nuevos() {
        let mut sc = SceneState::new();
        let padre = sc.create_entity("Padre".into(), vec!["Transform".into()]);
        let hijo = sc.create_entity("Hijo".into(), vec!["Transform".into()]);
        let nieto = sc.create_entity("Nieto".into(), vec!["Transform".into()]);
        assert!(sc.reparent(hijo, Some(padre)));
        assert!(sc.reparent(nieto, Some(hijo)));
        let total = sc.entities.len();

        let copia = sc.duplicate_entity(padre).expect("la raíz debe existir");
        assert_ne!(copia, padre);
        assert_eq!(sc.entities.len(), total + 3);
        let raiz = sc.get_entity(copia).unwrap();
        assert_eq!(raiz.name, "Padre");
        assert_eq!(raiz.parent, None);
        assert_eq!(raiz.children.len(), 1);
        let hijo_copia = raiz.children[0];
        assert_ne!(hijo_copia, hijo);
        let hc = sc.get_entity(hijo_copia).unwrap();
        assert_eq!(hc.name, "Hijo");
        assert_eq!(hc.parent, Some(copia));
        assert_eq!(hc.children.len(), 1);
        let nieto_copia = hc.children[0];
        assert_eq!(sc.get_entity(nieto_copia).unwrap().name, "Nieto");
        assert_eq!(sc.get_entity(nieto_copia).unwrap().parent, Some(hijo_copia));
        // El original queda intacto.
        assert_eq!(sc.get_entity(padre).unwrap().children, vec![hijo]);
        assert!(sc.duplicate_entity(9999).is_none());
    }

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

    fn rt_simple(pos: [f32; 2], size: [f32; 2]) -> RectTransformData {
        RectTransformData {
            anchored_position: pos,
            size_delta: size,
            anchor_min: [0.0, 0.0],
            anchor_max: [0.0, 0.0],
            pivot: [0.0, 0.0],
        }
    }

    fn canvas_ref(scene: &mut SceneState, id: u64, referencia: [f32; 2], activo: bool) {
        let e = scene.entities.iter_mut().find(|e| e.id == id).unwrap();
        e.active = activo;
        e.canvas = Some(CanvasData {
            render_mode: "ScreenSpaceOverlay".into(),
            reference_resolution: referencia,
            match_width_or_height: 0.5,
        });
    }

    #[test]
    fn layout_recoloca_tambien_a_los_nietos() {
        // Auditoría M-24: la etiqueta hija del botón debe resolverse
        // contra el rect FINAL del botón (el del layout), no el
        // autorado (que está en [300, 200]).
        let mut scene = SceneState::new();
        let canvas_id = scene.create_entity("Canvas".into(), vec!["Canvas".into()]);
        let panel_id = scene.create_entity("Panel".into(), vec!["RectTransform".into(), "LayoutGroup".into()]);
        let b1 = scene.create_entity("B1".into(), vec!["Button".into()]);
        let label = scene.create_entity("Label".into(), vec!["Text".into()]);
        canvas_ref(&mut scene, canvas_id, [1280.0, 720.0], true);
        {
            let e = scene.entities.iter_mut().find(|e| e.id == panel_id).unwrap();
            e.parent = Some(canvas_id);
            e.layout_group = Some(LayoutGroupData {
                direction: "horizontal".into(),
                spacing: 10.0,
                padding: [0.0, 0.0, 0.0, 0.0],
                alignment: "start".into(),
            });
            e.rect_transform = Some(rt_simple([0.0, 0.0], [400.0, 60.0]));
        }
        {
            let e = scene.entities.iter_mut().find(|e| e.id == b1).unwrap();
            e.parent = Some(panel_id);
            e.rect_transform = Some(rt_simple([300.0, 200.0], [100.0, 40.0]));
        }
        {
            let e = scene.entities.iter_mut().find(|e| e.id == label).unwrap();
            e.parent = Some(b1);
            e.rect_transform = Some(rt_simple([5.0, 5.0], [50.0, 20.0]));
        }
        let rects = resolve_scene_ui_rects(&scene, 1280.0, 720.0);
        assert_eq!(rects[&b1], [0.0, 660.0, 100.0, 40.0]);
        let lr = rects[&label];
        assert_eq!(lr[0], rects[&b1][0] + 5.0, "la etiqueta sigue al botón recolocado");
        assert!(lr[1] >= rects[&b1][1] && lr[1] + lr[3] <= rects[&b1][1] + rects[&b1][3],
            "la etiqueta queda DENTRO del botón final: {lr:?} vs {:?}", rects[&b1]);
    }

    #[test]
    fn cada_canvas_escala_su_subarbol_y_el_inactivo_no() {
        // Auditoría M-25: dos canvas con referencias distintas; y un
        // canvas inactivo no impone escala.
        let mut scene = SceneState::new();
        let ca = scene.create_entity("CA".into(), vec!["Canvas".into()]);
        let cb = scene.create_entity("CB".into(), vec!["Canvas".into()]);
        let cc = scene.create_entity("CC".into(), vec!["Canvas".into()]);
        let ba = scene.create_entity("BA".into(), vec!["Button".into()]);
        let bb = scene.create_entity("BB".into(), vec!["Button".into()]);
        let bc = scene.create_entity("BC".into(), vec!["Button".into()]);
        canvas_ref(&mut scene, ca, [1280.0, 720.0], true);
        canvas_ref(&mut scene, cb, [640.0, 360.0], true);
        canvas_ref(&mut scene, cc, [100.0, 100.0], false);
        for (b, p) in [(ba, ca), (bb, cb), (bc, cc)] {
            let e = scene.entities.iter_mut().find(|e| e.id == b).unwrap();
            e.parent = Some(p);
            e.rect_transform = Some(rt_simple([0.0, 0.0], [100.0, 40.0]));
        }
        let rects = resolve_scene_ui_rects(&scene, 1280.0, 720.0);
        assert_eq!(rects[&ba][2], 100.0, "canvas A escala 1");
        assert_eq!(rects[&bb][2], 200.0, "canvas B escala 2");
        assert_eq!(rects[&bc][2], 100.0, "el canvas inactivo no impone su escala");
    }

    #[test]
    fn layout_ignora_hijos_inactivos() {
        // Auditoría (menor): un hijo inactivo no ocupa hueco.
        let mut scene = SceneState::new();
        let canvas_id = scene.create_entity("Canvas".into(), vec!["Canvas".into()]);
        let panel_id = scene.create_entity("Panel".into(), vec!["RectTransform".into(), "LayoutGroup".into()]);
        let b1 = scene.create_entity("B1".into(), vec!["Button".into()]);
        let b2 = scene.create_entity("B2".into(), vec!["Button".into()]);
        let b3 = scene.create_entity("B3".into(), vec!["Button".into()]);
        canvas_ref(&mut scene, canvas_id, [1280.0, 720.0], true);
        {
            let e = scene.entities.iter_mut().find(|e| e.id == panel_id).unwrap();
            e.parent = Some(canvas_id);
            e.layout_group = Some(LayoutGroupData {
                direction: "horizontal".into(),
                spacing: 10.0,
                padding: [0.0, 0.0, 0.0, 0.0],
                alignment: "start".into(),
            });
            e.rect_transform = Some(rt_simple([0.0, 0.0], [400.0, 60.0]));
        }
        for (b, activo) in [(b1, true), (b2, false), (b3, true)] {
            let e = scene.entities.iter_mut().find(|e| e.id == b).unwrap();
            e.parent = Some(panel_id);
            e.active = activo;
            e.rect_transform = Some(rt_simple([0.0, 0.0], [100.0, 40.0]));
        }
        let rects = resolve_scene_ui_rects(&scene, 1280.0, 720.0);
        assert_eq!(rects[&b1][0], 0.0);
        assert_eq!(rects[&b3][0], 110.0, "B3 va tras B1: B2 inactivo no ocupa hueco");
    }

    #[test]
    fn ciclo_entre_no_ui_no_cuelga_el_resolvedor() {
        // Auditoría M-26: X (UI) -> Y -> Z -> Y (ciclo sin UI en
        // medio): el paseo de ancestros corta y X se trata como raíz.
        let mut scene = SceneState::new();
        let x = scene.create_entity("X".into(), vec!["Text".into()]);
        let y = scene.create_entity("Y".into(), vec![]);
        let z = scene.create_entity("Z".into(), vec![]);
        {
            let e = scene.entities.iter_mut().find(|e| e.id == x).unwrap();
            e.parent = Some(y);
            e.rect_transform = Some(rt_simple([0.0, 0.0], [10.0, 10.0]));
        }
        {
            let e = scene.entities.iter_mut().find(|e| e.id == y).unwrap();
            e.parent = Some(z);
        }
        {
            let e = scene.entities.iter_mut().find(|e| e.id == z).unwrap();
            e.parent = Some(y);
        }
        let rects = resolve_scene_ui_rects(&scene, 1280.0, 720.0);
        assert!(rects.contains_key(&x), "X resuelve (como raíz) pese al ciclo");
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
    fn guardar_crea_snapshot_de_la_version_anterior() {
        // Auditoría C9: al sobrescribir una escena queda su versión
        // anterior en <escena>.snapshots/, con poda a 10.
        let dir = std::env::temp_dir().join("glass_test_snapshot_c9");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Escena.scene.json");
        let path_str = path.to_string_lossy().to_string();

        let mut scene = SceneState::new();
        scene.create_entity("Uno".into(), vec![]);
        scene.save_to_json(&path_str).unwrap();
        let snap_dir = dir.join("Escena.scene.snapshots");
        assert!(!snap_dir.exists(), "sin versión previa no hay snapshot");

        scene.create_entity("Dos".into(), vec![]);
        scene.save_to_json(&path_str).unwrap();
        let snaps: Vec<_> = std::fs::read_dir(&snap_dir).unwrap().flatten().collect();
        assert_eq!(snaps.len(), 1);
        let contenido = std::fs::read_to_string(snaps[0].path()).unwrap();
        assert!(contenido.contains("Uno") && !contenido.contains("Dos"),
            "el snapshot guarda la versión ANTERIOR");

        // Poda: tras muchos guardados quedan como mucho 10.
        for i in 0..12 {
            scene.create_entity(format!("E{i}"), vec![]);
            scene.save_to_json(&path_str).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let snaps: Vec<_> = std::fs::read_dir(&snap_dir).unwrap().flatten().collect();
        assert!(snaps.len() <= 10, "poda a 10: {}", snaps.len());
        let _ = std::fs::remove_dir_all(&dir);
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
    ///    debe deserializar con `material == ""`.
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


#[cfg(test)]
mod scene_index_tests {
    use super::*;

    use crate::physics_material::PhysicsMaterial;

    // --- C2-A: Physics Material en el modelo de escena ---

    /// PhysicsData serializado como JSON, partiendo del default para no
    /// depender de qué campos son obligatorios en la carga.
    fn physics_json_con(extra: &[(&str, serde_json::Value)]) -> serde_json::Value {
        let mut v = serde_json::to_value(PhysicsData::default()).unwrap();
        let obj = v.as_object_mut().unwrap();
        for (k, val) in extra {
            obj.insert((*k).to_string(), val.clone());
        }
        v
    }

    #[test]
    fn escena_vieja_conserva_friction_y_restitution() {
        let json = physics_json_con(&[
            ("friction", serde_json::json!(0.3)),
            ("restitution", serde_json::json!(0.8)),
        ]);
        let cargado: PhysicsData = serde_json::from_value(json).unwrap();
        assert_eq!(cargado.legacy_friction, Some(0.3));
        assert_eq!(cargado.legacy_restitution, Some(0.8));

        // Equivalencia: el guardado reescribe las mismas claves y valores
        // (la serialización de f32 puede expandir el decimal: se compara
        // con tolerancia; la igualdad exacta la prueba la recarga).
        let guardado = serde_json::to_value(&cargado).unwrap();
        assert!((guardado["friction"].as_f64().unwrap() - 0.3).abs() < 1e-6);
        assert!((guardado["restitution"].as_f64().unwrap() - 0.8).abs() < 1e-6);
        let recargado: PhysicsData = serde_json::from_value(guardado).unwrap();
        assert_eq!(recargado.legacy_friction, Some(0.3));
        assert_eq!(recargado.legacy_restitution, Some(0.8));
    }

    #[test]
    fn objeto_nuevo_sin_legado_y_fallback_project_default() {
        let p = PhysicsData::default();
        let v = serde_json::to_value(&p).unwrap();
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("friction"));
        assert!(!obj.contains_key("restitution"));
        assert!(!obj.contains_key("mesh_collider_material"));
        assert_eq!(p.mesh_collider_material, "");
        assert!(!p.mesh_collider_is_trigger);
        assert_eq!(p.body_fallback_material(), PhysicsMaterial::PROJECT_DEFAULT);
    }

    #[test]
    fn fallback_compone_por_campos() {
        let solo_friccion = PhysicsData {
            legacy_friction: Some(0.3),
            ..PhysicsData::default()
        };
        let m = solo_friccion.body_fallback_material();
        assert_eq!(m.dynamic_friction, 0.3);
        assert_eq!(m.bounciness, 0.05);
        assert_eq!(m.static_friction, PhysicsMaterial::PROJECT_DEFAULT.static_friction);
        assert_eq!(m.friction_combine, PhysicsMaterial::PROJECT_DEFAULT.friction_combine);
        assert_eq!(m.bounce_combine, PhysicsMaterial::PROJECT_DEFAULT.bounce_combine);

        let solo_rebote = PhysicsData {
            legacy_restitution: Some(0.8),
            ..PhysicsData::default()
        };
        let m = solo_rebote.body_fallback_material();
        assert_eq!(m.dynamic_friction, 0.7);
        assert_eq!(m.bounciness, 0.8);
        assert_eq!(m.static_friction, PhysicsMaterial::PROJECT_DEFAULT.static_friction);
    }

    #[test]
    fn spec_sin_claves_nuevas_carga_defaults() {
        let spec: ColliderSpec =
            serde_json::from_str(r#"{"shape":"Capsule","size":[0.5,0.5,0.0]}"#).unwrap();
        assert_eq!(spec.material, "");
        assert_eq!(spec.direction, "Y");

        for forma in ["Box", "Sphere", "Capsule", "Cylinder", "Desconocida"] {
            let d = spec_por_defecto(forma);
            assert_eq!(d.material, "");
            assert_eq!(d.direction, "Y");
        }

        // El default no escribe las claves nuevas.
        let v = serde_json::to_value(spec_por_defecto("Capsule")).unwrap();
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("material"));
        assert!(!obj.contains_key("direction"));

        // Un material asignado se conserva al serializar.
        let mut con_material = spec_por_defecto("Box");
        con_material.material = "Materials/Hielo.physmat.json".into();
        let v = serde_json::to_value(&con_material).unwrap();
        assert_eq!(v["material"].as_str(), Some("Materials/Hielo.physmat.json"));
        let recargado: ColliderSpec = serde_json::from_value(v).unwrap();
        assert_eq!(recargado.material, "Materials/Hielo.physmat.json");
    }

    fn entidad_con_physics(id: u64, physics: Option<PhysicsData>) -> EntityData {
        let mut e = EntityData::new(id, format!("E{id}"), vec![]);
        e.physics = physics;
        e
    }

    #[test]
    fn physmat_refs_unicas_y_ordenadas() {
        let hielo = "Materials/Hielo.physmat.json";
        let goma = "Materials/Goma.physmat.json";

        let mut spec_a = spec_por_defecto("Box");
        spec_a.material = hielo.into();
        let mut a = PhysicsData::default();
        a.anadir_collider(spec_a);
        a.mesh_collider_material = goma.into();

        // Entidad con física pero sin referencias.
        let b = PhysicsData::default();

        // Tercera entidad que repite la misma referencia.
        let mut spec_c = spec_por_defecto("Sphere");
        spec_c.material = hielo.into();
        let mut c = PhysicsData::default();
        c.anadir_collider(spec_c);

        let scene = SceneState {
            entities: vec![
                entidad_con_physics(1, Some(a)),
                entidad_con_physics(2, Some(b)),
                entidad_con_physics(3, None),
                entidad_con_physics(4, Some(c)),
            ],
            ..SceneState::default()
        };
        assert_eq!(physmat_refs(&scene), vec![goma.to_string(), hielo.to_string()]);
    }

    fn fresh_scene() -> SceneState {
        let mut s = SceneState::new();
        s.entities.clear();
        s.next_entity_id = 1;
        s.id_index.borrow_mut().clear();
        s
    }

    #[test]
    fn lookups_equivalentes_tras_crear_y_borrar_en_desorden() {
        let mut s = fresh_scene();
        let ids: Vec<u64> = (0..20).map(|i| s.create_entity(format!("E{i}"), vec![])).collect();

        // Borrar en desorden: pares primero, luego algunos impares
        for &id in ids.iter().step_by(2) {
            assert!(s.delete_entity(id));
        }
        for &id in ids.iter().skip(1).step_by(4) {
            assert!(s.delete_entity(id));
        }

        // Verificación cruzada: lo que queda en el Vec debe ser
        // exactamente lo que resuelve get_entity, y nada más.
        let restantes: std::collections::HashSet<u64> =
            s.entities.iter().map(|e| e.id).collect();
        for id in &ids {
            let existe_en_vec = restantes.contains(id);
            let existe_en_lookup = s.get_entity(*id).is_some();
            assert_eq!(existe_en_vec, existe_en_lookup, "id {id} inconsistente");
        }
        assert_eq!(s.entities.len(), restantes.len());
    }

    #[test]
    fn tolera_push_externo_directo_al_vec() {
        let mut s = fresh_scene();
        let a = s.create_entity("A".into(), vec![]);

        // Simula código externo (canvas_ui.rs tests) mutando el Vec sin pasar por create_entity.
        s.entities.push(EntityData::new(999, "Externa".into(), vec![]));

        // El índice está desincronizado en longitud; debe repararse sola.
        assert!(s.get_entity(999).is_some());
        assert!(s.get_entity(a).is_some());

        // Ahora retain externo que borra `a` sin pasar por delete_entity.
        s.entities.retain(|e| e.id != a);
        assert!(s.get_entity(a).is_none());
        assert!(s.get_entity(999).is_some());

        // create_entity tras la mutación externa sigue siendo correcto.
        let b = s.create_entity("B".into(), vec![]);
        assert!(s.get_entity(b).is_some());
        assert_eq!(s.entities.len(), 2);
    }

    #[test]
    fn delete_conserva_semantica_de_padre_e_hijos_con_swap_remove() {
        let mut s = fresh_scene();
        let padre = s.create_entity("Padre".into(), vec![]);
        let hijo1 = s.create_entity("Hijo1".into(), vec![]);
        let hijo2 = s.create_entity("Hijo2".into(), vec![]);
        let nieto = s.create_entity("Nieto".into(), vec![]);
        // relleno para forzar swap_remove no trivial
        let relleno = s.create_entity("Relleno".into(), vec![]);

        s.reparent(hijo1, Some(padre));
        s.reparent(hijo2, Some(padre));
        s.reparent(nieto, Some(hijo1));

        // Borrar hijo1: sus hijos (nieto) deben pasar al padre (abuelo),
        // y padre debe perder a hijo1 pero ganar a nieto.
        assert!(s.delete_entity(hijo1));

        assert!(s.get_entity(hijo1).is_none());
        let p = s.get_entity(padre).unwrap();
        assert!(!p.children.contains(&hijo1));
        assert!(p.children.contains(&hijo2));
        assert!(p.children.contains(&nieto));

        let n = s.get_entity(nieto).unwrap();
        assert_eq!(n.parent, Some(padre));

        // El resto de entidades siguen siendo resolubles tras el swap_remove.
        assert!(s.get_entity(hijo2).is_some());
        assert!(s.get_entity(relleno).is_some());
        assert!(s.get_entity(padre).is_some());
        assert_eq!(s.entities.len(), 4); // padre, hijo2, nieto, relleno
    }
    #[test]
    fn luz_sin_campos_spot_parsea_con_defaults() {
        let json = r#"{"light_type":"Point","color":[1.0,1.0,1.0],"intensity":1.0,"range":10.0}"#;
        let light: LightData = serde_json::from_str(json).unwrap();
        assert_eq!(light.spot_outer_deg, 45.0);
        assert_eq!(light.spot_inner_deg, 35.0);
    }

    #[test]
    fn luz_sin_campos_area_parsea_con_defaults() {
        let json = r#"{"light_type":"Point","color":[1.0,1.0,1.0],"intensity":1.0,"range":10.0}"#;
        let light = serde_json::from_str::<LightData>(json).unwrap();
        assert_eq!(light.area_width, 5.0);
        assert_eq!(light.area_height, 3.0);
    }

    #[test]
    fn spot_cone_cos_sanea() {
        let mut light = LightData {
            spot_outer_deg: 30.0,
            spot_inner_deg: 50.0,
            ..Default::default()
        };
        let (cos_outer, cos_inner) = light.spot_cone_cos();
        assert!((cos_inner - cos_outer).abs() < 1e-6);

        light.spot_outer_deg = 200.0;
        light.spot_inner_deg = 10.0;
        let (cos_outer, _) = light.spot_cone_cos();
        assert!((cos_outer - 179.0_f32.to_radians().cos()).abs() < 1e-6);
    }

    #[test]
    fn presets_de_perfil() {
        assert_eq!(LightProfileData::preset("Vela").unwrap().prioridad, 0.3);
        let linterna = LightProfileData::preset("LinternaArma").unwrap();
        assert_eq!(linterna.grupo, 1);
        assert_eq!(linterna.tope_por_objeto, 2);
        assert!(LightProfileData::preset("Panteon").unwrap().sombra_por_prioridad);
        assert!(LightProfileData::preset("Inexistente").is_none());
    }

    #[test]
    fn perfil_sin_campos_parsea_con_defaults() {
        let profile: LightProfileData = serde_json::from_str(r#"{"preset":"Custom"}"#).unwrap();
        assert_eq!(profile.tope_por_objeto, 4);
        assert_eq!(profile.prioridad, 1.0);
    }

    fn physics_data_base() -> PhysicsData {
        PhysicsData {
            has_rigidbody: false,
            has_collider: false,
            ..PhysicsData::default()
        }
    }

    fn spec_con_offset(shape: &str, x: f32) -> ColliderSpec {
        let mut spec = spec_por_defecto(shape);
        spec.offset = [x, 0.0, 0.0];
        spec
    }

    #[test]
    fn spec_por_defecto_devuelve_defaults_por_forma() {
        let b = spec_por_defecto("Box");
        assert_eq!(b.shape, "Box");
        assert_eq!(b.size, [0.5, 0.5, 0.5]);
        assert_eq!(b.offset, [0.0, 0.0, 0.0]);
        assert!(!b.is_trigger);
        let e = spec_por_defecto("Sphere");
        assert_eq!(e.shape, "Sphere");
        assert_eq!(e.size, [0.5, 0.0, 0.0]);
        let c = spec_por_defecto("Capsule");
        assert_eq!(c.shape, "Capsule");
        assert_eq!(c.size, [0.5, 0.5, 0.0]);
        let ci = spec_por_defecto("Cylinder");
        assert_eq!(ci.shape, "Cylinder");
        assert_eq!(ci.size, [0.5, 0.5, 0.0]);
    }

    #[test]
    fn spec_por_defecto_forma_desconocida_cae_a_box() {
        let spec = spec_por_defecto("Pyramid");
        assert_eq!(spec.shape, "Box");
        assert_eq!(spec.size, [0.5, 0.5, 0.5]);
    }

    #[test]
    fn anadir_collider_pone_has_collider() {
        let mut physics = physics_data_base();
        assert!(!physics.has_collider);
        physics.anadir_collider(spec_por_defecto("Sphere"));
        assert!(physics.has_collider);
        assert_eq!(physics.colliders.len(), 1);
        assert_eq!(physics.total_colliders(), 1);
    }

    #[test]
    fn duplicar_collider_inserta_tras_el_original_y_conserva_orden() {
        let mut physics = physics_data_base();
        physics.anadir_collider(spec_con_offset("Box", 0.0));
        physics.anadir_collider(spec_con_offset("Sphere", 1.0));
        physics.anadir_collider(spec_con_offset("Capsule", 2.0));
        assert!(physics.duplicar_collider(1));
        let formas: Vec<_> = physics
            .colliders
            .iter()
            .map(|c| c.shape.as_str())
            .collect();
        assert_eq!(formas, vec!["Box", "Sphere", "Sphere", "Capsule"]);
        let offsets: Vec<_> = physics.colliders.iter().map(|c| c.offset).collect();
        assert_eq!(
            offsets,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
            ]
        );
    }

    #[test]
    fn duplicar_collider_fuera_de_rango_no_toca_nada() {
        let mut physics = physics_data_base();
        physics.anadir_collider(spec_con_offset("Box", 0.0));
        assert!(!physics.duplicar_collider(5));
        assert_eq!(physics.colliders.len(), 1);
    }

    #[test]
    fn quitar_ultimo_spec_sin_mesh_apaga_has_collider() {
        let mut physics = physics_data_base();
        physics.anadir_collider(spec_por_defecto("Box"));
        let quitado = physics.quitar_collider(0);
        assert!(quitado.is_some());
        assert!(physics.colliders.is_empty());
        assert!(!physics.has_collider);
        assert_eq!(physics.total_colliders(), 0);
    }

    #[test]
    fn quitar_spec_con_mesh_presente_conserva_has_collider() {
        let mut physics = physics_data_base();
        physics.anadir_collider(spec_por_defecto("Box"));
        physics.mesh_collider = MeshColliderMode::Convex;
        let quitado = physics.quitar_collider(0);
        assert!(quitado.is_some());
        assert!(physics.colliders.is_empty());
        assert!(physics.has_collider);
        assert_eq!(physics.total_colliders(), 1);
    }

    #[test]
    fn quitar_mesh_con_specs_conserva_has_collider_y_sin_specs_lo_apaga() {
        let mut con_specs = physics_data_base();
        con_specs.anadir_collider(spec_por_defecto("Box"));
        con_specs.mesh_collider = MeshColliderMode::Convex;
        con_specs.quitar_mesh_collider();
        assert!(matches!(con_specs.mesh_collider, MeshColliderMode::None));
        assert!(con_specs.has_collider);
        assert_eq!(con_specs.total_colliders(), 1);

        let mut sin_specs = physics_data_base();
        sin_specs.has_collider = true;
        sin_specs.mesh_collider = MeshColliderMode::Convex;
        sin_specs.quitar_mesh_collider();
        assert!(matches!(sin_specs.mesh_collider, MeshColliderMode::None));
        assert!(!sin_specs.has_collider);
        assert_eq!(sin_specs.total_colliders(), 0);
    }

    #[test]
    fn total_colliders_cubre_los_cuatro_estados() {
        let sin_collider = physics_data_base();
        assert_eq!(sin_collider.total_colliders(), 0);

        let mut legacy = physics_data_base();
        legacy.has_collider = true;
        assert_eq!(legacy.total_colliders(), 1);

        let mut specs = physics_data_base();
        specs.anadir_collider(spec_por_defecto("Box"));
        specs.anadir_collider(spec_por_defecto("Sphere"));
        assert_eq!(specs.total_colliders(), 2);

        let mut specs_mesh = physics_data_base();
        specs_mesh.anadir_collider(spec_por_defecto("Box"));
        specs_mesh.anadir_collider(spec_por_defecto("Sphere"));
        specs_mesh.mesh_collider = MeshColliderMode::Convex;
        assert_eq!(specs_mesh.total_colliders(), 3);
    }

const TOL: f32 = 1e-6;

    fn assert_f32(actual: f32, esperado: f32, que: &str) {
        assert!(
            (actual - esperado).abs() < TOL,
            "{que}: esperado {esperado}, obtenido {actual}"
        );
    }

    fn assert_opt_f32(actual: Option<f32>, esperado: f32, que: &str) {
        match actual {
            Some(v) => assert_f32(v, esperado, que),
            None => panic!("{que}: esperado Some({esperado}), obtenido None"),
        }
    }

    fn assert_arr3(actual: [f32; 3], esperado: [f32; 3], que: &str) {
        for i in 0..3 {
            assert_f32(actual[i], esperado[i], &format!("{que}[{i}]"));
        }
    }

    /// Busca una cadena exacta en cualquier nivel de un `serde_json::Value`.
    /// Se usa para comprobar `mesh` sin depender de su tipo concreto.
    fn json_contiene_cadena(v: &serde_json::Value, buscada: &str) -> bool {
        match v {
            serde_json::Value::String(s) => s == buscada,
            serde_json::Value::Array(xs) => xs.iter().any(|x| json_contiene_cadena(x, buscada)),
            serde_json::Value::Object(m) => m.values().any(|x| json_contiene_cadena(x, buscada)),
            _ => false,
        }
    }

    fn entidad<'a>(escena: &'a [EntityData], nombre: &str) -> &'a EntityData {
        escena
            .iter()
            .find(|e| e.name == nombre)
            .unwrap_or_else(|| panic!("la escena no contiene la entidad «{nombre}»"))
    }

    #[test]
    fn escena_antigua_real_carga_con_legado() {
        let texto = include_str!("../../../assets/scenes/PrimitivesPhysicsTest.scene.json");
        let escena: Vec<EntityData> =
            serde_json::from_str(texto).expect("la escena antigua debe deserializar");

        // «Suelo Fisico»
        let suelo = entidad(&escena, "Suelo Fisico");
        let fis = suelo
            .physics
            .as_ref()
            .expect("«Suelo Fisico» debe tener physics");

        assert_opt_f32(fis.legacy_friction, 0.1, "Suelo.legacy_friction");
        assert_opt_f32(fis.legacy_restitution, 0.6, "Suelo.legacy_restitution");
        assert!(
            fis.colliders.is_empty(),
            "una escena antigua no trae specs de collider; obtenidos {}",
            fis.colliders.len()
        );
        assert_eq!(fis.mesh_collider, MeshColliderMode::None);
        assert_arr3(fis.box_extents, [15.0, 0.5, 15.0], "Suelo.box_extents");
        assert_eq!(fis.body_type, "Static");

        let mat = fis.body_fallback_material();
        assert_f32(mat.dynamic_friction, 0.1, "Suelo.fallback.dynamic_friction");
        assert_f32(mat.bounciness, 0.6, "Suelo.fallback.bounciness");
        // Guardas explícitas contra caer en el defecto del proyecto (0.7 / 0.05).
        assert!((mat.dynamic_friction - 0.7).abs() > TOL, "usa el defecto 0.7 en vez del legado");
        assert!((mat.bounciness - 0.05).abs() > TOL, "usa el defecto 0.05 en vez del legado");

        // «Esfera»
        let esfera = entidad(&escena, "Esfera");
        let fis_esfera = esfera
            .physics
            .as_ref()
            .expect("«Esfera» debe tener physics");
        assert_opt_f32(fis_esfera.legacy_friction, 0.1, "Esfera.legacy_friction");

        let mesh = serde_json::to_value(&esfera.mesh).expect("mesh serializa");
        assert!(
            json_contiene_cadena(&mesh, "sphere"),
            "«Esfera» debe usar el modelo \"sphere\"; mesh = {mesh}"
        );
    }

    #[test]
    fn escena_de_bloque_conserva_referencia_trigger_y_malla() {
        let fisica_json = physics_json_con(&[
            (
                "colliders",
                serde_json::json!([{
                    "shape": "Box",
                    "size": [0.5, 0.5, 0.5],
                    "offset": [0.0, 0.0, 0.0],
                    "is_trigger": true,
                    "material": "materials/hielo",
                    "direction": "Y"
                }]),
            ),
            ("mesh_collider", serde_json::json!("Convex")),
            ("mesh_collider_material", serde_json::json!("materials/hielo")),
            ("mesh_collider_is_trigger", serde_json::json!(true)),
        ]);

        // Carga
        let fis: PhysicsData = serde_json::from_value(fisica_json)
            .expect("el JSON de bloque debe deserializar a PhysicsData");

        assert_eq!(fis.colliders.len(), 1, "debe haber exactamente un spec");
        let spec = &fis.colliders[0];
        assert_eq!(spec.material, "materials/hielo");
        assert!(spec.is_trigger, "el spec debe seguir siendo trigger");
        assert_eq!(fis.mesh_collider, MeshColliderMode::Convex);
        assert!(fis.mesh_collider_is_trigger);
        assert_eq!(fis.mesh_collider_material, "materials/hielo");

        // Guardado: cargar + guardar no debe reescribir los specs.
        let guardado = serde_json::to_value(&fis).expect("PhysicsData serializa");

        let specs = guardado["colliders"]
            .as_array()
            .expect("`colliders` guardado debe ser un array");
        assert_eq!(specs.len(), 1, "el guardado debe conservar un único spec");
        assert_eq!(specs[0]["material"], serde_json::json!("materials/hielo"));
        assert_eq!(specs[0]["is_trigger"], serde_json::json!(true));
        assert_eq!(guardado["mesh_collider_is_trigger"], serde_json::json!(true));
        assert_eq!(guardado["mesh_collider_material"], serde_json::json!("materials/hielo"));
        assert_eq!(guardado["mesh_collider"], serde_json::json!("Convex"));
    }

}

#[cfg(test)]
mod prefab_diff_tests {
    use super::*;
    use crate::prefab::{OverrideAnadida, OverridePropiedad, Prefab, PrefabOverrides};

    const FUENTE: &str = "assets/prefabs/diff_test.prefab.json";

    fn entidad(
        id: u64,
        nombre: &str,
        comps: &[&str],
        parent: Option<u64>,
        children: &[u64],
    ) -> EntityData {
        let mut e = EntityData::new(
            id,
            nombre.into(),
            comps.iter().map(|c| c.to_string()).collect(),
        );
        e.parent = parent;
        e.children = children.to_vec();
        e
    }

    /// Raiz(10) → A(11) → C(13, con malla); Raiz(10) → B(12).
    fn prefab_fixture() -> Prefab {
        Prefab {
            format_version: 1,
            name: "DiffTest".to_string(),
            root_entity: entidad(10, "Raiz", &["Transform"], None, &[11, 12]),
            descendant_entities: vec![
                entidad(11, "A", &["Transform"], Some(10), &[13]),
                entidad(12, "B", &["Transform"], Some(10), &[]),
                entidad(13, "C", &["Transform", "MeshRenderer"], Some(11), &[]),
            ],
            variant_of: None,
            variant_overrides: None,
        }
    }

    fn instanciar() -> (SceneState, Prefab, u64) {
        let prefab = prefab_fixture();
        let mut scene = SceneState::default();
        let raiz = scene.instantiate_prefab(&prefab, None, None, Some(FUENTE));
        (scene, prefab, raiz)
    }

    fn id_de(scene: &SceneState, local: u64) -> u64 {
        scene
            .entities
            .iter()
            .find(|e| e.prefab_local_id == Some(local))
            .map(|e| e.id)
            .expect("local_id presente en la instancia")
    }

    fn anadir_hijo(scene: &mut SceneState, padre: u64, nombre: &str) -> u64 {
        let id = scene.next_entity_id;
        scene.next_entity_id += 1;
        scene
            .entities
            .push(entidad(id, nombre, &["Transform"], Some(padre), &[]));
        scene.get_entity_mut(padre).unwrap().children.push(id);
        id
    }

    #[test]
    fn diff_sin_cambios_es_vacio() {
        let (scene, prefab, raiz) = instanciar();
        let diff = scene.calcular_diff_prefab(raiz, &prefab).expect("raíz vinculada");
        assert!(diff.esta_vacia());
    }

    #[test]
    fn diff_posicion_de_hijo() {
        let (mut scene, prefab, raiz) = instanciar();
        let c = id_de(&scene, 13);
        scene.get_entity_mut(c).unwrap().transform.position = [1.0, 2.0, 3.0];

        let diff = scene.calcular_diff_prefab(raiz, &prefab).unwrap();
        assert_eq!(diff.propiedades.len(), 1);
        let p = &diff.propiedades[0];
        assert_eq!(p.local_id, 13);
        assert_eq!(p.path, "transform.position");
        assert_eq!(p.valor, serde_json::json!([1.0, 2.0, 3.0]));
        assert!(!p.huerfana);
        assert!(diff.anadidas.is_empty());
        assert!(diff.quitadas.is_empty());
    }

    #[test]
    fn diff_raiz_movida_y_renombrada_es_vacio() {
        let (mut scene, prefab, raiz) = instanciar();
        {
            let r = scene.get_entity_mut(raiz).unwrap();
            r.transform.position = [5.0, 0.0, -3.0];
            r.name = "Renombrada".into();
        }
        let diff = scene.calcular_diff_prefab(raiz, &prefab).unwrap();
        assert!(diff.esta_vacia());
    }

    #[test]
    fn diff_hijo_anadido_bajo_descendiente() {
        let (mut scene, prefab, raiz) = instanciar();
        let a = id_de(&scene, 11);
        let extra = anadir_hijo(&mut scene, a, "Extra");
        // La nieta cuelga de una añadida: solo cuenta la raíz del subárbol.
        anadir_hijo(&mut scene, extra, "Nieta");

        let diff = scene.calcular_diff_prefab(raiz, &prefab).unwrap();
        assert_eq!(diff.anadidas.len(), 1);
        let an = &diff.anadidas[0];
        assert_eq!(an.padre_local_id, Some(11));
        assert_eq!(an.nombre, "Extra");
        assert_eq!(an.indice, 0);
        assert!(diff.propiedades.is_empty());
        assert!(diff.quitadas.is_empty());
    }

    #[test]
    fn diff_hijos_anadidos_bajo_raiz() {
        let (mut scene, prefab, raiz) = instanciar();
        anadir_hijo(&mut scene, raiz, "Extra0");
        anadir_hijo(&mut scene, raiz, "Extra1");

        let diff = scene.calcular_diff_prefab(raiz, &prefab).unwrap();
        assert_eq!(diff.anadidas.len(), 2);
        assert!(diff.anadidas.iter().all(|a| a.padre_local_id == Some(10)));
        assert_eq!(diff.anadidas[0].nombre, "Extra0");
        assert_eq!(diff.anadidas[0].indice, 0);
        assert_eq!(diff.anadidas[1].nombre, "Extra1");
        assert_eq!(diff.anadidas[1].indice, 1);
    }

    #[test]
    fn diff_hijo_quitado() {
        let (mut scene, prefab, raiz) = instanciar();
        let b = id_de(&scene, 12);
        scene.entities.retain(|e| e.id != b);
        scene.get_entity_mut(raiz).unwrap().children.retain(|&c| c != b);

        let diff = scene.calcular_diff_prefab(raiz, &prefab).unwrap();
        assert_eq!(diff.quitadas, vec![12]);
        assert!(diff.anadidas.is_empty());
        assert!(diff.propiedades.is_empty());
    }

    #[test]
    fn diff_mesh_de_hijo() {
        let (mut scene, prefab, raiz) = instanciar();
        let c = id_de(&scene, 13);
        scene.get_entity_mut(c).unwrap().mesh = None;

        let diff = scene.calcular_diff_prefab(raiz, &prefab).unwrap();
        assert_eq!(diff.propiedades.len(), 1);
        let p = &diff.propiedades[0];
        assert_eq!(p.local_id, 13);
        assert_eq!(p.path, "mesh");
        let esperado = crate::prefab::valor_en_path(scene.get_entity(c).unwrap(), "mesh")
            .unwrap_or(serde_json::Value::Null);
        assert_eq!(p.valor, esperado);
    }

    #[test]
    fn diff_no_raiz_vinculada_es_none() {
        let (mut scene, prefab, _raiz) = instanciar();
        let a = id_de(&scene, 11);
        assert!(scene.calcular_diff_prefab(a, &prefab).is_none());
        assert!(scene.calcular_diff_prefab(u64::MAX, &prefab).is_none());

        let suelta = scene.instantiate_prefab(&prefab, None, None, None);
        assert!(scene.calcular_diff_prefab(suelta, &prefab).is_none());
    }

    #[test]
    fn prefab_overrides_round_trip_json() {
        let mut e = entidad(7, "Inst", &["Transform"], None, &[]);
        e.prefab_source = Some(FUENTE.into());
        e.prefab_local_id = Some(10);
        e.prefab_overrides = Some(PrefabOverrides {
            propiedades: vec![OverridePropiedad {
                local_id: 13,
                path: "transform.position".into(),
                valor: serde_json::json!([1.0, 2.0, 3.0]),
                huerfana: false,
            }],
            anadidas: vec![OverrideAnadida {
                padre_local_id: Some(11),
                nombre: "Extra".into(),
                indice: 0,
                subarbol: None,
            }],
            quitadas: vec![12],
        });

        let json = serde_json::to_value(&e).unwrap();
        let vuelta: EntityData = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&vuelta).unwrap(), json);

        let o = vuelta.prefab_overrides.expect("overrides tras round-trip");
        assert_eq!(o.propiedades.len(), 1);
        assert_eq!(o.propiedades[0].path, "transform.position");
        assert_eq!(o.anadidas[0].padre_local_id, Some(11));
        assert_eq!(o.quitadas, vec![12]);
    }
}

#[cfg(test)]
mod prefab_refresco_diff_tests {
    use super::*;
    use crate::prefab::Prefab;

    const FUENTE: &str = "assets/prefabs/refresco_diff.prefab.json";

    fn entidad(
        id: u64,
        nombre: &str,
        comps: &[&str],
        parent: Option<u64>,
        children: &[u64],
    ) -> EntityData {
        let mut e = EntityData::new(
            id,
            nombre.into(),
            comps.iter().map(|c| c.to_string()).collect(),
        );
        e.parent = parent;
        e.children = children.to_vec();
        e
    }

    /// Raiz(10) → A(11) → C(13, con malla); Raiz(10) → B(12).
    fn prefab_fixture() -> Prefab {
        Prefab {
            format_version: 1,
            name: "RefrescoDiff".to_string(),
            root_entity: entidad(10, "Raiz", &["Transform"], None, &[11, 12]),
            descendant_entities: vec![
                entidad(11, "A", &["Transform"], Some(10), &[13]),
                entidad(12, "B", &["Transform"], Some(10), &[]),
                entidad(13, "C", &["Transform", "MeshRenderer"], Some(11), &[]),
            ],
            variant_of: None,
            variant_overrides: None,
        }
    }

    fn instanciar() -> (SceneState, Prefab, u64) {
        let prefab = prefab_fixture();
        let mut scene = SceneState::default();
        let raiz = scene.instantiate_prefab(&prefab, None, None, Some(FUENTE));
        (scene, prefab, raiz)
    }

    fn id_de(scene: &SceneState, local: u64) -> u64 {
        scene
            .entities
            .iter()
            .find(|e| e.prefab_local_id == Some(local))
            .map(|e| e.id)
            .expect("local_id presente en la instancia")
    }

    fn guardar_diff(scene: &mut SceneState, raiz: u64, prefab: &Prefab) {
        let diff = scene.calcular_diff_prefab(raiz, prefab).expect("raíz vinculada");
        assert!(!diff.esta_vacia(), "el test necesita un diff no vacío");
        scene.get_entity_mut(raiz).unwrap().prefab_overrides = Some(diff);
    }

    #[test]
    fn refresco_con_diff_trae_lo_nuevo_y_conserva_lo_propio() {
        let (mut scene, prefab, raiz) = instanciar();
        let c = id_de(&scene, 13);
        scene.get_entity_mut(c).unwrap().transform.position = [1.0, 2.0, 3.0];
        guardar_diff(&mut scene, raiz, &prefab);

        // El asset deriva: B se renombra y C cambia a un tercer valor.
        let mut editado = prefab_fixture();
        editado.descendant_entities[1].name = "B2".to_string();
        editado.descendant_entities[2].transform.position = [9.0, 9.0, 9.0];
        let huella_editada = editado.huella();

        let avisos = scene.refrescar_instancias_prefab(&|_| Some(editado.clone()));
        assert!(avisos.is_empty(), "avisos inesperados: {avisos:?}");

        let c2 = id_de(&scene, 13);
        assert_eq!(
            scene.get_entity(c2).unwrap().transform.position,
            [1.0, 2.0, 3.0],
            "la instancia conserva SU posición (override reaplicado)"
        );
        let b = id_de(&scene, 12);
        assert_eq!(scene.get_entity(b).unwrap().name, "B2", "llega lo nuevo del asset");
        assert_eq!(
            scene.get_entity(raiz).unwrap().prefab_hash.as_deref(),
            Some(huella_editada.as_str()),
            "la base del diff pasa a ser el asset nuevo"
        );

        // Segundo refresco sin deriva: no-op silencioso.
        let avisos2 = scene.refrescar_instancias_prefab(&|_| Some(editado.clone()));
        assert!(avisos2.is_empty(), "segundo refresco: {avisos2:?}");
        assert_eq!(scene.get_entity(c2).unwrap().transform.position, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn refresco_con_diff_marca_huerfano_y_aplica_el_resto() {
        let (mut scene, prefab, raiz) = instanciar();
        let c = id_de(&scene, 13);
        scene.get_entity_mut(c).unwrap().transform.position = [1.0, 2.0, 3.0];
        let b = id_de(&scene, 12);
        scene.get_entity_mut(b).unwrap().name = "BMio".to_string();
        guardar_diff(&mut scene, raiz, &prefab);

        // El asset nuevo ya no tiene a C.
        let mut editado = prefab_fixture();
        editado.root_entity.children = vec![11, 12];
        editado.descendant_entities[0].children = vec![];
        editado.descendant_entities.retain(|e| e.id != 13);

        let avisos = scene.refrescar_instancias_prefab(&|_| Some(editado.clone()));
        assert!(
            avisos.iter().any(|a| a.contains("huérfano")),
            "debe avisar del huérfano: {avisos:?}"
        );
        let b2 = id_de(&scene, 12);
        assert_eq!(scene.get_entity(b2).unwrap().name, "BMio", "el resto se aplica");
        assert!(
            scene.entities.iter().all(|e| e.prefab_local_id != Some(13)),
            "C desaparece con el asset"
        );
        let raiz_e = scene.get_entity(raiz).unwrap();
        let ov = raiz_e.prefab_overrides.as_ref().expect("diff conservado");
        let entrada_c = ov
            .propiedades
            .iter()
            .find(|p| p.local_id == 13)
            .expect("la entrada de C sigue en el diff");
        assert!(entrada_c.huerfana, "la entrada de C queda marcada huérfana");
        let entrada_b = ov
            .propiedades
            .iter()
            .find(|p| p.local_id == 12)
            .expect("la entrada de B sigue en el diff");
        assert!(!entrada_b.huerfana);
    }

    #[test]
    fn refresco_con_diff_respeta_quitadas() {
        let (mut scene, prefab, raiz) = instanciar();
        // La instancia quita a B (local 12).
        let b = id_de(&scene, 12);
        scene.entities.retain(|e| e.id != b);
        scene.get_entity_mut(raiz).unwrap().children.retain(|c| *c != b);
        guardar_diff(&mut scene, raiz, &prefab);

        // El asset deriva en A; B sigue existiendo en el asset.
        let mut editado = prefab_fixture();
        editado.descendant_entities[0].name = "A2".to_string();

        let avisos = scene.refrescar_instancias_prefab(&|_| Some(editado.clone()));
        assert!(avisos.is_empty(), "avisos inesperados: {avisos:?}");
        assert!(
            scene.entities.iter().all(|e| e.prefab_local_id != Some(12)),
            "la quitada sigue ausente tras el refresco"
        );
        let a = id_de(&scene, 11);
        assert_eq!(scene.get_entity(a).unwrap().name, "A2");
    }

    #[test]
    fn refresco_con_diff_preserva_anadidas_con_su_id() {
        let (mut scene, prefab, raiz) = instanciar();
        let a = id_de(&scene, 11);
        let extra = scene.next_entity_id;
        scene.next_entity_id += 1;
        scene
            .entities
            .push(entidad(extra, "Extra", &["Transform"], Some(a), &[]));
        scene.get_entity_mut(a).unwrap().children.push(extra);
        guardar_diff(&mut scene, raiz, &prefab);

        let mut editado = prefab_fixture();
        editado.descendant_entities[1].name = "B2".to_string();

        let avisos = scene.refrescar_instancias_prefab(&|_| Some(editado.clone()));
        assert!(avisos.is_empty(), "avisos inesperados: {avisos:?}");
        let extra_e = scene.get_entity(extra).expect("la añadida sobrevive al refresco");
        assert_eq!(extra_e.name, "Extra");
        assert_eq!(extra_e.parent, Some(a), "sigue hija del mismo padre");
        let b = id_de(&scene, 12);
        assert_eq!(scene.get_entity(b).unwrap().name, "B2");
    }

    #[test]
    fn refresco_sin_diff_no_toca_la_modificada() {
        let (mut scene, _prefab, _raiz) = instanciar();
        let c = id_de(&scene, 13);
        scene.get_entity_mut(c).unwrap().transform.position = [1.0, 2.0, 3.0];
        // Sin prefab_overrides: comportamiento de la Fase A.

        let mut editado = prefab_fixture();
        editado.descendant_entities[1].name = "B2".to_string();
        let avisos = scene.refrescar_instancias_prefab(&|_| Some(editado.clone()));
        assert!(
            avisos.iter().any(|a| a.contains("instancia modificada: no actualizada")),
            "aviso de la Fase A: {avisos:?}"
        );
        assert_eq!(scene.get_entity(c).unwrap().transform.position, [1.0, 2.0, 3.0]);
        let b = id_de(&scene, 12);
        assert_eq!(scene.get_entity(b).unwrap().name, "B", "no llega lo del asset");
    }
}

#[cfg(test)]
mod prefab_refresco_revision_tests {
    use super::*;
    use crate::prefab::Prefab;

    const FUENTE: &str = "assets/prefabs/revision.prefab.json";
    const INTERNO: &str = "assets/prefabs/interno.prefab.json";

    fn entidad(
        id: u64,
        nombre: &str,
        comps: &[&str],
        parent: Option<u64>,
        children: &[u64],
    ) -> EntityData {
        let mut e = EntityData::new(
            id,
            nombre.into(),
            comps.iter().map(|c| c.to_string()).collect(),
        );
        e.parent = parent;
        e.children = children.to_vec();
        e
    }

    fn prefab_fixture() -> Prefab {
        Prefab {
            format_version: 1,
            name: "Revision".to_string(),
            root_entity: entidad(10, "Raiz", &["Transform"], None, &[11, 12]),
            descendant_entities: vec![
                entidad(11, "A", &["Transform"], Some(10), &[13]),
                entidad(12, "B", &["Transform"], Some(10), &[]),
                entidad(13, "C", &["Transform", "MeshRenderer"], Some(11), &[]),
            ],
            variant_of: None,
            variant_overrides: None,
        }
    }

    fn prefab_interno() -> Prefab {
        Prefab {
            format_version: 1,
            name: "Interno".to_string(),
            root_entity: entidad(11, "Interno", &["Transform"], None, &[99]),
            descendant_entities: vec![entidad(99, "InternoHijo", &["Transform"], Some(11), &[])],
            variant_of: None,
            variant_overrides: None,
        }
    }

    fn id_de(scene: &SceneState, local: u64) -> u64 {
        scene
            .entities
            .iter()
            .find(|e| e.prefab_local_id == Some(local))
            .map(|e| e.id)
            .expect("local_id presente")
    }

    #[test]
    fn instancia_anidada_sobrevive_al_refresco_con_deriva() {
        let prefab = prefab_fixture();
        let interno = prefab_interno();
        let mut scene = SceneState::default();
        let raiz = scene.instantiate_prefab(&prefab, None, None, Some(FUENTE));

        // La escena pierde a A (11) y a C (13)...
        let a = id_de(&scene, 11);
        let c = id_de(&scene, 13);
        scene.entities.retain(|e| e.id != a && e.id != c);
        scene.get_entity_mut(raiz).unwrap().children.retain(|x| *x != a);

        // ...y bajo B cuelga una instancia anidada cuya raíz tiene
        // local_id 11 (el de A en el asset exterior).
        let b = id_de(&scene, 12);
        let in_raiz = scene.next_entity_id;
        scene.next_entity_id += 1;
        let in_hijo = scene.next_entity_id;
        scene.next_entity_id += 1;
        let mut r = entidad(in_raiz, "Interno", &["Transform"], Some(b), &[in_hijo]);
        r.prefab_source = Some(INTERNO.to_string());
        r.prefab_local_id = Some(11);
        r.prefab_hash = Some(interno.huella());
        scene.entities.push(r);
        let mut h = entidad(in_hijo, "InternoHijo", &["Transform"], Some(in_raiz), &[]);
        h.prefab_local_id = Some(99);
        scene.entities.push(h);
        scene.get_entity_mut(b).unwrap().children.push(in_raiz);

        // El diff NO debe contar el 11 como quitada (está presente como
        // raíz anidada) y SÍ como añadida.
        let diff = scene.calcular_diff_prefab(raiz, &prefab).expect("raíz vinculada");
        assert!(
            !diff.quitadas.contains(&11),
            "el local de la anidada no es una quitada: {diff:?}"
        );
        assert!(diff.quitadas.contains(&13));
        assert!(
            diff.anadidas.iter().any(|x| x.nombre == "Interno"),
            "la anidada cuenta como añadida: {diff:?}"
        );
        scene.get_entity_mut(raiz).unwrap().prefab_overrides = Some(diff);

        // El asset exterior deriva (B se renombra) y se refresca todo.
        let mut editado = prefab_fixture();
        editado.descendant_entities[1].name = "B2".to_string();
        let avisos = scene.refrescar_instancias_prefab(&|f: &str| {
            if f == INTERNO {
                Some(prefab_interno())
            } else {
                Some(editado.clone())
            }
        });
        assert!(avisos.is_empty(), "avisos inesperados: {avisos:?}");
        let r2 = scene.get_entity(in_raiz).expect("la anidada sobrevive al refresco");
        let b2 = id_de(&scene, 12);
        assert_eq!(r2.parent, Some(b2), "la anidada sigue colgando de B");
        assert_eq!(scene.get_entity(b2).unwrap().name, "B2");
        assert!(scene.get_entity(in_hijo).is_some(), "el hijo de la anidada viaja con ella");
        assert!(
            scene.entities.iter().all(|e| e.prefab_local_id != Some(13)),
            "la quitada C sigue ausente"
        );
    }

    #[test]
    fn duplicado_por_copiar_pegar_sobrevive_y_no_roba_el_override() {
        let prefab = prefab_fixture();
        let mut scene = SceneState::default();
        let raiz = scene.instantiate_prefab(&prefab, None, None, Some(FUENTE));
        let a = id_de(&scene, 11);
        let c = id_de(&scene, 13);

        // Copia de C con el mismo local_id (copiar-pegar en el editor).
        let copia = scene.next_entity_id;
        scene.next_entity_id += 1;
        let mut e = entidad(copia, "CCopia", &["Transform", "MeshRenderer"], Some(a), &[]);
        e.prefab_local_id = Some(13);
        scene.entities.push(e);
        scene.get_entity_mut(a).unwrap().children.push(copia);

        // Se mueve la C original: el override es del local 13.
        scene.get_entity_mut(c).unwrap().transform.position = [1.0, 2.0, 3.0];
        let diff = scene.calcular_diff_prefab(raiz, &prefab).expect("raíz vinculada");
        assert_eq!(diff.propiedades.len(), 1);
        assert!(
            diff.anadidas.iter().any(|x| x.nombre == "CCopia"),
            "la copia cuenta como añadida: {diff:?}"
        );
        scene.get_entity_mut(raiz).unwrap().prefab_overrides = Some(diff);

        let mut editado = prefab_fixture();
        editado.descendant_entities[1].name = "B2".to_string();
        let avisos = scene.refrescar_instancias_prefab(&|_| Some(editado.clone()));
        assert!(avisos.is_empty(), "avisos inesperados: {avisos:?}");

        let copia_e = scene.get_entity(copia).expect("la copia sobrevive al refresco");
        assert_eq!(copia_e.parent, Some(a));
        assert_eq!(
            copia_e.transform.position,
            [0.0, 0.0, 0.0],
            "la copia NO recibe el override del local 13"
        );
        assert_eq!(
            scene.get_entity(c).unwrap().transform.position,
            [1.0, 2.0, 3.0],
            "la vinculada original conserva su override"
        );
    }

    /// Asset exterior con una instancia anidada dentro: el descendiente
    /// 20 ES una raíz de instancia del asset INTERNO (su `prefab_local_id`
    /// en el asset, 11, es su id en el asset interior) y su hijo 21 trae
    /// el local_id interior 99. El 30 es un miembro normal.
    fn prefab_exterior_con_anidada() -> Prefab {
        let interno = prefab_interno();
        let mut anidada = entidad(20, "Interno", &["Transform"], Some(10), &[21]);
        anidada.prefab_source = Some(INTERNO.to_string());
        anidada.prefab_hash = Some(interno.huella());
        anidada.prefab_local_id = Some(11);
        let mut hijo_anidada = entidad(21, "InternoHijo", &["Transform"], Some(20), &[]);
        hijo_anidada.prefab_local_id = Some(99);
        Prefab {
            format_version: 1,
            name: "Exterior".to_string(),
            root_entity: entidad(10, "Exterior", &["Transform"], None, &[20, 30]),
            descendant_entities: vec![
                anidada,
                hijo_anidada,
                entidad(30, "Suelto", &["Transform"], Some(10), &[]),
            ],
            variant_of: None,
            variant_overrides: None,
        }
    }

    #[test]
    fn instanciar_preserva_el_vinculo_de_la_anidada() {
        let exterior = prefab_exterior_con_anidada();
        let interno = prefab_interno();
        let mut scene = SceneState::default();
        let raiz = scene.instantiate_prefab(&exterior, None, None, Some(FUENTE));

        let por_local = |l: u64| {
            scene
                .entities
                .iter()
                .find(|e| e.prefab_local_id == Some(l))
                .unwrap_or_else(|| panic!("sin entidad con local {l}"))
        };
        // La anidada conserva SU vínculo; su local_id pasa a ser su hueco
        // en el asset exterior (20), no su id interior (11).
        let anidada = por_local(20);
        assert_eq!(anidada.prefab_source.as_deref(), Some(INTERNO));
        assert_eq!(anidada.prefab_hash.as_deref(), Some(interno.huella().as_str()));
        // Su hijo conserva el local_id interior (99).
        let hijo = por_local(99);
        assert_eq!(hijo.parent, Some(anidada.id));
        assert!(hijo.prefab_source.is_none());
        // El miembro normal recibe su local del exterior y ningún vínculo.
        let suelto = por_local(30);
        assert!(suelto.prefab_source.is_none());
        assert_eq!(scene.get_entity(raiz).unwrap().prefab_source.as_deref(), Some(FUENTE));
    }

    #[test]
    fn desempaquetar_completa_corta_tambien_la_anidada() {
        let exterior = prefab_exterior_con_anidada();
        // Desempaquetar normal: la anidada SIGUE vinculada.
        let mut scene = SceneState::default();
        let raiz = scene.instantiate_prefab(&exterior, None, None, Some(FUENTE));
        scene.desempaquetar_instancia(raiz).unwrap();
        assert!(
            scene.entities.iter().any(|e| e.prefab_source.is_some()),
            "el desempaquetado normal deja viva la anidada"
        );

        // Completa: no queda vínculo alguno en el subárbol.
        let mut scene = SceneState::default();
        let raiz = scene.instantiate_prefab(&exterior, None, None, Some(FUENTE));
        scene.desempaquetar_instancia_completa(raiz).unwrap();
        assert!(scene.entities.iter().all(|e| e.prefab_source.is_none()
            && e.prefab_hash.is_none()
            && e.prefab_local_id.is_none()
            && e.prefab_overrides.is_none()));
        // Y sobre una entidad normal, error claro.
        let suelto = scene.entities.iter().find(|e| e.name == "Suelto").unwrap().id;
        assert!(scene.desempaquetar_instancia_completa(suelto).is_err());
    }

    #[test]
    fn la_anidada_se_refresca_cuando_deriva_su_propio_asset() {
        // Regresión del test físico de la Fase E: la raíz anidada guarda
        // en `prefab_local_id` su hueco en el asset EXTERIOR (20), que no
        // es su id en el suyo (11); clavear su huella con ese hueco la
        // hacía leerse «modificada» para siempre y nunca se refrescaba.
        let exterior = prefab_exterior_con_anidada();
        let mut scene = SceneState::default();
        scene.instantiate_prefab(&exterior, None, None, Some(FUENTE));

        let mut interno_nuevo = prefab_interno();
        interno_nuevo.descendant_entities[0].name = "InternoHijo2".to_string();
        interno_nuevo.descendant_entities[0].transform.position = [9.0, 0.0, 0.0];
        let huella_nueva = interno_nuevo.huella();

        let avisos = scene.refrescar_instancias_prefab(&|f: &str| {
            if f == INTERNO {
                Some(interno_nuevo.clone())
            } else {
                Some(prefab_exterior_con_anidada())
            }
        });
        assert!(avisos.is_empty(), "avisos inesperados: {avisos:?}");
        let hijo = scene
            .entities
            .iter()
            .find(|e| e.prefab_local_id == Some(99))
            .expect("el hijo de la anidada sigue");
        assert_eq!(hijo.name, "InternoHijo2");
        assert_eq!(hijo.transform.position, [9.0, 0.0, 0.0]);
        let anidada = scene
            .entities
            .iter()
            .find(|e| e.prefab_source.as_deref() == Some(INTERNO))
            .expect("la anidada sigue vinculada");
        assert_eq!(anidada.prefab_hash.as_deref(), Some(huella_nueva.as_str()));
        // Su local sigue siendo su hueco en la exterior (20), no su id
        // interior (11): la re-expansión de la anidada no debe pisarlo,
        // o la exterior la contaría como añadida y a su hueco como
        // quitada en el próximo recálculo.
        assert_eq!(anidada.prefab_local_id, Some(20));
        scene.recalcular_overrides_prefab(&|f: &str| {
            if f == INTERNO {
                Some(interno_nuevo.clone())
            } else {
                Some(prefab_exterior_con_anidada())
            }
        });
        assert!(
            scene.entities.iter().all(|e| e.prefab_overrides.is_none()),
            "tras refrescar la anidada, el recálculo no fabrica diffs"
        );
    }

    #[test]
    fn recalcular_no_fabrica_diff_para_la_exterior_con_anidada_almacenada() {
        // La exterior intacta con anidada ALMACENADA en su asset: el
        // recálculo al guardar no debe inventarse añadidas/quitadas con
        // el subárbol anidado (su hijo interior no es una quitada ni la
        // anidada una añadida de la exterior).
        let exterior = prefab_exterior_con_anidada();
        let mut scene = SceneState::default();
        scene.instantiate_prefab(&exterior, None, None, Some(FUENTE));
        scene.recalcular_overrides_prefab(&|f: &str| {
            if f == INTERNO {
                Some(prefab_interno())
            } else {
                Some(prefab_exterior_con_anidada())
            }
        });
        assert!(
            scene.entities.iter().all(|e| e.prefab_overrides.is_none()),
            "ninguna instancia intacta debe ganar un diff fabricado"
        );
    }

    #[test]
    fn la_deriva_de_la_exterior_reexpande_sin_romper_la_anidada_almacenada() {
        // El asset exterior deriva en un miembro normal («Suelto» se
        // renombra): la re-expansión trae el cambio y la anidada
        // almacenada conserva su vínculo y los locales interiores.
        let mut exterior_nuevo = prefab_exterior_con_anidada();
        exterior_nuevo.descendant_entities[2].name = "Suelto2".to_string();
        let mut scene = SceneState::default();
        scene.instantiate_prefab(&prefab_exterior_con_anidada(), None, None, Some(FUENTE));
        let avisos = scene.refrescar_instancias_prefab(&|f: &str| {
            if f == INTERNO {
                Some(prefab_interno())
            } else {
                Some(exterior_nuevo.clone())
            }
        });
        assert!(avisos.is_empty(), "avisos inesperados: {avisos:?}");
        assert!(scene.entities.iter().any(|e| e.name == "Suelto2"));
        let anidada = scene
            .entities
            .iter()
            .find(|e| e.prefab_source.as_deref() == Some(INTERNO))
            .expect("la anidada sigue vinculada tras re-expandir la exterior");
        assert_eq!(anidada.prefab_local_id, Some(20));
        let hijo = scene
            .entities
            .iter()
            .find(|e| e.prefab_local_id == Some(99))
            .expect("el hijo de la anidada conserva su local interior");
        assert_eq!(hijo.name, "InternoHijo");
        assert_eq!(hijo.parent, Some(anidada.id));
    }
}

#[cfg(test)]
mod prefab_recalculo_tests {
    use super::*;
    use crate::prefab::Prefab;

    const FUENTE: &str = "assets/prefabs/recalculo.prefab.json";

    fn entidad(id: u64, nombre: &str, parent: Option<u64>, children: &[u64]) -> EntityData {
        let mut e = EntityData::new(id, nombre.into(), vec!["Transform".to_string()]);
        e.parent = parent;
        e.children = children.to_vec();
        e
    }

    fn fixture() -> Prefab {
        Prefab {
            format_version: 1,
            name: "Recalculo".to_string(),
            root_entity: entidad(10, "Raiz", None, &[11]),
            descendant_entities: vec![entidad(11, "Hijo", Some(10), &[])],
            variant_of: None,
            variant_overrides: None,
        }
    }

    #[test]
    fn recalculo_intacta_sin_diff_y_modificada_con_diff() {
        let prefab = fixture();
        let mut scene = SceneState::default();
        let raiz = scene.instantiate_prefab(&prefab, None, None, Some(FUENTE));

        // Intacta: sin diff.
        scene.recalcular_overrides_prefab(&|_| Some(fixture()));
        assert!(scene.get_entity(raiz).unwrap().prefab_overrides.is_none());

        // Modificada (posición del hijo): el recálculo guarda el diff.
        let hijo = scene
            .entities
            .iter()
            .find(|e| e.prefab_local_id == Some(11))
            .map(|e| e.id)
            .unwrap();
        scene.get_entity_mut(hijo).unwrap().transform.position = [3.0, 0.0, 0.0];
        scene.recalcular_overrides_prefab(&|_| Some(fixture()));
        let ov = scene
            .get_entity(raiz)
            .unwrap()
            .prefab_overrides
            .as_ref()
            .expect("la modificada guarda su diff");
        assert_eq!(ov.propiedades.len(), 1);
        assert_eq!(ov.propiedades[0].path, "transform.position");

        // Vínculo roto: el diff guardado no se toca.
        scene.recalcular_overrides_prefab(&|_| None);
        assert!(scene.get_entity(raiz).unwrap().prefab_overrides.is_some());
    }
}

#[cfg(test)]
mod g1_ambient_tests {
    use super::*;

    /// Conversión sRGB -> lineal (IEC 61966-2-1), usada solo para
    /// documentar en el test de dónde vienen los literales URP_*.
    fn srgb_to_linear(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    const EPS: f32 = 1e-4;

    fn approx_eq3(a: [f32; 3], b: [f32; 3], eps: f32) {
        for i in 0..3 {
            assert!(
                (a[i] - b[i]).abs() <= eps,
                "canal {i}: {} vs {} (eps {eps})",
                a[i], b[i]
            );
        }
    }

    // --- Default / serde ---

    #[test]
    fn default_matches_urp_table() {
        let amb = AmbientComponent::default();
        assert_eq!(amb.sky, URP_AMBIENT_SKY);
        assert_eq!(amb.equator, URP_AMBIENT_EQUATOR);
        assert_eq!(amb.ground, URP_AMBIENT_GROUND);
        assert_eq!(amb.intensity, URP_AMBIENT_INTENSITY);
        // Los literales URP_* son los colores por defecto del gradient
        // de Unity (sRGB) convertidos a lineal: se comprueba la
        // conversión canal a canal con tolerancia de redondeo.
        let sky_srgb = [0.212, 0.227, 0.259];
        let equator_srgb = [0.114, 0.125, 0.133];
        let ground_srgb = [0.047, 0.043, 0.035];
        for i in 0..3 {
            assert!((srgb_to_linear(sky_srgb[i]) - URP_AMBIENT_SKY[i]).abs() < 2e-3);
            assert!((srgb_to_linear(equator_srgb[i]) - URP_AMBIENT_EQUATOR[i]).abs() < 2e-3);
            assert!((srgb_to_linear(ground_srgb[i]) - URP_AMBIENT_GROUND[i]).abs() < 2e-3);
        }
    }

    #[test]
    fn entity_without_key_resolves_to_none() {
        let e = EntityData::new(1, "E".into(), vec![]);
        let mut v = serde_json::to_value(&e).unwrap();
        v.as_object_mut().unwrap().remove("ambient");
        let back: EntityData = serde_json::from_value(v).unwrap();
        assert!(back.ambient.is_none());
    }

    #[test]
    fn entity_with_null_ambient_resolves_to_none() {
        let e = EntityData::new(1, "E".into(), vec![]);
        let mut v = serde_json::to_value(&e).unwrap();
        v.as_object_mut()
            .unwrap()
            .insert("ambient".into(), serde_json::Value::Null);
        let back: EntityData = serde_json::from_value(v).unwrap();
        assert!(back.ambient.is_none());
    }

    #[test]
    fn empty_object_resolves_to_default() {
        let amb: AmbientComponent = serde_json::from_str("{}").unwrap();
        assert_eq!(amb, AmbientComponent::default());
    }

    #[test]
    fn partial_object_fills_missing_with_default() {
        let amb: AmbientComponent = serde_json::from_str(r#"{ "intensity": 2.5 }"#).unwrap();
        assert_eq!(amb.sky, URP_AMBIENT_SKY);
        assert_eq!(amb.equator, URP_AMBIENT_EQUATOR);
        assert_eq!(amb.ground, URP_AMBIENT_GROUND);
        assert_eq!(amb.intensity, 2.5);
    }

    #[test]
    fn roundtrip_serialize_deserialize() {
        let amb = AmbientComponent {
            sky: [0.1, 0.2, 0.3],
            equator: [0.4, 0.5, 0.6],
            ground: [0.7, 0.8, 0.9],
            intensity: 3.0,
        };
        let json = serde_json::to_string(&amb).unwrap();
        let back: AmbientComponent = serde_json::from_str(&json).unwrap();
        assert_eq!(amb, back);
    }

    // --- Resolución ---

    fn entity_with_ambient(ambient: Option<AmbientComponent>) -> EntityData {
        let mut e = EntityData::new(1, "E".into(), vec![]);
        e.ambient = ambient;
        e
    }

    #[test]
    fn resolve_zero_carriers_is_legacy() {
        let entities = vec![entity_with_ambient(None), entity_with_ambient(None)];
        let resolved = resolve_ambient(&entities);
        assert_eq!(resolved.sky, LEGACY_AMBIENT_SKY);
        assert_eq!(resolved.equator, LEGACY_AMBIENT_EQUATOR);
        assert_eq!(resolved.ground, LEGACY_AMBIENT_GROUND);
        assert_eq!(resolved.intensity, None);
        assert_eq!(resolved.source, AmbientSource::Legacy);
        assert_eq!(resolved.extra_carriers, 0);
    }

    #[test]
    fn resolve_one_carrier_is_explicit() {
        let amb = AmbientComponent {
            sky: [0.1, 0.2, 0.3],
            equator: [0.4, 0.5, 0.6],
            ground: [0.7, 0.8, 0.9],
            intensity: 2.0,
        };
        let entities = vec![
            entity_with_ambient(None),
            entity_with_ambient(Some(amb.clone())),
        ];
        let resolved = resolve_ambient(&entities);
        assert_eq!(resolved.sky, amb.sky);
        assert_eq!(resolved.equator, amb.equator);
        assert_eq!(resolved.ground, amb.ground);
        assert_eq!(resolved.intensity, Some(2.0));
        assert_eq!(resolved.source, AmbientSource::Explicit);
        assert_eq!(resolved.extra_carriers, 0);
    }

    #[test]
    fn resolve_two_carriers_uses_first_and_counts_extra() {
        let first = AmbientComponent {
            sky: [0.1, 0.1, 0.1],
            ..Default::default()
        };
        let second = AmbientComponent {
            sky: [0.9, 0.9, 0.9],
            ..Default::default()
        };
        let entities = vec![
            entity_with_ambient(Some(first.clone())),
            entity_with_ambient(Some(second)),
        ];
        let resolved = resolve_ambient(&entities);
        assert_eq!(resolved.sky, first.sky);
        assert_eq!(resolved.source, AmbientSource::Explicit);
        assert_eq!(resolved.extra_carriers, 1);
    }

    #[test]
    fn resolve_ignores_inactive_carrier() {
        let mut e = entity_with_ambient(Some(AmbientComponent::default()));
        e.active = false;
        let resolved = resolve_ambient(&[e]);
        assert_eq!(resolved.source, AmbientSource::Legacy);
        assert_eq!(resolved.intensity, None);
    }

    #[test]
    fn resolve_sanitizes_negative_and_non_finite_channels() {
        let amb = AmbientComponent {
            sky: [-1.0, f32::NAN, f32::INFINITY],
            equator: [f32::NEG_INFINITY, 0.5, -0.0001],
            ground: [0.0, 1.0, 2.0],
            intensity: f32::NAN,
        };
        let entities = vec![entity_with_ambient(Some(amb))];
        let resolved = resolve_ambient(&entities);
        // No finito (NaN, ±∞) o negativo -> 0.0; el 2.0 HDR se conserva.
        assert_eq!(resolved.sky, [0.0, 0.0, 0.0]);
        assert_eq!(resolved.equator, [0.0, 0.5, 0.0]);
        assert_eq!(resolved.ground, [0.0, 1.0, 2.0]);
        assert_eq!(resolved.intensity, Some(1.0));
    }

    #[test]
    fn resolve_clamps_intensity_to_0_8_range() {
        let too_high = AmbientComponent {
            intensity: 100.0,
            ..Default::default()
        };
        let too_low = AmbientComponent {
            intensity: -5.0,
            ..Default::default()
        };
        let r_high = resolve_ambient(&[entity_with_ambient(Some(too_high))]);
        let r_low = resolve_ambient(&[entity_with_ambient(Some(too_low))]);
        assert_eq!(r_high.intensity, Some(8.0));
        assert_eq!(r_low.intensity, Some(0.0));
    }

    // --- Equivalencia de fórmula ---

    fn old_formula(normal_y: f32, intensity: f32) -> [f32; 3] {
        let g = [0.62_f32, 0.58, 0.55];
        let s = [0.82_f32, 0.88, 1.0];
        let t = normal_y * 0.5 + 0.5;
        let hemi = 0.45 + 0.55 * t;
        let mix3 = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
            [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]
        };
        let color = mix3(g, s, t);
        [
            intensity * hemi * color[0],
            intensity * hemi * color[1],
            intensity * hemi * color[2],
        ]
    }

    fn new_formula(
        normal_y: f32,
        intensity: f32,
        sky: [f32; 3],
        equator: [f32; 3],
        ground: [f32; 3],
    ) -> [f32; 3] {
        let pole = if normal_y >= 0.0 { sky } else { ground };
        let t = normal_y.abs();
        let mix3 = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
            [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]
        };
        let color = mix3(equator, pole, t);
        [
            intensity * color[0],
            intensity * color[1],
            intensity * color[2],
        ]
    }

    #[test]
    fn equivalence_exact_at_stops_for_lit_and_unlit() {
        for intensity in [LEGACY_AMBIENT_INTENSITY_LIT, LEGACY_AMBIENT_INTENSITY_UNLIT] {
            for ny in [-1.0_f32, 0.0, 1.0] {
                let old = old_formula(ny, intensity);
                let new = new_formula(
                    ny,
                    intensity,
                    LEGACY_AMBIENT_SKY,
                    LEGACY_AMBIENT_EQUATOR,
                    LEGACY_AMBIENT_GROUND,
                );
                approx_eq3(old, new, EPS);
            }
        }
    }

    #[test]
    fn equivalence_error_bounded_over_201_samples() {
        let intensity = LEGACY_AMBIENT_INTENSITY_LIT;
        let s = [0.82_f32, 0.88, 1.0];
        let g = [0.62_f32, 0.58, 0.55];
        for i in 0..=200 {
            let ny = -1.0 + 2.0 * (i as f32) / 200.0;
            let old = old_formula(ny, intensity);
            let new = new_formula(
                ny,
                intensity,
                LEGACY_AMBIENT_SKY,
                LEGACY_AMBIENT_EQUATOR,
                LEGACY_AMBIENT_GROUND,
            );
            for c in 0..3 {
                let diff = new[c] - old[c];
                let upper = 0.034375 * intensity * (s[c] - g[c]);
                assert!(diff >= -1e-5, "canal {c} ny={ny}: diff {diff} < 0");
                assert!(
                    diff <= upper + 1e-5,
                    "canal {c} ny={ny}: diff {diff} > cota {upper}"
                );
            }
        }
    }

    #[test]
    fn equivalence_error_at_half_stops_matches_expected() {
        let intensity = LEGACY_AMBIENT_INTENSITY_LIT;
        let expected = [0.001719_f32, 0.002578, 0.003867];
        for &ny in &[0.5_f32, -0.5_f32] {
            let old = old_formula(ny, intensity);
            let new = new_formula(
                ny,
                intensity,
                LEGACY_AMBIENT_SKY,
                LEGACY_AMBIENT_EQUATOR,
                LEGACY_AMBIENT_GROUND,
            );
            for c in 0..3 {
                let diff = (new[c] - old[c]).abs();
                assert!(
                    (diff - expected[c]).abs() <= 1e-5,
                    "ny={ny} canal {c}: diff {diff} esperado {}",
                    expected[c]
                );
            }
        }
    }

    #[test]
    fn equivalence_continuous_at_zero() {
        let intensity = LEGACY_AMBIENT_INTENSITY_LIT;
        let eps = 1e-4_f32;
        let below = new_formula(
            -eps,
            intensity,
            LEGACY_AMBIENT_SKY,
            LEGACY_AMBIENT_EQUATOR,
            LEGACY_AMBIENT_GROUND,
        );
        let above = new_formula(
            eps,
            intensity,
            LEGACY_AMBIENT_SKY,
            LEGACY_AMBIENT_EQUATOR,
            LEGACY_AMBIENT_GROUND,
        );
        let at_zero = new_formula(
            0.0,
            intensity,
            LEGACY_AMBIENT_SKY,
            LEGACY_AMBIENT_EQUATOR,
            LEGACY_AMBIENT_GROUND,
        );
        approx_eq3(below, at_zero, 1e-3);
        approx_eq3(above, at_zero, 1e-3);
    }
}

// G5: valores re-autorados de la Demo FPS Polygon (look URP). El test
// carga la escena real y afirma los valores de diseño; si alguien los
// toca a mano en el JSON, esto lo caza.
#[cfg(test)]
mod g5_demo_values {
    use super::*;

    #[test]
    fn g5_demo_values() {
        let data = include_str!("../../../assets/scenes/DemoFpsPolygon.scene.json");
        let entities: Vec<EntityData> = serde_json::from_str(data)
            .unwrap_or_else(|e| panic!("JSON inválido en DemoFpsPolygon.scene.json: {e}"));

        let post = resolve_post(&entities);
        assert!(
            (post.exposure - 1.0).abs() < 1e-6,
            "exposure debe ser 1.0, es {}",
            post.exposure
        );

        let ambient = resolve_ambient(&entities);
        assert_eq!(ambient.sky, URP_AMBIENT_SKY, "sky debe ser el valor URP de referencia");
        assert_eq!(
            ambient.equator, URP_AMBIENT_EQUATOR,
            "equator debe ser el valor URP de referencia"
        );
        assert_eq!(
            ambient.ground, URP_AMBIENT_GROUND,
            "ground debe ser el valor URP de referencia"
        );
        assert_eq!(
            ambient.intensity,
            Some(URP_AMBIENT_INTENSITY),
            "ambient intensity debe ser {}",
            URP_AMBIENT_INTENSITY
        );

        let sol = entities
            .iter()
            .find(|e| e.id == 2)
            .expect("no se encontró la entidad Sol (id 2)");
        let intensity = sol
            .light
            .as_ref()
            .expect("la entidad Sol no tiene light")
            .intensity;
        assert!(
            (intensity - 1.0).abs() < 1e-6,
            "intensidad autorada del Sol debe ser 1.0, es {intensity}"
        );
    }
}

#[cfg(test)]
mod g2_post_tests {
    use super::*;

    #[test]
    fn post_default_values() {
        let p = PostComponent::default();
        assert_eq!(p.exposure, 1.0);
        assert_eq!(p.intensity, 0.3);
        assert_eq!(p.vignette, 0.22);
    }

    #[test]
    fn post_resolve_explicit() {
        let mut e = EntityData::new(1, "E".into(), vec!["Transform".into()]);
        e.post = Some(PostComponent { exposure: 1.0, intensity: 0.5, vignette: 0.1 });
        let resolved = resolve_post(&[e]);
        assert_eq!(resolved.exposure, 1.0);
        assert_eq!(resolved.intensity, 0.5);
        assert_eq!(resolved.vignette, 0.1);
        assert_eq!(resolved.source, PostSource::Explicit);
    }

    #[test]
    fn post_resolve_first_active_wins() {
        let mut e1 = EntityData::new(1, "A".into(), vec!["Transform".into()]);
        e1.post = Some(PostComponent { exposure: 1.0, intensity: 0.4, vignette: 0.2 });
        let mut e2 = EntityData::new(2, "B".into(), vec!["Transform".into()]);
        e2.post = Some(PostComponent { exposure: 2.0, intensity: 0.9, vignette: 0.9 });
        e2.active = false;
        let resolved = resolve_post(&[e1, e2]);
        assert_eq!(resolved.exposure, 1.0);
        assert_eq!(resolved.intensity, 0.4);
        assert_eq!(resolved.vignette, 0.2);
        assert_eq!(resolved.source, PostSource::Explicit);
        assert_eq!(resolved.extra_carriers, 0);
    }

    #[test]
    fn post_resolve_legacy() {
        let e = EntityData::new(1, "E".into(), vec!["Transform".into()]);
        let resolved = resolve_post(&[e]);
        assert_eq!(resolved.exposure, 1.5);
        assert_eq!(resolved.intensity, 0.3);
        assert_eq!(resolved.vignette, 0.22);
        assert_eq!(resolved.source, PostSource::Legacy);
    }

    #[test]
    fn post_resolve_sanitizes() {
        let mut e = EntityData::new(1, "E".into(), vec!["Transform".into()]);
        e.post = Some(PostComponent {
            exposure: f32::NAN,
            intensity: -2.0,
            vignette: f32::INFINITY,
        });
        let resolved = resolve_post(&[e.clone()]);
        assert_eq!(resolved.exposure, 1.0);
        assert_eq!(resolved.intensity, 0.3);
        assert_eq!(resolved.vignette, 0.22);

        e.post = Some(PostComponent { exposure: 99.0, intensity: 0.3, vignette: 0.22 });
        let resolved = resolve_post(&[e]);
        assert_eq!(resolved.exposure, 8.0);
    }

    #[test]
    fn post_serde_roundtrip() {
        let json = r#"{"exposure": 2.0}"#;
        let parsed: PostComponent = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.exposure, 2.0);
        assert_eq!(parsed.intensity, 0.3);
        assert_eq!(parsed.vignette, 0.22);
    }
}

#[cfg(test)]
mod migrate_ambient_tests {


    use super::*;

    fn entity_with_light(id: u64, name: &str) -> EntityData {
        let mut e = EntityData::new(id, name.into(), vec!["Transform".into(), "Light".into()]);
        e.light = Some(LightData::default());
        e
    }

    #[test]
    fn migrate_adds_carrier_with_sun() {
        let mut entities = vec![
            entity_with_light(1, "Sun"),
            EntityData::new(2, "Other".into(), vec!["Transform".into()]),
        ];
        let result = migrate_scene_ambient(&mut entities, 3);
        assert_eq!(result, Some(LEGACY_AMBIENT_INTENSITY_LIT));
        assert_eq!(entities.len(), 3);
        let last = entities.last().unwrap();
        assert_eq!(last.name, "Ambiente");
        let amb = last.ambient.as_ref().unwrap();
        assert_eq!(amb.sky, LEGACY_AMBIENT_SKY);
        assert_eq!(amb.equator, LEGACY_AMBIENT_EQUATOR);
        assert_eq!(amb.ground, LEGACY_AMBIENT_GROUND);
        assert_eq!(amb.intensity, LEGACY_AMBIENT_INTENSITY_LIT);
    }

    #[test]
    fn migrate_adds_carrier_without_sun() {
        let mut entities = vec![EntityData::new(1, "Other".into(), vec!["Transform".into()])];
        let result = migrate_scene_ambient(&mut entities, 2);
        assert_eq!(result, Some(LEGACY_AMBIENT_INTENSITY_UNLIT));
        assert_eq!(
            entities.last().unwrap().ambient.as_ref().unwrap().intensity,
            LEGACY_AMBIENT_INTENSITY_UNLIT
        );
    }

    #[test]
    fn migrate_is_idempotent() {
        let mut entities = vec![entity_with_light(1, "Sun")];
        assert_eq!(
            migrate_scene_ambient(&mut entities, 2),
            Some(LEGACY_AMBIENT_INTENSITY_LIT)
        );
        let len_after_first = entities.len();
        assert_eq!(migrate_scene_ambient(&mut entities, 3), None);
        assert_eq!(entities.len(), len_after_first);
    }

    #[test]
    fn migrate_preserves_other_entities() {
        // EntityData no deriva PartialEq: se compara por su JSON.
        let original = vec![
            entity_with_light(1, "Sun"),
            EntityData::new(2, "Other".into(), vec!["Transform".into()]),
        ];
        let mut entities = original.clone();
        migrate_scene_ambient(&mut entities, 3);
        assert_eq!(
            serde_json::to_value(&entities[0]).unwrap(),
            serde_json::to_value(&original[0]).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&entities[1]).unwrap(),
            serde_json::to_value(&original[1]).unwrap()
        );
    }

    /// Verificación G1 de los ficheros reales: cada escena del
    /// directorio (GLASS_MIGRATE_DIR o ../../assets/scenes desde el
    /// crate) carga con el cargador del motor y resuelve su ambiente
    /// como Explicit con exactamente un portador. La migración en sí
    /// se hizo editando el JSON en su formato de lista (el guardador
    /// del motor escribe otro formato envuelto; ver el registro G1).
    /// `cargo test -p glass_core -- --ignored verify_scene_files_ambient`
    #[test]
    #[ignore]
    fn verify_scene_files_ambient() {
        let dir = std::env::var("GLASS_MIGRATE_DIR")
            .unwrap_or_else(|_| "../../assets/scenes".to_string());
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("no se pudo leer {dir}: {e}"))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().ends_with(".scene.json"))
                    .unwrap_or(false)
            })
            .collect();
        paths.sort();
        assert!(!paths.is_empty(), "sin escenas en {dir}");
        for path in &paths {
            let mut state = SceneState::new();
            state
                .load_from_json(path.to_str().unwrap())
                .unwrap_or_else(|e| panic!("carga {}: {e}", path.display()));
            let amb = state.ambient_settings();
            assert_eq!(
                amb.source,
                AmbientSource::Explicit,
                "{} sin portador explícito",
                path.display()
            );
            assert_eq!(amb.extra_carriers, 0, "{} con portadores extra", path.display());
            assert_eq!(amb.sky, LEGACY_AMBIENT_SKY);
            assert_eq!(amb.equator, LEGACY_AMBIENT_EQUATOR);
            assert_eq!(amb.ground, LEGACY_AMBIENT_GROUND);
        }
        println!("verificadas {} escenas", paths.len());
    }
}
