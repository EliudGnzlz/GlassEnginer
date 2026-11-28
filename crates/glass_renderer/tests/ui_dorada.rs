//! Imagen dorada del pase UI V2 (P3c): renderiza una escena fija con el
//! pintor nuevo (color, degradado, sombra, radios, texto) en un offscreen
//! 1280x720 con lavapipe y la compara con la dorada guardada en
//! `tests/fixtures/ui_dorada_1280x720.png`.
//!
//! Tolerancia (diseño V2): media de diferencia absoluta por canal ≤ 2/255
//! y ≤ 1 % de píxeles fuera de tolerancia (un píxel está fuera si algún
//! canal difiere más de 2 niveles). Si no hay dorada, el test escribe la
//! candidata en `target/ui_dorada_candidata.png` y falla indicando que hay
//! que revisarla y promoverla a mano (nunca se auto-acepta).

use ash::vk;
use glass_renderer::offscreen::OffscreenTarget;
use glass_renderer::sdf_atlas::SdfAtlas;
use glass_renderer::ui::{MODO_BORDE, MODO_COLOR, UiBatch, UiRenderer};
use gpu_allocator::vulkan::{AllocationCreateDesc, AllocationScheme, Allocator};
use gpu_allocator::vulkan::AllocatorCreateDesc as AllocatorDesc;
use gpu_allocator::MemoryLocation;
use std::path::PathBuf;
use std::sync::Arc;

const W: u32 = 1280;
const H: u32 = 720;

struct Ctx {
    _entry: ash::Entry,
    instance: ash::Instance,
    device: Arc<ash::Device>,
    // Option para poder soltarlo en Drop ANTES de destruir el dispositivo:
    // el Drop del allocator libera los bloques que quedan con el device,
    // y si el device ya está destruido, lavapipe segfaultea.
    allocator: Option<Allocator>,
    queue: vk::Queue,
    pool: vk::CommandPool,
}

impl Ctx {
    fn nuevo() -> Self {
        // Lavapipe si el entorno no fija otro ICD.
        if std::env::var_os("VK_ICD_FILENAMES").is_none() {
            std::env::set_var(
                "VK_ICD_FILENAMES",
                "/usr/share/vulkan/icd.d/lvp_icd.json",
            );
        }
        let entry = unsafe { ash::Entry::load().expect("cargar loader Vulkan") };
        let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_3);
        let create = vk::InstanceCreateInfo::default().application_info(&app);
        let instance = unsafe { entry.create_instance(&create, None).expect("instancia") };
        let fisicos = unsafe {
            instance
                .enumerate_physical_devices()
                .expect("enumerar GPUs")
        };
        assert!(!fisicos.is_empty(), "sin dispositivos Vulkan (¿lavapipe?)");
        let (fisico, familia) = fisicos
            .iter()
            .find_map(|&f| {
                let fams = unsafe { instance.get_physical_device_queue_family_properties(f) };
                fams.iter()
                    .position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))
                    .map(|i| (f, i as u32))
            })
            .expect("ninguna GPU con cola gráfica");
        let prioridad = [1.0f32];
        let qinfo = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(familia)
            .queue_priorities(&prioridad)];
        let dinfo = vk::DeviceCreateInfo::default().queue_create_infos(&qinfo);
        let device = Arc::new(unsafe {
            instance
                .create_device(fisico, &dinfo, None)
                .expect("dispositivo lógico")
        });
        let queue = unsafe { device.get_device_queue(familia, 0) };
        let allocator = Allocator::new(&AllocatorDesc {
            instance: instance.clone(),
            device: (*device).clone(),
            physical_device: fisico,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })
        .expect("allocator");
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(familia)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let pool = unsafe { device.create_command_pool(&pool_info, None).expect("pool") };
        Self {
            _entry: entry,
            instance,
            device,
            allocator: Some(allocator),
            queue,
            pool,
        }
    }

    /// Ejecuta `f` en un command buffer de un solo uso y espera a que termine.
    fn una_vez(&self, f: impl FnOnce(vk::CommandBuffer)) {
        unsafe {
            let alloc = vk::CommandBufferAllocateInfo::default()
                .command_pool(self.pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1);
            let cmd = self.device.allocate_command_buffers(&alloc).expect("cmd")[0];
            let begin = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device.begin_command_buffer(cmd, &begin).expect("begin");
            f(cmd);
            self.device.end_command_buffer(cmd).expect("end");
            let cmd_buf = [cmd];
            let submit = [vk::SubmitInfo::default().command_buffers(&cmd_buf)];
            let fence = self.device.create_fence(&Default::default(), None).expect("fence");
            self.device
                .queue_submit(self.queue, &submit, fence)
                .expect("submit");
            self.device
                .wait_for_fences(&[fence], true, u64::MAX)
                .expect("wait");
            self.device.destroy_fence(fence, None);
            self.device.free_command_buffers(self.pool, &[cmd]);
        }
    }
}

