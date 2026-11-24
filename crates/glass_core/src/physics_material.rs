//! Asset Physics Material estilo Unity, persistido como `.physmat.json`.
//!
//! Rapier aplica un único coeficiente de fricción de Coulomb: usa
//! `dynamic_friction`, mientras que `static_friction` se conserva solo para
//! mantener paridad de datos con Unity y no tiene efecto físico en esta versión.
//! [`PhysicsMaterial::PROJECT_DEFAULT`] reproduce el comportamiento histórico
//! del motor: fricción 0.7 y rebote 0.05 del cuerpo por defecto.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

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


/// De qué peldaño de la cadena salió el material efectivo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialOrigin {
    /// Del asset .physmat.json referenciado.
    Asset,
    /// Del legado del cuerpo (friction/restitution históricos).
    Legacy,
    /// Del defecto del proyecto (PROJECT_DEFAULT).
    ProjectDefault,
}

/// Indica si una referencia a un material físico es válida.
pub fn es_referencia_physmat_valida(reference: &str) -> bool {
    if reference.is_empty() || !reference.ends_with(PhysicsMaterial::SUFFIX) {
        return false;
    }

    if reference.starts_with('/') || reference.starts_with('\\') {
        return false;
    }

    let bytes = reference.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return false;
    }

    !reference
        .split(['/', '\\'])
        .any(|componente| componente == "..")
}

/// Caché perezosa de assets por referencia, con raíz en la carpeta Assets.
pub struct PhysicsMaterialLibrary {
    assets_root: PathBuf,
    cache: HashMap<String, Option<PhysicsMaterial>>,
}

impl PhysicsMaterialLibrary {
    pub fn new(assets_root: PathBuf) -> Self {
        Self {
            assets_root,
            cache: HashMap::new(),
        }
    }

    /// Devuelve el asset saneado, cacheando también los errores de carga.
    pub fn get(&mut self, reference: &str) -> Option<PhysicsMaterial> {
        if let Some(cacheado) = self.cache.get(reference) {
            return *cacheado;
        }

        if !es_referencia_physmat_valida(reference) {
            log::warn!(
                "No se pudo cargar el material físico '{reference}': referencia inválida"
            );
            self.cache.insert(reference.to_owned(), None);
            return None;
        }

        let ruta = self.assets_root.join(reference);
        let carga = std::fs::read_to_string(&ruta)
            .map_err(|error| error.to_string())
            .and_then(|texto| PhysicsMaterial::from_json(&texto))
            .map(PhysicsMaterial::sanitized);

        let material = match carga {
            Ok(material) => Some(material),
            Err(causa) => {
                log::warn!(
                    "No se pudo cargar el material físico '{reference}': {causa}"
                );
                None
            }
        };

        self.cache.insert(reference.to_owned(), material);
        material
    }
}

