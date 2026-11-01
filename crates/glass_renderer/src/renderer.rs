use crate::context::VulkanContext;
use crate::offscreen::OffscreenTarget;
use crate::pipeline::GraphicsPipeline;
use crate::swapchain::SwapchainContext;
use ash::vk;
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};
use std::sync::Arc;
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
}

pub struct GlassRenderer {
    pub device: Arc<ash::Device>,
    pub allocator: Allocator,
    pub swapchain: SwapchainContext,
    pub offscreen: OffscreenTarget,
    pub pipeline: GraphicsPipeline,
    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
    pub image_available_semaphore: vk::Semaphore,
    pub render_finished_semaphore: vk::Semaphore,
}

impl GlassRenderer {
    pub fn new(context: &VulkanContext, width: u32, height: u32) -> Result<Self, RendererError> {
        let device = Arc::new(unsafe {
            // Clonamos el handle de ash::Device
            ash::Device::load(context.instance.fp_v1_0(), context.device.handle())
        });

        // 1. Inicializar GPU Allocator
        let mut allocator = Allocator::new(&AllocatorCreateDesc {
            instance: context.instance.clone(),
            device: context.device.clone(),
            physical_device: context.physical_device,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })?;

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

        // 3. Inicializar Viewport Offscreen desacoplado (1280x720)
        let offscreen = OffscreenTarget::new(device.clone(), &mut allocator, width, height)?;

        // 4. Inicializar Pipeline Gráfico con shaders SPIR-V
        let pipeline = GraphicsPipeline::new(device.clone(), offscreen.render_pass, VERT_SPV, FRAG_SPV)?;

        // 5. Command Pool y Buffers
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(context.queue_families.graphics_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&pool_info, None)? };

        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let command_buffer = unsafe { device.allocate_command_buffers(&alloc_info)?[0] };

        // 6. Sincronización
        let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        let semaphore_info = vk::SemaphoreCreateInfo::default();
        let image_available_semaphore = unsafe { device.create_semaphore(&semaphore_info, None)? };
        let render_finished_semaphore = unsafe { device.create_semaphore(&semaphore_info, None)? };

        log::info!("GlassRenderer inicializado con éxito para Hito 3.");

        Ok(Self {
            device,
            allocator,
            swapchain,
            offscreen,
            pipeline,
            command_pool,
            command_buffer,
            fence,
            image_available_semaphore,
            render_finished_semaphore,
        })
    }

    /// Renderiza la escena 3D a la textura offscreen desacoplada.
    pub fn render_offscreen(&mut self, queue: vk::Queue) -> Result<(), RendererError> {
        unsafe {
            // Esperar que la GPU haya terminado el frame anterior
            self.device
                .wait_for_fences(&[self.fence], true, u64::MAX)?;
            self.device.reset_fences(&[self.fence])?;

            let cmd = self.command_buffer;
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device.begin_command_buffer(cmd, &begin_info)?;

            // Color de fondo característico de GlassEngine (Azul profundo oscuro)
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

            // Dynamic Viewport y Scissor
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

            // Enlazar Pipeline y dibujar triángulo básico
            self.device
                .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline.pipeline);

            // Dibuja 3 vértices generados proceduralmente en el shader
            self.device.cmd_draw(cmd, 3, 1, 0, 0);

            self.device.cmd_end_render_pass(cmd);
            self.device.end_command_buffer(cmd)?;

            // Enviar a la cola gráfica
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
            self.device.destroy_semaphore(self.image_available_semaphore, None);
            self.device.destroy_semaphore(self.render_finished_semaphore, None);
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
        }
        self.offscreen.destroy(&mut self.allocator);
    }
}
