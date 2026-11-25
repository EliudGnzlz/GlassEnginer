//! Conversión de materiales de Unity (`.mat`, ClassID 21) al
//! `MaterialData` del motor.
//!
//! El texto UnityYAML se recorre a mano sobre `serde_yaml::Value`:
//! Unity cambia campos entre versiones y aquí solo se leen los que
//! tienen equivalente en el motor. El módulo también expone, dentro
//! del crate, utilidades de lectura que reutiliza `fisico`.

use glass_core::scene::MaterialData;
use serde_yaml::Value;

use crate::RipperError;

/// GUID de los recursos extra integrados de Unity (shader Standard).
const GUID_RECURSOS_EXTRA: &str = "0000000000000000f000000000000000";
/// fileID del shader Standard dentro de los recursos extra.
const FILE_ID_STANDARD: i64 = 46;
/// GUID del shader Lit de URP.
const GUID_URP_LIT: &str = "933532a4fcc9baf4fa0491de14d08ed7";

/// Convierte el texto UnityYAML de un `.mat` (ClassID 21) al
/// `MaterialData` del motor. `resolver` traduce el GUID de una
/// textura a su ruta ya importada (relativa a la raíz del proyecto,
/// p. ej. "Assets/Texturas/madera.png"); si devuelve None, la
/// textura aún no está importada.
///
/// Devuelve el material y los avisos de conversión. Un shader sin mapa
/// de propiedades o un archivo sin documento `Material` son errores
/// `RipperError::Conversion`.
pub fn convertir_material(
    ruta: &str,
    texto: &str,
    resolver: &dyn Fn(&str) -> Option<String>,
) -> Result<(MaterialData, Vec<String>), RipperError> {
    let (material, mut avisos) = documento_raiz(ruta, texto, "Material")?;
    let shader = Shader::detectar(ruta, &material)?;
    let propiedades = Propiedades::leer(&material, &mut avisos);
    let mut datos = MaterialData::default();
    aplicar(&mut datos, shader, &propiedades, resolver, &mut avisos);
    Ok((datos, avisos))
}

/// Shaders de Unity con mapa de propiedades conocido.
#[derive(Clone, Copy)]
enum Shader {
    Standard,
    UrpLit,
}

impl Shader {
    /// Decide el shader a partir de `m_Shader` (guid y fileID).
    fn detectar(ruta: &str, material: &Value) -> Result<Self, RipperError> {
        let shader = material.get("m_Shader");
        let guid = shader
            .and_then(|s| s.get("guid"))
            .and_then(texto_escalar)
            .unwrap_or_default();
        let file_id = shader
            .and_then(|s| s.get("fileID"))
            .and_then(entero)
            .unwrap_or(0);
        if guid.eq_ignore_ascii_case(GUID_RECURSOS_EXTRA) && file_id == FILE_ID_STANDARD {
            Ok(Self::Standard)
        } else if guid.eq_ignore_ascii_case(GUID_URP_LIT) {
            Ok(Self::UrpLit)
        } else {
            Err(error_conversion(
                ruta,
                format!("shader sin mapa de propiedades: guid {guid} fileID {file_id}"),
            ))
        }
    }

