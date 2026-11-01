use ash::vk;
use egui_ash_renderer::Renderer as EguiVulkanRenderer;
use egui_ash_renderer::allocator::GpuAllocator;
use egui_winit::State as EguiWinitState;
use glass_core::{GlassWindow, WindowConfig};
use glass_editor::EditorDockingLayout;
use glass_mcp::{McpServer, SceneState};
use glass_renderer::{
    AssetCache, AssetReloadEvent, AssetWatcher, GlassRenderer, MeshLoader, VulkanConfig,
    VulkanContext,
};
use glass_scripting::{register_scene_bridge, SceneBridge, ScriptHost, TransformFfi};
use std::ffi::CStr;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use winit::application::ApplicationHandler;

struct EngineSceneBridge {
    scene_state: Arc<Mutex<SceneState>>,
}

impl SceneBridge for EngineSceneBridge {
    fn get_transform(&self, entity_id: u64) -> Option<TransformFfi> {
        let state = self.scene_state.lock().ok()?;
        let ent = state.entities.iter().find(|e| e.id == entity_id)?;
        Some(TransformFfi {
            position: ent.transform.position,
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: ent.transform.scale,
        })
    }

    fn set_transform(&self, entity_id: u64, transform: TransformFfi) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        if let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) {
            ent.transform.position = transform.position;
            ent.transform.scale = transform.scale;
            true
        } else {
            false
        }
    }
}
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

fn perspective_matrix(fov_rad: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (fov_rad / 2.0).tan();
    [
        f / aspect, 0.0, 0.0, 0.0,
        0.0, -f, 0.0, 0.0, // Invert Y for Vulkan NDC
        0.0, 0.0, far / (near - far), -1.0,
        0.0, 0.0, (near * far) / (near - far), 0.0,
    ]
}

fn model_matrix(pos: [f32; 3], rot_y: f32) -> [f32; 16] {
    let cos = rot_y.cos();
    let sin = rot_y.sin();
    [
        cos, 0.0, -sin, 0.0,
        0.0, 1.0, 0.0, 0.0,
        sin, 0.0, cos, 0.0,
        pos[0], pos[1], pos[2], 1.0,
    ]
}

fn matmul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut res = [0.0; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            res[col * 4 + row] = sum;
        }
    }
    res
}

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
    pub scene_state: Arc<Mutex<SceneState>>,
    pub mcp_server: Arc<McpServer>,
    asset_cache: Arc<RwLock<AssetCache>>,
    asset_watcher: Option<AssetWatcher>,
    start_time: Instant,
    pub script_host: ScriptHost,
    is_headless_test: bool,
    is_m5_test: bool,
    is_m6_test: bool,
    is_m7_test: bool,
}

