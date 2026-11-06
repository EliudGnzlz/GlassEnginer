use ash::vk;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator};
use gpu_allocator::MemoryLocation;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum OffscreenError {
    #[error("Vulkan driver error: {0}")]
    Driver(#[from] vk::Result),
    #[error("GPU allocator error: {0}")]
    Allocator(#[from] gpu_allocator::AllocationError),
}

pub struct OffscreenTarget {
    pub device: Arc<ash::Device>,
    pub extent: vk::Extent2D,
    pub color_image: vk::Image,
    pub color_allocation: Option<Allocation>,
    pub color_view: vk::ImageView,
    pub depth_image: vk::Image,
    pub depth_allocation: Option<Allocation>,
    pub depth_view: vk::ImageView,
    pub sampler: vk::Sampler,
    pub render_pass: vk::RenderPass,
    pub framebuffer: vk::Framebuffer,
}

impl OffscreenTarget {
    pub fn new(
        device: Arc<ash::Device>,
        allocator: &mut Allocator,
        width: u32,
        height: u32,
    ) -> Result<Self, OffscreenError> {
        Self::with_format(device, allocator, width, height, vk::Format::R8G8B8A8_UNORM)
    }

    /// Igual que `new` pero con el formato de color elegido por el
    /// llamante. El bloque de gráficos (sombras + post) crea los
    /// objetivos de escena en `R16G16B16A16_SFLOAT` (lineal HDR, para
    /// que el bloom trabaje sobre brillos reales y el ACES viva en la
    /// pasada final) y los objetivos de presentación en el UNORM de
    /// siempre. El pase de profundidad no cambia (D32).
    pub fn with_format(
        device: Arc<ash::Device>,
        allocator: &mut Allocator,
        width: u32,
        height: u32,
        color_format: vk::Format,
    ) -> Result<Self, OffscreenError> {
        let extent = vk::Extent2D { width, height };
        let depth_format = vk::Format::D32_SFLOAT;

        // 1. Textura de Color Offscreen (Muestreable por UI / egui)
        let color_image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(color_format)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(
                vk::ImageUsageFlags::COLOR_ATTACHMENT
                    | vk::ImageUsageFlags::SAMPLED
                    | vk::ImageUsageFlags::TRANSFER_SRC,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);

        let color_image = unsafe { device.create_image(&color_image_info, None)? };
        let color_mem_reqs = unsafe { device.get_image_memory_requirements(color_image) };

        let color_allocation = allocator.allocate(&AllocationCreateDesc {
            name: "glass_offscreen_color",
            requirements: color_mem_reqs,
            location: MemoryLocation::GpuOnly,
            linear: false,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })?;

        unsafe {
            device.bind_image_memory(color_image, color_allocation.memory(), color_allocation.offset())?;
        }

        let color_view_info = vk::ImageViewCreateInfo::default()
            .image(color_image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(color_format)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1),
            );
        let color_view = unsafe { device.create_image_view(&color_view_info, None)? };

        // 2. Textura de Profundidad Offscreen
        let depth_image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(depth_format)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);

        let depth_image = unsafe { device.create_image(&depth_image_info, None)? };
        let depth_mem_reqs = unsafe { device.get_image_memory_requirements(depth_image) };

        let depth_allocation = allocator.allocate(&AllocationCreateDesc {
            name: "glass_offscreen_depth",
            requirements: depth_mem_reqs,
            location: MemoryLocation::GpuOnly,
            linear: false,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })?;

        unsafe {
            device.bind_image_memory(depth_image, depth_allocation.memory(), depth_allocation.offset())?;
        }

        let depth_view_info = vk::ImageViewCreateInfo::default()
            .image(depth_image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(depth_format)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::DEPTH)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1),
            );
        let depth_view = unsafe { device.create_image_view(&depth_view_info, None)? };

        // 3. Sampler para la textura
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .mipmap_mode(vk::SamplerMipmapMode::LINEAR);
        let sampler = unsafe { device.create_sampler(&sampler_info, None)? };

        // 4. RenderPass desacoplado (salida: SHADER_READ_ONLY_OPTIMAL)
        let attachments = [
            // Color
            vk::AttachmentDescription::default()
                .format(color_format)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
            // Depth
            vk::AttachmentDescription::default()
                .format(depth_format)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::DONT_CARE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
        ];

        let color_attachment_ref = [vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

        let depth_attachment_ref = vk::AttachmentReference::default()
            .attachment(1)
            .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);

        let subpasses = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&color_attachment_ref)
            .depth_stencil_attachment(&depth_attachment_ref)];

        let dependencies = [vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                    | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
            )
            .dst_stage_mask(
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                    | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
            )
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                    | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
            )];

        let render_pass_info = vk::RenderPassCreateInfo::default()
            .attachments(&attachments)
            .subpasses(&subpasses)
            .dependencies(&dependencies);

        let render_pass = unsafe { device.create_render_pass(&render_pass_info, None)? };

        // 5. Framebuffer Offscreen
        let fb_attachments = [color_view, depth_view];
        let fb_info = vk::FramebufferCreateInfo::default()
            .render_pass(render_pass)
            .attachments(&fb_attachments)
            .width(width)
            .height(height)
            .layers(1);
        let framebuffer = unsafe { device.create_framebuffer(&fb_info, None)? };

        log::info!(
            "OffscreenTarget creado: {}x{} | Color: {:?} | Depth: {:?}",
            width,
            height,
            color_format,
            depth_format
        );

        Ok(Self {
            device,
            extent,
            color_image,
            color_allocation: Some(color_allocation),
            color_view,
            depth_image,
            depth_allocation: Some(depth_allocation),
            depth_view,
            sampler,
            render_pass,
            framebuffer,
        })
    }

    pub fn destroy(&mut self, allocator: &mut Allocator) {
        unsafe {
            self.device.destroy_framebuffer(self.framebuffer, None);
            self.device.destroy_render_pass(self.render_pass, None);
            self.device.destroy_sampler(self.sampler, None);
            self.device.destroy_image_view(self.color_view, None);
            self.device.destroy_image_view(self.depth_view, None);
            self.device.destroy_image(self.color_image, None);
            self.device.destroy_image(self.depth_image, None);
        }

        if let Some(alloc) = self.color_allocation.take() {
            allocator.free(alloc).unwrap_or(());
        }
        if let Some(alloc) = self.depth_allocation.take() {
            allocator.free(alloc).unwrap_or(());
        }
    }
}
