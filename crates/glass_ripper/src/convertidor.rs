//! Convertidor de scripts C# de Unity a la fachada del motor (v1).
//!
//! Por cada script hay tres salidas posibles por elemento:
//! - traducción directa: la fachada tiene el mismo nombre y el código no se toca;
//! - reescritura segura: en v1 solo la del preprocesador (ramas resueltas);
//! - marca: lo que el motor no tiene. Una marca bloqueante manda el script
//!   a cuarentena (`codigo: None`).
//!
//! La detección se hace sobre el texto con comentarios y cadenas
//! enmascarados, de modo que nada dentro de ellos cuenta como uso.

/// Marca del informe asociada a un script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarcaScript {
    /// Línea en el texto ORIGINAL (1-based); 0 = sin línea.
    pub linea: usize,
    /// Elemento detectado, p. ej. `"StartCoroutine"`.
    pub elemento: String,
    /// Explicación para el informe.
    pub motivo: String,
    /// `true` = el script va a cuarentena.
    pub bloqueante: bool,
}

/// Resultado de convertir un script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultadoScript {
    /// `None` = no entra como `.cs`.
    pub codigo: Option<String>,
    /// Marcas (avisos y bloqueantes), ordenadas por línea.
    pub marcas: Vec<MarcaScript>,
    /// `true` si es un script de editor (no se convierte).
    pub es_editor: bool,
}

const MOTIVO_BUSQUEDA: &str = "búsqueda por tipo en la escena: sin equivalente en v1";
const MOTIVO_2D: &str = "2D: el motor no tiene modo ni física 2D";
const MOTIVO_GET_FORMA: &str =
    "no hay clase por forma; el motor usa el Collider genérico y los Attach del cuerpo";
const MOTIVO_ADD_FORMA: &str =
    "no hay clase por forma; el equivalente manual es el Attach… del cuerpo rígido";

/// Identificadores que bloquean por sí solos (elemento, motivo).
const DIRECTOS: &[(&str, &str)] = &[
    ("FindObjectOfType", MOTIVO_BUSQUEDA),
    ("FindObjectsOfType", MOTIVO_BUSQUEDA),
    ("FindFirstObjectByType", MOTIVO_BUSQUEDA),
    ("FindAnyObjectByType", MOTIVO_BUSQUEDA),
    // Misma familia, tampoco existe en la fachada.
    ("FindObjectsByType", MOTIVO_BUSQUEDA),
    ("Rigidbody2D", MOTIVO_2D),
    ("Collider2D", MOTIVO_2D),
    ("BoxCollider2D", MOTIVO_2D),
    ("CircleCollider2D", MOTIVO_2D),
    ("CapsuleCollider2D", MOTIVO_2D),
    ("PolygonCollider2D", MOTIVO_2D),
    ("Physics2D", MOTIVO_2D),
];

/// Clases de collider por forma que la fachada no tiene.
const FORMAS: &[&str] = &["SphereCollider", "CapsuleCollider", "MeshCollider"];

/// Métodos genéricos de obtención de componentes (motivo «Get»).
const OBTENER: &[&str] = &[
    "GetComponent",
    "GetComponents",
    "GetComponentInChildren",
    "GetComponentsInChildren",
    "GetComponentInParent",
    "GetComponentsInParent",
    "TryGetComponent",
];

/// Convierte un script C# de Unity según el contrato U4.
///
/// Un `using UnityEditor` en una rama ACTIVA tras el preprocesador
/// clasifica el script como de editor; si solo aparece dentro de una
/// rama `#if UNITY_EDITOR` eliminada, el script es de runtime y se
/// convierte sin esa rama.
pub fn convertir_script(ruta_relativa: &str, texto: &str) -> ResultadoScript {
    if ruta_relativa.split(['/', '\\']).any(|s| s == "Editor") {
        return resultado_editor(
            0,
            "Editor",
            "script de editor (carpeta Editor): no se convierte al motor",
        );
    }

    let (lineas, salto_final) = partir_lineas(texto);

    let pre = match preprocesar(&lineas) {
        Ok(p) => p,
        Err(m) => {
            return ResultadoScript {
                codigo: None,
                marcas: vec![m],
                es_editor: false,
            }
        }
    };

    // Ajuste al integrar (Rayo): el `using UnityEditor` se busca en
    // las líneas EFECTIVAS tras el preprocesador, no en el texto
    // crudo: el patrón habitual de Unity (`#if UNITY_EDITOR` +
    // `using UnityEditor;` dentro) no convierte en «de editor» un
    // script de runtime con una sección solo-editor.
    if let Some(num) = buscar_using_editor(&pre.lineas) {
        return resultado_editor(
            num,
            "using UnityEditor",
            "script de editor (using UnityEditor): no se convierte al motor",
        );
    }

    let mut marcas = pre.avisos;
    let mut enmascarador = Enmascarador::default();
    for (num, linea) in &pre.lineas {
        let enmascarada = enmascarador.enmascarar(linea);
        detectar_linea(*num, &enmascarada, &mut marcas);
    }
    marcas.sort_by_key(|m| m.linea);

    let codigo = if marcas.iter().any(|m| m.bloqueante) {
        None
    } else {
        let mut c = pre
            .lineas
            .iter()
            .map(|(_, l)| *l)
            .collect::<Vec<_>>()
            .join("\n");
        if salto_final {
            c.push('\n');
        }
        Some(c)
    };

    ResultadoScript {
        codigo,
        marcas,
        es_editor: false,
    }
}