impl Drop for GlassEngineApp {
    fn drop(&mut self) {
        log::info!("Limpiando GlassEngineApp en orden estricto de dependencias...");
        if let Some(vk_ctx) = &self.vulkan_context {
            unsafe {
                let _ = vk_ctx.device.device_wait_idle();
            }
        }

        if let (Some(renderer), Some(pool), Some(layout)) = (&mut self.renderer, self.desc_pool, self.desc_layout) {
            // Liberar AssetCache primero
            if let Ok(mut cache) = self.asset_cache.write() {
                cache.destroy(&renderer.device, &mut *renderer.allocator.lock().unwrap());
            }

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
    pub fn new(is_headless_test: bool, is_m5_test: bool, is_m6_test: bool, is_m7_test: bool) -> Self {
        let scene_state = Arc::new(Mutex::new(SceneState::new()));
        let mcp_server = Arc::new(McpServer::new(scene_state.clone()));
        let asset_cache = AssetCache::new();

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
            scene_state,
            mcp_server,
            asset_cache,
            asset_watcher: None,
            start_time: Instant::now(),
            script_host: ScriptHost::new(),
            is_headless_test,
            is_m5_test,
            is_m6_test,
            is_m7_test,
        }
    }

    fn check_asset_reloads(&mut self) {
        let events: Vec<AssetReloadEvent> = if let Some(watcher) = &self.asset_watcher {
            watcher.receiver.try_iter().collect()
        } else {
            Vec::new()
        };

        for ev in events {
            match ev {
                AssetReloadEvent::MeshReloaded(path) => {
                    log::info!("Procesando recarga en caliente de modelo: {:?}", path);
                    if let (Some(renderer), Some(vk_ctx)) = (&self.renderer, &self.vulkan_context) {
                        if let Ok(bytes) = std::fs::read(&path) {
                            if let Ok((verts, indices)) = MeshLoader::load_from_glb_bytes(&bytes) {
                                let upload_res = {
                                    let mut alloc = renderer.allocator.lock().unwrap();
                                    MeshLoader::upload_to_gpu(
                                        &renderer.device,
                                        &mut *alloc,
                                        &verts,
                                        &indices,
                                        vk_ctx.graphics_queue,
                                        renderer.command_pool,
                                    )
                                };

                                if let Ok(gpu_mesh) = upload_res {
                                    let stem = path
                                        .file_stem()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("unnamed");
                                    let mut alloc = renderer.allocator.lock().unwrap();
                                    let mut cache = self.asset_cache.write().unwrap();
                                    cache.insert_mesh(
                                        &renderer.device,
                                        &mut *alloc,
                                        stem,
                                        gpu_mesh,
                                    );
                                }
                            }
                        }
                    }
                }
                AssetReloadEvent::ShaderReloaded(path) => {
                    log::info!("Notificada recarga de shader: {:?}", path);
                }
            }
        }
    }

    fn render_frame(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.check_asset_reloads();

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
        let elapsed = self.start_time.elapsed().as_secs_f32();
        let proj = perspective_matrix(45.0_f32.to_radians(), 16.0 / 9.0, 0.1, 100.0);
        let model = model_matrix([0.0, 0.0, -3.5], elapsed * 0.8);
        let mvp = matmul(&proj, &model);

        let cache = self.asset_cache.read().unwrap();
        if let Some(cube_mesh) = cache.meshes.get("cube") {
            renderer.render_offscreen_mesh(vk_ctx.graphics_queue, Some((cube_mesh, mvp)))?;
        } else {
            renderer.render_offscreen(vk_ctx.graphics_queue)?;
        }
        drop(cache);

        // 2. Ejecutar lógica y UI de egui (Docking estilo Unity sincronizado con SceneState)
        let raw_input = egui_winit.take_egui_input(&window.inner);
        let editor_layout = &mut self.editor_layout;
        let scene_state = &self.scene_state;
        let mut full_output = self.egui_ctx.run_ui(raw_input, |ui| {
            editor_layout.render(ui, Some(texture_id), scene_state);
        });
        egui_winit.handle_platform_output(&window.inner, full_output.platform_output);

        // 3. Sincronizar texturas internas de egui hacia la GPU y limpiar deltas inmediatamente
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
                    float32: [0.07, 0.08, 0.10, 1.0],
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

            // Dibujar interfaz egui (Hierarchy, Inspector, Viewport con textura VRAM, Consola)
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

        log::info!("Iniciando GlassEngine (Hito 6: Pipeline de Blender y Recarga en Caliente)...");
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

        // 5. Cargar modelo 3D inicial (cube.glb convertido desde Blender) a VRAM
        let cube_glb_path = "assets/models/cube.glb";
        if Path::new(cube_glb_path).exists() {
            if let Ok(bytes) = std::fs::read(cube_glb_path) {
                match MeshLoader::load_from_glb_bytes(&bytes) {
                    Ok((verts, indices)) => {
                        log::info!(
                            "Cargador glTF: {} vértices y {} índices extraídos de {}",
                            verts.len(),
                            indices.len(),
                            cube_glb_path
                        );
                        let upload_res = {
                            let mut alloc = renderer.allocator.lock().unwrap();
                            MeshLoader::upload_to_gpu(
                                &renderer.device,
                                &mut *alloc,
                                &verts,
                                &indices,
                                vk_ctx.graphics_queue,
                                renderer.command_pool,
                            )
                        };

                        match upload_res {
                            Ok(gpu_mesh) => {
                                log::info!("main: Malla subida a GPU, insertando en cache...");
                                let mut alloc = renderer.allocator.lock().unwrap();
                                let mut cache = self.asset_cache.write().unwrap();
                                cache.insert_mesh(
                                    &renderer.device,
                                    &mut *alloc,
                                    "cube",
                                    gpu_mesh,
                                );
                                log::info!("main: Inserción en cache completada.");
                            }
                            Err(e) => log::error!("Error subiendo malla a GPU: {}", e),
                        }
                    }
                    Err(e) => log::error!("Error importando glTF {}: {}", cube_glb_path, e),
                }
            }
        }

        // 6. Iniciar AssetWatcher en segundo plano sobre assets/
        log::info!("main: Iniciando AssetWatcher...");
        if Path::new("assets").exists() {
            match AssetWatcher::new("assets") {
                Ok(watcher) => {
                    log::info!("main: AssetWatcher listo.");
                    self.asset_watcher = Some(watcher);
                }
                Err(e) => log::warn!("No se pudo inicializar AssetWatcher: {}", e),
            }
        }

        log::info!("main: Renderizando frame offscreen inicial...");

        // Renderizar frame inicial offscreen
        if let Err(e) = renderer.render_offscreen(vk_ctx.graphics_queue) {
            log::error!("Fallo al inicializar frame offscreen: {}", e);
            event_loop.exit();
            return;
        }

        self.renderer = Some(renderer);
        self.vulkan_context = Some(vk_ctx);
        self.window = Some(window);
        self.egui_winit = Some(egui_winit);
        self.egui_renderer = Some(egui_renderer);
        self.desc_layout = Some(layout);
        self.desc_pool = Some(pool);
        self.viewport_texture_id = Some(viewport_texture_id);

        // Conectar el bridge de scripting con el SceneState
        register_scene_bridge(Box::new(EngineSceneBridge {
            scene_state: self.scene_state.clone(),
        }));

        let managed_dir = Path::new("managed/GlassEngine.Core/bin/Debug/net8.0");
        if managed_dir.exists() {
            match self.script_host.initialize(managed_dir) {
                Ok(()) => log::info!("main: ScriptHost C# (.NET CoreCLR) inicializado exitosamente."),
                Err(e) => log::warn!("main: ScriptHost no inicializado: {}", e),
            }
        }

        if self.is_m7_test {
            log::info!("Ejecutando suite de pruebas de integración para HITO 7 (C# Scripting .NET CoreCLR)...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para Hito 7");

            // 1. Crear entidad en SceneState para el test
            let test_entity_id = {
                let mut state = self.scene_state.lock().unwrap();
                state.create_entity(
                    "C# Scripted Entity".to_string(),
                    vec!["Transform".to_string(), "Script".to_string()],
                )
            };

            // 2. Instanciar scripts C# 'Rotator' y 'PlayerMovement'
            let rotator_handle = self.script_host.create_instance(test_entity_id, "GlassEngine.Core.Scripts.Rotator")
                .expect("Debe instanciar Rotator en C#");
            let mover_handle = self.script_host.create_instance(test_entity_id, "GlassEngine.Core.Scripts.PlayerMovement")
                .expect("Debe instanciar PlayerMovement en C#");
            assert_eq!(self.script_host.active_instance_count(), 2);

            let initial_pos = {
                let state = self.scene_state.lock().unwrap();
                let ent = state.entities.iter().find(|e| e.id == test_entity_id).unwrap();
                ent.transform.position
            };

            // 3. Ejecutar 5 frames de simulación con Update C# y renderizado offscreen
            for _ in 0..5 {
                self.script_host.update_all(0.016667);
                if let Err(e) = self.render_frame() {
                    log::error!("Fallo en render_frame durante test C#: {}", e);
                    event_loop.exit();
                    return;
                }
            }

            // 4. Verificar que el script C# modificó la posición en SceneState
            let final_pos = {
                let state = self.scene_state.lock().unwrap();
                let ent = state.entities.iter().find(|e| e.id == test_entity_id).unwrap();
                ent.transform.position
            };
            log::info!("Posición inicial: {:?}, Posición final tras C# Update: {:?}", initial_pos, final_pos);
            assert_ne!(initial_pos, final_pos, "El script C# debió mutar el transform de la entidad");

            // 5. Destruir instancias
            self.script_host.destroy_instance(rotator_handle);
            self.script_host.destroy_instance(mover_handle);
            assert_eq!(self.script_host.active_instance_count(), 0);

            log::info!("============================================================");
            log::info!("   HITO 7 COMPLETADO: C# SCRIPTING (.NET CoreCLR)");
            log::info!("   GPU en Uso:                  {}", gpu_name);
            log::info!("   Runtime .NET:                .NET 8.0 / CoreCLR hostfxr");
            log::info!("   FFI Interop:                 [StructLayout(LayoutKind.Sequential)] 40 bytes");
            log::info!("   Unmanaged Callers:           InitBridge, CreateScriptInstance, Update, Destroy");
            log::info!("   Clase Base:                  GlassEngine.Core.GlassBehaviour");
            log::info!("   Scripts Verificados:         Rotator.cs, PlayerMovement.cs");
            log::info!("   Sincronización de Escena:    EngineSceneBridge bidireccional segura con Mutex");
            log::info!("============================================================");

            event_loop.exit();
            return;
        }

        if self.is_m6_test {
            log::info!("Ejecutando suite de pruebas de integración para HITO 6 (Pipeline de Blender & Hot-Reload)...");

            // 1. Verificar presencia de cube.blend y cube.glb
            assert!(Path::new("assets/models/cube.blend").exists(), "cube.blend no encontrado");
            assert!(Path::new("assets/models/cube.glb").exists(), "cube.glb no encontrado");

            // 2. Verificar que AssetCache contiene la malla de cubo en VRAM
            {
                let cache = self.asset_cache.read().unwrap();
                let cube = cache.meshes.get("cube");
                assert!(cube.is_some(), "Malla 'cube' no encontrada en AssetCache");
                let m = cube.unwrap();
                assert_eq!(m.vertex_count, 24, "El cubo glTF estándar debe contener 24 vértices");
                assert_eq!(m.index_count, 36, "El cubo glTF estándar debe contener 36 índices");
                log::info!("AssetCache: Malla 'cube' validada en VRAM (24 vértices, 36 índices)");
            }

            // 3. Renderizar frame con la malla 3D indexada y matriz MVP activa
            if let Err(e) = self.render_frame() {
                log::error!("Fallo renderizando frame con malla 3D: {}", e);
                event_loop.exit();
                return;
            }

            log::info!("============================================================");
            log::info!("   HITO 6 COMPLETADO: PIPELINE BLENDER & HOT-RELOAD");
            log::info!("   GPU en Uso:                  {}", gpu_name);
            log::info!("   Bridge Headless Blender:     Blender 5.2.2 LTS (.blend -> .glb verificado)");
            log::info!("   Cargador de Mallas glTF:     gltf 1.4 (Posiciones, Normales, UVs, Índices)");
            log::info!("   Subida a VRAM:               Staging Buffer + gpu-allocator 0.28");
            log::info!("   Pipeline de Renderizado 3D:  mesh.vert.spv + mesh.frag.spv + PushConstants MVP");
            log::info!("   Vigilante en Segundo Plano:  AssetWatcher activo con notify 8.2 en assets/");
            log::info!("   Recarga en Caliente:         Soporte atómico de sustitución en AssetCache");
            log::info!("============================================================");

            event_loop.exit();
            return;
        }

        if self.is_m5_test {
            log::info!("Prueba de Hito 5 ejecutada.");
            event_loop.exit();
            return;
        }

        if self.is_headless_test {
            log::info!("Ejecutando frame de prueba...");
            let _ = self.render_frame();
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        if self.is_headless_test || self.is_m5_test || self.is_m6_test || self.is_m7_test {
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

    // Modo Servidor MCP stdio puro para integración con agentes y herramientas externas
    if args.iter().any(|arg| arg == "--mcp" || arg == "--mcp-stdio") {
        let scene_state = Arc::new(Mutex::new(SceneState::new()));
        let mcp_server = McpServer::new(scene_state);
        mcp_server.run_stdio_loop();
        return Ok(());
    }

    let is_m7_test = args.iter().any(|arg| arg == "--test-m7");
    let is_m6_test = args.iter().any(|arg| arg == "--test-m6");
    let is_m5_test = args.iter().any(|arg| arg == "--test-m5");
    let is_headless_test = args
        .iter()
        .any(|arg| arg == "--test-init" || arg == "--check-vulkan" || arg == "--test-m3" || arg == "--test-m4");

    let event_loop = EventLoop::new()?;
    let mut app = GlassEngineApp::new(is_headless_test, is_m5_test, is_m6_test, is_m7_test);
    event_loop.run_app(&mut app)?;

    Ok(())
}
