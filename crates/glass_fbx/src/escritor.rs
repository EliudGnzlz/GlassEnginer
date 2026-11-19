//! Escritor GLB (glTF 2.0 binario) a partir del IR de `glass_fbx`.
//!
//! Función pura: recibe una `EscenaIr` ya validada por el lector y devuelve
//! los bytes del `.glb` listo para el cargador del motor (crate `gltf`).

use serde_json::{json, Map, Value};

use crate::modelo::{CanalIr, EscenaIr, PrimitivaIr};

// Tipos de chunk GLB.
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;

// componentType de glTF.
const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;

// target de bufferView.
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

/// Acumulador del chunk BIN: bufferViews y accessors sobre un único buffer.
#[derive(Default)]
struct BinBuilder {
    bytes: Vec<u8>,
    vistas: Vec<Value>,
    accesores: Vec<Value>,
}

impl BinBuilder {
    /// Rellena con ceros hasta que el offset sea múltiplo de 4.
    fn alinear(&mut self) {
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
    }

    /// Escribe `datos` en un bufferView nuevo y devuelve su índice.
    fn push_vista(&mut self, datos: &[u8], target: Option<u32>) -> usize {
        self.alinear();
        let offset = self.bytes.len();
        self.bytes.extend_from_slice(datos);

        let mut vista = Map::new();
        vista.insert("buffer".into(), json!(0));
        vista.insert("byteOffset".into(), json!(offset));
        vista.insert("byteLength".into(), json!(datos.len()));
        if let Some(t) = target {
            vista.insert("target".into(), json!(t));
        }
        self.vistas.push(Value::Object(vista));
        self.vistas.len() - 1
    }

    /// Escribe `datos` en un bufferView nuevo con su accessor.
    /// Devuelve (bufferView, accessor).
    fn push(
        &mut self,
        datos: &[u8],
        count: usize,
        componente: u32,
        tipo: &str,
        target: Option<u32>,
        min_max: Option<(Vec<f32>, Vec<f32>)>,
    ) -> (usize, usize) {
        let vista = self.push_vista(datos, target);

        let mut acc = Map::new();
        acc.insert("bufferView".into(), json!(vista));
        acc.insert("byteOffset".into(), json!(0));
        acc.insert("componentType".into(), json!(componente));
        acc.insert("count".into(), json!(count));
        acc.insert("type".into(), json!(tipo));
        if let Some((min, max)) = min_max {
            acc.insert("min".into(), json!(min));
            acc.insert("max".into(), json!(max));
        }
        self.accesores.push(Value::Object(acc));
        (vista, self.accesores.len() - 1)
    }
}

// ---------------------------------------------------------------------------
// Conversión a bytes little-endian
// ---------------------------------------------------------------------------

fn bytes_f32(it: impl IntoIterator<Item = f32>) -> Vec<u8> {
    it.into_iter().flat_map(f32::to_le_bytes).collect()
}

fn bytes_u32(it: impl IntoIterator<Item = u32>) -> Vec<u8> {
    it.into_iter().flat_map(u32::to_le_bytes).collect()
}

fn bytes_u16(it: impl IntoIterator<Item = u16>) -> Vec<u8> {
    it.into_iter().flat_map(u16::to_le_bytes).collect()
}

/// Mínimo y máximo por componente de una lista de vectores.
fn min_max<const N: usize>(datos: &[[f32; N]]) -> (Vec<f32>, Vec<f32>) {
    let mut min = [f32::INFINITY; N];
    let mut max = [f32::NEG_INFINITY; N];
    for v in datos {
        for i in 0..N {
            min[i] = min[i].min(v[i]);
            max[i] = max[i].max(v[i]);
        }
    }
    (min.to_vec(), max.to_vec())
}

/// Mínimo y máximo de una lista escalar (tiempos de animación).
fn min_max_escalar(datos: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let min = datos.iter().copied().fold(f32::INFINITY, f32::min);
    let max = datos.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    (vec![min], vec![max])
}

