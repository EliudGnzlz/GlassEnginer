
use crate::ui::{GlyphEntry, ATLAS_H, ATLAS_RASTER_PX, ATLAS_W, SOLID_BLOCK};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Distancia de expansión SDF en texels (a 64 px base).
pub const SPREAD_PX: f32 = 4.0;
/// Versión del generador, para invalidar cachés.
pub const GENERADOR_VERSION: u32 = 1;
/// Máximo de glifos insertados por frame bajo demanda.
pub const PRESUPUESTO_POR_FRAME: usize = 8;
/// Número máximo de páginas del atlas.
pub const MAX_PAGINAS: usize = 4;

// ---------------------------------------------------------------------------
// Geometría auxiliar
// ---------------------------------------------------------------------------

struct Segmento {
    a: [f32; 2],
    b: [f32; 2],
}

/// Distancia mínima de un punto a un segmento de línea.
/// Distancia al cuadrado (sin raíz: para comparar candidatos; la raíz
/// se saca una sola vez por texel al final).
fn distancia2_punto_segmento(p: [f32; 2], s: &Segmento) -> f32 {
    let dx = s.b[0] - s.a[0];
    let dy = s.b[1] - s.a[1];
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-12 {
        let ex = p[0] - s.a[0];
        let ey = p[1] - s.a[1];
        return ex * ex + ey * ey;
    }
    let t = ((p[0] - s.a[0]) * dx + (p[1] - s.a[1]) * dy) / len2;
    let t = t.clamp(0.0, 1.0);
    let cx = s.a[0] + t * dx;
    let cy = s.a[1] + t * dy;
    let ex = p[0] - cx;
    let ey = p[1] - cy;
    ex * ex + ey * ey
}

// ---------------------------------------------------------------------------
// Constructor de contorno (ttf_parser::OutlineBuilder)
// ---------------------------------------------------------------------------

struct ConstructorContorno {
    segmentos: Vec<Segmento>,
    pos: [f32; 2],
    inicio: [f32; 2],
    /// Escala ab_glyph (px por unidad de fuente): las longitudes de las
    /// curvas llegan en unidades de fuente y la subdivisión se mide en
    /// texels, así que hay que escalarlas (sin esto, `n` se saturaba a 32
    /// en casi todas las curvas y un glifo tenía ~1000 segmentos).
    k: f32,
}

impl ConstructorContorno {
    fn nuevo(k: f32) -> Self {
        Self {
            segmentos: Vec::new(),
            pos: [0.0; 2],
            inicio: [0.0; 2],
            k,
        }
    }

    /// Número de subdivisiones para aplanar una curva, paso ≤ 0.5 texel, máx 32.
    fn subdivisiones(&self, longitud_aprox: f32) -> usize {
        let n = ((longitud_aprox * self.k) / 0.5).ceil() as usize;
        n.clamp(1, 32)
    }
}

impl ttf_parser::OutlineBuilder for ConstructorContorno {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pos = [x, y];
        self.inicio = [x, y];
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.segmentos.push(Segmento {
            a: self.pos,
            b: [x, y],
        });
        self.pos = [x, y];
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = self.pos;
        let p1 = [x1, y1];
        let p2 = [x, y];
        // Longitud aproximada: distancia de control a los extremos
        let d1 = ((p1[0] - p0[0]).powi(2) + (p1[1] - p0[1]).powi(2)).sqrt();
        let d2 = ((p2[0] - p1[0]).powi(2) + (p2[1] - p1[1]).powi(2)).sqrt();
        let n = self.subdivisiones(d1 + d2);
        let inv = 1.0 / n as f32;
        let mut prev = p0;
        for i in 1..=n {
            let t = i as f32 * inv;
            let u = 1.0 - t;
            let bx = u * u * p0[0] + 2.0 * u * t * p1[0] + t * t * p2[0];
            let by = u * u * p0[1] + 2.0 * u * t * p1[1] + t * t * p2[1];
            self.segmentos.push(Segmento {
                a: prev,
                b: [bx, by],
            });
            prev = [bx, by];
        }
        self.pos = p2;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = self.pos;
        let p1 = [x1, y1];
        let p2 = [x2, y2];
        let p3 = [x, y];
        let d1 = ((p1[0] - p0[0]).powi(2) + (p1[1] - p0[1]).powi(2)).sqrt();
        let d2 = ((p2[0] - p1[0]).powi(2) + (p2[1] - p1[1]).powi(2)).sqrt();
        let d3 = ((p3[0] - p2[0]).powi(2) + (p3[1] - p2[1]).powi(2)).sqrt();
        let n = self.subdivisiones(d1 + d2 + d3);
        let inv = 1.0 / n as f32;
        let mut prev = p0;
        for i in 1..=n {
            let t = i as f32 * inv;
            let u = 1.0 - t;
            let bx = u * u * u * p0[0]
                + 3.0 * u * u * t * p1[0]
                + 3.0 * u * t * t * p2[0]
                + t * t * t * p3[0];
            let by = u * u * u * p0[1]
                + 3.0 * u * u * t * p1[1]
                + 3.0 * u * t * t * p2[1]
                + t * t * t * p3[1];
            self.segmentos.push(Segmento {
                a: prev,
                b: [bx, by],
            });
            prev = [bx, by];
        }
        self.pos = p3;
    }

    fn close(&mut self) {
        let p = self.pos;
        let s = self.inicio;
        if (p[0] - s[0]).abs() > 1e-6 || (p[1] - s[1]).abs() > 1e-6 {
            self.segmentos.push(Segmento { a: p, b: s });
        }
        self.pos = s;
    }
}

