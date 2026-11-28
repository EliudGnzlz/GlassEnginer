//! Lectura y escritura genérica de componentes por nombre.
//!
//! Cada componente se expone como JSON. La escritura fusiona un JSON
//! parcial sobre el estado actual y vuelve a deserializar al tipo.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::scene::{EntityData, PhysicsData};

/// Nombres canónicos de los componentes soportados.
const COMPONENTES: &[&str] = &[
    "Transform",
    "MeshRenderer",
    "Rigidbody",
    "Collider",
    "CharacterController",
    "Camera",
    "Light",
    "Script",
    "AudioSource",
    "Canvas",
    "RectTransform",
    "Text",
    "Button",
    "Image",
    "DragDrop",
    "Animator",
    "Fog",
    "Particles",
    "Portal",
    "LayoutGroup",
    "LayoutElement",
    "ContentSizeFitter",
    "GridLayoutGroup",
];

/// JSON completo del componente `nombre` de la entidad, Ok(None) si la
/// entidad no tiene ese componente, Err si el nombre no es un componente
/// conocido (el mensaje lista los válidos).
pub fn componente_json(ent: &EntityData, nombre: &str) -> Result<Option<Value>, String> {
    let n = resolver(nombre)?;
    match n {
        "Transform" => a_json(&ent.transform, n).map(Some),
        "MeshRenderer" => get_opt(&ent.mesh, n),
        "Rigidbody" => match &ent.physics {
            Some(p) if p.has_rigidbody => {
                let mut copia = p.clone();
                neutralizar_collider(&mut copia);
                a_json(&copia, n).map(Some)
            }
            _ => Ok(None),
        },
        "Collider" => match &ent.physics {
            Some(p) if p.has_collider => {
                let mut copia = p.clone();
                copia.has_rigidbody = false;
                a_json(&copia, n).map(Some)
            }
            _ => Ok(None),
        },
        "CharacterController" => get_opt(&ent.character, n),
        "Camera" => get_opt(&ent.camera, n),
        "Light" => get_opt(&ent.light, n),
        "Script" => {
            if ent.script.is_none() && ent.script_data.is_none() {
                Ok(None)
            } else {
                foto_script(ent).map(Some)
            }
        }
        "AudioSource" => get_opt(&ent.audio, n),
        "Canvas" => get_opt(&ent.canvas, n),
        "RectTransform" => get_opt(&ent.rect_transform, n),
        "Text" => get_opt(&ent.ui_text, n),
        "Button" => get_opt(&ent.ui_button, n),
        "Image" => get_opt(&ent.ui_image, n),
        "DragDrop" => get_opt(&ent.ui_drag, n),
        "LayoutGroup" => get_opt(&ent.layout_group, n),
        "LayoutElement" => get_opt(&ent.layout_element, n),
        "ContentSizeFitter" => get_opt(&ent.content_size_fitter, n),
        "GridLayoutGroup" => get_opt(&ent.grid_layout_group, n),
        "Animator" => get_opt(&ent.animator, n),
        "Fog" => get_opt(&ent.fog, n),
        "Particles" => get_opt(&ent.particles, n),
        "Portal" => get_opt(&ent.portal, n),
        _ => Err(error_desconocido(nombre)),
    }
}