// ---------------------------------------------------------------------------
// Piezas del JSON
// ---------------------------------------------------------------------------

fn escribir_primitiva(bin: &mut BinBuilder, prim: &PrimitivaIr, n_materiales: usize) -> Value {
    let n = prim.posiciones.len();
    let mut atributos = Map::new();

    // POSITION: min/max obligatorios según la spec.
    let (_, acc) = bin.push(
        &bytes_f32(prim.posiciones.iter().flatten().copied()),
        n,
        FLOAT,
        "VEC3",
        Some(ARRAY_BUFFER),
        Some(min_max(&prim.posiciones)),
    );
    atributos.insert("POSITION".into(), json!(acc));

    if n > 0 && prim.normales.len() == n {
        let (_, acc) = bin.push(
            &bytes_f32(prim.normales.iter().flatten().copied()),
            n,
            FLOAT,
            "VEC3",
            Some(ARRAY_BUFFER),
            None,
        );
        atributos.insert("NORMAL".into(), json!(acc));
    }

    if n > 0 && prim.uvs.len() == n {
        let (_, acc) = bin.push(
            &bytes_f32(prim.uvs.iter().flatten().copied()),
            n,
            FLOAT,
            "VEC2",
            Some(ARRAY_BUFFER),
            None,
        );
        atributos.insert("TEXCOORD_0".into(), json!(acc));
    }

    // JOINTS_0 y WEIGHTS_0 van siempre juntos.
    if n > 0 && prim.joints.len() == n && prim.weights.len() == n {
        let (_, acc) = bin.push(
            &bytes_u16(prim.joints.iter().flatten().copied()),
            n,
            UNSIGNED_SHORT,
            "VEC4",
            Some(ARRAY_BUFFER),
            None,
        );
        atributos.insert("JOINTS_0".into(), json!(acc));

        let (_, acc) = bin.push(
            &bytes_f32(prim.weights.iter().flatten().copied()),
            n,
            FLOAT,
            "VEC4",
            Some(ARRAY_BUFFER),
            None,
        );
        atributos.insert("WEIGHTS_0".into(), json!(acc));
    }

    let mut p = Map::new();
    p.insert("attributes".into(), Value::Object(atributos));

    if !prim.indices.is_empty() {
        let (_, acc) = bin.push(
            &bytes_u32(prim.indices.iter().copied()),
            prim.indices.len(),
            UNSIGNED_INT,
            "SCALAR",
            Some(ELEMENT_ARRAY_BUFFER),
            None,
        );
        p.insert("indices".into(), json!(acc));
    }

    if let Some(m) = prim.material.filter(|&m| m < n_materiales) {
        p.insert("material".into(), json!(m));
    }

    // `mode` omitido: 4 (triángulos) es el valor por defecto.
    Value::Object(p)
}

/// Emite los canales glTF de un canal del IR. Los tres caminos (TRS)
/// comparten un único accessor de tiempos.
fn escribir_canal(
    bin: &mut BinBuilder,
    canal: &CanalIr,
    canales: &mut Vec<Value>,
    samplers: &mut Vec<Value>,
) {
    let n = canal.tiempos.len();
    if n == 0 {
        return;
    }

    let mut salidas: Vec<(&str, &str, Vec<u8>)> = Vec::new();
    if let Some(t) = canal.traslaciones.as_ref().filter(|v| v.len() == n) {
        salidas.push(("translation", "VEC3", bytes_f32(t.iter().flatten().copied())));
    }
    if let Some(r) = canal.rotaciones.as_ref().filter(|v| v.len() == n) {
        salidas.push(("rotation", "VEC4", bytes_f32(r.iter().flatten().copied())));
    }
    if let Some(e) = canal.escalas.as_ref().filter(|v| v.len() == n) {
        salidas.push(("scale", "VEC3", bytes_f32(e.iter().flatten().copied())));
    }
    if salidas.is_empty() {
        return;
    }

    let (_, entrada) = bin.push(
        &bytes_f32(canal.tiempos.iter().copied()),
        n,
        FLOAT,
        "SCALAR",
        None,
        Some(min_max_escalar(&canal.tiempos)),
    );

    for (ruta, tipo, datos) in salidas {
        let (_, salida) = bin.push(&datos, n, FLOAT, tipo, None, None);
        samplers.push(json!({
            "input": entrada,
            "output": salida,
            "interpolation": "LINEAR",
        }));
        canales.push(json!({
            "sampler": samplers.len() - 1,
            "target": { "node": canal.nodo, "path": ruta },
        }));
    }
}

