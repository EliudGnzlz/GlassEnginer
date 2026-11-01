use glass_core::{GlassWindow, WindowConfig};
use glass_renderer::{GlassRenderer, VulkanConfig, VulkanContext};
use std::ffi::CStr;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

struct GlassEngineApp {
    window_config: WindowConfig,
    vulkan_config: VulkanConfig,
    renderer: Option<GlassRenderer>,
    vulkan_context: Option<VulkanContext>,
    window: Option<GlassWindow>,
    is_headless_test: bool,
}

impl Drop for GlassEngineApp {
    fn drop(&mut self) {
        log::info!("Limpiando GlassEngineApp en orden estricto de dependencias...");
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
            is_headless_test,
        }
    }
}

impl ApplicationHandler for GlassEngineApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        log::info!("Iniciando GlassEngine (Hito 3: Swapchain, Render Passes & Offscreen Viewport)...");
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

        // Inicializar GlassRenderer con Viewport Offscreen desacoplado
        let mut renderer = match GlassRenderer::new(&vk_ctx, self.window_config.width, self.window_config.height) {
            Ok(r) => r,
            Err(e) => {
                log::error!("Fallo al crear GlassRenderer: {}", e);
                event_loop.exit();
                return;
            }
        };

        // Renderizar frame de prueba a la textura offscreen
        log::info!("Ejecutando renderizado de geometría de prueba a la textura offscreen...");
        match renderer.render_offscreen(vk_ctx.graphics_queue) {
            Ok(_) => {
                log::info!("============================================================");
                log::info!("   HITO 3 COMPLETADO: OFFSCREEN VIEWPORT RENDERIZADO CON ÉXITO");
                log::info!("   GPU en Uso:           {}", gpu_name);
                log::info!("   Swapchain Imágenes:   {} (Formato: {:?})", renderer.swapchain.images.len(), renderer.swapchain.format);
                log::info!("   Offscreen Viewport:   {}x{} (Formato: R8G8B8A8_UNORM)", renderer.offscreen.extent.width, renderer.offscreen.extent.height);
                log::info!("   Estado de Textura:    SHADER_READ_ONLY_OPTIMAL (Listo para egui en Hito 4)");
                log::info!("   Pipeline de Shaders:  SPIR-V (triangle.vert.spv + triangle.frag.spv)");
                log::info!("   Depth Testing:        D32_SFLOAT ACTIVO (LESS_OR_EQUAL)");
                log::info!("============================================================");
            }
            Err(e) => {
                log::error!("Fallo al renderizar en textura offscreen: {}", e);
                event_loop.exit();
                return;
            }
        }

        self.renderer = Some(renderer);
        self.vulkan_context = Some(vk_ctx);
        self.window = Some(window);

        if self.is_headless_test {
            log::info!("Prueba de Hito 3 completada con éxito. Cerrando ciclo de eventos.");
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
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
            WindowEvent::RedrawRequested => {
                if let (Some(renderer), Some(vk_ctx)) = (&mut self.renderer, &self.vulkan_context) {
                    let _ = renderer.render_offscreen(vk_ctx.graphics_queue);
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
    let is_headless_test = args.iter().any(|arg| arg == "--test-init" || arg == "--check-vulkan" || arg == "--test-m3");

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = GlassEngineApp::new(is_headless_test);
    event_loop.run_app(&mut app)?;

    Ok(())
}
