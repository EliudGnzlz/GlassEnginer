//! Mapa de sombras del sol y UBO de frame (bloque de gráficos: sombras
//! del sol + post-procesado). Registro: REGISTRO_GRAFICOS_SOMBRAS_POST.md.
//!
//! Diseño, en breve:
//! - UNA luz direccional (la principal de la escena) proyecta sobre un
//!   mapa D32 de 2048² con sampler de COMPARACIÓN (PCF por hardware en
//!   cada tap; el PBR hace 3×3 taps). La caja ortográfica (50 m de lado)
//!   sigue a la cámara con snap a téxeles — la matemática vive en
//!   `glass_core::directional_shadow_vp` y está testeada sin GPU.
//! - El pase de profundidad reutiliza los vertex shaders PBR (y el de
//!   piel para las entidades animadas, que proyectan su pose real, no
//!   la de unión) con pipelines sin fragmento ni color.
//! - La VP y los sesgos viajan en un UBO de frame en el SET 2, que solo
//!   declaran el PBR y el pipeline con piel. Los grafos declaran en su
//!   lugar un set 2 de RELLENO (un UBO vacío): su layout necesita la
//!   misma FORMA de layout que el del PBR para que enlazar el set 2 no
//!   invalide los sets 0/1 al alternar pipelines en la misma pasada.
//! - El UBO se escribe en CPU cada frame dentro de `render_views`, tras
//!   la espera del fence (un frame en vuelo, como los JointBuffer).

use ash::vk;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator};
use gpu_allocator::MemoryLocation;
use std::sync::Arc;

use crate::offscreen::OffscreenError;
use crate::pipeline::{GraphicsPipeline, PipelineError};

/// Lado del mapa de sombras. Fijo en esta entrega: 2048 da ~41 téxeles
/// por metro en la caja de 50 m, y en lavapipe el pase de profundidad
/// sigue siendo barato (sin fragmento).
pub const SHADOW_MAP_SIZE: u32 = 2048;
/// Semiextensión de la caja ortográfica: 50 m de lado centrados en la
/// cámara. Las cámaras de Plat3D miran desde ~10-14 m de altura con el
/// encuadre típico de plataformas; 25 m de radio cubre lo visible con
/// margen sin bajar de ~40 téxeles/m.
pub const SHADOW_ORTHO_HALF: f32 = 25.0;
/// Distancia del ojo de la luz al centro de la caja, hacia el sol.
pub const SHADOW_BACK: f32 = 60.0;
/// Fondo de la caja desde el centro, en contra del sol.
pub const SHADOW_FRONT: f32 = 60.0;
/// Sesgo de profundidad (en profundidad normalizada [0,1]) que el
/// fragmento resta a la referencia antes de comparar.
pub const SHADOW_DEPTH_BIAS: f32 = 0.0015;
/// Sesgo de normal (metros de mundo): la posición de muestreo se
/// desplaza a lo largo de la normal antes de proyectar.
pub const SHADOW_NORMAL_BIAS: f32 = 0.05;

/// Contenido del UBO de frame del set 2: la VP de la luz y
/// `params = (1/tamaño, bias de profundidad, bias de normal, activo)`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShadowUbo {
    pub light_vp: [f32; 16],
    pub params: [f32; 4],
    /// Niebla (Fase 2): rgb = color, w = distancia de inicio.
    pub fog_color_dist: [f32; 4],
    /// Niebla: x = distancia total, y = 1 si activa.
    pub fog_far_mode: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<ShadowUbo>() == 112);

fn pipe_err(e: PipelineError) -> OffscreenError {
    match e {
        PipelineError::Driver(r) => OffscreenError::Driver(r),
        PipelineError::ShaderLoadError => OffscreenError::Driver(vk::Result::ERROR_INITIALIZATION_FAILED),
    }
}

