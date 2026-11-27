//! Loader de texturas KTX2 con niveles BC ya comprimidos.
//!
//! G4a: parsea, valida y sube a VRAM archivos .ktx2 sin supercompresión
//! (Basis descartado). Reutiliza la subida de niveles comprimidos de
//! `mesh.rs` (`upload_compressed_levels`).

use ash::vk;
use gpu_allocator::vulkan::Allocator;

use crate::mesh::{texture_bc_enabled, texture_level_bytes, GpuTexture, MeshLoader};

/// Un .ktx2 ya parseado y validado, en la misma forma que la salida de
/// `prepare_bc_upload`: (formato, bytes concatenados, [(offset, ancho, alto)
/// por nivel]).
#[derive(Debug)]
pub struct Ktx2Texture {
    pub format: vk::Format,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub levels: Vec<(u64, u32, u32)>,
}

/// Mapeo permitido `ktx2::Format` -> `vk::Format` (mismo nombre de variante).
fn map_format(f: ktx2::Format) -> Result<vk::Format, String> {
    match f {
        ktx2::Format::BC1_RGB_SRGB_BLOCK => Ok(vk::Format::BC1_RGB_SRGB_BLOCK),
        ktx2::Format::BC1_RGBA_SRGB_BLOCK => Ok(vk::Format::BC1_RGBA_SRGB_BLOCK),
        ktx2::Format::BC3_SRGB_BLOCK => Ok(vk::Format::BC3_SRGB_BLOCK),
        ktx2::Format::BC7_SRGB_BLOCK => Ok(vk::Format::BC7_SRGB_BLOCK),
        ktx2::Format::BC7_UNORM_BLOCK => Ok(vk::Format::BC7_UNORM_BLOCK),
        ktx2::Format::BC5_UNORM_BLOCK => Ok(vk::Format::BC5_UNORM_BLOCK),
        otro => Err(format!(
            "formato KTX2 no soportado: {:?} (solo se admiten BC1/BC3/BC5/BC7 sRGB/UNORM)",
            otro
        )),
    }
}

/// Parsea y valida un archivo KTX2 completo en memoria.
pub fn parse_ktx2(bytes: &[u8]) -> Result<Ktx2Texture, String> {
    let reader = ktx2::Reader::new(bytes).map_err(|e| format!("KTX2 inválido: {}", e))?;
    let header = reader.header();

    // Supercompresión (Basis u otra): no soportada.
    if header.supercompression_scheme.is_some() {
        return Err(format!(
            "esquema de supercompresión no soportado: {:?}",
            header.supercompression_scheme
        ));
    }

    // Solo textura 2D de una capa.
    if header.pixel_depth != 0 {
        return Err(format!(
            "textura 3D no soportada (pixel_depth = {})",
            header.pixel_depth
        ));
    }
    if header.layer_count > 1 {
        return Err(format!(
            "texturas array no soportadas (layer_count = {})",
            header.layer_count
        ));
    }
    if header.face_count != 1 {
        return Err(format!(
            "cubemaps no soportadas (face_count = {})",
            header.face_count
        ));
    }

    if header.pixel_width == 0 || header.pixel_height == 0 {
        return Err(format!(
            "dimensiones inválidas: {}x{}",
            header.pixel_width, header.pixel_height
        ));
    }
    if header.level_count < 1 {
        return Err("el archivo no contiene ningún nivel de mipmap".to_string());
    }

    let kfmt = header
        .format
        .ok_or_else(|| "formato del archivo ausente (VK_FORMAT_UNDEFINED)".to_string())?;
    let format = map_format(kfmt)?;

    let width = header.pixel_width;
    let height = header.pixel_height;

    let levels_iter = reader.levels();
    if levels_iter.len() as u32 != header.level_count {
        return Err(format!(
            "level_count ({}) no casa con los niveles presentes ({})",
            header.level_count,
            levels_iter.len()
        ));
    }

    let mut data: Vec<u8> = Vec::new();
    let mut levels_info: Vec<(u64, u32, u32)> = Vec::with_capacity(header.level_count as usize);

    // Dimensiones por nivel con la regla iterativa de la spec Vulkan
    // (max(1, prev/2)), la misma que usa el escritor; el desplazamiento
    // directo (width >> i) es matemáticamente equivalente, pero la forma
    // iterativa no deja lugar a dudas y casa con write_ktx2.
    let mut lw = width;
    let mut lh = height;
    for (i, level) in levels_iter.enumerate() {
        let esperado = texture_level_bytes(lw, lh, format);
        let real = level.data.len() as u64;

        if real != esperado {
            return Err(format!(
                "nivel {} ({}x{}, {:?}): longitud {} bytes, se esperaban {}",
                i, lw, lh, format, real, esperado
            ));
        }
        if level.uncompressed_byte_length != real {
            return Err(format!(
                "nivel {}: uncompressed_byte_length={} != data.len()={} (supercompresión inesperada)",
                i, level.uncompressed_byte_length, real
            ));
        }

        let offset = data.len() as u64;
        data.extend_from_slice(level.data);
        levels_info.push((offset, lw, lh));
        lw = (lw / 2).max(1);
        lh = (lh / 2).max(1);
    }

    Ok(Ktx2Texture {
        format,
        width,
        height,
        data,
        levels: levels_info,
    })
}

