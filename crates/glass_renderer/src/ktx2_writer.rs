//! Escritor de contenedores KTX2 para texturas comprimidas BC ya generadas
//! en CPU (ver `mesh.rs::prepare_bc_upload`). Sin supercompresión, sin KVD,
//! sin SGD, solo texturas 2D de una capa.
//!
//! Disposición (spec Khronos KTX2):
//!   [12] identificador
//!   [68] cabecera (13 u32 + 2 u64: los offsets/longitudes SGD son u64)
//!   [24 * levelCount] índice de niveles
//!   [dfdByteLength] DFD (u32 dfdTotalSize + bloques)
//!   [datos de nivel, contiguos, 0..N-1]

use ash::vk;

const KTX2_IDENTIFIER: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];

/// Cabecera KTX2 completa: 12 de identificador + 68 de campos.
const HEADER_BYTE_LEN: u64 = 80;
const LEVEL_INDEX_ENTRY_LEN: u64 = 24;

/// Un nivel de mip ya comprimido, listo para escribir.
pub struct Ktx2Level<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// Bytes por bloque 4x4 para los formatos soportados. `None` si no soportado.
fn bytes_per_block(format: vk::Format) -> Option<u64> {
    match format {
        vk::Format::BC1_RGB_SRGB_BLOCK | vk::Format::BC1_RGBA_SRGB_BLOCK => Some(8),
        vk::Format::BC3_SRGB_BLOCK
        | vk::Format::BC7_SRGB_BLOCK
        | vk::Format::BC7_UNORM_BLOCK
        | vk::Format::BC5_UNORM_BLOCK => Some(16),
        _ => None,
    }
}

/// Mismo cálculo que `texture_level_bytes` en mesh.rs, restringido a los
/// formatos que este escritor soporta.
fn level_bytes(width: u32, height: u32, format: vk::Format) -> Result<u64, String> {
    let bpb = bytes_per_block(format).ok_or_else(|| {
        format!("formato KTX2 no soportado: {format:?}")
    })?;
    let bw = width.div_ceil(4) as u64;
    let bh = height.div_ceil(4) as u64;
    Ok(bw * bh * bpb)
}

/// Misma regla que `mip_level_count` en mesh.rs.
fn mip_level_count(width: u32, height: u32) -> u32 {
    let max_side = width.max(height).max(1);
    32 - max_side.leading_zeros()
}

/// Siguiente dimensión de mip: max(1, floor(prev / 2)).
fn next_mip_dim(prev: u32) -> u32 {
    (prev / 2).max(1)
}

/// Valida que `levels` sea la cadena completa de mips de `width`x`height`
/// (regla Vulkan) o un prefijo válido de ella, y que cada nivel tenga el
/// tamaño de datos exacto para su formato.
fn validate_levels(
    format: vk::Format,
    width: u32,
    height: u32,
    levels: &[Ktx2Level],
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err(format!(
            "dimensiones inválidas: {width}x{height}, ambas deben ser > 0"
        ));
    }
    if levels.is_empty() {
        return Err("se requiere al menos un nivel de mip".to_string());
    }

    let max_levels = mip_level_count(width, height);
    if levels.len() as u32 > max_levels {
        return Err(format!(
            "demasiados niveles: {} proporcionados, la cadena completa de {width}x{height} \
             solo tiene {max_levels}",
            levels.len()
        ));
    }

    let mut expected_w = width;
    let mut expected_h = height;
    for (i, level) in levels.iter().enumerate() {
        if level.width != expected_w || level.height != expected_h {
            return Err(format!(
                "nivel {i}: dimensiones {}x{} no coinciden con las esperadas {expected_w}x{expected_h} \
                 de la cadena de mips (nivel 0 debe ser {width}x{height}, los siguientes max(1, prev/2))",
                level.width, level.height
            ));
        }

        let expected_len = level_bytes(expected_w, expected_h, format)?;
        if level.data.len() as u64 != expected_len {
            return Err(format!(
                "nivel {i} ({}x{}): data.len() = {}, esperado {expected_len} bytes para este formato",
                level.width,
                level.height,
                level.data.len()
            ));
        }

        expected_w = next_mip_dim(expected_w);
        expected_h = next_mip_dim(expected_h);
    }

    Ok(())
}