    /// Color que se traduce a `albedo`.
    fn color_base(self) -> &'static str {
        match self {
            Self::Standard => "_Color",
            Self::UrpLit => "_BaseColor",
        }
    }

    /// Textura que se traduce a `texture`, `tiling` y `offset`.
    fn textura_base(self) -> &'static str {
        match self {
            Self::Standard => "_MainTex",
            Self::UrpLit => "_BaseMap",
        }
    }

    /// Float de suavidad; `roughness` = 1 − suavidad.
    fn suavidad(self) -> &'static str {
        match self {
            Self::Standard => "_Glossiness",
            Self::UrpLit => "_Smoothness",
        }
    }

    /// Float que decide si el material es transparente.
    fn modo(self) -> &'static str {
        match self {
            Self::Standard => "_Mode",
            Self::UrpLit => "_Surface",
        }
    }

    /// Standard: `_Mode` 2 (Fade) o 3 (Transparent) → transparente.
    /// URP Lit: `_Surface` 1 → transparente. Lo demás, opaco.
    fn transparente(self, valor: f32, avisos: &mut Vec<String>) -> bool {
        match (self, valor.round() as i64) {
            (_, 0) => false,
            (Self::Standard, 2 | 3) | (Self::UrpLit, 1) => true,
            (Self::Standard, 1) => {
                avisos.push(
                    "_Mode 1 (Cutout) sin equivalente en el motor: se importa como opaco"
                        .to_string(),
                );
                false
            }
            _ => {
                avisos.push(format!(
                    "{} con valor no reconocido ({valor}): se importa como opaco",
                    self.modo()
                ));
                false
            }
        }
    }
}

/// Propiedades guardadas del material, en el orden del archivo.
struct Propiedades {
    texturas: Vec<(String, Value)>,
    flotantes: Vec<(String, Value)>,
    colores: Vec<(String, Value)>,
}

impl Propiedades {
    fn leer(material: &Value, avisos: &mut Vec<String>) -> Self {
        let guardadas = material.get("m_SavedProperties");
        Self {
            texturas: leer_lista(guardadas, "m_TexEnvs", avisos),
            flotantes: leer_lista(guardadas, "m_Floats", avisos),
            colores: leer_lista(guardadas, "m_Colors", avisos),
        }
    }
}

/// Lee una lista de `m_SavedProperties` en cualquiera de sus formas:
/// secuencia de mapas de una clave, secuencia de `first`/`second`, o
/// mapa directo nombre → valor.
fn leer_lista(
    guardadas: Option<&Value>,
    lista: &str,
    avisos: &mut Vec<String>,
) -> Vec<(String, Value)> {
    let mut salida = Vec::new();
    match guardadas.and_then(|g| g.get(lista)) {
        None | Some(Value::Null) => {}
        Some(Value::Sequence(entradas)) => {
            for entrada in entradas {
                match entrada_propiedad(entrada) {
                    Some(propiedad) => salida.push(propiedad),
                    None => avisos.push(format!("entrada no reconocida en {lista}: se ignora")),
                }
            }
        }
        Some(Value::Mapping(mapa)) => {
            for (clave, valor) in mapa.iter() {
                match texto_escalar(clave) {
                    Some(nombre) => salida.push((nombre, valor.clone())),
                    None => avisos.push(format!("entrada no reconocida en {lista}: se ignora")),
                }
            }
        }
        Some(_) => avisos.push(format!("{lista} con formato no reconocido: se ignora")),
    }
    salida
}

/// Extrae (nombre, valor) de una entrada de propiedad. Con `first` y
/// `second`, el nombre es `first.name` (o `first` si es escalar); si
/// no, la entrada debe ser un mapa de una sola clave.
fn entrada_propiedad(entrada: &Value) -> Option<(String, Value)> {
    if let (Some(primero), Some(segundo)) = (entrada.get("first"), entrada.get("second")) {
        let nombre = match primero.get("name") {
            Some(nombre) => texto_escalar(nombre)?,
            None => texto_escalar(primero)?,
        };
        return Some((nombre, segundo.clone()));
    }
    match entrada {
        Value::Mapping(mapa) if mapa.len() == 1 => {
            let (clave, valor) = mapa.iter().next()?;
            Some((texto_escalar(clave)?, valor.clone()))
        }
        _ => None,
    }
}

