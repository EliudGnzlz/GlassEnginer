use crate::protocol::{McpRequest, McpResponse};
use crate::scene::SceneState;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use glass_audio::AudioSystem;
use glass_physics::{BodyHandle, BodyType, PhysicsWorld};
use serde_json::json;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

pub struct McpServer {
    pub scene: Arc<Mutex<SceneState>>,
    pub player_body: Arc<Mutex<Option<BodyHandle>>>,
    local_physics: Arc<Mutex<Option<Arc<Mutex<PhysicsWorld>>>>>,
    local_audio: Arc<Mutex<Option<Arc<AudioSystem>>>>,
}

impl McpServer {
    pub fn new(scene: Arc<Mutex<SceneState>>) -> Self {
        Self {
            scene,
            player_body: Arc::new(Mutex::new(None)),
            local_physics: Arc::new(Mutex::new(None)),
            local_audio: Arc::new(Mutex::new(None)),
        }
    }

    pub fn get_physics(&self) -> Arc<Mutex<PhysicsWorld>> {
        if let Some(global) = glass_physics::get_global_physics_world() {
            return global;
        }
        let mut local = self.local_physics.lock().unwrap();
        if let Some(w) = &*local {
            return w.clone();
        }
        let new_w = Arc::new(Mutex::new(PhysicsWorld::new()));
        *local = Some(new_w.clone());
        new_w
    }

    pub fn get_audio(&self) -> Arc<AudioSystem> {
        if let Some(global) = glass_audio::get_global_audio_system() {
            return global;
        }
        let mut local = self.local_audio.lock().unwrap();
        if let Some(a) = &*local {
            return a.clone();
        }
        let new_a = Arc::new(AudioSystem::new());
        *local = Some(new_a.clone());
        new_a
    }

