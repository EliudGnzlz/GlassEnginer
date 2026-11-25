//! Parseo de archivos `.meta` de Unity a `MetaAsset`.
//!
//! Toda la coerción de escalares YAML es tolerante: un valor que no se puede
//! interpretar se trata como ausente, nunca como error ni como panic.

use serde_yaml::{Mapping, Number, Value};

use crate::{MetaAsset, RipperError, SubObjeto, TexturaInfo};

/// Claves de nivel superior que nunca contienen el bloque del importer.
const CLAVES_NO_IMPORTER: [&str; 4] = ["fileFormatVersion", "guid", "timeCreated", "licenseType"];

/// Importer asignado cuando el .meta no declara ningún bloque de importer.
const IMPORTER_DESCONOCIDO: &str = "Desconocido";

/// Parsea el texto de un `.meta` de Unity.
///
/// `guid` se usa para los mensajes de error y como valor de respaldo si el
/// propio `.meta` no trae un `guid` válido.
pub fn parsear_meta(guid: &str, texto: &str) -> Result<MetaAsset, RipperError> {
    let valor: Value = serde_yaml::from_str(texto).map_err(|e| RipperError::Meta {
        guid: guid.to_string(),
        motivo: format!("YAML inválido: {e}"),
    })?;

    let raiz = match sin_etiqueta(&valor) {
        Value::Mapping(m) => m,
        _ => {
            return Err(RipperError::Meta {
                guid: guid.to_string(),
                motivo: "el documento .meta no es un mapping de nivel superior".to_string(),
            })
        }
    };

    let guid_final = campo(raiz, "guid")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(guid)
        .to_string();

    let importer = match detectar_importer(raiz) {
        Some(nombre) => nombre,
        None => {
            // .meta mínimo sin bloque de importer: no es un error.
            return Ok(MetaAsset {
                guid: guid_final,
                importer: IMPORTER_DESCONOCIDO.to_string(),
                sub_objetos: Vec::new(),
                global_scale: None,
                use_file_scale: None,
                textura: None,
            });
        }
    };

    // Un bloque ausente o no-mapping (p. ej. `DefaultImporter:` sin contenido)
    // se trata como un bloque vacío: todos sus campos quedan ausentes.
    let vacio = Mapping::new();
    let bloque = campo(raiz, &importer)
        .and_then(Value::as_mapping)
        .unwrap_or(&vacio);

    let (sub_objetos, global_scale, use_file_scale) = if importer == "ModelImporter" {
        // Unity guarda `globalScale` y `useFileScale` dentro de `meshes`; se
        // busca primero en el propio bloque y después en ese sub-bloque.
        (
            sub_objetos_modelo(bloque),
            buscar(bloque, "globalScale", "meshes", a_f32),
            buscar(bloque, "useFileScale", "meshes", a_bool),
        )
    } else {
        (Vec::new(), None, None)
    };

    let textura = if importer == "TextureImporter" {
        Some(textura_info(bloque))
    } else {
        None
    };

    Ok(MetaAsset {
        guid: guid_final,
        importer,
        sub_objetos,
        global_scale,
        use_file_scale,
        textura,
    })
}

/// Determina la clave de nivel superior que contiene el bloque del importer.
fn detectar_importer(raiz: &Mapping) -> Option<String> {
    let candidatas: Vec<&str> = raiz
        .iter()
        .filter_map(|(clave, _)| sin_etiqueta(clave).as_str())
        .filter(|clave| !CLAVES_NO_IMPORTER.contains(clave))
        .collect();

    match candidatas.as_slice() {
        [] => None,
        [unica] => Some((*unica).to_string()),
        varias => varias
            .iter()
            .find(|clave| clave.ends_with("Importer"))
            .map(|clave| (*clave).to_string()),
    }
}