/// Aplica el mapa de propiedades del shader sobre `datos`.
fn aplicar(
    datos: &mut MaterialData,
    shader: Shader,
    propiedades: &Propiedades,
    resolver: &dyn Fn(&str) -> Option<String>,
    avisos: &mut Vec<String>,
) {
    for (nombre, entorno) in &propiedades.texturas {
        let nombre = nombre.as_str();
        if nombre == shader.textura_base() {
            aplicar_textura(datos, nombre, entorno, resolver, avisos);
        } else {
            avisos.push(sin_equivalente(nombre));
        }
    }

    for (nombre, valor) in &propiedades.flotantes {
        let nombre = nombre.as_str();
        if !["_Metallic", shader.suavidad(), shader.modo()].contains(&nombre) {
            avisos.push(sin_equivalente(nombre));
            continue;
        }
        let Some(x) = numero(valor) else {
            avisos.push(format!("la propiedad {nombre} no es numérica: se ignora"));
            continue;
        };
        if nombre == "_Metallic" {
            datos.metallic = x.clamp(0.0, 1.0);
        } else if nombre == shader.suavidad() {
            datos.roughness = (1.0 - x).clamp(0.0, 1.0);
        } else {
            datos.transparent = shader.transparente(x, avisos);
        }
    }

    for (nombre, valor) in &propiedades.colores {
        let nombre = nombre.as_str();
        let es_base = nombre == shader.color_base();
        if !es_base && nombre != "_EmissionColor" {
            avisos.push(sin_equivalente(nombre));
            continue;
        }
        let Some(rgba) = color(valor) else {
            avisos.push(format!("el color {nombre} tiene un formato no reconocido: se ignora"));
            continue;
        };
        if es_base {
            datos.albedo = rgba;
        } else {
            let (rgb, intensidad) = emision([rgba[0], rgba[1], rgba[2]]);
            datos.emissive = rgb;
            datos.emissive_intensity = intensidad;
        }
    }
}

/// Traduce el entorno de la textura principal: `m_Scale` → `tiling`,
/// `m_Offset` → `offset` (mismo convenio en Unity y en el motor) y
/// `m_Texture` → `texture` mediante el resolvedor.
fn aplicar_textura(
    datos: &mut MaterialData,
    nombre: &str,
    entorno: &Value,
    resolver: &dyn Fn(&str) -> Option<String>,
    avisos: &mut Vec<String>,
) {
    if let Some(escala) = entorno.get("m_Scale").and_then(vec2) {
        datos.tiling = escala;
    }
    if let Some(desplazamiento) = entorno.get("m_Offset").and_then(vec2) {
        datos.offset = desplazamiento;
    }
    let textura = entorno.get("m_Texture");
    let file_id = textura
        .and_then(|t| t.get("fileID"))
        .and_then(entero)
        .unwrap_or(0);
    let guid = textura
        .and_then(|t| t.get("guid"))
        .and_then(texto_escalar)
        .unwrap_or_default();
    // fileID 0 o GUID vacío/nulo: la ranura no tiene textura asignada.
    if file_id == 0 || guid.bytes().all(|b| b == b'0') {
        return;
    }
    match resolver(&guid) {
        Some(ruta) => datos.texture = relativa_a_assets(&ruta),
        None => {
            avisos.push(format!(
                "la textura {nombre} (guid {guid}) aún no está importada"
            ));
            datos.texture = String::new();
        }
    }
}

/// Pasa una ruta relativa al proyecto ("Assets/...") a relativa a
/// `assets/`.
fn relativa_a_assets(ruta: &str) -> String {
    let ruta = ruta.replace('\\', "/");
    ruta.strip_prefix("Assets/")
        .unwrap_or(ruta.as_str())
        .to_string()
}

/// Emisión: con alguna componente > 1 (HDR), la intensidad es la
/// componente mayor y el rgb se normaliza por ella; si no, rgb tal cual
/// e intensidad 1 (0 si el color es negro).
fn emision(rgb: [f32; 3]) -> ([f32; 3], f32) {
    let maximo = rgb.iter().copied().fold(0.0_f32, f32::max);
    if maximo > 1.0 {
        (rgb.map(|c| c / maximo), maximo)
    } else {
        let negro = rgb.iter().all(|c| c.abs() <= f32::EPSILON);
        (rgb, if negro { 0.0 } else { 1.0 })
    }
}