/// Construye la sección DFD (Data Format Descriptor) para `format` usando
/// el crate `ktx2`: un u32 LE `dfdTotalSize` (que incluye sus propios 4
/// bytes) seguido del bloque DFD completo (`Block::Basic`, con su
/// BlockHeader). El u32 que devuelve `Basic::from_format` junto al bloque
/// es el `type_size` de la cabecera (1 para BC), NO el tamaño del DFD.
fn build_dfd(format: ktx2::Format) -> Result<Vec<u8>, String> {
    let (basic, _type_size) = ktx2::dfd::Basic::from_format(format)
        .map_err(|e| format!("no se pudo construir el DFD para {format:?}: {e:?}"))?;

    let block = ktx2::dfd::Block::Basic(basic).to_vec();
    let total_len = 4 + block.len();
    let mut dfd = Vec::with_capacity(total_len);
    dfd.extend_from_slice(&(total_len as u32).to_le_bytes());
    dfd.extend_from_slice(&block);
    debug_assert_eq!(dfd.len(), total_len);

    Ok(dfd)
}

/// Traduce `vk::Format` al `ktx2::Format` equivalente, restringido a los
/// formatos soportados por este escritor.
fn to_ktx2_format(format: vk::Format) -> Result<ktx2::Format, String> {
    match format {
        vk::Format::BC1_RGB_SRGB_BLOCK => Ok(ktx2::Format::BC1_RGB_SRGB_BLOCK),
        vk::Format::BC1_RGBA_SRGB_BLOCK => Ok(ktx2::Format::BC1_RGBA_SRGB_BLOCK),
        vk::Format::BC3_SRGB_BLOCK => Ok(ktx2::Format::BC3_SRGB_BLOCK),
        vk::Format::BC7_SRGB_BLOCK => Ok(ktx2::Format::BC7_SRGB_BLOCK),
        vk::Format::BC7_UNORM_BLOCK => Ok(ktx2::Format::BC7_UNORM_BLOCK),
        vk::Format::BC5_UNORM_BLOCK => Ok(ktx2::Format::BC5_UNORM_BLOCK),
        other => Err(format!("formato KTX2 no soportado: {other:?}")),
    }
}

