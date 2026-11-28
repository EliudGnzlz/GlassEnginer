//! Parser CSS del `style_extra` y cascada (V2 P1).
//!
//! Integración: al aplicar este archivo, borrar en `api.rs` los stubs
//! de `parsear_estilo_extra` y `resolver_estilos` (estas son las
//! definiciones reales; al borrar el segundo, `NodoDocumento` deja de
//! usarse en `api.rs` y hay que quitarlo de su `use`) y añadir
//! `pub mod parser;` en `lib.rs`.
//!
//! Cascada (de menos a más prioridad):
//! 1. Defecto del tipo: `display: Absoluto`; si hay LayoutGroup,
//!    `display: Flex` con dirección, gap, padding y alineación del grupo.
//! 2. Campos del componente: texto → `color` y `font_size`; imagen →
//!    `background_color` (su tinte); botón → `background_color`
//!    (color del Graphic objetivo, por encima del tinte de la imagen).
//! 3. `style_extra` parseado (capa base).
//! 4. Pseudo-clase del estado dado, solo la de mayor prioridad que
//!    aplique: disabled, active, hover, focus, selected. Un control
//!    deshabilitado no recibe hover ni active.
//!
//! Errores: la declaración inválida se ignora y se anota en `avisos`;
//! nunca hay pánico. Propiedades desconocidas: aviso e ignorada.

use crate::api::{EstadoPseudo, EstilosExtra};
use crate::documento::NodoDocumento;
use crate::estilos::{Dimension, Display, Sombra, StyleProps, TamanoColumna, TextOverflow};

/// Parte `texto` en `sep` sin partir dentro de paréntesis.
fn dividir_nivel_superior(texto: &str, sep: char) -> Vec<&str> {
    let mut partes = Vec::new();
    let mut prof = 0i32;
    let mut inicio = 0usize;
    for (i, c) in texto.char_indices() {
        match c {
            '(' => prof += 1,
            ')' => prof -= 1,
            _ if c == sep && prof == 0 => {
                partes.push(texto[inicio..i].trim());
                inicio = i + c.len_utf8();
            }
            _ => {}
        }
    }
    partes.push(texto[inicio..].trim());
    partes
}