// ---------------------------------------------------------------------------
// Página del atlas
// ---------------------------------------------------------------------------

struct Pagina {
    pixels: Vec<u8>,
    glyphs: HashMap<char, GlyphEntry>,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
}

impl Pagina {
    /// Crea una página vacía con un bloque sólido 8×8 blanco opaco en la esquina.
    fn nueva() -> Self {
        let total = (ATLAS_W * ATLAS_H * 4) as usize;
        let mut pixels = vec![0u8; total];
        // Bloque sólido SOLID_BLOCK × SOLID_BLOCK en (0,0), RGBA = (255,255,255,255)
        for y in 0..SOLID_BLOCK {
            for x in 0..SOLID_BLOCK {
                let idx = ((y * ATLAS_W + x) * 4) as usize;
                pixels[idx] = 255;
                pixels[idx + 1] = 255;
                pixels[idx + 2] = 255;
                pixels[idx + 3] = 255;
            }
        }
        Self {
            pixels,
            glyphs: HashMap::new(),
            // La estantería empieza después del bloque sólido
            shelf_x: SOLID_BLOCK + 1,
            shelf_y: 0,
            shelf_h: SOLID_BLOCK + 1,
        }
    }

    /// Intenta colocar una celda de glifo. Devuelve `false` si no cabe.
    fn colocar(&mut self, c: char, celda: &[u8], cw: u32, ch: u32, entry: GlyphEntry) -> bool {
        let margen: u32 = 1;
        let ancho = cw + margen;
        let alto = ch + margen;

        // ¿Cabe en la fila actual?
        if self.shelf_x + ancho > ATLAS_W {
            // Nueva fila
            self.shelf_y += self.shelf_h;
            self.shelf_x = 0;
            self.shelf_h = 0;
        }
        if self.shelf_y + alto > ATLAS_H {
            return false;
        }

        let ox = self.shelf_x;
        let oy = self.shelf_y;

        // Copiar la celda RGBA en la página
        for row in 0..ch {
            for col in 0..cw {
                let src = ((row * cw + col) * 4) as usize;
                let dst = (((oy + row) * ATLAS_W + (ox + col)) * 4) as usize;
                if src + 3 < celda.len() && dst + 3 < self.pixels.len() {
                    self.pixels[dst] = celda[src];
                    self.pixels[dst + 1] = celda[src + 1];
                    self.pixels[dst + 2] = celda[src + 2];
                    self.pixels[dst + 3] = celda[src + 3];
                }
            }
        }

        self.shelf_x = ox + ancho;
        if alto > self.shelf_h {
            self.shelf_h = alto;
        }

        self.glyphs.insert(c, entry);
        true
    }
}

// ---------------------------------------------------------------------------
// SdfAtlas
// ---------------------------------------------------------------------------

pub struct SdfAtlas {
    paginas: Vec<Pagina>,
    glyphs: HashMap<char, GlyphEntry>,
    /// Página (índice en `paginas`) donde vive cada glifo. El UV de la
    /// entrada es local a su página: el render debe muestrear la textura
    /// de esa página (la integración V3b lo usa).
    paginas_por_glifo: HashMap<char, usize>,
    pub ascent: f32,
    pub descent: f32,
    pub line_height: f32,
    pub solid_uv: [f32; 2],
    pub avisos: Vec<String>,
    pub sustituciones: usize,
    inserciones_frame: usize,
    sustitucion: GlyphEntry,
    /// Bytes de la fuente, retenidos para inserción dinámica.
    font_bytes: Vec<u8>,
}

