use std::collections::HashMap;

use glass_core::physics_material::{MaterialCombine, PhysicsMaterial};
use rapier3d::na::Vector3;
use rapier3d::dynamics::CoefficientCombineRule;
use rapier3d::geometry::ColliderBuilder;

/// Aplica el material a Rapier.
///
/// Rapier solo tiene un coeficiente de fricción por collider. Por tanto,
/// `static_friction` se conserva en `PhysicsMaterial`/`MaterialTable`, pero no
/// tiene efecto físico ni se emula mediante `dynamic_friction`.
pub fn apply_material(builder: ColliderBuilder, m: &PhysicsMaterial) -> ColliderBuilder {
    builder
        .friction(m.dynamic_friction)
        .restitution(m.bounciness)
        .friction_combine_rule(combine_rule(&m.friction_combine))
        .restitution_combine_rule(combine_rule(&m.bounce_combine))
}

fn combine_rule(combine: &MaterialCombine) -> CoefficientCombineRule {
    match combine {
        MaterialCombine::Average => CoefficientCombineRule::Average,
        MaterialCombine::Minimum => CoefficientCombineRule::Min,
        MaterialCombine::Multiply => CoefficientCombineRule::Multiply,
        MaterialCombine::Maximum => CoefficientCombineRule::Max,
    }
}

/// Rotación de la cápsula como vector de rotación (eje * ángulo), que es
/// lo que pide `ColliderBuilder::rotation` en esta versión de Rapier.
/// "Y" (y cualquier valor desconocido) es el vector cero: cápsula vertical.
pub fn capsule_rotation(direction: &str) -> Vector3<f32> {
    match direction {
        "X" => Vector3::new(0.0, 0.0, -std::f32::consts::FRAC_PI_2),
        "Z" => Vector3::new(std::f32::consts::FRAC_PI_2, 0.0, 0.0),
        _ => Vector3::zeros(),
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct MaterialId(pub u32);

pub struct MaterialTable {
    entries: HashMap<String, (MaterialId, PhysicsMaterial)>,
    next_id: u32,
}

impl Default for MaterialTable {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            next_id: 1,
        }
    }
}

impl MaterialTable {
    pub fn registrar(&mut self, referencia: &str, material: &PhysicsMaterial) -> MaterialId {
        if referencia.is_empty() {
            return MaterialId(0);
        }

        if let Some((id, _)) = self.entries.get(referencia) {
            return *id;
        }

        let id = MaterialId(self.next_id);
        self.next_id += 1;
        self.entries
            .insert(referencia.to_owned(), (id, *material));
        id
    }

    pub fn material(&self, id: MaterialId) -> Option<&PhysicsMaterial> {
        self.entries
            .values()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, material)| material)
    }
}
