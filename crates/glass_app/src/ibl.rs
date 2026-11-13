//! IBL básica de skybox: cubemap de entorno generado en CPU, con un
//! disco solar simple más halo y resplandor sobre un degradado
//! cielo-suelo. El resultado se empaqueta como RGBA16F listo para
//! `glass_renderer::create_ibl_cube`.

pub const IBL_SIZE: u32 = 64;
pub const IBL_MIPS: u32 = 5; // 64, 32, 16, 8, 4

const CIELO: [f32; 3] = [0.82, 0.88, 1.0];
const SUELO: [f32; 3] = [0.62, 0.58, 0.55];

/// Conversión f32 -> f16 (bits), redondeo al par en el bit de corte.
fn f32_a_f16(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 31) & 0x1) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x7f_ffff;

    // NaN
    if exp == 0xff && mant != 0 {
        return 0x7e00;
    }
    // Infinito
    if exp == 0xff {
        return (sign << 15) | 0x7c00;
    }
    // Cero (con signo)
    if exp == 0 && mant == 0 {
        return sign << 15;
    }

    // Exponente sin bias real (bias f32 = 127, bias f16 = 15)
    let unbiased = exp - 127;
    let new_exp = unbiased + 15;

    if new_exp >= 31 {
        // Desborda a infinito
        return (sign << 15) | 0x7c00;
    }

    if new_exp <= 0 {
        // Subnormal o cero en f16
        if new_exp < -10 {
            return sign << 15;
        }
        // mantisa completa de 24 bits (1.mant) desplazada
        let full_mant = mant | 0x80_0000;
        let shift = 14 - new_exp; // cuántos bits desplazar a la derecha
        if shift > 24 {
            return sign << 15;
        }
        let half_mant = full_mant >> shift;
        let remainder = full_mant & ((1u32 << shift) - 1);
        let halfway = 1u32 << (shift - 1);
        let mut m = half_mant;
        if remainder > halfway || (remainder == halfway && (m & 1) == 1) {
            m += 1;
        }
        return (sign << 15) | (m as u16);
    }

    // Caso normal: redondeo al par en los 13 bits inferiores de la mantisa de 23 bits
    let half_mant_full = mant >> 13;
    let remainder = mant & 0x1fff;
    let halfway = 0x1000u32;
    let mut m = half_mant_full;
    let mut e = new_exp as u32;
    if remainder > halfway || (remainder == halfway && (m & 1) == 1) {
        m += 1;
        if m == 0x400 {
            m = 0;
            e += 1;
        }
    }
    if e >= 31 {
        return (sign << 15) | 0x7c00;
    }
    (sign << 15) | ((e as u16) << 10) | (m as u16)
}

/// Conversión f16 (bits) -> f32. Usada solo en tests para verificar.
#[cfg(test)]
fn f16_a_f32(h: u16) -> f32 {
    let sign = ((h >> 15) & 0x1) as u32;
    let exp = ((h >> 10) & 0x1f) as u32;
    let mant = (h & 0x3ff) as u32;

    if exp == 0 {
        if mant == 0 {
            return f32::from_bits(sign << 31);
        }
        // Subnormal f16 -> normalizar
        let mut m = mant;
        let mut e: i32 = -14;
        while m & 0x400 == 0 {
            m <<= 1;
            e -= 1;
        }
        m &= 0x3ff;
        let new_exp = (e + 127) as u32;
        let new_mant = m << 13;
        return f32::from_bits((sign << 31) | (new_exp << 23) | new_mant);
    }
    if exp == 0x1f {
        if mant == 0 {
            return f32::from_bits((sign << 31) | 0xff << 23);
        }
        return f32::from_bits((sign << 31) | (0xff << 23) | (mant << 13));
    }
    let new_exp = (exp as i32 - 15 + 127) as u32;
    let new_mant = mant << 13;
    f32::from_bits((sign << 31) | (new_exp << 23) | new_mant)
}