fn sin_equivalente(nombre: &str) -> String {
    format!("propiedad sin equivalente en el material del motor: {nombre}")
}

/// Lee `{r, g, b, a}`; las componentes ausentes valen 0 (a = 1).
fn color(valor: &Value) -> Option<[f32; 4]> {
    if !matches!(valor, Value::Mapping(_)) {
        return None;
    }
    let componente = |clave: &str, defecto: f32| {
        valor.get(clave).and_then(numero).unwrap_or(defecto)
    };
    Some([
        componente("r", 0.0),
        componente("g", 0.0),
        componente("b", 0.0),
        componente("a", 1.0),
    ])
}

/// Lee `{x, y}`; exige ambas componentes.
fn vec2(valor: &Value) -> Option<[f32; 2]> {
    Some([numero(valor.get("x")?)?, numero(valor.get("y")?)?])
}

/// Texto de un escalar YAML (cadena, número o booleano).
fn texto_escalar(valor: &Value) -> Option<String> {
    match valor {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Coerción tolerante a `f32`: enteros, flotantes o cadenas numéricas.
/// Los valores no finitos se descartan.
pub(crate) fn numero(valor: &Value) -> Option<f32> {
    let x = match valor {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }?;
    x.is_finite().then_some(x as f32)
}

/// Coerción tolerante a entero (los flotantes se redondean).
pub(crate) fn entero(valor: &Value) -> Option<i64> {
    match valor {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|x| x.round() as i64)),
        Value::String(s) => {
            let s = s.trim();
            s.parse::<i64>()
                .ok()
                .or_else(|| s.parse::<f64>().ok().map(|x| x.round() as i64))
        }
        _ => None,
    }
}

pub(crate) fn error_conversion(ruta: &str, motivo: impl Into<String>) -> RipperError {
    RipperError::Conversion {
        ruta: ruta.to_string(),
        motivo: motivo.into(),
    }
}

/// Busca, en un archivo UnityYAML posiblemente multidocumento, el
/// documento cuyo mapa raíz tiene la clave `clave` y devuelve su valor.
pub(crate) fn documento_raiz(
    ruta: &str,
    texto: &str,
    clave: &str,
) -> Result<(Value, Vec<String>), RipperError> {
    let texto = entrecomillar_guids(texto.trim_start_matches('\u{feff}'));
    let mut error_yaml: Option<String> = None;
    for trozo in trocear_documentos(&texto) {
        match serde_yaml::from_str::<Value>(&trozo) {
            Ok(documento) => {
                if let Some(valor) = documento.get(clave) {
                    return Ok((valor.clone(), Vec::new()));
                }
            }
            Err(e) => {
                // Unity escribe a veces la misma clave dos veces en un
                // mapa (p. ej. varias `m_Texture` en un entorno de
                // textura) y serde_yaml lo rechaza; su lector lo
                // tolera. Se reintenta conservando la primera
                // aparición de cada clave duplicada.
                if e.to_string().contains("duplicate entry") {
                    let (depurado, quitadas) = deduplicar_claves(&trozo);
                    if quitadas > 0 {
                        if let Ok(documento) = serde_yaml::from_str::<Value>(&depurado) {
                            if let Some(valor) = documento.get(clave) {
                                let aviso = format!(
                                    "el YAML trae {quitadas} clave(s) duplicada(s) en un mismo mapa: se conserva la primera aparición de cada una"
                                );
                                return Ok((valor.clone(), vec![aviso]));
                            }
                        }
                    }
                }
                if error_yaml.is_none() {
                    error_yaml = Some(e.to_string());
                }
            }
        }
    }
    let motivo = match error_yaml {
        Some(e) => format!("no se encontró el documento {clave} (YAML inválido: {e})"),
        None => format!("no se encontró el documento {clave}"),
    };
    Err(error_conversion(ruta, motivo))
}

