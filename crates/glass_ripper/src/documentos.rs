//! Partición de archivos UnityYAML multidocumento y lectores de campos.
//!
//! Un archivo de Unity (escena o prefab) es una secuencia de documentos
//! que empiezan con `--- !u!<ClassID> &<fileID>` (con ` stripped`
//! opcional). Cada documento es un mapa con una única clave raíz que
//! nombra la clase (`GameObject:`, `Transform:`…).

use serde_yaml::Value;

use crate::materiales::{deduplicar_claves, entrecomillar_guids};

/// Un documento de un archivo UnityYAML.
#[derive(Debug, Clone)]
pub struct DocumentoUnity {
    /// ClassID de Unity (1 GameObject, 4 Transform…).
    pub clase: i32,
    /// fileID local del objeto dentro del archivo.
    pub file_id: i64,
    /// Cabecera con sufijo ` stripped` (ancla de una instancia de prefab).
    pub stripped: bool,
    /// El mapa bajo la clave raíz del documento (p. ej. el valor de
    /// "GameObject"). Value::Null si el documento no parseó.
    pub valor: Value,
    /// Nombre de la clave raíz ("GameObject", "Prefab"…).
    pub nombre: String,
}

/// Referencia de Unity `{fileID: N, guid: G, type: T}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefUnity {
    /// fileID del objeto referenciado (0 = referencia nula).
    pub file_id: i64,
    /// GUID del asset externo; None si la referencia es local.
    pub guid: Option<String>,
    /// Campo `type` de Unity (0 si falta).
    pub tipo: i32,
}

/// Lee una cabecera `--- !u!<clase> &<fileID>[ stripped]`.
fn cabecera(linea: &str) -> Option<(i32, i64, bool)> {
    let resto = linea.trim_end().strip_prefix("--- !u!")?;
    let mut partes = resto.split_whitespace();
    let clase = partes.next()?.parse().ok()?;
    let file_id = partes.next()?.strip_prefix('&')?.parse().ok()?;
    let stripped = partes.next() == Some("stripped");
    Some((clase, file_id, stripped))
}

/// Parsea el cuerpo de un documento y separa la clave raíz de su valor.
fn parsear_cuerpo(cuerpo: &str) -> (String, Value) {
    let texto = entrecomillar_guids(cuerpo);
    let raiz = match serde_yaml::from_str::<Value>(&texto) {
        Ok(v) => Some(v),
        Err(e) if e.to_string().contains("duplicate entry") => {
            serde_yaml::from_str::<Value>(&deduplicar_claves(&texto).0).ok()
        }
        Err(_) => None,
    };
    match raiz {
        Some(Value::Mapping(m)) => match m.into_iter().next() {
            Some((k, v)) => (k.as_str().unwrap_or_default().to_string(), v),
            None => (String::new(), Value::Null),
        },
        _ => (String::new(), Value::Null),
    }
}

fn documento(cab: (i32, i64, bool), cuerpo: &str) -> DocumentoUnity {
    let (nombre, valor) = parsear_cuerpo(cuerpo);
    DocumentoUnity {
        clase: cab.0,
        file_id: cab.1,
        stripped: cab.2,
        valor,
        nombre,
    }
}

/// Parte un archivo UnityYAML en sus documentos, en orden de aparición.
/// Un documento que no parsea se conserva con `valor: Value::Null`.
pub fn partir_documentos(texto: &str) -> Vec<DocumentoUnity> {
    let mut docs = Vec::new();
    let mut actual: Option<((i32, i64, bool), String)> = None;
    let lineas = texto
        .lines()
        .filter(|l| !l.starts_with("%YAML") && !l.starts_with("%TAG"));
    for linea in lineas {
        if let Some(cab) = cabecera(linea) {
            if let Some((c, cuerpo)) = actual.take() {
                docs.push(documento(c, &cuerpo));
            }
            actual = Some((cab, String::new()));
        } else if let Some((_, cuerpo)) = actual.as_mut() {
            cuerpo.push_str(linea);
            cuerpo.push('\n');
        }
    }
    if let Some((c, cuerpo)) = actual {
        docs.push(documento(c, &cuerpo));
    }
    docs
}

// Lectores numéricos locales: aceptan números, textos numéricos y
// booleanos, como los de `crate::materiales`.
fn numero_de(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

fn entero_valor(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_u64().map(|u| u as i64))
            .or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

/// Valor de `clave` en un mapa; None si no es mapa o falta la clave.
pub fn campo<'a>(v: &'a Value, clave: &str) -> Option<&'a Value> {
    v.as_mapping()?.get(Value::String(clave.to_string()))
}

/// Lee una referencia `{fileID, guid?, type?}`; un escalar (nulo) → None.
pub fn referencia(v: &Value) -> Option<RefUnity> {
    v.as_mapping()?;
    Some(RefUnity {
        file_id: entero_de(v, "fileID").unwrap_or(0),
        guid: texto_de(v, "guid").filter(|g| !g.is_empty()),
        tipo: entero_de(v, "type").map_or(0, |t| t as i32),
    })
}

/// Vector `{x, y, z}`.
pub fn vec3(v: &Value) -> Option<[f32; 3]> {
    Some([flotante(v, "x")?, flotante(v, "y")?, flotante(v, "z")?])
}

/// Cuaternión `{x, y, z, w}`.
pub fn quat(v: &Value) -> Option<[f32; 4]> {
    Some([
        flotante(v, "x")?,
        flotante(v, "y")?,
        flotante(v, "z")?,
        flotante(v, "w")?,
    ])
}