/// Buffer CpuToGpu mapeado de forma persistente (el patrón de los
/// JointBuffer y del UBO global de los grafos).
fn mapped_buffer(
    device: &ash::Device,
    allocator: &mut Allocator,
    size: u64,
    name: &str,
) -> Result<(vk::Buffer, Allocation, *mut u8), OffscreenError> {
    let buffer_info = vk::BufferCreateInfo::default()
        .size(size)
        .usage(vk::BufferUsageFlags::UNIFORM_BUFFER)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let buffer = unsafe { device.create_buffer(&buffer_info, None)? };
    let mem_reqs = unsafe { device.get_buffer_memory_requirements(buffer) };
    let allocation = allocator.allocate(&AllocationCreateDesc {
        name,
        requirements: mem_reqs,
        location: MemoryLocation::CpuToGpu,
        linear: true,
        allocation_scheme: AllocationScheme::GpuAllocatorManaged,
    })?;
    unsafe { device.bind_buffer_memory(buffer, allocation.memory(), allocation.offset())? };
    let mapped = allocation
        .mapped_ptr()
        .map(|p| p.as_ptr() as *mut u8)
        .ok_or(OffscreenError::Driver(vk::Result::ERROR_INITIALIZATION_FAILED))?;
    Ok((buffer, allocation, mapped))
}

pub struct ShadowResources {
    pub size: u32,
    image: vk::Image,
    allocation: Option<Allocation>,
    view: vk::ImageView,
    sampler: vk::Sampler,
    pub render_pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    /// Layout del set 2 real (binding 0 = UBO, binding 1 = sampler de
    /// comparación). Lo referencia el layout de los pipelines PBR/piel.
    pub set_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    pub set: vk::DescriptorSet,
    ubo_buffer: vk::Buffer,
    ubo_allocation: Option<Allocation>,
    ubo_mapped: *mut u8,
    pub depth_pipeline: vk::Pipeline,
    pub depth_layout: vk::PipelineLayout,
    pub depth_skinned_pipeline: vk::Pipeline,
    pub depth_skinned_layout: vk::PipelineLayout,
    /// Set 2 de relleno para los pipelines de grafo: mismo número de
    /// set, un solo UBO vacío que el shader generado ni declara.
    pub dummy_layout: vk::DescriptorSetLayout,
    dummy_pool: vk::DescriptorPool,
    pub dummy_set: vk::DescriptorSet,
    dummy_buffer: vk::Buffer,
    dummy_allocation: Option<Allocation>,
}