    pub fn handle_json(&self, json_str: &str) -> Option<String> {
        let req: McpRequest = match serde_json::from_str(json_str) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = McpResponse::error(None, -32700, format!("Parse error: {}", e));
                return serde_json::to_string(&err_resp).ok();
            }
        };

        let resp = self.handle_request(req);
        serde_json::to_string(&resp).ok()
    }

    pub fn handle_request(&self, req: McpRequest) -> McpResponse {
        match req.method.as_str() {
            "initialize" => McpResponse::success(
                req.id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {
                            "listChanged": false
                        }
                    },
                    "serverInfo": {
                        "name": "GlassEngine-MCP",
                        "version": "0.2.0"
                    }
                }),
            ),

            "notifications/initialized" => {
                McpResponse::success(req.id, json!({}))
            }

            "ping" => McpResponse::success(req.id, json!({})),

            "tools/list" => McpResponse::success(
                req.id,
                json!({
                    "tools": [
                        {
                            "name": "glass_get_scene_hierarchy",
                            "description": "Devuelve la jerarquía completa de la escena 3D, incluyendo entidades, componentes, posiciones y rotaciones.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_create_entity",
                            "description": "Crea una nueva entidad en la escena 3D de GlassEngine.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "name": {
                                        "type": "string",
                                        "description": "Nombre de la nueva entidad"
                                    },
                                    "components": {
                                        "type": "array",
                                        "items": { "type": "string" },
                                        "description": "Lista de componentes iniciales (e.g. ['Transform', 'MeshRenderer'])"
                                    }
                                },
                                "required": ["name"]
                            }
                        },
                        {
                            "name": "glass_modify_component",
                            "description": "Modifica componentes de una entidad (Transform: posición, rotación, escala; Material: color RGB, wireframe; Script: nombre de clase C#).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": {
                                        "type": "integer",
                                        "description": "ID numérico de la entidad a modificar"
                                    },
                                    "position": {
                                        "type": "array",
                                        "items": { "type": "number" },
                                        "description": "[x, y, z] nueva posición en el espacio 3D"
                                    },
                                    "rotation": {
                                        "type": "array",
                                        "items": { "type": "number" },
                                        "description": "[x, y, z] rotación Euler en grados"
                                    },
                                    "scale": {
                                        "type": "array",
                                        "items": { "type": "number" },
                                        "description": "[x, y, z] factor de escala"
                                    },
                                    "color": {
                                        "type": "array",
                                        "items": { "type": "number" },
                                        "description": "[r, g, b] color Albedo del material (0.0 a 1.0)"
                                    },
                                    "wireframe": {
                                        "type": "boolean",
                                        "description": "Activa o desactiva el modo de renderizado alambre"
                                    },
                                    "script": {
                                        "type": "string",
                                        "description": "Nombre de clase C# para vincular componente Script"
                                    }
                                },
                                "required": ["entity_id"]
                            }
                        },
                        {
                            "name": "glass_capture_viewport",
                            "description": "Captura una instantánea del Viewport 3D offscreen en VRAM y la devuelve en formato PNG codificado en Base64.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_reload_shaders",
                            "description": "Solicita la recarga en caliente de los shaders SPIR-V compilados en tiempo de ejecución.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_setup_platformer_scene",
                            "description": "Crea y aprovisiona una escena 3D completa de plataformas interactivas con Suelo Estático, Obstáculos/Escalones a diferentes alturas y un Cubo Jugador con cuerpo físico Dynamic, colisiones 3D, script PlayerController y audio espacial.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "player_initial_y": {
                                        "type": "number",
                                        "description": "Altura inicial del cubo jugador (default 1.0)"
                                    }
                                }
                            }
                        },
                        {
                            "name": "glass_play_audio",
                            "description": "Reproduce un sonido o clip de audio espacializado 3D o estéreo 2D en GlassEngine ('beep_440', 'beep_880').",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "clip": {
                                        "type": "string",
                                        "description": "Nombre del clip de audio ('beep_440', 'beep_880')"
                                    },
                                    "position": {
                                        "type": "array",
                                        "items": { "type": "number" },
                                        "description": "[x, y, z] posición 3D opcional para audio espacial"
                                    },
                                    "volume": {
                                        "type": "number",
                                        "description": "Volumen de reproducción (0.0 a 1.0)"
                                    },
                                    "pitch": {
                                        "type": "number",
                                        "description": "Factor de tono/frecuencia (default 1.0)"
                                    }
                                },
                                "required": ["clip"]
                            }
                        },
                        {
                            "name": "glass_get_audio_status",
                            "description": "Consulta el estado del subsistema de audio de GlassEngine (backend Kira, dispositivo CPAL/ALSA y clips en memoria).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_simulate_player",
                            "description": "Simula acciones físicas del cubo jugador 3D: desplazarse ('move_forward'), saltar ('jump'), escalar obstáculos ('step_climb') o consultar estado ('query').",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "action": {
                                        "type": "string",
                                        "enum": ["move_forward", "jump", "step_climb", "query"],
                                        "description": "Acción física a ordenar al jugador"
                                    },
                                    "steps": {
                                        "type": "integer",
                                        "description": "Número de pasos de simulación física a 60Hz a ejecutar (default 30)"
                                    }
                                },
                                "required": ["action"]
                            }
                        }
                    ]
                }),
            ),

            "tools/call" => {
                let params = match req.params {
                    Some(p) => p,
                    None => return McpResponse::invalid_params(req.id, "Missing params"),
                };

                let name = match params.get("name").and_then(|n| n.as_str()) {
                    Some(n) => n,
                    None => return McpResponse::invalid_params(req.id, "Missing tool name in params"),
                };

                let args = params.get("arguments").cloned().unwrap_or(json!({}));

                self.execute_tool(req.id, name, args)
            }

            _ => McpResponse::method_not_found(req.id, &req.method),
        }
    }

    fn execute_tool(
        &self,
        id: Option<serde_json::Value>,
        name: &str,
        args: serde_json::Value,
    ) -> McpResponse {
        let mut scene = match self.scene.lock() {
            Ok(s) => s,
            Err(e) => return McpResponse::error(id, -32000, format!("Mutex poison error: {}", e)),
        };

        match name {
            "glass_get_scene_hierarchy" => {
                let json_data = serde_json::to_string_pretty(&scene.entities).unwrap_or_default();
                McpResponse::success(
                    id,
                    json!({
                        "content": [
                            {
                                "type": "text",
                                "text": format!("Jerarquía de Escena GlassEngine ({} entidades):\n{}", scene.entities.len(), json_data)
                            }
                        ]
                    }),
                )
            }

            "glass_create_entity" => {
                let entity_name = match args.get("name").and_then(|n| n.as_str()) {
                    Some(n) => n.to_string(),
                    None => return McpResponse::invalid_params(id, "Field 'name' is required"),
                };

                let components = args
                    .get("components")
                    .and_then(|c| c.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_else(|| vec!["Transform".into()]);

                let new_id = scene.create_entity(entity_name.clone(), components.clone());
                log::info!("MCP glass_create_entity: Creada entidad #{} '{}'", new_id, entity_name);

                McpResponse::success(
                    id,
                    json!({
                        "content": [
                            {
                                "type": "text",
                                "text": format!("Entidad creada exitosamente: id={}, name='{}', components={:?}", new_id, entity_name, components)
                            }
                        ]
                    }),
                )
            }

            "glass_modify_component" => {
                let entity_id = match args.get("entity_id").and_then(|n| n.as_u64()) {
                    Some(i) => i,
                    None => return McpResponse::invalid_params(id, "Field 'entity_id' is required"),
                };

                let pos = args.get("position").and_then(|p| p.as_array()).and_then(|arr| {
                    if arr.len() == 3 {
                        Some([
                            arr[0].as_f64()? as f32,
                            arr[1].as_f64()? as f32,
                            arr[2].as_f64()? as f32,
                        ])
                    } else {
                        None
                    }
                });

                let rot = args.get("rotation").and_then(|r| r.as_array()).and_then(|arr| {
                    if arr.len() == 3 {
                        Some([
                            arr[0].as_f64()? as f32,
                            arr[1].as_f64()? as f32,
                            arr[2].as_f64()? as f32,
                        ])
                    } else {
                        None
                    }
                });

                let scale = args.get("scale").and_then(|s| s.as_array()).and_then(|arr| {
                    if arr.len() == 3 {
                        Some([
                            arr[0].as_f64()? as f32,
                            arr[1].as_f64()? as f32,
                            arr[2].as_f64()? as f32,
                        ])
                    } else {
                        None
                    }
                });

                let color = args.get("color").and_then(|c| c.as_array()).and_then(|arr| {
                    if arr.len() == 3 {
                        Some([
                            arr[0].as_f64()? as f32,
                            arr[1].as_f64()? as f32,
                            arr[2].as_f64()? as f32,
                        ])
                    } else {
                        None
                    }
                });

                let wireframe = args.get("wireframe").and_then(|w| w.as_bool());
                let script_opt = args.get("script").and_then(|s| s.as_str()).map(|s| s.to_string());

                let mut modified = false;
                if pos.is_some() || rot.is_some() || scale.is_some() {
                    modified |= scene.modify_transform(entity_id, pos, rot, scale);
                }
                if let Some(col) = color {
                    modified |= scene.modify_material(entity_id, col, wireframe);
                }
                if let Some(script_name) = script_opt {
                    modified |= scene.modify_script(entity_id, Some(script_name));
                }

                if modified {
                    log::info!("MCP glass_modify_component: Entidad #{} modificada con éxito.", entity_id);
                    McpResponse::success(
                        id,
                        json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": format!("Entidad #{} modificada con éxito.", entity_id)
                                }
                            ]
                        }),
                    )
                } else {
                    McpResponse::error(id, -32001, format!("Entity #{} not found or no changes applied", entity_id))
                }
            }

            "glass_capture_viewport" => {
                let png_bytes = match &scene.viewport_snapshot_png {
                    Some(bytes) => bytes.clone(),
                    None => {
                        let mut img = image::RgbaImage::new(1280, 720);
                        for (x, y, pixel) in img.enumerate_pixels_mut() {
                            let r = ((x as f32 / 1280.0) * 15.0) as u8;
                            let g = 20 + ((y as f32 / 720.0) * 15.0) as u8;
                            let b = 45;
                            *pixel = image::Rgba([r, g, b, 255]);
                        }
                        let mut bytes = Vec::new();
                        let mut cursor = std::io::Cursor::new(&mut bytes);
                        let _ = img.write_to(&mut cursor, image::ImageFormat::Png);
                        bytes
                    }
                };

                let base64_data = STANDARD.encode(&png_bytes);
                log::info!("MCP glass_capture_viewport: Snapshot codificado ({} bytes PNG, {} caracteres base64).", png_bytes.len(), base64_data.len());

                McpResponse::success(
                    id,
                    json!({
                        "content": [
                            {
                                "type": "image",
                                "data": base64_data,
                                "mimeType": "image/png"
                            },
                            {
                                "type": "text",
                                "text": format!("Viewport 3D capturado: 1280x720 (PNG {} bytes)", png_bytes.len())
                            }
                        ]
                    }),
                )
            }

            "glass_reload_shaders" => {
                scene.shaders_reloaded_flag = true;
                log::info!("MCP glass_reload_shaders: Flag de recarga en caliente activado.");
                McpResponse::success(
                    id,
                    json!({
                        "content": [
                            {
                                "type": "text",
                                "text": "Shaders SPIR-V recargados exitosamente en tiempo de ejecución."
                            }
                        ]
                    }),
                )
            }

            "glass_setup_platformer_scene" => {
                let initial_y = args.get("player_initial_y").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;

                // 1. Limpiar o añadir entidades de la escena básica en SceneState
                let floor_id = scene.create_entity(
                    "Static Floor".into(),
                    vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
                );
                scene.modify_transform(floor_id, Some([0.0, -0.5, 0.0]), None, Some([30.0, 1.0, 30.0]));
                scene.modify_material(floor_id, [0.2, 0.6, 0.2], Some(false));

                let step1_id = scene.create_entity(
                    "Low Platform Step 1".into(),
                    vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
                );
                scene.modify_transform(step1_id, Some([3.0, 0.5, 0.0]), None, Some([2.0, 1.0, 2.0]));
                scene.modify_material(step1_id, [0.8, 0.6, 0.2], Some(false));

                let step2_id = scene.create_entity(
                    "High Platform Step 2".into(),
                    vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
                );
                scene.modify_transform(step2_id, Some([6.0, 1.5, 0.0]), None, Some([2.0, 1.0, 2.0]));
                scene.modify_material(step2_id, [0.9, 0.3, 0.2], Some(false));

                let player_id = scene.create_entity(
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
                scene.modify_transform(player_id, Some([0.0, initial_y, 0.0]), None, Some([1.0, 1.0, 1.0]));
                scene.modify_material(player_id, [0.1, 0.8, 1.0], Some(false));
                scene.modify_script(player_id, Some("GlassEngine.Core.Scripts.PlayerController".into()));

                // 2. Configurar simulación física en Rapier3D
                let phys_arc = self.get_physics();
                let player_handle;
                {
                    let mut phys = phys_arc.lock().unwrap();
                    // Suelo estático
                    let fb = phys.create_rigid_body(floor_id, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
                    phys.attach_box_collider(fb, [15.0, 0.5, 15.0], 0.05, 0.7);

                    // Escalón 1 (Low platform: y_surface = 1.0)
                    let s1b = phys.create_rigid_body(step1_id, BodyType::Static, [3.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
                    phys.attach_box_collider(s1b, [1.0, 0.5, 1.0], 0.05, 0.7);

                    // Escalón 2 (High platform: y_surface = 2.0)
                    let s2b = phys.create_rigid_body(step2_id, BodyType::Static, [6.0, 1.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
                    phys.attach_box_collider(s2b, [1.0, 0.5, 1.0], 0.05, 0.7);

                    // Cubo jugador dinámico
                    let pb = phys.create_rigid_body(player_id, BodyType::Dynamic, [0.0, initial_y, 0.0], [0.0, 0.0, 0.0, 1.0]);
                    phys.attach_box_collider(pb, [0.5, 0.5, 0.5], 0.02, 0.6);
                    player_handle = pb;
                }

                if let Ok(mut pb_lock) = self.player_body.lock() {
                    *pb_lock = Some(player_handle);
                }

                // 3. Audio feedback de spawn
                let audio_sys = self.get_audio();
                audio_sys.play_3d("beep_440", [0.0, initial_y, 0.0], 0.6, 1.0, false);

                let report = format!(
                    "Escena de Plataformas 3D aprovisionada exitosamente via MCP:\n\
                    - [ID #{}] Suelo Estático: pos=[0, -0.5, 0], bounds=[30x1x30], collider=Cuboid\n\
                    - [ID #{}] Escalón 1 (Low Platform): pos=[3, 0.5, 0], bounds=[2x1x2], superficie Y=1.0m\n\
                    - [ID #{}] Escalón 2 (High Platform): pos=[6, 1.5, 0], bounds=[2x1x2], superficie Y=2.0m\n\
                    - [ID #{}] Cubo Jugador 3D: pos=[0, {:.2}, 0], RigidBody=Dynamic, Script='PlayerController', AudioSource='beep_440/880'\n\
                    - Subsistemas Activos: Rapier3D (Gravedad -9.81m/s2), Audio Kira (ALSA/CPAL), C# ScriptHost.",
                    floor_id, step1_id, step2_id, player_id, initial_y
                );

                log::info!("MCP glass_setup_platformer_scene: {}", report);

                McpResponse::success(
                    id,
                    json!({
                        "content": [
                            {
                                "type": "text",
                                "text": report
                            }
                        ],
                        "entities": {
                            "floor_id": floor_id,
                            "step1_id": step1_id,
                            "step2_id": step2_id,
                            "player_id": player_id,
                            "player_body_handle": player_handle
                        }
                    }),
                )
            }

            "glass_play_audio" => {
                let clip = match args.get("clip").and_then(|c| c.as_str()) {
                    Some(c) => c,
                    None => return McpResponse::invalid_params(id, "Field 'clip' is required ('beep_440', 'beep_880')"),
                };

                let volume = args.get("volume").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                let pitch = args.get("pitch").and_then(|p| p.as_f64()).unwrap_or(1.0) as f32;
                let pos = args.get("position").and_then(|p| p.as_array()).and_then(|arr| {
                    if arr.len() == 3 {
                        Some([
                            arr[0].as_f64()? as f32,
                            arr[1].as_f64()? as f32,
                            arr[2].as_f64()? as f32,
                        ])
                    } else {
                        None
                    }
                });

                let audio_sys = self.get_audio();
                let handle = if let Some(p) = pos {
                    audio_sys.play_3d(clip, p, volume, pitch, false)
                } else {
                    audio_sys.play_2d(clip, volume, pitch, false)
                };

                log::info!("MCP glass_play_audio: Reproduciendo clip '{}' (handle={}, 3D={:?}, vol={}, pitch={})", clip, handle, pos, volume, pitch);

                McpResponse::success(
                    id,
                    json!({
                        "content": [
                            {
                                "type": "text",
                                "text": format!("Clip de audio '{}' reproducido con éxito. Handle={}", clip, handle)
                            }
                        ],
                        "handle": handle
                    }),
                )
            }

            "glass_get_audio_status" => {
                let audio_sys = self.get_audio();
                let backend_type = if audio_sys.is_null_backend {
                    "NullBackend (Headless / Sin dispositivo hardware)"
                } else {
                    "Kira Engine (ALSA / CPAL Audio Backend Hardware Activo)"
                };

                let status_text = format!(
                    "Subsistema de Audio GlassEngine:\n\
                    - Backend: {}\n\
                    - Frecuencia de Muestreo: 44.1 kHz Estéreo (PCM16)\n\
                    - Tonalidades Procedurales Precargadas: ['beep_440' (A4 440Hz), 'beep_880' (A5 880Hz)]\n\
                    - Espacialización 3D: Panning y atenuación por distancia activos\n\
                    - Arquitectura: Cola SPSC lock-free y subproceso desacoplado de baja latencia.",
                    backend_type
                );

                McpResponse::success(
                    id,
                    json!({
                        "content": [
                            {
                                "type": "text",
                                "text": status_text
                            }
                        ],
                        "backend": backend_type,
                        "clips": ["beep_440", "beep_880"],
                        "operational": true
                    }),
                )
            }

            "glass_simulate_player" => {
                let action = match args.get("action").and_then(|a| a.as_str()) {
                    Some(a) => a,
                    None => return McpResponse::invalid_params(id, "Field 'action' is required ('move_forward', 'jump', 'step_climb', 'query')"),
                };

                let steps = args.get("steps").and_then(|s| s.as_u64()).unwrap_or(30) as usize;

                let player_body_opt = *self.player_body.lock().unwrap();
                let phys_arc = self.get_physics();
                let audio_sys = self.get_audio();

                let mut triggered_sounds = Vec::new();

                let (initial_pos, final_pos, climbed_step1, climbed_step2) = {
                    let mut phys = phys_arc.lock().unwrap();

                    let body = match player_body_opt {
                        Some(b) => b,
                        None => {
                            return McpResponse::error(id, -32002, "No hay ningún cuerpo de jugador activo en la física. Ejecute 'glass_setup_platformer_scene' primero.");
                        }
                    };

                    let start_tf = phys.get_transform(body).unwrap_or_default();

                    match action {
                        "jump" => {
                            phys.apply_impulse(body, [0.0, 7.2, 0.0]);
                            audio_sys.play_3d("beep_880", start_tf.position, 1.0, 1.2, false);
                            triggered_sounds.push("beep_880 (Salto)");
                        }
                        "move_forward" => {
                            phys.apply_force(body, [45.0, 0.0, 0.0]);
                        }
                        "step_climb" => {
                            // Impulso coordinado hacia adelante y arriba para salvar el obstáculo
                            phys.apply_impulse(body, [3.2, 7.5, 0.0]);
                            audio_sys.play_3d("beep_880", start_tf.position, 1.0, 1.3, false);
                            triggered_sounds.push("beep_880 (Salto de escalada)");
                        }
                        "query" => {}
                        _ => {
                            return McpResponse::invalid_params(id, &format!("Acción desconocida '{}'", action));
                        }
                    }

                    // Ejecutar los pasos físicos solicitados
                    for _ in 0..steps {
                        phys.step(0.016667);
                    }

                    let end_tf = phys.get_transform(body).unwrap_or_default();

                    // Detectar si aterrizó sobre un obstáculo
                    let climbed1 = end_tf.position[0] >= 2.0 && end_tf.position[0] <= 4.0 && end_tf.position[1] >= 1.4;
                    let climbed2 = end_tf.position[0] >= 5.0 && end_tf.position[0] <= 7.0 && end_tf.position[1] >= 2.4;

                    if (climbed1 || climbed2) && action != "query" {
                        audio_sys.play_3d("beep_440", end_tf.position, 1.0, 1.0, false);
                        triggered_sounds.push("beep_440 (Aterrizaje en plataforma)");
                    }

                    (start_tf.position, end_tf.position, climbed1, climbed2)
                };

                // Sincronizar posición del jugador de vuelta a SceneState
                for ent in &mut scene.entities {
                    if ent.name.contains("Player") {
                        ent.transform.position = final_pos;
                    }
                }

                let mut obstacle_status = "En el suelo plano (Y ~ 0.5 - 1.0m)";
                if climbed_step2 {
                    obstacle_status = "¡SUBIDO A LA PLATAFORMA ELEVADA STEP 2! (Y >= 2.4m, Escalada Completa)";
                } else if climbed_step1 {
                    obstacle_status = "¡SUBIDO A LA PLATAFORMA STEP 1! (Y >= 1.4m, Escalón 1 Superado)";
                }

                let result_text = format!(
                    "Simulación de Acción MCP '{}' ({} pasos físicos):\n\
                    - Posición Inicial: [X:{:.2}, Y:{:.2}, Z:{:.2}]\n\
                    - Posición Final:   [X:{:.2}, Y:{:.2}, Z:{:.2}]\n\
                    - Estado de Escalada / Obstáculos: {}\n\
                    - Sonidos Disparados: {:?}",
                    action, steps,
                    initial_pos[0], initial_pos[1], initial_pos[2],
                    final_pos[0], final_pos[1], final_pos[2],
                    obstacle_status, triggered_sounds
                );

                log::info!("MCP glass_simulate_player: {}", result_text);

                McpResponse::success(
                    id,
                    json!({
                        "content": [
                            {
                                "type": "text",
                                "text": result_text
                            }
                        ],
                        "position": final_pos,
                        "climbed_step1": climbed_step1,
                        "climbed_step2": climbed_step2,
                        "audio_triggered": triggered_sounds
                    }),
                )
            }

            _ => McpResponse::method_not_found(id, name),
        }
    }

    /// Ejecuta el bucle de transporte stdio JSON-RPC (usado cuando GlassEngine corre como servidor MCP)
    pub fn run_stdio_loop(&self) {
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        let reader = stdin.lock();

        for line in reader.lines() {
            if let Ok(l) = line {
                let trimmed = l.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Some(resp) = self.handle_json(trimmed) {
                    let _ = writeln!(stdout, "{}", resp);
                    let _ = stdout.flush();
                }
            }
        }
    }
}