/// Color `{r, g, b, a}` (alfa 1 si falta).
pub fn color(v: &Value) -> Option<[f32; 4]> {
    Some([
        flotante(v, "r")?,
        flotante(v, "g")?,
        flotante(v, "b")?,
        flotante(v, "a").unwrap_or(1.0),
    ])
}

/// Número en coma flotante bajo `clave`.
pub fn flotante(v: &Value, clave: &str) -> Option<f32> {
    campo(v, clave).and_then(numero_de).map(|f| f as f32)
}

/// Entero bajo `clave`.
pub fn entero_de(v: &Value, clave: &str) -> Option<i64> {
    campo(v, clave).and_then(entero_valor)
}

/// Booleano de Unity: número != 0 o `true`; ausente = false.
pub fn booleano(v: &Value, clave: &str) -> bool {
    campo(v, clave).is_some_and(|x| match x {
        Value::Bool(b) => *b,
        Value::String(s) if s.trim().eq_ignore_ascii_case("true") => true,
        otro => numero_de(otro).is_some_and(|n| n != 0.0),
    })
}

/// Texto bajo `clave` (los números y booleanos se pasan a texto).
pub fn texto_de(v: &Value, clave: &str) -> Option<String> {
    match campo(v, clave)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI: &str = r#"%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!1 &3057725258070210309
GameObject:
  m_ObjectHideFlags: 0
  m_Component:
  - component: {fileID: 11}
  - component: {fileID: 12}
  - component: {fileID: 13}
  m_Layer: 0
  m_Name: Caja
  m_IsActive: 1
--- !u!4 &11
Transform:
  m_GameObject: {fileID: 3057725258070210309}
  m_LocalPosition: {x: 1, y: 2, z: 3}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
--- !u!33 &12
MeshFilter:
  m_GameObject: {fileID: 3057725258070210309}
  m_Mesh: {fileID: 4300000, guid: 12345678901234567890123456789012, type: 3}
--- !u!23 &13
MeshRenderer:
  m_GameObject: {fileID: 3057725258070210309}
  m_Enabled: 1
"#;

    fn casi(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn parte_mini_prefab() {
        let docs = partir_documentos(MINI);
        assert_eq!(docs.len(), 4);
        let clases: Vec<i32> = docs.iter().map(|d| d.clase).collect();
        assert_eq!(clases, vec![1, 4, 33, 23]);
        assert_eq!(docs[0].file_id, 3_057_725_258_070_210_309);
        assert_eq!(docs[3].file_id, 13);
        assert!(docs.iter().all(|d| !d.stripped));
        assert_eq!(docs[0].nombre, "GameObject");
        assert_eq!(texto_de(&docs[0].valor, "m_Name").as_deref(), Some("Caja"));
        assert!(booleano(&docs[0].valor, "m_IsActive"));
        let malla = campo(&docs[2].valor, "m_Mesh").and_then(referencia).unwrap();
        assert_eq!(malla.file_id, 4_300_000);
        assert_eq!(
            malla.guid.as_deref(),
            Some("12345678901234567890123456789012")
        );
        assert_eq!(malla.tipo, 3);
    }

    #[test]
    fn documento_stripped() {
        let texto = "--- !u!4 &77 stripped\nTransform:\n  m_PrefabInstance: {fileID: 5}\n";
        let docs = partir_documentos(texto);
        assert_eq!(docs.len(), 1);
        assert!(docs[0].stripped);
        assert_eq!(docs[0].clase, 4);
        assert_eq!(docs[0].file_id, 77);
        let r = campo(&docs[0].valor, "m_PrefabInstance").and_then(referencia).unwrap();
        assert_eq!(r.file_id, 5);
        assert!(r.guid.is_none());
    }

    #[test]
    fn tolera_claves_duplicadas() {
        let texto = "--- !u!1 &10\nGameObject:\n  m_Name: A\n  m_Name: B\n";
        let docs = partir_documentos(texto);
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].nombre, "GameObject");
        assert!(!docs[0].valor.is_null());
    }

    #[test]
    fn referencias_y_vectores() {
        let v: Value = serde_yaml::from_str(
            "a: {fileID: 7}\nb: {fileID: 9, guid: abcdef0123456789abcdef0123456789, type: 2}\nc: \np: {x: 1, y: 2.5, z: -3}\nq: {x: 0, y: 0, z: 0, w: 1}\nk: {r: 1, g: 0.5, b: 0.25, a: 1}\n",
        )
        .unwrap();
        let a = campo(&v, "a").and_then(referencia).unwrap();
        assert_eq!((a.file_id, a.guid.clone(), a.tipo), (7, None, 0));
        let b = campo(&v, "b").and_then(referencia).unwrap();
        assert_eq!(b.guid.as_deref(), Some("abcdef0123456789abcdef0123456789"));
        assert_eq!(b.tipo, 2);
        assert!(campo(&v, "c").and_then(referencia).is_none());
        let p = campo(&v, "p").and_then(vec3).unwrap();
        assert!(casi(p[0], 1.0) && casi(p[1], 2.5) && casi(p[2], -3.0));
        let q = campo(&v, "q").and_then(quat).unwrap();
        assert!(casi(q[3], 1.0));
        let k = campo(&v, "k").and_then(color).unwrap();
        assert!(casi(k[1], 0.5) && casi(k[2], 0.25));
    }
}