impl ShadowResources {
    /// Layout del set 2 real. Se crea ANTES que los pipelines PBR para
    /// que puedan referenciarlo en sus layouts.
    pub fn create_set_layout(device: &ash::Device) -> Result<vk::DescriptorSetLayout, OffscreenError> {
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        ];
        let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
        Ok(unsafe { device.create_descriptor_set_layout(&info, None)? })
    }

    /// Layout del set 2 de relleno (grafos): un UBO en el binding 0.
    pub fn create_dummy_layout(device: &ash::Device) -> Result<vk::DescriptorSetLayout, OffscreenError> {
        let bindings = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
        Ok(unsafe { device.create_descriptor_set_layout(&info, None)? })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: Arc<ash::Device>,
        allocator: &mut Allocator,
        set_layout: vk::DescriptorSetLayout,
        dummy_layout: vk::DescriptorSetLayout,
        joints_set_layout: vk::DescriptorSetLayout,
        filler_set_layout: vk::DescriptorSetLayout,
        pbr_vert_spv: &[u8],
        skinned_vert_spv: &[u8],
    ) -> Result<Self, OffscreenError> {
        let size = SHADOW_MAP_SIZE;
        unsafe {
            // --- Imagen de profundidad del mapa ---
            let image_info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::D32_SFLOAT)
                .extent(vk::Extent3D { width: size, height: size, depth: 1 })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .initial_layout(vk::ImageLayout::UNDEFINED);
            let image = device.create_image(&image_info, None)?;
            let mem_reqs = device.get_image_memory_requirements(image);
            let allocation = allocator.allocate(&AllocationCreateDesc {
                name: "shadow_map",
                requirements: mem_reqs,
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })?;
            device.bind_image_memory(image, allocation.memory(), allocation.offset())?;
            let view_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(vk::Format::D32_SFLOAT)
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::DEPTH)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(1),
                );
            let view = device.create_image_view(&view_info, None)?;

            // Sampler de comparación: un tap devuelve el factor de luz
            // (1 si la referencia no supera la profundidad guardada).
            let sampler_info = vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .compare_enable(true)
                .compare_op(vk::CompareOp::LESS_OR_EQUAL)
                .mipmap_mode(vk::SamplerMipmapMode::NEAREST);
            let sampler = device.create_sampler(&sampler_info, None)?;

            // --- Render pass de solo profundidad ---
            let attachment = [vk::AttachmentDescription::default()
                .format(vk::Format::D32_SFLOAT)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL)];
            let depth_ref = vk::AttachmentReference::default()
                .attachment(0)
                .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
            let subpass = [vk::SubpassDescription::default()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .depth_stencil_attachment(&depth_ref)];
            let dependencies = [
                // Lo muestreado el frame anterior ya se leyó antes de
                // volver a escribir el mapa.
                vk::SubpassDependency::default()
                    .src_subpass(vk::SUBPASS_EXTERNAL)
                    .dst_subpass(0)
                    .src_stage_mask(vk::PipelineStageFlags::FRAGMENT_SHADER)
                    .src_access_mask(vk::AccessFlags::SHADER_READ)
                    .dst_stage_mask(vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS)
                    .dst_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE),
                // El mapa queda legible como textura al salir del pase.
                vk::SubpassDependency::default()
                    .src_subpass(0)
                    .dst_subpass(vk::SUBPASS_EXTERNAL)
                    .src_stage_mask(vk::PipelineStageFlags::LATE_FRAGMENT_TESTS)
                    .src_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags::FRAGMENT_SHADER)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ),
            ];
            let pass_info = vk::RenderPassCreateInfo::default()
                .attachments(&attachment)
                .subpasses(&subpass)
                .dependencies(&dependencies);
            let render_pass = device.create_render_pass(&pass_info, None)?;
            let fb_attachments = [view];
            let fb_info = vk::FramebufferCreateInfo::default()
                .render_pass(render_pass)
                .attachments(&fb_attachments)
                .width(size)
                .height(size)
                .layers(1);
            let framebuffer = device.create_framebuffer(&fb_info, None)?;

            // --- UBO de frame + set real ---
            let (ubo_buffer, ubo_allocation, ubo_mapped) = mapped_buffer(
                &device,
                allocator,
                std::mem::size_of::<ShadowUbo>() as u64,
                "shadow_ubo",
            )?;
            let pool_sizes = [
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(1),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(2),
            ];
            let pool_info = vk::DescriptorPoolCreateInfo::default()
                .pool_sizes(&pool_sizes)
                .max_sets(1);
            let pool = device.create_descriptor_pool(&pool_info, None)?;
            let set_layouts = [set_layout];
            let alloc_info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(pool)
                .set_layouts(&set_layouts);
            let set = device
                .allocate_descriptor_sets(&alloc_info)?
                .into_iter()
                .next()
                .ok_or(OffscreenError::Driver(vk::Result::ERROR_INITIALIZATION_FAILED))?;
            let buffer_desc = [vk::DescriptorBufferInfo::default()
                .buffer(ubo_buffer)
                .offset(0)
                .range(std::mem::size_of::<ShadowUbo>() as u64)];
            let image_desc = [vk::DescriptorImageInfo::default()
                .sampler(sampler)
                .image_view(view)
                .image_layout(vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL)];
            let writes = [
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(&buffer_desc),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(&image_desc),
            ];
            device.update_descriptor_sets(&writes, &[]);
            // Estado inicial: sombras inactivas hasta que la app fije la
            // primera VP (params.w = 0 → el shader devuelve 1,0).
            let initial = ShadowUbo {
                light_vp: [0.0; 16],
                params: [1.0 / size as f32, SHADOW_DEPTH_BIAS, SHADOW_NORMAL_BIAS, 0.0],
                fog_color_dist: [0.0; 4],
                fog_far_mode: [0.0; 4],
            };
            std::ptr::copy_nonoverlapping(
                bytemuck::bytes_of(&initial).as_ptr(),
                ubo_mapped,
                std::mem::size_of::<ShadowUbo>(),
            );

            // --- Set de relleno para los grafos ---
            let (dummy_buffer, dummy_allocation, _dummy_mapped) =
                mapped_buffer(&device, allocator, 16, "shadow_dummy")?;
            let dummy_pool_sizes = [vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)];
            let dummy_pool_info = vk::DescriptorPoolCreateInfo::default()
                .pool_sizes(&dummy_pool_sizes)
                .max_sets(1);
            let dummy_pool = device.create_descriptor_pool(&dummy_pool_info, None)?;
            let dummy_layouts = [dummy_layout];
            let dummy_alloc_info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(dummy_pool)
                .set_layouts(&dummy_layouts);
            let dummy_set = device
                .allocate_descriptor_sets(&dummy_alloc_info)?
                .into_iter()
                .next()
                .ok_or(OffscreenError::Driver(vk::Result::ERROR_INITIALIZATION_FAILED))?;
            let dummy_desc = [vk::DescriptorBufferInfo::default()
                .buffer(dummy_buffer)
                .offset(0)
                .range(16)];
            let dummy_write = [vk::WriteDescriptorSet::default()
                .dst_set(dummy_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&dummy_desc)];
            device.update_descriptor_sets(&dummy_write, &[]);

            // --- Pipelines de profundidad ---
            let (depth_pipeline, depth_layout) = GraphicsPipeline::new_depth_pipeline(
                device.clone(),
                render_pass,
                pbr_vert_spv,
                false,
                None,
                None,
            )
            .map_err(pipe_err)?;
            let (depth_skinned_pipeline, depth_skinned_layout) =
                GraphicsPipeline::new_depth_pipeline(
                    device.clone(),
                    render_pass,
                    skinned_vert_spv,
                    true,
                    Some(filler_set_layout),
                    Some(joints_set_layout),
                )
                .map_err(pipe_err)?;

            Ok(Self {
                size,
                image,
                allocation: Some(allocation),
                view,
                sampler,
                render_pass,
                framebuffer,
                set_layout,
                pool,
                set,
                ubo_buffer,
                ubo_allocation: Some(ubo_allocation),
                ubo_mapped,
                depth_pipeline,
                depth_layout,
                depth_skinned_pipeline,
                depth_skinned_layout,
                dummy_layout,
                dummy_pool,
                dummy_set,
                dummy_buffer,
                dummy_allocation: Some(dummy_allocation),
            })
        }
    }

    /// Escribe el UBO del frame. Solo es seguro con la GPU en reposo
    /// (tras la espera del fence de `render_views`, como los globales
    /// de grafo).
    pub fn write_ubo(&self, ubo: &ShadowUbo) {
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytemuck::bytes_of(ubo).as_ptr(),
                self.ubo_mapped,
                std::mem::size_of::<ShadowUbo>(),
            );
        }
    }

    /// Escribe el binding 2 del set real con el atlas de cubos de las
    /// luces puntuales (Fase 2). Lo llama el renderer justo tras crear
    /// ambos recursos, antes del primer frame: ningún shader debe
    /// ejecutarse con el binding sin escribir.
    pub fn write_point_shadow_set(
        &self,
        device: &ash::Device,
        cube_view: vk::ImageView,
        sampler: vk::Sampler,
    ) {
        let image_info = [vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL)
            .image_view(cube_view)
            .sampler(sampler)];
        let writes = [vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(2)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_info)];
        unsafe {
            device.update_descriptor_sets(&writes, &[]);
        }
    }

    /// Comienza el pase de profundidad en el command buffer: limpia el
    /// mapa a 1,0 y fija viewport/scissor al tamaño del mapa.
    ///
    /// # Safety
    /// `device` y `cmd` deben ser válidos: `cmd` en estado de
    /// grabación y fuera de un render pass, y los recursos de
    /// sombras vivos durante el pase.
    pub unsafe fn begin_pass(&self, device: &ash::Device, cmd: vk::CommandBuffer) {
        let clear = [vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 },
        }];
        let begin_info = vk::RenderPassBeginInfo::default()
            .render_pass(self.render_pass)
            .framebuffer(self.framebuffer)
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D { width: self.size, height: self.size },
            })
            .clear_values(&clear);
        device.cmd_begin_render_pass(cmd, &begin_info, vk::SubpassContents::INLINE);
        let viewport = [vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: self.size as f32,
            height: self.size as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }];
        device.cmd_set_viewport(cmd, 0, &viewport);
        let scissor = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D { width: self.size, height: self.size },
        }];
        device.cmd_set_scissor(cmd, 0, &scissor);
    }

    /// Destrucción completa con el device (el Renderer lo llama en su
    /// Drop, tras `device_wait_idle`).
    pub fn destroy_with_device(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        unsafe {
            device.destroy_pipeline(self.depth_pipeline, None);
            device.destroy_pipeline_layout(self.depth_layout, None);
            device.destroy_pipeline(self.depth_skinned_pipeline, None);
            device.destroy_pipeline_layout(self.depth_skinned_layout, None);
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_descriptor_pool(self.dummy_pool, None);
            device.destroy_buffer(self.ubo_buffer, None);
            device.destroy_buffer(self.dummy_buffer, None);
            if let Some(a) = self.ubo_allocation.take() {
                allocator.free(a).ok();
            }
            if let Some(a) = self.dummy_allocation.take() {
                allocator.free(a).ok();
            }
            device.destroy_framebuffer(self.framebuffer, None);
            device.destroy_render_pass(self.render_pass, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            if let Some(a) = self.allocation.take() {
                allocator.free(a).ok();
            }
            device.destroy_descriptor_set_layout(self.set_layout, None);
            device.destroy_descriptor_set_layout(self.dummy_layout, None);
        }
    }
}

