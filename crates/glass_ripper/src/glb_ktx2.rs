//! Reescritura a KTX2 de las imágenes incrustadas en un .glb.
//!
//! Se aplica tras `convertir_fbx`, que siempre incrusta las imágenes en el
//! BIN. Limitaciones registradas:
//! - El .glb no trae el rol Unity ni distingue normalTexture, así que TODAS
//!   las imágenes van a BC7 SRGB. Un mapa de normales dentro de un FBX queda
//!   en BC7 hasta que la conversión FBX registre roles. Los NormalMap
//!   sueltos sí van a BC5 (texturas_ktx2).
//! - Solo se reescribe un GLB con un único buffer interno (sin `uri`). Con
//!   cualquier otra forma se devuelve el original con un aviso.
//! - Los bytes del BIN que no cubre ningún bufferView se descartan al
//!   reconstruirlo.
//! - No se añade ninguna extensión al JSON: solo cambian `images`,
//!   `bufferViews` y `buffers`. El loader propio reconoce "image/ktx2".

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::compresor_texturas::comprimir_bc7;
use crate::texturas_ktx2::{ensamblar_ktx2, PERFIL_KTX2_IMPORT};

const MAGIA_GLB: u32 = 0x4654_6C67; // "glTF"
const VERSION_GLB: u32 = 2;
const CHUNK_JSON: u32 = 0x4E4F_534A; // "JSON"
const CHUNK_BIN: u32 = 0x004E_4942; // "BIN\0"

/// Contenido de un .glb ya separado en sus dos chunks.
struct GlbLeido<'a> {
    json: Value,
    bin: Option<&'a [u8]>,
}

/// Reescribe las imágenes de un .glb a KTX2 (BC7 SRGB, perfil Equilibrado).
/// Devuelve (glb_nuevo, avisos). Si no hay nada que reescribir, o el .glb no
/// tiene la forma esperada, devuelve el .glb ORIGINAL tal cual. El aviso
/// solo aparece cuando la reescritura no se pudo hacer. Una imagen que falla
/// queda en su formato original con un aviso que nombra su índice. Esta
/// función nunca falla el modelo.
pub fn reescribir_imagenes_ktx2(glb: &[u8]) -> (Vec<u8>, Vec<String>) {
    let mut avisos = Vec::new();
    match reescribir(glb, &mut avisos) {
        Ok(Some(nuevo)) => (nuevo, avisos),
        Ok(None) => (glb.to_vec(), avisos),
        Err(m) => {
            avisos.push(format!("imágenes del GLB no reescritas a KTX2: {m}"));
            (glb.to_vec(), avisos)
        }
    }
}