impl Drop for Ctx {
    fn drop(&mut self) {
        // Primero el allocator (libera sus bloques con el device vivo);
        // después pool, device e instancia.
        drop(self.allocator.take());
        unsafe {
            self.device.destroy_command_pool(self.pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

/// Escena fija: fondo, panel rojo con radios, degradado azul→verde,
/// sombra + panel encima, y "JUGAR" centrado. Semilla fija: no hay azar.
fn lote_fijo(atlas: &mut SdfAtlas) -> UiBatch {
    let mut lote = UiBatch::new();
    // Fondo completo (cubre el UNDEFINED inicial del offscreen).
    lote.push_solid(atlas, [0.0, 0.0, W as f32, H as f32], [0.10, 0.10, 0.12, 1.0]);
    // Panel rojo con esquinas redondeadas.
    lote.push_quad_extendido(
        [80.0, 80.0, 400.0, 240.0],
        [atlas.solid_uv[0], atlas.solid_uv[1], atlas.solid_uv[0], atlas.solid_uv[1]],
        [0.80, 0.10, 0.10, 1.0],
        glass_renderer::ui::MODO_COLOR,
        [16.0, 16.0, 16.0, 16.0],
        None,
        [0.0; 4],
        None,
        0.0,
    );
    // Degradado vertical azul → verde.
    lote.push_quad_extendido(
        [560.0, 80.0, 400.0, 240.0],
        [atlas.solid_uv[0], atlas.solid_uv[1], atlas.solid_uv[0], atlas.solid_uv[1]],
        [1.0, 1.0, 1.0, 1.0],
        glass_renderer::ui::MODO_COLOR,
        [0.0; 4],
        None,
        [0.0; 4],
        Some((90.0, 0.0, 1.0, [0.0, 0.0, 1.0, 1.0], [0.0, 1.0, 0.0, 1.0])),
        0.0,
    );
    // Sombra difuminada + panel blanco encima.
    lote.push_quad_extendido(
        [92.0, 392.0, 400.0, 160.0],
        [atlas.solid_uv[0], atlas.solid_uv[1], atlas.solid_uv[0], atlas.solid_uv[1]],
        [0.0, 0.0, 0.0, 0.5],
        glass_renderer::ui::MODO_SOMBRA,
        [12.0, 12.0, 12.0, 12.0],
        None,
        [0.0; 4],
        None,
        12.0,
    );
    lote.push_quad_extendido(
        [80.0, 380.0, 400.0, 160.0],
        [atlas.solid_uv[0], atlas.solid_uv[1], atlas.solid_uv[0], atlas.solid_uv[1]],
        [0.95, 0.95, 0.95, 1.0],
        glass_renderer::ui::MODO_COLOR,
        [12.0, 12.0, 12.0, 12.0],
        None,
        [0.0; 4],
        None,
        0.0,
    );
    // Texto centrado en el panel blanco.
    lote.push_text_centered(&mut *atlas, [80.0, 380.0, 400.0, 160.0], "JUGAR", 48.0, [0.05, 0.05, 0.05, 1.0]);
    lote
}

#[test]
fn ui_dorada_1280x720() {
    let mut ctx = Ctx::nuevo();
    let mut ui = UiRenderer::new(
        ctx.device.clone(),
        ctx.allocator.as_mut().expect("allocator"),
        ctx.queue,
        ctx.pool,
    )
    .expect("UiRenderer");
    let target = OffscreenTarget::new(ctx.device.clone(), ctx.allocator.as_mut().expect("allocator"), W, H)
        .expect("offscreen 1280x720");

    // 1. Limpiar el color a negro opaco y dejarlo en SHADER_READ_ONLY
    // (el pase UI es LOAD y exige ese layout inicial).
    ctx.una_vez(|cmd| unsafe {
        let rango = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let a_dst = [vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .image(target.color_image)
            .subresource_range(rango)];
        ctx.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &a_dst,
        );
        let color = vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] };
        ctx.device.cmd_clear_color_image(
            cmd,
            target.color_image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &color,
            &[rango],
        );
        let a_ro = [vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .image(target.color_image)
            .subresource_range(rango)];
        ctx.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &a_ro,
        );
    });