fn marca(linea: usize, elemento: &str, motivo: &str, bloqueante: bool) -> MarcaScript {
    MarcaScript {
        linea,
        elemento: elemento.to_string(),
        motivo: motivo.to_string(),
        bloqueante,
    }
}

fn resultado_editor(linea: usize, elemento: &str, motivo: &str) -> ResultadoScript {
    ResultadoScript {
        codigo: None,
        marcas: vec![marca(linea, elemento, motivo, false)],
        es_editor: true,
    }
}

/// Parte el texto por `\n` conservando cada línea tal cual (incluido un
/// posible `\r`). Devuelve también si el texto acababa en salto de línea,
/// para que un script sin directivas salga idéntico.
fn partir_lineas(texto: &str) -> (Vec<&str>, bool) {
    let salto_final = texto.ends_with('\n');
    let cuerpo = texto.strip_suffix('\n').unwrap_or(texto);
    (cuerpo.split('\n').collect(), salto_final)
}

// ---------------------------------------------------------------------------
// Preprocesador
// ---------------------------------------------------------------------------

/// Líneas efectivas (número original, contenido) y avisos no bloqueantes.
struct Preprocesado<'a> {
    lineas: Vec<(usize, &'a str)>,
    avisos: Vec<MarcaScript>,
}

/// Bloque `#if` abierto.
struct Marco {
    /// La rama actual de este bloque está activa (sin contar los padres).
    activa: bool,
    /// Alguna rama del bloque ya se eligió.
    tomada: bool,
    /// Ya apareció `#else`.
    visto_else: bool,
    /// Línea del `#if` que abrió el bloque.
    linea_if: usize,
}

/// Si la línea es una directiva, devuelve (nombre, resto sin comentario `//`).
fn directiva(linea: &str) -> Option<(&str, &str)> {
    let t = linea.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let resto = t.strip_prefix('#')?.trim_start();
    let fin = resto
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(resto.len());
    let (nombre, cond) = resto.split_at(fin);
    let cond = cond.split_once("//").map_or(cond, |(a, _)| a);
    Some((nombre, cond.trim()))
}

fn no_resoluble(linea: usize, elemento: &str, cond: &str) -> MarcaScript {
    marca(
        linea,
        elemento,
        &format!("símbolo de preprocesador no resoluble: {cond}"),
        true,
    )
}

fn desbalanceada(linea: usize, elemento: &str, detalle: &str) -> MarcaScript {
    marca(
        linea,
        elemento,
        &format!("directiva de preprocesador desbalanceada: {detalle}"),
        true,
    )
}

/// Resuelve `#if`/`#elif`/`#else`/`#endif`, elimina las ramas no elegidas y
/// las directivas, y conserva el número de línea original de cada línea.
/// Las condiciones solo se evalúan cuando hace falta para elegir rama.
fn preprocesar<'a>(lineas: &[&'a str]) -> Result<Preprocesado<'a>, MarcaScript> {
    let mut pila: Vec<Marco> = Vec::new();
    let mut efectivas = Vec::new();
    let mut avisos = Vec::new();

    for (indice, &linea) in lineas.iter().enumerate() {
        let num = indice + 1;
        let activo = pila.iter().all(|m| m.activa);
        match directiva(linea) {
            Some(("if", cond)) => {
                let activa = if activo {
                    evaluar(cond).ok_or_else(|| no_resoluble(num, "#if", cond))?
                } else {
                    false
                };
                pila.push(Marco {
                    activa,
                    tomada: activa,
                    visto_else: false,
                    linea_if: num,
                });
            }
            Some(("elif", cond)) => {
                let Some((cima, resto)) = pila.split_last_mut() else {
                    return Err(desbalanceada(num, "#elif", "#elif sin #if"));
                };
                if cima.visto_else {
                    return Err(desbalanceada(num, "#elif", "#elif después de #else"));
                }
                let padre = resto.iter().all(|m| m.activa);
                if padre && !cima.tomada {
                    let valor =
                        evaluar(cond).ok_or_else(|| no_resoluble(num, "#elif", cond))?;
                    cima.activa = valor;
                    cima.tomada = valor;
                } else {
                    cima.activa = false;
                }
            }
            Some(("else", _)) => {
                let Some((cima, resto)) = pila.split_last_mut() else {
                    return Err(desbalanceada(num, "#else", "#else sin #if"));
                };
                if cima.visto_else {
                    return Err(desbalanceada(num, "#else", "#else repetido"));
                }
                let padre = resto.iter().all(|m| m.activa);
                cima.visto_else = true;
                cima.activa = padre && !cima.tomada;
                cima.tomada = true;
            }
            Some(("endif", _)) => {
                if pila.pop().is_none() {
                    return Err(desbalanceada(num, "#endif", "#endif sin #if"));
                }
            }
            Some((nombre @ ("define" | "undef"), _)) => {
                if activo {
                    efectivas.push((num, linea));
                    avisos.push(marca(
                        num,
                        &format!("#{nombre}"),
                        &format!("#{nombre} local: se conserva tal cual"),
                        false,
                    ));
                }
            }
            // Resto de líneas (incluidas #region, #pragma, #nullable…).
            _ => {
                if activo {
                    efectivas.push((num, linea));
                }
            }
        }
    }

    if let Some(m) = pila.last() {
        return Err(desbalanceada(m.linea_if, "#if", "#if sin #endif"));
    }

    Ok(Preprocesado {
        lineas: efectivas,
        avisos,
    })
}

