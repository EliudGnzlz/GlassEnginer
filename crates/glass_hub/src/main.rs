//! Cáscara del binario `glass_hub`: ventana + Vulkan + egui, sin GlassRenderer ni escena viva.
//!
//! PENDIENTES (no venían en la especificación; revisar antes de dar por cerrado):
//! - `SceneState`: se asume en `glass_core::scene::SceneState`. Si vive en otro módulo, cambia
//!   solo el `use` marcado con SUPUESTO.
//! - `construir_escena_plataformas` y `construir_escena_fisica`: sus cuerpos no se incluyeron
//!   (el mensaje se cortó). Ahora construyen la escena vacía y lo avisan con log::warn!.
//!   Hay que pegar los cuerpos originales de glass_app.
//! - `clave_de_sample`: el cuerpo original no se incluyó. Se usa el nombre `Debug` del preset.
//! - Desviaciones deliberadas respecto al texto de la especificación, necesarias por corrección:
//!   el fence se crea SEÑALIZADO (esperar uno sin señalizar en el primer frame cuelga el
//!   programa); con SUBOPTIMAL en `acquire` se dibuja y se presenta igualmente, y se marca la
//!   recreación (si no, `image_available` quedaría señalizado sin consumir); las texturas usan
//!   `set_textures`/`free_textures` sobre los slices de `TexturesDelta`, porque `Vec::drain()`
//!   sin rango no compila.

// En Windows, el Hub es una aplicación de ventana: sin consola (la CMD
// que el probador veía al arrancar). Si se invoca con argumentos desde
// una terminal, main adjunta la consola del padre para que el error de
// argumentos se lea igual que antes.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ash::vk;
use egui_ash_renderer::allocator::GpuAllocator;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

use glass_core::scene::SceneState; // SUPUESTO: ruta de SceneState.
use glass_core::window::{GlassWindow, WindowConfig};
use glass_hub::launcher::{LauncherAction, LauncherPanel, ProjectTemplate, SampleScenePreset};
use glass_launcher::updates::AccionHub;
use glass_renderer::context::{VulkanConfig, VulkanContext};
use glass_renderer::swapchain::{SwapchainContext, SwapchainTarget};

/// Mantenimiento de instalaciones al arrancar el Hub (S4): la
/// reconciliación de huérfanas, los borrados diferidos y la
/// limpieza del respaldo viven SOLO en el proceso del Hub (un
/// motor hijo no escribe engines.json, D1). Antes vivía en el
/// modo Hub de glass_app; con la separación verdadera es de
/// este binario.
fn mantenimiento_instalaciones(tras_actualizar: bool) {
    let huerfanas = glass_launcher::updates::reconciliar_huerfanas();
    if !huerfanas.is_empty() {
        log::info!(
            "Instalaciones huérfanas reconciliadas como pendientes de borrar: {:?}",
            huerfanas
        );
    }
    let retiradas = glass_launcher::updates::procesar_pendientes_borrar();
    if tras_actualizar {
        glass_hub::launcher::anunciar_arranque_tras_actualizacion_hub();
    }
    glass_launcher::updates::limpiar_respaldo_hub();
    if !retiradas.is_empty() {
        log::info!("Borrados diferidos completados al arrancar: {:?}", retiradas);
    }
}

/// Valida los argumentos del Hub: cada binario valida solo los suyos.
/// El Hub acepta únicamente `--hub` (no-op por compatibilidad con los
/// lanzadores antiguos que lo relanzaban así) y `--tras-actualizar-hub`
/// (anuncia el arranque tras una actualización propia). Los flags del
/// motor (`--project`, `--scene`, `--sample`, `--standalone`, `--game`,
/// `--mcp`, `--mcp-stdio`, `--mcp-live` y los modos de prueba
/// `--test-*`/`--check-vulkan`) se rechazan con un mensaje que
/// señala al motor; cualquier otro argumento que empiece por `--` se
/// rechaza como desconocido para el Hub. Los argumentos que no empiezan
/// por `--` también se rechazan (el Hub no recibe rutas sueltas).
/// `args` es `std::env::args()` completo (argv[0] incluido): se salta.
fn validar_argumentos_hub(args: &[String]) -> Result<(), String> {
    const FLAGS_DEL_MOTOR: &[&str] = &[
        "--project",
        "--scene",
        "--sample",
        "--standalone",
        "--game",
        "--mcp",
        "--mcp-stdio",
        "--mcp-live",
        // Modos de prueba/headless del motor (misma familia que los
        // flags de arriba: pertenecen a glass_app, no al Hub).
        "--test-init",
        "--check-vulkan",
        "--test-m3",
        "--test-m4",
        "--test-m5",
        "--test-m6",
        "--test-m7",
        "--test-m8",
        "--test-m9",
        "--test-m10",
        "--test-standalone",
        "--test-sample-game",
        "--test-platformer",
    ];

    for arg in args.iter().skip(1) {
        let arg = arg.as_str();
        match arg {
            "--hub" | "--tras-actualizar-hub" => {}
            _ if arg.starts_with("--") => {
                let flag = arg.split_once('=').map_or(arg, |(nombre, _)| nombre);
                if FLAGS_DEL_MOTOR.contains(&flag) {
                    return Err(format!(
                        "el argumento {flag} es del motor (glass_app), no del Hub: el Hub lanza el motor instalado, no abre proyectos directamente."
                    ));
                }
                return Err(format!(
                    "argumento desconocido para el Hub: {arg} (el Hub solo acepta --hub y --tras-actualizar-hub)."
                ));
            }
            _ => {
                return Err(format!(
                    "argumento desconocido para el Hub: {arg} (el Hub solo acepta --hub y --tras-actualizar-hub)."
                ));
            }
        }
    }
    Ok(())
}

