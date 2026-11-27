//! Cadena de post-procesado (bloque de gráficos: sombras + post):
//! bright-pass → blur separable → pasada final con ACES y viñeta.
//!
//! La escena 3D se dibuja LINEAL en un objetivo HDR
//! (`R16G16B16A16_SFLOAT`); esta cadena produce el objetivo LDR
//! (`R8G8B8A8_UNORM`) de siempre, que es lo que consumen la UI 2D, el
//! blit del standalone y egui — aguas abajo de aquí nada cambia. El
//! ACES (Narkowicz, exposición 1,5) vive SOLO en la pasada final; el
//! PBR sin post (flag desactivado) usa el gemelo `pbr_legacy.frag` con
//! el ACES dentro, como antes de este bloque.
//!
//! Ajuste en caliente por entorno (leído una vez al crear el renderer):
//! `GLASS_POST=0` apaga la cadena entera, `GLASS_SHADOWS=0` las
//! sombras, `GLASS_BLOOM=<f>` la intensidad del bloom y
//! `GLASS_VIGNETTE=<f>` la de la viñeta.

use ash::vk;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator};
use gpu_allocator::MemoryLocation;
use std::sync::Arc;

use crate::offscreen::{OffscreenError, OffscreenTarget};
use crate::pipeline::{GraphicsPipeline, PipelineError};

const POST_VERT_SPV: &[u8] = include_bytes!("../../../assets/shaders/post.vert.spv");
const POST_BRIGHT_SPV: &[u8] = include_bytes!("../../../assets/shaders/post_bright.frag.spv");
const POST_BLUR_SPV: &[u8] = include_bytes!("../../../assets/shaders/post_blur.frag.spv");
const POST_FINAL_SPV: &[u8] = include_bytes!("../../../assets/shaders/post_final.frag.spv");

/// Parámetros de la cadena. Los valores por defecto son conservadores:
/// el bloom se nota en los brillos (fuego, especulares) sin lavar la
/// escena, y la viñeta solo se percibe en las esquinas.
#[derive(Clone, Copy, Debug)]
pub struct PostSettings {
    /// Umbral de luminancia lineal HDR para el bright-pass.
    pub threshold: f32,
    /// Rodilla suave del umbral.
    pub knee: f32,
    /// Intensidad con la que el bloom se suma a la escena.
    pub intensity: f32,
    /// Intensidad de la viñeta (0 = sin viñeta).
    pub vignette: f32,
    /// Exposición previa al ACES (la histórica del PBR: 1,5).
    pub exposure: f32,
}

impl Default for PostSettings {
    fn default() -> Self {
        Self {
            threshold: 1.0,
            knee: 0.5,
            intensity: 0.3,
            vignette: 0.22,
            exposure: 1.5,
        }
    }
}

impl PostSettings {
    /// Construye el post a partir de los valores resueltos de escena
    /// (G2: exposición/intensidad/viñeta). El umbral y la rodilla del
    /// bloom son constantes del motor, no datos de escena. Tras
    /// aplicar los valores de escena, los overrides de entorno
    /// GLASS_* siguen mandando (misma semántica que antes).
    pub fn from_scene_values(exposure: f32, intensity: f32, vignette: f32) -> Self {
        let mut s = Self {
            threshold: 1.0,
            knee: 0.5,
            intensity,
            vignette,
            exposure,
        };
        Self::apply_env_overrides(&mut s);
        s
    }

    /// Sin escena (compatibilidad): valores heredados + overrides de
    /// entorno. Equivalente a `from_scene_values` con los LEGACY_POST_*.
    pub fn from_env() -> Self {
        let mut s = Self::default();
        Self::apply_env_overrides(&mut s);
        s
    }

    fn apply_env_overrides(s: &mut Self) {
        if let Ok(v) = std::env::var("GLASS_BLOOM") {
            if let Ok(f) = v.parse::<f32>() { s.intensity = f.max(0.0); }
        }
        if let Ok(v) = std::env::var("GLASS_BLOOM_THRESHOLD") {
            if let Ok(f) = v.parse::<f32>() { s.threshold = f.max(0.0); }
        }
        if let Ok(v) = std::env::var("GLASS_VIGNETTE") {
            if let Ok(f) = v.parse::<f32>() { s.vignette = f.clamp(0.0, 1.0); }
        }
    }

