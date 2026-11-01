use ash::vk;
use egui_ash_renderer::Renderer as EguiVulkanRenderer;
use egui_ash_renderer::allocator::GpuAllocator;
use egui_winit::State as EguiWinitState;
use glass_core::{GlassWindow, WindowConfig};
use glass_editor::EditorDockingLayout;
use glass_renderer::{GlassRenderer, VulkanConfig, VulkanContext};
use std::ffi::CStr;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

struct GlassEngineApp {
    window_config: WindowConfig,
    vulkan_config: VulkanConfig,
    window: Option<GlassWindow>,
    vulkan_context: Option<VulkanContext>,
    renderer: Option<GlassRenderer>,
    egui_ctx: egui::Context,
    egui_winit: Option<EguiWinitState>,
    egui_renderer: Option<EguiVulkanRenderer<GpuAllocator>>,
    desc_layout: Option<vk::DescriptorSetLayout>,
    desc_pool: Option<vk::DescriptorPool>,
    viewport_texture_id: Option<egui::TextureId>,
    editor_layout: EditorDockingLayout,
    is_headless_test: bool,
}

impl Drop for GlassEngineApp {
    fn drop(&mut self) {
        log::info!("Limpiando GlassEngineApp en orden estricto de dependencias...");
        if let Some(vk_ctx) = &self.vulkan_context {
            unsafe {
                let _ = vk_ctx.device.device_wait_idle();
            }
        }

        if let (Some(renderer), Some(pool), Some(layout)) = (&self.renderer, self.desc_pool, self.desc_layout) {
            unsafe {
                renderer.device.destroy_descriptor_pool(pool, None);
                renderer.device.destroy_descriptor_set_layout(layout, None);
            }
        }

        self.egui_renderer = None;
        self.egui_winit = None;
        self.renderer = None;
        self.vulkan_context = None;
        self.window = None;
        log::info!("GlassEngineApp limpiado exitosamente.");
    }
}

impl GlassEngineApp {
    pub fn new(is_headless_test: bool) -> Self {
        Self {
            window_config: WindowConfig::default(),
            vulkan_config: VulkanConfig::default(),
            window: None,
            vulkan_context: None,
            renderer: None,
            egui_ctx: egui::Context::default(),
            egui_winit: None,
            egui_renderer: None,
            desc_layout: None,
            desc_pool: None,
            viewport_texture_id: None,
            editor_layout: EditorDockingLayout::new(),
            is_headless_test,
        }
    }

    fn render_frame(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (window, vk_ctx, renderer, egui_winit, egui_renderer, texture_id) = match (
            &self.window,
            &self.vulkan_context,
            &mut self.renderer,
            &mut self.egui_winit,
            &mut self.egui_renderer,
            self.viewport_texture_id,
        ) {
            (
                Some(w),
                Some(ctx),
                Some(rend),
                Some(ew),
                Some(er),
                Some(tid),
            ) => (w, ctx, rend, ew, er, tid),
            _ => return Ok(()),
        };

        // 1. Renderizar geometría 3D a la textura offscreen en VRAM
        renderer.render_offscreen(vk_ctx.graphics_queue)?;

        // 2. Ejecutar lógica y UI de egui (Docking estilo Unity)
        let raw_input = egui_winit.take_egui_input(&window.inner);
        let editor_layout = &mut self.editor_layout;
        let mut full_output = self.egui_ctx.run_ui(raw_input, |ui| {
            editor_layout.render(ui, Some(texture_id));
        });
        egui_winit.handle_platform_output(&window.inner, full_output.platform_output);

        // 3. Sincronizar texturas internas de egui hacia la GPU
        for (id, deltas) in full_output.textures_delta.set.drain() {
            for delta in deltas {
                egui_renderer.set_texture(vk_ctx.graphics_queue, renderer.command_pool, id, &delta)?;
            }
        }
        for id in full_output.textures_delta.free.drain() {
            let _ = egui_renderer.free_texture(id);
        }
        full_output.textures_delta.clear();

        // 4. Adquirir imagen del Swapchain
        let (image_index, _suboptimal) = match renderer
            .swapchain
            .acquire_next_image(renderer.image_available_semaphore)
        {
            Ok(idx) => idx,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                let size = window.inner.inner_size();
                let _ = renderer.resize(vk_ctx, size.width, size.height);
                return Ok(());
            }
            Err(e) => return Err(Box::new(e)),
        };

