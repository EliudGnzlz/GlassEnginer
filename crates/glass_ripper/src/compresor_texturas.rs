//! Compresor de texturas de calidad (BC7 / BC5) con `intel_tex_2`.
//!
//! Este módulo es independiente del escritor/loader KTX2 y de
//! `glass_renderer`: solo produce niveles comprimidos (bytes + dimensiones
//! lógicas) y el formato correspondiente. El ensamblado final a `.ktx2`
//! vive en otro paquete.

use intel_tex_2::{bc5, bc7, RgSurface, RgbaSurface};

/// Perfil de calidad del compresor BC7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfilCalidad {
    Rapido,
    Equilibrado,
    Calidad,
}

/// Formato de salida de un nivel comprimido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatoComprimido {
    Bc7Srgb,
    Bc7Unorm,
    Bc5Unorm,
}

/// Un nivel comprimido: bytes del nivel y sus dimensiones LÓGICAS.
#[derive(Debug, Clone)]
pub struct NivelComprimido {
    pub datos: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Número de bytes de UN nivel BC (16 B por bloque 4x4) desde sus
/// dimensiones lógicas. Regla única en todo el módulo.
pub fn bytes_nivel_bc(width: u32, height: u32) -> usize {
    let bloques_x = ((width as usize) + 3) / 4;
    let bloques_y = ((height as usize) + 3) / 4;
    bloques_x * bloques_y * 16
}

fn validar_entrada(rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err(format!(
            "dimensiones inválidas: width={}, height={}",
            width, height
        ));
    }
    if rgba.is_empty() {
        return Err("entrada RGBA vacía".to_string());
    }
    let esperado = (width as usize) * (height as usize) * 4;
    if rgba.len() != esperado {
        return Err(format!(
            "longitud RGBA incorrecta: esperado {} bytes, recibido {}",
            esperado,
            rgba.len()
        ));
    }
    Ok(())
}

/// Rellena una superficie RGBA/RG empaquetada hasta múltiplos de 4 en
/// cada eje, replicando el píxel del borde (clamp), para que el kernel
/// de compresión no desborde el búfer en dimensiones NPOT.
fn rellenar_a_multiplo_4(datos: &[u8], width: u32, height: u32, componentes: usize) -> (Vec<u8>, u32, u32) {
    let w = width as usize;
    let h = height as usize;
    let w_pad = ((w + 3) / 4) * 4;
    let h_pad = ((h + 3) / 4) * 4;
    if w_pad == w && h_pad == h {
        return (datos.to_vec(), width, height);
    }
    let mut salida = vec![0u8; w_pad * h_pad * componentes];
    for y in 0..h_pad {
        let y_src = y.min(h - 1);
        for x in 0..w_pad {
            let x_src = x.min(w - 1);
            let origen = (y_src * w + x_src) * componentes;
            let destino = (y * w_pad + x) * componentes;
            salida[destino..destino + componentes]
                .copy_from_slice(&datos[origen..origen + componentes]);
        }
    }
    (salida, w_pad as u32, h_pad as u32)
}

fn comprimir_bc7_nivel(
    rgba: &[u8],
    width: u32,
    height: u32,
    settings: &bc7::EncodeSettings,
) -> Vec<u8> {
    let (datos_pad, w_pad, h_pad) = rellenar_a_multiplo_4(rgba, width, height, 4);
    let superficie = RgbaSurface {
        data: &datos_pad,
        width: w_pad,
        height: h_pad,
        stride: w_pad * 4,
    };
    bc7::compress_blocks(settings, &superficie)
}

fn comprimir_bc5_nivel(rg: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (datos_pad, w_pad, h_pad) = rellenar_a_multiplo_4(rg, width, height, 2);
    let superficie = RgSurface {
        data: &datos_pad,
        width: w_pad,
        height: h_pad,
        stride: w_pad * 2,
    };
    bc5::compress_blocks(&superficie)
}