/// Evalúa una condición de `#if` con `&&`, `||`, `!` y paréntesis.
/// `None` = no resoluble. Se evalúa la expresión entera: si cualquier
/// término no se resuelve, o hay un error de sintaxis (paréntesis
/// desbalanceados, operador colgando, condición vacía, comparaciones
/// `==`/`!=`, etc.), el resultado es `None`. Nunca se cortocircuita ni
/// se adivina un símbolo.
fn evaluar(cond: &str) -> Option<bool> {
    if cond.contains("==") || cond.contains("!=") {
        return None;
    }
    let tokens = tokenizar_condicion(cond)?;
    let mut pos = 0usize;
    let resultado = parsear_or(&tokens, &mut pos)?;
    if pos != tokens.len() {
        return None;
    }
    resultado
}

/// Token de la gramática de condiciones de `#if`.
enum TokenCondicion {
    Y,
    O,
    No,
    ParenAbre,
    ParenCierra,
    Termino(String),
}

/// Divide la condición en tokens. `None` si aparece un carácter ajeno
/// a la gramática soportada (identificadores, `!`, `&&`, `||`, `(`, `)`).
fn tokenizar_condicion(cond: &str) -> Option<Vec<TokenCondicion>> {
    let mut tokens = Vec::new();
    let caracteres: Vec<char> = cond.chars().collect();
    let mut i = 0usize;
    while i < caracteres.len() {
        let c = caracteres[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' {
            tokens.push(TokenCondicion::ParenAbre);
            i += 1;
        } else if c == ')' {
            tokens.push(TokenCondicion::ParenCierra);
            i += 1;
        } else if c == '!' {
            tokens.push(TokenCondicion::No);
            i += 1;
        } else if c == '&' && caracteres.get(i + 1) == Some(&'&') {
            tokens.push(TokenCondicion::Y);
            i += 2;
        } else if c == '|' && caracteres.get(i + 1) == Some(&'|') {
            tokens.push(TokenCondicion::O);
            i += 2;
        } else if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            let inicio = i;
            while i < caracteres.len()
                && (caracteres[i].is_ascii_alphanumeric() || caracteres[i] == '_' || caracteres[i] == '.')
            {
                i += 1;
            }
            let termino: String = caracteres[inicio..i].iter().collect();
            tokens.push(TokenCondicion::Termino(termino));
        } else {
            return None;
        }
    }
    Some(tokens)
}

/// `or := and ("||" and)*`
fn parsear_or(tokens: &[TokenCondicion], pos: &mut usize) -> Option<Option<bool>> {
    let mut resultado = parsear_and(tokens, pos)?;
    while matches!(tokens.get(*pos), Some(TokenCondicion::O)) {
        *pos += 1;
        let siguiente = parsear_and(tokens, pos)?;
        resultado = combinar(resultado, siguiente, |a, b| a || b);
    }
    Some(resultado)
}

/// `and := unary ("&&" unary)*`
fn parsear_and(tokens: &[TokenCondicion], pos: &mut usize) -> Option<Option<bool>> {
    let mut resultado = parsear_unary(tokens, pos)?;
    while matches!(tokens.get(*pos), Some(TokenCondicion::Y)) {
        *pos += 1;
        let siguiente = parsear_unary(tokens, pos)?;
        resultado = combinar(resultado, siguiente, |a, b| a && b);
    }
    Some(resultado)
}

/// `unary := "!" unary | "(" expr ")" | término`
fn parsear_unary(tokens: &[TokenCondicion], pos: &mut usize) -> Option<Option<bool>> {
    match tokens.get(*pos) {
        Some(TokenCondicion::No) => {
            *pos += 1;
            let interior = parsear_unary(tokens, pos)?;
            Some(interior.map(|v| !v))
        }
        Some(TokenCondicion::ParenAbre) => {
            *pos += 1;
            let interior = parsear_or(tokens, pos)?;
            if !matches!(tokens.get(*pos), Some(TokenCondicion::ParenCierra)) {
                return None;
            }
            *pos += 1;
            Some(interior)
        }
        Some(TokenCondicion::Termino(t)) => {
            *pos += 1;
            Some(evaluar_termino(t))
        }
        _ => None,
    }
}

