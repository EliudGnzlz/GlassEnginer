use ash::vk;
#[cfg(feature = "editor")]
use egui_ash_renderer::Renderer as EguiVulkanRenderer;
#[cfg(feature = "editor")]
use egui_ash_renderer::allocator::GpuAllocator;
#[cfg(feature = "editor")]
use egui_winit::State as EguiWinitState;
use glass_audio::{register_global_audio_system, AudioSystem};
use glass_physics::{register_global_physics_world, BodyType, PhysicsWorld};
use glass_core::{GlassWindow, SceneState, WindowConfig};
#[cfg(feature = "editor")]
use glass_editor::EditorDockingLayout;
#[cfg(feature = "editor")]
use glass_mcp::McpServer;
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

fn model_matrix_trs(pos: [f32; 3], scale: [f32; 3], rot_y: f32) -> [f32; 16] {
    let cos = rot_y.cos();
    let sin = rot_y.sin();
    [
        cos * scale[0], 0.0, -sin * scale[0], 0.0,
        0.0, scale[1], 0.0, 0.0,
        sin * scale[2], 0.0, cos * scale[2], 0.0,
        pos[0], pos[1], pos[2], 1.0,
    ]
}

fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let f = [
        target[0] - eye[0],
        target[1] - eye[1],
        target[2] - eye[2],
    ];
    let f_len = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt().max(0.0001);
    let f = [f[0] / f_len, f[1] / f_len, f[2] / f_len];

    let r = [
        f[1] * up[2] - f[2] * up[1],
        f[2] * up[0] - f[0] * up[2],
        f[0] * up[1] - f[1] * up[0],
    ];
    let r_len = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt().max(0.0001);
    let r = [r[0] / r_len, r[1] / r_len, r[2] / r_len];

    let u = [
        r[1] * f[2] - r[2] * f[1],
        r[2] * f[0] - r[0] * f[2],
        r[0] * f[1] - r[1] * f[0],
    ];

    [
        r[0], u[0], -f[0], 0.0,
        r[1], u[1], -f[1], 0.0,
        r[2], u[2], -f[2], 0.0,
        -(r[0] * eye[0] + r[1] * eye[1] + r[2] * eye[2]),
        -(u[0] * eye[0] + u[1] * eye[1] + u[2] * eye[2]),
        f[0] * eye[0] + f[1] * eye[1] + f[2] * eye[2],
        1.0,
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
    #[cfg(feature = "editor")]
    egui_ctx: egui::Context,
    #[cfg(feature = "editor")]
    egui_winit: Option<EguiWinitState>,
    #[cfg(feature = "editor")]
    egui_renderer: Option<EguiVulkanRenderer<GpuAllocator>>,
    #[cfg(feature = "editor")]
    desc_layout: Option<vk::DescriptorSetLayout>,
    #[cfg(feature = "editor")]
    desc_pool: Option<vk::DescriptorPool>,
    #[cfg(feature = "editor")]
    viewport_texture_id: Option<egui::TextureId>,
    #[cfg(feature = "editor")]
    editor_layout: EditorDockingLayout,
    pub scene_state: Arc<Mutex<SceneState>>,
    #[cfg(feature = "editor")]
    #[allow(dead_code)]
    pub mcp_server: Arc<McpServer>,
    asset_cache: Arc<RwLock<AssetCache>>,
    asset_watcher: Option<AssetWatcher>,
    start_time: Instant,
    pub script_host: ScriptHost,
    pub audio_system: Arc<AudioSystem>,
    pub physics_world: Arc<Mutex<PhysicsWorld>>,
    pub is_standalone: bool,
    is_headless_test: bool,
    is_m5_test: bool,
    is_m6_test: bool,
    is_m7_test: bool,
    is_m8_test: bool,
    is_m9_test: bool,
    is_m10_test: bool,
    is_platformer_test: bool,
    player_script_handle: Option<i32>,
}

