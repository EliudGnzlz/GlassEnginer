//! Conversión de la física de una entidad de escena Unity (Rigidbody,
//! colliders y CharacterController) a los tipos de `glass_core`.
//!
//! Este paquete NO conoce el GameObject: la capa de la entidad la fija
//! `escenas` en `PhysicsData::layer` a partir del `m_Layer`; aquí solo
//! se traduce lo estrictamente físico.

use glass_core::scene::{CharacterData, ColliderSpec, MeshColliderMode, PhysicsData};
use serde_yaml::Value;

use crate::documentos::{booleano, campo, entero_de, flotante, referencia, vec3};
use crate::ejes::convertir_posicion;

/// Resultado de convertir la física de una entidad de escena.
pub struct FisicaEntidad {
    /// Física del cuerpo (Rigidbody y/o colliders). `None` si la
    /// entidad no tiene ningún documento físico relevante.
    pub physics: Option<PhysicsData>,
    /// Controlador de personaje. `None` si no hay CharacterController.
    pub character: Option<CharacterData>,
    /// Avisos emitidos durante la conversión (sin agregar).
    pub avisos: Vec<String>,
}

/// Devuelve el vector bajo `clave` si el documento lo trae como `{x,y,z}`.
fn vector(v: &Value, clave: &str) -> Option<[f32; 3]> {
    campo(v, clave).and_then(vec3)
}

/// Lee un booleano Unity con valor por defecto cuando la clave falta.
fn booleano_con_defecto(v: &Value, clave: &str, defecto: bool) -> bool {
    if campo(v, clave).is_some() {
        booleano(v, clave)
    } else {
        defecto
    }
}

/// Traduce la máscara de congelaciones de Unity a los bits del motor.
///
/// FreezePosition X/Y/Z (2/4/8) → 1/2/4 y FreezeRotation X/Y/Z
/// (16/32/64) → 8/16/32.
fn constraints_de_unity(mascara: i64) -> u8 {
    let pares: [(i64, u8); 6] = [(2, 1), (4, 2), (8, 4), (16, 8), (32, 16), (64, 32)];
    let mut salida: u8 = 0;
    for (unity, motor) in pares {
        if mascara & unity != 0 {
            salida |= motor;
        }
    }
    salida
}

/// Resuelve el material físico de un collider.
///
/// Si la referencia trae guid y el resolutor no lo conoce, se anota un
/// aviso y el collider queda sin material. Si el collider no trae
/// material no se emite nada (la agregación es cosa de `escenas`).
fn material_de(
    valor: &Value,
    resolver_material: &dyn Fn(Option<&str>) -> Option<String>,
    avisos: &mut Vec<String>,
) -> String {
    let Some(referencia_material) = campo(valor, "m_Material").and_then(referencia) else {
        return String::new();
    };
    let Some(guid) = referencia_material.guid else {
        return String::new();
    };
    match resolver_material(Some(&guid)) {
        Some(ruta) => ruta,
        None => {
            avisos.push(format!(
                "material físico no importado (guid {guid}): el collider queda sin material"
            ));
            String::new()
        }
    }
}