    // 2. Pintar la escena fija con el pase UI.
    let lote = lote_fijo(ui.font_mut());
    assert!(lote.vertex_count() > 0, "el lote fijo no puede estar vacío");
    ctx.una_vez(|cmd| {
        ui.record(cmd, &target, &lote);
    });

    // 3. Leer de vuelta los píxeles a un buffer de host.
    let nbytes = (W * H * 4) as u64;
    let (buf, alloc) = {
        let info = vk::BufferCreateInfo::default()
            .size(nbytes)
            .usage(vk::BufferUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let b = unsafe { ctx.device.create_buffer(&info, None).expect("staging") };
        let req = unsafe { ctx.device.get_buffer_memory_requirements(b) };
        let a = ctx
            .allocator
            .as_mut()
            .expect("allocator")
            .allocate(&AllocationCreateDesc {
                name: "ui_dorada_staging",
                requirements: req,
                location: MemoryLocation::GpuToCpu,
                linear: true,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .expect("staging alloc");
        unsafe {
            ctx.device
                .bind_buffer_memory(b, a.memory(), a.offset())
                .expect("bind staging");
        }
        (b, a)
    };
    ctx.una_vez(|cmd| unsafe {
        let rango = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let a_src = [vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_access_mask(vk::AccessFlags::SHADER_READ)
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
            .image(target.color_image)
            .subresource_range(rango)];
        ctx.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &a_src,
        );
        let region = [vk::BufferImageCopy::default()
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(vk::Extent3D { width: W, height: H, depth: 1 })];
        ctx.device.cmd_copy_image_to_buffer(
            cmd,
            target.color_image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            buf,
            &region,
        );
    });
    let mut pixeles = vec![0u8; nbytes as usize];
    if let Some(ptr) = alloc.mapped_ptr() {
        unsafe {
            std::ptr::copy_nonoverlapping(ptr.as_ptr() as *const u8, pixeles.as_mut_ptr(), pixeles.len());
        }
    }
    ctx.allocator.as_mut().expect("allocator").free(alloc).unwrap_or(());
    unsafe { ctx.device.destroy_buffer(buf, None) };

    let candidato = image::RgbaImage::from_raw(W, H, pixeles).expect("pixeles RGBA");
    let raiz = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dorada_path = raiz.join("tests/fixtures/ui_dorada_1280x720.png");
    if !dorada_path.exists() {
        let fuera = raiz.join("../../target/ui_dorada_candidata.png");
        candidato.save(&fuera).expect("guardar candidata");
        panic!(
            "no hay dorada guardada; candidata escrita en {} — revísala y promuévela a {}",
            fuera.display(),
            dorada_path.display()
        );
    }
    let dorada = image::open(&dorada_path)
        .expect("leer dorada")
        .to_rgba8();
    assert_eq!(dorada.dimensions(), (W, H), "la dorada debe ser 1280x720");

    // 4. Comparar: media por canal ≤ 2 y ≤ 1 % de píxeles fuera (>2 en algún canal).
    let mut suma: [u64; 4] = [0; 4];
    let mut fuera = 0u64;
    let total = (W * H) as u64;
    for (a, b) in candidato.pixels().zip(dorada.pixels()) {
        let mut max_d = 0u8;
        for c in 0..4 {
            let d = (a[c] as i32 - b[c] as i32).unsigned_abs() as u8;
            suma[c] += d as u64;
            max_d = max_d.max(d);
        }
        if max_d > 2 {
            fuera += 1;
        }
    }
    let media = suma.map(|s| s as f64 / total as f64);
    let pct_fuera = fuera as f64 / total as f64;
    assert!(
        media.iter().all(|&m| m <= 2.0),
        "media por canal {media:?} supera 2/255"
    );
    assert!(
        pct_fuera <= 0.01,
        "píxeles fuera de tolerancia: {:.2} % (máx 1 %)",
        pct_fuera * 100.0
    );
}


/// Diagnóstico P4c: el texto por el camino EXTENDIDO (V2) debe pintar
/// glifos, no bloques sólidos. Pinta "JUGAR" blanco sobre negro con
/// push_text_extendido y mide la cobertura blanca dentro de su caja:
/// glifos reales cubren ~15-30 %; un bloque sólido cubre ~100 %.
#[test]
fn texto_extendido_pinta_glifos() {
    let mut ctx = Ctx::nuevo();
    let mut ui = UiRenderer::new(
        ctx.device.clone(),
        ctx.allocator.as_mut().expect("allocator"),
        ctx.queue,
        ctx.pool,
    )
    .expect("UiRenderer");
    let target = OffscreenTarget::new(ctx.device.clone(), ctx.allocator.as_mut().expect("allocator"), W, H)
        .expect("offscreen 1280x720");
    // Limpiar a negro opaco (mismo patrón que la dorada).
    ctx.una_vez(|cmd| unsafe {
        let rango = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let a_dst = [vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .image(target.color_image)
            .subresource_range(rango)];
        ctx.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &a_dst,
        );
        let color = vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] };
        ctx.device.cmd_clear_color_image(
            cmd,
            target.color_image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &color,
            &[rango],
        );
        let a_ro = [vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .image(target.color_image)
            .subresource_range(rango)];
        ctx.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &a_ro,
        );
    });
    let mut lote = UiBatch::new();
    let (w_txt, ox, oy, alto_linea) = {
        let atlas = ui.font_mut();
        let w_txt = atlas.measure("JUGAR", 48.0);
        let alto_linea = atlas.line_height_at(48.0);
        let ox = 80.0 + (400.0 - w_txt) * 0.5;
        let oy = 380.0 + (160.0 - alto_linea) * 0.5;
        lote.push_text_extendido(atlas, [ox, oy], "JUGAR", 48.0, [1.0, 1.0, 1.0, 1.0], [0.0; 4], None, [0.0; 4], 0.0);
        (w_txt, ox, oy, alto_linea)
    };
    ctx.una_vez(|cmd| {
        ui.record(cmd, &target, &lote);
    });
    // Leer de vuelta (mismo patrón que la dorada).
    let nbytes = (W * H * 4) as u64;
    let (buf, alloc) = {
        let info = vk::BufferCreateInfo::default()
            .size(nbytes)
            .usage(vk::BufferUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let b = unsafe { ctx.device.create_buffer(&info, None).expect("staging") };
        let req = unsafe { ctx.device.get_buffer_memory_requirements(b) };
        let a = ctx
            .allocator
            .as_mut()
            .expect("allocator")
            .allocate(&AllocationCreateDesc {
                name: "texto_extendido_staging",
                requirements: req,
                location: MemoryLocation::GpuToCpu,
                linear: true,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .expect("staging alloc");
        unsafe {
            ctx.device
                .bind_buffer_memory(b, a.memory(), a.offset())
                .expect("bind staging");
        }
        (b, a)
    };
    ctx.una_vez(|cmd| unsafe {
        let rango = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let a_src = [vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_access_mask(vk::AccessFlags::SHADER_READ)
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
            .image(target.color_image)
            .subresource_range(rango)];
        ctx.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &a_src,
        );
        let region = [vk::BufferImageCopy::default()
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(vk::Extent3D { width: W, height: H, depth: 1 })];
        ctx.device.cmd_copy_image_to_buffer(
            cmd,
            target.color_image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            buf,
            &region,
        );
    });
    let mut pixeles = vec![0u8; nbytes as usize];
    if let Some(ptr) = alloc.mapped_ptr() {
        unsafe {
            std::ptr::copy_nonoverlapping(ptr.as_ptr() as *const u8, pixeles.as_mut_ptr(), pixeles.len());
        }
    }
    ctx.allocator.as_mut().expect("allocator").free(alloc).unwrap_or(());
    unsafe { ctx.device.destroy_buffer(buf, None) };
    let img = image::RgbaImage::from_raw(W, H, pixeles).expect("pixeles RGBA");
    let fuera = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/texto_extendido.png");
    img.save(&fuera).expect("guardar texto extendido");
    // Cobertura blanca dentro de la caja del texto.
    let x0 = ox as u32;
    let y0 = oy as u32;
    let x1 = (ox + w_txt) as u32;
    let y1 = (oy + alto_linea) as u32;
    let mut blancos = 0u64;
    let mut total = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            let p = img.get_pixel(x, y);
            total += 1;
            if p[0] > 200 && p[1] > 200 && p[2] > 200 {
                blancos += 1;
            }
        }
    }
    let cobertura = blancos as f64 / total as f64;
    eprintln!("cobertura blanca en la caja del texto: {cobertura:.3} ({blancos}/{total})");
    assert!(cobertura < 0.35, "el texto extendido pinta bloques sólidos: cobertura {cobertura:.3}");
    assert!(cobertura > 0.05, "el texto extendido no pinta nada: cobertura {cobertura:.3}");
}