/// Adjunta la consola del proceso padre (solo Windows) para que los
/// mensajes de error se vean cuando el Hub se invoca desde una
/// terminal. Sin consola padre (doble clic, actualizador, el propio
/// motor al relanzar el Hub) la llamada falla en silencio y no aparece
/// ninguna ventana: es justo lo que se quiere.
#[cfg(target_os = "windows")]
fn adjuntar_consola_del_padre() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(dwProcessId: u32) -> i32;
    }
    // ATTACH_PARENT_PROCESS vale (DWORD)-1.
    let _ = unsafe { AttachConsole(u32::MAX) };
}

#[cfg(not(target_os = "windows"))]
fn adjuntar_consola_del_padre() {}

fn main() {
    // Con argumentos hay una terminal (o un lanzador) detrás: en
    // Windows se adjunta su consola ANTES de inicializar el log y de
    // validar, para que un error de argumentos no quede mudo. Sin
    // argumentos (arranque gráfico normal) no se toca nada.
    if std::env::args_os().len() > 1 {
        adjuntar_consola_del_padre();
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = std::env::args().collect();
    if let Err(mensaje) = validar_argumentos_hub(&args) {
        log::error!("{mensaje}");
        eprintln!("{mensaje}");
        std::process::exit(2);
    }
    let tras_actualizar = args.iter().any(|a| a == "--tras-actualizar-hub");

    // Instancia única (Bug A): si ya hay un Hub vivo (visible o en
    // segundo plano), esta instancia le pide que se muestre y sale;
    // el mantenimiento de instalaciones lo hace solo el Hub vivo.
    // Tras una auto-actualización, el Hub viejo aún puede tener el
    // bloqueo unos instantes: se reintenta hasta ~5 s.
    let intentos: u32 = if tras_actualizar { 50 } else { 1 };
    let mut bloqueo = None;
    for intento in 0..intentos {
        if let Some(b) = glass_launcher::updates::tomar_bloqueo_hub() {
            bloqueo = Some(b);
            break;
        }
        if intento + 1 < intentos {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let Some(bloqueo) = bloqueo else {
        let avisado = glass_launcher::updates::solicitar_despertar_hub();
        log::info!(
            "Ya hay un GlassEnginer Hub en ejecución: se le pide que se muestre (bandera escrita: {avisado}); esta instancia sale."
        );
        return;
    };
    // Una bandera de despertar vieja (de una sesión anterior) no
    // debe provocar un «mostrar» espurio al arrancar.
    let _ = glass_launcher::updates::consumir_despertar_hub();

    mantenimiento_instalaciones(tras_actualizar);

    #[allow(unused_mut)]
    let mut builder = EventLoop::builder();
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        let forzar_x11 = std::env::var("WINIT_UNIX_BACKEND")
            .map(|v| v == "x11")
            .unwrap_or(false)
            || std::env::var_os("DISPLAY").is_some();
        if forzar_x11 {
            log::info!("Forzando backend X11/XWayland en Winit…");
            builder.with_x11();
        }
    }

    let event_loop = match builder.build() {
        Ok(el) => el,
        Err(e) => {
            log::error!("No se pudo crear el EventLoop: {e:?}");
            return;
        }
    };

    let mut app = HubApp {
        estado: None,
        bloqueo_hub: Some(bloqueo),
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("El bucle de eventos terminó con error: {e:?}");
    }
}

/// Bandeja del sistema (solo Windows): icono con menú «Abrir
/// GlassEnginer Hub» / «Salir». En Linux no hay bandeja: el
/// respaldo es ocultar al cerrar + despertar al relanzar
/// (instancia única) + el botón «Salir del Hub» del panel.
#[cfg(target_os = "windows")]
mod bandeja {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    const ID_ABRIR: &str = "abrir";
    const ID_SALIR: &str = "salir";

    /// Icono de bandeja del Hub. Soltarlo retira el icono.
    pub struct Bandeja {
        _icono: TrayIcon,
        _abrir: MenuItem,
        _salir: MenuItem,
    }

    impl Bandeja {
        pub fn crear() -> Result<Self, String> {
            let icono = cargar_icono()?;
            let menu = Menu::new();
            let abrir = MenuItem::with_id(ID_ABRIR, "Abrir GlassEnginer Hub", true, None);
            let separador = PredefinedMenuItem::separator();
            let salir = MenuItem::with_id(ID_SALIR, "Salir", true, None);
            menu.append_items(&[&abrir, &separador, &salir])
                .map_err(|e| format!("no se pudo montar el menú de bandeja: {e}"))?;
            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("GlassEnginer Hub")
                .with_icon(icono)
                .build()
                .map_err(|e| format!("no se pudo crear el icono de bandeja: {e}"))?;
            Ok(Self {
                _icono: tray,
                _abrir: abrir,
                _salir: salir,
            })
        }

        /// Vacía las colas de eventos de bandeja. Devuelve (abrir, salir).
        pub fn sondear(&self) -> (bool, bool) {
            let mut abrir = false;
            let mut salir = false;
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                match ev.id.0.as_str() {
                    ID_ABRIR => abrir = true,
                    ID_SALIR => salir = true,
                    _ => {}
                }
            }
            while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                match ev {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                    | TrayIconEvent::DoubleClick { .. } => abrir = true,
                    _ => {}
                }
            }
            (abrir, salir)
        }
    }

    fn cargar_icono() -> Result<Icon, String> {
        let bytes = include_bytes!("../../../installer/glassenginer.png");
        let img = image::load_from_memory(bytes)
            .map_err(|e| format!("no se pudo decodificar el icono: {e}"))?;
        let img = if img.width() > 32 || img.height() > 32 {
            img.resize(32, 32, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };
        let rgba = img.into_rgba8();
        let (w, h) = rgba.dimensions();
        Icon::from_rgba(rgba.into_raw(), w, h).map_err(|e| format!("icono de bandeja no válido: {e}"))
    }
}

struct HubApp {
    estado: Option<Estado>,
    /// Lock de instancia única: vive lo que el proceso; al soltarlo
    /// (salida) otro Hub puede arrancar.
    #[allow(dead_code)]
    bloqueo_hub: Option<glass_launcher::updates::BloqueoHub>,
}

// El orden de los campos fija el orden de Drop: primero lo que depende del device,
// después allocator, device, contexto Vulkan y la ventana.
struct Estado {
    /// Motores lanzados por este Hub que siguen vivos (los cosecha
    /// `about_to_wait` con try_wait; sustituye al hilo recoge-zombis).
    motores: Vec<std::process::Child>,
    /// Ventana cerrada por el usuario: el Hub sigue en segundo plano.
    oculto: bool,
    /// «Salir» pedido con motores abiertos: el proceso sale cuando
    /// el registro de motores se vacíe.
    saliendo: bool,
    /// Icono de bandeja (solo Windows; en el resto no hay bandeja y
    /// el respaldo es relanzar el Hub para despertarlo).
    #[cfg(target_os = "windows")]
    bandeja: Option<bandeja::Bandeja>,
    egui_renderer: egui_ash_renderer::Renderer<GpuAllocator>,
    panel: LauncherPanel,
    egui_winit: egui_winit::State,
    egui_ctx: egui::Context,
    swapchain: SwapchainContext,
    render_pass: vk::RenderPass,
    framebuffers: Vec<vk::Framebuffer>,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    fence: vk::Fence,
    image_available: vk::Semaphore,
    render_finished: Vec<vk::Semaphore>,
    resize_pendiente: Option<(u32, u32)>,
    _allocator: Arc<Mutex<gpu_allocator::vulkan::Allocator>>,
    device: Arc<ash::Device>,
    vk_ctx: VulkanContext,
    window: GlassWindow,
}

impl ApplicationHandler for HubApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.estado.is_some() {
            return;
        }
        match Estado::crear(event_loop) {
            Ok(estado) => self.estado = Some(estado),
            Err(e) => {
                log::error!("No se pudo inicializar el Hub: {e}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        let Some(estado) = self.estado.as_mut() else {
            return;
        };
        let _ = estado.egui_winit.on_window_event(&estado.window.inner, &event);
        match event {
            WindowEvent::CloseRequested => {
                // Como Steam: cerrar la ventana deja el Hub en
                // segundo plano; la salida real es «Salir» (botón
                // del panel o menú de la bandeja). El motor lanzado
                // sigue gestionado por este proceso.
                estado.ocultar();
                log::info!("Ventana del Hub cerrada: el Hub sigue en segundo plano (relánzalo para mostrarlo o usa «Salir»).");
            }
            WindowEvent::Resized(size) => {
                estado.resize_pendiente = Some((size.width, size.height));
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = estado.frame(event_loop) {
                    // Un error a mitad del frame deja la sincronización
                    // (fence, semáforos) en estado desconocido: seguir
                    // dibujando podría bloquear el siguiente wait o
                    // reutilizar un semáforo señalizado. Como en
                    // glass_app, el error de render es fatal: se anota
                    // y el proceso sale (auditoría V1).
                    log::error!("Error al dibujar el frame del Hub: {e}");
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(estado) = self.estado.as_mut() else {
            return;
        };

        // Cosecha de motores terminados (sustituye al hilo que hacía
        // wait() de cada hijo: sin zombis y con el registro al día).
        estado.motores.retain_mut(|motor| match motor.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                log::info!("Motor (pid {}) terminado: {status}", motor.id());
                false
            }
            Err(e) => {
                log::warn!(
                    "No se puede vigilar el motor (pid {}): {e}; se retira del registro",
                    motor.id()
                );
                false
            }
        });

        if estado.saliendo && estado.motores.is_empty() {
            log::info!("Todos los motores cerrados: el Hub sale.");
            event_loop.exit();
            return;
        }

        if glass_launcher::updates::consumir_despertar_hub() {
            log::info!("Petición de despertar recibida: se muestra el Hub.");
            estado.reabrir();
        }

        #[cfg(target_os = "windows")]
        {
            let (abrir, salir) = estado
                .bandeja
                .as_ref()
                .map_or((false, false), |b| b.sondear());
            if abrir {
                estado.reabrir();
            }
            if salir {
                estado.iniciar_salir(event_loop);
                if event_loop.exiting() {
                    return;
                }
            }
        }

        if estado.oculto {
            // Reposo real en segundo plano: sin dibujar; el bucle se
            // despierta cada 200 ms para las sondas de arriba.
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(200),
            ));
        } else {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
            estado.window.inner.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(estado) = self.estado.as_ref() {
            let _ = unsafe { estado.device.device_wait_idle() };
        }
    }
}

impl Estado {
    fn crear(event_loop: &ActiveEventLoop) -> Result<Self, String> {
        let window = GlassWindow::new(
            event_loop,
            &WindowConfig {
                title: "GlassEnginer Hub".into(),
                width: 1280,
                height: 720,
                resizable: true,
                vsync: true,
            },
        )
        .map_err(|e| format!("No se pudo crear la ventana: {e:?}"))?;

        let vk_ctx = VulkanContext::new(&window, VulkanConfig::default())
            .map_err(|e| format!("No se pudo crear el contexto Vulkan: {e:?}"))?;

        let device = Arc::new(unsafe {
            ash::Device::load(vk_ctx.instance.fp_v1_0(), vk_ctx.device.handle())
        });

        let allocator = Arc::new(Mutex::new(
            gpu_allocator::vulkan::Allocator::new(&gpu_allocator::vulkan::AllocatorCreateDesc {
                instance: vk_ctx.instance.clone(),
                device: vk_ctx.device.clone(),
                physical_device: vk_ctx.physical_device,
                debug_settings: Default::default(),
                buffer_device_address: false,
                allocation_sizes: Default::default(),
            })
            .map_err(|e| format!("No se pudo crear el allocator de GPU: {e:?}"))?,
        ));

        let size = window.inner.inner_size();
        let swapchain = SwapchainContext::new(
            &vk_ctx.instance,
            device.clone(),
            SwapchainTarget {
                surface_loader: &vk_ctx.surface_loader,
                surface: vk_ctx.surface,
                physical_device: vk_ctx.physical_device,
                width: size.width,
                height: size.height,
                old_swapchain: None,
            },
        )
        .map_err(|e| format!("No se pudo crear la swapchain: {e:?}"))?;

        let render_pass = create_swapchain_render_pass(&device, swapchain.format)
            .map_err(|e| format!("No se pudo crear el render pass: {e:?}"))?;
        let framebuffers =
            create_swapchain_framebuffers(&device, render_pass, &swapchain.image_views, swapchain.extent)
                .map_err(|e| format!("No se pudieron crear los framebuffers: {e:?}"))?;

        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(vk_ctx.queue_families.graphics_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&pool_info, None) }
            .map_err(|e| format!("No se pudo crear el command pool: {e:?}"))?;

        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let command_buffer = unsafe { device.allocate_command_buffers(&alloc_info) }
            .map_err(|e| format!("No se pudo reservar el command buffer: {e:?}"))?[0];

        // Señalizado: el primer frame espera este fence antes de grabar.
        let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
        let fence = unsafe { device.create_fence(&fence_info, None) }
            .map_err(|e| format!("No se pudo crear el fence: {e:?}"))?;

        let image_available = unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }
            .map_err(|e| format!("No se pudo crear el semáforo image_available: {e:?}"))?;
        let render_finished = crear_semaforos(&device, swapchain.images.len())?;

        let egui_ctx = egui::Context::default();
        glass_launcher::setup_editor_fonts(&egui_ctx);
        let scale_factor = window.inner.scale_factor();
        let egui_winit = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window.inner,
            Some(scale_factor as f32),
            None,
            None,
        );

        let egui_renderer = egui_ash_renderer::Renderer::with_gpu_allocator(
            allocator.clone(),
            (*device).clone(),
            egui_ash_renderer::RenderMode::RenderPass(render_pass),
            egui_ash_renderer::Options {
                in_flight_frames: 1,
                enable_depth_test: false,
                enable_depth_write: false,
                srgb_framebuffer: true,
            },
        )
        .map_err(|e| format!("No se pudo crear el renderer de egui: {e:?}"))?;

        let mut panel = LauncherPanel::new();
        // Este binario ES el Hub: el panel nace en personalidad Hub
        // (gestiona el motor registrado, no a sí mismo como combinado).
        panel.es_hub = true;
        panel.gpu_aviso = vk_ctx.gpu_aviso.clone();

        // Bandeja del sistema (hilo principal, requisito de tray-icon
        // en Windows). Sin bandeja el Hub funciona igual: cerrar la
        // ventana lo oculta y relanzarlo lo despierta.
        #[cfg(target_os = "windows")]
        let bandeja = match bandeja::Bandeja::crear() {
            Ok(b) => Some(b),
            Err(e) => {
                log::warn!("No se pudo crear el icono de bandeja del Hub: {e}");
                None
            }
        };

        Ok(Self {
            motores: Vec::new(),
            oculto: false,
            saliendo: false,
            #[cfg(target_os = "windows")]
            bandeja,
            egui_renderer,
            panel,
            egui_winit,
            egui_ctx,
            swapchain,
            render_pass,
            framebuffers,
            command_pool,
            command_buffer,
            fence,
            image_available,
            render_finished,
            resize_pendiente: None,
            _allocator: allocator,
            device,
            vk_ctx,
            window,
        })
    }

    fn recrear_swapchain(&mut self, width: u32, height: u32) -> Result<(), String> {
        unsafe { self.device.device_wait_idle() }
            .map_err(|e| format!("device_wait_idle falló: {e:?}"))?;
        for fb in self.framebuffers.drain(..) {
            unsafe { self.device.destroy_framebuffer(fb, None) };
        }
        for s in self.render_finished.drain(..) {
            unsafe { self.device.destroy_semaphore(s, None) };
        }

        let nueva = SwapchainContext::new(
            &self.vk_ctx.instance,
            self.device.clone(),
            SwapchainTarget {
                surface_loader: &self.vk_ctx.surface_loader,
                surface: self.vk_ctx.surface,
                physical_device: self.vk_ctx.physical_device,
                width,
                height,
                old_swapchain: Some(self.swapchain.swapchain),
            },
        )
        .map_err(|e| format!("No se pudo recrear la swapchain: {e:?}"))?;
        // El Drop de la vieja destruye sus vistas y su swapchain.
        self.swapchain = nueva;

        self.framebuffers = create_swapchain_framebuffers(
            &self.device,
            self.render_pass,
            &self.swapchain.image_views,
            self.swapchain.extent,
        )
        .map_err(|e| format!("No se pudieron recrear los framebuffers: {e:?}"))?;
        self.render_finished = crear_semaforos(&self.device, self.swapchain.images.len())?;
        Ok(())
    }

    fn tamano_ventana(&self) -> (u32, u32) {
        let s = self.window.inner.inner_size();
        (s.width, s.height)
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        if self.oculto {
            return Ok(());
        }
        if let Some((w, h)) = self.resize_pendiente {
            if w == 0 || h == 0 {
                return Ok(());
            }
            self.recrear_swapchain(w, h)?;
            self.resize_pendiente = None;
        }

        let raw_input = self.egui_winit.take_egui_input(&self.window.inner);
        let mut accion = None;
        let panel = &mut self.panel;
        let mut full_output = self.egui_ctx.run_ui(raw_input, |ui| {
            accion = panel.render(ui);
        });
        self.egui_winit
            .handle_platform_output(&self.window.inner, full_output.platform_output);

        if let Some(action) = accion {
            if let Some(hijo) = ejecutar_accion_hub(&mut self.panel, action) {
                self.motores.push(hijo);
            }
            self.egui_ctx.request_repaint();
        }

        if let Some(accion_hub) = self.panel.accion_hub_pendiente.take() {
            let lanzado = match accion_hub {
                AccionHub::Relanzar { programa } => Some(
                    std::process::Command::new(&programa)
                        .args(["--tras-actualizar-hub"])
                        .spawn(),
                ),
                AccionHub::LanzarInstalador { setup } => {
                    Some(std::process::Command::new(&setup).spawn())
                }
                #[allow(unreachable_patterns)]
                _ => None,
            };
            match lanzado {
                Some(Ok(_)) => {
                    event_loop.exit();
                    return Ok(());
                }
                Some(Err(e)) => {
                    log::error!("No se pudo reiniciar el Hub: {e}");
                    self.panel.hub_update_state = glass_hub::launcher::UpdateUiState::Error(format!(
                        "No se pudo reiniciar el Hub: {e}"
                    ));
                }
                None => {}
            }
        }

        if self.panel.solicita_salir {
            self.panel.solicita_salir = false;
            // El frame sigue hasta el final a propósito: cortarlo aquí
            // perdería los textures_delta de egui de este frame.
            self.iniciar_salir(event_loop);
        }

        // Un solo frame en vuelo: tras esperar el fence la GPU ya no usa nada del frame anterior.
        unsafe { self.device.wait_for_fences(&[self.fence], true, u64::MAX) }
            .map_err(|e| format!("wait_for_fences falló: {e:?}"))?;

        for (id, deltas) in full_output.textures_delta.set.drain() {
            for delta in deltas {
                self.egui_renderer
                    .set_texture(self.vk_ctx.graphics_queue, self.command_pool, id, &delta)
                    .map_err(|e| format!("set_texture falló: {e:?}"))?;
            }
        }
        for id in full_output.textures_delta.free.drain() {
            let _ = self.egui_renderer.free_texture(id);
        }
        full_output.textures_delta.clear();

        let (image_index, suboptimo) = match self.swapchain.acquire_next_image(self.image_available) {
            Ok(r) => r,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR)
            | Err(vk::Result::SUBOPTIMAL_KHR)
            | Err(vk::Result::ERROR_SURFACE_LOST_KHR) => {
                let (w, h) = self.tamano_ventana();
                if w > 0 && h > 0 {
                    self.recrear_swapchain(w, h)?;
                } else {
                    self.resize_pendiente = Some((w, h));
                }
                return Ok(());
            }
            Err(e) => return Err(format!("acquire_next_image falló: {e:?}")),
        };
        if suboptimo {
            self.resize_pendiente = Some(self.tamano_ventana());
        }

        let clipped = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        let cmd = self.command_buffer;
        let extent = self.swapchain.extent;
        unsafe {
            self.device
                .reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
                .map_err(|e| format!("reset_command_buffer falló: {e:?}"))?;
            let begin = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device
                .begin_command_buffer(cmd, &begin)
                .map_err(|e| format!("begin_command_buffer falló: {e:?}"))?;

            let clear = [vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.07, 0.08, 0.10, 1.0],
                },
            }];
            let rp_begin = vk::RenderPassBeginInfo::default()
                .render_pass(self.render_pass)
                .framebuffer(self.framebuffers[image_index as usize])
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent,
                })
                .clear_values(&clear);
            self.device
                .cmd_begin_render_pass(cmd, &rp_begin, vk::SubpassContents::INLINE);
        }

        self.egui_renderer
            .cmd_draw(cmd, extent, full_output.pixels_per_point, &clipped)
            .map_err(|e| format!("cmd_draw falló: {e:?}"))?;

        unsafe {
            self.device.cmd_end_render_pass(cmd);
            self.device
                .end_command_buffer(cmd)
                .map_err(|e| format!("end_command_buffer falló: {e:?}"))?;
        }

        let wait = [self.image_available];
        let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let cmds = [cmd];
        let signal = [self.render_finished[image_index as usize]];
        let submit = vk::SubmitInfo::default()
            .wait_semaphores(&wait)
            .wait_dst_stage_mask(&stages)
            .command_buffers(&cmds)
            .signal_semaphores(&signal);
        unsafe {
            // El fence se reinicia justo antes de enviar: si la grabación falla, sigue señalizado
            // y el siguiente frame no se cuelga esperándolo.
            self.device
                .reset_fences(&[self.fence])
                .map_err(|e| format!("reset_fences falló: {e:?}"))?;
            self.device
                .queue_submit(self.vk_ctx.graphics_queue, &[submit], self.fence)
                .map_err(|e| format!("queue_submit falló: {e:?}"))?;
        }

        let swapchains = [self.swapchain.swapchain];
        let indices = [image_index];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&signal)
            .swapchains(&swapchains)
            .image_indices(&indices);
        match unsafe {
            self.swapchain
                .loader
                .queue_present(self.vk_ctx.present_queue, &present_info)
        } {
            Ok(false) => {}
            Ok(true) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                self.resize_pendiente = Some(self.tamano_ventana());
            }
            Err(e) => log::warn!("queue_present falló: {e:?}"),
        }

        Ok(())
    }
}