    fn push_bytes(&self, blur_dir: [f32; 2]) -> [u8; 32] {
        let params: [f32; 8] = [
            self.threshold,
            self.knee,
            self.intensity,
            self.vignette,
            blur_dir[0],
            blur_dir[1],
            self.exposure,
            1.0,
        ];
        bytemuck::bytes_of(&params).try_into().unwrap()
    }
}

/// Un objetivo intermedio de la cadena (color SFLOAT muestreable, sin
/// profundidad): bright-pass y los dos lados del ping-pong del blur.
struct PostTarget {
    image: vk::Image,
    allocation: Option<Allocation>,
    view: vk::ImageView,
    sampler: vk::Sampler,
    framebuffer: vk::Framebuffer,
    extent: vk::Extent2D,
}

impl PostTarget {
    fn new(
        device: &ash::Device,
        allocator: &mut Allocator,
        render_pass: vk::RenderPass,
        width: u32,
        height: u32,
        name: &str,
    ) -> Result<Self, OffscreenError> {
        unsafe {
            let image_info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::R16G16B16A16_SFLOAT)
                .extent(vk::Extent3D { width, height, depth: 1 })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .initial_layout(vk::ImageLayout::UNDEFINED);
            let image = device.create_image(&image_info, None)?;
            let mem_reqs = device.get_image_memory_requirements(image);
            let allocation = allocator.allocate(&AllocationCreateDesc {
                name,
                requirements: mem_reqs,
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })?;
            device.bind_image_memory(image, allocation.memory(), allocation.offset())?;
            let view_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(vk::Format::R16G16B16A16_SFLOAT)
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(1),
                );
            let view = device.create_image_view(&view_info, None)?;
            let sampler_info = vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .mipmap_mode(vk::SamplerMipmapMode::LINEAR);
            let sampler = device.create_sampler(&sampler_info, None)?;
            let attachments = [view];
            let fb_info = vk::FramebufferCreateInfo::default()
                .render_pass(render_pass)
                .attachments(&attachments)
                .width(width)
                .height(height)
                .layers(1);
            let framebuffer = device.create_framebuffer(&fb_info, None)?;
            Ok(Self {
                image,
                allocation: Some(allocation),
                view,
                sampler,
                framebuffer,
                extent: vk::Extent2D { width, height },
            })
        }
    }

    fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        unsafe {
            device.destroy_framebuffer(self.framebuffer, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            if let Some(a) = self.allocation.take() {
                allocator.free(a).ok();
            }
        }
    }
}

fn pipe_err(e: PipelineError) -> OffscreenError {
    match e {
        PipelineError::Driver(r) => OffscreenError::Driver(r),
        PipelineError::ShaderLoadError => {
            OffscreenError::Driver(vk::Result::ERROR_INITIALIZATION_FAILED)
        }
    }
}

/// La cadena completa. Posee sus tres pipelines, el render pass de los
/// objetivos intermedios y una pequeña caché de sets (las combinaciones
/// (fuente, auxiliar) son dos en la práctica: vista de escena y de
/// juego).
pub struct PostChain {
    set_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    render_pass_small: vk::RenderPass,
    /// 0 = bright, 1 y 2 = ping-pong del blur; tras el blur vertical el
    /// resultado vuelve a estar en el 0.
    targets: [PostTarget; 3],
    bright_pipeline: GraphicsPipeline,
    blur_pipeline: GraphicsPipeline,
    final_pipeline: GraphicsPipeline,
    sets: Vec<(vk::ImageView, vk::ImageView, vk::DescriptorSet)>,
}