/// Tamaño de cada cara del atlas de sombras puntuales.
pub const POINT_SHADOW_SIZE: u32 = 512;
/// Cubos del atlas (luces puntuales con sombra simultáneas).
pub const POINT_SHADOW_CUBES: usize = 4;
/// Caras totales del atlas: 4 cubos × 6 caras.
pub const POINT_SHADOW_LAYERS: usize = POINT_SHADOW_CUBES * 6;

/// Atlas de sombras de luces puntuales (Fase 2): una sola imagen D32
/// cube-compatible de 24 capas; cada luz con sombra posee un cubo
/// (6 capas) y el shader lo muestrea como samplerCubeArray. Las caras
/// se renderizan con el render pass del mapa del sol (mismo formato).
pub struct PointShadowResources {
    pub size: u32,
    image: vk::Image,
    allocation: Option<Allocation>,
    render_pass: vk::RenderPass,
    /// Vista cube-array para el muestreo del PBR.
    pub sample_view: vk::ImageView,
    pub sampler: vk::Sampler,
    /// Vistas 2D por capa, usadas solo para construir los framebuffers.
    /// Se guardan para destruirlas de forma segura en destroy_with_device.
    layer_views: Vec<vk::ImageView>,
    framebuffers: Vec<vk::Framebuffer>,
}