/// Convierte la física de UNA entidad de escena.
///
/// `componentes` son los documentos (clase, valor) de la entidad con
/// clase en {54, 64, 65, 135, 136, 143}. `resolver_material` mapea el
/// guid de un `m_Material` a la ruta importada del `.physmat.json`
/// (relativa a `Assets`) o `None` si no está importado.
pub fn fisica_de_entidad(
    componentes: &[(i32, &Value)],
    resolver_material: &dyn Fn(Option<&str>) -> Option<String>,
) -> FisicaEntidad {
    let mut avisos: Vec<String> = Vec::new();
    let mut physics = PhysicsData::default();
    let mut tiene_rigidbody = false;
    let mut tiene_collider = false;
    let mut colliders: Vec<ColliderSpec> = Vec::new();
    let mut primer_box = true;
    let mut mesh_collider = MeshColliderMode::None;
    let mut mesh_collider_material = String::new();
    let mut mesh_collider_is_trigger = false;
    let mut character: Option<CharacterData> = None;

    for (clase, valor) in componentes.iter().copied() {
        match clase {
            // Rigidbody
            54 => {
                tiene_rigidbody = true;
                physics.has_rigidbody = true;
                physics.body_type = if booleano(valor, "m_IsKinematic") {
                    "Kinematic".to_string()
                } else {
                    "Dynamic".to_string()
                };
                physics.mass = flotante(valor, "m_Mass").unwrap_or(1.0);
                // Un cuerpo de Unity sin drag no debe heredar el freno
                // por defecto del motor (1.5): el defecto aquí es 0.
                physics.linear_damping = flotante(valor, "m_Drag").unwrap_or(0.0);
                if let Some(angular) = flotante(valor, "m_AngularDrag") {
                    physics.angular_damping = angular;
                }
                physics.use_gravity = booleano_con_defecto(valor, "m_UseGravity", true);
                physics.interpolate = entero_de(valor, "m_Interpolation").unwrap_or(0) != 0;
                physics.ccd = entero_de(valor, "m_CollisionDetection").unwrap_or(0) >= 1;
                physics.constraints =
                    constraints_de_unity(entero_de(valor, "m_Constraints").unwrap_or(0));
            }
            // MeshCollider
            64 => {
                tiene_collider = true;
                mesh_collider = if booleano(valor, "m_Convex") {
                    MeshColliderMode::Convex
                } else {
                    MeshColliderMode::TriMesh
                };
                mesh_collider_is_trigger = booleano(valor, "m_IsTrigger");
                mesh_collider_material =
                    material_de(valor, resolver_material, &mut avisos);
            }
            // BoxCollider
            65 => {
                tiene_collider = true;
                let tamano = vector(valor, "m_Size").unwrap_or([1.0, 1.0, 1.0]);
                let semi = [tamano[0] * 0.5, tamano[1] * 0.5, tamano[2] * 0.5];
                let centro = vector(valor, "m_Center")
                    .map(convertir_posicion)
                    .unwrap_or([0.0, 0.0, 0.0]);
                if primer_box {
                    physics.box_extents = semi;
                    physics.box_offset = centro;
                    primer_box = false;
                }
                colliders.push(ColliderSpec {
                    shape: "Box".to_string(),
                    size: semi,
                    offset: centro,
                    is_trigger: booleano(valor, "m_IsTrigger"),
                    material: material_de(valor, resolver_material, &mut avisos),
                    direction: "Y".to_string(),
                });
            }
            // SphereCollider
            135 => {
                tiene_collider = true;
                let radio = flotante(valor, "m_Radius").unwrap_or(0.5);
                let centro = vector(valor, "m_Center")
                    .map(convertir_posicion)
                    .unwrap_or([0.0, 0.0, 0.0]);
                colliders.push(ColliderSpec {
                    shape: "Sphere".to_string(),
                    size: [radio, 0.0, 0.0],
                    offset: centro,
                    is_trigger: booleano(valor, "m_IsTrigger"),
                    material: material_de(valor, resolver_material, &mut avisos),
                    direction: "Y".to_string(),
                });
            }
            // CapsuleCollider
            136 => {
                tiene_collider = true;
                let radio = flotante(valor, "m_Radius").unwrap_or(0.5);
                let altura = flotante(valor, "m_Height").unwrap_or(2.0);
                let medio = ((altura - 2.0 * radio) / 2.0).max(0.0);
                let direction = match entero_de(valor, "m_Direction").unwrap_or(1) {
                    0 => "X",
                    2 => "Z",
                    _ => "Y",
                };
                let centro = vector(valor, "m_Center")
                    .map(convertir_posicion)
                    .unwrap_or([0.0, 0.0, 0.0]);
                colliders.push(ColliderSpec {
                    shape: "Capsule".to_string(),
                    size: [medio, radio, 0.0],
                    offset: centro,
                    is_trigger: booleano(valor, "m_IsTrigger"),
                    material: material_de(valor, resolver_material, &mut avisos),
                    direction: direction.to_string(),
                });
            }
            // CharacterController
            143 => {
                let defecto = CharacterData::default();
                character = Some(CharacterData {
                    enabled: true,
                    radius: flotante(valor, "m_Radius").unwrap_or(defecto.radius),
                    height: flotante(valor, "m_Height").unwrap_or(defecto.height),
                    slope_limit_deg: flotante(valor, "m_SlopeLimit")
                        .unwrap_or(defecto.slope_limit_deg),
                    step_offset: flotante(valor, "m_StepOffset")
                        .unwrap_or(defecto.step_offset),
                    skin_width: flotante(valor, "m_SkinWidth").unwrap_or(defecto.skin_width),
                    min_move_distance: flotante(valor, "m_MinMoveDistance")
                        .unwrap_or(defecto.min_move_distance),
                    center: vector(valor, "m_Center")
                        .map(convertir_posicion)
                        .unwrap_or([0.0, 0.0, 0.0]),
                    detect_collisions: true,
                    snap: defecto.snap,
                });
            }
            _ => {}
        }
    }

    let mut physics_final: Option<PhysicsData> = None;
    if tiene_rigidbody || tiene_collider {
        physics.has_rigidbody = tiene_rigidbody;
        physics.has_collider = tiene_collider;
        if !tiene_rigidbody {
            // Collider solo = cuerpo estático en el motor.
            physics.body_type = "Static".to_string();
        }
        physics.is_trigger = match colliders.as_slice() {
            [unico] => unico.is_trigger,
            _ => false,
        };
        physics.colliders = colliders;
        physics.mesh_collider = mesh_collider;
        physics.mesh_collider_material = mesh_collider_material;
        physics.mesh_collider_is_trigger = mesh_collider_is_trigger;
        physics_final = Some(physics);
    }

    FisicaEntidad {
        physics: physics_final,
        character,
        avisos,
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    const GUID_MATERIAL: &str = "materialfisico01";

    fn yaml(texto: &str) -> Value {
        serde_yaml::from_str(texto).expect("yaml de prueba válido")
    }

    fn resolver_con_material(guid: Option<&str>) -> Option<String> {
        match guid {
            Some(g) if g == GUID_MATERIAL => Some("Materiales/Hormigon.physmat.json".to_string()),
            _ => None,
        }
    }

    #[test]
    fn rigidbody_con_box_conserva_drag_y_constraints() {
        let rigidbody = yaml(
            r#"
m_IsKinematic: 0
m_Mass: 2.0
m_Drag: 0.1
m_AngularDrag: 0.05
m_UseGravity: 1
m_Constraints: 66
m_Interpolation: 0
m_CollisionDetection: 0
"#,
        );
        let caja = yaml(
            r#"
m_Size: {x: 2.0, y: 1.0, z: 2.0}
m_Center: {x: 0.0, y: 0.5, z: 0.0}
m_IsTrigger: 0
m_Material: {fileID: 2100000, guid: materialfisico01, type: 2}
"#,
        );

        let componentes: Vec<(i32, &Value)> = vec![(54, &rigidbody), (65, &caja)];
        let fisica = fisica_de_entidad(&componentes, &resolver_con_material);

        let physics = fisica.physics.as_ref().expect("física presente");
        assert_eq!(physics.body_type, "Dynamic");
        assert!(physics.has_rigidbody);
        assert!(physics.has_collider);
        assert_eq!(physics.mass, 2.0);
        assert_eq!(physics.linear_damping, 0.1);
        assert_eq!(physics.constraints, 1 | 32);
        assert_eq!(physics.colliders.len(), 1);
        let spec = &physics.colliders[0];
        assert_eq!(spec.shape, "Box");
        assert_eq!(spec.size, [1.0, 0.5, 1.0]);
        assert_eq!(spec.material, "Materiales/Hormigon.physmat.json");
        assert_eq!(physics.box_extents, [1.0, 0.5, 1.0]);
        assert_eq!(physics.box_offset, [0.0, 0.5, 0.0]);
        assert!(fisica.avisos.is_empty());
    }

    #[test]
    fn solo_sphere_sin_rigidbody_es_estatico() {
        let esfera = yaml(
            r#"
m_Radius: 0.75
m_Center: {x: 0.0, y: 0.0, z: 0.0}
m_IsTrigger: 0
m_Material: {fileID: 0}
"#,
        );

        let componentes: Vec<(i32, &Value)> = vec![(135, &esfera)];
        let fisica = fisica_de_entidad(&componentes, &resolver_con_material);

        let physics = fisica.physics.as_ref().expect("física presente");
        assert!(!physics.has_rigidbody);
        assert!(physics.has_collider);
        assert_eq!(physics.body_type, "Static");
        assert_eq!(physics.colliders.len(), 1);
        assert_eq!(physics.colliders[0].shape, "Sphere");
        assert_eq!(physics.colliders[0].size, [0.75, 0.0, 0.0]);
        assert!(fisica.avisos.is_empty());
    }

    #[test]
    fn capsula_direccion_y_media_altura() {
        let capsula = yaml(
            r#"
m_Radius: 0.5
m_Height: 2.0
m_Direction: 0
m_Center: {x: 0.0, y: 0.0, z: 0.0}
m_IsTrigger: 0
m_Material: {fileID: 0}
"#,
        );

        let componentes: Vec<(i32, &Value)> = vec![(136, &capsula)];
        let fisica = fisica_de_entidad(&componentes, &resolver_con_material);

        let physics = fisica.physics.as_ref().expect("física presente");
        let spec = &physics.colliders[0];
        assert_eq!(spec.shape, "Capsule");
        assert_eq!(spec.direction, "X");
        assert_eq!(spec.size, [0.5, 0.5, 0.0]);
    }

    #[test]
    fn mesh_collider_no_convexo_con_trigger_y_material_sin_resolver() {
        let malla = yaml(
            r#"
m_Convex: 0
m_IsTrigger: 1
m_Material: {fileID: 13400000, guid: materialdesconocido, type: 2}
"#,
        );

        let componentes: Vec<(i32, &Value)> = vec![(64, &malla)];
        let fisica = fisica_de_entidad(&componentes, &resolver_con_material);

        let physics = fisica.physics.as_ref().expect("física presente");
        assert!(matches!(physics.mesh_collider, MeshColliderMode::TriMesh));
        assert!(physics.mesh_collider_is_trigger);
        assert!(physics.colliders.is_empty());
        assert!(!physics.is_trigger);
        assert!(fisica
            .avisos
            .iter()
            .any(|aviso| aviso.contains("material físico no importado")));
    }

    #[test]
    fn character_controller_completo_sin_physics() {
        let caracter = yaml(
            r#"
m_Radius: 0.4
m_Height: 1.8
m_SlopeLimit: 50.0
m_StepOffset: 0.4
m_SkinWidth: 0.06
m_MinMoveDistance: 0.002
m_Center: {x: 0.0, y: 0.9, z: 0.0}
"#,
        );

        let componentes: Vec<(i32, &Value)> = vec![(143, &caracter)];
        let fisica = fisica_de_entidad(&componentes, &resolver_con_material);

        assert!(fisica.physics.is_none());
        let personaje = fisica.character.as_ref().expect("personaje presente");
        assert!(personaje.enabled);
        assert_eq!(personaje.radius, 0.4);
        assert_eq!(personaje.height, 1.8);
        assert_eq!(personaje.slope_limit_deg, 50.0);
        assert_eq!(personaje.step_offset, 0.4);
        assert_eq!(personaje.skin_width, 0.06);
        assert_eq!(personaje.min_move_distance, 0.002);
        assert_eq!(personaje.center, [0.0, 0.9, 0.0]);
        assert!(personaje.detect_collisions);
        assert!(fisica.avisos.is_empty());
    }

    #[test]
    fn sin_documentos_no_hay_fisica() {
        let componentes: Vec<(i32, &Value)> = Vec::new();
        let fisica = fisica_de_entidad(&componentes, &resolver_con_material);

        assert!(fisica.physics.is_none());
        assert!(fisica.character.is_none());
        assert!(fisica.avisos.is_empty());
    }
}