/// Extrae los sub-objetos de un `ModelImporter`.
///
/// `internalIDToNameTable` (formato 2020+) tiene prioridad sobre
/// `fileIDToRecycleName` (formato plano antiguo).
fn sub_objetos_modelo(bloque: &Mapping) -> Vec<SubObjeto> {
    if let Some(tabla) = campo(bloque, "internalIDToNameTable").and_then(Value::as_sequence) {
        return tabla.iter().filter_map(entrada_tabla_interna).collect();
    }

    if let Some(tabla) = campo(bloque, "fileIDToRecycleName").and_then(Value::as_mapping) {
        let mut salida: Vec<SubObjeto> = tabla
            .iter()
            .filter_map(|(clave, nombre)| {
                let file_id = a_i64(clave)?;
                let nombre = sin_etiqueta(nombre).as_str()?;
                Some(SubObjeto {
                    // En el formato plano el class_id va codificado en el file_id.
                    class_id: (file_id / 100_000) as i32,
                    file_id,
                    nombre: nombre.to_string(),
                })
            })
            .collect();
        // Orden estable por file_id para que la salida sea determinista.
        salida.sort_by_key(|s| s.file_id);
        return salida;
    }

    Vec::new()
}

/// Convierte una entrada `{first: {<class_id>: <file_id>}, second: <nombre>}`.
/// Cualquier entrada con forma inesperada se descarta.
fn entrada_tabla_interna(entrada: &Value) -> Option<SubObjeto> {
    let entrada = sin_etiqueta(entrada).as_mapping()?;
    let first = campo(entrada, "first")?.as_mapping()?;
    if first.len() != 1 {
        return None;
    }
    let (clave, valor) = first.iter().next()?;
    let class_id = i32::try_from(a_i64(clave)?).ok()?;
    let file_id = a_i64(valor)?;
    let nombre = campo(entrada, "second")?.as_str()?.to_string();
    Some(SubObjeto {
        class_id,
        file_id,
        nombre,
    })
}

/// Construye la información de textura de un `TextureImporter`.
fn textura_info(bloque: &Mapping) -> TexturaInfo {
    let tipo = match campo(bloque, "textureType").and_then(a_i64) {
        None | Some(0) => "Default".to_string(),
        Some(1) => "NormalMap".to_string(),
        Some(2) => "GUI".to_string(),
        Some(3) => "Sprite".to_string(),
        Some(4) => "Cursor".to_string(),
        Some(5) => "Cookie".to_string(),
        Some(6) => "Lightmap".to_string(),
        Some(7) => "SingleChannel".to_string(),
        Some(n) => format!("Otro({n})"),
    };

    // Unity guarda `sRGBTexture` dentro de `mipmaps`; se acepta en ambos sitios.
    let srgb = buscar(bloque, "sRGBTexture", "mipmaps", a_bool).unwrap_or(false);

    let ajustes = campo(bloque, "textureSettings").and_then(Value::as_mapping);
    let entero = |clave: &str| ajustes.and_then(|m| campo(m, clave)).and_then(a_i64);

    let filtro = match entero("filterMode") {
        Some(0) => "Point".to_string(),
        None | Some(1) => "Bilinear".to_string(),
        Some(2) => "Trilinear".to_string(),
        Some(n) => format!("Otro({n})"),
    };

    TexturaInfo {
        tipo,
        srgb,
        filtro,
        wrap_u: nombre_wrap(entero("wrapU")),
        wrap_v: nombre_wrap(entero("wrapV")),
    }
}

/// Nombre del modo de wrap; ausente equivale al valor por defecto de Unity.
fn nombre_wrap(valor: Option<i64>) -> String {
    match valor {
        None | Some(0) => "Repeat".to_string(),
        Some(1) => "Clamp".to_string(),
        Some(2) => "Mirror".to_string(),
        Some(3) => "MirrorOnce".to_string(),
        Some(n) => format!("Otro({n})"),
    }
}

/// Busca `clave` en el bloque y, si no está o no es coercible, en el
/// sub-bloque anidado `sub_bloque`.
fn buscar<T>(
    bloque: &Mapping,
    clave: &str,
    sub_bloque: &str,
    coercer: fn(&Value) -> Option<T>,
) -> Option<T> {
    campo(bloque, clave).and_then(coercer).or_else(|| {
        campo(bloque, sub_bloque)
            .and_then(Value::as_mapping)
            .and_then(|m| campo(m, clave))
            .and_then(coercer)
    })
}

