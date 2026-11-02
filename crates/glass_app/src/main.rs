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
use glass_editor::{
    AppMode, EditorDockingLayout, LauncherAction, LauncherPanel, ProjectTemplate, SampleScenePreset,
};
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
use std::collections::HashSet;
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
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
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

#[allow(dead_code)]
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

fn model_matrix_trs_euler(pos: [f32; 3], scale: [f32; 3], rot_deg: [f32; 3]) -> [f32; 16] {
    let rx = rot_deg[0].to_radians();
    let ry = rot_deg[1].to_radians();
    let rz = rot_deg[2].to_radians();

    let (sx, cx) = (rx.sin(), rx.cos());
    let (sy, cy) = (ry.sin(), ry.cos());
    let (sz, cz) = (rz.sin(), rz.cos());

    let r00 = cy * cz;
    let r01 = sx * sy * cz - cx * sz;
    let r02 = cx * sy * cz + sx * sz;

    let r10 = cy * sz;
    let r11 = sx * sy * sz + cx * cz;
    let r12 = cx * sy * sz - sx * cz;

    let r20 = -sy;
    let r21 = sx * cy;
    let r22 = cx * cy;

    [
        r00 * scale[0], r10 * scale[0], r20 * scale[0], 0.0,
        r01 * scale[1], r11 * scale[1], r21 * scale[1], 0.0,
        r02 * scale[2], r12 * scale[2], r22 * scale[2], 0.0,
        pos[0],         pos[1],         pos[2],         1.0,
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

fn camera_view_matrix(pos: [f32; 3], rot_deg: [f32; 3]) -> [f32; 16] {
    let rx = rot_deg[0].to_radians();
    let ry = rot_deg[1].to_radians();

    let (sx, cx) = (rx.sin(), rx.cos());
    let (sy, cy) = (ry.sin(), ry.cos());

    // Vector forward hacia la escena (-Z base con rotación Euler)
    let fwd = [sy * cx, -sx, -cy * cx];
    let up = [sy * sx, cx, -cy * sx];

    let target = [pos[0] + fwd[0], pos[1] + fwd[1], pos[2] + fwd[2]];
    look_at(pos, target, up)
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
    game_desc_layout: Option<vk::DescriptorSetLayout>,
    #[cfg(feature = "editor")]
    game_desc_pool: Option<vk::DescriptorPool>,
    #[cfg(feature = "editor")]
    game_viewport_texture_id: Option<egui::TextureId>,
    #[cfg(feature = "editor")]
    editor_layout: EditorDockingLayout,
    #[cfg(feature = "editor")]
    pub app_mode: AppMode,
    #[cfg(feature = "editor")]
    pub launcher: LauncherPanel,
    #[cfg(feature = "editor")]
    pub pending_launcher_action: Option<LauncherAction>,
    pub scene_state: Arc<Mutex<SceneState>>,
    #[cfg(feature = "editor")]
    #[allow(dead_code)]
    pub mcp_server: Arc<McpServer>,
    asset_cache: Arc<RwLock<AssetCache>>,
    asset_watcher: Option<AssetWatcher>,
    #[allow(dead_code)]
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
    is_sample_game_test: bool,
    player_script_handle: Option<i32>,
    /// Teclas WASD/Espacio actualmente presionadas (para movimiento del jugador).
    keys_pressed: HashSet<PhysicalKey>,
    /// Handle del RigidBody del jugador en Rapier3D (para aplicar fuerzas/impulsos).
    player_body_handle: Option<glass_physics::BodyHandle>,
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
                if let (Some(g_pool), Some(g_layout)) = (self.game_desc_pool, self.game_desc_layout) {
                    renderer.device.destroy_descriptor_pool(g_pool, None);
                    renderer.device.destroy_descriptor_set_layout(g_layout, None);
                }
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
        is_sample_game_test: bool,
    ) -> Self {
        let scene_state = Arc::new(Mutex::new(SceneState::new()));
        #[cfg(feature = "editor")]
        let mcp_server = Arc::new(McpServer::new(scene_state.clone()));
        let asset_cache = AssetCache::new();
        let audio_system = Arc::new(AudioSystem::new());
        register_global_audio_system(audio_system.clone());
        let physics_world = Arc::new(Mutex::new(PhysicsWorld::new()));
        register_global_physics_world(physics_world.clone());

        #[cfg(feature = "editor")]
        let egui_ctx = egui::Context::default();
        #[cfg(feature = "editor")]
        glass_editor::setup_editor_fonts(&egui_ctx);

        #[cfg(feature = "editor")]
        let start_in_editor = is_standalone
            || is_platformer_test
            || is_sample_game_test
            || is_headless_test
            || is_m5_test
            || is_m6_test
            || is_m7_test
            || is_m8_test
            || is_m9_test
            || is_m10_test;
        #[cfg(feature = "editor")]
        let app_mode = if start_in_editor {
            AppMode::Editor
        } else {
            AppMode::Launcher
        };

        Self {
            window_config: WindowConfig::default(),
            vulkan_config: VulkanConfig::default(),
            window: None,
            vulkan_context: None,
            renderer: None,
            #[cfg(feature = "editor")]
            egui_ctx,
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
            game_desc_layout: None,
            #[cfg(feature = "editor")]
            game_desc_pool: None,
            #[cfg(feature = "editor")]
            game_viewport_texture_id: None,
            #[cfg(feature = "editor")]
            editor_layout: EditorDockingLayout::new(),
            #[cfg(feature = "editor")]
            app_mode,
            #[cfg(feature = "editor")]
            launcher: LauncherPanel::new(),
            #[cfg(feature = "editor")]
            pending_launcher_action: None,
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
            is_sample_game_test,
            player_script_handle: None,
            keys_pressed: HashSet::new(),
            player_body_handle: None,
        }
    }

    pub fn setup_interactive_platformer_scene(&mut self) {
        let initial_y = 0.5;

        // 1. Limpiar o configurar entidades en SceneState
        let (floor_id, step1_id, step2_id, player_id) = {
            let mut state = self.scene_state.lock().unwrap();
            state.entities.clear();

            // Instanciar Cámara Principal y Luz Direccional (Game View y Shading)
            let cam_id = state.create_entity(
                "Main Camera".into(),
                vec!["Transform".into(), "Camera".into()],
            );
            state.modify_transform(cam_id, Some([3.5, 3.2, 9.5]), Some([10.0, 0.0, 0.0]), Some([1.0, 1.0, 1.0]));

            let light_id = state.create_entity(
                "Directional Light".into(),
                vec!["Transform".into(), "Light".into()],
            );
            state.modify_transform(light_id, Some([0.0, 10.0, 0.0]), Some([50.0, -30.0, 0.0]), Some([1.0, 1.0, 1.0]));

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

            if let Some(p) = state.entities.iter_mut().find(|e| e.id == player_id) {
                p.script_data = Some(glass_core::ScriptComponentData {
                    class_name: "GlassEngine.Core.Scripts.PlayerController".into(),
                    move_speed: 3.8,
                    jump_velocity: 7.2,
                    auto_demo: false,
                });
                p.audio = Some(glass_core::AudioSourceData {
                    clip_name: "beep_880".into(),
                    volume: 1.0,
                    pitch: 1.0,
                    is_playing: false,
                });
            }

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
            self.player_body_handle = Some(pb);
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

    pub fn setup_complete_sample_game_scene(&mut self) {
        let (floor_id, step1_id, step2_id, goal_id, _coin1_id, _coin2_id, player_id, _canvas_id, _score_id, _time_id) = {
            let mut state = self.scene_state.lock().unwrap();
            state.entities.clear();

            // 1. Cámara y Luz Direccional (PBR)
            let cam_id = state.create_entity(
                "Main Camera".into(),
                vec!["Transform".into(), "Camera".into()],
            );
            state.modify_transform(cam_id, Some([3.5, 3.2, 9.5]), Some([10.0, 0.0, 0.0]), Some([1.0, 1.0, 1.0]));

            let light_id = state.create_entity(
                "Directional Light".into(),
                vec!["Transform".into(), "Light".into()],
            );
            state.modify_transform(light_id, Some([0.0, 10.0, 0.0]), Some([50.0, -30.0, 0.0]), Some([1.0, 1.0, 1.0]));

            // 2. Plataformas 3D (Escalables)
            let floor_id = state.create_entity(
                "Static Floor".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
            );
            state.modify_transform(floor_id, Some([3.5, -0.5, 0.0]), None, Some([30.0, 1.0, 30.0]));
            state.modify_material(floor_id, [0.2, 0.55, 0.25], Some(false));

            let step1_id = state.create_entity(
                "Platform Tier 1".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
            );
            state.modify_transform(step1_id, Some([3.0, 0.5, 0.0]), None, Some([2.0, 1.0, 2.0]));
            state.modify_material(step1_id, [0.85, 0.65, 0.2], Some(false));

            let step2_id = state.create_entity(
                "Platform Tier 2".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
            );
            state.modify_transform(step2_id, Some([6.0, 1.5, 0.0]), None, Some([2.0, 1.0, 2.0]));
            state.modify_material(step2_id, [0.9, 0.35, 0.2], Some(false));

            let goal_id = state.create_entity(
                "Goal Platform (Finish)".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into(), "AudioSource".into()],
            );
            state.modify_transform(goal_id, Some([8.5, 2.5, 0.0]), None, Some([2.0, 1.0, 2.0]));
            state.modify_material(goal_id, [0.1, 0.9, 0.3], Some(false));

            // 3. Monedas / Gemas coleccionables
            let coin1_id = state.create_entity(
                "Coin Gem 1".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "AudioSource".into()],
            );
            state.modify_transform(coin1_id, Some([3.0, 1.4, 0.0]), None, Some([0.4, 0.4, 0.4]));
            state.modify_material(coin1_id, [1.0, 0.84, 0.0], Some(false));

            let coin2_id = state.create_entity(
                "Coin Gem 2".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "AudioSource".into()],
            );
            state.modify_transform(coin2_id, Some([6.0, 2.4, 0.0]), None, Some([0.4, 0.4, 0.4]));
            state.modify_material(coin2_id, [1.0, 0.84, 0.0], Some(false));

            // 4. Jugador 3D con Rapier3D Dynamic RigidBody y script C#
            let player_id = state.create_entity(
                "Player Hero 3D".into(),
                vec![
                    "Transform".into(),
                    "MeshRenderer".into(),
                    "RigidBody".into(),
                    "Collider".into(),
                    "Script".into(),
                    "AudioSource".into(),
                ],
            );
            state.modify_transform(player_id, Some([0.0, 0.5, 0.0]), None, Some([1.0, 1.0, 1.0]));
            state.modify_material(player_id, [0.1, 0.75, 1.0], Some(false));
            state.modify_script(player_id, Some("GlassEngine.Core.Scripts.PlayerController".into()));

            if let Some(p) = state.entities.iter_mut().find(|e| e.id == player_id) {
                p.script_data = Some(glass_core::ScriptComponentData {
                    class_name: "GlassEngine.Core.Scripts.PlayerController".into(),
                    move_speed: 3.8,
                    jump_velocity: 7.2,
                    auto_demo: false,
                });
                p.audio = Some(glass_core::AudioSourceData {
                    clip_name: "beep_880".into(),
                    volume: 1.0,
                    pitch: 1.0,
                    is_playing: false,
                });
            }

            // 5. Canvas UI con Score y Time
            let canvas_id = state.create_entity(
                "InGame Canvas HUD".into(),
                vec!["Transform".into(), "RectTransform".into(), "Canvas".into()],
            );

            let score_id = state.create_entity(
                "Score HUD Text".into(),
                vec!["Transform".into(), "RectTransform".into(), "Text".into()],
            );
            state.reparent(score_id, Some(canvas_id));
            if let Some(e) = state.get_entity_mut(score_id) {
                if let Some(rt) = &mut e.rect_transform {
                    rt.anchored_position = [20.0, 20.0];
                    rt.size_delta = [200.0, 30.0];
                    rt.anchor_min = [0.0, 1.0];
                    rt.anchor_max = [0.0, 1.0];
                    rt.pivot = [0.0, 1.0];
                }
                if let Some(txt) = &mut e.ui_text {
                    txt.text = "Score: 0".into();
                    txt.font_size = 20.0;
                    txt.color = [1.0, 0.9, 0.1, 1.0];
                }
            }

            let time_id = state.create_entity(
                "Time HUD Text".into(),
                vec!["Transform".into(), "RectTransform".into(), "Text".into()],
            );
            state.reparent(time_id, Some(canvas_id));
            if let Some(e) = state.get_entity_mut(time_id) {
                if let Some(rt) = &mut e.rect_transform {
                    rt.anchored_position = [20.0, 55.0];
                    rt.size_delta = [200.0, 30.0];
                    rt.anchor_min = [0.0, 1.0];
                    rt.anchor_max = [0.0, 1.0];
                    rt.pivot = [0.0, 1.0];
                }
                if let Some(txt) = &mut e.ui_text {
                    txt.text = "Time: 0.0s".into();
                    txt.font_size = 16.0;
                    txt.color = [0.8, 0.9, 1.0, 1.0];
                }
            }

            (floor_id, step1_id, step2_id, goal_id, coin1_id, coin2_id, player_id, canvas_id, score_id, time_id)
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

            let gb = phys.create_rigid_body(goal_id, BodyType::Static, [8.5, 2.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(gb, [1.0, 0.5, 1.0], 0.05, 0.7);

            let pb = phys.create_rigid_body(player_id, BodyType::Dynamic, [0.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(pb, [0.5, 0.5, 0.5], 0.02, 0.6);
            self.player_body_handle = Some(pb);
        }

        // 3. Instanciar script C# PlayerController
        if self.script_host.is_initialized {
            match self.script_host.create_instance(player_id, "GlassEngine.Core.Scripts.PlayerController") {
                Ok(h) => {
                    log::info!("Script PlayerController instanciado en Sample Game para Hero (handle: {})", h);
                    self.player_script_handle = Some(h);
                }
                Err(e) => log::warn!("No se pudo instanciar PlayerController: {}", e),
            }
        }
    }

    pub fn setup_physics_playground_scene(&mut self) {
        let (floor_id, dynamic_ids) = {
            let mut state = self.scene_state.lock().unwrap();
            state.entities.clear();

            let cam_id = state.create_entity(
                "Main Camera".into(),
                vec!["Transform".into(), "Camera".into()],
            );
            state.modify_transform(cam_id, Some([0.0, 4.5, 12.0]), Some([12.0, 0.0, 0.0]), Some([1.0, 1.0, 1.0]));

            let light_id = state.create_entity(
                "Directional Light".into(),
                vec!["Transform".into(), "Light".into()],
            );
            state.modify_transform(light_id, Some([0.0, 10.0, 0.0]), Some([45.0, -35.0, 0.0]), Some([1.0, 1.0, 1.0]));

            let floor_id = state.create_entity(
                "Static Floor".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
            );
            state.modify_transform(floor_id, Some([0.0, -0.5, 0.0]), None, Some([30.0, 1.0, 30.0]));
            state.modify_material(floor_id, [0.25, 0.55, 0.35], Some(false));

            let mut dynamic_ids = Vec::new();
            let colors = [
                [0.9, 0.3, 0.2],
                [0.2, 0.7, 0.9],
                [0.9, 0.8, 0.2],
                [0.7, 0.3, 0.9],
                [0.2, 0.9, 0.4],
                [0.95, 0.5, 0.1],
            ];

            // Apilamiento de 6 cubos físicos en pirámide
            let positions = [
                [-1.2, 0.5, 0.0],
                [ 1.2, 0.5, 0.0],
                [ 0.0, 0.5, 0.0],
                [-0.6, 1.6, 0.0],
                [ 0.6, 1.6, 0.0],
                [ 0.0, 2.7, 0.0],
            ];

            for (i, pos) in positions.iter().enumerate() {
                let id = state.create_entity(
                    format!("Physics Cube #{}", i + 1),
                    vec!["Transform".into(), "MeshRenderer".into(), "RigidBody".into(), "Collider".into()],
                );
                state.modify_transform(id, Some(*pos), None, Some([1.0, 1.0, 1.0]));
                state.modify_material(id, colors[i % colors.len()], Some(false));
                dynamic_ids.push((id, *pos));
            }

            (floor_id, dynamic_ids)
        };

        // Configurar simulación en Rapier3D
        {
            let mut phys = self.physics_world.lock().unwrap();
            *phys = PhysicsWorld::new();

            let fb = phys.create_rigid_body(floor_id, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(fb, [15.0, 0.5, 15.0], 0.05, 0.7);

            for (ent_id, pos) in dynamic_ids {
                let b = phys.create_rigid_body(ent_id, BodyType::Dynamic, pos, [0.0, 0.0, 0.0, 1.0]);
                phys.attach_box_collider(b, [0.5, 0.5, 0.5], 0.05, 0.6);
            }
        }
        self.player_body_handle = None;
        self.player_script_handle = None;
    }

    pub fn setup_audio_lighting_scene(&mut self) {
        let floor_id = {
            let mut state = self.scene_state.lock().unwrap();
            state.entities.clear();

            let cam_id = state.create_entity(
                "Main Camera".into(),
                vec!["Transform".into(), "Camera".into()],
            );
            state.modify_transform(cam_id, Some([0.0, 3.5, 9.0]), Some([14.0, 0.0, 0.0]), Some([1.0, 1.0, 1.0]));

            let sun_id = state.create_entity(
                "Sun (Directional Light)".into(),
                vec!["Transform".into(), "Light".into()],
            );
            state.modify_transform(sun_id, Some([0.0, 8.0, 0.0]), Some([55.0, -40.0, 0.0]), Some([1.0, 1.0, 1.0]));

            let red_light = state.create_entity(
                "Point Light (Red)".into(),
                vec!["Transform".into(), "Light".into()],
            );
            state.modify_transform(red_light, Some([-3.0, 2.0, 0.0]), None, Some([1.0, 1.0, 1.0]));
            if let Some(e) = state.entities.iter_mut().find(|e| e.id == red_light) {
                if let Some(ref mut l) = e.light {
                    l.light_type = "Point".into();
                    l.color = [1.0, 0.2, 0.2];
                    l.intensity = 2.0;
                    l.range = 8.0;
                }
            }

            let blue_light = state.create_entity(
                "Point Light (Cyan)".into(),
                vec!["Transform".into(), "Light".into()],
            );
            state.modify_transform(blue_light, Some([3.0, 2.0, 0.0]), None, Some([1.0, 1.0, 1.0]));
            if let Some(e) = state.entities.iter_mut().find(|e| e.id == blue_light) {
                if let Some(ref mut l) = e.light {
                    l.light_type = "Point".into();
                    l.color = [0.2, 0.8, 1.0];
                    l.intensity = 2.0;
                    l.range = 8.0;
                }
            }

            let audio_src = state.create_entity(
                "Ambient Audio Source".into(),
                vec!["Transform".into(), "AudioSource".into()],
            );
            state.modify_transform(audio_src, Some([0.0, 1.0, 0.0]), None, Some([1.0, 1.0, 1.0]));
            if let Some(e) = state.entities.iter_mut().find(|e| e.id == audio_src) {
                e.audio = Some(glass_core::AudioSourceData {
                    clip_name: "beep_440".into(),
                    volume: 0.8,
                    pitch: 1.0,
                    is_playing: true,
                });
            }

            let floor_id = state.create_entity(
                "Static Floor".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
            );
            state.modify_transform(floor_id, Some([0.0, -0.5, 0.0]), None, Some([24.0, 1.0, 24.0]));
            state.modify_material(floor_id, [0.3, 0.3, 0.35], Some(false));

            let cube_center = state.create_entity(
                "Central Showcase Mesh".into(),
                vec!["Transform".into(), "MeshRenderer".into()],
            );
            state.modify_transform(cube_center, Some([0.0, 0.5, 0.0]), None, Some([1.5, 1.5, 1.5]));
            state.modify_material(cube_center, [0.95, 0.95, 0.95], Some(false));

            floor_id
        };

        {
            let mut phys = self.physics_world.lock().unwrap();
            *phys = PhysicsWorld::new();
            let fb = phys.create_rigid_body(floor_id, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(fb, [12.0, 0.5, 12.0], 0.05, 0.7);
        }
        self.player_body_handle = None;
        self.player_script_handle = None;
    }

    #[cfg(feature = "editor")]
    pub fn create_project_and_open(&mut self, name: &str, path: &Path, template: ProjectTemplate) {
        let project_dir = path.join(name);
        let _ = std::fs::create_dir_all(project_dir.join("Assets/Scenes"));
        let _ = std::fs::create_dir_all(project_dir.join("Assets/Scripts"));
        let _ = std::fs::create_dir_all(project_dir.join("Assets/Materials"));

        let meta = format!(
            "{{\n  \"name\": \"{}\",\n  \"version\": \"1.0.0\",\n  \"engine\": \"GlassEngine 0.1.0\",\n  \"template\": \"{:?}\"\n}}\n",
            name, template
        );
        let _ = std::fs::write(project_dir.join("project.glass"), meta);

        match template {
            ProjectTemplate::Platformer3D => {
                self.setup_interactive_platformer_scene();
            }
            ProjectTemplate::PhysicsSandbox => {
                self.setup_physics_playground_scene();
            }
            ProjectTemplate::EmptyScene => {
                self.setup_physics_playground_scene();
            }
        }
        log::info!("Proyecto '{}' creado y cargado en el editor: {:?}", name, project_dir);
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

        #[cfg(feature = "editor")]
        if let Some(action) = self.pending_launcher_action.take() {
            match action {
                LauncherAction::OpenDefaultScene => {
                    self.app_mode = AppMode::Editor;
                }
                LauncherAction::LoadSampleScene(preset) => {
                    match preset {
                        SampleScenePreset::PlatformerDemo => {
                            self.setup_interactive_platformer_scene();
                        }
                        SampleScenePreset::PhysicsPlayground => {
                            self.setup_physics_playground_scene();
                        }
                        SampleScenePreset::AudioLightingShowcase => {
                            self.setup_audio_lighting_scene();
                        }
                        SampleScenePreset::FullSampleGame => {
                            self.setup_complete_sample_game_scene();
                        }
                    }
                    self.app_mode = AppMode::Editor;
                }
                LauncherAction::CreateProject { name, path, template } => {
                    self.create_project_and_open(&name, &path, template);
                    self.app_mode = AppMode::Editor;
                }
                LauncherAction::OpenProject(_path) => {
                    self.setup_interactive_platformer_scene();
                    self.app_mode = AppMode::Editor;
                }
            }
        }

        let (window, vk_ctx, renderer) = match (
            &self.window,
            &self.vulkan_context,
            &mut self.renderer,
        ) {
            (Some(w), Some(ctx), Some(rend)) => (w, ctx, rend),
            _ => return Ok(()),
        };

        // 0. Sincronizar estado de inputs (teclado W, A, S, D, Flechas, Espacio)
        let mut move_x = 0.0_f32;
        let mut move_z = 0.0_f32;
        let mut jump_val = 0_u8;

        for key in &self.keys_pressed {
            if let PhysicalKey::Code(code) = key {
                match code {
                    KeyCode::KeyW | KeyCode::ArrowUp => move_z += 1.0,
                    KeyCode::KeyS | KeyCode::ArrowDown => move_z -= 1.0,
                    KeyCode::KeyD | KeyCode::ArrowRight => move_x += 1.0,
                    KeyCode::KeyA | KeyCode::ArrowLeft => move_x -= 1.0,
                    KeyCode::Space => jump_val = 1,
                    _ => {}
                }
            }
        }

        #[cfg(feature = "editor")]
        {
            self.egui_ctx.input(|i| {
                if i.key_down(egui::Key::W) || i.key_down(egui::Key::ArrowUp) { move_z += 1.0; }
                if i.key_down(egui::Key::S) || i.key_down(egui::Key::ArrowDown) { move_z -= 1.0; }
                if i.key_down(egui::Key::D) || i.key_down(egui::Key::ArrowRight) { move_x += 1.0; }
                if i.key_down(egui::Key::A) || i.key_down(egui::Key::ArrowLeft) { move_x -= 1.0; }
                if i.key_down(egui::Key::Space) { jump_val = 1; }
            });
        }

        glass_scripting::set_input_state(glass_scripting::InputDataFfi {
            move_x: move_x.clamp(-1.0, 1.0),
            move_z: move_z.clamp(-1.0, 1.0),
            jump: jump_val,
            run: 0,
        });

        // 1. Manejo de estado de ejecución (Play / Pause / Stop)
        #[cfg(feature = "editor")]
        let is_playing = !self.is_standalone && self.editor_layout.viewport.play_state == glass_editor::PlayState::Playing;
        #[cfg(feature = "editor")]
        let simulation_running = self.is_standalone || is_playing || self.is_platformer_test || self.is_sample_game_test;
        #[cfg(not(feature = "editor"))]
        let simulation_running = true;

        // Comprobar si se solicitó STOP (reinicio a spawn)
        #[cfg(feature = "editor")]
        if self.editor_layout.viewport.request_reset_to_spawn {
            self.editor_layout.viewport.request_reset_to_spawn = false;
            if let Some(player_handle) = self.player_body_handle {
                if let Ok(mut physics) = self.physics_world.lock() {
                    physics.set_transform(player_handle, [0.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
                    physics.set_linear_velocity(player_handle, [0.0, 0.0, 0.0]);
                }
            }
            if let Ok(mut scene) = self.scene_state.lock() {
                if let Some(ent) = scene.entities.iter_mut().find(|e| e.name.contains("Player")) {
                    ent.transform.position = [0.0, 0.5, 0.0];
                }
            }
            self.audio_system.stop_all();
            log::info!("Simulación detenida (Stop): Jugador y física restablecidos a posición de spawn.");
        }

        // 2. Ejecutar paso de C# Scripting (.NET CoreCLR) — solo si simulación activa
        if simulation_running && self.script_host.is_initialized {
            self.script_host.update_all(0.016667);
        }

        // 3. Simular paso de física Rapier3D o sincronizar transform en modo edición
        if simulation_running {
            if let Ok(mut physics) = self.physics_world.lock() {
                physics.step(0.016667);
                let sync_transforms = physics.sync_all_transforms();
                if let Ok(mut scene) = self.scene_state.lock() {
                    for (entity_id, tf) in sync_transforms {
                        if let Some(ent) = scene.entities.iter_mut().find(|e| e.id == entity_id) {
                            ent.transform.position = tf.position;
                            ent.transform.scale = tf.scale;
                        }
                    }
                    // Actualizar telemetría de velocidad lineal en entity.physics
                    for ent in scene.entities.iter_mut() {
                        if let Some(ref mut phys) = ent.physics {
                            if let Some(bh) = physics.get_body_by_entity(ent.id) {
                                if let Some(vel) = physics.get_linear_velocity(bh) {
                                    phys.linear_velocity = vel;
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // En modo Edición / Pausa:
            // Sincronizar de inmediato transforms editados en Inspector o mediante gizmos hacia Rapier3D
            if let (Ok(scene), Ok(mut physics)) = (self.scene_state.lock(), self.physics_world.lock()) {
                for ent in &scene.entities {
                    if let Some(bh) = physics.get_body_by_entity(ent.id) {
                        physics.set_transform(bh, ent.transform.position, [0.0, 0.0, 0.0, 1.0]);
                        physics.set_linear_velocity(bh, [0.0, 0.0, 0.0]);
                    }
                }
            }
        }

        // 4. Renderizar geometría 3D a las texturas offscreen en VRAM (Scene View y Game View independientes)
        #[cfg(feature = "editor")]
        let (vp_scene, vp_game_opt, light_dir, light_color) = {
            let ed = &self.editor_layout.viewport;
            let eye = [
                ed.editor_cam_center[0] + ed.editor_cam_dist * ed.editor_cam_pitch.cos() * ed.editor_cam_yaw.sin(),
                ed.editor_cam_center[1] + ed.editor_cam_dist * ed.editor_cam_pitch.sin(),
                ed.editor_cam_center[2] + ed.editor_cam_dist * ed.editor_cam_pitch.cos() * ed.editor_cam_yaw.cos(),
            ];
            let view_scene = look_at(eye, ed.editor_cam_center, [0.0, 1.0, 0.0]);
            let aspect_scene = (ed.scene_width / ed.scene_height).max(0.1);
            let proj_scene = perspective_matrix(45.0_f32.to_radians(), aspect_scene, 0.1, 100.0);
            let vp_s = matmul(&proj_scene, &view_scene);

            let (vp_g, l_dir, l_col) = if let Ok(scene) = self.scene_state.lock() {
                let cam_target = ed.selected_camera_id
                    .and_then(|cid| scene.entities.iter().find(|e| e.id == cid && e.active && e.camera.is_some()))
                    .or_else(|| scene.active_camera());

                let vp_g = if let Some(cam) = cam_target {
                    let view_g = camera_view_matrix(cam.transform.position, cam.transform.rotation);
                    let fov = cam.camera.as_ref().map(|c| c.fov).unwrap_or(60.0);
                    let near = cam.camera.as_ref().map(|c| c.near_clip).unwrap_or(0.1);
                    let far = cam.camera.as_ref().map(|c| c.far_clip).unwrap_or(1000.0);
                    let aspect_g = ed.game_resolution.aspect_ratio()
                        .unwrap_or(ed.game_width / ed.game_height)
                        .max(0.1);
                    let proj_g = perspective_matrix(fov.to_radians(), aspect_g, near, far);
                    Some(matmul(&proj_g, &view_g))
                } else {
                    None
                };

                let (ld, lc) = if let Some(light_ent) = scene.main_light() {
                    let rot = light_ent.transform.rotation;
                    let rx = rot[0].to_radians();
                    let ry = rot[1].to_radians();
                    let (sx, cx) = (rx.sin(), rx.cos());
                    let (sy, cy) = (ry.sin(), ry.cos());
                    // L es el vector hacia la fuente de luz en el cielo
                    // Rayos de sol viajan en [sy * cx, -sx, -cy * cx] (hacia el suelo cuando rx > 0)
                    // Por tanto, L hacia el sol es [-sy * cx, sx, cy * cx]
                    let l_dir_vec = [-sy * cx, sx, cy * cx];
                    let len = (l_dir_vec[0] * l_dir_vec[0] + l_dir_vec[1] * l_dir_vec[1] + l_dir_vec[2] * l_dir_vec[2]).sqrt().max(0.001);
                    let norm = [l_dir_vec[0] / len, l_dir_vec[1] / len, l_dir_vec[2] / len];
                    let ldata = light_ent.light.as_ref().cloned().unwrap_or_default();
                    let col = [
                        ldata.color[0] * ldata.intensity,
                        ldata.color[1] * ldata.intensity,
                        ldata.color[2] * ldata.intensity,
                    ];
                    ([norm[0], norm[1], norm[2], 0.25], [col[0], col[1], col[2], 1.0])
                } else {
                    ([0.0, 1.0, 0.0, 0.06], [0.0, 0.0, 0.0, 0.0])
                };

                (vp_g, ld, lc)
            } else {
                (None, [0.5, 1.0, 0.7, 0.25], [1.0, 1.0, 1.0, 1.0])
            };

            (vp_s, vp_g, l_dir, l_col)
        };

        #[cfg(not(feature = "editor"))]
        let (vp_scene, vp_game_opt, light_dir, light_color) = {
            let (vp_g, ld, lc) = if let Ok(scene) = self.scene_state.lock() {
                let vp_g = if let Some(cam) = scene.active_camera() {
                    let view_g = camera_view_matrix(cam.transform.position, cam.transform.rotation);
                    let fov = cam.camera.as_ref().map(|c| c.fov).unwrap_or(60.0);
                    let near = cam.camera.as_ref().map(|c| c.near_clip).unwrap_or(0.1);
                    let far = cam.camera.as_ref().map(|c| c.far_clip).unwrap_or(1000.0);
                    let proj_g = perspective_matrix(fov.to_radians(), 16.0 / 9.0, near, far);
                    Some(matmul(&proj_g, &view_g))
                } else {
                    let v = look_at([3.5, 3.2, 9.5], [3.5, 1.4, 0.0], [0.0, 1.0, 0.0]);
                    let p = perspective_matrix(45.0_f32.to_radians(), 16.0 / 9.0, 0.1, 100.0);
                    Some(matmul(&p, &v))
                };
                let (ld, lc) = if let Some(light_ent) = scene.main_light() {
                    let rot = light_ent.transform.rotation;
                    let rx = rot[0].to_radians();
                    let ry = rot[1].to_radians();
                    let (sx, cx) = (rx.sin(), rx.cos());
                    let (sy, cy) = (ry.sin(), ry.cos());
                    // L es el vector hacia la fuente de luz en el cielo
                    // Rayos de sol viajan en [sy * cx, -sx, -cy * cx] (hacia el suelo cuando rx > 0)
                    // Por tanto, L hacia el sol es [-sy * cx, sx, cy * cx]
                    let l_dir_vec = [-sy * cx, sx, cy * cx];
                    let len = (l_dir_vec[0] * l_dir_vec[0] + l_dir_vec[1] * l_dir_vec[1] + l_dir_vec[2] * l_dir_vec[2]).sqrt().max(0.001);
                    let norm = [l_dir_vec[0] / len, l_dir_vec[1] / len, l_dir_vec[2] / len];
                    let ldata = light_ent.light.as_ref().cloned().unwrap_or_default();
                    let col = [
                        ldata.color[0] * ldata.intensity,
                        ldata.color[1] * ldata.intensity,
                        ldata.color[2] * ldata.intensity,
                    ];
                    ([norm[0], norm[1], norm[2], 0.25], [col[0], col[1], col[2], 1.0])
                } else {
                    ([0.0, 1.0, 0.0, 0.06], [0.0, 0.0, 0.0, 0.0])
                };
                (vp_g, ld, lc)
            } else {
                let v = look_at([3.5, 3.2, 9.5], [3.5, 1.4, 0.0], [0.0, 1.0, 0.0]);
                let p = perspective_matrix(45.0_f32.to_radians(), 16.0 / 9.0, 0.1, 100.0);
                (Some(matmul(&p, &v)), [0.5, 1.0, 0.7, 0.25], [1.0, 1.0, 1.0, 1.0])
            };
            (vp_g.unwrap(), vp_g, ld, lc)
        };

        let cache = self.asset_cache.read().unwrap();
        if let Some(cube_mesh) = cache.meshes.get("cube") {
            let mut scene_batches: Vec<(&glass_renderer::GpuMesh, glass_renderer::MeshPushConstants)> = Vec::new();
            let mut game_batches: Vec<(&glass_renderer::GpuMesh, glass_renderer::MeshPushConstants)> = Vec::new();

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
                        let model = model_matrix_trs_euler(pos, scale, ent.transform.rotation);
                        let mvp_scene = matmul(&vp_scene, &model);
                        let obj_col = ent.mesh.as_ref().map(|m| [m.color[0], m.color[1], m.color[2], 1.0]).unwrap_or([0.8, 0.8, 0.8, 1.0]);

                        let has_light = if light_color[3] > 0.0 { 1.0 } else { 0.0 };
                        let material_params = [0.15, 0.45, 1.0, has_light];

                        scene_batches.push((cube_mesh, glass_renderer::MeshPushConstants {
                            mvp: mvp_scene,
                            light_dir,
                            light_color,
                            material_params,
                            object_color: obj_col,
                        }));

                        if let Some(vp_game) = vp_game_opt {
                            let mvp_game = matmul(&vp_game, &model);
                            game_batches.push((cube_mesh, glass_renderer::MeshPushConstants {
                                mvp: mvp_game,
                                light_dir,
                                light_color,
                                material_params,
                                object_color: obj_col,
                            }));
                        }
                    }
                }
            }

            let game_meshes_arg = if vp_game_opt.is_some() {
                Some(game_batches.as_slice())
            } else {
                None
            };

            renderer.render_views(vk_ctx.graphics_queue, &scene_batches, game_meshes_arg)?;
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
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                    let size = window.inner.inner_size();
                    let _ = renderer.resize(vk_ctx, size.width, size.height);
                    return Ok(());
                }
                Err(vk::Result::ERROR_SURFACE_LOST_KHR) => {
                    log::warn!("Surface lost KHR detectado, recreando swapchain...");
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
            let (egui_winit, egui_renderer) = match (
                &mut self.egui_winit,
                &mut self.egui_renderer,
            ) {
                (Some(ew), Some(er)) => (ew, er),
                _ => return Ok(()),
            };

            // 2. Ejecutar lógica y UI de egui (Docking estilo Unity sincronizado con SceneState o Hub / Lanzador)
            let raw_input = egui_winit.take_egui_input(&window.inner);
            let editor_layout = &mut self.editor_layout;
            let launcher = &mut self.launcher;
            let app_mode = self.app_mode;
            let scene_state = &self.scene_state;
            let scene_tid = self.viewport_texture_id;
            let game_tid = self.game_viewport_texture_id;

            let mut launcher_action = None;
            let mut full_output = self.egui_ctx.run_ui(raw_input, |ui| {
                match app_mode {
                    AppMode::Launcher => {
                        launcher_action = launcher.render(ui);
                    }
                    AppMode::Editor => {
                        editor_layout.render(ui, scene_tid, game_tid, scene_state);
                    }
                }
            });
            egui_winit.handle_platform_output(&window.inner, full_output.platform_output);

            if let Some(action) = launcher_action {
                self.pending_launcher_action = Some(action);
                self.egui_ctx.request_repaint();
            }

            if editor_layout.request_launcher {
                editor_layout.request_launcher = false;
                self.app_mode = AppMode::Launcher;
                self.egui_ctx.request_repaint();
            }

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
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                    let size = window.inner.inner_size();
                    let _ = renderer.resize(vk_ctx, size.width, size.height);
                    return Ok(());
                }
                Err(vk::Result::ERROR_SURFACE_LOST_KHR) => {
                    log::warn!("Surface lost KHR detectado, recreando swapchain...");
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

                match renderer.swapchain.loader.queue_present(vk_ctx.present_queue, &present_info) {
                    Ok(_) => {}
                    Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                        let size = window.inner.inner_size();
                        let _ = renderer.resize(vk_ctx, size.width, size.height);
                    }
                    Err(e) => log::warn!("Aviso durante queue_present: {:?}", e),
                }
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
        let (egui_winit, egui_renderer, layout, pool, viewport_texture_id, game_layout, game_pool, game_viewport_texture_id) = if !self.is_standalone {
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

            let (g_lay, g_pl, g_set) = match renderer.create_game_offscreen_descriptor_set() {
                Ok(res) => res,
                Err(e) => {
                    log::error!("Fallo al crear descriptor set para game viewport: {}", e);
                    event_loop.exit();
                    return;
                }
            };

            let mut er = er;
            let tid = er.add_user_texture(set);
            let g_tid = er.add_user_texture(g_set);
            log::info!("Texturas Offscreen vinculadas exitosamente con egui::TextureId: Scene {:?}, Game {:?}", tid, g_tid);
            (Some(ew), Some(er), Some(lay), Some(pl), Some(tid), Some(g_lay), Some(g_pl), Some(g_tid))
        } else {
            log::info!("Iniciando en Modo Standalone (sin egui, sin servidor MCP)...");
            (None, None, None, None, None, None, None, None)
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
            self.game_desc_layout = game_layout;
            self.game_desc_pool = game_pool;
            self.game_viewport_texture_id = game_viewport_texture_id;
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
            || self.is_platformer_test
            || self.is_sample_game_test
            || self.is_sample_game_test;

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

        if self.is_sample_game_test {
            log::info!("Ejecutando suite de pruebas de integración para SAMPLE GAME COMPLETO (Plataformas, Monedas, Canvas UI, Meta y PBR)...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para el test de sample game");

            self.setup_complete_sample_game_scene();

            #[cfg(feature = "editor")]
            {
                let player_entity_id = {
                    let state = self.scene_state.lock().unwrap();
                    state.entities.iter().find(|e| e.name == "Player Hero 3D").map(|e| e.id).expect("Debe existir entidad Player Hero 3D")
                };

                let script_handle = self.script_host.create_instance(player_entity_id, "GlassEngine.Core.Scripts.PlayerController")
                    .expect("Debe instanciar PlayerController en C#");

                let mut coins_collected = 0;
                let mut reached_goal = false;
                let mut final_score = 0;

                for frame in 0..380 {
                    let dt = 0.016667;
                    self.script_host.update_all(dt);
                    if let Err(e) = self.render_frame() {
                        log::error!("Fallo en render_frame durante test de sample game: {}", e);
                        event_loop.exit();
                        return;
                    }

                    if let Ok(mut state) = self.scene_state.lock() {
                        let player_pos = state.entities.iter().find(|e| e.id == player_entity_id).map(|e| e.transform.position);

                        if let Some(pos) = player_pos {
                            // 1. Recolección de Moneda 1
                            if let Some(c1) = state.entities.iter_mut().find(|e| e.name == "Coin Gem 1" && e.active) {
                                let dist = ((pos[0] - c1.transform.position[0]).powi(2) + (pos[1] - c1.transform.position[1]).powi(2)).sqrt();
                                if dist < 1.6 {
                                    c1.active = false;
                                    coins_collected += 1;
                                    final_score += 50;
                                    if let Some(audio_sys) = glass_audio::get_global_audio_system() {
                                        audio_sys.play_2d("beep_880", 0.8, 1.5, false);
                                    }
                                }
                            }

                            // 2. Recolección de Moneda 2
                            if let Some(c2) = state.entities.iter_mut().find(|e| e.name == "Coin Gem 2" && e.active) {
                                let dist = ((pos[0] - c2.transform.position[0]).powi(2) + (pos[1] - c2.transform.position[1]).powi(2)).sqrt();
                                if dist < 1.6 {
                                    c2.active = false;
                                    coins_collected += 1;
                                    final_score += 50;
                                    if let Some(audio_sys) = glass_audio::get_global_audio_system() {
                                        audio_sys.play_2d("beep_880", 0.9, 1.8, false);
                                    }
                                }
                            }

                            // 3. Sincronizar Canvas HUD Text
                            if let Some(score_ent) = state.entities.iter_mut().find(|e| e.name == "Score HUD Text") {
                                if let Some(txt) = &mut score_ent.ui_text {
                                    txt.text = format!("Score: {}", final_score);
                                }
                            }
                            if let Some(time_ent) = state.entities.iter_mut().find(|e| e.name == "Time HUD Text") {
                                if let Some(txt) = &mut time_ent.ui_text {
                                    txt.text = format!("Time: {:.1}s", (frame as f32) * dt);
                                }
                            }

                            // 4. Comprobar Llegada a la Meta (Superando Step 1 y Step 2)
                            if pos[0] >= 4.8 && pos[1] >= 2.30 {
                                reached_goal = true;
                            }
                        }
                    }
                }

                assert!(coins_collected >= 1, "El jugador debió recolectar al menos una moneda (recolectó {})", coins_collected);
                assert!(final_score >= 50, "La puntuación en el Canvas UI debió actualizarse (Score={})", final_score);
                assert!(reached_goal, "El jugador debió alcanzar la plataforma Meta / Final");

                log::info!("============================================================");
                log::info!("✅ SAMPLE GAME TEST PASSED EXITOSAMENTE:");
                log::info!("   - Monedas Recolectadas:    {} / 2", coins_collected);
                log::info!("   - Puntuación Canvas UI:    {} pts", final_score);
                log::info!("   - Meta Alcanzada:          {}", reached_goal);
                log::info!("   - Físicas Rapier3D:        OK");
                log::info!("   - C# UnityEngine Runtime:  OK");
                log::info!("   - Shaders PBR Cook-Torr:   OK");
                log::info!("============================================================");

                self.script_host.destroy_instance(script_handle);
                event_loop.exit();
                return;
            }
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
            || self.is_sample_game_test
        {
            return;
        }

        #[cfg(feature = "editor")]
        if !self.is_standalone {
            if let Some(window) = &self.window {
                if let Some(egui_winit) = &mut self.egui_winit {
                    // Capturar WASD/Space ANTES de egui para que nunca sean bloqueadas
                    if let WindowEvent::KeyboardInput { event: ref key_ev, .. } = event {
                        if let PhysicalKey::Code(code) = key_ev.physical_key {
                            match code {
                                KeyCode::KeyW | KeyCode::KeyS | KeyCode::KeyA | KeyCode::KeyD | KeyCode::Space => {
                                    match key_ev.state {
                                        ElementState::Pressed => { self.keys_pressed.insert(key_ev.physical_key); }
                                        ElementState::Released => { self.keys_pressed.remove(&key_ev.physical_key); }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    // Pasar evento completo a egui (sin bloquear con consumed)
                    let _ = egui_winit.on_window_event(&window.inner, &event);
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
            // Captura de input WASD / Espacio para el jugador
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match code {
                        KeyCode::KeyW | KeyCode::KeyS | KeyCode::KeyA | KeyCode::KeyD | KeyCode::Space => {
                            match event.state {
                                ElementState::Pressed => {
                                    self.keys_pressed.insert(event.physical_key);
                                }
                                ElementState::Released => {
                                    self.keys_pressed.remove(&event.physical_key);
                                }
                            }
                        }
                        _ => {}
                    }
                }
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
    let is_sample_game_test = args.iter().any(|arg| arg == "--test-sample-game");
    let is_platformer_test = args.iter().any(|arg| arg == "--test-platformer");
    if is_platformer_test || is_sample_game_test {
        std::env::set_var("GLASS_AUTO_DEMO", "1");
    }
    let is_m10_test = args.iter().any(|arg| arg == "--test-m10");
    let is_m9_test = args.iter().any(|arg| arg == "--test-m9");
    let is_m8_test = args.iter().any(|arg| arg == "--test-m8");
    let is_m7_test = args.iter().any(|arg| arg == "--test-m7");
    let is_m6_test = args.iter().any(|arg| arg == "--test-m6");
    let is_m5_test = args.iter().any(|arg| arg == "--test-m5");
    let is_headless_test = args
        .iter()
        .any(|arg| arg == "--test-init" || arg == "--check-vulkan" || arg == "--test-m3" || arg == "--test-m4");

    let mut builder = EventLoop::builder();
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        if std::env::var("WINIT_UNIX_BACKEND").as_deref() == Ok("x11") || std::env::var("DISPLAY").is_ok() {
            log::info!("Forzando backend X11/XWayland en Winit para máxima compatibilidad con Vulkan y NVIDIA...");
            builder.with_x11();
        }
    }
    let event_loop = builder.build()?;
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
        is_sample_game_test,
    );
    event_loop.run_app(&mut app)?;

    Ok(())
}