/// Diagnóstico P4c (2): compara vértice a vértice el texto clásico y
/// el extendido para encontrar el campo que difiere.
#[test]
fn texto_clasico_vs_extendido_vertices() {
    let mut atlas = SdfAtlas::build(include_bytes!("../../../assets/fonts/DejaVuSans.ttf")).expect("atlas");
    let mut a = UiBatch::new();
    a.push_text_centered(&mut atlas, [80.0, 380.0, 400.0, 160.0], "JUGAR", 48.0, [1.0, 1.0, 1.0, 1.0]);
    let mut b = UiBatch::new();
    let w = atlas.measure("JUGAR", 48.0);
    let ox = 80.0 + (400.0 - w) * 0.5;
    let oy = 380.0 + (160.0 - atlas.line_height_at(48.0)) * 0.5;
    b.push_text_extendido(&mut atlas, [ox, oy], "JUGAR", 48.0, [1.0, 1.0, 1.0, 1.0], [0.0; 4], None, [0.0; 4], 0.0);
    let va = a.vertices();
    let vb = b.vertices();
    eprintln!("vértices clásico: {}, extendido: {}", va.len(), vb.len());
    assert_eq!(va.len(), vb.len());
    for (i, (x, y)) in va.iter().zip(vb.iter()).enumerate() {
        if x.pos != y.pos || x.uv != y.uv || x.color != y.color || x.local != y.local
            || x.tamano != y.tamano || x.radios != y.radios || x.clip != y.clip
            || x.pack1 != y.pack1 || x.pack2 != y.pack2 || x.grad_c0 != y.grad_c0 || x.grad_c1 != y.grad_c1
        {
            eprintln!("vértice {i} difiere:\n  clásico: pos={:?} uv={:?} color={:?} local={:?} tam={:?} radios={:?} clip={:?} p1={:?} p2={:?}\n  extendido: pos={:?} uv={:?} color={:?} local={:?} tam={:?} radios={:?} clip={:?} p1={:?} p2={:?}",
                x.pos, x.uv, x.color, x.local, x.tamano, x.radios, x.clip, x.pack1, x.pack2,
                y.pos, y.uv, y.color, y.local, y.tamano, y.radios, y.clip, y.pack1, y.pack2);
            panic!("vértice {i} difiere");
        }
    }
    eprintln!("vértices idénticos");
}