/// Obtiene un campo de un mapping, ya sin etiquetas YAML.
fn campo<'a>(mapa: &'a Mapping, clave: &str) -> Option<&'a Value> {
    mapa.get(clave).map(sin_etiqueta)
}

/// Elimina las etiquetas YAML (`!tag valor`) que envuelvan a un valor.
fn sin_etiqueta(mut valor: &Value) -> &Value {
    while let Value::Tagged(etiquetado) = valor {
        valor = &etiquetado.value;
    }
    valor
}

/// Coerción a i64: entero con signo, entero sin signo que quepa, flotante
/// entero o string numérico.
fn a_i64(valor: &Value) -> Option<i64> {
    match sin_etiqueta(valor) {
        Value::Number(n) => numero_a_i64(n),
        Value::String(s) => {
            let s = s.trim();
            s.parse::<i64>()
                .ok()
                .or_else(|| s.parse::<f64>().ok().and_then(f64_entero_a_i64))
        }
        _ => None,
    }
}

fn numero_a_i64(n: &Number) -> Option<i64> {
    if let Some(i) = n.as_i64() {
        return Some(i);
    }
    if let Some(u) = n.as_u64() {
        return i64::try_from(u).ok();
    }
    n.as_f64().and_then(f64_entero_a_i64)
}

/// Convierte un f64 a i64 solo si es finito, entero y está en rango.
fn f64_entero_a_i64(f: f64) -> Option<i64> {
    // 2^63, representable exactamente en f64.
    const LIMITE: f64 = 9_223_372_036_854_775_808.0;
    if f.is_finite() && f.fract() == 0.0 && (-LIMITE..LIMITE).contains(&f) {
        Some(f as i64)
    } else {
        None
    }
}

/// Coerción a bool: booleano o entero distinto de cero.
fn a_bool(valor: &Value) -> Option<bool> {
    match sin_etiqueta(valor) {
        Value::Bool(b) => Some(*b),
        otro => a_i64(otro).map(|i| i != 0),
    }
}