/// `Ok(None)`: no hay nada que reescribir (se conserva el original).
/// `Err`: el GLB no tiene la forma esperada (se conserva el original con aviso).
fn reescribir(glb: &[u8], avisos: &mut Vec<String>) -> Result<Option<Vec<u8>>, String> {
    let leido = leer_glb(glb)?;
    let mut json = leido.json;

    // Imágenes candidatas: (índice de imagen, índice de bufferView).
    let candidatas: Vec<(usize, usize)> = match json.get("images").and_then(Value::as_array) {
        None => return Ok(None),
        Some(imagenes) => imagenes
            .iter()
            .enumerate()
            .filter_map(|(i, img)| {
                let mime = img.get("mimeType")?.as_str()?;
                if mime != "image/png" && mime != "image/jpeg" {
                    return None;
                }
                let vista = usize::try_from(img.get("bufferView")?.as_u64()?).ok()?;
                Some((i, vista))
            })
            .collect(),
    };
    if candidatas.is_empty() {
        return Ok(None);
    }

    let bin = leido.bin.ok_or("el GLB no tiene chunk BIN")?;
    let vistas = leer_vistas(&json, bin.len())?;

    // Una misma vista puede compartirse entre imágenes: se comprime una vez.
    let mut reemplazos: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut vistas_fallidas: HashSet<usize> = HashSet::new();
    let mut imagenes_reescritas: Vec<usize> = Vec::new();
    for (imagen, vista) in candidatas {
        let Some(&(inicio, largo)) = vistas.get(vista) else {
            avisos.push(format!(
                "imagen {imagen}: el bufferView {vista} no existe; queda sin reescribir"
            ));
            continue;
        };
        if reemplazos.contains_key(&vista) {
            imagenes_reescritas.push(imagen);
            continue;
        }
        if vistas_fallidas.contains(&vista) {
            avisos.push(format!(
                "imagen {imagen}: comparte el bufferView {vista} con otra que falló; queda en su formato original"
            ));
            continue;
        }
        match comprimir_imagen(&bin[inicio..inicio + largo]) {
            Ok(ktx2) => {
                reemplazos.insert(vista, ktx2);
                imagenes_reescritas.push(imagen);
            }
            Err(m) => {
                vistas_fallidas.insert(vista);
                avisos.push(format!("imagen {imagen}: queda en su formato original ({m})"));
            }
        }
    }
    if reemplazos.is_empty() {
        return Ok(None);
    }

    // BIN nuevo: vistas en orden, cada una alineada a 4 con ceros.
    let mut nuevo_bin = Vec::with_capacity(bin.len());
    let mut nuevas_vistas: Vec<(usize, usize)> = Vec::with_capacity(vistas.len());
    for (i, &(inicio, largo)) in vistas.iter().enumerate() {
        let datos: &[u8] = match reemplazos.get(&i) {
            Some(ktx2) => ktx2,
            None => &bin[inicio..inicio + largo],
        };
        let nuevo_inicio = nuevo_bin.len();
        nuevo_bin.extend_from_slice(datos);
        while nuevo_bin.len() % 4 != 0 {
            nuevo_bin.push(0);
        }
        nuevas_vistas.push((nuevo_inicio, datos.len()));
    }

    if let Some(arr) = json.get_mut("bufferViews").and_then(Value::as_array_mut) {
        for (vista, &(inicio, largo)) in arr.iter_mut().zip(&nuevas_vistas) {
            vista["byteOffset"] = json!(inicio);
            vista["byteLength"] = json!(largo);
        }
    }
    json["buffers"][0]["byteLength"] = json!(nuevo_bin.len());
    if let Some(imagenes) = json.get_mut("images").and_then(Value::as_array_mut) {
        for &i in &imagenes_reescritas {
            imagenes[i]["mimeType"] = json!("image/ktx2");
        }
    }

    escribir_glb(&json, &nuevo_bin).map(Some)
}

/// Decodifica PNG/JPEG y comprime a BC7 SRGB en un contenedor KTX2.
fn comprimir_imagen(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let rgba = image::load_from_memory(bytes)
        .map_err(|e| format!("no decodifica: {e}"))?
        .to_rgba8();
    let (width, height) = rgba.dimensions();
    let (formato, niveles) = comprimir_bc7(rgba.as_raw(), width, height, true, PERFIL_KTX2_IMPORT)?;
    ensamblar_ktx2(formato, width, height, &niveles)
}

/// Valida el buffer único y devuelve (byteOffset, byteLength) de cada vista,
/// comprobando que todas caben dentro del BIN.
fn leer_vistas(json: &Value, largo_bin: usize) -> Result<Vec<(usize, usize)>, String> {
    let buffers = json
        .get("buffers")
        .and_then(Value::as_array)
        .ok_or("el JSON no tiene buffers")?;
    if buffers.len() != 1 {
        return Err(format!("se esperaba un único buffer y hay {}", buffers.len()));
    }
    if buffers[0].get("uri").and_then(Value::as_str).is_some() {
        return Err("el buffer 0 es externo (uri), no el BIN del GLB".to_string());
    }
    let vistas = json
        .get("bufferViews")
        .and_then(Value::as_array)
        .ok_or("el JSON no tiene bufferViews")?;
    vistas
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if !v.is_object() {
                return Err(format!("bufferView {i} no es un objeto"));
            }
            let buffer = v.get("buffer").and_then(Value::as_u64);
            if buffer != Some(0) {
                return Err(format!("bufferView {i} no apunta al buffer 0"));
            }
            let inicio = v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
            let largo = v
                .get("byteLength")
                .and_then(Value::as_u64)
                .ok_or(format!("bufferView {i} sin byteLength"))? as usize;
            match inicio.checked_add(largo) {
                Some(fin) if fin <= largo_bin => Ok((inicio, largo)),
                _ => Err(format!("bufferView {i} se sale del BIN")),
            }
        })
        .collect()
}