/// Aplica un JSON PARCIAL sobre el componente `nombre`: fusiona los
/// campos dados sobre el estado actual (o sobre el valor por defecto si
/// el componente no existe: en ese caso lo crea y registra su nombre en
/// `ent.components`). Err con mensaje claro si: el nombre no es conocido,
/// `parcial` no es un objeto, trae una clave de primer nivel que el
/// componente no tiene, trae un número no finito, o el resultado no
/// deserializa en el tipo del componente.
pub fn aplicar_componente(ent: &mut EntityData, nombre: &str, parcial: &Value) -> Result<(), String> {
    let n = resolver(nombre)?;
    match n {
        "Transform" => {
            let fusion = preparar(n, a_json(&ent.transform, n)?, parcial)?;
            ent.transform = desde_json(fusion, n)?;
            Ok(())
        }
        "MeshRenderer" => set_opt(&mut ent.mesh, &mut ent.components, n, parcial),
        "Rigidbody" => set_fisica(ent, n, parcial, true),
        "Collider" => set_fisica(ent, n, parcial, false),
        "CharacterController" => set_opt(&mut ent.character, &mut ent.components, n, parcial),
        "Camera" => set_opt(&mut ent.camera, &mut ent.components, n, parcial),
        "Light" => set_opt(&mut ent.light, &mut ent.components, n, parcial),
        "Script" => set_script(ent, n, parcial),
        "AudioSource" => set_opt(&mut ent.audio, &mut ent.components, n, parcial),
        "Canvas" => set_opt(&mut ent.canvas, &mut ent.components, n, parcial),
        "RectTransform" => set_opt(&mut ent.rect_transform, &mut ent.components, n, parcial),
        "Text" => set_opt(&mut ent.ui_text, &mut ent.components, n, parcial),
        "Button" => set_opt(&mut ent.ui_button, &mut ent.components, n, parcial),
        "Image" => set_opt(&mut ent.ui_image, &mut ent.components, n, parcial),
        "DragDrop" => set_opt(&mut ent.ui_drag, &mut ent.components, n, parcial),
        "LayoutGroup" => set_opt(&mut ent.layout_group, &mut ent.components, n, parcial),
        "LayoutElement" => set_opt(&mut ent.layout_element, &mut ent.components, n, parcial),
        "ContentSizeFitter" => {
            set_opt(&mut ent.content_size_fitter, &mut ent.components, n, parcial)
        }
        "GridLayoutGroup" => {
            set_opt(&mut ent.grid_layout_group, &mut ent.components, n, parcial)
        }
        "Animator" => set_opt(&mut ent.animator, &mut ent.components, n, parcial),
        "Fog" => set_fog(ent, n, parcial),
        "Particles" => set_opt(&mut ent.particles, &mut ent.components, n, parcial),
        "Portal" => set_opt(&mut ent.portal, &mut ent.components, n, parcial),
        _ => Err(error_desconocido(nombre)),
    }
}

// ---------------------------------------------------------------------------
// Utilidades
// ---------------------------------------------------------------------------

/// Devuelve el nombre canónico. No distingue mayúsculas.
fn resolver(nombre: &str) -> Result<&'static str, String> {
    COMPONENTES
        .iter()
        .copied()
        .find(|c| c.eq_ignore_ascii_case(nombre))
        .ok_or_else(|| error_desconocido(nombre))
}

fn error_desconocido(nombre: &str) -> String {
    format!(
        "componente desconocido: {nombre}. Válidos: {}",
        COMPONENTES.join(", ")
    )
}

fn a_json<T: Serialize>(valor: &T, nombre: &str) -> Result<Value, String> {
    serde_json::to_value(valor).map_err(|e| format!("no se pudo serializar {nombre}: {e}"))
}

fn desde_json<T: DeserializeOwned>(valor: Value, nombre: &str) -> Result<T, String> {
    serde_json::from_value(valor).map_err(|e| format!("valores inválidos para {nombre}: {e}"))
}

/// Falla si algún número del JSON no es finito.
fn comprobar_finitos(valor: &Value, nombre: &str) -> Result<(), String> {
    match valor {
        Value::Number(num) => match num.as_f64() {
            Some(f) if !f.is_finite() => Err(format!("número no finito en {nombre}")),
            _ => Ok(()),
        },
        Value::Array(lista) => lista.iter().try_for_each(|v| comprobar_finitos(v, nombre)),
        Value::Object(mapa) => mapa.values().try_for_each(|v| comprobar_finitos(v, nombre)),
        _ => Ok(()),
    }
}