/// Resuelve el material efectivo de un collider siguiendo la cadena de prioridad.
pub fn resolve_collider_material(
    material_ref: &str,
    physics: &crate::scene::PhysicsData,
    lib: &mut PhysicsMaterialLibrary,
) -> (PhysicsMaterial, MaterialOrigin) {
    if !material_ref.is_empty() {
        if let Some(material) = lib.get(material_ref) {
            return (material, MaterialOrigin::Asset);
        }
    }

    if physics.legacy_friction.is_some() || physics.legacy_restitution.is_some() {
        return (
            physics.body_fallback_material(),
            MaterialOrigin::Legacy,
        );
    }

    (
        PhysicsMaterial::PROJECT_DEFAULT,
        MaterialOrigin::ProjectDefault,
    )
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

    fn directorio_temporal_physmat(nombre: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};

        static CONTADOR: AtomicU64 = AtomicU64::new(0);
        let sufijo = CONTADOR.fetch_add(1, Ordering::Relaxed);
        let directorio = std::env::temp_dir().join(format!(
            "glassenginer_{nombre}_{}_{sufijo}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directorio).unwrap();
        directorio
    }

    #[test]
    fn resolucion_material_recorre_toda_la_cadena() {
        let directorio = directorio_temporal_physmat("cadena_resolucion");
        let materiales = directorio.join("Materials");
        std::fs::create_dir_all(&materiales).unwrap();

        let asset = PhysicsMaterial {
            dynamic_friction: 0.1,
            ..PhysicsMaterial::NEW_ASSET
        };
        std::fs::write(
            materiales.join("Hielo.physmat.json"),
            asset.to_json(),
        )
        .unwrap();

        let mut lib = PhysicsMaterialLibrary::new(directorio.clone());
        let con_legado = crate::scene::PhysicsData {
            legacy_friction: Some(0.3),
            legacy_restitution: Some(0.8),
            ..Default::default()
        };

        let (material, origen) = resolve_collider_material(
            "Materials/Hielo.physmat.json",
            &con_legado,
            &mut lib,
        );
        assert_eq!(origen, MaterialOrigin::Asset);
        assert_eq!(material.dynamic_friction, 0.1);

        let (material, origen) = resolve_collider_material(
            "Materials/Inexistente.physmat.json",
            &con_legado,
            &mut lib,
        );
        assert_eq!(origen, MaterialOrigin::Legacy);
        assert_eq!(material, con_legado.body_fallback_material());
        assert_eq!(material.dynamic_friction, 0.3);
        assert_eq!(material.bounciness, 0.8);

        let sin_legado = crate::scene::PhysicsData::default();
        let (material, origen) =
            resolve_collider_material("", &sin_legado, &mut lib);
        assert_eq!(origen, MaterialOrigin::ProjectDefault);
        assert_eq!(material, PhysicsMaterial::PROJECT_DEFAULT);

        let solo_friccion = crate::scene::PhysicsData {
            legacy_friction: Some(0.3),
            ..Default::default()
        };
        let (material, origen) =
            resolve_collider_material("", &solo_friccion, &mut lib);
        assert_eq!(origen, MaterialOrigin::Legacy);
        assert_eq!(material.dynamic_friction, 0.3);
        assert_eq!(material.bounciness, 0.05);

        let _ = std::fs::remove_dir_all(directorio);
    }

    #[test]
    fn referencias_invalidas_se_rechazan_y_caen_al_legado() {
        for referencia in [
            "../fuera.physmat.json",
            "Materials/sin_sufijo.json",
            "/absoluta.physmat.json",
            "",
        ] {
            assert!(!es_referencia_physmat_valida(referencia));
        }

        let directorio = directorio_temporal_physmat("referencias_invalidas");
        let mut lib = PhysicsMaterialLibrary::new(directorio.clone());
        let physics = crate::scene::PhysicsData {
            legacy_friction: Some(0.3),
            legacy_restitution: Some(0.8),
            ..Default::default()
        };

        let (material, origen) =
            resolve_collider_material("../x.physmat.json", &physics, &mut lib);
        assert_eq!(origen, MaterialOrigin::Legacy);
        assert_eq!(material, physics.body_fallback_material());

        let _ = std::fs::remove_dir_all(directorio);
    }

    #[test]
    fn error_de_asset_queda_cacheado() {
        let directorio = directorio_temporal_physmat("error_cacheado");
        let ruta = directorio.join("Roto.physmat.json");
        std::fs::write(&ruta, "{ json corrupto").unwrap();

        let mut lib = PhysicsMaterialLibrary::new(directorio.clone());
        assert_eq!(lib.get("Roto.physmat.json"), None);

        std::fs::write(&ruta, PhysicsMaterial::NEW_ASSET.to_json()).unwrap();
        assert_eq!(lib.get("Roto.physmat.json"), None);

        let _ = std::fs::remove_dir_all(directorio);
    }

    #[test]
    fn asset_se_sanea_al_resolver() {
        let directorio = directorio_temporal_physmat("asset_saneado");
        std::fs::write(
            directorio.join("Rebote.physmat.json"),
            r#"{"bounciness": 1.7}"#,
        )
        .unwrap();

        let mut lib = PhysicsMaterialLibrary::new(directorio.clone());
        let physics = crate::scene::PhysicsData::default();
        let (material, origen) =
            resolve_collider_material("Rebote.physmat.json", &physics, &mut lib);

        assert_eq!(origen, MaterialOrigin::Asset);
        assert_eq!(material.bounciness, 1.0);

        let _ = std::fs::remove_dir_all(directorio);
    }
}