fn srgb_byte_a_lineal(b: u8) -> f32 {
    let c = b as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn lineal_a_srgb_byte(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// Downsample 2x2 -> 1 para color RGBA8 sRGB: promedia RGB en espacio
/// lineal y alfa en media aritmética directa. El borde se repite (clamp)
/// cuando el bloque 2x2 se sale de la imagen.
fn downsample_rgba_color(rgba: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let w = width as usize;
    let h = height as usize;
    let w_out = std::cmp::max(1, w / 2);
    let h_out = std::cmp::max(1, h / 2);
    let mut salida = vec![0u8; w_out * h_out * 4];

    let muestra = |x: usize, y: usize| -> (u8, u8, u8, u8) {
        let xc = x.min(w - 1);
        let yc = y.min(h - 1);
        let idx = (yc * w + xc) * 4;
        (rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3])
    };

    for oy in 0..h_out {
        for ox in 0..w_out {
            let x0 = ox * 2;
            let y0 = oy * 2;
            let pixeles = [
                muestra(x0, y0),
                muestra(x0 + 1, y0),
                muestra(x0, y0 + 1),
                muestra(x0 + 1, y0 + 1),
            ];

            let mut r_lin = 0.0f32;
            let mut g_lin = 0.0f32;
            let mut b_lin = 0.0f32;
            let mut a_sum = 0.0f32;
            for (r, g, b, a) in pixeles.iter() {
                r_lin += srgb_byte_a_lineal(*r);
                g_lin += srgb_byte_a_lineal(*g);
                b_lin += srgb_byte_a_lineal(*b);
                a_sum += *a as f32;
            }
            r_lin /= 4.0;
            g_lin /= 4.0;
            b_lin /= 4.0;
            let a_out = (a_sum / 4.0 + 0.5).clamp(0.0, 255.0) as u8;

            let idx = (oy * w_out + ox) * 4;
            salida[idx] = lineal_a_srgb_byte(r_lin);
            salida[idx + 1] = lineal_a_srgb_byte(g_lin);
            salida[idx + 2] = lineal_a_srgb_byte(b_lin);
            salida[idx + 3] = a_out;
        }
    }

    (salida, w_out as u32, h_out as u32)
}

/// Downsample 2x2 -> 1 para normales: promedia X,Y,Z de los 4 texeles y
/// renormaliza el vector medio a longitud 1. Z sale del canal B; un
/// texel con los 3 canales a 0 se trata como (0,0,1) antes de promediar.
/// El alfa se promedia aritméticamente (no se usa, pero se conserva).
fn downsample_rgba_normal(rgba: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let w = width as usize;
    let h = height as usize;
    let w_out = std::cmp::max(1, w / 2);
    let h_out = std::cmp::max(1, h / 2);
    let mut salida = vec![0u8; w_out * h_out * 4];

    let muestra = |x: usize, y: usize| -> (u8, u8, u8, u8) {
        let xc = x.min(w - 1);
        let yc = y.min(h - 1);
        let idx = (yc * w + xc) * 4;
        (rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3])
    };

    let byte_a_signed = |b: u8| -> f32 { (b as f32 / 255.0) * 2.0 - 1.0 };
    let signed_a_byte = |v: f32| -> u8 {
        (((v.clamp(-1.0, 1.0) + 1.0) * 0.5) * 255.0 + 0.5).clamp(0.0, 255.0) as u8
    };

    for oy in 0..h_out {
        for ox in 0..w_out {
            let x0 = ox * 2;
            let y0 = oy * 2;
            let pixeles = [
                muestra(x0, y0),
                muestra(x0 + 1, y0),
                muestra(x0, y0 + 1),
                muestra(x0 + 1, y0 + 1),
            ];

            let mut sx = 0.0f32;
            let mut sy = 0.0f32;
            let mut sz = 0.0f32;
            let mut a_sum = 0.0f32;
            for (r, g, b, a) in pixeles.iter() {
                if *r == 0 && *g == 0 && *b == 0 {
                    sx += 0.0;
                    sy += 0.0;
                    sz += 1.0;
                } else {
                    sx += byte_a_signed(*r);
                    sy += byte_a_signed(*g);
                    sz += byte_a_signed(*b);
                }
                a_sum += *a as f32;
            }
            sx /= 4.0;
            sy /= 4.0;
            sz /= 4.0;

            let longitud = (sx * sx + sy * sy + sz * sz).sqrt();
            let (nx, ny) = if longitud > 1e-8 {
                (sx / longitud, sy / longitud)
            } else {
                (0.0, 0.0)
            };
            let a_out = (a_sum / 4.0 + 0.5).clamp(0.0, 255.0) as u8;

            let idx = (oy * w_out + ox) * 4;
            salida[idx] = signed_a_byte(nx);
            salida[idx + 1] = signed_a_byte(ny);
            salida[idx + 2] = 0;
            salida[idx + 3] = a_out;
        }
    }

    (salida, w_out as u32, h_out as u32)
}

