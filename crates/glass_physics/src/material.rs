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

/// Aplica `m` a un collider ya insertado en el `ColliderSet`, sin reconstruirlo.
///
/// Mismo mapeo que `apply_material`: friction = dynamic_friction,
/// restitution = bounciness y ambas reglas de combinación. El handle y los
/// contactos del collider se conservan; el cambio surte efecto en el
/// siguiente step.
pub fn aplicar_material_en_sitio(collider: &mut rapier3d::geometry::Collider, m: &PhysicsMaterial) {
    collider.set_friction(m.dynamic_friction);
    collider.set_restitution(m.bounciness);
    collider.set_friction_combine_rule(combine_rule(&m.friction_combine));
    collider.set_restitution_combine_rule(combine_rule(&m.bounce_combine));
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

    /// Sustituye el material guardado para `referencia` y devuelve su id.
    ///
    /// Si la referencia no está registrada devuelve `None` y no la registra.
    /// El id se conserva, así que los colliders que ya lo tienen asignado
    /// no cambian de id.
    pub fn actualizar(&mut self, referencia: &str, material: &PhysicsMaterial) -> Option<MaterialId> {
        let (id, guardado) = self.entries.get_mut(referencia)?;
        *guardado = *material;
        Some(*id)
    }

    pub fn material(&self, id: MaterialId) -> Option<&PhysicsMaterial> {
        self.entries
            .values()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, material)| material)
    }
}

impl MaterialTable {
    /// Prefijo de las claves sintéticas de instancia. `#` no aparece en rutas de asset,
    /// así que una instancia nunca colisiona con `registrar(referencia)`.
    const PREFIJO_INSTANCIA: &'static str = "#instancia/";

    /// Registra un material instancia (sin archivo). Comparte `next_id` con los assets,
    /// así que su id nunca coincide con el de un asset. Devuelve `MaterialId(0)` si los ids
    /// se agotaran (no se hace panic: se llama desde la frontera FFI).
    pub fn registrar_instancia(&mut self, material: PhysicsMaterial) -> MaterialId {
        let Some(siguiente) = self.next_id.checked_add(1) else {
            return MaterialId(0);
        };
        let id = MaterialId(self.next_id);
        self.next_id = siguiente;
        self.entries
            .insert(format!("{}{}", Self::PREFIJO_INSTANCIA, id.0), (id, material));
        id
    }

    /// Material guardado con ese id (asset o instancia). `MaterialId(0)` = sin referencia → None.
    /// Búsqueda lineal: la tabla tiene decenas de entradas y se consulta desde scripts, no por paso.
    pub fn por_id(&self, id: MaterialId) -> Option<PhysicsMaterial> {
        if id.0 == 0 {
            return None;
        }
        self.entries
            .values()
            .find(|(i, _)| *i == id)
            .map(|(_, m)| *m)
    }

    /// Sustituye el material guardado con ese id (asset o instancia), conservando el id.
    pub fn actualizar_por_id(&mut self, id: MaterialId, material: PhysicsMaterial) -> bool {
        if id.0 == 0 {
            return false;
        }
        match self.entries.values_mut().find(|(i, _)| *i == id) {
            Some(entrada) => {
                entrada.1 = material;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests_instancias {
    use super::*;
    use glass_core::physics_material::PhysicsMaterial;

    #[test]
    fn instancia_no_colisiona_con_assets_y_se_resuelve_por_id() {
        let mut t = MaterialTable::default();
        let a = t.registrar("assets/hielo.physmat", &PhysicsMaterial::PROJECT_DEFAULT);
        let i = t.registrar_instancia(PhysicsMaterial::PROJECT_DEFAULT);
        let b = t.registrar("assets/goma.physmat", &PhysicsMaterial::PROJECT_DEFAULT);
        assert!(a.0 > 0 && i.0 > 0 && b.0 > 0);
        assert!(a != i && i != b && a != b);
        assert!(t.por_id(i).is_some());
        assert!(t.por_id(MaterialId(0)).is_none());
    }

    #[test]
    fn actualizar_por_id_solo_toca_esa_entrada() {
        let mut t = MaterialTable::default();
        let a = t.registrar("assets/hielo.physmat", &PhysicsMaterial::PROJECT_DEFAULT);
        let i = t.registrar_instancia(PhysicsMaterial::PROJECT_DEFAULT);
        let mut m = PhysicsMaterial::PROJECT_DEFAULT;
        m.bounciness = 0.9;
        assert!(t.actualizar_por_id(i, m));
        assert_eq!(t.por_id(i).unwrap().bounciness, 0.9);
        assert_eq!(t.por_id(a).unwrap().bounciness, PhysicsMaterial::PROJECT_DEFAULT.bounciness);
        assert!(!t.actualizar_por_id(MaterialId(9999), m));
    }
}