fn leer_u32(bytes: &[u8], inicio: usize) -> Option<u32> {
    let trozo = bytes.get(inicio..inicio.checked_add(4)?)?;
    Some(u32::from_le_bytes([trozo[0], trozo[1], trozo[2], trozo[3]]))
}

/// Separa un .glb en JSON parseado y BIN (si existe).
fn leer_glb(glb: &[u8]) -> Result<GlbLeido<'_>, String> {
    if leer_u32(glb, 0) != Some(MAGIA_GLB) {
        return Err("la cabecera no es glTF binario".to_string());
    }
    if leer_u32(glb, 4) != Some(VERSION_GLB) {
        return Err("versión de GLB distinta de 2".to_string());
    }
    let total = leer_u32(glb, 8).ok_or("cabecera GLB truncada")? as usize;
    if total > glb.len() || total < 12 {
        return Err("el largo declarado del GLB no cuadra con los bytes".to_string());
    }
    let glb = &glb[..total];
    let mut cursor = 12usize;
    let mut json = None;
    let mut bin = None;
    while cursor + 8 <= glb.len() {
        let largo = leer_u32(glb, cursor).ok_or("chunk truncado")? as usize;
        let tipo = leer_u32(glb, cursor + 4).ok_or("chunk truncado")?;
        let inicio = cursor + 8;
        let fin = inicio
            .checked_add(largo)
            .filter(|&f| f <= glb.len())
            .ok_or("un chunk se sale del GLB")?;
        let datos = &glb[inicio..fin];
        match tipo {
            CHUNK_JSON if json.is_none() => json = Some(datos),
            CHUNK_BIN if bin.is_none() => bin = Some(datos),
            _ => {}
        }
        cursor = fin;
    }
    let json = json.ok_or("el GLB no tiene chunk JSON")?;
    let json: Value =
        serde_json::from_slice(json).map_err(|e| format!("el JSON del GLB no parsea: {e}"))?;
    Ok(GlbLeido { json, bin })
}