/// Regresión M3 (V5c): el hijo rojo [200,160,600,100] con clip del panel
/// [-100,-60,600,400] + radios 60 NO debe pintar fuera del clip.
/// Cubre el fallo premultiplicado (pasos 4/5/6 del shader deben escalar
/// `base` entero, no solo `base.a`): si el clip solo escalara la alfa,
/// (750,200) saldría rosa en vez de negro.
#[test]
fn clip_redondeado_recorta_en_gpu() {
    let mut ctx = Ctx::nuevo();
    let mut ui = UiRenderer::new(
        ctx.device.clone(),
        ctx.allocator.as_mut().expect("allocator"),
        ctx.queue,
        ctx.pool,
    )
    .expect("UiRenderer");
    let target = OffscreenTarget::new(ctx.device.clone(), ctx.allocator.as_mut().expect("allocator"), W, H)
        .expect("offscreen 1280x720");
    limpiar_negro(&ctx, &target);
    let (u, v) = {
        let atlas = ui.font();
        (atlas.solid_uv[0], atlas.solid_uv[1])
    };
    let mut lote = UiBatch::new();
    // Panel blanco (sin clip).
    lote.push_quad_extendido(
        [100.0, 100.0, 600.0, 400.0],
        [u, v, u, v],
        [1.0, 1.0, 1.0, 1.0],
        MODO_COLOR,
        [60.0; 4],
        None,
        [0.0; 4],
        None,
        0.0,
    );
    // Hijo rojo con el clip del panel en coords locales + radios 60.
    lote.push_quad_extendido(
        [200.0, 160.0, 600.0, 100.0],
        [u, v, u, v],
        [0.878, 0.125, 0.125, 1.0],
        MODO_COLOR,
        [0.0; 4],
        Some(([-100.0, -60.0, 600.0, 400.0], 1.0)),
        [60.0; 4],
        None,
        0.0,
    );
    ctx.una_vez(|cmd| {
        ui.record(cmd, &target, &lote);
    });
    let img = leer_imagen(&mut ctx, &target);
    let dentro = img.get_pixel(300, 200);
    let fuera = img.get_pixel(750, 200);
    let esquina = img.get_pixel(110, 105);
    assert!(dentro[0] > 200 && dentro[1] < 60, "dentro debe ser rojo, dentro={dentro:?}");
    assert!(fuera[0] < 10 && fuera[1] < 10 && fuera[2] < 10, "fuera del clip debe ser negro, fuera={fuera:?}");
    assert!(esquina[0] < 10 && esquina[1] < 10 && esquina[2] < 10, "la esquina propia del panel (radios 60) también recorta, esquina={esquina:?}");
}

