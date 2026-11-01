use crate::context::VulkanContext;
use crate::offscreen::OffscreenTarget;
use crate::pipeline::GraphicsPipeline;
use crate::swapchain::SwapchainContext;
use ash::vk;
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};
use std::sync::{Arc, Mutex};
use thiserror::Error;

const VERT_SPV: &[u8] = include_bytes!("../../../assets/shaders/triangle.vert.spv");
const FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/triangle.frag.spv");

#[derive(Error, Debug)]
pub enum RendererError {
    #[error("Vulkan driver error: {0}")]
    Driver(#[from] vk::Result),
    #[error("Swapchain error: {0}")]
    Swapchain(#[from] crate::swapchain::SwapchainError),
    #[error("Offscreen error: {0}")]
    Offscreen(#[from] crate::offscreen::OffscreenError),
    #[error("Pipeline error: {0}")]
    Pipeline(#[from] crate::pipeline::PipelineError),
    #[error("GPU allocator initialization error: {0}")]
    Allocator(#[from] gpu_allocator::AllocationError),
    #[error("Descriptor error: {0}")]
    Descriptor(String),
}

pub struct GlassRenderer {
    pub device: Arc<ash::Device>,
    pub allocator: Arc<Mutex<Allocator>>,
    pub swapchain: SwapchainContext,
    pub swapchain_render_pass: vk::RenderPass,
    pub swapchain_framebuffers: Vec<vk::Framebuffer>,
    pub offscreen: OffscreenTarget,
    pub pipeline: GraphicsPipeline,
    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
    pub image_available_semaphore: vk::Semaphore,
    pub render_finished_semaphore: vk::Semaphore,
}

fn create_swapchain_render_pass(
    device: &ash::Device,
    format: vk::Format,
) -> Result<vk::RenderPass, vk::Result> {
    let attachments = [vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)];

    let color_attachment_ref = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_attachment_ref)];

    let dependencies = [vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)];

    let create_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);

    unsafe { device.create_render_pass(&create_info, None) }
}

fn create_swapchain_framebuffers(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    views: &[vk::ImageView],
    extent: vk::Extent2D,
) -> Result<Vec<vk::Framebuffer>, vk::Result> {
    let mut framebuffers = Vec::with_capacity(views.len());
    for &view in views {
        let attachments = [view];
        let create_info = vk::FramebufferCreateInfo::default()
            .render_pass(render_pass)
            .attachments(&attachments)
            .width(extent.width)
            .height(extent.height)
            .layers(1);
        let fb = unsafe { device.create_framebuffer(&create_info, None)? };
        framebuffers.push(fb);
    }
    Ok(framebuffers)
}

impl GlassRenderer {
    pub fn new(context: &VulkanContext, width: u32, height: u32) -> Result<Self, RendererError> {
        let device = Arc::new(unsafe {
            ash::Device::load(context.instance.fp_v1_0(), context.device.handle())
        });

        // 1. Inicializar GPU Allocator compartido
        let allocator = Arc::new(Mutex::new(Allocator::new(&AllocatorCreateDesc {
            instance: context.instance.clone(),
            device: context.device.clone(),
            physical_device: context.physical_device,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })?));

        // 2. Inicializar Swapchain
        let swapchain = SwapchainContext::new(
            &context.instance,
            device.clone(),
            &context.surface_loader,
            context.surface,
            context.physical_device,
            width,
            height,
        )?;

        // 3. Crear RenderPass y Framebuffers para Swapchain
        let swapchain_render_pass = create_swapchain_render_pass(&device, swapchain.format)?;
        let swapchain_framebuffers = create_swapchain_framebuffers(
            &device,
            swapchain_render_pass,
            &swapchain.image_views,
            swapchain.extent,
        )?;

        // 4. Inicializar Viewport Offscreen desacoplado (1280x720)
        let offscreen = OffscreenTarget::new(
            device.clone(),
            &mut *allocator.lock().unwrap(),
            1280,
            720,
        )?;

        // 5. Inicializar Pipeline Gráfico con shaders SPIR-V
        let pipeline = GraphicsPipeline::new(device.clone(), offscreen.render_pass, VERT_SPV, FRAG_SPV)?;

        // 6. Command Pool y Buffers
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(context.queue_families.graphics_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&pool_info, None)? };

        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let command_buffer = unsafe { device.allocate_command_buffers(&alloc_info)?[0] };

        // 7. Sincronización
        let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        let semaphore_info = vk::SemaphoreCreateInfo::default();
        let image_available_semaphore = unsafe { device.create_semaphore(&semaphore_info, None)? };
        let render_finished_semaphore = unsafe { device.create_semaphore(&semaphore_info, None)? };