/// Ensambla un .glb: JSON rellenado con espacios y BIN rellenado con ceros,
/// ambos a múltiplo de 4.
fn escribir_glb(json: &Value, bin: &[u8]) -> Result<Vec<u8>, String> {
    let mut json_bytes =
        serde_json::to_vec(json).map_err(|e| format!("no se pudo serializar el JSON: {e}"))?;
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let mut bin_bytes = bin.to_vec();
    while bin_bytes.len() % 4 != 0 {
        bin_bytes.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin_bytes.len();
    let total = u32::try_from(total).map_err(|_| "el GLB resultante supera 4 GiB".to_string())?;
    let mut salida = Vec::with_capacity(total as usize);
    salida.extend_from_slice(&MAGIA_GLB.to_le_bytes());
    salida.extend_from_slice(&VERSION_GLB.to_le_bytes());
    salida.extend_from_slice(&total.to_le_bytes());
    salida.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    salida.extend_from_slice(&CHUNK_JSON.to_le_bytes());
    salida.extend_from_slice(&json_bytes);
    salida.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
    salida.extend_from_slice(&CHUNK_BIN.to_le_bytes());
    salida.extend_from_slice(&bin_bytes);
    Ok(salida)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compresor_texturas::bytes_nivel_bc;
    use std::io::Cursor;

    fn png_rojo(lado: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(lado, lado, image::Rgba([255, 0, 0, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("codificar PNG de prueba");
        bytes
    }

    /// BIN de geometría: 3 posiciones vec3 f32 (36 B) + 3 índices u16 (6 B),
    /// relleno hasta 44 B.
    fn bin_geometria() -> Vec<u8> {
        let mut bin = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for i in [0u16, 1, 2] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        bin
    }

    /// GLB mínimo; con `imagen`, añade su bufferView 2, material, textura e imagen PNG.
    fn glb_minimo(imagen: Option<&[u8]>) -> (Vec<u8>, Value, Vec<u8>) {
        let mut bin = bin_geometria();
        let mut json = json!({
            "asset": { "version": "2.0" },
            "buffers": [{ "byteLength": 0 }],
            "bufferViews": [
                { "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 },
                { "buffer": 0, "byteOffset": 36, "byteLength": 6, "target": 34963 }
            ],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                  "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] },
                { "bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR" }
            ],
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1 }] }]
        });
        if let Some(datos) = imagen {
            let inicio = bin.len();
            bin.extend_from_slice(datos);
            json["bufferViews"]
                .as_array_mut()
                .unwrap()
                .push(json!({ "buffer": 0, "byteOffset": inicio, "byteLength": datos.len() }));
            json["meshes"][0]["primitives"][0]["material"] = json!(0);
            json["materials"] =
                json!([{ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } } }]);
            json["textures"] = json!([{ "source": 0 }]);
            json["images"] = json!([{ "bufferView": 2, "mimeType": "image/png" }]);
        }
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        json["buffers"][0]["byteLength"] = json!(bin.len());
        let glb = escribir_glb(&json, &bin).unwrap();
        (glb, json, bin)
    }

    fn bytes_vista<'a>(json: &Value, bin: &'a [u8], vista: usize) -> &'a [u8] {
        let v = &json["bufferViews"][vista];
        let inicio = v["byteOffset"].as_u64().unwrap_or(0) as usize;
        let largo = v["byteLength"].as_u64().unwrap() as usize;
        &bin[inicio..inicio + largo]
    }

    #[test]
    fn glb_sin_imagenes_no_se_toca() {
        let (glb, _, _) = glb_minimo(None);
        let (salida, avisos) = reescribir_imagenes_ktx2(&glb);
        assert_eq!(salida, glb);
        assert!(avisos.is_empty(), "avisos inesperados: {avisos:?}");
    }

    #[test]
    fn glb_reescribe_png_a_ktx2() {
        let (glb, json_orig, bin_orig) = glb_minimo(Some(&png_rojo(8)));
        let (salida, avisos) = reescribir_imagenes_ktx2(&glb);
        assert!(avisos.is_empty(), "avisos inesperados: {avisos:?}");
        assert_eq!(leer_u32(&salida, 8), Some(salida.len() as u32));

        let leido = leer_glb(&salida).expect("la salida debe ser un GLB válido");
        let json = &leido.json;
        let bin = leido.bin.expect("la salida debe tener BIN");
        assert_eq!(json["meshes"], json_orig["meshes"]);
        assert_eq!(json["accessors"], json_orig["accessors"]);
        assert_eq!(json["materials"], json_orig["materials"]);
        assert_eq!(json["images"][0]["mimeType"], "image/ktx2");
        assert_eq!(json["buffers"][0]["byteLength"].as_u64(), Some(bin.len() as u64));

        for vista in 0..2 {
            assert_eq!(bytes_vista(json, bin, vista), bytes_vista(&json_orig, &bin_orig, vista));
        }
        let reader = ktx2::Reader::new(bytes_vista(json, bin, 2)).expect("KTX2 válido");
        assert_eq!(reader.header().format, Some(ktx2::Format::BC7_SRGB_BLOCK));
        let nivel0 = reader.levels().next().expect("al menos un nivel");
        assert_eq!(nivel0.data.len(), bytes_nivel_bc(8, 8));
    }

    #[test]
    fn glb_imagen_rota_queda_en_png() {
        let (glb, _, _) = glb_minimo(Some(b"basura"));
        let (salida, avisos) = reescribir_imagenes_ktx2(&glb);
        assert!(avisos.iter().any(|a| a.contains("imagen 0")), "avisos: {avisos:?}");
        let leido = leer_glb(&salida).expect("la salida debe ser un GLB válido");
        assert_eq!(leido.json["images"][0]["mimeType"], "image/png");
        assert!(leer_vistas(&leido.json, leido.bin.unwrap().len()).is_ok());
    }
}