// ---------------------------------------------------------------------------
// Entrada pública
// ---------------------------------------------------------------------------

/// Serializa la escena IR como GLB (glTF 2.0 binario).
pub fn escribir_glb(escena: &EscenaIr) -> Vec<u8> {
    let mut bin = BinBuilder::default();

    let n_nodos = escena.nodos.len();
    let n_mallas = escena.mallas.len();
    let n_materiales = escena.materiales.len();
    let n_texturas = escena.texturas.len();

    let hay_piel = escena
        .piel
        .as_ref()
        .is_some_and(|p| !p.articulaciones.is_empty());

    // --- Imágenes y texturas (siempre incrustadas en el BIN) ---
    let mut imagenes = Vec::with_capacity(n_texturas);
    let mut texturas = Vec::with_capacity(n_texturas);
    for (i, tex) in escena.texturas.iter().enumerate() {
        let vista = bin.push_vista(&tex.bytes, None);
        imagenes.push(json!({
            "bufferView": vista,
            "mimeType": tex.mime,
            "name": tex.nombre,
        }));
        texturas.push(json!({
            "sampler": 0,
            "source": i,
            "name": tex.nombre,
        }));
    }

    // --- Materiales ---
    let materiales: Vec<Value> = escena
        .materiales
        .iter()
        .map(|m| {
            let mut pbr = Map::new();
            pbr.insert("baseColorFactor".into(), json!(m.color_base));
            pbr.insert("metallicFactor".into(), json!(m.metalico));
            pbr.insert("roughnessFactor".into(), json!(m.rugosidad));
            if let Some(t) = m.textura_base.filter(|&t| t < n_texturas) {
                // Índice de TEXTURA glTF (1:1 con `TexturaIr`).
                pbr.insert("baseColorTexture".into(), json!({ "index": t }));
            }

            let mut mat = Map::new();
            mat.insert("name".into(), json!(m.nombre));
            mat.insert("pbrMetallicRoughness".into(), Value::Object(pbr));
            if m.emision != [0.0, 0.0, 0.0] {
                mat.insert("emissiveFactor".into(), json!(m.emision));
            }
            if m.doble_cara {
                mat.insert("doubleSided".into(), json!(true));
            }
            Value::Object(mat)
        })
        .collect();

    // --- Mallas ---
    let mallas: Vec<Value> = escena
        .mallas
        .iter()
        .map(|malla| {
            let primitivas: Vec<Value> = malla
                .primitivas
                .iter()
                .map(|p| escribir_primitiva(&mut bin, p, n_materiales))
                .collect();
            json!({ "name": malla.nombre, "primitives": primitivas })
        })
        .collect();

    // --- Nodos (árbol completo con TRS local) ---
    let nodos: Vec<Value> = escena
        .nodos
        .iter()
        .map(|n| {
            let mut nodo = Map::new();
            nodo.insert("name".into(), json!(n.nombre));
            nodo.insert("translation".into(), json!(n.traslacion));
            nodo.insert("rotation".into(), json!(n.rotacion));
            nodo.insert("scale".into(), json!(n.escala));

            let hijos: Vec<usize> = n.hijos.iter().copied().filter(|&h| h < n_nodos).collect();
            if !hijos.is_empty() {
                nodo.insert("children".into(), json!(hijos));
            }

            if let Some(m) = n.malla.filter(|&m| m < n_mallas) {
                nodo.insert("mesh".into(), json!(m));
                if hay_piel && escena.mallas[m].con_piel {
                    nodo.insert("skin".into(), json!(0));
                }
            }
            Value::Object(nodo)
        })
        .collect();

    // --- Piel ---
    let mut pieles = Vec::new();
    if let Some(piel) = escena.piel.as_ref().filter(|_| hay_piel) {
        let (_, acc) = bin.push(
            &bytes_f32(piel.inversas.iter().flatten().copied()),
            piel.inversas.len(),
            FLOAT,
            "MAT4",
            None,
            None,
        );
        pieles.push(json!({
            "joints": piel.articulaciones,
            "inverseBindMatrices": acc,
            "name": "Piel",
        }));
    }

    // --- Animaciones ---
    let mut animaciones = Vec::new();
    for anim in &escena.animaciones {
        let mut canales = Vec::new();
        let mut samplers = Vec::new();
        for canal in anim.canales.iter().filter(|c| c.nodo < n_nodos) {
            escribir_canal(&mut bin, canal, &mut canales, &mut samplers);
        }
        if !canales.is_empty() {
            animaciones.push(json!({
                "name": anim.nombre,
                "channels": canales,
                "samplers": samplers,
            }));
        }
    }

    // --- Escena: raíces en orden ---
    let raices: Vec<usize> = escena
        .nodos
        .iter()
        .enumerate()
        .filter(|(_, n)| n.padre.is_none())
        .map(|(i, _)| i)
        .collect();

    // --- Raíz del JSON (los arreglos vacíos se omiten: minItems = 1) ---
    let longitud_bin = bin.bytes.len();
    let BinBuilder { bytes: bin_bytes, vistas, accesores } = bin;

    let mut raiz = Map::new();
    raiz.insert(
        "asset".into(),
        json!({ "version": "2.0", "generator": "glass_fbx" }),
    );
    raiz.insert("scene".into(), json!(0));
    raiz.insert(
        "scenes".into(),
        json!([{ "name": escena.nombre, "nodes": raices }]),
    );

    let mut insertar = |clave: &str, valores: Vec<Value>| {
        if !valores.is_empty() {
            raiz.insert(clave.into(), Value::Array(valores));
        }
    };
    insertar("nodes", nodos);
    insertar("meshes", mallas);
    insertar("materials", materiales);
    if !texturas.is_empty() {
        insertar(
            "samplers",
            vec![json!({
                "magFilter": 9729,
                "minFilter": 9987,
                "wrapS": 10497,
                "wrapT": 10497,
            })],
        );
    }
    insertar("textures", texturas);
    insertar("images", imagenes);
    insertar("skins", pieles);
    insertar("animations", animaciones);
    insertar("accessors", accesores);
    insertar("bufferViews", vistas);
    if longitud_bin > 0 {
        // byteLength sin el relleno final del chunk (permitido por la spec).
        insertar("buffers", vec![json!({ "byteLength": longitud_bin })]);
    }

    let json_bytes = Value::Object(raiz).to_string().into_bytes();
    empaquetar_glb(json_bytes, bin_bytes)
}

/// Monta el contenedor GLB: cabecera + chunk JSON + chunk BIN (si hay datos).
fn empaquetar_glb(mut json: Vec<u8>, mut bin: Vec<u8>) -> Vec<u8> {
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }

    let hay_bin = !bin.is_empty();
    let total = 12 + 8 + json.len() + if hay_bin { 8 + bin.len() } else { 0 };

    let mut glb = Vec::with_capacity(total);

    // Cabecera.
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total as u32).to_le_bytes());

    // Chunk 0: JSON.
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(&CHUNK_JSON.to_le_bytes());
    glb.extend_from_slice(&json);

    // Chunk 1: BIN.
    if hay_bin {
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&CHUNK_BIN.to_le_bytes());
        glb.extend_from_slice(&bin);
    }

    debug_assert_eq!(glb.len(), total);
    glb
}