/// Coerción a f32: cualquier número o string numérico. Los valores no
/// finitos (NaN, infinito) se tratan como no coercibles.
fn a_f32(valor: &Value) -> Option<f32> {
    let f = match sin_etiqueta(valor) {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }?;
    if f.is_finite() {
        Some(f as f32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsear(guid: &str, texto: &str) -> MetaAsset {
        match parsear_meta(guid, texto) {
            Ok(meta) => meta,
            Err(e) => panic!("se esperaba un .meta válido: {e}"),
        }
    }

    fn tripletas(meta: &MetaAsset) -> Vec<(i32, i64, String)> {
        meta.sub_objetos
            .iter()
            .map(|s| (s.class_id, s.file_id, s.nombre.clone()))
            .collect()
    }

    const META_MODELO_20200: &str = r#"fileFormatVersion: 2
guid: 3f2a9c1e5b7d4e8fa1c2b3d4e5f6a718
ModelImporter:
  serializedVersion: 20200
  internalIDToNameTable:
  - first:
      43: 4300000
    second: Tree3
  - first:
      74: 7400000
    second: Idle
  - first:
      1: 100000
    second: //RootNode
  externalObjects: {}
  materials:
    materialImportMode: 1
    materialName: 0
    materialSearch: 1
    materialLocation: 1
  animations:
    legacyGenerateAnimations: 4
    bakeSimulation: 0
    resampleCurves: 1
  meshes:
    lODScreenPercentages: []
    globalScale: 1
    meshCompression: 0
    addColliders: 0
    importVisibility: 1
    importBlendShapes: 1
    importCameras: 1
    importLights: 1
    swapUVChannels: 0
    generateSecondaryUV: 0
    useFileUnits: 1
    keepQuads: 0
    weldVertices: 1
    useFileScale: 1
  importAnimation: 1
  userData: 
  assetBundleName: 
  assetBundleVariant: 
"#;

    const META_MODELO_23: &str = r#"fileFormatVersion: 2
guid: 7b9e2d4c6a8f4b1d9e3c5a7b9d1f3e5a
ModelImporter:
  serializedVersion: 23
  fileIDToRecycleName:
    7400000: Buzzer Hover
    100000: //RootNode
    4300000: Tree3
  externalObjects: {}
  materials:
    importMaterials: 1
    materialName: 0
    materialSearch: 1
  meshes:
    lODScreenPercentages: []
    globalScale: 0.01
    meshCompression: 0
    useFileScale: 1
  importAnimation: 1
  userData: 
  assetBundleName: 
  assetBundleVariant: 
"#;

    const META_TEXTURA: &str = r#"fileFormatVersion: 2
guid: c4d5e6f7a8b94c0d8e1f2a3b4c5d6e7f
TextureImporter:
  internalIDToNameTable: []
  externalObjects: {}
  serializedVersion: 11
  mipmaps:
    mipMapMode: 0
    enableMipMap: 1
    sRGBTexture: 1
    linearTexture: 0
    fadeOut: 0
    borderMipMap: 0
  bumpmap:
    convertToNormalMap: 0
    externalNormalMap: 0
    heightScale: 0.25
  isReadable: 0
  maxTextureSize: 2048
  textureSettings:
    serializedVersion: 2
    filterMode: 1
    aniso: 1
    mipBias: 0
    wrapU: 0
    wrapV: 0
    wrapW: 0
  nPOTScale: 1
  alphaUsage: 1
  textureType: 0
  textureShape: 1
  userData: 
  assetBundleName: 
  assetBundleVariant: 
"#;

    const META_MINIMO: &str = r#"fileFormatVersion: 2
guid: e1f2a3b4c5d64e7f8a9b0c1d2e3f4a5b
FolderAsset: {}
"#;

    #[test]
    fn modelo_20200_usa_internal_id_to_name_table() {
        let meta = parsear("respaldo", META_MODELO_20200);
        assert_eq!(meta.guid, "3f2a9c1e5b7d4e8fa1c2b3d4e5f6a718");
        assert_eq!(meta.importer, "ModelImporter");
        assert_eq!(
            tripletas(&meta),
            vec![
                (43, 4_300_000, "Tree3".to_string()),
                (74, 7_400_000, "Idle".to_string()),
                (1, 100_000, "//RootNode".to_string()),
            ]
        );
        assert_eq!(meta.global_scale, Some(1.0));
        assert_eq!(meta.use_file_scale, Some(true));
        assert!(meta.textura.is_none());
    }

    #[test]
    fn modelo_23_deriva_class_id_de_file_id_to_recycle_name() {
        let meta = parsear("respaldo", META_MODELO_23);
        assert_eq!(meta.importer, "ModelImporter");
        assert_eq!(
            tripletas(&meta),
            vec![
                (1, 100_000, "//RootNode".to_string()),
                (43, 4_300_000, "Tree3".to_string()),
                (74, 7_400_000, "Buzzer Hover".to_string()),
            ]
        );
    }

    #[test]
    fn texture_importer_produce_textura_info() {
        let meta = parsear("respaldo", META_TEXTURA);
        assert_eq!(meta.importer, "TextureImporter");
        assert!(meta.sub_objetos.is_empty());
        assert!(meta.global_scale.is_none());
        assert!(meta.use_file_scale.is_none());
        let textura = match meta.textura {
            Some(t) => t,
            None => panic!("se esperaba TexturaInfo"),
        };
        assert_eq!(textura.tipo, "Default");
        assert!(textura.srgb);
        assert_eq!(textura.filtro, "Bilinear");
        assert_eq!(textura.wrap_u, "Repeat");
        assert_eq!(textura.wrap_v, "Repeat");
    }

    #[test]
    fn meta_minimo_folder_asset() {
        let meta = parsear("respaldo", META_MINIMO);
        assert_eq!(meta.guid, "e1f2a3b4c5d64e7f8a9b0c1d2e3f4a5b");
        assert_eq!(meta.importer, "FolderAsset");
        assert!(meta.sub_objetos.is_empty());
        assert!(meta.textura.is_none());
        assert!(meta.global_scale.is_none());
        assert!(meta.use_file_scale.is_none());
    }

    #[test]
    fn texto_basura_es_error() {
        let resultado = parsear_meta("abc", "esto no es: [yaml");
        assert!(matches!(resultado, Err(RipperError::Meta { .. })));
    }
}