fn normalizar(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= 0.0 {
        return [0.0, 1.0, 0.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn smoothstep(borde0: f32, borde1: f32, x: f32) -> f32 {
    let t = ((x - borde0) / (borde1 - borde0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Dirección normalizada desde coordenadas de cara.
/// u, v en [-1, 1]; u crece hacia la derecha, v crece hacia abajo.
pub fn direccion_cara(cara: u32, u: f32, v: f32) -> [f32; 3] {
    let dir = match cara {
        0 => [1.0, -v, -u],
        1 => [-1.0, -v, u],
        2 => [u, 1.0, v],
        3 => [u, -1.0, -v],
        4 => [u, -v, 1.0],
        5 => [-u, -v, -1.0],
        _ => [0.0, 1.0, 0.0],
    };
    normalizar(dir)
}

fn color_cielo(dir: [f32; 3], sol_dir: [f32; 3], sol_color: [f32; 3]) -> [f32; 3] {
    let t = dir[1] * 0.5 + 0.5;
    let t = t.clamp(0.0, 1.0);
    let mut base = [0.0f32; 3];
    for i in 0..3 {
        base[i] = SUELO[i] + (CIELO[i] - SUELO[i]) * t;
    }

    let d = dot3(dir, sol_dir);
    let disco = smoothstep(0.9993, 0.9998, d) * 16.0;
    let halo = d.max(0.0).powf(600.0) * 3.0;
    let resplandor = d.max(0.0).powf(8.0) * 0.18;
    let factor = disco + halo + resplandor;

    let mut out = [0.0f32; 3];
    for i in 0..3 {
        out[i] = base[i] + sol_color[i] * factor;
    }
    out
}

fn generar_nivel0_f32(sol_dir: [f32; 3], sol_color: [f32; 3]) -> Vec<[f32; 4]> {
    let lado = IBL_SIZE;
    let mut datos = Vec::with_capacity((6 * lado * lado) as usize);
    for cara in 0..6u32 {
        for y in 0..lado {
            let v = 2.0 * (y as f32 + 0.5) / lado as f32 - 1.0;
            for x in 0..lado {
                let u = 2.0 * (x as f32 + 0.5) / lado as f32 - 1.0;
                let dir = direccion_cara(cara, u, v);
                let color = color_cielo(dir, sol_dir, sol_color);
                datos.push([color[0], color[1], color[2], 1.0]);
            }
        }
    }
    datos
}

fn reducir_nivel(anterior: &[[f32; 4]], lado_anterior: u32) -> Vec<[f32; 4]> {
    let lado_nuevo = (lado_anterior / 2).max(1);
    let mut datos = Vec::with_capacity((6 * lado_nuevo * lado_nuevo) as usize);
    for cara in 0..6u32 {
        let base_cara = (cara * lado_anterior * lado_anterior) as usize;
        for y in 0..lado_nuevo {
            for x in 0..lado_nuevo {
                let x0 = (x * 2).min(lado_anterior - 1);
                let x1 = (x * 2 + 1).min(lado_anterior - 1);
                let y0 = (y * 2).min(lado_anterior - 1);
                let y1 = (y * 2 + 1).min(lado_anterior - 1);

                let idx = |xx: u32, yy: u32| -> usize {
                    base_cara + (yy * lado_anterior + xx) as usize
                };

                let p00 = anterior[idx(x0, y0)];
                let p10 = anterior[idx(x1, y0)];
                let p01 = anterior[idx(x0, y1)];
                let p11 = anterior[idx(x1, y1)];

                let mut acc = [0.0f32; 4];
                for i in 0..4 {
                    acc[i] = (p00[i] + p10[i] + p01[i] + p11[i]) * 0.25;
                }
                datos.push(acc);
            }
        }
    }
    datos
}

fn empaquetar_nivel(nivel: &[[f32; 4]]) -> Vec<u8> {
    let mut salida = Vec::with_capacity(nivel.len() * 8);
    for texel in nivel {
        for &valor in texel {
            let h = f32_a_f16(valor);
            salida.push((h & 0xff) as u8);
            salida.push((h >> 8) as u8);
        }
    }
    salida
}

/// Genera el cubemap completo (todos los mips) ya empaquetado como
/// bytes RGBA16F, en el orden nivel -> cara -> fila -> téxel, que
/// espera `glass_renderer::create_ibl_cube`.
pub fn generar_cubemapa(sol_dir: [f32; 3], sol_color: [f32; 3]) -> Vec<u8> {
    let sol_dir = normalizar(sol_dir);

    let mut niveles: Vec<Vec<[f32; 4]>> = Vec::with_capacity(IBL_MIPS as usize);
    niveles.push(generar_nivel0_f32(sol_dir, sol_color));

    let mut lado = IBL_SIZE;
    for _ in 1..IBL_MIPS {
        let siguiente = reducir_nivel(niveles.last().unwrap(), lado);
        niveles.push(siguiente);
        lado = (lado / 2).max(1);
    }

    let mut salida = Vec::new();
    for nivel in &niveles {
        salida.extend(empaquetar_nivel(nivel));
    }
    salida
}

/// Estado para decidir cuándo regenerar el cubemap de IBL.
#[derive(Default)]
pub struct EstadoIbl {
    pub ultimo_sol: Option<([f32; 3], [f32; 3])>,
    pub ultimo_instante: Option<std::time::Instant>,
    pub hay_entorno: bool,
}


impl EstadoIbl {
    /// ¿Toca regenerar? Sí si nunca se generó, si el sol se movió más
    /// de 3° o si algún canal de color cambió más de 0,05; con freno
    /// de 2 s entre regeneraciones. Al devolver true ya registra el
    /// nuevo estado.
    pub fn toca_regenerar(&mut self, sol: [f32; 3], color: [f32; 3]) -> bool {
        let ahora = std::time::Instant::now();

        if let Some(ultimo) = self.ultimo_instante {
            if ahora.duration_since(ultimo).as_secs_f32() < 2.0 {
                return false;
            }
        }

        let necesita = match self.ultimo_sol {
            None => true,
            Some((sol_prev, color_prev)) => {
                let a = normalizar(sol);
                let b = normalizar(sol_prev);
                let d = dot3(a, b).clamp(-1.0, 1.0);
                let angulo = d.acos();
                let umbral_angulo = 3.0f32.to_radians();

                let color_cambio = (0..3).any(|i| (color[i] - color_prev[i]).abs() > 0.05);

                angulo > umbral_angulo || color_cambio
            }
        };

        if necesita {
            self.ultimo_sol = Some((sol, color));
            self.ultimo_instante = Some(ahora);
            self.hay_entorno = true;
        }

        necesita
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminancia(c: [f32; 4]) -> f32 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }

    fn desempaquetar_nivel(bytes: &[u8], num_texeles: usize) -> Vec<[f32; 4]> {
        let mut datos = Vec::with_capacity(num_texeles);
        for i in 0..num_texeles {
            let base = i * 8;
            let mut c = [0.0f32; 4];
            for (canal, comp) in c.iter_mut().enumerate() {
                let off = base + canal * 2;
                let h = (bytes[off] as u16) | ((bytes[off + 1] as u16) << 8);
                *comp = f16_a_f32(h);
            }
            datos.push(c);
        }
        datos
    }

    #[test]
    fn el_brillo_maximo_apunta_al_sol() {
        let sol_dir = normalizar([0.4, 0.75, 0.3]);
        let sol_color = [1.0, 1.0, 1.0];
        let nivel0 = generar_nivel0_f32(sol_dir, sol_color);

        let lado = IBL_SIZE;
        let mut mejor_idx = 0usize;
        let mut mejor_lum = f32::MIN;
        for (i, c) in nivel0.iter().enumerate() {
            let l = luminancia(*c);
            if l > mejor_lum {
                mejor_lum = l;
                mejor_idx = i;
            }
        }

        let texeles_por_cara = (lado * lado) as usize;
        let cara = (mejor_idx / texeles_por_cara) as u32;
        let resto = mejor_idx % texeles_por_cara;
        let y = (resto / lado as usize) as u32;
        let x = (resto % lado as usize) as u32;

        let u = 2.0 * (x as f32 + 0.5) / lado as f32 - 1.0;
        let v = 2.0 * (y as f32 + 0.5) / lado as f32 - 1.0;
        let dir = direccion_cara(cara, u, v);

        let d = dot3(dir, sol_dir).clamp(-1.0, 1.0);
        let angulo_grados = d.acos().to_degrees();
        assert!(angulo_grados < 8.0, "ángulo al sol: {angulo_grados}");
    }

    #[test]
    fn arriba_es_cielo_y_abajo_suelo() {
        let sol_dir = normalizar([1.0, 0.0, 0.0]);
        let sol_color = [1.0, 1.0, 1.0];
        let nivel0 = generar_nivel0_f32(sol_dir, sol_color);

        let lado = IBL_SIZE as usize;
        let texeles_por_cara = lado * lado;
        let centro = lado / 2;
        let idx_central = centro * lado + centro;

        // cara 2 = +Y, cara 3 = -Y
        let idx_arriba = 2 * texeles_por_cara + idx_central;
        let idx_abajo = 3 * texeles_por_cara + idx_central;

        let lum_arriba = luminancia(nivel0[idx_arriba]);
        let lum_abajo = luminancia(nivel0[idx_abajo]);

        assert!(lum_arriba > lum_abajo, "{lum_arriba} vs {lum_abajo}");
    }

    #[test]
    fn tamano_empaquetado_exacto() {
        let sol_dir = normalizar([0.3, 0.6, 0.2]);
        let sol_color = [1.0, 0.9, 0.8];
        let bytes = generar_cubemapa(sol_dir, sol_color);

        let mut esperado = 0usize;
        let mut lado = IBL_SIZE as usize;
        for _ in 0..IBL_MIPS {
            esperado += 6 * lado * lado * 8;
            lado = (lado / 2).max(1);
        }

        assert_eq!(bytes.len(), esperado);
    }

    #[test]
    fn los_mips_conservan_la_energia() {
        let sol_dir = normalizar([0.2, 0.8, 0.1]);
        let sol_color = [1.0, 1.0, 1.0];
        let bytes = generar_cubemapa(sol_dir, sol_color);

        let mut offsets = Vec::new();
        let mut offset = 0usize;
        let mut lado = IBL_SIZE as usize;
        for _ in 0..IBL_MIPS {
            let num_texeles = 6 * lado * lado;
            offsets.push((offset, num_texeles));
            offset += num_texeles * 8;
            lado = (lado / 2).max(1);
        }

        let (off0, num0) = offsets[0];
        let nivel0 = desempaquetar_nivel(&bytes[off0..off0 + num0 * 8], num0);
        let media0: f32 = nivel0.iter().map(|c| luminancia(*c)).sum::<f32>() / num0 as f32;

        let (off4, num4) = offsets[4];
        let nivel4 = desempaquetar_nivel(&bytes[off4..off4 + num4 * 8], num4);
        let media4: f32 = nivel4.iter().map(|c| luminancia(*c)).sum::<f32>() / num4 as f32;

        assert!((media0 - media4).abs() < 0.02, "{media0} vs {media4}");
    }
}