/// Genera la celda SDF para un carácter usando ttf_parser y ab_glyph.
/// Devuelve `Ok(Some((celda_rgba, cw, ch, entry)))` si tiene contorno,
/// `Ok(None)` si no tiene contorno (p.ej. espacio),
/// y `Err(msg)` si falla algo inesperado.
fn generar_celda(
    c: char,
    face: &ttf_parser::Face<'_>,
    font: &FontRef<'_>,
    k: f32,
) -> Result<Option<(Vec<u8>, u32, u32, GlyphEntry)>, String> {
    let glyph_id_ab = font.glyph_id(c);
    let avance_h = font.h_advance_unscaled(glyph_id_ab) * k;

    // Obtener contorno vía ttf_parser
    let glyph_id_ttf = face.glyph_index(c);
    let glyph_id_ttf = match glyph_id_ttf {
        Some(id) => id,
        None => {
            // Sin glifo en la fuente: solo avance
            return Ok(None);
        }
    };

    let mut contorno = ConstructorContorno::nuevo(k);
    let bbox_opt = face.outline_glyph(glyph_id_ttf, &mut contorno);
    if bbox_opt.is_none() || contorno.segmentos.is_empty() {
        // Glifo sin contorno (espacio, etc.): solo avance
        let entry = GlyphEntry {
            uv: [0.0, 0.0, 0.0, 0.0],
            size: [0.0, 0.0],
            bearing: [0.0, 0.0],
            advance: avance_h,
        };
        return Ok(Some((Vec::new(), 0, 0, entry)));
    }

    // Escalar segmentos de unidades de fuente a píxeles
    // ttf_parser da coordenadas en unidades de fuente, Y hacia arriba
    // Convertimos a Y hacia abajo para consistencia con el atlas
    let segmentos_px: Vec<Segmento> = contorno
        .segmentos
        .iter()
        .map(|s| Segmento {
            a: [s.a[0] * k, -s.a[1] * k],
            b: [s.b[0] * k, -s.b[1] * k],
        })
        .collect();

    // Calcular bounding box de los segmentos escalados
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for s in &segmentos_px {
        for p in &[s.a, s.b] {
            if p[0] < min_x { min_x = p[0]; }
            if p[1] < min_y { min_y = p[1]; }
            if p[0] > max_x { max_x = p[0]; }
            if p[1] > max_y { max_y = p[1]; }
        }
    }

    // Celda = bbox + SPREAD_PX de margen por lado
    let celda_x0 = min_x - SPREAD_PX;
    let celda_y0 = min_y - SPREAD_PX;
    let cw = ((max_x - min_x) + 2.0 * SPREAD_PX).ceil() as u32;
    let ch = ((max_y - min_y) + 2.0 * SPREAD_PX).ceil() as u32;

    if cw == 0 || ch == 0 {
        let entry = GlyphEntry {
            uv: [0.0, 0.0, 0.0, 0.0],
            size: [0.0, 0.0],
            bearing: [0.0, 0.0],
            advance: avance_h,
        };
        return Ok(Some((Vec::new(), 0, 0, entry)));
    }

    let mut celda = vec![0u8; (cw * ch * 4) as usize];

    // Precalcular el bbox de cada segmento: la distancia solo importa
    // dentro del spread (más allá se satura a 0/255), así que por texel
    // solo se evalúan los segmentos cuyo bbox está cerca. Por fila se
    // guarda la lista de candidatos (bbox expandido por SPREAD), lo que
    // deja la generación en <1 ms/glifo en vez de ~125 ms.
    let cajas: Vec<[f32; 4]> = segmentos_px
        .iter()
        .map(|s| {
            [
                s.a[0].min(s.b[0]),
                s.a[1].min(s.b[1]),
                s.a[0].max(s.b[0]),
                s.a[1].max(s.b[1]),
            ]
        })
        .collect();
    // Rejilla 2D (buckets de 8 texels) para la distancia: por texel solo
    // se evalúan los segmentos cuyo bbox (expandido por SPREAD) toca su
    // bucket.
    const BUCKET: f32 = 4.0;
    let nbx = ((cw as f32 / BUCKET).ceil() as usize).max(1);
    let nby = ((ch as f32 / BUCKET).ceil() as usize).max(1);
    let mut rejilla: Vec<Vec<usize>> = vec![Vec::new(); nbx * nby];
    for (i, c) in cajas.iter().enumerate() {
        let bx0 = ((c[0] - SPREAD_PX - celda_x0) / BUCKET).floor() as i32;
        let bx1 = ((c[2] + SPREAD_PX - celda_x0) / BUCKET).floor() as i32;
        let by0 = ((c[1] - SPREAD_PX - celda_y0) / BUCKET).floor() as i32;
        let by1 = ((c[3] + SPREAD_PX - celda_y0) / BUCKET).floor() as i32;
        for by in by0.max(0)..=by1.min(nby as i32 - 1) {
            for bx in bx0.max(0)..=bx1.min(nbx as i32 - 1) {
                rejilla[(by as usize) * nbx + bx as usize].push(i);
            }
        }
    }

    // Signo por scanline: por fila, los cruces del contorno con la
    // horizontal del centro del texel, ordenados por x con su winding
    // acumulado. Un contorno cerrado tiene suma total 0, así que el
    // winding en px es -prefijo(px): dentro si el prefijo != 0. Por
    // texel es una búsqueda binaria, no un bucle por segmento.
    let mut cruces_por_fila: Vec<Vec<(f32, i32)>> = vec![Vec::new(); ch as usize];
    for s in &segmentos_px {
        let y0 = s.a[1].min(s.b[1]);
        let y1 = s.a[1].max(s.b[1]);
        if y0 == y1 {
            continue;
        }
        let r0 = ((y0 - celda_y0).floor() as i32).max(0) as usize;
        let r1 = ((y1 - celda_y0).ceil() as i32).min(ch as i32 - 1).max(0) as usize;
        for r in r0..=r1 {
            let py = celda_y0 + r as f32 + 0.5;
            if (s.a[1] <= py && s.b[1] > py) || (s.b[1] <= py && s.a[1] > py) {
                let t = (py - s.a[1]) / (s.b[1] - s.a[1]);
                let x = s.a[0] + t * (s.b[0] - s.a[0]);
                let dir = if s.b[1] > s.a[1] { 1 } else { -1 };
                cruces_por_fila[r].push((x, dir));
            }
        }
    }
    for cruces in cruces_por_fila.iter_mut() {
        cruces.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut acc = 0i32;
        for c in cruces.iter_mut() {
            acc += c.1;
            c.1 = acc;
        }
    }

    for row in 0..ch {
        // Signo por scanline con barrido: los cruces de la fila están
        // ordenados, así que el winding se mantiene al avanzar en x sin
        // búsqueda binaria por texel.
        let cruces = &cruces_por_fila[row as usize];
        let mut ci: usize = 0;
        for col in 0..cw {
            let px = celda_x0 + col as f32 + 0.5;
            let py = celda_y0 + row as f32 + 0.5;
            while ci < cruces.len() && cruces[ci].0 <= px {
                ci += 1;
            }

            // Distancia mínima al contorno (al cuadrado: una sola raíz
            // por texel) con los candidatos del bucket; más allá de
            // SPREAD el valor se satura, así que no hace falta más.
            // Si el bucket está vacío, ningún segmento está a menos de
            // SPREAD del texel: la distancia se satura sin evaluar nada.
            let bi = ((row as f32 / BUCKET) as usize) * nbx + (col as f32 / BUCKET) as usize;
            let mut mejor2 = SPREAD_PX * SPREAD_PX;
            for &i in &rejilla[bi] {
                let s = &segmentos_px[i];
                let c = &cajas[i];
                // Distancia mínima del punto al bbox (al cuadrado): si ya
                // supera el mejor, el segmento no puede mejorar.
                let dx = (c[0] - px).max(0.0).max(px - c[2]);
                let dy = (c[1] - py).max(0.0).max(py - c[3]);
                if dx * dx + dy * dy >= mejor2 {
                    continue;
                }
                let d2 = distancia2_punto_segmento([px, py], s);
                if d2 < mejor2 {
                    mejor2 = d2;
                }
            }
            let dist_min = mejor2.sqrt();

            // Dentro si el winding acumulado hasta px es distinto de 0.
            let dentro = if ci == 0 { false } else { cruces[ci - 1].1 != 0 };
            let dist_con_signo = if dentro { dist_min } else { -dist_min };

            // Codificar: a = 128 + dist * 128 / SPREAD_PX
            let a_f = 128.0 + dist_con_signo * 128.0 / SPREAD_PX;
            let a = a_f.round().clamp(0.0, 255.0) as u8;

            let idx = ((row * cw + col) * 4) as usize;
            celda[idx] = 255;     // R
            celda[idx + 1] = 255; // G
            celda[idx + 2] = 255; // B
            celda[idx + 3] = a;   // A
        }
    }

    // La entry se rellenará con UV tras la colocación; aquí solo bearing/size/advance
    let entry = GlyphEntry {
        uv: [0.0, 0.0, 0.0, 0.0], // Placeholder, se reescribe al colocar
        size: [cw as f32, ch as f32],
        bearing: [celda_x0, celda_y0],
        advance: avance_h,
    };

    Ok(Some((celda, cw, ch, entry)))
}

