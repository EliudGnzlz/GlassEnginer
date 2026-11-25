//! Conversión de modelos del paquete (FBX → GLB) y ajuste de la escala del ModelImporter.

use std::fs;
use std::path::Path;

use glass_fbx::{convertir_fbx, InformeConversion};
use serde_json::Value;

use crate::{MetaAsset, RipperError};

/// Cabecera mágica de un GLB.
const MAGIC: &[u8] = b"glTF";
/// Tipo de trozo JSON.
const TIPO_JSON: u32 = 0x4E4F_534A;
/// Tipo de trozo BIN.
const TIPO_BIN: u32 = 0x004E_4942;

/// Convierte un `.fbx` del paquete en `.glb` por la vía del motor (glass_fbx)
/// y aplica encima la escala del ModelImporter.
pub fn convertir_modelo(
    fuente: &Path,
    destino_glb: &Path,
    meta: Option<&MetaAsset>,
) -> Result<(InformeConversion, Vec<String>), RipperError> {
    let ext = fuente
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if ext != "fbx" {
        return Err(RipperError::Conversion {
            ruta: fuente.display().to_string(),
            motivo: format!(
                "formato de modelo no convertible en U2: {ext} (el FBX es la vía; .blend entra por el puente del editor)"
            ),
        });
    }
    let informe = convertir_fbx(fuente, destino_glb).map_err(|e| RipperError::Conversion {
        ruta: fuente.display().to_string(),
        motivo: e.to_string(),
    })?;

    let mut avisos = informe.avisos.clone();
    if matches!(meta.and_then(|m| m.use_file_scale), Some(false)) {
        avisos.push(
            "useFileScale desactivado en Unity: la vía del motor normaliza siempre las unidades del FBX a metros, el tamaño puede diferir del de Unity si el archivo no está en metros"
                .to_string(),
        );
    }

    let factor = meta.and_then(|m| m.global_scale).unwrap_or(1.0);
    if (factor - 1.0).abs() > 1e-6 {
        let ruta = destino_glb.display().to_string();
        let bytes = fs::read(destino_glb).map_err(|e| RipperError::Io {
            ruta: ruta.clone(),
            fuente: e,
        })?;
        // Ajuste al integrar (Rayo): los errores de aplicar_escala_glb
        // traen la ruta comodín "<glb>"; aquí se re-etiquetan con la
        // ruta real del destino para que el informe señale el archivo.
        let (nuevo, saltados) = aplicar_escala_glb(&bytes, factor).map_err(|e| match e {
            RipperError::Conversion { motivo, .. } => RipperError::Conversion {
                ruta: ruta.clone(),
                motivo,
            },
            otro => otro,
        })?;
        fs::write(destino_glb, &nuevo).map_err(|e| RipperError::Io { ruta, fuente: e })?;
        if saltados > 0 {
            avisos.push(format!(
                "{saltados} nodo(s) raíz con matriz no recibieron la escala del ModelImporter"
            ));
        }
    }
    Ok((informe, avisos))
}

