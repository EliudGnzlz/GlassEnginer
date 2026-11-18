//! Miniaturas de modelos 3D por rasterización en software (sin GPU ni egui).
//!
//! `renderizar_miniatura_modelo` es un adaptador fino: resuelve materiales a
//! colores y delega en `rasterizar`, que solo trabaja con vértices, índices y
//! rangos ya coloreados (así los tests no necesitan un `LoadedModel` completo).

use glass_renderer::mesh::{LoadedModel, Vertex};

/// Gris arcilla para primitivas sin material.
const ARCILLA: [f32; 3] = [0.62, 0.64, 0.68];

/// Renderiza una miniatura RGBA (`lado` x `lado`, fondo transparente)
/// del modelo: vista 3/4 ortográfica que encuadra su esfera
/// envolvente, sombreado plano por cara con luz direccional y el
/// color base de cada primitiva (o un gris arcilla si no declara
/// material). Z-buffer por píxel.
pub fn renderizar_miniatura_modelo(modelo: &LoadedModel, lado: u32) -> Vec<u8> {
    let rangos: Vec<(u32, u32, [f32; 3])> = modelo
        .primitive_ranges
        .iter()
        .map(|r| {
            let color = r
                .material
                .and_then(|m| modelo.materials.get(m))
                .map(|m| {
                    let c = m.base_color_factor;
                    [c[0], c[1], c[2]]
                })
                .unwrap_or(ARCILLA);
            (r.first_index, r.index_count, color)
        })
        .collect();
    rasterizar(&modelo.vertices, &modelo.indices, &rangos, lado)
}