/// Quita las apariciones posteriores de claves repetidas dentro de
/// un mismo mapa en bloque (la segunda `m_Texture:` y su sub-bloque),
/// conservando la primera. Devuelve el texto y cuántas se quitaron.
/// Los mapas en flujo (`{...}`) no se tocan.
fn deduplicar_claves(texto: &str) -> (String, usize) {
    use std::collections::HashSet;
    // Marcos de mapas en bloque abiertos: (indentación de la clave,
    // claves ya vistas en ese mapa).
    let mut marcos: Vec<(usize, HashSet<String>)> = Vec::new();
    let mut salida: Vec<&str> = Vec::new();
    let mut quitadas = 0usize;
    // Si > 0, estamos descartando el sub-bloque de una clave
    // duplicada: se saltan las líneas más indentadas que ese valor.
    let mut descartando_desde: Option<usize> = None;

    for linea in texto.lines() {
        let indent = linea.len() - linea.trim_start_matches(' ').len();
        let contenido = linea.trim_start();
        if contenido.is_empty() || contenido.starts_with('#') {
            if descartando_desde.is_none() {
                salida.push(linea);
            }
            continue;
        }
        if let Some(corte) = descartando_desde {
            if indent > corte {
                continue;
            }
            descartando_desde = None;
        }
        // Al bajar la indentación se cierran los mapas más profundos.
        while marcos.last().is_some_and(|(i, _)| *i > indent) {
            marcos.pop();
        }
        // Entrada de secuencia: el ítem abre un mapa nuevo para sus
        // claves (a indentación + 2) y cierra los del ítem anterior.
        let (efectiva, resto) = if let Some(r) = contenido.strip_prefix("- ") {
            while marcos.last().is_some_and(|(i, _)| *i > indent + 2) {
                marcos.pop();
            }
            (indent + 2, r)
        } else if contenido == "-" {
            (indent + 2, "")
        } else {
            (indent, contenido)
        };
        if let Some(clave) = clave_de_linea(resto) {
            if marcos.last().is_some_and(|(i, _)| *i < efectiva) || marcos.is_empty() {
                marcos.push((efectiva, HashSet::new()));
            }
            if let Some((i, vistas)) = marcos.last_mut() {
                if *i == efectiva && !vistas.insert(clave) {
                    quitadas += 1;
                    descartando_desde = Some(indent);
                    continue;
                }
            }
        }
        salida.push(linea);
    }
    (salida.join("\n"), quitadas)
}

/// La clave de una línea `clave: ...` o `clave:` si la hay; las
/// claves de UnityYAML son identificadores simples.
fn clave_de_linea(resto: &str) -> Option<String> {
    let pos = resto.find(':')?;
    let clave = &resto[..pos];
    let tras = &resto[pos..];
    let clave_valida = !clave.is_empty()
        && clave
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if clave_valida && (tras == ":" || tras.starts_with(": ")) {
        Some(clave.to_string())
    } else {
        None
    }
}

/// Trocea por las líneas `--- !u!<class> &<fileID>` y descarta las
/// directivas `%YAML`/`%TAG`, cuyas etiquetas no interesan aquí.
fn trocear_documentos(texto: &str) -> Vec<String> {
    let mut trozos = Vec::new();
    let mut actual = String::new();
    for linea in texto.lines() {
        if linea.starts_with("---") {
            if !actual.trim().is_empty() {
                trozos.push(std::mem::take(&mut actual));
            }
            actual.clear();
            continue;
        }
        if linea.starts_with('%') {
            continue;
        }
        actual.push_str(linea);
        actual.push('\n');
    }
    if !actual.trim().is_empty() {
        trozos.push(actual);
    }
    trozos
}