/// Limpia el offscreen a negro opaco (mismo patrón que la dorada).
fn limpiar_negro(ctx: &Ctx, target: &OffscreenTarget) {
    ctx.una_vez(|cmd| unsafe {
        let rango = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let a_dst = [vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .image(target.color_image)
            .subresource_range(rango)];
        ctx.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &a_dst,
        );
        let color = vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] };
        ctx.device.cmd_clear_color_image(
            cmd,
            target.color_image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &color,
            &[rango],
        );
        let a_ro = [vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .image(target.color_image)
            .subresource_range(rango)];
        ctx.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &a_ro,
        );
    });
}

/// Lee el offscreen a una imagen RGBA (staging + copia + mapeo).
fn leer_imagen(ctx: &mut Ctx, target: &OffscreenTarget) -> image::RgbaImage {
    let nbytes = (W * H * 4) as u64;
    let (buf, alloc) = {
        let info = vk::BufferCreateInfo::default()
            .size(nbytes)
            .usage(vk::BufferUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let b = unsafe { ctx.device.create_buffer(&info, None).expect("staging") };
        let req = unsafe { ctx.device.get_buffer_memory_requirements(b) };
        let a = ctx
            .allocator
            .as_mut()
            .expect("allocator")
            .allocate(&AllocationCreateDesc {
                name: "ui_dorada_staging",
                requirements: req,
                location: MemoryLocation::GpuToCpu,
                linear: true,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .expect("staging alloc");
        unsafe {
            ctx.device
                .bind_buffer_memory(b, a.memory(), a.offset())
                .expect("bind staging");
        }
        (b, a)
    };
    ctx.una_vez(|cmd| unsafe {
        let rango = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let a_src = [vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_access_mask(vk::AccessFlags::SHADER_READ)
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
            .image(target.color_image)
            .subresource_range(rango)];
        ctx.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &a_src,
        );
        let region = [vk::BufferImageCopy::default()
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(vk::Extent3D { width: W, height: H, depth: 1 })];
        ctx.device.cmd_copy_image_to_buffer(
            cmd,
            target.color_image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            buf,
            &region,
        );
    });
    let mut pixeles = vec![0u8; nbytes as usize];
    if let Some(ptr) = alloc.mapped_ptr() {
        unsafe {
            std::ptr::copy_nonoverlapping(ptr.as_ptr() as *const u8, pixeles.as_mut_ptr(), pixeles.len());
        }
    }
    ctx.allocator.as_mut().expect("allocator").free(alloc).unwrap_or(());
    unsafe { ctx.device.destroy_buffer(buf, None) };
    image::RgbaImage::from_raw(W, H, pixeles).expect("pixeles RGBA")
}

/// V3b (G-SDF1, borde): el texto SDF debe tener un borde de ≤ 1,5 px
/// a 12 y a 96 px. Mide la transición 10 %→90 % de cobertura en una
/// fila que cruza el asta vertical de la 'H' (texto blanco sobre negro).
/// El anti-alias por `fwidth()` mantiene el borde en ~1 px de pantalla
/// a cualquier tamaño; sin él, a 12 px el borde saldría de ~5 px.
#[test]
fn ancho_borde_sdf_12_y_96() {
    for size in [12.0f32, 96.0] {
        let mut ctx = Ctx::nuevo();
        let mut ui = UiRenderer::new(
            ctx.device.clone(),
            ctx.allocator.as_mut().expect("allocator"),
            ctx.queue,
            ctx.pool,
        )
        .expect("UiRenderer");
        let target = OffscreenTarget::new(ctx.device.clone(), ctx.allocator.as_mut().expect("allocator"), W, H)
            .expect("offscreen 1280x720");
        limpiar_negro(&ctx, &target);
        // Fila a 1/4 de la celda de la 'H' (asta vertical, por encima
        // del travesaño): misma geometría que push_text_extendido.
        let (ox, oy, y_fila) = {
            let atlas = ui.font();
            let k = size / 64.0;
            let g = atlas.glifo('H');
            let oy = 200.0;
            let baseline = oy + atlas.ascent * k;
            let y_fila = baseline + (g.bearing[1] + g.size[1] * 0.25) * k;
            (80.0, oy, y_fila as u32)
        };
        let mut lote = UiBatch::new();
        lote.push_text_extendido(
            ui.font_mut(),
            [ox, oy],
            "H",
            size,
            [1.0, 1.0, 1.0, 1.0],
            [0.0; 4],
            None,
            [0.0; 4],
            0.0,
        );
        ctx.una_vez(|cmd| {
            ui.record(cmd, &target, &lote);
        });
        let img = leer_imagen(&mut ctx, &target);
        let mut x01: Option<f32> = None;
        let mut x09: Option<f32> = None;
        for x in 0..W {
            let cov = img.get_pixel(x, y_fila)[0] as f32 / 255.0;
            if x01.is_none() && cov > 0.1 {
                x01 = Some(x as f32);
            }
            if x09.is_none() && cov > 0.9 {
                x09 = Some(x as f32);
            }
            if x01.is_some() && x09.is_some() {
                break;
            }
        }
        let ancho = match (x01, x09) {
            (Some(a), Some(b)) => b - a,
            _ => f32::INFINITY,
        };
        eprintln!("tamaño {size}: ancho de borde = {ancho:.2} px (fila y={y_fila})");
        assert!(ancho <= 1.5, "borde de {ancho:.2} px a {size} px (> 1,5)");
    }
}

/// V4/Q3: el borde solo pinta su banda (4 px hacia dentro del rect),
/// el interior queda transparente (fondo negro del offscreen).
#[test]
fn borde_solo_la_banda() {
    let mut ctx = Ctx::nuevo();
    let mut ui = UiRenderer::new(
        ctx.device.clone(),
        ctx.allocator.as_mut().expect("allocator"),
        ctx.queue,
        ctx.pool,
    )
    .expect("UiRenderer");
    let target = OffscreenTarget::new(ctx.device.clone(), ctx.allocator.as_mut().expect("allocator"), W, H)
        .expect("offscreen 1280x720");
    limpiar_negro(&ctx, &target);
    let (u, v) = {
        let atlas = ui.font();
        (atlas.solid_uv[0], atlas.solid_uv[1])
    };
    let mut lote = UiBatch::new();
    lote.push_quad_extendido(
        [400.0, 300.0, 200.0, 100.0],
        [u, v, u, v],
        [0.0123, 0.679, 0.117, 1.0],
        MODO_BORDE,
        [12.0; 4],
        None,
        [0.0; 4],
        None,
        4.0,
    );
    ctx.una_vez(|cmd| {
        ui.record(cmd, &target, &lote);
    });
    let img = leer_imagen(&mut ctx, &target);
    let centro = img.get_pixel(500, 350);
    eprintln!("centro (500,350): {:?}", centro);
    assert!(centro[0] < 10 && centro[1] < 10 && centro[2] < 10, "el interior del borde debe quedar transparente, centro={centro:?}");
    let borde_sup = img.get_pixel(500, 302);
    eprintln!("borde sup (500,302): {:?}", borde_sup);
    assert!(borde_sup[1] > 150, "la banda superior debe ser verde, pixel={borde_sup:?}");
    let borde_izq = img.get_pixel(402, 350);
    eprintln!("borde izq (402,350): {:?}", borde_izq);
    assert!(borde_izq[1] > 150, "la banda izquierda debe ser verde, pixel={borde_izq:?}");
}
