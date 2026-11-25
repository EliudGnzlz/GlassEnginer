use std::io::Cursor;

use image::ImageFormat;

use crate::{MetaAsset, RipperError};

const FIRMA_PNG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Convierte los bytes de una textura de Unity en PNG, que es lo
/// único que carga el motor. Devuelve los bytes PNG y los avisos.
/// - png → se copia TAL CUAL (sin recodificar), tras comprobar que
///   la cabecera es PNG.
/// - jpg/jpeg/tga/bmp/gif → se decodifica con el crate `image` y se
///   codifica a PNG.
/// - psd/exr y cualquier otra extensión → error Conversion con
///   motivo "formato de textura no convertible en v1: <ext>".
pub fn convertir_textura(
    ruta: &str,
    extension: &str,
    bytes: &[u8],
    meta: Option<&MetaAsset>,
) -> Result<(Vec<u8>, Vec<String>), RipperError> {
    let extension = extension.trim_start_matches('.').to_lowercase();

    let png = match extension.as_str() {
        "png" => {
            if !bytes.starts_with(&FIRMA_PNG) {
                return Err(RipperError::Conversion {
                    ruta: ruta.to_owned(),
                    motivo: "cabecera PNG no válida".to_owned(),
                });
            }
            bytes.to_vec()
        }
        "jpg" | "jpeg" | "tga" | "bmp" | "gif" => {
            let formato = match extension.as_str() {
                "jpg" | "jpeg" => ImageFormat::Jpeg,
                "tga" => ImageFormat::Tga,
                "bmp" => ImageFormat::Bmp,
                "gif" => ImageFormat::Gif,
                _ => unreachable!("la extensión ya ha sido validada"),
            };
            let imagen = image::load_from_memory_with_format(bytes, formato).map_err(|error| {
                RipperError::Conversion {
                    ruta: ruta.to_owned(),
                    motivo: format!("no se pudo decodificar la textura {extension}: {error}"),
                }
            })?;
            let mut png = Vec::new();
            imagen
                .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
                .map_err(|error| RipperError::Conversion {
                    ruta: ruta.to_owned(),
                    motivo: format!("no se pudo codificar la textura como PNG: {error}"),
                })?;
            png
        }
        _ => {
            return Err(RipperError::Conversion {
                ruta: ruta.to_owned(),
                motivo: format!("formato de textura no convertible en v1: {extension}"),
            });
        }
    };

    Ok((png, avisos_de_ajustes(meta)))
}

/// Recoge los ajustes de importación que el motor no puede representar.
fn avisos_de_ajustes(meta: Option<&MetaAsset>) -> Vec<String> {
    let Some(textura) = meta.and_then(|meta| meta.textura.as_ref()) else {
        return Vec::new();
    };

    // Adaptado al integrar (Rayo): el TexturaInfo real de U1 trae los
    // ajustes ya decodificados como texto (tipo, filtro, wraps) y el
    // sRGB como bool, no como enteros opcionales del .meta crudo.
    let mut avisos = Vec::new();
    if !textura.srgb {
        avisos.push(
            "textura marcada como lineal (sRGB desactivado en Unity); el motor la tratará como sRGB"
                .to_owned(),
        );
    }
    if textura.tipo == "NormalMap" {
        avisos.push(
            "textura de tipo NormalMap: el material del motor no tiene ranura de normales en v1"
                .to_owned(),
        );
    }
    if textura.filtro != "Bilinear" || textura.wrap_u != "Repeat" || textura.wrap_v != "Repeat" {
        avisos.push(
            "el motor usa un único modo de filtrado/envoltura para todas las texturas (v1)"
                .to_owned(),
        );
    }
    avisos
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, ImageBuffer, Rgb};

    use super::*;
    use crate::TexturaInfo;

    const PNG_MINIMO: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0,
        1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 8, 215, 99, 248,
        207, 192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68,
        174, 66, 96, 130,
    ];

    fn meta_con_textura(textura: TexturaInfo) -> MetaAsset {
        MetaAsset {
            guid: "guid-de-prueba".to_owned(),
            importer: "TextureImporter".to_owned(),
            sub_objetos: Vec::new(),
            global_scale: None,
            use_file_scale: None,
            textura: Some(textura),
        }
    }

    #[test]
    fn copia_un_png_sin_recodificar() {
        let (convertido, avisos) =
            convertir_textura("pixel.png", "PNG", PNG_MINIMO, None).unwrap();

        assert_eq!(convertido, PNG_MINIMO);
        assert!(avisos.is_empty());
    }

    #[test]
    fn convierte_un_jpeg_a_png_de_uno_por_uno() {
        let imagen = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(1, 1, Rgb([255, 0, 0])));
        let mut jpeg = Vec::new();
        imagen
            .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .unwrap();

        let (png, avisos) =
            convertir_textura("rojo.jpeg", "JpEg", &jpeg, None).unwrap();
        assert!(png.starts_with(&FIRMA_PNG));
        assert!(avisos.is_empty());

        let decodificada =
            image::load_from_memory_with_format(&png, ImageFormat::Png).unwrap();
        assert_eq!((decodificada.width(), decodificada.height()), (1, 1));
    }

    #[test]
    fn rechaza_los_formatos_no_convertibles() {
        let resultado = convertir_textura("textura.psd", "PSD", &[], None);
        match resultado {
            Err(RipperError::Conversion { motivo, .. }) => {
                assert_eq!(motivo, "formato de textura no convertible en v1: psd");
            }
            _ => panic!("un PSD debe producir un error de conversión"),
        }
    }

    #[test]
    fn avisa_de_color_lineal_y_mapa_de_normales() {
        let meta = meta_con_textura(TexturaInfo {
            tipo: "NormalMap".to_owned(),
            srgb: false,
            filtro: "Bilinear".to_owned(),
            wrap_u: "Repeat".to_owned(),
            wrap_v: "Repeat".to_owned(),
        });

        let (_, avisos) =
            convertir_textura("normal.png", "png", PNG_MINIMO, Some(&meta)).unwrap();
        assert_eq!(avisos.len(), 2);
        assert_eq!(
            avisos[0],
            "textura marcada como lineal (sRGB desactivado en Unity); el motor la tratará como sRGB"
        );
        assert_eq!(
            avisos[1],
            "textura de tipo NormalMap: el material del motor no tiene ranura de normales en v1"
        );
    }

    #[test]
    fn no_avisa_de_los_ajustes_habituales() {
        let meta = meta_con_textura(TexturaInfo {
            tipo: "Default".to_owned(),
            srgb: true,
            filtro: "Bilinear".to_owned(),
            wrap_u: "Repeat".to_owned(),
            wrap_v: "Repeat".to_owned(),
        });

        let (_, avisos) =
            convertir_textura("albedo.png", "png", PNG_MINIMO, Some(&meta)).unwrap();
        assert!(avisos.is_empty());
    }
}