/// Coloca una celda en las páginas del atlas, creando páginas nuevas si es necesario.
/// Devuelve la GlyphEntry con UV correctos, o None si no cabe.
fn colocar_en_paginas(
    paginas: &mut Vec<Pagina>,
    c: char,
    celda: &[u8],
    cw: u32,
    ch: u32,
    mut entry: GlyphEntry,
) -> Option<(GlyphEntry, usize)> {
    // Intentar en cada página existente
    for (pi, pagina) in paginas.iter_mut().enumerate() {
        let ox = pagina.shelf_x;
        let oy = pagina.shelf_y;
        // Comprobar si cabría
        let margen: u32 = 1;
        let ancho = cw + margen;
        let alto = ch + margen;
        let mut tx = ox;
        let mut ty = oy;
        let th = pagina.shelf_h;
        if tx + ancho > ATLAS_W {
            ty += th;
            tx = 0;
        }
        if ty + alto <= ATLAS_H {
            // Calcular UV antes de colocar
            let u0 = tx as f32 / ATLAS_W as f32;
            let v0 = ty as f32 / ATLAS_H as f32;
            let u1 = (tx + cw) as f32 / ATLAS_W as f32;
            let v1 = (ty + ch) as f32 / ATLAS_H as f32;
            // Offset de página en V: cada página ocupa un rango [0..1] propio,
            // pero como usamos páginas separadas, basta con UV en [0..1].
            // Indexamos la página por el índice; el UV es local a la página.
            entry.uv = [u0, v0, u1, v1];
            if pagina.colocar(c, celda, cw, ch, entry) {
                return Some((entry, pi));
            }
        }
    }

    // Crear nueva página si no excedemos el tope
    if paginas.len() < MAX_PAGINAS {
        let mut nueva = Pagina::nueva();
        let tx = nueva.shelf_x;
        let ty = nueva.shelf_y;
        let u0 = tx as f32 / ATLAS_W as f32;
        let v0 = ty as f32 / ATLAS_H as f32;
        let u1 = (tx + cw) as f32 / ATLAS_W as f32;
        let v1 = (ty + ch) as f32 / ATLAS_H as f32;
        entry.uv = [u0, v0, u1, v1];
        if nueva.colocar(c, celda, cw, ch, entry) {
            let idx = paginas.len();
            paginas.push(nueva);
            return Some((entry, idx));
        }
    }

    None
}