/// Extrae los bloques `:pseudo{...}`; devuelve el texto base sin
/// ellos, los bloques (nombre en minúsculas, interior) y avisos.
fn extraer_bloques(texto: &str) -> (String, Vec<(String, String)>, Vec<String>) {
    let mut base = String::new();
    let mut bloques = Vec::new();
    let mut avisos = Vec::new();
    let bytes = texto.as_bytes();
    let mut i = 0usize;
    let mut inicio_base = 0usize;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let mut j = i + 1;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-' || bytes[j] == b'_')
            {
                j += 1;
            }
            if j > i + 1 {
                let nombre = &texto[i + 1..j];
                let mut k = j;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k < bytes.len() && bytes[k] == b'{' {
                    let mut prof = 0i32;
                    let mut m = k;
                    let mut cierre = None;
                    while m < bytes.len() {
                        if bytes[m] == b'{' {
                            prof += 1;
                        }
                        if bytes[m] == b'}' {
                            prof -= 1;
                            if prof == 0 {
                                cierre = Some(m);
                                break;
                            }
                        }
                        m += 1;
                    }
                    match cierre {
                        Some(fin) => {
                            base.push_str(&texto[inicio_base..i]);
                            bloques.push((nombre.to_ascii_lowercase(), texto[k + 1..fin].to_string()));
                            i = fin + 1;
                            inicio_base = i;
                            continue;
                        }
                        None => {
                            avisos.push(format!("bloque :{nombre} sin cierre, se ignora"));
                            base.push_str(&texto[inicio_base..i]);
                            inicio_base = texto.len();
                            break;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    base.push_str(&texto[inicio_base..]);
    (base, bloques, avisos)
}

/// Número con sufijo `px` opcional; rechaza NaN e infinito.
fn parse_px(valor: &str) -> Option<f32> {
    let v = valor.trim();
    let num = v.strip_suffix("px").unwrap_or(v);
    if num.is_empty() {
        return None;
    }
    let n: f32 = num.parse().ok()?;
    if n.is_finite() {
        Some(n)
    } else {
        None
    }
}

/// Dimensión: `auto`, `50%` o px (con o sin sufijo).
fn parse_dimension(valor: &str) -> Option<Dimension> {
    let v = valor.trim();
    if v.eq_ignore_ascii_case("auto") {
        return Some(Dimension::Auto);
    }
    if let Some(n) = v.strip_suffix('%') {
        let x: f32 = n.trim().parse().ok()?;
        return if x.is_finite() {
            Some(Dimension::Porcentaje(x))
        } else {
            None
        };
    }
    parse_px(v).map(Dimension::Px)
}

/// Color `#rrggbb`, `rgb(r,g,b)` o `rgba(r,g,b,a)` (r/g/b en 0-255,
/// a en 0-1). Sin colores con nombre en este paquete.
/// sRGB (0..1) → lineal (Q4a). El pipeline trata el color de vértice
/// como lineal y lo convierte a sRGB al pintar, así que el CSS debe
/// entrar ya convertido para que el color escrito sea el que se ve.
fn srgb_a_lineal(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Lineal (0..1) → sRGB (0..1): inversa de `srgb_a_lineal`. La usa el
/// editor para escribir de vuelta en CSS un color que el parser ya
/// convirtió a lineal.
pub fn lineal_a_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Un color lineal (como los que devuelve `parse_color`) a texto CSS:
/// `#rrggbb` si es opaco, `rgba(r, g, b, a)` si no. Los canales se
/// escriben en sRGB, que es lo que el parser espera leer (Q4a).
pub fn color_a_css(c: [f32; 4]) -> String {
    let r = (lineal_a_srgb(c[0].clamp(0.0, 1.0)) * 255.0).round() as u8;
    let g = (lineal_a_srgb(c[1].clamp(0.0, 1.0)) * 255.0).round() as u8;
    let b = (lineal_a_srgb(c[2].clamp(0.0, 1.0)) * 255.0).round() as u8;
    if (c[3] - 1.0).abs() < 0.001 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("rgba({}, {}, {}, {:.2})", r, g, b, c[3].clamp(0.0, 1.0))
    }
}

fn parse_color(valor: &str) -> Option<[f32; 4]> {
    let v = valor.trim();
    if let Some(hex) = v.strip_prefix('#') {
        if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some([
                srgb_a_lineal(r as f32 / 255.0),
                srgb_a_lineal(g as f32 / 255.0),
                srgb_a_lineal(b as f32 / 255.0),
                1.0,
            ]);
        }
        return None;
    }
    let bajo = v.to_ascii_lowercase();
    if let Some(inner) = bajo.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) {
        let p: Vec<&str> = inner.split(',').map(str::trim).collect();
        if p.len() == 4 {
            let (r, g, b, a): (f32, f32, f32, f32) = (
                p[0].parse().ok()?,
                p[1].parse().ok()?,
                p[2].parse().ok()?,
                p[3].parse().ok()?,
            );
            if (0.0..=255.0).contains(&r)
                && (0.0..=255.0).contains(&g)
                && (0.0..=255.0).contains(&b)
                && (0.0..=1.0).contains(&a)
            {
                return Some([
                    srgb_a_lineal(r / 255.0),
                    srgb_a_lineal(g / 255.0),
                    srgb_a_lineal(b / 255.0),
                    a,
                ]);
            }
        }
        return None;
    }
    if let Some(inner) = bajo.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let p: Vec<&str> = inner.split(',').map(str::trim).collect();
        if p.len() == 3 {
            let (r, g, b): (f32, f32, f32) =
                (p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?);
            if (0.0..=255.0).contains(&r) && (0.0..=255.0).contains(&g) && (0.0..=255.0).contains(&b)
            {
                return Some([
                    srgb_a_lineal(r / 255.0),
                    srgb_a_lineal(g / 255.0),
                    srgb_a_lineal(b / 255.0),
                    1.0,
                ]);
            }
        }
        return None;
    }
    None
}

/// Lista de 1 a 4 valores en px (para padding y border-radius).
fn parse_lista_px(valor: &str) -> Option<Vec<f32>> {
    let mut out = Vec::new();
    for parte in valor.split_whitespace() {
        out.push(parse_px(parte)?);
    }
    if out.is_empty() || out.len() > 4 {
        None
    } else {
        Some(out)
    }
}

/// Expansión CSS de padding (orden top right bottom left) al orden
/// de `StyleProps::padding`: [izquierda, arriba, derecha, abajo].
fn expandir_padding(v: &[f32]) -> [f32; 4] {
    match v {
        [a] => [*a, *a, *a, *a],
        [a, b] => [*b, *a, *b, *a],
        [a, b, c] => [*b, *a, *b, *c],
        [a, b, c, d] => [*d, *a, *b, *c],
        _ => [0.0; 4],
    }
}

/// Expansión CSS de border-radius (orden TL TR BR BL, igual que
/// `StyleProps::border_radius`).
fn expandir_radio(v: &[f32]) -> [f32; 4] {
    match v {
        [a] => [*a, *a, *a, *a],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => [0.0; 4],
    }
}

/// Una parada de degradado: color con porcentaje opcional.
fn parse_parada(parada: &str) -> Option<[f32; 4]> {
    let p = parada.trim();
    if p.starts_with('#') {
        let token = p.split_whitespace().next()?;
        return parse_color(token);
    }
    let bajo = p.to_ascii_lowercase();
    for pref in ["rgba(", "rgb("] {
        if let Some(idx) = bajo.find(pref) {
            let resto = &p[idx..];
            let fin = resto.find(')')?;
            return parse_color(&resto[..=fin]);
        }
    }
    None
}

/// `linear-gradient(90deg, color 0%, color 100%)`: solo ángulo y
/// exactamente 2 paradas en este paquete. El ángulo se guarda en la
/// convención del shader («0° = izq→der, 90° = arriba→abajo»), así
/// que el ángulo CSS se convierte restando 90 (Q4b: CSS 90° = izq→der).
fn parse_gradiente(valor: &str) -> Option<(f32, Vec<[f32; 4]>)> {
    let bajo = valor.trim().to_ascii_lowercase();
    let inner = bajo.strip_prefix("linear-gradient(")?.strip_suffix(')')?;
    let partes = dividir_nivel_superior(inner, ',');
    if partes.len() != 3 {
        return None;
    }
    let angulo: f32 = partes[0].trim().strip_suffix("deg")?.trim().parse().ok()?;
    if !angulo.is_finite() {
        return None;
    }
    let mut paradas = Vec::new();
    for parada in &partes[1..] {
        paradas.push(parse_parada(parada)?);
    }
    Some((angulo - 90.0, paradas))
}

/// `grid-template-columns`: `100px 1fr`, `repeat(auto-fill, 100px)`.
/// Los tokens se parten sin romper dentro de paréntesis.
fn parse_columnas_grid(valor: &str) -> Option<Vec<TamanoColumna>> {
    let mut out = Vec::new();
    for token in dividir_nivel_superior(valor, ' ') {
        let t = token.trim();
        if t.is_empty() {
            continue;
        }
        let bajo = t.to_ascii_lowercase();
        if let Some(inner) = bajo.strip_prefix("repeat(auto-fill,").and_then(|s| s.strip_suffix(')')) {
            let n = parse_px(inner.trim())?;
            if n <= 0.0 {
                return None;
            }
            out.push(TamanoColumna::Relleno(n));
        } else if let Some(n) = bajo.strip_suffix("fr") {
            let x: f32 = n.trim().parse().ok()?;
            if !x.is_finite() || x <= 0.0 {
                return None;
            }
            out.push(TamanoColumna::Fraccion(x));
        } else {
            let v = parse_px(t)?;
            if v < 0.0 {
                return None;
            }
            out.push(TamanoColumna::Fija(v));
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// `dx dy blur color` (desplazamientos con signo, blur >= 0).
fn parse_sombra(valor: &str) -> Option<Sombra> {
    let partes = dividir_nivel_superior(valor, ' ');
    if partes.len() != 4 {
        return None;
    }
    let dx = parse_px(partes[0])?;
    let dy = parse_px(partes[1])?;
    let blur = parse_px(partes[2])?;
    if blur < 0.0 {
        return None;
    }
    let color = parse_color(partes[3])?;
    Some(Sombra { dx, dy, desenfoque: blur, color })
}

/// Borde CSS: "<ancho>px <color>" (p. ej. "3px #1ed760"). El color
/// pasa por parse_color, así que ya sale en lineal (Q4a).
fn parse_borde(valor: &str) -> Option<(f32, [f32; 4])> {
    let partes = dividir_nivel_superior(valor, ' ');
    if partes.len() != 2 {
        return None;
    }
    let ancho = parse_px(partes[0])?;
    if ancho < 0.0 {
        return None;
    }
    let color = parse_color(partes[1])?;
    Some((ancho, color))
}

fn aviso_invalido(avisos: &mut Vec<String>, prop: &str, valor: &str) {
    avisos.push(format!("valor inválido para {prop}: '{valor}'"));
}

/// Una declaración `propiedad: valor` sobre `props`.
fn aplicar_declaracion(props: &mut StyleProps, avisos: &mut Vec<String>, prop: &str, valor: &str) {
    match prop {
        "display" => match valor.to_ascii_lowercase().as_str() {
            "absolute" => props.display = Some(Display::Absoluto),
            "flex" => props.display = Some(Display::Flex),
            "grid" => props.display = Some(Display::Grid),
            "none" => props.display = Some(Display::Ninguno),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "flex-direction" => match valor.to_ascii_lowercase().as_str() {
            "row" | "column" => props.flex_direction = Some(valor.to_ascii_lowercase()),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "gap" => match parse_px(valor) {
            Some(v) if v >= 0.0 => props.gap = Some(v),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "padding" => match parse_lista_px(valor) {
            Some(v) if v.iter().all(|x| *x >= 0.0) => props.padding = Some(expandir_padding(&v)),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "justify-content" => match valor.to_ascii_lowercase().as_str() {
            "start" | "center" | "end" => props.justify_content = Some(valor.to_ascii_lowercase()),
            "flex-start" => props.justify_content = Some("start".into()),
            "flex-end" => props.justify_content = Some("end".into()),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "align-items" => match valor.to_ascii_lowercase().as_str() {
            "start" | "center" | "end" | "stretch" => {
                props.align_items = Some(valor.to_ascii_lowercase())
            }
            "flex-start" => props.align_items = Some("start".into()),
            "flex-end" => props.align_items = Some("end".into()),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "width" => match parse_dimension(valor) {
            Some(d @ Dimension::Px(v)) if v >= 0.0 => props.width = Some(d),
            Some(d @ Dimension::Porcentaje(v)) if v >= 0.0 => props.width = Some(d),
            Some(Dimension::Auto) => props.width = Some(Dimension::Auto),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "height" => match parse_dimension(valor) {
            Some(d @ Dimension::Px(v)) if v >= 0.0 => props.height = Some(d),
            Some(d @ Dimension::Porcentaje(v)) if v >= 0.0 => props.height = Some(d),
            Some(Dimension::Auto) => props.height = Some(Dimension::Auto),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "min-width" => match parse_dimension(valor) {
            Some(d @ Dimension::Px(v)) if v >= 0.0 => props.min_width = Some(d),
            Some(d @ Dimension::Porcentaje(v)) if v >= 0.0 => props.min_width = Some(d),
            Some(Dimension::Auto) => props.min_width = Some(Dimension::Auto),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "min-height" => match parse_dimension(valor) {
            Some(d @ Dimension::Px(v)) if v >= 0.0 => props.min_height = Some(d),
            Some(d @ Dimension::Porcentaje(v)) if v >= 0.0 => props.min_height = Some(d),
            Some(Dimension::Auto) => props.min_height = Some(Dimension::Auto),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "max-width" => match parse_dimension(valor) {
            Some(d @ Dimension::Px(v)) if v >= 0.0 => props.max_width = Some(d),
            Some(d @ Dimension::Porcentaje(v)) if v >= 0.0 => props.max_width = Some(d),
            Some(Dimension::Auto) => props.max_width = Some(Dimension::Auto),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "max-height" => match parse_dimension(valor) {
            Some(d @ Dimension::Px(v)) if v >= 0.0 => props.max_height = Some(d),
            Some(d @ Dimension::Porcentaje(v)) if v >= 0.0 => props.max_height = Some(d),
            Some(Dimension::Auto) => props.max_height = Some(Dimension::Auto),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "aspect-ratio" => {
            let v = valor.trim();
            let ratio = if let Some((a, b)) = v.split_once('/') {
                let (a, b): (f32, f32) =
                    match (a.trim().parse(), b.trim().parse()) {
                        (Ok(a), Ok(b)) => (a, b),
                        _ => (f32::NAN, 1.0),
                    };
                if b != 0.0 { a / b } else { f32::NAN }
            } else {
                v.parse().unwrap_or(f32::NAN)
            };
            if ratio.is_finite() && ratio > 0.0 {
                props.aspect_ratio = Some(ratio);
            } else {
                aviso_invalido(avisos, prop, valor);
            }
        }
        "background-color" => match parse_color(valor) {
            Some(c) => props.background_color = Some(c),
            None => aviso_invalido(avisos, prop, valor),
        },
        "background" => match parse_gradiente(valor) {
            Some(g) => props.background_gradient = Some(g),
            None => aviso_invalido(avisos, prop, valor),
        },
        "border-radius" => match parse_lista_px(valor) {
            Some(v) if v.iter().all(|x| *x >= 0.0) => props.border_radius = Some(expandir_radio(&v)),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "border" => match parse_borde(valor) {
            Some(b) => props.border = Some(b),
            None => aviso_invalido(avisos, prop, valor),
        },
        "box-shadow" => match parse_sombra(valor) {
            Some(s) => props.box_shadow = Some(s),
            None => aviso_invalido(avisos, prop, valor),
        },
        "opacity" => match valor.trim().parse::<f32>() {
            Ok(v) if v.is_finite() && (0.0..=1.0).contains(&v) => props.opacity = Some(v),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "overflow" => match valor.to_ascii_lowercase().as_str() {
            "visible" | "hidden" => props.overflow = Some(valor.to_ascii_lowercase()),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "color" => match parse_color(valor) {
            Some(c) => props.color = Some(c),
            None => aviso_invalido(avisos, prop, valor),
        },
        "font-size" => match parse_px(valor) {
            Some(v) if v >= 0.0 => props.font_size = Some(v),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "letter-spacing" => match parse_px(valor) {
            Some(v) if v >= 0.0 => props.letter_spacing = Some(v),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "grid-template-columns" => match parse_columnas_grid(valor) {
            Some(c) => props.grid_template_columns = Some(c),
            None => aviso_invalido(avisos, prop, valor),
        },
        "sort-order" => match valor.trim().parse::<i32>() {
            Ok(v) => props.sort_order = Some(v),
            Err(_) => aviso_invalido(avisos, prop, valor),
        },
        "override-sorting" => match valor.trim().to_ascii_lowercase().as_str() {
            "true" => props.override_sorting = Some(true),
            "false" => props.override_sorting = Some(false),
            _ => aviso_invalido(avisos, prop, valor),
        },
        "text-overflow" => match valor.to_ascii_lowercase().as_str() {
            "wrap" => props.text_overflow = Some(TextOverflow::Envolver),
            "truncate" => props.text_overflow = Some(TextOverflow::Truncar),
            "ellipsis" => props.text_overflow = Some(TextOverflow::PuntosSuspensivos),
            _ => aviso_invalido(avisos, prop, valor),
        },
        _ => avisos.push(format!("propiedad desconocida: '{prop}'")),
    }
}

/// Todas las declaraciones de `fuente` (separadas por `;`).
fn aplicar_declaraciones(props: &mut StyleProps, avisos: &mut Vec<String>, fuente: &str) {
    for decl in dividir_nivel_superior(fuente, ';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        let Some((prop, valor)) = decl.split_once(':') else {
            avisos.push(format!("declaración sin ':': '{decl}'"));
            continue;
        };
        aplicar_declaracion(props, avisos, prop.trim().to_ascii_lowercase().as_str(), valor.trim());
    }
}

/// Parser del `style_extra`: declaraciones + bloques de pseudo-clase.
pub fn parsear_estilo_extra(texto: &str) -> EstilosExtra {
    let mut salida = EstilosExtra::default();
    let (base, bloques, mut avisos) = extraer_bloques(texto);
    aplicar_declaraciones(&mut salida.base, &mut avisos, &base);
    for (nombre, interior) in bloques {
        let capa = match nombre.as_str() {
            "hover" => &mut salida.hover,
            "active" => &mut salida.active,
            "disabled" => &mut salida.disabled,
            "focus" => &mut salida.focus,
            "selected" => &mut salida.selected,
            otro => {
                avisos.push(format!("pseudo-clase no soportada: :{otro}"));
                continue;
            }
        };
        aplicar_declaraciones(capa, &mut avisos, &interior);
    }
    salida.avisos = avisos;
    salida
}

macro_rules! fusionar_campo {
    ($destino:expr, $capa:expr, $campo:ident) => {
        if $capa.$campo.is_some() {
            $destino.$campo = $capa.$campo.clone();
        }
    };
}

/// Fusión campo a campo: lo fijado en `capa` gana sobre `destino`.
fn fusionar(destino: &mut StyleProps, capa: &StyleProps) {
    fusionar_campo!(destino, capa, display);
    fusionar_campo!(destino, capa, flex_direction);
    fusionar_campo!(destino, capa, gap);
    fusionar_campo!(destino, capa, padding);
    fusionar_campo!(destino, capa, justify_content);
    fusionar_campo!(destino, capa, align_items);
    fusionar_campo!(destino, capa, width);
    fusionar_campo!(destino, capa, height);
    fusionar_campo!(destino, capa, min_width);
    fusionar_campo!(destino, capa, min_height);
    fusionar_campo!(destino, capa, max_width);
    fusionar_campo!(destino, capa, max_height);
    fusionar_campo!(destino, capa, aspect_ratio);
    fusionar_campo!(destino, capa, background_color);
    fusionar_campo!(destino, capa, background_gradient);
    fusionar_campo!(destino, capa, border_radius);
    fusionar_campo!(destino, capa, border);
    fusionar_campo!(destino, capa, box_shadow);
    fusionar_campo!(destino, capa, opacity);
    fusionar_campo!(destino, capa, overflow);
    fusionar_campo!(destino, capa, color);
    fusionar_campo!(destino, capa, font_size);
    fusionar_campo!(destino, capa, letter_spacing);
    fusionar_campo!(destino, capa, grid_template_columns);
    fusionar_campo!(destino, capa, sort_order);
    fusionar_campo!(destino, capa, override_sorting);
    fusionar_campo!(destino, capa, text_overflow);
}

/// Defecto del tipo: absoluto; con LayoutGroup, flex del grupo.
fn defecto_tipo(nodo: &NodoDocumento) -> StyleProps {
    let (display, flex_direction, gap, padding, justify_content) =
        match &nodo.layout_group {
            Some(grupo) => (
                Display::Flex,
                Some(grupo.direction.clone()),
                if grupo.spacing > 0.0 {
                    Some(grupo.spacing)
                } else {
                    None
                },
                Some(grupo.padding),
                Some(grupo.alignment.clone()),
            ),
            None => (Display::Absoluto, None, None, None, None),
        };
    StyleProps {
        display: Some(display),
        flex_direction,
        gap,
        padding,
        justify_content,
        ..StyleProps::default()
    }
}

/// Capa del componente: texto, tinte de imagen y color del botón.
/// La prioridad declarada es imagen > texto > botón: el botón aporta
/// su color PRIMERO para que el tinte de la Image lo sobrescriba.
/// (Antes era al revés y el azul por defecto del Button pisaba el
/// tinte de la Image salvo que el CSS tuviera `background-color`;
/// hallazgo de los peces de fin de módulo V4.)
fn capa_componente(nodo: &NodoDocumento) -> StyleProps {
    let mut p = StyleProps::default();
    if let Some(texto) = &nodo.texto {
        p.color = Some(texto.color);
        p.font_size = Some(texto.font_size);
    }
    if let Some(boton) = &nodo.boton {
        p.background_color = Some(boton.target_graphic_color);
    }
    if let Some(imagen) = &nodo.imagen {
        p.background_color = Some(imagen.color);
    }
    p
}

/// Cascada completa de un nodo con un estado dado (P1 + P2).
pub fn resolver_estilos(nodo: &NodoDocumento, estado: &EstadoPseudo) -> StyleProps {
    let mut salida = defecto_tipo(nodo);
    let componente = capa_componente(nodo);
    fusionar(&mut salida, &componente);
    let extra = parsear_estilo_extra(&nodo.style_extra);
    fusionar(&mut salida, &extra.base);
    let capa = if estado.disabled {
        Some(&extra.disabled)
    } else if estado.active {
        Some(&extra.active)
    } else if estado.hover {
        Some(&extra.hover)
    } else if estado.focus {
        Some(&extra.focus)
    } else if estado.selected {
        Some(&extra.selected)
    } else {
        None
    };
    if let Some(capa) = capa {
        fusionar(&mut salida, capa);
    }
    salida
}

/// Posición del primer bloque de pseudo-clase (`:hover{`, `:active{`,
/// ...) en el CSS extra. Todo lo anterior es el bloque base. Las
/// declaraciones se escriben siempre en el bloque base; los bloques de
/// pseudo-clase no se tocan (su contenido no tiene efecto en runtime
/// hasta V6, deuda D-V4-1).
fn fin_bloque_base(css: &str) -> usize {
    let b = css.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b':' {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-') {
                j += 1;
            }
            let mut k = j;
            while k < b.len() && (b[k] == b' ' || b[k] == b'\t') {
                k += 1;
            }
            if j > i + 1 && k < b.len() && b[k] == b'{' {
                return i;
            }
        }
        i += 1;
    }
    css.len()
}

/// Reescribe (o añade) una declaración `prop: valor;` en el bloque base
/// del CSS extra, sin tocar los bloques de pseudo-clase. Si la propiedad
/// ya existe en el base se sustituye su valor; si no, se añade al final
/// del bloque base.
pub fn establecer_declaracion(css: &str, prop: &str, valor: &str) -> String {
    let fin = fin_bloque_base(css);
    let (base, resto) = css.split_at(fin);
    // Buscar `prop` como nombre completo de declaración en el base.
    let mut inicio = 0;
    while let Some(rel) = base[inicio..].find(prop) {
        let pos = inicio + rel;
        let antes_ok = pos == 0
            || matches!(
                base.as_bytes()[pos - 1],
                b' ' | b'\t' | b'\n' | b';' | b'{'
            );
        let despues = &base[pos + prop.len()..];
        let despues_ok = despues
            .trim_start()
            .as_bytes()
            .first()
            .is_some_and(|c| *c == b':');
        if antes_ok && despues_ok {
            let fin_declaracion = base[pos..]
                .find(';')
                .map(|r| pos + r + 1)
                .unwrap_or(base.len());
            return format!(
                "{}{}: {};{}{}",
                &base[..pos],
                prop,
                valor,
                &base[fin_declaracion..],
                resto
            );
        }
        inicio = pos + prop.len();
    }
    let separador = if base.is_empty() {
        ""
    } else if base.ends_with(';') {
        " "
    } else {
        "; "
    };
    format!("{base}{separador}{prop}: {valor};{resto}")
}

/// ¿Todos los f32 de un `StyleProps` son finitos? (para el fuzz).
#[cfg(test)]
fn estilos_validos(p: &StyleProps) -> bool {
    let mut ok = true;
    let mut f = |v: f32| ok &= v.is_finite();
    if let Some(v) = p.gap {
        f(v);
    }
    if let Some(v) = p.aspect_ratio {
        f(v);
    }
    if let Some(v) = p.opacity {
        f(v);
    }
    if let Some(v) = p.font_size {
        f(v);
    }
    for arr in [p.padding, p.border_radius].into_iter().flatten() {
        for v in arr {
            f(v);
        }
    }
    if let Some(c) = p.background_color {
        for v in c {
            f(v);
        }
    }
    if let Some(c) = p.color {
        for v in c {
            f(v);
        }
    }
    if let Some((a, paradas)) = &p.background_gradient {
        f(*a);
        for c in paradas {
            for v in c {
                f(*v);
            }
        }
    }
    if let Some(s) = &p.box_shadow {
        f(s.dx);
        f(s.dy);
        f(s.desenfoque);
        for v in s.color {
            f(v);
        }
    }
    if let Some(cols) = &p.grid_template_columns {
        for c in cols {
            match c {
                TamanoColumna::Fija(v) | TamanoColumna::Fraccion(v) | TamanoColumna::Relleno(v) => {
                    f(*v);
                }
            }
        }
    }
    let mut dim = |d: Option<Dimension>| match d {
        Some(Dimension::Px(v)) | Some(Dimension::Porcentaje(v)) => f(v),
        _ => {}
    };
    dim(p.width);
    dim(p.height);
    dim(p.min_width);
    dim(p.min_height);
    dim(p.max_width);
    dim(p.max_height);
    ok
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::documento::TipoGraphic;
    use glass_core::scene::{RectTransformData, UIButtonData, UIImageData, UITextData};

    fn nodo_texto(extra: &str) -> NodoDocumento {
        NodoDocumento {
            entidad: 1, nombre: String::new(),
            padre: None,
            hijos: vec![],
            graphic: TipoGraphic::Texto,
            es_canvas: false,
            es_canvas_raiz: false,
            activo: true,
            rect_transform: RectTransformData {
                anchored_position: [0.0, 0.0],
                size_delta: [100.0, 50.0],
                anchor_min: [0.0, 0.0],
                anchor_max: [0.0, 0.0],
                pivot: [0.5, 0.5],
            },
            canvas: None,
            imagen: None,
            texto: Some(UITextData {
                text: "Hola".into(),
                font_size: 14.0,
                color: [0.0, 0.0, 0.0, 1.0],
                font: "default".into(),
                align: "Left".into(),
                style_extra: String::new(),
                raycast_target: true,
            }),
            boton: None,
            drag: None,
            layout_group: None,
            layout_element: None,
            content_size_fitter: None,
            style_extra: extra.to_string(),
            raycast_target: true,
        }
    }

    fn estado_hover() -> EstadoPseudo {
        EstadoPseudo { hover: true, ..EstadoPseudo::default() }
    }

    #[test]
    fn pseudo_hover_resuelve_con_estado_sintetico() {
        let nodo = nodo_texto(":hover{ color: rgba(0, 255, 0, 1); }");
        assert_eq!(resolver_estilos(&nodo, &estado_hover()).color, Some([0.0, 1.0, 0.0, 1.0]));
        // Sin hover, manda el color del componente (negro).
        assert_eq!(resolver_estilos(&nodo, &EstadoPseudo::default()).color, Some([0.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn pseudo_active_resuelve_con_estado_sintetico() {
        let extra = parsear_estilo_extra(":active{ opacity: 0.5; }");
        assert_eq!(extra.active.opacity, Some(0.5));
        assert_eq!(extra.base.opacity, None);
        let nodo = nodo_texto(":active{ opacity: 0.5; }");
        let estado = EstadoPseudo { active: true, ..EstadoPseudo::default() };
        assert_eq!(resolver_estilos(&nodo, &estado).opacity, Some(0.5));
    }

    #[test]
    fn pseudo_disabled_gana_sobre_hover() {
        let nodo = nodo_texto(":hover{ opacity: 0.8; } :disabled{ opacity: 0.2; }");
        let estado = EstadoPseudo { hover: true, disabled: true, ..EstadoPseudo::default() };
        assert_eq!(resolver_estilos(&nodo, &estado).opacity, Some(0.2));
    }

    #[test]
    fn pseudo_focus_resuelve_con_estado_sintetico() {
        let extra = parsear_estilo_extra(":focus{ border-radius: 4px; }");
        assert_eq!(extra.focus.border_radius, Some([4.0, 4.0, 4.0, 4.0]));
        let nodo = nodo_texto(":focus{ border-radius: 4px; }");
        let estado = EstadoPseudo { focus: true, ..EstadoPseudo::default() };
        assert_eq!(resolver_estilos(&nodo, &estado).border_radius, Some([4.0, 4.0, 4.0, 4.0]));
    }

    #[test]
    fn pseudo_selected_resuelve_con_estado_sintetico() {
        let extra = parsear_estilo_extra(":selected{ background-color: #ff0000; }");
        assert_eq!(extra.selected.background_color, Some([1.0, 0.0, 0.0, 1.0]));
        let nodo = nodo_texto(":selected{ background-color: #ff0000; }");
        let estado = EstadoPseudo { selected: true, ..EstadoPseudo::default() };
        assert_eq!(
            resolver_estilos(&nodo, &estado).background_color,
            Some([1.0, 0.0, 0.0, 1.0])
        );
    }

    #[test]
    fn cascada_componente_vs_extra_gana_el_extra() {
        // Componente: texto negro 14px. Extra: rojo 20px.
        let nodo = nodo_texto("color: rgba(255, 0, 0, 1); font-size: 20px;");
        let resuelto = resolver_estilos(&nodo, &EstadoPseudo::default());
        assert_eq!(resuelto.color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(resuelto.font_size, Some(20.0));
        // Sin extra, mandan los del componente.
        let nodo2 = nodo_texto("");
        let resuelto2 = resolver_estilos(&nodo2, &EstadoPseudo::default());
        assert_eq!(resuelto2.color, Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(resuelto2.font_size, Some(14.0));
    }

    /// H2 (peces V4): la prioridad declarada es imagen > texto > botón.
    /// Con colores divergentes, el tinte de la Image debe ganar sobre el
    /// `target_graphic_color` del Button; sin Image, manda el del botón.
    #[test]
    fn imagen_gana_a_target_graphic_color_del_boton() {
        fn nodo_boton(imagen: Option<[f32; 4]>, boton_color: [f32; 4]) -> NodoDocumento {
            NodoDocumento {
                entidad: 1,
                nombre: String::new(),
                padre: None,
                hijos: vec![],
                graphic: TipoGraphic::Imagen,
                es_canvas: false,
                es_canvas_raiz: false,
                activo: true,
                rect_transform: RectTransformData {
                    anchored_position: [0.0, 0.0],
                    size_delta: [100.0, 50.0],
                    anchor_min: [0.0, 0.0],
                    anchor_max: [0.0, 0.0],
                    pivot: [0.5, 0.5],
                },
                canvas: None,
                imagen: imagen.map(|c| UIImageData {
                    color: c,
                    ..UIImageData::default()
                }),
                texto: None,
                boton: Some(UIButtonData {
                    target_graphic_color: boton_color,
                    ..UIButtonData::default()
                }),
                drag: None,
                layout_group: None,
                layout_element: None,
            content_size_fitter: None,
                style_extra: String::new(),
                raycast_target: true,
            }
        }
        // Imagen roja + botón azul (el azul por defecto del editor):
        // gana la imagen.
        let nodo = nodo_boton(Some([1.0, 0.0, 0.0, 1.0]), [0.25, 0.55, 0.85, 1.0]);
        assert_eq!(
            resolver_estilos(&nodo, &EstadoPseudo::default()).background_color,
            Some([1.0, 0.0, 0.0, 1.0])
        );
        // Sin imagen, manda el color del botón.
        let nodo2 = nodo_boton(None, [0.25, 0.55, 0.85, 1.0]);
        assert_eq!(
            resolver_estilos(&nodo2, &EstadoPseudo::default()).background_color,
            Some([0.25, 0.55, 0.85, 1.0])
        );
    }

    #[test]
    fn declaracion_invalida_se_ignora_con_aviso() {
        let extra = parsear_estilo_extra("width: 10em; color: red; opacity: 2;");
        assert_eq!(extra.base.width, None);
        assert_eq!(extra.base.color, None);
        assert_eq!(extra.base.opacity, None);
        assert_eq!(extra.avisos.len(), 3);
    }

    #[test]
    fn padding_y_radio_usan_la_taquiografia_css() {
        let extra = parsear_estilo_extra("padding: 4px 8px; border-radius: 2px 4px 6px 8px;");
        assert_eq!(extra.base.padding, Some([8.0, 4.0, 8.0, 4.0]));
        assert_eq!(extra.base.border_radius, Some([2.0, 4.0, 6.0, 8.0]));
        let extra2 = parsear_estilo_extra("padding: 5px;");
        assert_eq!(extra2.base.padding, Some([5.0, 5.0, 5.0, 5.0]));
    }

    #[test]
    fn degradado_y_sombra_se_parsean() {
        let extra = parsear_estilo_extra(
            "background: linear-gradient(90deg, rgba(0, 0, 0, 1) 0%, rgba(255, 255, 255, 1) 100%); \
             box-shadow: 2px 3px 4px rgba(0, 0, 0, 0.5);",
        );
        assert!(extra.base.background_gradient.is_some());
        let (angulo, paradas) = extra.base.background_gradient.unwrap();
        assert_eq!(angulo, 0.0);
        assert_eq!(paradas.len(), 2);
        let sombra = extra.base.box_shadow.unwrap();
        assert_eq!((sombra.dx, sombra.dy, sombra.desenfoque), (2.0, 3.0, 4.0));
        assert_eq!(sombra.color, [0.0, 0.0, 0.0, 0.5]);
    }

    #[test]
    fn dimensiones_aceptan_px_porcentaje_y_auto() {
        let extra = parsear_estilo_extra("width: 120px; height: 50%; min-width: auto;");
        assert_eq!(extra.base.width, Some(Dimension::Px(120.0)));
        assert_eq!(extra.base.height, Some(Dimension::Porcentaje(50.0)));
        assert_eq!(extra.base.min_width, Some(Dimension::Auto));
    }

    /// Generador pseudoaleatorio con semilla fija (LCG, sin dependencias).
    struct Azar {
        estado: u64,
    }

    impl Azar {
        fn nuevo(semilla: u64) -> Self {
            Self { estado: semilla }
        }
        fn siguiente(&mut self) -> u64 {
            self.estado = self
                .estado
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.estado >> 33
        }
        fn elegir<'a>(&mut self, lista: &'a [&'a str]) -> &'a str {
            lista[(self.siguiente() as usize) % lista.len()]
        }
    }

    #[test]
    fn fuzz_500_entradas_sin_panico_ni_valores_invalidos() {
        let props = [
            "width", "height", "display", "color", "opacity", "gap", "padding",
            "border-radius", "background-color", "font-size", "unknown-prop",
            "WIDTH", "justify-content", "overflow", "aspect-ratio", "box-shadow",
            "background", "min-width", "flex-direction",
        ];
        let valores = [
            "10px", "50%", "auto", "red", "rgba(1, 2, 3, 0.5)", "-5px", "10em",
            "NaN", "inf", "1e3px", "#ff00aa", "#gg",
            "linear-gradient(90deg, rgba(0, 0, 0, 1) 0%, rgba(255, 255, 255, 1) 100%)",
            "row", "column", "flex", "visible", "hidden", "0.5", "2", "", "  ",
            "}", "{", ";", "16 / 9", "2px 2px 4px rgba(0, 0, 0, 0.5)",
        ];
        let pseudos = ["hover", "active", "disabled", "focus", "selected", "visited", ""];
        let mut azar = Azar::nuevo(0xC0FFEE);
        for _ in 0..500 {
            let mut entrada = String::new();
            let n = 1 + (azar.siguiente() % 4) as usize;
            for _ in 0..n {
                match azar.siguiente() % 4 {
                    0 => {
                        entrada.push_str(azar.elegir(&props));
                        entrada.push_str(": ");
                        entrada.push_str(azar.elegir(&valores));
                        entrada.push(';');
                    }
                    1 => {
                        entrada.push(':');
                        entrada.push_str(azar.elegir(&pseudos));
                        entrada.push('{');
                        entrada.push_str(azar.elegir(&props));
                        entrada.push_str(": ");
                        entrada.push_str(azar.elegir(&valores));
                        entrada.push_str("; }");
                    }
                    2 => entrada.push_str(azar.elegir(&valores)),
                    _ => entrada.push_str("; } { : "),
                }
                entrada.push(' ');
            }
            let extra = parsear_estilo_extra(&entrada);
            for capa in [&extra.base, &extra.hover, &extra.active, &extra.disabled, &extra.focus, &extra.selected] {
                assert!(estilos_validos(capa), "StyleProps inválido con entrada: {entrada:?}");
            }
        }
    }

    #[test]
    fn text_overflow_parsea_los_tres_modos_y_rechaza_valor_desconocido() {
        let r = parsear_estilo_extra("text-overflow: ellipsis;");
        assert_eq!(r.base.text_overflow, Some(TextOverflow::PuntosSuspensivos));

        let r = parsear_estilo_extra("text-overflow: truncate;");
        assert_eq!(r.base.text_overflow, Some(TextOverflow::Truncar));

        let r = parsear_estilo_extra("text-overflow: wrap;");
        assert_eq!(r.base.text_overflow, Some(TextOverflow::Envolver));

        // Un valor desconocido no asigna nada y genera aviso.
        let r = parsear_estilo_extra("text-overflow: inventado;");
        assert_eq!(r.base.text_overflow, None);
        assert!(!r.avisos.is_empty());
    }

    #[test]
    fn outline_no_existe_como_propiedad() {
        // Hallazgo pez 2 (fin de módulo V3): V3b afirma contorno
        // "verificado vacuo" sin ninguna comprobación. No hay propiedad
        // de contorno en V3: cualquier declaración outline se rechaza
        // con aviso y no asigna nada.
        let r = parsear_estilo_extra("outline: 2px solid red;");
        assert_eq!(r.base.text_overflow, None);
        assert!(!r.avisos.is_empty());
        let r = parsear_estilo_extra("text-outline: 1px;");
        assert!(!r.avisos.is_empty());
    }

    #[test]
    fn fusionar_text_overflow_respeta_la_capa() {
        let mut destino = StyleProps::default();
        let capa = StyleProps {
            text_overflow: Some(TextOverflow::Truncar),
            ..StyleProps::default()
        };
        fusionar(&mut destino, &capa);
        assert_eq!(destino.text_overflow, Some(TextOverflow::Truncar));

        // Una capa vacía posterior no borra el valor ya fusionado.
        let vacia = StyleProps::default();
        fusionar(&mut destino, &vacia);
        assert_eq!(destino.text_overflow, Some(TextOverflow::Truncar));
    }

    #[test]
    fn borde_se_parsea() {
        let extra = parsear_estilo_extra("border: 3px #1ed760;");
        let (ancho, color) = extra.base.border.unwrap();
        assert_eq!(ancho, 3.0);
        assert!(color[1] > 0.5);
    }

    #[test]
    fn borde_invalido_da_aviso_y_no_panico() {
        let extra = parsear_estilo_extra("border: 3px;");
        assert_eq!(extra.base.border, None);
        assert!(!extra.avisos.is_empty());
    }

    #[test]
    fn letter_spacing_se_parsea_y_rechaza_negativo() {
        let extra = parsear_estilo_extra("letter-spacing: 8px;");
        assert_eq!(extra.base.letter_spacing, Some(8.0));
        let extra2 = parsear_estilo_extra("letter-spacing: -2px;");
        assert_eq!(extra2.base.letter_spacing, None);
    }

    #[test]
    fn color_hex_se_convierte_a_lineal() {
        let extra = parsear_estilo_extra("background-color: #0a1531;");
        let c = extra.base.background_color.unwrap();
        assert!((c[0] - 0.0030).abs() < 0.001);
        assert!((c[1] - 0.0075).abs() < 0.001);
        assert!((c[2] - 0.0308).abs() < 0.001);
        assert_eq!(c[3], 1.0);
    }

    #[test]
    fn color_rgba_medio_se_convierte_a_lineal() {
        let extra = parsear_estilo_extra("color: rgba(128, 128, 128, 1);");
        let c = extra.base.color.unwrap();
        assert!((c[0] - 0.2159).abs() < 0.001);
        assert_eq!(c[3], 1.0);
    }

    #[test]
    fn angulo_degradado_sigue_css() {
        let extra = parsear_estilo_extra("background: linear-gradient(90deg, #000000, #ffffff);");
        let (angulo, paradas) = extra.base.background_gradient.unwrap();
        assert_eq!(angulo, 0.0);
        assert_eq!(paradas.len(), 2);
    }

    #[test]
    fn color_a_css_redondea_a_hex() {
        let extra = parsear_estilo_extra("background-color: #1ed760;");
        let c = extra.base.background_color.unwrap();
        assert_eq!(color_a_css(c), "#1ed760");
        let extra2 = parsear_estilo_extra("color: rgba(255, 0, 0, 0.5);");
        let c2 = extra2.base.color.unwrap();
        assert!(color_a_css(c2).starts_with("rgba(255, 0, 0, 0.5"));
    }

    mod tests_establecer_declaracion {
        use super::*;

        #[test]
        fn sustituye_propiedad_existente_en_base() {
            let css = "color: red; opacity: 0.500;";
            let resultado = establecer_declaracion(css, "opacity", "1.000");
            assert_eq!(resultado, "color: red; opacity: 1.000;");
        }

        #[test]
        fn anade_propiedad_nueva_en_base() {
            let css = "color: red;";
            let resultado = establecer_declaracion(css, "opacity", "0.500");
            assert_eq!(resultado, "color: red; opacity: 0.500;");
        }

        #[test]
        fn no_toca_bloque_hover() {
            let css = "color: red;:hover{ color: blue; }";
            let resultado = establecer_declaracion(css, "color", "green");
            assert_eq!(resultado, "color: green;:hover{ color: blue; }");
        }

        #[test]
        fn css_vacio_crea_declaracion() {
            let resultado = establecer_declaracion("", "opacity", "0.500");
            assert_eq!(resultado, "opacity: 0.500;");
        }

        #[test]
        fn base_sin_punto_y_coma_final_anade_separador() {
            let css = "color: red";
            let resultado = establecer_declaracion(css, "opacity", "0.500");
            assert_eq!(resultado, "color: red; opacity: 0.500;");
        }
    }

    mod tests_puente_estilo_parseo {
        use super::*;

        #[test]
        fn style_extra_del_puente_parsea_correctamente() {
            let css = "background-color: #1ed760; border-radius: 4.0px 4.0px 4.0px 4.0px; opacity: 0.500; overflow: hidden; letter-spacing: 2.0px; border: 2.0px #ffffff;";
            let props = parsear_estilo_extra(css).base;

            // background-color: #1ed760 es sRGB (0x1e=30, 0xd7=215, 0x60=96);
            // el parser convierte sRGB→lineal.
            let srgb_a_lineal_local = |c: f32| -> f32 {
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            let bg = props.background_color.expect("background_color debería estar presente");
            assert!((bg[0] - srgb_a_lineal_local(30.0 / 255.0)).abs() < 0.01);
            assert!((bg[1] - srgb_a_lineal_local(215.0 / 255.0)).abs() < 0.01);
            assert!((bg[2] - srgb_a_lineal_local(96.0 / 255.0)).abs() < 0.01);
            assert!((bg[3] - 1.0).abs() < 0.01);
            // El color #1ed760 NO debe ser igual al sRGB sin convertir.
            assert!((bg[0] - 30.0 / 255.0).abs() > 0.001);

            let br = props.border_radius.expect("border_radius debería estar presente");
            assert!((br[0] - 4.0).abs() < 0.1);
            assert!((br[1] - 4.0).abs() < 0.1);
            assert!((br[2] - 4.0).abs() < 0.1);
            assert!((br[3] - 4.0).abs() < 0.1);

            let op = props.opacity.expect("opacity debería estar presente");
            assert!((op - 0.5).abs() < 0.01);

            assert_eq!(props.overflow, Some("hidden".to_string()));

            let ls = props.letter_spacing.expect("letter_spacing debería estar presente");
            assert!((ls - 2.0).abs() < 0.1);

            let (ancho, color_borde) = props.border.expect("border debería estar presente");
            assert!((ancho - 2.0).abs() < 0.1);
            assert!((color_borde[0] - 1.0).abs() < 0.01);
            assert!((color_borde[1] - 1.0).abs() < 0.01);
            assert!((color_borde[2] - 1.0).abs() < 0.01);
            assert!((color_borde[3] - 1.0).abs() < 0.01);
        }
    }

}