/// Sube a VRAM un .ktx2 con niveles BC. Sin BC activo devuelve `Err` para
/// que el llamador caiga a la ruta RGBA8.
pub fn upload_texture_to_gpu_ktx2(
    device: &ash::Device,
    allocator: &mut Allocator,
    bytes: &[u8],
    queue: vk::Queue,
    command_pool: vk::CommandPool,
) -> Result<GpuTexture, String> {
    let tex = parse_ktx2(bytes)?;

    if !texture_bc_enabled() {
        return Err(
            "la GPU no soporta compresión BC; el llamador debe caer a la ruta RGBA8".to_string(),
        );
    }

    MeshLoader::upload_compressed_levels(
        device,
        allocator,
        tex.width,
        tex.height,
        tex.format,
        &tex.data,
        &tex.levels,
        queue,
        command_pool,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ktx2::dfd;

    const KTX2_ID: [u8; 12] = [
        0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB, b'\r', b'\n', 0x1A, b'\n',
    ];

    fn u32le(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    // Reescritura limpia: builder explícito.
    struct BuildOpts {
        fmt: u32,
        tw: u32,
        th: u32,
        td: u32,
        lc: u32,
        fc: u32,
        nlevels: u32,
        scs: u32,
        /// Modifica las longitudes reales de los datos por nivel.
        /// Puntero a función (no Box<dyn Fn>): el único uso es un closure
        /// sin captura, y el tipo complejo disparaba clippy::type_complexity.
        len_override: Option<fn(u32, u32, u32) -> u32>,
    }

    fn block_bytes(fmt: vk::Format) -> u32 {
        match fmt {
            vk::Format::BC1_RGB_SRGB_BLOCK | vk::Format::BC1_RGBA_SRGB_BLOCK => 8,
            vk::Format::BC3_SRGB_BLOCK => 16,
            vk::Format::BC7_SRGB_BLOCK | vk::Format::BC7_UNORM_BLOCK => 16,
            vk::Format::BC5_UNORM_BLOCK => 16,
            _ => 16,
        }
    }

    fn vk_of(fmt: u32) -> vk::Format {
        match fmt {
            131 => vk::Format::BC1_RGB_UNORM_BLOCK,
            132 => vk::Format::BC1_RGB_SRGB_BLOCK,
            133 => vk::Format::BC1_RGBA_UNORM_BLOCK,
            134 => vk::Format::BC1_RGBA_SRGB_BLOCK,
            137 => vk::Format::BC3_UNORM_BLOCK,
            138 => vk::Format::BC3_SRGB_BLOCK,
            141 => vk::Format::BC5_UNORM_BLOCK,
            145 => vk::Format::BC7_UNORM_BLOCK,
            146 => vk::Format::BC7_SRGB_BLOCK,
            _ => vk::Format::UNDEFINED,
        }
    }

    fn block_size_for(fmt: u32) -> u32 {
        let f = vk_of(fmt);
        block_bytes(f)
    }

    fn build_ktx2(o: &BuildOpts) -> Vec<u8> {
        let level_lens: Vec<u32> = (0..o.nlevels)
            .map(|i| {
                let w = (o.tw >> i).max(1);
                let h = (o.th >> i).max(1);
                let natural = w.div_ceil(4) * h.div_ceil(4) * block_size_for(o.fmt);
                match &o.len_override {
                    Some(f) => f(w, h, natural),
                    None => natural,
                }
            })
            .collect();
        let level_data: Vec<Vec<u8>> = level_lens
            .iter()
            .enumerate()
            .map(|(i, len)| vec![(i as u8).wrapping_mul(37).wrapping_add(11); *len as usize])
            .collect();

        // DFD: u32 dfdTotalSize (incluye sus 4 bytes) + bloque DFD completo.
        // Se construye desde el formato declarado; si el crate no lo conoce
        // (p. ej. formato no soportado o UNDEFINED), se usa un DFD válido
        // de BC1 para que el parseo del lector llegue al rechazo de formato.
        let dfd_block: Vec<u8> =
            match ktx2::Format::new(o.fmt).and_then(|f| dfd::Basic::from_format(f).ok()) {
                Some((basic, _)) => dfd::Block::Basic(basic).to_vec(),
                None => dfd::Block::Basic(
                    dfd::Basic::from_format(ktx2::Format::BC1_RGBA_SRGB_BLOCK)
                        .expect("dfd")
                        .0,
                )
                .to_vec(),
            };
        let dfd_total = 4 + dfd_block.len();
        let dfd_length = dfd_total as u32;

        // Cabecera KTX2: 80 bytes (12 identificador + 13 u32 + 2 u64 SGD).
        let header_len = 80u32 + 24 * o.nlevels;
        let dfd_offset = header_len;
        let data_start = dfd_offset + dfd_length;

        let mut out =
            Vec::with_capacity((data_start as usize) + level_data.iter().map(|v| v.len()).sum::<usize>());
        out.extend_from_slice(&KTX2_ID);
        // 13 u32: vkFormat, typeSize=1, pixelWidth, pixelHeight, pixelDepth,
        // layerCount, faceCount, levelCount, supercompressionScheme,
        // dfdByteOffset, dfdByteLength, kvdByteOffset, kvdByteLength.
        let header_u32: [u32; 13] = [
            o.fmt,
            1, // typeSize (1 para block-compressed)
            o.tw,
            o.th,
            o.td,
            o.lc,
            o.fc,
            o.nlevels,
            o.scs,
            dfd_offset,
            dfd_length,
            0, // kvdByteOffset
            0, // kvdByteLength
        ];
        for v in header_u32 {
            u32le(&mut out, v);
        }
        for v in [0u64, 0u64] {
            // sgdByteOffset, sgdByteLength (u64).
            out.extend_from_slice(&v.to_le_bytes());
        }
        debug_assert_eq!(out.len(), 80);
        // Índice de niveles.
        let mut off = data_start;
        for d in level_data.iter() {
            let li = ktx2::LevelIndex {
                byte_offset: off as u64,
                byte_length: d.len() as u64,
                uncompressed_byte_length: d.len() as u64,
            };
            out.extend_from_slice(&li.as_bytes());
            off += d.len() as u32;
        }
        // DFD (con su prefijo de longitud total).
        out.extend_from_slice(&(dfd_total as u32).to_le_bytes());
        out.extend_from_slice(&dfd_block);
        // Datos.
        for d in &level_data {
            out.extend_from_slice(d);
        }
        out
    }

    const SOPORTADOS: [(u32, vk::Format); 6] = [
        (132, vk::Format::BC1_RGB_SRGB_BLOCK),
        (134, vk::Format::BC1_RGBA_SRGB_BLOCK),
        (138, vk::Format::BC3_SRGB_BLOCK),
        (146, vk::Format::BC7_SRGB_BLOCK),
        (145, vk::Format::BC7_UNORM_BLOCK),
        (141, vk::Format::BC5_UNORM_BLOCK),
    ];

    fn base_opts(fmt: u32) -> BuildOpts {
        BuildOpts {
            fmt,
            tw: 8,
            th: 8,
            td: 0,
            lc: 1,
            fc: 1,
            nlevels: 4,
            scs: 0,
            len_override: None,
        }
    }

    #[test]
    fn parse_valido_seis_formatos() {
        for (code, vkf) in SOPORTADOS {
            let opts = base_opts(code);
            let blob = build_ktx2(&opts);
            let t = parse_ktx2(&blob).expect("parse");
            assert_eq!(t.format, vkf);
            assert_eq!(t.width, 8);
            assert_eq!(t.height, 8);
            assert_eq!(t.levels.len(), 4);
            // Offsets acumulados.
            let mut acc = 0u64;
            for (i, (off, w, h)) in t.levels.iter().enumerate() {
                assert_eq!(*off, acc);
                let ew = (8u32 >> i).max(1);
                let eh = (8u32 >> i).max(1);
                assert_eq!((*w, *h), (ew, eh));
                let lb = texture_level_bytes(ew, eh, vkf);
                acc += lb;
            }
            assert_eq!(t.data.len() as u64, acc);
        }
    }

    #[test]
    fn rechazo_bytes_vacios() {
        assert!(parse_ktx2(&[]).is_err());
    }

    #[test]
    fn rechazo_identificador_roto() {
        let mut blob = build_ktx2(&base_opts(134));
        blob[0] = 0x00;
        assert!(parse_ktx2(&blob).is_err());
    }

    #[test]
    fn rechazo_formato_rgba8() {
        // VK_FORMAT_R8G8B8A8_SRGB = 43.
        let blob = build_ktx2(&base_opts(43));
        let e = parse_ktx2(&blob).unwrap_err();
        assert!(e.contains("formato"), "{}", e);
    }

    #[test]
    fn rechazo_formato_none() {
        // vkFormat = 0 => VK_FORMAT_UNDEFINED.
        let blob = build_ktx2(&base_opts(0));
        assert!(parse_ktx2(&blob).is_err());
    }

    #[test]
    fn rechazo_supercompresion() {
        let mut o = base_opts(134);
        o.scs = 1; // BASIS_LZ supercompression scheme = 1
        let blob = build_ktx2(&o);
        assert!(parse_ktx2(&blob).is_err());
    }

    #[test]
    fn rechazo_cubemap() {
        let mut o = base_opts(134);
        o.fc = 6;
        let blob = build_ktx2(&o);
        assert!(parse_ktx2(&blob).is_err());
    }

    #[test]
    fn rechazo_3d() {
        let mut o = base_opts(134);
        o.td = 1;
        let blob = build_ktx2(&o);
        assert!(parse_ktx2(&blob).is_err());
    }

    #[test]
    fn rechazo_longitud_nivel_incorrecta() {
        let mut o = base_opts(134);
        o.len_override = Some(|_w, _h, natural| natural + 4);
        let blob = build_ktx2(&o);
        assert!(parse_ktx2(&blob).is_err());
    }

    #[test]
    fn rechazo_level_count_desajustado() {
        // Declara 4 niveles pero sólo se emiten 2 en el índice.
        // Truco: nlevels=2 en datos, levelCount=4 en cabecera.
        let mut o = base_opts(134);
        o.nlevels = 2;
        let mut blob = build_ktx2(&o);
        // levelCount es el octavo u32 tras el identificador.
        // 12 + 7*4 = 40.
        blob[40..44].copy_from_slice(&4u32.to_le_bytes());
        assert!(parse_ktx2(&blob).is_err());
    }

    #[test]
    fn parse_npot_10x6() {
        let mut o = base_opts(134);
        o.tw = 10;
        o.th = 6;
        o.nlevels = 4;
        let blob = build_ktx2(&o);
        let t = parse_ktx2(&blob).expect("parse");
        let vkf = vk::Format::BC1_RGBA_SRGB_BLOCK;
        let mut acc = 0u64;
        for (i, (off, w, h)) in t.levels.iter().enumerate() {
            assert_eq!(*off, acc);
            let ew = (10u32 >> i).max(1);
            let eh = (6u32 >> i).max(1);
            assert_eq!((*w, *h), (ew, eh));
            let lb = ew.div_ceil(4) as u64 * eh.div_ceil(4) as u64 * 8;
            assert_eq!(texture_level_bytes(ew, eh, vkf), lb);
            acc += lb;
        }
        assert_eq!(t.data.len() as u64, acc);
    }

    /// Test cruzado escritor→loader (exigido por la revisión cruzada de
    /// S2): la salida de `write_ktx2` pasa directamente por `parse_ktx2`
    /// y se compara campo a campo, con datos concatenados byte-idénticos.
    /// Cubre los 6 formatos, NPOT 10x6 y una cadena larga NPOT (100x37).
    #[test]
    fn roundtrip_writer_to_loader() {
        use crate::ktx2_writer::{write_ktx2, Ktx2Level};

        let cases: &[(vk::Format, u32, u32)] = &[
            (vk::Format::BC1_RGB_SRGB_BLOCK, 16, 16),
            (vk::Format::BC1_RGBA_SRGB_BLOCK, 16, 16),
            (vk::Format::BC3_SRGB_BLOCK, 16, 16),
            (vk::Format::BC7_SRGB_BLOCK, 16, 16),
            (vk::Format::BC7_UNORM_BLOCK, 16, 16),
            (vk::Format::BC5_UNORM_BLOCK, 16, 16),
            (vk::Format::BC3_SRGB_BLOCK, 10, 6),
            (vk::Format::BC7_SRGB_BLOCK, 100, 37),
        ];
        for &(format, w, h) in cases {
            // Cadena completa de mips con la regla iterativa max(1, prev/2).
            let mut chain: Vec<(u32, u32, Vec<u8>)> = Vec::new();
            let (mut cw, mut ch) = (w, h);
            loop {
                let len = texture_level_bytes(cw, ch, format) as usize;
                let data: Vec<u8> = (0..len)
                    .map(|i| (i as u8).wrapping_add(cw as u8).wrapping_add(ch as u8))
                    .collect();
                chain.push((cw, ch, data));
                if cw == 1 && ch == 1 {
                    break;
                }
                cw = (cw / 2).max(1);
                ch = (ch / 2).max(1);
            }
            let levels: Vec<Ktx2Level> = chain
                .iter()
                .map(|(lw, lh, d)| Ktx2Level {
                    data: d,
                    width: *lw,
                    height: *lh,
                })
                .collect();
            let blob = write_ktx2(format, w, h, &levels).expect("write_ktx2");
            let parsed = parse_ktx2(&blob).expect("parse_ktx2 del blob del escritor");
            assert_eq!(parsed.format, format);
            assert_eq!(parsed.width, w);
            assert_eq!(parsed.height, h);
            assert_eq!(parsed.levels.len(), chain.len());
            let mut acc = 0u64;
            for (i, ((off, pw, ph), (cw, ch, data))) in
                parsed.levels.iter().zip(chain.iter()).enumerate()
            {
                assert_eq!(
                    (*off, *pw, *ph),
                    (acc, *cw, *ch),
                    "nivel {i} de {w}x{h} {format:?}"
                );
                assert_eq!(texture_level_bytes(*cw, *ch, format), data.len() as u64);
                acc += data.len() as u64;
            }
            // Datos concatenados byte-idénticos a la cadena del escritor.
            let expected: Vec<u8> = chain
                .iter()
                .flat_map(|(_, _, d)| d.iter().copied())
                .collect();
            assert_eq!(parsed.data, expected);
        }
    }

    /// Guarda de la revisión cruzada (defecto 2, menor): cada formato que
    /// `map_format` admite debe tener rama explícita en
    /// `texture_level_bytes`; si se añade un séptimo formato BC a
    /// `map_format` sin listarlo allí, caería en silencio al cálculo
    /// RGBA8 (w*h*4). Este test recorre los 6 y exige el tamaño por
    /// bloques (BC1: 8 B/bloque; BC3/BC5/BC7: 16 B/bloque), no el de RGBA8.
    #[test]
    fn map_format_cubierto_por_texture_level_bytes() {
        let casos: [(ktx2::Format, u64); 6] = [
            (ktx2::Format::BC1_RGB_SRGB_BLOCK, 8),
            (ktx2::Format::BC1_RGBA_SRGB_BLOCK, 8),
            (ktx2::Format::BC3_SRGB_BLOCK, 16),
            (ktx2::Format::BC7_SRGB_BLOCK, 16),
            (ktx2::Format::BC7_UNORM_BLOCK, 16),
            (ktx2::Format::BC5_UNORM_BLOCK, 16),
        ];
        for (kfmt, bytes_por_bloque) in casos {
            let vkf = map_format(kfmt).expect("map_format");
            // 4x4 = exactamente 1 bloque; el respaldo RGBA8 daría 64.
            assert_eq!(
                texture_level_bytes(4, 4, vkf),
                bytes_por_bloque,
                "formato {kfmt:?} sin rama explícita en texture_level_bytes"
            );
        }
    }

    // ---------- Test de subida headless (lavapipe) ----------

    fn build_ktx2_4x4_bc1() -> Vec<u8> {
        let mut o = base_opts(134); // BC1_RGBA_SRGB
        o.tw = 4;
        o.th = 4;
        o.nlevels = 1;
        build_ktx2(&o)
    }

    #[test]
    fn subida_ktx2_4x4_bc1_lavapipe() {
        let entry = unsafe { ash::Entry::load() }.expect("cargar Vulkan");
        let app_info = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_3);
        let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);
        let instance = unsafe { entry.create_instance(&create_info, None) }.expect("instancia");

        let fisicos = unsafe { instance.enumerate_physical_devices() }.expect("GPUs");

        let mut elegido: Option<(vk::PhysicalDevice, u32)> = None;
        for pd in &fisicos {
            let feats = unsafe { instance.get_physical_device_features(*pd) };
            if feats.texture_compression_bc != vk::TRUE {
                continue;
            }
            let qfams = unsafe { instance.get_physical_device_queue_family_properties(*pd) };
            if let Some((idx, _)) = qfams
                .iter()
                .enumerate()
                .find(|(_, p)| p.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            {
                elegido = Some((*pd, idx as u32));
                break;
            }
        }
        let (fisico, graphics_family) =
            elegido.expect("ningún dispositivo con GRAPHICS + textureCompressionBC (lavapipe)");

        let queue_priorities = [1.0f32];
        let queue_create_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(graphics_family)
            .queue_priorities(&queue_priorities);
        let device_features = vk::PhysicalDeviceFeatures {
            texture_compression_bc: vk::TRUE,
            ..Default::default()
        };
        let device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(std::slice::from_ref(&queue_create_info))
            .enabled_features(&device_features);
        let device = unsafe { instance.create_device(fisico, &device_create_info, None) }
            .expect("dispositivo");
        let queue = unsafe { device.get_device_queue(graphics_family, 0) };
        let mut allocator = Allocator::new(&gpu_allocator::vulkan::AllocatorCreateDesc {
            instance: instance.clone(),
            device: device.clone(),
            physical_device: fisico,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })
        .expect("allocator");
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(graphics_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&pool_info, None) }
            .expect("command pool");

        crate::mesh::set_texture_bc_enabled(true);

        let blob = build_ktx2_4x4_bc1();
        let mut tex = upload_texture_to_gpu_ktx2(&device, &mut allocator, &blob, queue, command_pool)
            .expect("subida ktx2");

        assert_eq!(tex.width, 4);
        assert_eq!(tex.height, 4);
        assert_eq!(tex.format, vk::Format::BC1_RGBA_SRGB_BLOCK);
        assert_eq!(tex.mip_levels, 1);

        tex.destroy(&device, &mut allocator);

        unsafe {
            device.destroy_command_pool(command_pool, None);
        }
        drop(allocator);
        unsafe {
            device.destroy_device(None);
            instance.destroy_instance(None);
        }
    }
}