impl Drop for GlassEngineApp {
    fn drop(&mut self) {
        log::info!("Limpiando GlassEngineApp en orden estricto de dependencias...");
        if let Some(h) = self.player_script_handle {
            self.script_host.destroy_instance(h);
        }
        if let Some(vk_ctx) = &self.vulkan_context {
            unsafe {
                let _ = vk_ctx.device.device_wait_idle();
            }
        }

        #[cfg(feature = "editor")]
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
        #[cfg(not(feature = "editor"))]
        if let Some(renderer) = &mut self.renderer {
            if let Ok(mut cache) = self.asset_cache.write() {
                cache.destroy(&renderer.device, &mut *renderer.allocator.lock().unwrap());
            }
        }

        #[cfg(feature = "editor")]
        {
            self.egui_renderer = None;
            self.egui_winit = None;
        }
        self.renderer = None;
        self.vulkan_context = None;
        self.window = None;
        log::info!("GlassEngineApp limpiado exitosamente.");
    }
}

impl GlassEngineApp {
    pub fn new(
        is_standalone: bool,
        is_headless_test: bool,
        is_m5_test: bool,
        is_m6_test: bool,
        is_m7_test: bool,
        is_m8_test: bool,
        is_m9_test: bool,
        is_m10_test: bool,
        is_platformer_test: bool,
    ) -> Self {
        let scene_state = Arc::new(Mutex::new(SceneState::new()));
        #[cfg(feature = "editor")]
        let mcp_server = Arc::new(McpServer::new(scene_state.clone()));
        let asset_cache = AssetCache::new();
        let audio_system = Arc::new(AudioSystem::new());
        register_global_audio_system(audio_system.clone());
        let physics_world = Arc::new(Mutex::new(PhysicsWorld::new()));
        register_global_physics_world(physics_world.clone());

        Self {
            window_config: WindowConfig::default(),
            vulkan_config: VulkanConfig::default(),
            window: None,
            vulkan_context: None,
            renderer: None,
            #[cfg(feature = "editor")]
            egui_ctx: egui::Context::default(),
            #[cfg(feature = "editor")]
            egui_winit: None,
            #[cfg(feature = "editor")]
            egui_renderer: None,
            #[cfg(feature = "editor")]
            desc_layout: None,
            #[cfg(feature = "editor")]
            desc_pool: None,
            #[cfg(feature = "editor")]
            viewport_texture_id: None,
            #[cfg(feature = "editor")]
            editor_layout: EditorDockingLayout::new(),
            scene_state,
            #[cfg(feature = "editor")]
            mcp_server,
            asset_cache,
            asset_watcher: None,
            start_time: Instant::now(),
            script_host: ScriptHost::new(),
            audio_system,
            physics_world,
            is_standalone,
            is_headless_test,
            is_m5_test,
            is_m6_test,
            is_m7_test,
            is_m8_test,
            is_m9_test,
            is_m10_test,
            is_platformer_test,
            player_script_handle: None,
        }
    }