impl SdfAtlas {
    /// Construye el atlas SDF con el conjunto inicial de 191 glifos (ASCII 32–126 + Latin-1 160–255).
    pub fn build(font_bytes: &[u8]) -> Result<Self, String> {
        let font = FontRef::try_from_slice(font_bytes)
            .map_err(|e| format!("ab_glyph no pudo cargar la fuente: {e}"))?;
        let face = ttf_parser::Face::parse(font_bytes, 0)
            .map_err(|e| format!("ttf_parser no pudo cargar la fuente: {e}"))?;

        // HALLAZGO V3a: en ab_glyph, PxScale es la ALTURA DE LÍNEA
        // (ascenso - descenso), no el EM. A 64 px nominales el factor es
        // 64 / (ascenso - descenso) = 64/2384 ≈ 0,0268 (no 64/2048).
        // Si se usa 64/upem, la geometría SDF sale ~14 % más grande que
        // la rasterización directa de ab_glyph y la puerta G-SDF1 no se
        // puede cumplir. Por eso k viene del factor de ab_glyph, que es
        // además la escala que usan el layout (MedidorTexto) y el atlas
        // de bits: avances y métricas quedan consistentes con ellos.
        let k = font
            .as_scaled(PxScale::from(ATLAS_RASTER_PX))
            .scale_factor()
            .horizontal;

        let ascent = face.ascender() as f32 * k;
        let descent = face.descender() as f32 * k;
        let line_gap = face.line_gap() as f32 * k;
        let line_height = ascent - descent + line_gap;

        let solid_uv = [
            (SOLID_BLOCK as f32 * 0.5) / ATLAS_W as f32,
            (SOLID_BLOCK as f32 * 0.5) / ATLAS_H as f32,
        ];

        let mut paginas = vec![Pagina::nueva()];
        let mut glyphs: HashMap<char, GlyphEntry> = HashMap::new();
        let mut paginas_por_glifo: HashMap<char, usize> = HashMap::new();
        let mut avisos: Vec<String> = Vec::new();
        let mut sustituciones: usize = 0;

        // Conjunto inicial: ASCII 32–126 + Latin-1 160–255
        let conjunto: Vec<char> = (32u8..=126)
            .chain(160u8..=255)
            .map(|b| b as char)
            .collect();

        // Primero generar '?' para tener sustitución
        let sustitucion: GlyphEntry;
        match generar_celda('?', &face, &font, k) {
            Ok(Some((celda, cw, ch, entry))) if !celda.is_empty() => {
                match colocar_en_paginas(&mut paginas, '?', &celda, cw, ch, entry) {
                    Some((e, pg)) => {
                        glyphs.insert('?', e);
                        paginas_por_glifo.insert('?', pg);
                        sustitucion = e;
                    }
                    None => {
                        // Fallback: celda sólida 8×8
                        let e = GlyphEntry {
                            uv: [0.0, 0.0, SOLID_BLOCK as f32 / ATLAS_W as f32, SOLID_BLOCK as f32 / ATLAS_H as f32],
                            size: [SOLID_BLOCK as f32, SOLID_BLOCK as f32],
                            bearing: [0.0, 0.0],
                            advance: ATLAS_RASTER_PX * 0.5,
                        };
                        glyphs.insert('?', e);
                        sustitucion = e;
                    }
                }
            }
            Ok(Some((_celda, _cw, _ch, entry))) => {
                // '?' sin contorno visible (solo avance)
                let e = GlyphEntry {
                    uv: [0.0, 0.0, SOLID_BLOCK as f32 / ATLAS_W as f32, SOLID_BLOCK as f32 / ATLAS_H as f32],
                    size: [SOLID_BLOCK as f32, SOLID_BLOCK as f32],
                    bearing: [0.0, 0.0],
                    advance: entry.advance,
                };
                glyphs.insert('?', e);
                sustitucion = e;
            }
            _ => {
                // Sin glifo '?' en absoluto: usar bloque sólido
                let e = GlyphEntry {
                    uv: [0.0, 0.0, SOLID_BLOCK as f32 / ATLAS_W as f32, SOLID_BLOCK as f32 / ATLAS_H as f32],
                    size: [SOLID_BLOCK as f32, SOLID_BLOCK as f32],
                    bearing: [0.0, 0.0],
                    advance: ATLAS_RASTER_PX * 0.5,
                };
                glyphs.insert('?', e);
                sustitucion = e;
            }
        }

        // Resto del conjunto
        for c in &conjunto {
            if *c == '?' {
                continue; // ya procesado
            }
            match generar_celda(*c, &face, &font, k) {
                Ok(Some((celda, cw, ch, entry))) => {
                    if celda.is_empty() {
                        // Glifo sin contorno (espacio, etc.): solo avance
                        glyphs.insert(*c, entry);
                    } else {
                        match colocar_en_paginas(&mut paginas, *c, &celda, cw, ch, entry) {
                            Some((e, pg)) => {
                                glyphs.insert(*c, e);
                                paginas_por_glifo.insert(*c, pg);
                            }
                            None => {
                                avisos.push(format!("No cabe '{}' (U+{:04X}) en el atlas", c, *c as u32));
                                glyphs.insert(*c, sustitucion);
                                sustituciones += 1;
                            }
                        }
                    }
                }
                Ok(None) => {
                    // Sin glifo en la fuente: sustitución
                    avisos.push(format!("Sin glifo para '{}' (U+{:04X})", c, *c as u32));
                    glyphs.insert(*c, sustitucion);
                    sustituciones += 1;
                }
                Err(msg) => {
                    avisos.push(format!("Error generando '{}' (U+{:04X}): {}", c, *c as u32, msg));
                    glyphs.insert(*c, sustitucion);
                    sustituciones += 1;
                }
            }
        }

        Ok(Self {
            paginas,
            glyphs,
            paginas_por_glifo,
            ascent,
            descent,
            line_height,
            solid_uv,
            avisos,
            sustituciones,
            inserciones_frame: 0,
            sustitucion,
            font_bytes: font_bytes.to_vec(),
        })
    }