/// Fusiona `parcial` sobre `base`. Solo fusiona objeto con objeto;
/// cualquier otro valor, incluidos arrays, reemplaza.
fn fusionar(base: &mut Value, parcial: &Value) {
    match (base, parcial) {
        (Value::Object(b), Value::Object(p)) => {
            for (clave, valor) in p {
                match b.get_mut(clave) {
                    Some(actual) => fusionar(actual, valor),
                    None => {
                        b.insert(clave.clone(), valor.clone());
                    }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// Valida el parcial (objeto, finitud, claves de primer nivel) y lo
/// fusiona sobre `base`.
fn preparar(nombre: &str, mut base: Value, parcial: &Value) -> Result<Value, String> {
    let campos = parcial
        .as_object()
        .ok_or_else(|| format!("los valores de {nombre} deben ser un objeto JSON"))?;
    comprobar_finitos(parcial, nombre)?;
    let base_obj = base
        .as_object()
        .ok_or_else(|| format!("{nombre} no se serializa como objeto"))?;
    if let Some(clave) = campos.keys().find(|k| !base_obj.contains_key(*k)) {
        return Err(format!("campo desconocido para {nombre}: {clave}"));
    }
    fusionar(&mut base, parcial);
    Ok(base)
}

/// Registra el nombre en la lista de componentes, sin duplicar.
fn registrar(components: &mut Vec<String>, nombre: &str) {
    if !components.iter().any(|c| c == nombre) {
        components.push(nombre.to_string());
    }
}

fn get_opt<T: Serialize>(campo: &Option<T>, nombre: &str) -> Result<Option<Value>, String> {
    campo.as_ref().map(|v| a_json(v, nombre)).transpose()
}

/// Set genérico para componentes Option. Si no existe, parte del
/// `Default` del tipo.
fn set_opt<T>(
    campo: &mut Option<T>,
    components: &mut Vec<String>,
    nombre: &str,
    parcial: &Value,
) -> Result<(), String>
where
    T: Serialize + DeserializeOwned + Default,
{
    let base = match campo {
        Some(actual) => a_json(actual, nombre)?,
        None => a_json(&T::default(), nombre)?,
    };
    let fusion = preparar(nombre, base, parcial)?;
    let nuevo: T = desde_json(fusion, nombre)?;
    *campo = Some(nuevo);
    registrar(components, nombre);
    Ok(())
}

// ---------------------------------------------------------------------------
// Fog: FogData no implementa Default ni trae defaults de serde
// ---------------------------------------------------------------------------

/// Set de Fog. Al crear parte de una niebla lineal neutra (gris,
/// 10–300 m, los valores con los que Unity crea la suya).
fn set_fog(ent: &mut EntityData, nombre: &str, parcial: &Value) -> Result<(), String> {
    let base = match &ent.fog {
        Some(f) => a_json(f, nombre)?,
        None => serde_json::json!({ "color": [0.5, 0.5, 0.5], "near": 10.0, "far": 300.0 }),
    };
    let fusion = preparar(nombre, base, parcial)?;
    ent.fog = Some(desde_json(fusion, nombre)?);
    registrar(&mut ent.components, nombre);
    Ok(())
}

// ---------------------------------------------------------------------------
// Script
// ---------------------------------------------------------------------------

/// Foto del script: {"script": ..., "data": ...}, como el editor.
fn foto_script(ent: &EntityData) -> Result<Value, String> {
    let mut mapa = Map::new();
    mapa.insert("script".into(), a_json(&ent.script, "Script")?);
    mapa.insert("data".into(), a_json(&ent.script_data, "Script")?);
    Ok(Value::Object(mapa))
}

fn set_script(ent: &mut EntityData, nombre: &str, parcial: &Value) -> Result<(), String> {
    let fusion = preparar(nombre, foto_script(ent)?, parcial)?;
    let script = desde_json(fusion.get("script").cloned().unwrap_or(Value::Null), nombre)?;
    let data = desde_json(fusion.get("data").cloned().unwrap_or(Value::Null), nombre)?;
    ent.script = script;
    ent.script_data = data;
    registrar(&mut ent.components, nombre);
    Ok(())
}

// ---------------------------------------------------------------------------
// Física: Rigidbody y Collider comparten PhysicsData
// ---------------------------------------------------------------------------

/// Quita la parte Collider de un PhysicsData.
fn neutralizar_collider(p: &mut PhysicsData) {
    p.has_collider = false;
    p.colliders.clear();
    p.mesh_collider = PhysicsData::default().mesh_collider;
}

/// Copia solo la parte Rigidbody de `src` sobre `dest`.
fn copiar_parte_rb(dest: &mut PhysicsData, src: PhysicsData) {
    dest.body_type = src.body_type;
    dest.mass = src.mass;
    dest.linear_damping = src.linear_damping;
    dest.angular_damping = src.angular_damping;
    dest.use_gravity = src.use_gravity;
    dest.constraints = src.constraints;
    dest.ccd = src.ccd;
    dest.interpolate = src.interpolate;
    dest.solver_iterations = src.solver_iterations;
    dest.solver_velocity_iterations = src.solver_velocity_iterations;
    dest.max_depenetration_velocity = src.max_depenetration_velocity;
    dest.automatic_center_of_mass = src.automatic_center_of_mass;
    dest.automatic_inertia_tensor = src.automatic_inertia_tensor;
}

/// Copia solo la parte Collider de `src` sobre `dest`.
fn copiar_parte_collider(dest: &mut PhysicsData, src: PhysicsData) {
    dest.colliders = src.colliders;
    dest.mesh_collider = src.mesh_collider;
    dest.box_extents = src.box_extents;
    dest.box_offset = src.box_offset;
    dest.is_trigger = src.is_trigger;
    dest.legacy_friction = src.legacy_friction;
    dest.legacy_restitution = src.legacy_restitution;
    dest.mesh_collider_material = src.mesh_collider_material;
    dest.mesh_collider_is_trigger = src.mesh_collider_is_trigger;
}

/// Set de Rigidbody (`rigidbody = true`) o Collider (`false`).
///
/// La guardia de claves se hace contra el JSON completo de PhysicsData:
/// se aceptan claves de la otra parte, pero solo se copia la parte propia.
fn set_fisica(ent: &mut EntityData, nombre: &str, parcial: &Value, rigidbody: bool) -> Result<(), String> {
    let mut base = match &ent.physics {
        Some(p) => a_json(p, nombre)?,
        None => a_json(&PhysicsData::default(), nombre)?,
    };
    // PhysicsData omite al serializar tres claves cuando están vacías
    // (skip_serializing_if): sin sembrarlas, la guardia de claves de
    // `preparar` las rechazaría como desconocidas al escribirlas.
    if let Some(obj) = base.as_object_mut() {
        obj.entry("friction").or_insert(Value::Null);
        obj.entry("restitution").or_insert(Value::Null);
        obj.entry("mesh_collider_material")
            .or_insert_with(|| Value::String(String::new()));
    }
    let fusion = preparar(nombre, base, parcial)?;
    let nuevo: PhysicsData = desde_json(fusion, nombre)?;
    let destino = ent.physics.get_or_insert_with(PhysicsData::default);
    if rigidbody {
        copiar_parte_rb(destino, nuevo);
        destino.has_rigidbody = true;
    } else {
        copiar_parte_collider(destino, nuevo);
        destino.has_collider = true;
    }
    registrar(&mut ent.components, nombre);
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entidad() -> EntityData {
        EntityData::new(1, "Prueba".into(), vec!["Transform".into()])
    }

    /// Suma 1 a todos los números del JSON, sea cual sea su forma.
    fn sumar_uno(v: &Value) -> Value {
        match v {
            Value::Number(n) => json!(n.as_f64().unwrap() + 1.0),
            Value::Array(a) => Value::Array(a.iter().map(sumar_uno).collect()),
            Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), sumar_uno(x))).collect()),
            otro => otro.clone(),
        }
    }

    const CLAVES_COLLIDER: &[&str] = &[
        "has_collider",
        "colliders",
        "mesh_collider",
        "box_extents",
        "box_offset",
        "is_trigger",
        "friction",
        "restitution",
        "mesh_collider_material",
        "mesh_collider_is_trigger",
    ];

    #[test]
    fn transform_cambiar_position_conserva_rotation() {
        let mut ent = entidad();
        let antes = componente_json(&ent, "Transform").unwrap().unwrap();
        let nueva = sumar_uno(&antes["position"]);
        aplicar_componente(&mut ent, "Transform", &json!({ "position": nueva })).unwrap();
        let despues = componente_json(&ent, "Transform").unwrap().unwrap();
        assert_eq!(despues["position"], nueva);
        assert_eq!(despues["rotation"], antes["rotation"]);
    }

    #[test]
    fn set_crea_collider_sin_physics() {
        let mut ent = entidad();
        assert!(ent.physics.is_none());
        aplicar_componente(&mut ent, "Collider", &json!({ "box_extents": [2.0, 1.0, 2.0] })).unwrap();
        assert!(ent.components.iter().any(|c| c == "Collider"));
        assert!(ent.physics.as_ref().unwrap().has_collider);
        let v = componente_json(&ent, "Collider").unwrap().unwrap();
        assert_eq!(v["box_extents"], json!([2.0, 1.0, 2.0]));
    }

    #[test]
    fn rigidbody_y_collider_no_se_pisan() {
        let mut ent = entidad();
        aplicar_componente(
            &mut ent,
            "Collider",
            &json!({ "box_extents": [3.0, 3.0, 3.0], "is_trigger": true }),
        )
        .unwrap();
        let col_antes = componente_json(&ent, "Collider").unwrap().unwrap();
        let n_colliders = ent.physics.as_ref().unwrap().colliders.len();

        aplicar_componente(&mut ent, "Rigidbody", &json!({ "mass": 5.0 })).unwrap();
        let p = ent.physics.as_ref().unwrap();
        assert!(p.has_rigidbody);
        assert!(p.has_collider);
        assert_eq!(p.colliders.len(), n_colliders);
        let col_despues = componente_json(&ent, "Collider").unwrap().unwrap();
        for k in CLAVES_COLLIDER {
            assert_eq!(col_antes.get(*k), col_despues.get(*k), "Rigidbody tocó {k}");
        }

        // La clave "mass" se acepta en Collider, pero no se copia.
        aplicar_componente(&mut ent, "Collider", &json!({ "mass": 99.0, "is_trigger": false })).unwrap();
        let rb = componente_json(&ent, "Rigidbody").unwrap().unwrap();
        assert_eq!(rb["mass"], json!(5.0));
        assert!(ent.components.iter().any(|c| c == "Rigidbody"));
    }

    #[test]
    fn light_merge_parcial_conserva_resto() {
        let mut ent = entidad();
        ent.light = Some(Default::default());
        let antes = componente_json(&ent, "Light").unwrap().unwrap();
        let (clave, valor) = antes
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, v)| v.is_f64())
            .map(|(k, v)| (k.clone(), v.as_f64().unwrap()))
            .expect("Light sin campo numérico decimal");
        let nuevo = valor + 0.5;
        let mut parcial = Map::new();
        parcial.insert(clave.clone(), json!(nuevo));
        aplicar_componente(&mut ent, "Light", &Value::Object(parcial)).unwrap();

        let despues = componente_json(&ent, "Light").unwrap().unwrap();
        for (k, v) in antes.as_object().unwrap() {
            if *k == clave {
                assert!((despues[k].as_f64().unwrap() - nuevo).abs() < 1e-5);
            } else {
                assert_eq!(&despues[k], v, "cambió {k}");
            }
        }
    }

    #[test]
    fn get_componente_ausente_es_none() {
        assert_eq!(componente_json(&entidad(), "Light").unwrap(), None);
    }

    #[test]
    fn errores_de_nombre_clave_y_forma() {
        let mut ent = entidad();
        assert!(componente_json(&ent, "NoExiste").is_err());
        assert!(aplicar_componente(&mut ent, "NoExiste", &json!({})).is_err());
        let e = aplicar_componente(&mut ent, "Light", &json!({ "inventado": 1 })).unwrap_err();
        assert!(e.contains("inventado"));
        assert!(aplicar_componente(&mut ent, "Light", &json!([1, 2])).is_err());
        assert!(ent.light.is_none());
        assert!(!ent.components.iter().any(|c| c == "Light"));
    }
}