        log::info!("GlassRenderer inicializado con éxito con soporte de Swapchain y Viewport Offscreen.");

        Ok(Self {
            device,
            allocator,
            swapchain,
            swapchain_render_pass,
            swapchain_framebuffers,
            offscreen,
            pipeline,
            command_pool,
            command_buffer,
            fence,
            image_available_semaphore,
            render_finished_semaphore,
        })
    }

    /// Recrea el Swapchain y sus framebuffers cuando la ventana nativa cambia de tamaño.
    pub fn resize(
        &mut self,
        context: &VulkanContext,
        width: u32,
        height: u32,
    ) -> Result<(), RendererError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        unsafe {
            self.device.device_wait_idle()?;
            for fb in self.swapchain_framebuffers.drain(..) {
                self.device.destroy_framebuffer(fb, None);
            }
        }
        self.swapchain = SwapchainContext::new(
            &context.instance,
            self.device.clone(),
            &context.surface_loader,
            context.surface,
            context.physical_device,
            width,
            height,
        )?;
        self.swapchain_framebuffers = create_swapchain_framebuffers(
            &self.device,
            self.swapchain_render_pass,
            &self.swapchain.image_views,
            self.swapchain.extent,
        )?;
        log::debug!(
            "Swapchain redimensionado exitosamente a {}x{}",
            width,
            height
        );
        Ok(())
    }

    /// Crea un DescriptorSet para la textura offscreen compatible con egui_ash_renderer.
    pub fn create_offscreen_descriptor_set(
        &self,
    ) -> Result<(vk::DescriptorSetLayout, vk::DescriptorPool, vk::DescriptorSet), RendererError> {
        let layout = egui_ash_renderer::vulkan::create_vulkan_descriptor_set_layout(&self.device)
            .map_err(|e| RendererError::Descriptor(e.to_string()))?;
        let pool = egui_ash_renderer::vulkan::create_vulkan_descriptor_pool(&self.device, 16)
            .map_err(|e| RendererError::Descriptor(e.to_string()))?;
        let set = egui_ash_renderer::vulkan::create_vulkan_descriptor_set(
            &self.device,
            layout,
            pool,
            self.offscreen.color_view,
            self.offscreen.sampler,
        )
        .map_err(|e| RendererError::Descriptor(e.to_string()))?;

        Ok((layout, pool, set))
    }

    /// Renderiza la escena 3D a la textura offscreen desacoplada.
    pub fn render_offscreen(&mut self, queue: vk::Queue) -> Result<(), RendererError> {
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            self.device.reset_fences(&[self.fence])?;

            let cmd = self.command_buffer;
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device.begin_command_buffer(cmd, &begin_info)?;

            let clear_values = [
                vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: [0.05, 0.08, 0.15, 1.0],
                    },
                },
                vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: 1.0,
                        stencil: 0,
                    },
                },
            ];

            let render_pass_begin_info = vk::RenderPassBeginInfo::default()
                .render_pass(self.offscreen.render_pass)
                .framebuffer(self.offscreen.framebuffer)
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: self.offscreen.extent,
                })
                .clear_values(&clear_values);

            self.device.cmd_begin_render_pass(
                cmd,
                &render_pass_begin_info,
                vk::SubpassContents::INLINE,
            );

            let viewport = [vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: self.offscreen.extent.width as f32,
                height: self.offscreen.extent.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            }];
            self.device.cmd_set_viewport(cmd, 0, &viewport);

            let scissor = [vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: self.offscreen.extent,
            }];
            self.device.cmd_set_scissor(cmd, 0, &scissor);

            self.device
                .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline.pipeline);

            self.device.cmd_draw(cmd, 3, 1, 0, 0);

            self.device.cmd_end_render_pass(cmd);
            self.device.end_command_buffer(cmd)?;

            let command_buffers = [cmd];
            let submit_info = [vk::SubmitInfo::default().command_buffers(&command_buffers)];

            self.device.queue_submit(queue, &submit_info, self.fence)?;
        }

        Ok(())
    }
}

impl Drop for GlassRenderer {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().unwrap_or(());
            for fb in self.swapchain_framebuffers.drain(..) {
                self.device.destroy_framebuffer(fb, None);
            }
            self.device
                .destroy_render_pass(self.swapchain_render_pass, None);
            self.device.destroy_semaphore(self.image_available_semaphore, None);
            self.device.destroy_semaphore(self.render_finished_semaphore, None);
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
        }
        self.offscreen
            .destroy(&mut *self.allocator.lock().unwrap());
    }
}