    /// Devuelve la entrada del glifo o la sustitución si no existe.
    /// Página donde vive el glifo (0 si falta: la sustitución vive en la 0).
    pub fn pagina_de(&self, c: char) -> usize {
        self.paginas_por_glifo.get(&c).copied().unwrap_or(0)
    }

    pub fn glifo(&self, c: char) -> &GlyphEntry {
        self.glyphs.get(&c).unwrap_or(&self.sustitucion)
    }

    /// Inserta un glifo bajo demanda, respetando el presupuesto por frame.
    pub fn insertar(&mut self, c: char) -> &GlyphEntry {
        if self.glyphs.contains_key(&c) {
            return self.glyphs.get(&c).unwrap_or(&self.sustitucion);
        }
        if self.inserciones_frame >= PRESUPUESTO_POR_FRAME {
            return &self.sustitucion;
        }

        let font = match FontRef::try_from_slice(&self.font_bytes) {
            Ok(f) => f,
            Err(_) => return &self.sustitucion,
        };
        let face = match ttf_parser::Face::parse(&self.font_bytes, 0) {
            Ok(f) => f,
            Err(_) => return &self.sustitucion,
        };
        let k = font
            .as_scaled(PxScale::from(ATLAS_RASTER_PX))
            .scale_factor()
            .horizontal;

        match generar_celda(c, &face, &font, k) {
            Ok(Some((celda, cw, ch, entry))) => {
                if celda.is_empty() {
                    self.glyphs.insert(c, entry);
                    self.inserciones_frame += 1;
                } else {
                    match colocar_en_paginas(&mut self.paginas, c, &celda, cw, ch, entry) {
                        Some((e, pg)) => {
                            self.glyphs.insert(c, e);
                            self.paginas_por_glifo.insert(c, pg);
                            self.inserciones_frame += 1;
                        }
                        None => {
                            self.avisos.push(format!("No cabe '{}' (U+{:04X}) en el atlas", c, c as u32));
                            self.glyphs.insert(c, self.sustitucion);
                            self.sustituciones += 1;
                        }
                    }
                }
            }
            Ok(None) => {
                self.avisos.push(format!("Sin glifo para '{}' (U+{:04X})", c, c as u32));
                self.glyphs.insert(c, self.sustitucion);
                self.sustituciones += 1;
            }
            Err(msg) => {
                self.avisos.push(format!("Error generando '{}' (U+{:04X}): {}", c, c as u32, msg));
                self.glyphs.insert(c, self.sustitucion);
                self.sustituciones += 1;
            }
        }

        self.glyphs.get(&c).unwrap_or(&self.sustitucion)
    }