/// Núcleo del rasterizador. `rangos` = (primer índice, nº de índices, rgb).
fn rasterizar(
    vertices: &[Vertex],
    indices: &[u32],
    rangos: &[(u32, u32, [f32; 3])],
    lado: u32,
) -> Vec<u8> {
    let n = lado as usize;
    let mut rgba = vec![0u8; n * n * 4];
    if n == 0 || vertices.is_empty() || indices.len() < 3 {
        return rgba;
    }

    // AABB (ignorando coordenadas no finitas).
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for v in vertices {
        for k in 0..3 {
            let c = v.position[k];
            if c.is_finite() {
                min[k] = min[k].min(c);
                max[k] = max[k].max(c);
            }
        }
    }
    if (0..3).any(|k| min[k] > max[k]) {
        return rgba;
    }
    let centro = escalar(sumar(min, max), 0.5);
    let mut radio = longitud(restar(max, min)) * 0.5;
    if !radio.is_finite() {
        return rgba;
    }
    if radio <= 0.0 {
        radio = 1.0;
    }

    // Cámara: el ojo está en centro + dir * radio * 3; al ser ortográfica,
    // solo importa la orientación, así que proyectamos relativo al centro.
    let (az, el) = ((-35.0f32).to_radians(), 22.0f32.to_radians());
    let dir = [el.cos() * az.sin(), el.sin(), el.cos() * az.cos()];
    let adelante = escalar(dir, -1.0); // del ojo hacia el centro
    let arriba_mundo = if dot(adelante, [0.0, 1.0, 0.0]).abs() > 0.999 {
        [0.0, 0.0, 1.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let derecha = normalizar(cross(adelante, arriba_mundo)).unwrap_or([1.0, 0.0, 0.0]);
    let arriba = normalizar(cross(derecha, adelante)).unwrap_or([0.0, 1.0, 0.0]);

    let semi = radio * 1.08;
    let escala = (lado - 1) as f32;
    // Devuelve [px, py, profundidad]; profundidad mayor = más lejos.
    let proyectar = |p: [f32; 3]| -> [f32; 3] {
        let d = restar(p, centro);
        let x = dot(d, derecha) / semi;
        let y = dot(d, arriba) / semi;
        [
            (x + 1.0) * 0.5 * escala,
            (1.0 - (y + 1.0) * 0.5) * escala,
            dot(d, adelante),
        ]
    };

    let luz = normalizar([-0.45, 0.75, 0.55]).unwrap_or([0.0, 1.0, 0.0]);
    let mut zbuf = vec![f32::INFINITY; n * n];
    let nv = vertices.len();
    let mut rango_cache = 0usize;

    for (t, tri) in indices.as_chunks::<3>().0.iter().enumerate() {
        let (ia, ib, ic) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if ia >= nv || ib >= nv || ic >= nv {
            continue;
        }
        let (a, b, c) = (vertices[ia].position, vertices[ib].position, vertices[ic].position);
        if !a.iter().chain(&b).chain(&c).all(|v| v.is_finite()) {
            continue;
        }

        // Sombreado plano; |dot| para que el winding invertido no deje huecos.
        let normal = match normalizar(cross(restar(b, a), restar(c, a))) {
            Some(nrm) => nrm,
            None => continue,
        };
        let intensidad = 0.36 + 0.64 * dot(normal, luz).abs();
        let base = color_de(rangos, (t * 3) as u32, &mut rango_cache);
        let pixel = [
            cuantizar(base[0] * intensidad),
            cuantizar(base[1] * intensidad),
            cuantizar(base[2] * intensidad),
            255,
        ];

        let (pa, pb, pc) = (proyectar(a), proyectar(b), proyectar(c));
        let area = borde(pa, pb, pc);
        if area.is_nan() || area.abs() <= 1e-9 {
            continue; // degenerado en pantalla (o NaN)
        }
        let inv = 1.0 / area;

        // Caja envolvente recortada a [0, lado).
        let x0 = pa[0].min(pb[0]).min(pc[0]).floor().max(0.0);
        let x1 = pa[0].max(pb[0]).max(pc[0]).ceil().min(escala);
        let y0 = pa[1].min(pb[1]).min(pc[1]).floor().max(0.0);
        let y1 = pa[1].max(pb[1]).max(pc[1]).ceil().min(escala);
        if x0 > x1 || y0 > y1 {
            continue;
        }
        let (x0, x1, y0, y1) = (x0 as usize, x1 as usize, y0 as usize, y1 as usize);

        const TOL: f32 = -1e-5; // evita grietas en aristas compartidas
        for y in y0..=y1 {
            for x in x0..=x1 {
                let p = [x as f32, y as f32, 0.0];
                let w0 = borde(pb, pc, p) * inv;
                let w1 = borde(pc, pa, p) * inv;
                let w2 = borde(pa, pb, p) * inv;
                if w0 < TOL || w1 < TOL || w2 < TOL {
                    continue;
                }
                let z = w0 * pa[2] + w1 * pb[2] + w2 * pc[2];
                let i = y * n + x;
                if z < zbuf[i] {
                    zbuf[i] = z;
                    rgba[i * 4..i * 4 + 4].copy_from_slice(&pixel);
                }
            }
        }
    }
    rgba
}

/// Color del rango que contiene `indice`; prueba primero el último usado.
fn color_de(rangos: &[(u32, u32, [f32; 3])], indice: u32, cache: &mut usize) -> [f32; 3] {
    let contiene = |r: &(u32, u32, [f32; 3])| indice >= r.0 && indice - r.0 < r.1;
    if let Some(r) = rangos.get(*cache) {
        if contiene(r) {
            return r.2;
        }
    }
    match rangos.iter().position(contiene) {
        Some(i) => {
            *cache = i;
            rangos[i].2
        }
        None => ARCILLA,
    }
}

/// Función de borde 2D (usa solo x, y).
#[inline]
fn borde(a: [f32; 3], b: [f32; 3], p: [f32; 3]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

#[inline]
fn cuantizar(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[inline]
fn sumar(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[inline]
fn restar(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn escalar(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

#[inline]
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
fn longitud(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

/// Normaliza; `None` si la longitud es ~0 o no finita.
#[inline]
fn normalizar(a: [f32; 3]) -> Option<[f32; 3]> {
    let l = longitud(a);
    if l.is_finite() && l > f32::MIN_POSITIVE {
        Some(escalar(a, 1.0 / l))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32, z: f32) -> Vertex {
        Vertex { position: [x, y, z], normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0] }
    }

    fn alfa(buf: &[u8], lado: usize, x: usize, y: usize) -> u8 {
        buf[(y * lado + x) * 4 + 3]
    }

    fn cubo() -> (Vec<Vertex>, Vec<u32>) {
        // Vértice i: bit0 = x, bit1 = y, bit2 = z (±1).
        let vs = (0..8)
            .map(|i| {
                let s = |b: u32| if i & b != 0 { 1.0 } else { -1.0 };
                v(s(1), s(2), s(4))
            })
            .collect();
        let is = vec![
            0, 2, 6, 0, 6, 4, // -X
            1, 5, 7, 1, 7, 3, // +X
            0, 4, 5, 0, 5, 1, // -Y
            2, 3, 7, 2, 7, 6, // +Y
            0, 1, 3, 0, 3, 2, // -Z
            4, 6, 7, 4, 7, 5, // +Z
        ];
        (vs, is)
    }

    #[test]
    fn sin_geometria_es_transparente() {
        let buf = rasterizar(&[], &[], &[], 16);
        assert_eq!(buf.len(), 16 * 16 * 4);
        assert!(buf.iter().all(|&b| b == 0));

        assert!(rasterizar(&[v(0.0, 0.0, 0.0)], &[0, 0, 0], &[], 0).is_empty());
    }

    #[test]
    fn suelo_cubre_el_centro() {
        let vs = vec![v(-1.0, 0.0, -1.0), v(1.0, 0.0, -1.0), v(1.0, 0.0, 1.0), v(-1.0, 0.0, 1.0)];
        let is = [0, 1, 2, 0, 2, 3];
        let lado = 65;
        let buf = rasterizar(&vs, &is, &[], lado as u32);
        assert_eq!(alfa(&buf, lado, 32, 32), 255);
    }

    #[test]
    fn cubo_centro_opaco_esquinas_transparentes_y_sombreado() {
        let (vs, is) = cubo();
        let lado = 65;
        let rangos = [(0u32, is.len() as u32, [0.8, 0.3, 0.2])];
        let buf = rasterizar(&vs, &is, &rangos, lado as u32);

        assert_eq!(alfa(&buf, lado, 32, 32), 255);
        for &(x, y) in &[(0, 0), (lado - 1, 0), (0, lado - 1), (lado - 1, lado - 1)] {
            assert_eq!(alfa(&buf, lado, x, y), 0);
        }

        let mut niveles: Vec<[u8; 3]> = buf
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] == 255)
            .map(|p| [p[0], p[1], p[2]])
            .collect();
        niveles.sort_unstable();
        niveles.dedup();
        assert!(niveles.len() >= 2, "niveles de brillo: {niveles:?}");
    }

    #[test]
    fn determinista() {
        let (vs, is) = cubo();
        let a = rasterizar(&vs, &is, &[], 48);
        let b = rasterizar(&vs, &is, &[], 48);
        assert_eq!(a, b);
    }
}