/// Aplica una escala uniforme a los nodos raíz de la escena de un GLB ya generado.
pub fn aplicar_escala_glb(glb: &[u8], factor: f32) -> Result<(Vec<u8>, usize), RipperError> {
    if factor.is_finite() && (factor - 1.0).abs() <= 1e-6 {
        return Ok((glb.to_vec(), 0));
    }
    if glb.len() < 12 {
        return Err(err_glb("GLB: cabecera incompleta"));
    }
    if glb.get(..4) != Some(MAGIC) {
        return Err(err_glb("GLB: cabecera mágica inválida (se espera \"glTF\")"));
    }
    let version = glb
        .get(4..8)
        .and_then(le32)
        .ok_or_else(|| err_glb("GLB: cabecera incompleta"))?;
    if version != 2 {
        return Err(err_glb(format!(
            "GLB: versión {version} no soportada (se espera 2)"
        )));
    }
    let total = glb
        .get(8..12)
        .and_then(le32)
        .ok_or_else(|| err_glb("GLB: cabecera incompleta"))? as usize;
    if total != glb.len() {
        return Err(err_glb(
            "GLB: la longitud de la cabecera no coincide con el archivo",
        ));
    }

    let mut pos = 12usize;
    let mut json: Option<&[u8]> = None;
    let mut bin: Option<&[u8]> = None;
    while pos < glb.len() {
        let resto = glb.get(pos..).ok_or_else(|| err_glb("GLB: trozo truncado"))?;
        let cabeza = resto.get(..8).ok_or_else(|| err_glb("GLB: trozo truncado"))?;
        let long = le32(cabeza).ok_or_else(|| err_glb("GLB: trozo truncado"))? as usize;
        let tipo = cabeza
            .get(4..8)
            .and_then(le32)
            .ok_or_else(|| err_glb("GLB: trozo truncado"))?;
        let datos = resto
            .get(8..)
            .and_then(|d| d.get(..long))
            .ok_or_else(|| err_glb("GLB: trozo truncado"))?;
        match tipo {
            TIPO_JSON if json.is_none() => json = Some(datos),
            TIPO_BIN if bin.is_none() => bin = Some(datos),
            _ => {}
        }
        pos += 8 + datos.len();
    }

    let json_bytes = json.ok_or_else(|| err_glb("GLB: falta el trozo JSON"))?;
    let mut doc: Value = serde_json::from_slice(json_bytes)
        .map_err(|e| err_glb(format!("GLB: JSON inválido: {e}")))?;

    let escena = doc.get("scene").and_then(Value::as_u64).unwrap_or(0);
    let raices: Vec<usize> = doc
        .get("scenes")
        .and_then(Value::as_array)
        .and_then(|s| s.get(escena as usize))
        .and_then(|e| e.get("nodes"))
        .and_then(Value::as_array)
        .map(|ns| {
            ns.iter()
                .filter_map(Value::as_u64)
                .map(|v| v as usize)
                .collect()
        })
        .unwrap_or_default();

    let mut saltados = 0usize;
    if let Some(nodos) = doc.get_mut("nodes").and_then(Value::as_array_mut) {
        for idx in raices {
            let Some(nodo) = nodos.get_mut(idx).and_then(Value::as_object_mut) else {
                continue;
            };
            if nodo.contains_key("matrix") {
                saltados += 1;
                continue;
            }
            match nodo.get_mut("scale").and_then(Value::as_array_mut) {
                Some(esc) => {
                    for v in esc.iter_mut() {
                        if let Some(x) = v.as_f64() {
                            *v = Value::from(escalar(x, factor));
                        }
                    }
                }
                None => {
                    let f = Value::from(limpio(factor));
                    let escala = Value::Array(vec![f.clone(), f.clone(), f]);
                    nodo.insert("scale".to_string(), escala);
                }
            }
        }
    }

    let mut json_pad = serde_json::to_vec(&doc)
        .map_err(|e| err_glb(format!("GLB: JSON no serializable: {e}")))?;
    while !json_pad.len().is_multiple_of(4) {
        json_pad.push(b' ');
    }

    let total_nuevo = 12u64
        + 8
        + json_pad.len() as u64
        + bin.map_or(0, |b| 8 + b.len() as u64);
    let total_nuevo = u32::try_from(total_nuevo)
        .map_err(|_| err_glb("GLB: el resultado excede el tamaño máximo"))?;

    let mut salida = Vec::with_capacity(total_nuevo as usize);
    salida.extend_from_slice(MAGIC);
    salida.extend_from_slice(&2u32.to_le_bytes());
    salida.extend_from_slice(&total_nuevo.to_le_bytes());
    salida.extend_from_slice(&(json_pad.len() as u32).to_le_bytes());
    salida.extend_from_slice(&TIPO_JSON.to_le_bytes());
    salida.extend_from_slice(&json_pad);
    if let Some(b) = bin {
        salida.extend_from_slice(&(b.len() as u32).to_le_bytes());
        salida.extend_from_slice(&TIPO_BIN.to_le_bytes());
        salida.extend_from_slice(b);
    }
    Ok((salida, saltados))
}

/// Error de conversión del GLB (la ruta del archivo la aporta quien llama).
fn err_glb(motivo: impl Into<String>) -> RipperError {
    RipperError::Conversion {
        ruta: "<glb>".to_string(),
        motivo: motivo.into(),
    }
}

/// Lee un `u32` little-endian de los primeros 4 bytes.
fn le32(b: &[u8]) -> Option<u32> {
    b.get(..4)
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .map(u32::from_le_bytes)
}

/// Redondea un f32 a su representación decimal corta en f64 (0.01, no 0.0099999997…).
fn limpio(v: f32) -> f64 {
    match v.to_string().parse::<f64>() {
        Ok(x) => x,
        Err(_) => f64::from(v),
    }
}