    /// Reinicia el contador de inserciones por frame.
    pub fn avanzar_frame(&mut self) {
        self.inserciones_frame = 0;
    }

    /// Número de páginas en uso.
    pub fn pagina_actual(&self) -> usize {
        self.paginas.len()
    }

    /// Píxeles RGBA de la página indicada.
    pub fn pixels_pagina(&self, i: usize) -> Option<&[u8]> {
        self.paginas.get(i).map(|p| p.pixels.as_slice())
    }

    /// Dimensiones de cada página (todas iguales).
    pub fn tamano_pagina(&self) -> (u32, u32) {
        (ATLAS_W, ATLAS_H)
    }

    // -----------------------------------------------------------------------
    // Serialización simple
    // -----------------------------------------------------------------------

    /// Serializa el atlas a bytes.
    pub fn a_bytes(&self) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        // Magic
        buf.extend_from_slice(b"SDF1");
        // Versión
        buf.extend_from_slice(&GENERADOR_VERSION.to_le_bytes());
        // Nº páginas
        buf.extend_from_slice(&(self.paginas.len() as u32).to_le_bytes());
        // Métricas globales: ascent, descent, line_height, solid_uv (5 f32)
        buf.extend_from_slice(&self.ascent.to_le_bytes());
        buf.extend_from_slice(&self.descent.to_le_bytes());
        buf.extend_from_slice(&self.line_height.to_le_bytes());
        buf.extend_from_slice(&self.solid_uv[0].to_le_bytes());
        buf.extend_from_slice(&self.solid_uv[1].to_le_bytes());