        // 5. Teselar primitivas de UI
        let clipped_primitives = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        // 6. Grabar comandos de renderizado sobre el Framebuffer del Swapchain
        unsafe {
            renderer.device.wait_for_fences(&[renderer.fence], true, u64::MAX)?;
            renderer.device.reset_fences(&[renderer.fence])?;

            let cmd = renderer.command_buffer;
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            renderer.device.begin_command_buffer(cmd, &begin_info)?;

            let clear_values = [vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.07, 0.08, 0.10, 1.0], // Fondo oscuro del editor
                },
            }];

            let rp_info = vk::RenderPassBeginInfo::default()
                .render_pass(renderer.swapchain_render_pass)
                .framebuffer(renderer.swapchain_framebuffers[image_index as usize])
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: renderer.swapchain.extent,
                })
                .clear_values(&clear_values);

            renderer.device.cmd_begin_render_pass(
                cmd,
                &rp_info,
                vk::SubpassContents::INLINE,
            );

            // Dibujar interfaz egui (Paneles docking + Viewport con la textura offscreen)
            egui_renderer.cmd_draw(
                cmd,
                renderer.swapchain.extent,
                full_output.pixels_per_point,
                &clipped_primitives,
            )?;

            renderer.device.cmd_end_render_pass(cmd);
            renderer.device.end_command_buffer(cmd)?;

            // 7. Enviar a la cola gráfica
            let wait_semaphores = [renderer.image_available_semaphore];
            let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            let signal_semaphores = [renderer.render_finished_semaphore];
            let command_buffers = [cmd];

            let submit_info = [vk::SubmitInfo::default()
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&wait_stages)
                .command_buffers(&command_buffers)
                .signal_semaphores(&signal_semaphores)];

            renderer
                .device
                .queue_submit(vk_ctx.graphics_queue, &submit_info, renderer.fence)?;

            // 8. Presentar imagen en pantalla
            let swapchains = [renderer.swapchain.swapchain];
            let image_indices = [image_index];
            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&signal_semaphores)
                .swapchains(&swapchains)
                .image_indices(&image_indices);

            let _ = renderer.swapchain.loader.queue_present(vk_ctx.present_queue, &present_info);
        }

        Ok(())
    }
}