impl PostChain {
    /// `scene_width/height`: la resolución de los objetivos de escena;
    /// los intermedios trabajan a la mitad. `final_render_pass`: el del
    /// objetivo LDR de presentación, contra el que se crea el pipeline
    /// de la pasada final.
    pub fn new(
        device: Arc<ash::Device>,
        allocator: &mut Allocator,
        scene_width: u32,
        scene_height: u32,
        final_render_pass: vk::RenderPass,
    ) -> Result<Self, OffscreenError> {
        let half_w = (scene_width / 2).max(1);
        let half_h = (scene_height / 2).max(1);
        unsafe {
            // Set 0 del post: dos texturas muestreadas (escena y aux).
            let bindings = [
                vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(1)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            ];
            let set_layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
            let set_layout = device.create_descriptor_set_layout(&set_layout_info, None)?;
            let pool_sizes = [vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(32)];
            let pool_info = vk::DescriptorPoolCreateInfo::default()
                .pool_sizes(&pool_sizes)
                .max_sets(16);
            let pool = device.create_descriptor_pool(&pool_info, None)?;

            // Render pass de los objetivos intermedios: color SFLOAT,
            // sin profundidad, salida muestreable. Las dependencias
            // ordenan escritura→lectura entre pasadas de la cadena.
            let attachment = [vk::AttachmentDescription::default()
                .format(vk::Format::R16G16B16A16_SFLOAT)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let color_ref = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            let subpass = [vk::SubpassDescription::default()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(&color_ref)];
            let dependencies = [
                vk::SubpassDependency::default()
                    .src_subpass(vk::SUBPASS_EXTERNAL)
                    .dst_subpass(0)
                    .src_stage_mask(vk::PipelineStageFlags::FRAGMENT_SHADER)
                    .src_access_mask(vk::AccessFlags::SHADER_READ)
                    .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                    .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
                vk::SubpassDependency::default()
                    .src_subpass(0)
                    .dst_subpass(vk::SUBPASS_EXTERNAL)
                    .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                    .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags::FRAGMENT_SHADER)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ),
            ];
            let pass_info = vk::RenderPassCreateInfo::default()
                .attachments(&attachment)
                .subpasses(&subpass)
                .dependencies(&dependencies);
            let render_pass_small = device.create_render_pass(&pass_info, None)?;

            let targets = [
                PostTarget::new(&device, allocator, render_pass_small, half_w, half_h, "post_bright")?,
                PostTarget::new(&device, allocator, render_pass_small, half_w, half_h, "post_blur_a")?,
                PostTarget::new(&device, allocator, render_pass_small, half_w, half_h, "post_blur_b")?,
            ];

            let bright_pipeline = GraphicsPipeline::new_post_pipeline(
                device.clone(),
                render_pass_small,
                POST_VERT_SPV,
                POST_BRIGHT_SPV,
                set_layout,
            )
            .map_err(pipe_err)?;
            let blur_pipeline = GraphicsPipeline::new_post_pipeline(
                device.clone(),
                render_pass_small,
                POST_VERT_SPV,
                POST_BLUR_SPV,
                set_layout,
            )
            .map_err(pipe_err)?;
            let final_pipeline = GraphicsPipeline::new_post_pipeline(
                device.clone(),
                final_render_pass,
                POST_VERT_SPV,
                POST_FINAL_SPV,
                set_layout,
            )
            .map_err(pipe_err)?;

            Ok(Self {
                set_layout,
                pool,
                render_pass_small,
                targets,
                bright_pipeline,
                blur_pipeline,
                final_pipeline,
                sets: Vec::new(),
            })
        }
    }

    fn set_for(
        &mut self,
        device: &ash::Device,
        src: (vk::ImageView, vk::Sampler),
        aux: (vk::ImageView, vk::Sampler),
    ) -> Result<vk::DescriptorSet, OffscreenError> {
        if let Some((_, _, set)) = self
            .sets
            .iter()
            .find(|(s, a, _)| *s == src.0 && *a == aux.0)
        {
            return Ok(*set);
        }
        let layouts = [self.set_layout];
        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.pool)
            .set_layouts(&layouts);
        let set = unsafe { device.allocate_descriptor_sets(&alloc_info)? }
            .into_iter()
            .next()
            .ok_or(OffscreenError::Driver(vk::Result::ERROR_INITIALIZATION_FAILED))?;
        let infos = [
            [vk::DescriptorImageInfo::default()
                .sampler(src.1)
                .image_view(src.0)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)],
            [vk::DescriptorImageInfo::default()
                .sampler(aux.1)
                .image_view(aux.0)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)],
        ];
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&infos[0]),
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&infos[1]),
        ];
        unsafe { device.update_descriptor_sets(&writes, &[]) };
        self.sets.push((src.0, aux.0, set));
        Ok(set)
    }

    /// Graba la cadena entera en `cmd` (ya comenzado): bright-pass y
    /// blur sobre los intermedios y la pasada final sobre `dst`. `src`
    /// debe estar en SHADER_READ_ONLY_OPTIMAL (el layout final del pase
    /// 3D) y `dst` es un objetivo LDR cuyo render pass deja el mismo
    /// layout al salir.
    pub fn record(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        src: &OffscreenTarget,
        dst: &OffscreenTarget,
        settings: &PostSettings,
    ) -> Result<(), OffscreenError> {
        let src_pair = (src.color_view, src.sampler);
        let t0 = (self.targets[0].view, self.targets[0].sampler);
        let t1 = (self.targets[1].view, self.targets[1].sampler);
        let set_bright = self.set_for(device, src_pair, src_pair)?;
        let set_blur_h = self.set_for(device, t0, t0)?;
        let set_blur_v = self.set_for(device, t1, t1)?;
        let set_final = self.set_for(device, src_pair, t0)?;

        unsafe {
            // 1) Bright-pass: escena HDR → intermedio 0.
            self.pass(
                device,
                cmd,
                self.render_pass_small,
                self.targets[0].framebuffer,
                self.targets[0].extent,
                &self.bright_pipeline,
                set_bright,
                &settings.push_bytes([0.0, 0.0]),
            );
            // 2) Blur horizontal: intermedio 0 → intermedio 1.
            self.pass(
                device,
                cmd,
                self.render_pass_small,
                self.targets[1].framebuffer,
                self.targets[1].extent,
                &self.blur_pipeline,
                set_blur_h,
                &settings.push_bytes([1.0, 0.0]),
            );
            // 3) Blur vertical: intermedio 1 → intermedio 0.
            self.pass(
                device,
                cmd,
                self.render_pass_small,
                self.targets[0].framebuffer,
                self.targets[0].extent,
                &self.blur_pipeline,
                set_blur_v,
                &settings.push_bytes([0.0, 1.0]),
            );
            // 4) Final: escena + bloom → ACES + viñeta → objetivo LDR.
            self.pass(
                device,
                cmd,
                dst.render_pass,
                dst.framebuffer,
                dst.extent,
                &self.final_pipeline,
                set_final,
                &settings.push_bytes([0.0, 0.0]),
            );
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn pass(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        render_pass: vk::RenderPass,
        framebuffer: vk::Framebuffer,
        extent: vk::Extent2D,
        pipeline: &GraphicsPipeline,
        set: vk::DescriptorSet,
        push: &[u8; 32],
    ) {
        let clear = [vk::ClearValue {
            color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] },
        }];
        let begin_info = vk::RenderPassBeginInfo::default()
            .render_pass(render_pass)
            .framebuffer(framebuffer)
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent,
            })
            .clear_values(&clear);
        device.cmd_begin_render_pass(cmd, &begin_info, vk::SubpassContents::INLINE);
        let viewport = [vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: extent.width as f32,
            height: extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }];
        device.cmd_set_viewport(cmd, 0, &viewport);
        let scissor = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent,
        }];
        device.cmd_set_scissor(cmd, 0, &scissor);
        device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline.pipeline);
        device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            pipeline.layout,
            0,
            &[set],
            &[],
        );
        device.cmd_push_constants(
            cmd,
            pipeline.layout,
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            0,
            push,
        );
        device.cmd_draw(cmd, 3, 1, 0, 0);
        device.cmd_end_render_pass(cmd);
    }

    pub fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        for t in self.targets.iter_mut() {
            t.destroy(device, allocator);
        }
        unsafe {
            device.destroy_render_pass(self.render_pass_small, None);
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
        }
    }
}