impl PointShadowResources {
    pub fn new(
        device: &ash::Device,
        allocator: &mut Allocator,
        render_pass: vk::RenderPass,
    ) -> Result<Self, OffscreenError> {
        let size = POINT_SHADOW_SIZE;

        let image_info = vk::ImageCreateInfo::default()
            .flags(vk::ImageCreateFlags::CUBE_COMPATIBLE)
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::D32_SFLOAT)
            .extent(vk::Extent3D {
                width: size,
                height: size,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(POINT_SHADOW_LAYERS as u32)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let image = unsafe { device.create_image(&image_info, None) }?;

        let requirements = unsafe { device.get_image_memory_requirements(image) };
        let allocation = allocator.allocate(&AllocationCreateDesc {
            name: "point_shadow_atlas",
            requirements,
            location: MemoryLocation::GpuOnly,
            linear: false,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })?;
        unsafe { device.bind_image_memory(image, allocation.memory(), allocation.offset()) }?;

        let sample_view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::CUBE_ARRAY)
            .format(vk::Format::D32_SFLOAT)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::DEPTH,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: POINT_SHADOW_LAYERS as u32,
            });
        let sample_view = unsafe { device.create_image_view(&sample_view_info, None) }?;

        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .compare_enable(false)
            .compare_op(vk::CompareOp::ALWAYS)
            .border_color(vk::BorderColor::FLOAT_OPAQUE_WHITE)
            .min_lod(0.0)
            .max_lod(0.0);
        let sampler = unsafe { device.create_sampler(&sampler_info, None) }?;

        let mut layer_views = Vec::with_capacity(POINT_SHADOW_LAYERS);
        let mut framebuffers = Vec::with_capacity(POINT_SHADOW_LAYERS);
        for layer in 0..POINT_SHADOW_LAYERS as u32 {
            let layer_view_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(vk::Format::D32_SFLOAT)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::DEPTH,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: layer,
                    layer_count: 1,
                });
            let layer_view = unsafe { device.create_image_view(&layer_view_info, None) }?;

            let attachments = [layer_view];
            let fb_info = vk::FramebufferCreateInfo::default()
                .render_pass(render_pass)
                .attachments(&attachments)
                .width(size)
                .height(size)
                .layers(1);
            let framebuffer = unsafe { device.create_framebuffer(&fb_info, None) }?;

            layer_views.push(layer_view);
            framebuffers.push(framebuffer);
        }

        Ok(Self {
            size,
            image,
            allocation: Some(allocation),
            render_pass,
            sample_view,
            sampler,
            layer_views,
            framebuffers,
        })
    }

    /// Comienza el pase de una cara del atlas (layer = cubo*6 + cara).
    ///
    /// # Safety
    /// `device` y `cmd` deben ser válidos: `cmd` en estado de
    /// grabación y fuera de un render pass, `layer` dentro del
    /// atlas y los recursos de sombras vivos durante el pase.
    pub unsafe fn begin_face_pass(&self, device: &ash::Device, cmd: vk::CommandBuffer, layer: usize) {
        let clear_values = [vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue {
                depth: 1.0,
                stencil: 0,
            },
        }];
        let render_area = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: self.size,
                height: self.size,
            },
        };
        let begin_info = vk::RenderPassBeginInfo::default()
            .render_pass(self.render_pass)
            .framebuffer(self.framebuffers[layer])
            .render_area(render_area)
            .clear_values(&clear_values);
        device.cmd_begin_render_pass(cmd, &begin_info, vk::SubpassContents::INLINE);

        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: self.size as f32,
            height: self.size as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        device.cmd_set_viewport(cmd, 0, &[viewport]);
        device.cmd_set_scissor(cmd, 0, &[render_area]);
    }

    pub fn destroy_with_device(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        unsafe {
            for fb in self.framebuffers.drain(..).rev() {
                device.destroy_framebuffer(fb, None);
            }
            for view in self.layer_views.drain(..).rev() {
                device.destroy_image_view(view, None);
            }
            device.destroy_image_view(self.sample_view, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_image(self.image, None);
        }
        if let Some(allocation) = self.allocation.take() {
            let _ = allocator.free(allocation);
        }
    }
}