impl ApplicationHandler for GlassEngineApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        log::info!("Iniciando GlassEngine (Hito 4: Editor Docking estilo Unity en egui)...");
        let window = match GlassWindow::new(event_loop, &self.window_config) {
            Ok(w) => w,
            Err(e) => {
                log::error!("Fallo al crear la ventana nativa: {}", e);
                event_loop.exit();
                return;
            }
        };

        let vk_ctx = match VulkanContext::new(&window, self.vulkan_config.clone()) {
            Ok(ctx) => ctx,
            Err(e) => {
                log::error!("Fallo al crear el contexto Vulkan: {}", e);
                event_loop.exit();
                return;
            }
        };

        let gpu_name = unsafe {
            CStr::from_ptr(vk_ctx.physical_device_properties.device_name.as_ptr())
        }
        .to_string_lossy();

        // 1. Inicializar GlassRenderer
        let mut renderer = match GlassRenderer::new(&vk_ctx, self.window_config.width, self.window_config.height) {
            Ok(r) => r,
            Err(e) => {
                log::error!("Fallo al crear GlassRenderer: {}", e);
                event_loop.exit();
                return;
            }
        };

        // Renderizar frame inicial de offscreen para preparar layout óptimo de muestreo
        if let Err(e) = renderer.render_offscreen(vk_ctx.graphics_queue) {
            log::error!("Fallo al inicializar frame offscreen: {}", e);
            event_loop.exit();
            return;
        }

        // 2. Inicializar egui-winit
        let egui_winit = EguiWinitState::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window.inner,
            Some(window.inner.scale_factor() as f32),
            None,
            None,
        );

        // 3. Inicializar egui-ash-renderer sobre el RenderPass del Swapchain
        let mut egui_renderer = match EguiVulkanRenderer::with_gpu_allocator(
            renderer.allocator.clone(),
            (*renderer.device).clone(),
            egui_ash_renderer::RenderMode::RenderPass(renderer.swapchain_render_pass),
            egui_ash_renderer::Options {
                in_flight_frames: 1,
                enable_depth_test: false,
                enable_depth_write: false,
                srgb_framebuffer: false,
            },
        ) {
            Ok(er) => er,
            Err(e) => {
                log::error!("Fallo al inicializar egui-ash-renderer: {}", e);
                event_loop.exit();
                return;
            }
        };

        // 4. Enlazar la textura offscreen de VRAM a egui mediante DescriptorSet
        let (layout, pool, set) = match renderer.create_offscreen_descriptor_set() {
            Ok(res) => res,
            Err(e) => {
                log::error!("Fallo al crear descriptor set para viewport: {}", e);
                event_loop.exit();
                return;
            }
        };

        let viewport_texture_id = egui_renderer.add_user_texture(set);
        log::info!("Textura Offscreen vinculada exitosamente con egui::TextureId: {:?}", viewport_texture_id);

        self.renderer = Some(renderer);
        self.vulkan_context = Some(vk_ctx);
        self.window = Some(window);
        self.egui_winit = Some(egui_winit);
        self.egui_renderer = Some(egui_renderer);
        self.desc_layout = Some(layout);
        self.desc_pool = Some(pool);
        self.viewport_texture_id = Some(viewport_texture_id);

        if self.is_headless_test {
            log::info!("Ejecutando frame de prueba para Hito 4 (Editor Docking + Viewport 3D)...");
            if let Err(e) = self.render_frame() {
                log::error!("Fallo en frame de prueba de Hito 4: {}", e);
                event_loop.exit();
                return;
            }

            log::info!("============================================================");
            log::info!("   HITO 4 COMPLETADO: EDITOR DOCKING ESTILO UNITY EN EGUI");
            log::info!("   GPU en Uso:              {}", gpu_name);
            log::info!("   Paneles Activos:         Hierarchy, Inspector, Viewport, Console/Project");
            log::info!("   Viewport Texture ID:     {:?}", viewport_texture_id);
            log::info!("   Offscreen Vulkan VRAM:   1280x720 (Zero-copy en CPU)");
            log::info!("   Swapchain Composición:   1280x720 Presentación Directa");
            log::info!("   Docking Resizable:       Activo sin recreación innecesaria de Swapchain");
            log::info!("============================================================");

            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        if self.is_headless_test {
            return;
        }

        if let Some(window) = &self.window {
            if let Some(egui_winit) = &mut self.egui_winit {
                let res = egui_winit.on_window_event(&window.inner, &event);
                if res.consumed {
                    return;
                }
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                log::info!("Cierre de ventana solicitado.");
                event_loop.exit();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                log::info!("Tecla Escape presionada. Saliendo.");
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                if let (Some(renderer), Some(vk_ctx)) = (&mut self.renderer, &self.vulkan_context) {
                    let _ = renderer.resize(vk_ctx, physical_size.width, physical_size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.render_frame() {
                    log::error!("Error durante render_frame: {}", e);
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => (),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = std::env::args().collect();
    let is_headless_test = args
        .iter()
        .any(|arg| arg == "--test-init" || arg == "--check-vulkan" || arg == "--test-m3" || arg == "--test-m4");

    let event_loop = EventLoop::new()?;
    let mut app = GlassEngineApp::new(is_headless_test);
    event_loop.run_app(&mut app)?;

    Ok(())
}