/// Escribe un archivo KTX2 completo (sin supercompresión, sin KVD, sin SGD)
/// a partir de niveles ya comprimidos en el formato dado. Textura 2D, una
/// capa, sin cubemap/array/3D.
pub fn write_ktx2(
    format: vk::Format,
    width: u32,
    height: u32,
    levels: &[Ktx2Level],
) -> Result<Vec<u8>, String> {
    validate_levels(format, width, height, levels)?;
    let ktx2_format = to_ktx2_format(format)?;

    let dfd = build_dfd(ktx2_format)?;

    let level_count = levels.len() as u32;
    let index_byte_len = LEVEL_INDEX_ENTRY_LEN * level_count as u64;

    let dfd_byte_offset = HEADER_BYTE_LEN + index_byte_len;
    let dfd_byte_length = dfd.len() as u64;

    let mut level_offset = dfd_byte_offset + dfd_byte_length;
    let mut level_entries: Vec<(u64, u64, u64)> = Vec::with_capacity(levels.len());
    for level in levels {
        let len = level.data.len() as u64;
        debug_assert!(
            level_offset.is_multiple_of(4),
            "offset de nivel {level_offset} no alineado a 4 bytes"
        );
        if !level_offset.is_multiple_of(4) {
            return Err(format!(
                "offset de nivel {level_offset} no está alineado a 4 bytes"
            ));
        }
        level_entries.push((level_offset, len, len));
        level_offset += len;
    }

    let vk_format_value = format.as_raw() as u32;
    let type_size: u32 = 1; // Block-compressed: siempre 1.

    // Cabecera: 13 u32 y luego 2 u64 (sgdByteOffset/sgdByteLength son u64
    // en la spec; Header::LENGTH del crate es 80, no 72).
    let header_u32: [u32; 13] = [
        vk_format_value,
        type_size,
        width,
        height,
        0, // pixelDepth
        0, // layerCount (0 = sin array, una capa implícita)
        1, // faceCount
        level_count,
        0, // supercompressionScheme
        dfd_byte_offset as u32,
        dfd_byte_length as u32,
        0, // kvdByteOffset
        0, // kvdByteLength
    ];
    let header_u64: [u64; 2] = [
        0, // sgdByteOffset
        0, // sgdByteLength
    ];

    let mut out = Vec::with_capacity(level_offset as usize);
    out.extend_from_slice(&KTX2_IDENTIFIER);
    for field in header_u32 {
        out.extend_from_slice(&field.to_le_bytes());
    }
    for field in header_u64 {
        out.extend_from_slice(&field.to_le_bytes());
    }
    debug_assert_eq!(out.len() as u64, HEADER_BYTE_LEN);
    for (offset, length, uncompressed_length) in &level_entries {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&uncompressed_length.to_le_bytes());
    }
    out.extend_from_slice(&dfd);
    for level in levels {
        out.extend_from_slice(level.data);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{mip_level_count as mesh_mip_level_count, prepare_bc_upload, texture_level_bytes};

    fn synthetic_level_data(width: u32, height: u32, format: vk::Format, seed: u8) -> Vec<u8> {
        let len = level_bytes(width, height, format).unwrap() as usize;
        (0..len)
            .map(|i| seed.wrapping_add(i as u8))
            .collect()
    }

    fn full_mip_chain(
        width: u32,
        height: u32,
        format: vk::Format,
    ) -> Vec<(u32, u32, Vec<u8>)> {
        let count = mip_level_count(width, height);
        let mut w = width;
        let mut h = height;
        let mut out = Vec::with_capacity(count as usize);
        for level_index in 0..count {
            let data = synthetic_level_data(w, h, format, level_index as u8 * 17 + 1);
            out.push((w, h, data));
            w = next_mip_dim(w);
            h = next_mip_dim(h);
        }
        out
    }

    const ALL_FORMATS: [vk::Format; 6] = [
        vk::Format::BC1_RGB_SRGB_BLOCK,
        vk::Format::BC1_RGBA_SRGB_BLOCK,
        vk::Format::BC3_SRGB_BLOCK,
        vk::Format::BC7_SRGB_BLOCK,
        vk::Format::BC7_UNORM_BLOCK,
        vk::Format::BC5_UNORM_BLOCK,
    ];

    #[test]
    fn roundtrip_all_supported_formats() {
        for &format in &ALL_FORMATS {
            let width = 16;
            let height = 16;
            let chain = full_mip_chain(width, height, format);
            // Solo 5 niveles (la cadena completa de 16x16 tiene 5).
            let chain: Vec<_> = chain.into_iter().take(5).collect();
            let levels: Vec<Ktx2Level> = chain
                .iter()
                .map(|(w, h, data)| Ktx2Level {
                    data,
                    width: *w,
                    height: *h,
                })
                .collect();

            let bytes = write_ktx2(format, width, height, &levels)
                .unwrap_or_else(|e| panic!("write_ktx2 falló para {format:?}: {e}"));

            let reader = ktx2::Reader::new(&bytes)
                .unwrap_or_else(|e| panic!("re-parseo falló para {format:?}: {e:?}"));
            let header = reader.header();

            assert_eq!(header.format, Some(to_ktx2_format(format).unwrap()));
            assert_eq!(header.type_size, 1);
            assert_eq!(header.pixel_width, width);
            assert_eq!(header.pixel_height, height);
            assert_eq!(header.level_count, levels.len() as u32);
            assert_eq!(header.supercompression_scheme, None);

            assert!(
                reader.basic_dfd().is_some(),
                "DFD ausente para {format:?}"
            );

            let parsed_levels: Vec<_> = reader.levels().collect();
            assert_eq!(parsed_levels.len(), levels.len());
            for (i, (parsed, (_, _, original))) in
                parsed_levels.iter().zip(chain.iter()).enumerate()
            {
                assert_eq!(
                    parsed.data,
                    original.as_slice(),
                    "nivel {i} no coincide para {format:?}"
                );
            }
        }
    }

    #[test]
    fn file_starts_with_ktx2_identifier() {
        let width = 8;
        let height = 8;
        let format = vk::Format::BC1_RGB_SRGB_BLOCK;
        let data = synthetic_level_data(width, height, format, 1);
        let levels = [Ktx2Level { data: &data, width, height }];
        let bytes = write_ktx2(format, width, height, &levels).unwrap();
        assert_eq!(&bytes[..12], &KTX2_IDENTIFIER);
    }

    #[test]
    fn matches_prepare_bc_upload_output() {
        use crate::mesh::GltfImageData;

        let width = 64u32;
        let height = 64u32;
        // Imagen RGBA sintética no trivial.
        let pixels: Vec<u8> = (0..(width * height * 4))
            .map(|i| (i % 251) as u8)
            .collect();
        let image = GltfImageData {
            width,
            height,
            rgba: pixels,
        };

        let (format, concatenated, level_meta) = prepare_bc_upload(&image);

        let levels: Vec<Ktx2Level> = level_meta
            .iter()
            .map(|&(offset, w, h)| {
                let len = texture_level_bytes(w, h, format) as usize;
                Ktx2Level {
                    data: &concatenated[offset as usize..offset as usize + len],
                    width: w,
                    height: h,
                }
            })
            .collect();

        let bytes = write_ktx2(format, width, height, &levels).unwrap();

        let reader = ktx2::Reader::new(&bytes).unwrap();
        let parsed_levels: Vec<_> = reader.levels().collect();
        assert_eq!(parsed_levels.len(), levels.len());
        for (parsed, original) in parsed_levels.iter().zip(levels.iter()) {
            assert_eq!(parsed.data, original.data);
        }
    }

    #[test]
    fn rejects_empty_levels() {
        let result = write_ktx2(vk::Format::BC1_RGB_SRGB_BLOCK, 16, 16, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_zero_width_or_height() {
        let data = synthetic_level_data(16, 16, vk::Format::BC1_RGB_SRGB_BLOCK, 1);
        let levels = [Ktx2Level { data: &data, width: 16, height: 16 }];
        assert!(write_ktx2(vk::Format::BC1_RGB_SRGB_BLOCK, 0, 16, &levels).is_err());
        assert!(write_ktx2(vk::Format::BC1_RGB_SRGB_BLOCK, 16, 0, &levels).is_err());
    }

    #[test]
    fn rejects_level_zero_dimension_mismatch() {
        let format = vk::Format::BC1_RGB_SRGB_BLOCK;
        let data = synthetic_level_data(8, 8, format, 1);
        let levels = [Ktx2Level { data: &data, width: 8, height: 8 }];
        // Declarado como 16x16 pero el nivel 0 trae 8x8.
        let result = write_ktx2(format, 16, 16, &levels);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_broken_mip_chain() {
        let format = vk::Format::BC1_RGB_SRGB_BLOCK;
        let level0 = synthetic_level_data(16, 16, format, 1);
        // Tras 16x16 debería venir 8x8, no 9x9.
        let level1 = synthetic_level_data(9, 9, format, 2);
        let levels = [
            Ktx2Level { data: &level0, width: 16, height: 16 },
            Ktx2Level { data: &level1, width: 9, height: 9 },
        ];
        let result = write_ktx2(format, 16, 16, &levels);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_wrong_level_data_length() {
        let format = vk::Format::BC1_RGB_SRGB_BLOCK;
        let wrong_data = vec![0u8; 10]; // tamaño incorrecto para 16x16 BC1.
        let levels = [Ktx2Level { data: &wrong_data, width: 16, height: 16 }];
        let result = write_ktx2(format, 16, 16, &levels);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_unsupported_format() {
        let data = vec![0u8; 16 * 16 * 4];
        let levels = [Ktx2Level { data: &data, width: 16, height: 16 }];
        let result = write_ktx2(vk::Format::R8G8B8A8_SRGB, 16, 16, &levels);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("R8G8B8A8_SRGB"));
    }

    #[test]
    fn npot_texture_roundtrip() {
        let format = vk::Format::BC3_SRGB_BLOCK;
        let width = 10u32;
        let height = 6u32;
        let chain = full_mip_chain(width, height, format);
        let levels: Vec<Ktx2Level> = chain
            .iter()
            .map(|(w, h, data)| Ktx2Level { data, width: *w, height: *h })
            .collect();

        // Comprobación directa del tamaño esperado por ceil(w/4)*ceil(h/4)*16.
        let expected_level0_bytes = (10u64.div_ceil(4)) * (6u64.div_ceil(4)) * 16;
        assert_eq!(chain[0].2.len() as u64, expected_level0_bytes);

        let bytes = write_ktx2(format, width, height, &levels).unwrap();
        let reader = ktx2::Reader::new(&bytes).unwrap();
        let parsed_levels: Vec<_> = reader.levels().collect();
        assert_eq!(parsed_levels.len(), levels.len());
        for (parsed, (_, _, original)) in parsed_levels.iter().zip(chain.iter()) {
            assert_eq!(parsed.data, original.as_slice());
        }
    }

    #[test]
    fn mip_level_count_matches_mesh_helper() {
        assert_eq!(mip_level_count(16, 16), mesh_mip_level_count(16, 16));
        assert_eq!(mip_level_count(10, 6), mesh_mip_level_count(10, 6));
    }
}