/// Combina dos resultados parcialmente resueltos sin cortocircuitar:
/// si cualquiera de los dos es `None`, el resultado es `None`.
fn combinar(a: Option<bool>, b: Option<bool>, f: impl Fn(bool, bool) -> bool) -> Option<bool> {
    match (a, b) {
        (Some(a), Some(b)) => Some(f(a, b)),
        _ => None,
    }
}

/// Resuelve un término individual de una condición `#if`.
///
/// El destino de la conversión es el reproductor standalone de Windows del
/// motor. Por eso `UNITY_STANDALONE` y `UNITY_STANDALONE_WIN` valen `true`,
/// mientras que las demás plataformas standalone y todas las variantes de
/// editor por SO valen `false`; `UNITY_EDITOR` también vale `false`.
///
/// Los paquetes de Unity se fijan por presencia en el material importado:
/// `UNITY_POST_PROCESSING_STACK_V2` no está definido porque el paquete Post
/// Processing no viene en el material, y `ENABLE_LEGACY_INPUT_MANAGER` no
/// está definido porque no hay evidencia de que el proyecto de origen activara
/// la entrada legada.
///
/// Los defines opcionales del proyecto NeoFPS medidos en la importación no
/// están definidos en ningún material del paquete (no hay `.asmdef`, `.rsp`
/// ni ProjectSettings que los activen), así que se consideran apagados:
/// `NEOFPS_FORCE_QUALITY`, `NEOFPS_FORCE_LIGHTWEIGHT`, `NEOFPS_LIGHTWEIGHT`,
/// `NEOFPS_PROCEDURAL_INPUT`, `NEOFPS_LOAD_ON_DEMAND`, `VISUALISE_MOVEMENT`,
/// `SETTINGS_USES_JSON` y `AGGRESIVE_CURSOR_LOCK` valen `false`.
///
/// Cualquier otro símbolo desconocido sigue sin resolverse: no se adivina nada
/// fuera de esta tabla.
fn evaluar_termino(termino: &str) -> Option<bool> {
    if let Some(resto) = termino.strip_prefix('!') {
        return evaluar_termino(resto.trim_start()).map(|v| !v);
    }
    match termino {
        "true" => Some(true),
        "false" | "UNITY_EDITOR" => Some(false),
        "UNITY_STANDALONE" | "UNITY_STANDALONE_WIN" => Some(true),
        "UNITY_ANDROID"
        | "UNITY_IOS"
        | "UNITY_TIZEN"
        | "UNITY_WSA"
        | "UNITY_WEBGL"
        | "NETFX_CORE"
        | "UNITY_STANDALONE_LINUX"
        | "UNITY_STANDALONE_OSX"
        | "UNITY_EDITOR_WIN"
        | "UNITY_EDITOR_LINUX"
        | "UNITY_EDITOR_OSX"
        | "UNITY_POST_PROCESSING_STACK_V2"
        | "ENABLE_LEGACY_INPUT_MANAGER"
        | "NEOFPS_FORCE_QUALITY"
        | "NEOFPS_FORCE_LIGHTWEIGHT"
        | "NEOFPS_LIGHTWEIGHT"
        | "NEOFPS_PROCEDURAL_INPUT"
        | "NEOFPS_LOAD_ON_DEMAND"
        | "VISUALISE_MOVEMENT"
        | "SETTINGS_USES_JSON"
        | "AGGRESIVE_CURSOR_LOCK" => Some(false),
        _ => version_unity(termino),
    }
}

/// `UNITY_<aaaa>_OR_NEWER` y `UNITY_<aaaa>_<m>_OR_NEWER`: cierto si la
/// versión principal es ≤ 2023. También acepta los símbolos antiguos de
/// versión principal corta (p. ej. `UNITY_5_3_OR_NEWER`, cierto).
fn version_unity(simbolo: &str) -> Option<bool> {
    let medio = simbolo.strip_prefix("UNITY_")?.strip_suffix("_OR_NEWER")?;
    let (principal, menor) = match medio.split_once('_') {
        Some((p, m)) => (p, Some(m)),
        None => (medio, None),
    };
    let solo_digitos = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !solo_digitos(principal) || menor.is_some_and(|m| !solo_digitos(m)) {
        return None;
    }
    let principal: u32 = principal.parse().ok()?;
    Some(principal <= 2023)
}

// ---------------------------------------------------------------------------
// Enmascarador (mini-lexer C#)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Estado {
    #[default]
    Codigo,
    /// `/* ... */`, puede cruzar líneas.
    ComentarioBloque,
    /// `@"..."`, `$@"..."`, `@$"..."`, puede cruzar líneas.
    Verbatim,
    /// `"..."` o `$"..."`, termina en la línea.
    Cadena,
    /// `'x'`, termina en la línea.
    Caracter,
}