/// Multiplica una componente de escala conservando la legibilidad decimal.
fn escalar(x: f64, factor: f32) -> f64 {
    limpio((x * f64::from(factor)) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GLB mínimo con un nodo raíz sin `scale` y otro con `scale` [2,2,2].
    fn glb_de_prueba() -> Vec<u8> {
        glb_desde_json(
            r#"{"scene":0,"scenes":[{"nodes":[0,1]}],"nodes":[{"name":"a"},{"name":"b","scale":[2,2,2]}]}"#,
        )
    }

    fn glb_desde_json(json: &str) -> Vec<u8> {
        let mut j = json.as_bytes().to_vec();
        while !j.len().is_multiple_of(4) {
            j.push(b' ');
        }
        let bin = [1u8, 2, 3, 4];
        let total = 12 + 8 + j.len() + 8 + bin.len();
        let mut g = Vec::with_capacity(total);
        g.extend_from_slice(MAGIC);
        g.extend_from_slice(&2u32.to_le_bytes());
        g.extend_from_slice(&(total as u32).to_le_bytes());
        g.extend_from_slice(&(j.len() as u32).to_le_bytes());
        g.extend_from_slice(&TIPO_JSON.to_le_bytes());
        g.extend_from_slice(&j);
        g.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        g.extend_from_slice(&TIPO_BIN.to_le_bytes());
        g.extend_from_slice(&bin);
        g
    }

    fn trozo(glb: &[u8], tipo: u32) -> Option<Vec<u8>> {
        let mut pos = 12;
        while pos + 8 <= glb.len() {
            let n = le32(&glb[pos..]).expect("longitud") as usize;
            let t = le32(&glb[pos + 4..]).expect("tipo");
            let datos = glb.get(pos + 8..pos + 8 + n).expect("datos");
            if t == tipo {
                return Some(datos.to_vec());
            }
            pos += 8 + n;
        }
        None
    }

    fn json_de(glb: &[u8]) -> serde_json::Value {
        let j = trozo(glb, TIPO_JSON).expect("trozo JSON");
        serde_json::from_slice(&j).expect("json")
    }

    #[test]
    fn escala_raices_y_bin_intacto() {
        let original = glb_de_prueba();
        let (nuevo, saltados) = aplicar_escala_glb(&original, 0.01).expect("escala");
        assert_eq!(saltados, 0);
        let doc = json_de(&nuevo);
        let nodos = doc["nodes"].as_array().expect("nodos");
        let escala = |i: usize| -> Vec<f64> {
            nodos[i]["scale"]
                .as_array()
                .expect("scale")
                .iter()
                .map(|v| v.as_f64().expect("número"))
                .collect()
        };
        assert_eq!(escala(0), vec![0.01, 0.01, 0.01]);
        assert_eq!(escala(1), vec![0.02, 0.02, 0.02]);
        assert_eq!(le32(&nuevo[8..12]).expect("longitud") as usize, nuevo.len());
        assert_eq!(trozo(&nuevo, TIPO_BIN), Some(vec![1, 2, 3, 4]));
    }

    #[test]
    fn factor_uno_copia_identica() {
        let original = glb_de_prueba();
        let (nuevo, saltados) = aplicar_escala_glb(&original, 1.0).expect("escala");
        assert_eq!(nuevo, original);
        assert_eq!(saltados, 0);
    }

    #[test]
    fn cabecera_invalida_es_error() {
        let mut malo = glb_de_prueba();
        malo[0] = b'X';
        assert!(aplicar_escala_glb(&malo, 0.5).is_err());
        assert!(aplicar_escala_glb(&[], 0.5).is_err());
    }

    #[test]
    fn nodo_con_matriz_se_salta() {
        let glb = glb_desde_json(
            r#"{"scenes":[{"nodes":[0]}],"nodes":[{"matrix":[2,0,0,0,0,2,0,0,0,0,2,0,0,0,0,1]}]}"#,
        );
        let (nuevo, saltados) = aplicar_escala_glb(&glb, 0.5).expect("escala");
        assert_eq!(saltados, 1);
        let doc = json_de(&nuevo);
        assert!(doc["nodes"][0]["scale"].is_null());
        assert_eq!(
            doc["nodes"][0]["matrix"].as_array().expect("matriz").len(),
            16
        );
    }
}