        for pagina in &self.paginas {
            // Píxeles
            buf.extend_from_slice(&pagina.pixels);
            // Nº de glifos en esta página
            let n = pagina.glyphs.len() as u32;
            buf.extend_from_slice(&n.to_le_bytes());
            for (&c, entry) in &pagina.glyphs {
                buf.extend_from_slice(&(c as u32).to_le_bytes());
                // 11 f32: uv[4], size[2], bearing[2], advance, (reserva 2)
                buf.extend_from_slice(&entry.uv[0].to_le_bytes());
                buf.extend_from_slice(&entry.uv[1].to_le_bytes());
                buf.extend_from_slice(&entry.uv[2].to_le_bytes());
                buf.extend_from_slice(&entry.uv[3].to_le_bytes());
                buf.extend_from_slice(&entry.size[0].to_le_bytes());
                buf.extend_from_slice(&entry.size[1].to_le_bytes());
                buf.extend_from_slice(&entry.bearing[0].to_le_bytes());
                buf.extend_from_slice(&entry.bearing[1].to_le_bytes());
                buf.extend_from_slice(&entry.advance.to_le_bytes());
                // 2 reserva para llegar a 11
                buf.extend_from_slice(&0.0f32.to_le_bytes());
                buf.extend_from_slice(&0.0f32.to_le_bytes());
            }
        }
        buf
    }

    /// Deserializa el atlas desde bytes.
    pub fn desde_bytes(bytes: &[u8]) -> Result<Self, String> {

        let leer_u32 = |pos: &mut usize, bytes: &[u8]| -> Result<u32, String> {
            if *pos + 4 > bytes.len() {
                return Err("Datos truncados al leer u32".into());
            }
            let v = u32::from_le_bytes([
                bytes[*pos],
                bytes[*pos + 1],
                bytes[*pos + 2],
                bytes[*pos + 3],
            ]);
            *pos += 4;
            Ok(v)
        };

        let leer_f32 = |pos: &mut usize, bytes: &[u8]| -> Result<f32, String> {
            if *pos + 4 > bytes.len() {
                return Err("Datos truncados al leer f32".into());
            }
            let v = f32::from_le_bytes([
                bytes[*pos],
                bytes[*pos + 1],
                bytes[*pos + 2],
                bytes[*pos + 3],
            ]);
            *pos += 4;
            Ok(v)
        };

        // Magic
        if bytes.len() < 4 || &bytes[0..4] != b"SDF1" {
            return Err("Magic incorrecto: se esperaba SDF1".into());
        }
        let mut pos: usize = 4;

        let version = leer_u32(&mut pos, bytes)?;
        if version != GENERADOR_VERSION {
            return Err(format!("Versión incompatible: {} (esperada {})", version, GENERADOR_VERSION));
        }

        let num_paginas = leer_u32(&mut pos, bytes)? as usize;
        if num_paginas > MAX_PAGINAS {
            return Err(format!("Demasiadas páginas: {}", num_paginas));
        }

        let ascent = leer_f32(&mut pos, bytes)?;
        let descent = leer_f32(&mut pos, bytes)?;
        let line_height = leer_f32(&mut pos, bytes)?;
        let su0 = leer_f32(&mut pos, bytes)?;
        let su1 = leer_f32(&mut pos, bytes)?;
        let solid_uv = [su0, su1];

        let pix_size = (ATLAS_W * ATLAS_H * 4) as usize;
        let mut paginas: Vec<Pagina> = Vec::with_capacity(num_paginas);
        let mut glyphs: HashMap<char, GlyphEntry> = HashMap::new();
        let mut paginas_por_glifo: HashMap<char, usize> = HashMap::new();

        for pg in 0..num_paginas {
            if pos + pix_size > bytes.len() {
                return Err("Datos truncados al leer píxeles de página".into());
            }
            let pixels = bytes[pos..pos + pix_size].to_vec();
            pos += pix_size;

            let n_glyphs = leer_u32(&mut pos, bytes)? as usize;
            let mut page_glyphs: HashMap<char, GlyphEntry> = HashMap::with_capacity(n_glyphs);

            for _ in 0..n_glyphs {
                let code = leer_u32(&mut pos, bytes)?;
                let c = char::from_u32(code)
                    .ok_or_else(|| format!("Codepoint inválido: {}", code))?;

                let uv0 = leer_f32(&mut pos, bytes)?;
                let uv1 = leer_f32(&mut pos, bytes)?;
                let uv2 = leer_f32(&mut pos, bytes)?;
                let uv3 = leer_f32(&mut pos, bytes)?;
                let s0 = leer_f32(&mut pos, bytes)?;
                let s1 = leer_f32(&mut pos, bytes)?;
                let b0 = leer_f32(&mut pos, bytes)?;
                let b1 = leer_f32(&mut pos, bytes)?;
                let advance = leer_f32(&mut pos, bytes)?;
                let _r0 = leer_f32(&mut pos, bytes)?;
                let _r1 = leer_f32(&mut pos, bytes)?;

                let entry = GlyphEntry {
                    uv: [uv0, uv1, uv2, uv3],
                    size: [s0, s1],
                    bearing: [b0, b1],
                    advance,
                };
                page_glyphs.insert(c, entry);
                glyphs.insert(c, entry);
                paginas_por_glifo.insert(c, pg);
            }

            paginas.push(Pagina {
                pixels,
                glyphs: page_glyphs,
                // No necesitamos shelf exacto; ponemos al final para no sobreescribir
                shelf_x: ATLAS_W,
                shelf_y: ATLAS_H,
                shelf_h: 0,
            });
        }

        // Sustitución: usar '?' si existe, si no, bloque sólido
        let sustitucion = glyphs.get(&'?').copied().unwrap_or(GlyphEntry {
            uv: [0.0, 0.0, SOLID_BLOCK as f32 / ATLAS_W as f32, SOLID_BLOCK as f32 / ATLAS_H as f32],
            size: [SOLID_BLOCK as f32, SOLID_BLOCK as f32],
            bearing: [0.0, 0.0],
            advance: ATLAS_RASTER_PX * 0.5,
        });

        Ok(Self {
            paginas,
            glyphs,
            paginas_por_glifo,
            ascent,
            descent,
            line_height,
            solid_uv,
            avisos: Vec::new(),
            sustituciones: 0,
            inserciones_frame: 0,
            sustitucion,
            font_bytes: Vec::new(), // No se retienen al deserializar
        })
    }
}

// ---------------------------------------------------------------------------
// Clave de caché
// ---------------------------------------------------------------------------

/// Genera una clave de caché determinista para el atlas.
pub fn clave_cache(font_bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    font_bytes.hash(&mut hasher);
    ATLAS_RASTER_PX.to_bits().hash(&mut hasher);
    SPREAD_PX.to_bits().hash(&mut hasher);
    // Conjunto de glifos: ASCII 32–126 + Latin-1 160–255
    for b in 32u8..=126 {
        b.hash(&mut hasher);
    }
    for b in 160u8..=255u8 {
        b.hash(&mut hasher);
    }
    GENERADOR_VERSION.hash(&mut hasher);
    let h = hasher.finish();
    format!("sdf-v1-{:016x}", h)
}