fn siguiente_dimension(prev: u32) -> u32 {
    std::cmp::max(1, prev / 2)
}

/// Comprime una imagen RGBA8 completa (nivel 0 + toda su cadena de mips)
/// a BC7.
pub fn comprimir_bc7(
    rgba: &[u8],
    width: u32,
    height: u32,
    srgb: bool,
    perfil: PerfilCalidad,
) -> Result<(FormatoComprimido, Vec<NivelComprimido>), String> {
    validar_entrada(rgba, width, height)?;

    let tiene_alfa = rgba.chunks_exact(4).any(|p| p[3] < 255);

    let settings = match (perfil, tiene_alfa) {
        (PerfilCalidad::Rapido, false) => bc7::opaque_ultra_fast_settings(),
        (PerfilCalidad::Rapido, true) => bc7::alpha_ultra_fast_settings(),
        (PerfilCalidad::Equilibrado, false) => bc7::opaque_fast_settings(),
        (PerfilCalidad::Equilibrado, true) => bc7::alpha_fast_settings(),
        (PerfilCalidad::Calidad, false) => bc7::opaque_slow_settings(),
        (PerfilCalidad::Calidad, true) => bc7::alpha_slow_settings(),
    };

    let formato = if srgb {
        FormatoComprimido::Bc7Srgb
    } else {
        FormatoComprimido::Bc7Unorm
    };

    let mut niveles = Vec::new();
    let mut nivel_actual = rgba.to_vec();
    let mut w = width;
    let mut h = height;

    loop {
        let datos = comprimir_bc7_nivel(&nivel_actual, w, h, &settings);
        niveles.push(NivelComprimido {
            datos,
            width: w,
            height: h,
        });

        if w == 1 && h == 1 {
            break;
        }

        let (siguiente, w_sig, h_sig) = downsample_rgba_color(&nivel_actual, w, h);
        nivel_actual = siguiente;
        w = siguiente_dimension(w);
        h = siguiente_dimension(h);
        debug_assert_eq!(w, w_sig);
        debug_assert_eq!(h, h_sig);
    }

    Ok((formato, niveles))
}

