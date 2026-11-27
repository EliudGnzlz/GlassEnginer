//! Compresión a KTX2 de las texturas sueltas importadas.
//!
//! Parte de la PNG que ya produjo `convertir_textura` (esa vía no cambia) y
//! genera el `.ktx2` que se escribe a su lado. El rol sale del `.meta` de
//! Unity: NormalMap → BC5; resto → BC7 SRGB o UNORM según `srgb`.

use ash::vk;
use glass_renderer::ktx2_writer::{write_ktx2, Ktx2Level};

use crate::compresor_texturas::{
    comprimir_bc5_normal, comprimir_bc7, FormatoComprimido, NivelComprimido, PerfilCalidad,
};
use crate::{MetaAsset, TexturaInfo};

/// Perfil fijo del importador. Equilibrado: calidad suficiente sin disparar
/// el tiempo de importación de paquetes grandes.
pub const PERFIL_KTX2_IMPORT: PerfilCalidad = PerfilCalidad::Equilibrado;

/// Comprime los bytes PNG ya convertidos a KTX2. Decodifica el PNG con el
/// crate `image` y comprime con el compresor de S1.
/// Rol: NormalMap → BC5; resto → BC7 (SRGB si el .meta dice srgb, UNORM si
/// dice lineal). Sin .meta (o sin TexturaInfo) → BC7 SRGB.
/// Un PNG que no decodifica → Err (el llamador lo convierte en aviso).
pub fn comprimir_textura_importada(
    png: &[u8],
    meta: Option<&MetaAsset>,
) -> Result<(Vec<u8>, FormatoComprimido), String> {
    comprimir_con_info(png, meta.and_then(|m| m.textura.as_ref()))
}

/// Núcleo de `comprimir_textura_importada`, separado para poder probar los
/// roles construyendo solo `TexturaInfo`.
fn comprimir_con_info(
    png: &[u8],
    info: Option<&TexturaInfo>,
) -> Result<(Vec<u8>, FormatoComprimido), String> {
    let rgba = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|e| format!("el PNG no decodifica: {e}"))?
        .to_rgba8();
    let (width, height) = rgba.dimensions();
    let (formato, niveles) = match info {
        Some(t) if t.tipo == "NormalMap" => comprimir_bc5_normal(rgba.as_raw(), width, height)?,
        Some(t) => comprimir_bc7(rgba.as_raw(), width, height, t.srgb, PERFIL_KTX2_IMPORT)?,
        None => comprimir_bc7(rgba.as_raw(), width, height, true, PERFIL_KTX2_IMPORT)?,
    };
    let ktx2 = ensamblar_ktx2(formato, width, height, &niveles)?;
    Ok((ktx2, formato))
}

/// Traduce el formato de S1 a `vk::Format` y serializa los niveles con el
/// escritor KTX2 de G4a. Compartido con la reescritura de imágenes del GLB.
pub(crate) fn ensamblar_ktx2(
    formato: FormatoComprimido,
    width: u32,
    height: u32,
    niveles: &[NivelComprimido],
) -> Result<Vec<u8>, String> {
    let formato_vk = match formato {
        FormatoComprimido::Bc7Srgb => vk::Format::BC7_SRGB_BLOCK,
        FormatoComprimido::Bc7Unorm => vk::Format::BC7_UNORM_BLOCK,
        FormatoComprimido::Bc5Unorm => vk::Format::BC5_UNORM_BLOCK,
    };
    let levels: Vec<Ktx2Level> = niveles
        .iter()
        .map(|n| Ktx2Level { data: &n.datos, width: n.width, height: n.height })
        .collect();
    write_ktx2(formato_vk, width, height, &levels)
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

    fn info(tipo: &str, srgb: bool) -> TexturaInfo {
        TexturaInfo {
            tipo: tipo.to_string(),
            srgb,
            filtro: "Bilinear".to_string(),
            wrap_u: "Repeat".to_string(),
            wrap_v: "Repeat".to_string(),
        }
    }

    /// Comprueba formato y niveles del KTX2 contra lo que S1 produce para la
    /// misma imagen (la regla de niveles es la de S1, no se duplica aquí).
    fn comprobar_ktx2(ktx2: &[u8], formato: ktx2::Format, esperados: &[NivelComprimido]) {
        let reader = ktx2::Reader::new(ktx2).expect("el KTX2 debe parsear");
        assert_eq!(reader.header().format, Some(formato));
        let niveles: Vec<_> = reader.levels().collect();
        assert_eq!(niveles.len(), esperados.len());
        for (leido, esperado) in niveles.iter().zip(esperados) {
            assert_eq!(leido.data.len(), bytes_nivel_bc(esperado.width, esperado.height));
        }
        assert_eq!(esperados[0].width, 16);
        assert_eq!(esperados[0].height, 16);
    }

    fn rgba_rojo_16() -> Vec<u8> {
        image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 0, 0, 255])).into_raw()
    }

    #[test]
    fn comprimir_textura_importada_color() {
        let (ktx2, formato) = comprimir_textura_importada(&png_rojo(16), None).expect("Ok");
        assert_eq!(formato, FormatoComprimido::Bc7Srgb);
        let (_, esperados) =
            comprimir_bc7(&rgba_rojo_16(), 16, 16, true, PERFIL_KTX2_IMPORT).unwrap();
        comprobar_ktx2(&ktx2, ktx2::Format::BC7_SRGB_BLOCK, &esperados);
    }

    #[test]
    fn comprimir_textura_importada_normal() {
        let t = info("NormalMap", false);
        let (ktx2, formato) = comprimir_con_info(&png_rojo(16), Some(&t)).expect("Ok");
        assert_eq!(formato, FormatoComprimido::Bc5Unorm);
        let (_, esperados) = comprimir_bc5_normal(&rgba_rojo_16(), 16, 16).unwrap();
        comprobar_ktx2(&ktx2, ktx2::Format::BC5_UNORM_BLOCK, &esperados);
    }

    #[test]
    fn comprimir_textura_importada_lineal() {
        let t = info("Default", false);
        let (ktx2, formato) = comprimir_con_info(&png_rojo(16), Some(&t)).expect("Ok");
        assert_eq!(formato, FormatoComprimido::Bc7Unorm);
        let (_, esperados) =
            comprimir_bc7(&rgba_rojo_16(), 16, 16, false, PERFIL_KTX2_IMPORT).unwrap();
        comprobar_ktx2(&ktx2, ktx2::Format::BC7_UNORM_BLOCK, &esperados);
    }

    #[test]
    fn comprimir_textura_importada_rechaza_basura() {
        assert!(comprimir_textura_importada(b"no es un png", None).is_err());
    }
}
