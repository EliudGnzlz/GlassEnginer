//! Conversión de materiales físicos de Unity (`.physicMaterial`,
//! ClassID 134) al `PhysicsMaterial` del motor.

use glass_core::physics_material::{MaterialCombine, PhysicsMaterial};
use serde_yaml::Value;

use crate::materiales::{documento_raiz, entero, numero};
use crate::RipperError;

/// Convierte el texto UnityYAML de un `.physicMaterial`
/// (ClassID 134) al `PhysicsMaterial` del motor.
///
/// Los campos ausentes toman el valor de `PhysicsMaterial::NEW_ASSET`.
/// `bounciness` se limita a [0, 1] y las combinaciones fuera de 0..=3
/// pasan a Average; ambos casos generan aviso.
pub fn convertir_material_fisico(
    ruta: &str,
    texto: &str,
) -> Result<(PhysicsMaterial, Vec<String>), RipperError> {
    let (documento, mut avisos) = documento_raiz(ruta, texto, "PhysicMaterial")?;
    let mut material = PhysicsMaterial::NEW_ASSET;

    if let Some(x) = leer_numero(&documento, "dynamicFriction", &mut avisos) {
        material.dynamic_friction = x;
    }
    if let Some(x) = leer_numero(&documento, "staticFriction", &mut avisos) {
        material.static_friction = x;
    }
    if let Some(x) = leer_numero(&documento, "bounciness", &mut avisos) {
        if !(0.0..=1.0).contains(&x) {
            avisos.push(format!("bounciness {x} fuera de [0, 1]: se limita"));
        }
        material.bounciness = x.clamp(0.0, 1.0);
    }
    if let Some(valor) = documento.get("frictionCombine") {
        material.friction_combine = combinacion("frictionCombine", valor, &mut avisos);
    }
    if let Some(valor) = documento.get("bounceCombine") {
        material.bounce_combine = combinacion("bounceCombine", valor, &mut avisos);
    }

    Ok((material, avisos))
}

/// Lee un campo numérico; si está pero no es un número, avisa y
/// devuelve None (se conserva el valor por defecto).
fn leer_numero(documento: &Value, campo: &str, avisos: &mut Vec<String>) -> Option<f32> {
    let valor = documento.get(campo)?;
    let x = numero(valor);
    if x.is_none() {
        avisos.push(format!(
            "{campo} no es numérico: se conserva el valor por defecto"
        ));
    }
    x
}

/// `PhysicMaterialCombine` de Unity por significado:
/// 0 Average, 1 Minimum, 2 Multiply, 3 Maximum.
fn combinacion(campo: &str, valor: &Value, avisos: &mut Vec<String>) -> MaterialCombine {
    match entero(valor) {
        Some(0) => MaterialCombine::Average,
        Some(1) => MaterialCombine::Minimum,
        Some(2) => MaterialCombine::Multiply,
        Some(3) => MaterialCombine::Maximum,
        Some(n) => {
            avisos.push(format!(
                "{campo} con valor fuera de 0..=3 ({n}): se usa Average"
            ));
            MaterialCombine::Average
        }
        None => {
            avisos.push(format!("{campo} no es un entero: se usa Average"));
            MaterialCombine::Average
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exito<T>(r: Result<T, RipperError>) -> T {
        match r {
            Ok(v) => v,
            Err(RipperError::Conversion { motivo, .. }) => panic!("conversión fallida: {motivo}"),
            Err(_) => panic!("error inesperado"),
        }
    }

    fn cerca(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn ejemplo_del_contrato() {
        let texto = r#"%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!134 &13400000
PhysicMaterial:
  m_ObjectHideFlags: 0
  m_Name: Hielo
  dynamicFriction: 0.1
  staticFriction: 0.1
  bounciness: 0.8
  frictionCombine: 2
  bounceCombine: 3
"#;
        let (m, avisos) = exito(convertir_material_fisico("Hielo.physicMaterial", texto));
        assert!(cerca(m.dynamic_friction, 0.1));
        assert!(cerca(m.static_friction, 0.1));
        assert!(cerca(m.bounciness, 0.8));
        assert!(matches!(m.friction_combine, MaterialCombine::Multiply));
        assert!(matches!(m.bounce_combine, MaterialCombine::Maximum));
        assert!(avisos.is_empty(), "{avisos:?}");
    }

    #[test]
    fn combinacion_fuera_de_rango_es_average_con_aviso() {
        let texto = "PhysicMaterial:\n  frictionCombine: 7\n  bounceCombine: 1\n";
        let (m, avisos) = exito(convertir_material_fisico("X.physicMaterial", texto));
        assert!(matches!(m.friction_combine, MaterialCombine::Average));
        assert!(matches!(m.bounce_combine, MaterialCombine::Minimum));
        assert!(avisos.iter().any(|a| a.contains("frictionCombine")));
    }

    #[test]
    fn bounciness_se_limita_con_aviso() {
        let texto = "PhysicMaterial:\n  bounciness: 1.5\n";
        let (m, avisos) = exito(convertir_material_fisico("X.physicMaterial", texto));
        assert!(cerca(m.bounciness, 1.0));
        assert!(avisos.iter().any(|a| a.contains("bounciness")));
    }

    #[test]
    fn documento_sin_la_clave_es_error() {
        let texto = "Material:\n  m_Name: X\n";
        match convertir_material_fisico("X.physicMaterial", texto) {
            Err(RipperError::Conversion { motivo, .. }) => {
                assert!(motivo.contains("no se encontró el documento PhysicMaterial"));
            }
            _ => panic!("se esperaba error de conversión"),
        }
    }
}