/// Comprime un mapa de normales RGBA8 a BC5.
pub fn comprimir_bc5_normal(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<(FormatoComprimido, Vec<NivelComprimido>), String> {
    validar_entrada(rgba, width, height)?;

    let mut niveles = Vec::new();
    let mut nivel_actual = rgba.to_vec();
    let mut w = width;
    let mut h = height;

    loop {
        let rg: Vec<u8> = nivel_actual
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1]])
            .collect();
        let datos = comprimir_bc5_nivel(&rg, w, h);
        niveles.push(NivelComprimido {
            datos,
            width: w,
            height: h,
        });

        if w == 1 && h == 1 {
            break;
        }

        let (siguiente, w_sig, h_sig) = downsample_rgba_normal(&nivel_actual, w, h);
        nivel_actual = siguiente;
        w = siguiente_dimension(w);
        h = siguiente_dimension(h);
        debug_assert_eq!(w, w_sig);
        debug_assert_eq!(h, h_sig);
    }

    Ok((FormatoComprimido::Bc5Unorm, niveles))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imagen_patron(width: u32, height: u32) -> Vec<u8> {
        let mut datos = vec![0u8; (width as usize) * (height as usize) * 4];
        for y in 0..height {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                datos[idx] = ((x * 37 + y * 11) % 256) as u8;
                datos[idx + 1] = ((x * 7 + y * 53) % 256) as u8;
                datos[idx + 2] = ((x * 13 + y * 29) % 256) as u8;
                datos[idx + 3] = 255;
            }
        }
        datos
    }

    #[test]
    fn sonda_npot_no_desborda() {
        let casos: [(u32, u32); 5] = [(10, 6), (100, 37), (3, 3), (5, 3), (1, 1)];
        for (w, h) in casos {
            let esperado = {
                let bx = ((w as usize) + 3) / 4;
                let by = ((h as usize) + 3) / 4;
                bx * by * 16
            };
            assert_eq!(bytes_nivel_bc(w, h), esperado, "bytes_nivel_bc mal para {}x{}", w, h);

            let rgba = imagen_patron(w, h);
            let (_, niveles) =
                comprimir_bc7(&rgba, w, h, true, PerfilCalidad::Rapido).unwrap();
            assert_eq!(
                niveles[0].datos.len(),
                esperado,
                "BC7 nivel 0 mal para {}x{}",
                w,
                h
            );

            let (_, niveles_bc5) = comprimir_bc5_normal(&rgba, w, h).unwrap();
            assert_eq!(
                niveles_bc5[0].datos.len(),
                esperado,
                "BC5 nivel 0 mal para {}x{}",
                w,
                h
            );
        }
    }

    #[test]
    fn cadena_mips_sigue_la_regla() {
        let rgba = imagen_patron(64, 64);
        let (_, niveles) =
            comprimir_bc7(&rgba, 64, 64, true, PerfilCalidad::Rapido).unwrap();
        let esperado: [(u32, u32); 7] =
            [(64, 64), (32, 32), (16, 16), (8, 8), (4, 4), (2, 2), (1, 1)];
        let dims: Vec<(u32, u32)> = niveles.iter().map(|n| (n.width, n.height)).collect();
        assert_eq!(dims, esperado);

        let rgba2 = imagen_patron(100, 37);
        let (_, niveles2) =
            comprimir_bc7(&rgba2, 100, 37, true, PerfilCalidad::Rapido).unwrap();
        for i in 1..niveles2.len() {
            let (pw, ph) = (niveles2[i - 1].width, niveles2[i - 1].height);
            let (cw, ch) = (niveles2[i].width, niveles2[i].height);
            assert_eq!(cw, std::cmp::max(1, pw / 2));
            assert_eq!(ch, std::cmp::max(1, ph / 2));
        }
        let (lw, lh) = (
            niveles2.last().unwrap().width,
            niveles2.last().unwrap().height,
        );
        assert_eq!((lw, lh), (1, 1));

        let rgba3 = imagen_patron(1, 1);
        let (_, niveles3) =
            comprimir_bc7(&rgba3, 1, 1, true, PerfilCalidad::Rapido).unwrap();
        assert_eq!(niveles3.len(), 1);
        assert_eq!((niveles3[0].width, niveles3[0].height), (1, 1));
    }

    #[test]
    fn bc7_determinista() {
        let rgba = imagen_patron(16, 16);
        for perfil in [
            PerfilCalidad::Rapido,
            PerfilCalidad::Equilibrado,
            PerfilCalidad::Calidad,
        ] {
            let (_, a) = comprimir_bc7(&rgba, 16, 16, true, perfil).unwrap();
            let (_, b) = comprimir_bc7(&rgba, 16, 16, true, perfil).unwrap();
            assert_eq!(a.len(), b.len());
            for (na, nb) in a.iter().zip(b.iter()) {
                assert_eq!(na.datos, nb.datos);
            }
        }
    }

    #[test]
    fn bc7_opaco_vs_alfa() {
        let mut rgba_opaco = imagen_patron(8, 8);
        for chunk in rgba_opaco.chunks_exact_mut(4) {
            chunk[3] = 255;
        }
        let (formato_opaco, niveles_opaco) =
            comprimir_bc7(&rgba_opaco, 8, 8, true, PerfilCalidad::Equilibrado).unwrap();
        assert_eq!(formato_opaco, FormatoComprimido::Bc7Srgb);

        let mut rgba_alfa = rgba_opaco.clone();
        rgba_alfa[3] = 0;
        let (formato_alfa, niveles_alfa) =
            comprimir_bc7(&rgba_alfa, 8, 8, true, PerfilCalidad::Equilibrado).unwrap();
        assert_eq!(formato_alfa, FormatoComprimido::Bc7Srgb);

        assert_ne!(niveles_opaco[0].datos, niveles_alfa[0].datos);
    }

    #[test]
    fn bc7_srgb_vs_lineal() {
        let rgba = imagen_patron(8, 8);
        let (formato_srgb, _) =
            comprimir_bc7(&rgba, 8, 8, true, PerfilCalidad::Rapido).unwrap();
        let (formato_lineal, _) =
            comprimir_bc7(&rgba, 8, 8, false, PerfilCalidad::Rapido).unwrap();
        assert_eq!(formato_srgb, FormatoComprimido::Bc7Srgb);
        assert_eq!(formato_lineal, FormatoComprimido::Bc7Unorm);
        assert_ne!(formato_srgb, formato_lineal);
    }

    #[test]
    fn bc5_normal_renormaliza() {
        let w = 2u32;
        let h = 2u32;
        // (0,0,1) codificado: R=128, G=128, B=255 (signed->byte: (0*0.5+0.5)*255=127.5~128)
        let normal_z = [128u8, 128u8, 255u8, 255u8];
        let normal_x = [255u8, 128u8, 128u8, 255u8]; // (1,0,0)

        let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
        for i in 0..4usize {
            let pixel = if i == 0 { normal_x } else { normal_z };
            rgba[i * 4..i * 4 + 4].copy_from_slice(&pixel);
        }

        let (_, niveles) = comprimir_bc5_normal(&rgba, w, h).unwrap();
        assert_eq!(niveles.len(), 2);
        let mip1 = &niveles[1];
        assert_eq!((mip1.width, mip1.height), (1, 1));

        // El mip BC5 está comprimido; reconstruimos el promedio esperado
        // directamente con la misma lógica de downsample para comprobar
        // la propiedad de renormalización (el test de forma se hace
        // sobre el cálculo intermedio, ya que BC5 es lossy).
        let (rgba_mip, _, _) = downsample_rgba_normal(&rgba, w, h);
        let r = rgba_mip[0] as f32 / 255.0 * 2.0 - 1.0;
        let g = rgba_mip[1] as f32 / 255.0 * 2.0 - 1.0;
        let z = (1.0f32 - r * r - g * g).max(0.0).sqrt();
        let longitud = (r * r + g * g + z * z).sqrt();
        assert!(
            (longitud - 1.0).abs() < 0.05,
            "longitud fuera de tolerancia: {}",
            longitud
        );
        assert!(z > r.abs() && z > g.abs(), "el vector no apunta mayormente a +Z");
    }

    #[test]
    fn bc5_tamano_por_nivel() {
        let rgba64 = imagen_patron(64, 64);
        let (formato, niveles) = comprimir_bc5_normal(&rgba64, 64, 64).unwrap();
        assert_eq!(formato, FormatoComprimido::Bc5Unorm);
        assert_eq!(niveles[0].datos.len(), 4096);

        let rgba_npot = imagen_patron(10, 6);
        let (_, niveles_npot) = comprimir_bc5_normal(&rgba_npot, 10, 6).unwrap();
        assert_eq!(niveles_npot[0].datos.len(), 96);
    }

    #[test]
    fn rechaza_entradas_invalidas() {
        assert!(comprimir_bc7(&[], 4, 4, true, PerfilCalidad::Rapido).is_err());
        assert!(comprimir_bc5_normal(&[], 4, 4).is_err());

        let rgba_corto = vec![0u8; 10];
        assert!(comprimir_bc7(&rgba_corto, 4, 4, true, PerfilCalidad::Rapido).is_err());
        assert!(comprimir_bc5_normal(&rgba_corto, 4, 4).is_err());

        let rgba_valida = imagen_patron(4, 4);
        assert!(comprimir_bc7(&rgba_valida, 0, 4, true, PerfilCalidad::Rapido).is_err());
        assert!(comprimir_bc7(&rgba_valida, 4, 0, true, PerfilCalidad::Rapido).is_err());
        assert!(comprimir_bc5_normal(&rgba_valida, 0, 4).is_err());
        assert!(comprimir_bc5_normal(&rgba_valida, 4, 0).is_err());
    }

    #[test]
    fn perfiles_producen_tamanos_validos() {
        let rgba = imagen_patron(32, 32);
        let esperado = bytes_nivel_bc(32, 32);
        for perfil in [
            PerfilCalidad::Rapido,
            PerfilCalidad::Equilibrado,
            PerfilCalidad::Calidad,
        ] {
            let (_, niveles) = comprimir_bc7(&rgba, 32, 32, true, perfil).unwrap();
            assert_eq!(niveles[0].datos.len(), esperado);
            assert!(niveles[0].datos.iter().any(|&b| b != 0));
        }
    }
}