fn vec3_sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn vec3_dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn vec3_cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn vec3_normalize(v: [f32; 3]) -> [f32; 3] {
    let len = vec3_dot(v, v).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}

fn point_shadow_perspective(near: f32, far: f32) -> [f32; 16] {
    // fov 90°, aspect 1: f = 1 / tan(45°) = 1.
    let f = 1.0f32;
    [
        f, 0.0, 0.0, 0.0,
        0.0, -f, 0.0, 0.0,
        0.0, 0.0, far / (near - far), -1.0,
        0.0, 0.0, near * far / (near - far), 0.0,
    ]
}

fn point_shadow_look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let f = vec3_normalize(vec3_sub(target, eye));
    let r = vec3_normalize(vec3_cross(f, up));
    let u = vec3_cross(r, f);
    [
        r[0], u[0], -f[0], 0.0,
        r[1], u[1], -f[1], 0.0,
        r[2], u[2], -f[2], 0.0,
        -vec3_dot(r, eye), -vec3_dot(u, eye), vec3_dot(f, eye), 1.0,
    ]
}

/// VP de las 6 caras del cubo de una luz puntual (Fase 2), en el orden
/// de capas de Vulkan (+X, -X, +Y, -Y, +Z, -Z): perspectiva de 90° con
/// la convención del motor (Y invertida, profundidad [0, 1]) por la
/// vista look_at de cada eje. Los pares (dirección, up) son los
/// habituales de los cubos de sombras; la verificación visual de la
/// orientación se hace en el test físico de la fase.
pub fn point_shadow_face_vps(pos: [f32; 3], near: f32, far: f32) -> [[f32; 16]; 6] {
    const FACES: [([f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, -1.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
        ([0.0, 0.0, -1.0], [0.0, -1.0, 0.0]),
    ];
    let proj = point_shadow_perspective(near, far);
    let mut out = [[0.0f32; 16]; 6];
    for (i, (dir, up)) in FACES.iter().enumerate() {
        let target = [pos[0] + dir[0], pos[1] + dir[1], pos[2] + dir[2]];
        let view = point_shadow_look_at(pos, target, *up);
        out[i] = glass_core::mat4_mul(proj, view);
    }
    out
}

#[cfg(test)]
mod point_shadow_face_tests {
    use super::*;
    use glass_core::math::Frustum;

    #[test]
    fn cada_cara_mira_a_su_propio_eje() {
        const FACES: [[f32; 3]; 6] = [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        let pos = [0.0f32, 0.0, 0.0];
        let vps = point_shadow_face_vps(pos, 0.1, 25.0);
        for (k, dir) in FACES.iter().enumerate() {
            let vp = vps[k];
            let frustum = Frustum::from_view_proj(&vp);

            let in_front = [
                pos[0] + dir[0] * 5.0,
                pos[1] + dir[1] * 5.0,
                pos[2] + dir[2] * 5.0,
            ];
            // intersects_aabb recibe (min, max): la caja es el punto ± 0,1.
            let caja = |c: [f32; 3]| {
                (
                    [c[0] - 0.1, c[1] - 0.1, c[2] - 0.1],
                    [c[0] + 0.1, c[1] + 0.1, c[2] + 0.1],
                )
            };
            let (mn, mx) = caja(in_front);
            assert!(
                frustum.intersects_aabb(mn, mx),
                "cara {k}: el punto delante de su eje debe estar dentro del frustum"
            );

            let behind = [
                pos[0] - dir[0] * 5.0,
                pos[1] - dir[1] * 5.0,
                pos[2] - dir[2] * 5.0,
            ];
            let (mn, mx) = caja(behind);
            assert!(
                !frustum.intersects_aabb(mn, mx),
                "cara {k}: el punto detrás de su eje no debe estar dentro del frustum"
            );
        }
    }
}