impl Estado {
    /// Cerrar la ventana: el Hub queda en segundo plano.
    fn ocultar(&mut self) {
        self.window.inner.set_visible(false);
        self.oculto = true;
    }

    fn mostrar(&mut self) {
        self.window.inner.set_visible(true);
        self.window.inner.focus_window();
        self.oculto = false;
        self.window.inner.request_redraw();
    }

    /// Mostrar a petición del usuario (relanzar el Hub o «Abrir» en
    /// la bandeja). Si había un «Salir» esperando a los motores, se
    /// cancela: las peticiones de cierre ya salieron, pero si un
    /// motor sigue abierto tras su diálogo, el Hub vuelve a estar
    /// disponible (semántica decidida del Bug A).
    fn reabrir(&mut self) {
        if self.saliendo {
            self.saliendo = false;
            log::info!("Salida del Hub cancelada: el usuario volvió a abrir el Hub.");
        }
        self.mostrar();
    }

    /// Salida real del Hub: pide a cada motor en curso que se cierre
    /// con su regla de guardado y sale cuando no quede ninguno.
    /// Nunca mata un motor: si uno no sale (su usuario canceló el
    /// guardado), el Hub se queda en segundo plano a la espera.
    fn iniciar_salir(&mut self, event_loop: &ActiveEventLoop) {
        for motor in &self.motores {
            let pid = motor.id();
            if glass_launcher::updates::solicitar_cierre_motor(pid) {
                log::info!("Pedido el cierre del motor (pid {pid}).");
            } else {
                log::warn!("No se pudo pedir el cierre del motor (pid {pid}).");
            }
        }
        if self.motores.is_empty() {
            log::info!("Saliendo del Hub.");
            event_loop.exit();
            return;
        }
        log::info!(
            "Saliendo del Hub: se espera en segundo plano a que cierren {} motor(es).",
            self.motores.len()
        );
        self.saliendo = true;
        self.ocultar();
    }
}

