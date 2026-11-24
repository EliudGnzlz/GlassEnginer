//! Asset Physics Material estilo Unity, persistido como `.physmat.json`.
//!
//! Rapier aplica un único coeficiente de fricción de Coulomb: usa
//! `dynamic_friction`, mientras que `static_friction` se conserva solo para
//! mantener paridad de datos con Unity y no tiene efecto físico en esta versión.
//! [`PhysicsMaterial::PROJECT_DEFAULT`] reproduce el comportamiento histórico
//! del motor: fricción 0.7 y rebote 0.05 del cuerpo por defecto.

use serde::{Deserialize, Serialize};

/// Regla para combinar los coeficientes de dos materiales en contacto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialCombine {
    #[default]
    Average,
    Minimum,
    Multiply,
    Maximum,
}

/// Material físico serializable con los valores usados al crear un asset nuevo.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicsMaterial {
    /// Coeficiente de fricción dinámica; es el que recibe Rapier.
    pub dynamic_friction: f32,
    /// Coeficiente de fricción estática conservado por paridad con Unity.
    ///
    /// Rapier usa un único coeficiente de fricción de Coulomb, por lo que este
    /// campo se guarda, se serializa y se expone, pero no tiene efecto físico
    /// en esta versión.
    pub static_friction: f32,
    /// Coeficiente de restitución, limitado al intervalo `[0, 1]` al sanear.
    pub bounciness: f32,
    /// Regla usada para combinar la fricción de dos materiales.
    pub friction_combine: MaterialCombine,
    /// Regla usada para combinar el rebote de dos materiales.
    pub bounce_combine: MaterialCombine,
}

impl Default for PhysicsMaterial {
    fn default() -> Self {
        Self::NEW_ASSET
    }
}

impl PhysicsMaterial {
    /// Valores con los que Unity crea un Physics Material nuevo.
    pub const NEW_ASSET: PhysicsMaterial = PhysicsMaterial {
        dynamic_friction: 0.6,
        static_friction: 0.6,
        bounciness: 0.0,
        friction_combine: MaterialCombine::Average,
        bounce_combine: MaterialCombine::Average,
    };

    /// Material histórico del cuerpo por defecto del proyecto.
    pub const PROJECT_DEFAULT: PhysicsMaterial = PhysicsMaterial {
        dynamic_friction: 0.7,
        static_friction: 0.7,
        bounciness: 0.05,
        friction_combine: MaterialCombine::Average,
        bounce_combine: MaterialCombine::Average,
    };

    /// Sufijo de archivo que identifica estos assets.
    pub const SUFFIX: &'static str = ".physmat.json";

    /// Corrige coeficientes inválidos sin alterar las reglas de combinación.
    pub fn sanitized(self) -> PhysicsMaterial {
        let sanitize_friction = |value: f32| {
            if value.is_finite() && value >= 0.0 {
                value
            } else {
                0.0
            }
        };

        PhysicsMaterial {
            dynamic_friction: sanitize_friction(self.dynamic_friction),
            static_friction: sanitize_friction(self.static_friction),
            bounciness: if self.bounciness.is_finite() {
                self.bounciness.clamp(0.0, 1.0)
            } else {
                0.0
            },
            friction_combine: self.friction_combine,
            bounce_combine: self.bounce_combine,
        }
    }

    /// Parsea un asset JSON y sanea sus coeficientes.
    pub fn from_json(texto: &str) -> Result<PhysicsMaterial, String> {
        serde_json::from_str::<PhysicsMaterial>(texto)
            .map(PhysicsMaterial::sanitized)
            .map_err(|error| error.to_string())
    }

    /// Serializa el material con formato JSON legible.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self)
            .expect("PhysicsMaterial solo contiene valores serializables")
    }
}

/// Indica si un nombre de archivo tiene el sufijo de material físico.
pub fn es_physmat(nombre: &str) -> bool {
    nombre
        .to_ascii_lowercase()
        .ends_with(PhysicsMaterial::SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_json() {
        let json = PhysicsMaterial::NEW_ASSET.to_json();
        let material = PhysicsMaterial::from_json(&json).unwrap();

        assert_eq!(material, PhysicsMaterial::NEW_ASSET);
        for clave in [
            "dynamic_friction",
            "static_friction",
            "bounciness",
            "friction_combine",
            "bounce_combine",
        ] {
            assert!(json.contains(&format!("\"{clave}\"")));
        }
        assert!(json.contains("\"friction_combine\": \"Average\""));
        assert!(json.contains("\"bounce_combine\": \"Average\""));
    }

    #[test]
    fn campos_ausentes_dan_asset_nuevo() {
        assert_eq!(
            PhysicsMaterial::from_json("{}").unwrap(),
            PhysicsMaterial::NEW_ASSET
        );
        assert_eq!(
            PhysicsMaterial::from_json("{\"bounciness\": 0.5}").unwrap(),
            PhysicsMaterial {
                bounciness: 0.5,
                ..PhysicsMaterial::NEW_ASSET
            }
        );
    }

    #[test]
    fn saneado_de_rangos() {
        let material = PhysicsMaterial::from_json(
            r#"{
                "dynamic_friction": -2.0,
                "static_friction": -0.1,
                "bounciness": 1.7
            }"#,
        )
        .unwrap();
        assert_eq!(material.dynamic_friction, 0.0);
        assert_eq!(material.static_friction, 0.0);
        assert_eq!(material.bounciness, 1.0);

        assert_eq!(
            PhysicsMaterial::from_json("{\"bounciness\": -0.3}")
                .unwrap()
                .bounciness,
            0.0
        );

        let no_finitos = PhysicsMaterial {
            dynamic_friction: f32::NAN,
            static_friction: f32::INFINITY,
            bounciness: f32::NAN,
            ..PhysicsMaterial::NEW_ASSET
        }
        .sanitized();
        assert_eq!(no_finitos.dynamic_friction, 0.0);
        assert_eq!(no_finitos.static_friction, 0.0);
        assert_eq!(no_finitos.bounciness, 0.0);
    }

    #[test]
    fn combine_desconocido_es_error() {
        assert!(PhysicsMaterial::from_json("{\"friction_combine\": \"Sum\"}").is_err());
        assert!(PhysicsMaterial::from_json("{\"bounce_combine\": \"average\"}").is_err());
    }

    #[test]
    fn project_default_iguala_dorados_c0() {
        let material = PhysicsMaterial::PROJECT_DEFAULT;
        assert_eq!(material.dynamic_friction, 0.7);
        assert_eq!(material.static_friction, 0.7);
        assert_eq!(material.bounciness, 0.05);
        assert_eq!(material.friction_combine, MaterialCombine::Average);
        assert_eq!(material.bounce_combine, MaterialCombine::Average);
    }

    #[test]
    fn es_physmat_reconoce_sufijo() {
        assert!(es_physmat("Materials/Hielo.physmat.json"));
        assert!(es_physmat("Hielo.PHYSMAT.JSON"));
        assert!(!es_physmat("Hielo.json"));
        assert!(!es_physmat("Hielo.physmat.json.bak"));
    }
}