/// Sustituye comentarios y cadenas por espacios, línea a línea, con estado
/// entre líneas para los comentarios de bloque y las cadenas verbatim.
#[derive(Debug, Default)]
struct Enmascarador {
    estado: Estado,
}

impl Enmascarador {
    fn enmascarar(&mut self, linea: &str) -> String {
        let cs: Vec<char> = linea.chars().collect();
        let mut out = String::with_capacity(linea.len());
        let mut i = 0;
        while i < cs.len() {
            match self.estado {
                Estado::ComentarioBloque => {
                    if cs[i] == '*' && cs.get(i + 1) == Some(&'/') {
                        out.push_str("  ");
                        i += 2;
                        self.estado = Estado::Codigo;
                    } else {
                        out.push(' ');
                        i += 1;
                    }
                }
                Estado::Verbatim => {
                    if cs[i] == '"' {
                        if cs.get(i + 1) == Some(&'"') {
                            out.push_str("  ");
                            i += 2;
                        } else {
                            out.push(' ');
                            i += 1;
                            self.estado = Estado::Codigo;
                        }
                    } else {
                        out.push(' ');
                        i += 1;
                    }
                }
                Estado::Cadena | Estado::Caracter => {
                    let cierre = if self.estado == Estado::Cadena { '"' } else { '\'' };
                    if cs[i] == '\\' {
                        out.push(' ');
                        i += 1;
                        if i < cs.len() {
                            out.push(' ');
                            i += 1;
                        }
                    } else {
                        if cs[i] == cierre {
                            self.estado = Estado::Codigo;
                        }
                        out.push(' ');
                        i += 1;
                    }
                }
                Estado::Codigo => {
                    let c = cs[i];
                    let sig = cs.get(i + 1).copied();
                    let tras = cs.get(i + 2).copied();
                    if c == '/' && sig == Some('/') {
                        out.push_str(&" ".repeat(cs.len() - i));
                        i = cs.len();
                    } else if c == '/' && sig == Some('*') {
                        out.push_str("  ");
                        i += 2;
                        self.estado = Estado::ComentarioBloque;
                    } else if c == '@' && sig == Some('"') {
                        out.push_str("  ");
                        i += 2;
                        self.estado = Estado::Verbatim;
                    } else if matches!((c, sig), ('@', Some('$')) | ('$', Some('@')))
                        && tras == Some('"')
                    {
                        out.push_str("   ");
                        i += 3;
                        self.estado = Estado::Verbatim;
                    } else if c == '$' && sig == Some('"') {
                        out.push_str("  ");
                        i += 2;
                        self.estado = Estado::Cadena;
                    } else if c == '"' {
                        out.push(' ');
                        i += 1;
                        self.estado = Estado::Cadena;
                    } else if c == '\'' {
                        out.push(' ');
                        i += 1;
                        self.estado = Estado::Caracter;
                    } else {
                        out.push(c);
                        i += 1;
                    }
                }
            }
        }
        // Las cadenas normales y los caracteres no cruzan líneas.
        if matches!(self.estado, Estado::Cadena | Estado::Caracter) {
            self.estado = Estado::Codigo;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Detección
// ---------------------------------------------------------------------------

fn es_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Identificadores de la línea: (texto, inicio, fin) en posiciones de char.
fn identificadores(cs: &[char]) -> Vec<(String, usize, usize)> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if es_ident(cs[i]) {
            let ini = i;
            while i < cs.len() && es_ident(cs[i]) {
                i += 1;
            }
            v.push((cs[ini..i].iter().collect(), ini, i));
        } else {
            i += 1;
        }
    }
    v
}

/// `true` si entre `desde` y `hasta` solo hay espacios.
fn solo_espacios(cs: &[char], desde: usize, hasta: usize) -> bool {
    cs.get(desde..hasta)
        .is_some_and(|s| s.iter().all(|c| c.is_whitespace()))
}

/// Lee `<Tipo>` tras `desde` (con cualquier espaciado y nombre cualificado)
/// y devuelve el último segmento del tipo.
fn argumento_generico(cs: &[char], desde: usize) -> Option<String> {
    let mut i = desde;
    while i < cs.len() && cs[i].is_whitespace() {
        i += 1;
    }
    if cs.get(i) != Some(&'<') {
        return None;
    }
    let mut arg = String::new();
    for &c in cs.get(i + 1..)? {
        if c == '>' {
            return arg.rsplit('.').next().map(|s| s.trim().to_string());
        }
        if es_ident(c) || c == '.' || c.is_whitespace() {
            arg.push(c);
        } else {
            return None;
        }
    }
    None
}

/// Línea original del primer `using UnityEditor` fuera de comentarios
/// y cadenas (admite `using static UnityEditor…`), buscado sobre las
/// líneas efectivas que deja el preprocesador.
fn buscar_using_editor(lineas: &[(usize, &str)]) -> Option<usize> {
    let mut enmascarador = Enmascarador::default();
    for (num, linea) in lineas.iter() {
        let cs: Vec<char> = enmascarador.enmascarar(linea).chars().collect();
        let ids = identificadores(&cs);
        for (k, (palabra, ini, _)) in ids.iter().enumerate() {
            if palabra.as_str() != "UnityEditor" || k == 0 {
                continue;
            }
            let previo = &ids[k - 1];
            let pegado = solo_espacios(&cs, previo.2, *ini);
            let es_using = previo.0.as_str() == "using"
                || (previo.0.as_str() == "static"
                    && k >= 2
                    && ids[k - 2].0.as_str() == "using"
                    && solo_espacios(&cs, ids[k - 2].2, previo.1));
            if pegado && es_using {
                return Some(*num);
            }
        }
    }
    None
}

/// Busca los patrones bloqueantes en una línea ya enmascarada.
fn detectar_linea(num: usize, linea: &str, marcas: &mut Vec<MarcaScript>) {
    let cs: Vec<char> = linea.chars().collect();
    let ids = identificadores(&cs);
    for (palabra, _, fin) in ids.iter() {
        let palabra = palabra.as_str();
        match palabra {
            "AddComponent" => {
                if let Some(forma) =
                    argumento_generico(&cs, *fin).filter(|f| FORMAS.contains(&f.as_str()))
                {
                    marcas.push(marca(
                        num,
                        &format!("{palabra}<{forma}>"),
                        MOTIVO_ADD_FORMA,
                        true,
                    ));
                }
            }
            _ if OBTENER.contains(&palabra) => {
                if let Some(forma) =
                    argumento_generico(&cs, *fin).filter(|f| FORMAS.contains(&f.as_str()))
                {
                    marcas.push(marca(
                        num,
                        &format!("{palabra}<{forma}>"),
                        MOTIVO_GET_FORMA,
                        true,
                    ));
                }
            }
            _ => {
                if let Some(&(nombre, motivo)) = DIRECTOS.iter().find(|e| e.0 == palabra) {
                    marcas.push(marca(num, nombre, motivo, true));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn bloqueantes(r: &ResultadoScript) -> Vec<&MarcaScript> {
        r.marcas.iter().filter(|m| m.bloqueante).collect()
    }

    #[test]
    fn script_directo_minimo_queda_identico() {
        let texto = "using UnityEngine;\n\npublic class Mover : MonoBehaviour\n{\n    void Start()\n    {\n        Debug.Log(\"hola\");\n    }\n\n    void Update()\n    {\n        transform.Translate(Vector3.forward * Time.deltaTime);\n    }\n}\n";
        let r = convertir_script("Assets/Scripts/Mover.cs", texto);
        assert!(!r.es_editor);
        assert!(r.marcas.is_empty());
        assert_eq!(r.codigo.as_deref(), Some(texto));

        let crlf = "using UnityEngine;\r\npublic class A : MonoBehaviour { }\r\n";
        let r = convertir_script("Assets/A.cs", crlf);
        assert_eq!(r.codigo.as_deref(), Some(crlf));
    }

    #[test]
    fn start_coroutine_ya_no_bloquea() {
        let texto = "using UnityEngine;\npublic class A : MonoBehaviour\n{\n    void Start()\n    {\n        StartCoroutine(Bucle());\n    }\n}\n";
        let r = convertir_script("Assets/A.cs", texto);
        assert_eq!(r.codigo.as_deref(), Some(texto));
        assert!(bloqueantes(&r).is_empty(), "{:?}", r.marcas);
    }

    #[test]
    fn comentarios_y_cadenas_no_cuentan() {
        let texto = "public class A : MonoBehaviour\n{\n    // StartCoroutine(x);\n    /* StartCoroutine\n       StartCoroutine */\n    string s = \"StartCoroutine\";\n    string v = @\"línea \"\"\nStartCoroutine\";\n    string i = $\"{n} StartCoroutine\";\n    char c = '\\'';\n    void F() { Debug.Log(\"\\\"StartCoroutine\\\"\"); }\n}\n";
        let r = convertir_script("Assets/A.cs", texto);
        assert!(bloqueantes(&r).is_empty(), "{:?}", r.marcas);
        assert_eq!(r.codigo.as_deref(), Some(texto));

        // Tras un comentario de bloque cerrado y un carácter, el código real ya no bloquea.
        let texto = "/* x */ char c = '\"'; StartCoroutine(y);\n";
        let r = convertir_script("Assets/B.cs", texto);
        assert!(bloqueantes(&r).is_empty(), "{:?}", r.marcas);
        assert_eq!(r.codigo.as_deref(), Some(texto));
    }

    #[test]
    fn rama_de_editor_desaparece() {
        let texto = "public class A : MonoBehaviour\n{\n#if UNITY_EDITOR\n    IEnumerator Co() { yield return null; }\n#else\n    void Start() { }\n#endif\n}\n";
        let r = convertir_script("Assets/A.cs", texto);
        assert!(bloqueantes(&r).is_empty(), "{:?}", r.marcas);
        assert_eq!(
            r.codigo.as_deref(),
            Some("public class A : MonoBehaviour\n{\n    void Start() { }\n}\n")
        );

        // La rama #else conserva su línea original tras eliminar ramas.
        let texto = "public class A : MonoBehaviour\n{\n#if UNITY_EDITOR\n    void Start() { }\n#else\n    object w = new WaitForSeconds(1f);\n#endif\n}\n";
        let r = convertir_script("Assets/A.cs", texto);
        assert!(bloqueantes(&r).is_empty(), "{:?}", r.marcas);
        assert_eq!(
            r.codigo.as_deref(),
            Some("public class A : MonoBehaviour\n{\n    object w = new WaitForSeconds(1f);\n}\n")
        );
    }

    #[test]
    fn simbolos_de_version_y_desconocidos() {
        let texto = "#if UNITY_2022_OR_NEWER\nint a;\n#endif\n#if UNITY_2026_OR_NEWER\nint b;\n#endif\n#if !UNITY_EDITOR && UNITY_2021_3_OR_NEWER\nint c;\n#elif ALGO\nint d;\n#endif\n";
        let r = convertir_script("Assets/A.cs", texto);
        assert!(r.marcas.is_empty(), "{:?}", r.marcas);
        assert_eq!(r.codigo.as_deref(), Some("int a;\nint c;\n"));

        let r = convertir_script("Assets/A.cs", "#if ALGO_DESCONOCIDO\nint a;\n#endif\n");
        assert!(r.codigo.is_none());
        let b = bloqueantes(&r);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].linea, 1);
        assert_eq!(
            b[0].motivo,
            "símbolo de preprocesador no resoluble: ALGO_DESCONOCIDO"
        );

        // Con paréntesis, `(UNITY_EDITOR)` se resuelve a `false`: la rama
        // se descarta y el script convierte sin marcas bloqueantes.
        let r = convertir_script("Assets/A.cs", "#if (UNITY_EDITOR)\n#endif\n");
        assert_eq!(r.codigo.as_deref(), Some("\n"));
        assert_eq!(bloqueantes(&r).len(), 0);
    }

    #[test]
fn plataforma_standalone_y_parentesis() {
    let r = convertir_script("Assets/A.cs", "#if UNITY_STANDALONE\nint a;\n#endif\n");
    assert_eq!(r.codigo, Some("int a;\n".to_string()));
    assert_eq!(bloqueantes(&r).len(), 0);

    let r = convertir_script(
        "Assets/A.cs",
        "#if (UNITY_ANDROID || UNITY_IOS)\nint a;\n#else\nint b;\n#endif\n",
    );
    assert_eq!(r.codigo, Some("int b;\n".to_string()));
    assert_eq!(bloqueantes(&r).len(), 0);

    let r = convertir_script(
        "Assets/A.cs",
        "#if !UNITY_EDITOR && (UNITY_STANDALONE || UNITY_WEBGL)\nint a;\n#endif\n",
    );
    assert_eq!(r.codigo, Some("int a;\n".to_string()));
    assert_eq!(bloqueantes(&r).len(), 0);
}


#[test]
fn sintaxis_rota_no_resuelve() {
    assert_eq!(evaluar("(UNITY_STANDALONE"), None);
    assert_eq!(evaluar("UNITY_STANDALONE &&"), None);
    assert_eq!(evaluar(""), None);
    assert_eq!(evaluar("UNITY_STANDALONE == true"), None);
}

        #[test]
    fn condicion_real_de_neofps_ya_resuelve() {
        let texto = "#if !NEOFPS_FORCE_QUALITY && (UNITY_ANDROID || UNITY_IOS || UNITY_TIZEN || (UNITY_WSA && NETFX_CORE) || NEOFPS_FORCE_LIGHTWEIGHT)\nint movil;\n#endif\n";
        let r = convertir_script("Assets/A.cs", texto);
        assert_eq!(r.codigo.as_deref(), Some("\n"));
        assert!(bloqueantes(&r).is_empty());
    }

    #[test]
    fn cadena_por_so_cae_en_windows() {
        let texto = "#if UNITY_STANDALONE_OSX || UNITY_EDITOR_OSX\nint mac;\n#elif UNITY_STANDALONE_LINUX || UNITY_EDITOR_LINUX\nint linux;\n#else\nint windows;\n#endif\n";
        let r = convertir_script("Assets/A.cs", texto);
        assert_eq!(r.codigo.as_deref(), Some("int windows;\n"));
        assert!(bloqueantes(&r).is_empty());
    }

    #[test]
    fn legado_de_entrada_apagado() {
        let r = convertir_script(
            "Assets/A.cs",
            "#if ENABLE_LEGACY_INPUT_MANAGER\nint viejo;\n#else\nint nuevo;\n#endif\n",
        );
        assert_eq!(r.codigo.as_deref(), Some("int nuevo;\n"));
        assert!(bloqueantes(&r).is_empty());

        let r = convertir_script(
            "Assets/A.cs",
            "#if !ENABLE_LEGACY_INPUT_MANAGER\nint nuevo;\n#endif\n",
        );
        assert_eq!(r.codigo.as_deref(), Some("int nuevo;\n"));
        assert!(bloqueantes(&r).is_empty());
    }

    #[test]
    fn define_se_conserva_con_aviso() {
        let texto = "#define DEPURAR\nusing UnityEngine;\n";
        let r = convertir_script("Assets/A.cs", texto);
        assert_eq!(r.codigo.as_deref(), Some(texto));
        assert_eq!(r.marcas.len(), 1);
        assert!(!r.marcas[0].bloqueante);
        assert_eq!(r.marcas[0].linea, 1);
        assert_eq!(r.marcas[0].motivo, "#define local: se conserva tal cual");
    }

    #[test]
    fn scripts_de_editor() {
        let r = convertir_script("Assets/X/Editor/Tool.cs", "public class T { }\n");
        assert!(r.es_editor);
        assert!(r.codigo.is_none());
        assert_eq!(r.marcas.len(), 1);
        assert!(!r.marcas[0].bloqueante);

        let r = convertir_script("Assets\\Editor\\T.cs", "class T { }");
        assert!(r.es_editor);

        let r = convertir_script(
            "Assets/X/Tool.cs",
            "using UnityEngine;\nusing UnityEditor;\npublic class T { }\n",
        );
        assert!(r.es_editor);
        assert!(r.codigo.is_none());
        assert_eq!(r.marcas[0].linea, 2);
        assert!(!r.marcas[0].bloqueante);

        let r = convertir_script(
            "Assets/EditorTools/T.cs",
            "// using UnityEditor;\nstring s = \"using UnityEditor\";\n",
        );
        assert!(!r.es_editor);
        assert!(r.codigo.is_some());
    }

    #[test]
    fn using_editor_solo_en_rama_editor_no_clasifica() {
        let texto = "#if UNITY_EDITOR\nusing UnityEditor;\n#endif\nusing UnityEngine;\npublic class A : MonoBehaviour { }\n";
        let r = convertir_script("Assets/A.cs", texto);
        assert!(!r.es_editor);
        assert!(r.codigo.is_some());
    }

    #[test]
    fn colliders_por_forma() {
        let texto = "public class A : MonoBehaviour\n{\n    void Start() {\n        var e = GetComponent<SphereCollider>();\n        var c = gameObject.AddComponent< CapsuleCollider >();\n        var r = GetComponent<Rigidbody>();\n    }\n}\n";
        let r = convertir_script("Assets/A.cs", texto);
        assert!(r.codigo.is_none());
        let b = bloqueantes(&r);
        assert_eq!(b.len(), 2, "{:?}", r.marcas);
        assert_eq!(b[0].linea, 4);
        assert_eq!(b[0].elemento, "GetComponent<SphereCollider>");
        assert_eq!(b[0].motivo, MOTIVO_GET_FORMA);
        assert_eq!(b[1].linea, 5);
        assert_eq!(b[1].elemento, "AddComponent<CapsuleCollider>");
        assert_eq!(b[1].motivo, MOTIVO_ADD_FORMA);

        let r = convertir_script("Assets/A.cs", "var r = GetComponent<Rigidbody>();\n");
        assert!(r.marcas.is_empty());
        assert!(r.codigo.is_some());
    }

    #[test]
    fn busqueda_por_tipo_y_2d() {
        let texto = "public class A : MonoBehaviour\n{\n    Rigidbody2D cuerpo;\n    void Start() { var e = FindObjectOfType<Enemigo>(); }\n}\n";
        let r = convertir_script("Assets/A.cs", texto);
        assert!(r.codigo.is_none());
        let b = bloqueantes(&r);
        assert_eq!(b.len(), 2, "{:?}", r.marcas);
        assert_eq!(b[0].elemento, "Rigidbody2D");
        assert_eq!(b[0].linea, 3);
        assert_eq!(b[0].motivo, MOTIVO_2D);
        assert_eq!(b[1].elemento, "FindObjectOfType");
        assert_eq!(b[1].linea, 4);
        assert_eq!(b[1].motivo, MOTIVO_BUSQUEDA);
    }

    #[test]
    fn directivas_desbalanceadas() {
        let r = convertir_script("Assets/A.cs", "#endif\nint a;\n");
        assert!(r.codigo.is_none());
        let b = bloqueantes(&r);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].elemento, "#endif");
        assert_eq!(b[0].linea, 1);

        let r = convertir_script("Assets/A.cs", "int a;\n#if UNITY_2020_OR_NEWER\nint b;\n");
        assert!(r.codigo.is_none());
        let b = bloqueantes(&r);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].linea, 2);

        let r = convertir_script("Assets/A.cs", "#if UNITY_EDITOR\n#else\n#else\n#endif\n");
        assert!(r.codigo.is_none());
        assert_eq!(bloqueantes(&r)[0].linea, 3);
    }
}
