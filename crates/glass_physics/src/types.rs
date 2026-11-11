use serde::{Deserialize, Serialize};

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyType {
    Dynamic = 0,
    Static = 1,
    Kinematic = 2,
}

impl BodyType {
    pub fn from_i32(val: i32) -> Self {
        match val {
            0 => Self::Dynamic,
            1 => Self::Static,
            _ => Self::Kinematic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ColliderShape {
    Cuboid { half_extents: [f32; 3] },
    Ball { radius: f32 },
    Capsule { half_height: f32, radius: f32 },
    /// Cilindro de Rapier (`ColliderBuilder::cylinder`): `half_height`
    /// es la mitad de la altura total, como en la cápsula.
    Cylinder { half_height: f32, radius: f32 },
}

/// Deriva el collider de una entidad a partir del nombre de su malla.
///
/// SOLO los tres nombres integrados nuevos (`"sphere"`, `"capsule"`,
/// `"cylinder"`) producen una forma no-caja, a partir de la escala
/// MUNDIAL de la entidad y de las dimensiones canónicas de
/// `glass_core::primitives` (la misma fuente que usan los generadores
/// de malla del renderer: si la malla cambia, el collider la sigue).
/// Cualquier otro nombre —`"cube"`, `"plane"`, `""` (sin modelo: el
/// render cae al cubo), nombres de .glb— devuelve la caja de siempre
/// con `box_extents` tal cual: es la regla de compatibilidad que hace
/// que todo el contenido existente reciba exactamente el collider que
/// recibía antes de existir esta función (ninguna escena del proyecto
/// usa aún los tres nombres nuevos, verificado por grep).
///
/// Decisiones y límites (conscientes, revisados en diseño):
/// - La escala sale de `mat4_scale` sobre la matriz de mundo: con
///   jerarquías que combinen rotación y escala no uniforme del padre
///   la extracción es «best effort» (no hay descomposición polar).
/// - Con escala no uniforme, una esfera se aproxima por su eje MAYOR
///   y la cápsula/cilindro toman el radio del eje XZ mayor: el
///   collider puede sobresalir por el lado menor. Rapier no tiene
///   elipsoides; es la aproximación estándar.
/// - Guarda numérica: si la forma derivada no es finita o su radio
///   (o la altura del cilindro) no es > 0, se cae a la caja. La guarda
///   ataja fallos numéricos (escala 0, NaN), no formas «válidas pero
///   inadecuadas» (un elipsoide visual seguirá teniendo esfera).
///
/// DEUDA TÉCNICA DECLARADA: la forma se infiere del nombre de la
/// malla VISUAL porque es la única vía sin migrar el contenido
/// existente. Lo limpio a largo plazo es un campo de forma explícito
/// en `PhysicsData` (como Unity separa MeshFilter de Collider); esta
/// inferencia es el paso intermedio compatible, no la arquitectura
/// final.
pub fn collider_shape_for_builtin(
    model: &str,
    world_scale: [f32; 3],
    box_extents: [f32; 3],
) -> ColliderShape {
    use glass_core::primitives::{
        CAPSULE_CYLINDER_HALF_HEIGHT, CAPSULE_RADIUS, CYLINDER_HALF_HEIGHT, CYLINDER_RADIUS,
        SPHERE_RADIUS,
    };
    let fallback = ColliderShape::Cuboid { half_extents: box_extents };
    let sx = world_scale[0].abs();
    let sy = world_scale[1].abs();
    let sz = world_scale[2].abs();
    let radio_xz = |r_unit: f32| r_unit * sx.max(sz);
    match model {
        "sphere" => {
            let radius = SPHERE_RADIUS * sx.max(sy).max(sz);
            if radius.is_finite() && radius > 0.0 {
                ColliderShape::Ball { radius }
            } else {
                fallback
            }
        }
        "capsule" => {
            let radius = radio_xz(CAPSULE_RADIUS);
            // Altura mundial total = 2 · (tramo + radio) · |sy|; la
            // half_height de Rapier es altura/2 − radio del casquete
            // YA escalado, de ahí la resta con el radio mundial.
            let half_height =
                ((CAPSULE_CYLINDER_HALF_HEIGHT + CAPSULE_RADIUS) * sy - radius).max(0.0);
            if radius.is_finite() && radius > 0.0 && half_height.is_finite() {
                ColliderShape::Capsule { half_height, radius }
            } else {
                fallback
            }
        }
        "cylinder" => {
            let radius = radio_xz(CYLINDER_RADIUS);
            let half_height = CYLINDER_HALF_HEIGHT * sy;
            if radius.is_finite() && radius > 0.0 && half_height.is_finite() && half_height > 0.0 {
                ColliderShape::Cylinder { half_height, radius }
            } else {
                fallback
            }
        }
        _ => fallback,
    }
}

pub type BodyHandle = u64;
pub type ColliderHandle = u64;

#[cfg(test)]
mod tests {
    use super::*;

    const EXT: [f32; 3] = [0.5, 0.5, 0.5];

    #[test]
    fn inferencia_esfera_escala_unitaria_y_escalada() {
        assert_eq!(
            collider_shape_for_builtin("sphere", [1.0, 1.0, 1.0], EXT),
            ColliderShape::Ball { radius: 0.5 }
        );
        // Escala uniforme 2 → radio 1 (la malla mide 1 de diámetro).
        assert_eq!(
            collider_shape_for_builtin("sphere", [2.0, 2.0, 2.0], EXT),
            ColliderShape::Ball { radius: 1.0 }
        );
        // No uniforme: manda el eje mayor (aproximación documentada).
        assert_eq!(
            collider_shape_for_builtin("sphere", [1.0, 3.0, 1.0], EXT),
            ColliderShape::Ball { radius: 1.5 }
        );
    }

    #[test]
    fn inferencia_capsula_formula_contra_la_malla() {
        // Preset del editor: escala [1,2,1] sobre malla de alto 2 y
        // radio 0,5 → altura mundial 4, radio 0,5, half_height 1,5.
        assert_eq!(
            collider_shape_for_builtin("capsule", [1.0, 2.0, 1.0], EXT),
            ColliderShape::Capsule { half_height: 1.5, radius: 0.5 }
        );
        // Escala 1: la cápsula canónica (tramo 0,5 + radio 0,5).
        assert_eq!(
            collider_shape_for_builtin("capsule", [1.0, 1.0, 1.0], EXT),
            ColliderShape::Capsule { half_height: 0.5, radius: 0.5 }
        );
        // Muy ancha para su alto: half_height se satura a 0 (cápsula =
        // esfera de Rapier), nunca negativa.
        match collider_shape_for_builtin("capsule", [4.0, 0.5, 4.0], EXT) {
            ColliderShape::Capsule { half_height, radius } => {
                assert_eq!(half_height, 0.0);
                assert_eq!(radius, 2.0);
            }
            otro => panic!("se esperaba Capsule, llegó {:?}", otro),
        }
    }

    #[test]
    fn inferencia_cilindro_formula_contra_la_malla() {
        assert_eq!(
            collider_shape_for_builtin("cylinder", [1.0, 1.0, 1.0], EXT),
            ColliderShape::Cylinder { half_height: 1.0, radius: 0.5 }
        );
        // Preset del editor: escala [1,2,1] → cilindro de 4 de alto.
        assert_eq!(
            collider_shape_for_builtin("cylinder", [1.0, 2.0, 1.0], EXT),
            ColliderShape::Cylinder { half_height: 2.0, radius: 0.5 }
        );
    }

    #[test]
    fn inferencia_todo_lo_demas_es_la_caja_de_siempre() {
        for nombre in ["", "cube", "plane", "serpiente", "mi_modelo", "Sphere"] {
            assert_eq!(
                collider_shape_for_builtin(nombre, [3.0, 2.0, 1.0], [1.3, 0.25, 1.5]),
                ColliderShape::Cuboid { half_extents: [1.3, 0.25, 1.5] },
                "el nombre {:?} no debía inferir forma",
                nombre
            );
        }
    }

    #[test]
    fn inferencia_guarda_numerica_cae_a_caja() {
        // Escala cero: sin radio no hay forma; caja (comportamiento
        // degenerado anterior, no un pánico de Rapier).
        assert_eq!(
            collider_shape_for_builtin("sphere", [0.0, 0.0, 0.0], EXT),
            ColliderShape::Cuboid { half_extents: EXT }
        );
        assert_eq!(
            collider_shape_for_builtin("cylinder", [1.0, 0.0, 1.0], EXT),
            ColliderShape::Cuboid { half_extents: EXT }
        );
        // NaN en toda la escala (matriz degenerada): la guarda lo
        // ataja. (Un solo eje NaN con los demás válidos produce una
        // forma finita por el `max` de Rust, que trata NaN como
        // ausente: el collider resultante es finito y no envenena
        // Rapier, que es lo que la guarda garantiza.)
        assert_eq!(
            collider_shape_for_builtin("capsule", [f32::NAN, f32::NAN, f32::NAN], EXT),
            ColliderShape::Cuboid { half_extents: EXT }
        );
        assert_eq!(
            collider_shape_for_builtin("sphere", [f32::INFINITY, 1.0, 1.0], EXT),
            ColliderShape::Cuboid { half_extents: EXT }
        );
        // Escala espejo (negativa): el valor absoluto la neutraliza.
        assert_eq!(
            collider_shape_for_builtin("sphere", [-2.0, 2.0, 2.0], EXT),
            ColliderShape::Ball { radius: 1.0 }
        );
    }
}

/// Configuración de grupos y sensor de un collider (Fase 1): capa propia
/// (0..31), máscara de capas con las que interactúa y si es trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColliderConfig {
    pub layer: u8,
    pub mask: u32,
    pub is_trigger: bool,
}

impl Default for ColliderConfig {
    fn default() -> Self {
        Self {
            layer: 0,
            mask: u32::MAX,
            is_trigger: false,
        }
    }
}

/// Tipo de evento de contacto/intersección entre dos entidades.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContactKind {
    /// El par entró en contacto o intersección.
    Enter,
    /// El par dejó de estar en contacto o intersección.
    Exit,
}

/// Evento de contacto entre dos entidades, generado por el mundo físico
/// desde los CollisionEvent de Rapier. El par va ordenado (a < b); Stay no
/// es un evento: se deriva comparando `contact_pairs()` entre frames.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ContactEvent {
    /// Entidad menor del par.
    pub a: u64,
    /// Entidad mayor del par.
    pub b: u64,
    pub kind: ContactKind,
    /// true si alguno de los colliders del par es sensor (trigger).
    pub is_trigger: bool,
    /// Substep del mundo en el que ocurrió.
    pub step: u64,
}