/// Entrecomilla los valores `guid: <hex>` para que YAML no lea como
/// número un GUID de solo dígitos (perdería ceros y precisión).
fn entrecomillar_guids(texto: &str) -> String {
    const CLAVE: &str = "guid: ";
    let mut salida = String::with_capacity(texto.len() + 32);
    let mut resto = texto;
    while let Some(posicion) = resto.find(CLAVE) {
        let (antes, despues) = resto.split_at(posicion + CLAVE.len());
        salida.push_str(antes);
        let fin = despues
            .find(|c: char| !c.is_ascii_hexdigit())
            .unwrap_or(despues.len());
        if fin > 0 {
            salida.push('"');
            salida.push_str(&despues[..fin]);
            salida.push('"');
        }
        resto = &despues[fin..];
    }
    salida.push_str(resto);
    salida
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUID_MADERA: &str = "8a3f0c1d2e4b5a6978c0d1e2f3a4b5c6";

    fn resolver_prueba(guid: &str) -> Option<String> {
        match guid {
            GUID_MADERA => Some("Assets/Texturas/madera.png".to_string()),
            "c0ffee00c0ffee00c0ffee00c0ffee00" => Some("Assets/Cristal.png".to_string()),
            _ => None,
        }
    }

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

    fn cerca_n(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| cerca(*x, *y))
    }

    fn tiene(avisos: &[String], texto: &str) -> bool {
        avisos.contains(&texto.to_owned())
    }

    fn comprobar_madera(m: &MaterialData) {
        assert!(cerca_n(&m.albedo, &[1.0, 0.5, 0.25, 1.0]));
        assert_eq!(m.texture, "Texturas/madera.png");
        assert!(cerca_n(&m.tiling, &[2.0, 2.0]));
        assert!(cerca_n(&m.offset, &[0.25, 0.5]));
        assert!(cerca(m.metallic, 0.5));
        assert!(cerca(m.roughness, 0.25));
        assert!(cerca_n(&m.emissive, &[0.0, 0.0, 0.0]));
        assert!(cerca(m.emissive_intensity, 0.0));
        assert!(!m.transparent);
    }

    const STANDARD_ACTUAL: &str = r#"%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!114 &3
MonoBehaviour:
  m_Name: Ajeno
--- !u!21 &2100000
Material:
  serializedVersion: 6
  m_Name: Madera
  m_Shader: {fileID: 46, guid: 0000000000000000f000000000000000, type: 0}
  m_ShaderKeywords:
  m_SavedProperties:
    serializedVersion: 3
    m_TexEnvs:
    - _MainTex:
        m_Texture: {fileID: 2800000, guid: 8a3f0c1d2e4b5a6978c0d1e2f3a4b5c6, type: 3}
        m_Scale: {x: 2, y: 2}
        m_Offset: {x: 0.25, y: 0.5}
    m_Floats:
    - _Glossiness: 0.75
    - _Metallic: 0.5
    - _Mode: 0
    m_Colors:
    - _Color: {r: 1, g: 0.5, b: 0.25, a: 1}
    - _EmissionColor: {r: 0, g: 0, b: 0, a: 1}
"#;

    const STANDARD_V2: &str = r#"%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!21 &2100000
Material:
  serializedVersion: 3
  m_Name: Madera
  m_Shader: {fileID: 46, guid: 0000000000000000f000000000000000, type: 0}
  m_SavedProperties:
    serializedVersion: 2
    m_TexEnvs:
    - first:
        name: _MainTex
      second:
        m_Texture: {fileID: 2800000, guid: 8a3f0c1d2e4b5a6978c0d1e2f3a4b5c6, type: 3}
        m_Scale: {x: 2, y: 2}
        m_Offset: {x: 0.25, y: 0.5}
    m_Floats:
    - first:
        name: _Glossiness
      second: 0.75
    - first:
        name: _Metallic
      second: 0.5
    - first:
        name: _Mode
      second: 0
    m_Colors:
    - first:
        name: _Color
      second: {r: 1, g: 0.5, b: 0.25, a: 1}
    - first:
        name: _EmissionColor
      second: {r: 0, g: 0, b: 0, a: 1}