fn crear_semaforos(device: &ash::Device, n: usize) -> Result<Vec<vk::Semaphore>, String> {
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let s = unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }
            .map_err(|e| format!("No se pudo crear el semáforo render_finished: {e:?}"))?;
        v.push(s);
    }
    Ok(v)
}

fn create_swapchain_render_pass(device: &ash::Device, format: vk::Format) -> Result<vk::RenderPass, vk::Result> {
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

fn create_swapchain_framebuffers(device: &ash::Device, render_pass: vk::RenderPass, views: &[vk::ImageView], extent: vk::Extent2D) -> Result<Vec<vk::Framebuffer>, vk::Result> {
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

// ---------------------------------------------------------------------------
// Despacho de acciones del panel (adaptado de glass_app).
// ---------------------------------------------------------------------------

fn absolutizar(ruta: &Path) -> PathBuf {
    if ruta.is_absolute() {
        ruta.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(ruta))
            .unwrap_or_else(|_| ruta.to_path_buf())
    }
}

// PENDIENTE: sustituir por el cuerpo original de glass_app (no venía en la especificación).
fn clave_de_sample(preset: SampleScenePreset) -> String {
    format!("{preset:?}")
}

fn crear_proyecto_en_disco(
    name: &str,
    path: &Path,
    template: ProjectTemplate,
) -> Result<std::path::PathBuf, String> {
    // Validación de nombre y carpeta destino (espejo del Unity Hub:
    // la carpeta debe ser única en la ubicación; crear sobre un
    // proyecto existente sobrescribía su escena y su project.glass).
    // `name` queda sombreado por el nombre recortado y `project_dir`
    // es la carpeta ya resuelta; no se vuelve a derivar.
    let (nombre_recortado, project_dir) =
        glass_launcher::archivos::validar_creacion_proyecto(name, path)?;
    let name: &str = nombre_recortado.as_str();
    // Estructura de carpetas tipo Unity.
    for sub in [
        "Assets/Scenes",
        "Assets/Scripts",
        "Assets/Materials",
        "Assets/Models",
        "Assets/Textures",
        "ProjectSettings",
        "Packages",
    ] {
        std::fs::create_dir_all(project_dir.join(sub))
            .map_err(|e| format!("No se pudo crear {:?}: {}", project_dir.join(sub), e))?;
    }

    // Escena de la plantilla en un SceneState desechable: ni la escena
    // viva ni el runtime se tocan. Assets/Scenes ya existe (save_to_json
    // no crea carpetas).
    let mut state = SceneState::new();
    match template {
        ProjectTemplate::Platformer3D => {
            construir_escena_plataformas(&mut state);
        }
        ProjectTemplate::PhysicsSandbox => {
            construir_escena_fisica(&mut state);
        }
        ProjectTemplate::EmptyScene => construir_escena_vacia(&mut state),
    }
    let scene_path = project_dir.join("Assets/Scenes/SampleScene.scene.json");
    state
        .save_to_json(&scene_path.to_string_lossy())
        .map_err(|e| {
            format!(
                "No se pudo guardar la escena del proyecto ({}): {}",
                scene_path.display(),
                e
            )
        })?;

    let meta = format!(
        "{{\n  \"name\": \"{}\",\n  \"version\": \"1.0.0\",\n  \"engine\": \"{}\",\n  \"template\": \"{:?}\",\n  \"scene\": \"Assets/Scenes/SampleScene.scene.json\"\n}}\n",
        name,
        glass_launcher::updates::ENGINE_VERSION,
        template
    );
    std::fs::write(project_dir.join("project.glass"), meta)
        .map_err(|e| format!("No se pudo escribir project.glass: {}", e))?;
    Ok(project_dir)
}

/// Construye en `state` la escena vacia: camara principal y luz
/// solar. Sin suelo, fisica ni scripts.
fn construir_escena_vacia(state: &mut SceneState) {
    state.entities.clear();
    let cam_id = state.create_entity(
        "Main Camera".into(),
        vec!["Transform".into(), "Camera".into()],
    );
    state.modify_transform(cam_id, Some([0.0, 3.5, 9.0]), Some([-14.0, 0.0, 0.0]), Some([1.0, 1.0, 1.0]));
    let sun_id = state.create_entity(
        "Sun (Directional Light)".into(),
        vec!["Transform".into(), "Light".into()],
    );
    state.modify_transform(sun_id, Some([0.0, 8.0, 0.0]), Some([-55.0, 40.0, 0.0]), Some([1.0, 1.0, 1.0]));
}

// PENDIENTE: pegar el cuerpo original de glass_app (no venía en la especificación).
fn construir_escena_plataformas(state: &mut SceneState) {
    log::warn!("Plantilla Platformer3D pendiente de portar al Hub: se crea la escena vacía");
    construir_escena_vacia(state);
}

// PENDIENTE: pegar el cuerpo original de glass_app (no venía en la especificación).
fn construir_escena_fisica(state: &mut SceneState) {
    log::warn!("Plantilla PhysicsSandbox pendiente de portar al Hub: se crea la escena vacía");
    construir_escena_vacia(state);
}

/// Lanza el motor gestionado en uso como proceso hijo con los
/// argumentos dados (S6). El hijo comparte la raíz de datos
/// (GLASS_LAUNCHER_HOME) pero NO hereda los overrides GLASS_*
/// de prueba/desarrollo del entorno del Hub (p. ej.
/// GLASS_UPDATE_API): se retiran todos menos
/// GLASS_LAUNCHER_HOME (auditoría S6). Devuelve el hijo para que
/// el Hub lo registre y lo coseche en about_to_wait (sin zombis).
fn lanzar_motor_hijo(panel: &mut LauncherPanel, args: &[&str]) -> Option<std::process::Child> {
    match glass_launcher::updates::objetivo_sonda() {
        Some((exe, _)) => {
            let mut cmd = std::process::Command::new(&exe);
            cmd.args(args);
            for (k, _) in std::env::vars() {
                if k.starts_with("GLASS_") && k != "GLASS_LAUNCHER_HOME" {
                    cmd.env_remove(&k);
                }
            }
            // El hijo es el ejecutable del motor en engines/, no el AppImage del Hub:
            // si hereda APPIMAGE, cualquier lógica que localice «su» ejecutable apuntaría al Hub.
            cmd.env_remove("APPIMAGE");
            cmd.env_remove("LD_LIBRARY_PATH");
            cmd.env_remove("LD_PRELOAD");
            // APPDIR (la raíz del montaje del AppImage del Hub) es
            // el mismo problema por otra puerta (auditoría S10).
            cmd.env_remove("APPDIR");
            match cmd.spawn() {
                Ok(hijo) => {
                    log::info!("Motor lanzado ({}, pid {}) con {:?}", exe.display(), hijo.id(), args);
                    Some(hijo)
                }
                Err(e) => {
                    panel.status_message = Some(format!("⚠ No se pudo lanzar el motor: {e}"));
                    None
                }
            }
        }
        None => {
            panel.status_message = Some(
                "⚠ No hay motor instalado: ve a Instalaciones y descarga el motor.".into(),
            );
            None
        }
    }
}

/// Ejecuta una acción del Launcher en modo Hub (S6): todo se
/// lanza como proceso del motor gestionado; este proceso no
/// abre nada en el editor. El registro de la apertura lo hace
/// el propio hijo en su abrir_proyecto (escritor único del
/// registro: si el Hub también registrara al lanzar, los dos
/// guardados se pisarían — auditoría S6). Devuelve el motor
/// lanzado, si lo hubo, para el registro de motores en curso.
fn ejecutar_accion_hub(panel: &mut LauncherPanel, action: LauncherAction) -> Option<std::process::Child> {
    match action {
        LauncherAction::OpenProject(path) => {
            let abs = absolutizar(&path);
            let ruta = abs.to_string_lossy().into_owned();
            lanzar_motor_hijo(panel, &["--project", ruta.as_str()])
        }
        LauncherAction::CreateProject { name, path, template } => {
            match crear_proyecto_en_disco(&name, &path, template) {
                Ok(dir) => {
                    log::info!("Proyecto '{}' creado en disco: {:?}", name, dir);
                    let ruta = dir.to_string_lossy().into_owned();
                    lanzar_motor_hijo(panel, &["--project", ruta.as_str()])
                }
                Err(e) => {
                    log::error!("No se pudo crear el proyecto '{}': {}", name, e);
                    panel.status_message = Some(format!("⚠ No se pudo crear el proyecto: {e}"));
                    None
                }
            }
        }
        LauncherAction::LoadSampleScene(preset) => {
            let clave = clave_de_sample(preset);
            lanzar_motor_hijo(panel, &["--sample", clave.as_str()])
        }
        LauncherAction::OpenSceneFile(ruta) => {
            let abs = absolutizar(&ruta);
            let ruta = abs.to_string_lossy().into_owned();
            lanzar_motor_hijo(panel, &["--scene", ruta.as_str()])
        }
    }
}
#[cfg(test)]
mod tests_validacion_hub {
    use super::validar_argumentos_hub;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn acepta_los_argumentos_del_hub() {
        assert!(validar_argumentos_hub(&args(&["glass_hub"])).is_ok());
        assert!(validar_argumentos_hub(&args(&["glass_hub", "--hub"])).is_ok());
        assert!(validar_argumentos_hub(&args(&["glass_hub", "--tras-actualizar-hub"])).is_ok());
        assert!(
            validar_argumentos_hub(&args(&["glass_hub", "--hub", "--tras-actualizar-hub"])).is_ok()
        );
    }

    #[test]
    fn rechaza_los_flags_del_motor() {
        let casos: &[(&str, &[&str])] = &[
            ("--project", &["glass_hub", "--project", "/tmp/p"]),
            ("--scene", &["glass_hub", "--scene", "escena.ron"]),
            ("--sample", &["glass_hub", "--sample", "demo"]),
            ("--standalone", &["glass_hub", "--standalone"]),
            ("--game", &["glass_hub", "--game"]),
            ("--mcp", &["glass_hub", "--mcp"]),
            ("--mcp-stdio", &["glass_hub", "--mcp-stdio"]),
            ("--mcp-live", &["glass_hub", "--mcp-live"]),
            ("--check-vulkan", &["glass_hub", "--check-vulkan"]),
            ("--test-m5", &["glass_hub", "--test-m5"]),
        ];
        for (flag, lista) in casos {
            let err = validar_argumentos_hub(&args(lista))
                .expect_err(&format!("{flag} debería rechazarse"));
            assert!(err.contains(flag), "el mensaje no menciona {flag}: {err}");
            assert!(err.contains("glass_app"), "el mensaje no señala al motor: {err}");
        }
    }

    #[test]
    fn rechaza_argumentos_desconocidos() {
        assert!(validar_argumentos_hub(&args(&["glass_hub", "--foo"])).is_err());
        assert!(validar_argumentos_hub(&args(&["glass_hub", "/tmp/p"])).is_err());
    }
}