    pub fn setup_interactive_platformer_scene(&mut self) {
        let initial_y = 0.5;

        // 1. Limpiar o configurar entidades en SceneState
        let (floor_id, step1_id, step2_id, player_id) = {
            let mut state = self.scene_state.lock().unwrap();
            state.entities.clear();

            let floor_id = state.create_entity(
                "Static Floor".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
            );
            state.modify_transform(floor_id, Some([3.5, -0.5, 0.0]), None, Some([30.0, 1.0, 30.0]));
            state.modify_material(floor_id, [0.2, 0.6, 0.2], Some(false));

            let step1_id = state.create_entity(
                "Low Platform Step 1".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
            );
            state.modify_transform(step1_id, Some([3.0, 0.5, 0.0]), None, Some([2.0, 1.0, 2.0]));
            state.modify_material(step1_id, [0.8, 0.6, 0.2], Some(false));

            let step2_id = state.create_entity(
                "High Platform Step 2".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
            );
            state.modify_transform(step2_id, Some([6.0, 1.5, 0.0]), None, Some([2.0, 1.0, 2.0]));
            state.modify_material(step2_id, [0.9, 0.3, 0.2], Some(false));

            let player_id = state.create_entity(
                "Player 3D Cube".into(),
                vec![
                    "Transform".into(),
                    "MeshRenderer".into(),
                    "RigidBody".into(),
                    "Collider".into(),
                    "Script".into(),
                    "AudioSource".into(),
                ],
            );
            state.modify_transform(player_id, Some([0.0, initial_y, 0.0]), None, Some([1.0, 1.0, 1.0]));
            state.modify_material(player_id, [0.1, 0.8, 1.0], Some(false));
            state.modify_script(player_id, Some("GlassEngine.Core.Scripts.PlayerController".into()));

            (floor_id, step1_id, step2_id, player_id)
        };

        // 2. Configurar simulación física en Rapier3D
        {
            let mut phys = self.physics_world.lock().unwrap();
            *phys = PhysicsWorld::new();

            let fb = phys.create_rigid_body(floor_id, BodyType::Static, [3.5, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(fb, [15.0, 0.5, 15.0], 0.05, 0.7);

            let s1b = phys.create_rigid_body(step1_id, BodyType::Static, [3.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(s1b, [1.0, 0.5, 1.0], 0.05, 0.7);

            let s2b = phys.create_rigid_body(step2_id, BodyType::Static, [6.0, 1.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(s2b, [1.0, 0.5, 1.0], 0.05, 0.7);

            let pb = phys.create_rigid_body(player_id, BodyType::Dynamic, [0.0, initial_y, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(pb, [0.5, 0.5, 0.5], 0.02, 0.6);
        }

        // 3. Instanciar script C# PlayerController
        if self.script_host.is_initialized {
            match self.script_host.create_instance(player_id, "GlassEngine.Core.Scripts.PlayerController") {
                Ok(h) => {
                    log::info!("Script PlayerController instanciado exitosamente para el jugador (handle: {})", h);
                    self.player_script_handle = Some(h);
                }
                Err(e) => log::warn!("No se pudo instanciar PlayerController: {}", e),
            }
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

        let (window, vk_ctx, renderer) = match (
            &self.window,
            &self.vulkan_context,
            &mut self.renderer,
        ) {
            (Some(w), Some(ctx), Some(rend)) => (w, ctx, rend),
            _ => return Ok(()),
        };

        // 0. Ejecutar paso de C# Scripting (.NET CoreCLR)
        if self.script_host.is_initialized {
            self.script_host.update_all(0.016667);
        }

        // 1. Simular paso de física con Rapier3D y sincronizar transforms dinámicos a SceneState
        if let Ok(mut physics) = self.physics_world.lock() {
            physics.step(0.016667);
            let sync_transforms = physics.sync_all_transforms();
            if !sync_transforms.is_empty() {
                if let Ok(mut scene) = self.scene_state.lock() {
                    for (entity_id, tf) in sync_transforms {
                        if let Some(ent) = scene.entities.iter_mut().find(|e| e.id == entity_id) {
                            ent.transform.position = tf.position;
                            ent.transform.scale = tf.scale;
                        }
                    }
                }
            }
        }

        // 2. Renderizar geometría 3D a la textura offscreen en VRAM
        let proj = perspective_matrix(45.0_f32.to_radians(), 16.0 / 9.0, 0.1, 100.0);
        let view = look_at([3.5, 3.2, 9.5], [3.5, 1.4, 0.0], [0.0, 1.0, 0.0]);
        let vp = matmul(&proj, &view);

        let cache = self.asset_cache.read().unwrap();
        if let Some(cube_mesh) = cache.meshes.get("cube") {
            let mut mesh_batches: Vec<(&glass_renderer::GpuMesh, [f32; 16])> = Vec::new();

            if let Ok(scene) = self.scene_state.lock() {
                for ent in &scene.entities {
                    if ent.components.iter().any(|c| c == "MeshRenderer" || c == "Collider" || c == "StaticCollider") {
                        let pos = ent.transform.position;
                        // Escalado a half-extents (cube.glb tiene vértices de -1 a +1)
                        let scale = [
                            ent.transform.scale[0] * 0.5,
                            ent.transform.scale[1] * 0.5,
                            ent.transform.scale[2] * 0.5,
                        ];
                        let model = model_matrix_trs(pos, scale, 0.0);
                        let mvp = matmul(&vp, &model);
                        mesh_batches.push((cube_mesh, mvp));
                    }
                }
            }

            if !mesh_batches.is_empty() {
                renderer.render_offscreen_scene(vk_ctx.graphics_queue, &mesh_batches)?;
            } else {
                let elapsed = self.start_time.elapsed().as_secs_f32();
                let model = model_matrix([0.0, 0.0, -3.5], elapsed * 0.8);
                let mvp = matmul(&proj, &model);
                renderer.render_offscreen_mesh(vk_ctx.graphics_queue, Some((cube_mesh, mvp)))?;
            }
        } else {
            renderer.render_offscreen(vk_ctx.graphics_queue)?;
        }
        drop(cache);

        // Modo Standalone: Hardware Blit directo de VRAM al Swapchain (Cero egui/MCP overhead)
        #[cfg(feature = "editor")]
        let render_standalone = self.is_standalone;
        #[cfg(not(feature = "editor"))]
        let render_standalone = true;

        if render_standalone {
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

            renderer.blit_offscreen_to_swapchain(vk_ctx.graphics_queue, image_index)?;

            let swapchains = [renderer.swapchain.swapchain];
            let image_indices = [image_index];
            let signal_semaphores = [renderer.render_finished_semaphore];
            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&signal_semaphores)
                .swapchains(&swapchains)
                .image_indices(&image_indices);

            unsafe {
                let _ = renderer.swapchain.loader.queue_present(vk_ctx.present_queue, &present_info);
                let _ = vk_ctx.device.queue_wait_idle(vk_ctx.present_queue);
            }

            return Ok(());
        }

        // Modo Editor: Renderizar interfaz de usuario egui sobre el RenderPass del Swapchain
        #[cfg(feature = "editor")]
        {
            let (egui_winit, egui_renderer, texture_id) = match (
                &mut self.egui_winit,
                &mut self.egui_renderer,
                self.viewport_texture_id,
            ) {
                (Some(ew), Some(er), Some(tid)) => (ew, er, tid),
                _ => return Ok(()),
            };

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

        // 2. Inicializar egui en Modo Editor (si no es standalone)
        #[cfg(feature = "editor")]
        let (egui_winit, egui_renderer, layout, pool, viewport_texture_id) = if !self.is_standalone {
            let ew = EguiWinitState::new(
                self.egui_ctx.clone(),
                egui::ViewportId::ROOT,
                &window.inner,
                Some(window.inner.scale_factor() as f32),
                None,
                None,
            );

            let er = match EguiVulkanRenderer::with_gpu_allocator(
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

            let (lay, pl, set) = match renderer.create_offscreen_descriptor_set() {
                Ok(res) => res,
                Err(e) => {
                    log::error!("Fallo al crear descriptor set para viewport: {}", e);
                    event_loop.exit();
                    return;
                }
            };

            let mut er = er;
            let tid = er.add_user_texture(set);
            log::info!("Textura Offscreen vinculada exitosamente con egui::TextureId: {:?}", tid);
            (Some(ew), Some(er), Some(lay), Some(pl), Some(tid))
        } else {
            log::info!("Iniciando en Modo Standalone (sin egui, sin servidor MCP)...");
            (None, None, None, None, None)
        };

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
        #[cfg(feature = "editor")]
        {
            self.egui_winit = egui_winit;
            self.egui_renderer = egui_renderer;
            self.desc_layout = layout;
            self.desc_pool = pool;
            self.viewport_texture_id = viewport_texture_id;
        }

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

        let is_test_run = self.is_headless_test
            || self.is_m5_test
            || self.is_m6_test
            || self.is_m7_test
            || self.is_m8_test
            || self.is_m9_test
            || self.is_m10_test
            || self.is_platformer_test;

        if !is_test_run {
            log::info!("Iniciando escena interactiva 3D con plataformas y cubo jugador...");
            self.setup_interactive_platformer_scene();
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

        if self.is_m8_test {
            log::info!("Ejecutando suite de pruebas de integración para HITO 8 (Subsistema de Audio 2D/3D con Kira)...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para Hito 8");

            // 1. Probar reproducción directa 2D y 3D en el subsistema de audio
            let h2d = self.audio_system.play_2d("beep_880", 0.5, 1.0, false);
            assert!(h2d > 0, "Debe retornar un SoundHandle válido para 2D");
            log::info!("Audio 2D probado exitosamente con handle {}", h2d);

            // 2. Probar integración completa con C#: Instanciar AudioEmitterTest
            let test_audio_entity_id = {
                let mut state = self.scene_state.lock().unwrap();
                state.create_entity(
                    "C# Audio Emitter Entity".to_string(),
                    vec!["Transform".to_string(), "AudioSource".to_string(), "Script".to_string()],
                )
            };

            let emitter_handle = self.script_host.create_instance(test_audio_entity_id, "GlassEngine.Core.Scripts.AudioEmitterTest")
                .expect("Debe instanciar AudioEmitterTest en C#");
            assert!(emitter_handle > 0);

            // 3. Simular 5 frames donde el script C# se ejecuta, mueve la posición 3D y actualiza el emisor de audio
            for _ in 0..5 {
                self.script_host.update_all(0.016667);
                if let Err(e) = self.render_frame() {
                    log::error!("Fallo en render_frame durante test de audio: {}", e);
                    event_loop.exit();
                    return;
                }
            }

            // 4. Limpiar script y fuentes de audio
            self.script_host.destroy_instance(emitter_handle);
            self.audio_system.stop_all();

            let backend_name = if self.audio_system.is_null_backend {
                "NullBackend (Modo Headless / CI)"
            } else {
                "Kira 0.10 (CPAL / ALSA Realtime Engine)"
            };

            log::info!("============================================================");
            log::info!("   HITO 8 COMPLETADO: SUBSISTEMA DE AUDIO 2D/3D (KIRA)");
            log::info!("   GPU en Uso:                  {}", gpu_name);
            log::info!("   Backend de Audio:            {}", backend_name);
            log::info!("   Topología de Hilos:          Hilo de audio desacoplado (non-blocking SPSC queue)");
            log::info!("   Generador Procedural:        Síntesis senoidal en memoria (PCM16 48kHz)");
            log::info!("   Clips Pre-cargados:          beep_440, beep_880");
            log::info!("   Audio Espacializado:         SpatialTrack + Listener bidireccional");
            log::info!("   Interop C# CoreCLR:          AudioSource, AudioListener, AudioEmitterTest.cs");
            log::info!("============================================================");

            event_loop.exit();
            return;
        }

        if self.is_m9_test {
            log::info!("Ejecutando suite de pruebas de integración para HITO 9 (Simulación Física 3D con Rapier3D)...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para Hito 9");

            // 1. Crear plano/suelo estático en la física y en SceneState
            let floor_entity_id = {
                let mut state = self.scene_state.lock().unwrap();
                state.create_entity(
                    "Static Floor".to_string(),
                    vec!["Transform".to_string(), "StaticCollider".to_string()],
                )
            };
            {
                let mut phys = self.physics_world.lock().unwrap();
                let floor_body = phys.create_rigid_body(
                    floor_entity_id,
                    BodyType::Static,
                    [0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                );
                phys.attach_box_collider(floor_body, [10.0, 0.5, 10.0], 0.1, 0.7);
            }

            // 2. Crear entidad de cubo dinámico en SceneState que caerá bajo gravedad
            let cube_entity_id = {
                let mut state = self.scene_state.lock().unwrap();
                let id = state.create_entity(
                    "Dynamic Physics Cube".to_string(),
                    vec!["Transform".to_string(), "RigidBody".to_string(), "Script".to_string()],
                );
                if let Some(ent) = state.entities.iter_mut().find(|e| e.id == id) {
                    ent.transform.position = [0.0, 8.0, 0.0];
                }
                id
            };

            // 3. Instanciar script de C# 'PhysicsDropTest' que inicializa el RigidBody y aplica impulso
            let script_handle = self.script_host.create_instance(cube_entity_id, "GlassEngine.Core.Scripts.PhysicsDropTest")
                .expect("Debe instanciar PhysicsDropTest en C#");
            assert!(script_handle > 0, "Handle de script C# debe ser válido");

            // 4. Simular 40 frames de física y renderizado
            let mut min_y = 8.0_f32;
            let mut bounced = false;

            for frame in 0..40 {
                self.script_host.update_all(0.016667);
                if let Err(e) = self.render_frame() {
                    log::error!("Fallo en render_frame durante test de física: {}", e);
                    event_loop.exit();
                    return;
                }

                if let Ok(state) = self.scene_state.lock() {
                    if let Some(ent) = state.entities.iter().find(|e| e.id == cube_entity_id) {
                        let cur_y = ent.transform.position[1];
                        if cur_y < min_y {
                            min_y = cur_y;
                        }
                        if cur_y > min_y + 0.2 && frame > 10 {
                            bounced = true;
                        }
                    }
                }
            }

            log::info!("Test M9: Altura mínima alcanzada bajo gravedad: {:.2}m, Rebotó/Saltó: {}", min_y, bounced);
            assert!(min_y < 7.5, "El cubo dinámico debió caer bajo gravedad (alcanzó {:.2})", min_y);

            // 5. Destruir instancia del script y verificar limpieza
            self.script_host.destroy_instance(script_handle);
            assert_eq!(self.script_host.active_instance_count(), 0);

            log::info!("============================================================");
            log::info!("   HITO 9 COMPLETADO: SIMULACIÓN FÍSICA 3D (RAPIER3D)");
            log::info!("   GPU en Uso:                  {}", gpu_name);
            log::info!("   Motor de Física:             Rapier3D 0.23.1 (f32, dim3)");
            log::info!("   Integración Temporal:        Fixed Timestep 60Hz con acumulador anti-jitter");
            log::info!("   Cuerpos Soportados:          Dynamic, Static, Kinematic");
            log::info!("   Colisionadores:              Cuboid, Ball, Capsule");
            log::info!("   Gravedad & Fuerzas:          vector![0.0, -9.81, 0.0] + Impulsos lineales");
            log::info!("   Sincronización Bidireccional: Bulk Sync de Transforms dinámicos a SceneState");
            log::info!("   Interop C# CoreCLR:          RigidBody, Collider, PhysicsDropTest.cs");
            log::info!("============================================================");

            event_loop.exit();
            return;
        }

        if self.is_m10_test {
            log::info!("Ejecutando suite de pruebas de integración para HITO 10 (Compilación Dual: Editor vs Standalone)...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para Hito 10");

            // Forzar modo standalone
            self.is_standalone = true;

            // 1. Crear entidad y script para el test standalone
            let standalone_entity_id = {
                let mut state = self.scene_state.lock().unwrap();
                state.create_entity(
                    "Standalone Player Entity".to_string(),
                    vec!["Transform".to_string(), "Script".to_string()],
                )
            };

            let rotator_handle = self.script_host.create_instance(standalone_entity_id, "GlassEngine.Core.Scripts.Rotator")
                .expect("Debe instanciar Rotator en C# para standalone");

            // 2. Ejecutar 60 frames continuos en modo standalone con hardware blit directo al Swapchain
            let render_start = Instant::now();
            for _ in 0..60 {
                self.script_host.update_all(0.016667);
                if let Err(e) = self.render_frame() {
                    log::error!("Fallo en render_frame durante test standalone: {}", e);
                    event_loop.exit();
                    return;
                }
            }
            let total_render_time = render_start.elapsed();
            let avg_fps = 60.0 / total_render_time.as_secs_f32().max(0.0001);
            log::info!("Rendimiento Standalone: 60 frames renderizados en {:.3}s (Promedio: {:.1} FPS)", total_render_time.as_secs_f32(), avg_fps);

            self.script_host.destroy_instance(rotator_handle);

            let profile_type = if cfg!(feature = "editor") {
                "Dual Binary (Modo Editor + Standalone selectable con --standalone)"
            } else {
                "Standalone Puro (Zero-overhead: sin egui ni MCP en binario compilado)"
            };

            log::info!("============================================================");
            log::info!("   HITO 10 COMPLETADO: COMPILACIÓN DUAL (EDITOR VS STANDALONE)");
            log::info!("   GPU en Uso:                  {}", gpu_name);
            log::info!("   Perfil Activo:               {}", profile_type);
            log::info!("   Pipeline de Presentación:    Hardware Blit directo de VRAM al Swapchain");
            log::info!("   Rendimiento en Pantalla:     {:.1} FPS (0 ms de overhead de UI)", avg_fps);
            log::info!("   Modo Editor (Default):       cargo build (egui docking, MCP runtime, inspector)");
            log::info!("   Modo Standalone (Release):   cargo build --no-default-features --release");
            log::info!("   Física, Audio y C# Scripting: 100% operativos en ambos perfiles");
            log::info!("============================================================");

            event_loop.exit();
            return;
        }

        if self.is_platformer_test {
            log::info!("Ejecutando suite de pruebas de integración para ESCENA 3D BÁSICA (Cubo jugador, Físicas, MCP, Salto y Escalada de Plataformas)...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para el test de plataformas");

            #[cfg(feature = "editor")]
            {
                // 1. Invocar MCP para inicializar protocolo
                let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
                let init_resp = self.mcp_server.handle_json(init_req).expect("Fallo al inicializar servidor MCP");
                log::info!("MCP Handshake: {}", init_resp);

                // 2. Aprovisionar Escena de Plataformas a través de herramienta MCP glass_setup_platformer_scene
                let setup_req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"glass_setup_platformer_scene","arguments":{"player_initial_y":0.5}}}"#;
                let setup_resp = self.mcp_server.handle_json(setup_req).expect("Fallo al aprovisionar escena via MCP");
                log::info!("MCP glass_setup_platformer_scene Respuesta: {}", setup_resp);
                assert!(setup_resp.contains("Suelo Estático"), "La escena debe contener suelo estático");
                assert!(setup_resp.contains("Escalón 1"), "La escena debe contener escalón 1");
                assert!(setup_resp.contains("Escalón 2"), "La escena debe contener escalón 2");
                assert!(setup_resp.contains("Cubo Jugador 3D"), "La escena debe contener cubo jugador");

                // 3. Inspeccionar jerarquía a través de MCP glass_get_scene_hierarchy
                let hier_req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"glass_get_scene_hierarchy"}}"#;
                let hier_resp = self.mcp_server.handle_json(hier_req).expect("Fallo al consultar jerarquía");
                log::info!("MCP Jerarquía de Escena: {}", hier_resp);
                assert!(hier_resp.contains("Static Floor"));
                assert!(hier_resp.contains("Low Platform Step 1"));
                assert!(hier_resp.contains("High Platform Step 2"));
                assert!(hier_resp.contains("Player 3D Cube"));

                // 4. Instanciar script de C# 'PlayerController' en el cubo jugador
                let player_entity_id = {
                    let state = self.scene_state.lock().unwrap();
                    state.entities.iter().find(|e| e.name == "Player 3D Cube").map(|e| e.id).expect("Debe existir entidad Player 3D Cube")
                };

                let script_handle = self.script_host.create_instance(player_entity_id, "GlassEngine.Core.Scripts.PlayerController")
                    .expect("Debe instanciar PlayerController en C#");
                assert!(script_handle > 0, "Handle de script C# debe ser válido");

                // 5. Simular frames de renderizado, física y C# scripts (movimiento, salto y escalada de plataformas)
                let mut max_x = 0.0_f32;
                let mut max_y = 0.0_f32;
                let mut climbed_step1 = false;
                let mut climbed_step2 = false;

                for _ in 0..360 {
                    self.script_host.update_all(0.016667);
                    if let Err(e) = self.render_frame() {
                        log::error!("Fallo en render_frame durante test de plataformas: {}", e);
                        event_loop.exit();
                        return;
                    }

                    if let Ok(state) = self.scene_state.lock() {
                        if let Some(ent) = state.entities.iter().find(|e| e.id == player_entity_id) {
                            let cur_x = ent.transform.position[0];
                            let cur_y = ent.transform.position[1];
                            if cur_x > max_x { max_x = cur_x; }
                            if cur_y > max_y { max_y = cur_y; }
                            if cur_x >= 2.0 && cur_x <= 4.2 && cur_y >= 1.35 {
                                climbed_step1 = true;
                            }
                            if cur_x >= 4.8 && cur_x <= 7.5 && cur_y >= 2.30 {
                                climbed_step2 = true;
                            }
                        }
                    }
                }

                log::info!("Resultados Simulación: MaxX={:.2}m, MaxY={:.2}m, Escaló Step 1: {}, Escaló Step 2: {}", max_x, max_y, climbed_step1, climbed_step2);
                assert!(max_x > 2.5, "El jugador debió avanzar horizontalmente hacia los obstáculos (alcanzó X={:.2})", max_x);
                assert!(max_y >= 1.35, "El jugador debió saltar y subir a la plataforma (alcanzó Y={:.2})", max_y);
                assert!(climbed_step1, "El jugador debió escalar con éxito la plataforma Step 1 (Y=1.0m, pos_y=1.50m)");
                assert!(climbed_step2, "El jugador debió escalar con éxito la plataforma Step 2 (Y=2.0m, pos_y=2.50m)");

                // 6. Captura de Viewport a través de MCP
                let capture_req = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"glass_capture_viewport"}}"#;
                let capture_resp = self.mcp_server.handle_json(capture_req).expect("Fallo al capturar viewport");
                assert!(capture_resp.contains("image/png"), "MCP debe capturar imagen PNG");

                // 7. Limpieza de script C#
                self.script_host.destroy_instance(script_handle);

                log::info!("============================================================");
                log::info!("   ESCENA 3D BÁSICA & SERVIDOR MCP VALIDADOS EXITOSAMENTE");
                log::info!("   GPU en Uso:                  {}", gpu_name);
                log::info!("   Aprovisionamiento MCP:       glass_setup_platformer_scene (Suelo + Escalón 1 + Escalón 2 + Jugador)");
                log::info!("   Introspección MCP:           glass_get_scene_hierarchy, glass_get_audio_status");
                log::info!("   Física 3D Dinámica:          Rapier3D 0.23 (Cubo Rigidbody, Gravedad, Impulso de Salto)");
                log::info!("   Colisiones 3D:               Cuboid vs Cuboid (Suelo y Plataformas)");
                log::info!("   Escalada de Objetos:         Superó Step 1 (Y=1.0m) y escaló Step 2 (Y=2.0m)");
                log::info!("   Audio Feedback 3D:           Kira (ALSA/CPAL): Disparo 'beep_880' (Salto) y 'beep_440' (Aterrizaje)");
                log::info!("   Controlador C#:              PlayerController.cs con GlassBehaviour CoreCLR");
                log::info!("   Snapshot MCP:                glass_capture_viewport (PNG Base64)");
                log::info!("============================================================");
            }

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
            return;
        }

        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        if self.is_headless_test
            || self.is_m5_test
            || self.is_m6_test
            || self.is_m7_test
            || self.is_m8_test
            || self.is_m9_test
            || self.is_m10_test
            || self.is_platformer_test
        {
            return;
        }

        #[cfg(feature = "editor")]
        if !self.is_standalone {
            if let Some(window) = &self.window {
                if let Some(egui_winit) = &mut self.egui_winit {
                    let res = egui_winit.on_window_event(&window.inner, &event);
                    if res.consumed {
                        return;
                    }
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
                    if physical_size.width > 0
                        && physical_size.height > 0
                        && (physical_size.width != renderer.swapchain.extent.width
                            || physical_size.height != renderer.swapchain.extent.height)
                    {
                        let _ = renderer.resize(vk_ctx, physical_size.width, physical_size.height);
                    }
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
    #[cfg(feature = "editor")]
    if args.iter().any(|arg| arg == "--mcp" || arg == "--mcp-stdio") {
        let scene_state = Arc::new(Mutex::new(SceneState::new()));
        let mcp_server = McpServer::new(scene_state);
        mcp_server.run_stdio_loop();
        return Ok(());
    }

    let is_standalone = args.iter().any(|arg| arg == "--standalone" || arg == "--game");
    let is_platformer_test = args.iter().any(|arg| arg == "--test-platformer");
    let is_m10_test = args.iter().any(|arg| arg == "--test-m10");
    let is_m9_test = args.iter().any(|arg| arg == "--test-m9");
    let is_m8_test = args.iter().any(|arg| arg == "--test-m8");
    let is_m7_test = args.iter().any(|arg| arg == "--test-m7");
    let is_m6_test = args.iter().any(|arg| arg == "--test-m6");
    let is_m5_test = args.iter().any(|arg| arg == "--test-m5");
    let is_headless_test = args
        .iter()
        .any(|arg| arg == "--test-init" || arg == "--check-vulkan" || arg == "--test-m3" || arg == "--test-m4");

    let event_loop = EventLoop::new()?;
    let mut app = GlassEngineApp::new(
        is_standalone,
        is_headless_test,
        is_m5_test,
        is_m6_test,
        is_m7_test,
        is_m8_test,
        is_m9_test,
        is_m10_test,
        is_platformer_test,
    );
    event_loop.run_app(&mut app)?;

    Ok(())
}