"#;

    #[test]
    fn standard_forma_actual() {
        let (m, avisos) = exito(convertir_material("Madera.mat", STANDARD_ACTUAL, &resolver_prueba));
        comprobar_madera(&m);
        assert!(avisos.is_empty(), "{avisos:?}");
    }

    #[test]
    fn standard_forma_first_second_da_lo_mismo() {
        let (actual, _) = exito(convertir_material("Madera.mat", STANDARD_ACTUAL, &resolver_prueba));
        let (v2, avisos) = exito(convertir_material("Madera.mat", STANDARD_V2, &resolver_prueba));
        comprobar_madera(&v2);
        assert!(avisos.is_empty(), "{avisos:?}");
        assert!(cerca_n(&actual.albedo, &v2.albedo));
        assert_eq!(actual.texture, v2.texture);
        assert!(cerca_n(&actual.tiling, &v2.tiling));
        assert!(cerca_n(&actual.offset, &v2.offset));
        assert!(cerca(actual.metallic, v2.metallic));
        assert!(cerca(actual.roughness, v2.roughness));
        assert!(cerca_n(&actual.emissive, &v2.emissive));
        assert!(cerca(actual.emissive_intensity, v2.emissive_intensity));
        assert_eq!(actual.transparent, v2.transparent);
    }

    #[test]
    fn urp_lit_transparente() {
        let texto = r#"--- !u!21 &2100000
Material:
  m_Name: Cristal
  m_Shader: {fileID: 4800000, guid: 933532a4fcc9baf4fa0491de14d08ed7, type: 3}
  m_SavedProperties:
    serializedVersion: 3
    m_TexEnvs:
    - _BaseMap:
        m_Texture: {fileID: 2800000, guid: c0ffee00c0ffee00c0ffee00c0ffee00, type: 3}
        m_Scale: {x: 3, y: 1}
        m_Offset: {x: 0, y: 0.5}
    m_Floats:
    - _Metallic: 0
    - _Smoothness: 0.9
    - _Surface: 1
    m_Colors:
    - _BaseColor: {r: 0.2, g: 0.4, b: 0.6, a: 0.5}
"#;
        let (m, avisos) = exito(convertir_material("Cristal.mat", texto, &resolver_prueba));
        assert!(cerca_n(&m.albedo, &[0.2, 0.4, 0.6, 0.5]));
        assert_eq!(m.texture, "Cristal.png");
        assert!(cerca_n(&m.tiling, &[3.0, 1.0]));
        assert!(cerca_n(&m.offset, &[0.0, 0.5]));
        assert!(cerca(m.metallic, 0.0));
        assert!(cerca(m.roughness, 0.1));
        assert!(m.transparent);
        assert!(avisos.is_empty(), "{avisos:?}");
    }

    #[test]
    fn emision_hdr() {
        let texto = r#"Material:
  m_Shader: {fileID: 46, guid: 0000000000000000f000000000000000, type: 0}
  m_SavedProperties:
    m_Colors:
    - _EmissionColor: {r: 2, g: 1, b: 0.5, a: 1}
"#;
        let (m, avisos) = exito(convertir_material("Lava.mat", texto, &resolver_prueba));
        assert!(cerca(m.emissive_intensity, 2.0));
        assert!(cerca_n(&m.emissive, &[1.0, 0.5, 0.25]));
        assert!(avisos.is_empty(), "{avisos:?}");
    }

    #[test]
    fn shader_desconocido_es_error() {
        let texto = r#"Material:
  m_Shader: {fileID: 4800000, guid: 1234abcd1234abcd1234abcd1234abcd, type: 3}
"#;
        match convertir_material("Raro.mat", texto, &resolver_prueba) {
            Err(RipperError::Conversion { ruta, motivo }) => {
                assert_eq!(ruta, "Raro.mat");
                assert_eq!(
                    motivo,
                    "shader sin mapa de propiedades: guid 1234abcd1234abcd1234abcd1234abcd fileID 4800000"
                );
            }
            _ => panic!("se esperaba error de conversión"),
        }
        // Mismo GUID que Standard pero otro fileID (p. ej. Specular).
        let especular = r#"Material:
  m_Shader: {fileID: 45, guid: 0000000000000000f000000000000000, type: 0}
"#;
        assert!(matches!(
            convertir_material("Especular.mat", especular, &resolver_prueba),
            Err(RipperError::Conversion { .. })
        ));
    }

    #[test]
    fn propiedades_ajenas_avisan() {
        let texto = r#"Material:
  m_Shader: {fileID: 46, guid: 0000000000000000f000000000000000, type: 0}
  m_SavedProperties:
    m_TexEnvs:
    - _BumpMap:
        m_Texture: {fileID: 0}
        m_Scale: {x: 1, y: 1}
        m_Offset: {x: 0, y: 0}
    - _OcclusionMap:
        m_Texture: {fileID: 0}
        m_Scale: {x: 1, y: 1}
        m_Offset: {x: 0, y: 0}
    m_Floats:
    - _BumpScale: 1
"#;
        let (_, avisos) = exito(convertir_material("Roca.mat", texto, &resolver_prueba));
        assert!(tiene(&avisos, "propiedad sin equivalente en el material del motor: _BumpMap"));
        assert!(tiene(&avisos, "propiedad sin equivalente en el material del motor: _OcclusionMap"));
        assert!(tiene(&avisos, "propiedad sin equivalente en el material del motor: _BumpScale"));
    }

    #[test]
    fn textura_no_resuelta_avisa_y_queda_vacia() {
        // GUID de solo dígitos: debe llegar intacto al resolvedor y al aviso.
        let texto = r#"Material:
  m_Shader: {fileID: 46, guid: 0000000000000000f000000000000000, type: 0}
  m_SavedProperties:
    m_TexEnvs:
    - _MainTex:
        m_Texture: {fileID: 2800000, guid: 12345678901234567890123456789012, type: 3}
        m_Scale: {x: 1, y: 1}
        m_Offset: {x: 0, y: 0}
"#;
        let (m, avisos) = exito(convertir_material("Sin.mat", texto, &resolver_prueba));
        assert_eq!(m.texture, "");
        assert!(tiene(
            &avisos,
            "la textura _MainTex (guid 12345678901234567890123456789012) aún no está importada"
        ));
    }

    #[test]
    fn sin_documento_material_es_error() {
        match convertir_material("X.mat", "MonoBehaviour:\n  m_Name: X\n", &resolver_prueba) {
            Err(RipperError::Conversion { motivo, .. }) => {
                assert!(motivo.contains("no se encontró el documento Material"));
            }
            _ => panic!("se esperaba error de conversión"),
        }
    }

    #[test]
    fn claves_duplicadas_se_toleran_conservando_la_primera() {
        // Caso real (Synty Polygon Kaiju, Kaiju_02_01_A.mat): Unity
        // escribe varias `m_Texture` en el mismo entorno; serde_yaml
        // lo rechaza y el conversor reintenta sin las repetidas.
        let texto = r#"Material:
  m_Shader: {fileID: 46, guid: 0000000000000000f000000000000000, type: 0}
  m_SavedProperties:
    m_TexEnvs:
    - _MainTex:
        m_Texture: {fileID: 2800000, guid: 8a3f0c1d2e4b5a6978c0d1e2f3a4b5c6, type: 3}
        m_Texture: {fileID: 2800000, guid: c0ffee00c0ffee00c0ffee00c0ffee00, type: 3}
        m_Scale: {x: 1, y: 1}
        m_Offset: {x: 0, y: 0}
"#;
        let (m, avisos) = exito(convertir_material("Dup.mat", texto, &resolver_prueba));
        assert_eq!(m.texture, "Texturas/madera.png");
        assert!(avisos.iter().any(|a| a.contains("duplicada")));
    }
}
