use crate::protocol::{McpRequest, McpResponse};
use crate::scene::SceneState;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use glass_audio::AudioSystem;
use glass_physics::{BodyHandle, BodyType, PhysicsWorld};
use serde_json::json;
use std::io::{BufRead, Read, Write};
use std::sync::{Arc, Mutex};

// Cotas de entrada del servidor MCP: la superficie la conduce un agente y
// ninguna petición puede colgar el proceso ni agotar la memoria. Los
// excesos devuelven un error JSON-RPC claro; nunca se truncan en silencio.
const MAX_SIMULATION_STEPS: u64 = 3600; // 60 s de simulación a 60 Hz
const MAX_ENTITY_NAME_CHARS: usize = 256;
const MAX_COMPONENTS_PER_CALL: usize = 64;
const MAX_ENTITIES_PER_SCENE: usize = 10_000;
const MAX_LINE_BYTES: u64 = 10 * 1024 * 1024; // 10 MiB por línea stdio
// Tope de entidades admitidas en una sola llamada de glass_batch_spawn_entities.
const MAX_BATCH_SPAWN: usize = 256;

/// Tools MCP que pertenecen al complemento «herramienta-3d».
pub const MESH_TOOL_NAMES: [&str; 7] = [
    "glass_mesh_enter_edit",
    "glass_mesh_apply_op",
    "glass_mesh_undo",
    "glass_mesh_redo",
    "glass_mesh_set_dimension",
    "glass_mesh_get_stats",
    "glass_mesh_bake",
];

pub struct McpServer {
    pub scene: Arc<Mutex<SceneState>>,
    pub player_body: Arc<Mutex<Option<BodyHandle>>>,
    local_physics: Arc<Mutex<Option<Arc<Mutex<PhysicsWorld>>>>>,
    local_audio: Arc<Mutex<Option<Arc<AudioSystem>>>>,
    /// Sesión de edición de malla (herramienta 3D): vive en proceso, la conducen las tools glass_mesh_*.
    mesh_session: Arc<Mutex<Option<glass_mesh_edit::MeshSession>>>,
    /// Puerta del complemento «herramienta-3d»: si es false, no se listan ni ejecutan las tools glass_mesh_*.
    mesh_tools_enabled: std::sync::atomic::AtomicBool,
    /// Command Queue (Fase 8): la ejecuta el bucle del motor.
    pub queue: Arc<crate::queue::CommandQueue>,
}

/// Tools que ALTERAN el estado (Fase 8): pasan por la Command
/// Queue cuando el bucle del motor está bombeando. Las de consulta
/// responden directas, como siempre.
pub fn is_mutating_tool(name: &str) -> bool {
    matches!(
        name,
        "glass_create_entity"
            | "glass_batch_spawn_entities"
            | "glass_destroy_entity"
            | "glass_modify_component"
            | "glass_prefab_create"
            | "glass_prefab_create_variant"
            | "glass_prefab_instantiate"
            | "glass_prefab_revert"
            | "glass_set_ui_text"
            | "glass_set_entity_material"
            | "glass_material_property_set"
            | "glass_shader_node_add"
            | "glass_shader_pass_configure"
            | "glass_shader_preset_apply"
            | "glass_animator_set_param"
            | "glass_audio_buses"
            | "glass_play_audio"
            | "glass_reload_shaders"
            | "glass_bake_lightmap"
            | "glass_setup_platformer_scene"
            | "glass_asset_reload"
            | "glass_mesh_enter_edit"
            | "glass_mesh_apply_op"
            | "glass_mesh_undo"
            | "glass_mesh_redo"
            | "glass_mesh_set_dimension"
            | "glass_mesh_bake"
            | "glass_particle_emitter"
            | "glass_particle_emit"
    )
}

impl McpServer {
    /// Abre o cierra la puerta de las tools de malla (complemento «herramienta-3d»).
    pub fn set_mesh_tools_enabled(&self, on: bool) {
        self.mesh_tools_enabled
            .store(on, std::sync::atomic::Ordering::SeqCst);
    }

    /// Indica si las tools de malla están disponibles.
    pub fn mesh_tools_enabled(&self) -> bool {
        self.mesh_tools_enabled
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn new(scene: Arc<Mutex<SceneState>>) -> Self {
        Self {
            scene,
            player_body: Arc::new(Mutex::new(None)),
            local_physics: Arc::new(Mutex::new(None)),
            local_audio: Arc::new(Mutex::new(None)),
            mesh_session: Arc::new(Mutex::new(None)),
            // Abierta por defecto: los usos sin puerta (tests incluidos) no cambian.
            mesh_tools_enabled: std::sync::atomic::AtomicBool::new(true),
            queue: Arc::new(crate::queue::CommandQueue::new()),
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

            "tools/list" => {
                let mut result = json!({
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
                            "description": "Devuelve el último snapshot del Viewport 3D offscreen si el motor lo publicó en la escena. Si no existe (motor no ejecutándose con editor, o sin frame capturado), devuelve una imagen degradada sintética marcada con \"synthetic\": true, nunca una captura real disfrazada.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_reload_shaders",
                            "description": "Activa el flag shaders_reloaded_flag de la escena. ATENCIÓN: los shaders SPIR-V están embebidos en el binario con include_bytes!, por lo que esta llamada NO reconstruye el pipeline en caliente; exige recompilar glass_renderer y reiniciar el motor.",
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
                                    },
                                    "bus": {
                                        "type": "string",
                                        "enum": ["master", "music", "sfx"],
                                        "description": "Bus de destino (Fase 3, default sfx)"
                                    },
                                    "looping": {
                                        "type": "boolean",
                                        "description": "Reproducir en bucle (default false)"
                                    }
                                },
                                "required": ["clip"]
                            }
                        },
                        {
                            "name": "glass_audio_buses",
                            "description": "Jerarquía de buses de audio (Fase 3): Master, Music y SFX con volumen base, volumen efectivo y estado de ducking, más las fuentes vivas con su bus y su oclusión. Sin 'action' solo consulta. Con action=set_volume fija el volumen base de un bus; con action=duck dispara el ducking de un bus (factor amount durante duration_secs).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "action": { "type": "string", "enum": ["set_volume", "duck"] },
                                    "bus": { "type": "string", "enum": ["master", "music", "sfx"] },
                                    "volume": { "type": "number" },
                                    "amount": { "type": "number" },
                                    "duration_secs": { "type": "number" }
                                }
                            }
                        },
                        {
                            "name": "glass_animator_state",
                            "description": "Estado del Animator de una entidad (Fase 4): controlador, estado actual, tiempo en el estado, fundido en curso con su origen y peso, parámetros y, si el estado es un blend tree 1D, los hijos con sus pesos y relojes según el parámetro.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": { "type": "integer" }
                                },
                                "required": ["entity_id"]
                            }
                        },
                        {
                            "name": "glass_animator_set_param",
                            "description": "Fija un parámetro (float/bool/trigger como número) del Animator de una entidad viva (Fase 4). Las transiciones y los blend trees reaccionan en el siguiente frame.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": { "type": "integer" },
                                    "param": { "type": "string" },
                                    "value": { "type": "number" }
                                },
                                "required": ["entity_id", "param", "value"]
                            }
                        },
                        {
                            "name": "glass_ui_tree",
                            "description": "Árbol de UI de la escena viva (Fase 5): factor de escala del Canvas Scaler y, por cada entidad UI, su rectángulo resuelto en píxeles (jerarquía, anclas completas y layout groups aplicados) para la resolución dada (defecto 1280x720).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "width": { "type": "number" },
                                    "height": { "type": "number" }
                                }
                            }
                        },
                        {
                            "name": "glass_ui_stats",
                            "description": "Conteos de la UI de la escena viva (Fase 5): canvas, elementos, textos, botones, imágenes, layout groups, profundidad máxima de la jerarquía y factor de escala para la resolución dada (defecto 1280x720).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "width": { "type": "number" },
                                    "height": { "type": "number" }
                                }
                            }
                        },
                        {
                            "name": "glass_transform_stats",
                            "description": "Estadísticas de la resolución de transforms SoA (Fase 7): entidades y recomputadas de la última pasada, profundidad máxima de la jerarquía, microsegundos de la pasada y total de pasadas desde el arranque.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_asset_validate",
                            "description": "Valida un asset de modelo (Fase 6): conteos del GLB, objetos, instancias enlazadas (nodos que comparten malla), grupos LOD por sufijo _LOD<n>, colecciones/prefabs del sidecar del bridge y frescura (si la fuente .blend/.fbx cambió desde la exportación, por SHA-256). Acepta la ruta del .blend/.fbx o la del .glb bajo assets/.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string" }
                                },
                                "required": ["path"]
                            }
                        },
                        {
                            "name": "glass_asset_reload",
                            "description": "Re-exporta un .blend/.fbx con el bridge de Blender en el acto (Fase 6) y devuelve el resultado con el resumen del sidecar; la escritura del .glb dispara la recarga en caliente por el vigía. Para un .glb devuelve su validación. La ruta debe estar bajo assets/.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string" }
                                },
                                "required": ["path"]
                            }
                        },
                        {
                            "name": "glass_import_unitypackage",
                            "description": "Importa un .unitypackage de Unity al proyecto abierto (Ripper): convierte texturas a PNG, materiales a assets/materials, modelos FBX a GLB, prefabs y escenas al formato del motor y scripts C# contra la fachada, y devuelve el resumen con el informe completo. 'path' puede ser absoluto o relativo a la raíz del proyecto. No sobrescribe assets existentes salvo que 'overwrite' sea true.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string" },
                                    "overwrite": { "type": "boolean" }
                                },
                                "required": ["path"]
                            }
                        },
                        {
                            "name": "glass_destroy_entity",
                            "description": "Destruye una entidad de la escena por su id (Fase 8). Es mutante: pasa por la Command Queue como las demás tools de escritura.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": { "type": "number" }
                                },
                                "required": ["entity_id"]
                            }
                        },
                        {
                            "name": "glass_prefab_list",
                            "description": "Lista los assets *.prefab.json del proyecto (bajo Assets/ y assets/) con su ruta relativa, su nombre y su número de entidades. Es de consulta: no modifica la escena.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_prefab_create",
                            "description": "Crea un asset de prefab (.prefab.json) a partir de una entidad de la escena y todos sus descendientes. La ruta debe empezar por Assets/ o assets/ y terminar en .prefab.json. Es mutante: pasa por la Command Queue como las demás tools de escritura.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": { "type": "number" },
                                    "path": { "type": "string" },
                                    "overwrite": { "type": "boolean" }
                                },
                                "required": ["entity_id", "path"]
                            }
                        },
                        {
                            "name": "glass_prefab_create_variant",
                            "description": "Crea una variante de un prefab del proyecto: un nuevo .prefab.json que hereda de la base (variant_of) con el diff vacío. Las rutas deben empezar por Assets/ o assets/ y terminar en .prefab.json. Es mutante: pasa por la Command Queue como las demás tools de escritura.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string" },
                                    "variant_path": { "type": "string" },
                                    "name": { "type": "string" },
                                    "overwrite": { "type": "boolean" }
                                },
                                "required": ["path", "variant_path"]
                            }
                        },
                        {
                            "name": "glass_prefab_instantiate",
                            "description": "Instancia un prefab del proyecto en la escena, con posición y padre opcionales, y deja escrito el vínculo vivo con su asset. Es mutante: pasa por la Command Queue como las demás tools de escritura.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string" },
                                    "position": {
                                        "type": "array",
                                        "items": { "type": "number" },
                                        "minItems": 3,
                                        "maxItems": 3
                                    },
                                    "parent_id": { "type": "number" }
                                },
                                "required": ["path"]
                            }
                        },
                        {
                            "name": "glass_prefab_revert",
                            "description": "Revierte una instancia de prefab a su asset y descarta los cambios locales. Recibe el id de la raíz de la instancia. Es mutante: pasa por la Command Queue como las demás tools de escritura.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": { "type": "number" }
                                },
                                "required": ["entity_id"]
                            }
                        },
                        {
                            "name": "glass_prefab_overrides",
                            "description": "Devuelve el diff de overrides de una instancia de prefab (Fase D): propiedades modificadas con su valor, entidades añadidas y entidades quitadas frente a su asset. Recibe el id de la raíz de la instancia. Es de solo lectura.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": { "type": "number" }
                                },
                                "required": ["entity_id"]
                            }
                        },
                        {
                            "name": "glass_queue_submit",
                            "description": "Encola un lote de comandos MUTANTES para que el bucle del motor los ejecute en orden (Fase 8), sin esperar: devuelve los command_ids asignados. Las tools de consulta, las de la propia cola y las desconocidas se rechazan sin encolar nada.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "commands": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "tool": { "type": "string" },
                                                "args": { "type": "object" }
                                            },
                                            "required": ["tool"]
                                        }
                                    }
                                },
                                "required": ["commands"]
                            }
                        },
                        {
                            "name": "glass_queue_status",
                            "description": "Estado de la Command Queue (Fase 8): con command_id, el registro completo de ese comando (estado, pasada de ejecución, resultado o error); sin él, el resumen y los últimos 10 comandos.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "command_id": { "type": "number" }
                                }
                            }
                        },
                        {
                            "name": "glass_particle_emitter",
                            "description": "Crea o configura el emisor de partículas de una entidad (Fase 8P): sin campos de configuración devuelve el actual; con ellos, los aplica sobre el dato de escena (la app reinicia el pool con la configuración nueva). Campos: rate, max_particles, lifetime [min,max], start_speed, start_size, gravity [x,y,z], drag, shape (point|sphere|box|cone), shape_radius, shape_extents, cone_angle, local_space, color_start, color_end, size_end_factor, rotation_speed, stretched, stretch_factor, seed, playing.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": { "type": "number" },
                                    "rate": { "type": "number" },
                                    "max_particles": { "type": "number" },
                                    "shape": { "type": "string" },
                                    "playing": { "type": "boolean" }
                                },
                                "required": ["entity_id"]
                            }
                        },
                        {
                            "name": "glass_particle_emit",
                            "description": "Control de un emisor de partículas en runtime (Fase 8P): action play|stop|burst (burst emite 'count' partículas al momento). El comando viaja a la app por la cola de comandos del sistema y se aplica en el siguiente frame.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": { "type": "number" },
                                    "action": { "type": "string" },
                                    "count": { "type": "number" }
                                },
                                "required": ["entity_id", "action"]
                            }
                        },
                        {
                            "name": "glass_particle_stats",
                            "description": "Presupuestos del sistema de partículas (Fase 8P): partículas vivas, emisores activos, draws del último frame, coste de simulación en microsegundos y overdraw estimado.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
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
                        },
                        {
                            "name": "glass_bake_lightmap",
                            "description": "Hornea el lightmap de la escena actual (sol + luces puntuales, con oclusión por raycasts) y lo escribe en assets/lightmaps/<nombre>.lm.png/.json, listo para que el runtime lo cargue.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "name": {
                                        "type": "string",
                                        "description": "Nombre base de los archivos (sin extensión). Por defecto 'LiveScene'."
                                    },
                                    "density": {
                                        "type": "number",
                                        "description": "Densidad de téxeles por metro (default 8)"
                                    }
                                }
                            }
                        },
                        {
                            "name": "glass_get_input_axes",
                            "description": "Devuelve los ejes de entrada configurados (Input Manager): nombre, teclas, gravity, dead, sensitivity, snap e invert.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_list_materials",
                            "description": "Lista los materiales disponibles en assets/materials/ con sus parámetros (albedo, textura, metallic, roughness, emisivo, tiling, transparente).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_set_entity_material",
                            "description": "Asigna un material de assets/materials/ a la malla de una entidad (nombre sin extensión .material.json; cadena vacía para quitarlo).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": { "type": "integer", "description": "Id de la entidad" },
                                    "material": { "type": "string", "description": "Nombre del material" }
                                },
                                "required": ["entity_id", "material"]
                            }
                        },
                        {
                            "name": "glass_set_ui_text",
                            "description": "Pone el texto del componente UIText de una entidad del Canvas (lo crea con valores por defecto si la entidad no lo tiene).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": { "type": "integer", "description": "Id de la entidad" },
                                    "text": { "type": "string", "description": "Texto a mostrar" }
                                },
                                "required": ["entity_id", "text"]
                            }
                        },
                        {
                            "name": "glass_batch_spawn_entities",
                            "description": "Crea varias entidades en una sola llamada bajo el mismo candado de escena. Valida todas las especificaciones antes de crear: si alguna es inválida o no cabe en la escena, no se crea ninguna.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entities": {
                                        "type": "array",
                                        "description": "Especificaciones a crear; no vacío y como máximo 256 elementos.",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "name": {
                                                    "type": "string",
                                                    "description": "Nombre de la entidad; obligatorio, no vacío y de hasta 256 caracteres."
                                                },
                                                "components": {
                                                    "type": "array",
                                                    "items": { "type": "string" },
                                                    "description": "Componentes iniciales; por defecto [\"Transform\"] y como máximo 64."
                                                },
                                                "position": {
                                                    "type": "array",
                                                    "items": { "type": "number" },
                                                    "minItems": 3,
                                                    "maxItems": 3,
                                                    "description": "Posición opcional [x, y, z] con valores finitos."
                                                }
                                            },
                                            "required": ["name"]
                                        }
                                    }
                                },
                                "required": ["entities"]
                            }
                        },
                        {
                            "name": "glass_inspect_spatial",
                            "description": "Lista las entidades cuyo transform está dentro de un radio alrededor de un punto, ordenadas por distancia ascendente y truncadas al límite indicado.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "center": {
                                        "type": "array",
                                        "items": { "type": "number" },
                                        "minItems": 3,
                                        "maxItems": 3,
                                        "description": "Centro de la consulta [x, y, z] con valores finitos."
                                    },
                                    "radius": {
                                        "type": "number",
                                        "description": "Radio de búsqueda; finito y mayor que cero."
                                    },
                                    "limit": {
                                        "type": "integer",
                                        "description": "Número máximo de resultados; por defecto 50 y tope 500.",
                                        "minimum": 1
                                    }
                                },
                                "required": ["center", "radius"]
                            }
                        },
                        {
                            "name": "glass_profiler_snapshot",
                            "description": "Devuelve una foto de los contadores medibles en este servidor (entidades, componentes de datos presentes, siguiente id, flag de shaders y tamaño de los sets físicos). Los tiempos de frame y la memoria viven en glass_frame_timing y glass_memory_stats.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_list_portals",
                            "description": "Lista las entidades con componente Portal de la escena: su id, nombre, pareja (pair), tamaño y posición, y el id de la entidad pareja con el mismo pair (o null si no tiene pareja en la escena).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_frame_timing",
                            "description": "Devuelve las estadísticas de tiempo por fase de frame (update, física, render, present y GPU del pase principal) sobre las últimas 240 muestras registradas por el motor en este proceso: media, p95 y máximo por fase. En modo MCP headless, sin motor gráfico en el proceso, no hay muestras y la respuesta lo declara.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_memory_stats",
                            "description": "Devuelve la memoria del proceso (RSS actual y pico; en Linux, de /proc/self/status) y la memoria GPU del allocator de Vulkan (asignada y capacidad) si el motor la registró en este proceso. Lo no disponible va como null, nunca inventado.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_physics_cast",
                            "description": "Lanza una consulta física contra el mundo vivo (Fase 1): rayo, barrido de esfera o de cápsula con máscara de capas, u overlap de esfera/caja. Devuelve el primer impacto o las entidades solapadas.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "kind": {"type": "string", "enum": ["ray", "sphere", "capsule", "overlap_sphere", "overlap_box"]},
                                    "origin": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": "Origen del barrido o centro del overlap [x, y, z]."},
                                    "direction": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": "Dirección del barrido (ray/sphere/capsule)."},
                                    "radius": {"type": "number", "description": "Radio (sphere/capsule/overlap_sphere)."},
                                    "half_height": {"type": "number", "description": "Mitad del tramo recto (capsule)."},
                                    "half_extents": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": "Semiejes de la caja (overlap_box)."},
                                    "max_dist": {"type": "number", "default": 100.0},
                                    "mask": {"type": "integer", "default": u32::MAX, "description": "Máscara de capas visibles para la consulta."},
                                    "exclude_entity": {"type": "integer", "default": 0}
                                },
                                "required": ["kind", "origin"]
                            }
                        },
                        {
                            "name": "glass_character_state",
                            "description": "Estado del CharacterController (bloque CC) de una entidad: si existe, la posición del centro de su cápsula y si está apoyado (grounded) o deslizándose por una pendiente (sliding), del mundo físico vivo.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": {"type": "integer", "description": "Id de la entidad con CharacterController."}
                                },
                                "required": ["entity_id"]
                            }
                        },
                        {
                            "name": "glass_mesh_enter_edit",
                            "description": "Entra en edición de malla (MeshForge): carga un .glb bajo assets/ en una sesión en proceso y la deja lista para aplicar operadores.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": {"type": "string", "description": "Ruta bajo assets/, p. ej. assets/models/cubo.glb"}
                                },
                                "required": ["path"]
                            }
                        },
                        {
                            "name": "glass_mesh_apply_op",
                            "description": "Aplica un operador a la sesión de malla activa: el comando va como JSON de MeshCommand (move_verts, set_vert, scale_selection, mirror, subdivide, array, weld, delete_verts, boolean).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "command": {"type": "object", "description": "MeshCommand serializado en JSON."}
                                },
                                "required": ["command"]
                            }
                        },
                        {
                            "name": "glass_mesh_undo",
                            "description": "Deshace el último operador de la sesión de malla activa.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_mesh_redo",
                            "description": "Rehace el último operador deshecho de la sesión de malla activa.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_mesh_set_dimension",
                            "description": "Dimensionado numérico exacto: escala la malla de la sesión activa para que su caja envolvente sea exactamente `size` [x, y, z] (metros).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "size": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": "Tamaño final [x, y, z] en metros."}
                                },
                                "required": ["size"]
                            }
                        },
                        {
                            "name": "glass_mesh_get_stats",
                            "description": "Estadísticas de la sesión de malla activa: conteos, bbox, área y validación manifold.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_mesh_bake",
                            "description": "Hornea la sesión de malla activa: escribe el .glb resultante y el sidecar de receta (.mesh.json) bajo assets/.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "glb_path": {"type": "string", "description": "Ruta del .glb de salida bajo assets/."},
                                    "recipe_path": {"type": "string", "description": "Ruta del sidecar .mesh.json bajo assets/."}
                                },
                                "required": ["glb_path", "recipe_path"]
                            }
                        },
                        {
                            "name": "glass_list_contact_events",
                            "description": "Pares de contacto activos y últimos eventos Enter/Exit del mundo físico vivo (Fase 1), con el contador de substeps.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "limit": {"type": "integer", "default": 32, "description": "Máximo de eventos recientes a devolver."}
                                }
                            }
                        },
                        {
                            "name": "glass_render_stats",
                            "description": "Estadísticas de render del último frame recolectado (Fase 2): entidades consideradas y descartadas por frustum culling en la vista de juego, y draw calls por vista y del pase de sombras.",
                            "inputSchema": {"type": "object", "properties": {}}
                        },
                        {
                            "name": "glass_render_lighting_debug",
                            "description": "Iluminación del último frame (Fase 2S): luz direccional principal, luces puntuales publicadas en el UBO (posición, color, intensidad, rango y cubo de sombra) y niebla de la escena. Es la misma foto que consumen los nodos de luz de los grafos.",
                            "inputSchema": {"type": "object", "properties": {}}
                        },
                        {
                            "name": "glass_light_assignment",
                            "description": "Asignación de luces POR OBJETO del último frame (integración de iluminación): dada una entidad con malla (entity_id), devuelve su ranura en el buffer dinámico de luces y los ids de las luces que la alumbran según la asignación con histéresis. Sin entity_id devuelve el modo (global/por objeto) y cuántos objetos tienen asignación.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": {"type": "integer", "description": "Id de la entidad a consultar (opcional)."}
                                }
                            }
                        },
                        
                        {
                            "name": "glass_texture_info",
                            "description": "Texturas cargadas en VRAM (Fase 2): formato Vulkan (RGBA8 o BC), dimensiones, niveles de mip y bytes por textura, con el total. Parámetro opcional model para filtrar por clave de modelo.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "model": {"type": "string", "description": "Clave de modelo a filtrar (opcional)."}
                                }
                            }
                        },
                        {
                            "name": "glass_shader_schema_get",
                            "description": "Esquema de propiedades declaradas por un grafo de shaders (Fase 2S): nombre, tipo y valor por defecto de cada propiedad editable desde el material. Parámetro graph requerido.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "graph": {"type": "string", "description": "Nombre del grafo de shaders."}
                                }
                            }
                        },
                        {
                            "name": "glass_material_property_set",
                            "description": "Fija el valor de una propiedad del shader de grafo de una entidad viva (Fase 2S): se escribe en MeshData.shader_properties y el motor lo sube a su UBO de propiedades en el siguiente frame.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "entity_id": {"type": "number", "description": "ID de la entidad."},
                                    "property": {"type": "string", "description": "Nombre de la propiedad del shader."},
                                    "value": {"description": "Nuevo valor de la propiedad (tipo según el esquema del grafo)."}
                                }
                            }
                        },
                        {
                            "name": "glass_shader_graph_validate",
                            "description": "Valida un grafo de shader en disco (Fase 2S): lee assets/shader_graphs/<graph>.graph.json, comprueba que el JSON deserializa como ShaderGraph y que compila el fragment y el vertex shader. Devuelve valid=false con el error concreto si algo falla.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "graph": {"type": "string", "description": "Nombre del grafo (sin extensión); se resuelve como assets/shader_graphs/<graph>.graph.json."}
                                },
                                "required": ["graph"]
                            }
                        },
                        {
                            "name": "glass_shader_node_add",
                            "description": "Añade un nodo a un grafo de shader editando el .graph.json en disco (Fase 2S): carga assets/shader_graphs/<graph>.graph.json, inserta el nodo con un id nuevo (máximo + 1) y reescribe el archivo. El cambio requiere recompilar el .spv para verse en el motor.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "graph": {"type": "string", "description": "Nombre del grafo (sin extensión); se resuelve como assets/shader_graphs/<graph>.graph.json."},
                                    "kind": {"type": "object", "description": "NodeKind en su forma serde, p. ej. {\"kind\": \"Float\", \"value\": 0.5} o {\"kind\": \"Output\"}."},
                                    "position": {
                                        "type": "array",
                                        "items": {"type": "number"},
                                        "description": "Posición [x, y] del nodo en el editor (por defecto [0.0, 0.0])."
                                    }
                                },
                                "required": ["graph", "kind"]
                            }
                        },
                        {
                            "name": "glass_shader_compile_check",
                            "description": "Comprobación completa de un grafo (Fase 2S): parsea y compila el .graph.json a GLSL y verifica que su .spv exista en assets/shaders. ready=true solo si compila y el .spv está presente; si no, el motor dibuja esas entidades con el material de error magenta.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "graph": {
                                        "type": "string",
                                        "description": "Nombre del grafo (sin ruta ni extensión)."
                                    }
                                },
                                "required": ["graph"]
                            }
                        },
                        {
                            "name": "glass_shader_pass_configure",
                            "description": "Fija los pases de render de un grafo en su .graph.json en disco (Fase 2S): Forward (dibujo normal), Shadow (su etapa de vértices proyecta la sombra del sol, con recorte si declara la propiedad alpha_clip) y Outline (contorno toon, S5). El cambio se aplica al reiniciar el motor, que es cuando se registran los pipelines.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "graph": {"type": "string", "description": "Nombre del grafo (sin extensión ni ruta)."},
                                    "passes": {"type": "array", "items": {"type": "string"}, "description": "Lista de pases: subconjunto de [\"Forward\", \"Shadow\", \"Outline\"]."}
                                },
                                "required": ["graph", "passes"]
                            }
                        },
                        {
                            "name": "glass_shader_preset_apply",
                            "description": "Aplica un preset de propiedades a una entidad viva (Fase 2S): lee assets/shader_graphs/<graph>.<preset>.preset.json (un objeto con 'properties') y escribe cada propiedad en los shader_properties de la entidad, que debe usar ese grafo. El efecto es inmediato en el siguiente frame.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "graph": { "type": "string" },
                                    "preset": { "type": "string" },
                                    "entity_id": { "type": "number" }
                                },
                                "required": ["graph", "preset", "entity_id"]
                            }
                        },
                        {
                            "name": "glass_render_pass_stats",
                            "description": "Draw calls por pase del último frame (Fase 2S): forward (vistas de juego y escena), sombras del sol, sombras con vértices de grafo y contornos.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "glass_render_draw_stats",
                            "description": "Enlaces del último frame renderizado (Fase 2S, S6): cuántos pipelines y descriptor sets se enlazaron en los pases de vista, con los draw calls forward de referencia. Tras la ordenación de draws, los enlaces de pipeline se acercan al número de shaders distintos.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        }
                    ]
                });
                // Puerta del complemento: sin la herramienta 3D no se anuncian sus tools.
                if !self.mesh_tools_enabled() {
                    if let Some(tools) = result.get_mut("tools").and_then(|t| t.as_array_mut()) {
                        tools.retain(|t| {
                            !matches!(
                                t.get("name").and_then(|n| n.as_str()),
                                Some(n) if MESH_TOOL_NAMES.contains(&n)
                            )
                        });
                    }
                }
                McpResponse::success(req.id, result)
            }

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

                self.call_tool(req.id, name, args)
            }

            _ => McpResponse::method_not_found(req.id, &req.method),
        }
    }

    /// Puerta de entrada de las tools (Fase 8): una tool mutante
    /// con el bucle bombeando se ENCOLA y esta llamada espera su
    /// resultado (la semántica síncrona del cliente no cambia; la
    /// ejecución ocurre en orden dentro del bucle del motor). Sin
    /// bombeo vivo, se ejecuta directa y queda registrada.
    fn call_tool(
        &self,
        id: Option<serde_json::Value>,
        name: &str,
        args: serde_json::Value,
    ) -> McpResponse {
        if !is_mutating_tool(name) || name == "glass_queue_submit" {
            return self.execute_tool(id, name, args);
        }
        if self.queue.pump_alive() {
            let cid = self.queue.submit(name, args);
            match self.queue.wait(cid, std::time::Duration::from_secs(15)) {
                Some(crate::queue::CmdState::Done) => {
                    let c = self.queue.status(cid).unwrap();
                    McpResponse::success(id, c.result.unwrap_or(serde_json::json!({})))
                }
                Some(crate::queue::CmdState::Error) => {
                    let c = self.queue.status(cid).unwrap();
                    McpResponse::error(
                        id,
                        -32006,
                        c.error.unwrap_or_else(|| "Error de comando".into()),
                    )
                }
                _ => McpResponse::error(
                    id,
                    -32007,
                    format!(
                        "El comando {cid} sigue encolado tras 15 s; su estado se consulta con glass_queue_status"
                    ),
                ),
            }
        } else {
            // Sin bucle (stdio puro o arranque): directo, registrado.
            let cid = self.queue.submit(name, args.clone());
            let resp = self.execute_tool(id, name, args);
            match (&resp.result, &resp.error) {
                (_, Some(e)) => self
                    .queue
                    .complete(cid, Err(e.message.clone()), 0, true),
                (Some(r), _) => self.queue.complete(cid, Ok(r.clone()), 0, true),
                _ => self.queue.complete(cid, Ok(serde_json::json!({})), 0, true),
            }
            resp
        }
    }

    /// Bombeo de la Command Queue (Fase 8): lo llama el bucle del
    /// motor una vez por frame, antes de física y scripts. Ejecuta
    /// los pendientes en orden de envío y guarda cada desenlace.
    pub fn pump_queue(&self) {
        let pass = self.queue.note_pump();
        for cmd in self.queue.take_pending() {
            let resp = self.execute_tool(None, &cmd.tool, cmd.args.clone());
            match (&resp.result, &resp.error) {
                (_, Some(e)) => self
                    .queue
                    .complete(cmd.id, Err(e.message.clone()), pass, false),
                (Some(r), _) => self
                    .queue
                    .complete(cmd.id, Ok(r.clone()), pass, false),
                _ => self
                    .queue
                    .complete(cmd.id, Ok(serde_json::json!({})), pass, false),
            }
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

                if entity_name.chars().count() > MAX_ENTITY_NAME_CHARS {
                    return McpResponse::invalid_params(id, "Field 'name' exceeds 256 characters");
                }
                if components.len() > MAX_COMPONENTS_PER_CALL {
                    return McpResponse::invalid_params(id, "Too many components in one call (max 64)");
                }
                if scene.entities.len() >= MAX_ENTITIES_PER_SCENE {
                    return McpResponse::error(id, -32003, "Scene capacity reached (10000 entities)");
                }

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

            // Crea N entidades en una sola llamada: valida todo
            // primero y crea después (un lote inválido no crea nada).
            "glass_batch_spawn_entities" => {
                let specs = match args.get("entities").and_then(|v| v.as_array()) {
                    Some(a) => a,
                    None => {
                        return McpResponse::invalid_params(
                            id,
                            "glass_batch_spawn_entities requiere 'entities' como array",
                        )
                    }
                };
                if specs.is_empty() {
                    return McpResponse::invalid_params(id, "'entities' no puede estar vacío");
                }
                if specs.len() > MAX_BATCH_SPAWN {
                    return McpResponse::invalid_params(
                        id,
                        &format!("'entities' supera el máximo de {} elementos", MAX_BATCH_SPAWN),
                    );
                }

                // Fase 1: validar todas las especificaciones sin tocar la escena.
                let mut plan: Vec<(String, Vec<String>, Option<[f32; 3]>)> =
                    Vec::with_capacity(specs.len());
                for (i, spec) in specs.iter().enumerate() {
                    let obj = match spec.as_object() {
                        Some(o) => o,
                        None => {
                            return McpResponse::invalid_params(
                                id,
                                &format!("entities[{}] debe ser un objeto", i),
                            )
                        }
                    };

                    let name = match obj.get("name").and_then(|v| v.as_str()) {
                        Some(n) if !n.trim().is_empty() => n.to_string(),
                        _ => {
                            return McpResponse::invalid_params(
                                id,
                                &format!(
                                    "entities[{}].name es obligatorio y no puede estar vacío",
                                    i
                                ),
                            )
                        }
                    };
                    if name.chars().count() > MAX_ENTITY_NAME_CHARS {
                        return McpResponse::invalid_params(
                            id,
                            &format!(
                                "entities[{}].name supera {} caracteres",
                                i, MAX_ENTITY_NAME_CHARS
                            ),
                        );
                    }

                    let components = match obj.get("components") {
                        None => vec!["Transform".to_string()],
                        Some(v) => {
                            let arr = match v.as_array() {
                                Some(a) => a,
                                None => {
                                    return McpResponse::invalid_params(
                                        id,
                                        &format!(
                                            "entities[{}].components debe ser un array de strings",
                                            i
                                        ),
                                    )
                                }
                            };
                            let mut out = Vec::with_capacity(arr.len());
                            for c in arr {
                                match c.as_str() {
                                    Some(s) => out.push(s.to_string()),
                                    None => {
                                        return McpResponse::invalid_params(
                                            id,
                                            &format!(
                                                "entities[{}].components solo admite strings",
                                                i
                                            ),
                                        )
                                    }
                                }
                            }
                            out
                        }
                    };
                    if components.len() > MAX_COMPONENTS_PER_CALL {
                        return McpResponse::invalid_params(
                            id,
                            &format!(
                                "entities[{}].components supera {} elementos",
                                i, MAX_COMPONENTS_PER_CALL
                            ),
                        );
                    }

                    let position = match obj.get("position") {
                        None => None,
                        Some(v) => {
                            let arr = match v.as_array() {
                                Some(a) if a.len() == 3 => a,
                                _ => {
                                    return McpResponse::invalid_params(
                                        id,
                                        &format!("entities[{}].position debe ser [x, y, z]", i),
                                    )
                                }
                            };
                            let mut p = [0.0f64; 3];
                            for (k, comp) in arr.iter().enumerate() {
                                match comp.as_f64() {
                                    Some(x) if x.is_finite() => p[k] = x,
                                    _ => {
                                        return McpResponse::invalid_params(
                                            id,
                                            &format!(
                                                "entities[{}].position[{}] debe ser un número finito",
                                                i, k
                                            ),
                                        )
                                    }
                                }
                            }
                            Some([p[0] as f32, p[1] as f32, p[2] as f32])
                        }
                    };

                    plan.push((name, components, position));
                }

                // Capacidad de la escena: si no cabe el lote completo, no se crea nada.
                if scene.entities.len() + plan.len() > MAX_ENTITIES_PER_SCENE {
                    return McpResponse::error(
                        id,
                        -32003,
                        format!(
                            "capacidad de escena excedida: {} + {} > {}",
                            scene.entities.len(),
                            plan.len(),
                            MAX_ENTITIES_PER_SCENE
                        ),
                    );
                }

                // Fase 2: crear y aplicar posición.
                let mut ids: Vec<u64> = Vec::with_capacity(plan.len());
                for (name, components, position) in plan {
                    let nuevo_id = scene.create_entity(name, components);
                    if let Some(p) = position {
                        scene.modify_transform(nuevo_id, Some(p), None, None);
                    }
                    ids.push(nuevo_id);
                }
                let ids_txt = ids
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<String>>()
                    .join(", ");
                McpResponse::success(
                    id,
                    json!({"content":[{"type":"text","text": format!(
                        "Creadas {} entidades. ids asignados: [{}]",
                        ids.len(),
                        ids_txt
                    )}]}),
                )
            }

            // Entidades cercanas a un punto, ordenadas por distancia ascendente.
            "glass_inspect_spatial" => {
                let center = match args.get("center").and_then(|v| v.as_array()) {
                    Some(a) if a.len() == 3 => {
                        let mut c = [0.0f64; 3];
                        for (k, comp) in a.iter().enumerate() {
                            match comp.as_f64() {
                                Some(x) if x.is_finite() => c[k] = x,
                                _ => {
                                    return McpResponse::invalid_params(
                                        id,
                                        &format!("center[{}] debe ser un número finito", k),
                                    )
                                }
                            }
                        }
                        c
                    }
                    _ => {
                        return McpResponse::invalid_params(
                            id,
                            "glass_inspect_spatial requiere 'center' como [x, y, z] finito",
                        )
                    }
                };

                let radius = match args.get("radius").and_then(|v| v.as_f64()) {
                    Some(r) if r.is_finite() && r > 0.0 => r,
                    _ => {
                        return McpResponse::invalid_params(
                            id,
                            "'radius' debe ser un número finito mayor que cero",
                        )
                    }
                };

                let limit = match args.get("limit") {
                    None => 50usize,
                    Some(v) => {
                        let n = match v.as_i64() {
                            Some(n) => n,
                            None => {
                                return McpResponse::invalid_params(
                                    id,
                                    "'limit' debe ser un entero",
                                )
                            }
                        };
                        if n <= 0 {
                            return McpResponse::invalid_params(
                                id,
                                "'limit' debe ser un entero positivo",
                            );
                        }
                        (n as usize).min(500)
                    }
                };

                let radio2 = radius * radius;
                let mut halladas: Vec<(f64, u64, String, [f32; 3])> = Vec::new();
                for e in &scene.entities {
                    let p = e.transform.position;
                    let dx = p[0] as f64 - center[0];
                    let dy = p[1] as f64 - center[1];
                    let dz = p[2] as f64 - center[2];
                    let d2 = dx * dx + dy * dy + dz * dz;
                    if d2 <= radio2 {
                        halladas.push((d2.sqrt(), e.id, e.name.clone(), p));
                    }
                }
                let total = halladas.len();
                halladas.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                halladas.truncate(limit);

                let items: Vec<serde_json::Value> = halladas
                    .iter()
                    .map(|(d, eid, nombre, p)| {
                        json!({
                            "id": eid,
                            "name": nombre,
                            "position": [p[0], p[1], p[2]],
                            // Distancia con tres decimales: redondeo a milésimas en f64.
                            "distance": (d * 1000.0).round() / 1000.0,
                        })
                    })
                    .collect();
                let pretty = serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".to_string());
                let texto = format!(
                    "Centro: [{}, {}, {}] | Radio: {} | Encontradas: {}\n{}",
                    center[0], center[1], center[2], radius, total, pretty
                );
                McpResponse::success(id, json!({"content":[{"type":"text","text": texto}]}))
            }

            // Foto honesta de los contadores medibles en este crate
            // (sin VRAM ni tiempos de frame: no viven aquí).
            "glass_profiler_snapshot" => {
                let entities_total = scene.entities.len();
                let mut entities_with_mesh = 0usize;
                let mut entities_with_physics = 0usize;
                let mut entities_with_script = 0usize;
                for e in &scene.entities {
                    if e.mesh.is_some() {
                        entities_with_mesh += 1;
                    }
                    if e.physics.is_some() {
                        entities_with_physics += 1;
                    }
                    if e.script.is_some() {
                        entities_with_script += 1;
                    }
                }
                let next_entity_id = scene.next_entity_id;
                let shaders_reloaded_flag = scene.shaders_reloaded_flag;

                // Mismo orden establecido que otros brazos: candado
                // de escena tomado, luego el del mundo físico.
                let (physics_bodies, physics_colliders) = {
                    let fisica = self.get_physics();
                    let mundo = match fisica.lock() {
                        Ok(w) => w,
                        Err(_) => {
                            return McpResponse::error(
                                id,
                                -32003,
                                "no se pudo bloquear el mundo físico",
                            )
                        }
                    };
                    (mundo.rigid_body_set.len(), mundo.collider_set.len())
                };

                let snapshot = json!({
                    "entities_total": entities_total,
                    "entities_with_mesh": entities_with_mesh,
                    "entities_with_physics": entities_with_physics,
                    "entities_with_script": entities_with_script,
                    "next_entity_id": next_entity_id,
                    "shaders_reloaded_flag": shaders_reloaded_flag,
                    "physics_bodies": physics_bodies,
                    "physics_colliders": physics_colliders,
                });
                let pretty = serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| snapshot.to_string());
                McpResponse::success(id, json!({"content":[{"type":"text","text": pretty}]}))
            }

            // Portales de la escena (bloque Portales): inventario de
            // superficies con su pareja declarada.
            "glass_list_portals" => {
                let mut portals = Vec::new();
                for entity in scene.entities.iter() {
                    if let Some(portal) = &entity.portal {
                        let partner_id = scene
                            .entities
                            .iter()
                            .find(|other| {
                                other.id != entity.id
                                    && other
                                        .portal
                                        .as_ref()
                                        .map(|p| p.pair == portal.pair)
                                        .unwrap_or(false)
                            })
                            .map(|other| other.id);
                        let position = entity.transform.position;
                        portals.push(json!({
                            "entity_id": entity.id,
                            "name": entity.name,
                            "pair": portal.pair,
                            "width": portal.width,
                            "height": portal.height,
                            "position": [position[0], position[1], position[2]],
                            "partner_id": partner_id
                        }));
                    }
                }
                let count = portals.len();
                let payload = json!({"portals": portals, "count": count});
                let pretty = serde_json::to_string_pretty(&payload)
                    .unwrap_or_else(|_| payload.to_string());
                McpResponse::success(id, json!({"content":[{"type":"text","text": pretty}]}))
            }

            // Tiempos por fase de frame (Fase 0): estadísticas sobre las
            // últimas 240 muestras registradas por el motor en ESTE proceso.
            "glass_frame_timing" => {
                let snap = glass_core::profiling::frame_timing_snapshot();
                let mut datos = serde_json::to_value(snap).unwrap_or(json!({}));
                if snap.samples == 0 {
                    // Sin motor gráfico en el proceso no hay muestras: se
                    // declara en vez de devolver una tabla de ceros.
                    if let serde_json::Value::Object(ref mut mapa) = datos {
                        mapa.insert(
                            "note".to_string(),
                            json!("sin muestras: ningún frame registrado en este proceso (modo MCP headless o motor aún sin frames)"),
                        );
                    }
                }
                let texto = serde_json::to_string_pretty(&datos).unwrap_or_default();
                McpResponse::success(id, json!({"content": [{"type": "text", "text": texto}]}))
            }

            // Memoria del proceso (RSS) y del allocator GPU (Fase 0):
            // lo que no se puede medir va como null, nunca inventado.
            "glass_memory_stats" => {
                let pm = glass_core::profiling::process_memory_snapshot();
                let gpu = glass_core::profiling::gpu_memory_snapshot();
                let datos = json!({
                    "process": serde_json::to_value(pm).unwrap_or(json!({})),
                    "gpu": serde_json::to_value(gpu).unwrap_or(serde_json::Value::Null)
                });
                let texto = serde_json::to_string_pretty(&datos).unwrap_or_default();
                McpResponse::success(id, json!({"content": [{"type": "text", "text": texto}]}))
            }

            // Estadísticas de render del último frame (Fase 2): culling
            // por frustum y draw calls por vista, del registro global
            // que alimenta la app al recolectar los draws.
            "glass_render_stats" => {
                let stats = glass_core::render_stats::render_stats_snapshot();
                let drawn = stats.entities_considered.saturating_sub(stats.entities_culled);
                let pct = if stats.entities_considered > 0 {
                    (stats.entities_culled as f64 * 1000.0 / stats.entities_considered as f64).round() / 10.0
                } else {
                    0.0
                };
                let mut datos = json!({
                    "entities_considered": stats.entities_considered,
                    "entities_culled": stats.entities_culled,
                    "entities_drawn": drawn,
                    "culling_pct": pct,
                    "game_draw_calls": stats.game_draw_calls,
                    "scene_draw_calls": stats.scene_draw_calls,
                    "shadow_draw_calls": stats.shadow_draw_calls,
                    "scene_triangles": stats.scene_triangles,
                    "game_triangles": stats.game_triangles
                });
                if stats.entities_considered == 0 {
                    datos.as_object_mut().unwrap().insert(
                        "note".to_string(),
                        json!("sin datos: ningún frame recolectado en este proceso (modo MCP headless o motor aún sin frames)"),
                    );
                }
                let texto = serde_json::to_string_pretty(&datos).unwrap_or_default();
                McpResponse::success(id, json!({"content": [{"type": "text", "text": texto}]}))
            }
            "glass_render_lighting_debug" => {
                let snap = glass_core::lighting_stats::lighting_stats_snapshot();
                let puntos: Vec<_> = (0..(snap.point_count as usize).min(8))
                    .map(|i| {
                        let p = snap.points[i];
                        json!({
                            "position": p.position,
                            "range": p.range,
                            "color": p.color,
                            "intensity": p.intensity,
                            "shadow_cube": p.shadow_cube,
                        })
                    })
                    .collect();
                let mut datos = json!({
                    "directional": {
                        "present": snap.has_directional,
                        "direction": snap.directional_dir,
                        "color": snap.directional_color,
                    },
                    "point_lights": puntos,
                    "point_count": snap.point_count,
                    "fog": {
                        "enabled": snap.fog_enabled,
                        "color": snap.fog_color,
                        "near": snap.fog_near,
                        "far": snap.fog_far,
                    },
                });
                if !snap.has_directional && snap.point_count == 0 {
                    datos.as_object_mut().unwrap().insert(
                        "note".to_string(),
                        json!("sin datos: ningún frame con iluminación recolectado en este proceso"),
                    );
                }
                let texto = serde_json::to_string_pretty(&datos).unwrap_or_default();
                McpResponse::success(id, json!({"content": [{"type": "text", "text": texto}]}))
            }

            // Texturas en VRAM (Fase 2): formato, tamaño y bytes de la
            // caché de assets global que registra la app al arrancar.
            "glass_light_assignment" => {
                let snap = glass_core::lighting_stats::light_assignments_snapshot();
                let datos = match args.get("entity_id").and_then(|v| v.as_u64()) {
                    Some(entidad) => match snap.objects.get(&entidad) {
                        Some(a) => json!({
                            "entity_id": entidad,
                            "slot": a.slot,
                            "lights": a.lights,
                            "light_count": a.lights.len(),
                            "global_mode": snap.global_mode,
                        }),
                        None => json!({
                            "entity_id": entidad,
                            "slot": 0,
                            "lights": [],
                            "light_count": 0,
                            "global_mode": snap.global_mode,
                            "note": "sin asignación propia: usa el UBO global (ranura 0)",
                        }),
                    },
                    None => json!({
                        "global_mode": snap.global_mode,
                        "assigned_objects": snap.objects.len(),
                    }),
                };
                let texto = serde_json::to_string_pretty(&datos).unwrap_or_default();
                McpResponse::success(id, json!({"content": [{"type": "text", "text": texto}]}))
            }
            "glass_texture_info" => {
                let filtro = args.get("model").and_then(|m| m.as_str()).map(|s| s.to_string());
                let datos = match glass_renderer::global_asset_cache() {
                    Some(cache) => match cache.read() {
                        Ok(cache) => {
                            let mut lista: Vec<serde_json::Value> = cache
                                .textures
                                .iter()
                                .filter(|((modelo, _), _)| {
                                    filtro.as_ref().map(|f| f == modelo).unwrap_or(true)
                                })
                                .map(|((modelo, indice), tex)| {
                                    json!({
                                        "model": modelo,
                                        "image_index": indice,
                                        "format": format!("{:?}", tex.format),
                                        "width": tex.width,
                                        "height": tex.height,
                                        "mip_levels": tex.mip_levels,
                                        "vram_bytes": tex.vram_bytes
                                    })
                                })
                                .collect();
                            lista.sort_by(|a, b| {
                                let ma = a["model"].as_str().unwrap_or("");
                                let mb = b["model"].as_str().unwrap_or("");
                                ma.cmp(mb).then(
                                    a["image_index"].as_u64().unwrap_or(0)
                                        .cmp(&b["image_index"].as_u64().unwrap_or(0)),
                                )
                            });
                            let total: u64 = lista
                                .iter()
                                .map(|t| t["vram_bytes"].as_u64().unwrap_or(0))
                                .sum();
                            json!({
                                "textures": lista,
                                "count": lista.len(),
                                "total_vram_bytes": total
                            })
                        }
                        Err(_) => json!({"note": "la caché de assets está bloqueada (lock envenenado)"}),
                    },
                    None => json!({"note": "la caché de assets no está registrada: esta tool requiere el motor en marcha (modo --mcp-live)"}),
                };
                let texto = serde_json::to_string_pretty(&datos).unwrap_or_default();
                McpResponse::success(id, json!({"content": [{"type": "text", "text": texto}]}))
            }

"glass_shader_schema_get" => {
                let graph = match args.get("graph").and_then(|v| v.as_str()) {
                    Some(g) => g,
                    None => return McpResponse::invalid_params(id, "Field 'graph' is required"),
                };
                match glass_core::shader_graph::graph_schema(graph) {
                    None => McpResponse::success(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string_pretty(&json!({
                                    "graph": graph,
                                    "note": "sin esquema registrado con ese nombre (los esquemas se registran al arrancar desde assets/shader_graphs)"
                                })).unwrap()
                            }]
                        }),
                    ),
                    Some(props) => {
                        let properties: Vec<serde_json::Value> = props
                            .iter()
                            .map(|p| {
                                json!({
                                    "name": p.name,
                                    "kind": serde_json::to_value(p.kind).unwrap(),
                                    "default": serde_json::to_value(&p.default).unwrap()
                                })
                            })
                            .collect();
                        McpResponse::success(
                            id,
                            json!({
                                "content": [{
                                    "type": "text",
                                    "text": serde_json::to_string_pretty(&json!({
                                        "graph": graph,
                                        "properties": properties
                                    })).unwrap()
                                }]
                            }),
                        )
                    }
                }
            }
            "glass_material_property_set" => {
                let entity_id = match args.get("entity_id").and_then(|n| n.as_u64()) {
                    Some(v) => v,
                    None => return McpResponse::invalid_params(id, "Field 'entity_id' is required"),
                };
                let property = match args.get("property").and_then(|v| v.as_str()) {
                    Some(p) => p.to_string(),
                    None => return McpResponse::invalid_params(id, "Field 'property' is required"),
                };
                let value = match args.get("value").cloned() {
                    Some(v) => v,
                    None => return McpResponse::invalid_params(id, "Field 'value' is required"),
                };
                let Some(ent) = scene.entities.iter_mut().find(|e| e.id == entity_id) else {
                    return McpResponse::error(id, -32004, "Entity not found");
                };
                if ent.mesh.is_none() {
                    return McpResponse::invalid_params(id, "La entidad no tiene MeshData");
                }
                ent.mesh
                    .as_mut()
                    .unwrap()
                    .shader_properties
                    .insert(property.clone(), value.clone());
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&json!({
                                "ok": true,
                                "entity_id": entity_id,
                                "property": property,
                                "value": value
                            })).unwrap()
                        }]
                    }),
                )
            }
            "glass_shader_graph_validate" => {
                let graph = match args.get("graph").and_then(|v| v.as_str()) {
                    Some(g) => g,
                    None => return McpResponse::invalid_params(id, "Field 'graph' is required"),
                };
                if graph.contains('/') || graph.contains('\\') || graph.contains("..") {
                    return McpResponse::invalid_params(id, "Field 'graph' contains invalid characters");
                }
                let path = format!("assets/shader_graphs/{}.graph.json", graph);
                let outcome = match std::fs::read_to_string(&path) {
                    Err(e) => json!({
                        "graph": graph,
                        "valid": false,
                        "error": format!("no se pudo leer el archivo: {}", e)
                    }),
                    Ok(text) => match serde_json::from_str::<glass_core::shader_graph::ShaderGraph>(&text) {
                        Err(e) => json!({
                            "graph": graph,
                            "valid": false,
                            "error": format!("JSON inválido: {}", e)
                        }),
                        Ok(shader_graph) => {
                            let fragment = shader_graph.compile_fragment();
                            let vertex = shader_graph.compile_vertex();
                            let compile_error = match (&fragment, &vertex) {
                                (Err(e), _) => Some(format!("{}", e)),
                                (_, Err(e)) => Some(format!("{}", e)),
                                (Ok(_), Ok(_)) => None,
                            };
                            match compile_error {
                                Some(e) => json!({
                                    "graph": graph,
                                    "valid": false,
                                    "error": e
                                }),
                                None => json!({
                                    "graph": graph,
                                    "valid": true,
                                    "nodes": shader_graph.nodes.len(),
                                    "properties": shader_graph.properties.len()
                                }),
                            }
                        }
                    },
                };
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&outcome).unwrap()
                        }]
                    }),
                )
            }
            "glass_shader_node_add" => {
                let graph = match args.get("graph").and_then(|v| v.as_str()) {
                    Some(g) => g,
                    None => return McpResponse::invalid_params(id, "Field 'graph' is required"),
                };
                if graph.contains('/') || graph.contains('\\') || graph.contains("..") {
                    return McpResponse::invalid_params(id, "Field 'graph' contains invalid characters");
                }
                let kind_value = match args.get("kind") {
                    Some(v) => v.clone(),
                    None => return McpResponse::invalid_params(id, "Field 'kind' is required"),
                };
                let position = match args.get("position").and_then(|v| v.as_array()) {
                    Some(a) if a.len() == 2 => match (a[0].as_f64(), a[1].as_f64()) {
                        (Some(x), Some(y)) => [x as f32, y as f32],
                        _ => [0.0, 0.0],
                    },
                    _ => [0.0, 0.0],
                };
                let kind = match serde_json::from_value::<glass_core::shader_graph::NodeKind>(kind_value) {
                    Ok(k) => k,
                    Err(e) => return McpResponse::invalid_params(id, &format!("Invalid 'kind': {}", e)),
                };
                let path = format!("assets/shader_graphs/{}.graph.json", graph);
                let text = match std::fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) => {
                        return McpResponse::error(
                            id,
                            -32000,
                            format!("no se pudo leer el archivo {}: {}", path, e),
                        )
                    }
                };
                let mut shader_graph = match serde_json::from_str::<glass_core::shader_graph::ShaderGraph>(&text) {
                    Ok(g) => g,
                    Err(e) => {
                        return McpResponse::error(
                            id,
                            -32000,
                            format!("JSON inválido en {}: {}", path, e),
                        )
                    }
                };
                let new_id = shader_graph.nodes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
                shader_graph.nodes.push(glass_core::shader_graph::GraphNode {
                    id: new_id,
                    kind,
                    position,
                });
                let serialized = match serde_json::to_string_pretty(&shader_graph) {
                    Ok(s) => s,
                    Err(e) => {
                        return McpResponse::error(
                            id,
                            -32000,
                            format!("no se pudo serializar el grafo: {}", e),
                        )
                    }
                };
                if let Err(e) = std::fs::write(&path, serialized) {
                    return McpResponse::error(
                        id,
                        -32000,
                        format!("no se pudo escribir el archivo {}: {}", path, e),
                    );
                }
                let total = shader_graph.nodes.len();
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&json!({
                                "ok": true,
                                "graph": graph,
                                "node_id": new_id,
                                "nodes": total
                            })).unwrap()
                        }]
                    }),
                )
            }
            "glass_shader_compile_check" => {
                let graph = match args.get("graph").and_then(|v| v.as_str()) {
                    Some(g) if !g.is_empty() => g.to_string(),
                    _ => return McpResponse::invalid_params(id, "Field 'graph' is required"),
                };
                if graph.contains('/') || graph.contains('\\') || graph.contains("..") {
                    return McpResponse::invalid_params(id, "Field 'graph' contains invalid characters");
                }
                let path = format!("assets/shader_graphs/{}.graph.json", graph);
                let mut compiles = false;
                let mut error: Option<String> = None;
                match std::fs::read_to_string(&path) {
                    Ok(src) => match serde_json::from_str::<glass_core::shader_graph::ShaderGraph>(&src) {
                        Ok(sg) => {
                            if let Err(e) = sg.compile_fragment() {
                                error = Some(format!("{}", e));
                            } else if let Err(e) = sg.compile_vertex() {
                                error = Some(format!("{}", e));
                            } else {
                                compiles = true;
                            }
                        }
                        Err(e) => error = Some(format!("no se pudo parsear el archivo: {}", e)),
                    },
                    Err(e) => error = Some(format!("no se pudo leer el archivo: {}", e)),
                }
                let spv_present = std::path::Path::new(&format!("assets/shaders/graph_{}.frag.spv", graph)).exists();
                let ready = compiles && spv_present;
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&json!({
                                "graph": graph,
                                "compiles": compiles,
                                "error": error,
                                "spv_present": spv_present,
                                "ready": ready
                            })).unwrap()
                        }]
                    }),
                )
            }
            "glass_shader_pass_configure" => {
                let graph = match args.get("graph").and_then(|v| v.as_str()) {
                    Some(g) => g.to_string(),
                    None => return McpResponse::invalid_params(id, "Field 'graph' is required"),
                };
                if graph.contains('/') || graph.contains('\\') || graph.contains("..") {
                    return McpResponse::invalid_params(id, "El nombre del grafo no puede contener '/', '\\' ni '..'");
                }
                let nombres = match args.get("passes").and_then(|v| v.as_array()) {
                    Some(a) if !a.is_empty() => a.clone(),
                    _ => return McpResponse::invalid_params(id, "Field 'passes' is required (array no vacío)"),
                };
                let mut pases = Vec::new();
                for n in &nombres {
                    let Some(txt) = n.as_str() else {
                        return McpResponse::invalid_params(id, "Cada pase debe ser un string");
                    };
                    match serde_json::from_value::<glass_core::shader_graph::PassKind>(json!(txt)) {
                        Ok(p) => pases.push(p),
                        Err(_) => {
                            return McpResponse::invalid_params(
                                id,
                                "Pase no válido: usa Forward, Shadow u Outline",
                            )
                        }
                    }
                }
                let ruta = format!("assets/shader_graphs/{}.graph.json", graph);
                let mut grafo: glass_core::shader_graph::ShaderGraph =
                    match std::fs::read_to_string(&ruta)
                        .ok()
                        .and_then(|t| glass_core::shader_graph::ShaderGraph::from_json(&t).ok())
                    {
                        Some(g) => g,
                        None => {
                            return McpResponse::error(
                                id,
                                -32000,
                                format!("No se pudo leer o parsear {}", ruta),
                            )
                        }
                    };
                grafo.passes = pases.clone();
                match serde_json::to_string_pretty(&grafo)
                    .ok()
                    .and_then(|t| std::fs::write(&ruta, t).ok())
                {
                    Some(()) => McpResponse::success(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string_pretty(&json!({
                                    "ok": true,
                                    "graph": graph,
                                    "passes": pases.iter().map(|p| format!("{:?}", p)).collect::<Vec<_>>()
                                })).unwrap()
                            }]
                        }),
                    ),
                    None => McpResponse::error(id, -32000, format!("No se pudo escribir {}", ruta)),
                }
            }
            "glass_render_pass_stats" => {
                let stats = glass_core::render_stats::render_stats_snapshot();
                let mut datos = json!({
                    "forward_draw_calls": stats.game_draw_calls + stats.scene_draw_calls,
                    "game_draw_calls": stats.game_draw_calls,
                    "scene_draw_calls": stats.scene_draw_calls,
                    "shadow_draw_calls": stats.shadow_draw_calls,
                    "graph_shadow_draw_calls": stats.graph_shadow_draw_calls,
                    "outline_draw_calls": stats.outline_draw_calls
                });
                if stats.game_draw_calls == 0
                    && stats.scene_draw_calls == 0
                    && stats.shadow_draw_calls == 0
                    && stats.graph_shadow_draw_calls == 0
                    && stats.outline_draw_calls == 0
                {
                    datos["note"] =
                        json!("sin datos: ningún frame recolectado en este proceso");
                }
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&datos).unwrap()
                        }]
                    }),
                )
            }
            "glass_shader_preset_apply" => {
                let graph = match args.get("graph").and_then(|v| v.as_str()) {
                    Some(g) => g.to_string(),
                    None => return McpResponse::invalid_params(id, "Field 'graph' is required"),
                };
                let preset = match args.get("preset").and_then(|v| v.as_str()) {
                    Some(p) => p.to_string(),
                    None => return McpResponse::invalid_params(id, "Field 'preset' is required"),
                };
                if graph.contains('/') || graph.contains('\\') || graph.contains("..")
                    || preset.contains('/') || preset.contains('\\') || preset.contains("..")
                {
                    return McpResponse::invalid_params(id, "graph/preset no pueden contener '/', '\\' ni '..'");
                }
                let entity_id = match args.get("entity_id").and_then(|n| n.as_u64()) {
                    Some(v) => v,
                    None => return McpResponse::invalid_params(id, "Field 'entity_id' is required"),
                };
                let ruta = format!("assets/shader_graphs/{}.{}.preset.json", graph, preset);
                let doc: serde_json::Value = match std::fs::read_to_string(&ruta)
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                {
                    Some(d) => d,
                    None => {
                        return McpResponse::error(
                            id,
                            -32000,
                            format!("No se pudo leer o parsear {}", ruta),
                        )
                    }
                };
                let Some(props) = doc.get("properties").and_then(|p| p.as_object()) else {
                    return McpResponse::invalid_params(id, "El preset no trae un objeto 'properties'");
                };
                let Some(ent) = scene.entities.iter_mut().find(|e| e.id == entity_id) else {
                    return McpResponse::error(id, -32004, "Entity not found");
                };
                let Some(mesh) = ent.mesh.as_mut() else {
                    return McpResponse::invalid_params(id, "La entidad no tiene MeshData");
                };
                if mesh.shader != format!("graph:{}", graph) {
                    return McpResponse::error(
                        id,
                        -32004,
                        format!("La entidad no usa el grafo '{}' (usa '{}')", graph, mesh.shader),
                    );
                }
                let mut aplicadas: Vec<String> = Vec::new();
                for (k, v) in props {
                    mesh.shader_properties.insert(k.clone(), v.clone());
                    aplicadas.push(k.clone());
                }
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&json!({
                                "ok": true,
                                "graph": graph,
                                "preset": preset,
                                "entity_id": entity_id,
                                "applied": aplicadas
                            })).unwrap()
                        }]
                    }),
                )
            }
            "glass_render_draw_stats" => {
                let stats = glass_core::render_stats::render_stats_snapshot();
                let mut datos = json!({
                    "pipeline_binds": stats.pipeline_binds,
                    "descriptor_binds": stats.descriptor_binds,
                    "forward_draw_calls": stats.game_draw_calls + stats.scene_draw_calls
                });
                if stats.pipeline_binds == 0 && stats.descriptor_binds == 0 {
                    datos["note"] = json!("sin datos: ningún frame renderizado en este proceso");
                }
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&datos).unwrap()
                        }]
                    }),
                )
            }
            "glass_character_state" => {
                let entity_id = match args.get("entity_id").and_then(|n| n.as_u64()) {
                    Some(v) => v,
                    None => return McpResponse::invalid_params(id, "Field 'entity_id' is required"),
                };
                let info = glass_physics::get_global_physics_world().and_then(|sys| {
                    sys.lock().ok().and_then(|w| w.character_info(entity_id))
                });
                let datos = match info {
                    Some((pos, grounded, sliding)) => json!({
                        "entity_id": entity_id, "exists": true,
                        "position": [pos[0], pos[1], pos[2]],
                        "grounded": grounded, "sliding": sliding
                    }),
                    None => json!({"entity_id": entity_id, "exists": false}),
                };
                let texto = serde_json::to_string_pretty(&datos).unwrap_or_default();
                McpResponse::success(id, json!({"content": [{"type": "text", "text": texto}]}))
            }

            "glass_physics_cast" => {
                let v3 = |v: &serde_json::Value| -> Option<[f32; 3]> {
                    let a = v.as_array()?;
                    if a.len() != 3 {
                        return None;
                    }
                    Some([
                        a[0].as_f64()? as f32,
                        a[1].as_f64()? as f32,
                        a[2].as_f64()? as f32,
                    ])
                };
                let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                let Some(origin) = v3(&args["origin"]) else {
                    return McpResponse::error(id, -32602, "glass_physics_cast: 'origin' debe ser [x, y, z]".to_string());
                };
                let max_dist = args.get("max_dist").and_then(|v| v.as_f64()).unwrap_or(100.0) as f32;
                let mask = args.get("mask").and_then(|v| v.as_u64()).unwrap_or(u32::MAX as u64) as u32;
                let exclude = args.get("exclude_entity").and_then(|v| v.as_u64()).unwrap_or(0);
                let physics = self.get_physics();
                let mut physics = match physics.lock() {
                    Ok(g) => g,
                    Err(e) => return McpResponse::error(id, -32000, format!("Mutex poison error: {}", e)),
                };
                let hit_json = |entity_id: u64, point: [f32; 3], normal: [f32; 3], distance: f32| {
                    json!({"entity_id": entity_id, "point": point, "normal": normal, "distance": distance})
                };
                let payload = match kind {
                    "ray" => {
                        let Some(dir) = v3(&args["direction"]) else {
                            return McpResponse::error(id, -32602, "glass_physics_cast: 'direction' requerida para kind=ray".to_string());
                        };
                        let hit = physics.cast_ray_masked(origin, dir, max_dist, mask, exclude);
                        json!({"hit": hit.map(|h| hit_json(h.entity_id, h.point, h.normal, h.distance))})
                    }
                    "sphere" => {
                        let Some(dir) = v3(&args["direction"]) else {
                            return McpResponse::error(id, -32602, "glass_physics_cast: 'direction' requerida para kind=sphere".to_string());
                        };
                        let radius = args.get("radius").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                        let hit = physics.cast_sphere(origin, radius, dir, max_dist, mask, exclude);
                        json!({"hit": hit.map(|h| hit_json(h.entity_id, h.point, h.normal, h.distance))})
                    }
                    "capsule" => {
                        let Some(dir) = v3(&args["direction"]) else {
                            return McpResponse::error(id, -32602, "glass_physics_cast: 'direction' requerida para kind=capsule".to_string());
                        };
                        let half_height = args.get("half_height").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                        let radius = args.get("radius").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                        let hit = physics.cast_capsule(
                            glass_physics::BarridoCapsula { origin, half_height, radius, dir, max_dist },
                            mask,
                            exclude,
                        );
                        json!({"hit": hit.map(|h| hit_json(h.entity_id, h.point, h.normal, h.distance))})
                    }
                    "overlap_sphere" => {
                        let radius = args.get("radius").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                        let entities = physics.overlap_sphere(origin, radius, mask, exclude);
                        json!({"entities": entities, "count": entities.len()})
                    }
                    "overlap_box" => {
                        let Some(half) = v3(&args["half_extents"]) else {
                            return McpResponse::error(id, -32602, "glass_physics_cast: 'half_extents' requerido para kind=overlap_box".to_string());
                        };
                        let entities = physics.overlap_box(origin, half, mask, exclude);
                        json!({"entities": entities, "count": entities.len()})
                    }
                    other => {
                        return McpResponse::error(id, -32602, format!("glass_physics_cast: kind no válido: {other}"));
                    }
                };
                let texto = serde_json::to_string_pretty(&payload).unwrap_or_default();
                McpResponse::success(id, json!({"content": [{"type": "text", "text": texto}]}))
            }
            "glass_list_contact_events" => {
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(32) as usize;
                let physics = self.get_physics();
                let physics = match physics.lock() {
                    Ok(g) => g,
                    Err(e) => return McpResponse::error(id, -32000, format!("Mutex poison error: {}", e)),
                };
                let active_pairs: Vec<_> = physics
                    .contact_pairs()
                    .into_iter()
                    .map(|(a, b, is_trigger)| json!({"a": a, "b": b, "is_trigger": is_trigger}))
                    .collect();
                let payload = json!({
                    "active_pairs": active_pairs,
                    "recent_events": physics.recent_contact_events(limit),
                    "step_count": physics.step_count(),
                });
                let texto = serde_json::to_string_pretty(&payload).unwrap_or_default();
                McpResponse::success(id, json!({"content": [{"type": "text", "text": texto}]}))
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

                // Ningún valor no finito llega al transform ni al material:
                // un f64 gigante se convierte en `inf` en el cast a f32, y
                // NaN/inf acabarían en la escena y en Rapier.
                for (field, vals) in [
                    ("position", pos),
                    ("rotation", rot),
                    ("scale", scale),
                    ("color", color),
                ] {
                    if let Some(v) = vals {
                        if !v.iter().all(|c| c.is_finite()) {
                            return McpResponse::invalid_params(
                                id,
                                &format!("Field '{}' must contain only finite numbers", field),
                            );
                        }
                    }
                }

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
                // `is_real` distingue una captura publicada por el motor de un
                // relleno generado aquí. La respuesta lo declara explícitamente
                // para que un cliente MCP no confunda un degradado con el frame.
                let (png_bytes, is_real) = match &scene.viewport_snapshot_png {
                    Some(bytes) => (bytes.clone(), true),
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
                        (bytes, false)
                    }
                };

                let base64_data = STANDARD.encode(&png_bytes);
                log::info!("MCP glass_capture_viewport: Snapshot codificado ({} bytes PNG, {} caracteres base64).", png_bytes.len(), base64_data.len());

                let note = if is_real {
                    format!(
                        "Viewport 3D capturado: 1280x720 (PNG {} bytes). Snapshot real publicado por el motor.",
                        png_bytes.len()
                    )
                } else {
                    format!(
                        "ADVERTENCIA - imagen sintética: no había snapshot en scene.viewport_snapshot_png, \
                         así que se generó un degradado de 1280x720 (PNG {} bytes). No representa lo que \
                         renderiza el motor; sólo sirve para validar el canal de transporte MCP.",
                        png_bytes.len()
                    )
                };

                McpResponse::success(
                    id,
                    json!({
                        "synthetic": !is_real,
                        "content": [
                            {
                                "type": "image",
                                "data": base64_data,
                                "mimeType": "image/png"
                            },
                            {
                                "type": "text",
                                "text": note
                            }
                        ]
                    }),
                )
            }

            "glass_reload_shaders" => {
                scene.shaders_reloaded_flag = true;
                log::warn!(
                    "MCP glass_reload_shaders: flag activado, pero los shaders están embebidos \
                     con include_bytes! y el pipeline no se reconstruye en caliente."
                );
                McpResponse::success(
                    id,
                    json!({
                        "applied": false,
                        "flag_set": true,
                        "content": [
                            {
                                "type": "text",
                                "text": "Flag shaders_reloaded_flag activado (flag_set: true, applied: false). \
                                         Los shaders SPIR-V NO quedan recargados exitosamente con esta \
                                         llamada: los binarios .spv se incorporan al ejecutable con \
                                         include_bytes! en tiempo de compilación, y todavía no existe \
                                         una ruta que reconstruya el pipeline en caliente. Para aplicar \
                                         recompilar glass_renderer y reiniciar el motor."
                            }
                        ]
                    }),
                )
            }

            "glass_setup_platformer_scene" => {
                let initial_y = args.get("player_initial_y").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                if !initial_y.is_finite() {
                    return McpResponse::invalid_params(id, "Field 'player_initial_y' must be a finite number");
                }

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

                if let Some(p) = scene.entities.iter_mut().find(|e| e.id == player_id) {
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
                        bus: "sfx".to_string(),
                    });
                }

                // 2. Configurar simulación física en Rapier3D
                let phys_arc = self.get_physics();
                let player_handle;
                {
                    let mut phys = phys_arc.lock().unwrap();
                    // Suelo estático
                    let fb = phys.create_rigid_body(floor_id, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
                    phys.attach_box_collider(fb, [15.0, 0.5, 15.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");

                    // Escalón 1 (Low platform: y_surface = 1.0)
                    let s1b = phys.create_rigid_body(step1_id, BodyType::Static, [3.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
                    phys.attach_box_collider(s1b, [1.0, 0.5, 1.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");

                    // Escalón 2 (High platform: y_surface = 2.0)
                    let s2b = phys.create_rigid_body(step2_id, BodyType::Static, [6.0, 1.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
                    phys.attach_box_collider(s2b, [1.0, 0.5, 1.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");

                    // Cubo jugador dinámico
                    let pb = phys.create_rigid_body(player_id, BodyType::Dynamic, [0.0, initial_y, 0.0], [0.0, 0.0, 0.0, 1.0]);
                    phys.attach_box_collider(pb, [0.5, 0.5, 0.5], &glass_core::PhysicsMaterial { dynamic_friction: 0.6, bounciness: 0.02, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
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

                let looping = args.get("looping").and_then(|l| l.as_bool()).unwrap_or(false);
                let bus = args
                    .get("bus")
                    .and_then(|b| b.as_str())
                    .and_then(glass_audio::BusId::from_name)
                    .unwrap_or(glass_audio::BusId::Sfx);

                let audio_sys = self.get_audio();
                let handle = if let Some(p) = pos {
                    audio_sys.play_3d_on_bus(clip, p, volume, pitch, looping, bus)
                } else {
                    audio_sys.play_2d_on_bus(clip, volume, pitch, looping, bus)
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

            "glass_audio_buses" => {
                let audio_sys = self.get_audio();
                if let Some(action) = args.get("action").and_then(|a| a.as_str()) {
                    let bus_nombre = args.get("bus").and_then(|b| b.as_str()).unwrap_or("");
                    let bus = match glass_audio::BusId::from_name(bus_nombre) {
                        Some(b) => b,
                        None => return McpResponse::invalid_params(id, "Field 'bus' must be 'master', 'music' or 'sfx'"),
                    };
                    match action {
                        "set_volume" => {
                            let volume = match args.get("volume").and_then(|v| v.as_f64()) {
                                Some(v) => v as f32,
                                None => return McpResponse::invalid_params(id, "Field 'volume' is required for set_volume"),
                            };
                            audio_sys.set_bus_volume(bus, volume);
                        }
                        "duck" => {
                            let amount = args.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.2) as f32;
                            let duration = args.get("duration_secs").and_then(|v| v.as_f64()).unwrap_or(3.0) as f32;
                            audio_sys.duck_bus(bus, amount, duration);
                        }
                        _otro => return McpResponse::invalid_params(id, "Unknown action"),
                    }
                }
                let buses: Vec<_> = audio_sys
                    .bus_snapshot()
                    .iter()
                    .map(|b| {
                        json!({
                            "name": b.name,
                            "volume": b.volume,
                            "effective_volume": b.effective_volume,
                            "ducking": b.ducking,
                            "duck_factor": b.duck_factor
                        })
                    })
                    .collect();
                let sources: Vec<_> = audio_sys
                    .sources_snapshot()
                    .iter()
                    .map(|(h, info)| {
                        json!({
                            "handle": h,
                            "bus": info.bus.name(),
                            "position": info.position,
                            "occluded": info.occluded
                        })
                    })
                    .collect();
                let datos = json!({
                    "backend": if audio_sys.is_null_backend { "null" } else { "kira" },
                    "buses": buses,
                    "sources": sources
                });
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&datos).unwrap()
                        }]
                    }),
                )
            }

            "glass_animator_state" => {
                let entity_id = match args.get("entity_id").and_then(|n| n.as_u64()) {
                    Some(v) => v,
                    None => return McpResponse::invalid_params(id, "Field 'entity_id' is required"),
                };
                let ent = match scene.entities.iter().find(|e| e.id == entity_id) {
                    Some(e) => e,
                    None => return McpResponse::error(id, -32004, "Entity not found"),
                };
                let anim = match &ent.animator {
                    Some(a) => a,
                    None => {
                        return McpResponse::error(id, -32004, "La entidad no tiene Animator")
                    }
                };
                let mut blend_json = serde_json::Value::Null;
                let mut root_motion = false;
                if !anim.controller.is_empty() {
                    let path = format!("assets/animators/{}.animator.json", anim.controller);
                    if let Ok(ctrl) =
                        glass_core::animator::AnimatorController::load_from_file(&path)
                    {
                        if let Some(st) = ctrl.state(&anim.current_state) {
                            root_motion = st.root_motion;
                            if let Some(tree) = &st.blend {
                                let v = anim.params.get(&tree.param).copied().unwrap_or(0.0);
                                let hijos: Vec<serde_json::Value> =
                                    glass_core::animator::blend_weights(tree, v)
                                        .into_iter()
                                        .map(|(c, w)| {
                                            json!({
                                                "clip": c.clip,
                                                "threshold": c.threshold,
                                                "weight": w,
                                                "time": anim
                                                    .blend_times
                                                    .get(&c.clip)
                                                    .copied()
                                                    .unwrap_or(0.0),
                                            })
                                        })
                                        .collect();
                                blend_json = json!({
                                    "param": tree.param,
                                    "value": v,
                                    "children": hijos,
                                });
                            } else if let Some(tree) = &st.blend2d {
                                let x = anim.params.get(&tree.param_x).copied().unwrap_or(0.0);
                                let y = anim.params.get(&tree.param_y).copied().unwrap_or(0.0);
                                let hijos: Vec<serde_json::Value> =
                                    glass_core::animator::blend_weights_2d(tree, x, y)
                                        .into_iter()
                                        .map(|(c, w)| {
                                            json!({
                                                "clip": c.clip,
                                                "position": c.position,
                                                "weight": w,
                                                "time": anim
                                                    .blend_times
                                                    .get(&c.clip)
                                                    .copied()
                                                    .unwrap_or(0.0),
                                            })
                                        })
                                        .collect();
                                blend_json = json!({
                                    "param_x": tree.param_x,
                                    "param_y": tree.param_y,
                                    "value_x": x,
                                    "value_y": y,
                                    "children": hijos,
                                });
                            }
                        }
                    }
                }
                let fade_weight = if anim.fade_from_state.is_some() && anim.fade_duration > 0.0 {
                    json!((anim.fade_elapsed / anim.fade_duration).clamp(0.0, 1.0))
                } else {
                    serde_json::Value::Null
                };
                let datos = json!({
                    "entity_id": entity_id,
                    "controller": anim.controller,
                    "state": anim.current_state,
                    "state_time": anim.state_time,
                    "position": ent.transform.position,
                    "root_motion": root_motion,
                    "fading_from": anim.fade_from_state,
                    "fade_weight": fade_weight,
                    "params": anim.params,
                    "blend": blend_json,
                    "recent_events": glass_core::animator_stats::recent_anim_events(entity_id, 10),
                    "recent_state_events": glass_core::animator_stats::recent_state_events(entity_id, 10),
                });
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&datos).unwrap()
                        }]
                    }),
                )
            }

            "glass_animator_set_param" => {
                let entity_id = match args.get("entity_id").and_then(|n| n.as_u64()) {
                    Some(v) => v,
                    None => return McpResponse::invalid_params(id, "Field 'entity_id' is required"),
                };
                let param = match args.get("param").and_then(|p| p.as_str()) {
                    Some(s) => s.to_string(),
                    None => return McpResponse::invalid_params(id, "Field 'param' is required"),
                };
                let value = match args.get("value").and_then(|v| v.as_f64()) {
                    Some(n) => n as f32,
                    None => return McpResponse::invalid_params(id, "Field 'value' is required"),
                };
                let ent = match scene.entities.iter_mut().find(|e| e.id == entity_id) {
                    Some(e) => e,
                    None => return McpResponse::error(id, -32004, "Entity not found"),
                };
                let anim = match &mut ent.animator {
                    Some(a) => a,
                    None => {
                        return McpResponse::error(id, -32004, "La entidad no tiene Animator")
                    }
                };
                anim.params.insert(param.clone(), value);
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "Parámetro '{}' del Animator de la entidad {} fijado a {}.",
                                param, entity_id, value
                            )
                        }]
                    }),
                )
            }

            "glass_ui_tree" => {
                let width = args.get("width").and_then(|v| v.as_f64()).unwrap_or(1280.0) as f32;
                let height = args.get("height").and_then(|v| v.as_f64()).unwrap_or(720.0) as f32;
                let rects = glass_core::resolve_scene_ui_rects(&scene, width, height);
                let scale = scene
                    .entities
                    .iter()
                    .find_map(|e| e.canvas.as_ref())
                    .map(|c| glass_core::canvas_scale_factor(c, width, height))
                    .unwrap_or(1.0);
                let redondea = |r: [f32; 4]| -> [f32; 4] {
                    [
                        (r[0] * 10.0).round() / 10.0,
                        (r[1] * 10.0).round() / 10.0,
                        (r[2] * 10.0).round() / 10.0,
                        (r[3] * 10.0).round() / 10.0,
                    ]
                };
                let elementos: Vec<serde_json::Value> = scene
                    .entities
                    .iter()
                    .filter(|e| e.rect_transform.is_some() || e.canvas.is_some())
                    .map(|e| {
                        let mut kinds: Vec<&str> = Vec::new();
                        if e.canvas.is_some() {
                            kinds.push("Canvas");
                        }
                        if e.ui_text.is_some() {
                            kinds.push("Text");
                        }
                        if e.ui_button.is_some() {
                            kinds.push("Button");
                        }
                        if e.ui_image.is_some() {
                            kinds.push("Image");
                        }
                        if e.layout_group.is_some() {
                            kinds.push("LayoutGroup");
                        }
                        let rect = rects
                            .get(&e.id)
                            .map(|r| json!(redondea(*r)))
                            .unwrap_or_else(|| json!([0.0, 0.0, width, height]));
                        json!({
                            "entity_id": e.id,
                            "name": e.name,
                            "parent": e.parent,
                            "active": e.active,
                            "rect": rect,
                            "kinds": kinds,
                        })
                    })
                    .collect();
                let datos = json!({
                    "width": width,
                    "height": height,
                    "scale_factor": scale,
                    "elements": elementos,
                });
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&datos).unwrap()
                        }]
                    }),
                )
            }

            "glass_ui_stats" => {
                let width = args.get("width").and_then(|v| v.as_f64()).unwrap_or(1280.0) as f32;
                let height = args.get("height").and_then(|v| v.as_f64()).unwrap_or(720.0) as f32;
                let scale = scene
                    .entities
                    .iter()
                    .find_map(|e| e.canvas.as_ref())
                    .map(|c| glass_core::canvas_scale_factor(c, width, height))
                    .unwrap_or(1.0);
                let cuenta = |f: &dyn Fn(&glass_core::EntityData) -> bool| {
                    scene.entities.iter().filter(|e| f(e)).count()
                };
                let by_id: std::collections::HashMap<u64, &glass_core::EntityData> =
                    scene.entities.iter().map(|e| (e.id, e)).collect();
                let mut max_depth = 0u32;
                for e in scene
                    .entities
                    .iter()
                    .filter(|e| e.rect_transform.is_some() || e.canvas.is_some())
                {
                    let mut d = 0u32;
                    let mut cur = e.parent;
                    while let Some(pid) = cur {
                        d += 1;
                        match by_id.get(&pid) {
                            Some(p) => cur = p.parent,
                            None => break,
                        }
                        if d > 64 {
                            break;
                        }
                    }
                    max_depth = max_depth.max(d);
                }
                let datos = json!({
                    "canvases": cuenta(&|e: &glass_core::EntityData| e.canvas.is_some()),
                    "elements": cuenta(&|e: &glass_core::EntityData| {
                        e.rect_transform.is_some() || e.canvas.is_some()
                    }),
                    "texts": cuenta(&|e: &glass_core::EntityData| e.ui_text.is_some()),
                    "buttons": cuenta(&|e: &glass_core::EntityData| e.ui_button.is_some()),
                    "images": cuenta(&|e: &glass_core::EntityData| e.ui_image.is_some()),
                    "layout_groups": cuenta(&|e: &glass_core::EntityData| {
                        e.layout_group.is_some()
                    }),
                    "max_depth": max_depth,
                    "scale_factor": scale,
                    "width": width,
                    "height": height,
                });
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&datos).unwrap()
                        }]
                    }),
                )
            }

            "glass_transform_stats" => {
                let s = glass_core::transform_stats::snapshot();
                let datos = json!({
                    "entities": s.entities,
                    "recomputed": s.recomputed,
                    "max_depth": s.max_depth,
                    "last_micros": s.last_micros,
                    "total_passes": s.total_passes,
                });
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&datos).unwrap()
                        }]
                    }),
                )
            }

            "glass_asset_validate" => {
                let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
                    return McpResponse::invalid_params(id, "Field 'path' is required");
                };
                match validar_asset(path) {
                    Ok(datos) => McpResponse::success(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string_pretty(&datos).unwrap()
                            }]
                        }),
                    ),
                    Err(e) => McpResponse::error(id, -32005, e),
                }
            }

            "glass_asset_reload" => {
                let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
                    return McpResponse::invalid_params(id, "Field 'path' is required");
                };
                let ruta = std::path::Path::new(path);
                let ext = ruta
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !path.starts_with("assets/") || path.contains("..") {
                    return McpResponse::error(
                        id,
                        -32005,
                        "La ruta debe estar bajo assets/ y sin '..'",
                    );
                }
                if ext == "glb" {
                    return match validar_asset(path) {
                        Ok(datos) => McpResponse::success(
                            id,
                            json!({
                                "content": [{
                                    "type": "text",
                                    "text": serde_json::to_string_pretty(&datos).unwrap()
                                }]
                            }),
                        ),
                        Err(e) => McpResponse::error(id, -32005, e),
                    };
                }
                if ext != "blend" && ext != "fbx" {
                    return McpResponse::error(
                        id,
                        -32005,
                        "Solo se re-exportan .blend/.fbx (o se valida un .glb)",
                    );
                }
                let glb = ruta.with_extension("glb");
                // Rutas ABSOLUTAS contra la raíz del proyecto: algún
                // eslabón de la cadena (app → la cadena de conversión)
                // resolvía las relativas contra el $PWD del lanzador
                // y la recarga fallaba al abrir la app desde un
                // acceso directo o un IDE (defecto de la Fase 10).
                let raiz = raiz_proyecto();
                let salida = glass_renderer::watcher::convertir_fuente(
                    &raiz.join(path),
                    &raiz.join(&glb),
                );
                match salida {
                    Ok(informe) => {
                        let via = match informe.via {
                            glass_renderer::watcher::ViaConversion::Nativa => "nativa",
                            glass_renderer::watcher::ViaConversion::Blender => "blender",
                        };
                        let resultado = json!({
                            "ok": true,
                            "via": via,
                            "counts": informe.conteos,
                            "avisos": informe.avisos,
                            "note": "La escritura del .glb dispara la recarga en caliente por el vigía.",
                        });
                        McpResponse::success(
                            id,
                            json!({
                                "content": [{
                                    "type": "text",
                                    "text": serde_json::to_string_pretty(&resultado).unwrap()
                                }]
                            }),
                        )
                    }
                    Err(e) => McpResponse::error(id, -32005, e),
                }
            }

            "glass_import_unitypackage" => {
                let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
                    return McpResponse::invalid_params(id, "Field 'path' is required");
                };
                let sobrescribir = args
                    .get("overwrite")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let paquete = {
                    let p = std::path::PathBuf::from(path);
                    if p.is_absolute() {
                        p
                    } else {
                        raiz_proyecto().join(p)
                    }
                };
                if !paquete.is_file() {
                    return McpResponse::error(
                        id,
                        -32005,
                        format!("No existe el paquete: {}", paquete.display()),
                    );
                }
                let raiz = raiz_proyecto();
                let assets = ["assets", "Assets"]
                    .iter()
                    .map(|n| raiz.join(n))
                    .find(|p| p.is_dir())
                    .unwrap_or_else(|| raiz.join("assets"));
                let verificador = glass_ripper::entorno::crear_verificador();
                match glass_ripper::importador::importar_paquete(
                    &paquete,
                    &assets,
                    sobrescribir,
                    verificador.as_deref(),
                ) {
                    Ok(informe) => {
                        let resumen = glass_ripper::entorno::resumen_importacion(&informe);
                        McpResponse::success(
                            id,
                            json!({
                                "content": [{
                                    "type": "text",
                                    "text": format!(
                                        "Importación de {}: {}\n\n{}",
                                        paquete.display(),
                                        resumen,
                                        informe.a_texto()
                                    )
                                }]
                            }),
                        )
                    }
                    Err(e) => McpResponse::error(id, -32005, e.to_string()),
                }
            }
            "glass_destroy_entity" => {
                let Some(entity_id) = args.get("entity_id").and_then(|n| n.as_u64()) else {
                    return McpResponse::invalid_params(id, "Field 'entity_id' is required");
                };
                if scene.delete_entity(entity_id) {
                    McpResponse::success(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Entidad {entity_id} destruida")
                            }]
                        }),
                    )
                } else {
                    McpResponse::error(id, -32004, "Entity not found")
                }
            }

            "glass_prefab_list" => {
                let raiz = raiz_proyecto();
                let mut pila: Vec<std::path::PathBuf> = Vec::new();
                // Se usa read_dir sobre la raíz para que, en sistemas de archivos
                // insensibles a mayúsculas, Assets/ y assets/ no se recorran dos veces.
                if let Ok(entradas) = std::fs::read_dir(&raiz) {
                    for entrada in entradas.flatten() {
                        let nombre = entrada.file_name();
                        let es_assets = nombre == "Assets" || nombre == "assets";
                        let es_dir = entrada.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        if es_assets && es_dir {
                            pila.push(entrada.path());
                        }
                    }
                }

                let mut encontrados: Vec<(String, String, usize, Option<String>)> = Vec::new();
                let mut ilegibles = 0usize;
                while let Some(dir) = pila.pop() {
                    let Ok(entradas) = std::fs::read_dir(&dir) else {
                        continue;
                    };
                    for entrada in entradas.flatten() {
                        let Ok(tipo) = entrada.file_type() else {
                            continue;
                        };
                        let ruta = entrada.path();
                        // file_type() no sigue symlinks: así no hay bucles en el recorrido.
                        if tipo.is_dir() {
                            pila.push(ruta);
                            continue;
                        }
                        if !entrada.file_name().to_string_lossy().ends_with(".prefab.json") {
                            continue;
                        }
                        match glass_core::prefab::Prefab::load_from_file(&ruta) {
                            Ok(prefab) => {
                                let relativa = ruta
                                    .strip_prefix(&raiz)
                                    .unwrap_or(&ruta)
                                    .components()
                                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                                    .collect::<Vec<_>>()
                                    .join("/");
                                let k = 1 + prefab.descendant_entities.len();
                                let variante_de = prefab.variant_of.clone();
                                encontrados.push((relativa, prefab.name, k, variante_de));
                            }
                            Err(_) => ilegibles += 1,
                        }
                    }
                }
                encontrados.sort_by(|a, b| a.0.cmp(&b.0));

                let mut texto = format!("Prefabs en el proyecto ({}):", encontrados.len());
                for (ruta, nombre, k, variante_de) in &encontrados {
                    texto.push_str(&format!("\n- {ruta} · {nombre} · {k} entidades"));
                    if let Some(base) = variante_de {
                        texto.push_str(&format!(" · variante de {base}"));
                    }
                }
                if ilegibles > 0 {
                    texto.push_str(&format!(
                        "\n({ilegibles} archivos .prefab.json ilegibles omitidos)"
                    ));
                }
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": texto
                        }]
                    }),
                )
            }
            "glass_prefab_create" => {
                let Some(entity_id) = args.get("entity_id").and_then(|n| n.as_u64()) else {
                    return McpResponse::invalid_params(id, "Field 'entity_id' is required");
                };
                let Some(path) = args.get("path").and_then(|p| p.as_str()) else {
                    return McpResponse::invalid_params(id, "Field 'path' is required");
                };
                let ruta_valida = (path.starts_with("Assets/") || path.starts_with("assets/"))
                    && path.ends_with(".prefab.json")
                    && !path.contains("..");
                if !ruta_valida {
                    return McpResponse::invalid_params(
                        id,
                        "Field 'path' must start with 'Assets/' or 'assets/', end with '.prefab.json' and not contain '..'",
                    );
                }
                let overwrite = args
                    .get("overwrite")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let destino = raiz_proyecto().join(path);
                if destino.exists() && !overwrite {
                    return McpResponse::error(
                        id,
                        -32009,
                        format!("Ya existe un prefab en '{path}' (usa overwrite para reemplazarlo)"),
                    );
                }

                let prefab = match glass_core::prefab::Prefab::from_entity(&scene, entity_id) {
                    Ok(p) => p,
                    Err(msg) => return McpResponse::error(id, -32004, msg),
                };

                if let Err(e) = prefab.save_to_file(&destino) {
                    return McpResponse::error(id, -32002, e.to_string());
                }

                let k = 1 + prefab.descendant_entities.len();
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "Prefab '{}' creado en {path} ({k} entidades) desde la entidad #{entity_id}",
                                prefab.name
                            )
                        }]
                    }),
                )
            }
            "glass_prefab_create_variant" => {
                let Some(path) = args.get("path").and_then(|p| p.as_str()) else {
                    return McpResponse::invalid_params(id, "Field 'path' is required");
                };
                let Some(variant_path) = args.get("variant_path").and_then(|p| p.as_str()) else {
                    return McpResponse::invalid_params(id, "Field 'variant_path' is required");
                };
                let ruta_valida = |p: &str| {
                    (p.starts_with("Assets/") || p.starts_with("assets/"))
                        && p.ends_with(".prefab.json")
                        && !p.contains("..")
                };
                if !ruta_valida(path) {
                    return McpResponse::invalid_params(
                        id,
                        "Field 'path' must start with 'Assets/' or 'assets/', end with '.prefab.json' and not contain '..'",
                    );
                }
                if !ruta_valida(variant_path) {
                    return McpResponse::invalid_params(
                        id,
                        "Field 'variant_path' must start with 'Assets/' or 'assets/', end with '.prefab.json' and not contain '..'",
                    );
                }
                let overwrite = args
                    .get("overwrite")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let destino = raiz_proyecto().join(variant_path);
                if destino.exists() && !overwrite {
                    return McpResponse::error(
                        id,
                        -32009,
                        format!("Ya existe un prefab en '{variant_path}' (usa overwrite para reemplazarlo)"),
                    );
                }
                let Some(base) =
                    glass_core::prefab::Prefab::cargar_desde_proyecto(&raiz_proyecto(), path)
                else {
                    return McpResponse::error(
                        id,
                        -32004,
                        format!("No se encontró el prefab base '{path}'"),
                    );
                };
                let nombre = args
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        variant_path
                            .rsplit('/')
                            .next()
                            .unwrap_or(variant_path)
                            .trim_end_matches(".prefab.json")
                            .to_string()
                    });
                let variante = glass_core::prefab::Prefab::variante_vacia(&nombre, path, &base);
                if let Err(e) = variante.save_to_file(&destino) {
                    return McpResponse::error(id, -32002, e.to_string());
                }
                let k = 1 + variante.descendant_entities.len();
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "Variante '{nombre}' creada en {variant_path} desde {path} (diff vacío, {k} entidades)"
                            )
                        }]
                    }),
                )
            }
            "glass_prefab_instantiate" => {
                let Some(path) = args.get("path").and_then(|p| p.as_str()) else {
                    return McpResponse::invalid_params(id, "Field 'path' is required");
                };
                let ruta_valida = (path.starts_with("Assets/") || path.starts_with("assets/"))
                    && path.ends_with(".prefab.json")
                    && !path.contains("..");
                if !ruta_valida {
                    return McpResponse::invalid_params(
                        id,
                        "Field 'path' must start with 'Assets/' or 'assets/', end with '.prefab.json' and not contain '..'",
                    );
                }

                let world_pos = match args.get("position") {
                    None | Some(serde_json::Value::Null) => None,
                    Some(v) => {
                        let comps: Option<Vec<f32>> = v.as_array().and_then(|a| {
                            a.iter().map(|c| c.as_f64().map(|n| n as f32)).collect()
                        });
                        match comps {
                            Some(c) if c.len() == 3 => Some([c[0], c[1], c[2]]),
                            _ => {
                                return McpResponse::invalid_params(
                                    id,
                                    "Field 'position' must be an array of 3 numbers",
                                )
                            }
                        }
                    }
                };

                let parent_id = match args.get("parent_id") {
                    None | Some(serde_json::Value::Null) => None,
                    Some(v) => match v.as_u64() {
                        Some(n) => Some(n),
                        None => {
                            return McpResponse::invalid_params(
                                id,
                                "Field 'parent_id' must be a non-negative integer",
                            )
                        }
                    },
                };

                let Some(prefab) =
                    glass_core::prefab::Prefab::cargar_desde_proyecto(&raiz_proyecto(), path)
                else {
                    return McpResponse::error(
                        id,
                        -32010,
                        format!("Prefab no encontrado: '{path}'"),
                    );
                };

                let k = 1 + prefab.descendant_entities.len();
                if scene.entities.len() + k > MAX_ENTITIES_PER_SCENE {
                    return McpResponse::error(id, -32003, "Scene capacity reached (10000 entities)");
                }

                if let Some(p) = parent_id {
                    if scene.get_entity(p).is_none() {
                        return McpResponse::error(id, -32004, "Entity not found");
                    }
                }

                let new_id = scene.instantiate_prefab(&prefab, parent_id, world_pos, Some(path));
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "Prefab '{}' instanciado desde {path}: raíz #{new_id} ({k} entidades)",
                                prefab.name
                            )
                        }]
                    }),
                )
            }
            "glass_prefab_revert" => {
                let Some(entity_id) = args.get("entity_id").and_then(|n| n.as_u64()) else {
                    return McpResponse::invalid_params(id, "Field 'entity_id' is required");
                };

                let fuente = match scene.get_entity(entity_id) {
                    None => return McpResponse::error(id, -32004, "Entity not found"),
                    Some(e) => match e.prefab_source.clone() {
                        Some(f) => f,
                        None => {
                            return McpResponse::error(
                                id,
                                -32013,
                                format!(
                                    "La entidad #{entity_id} no es raíz de una instancia de prefab"
                                ),
                            )
                        }
                    },
                };

                let Some(prefab) =
                    glass_core::prefab::Prefab::cargar_desde_proyecto(&raiz_proyecto(), &fuente)
                else {
                    return McpResponse::error(
                        id,
                        -32010,
                        format!("Vínculo roto: el asset '{fuente}' no se encuentra en el proyecto"),
                    );
                };

                match scene.revertir_instancia_prefab(entity_id, &prefab) {
                    Ok(()) => McpResponse::success(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": format!(
                                    "Instancia #{entity_id} revertida a su prefab '{fuente}'"
                                )
                            }]
                        }),
                    ),
                    Err(msg) => McpResponse::error(id, -32013, msg),
                }
            }

            "glass_prefab_overrides" => {
                let Some(entity_id) = args.get("entity_id").and_then(|n| n.as_u64()) else {
                    return McpResponse::invalid_params(id, "Field 'entity_id' is required");
                };

                let (fuente, overrides) = match scene.get_entity(entity_id) {
                    None => return McpResponse::error(id, -32004, "Entity not found"),
                    Some(e) => match e.prefab_source.clone() {
                        Some(f) => (f, e.prefab_overrides.clone()),
                        None => {
                            return McpResponse::error(
                                id,
                                -32013,
                                format!(
                                    "La entidad #{entity_id} no es raíz de una instancia de prefab"
                                ),
                            )
                        }
                    },
                };

                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&json!({
                                "entity_id": entity_id,
                                "source": fuente,
                                "overrides": overrides
                            })).unwrap_or_default()
                        }]
                    }),
                )
            }

            "glass_queue_submit" => {
                let Some(lista) = args.get("commands").and_then(|v| v.as_array()) else {
                    return McpResponse::invalid_params(id, "Field 'commands' is required");
                };
                if lista.is_empty() {
                    return McpResponse::invalid_params(id, "Field 'commands' must not be empty");
                }
                // Validación completa ANTES de encolar nada.
                let mut preparados: Vec<(String, serde_json::Value)> = Vec::new();
                for c in lista {
                    let Some(tool) = c.get("tool").and_then(|v| v.as_str()) else {
                        return McpResponse::invalid_params(
                            id,
                            "Cada comando necesita su 'tool'",
                        );
                    };
                    if !is_mutating_tool(tool) {
                        return McpResponse::error(
                            id,
                            -32008,
                            format!("'{tool}' no es una tool mutante encolable"),
                        );
                    }
                    preparados.push((
                        tool.to_string(),
                        c.get("args").cloned().unwrap_or(json!({})),
                    ));
                }
                let ids: Vec<u64> = preparados
                    .iter()
                    .map(|(tool, a)| self.queue.submit(tool, a.clone()))
                    .collect();
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&json!({
                                "command_ids": ids,
                                "note": "Encolados en orden; el bucle los ejecuta en la siguiente pasada. Estado con glass_queue_status."
                            }))
                            .unwrap()
                        }]
                    }),
                )
            }

            "glass_queue_status" => {
                if let Some(cid) = args.get("command_id").and_then(|v| v.as_u64()) {
                    match self.queue.status(cid) {
                        Some(c) => {
                            let datos = crate::queue::command_json(&c);
                            McpResponse::success(
                                id,
                                json!({
                                    "content": [{
                                        "type": "text",
                                        "text": serde_json::to_string_pretty(&datos).unwrap()
                                    }]
                                }),
                            )
                        }
                        None => McpResponse::error(id, -32004, "Command not found"),
                    }
                } else {
                    let (pendientes, hechos, errores, total, pasada) = self.queue.summary();
                    let recientes: Vec<serde_json::Value> = self
                        .queue
                        .recent(10)
                        .iter()
                        .map(crate::queue::command_json)
                        .collect();
                    let datos = json!({
                        "pending": pendientes,
                        "done": hechos,
                        "errors": errores,
                        "total": total,
                        "current_pass": pasada,
                        "pump_alive": self.queue.pump_alive(),
                        "recent": recientes,
                    });
                    McpResponse::success(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string_pretty(&datos).unwrap()
                            }]
                        }),
                    )
                }
            }

            "glass_particle_emitter" => {
                let Some(entity_id) = args.get("entity_id").and_then(|n| n.as_u64()) else {
                    return McpResponse::invalid_params(id, "Field 'entity_id' is required");
                };
                let Some(ent) = scene.entities.iter_mut().find(|e| e.id == entity_id) else {
                    return McpResponse::error(id, -32004, "Entity not found");
                };
                if ent.particles.is_none() {
                    ent.particles = Some(crate::scene::ParticleEmitterData::default());
                    if !ent.components.iter().any(|c| c == "ParticleEmitter") {
                        ent.components.push("ParticleEmitter".to_string());
                    }
                }
                let d = ent.particles.as_mut().unwrap();
                // Aplicar los campos presentes (los demás quedan).
                let num = |args: &serde_json::Value, k: &str| args.get(k).and_then(|v| v.as_f64()).map(|v| v as f32);
                let arr3 = |args: &serde_json::Value, k: &str| -> Option<[f32; 3]> {
                    args.get(k).and_then(|v| v.as_array()).and_then(|a| {
                        if a.len() == 3 {
                            Some([a[0].as_f64()? as f32, a[1].as_f64()? as f32, a[2].as_f64()? as f32])
                        } else { None }
                    })
                };
                let arr2 = |args: &serde_json::Value, k: &str| -> Option<[f32; 2]> {
                    args.get(k).and_then(|v| v.as_array()).and_then(|a| {
                        if a.len() == 2 {
                            Some([a[0].as_f64()? as f32, a[1].as_f64()? as f32])
                        } else { None }
                    })
                };
                let arr4 = |args: &serde_json::Value, k: &str| -> Option<[f32; 4]> {
                    args.get(k).and_then(|v| v.as_array()).and_then(|a| {
                        if a.len() == 4 {
                            Some([a[0].as_f64()? as f32, a[1].as_f64()? as f32, a[2].as_f64()? as f32, a[3].as_f64()? as f32])
                        } else { None }
                    })
                };
                if let Some(v) = num(&args, "rate") { d.rate = v.max(0.0); }
                if let Some(v) = args.get("max_particles").and_then(|v| v.as_u64()) { d.max_particles = (v as usize).max(1); }
                if let Some(v) = arr2(&args, "lifetime") { d.lifetime = v; }
                if let Some(v) = arr2(&args, "start_speed") { d.start_speed = v; }
                if let Some(v) = arr2(&args, "start_size") { d.start_size = v; }
                if let Some(v) = arr3(&args, "gravity") { d.gravity = v; }
                if let Some(v) = num(&args, "drag") { d.drag = v.max(0.0); }
                if let Some(v) = args.get("shape").and_then(|v| v.as_str()) { d.shape = v.to_string(); }
                if let Some(v) = num(&args, "shape_radius") { d.shape_radius = v.max(0.0); }
                if let Some(v) = arr3(&args, "shape_extents") { d.shape_extents = v; }
                if let Some(v) = num(&args, "cone_angle") { d.cone_angle = v.clamp(0.0, 180.0); }
                if let Some(v) = args.get("local_space").and_then(|v| v.as_bool()) { d.local_space = v; }
                if let Some(v) = arr4(&args, "color_start") { d.color_start = v; }
                if let Some(v) = arr4(&args, "color_end") { d.color_end = v; }
                if let Some(v) = num(&args, "size_end_factor") { d.size_end_factor = v.max(0.0); }
                if let Some(v) = arr2(&args, "rotation_speed") { d.rotation_speed = v; }
                if let Some(v) = args.get("stretched").and_then(|v| v.as_bool()) { d.stretched = v; }
                if let Some(v) = num(&args, "stretch_factor") { d.stretch_factor = v.max(0.0); }
                if let Some(v) = args.get("seed").and_then(|v| v.as_u64()) { d.seed = v as u32; }
                if let Some(v) = args.get("playing").and_then(|v| v.as_bool()) { d.playing = v; }
                let datos = serde_json::to_value(&*d).unwrap_or(json!({}));
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&datos).unwrap()
                        }]
                    }),
                )
            }

            "glass_particle_emit" => {
                let Some(entity_id) = args.get("entity_id").and_then(|n| n.as_u64()) else {
                    return McpResponse::invalid_params(id, "Field 'entity_id' is required");
                };
                if !scene.entities.iter().any(|e| e.id == entity_id && e.particles.is_some()) {
                    return McpResponse::error(id, -32004, "Entity not found or has no particle emitter");
                }
                let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
                let cmd = match action {
                    "play" => glass_particles::ParticleCommand::Play(entity_id),
                    "stop" => glass_particles::ParticleCommand::Stop(entity_id),
                    "burst" => {
                        let n = args.get("count").and_then(|v| v.as_u64()).unwrap_or(50) as u32;
                        glass_particles::ParticleCommand::Burst(entity_id, n)
                    }
                    _ => return McpResponse::invalid_params(id, "Field 'action' must be play|stop|burst"),
                };
                glass_particles::push_command(cmd);
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Comando {action} encolado para la entidad {entity_id}; se aplica en el siguiente frame")
                        }]
                    }),
                )
            }

            "glass_particle_stats" => {
                let s = glass_core::particle_stats::snapshot();
                let datos = json!({
                    "alive": s.alive,
                    "emitters": s.emitters,
                    "draws": s.draws,
                    "sim_micros": s.sim_micros,
                    "overdraw": s.overdraw,
                });
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&datos).unwrap()
                        }]
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

                let steps = args.get("steps").and_then(|s| s.as_u64()).unwrap_or(30);
                if steps > MAX_SIMULATION_STEPS {
                    return McpResponse::invalid_params(id, "Field 'steps' exceeds the maximum of 3600 (60 s at 60 Hz)");
                }
                let steps = steps as usize;

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
            "glass_bake_lightmap" => {
                let nombre = args
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("LiveScene")
                    .to_string();
                if nombre.is_empty()
                    || nombre.chars().any(|c| c == '/' || c == '\\' || c == '.')
                {
                    return McpResponse::invalid_params(
                        id,
                        "Field 'name' must be a plain file stem",
                    );
                }
                let densidad = args
                    .get("density")
                    .and_then(|d| d.as_f64())
                    .unwrap_or(8.0) as f32;
                if !matches!(densidad.partial_cmp(&0.0), Some(std::cmp::Ordering::Greater))
                    || densidad > 256.0
                {
                    return McpResponse::invalid_params(
                        id,
                        "Field 'density' must be in (0, 256]",
                    );
                }
                let physics = self.get_physics();
                match crate::bake::bake_scene_lightmap(&scene, &physics, densidad) {
                    Ok(out) => {
                        match crate::bake::write_lightmap_files(
                            "assets/lightmaps",
                            &nombre,
                            &out,
                        ) {
                            Ok((png, json_path)) => McpResponse::success(
                                id,
                                json!({
                                    "content": [{
                                        "type": "text",
                                        "text": format!(
                                            "Lightmap horneado: {} entidades en {} (atlas {} px); saltados: {:?}. Metadatos: {}",
                                            out.baked_entities.len(),
                                            png,
                                            out.size,
                                            out.skipped_models,
                                            json_path
                                        )
                                    }]
                                }),
                            ),
                            Err(e) => McpResponse::error(
                                id,
                                -32000,
                                format!("No se pudo escribir el lightmap: {e}"),
                            ),
                        }
                    }
                    Err(e) => McpResponse::error(
                        id,
                        -32000,
                        format!("El horneado falló: {e}"),
                    ),
                }
            }

            "glass_get_input_axes" => {
                // Registro vivo primero (modo --mcp-live dentro del
                // editor); en stdio puro el registro está vacío y
                // se lee el archivo del proyecto, con los ejes por
                // defecto como último recurso.
                let mut defs = glass_scripting::input_axes::axis_defs();
                let mut fuente = "registro del motor en vivo";
                if defs.is_empty() {
                    if let Ok(texto) = std::fs::read_to_string("assets/input_axes.json") {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&texto) {
                            if let Some(arr) = v.get("axes") {
                                if let Ok(d) = serde_json::from_value::<
                                    Vec<glass_scripting::input_axes::InputAxisDef>,
                                >(arr.clone())
                                {
                                    defs = d;
                                    fuente = "assets/input_axes.json";
                                }
                            }
                        }
                    }
                }
                if defs.is_empty() {
                    defs = glass_scripting::input_axes::default_input_axes();
                    fuente = "valores por defecto (sin archivo ni registro)";
                }
                let texto = serde_json::to_string_pretty(&defs).unwrap_or_default();
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "Ejes de entrada configurados ({}; fuente: {}):\n{}",
                                defs.len(),
                                fuente,
                                texto
                            )
                        }]
                    }),
                )
            }

            "glass_list_materials" => {
                let mut lista: Vec<serde_json::Value> = Vec::new();
                if let Ok(rd) = std::fs::read_dir("assets/materials") {
                    for ent in rd.flatten() {
                        let path = ent.path();
                        let nombre = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if !nombre.ends_with(".material.json") {
                            continue;
                        }
                        if let Ok(texto) = std::fs::read_to_string(&path) {
                            if let Ok(mat) =
                                serde_json::from_str::<glass_core::MaterialData>(&texto)
                            {
                                lista.push(json!({
                                    "name": nombre.replace(".material.json", ""),
                                    "material": mat,
                                }));
                            }
                        }
                    }
                }
                lista.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "Materiales en assets/materials/ ({}):\n{}",
                                lista.len(),
                                serde_json::to_string_pretty(&lista).unwrap_or_default()
                            )
                        }]
                    }),
                )
            }

            "glass_set_entity_material" => {
                let entity_id = match args.get("entity_id").and_then(|n| n.as_u64()) {
                    Some(v) => v,
                    None => {
                        return McpResponse::invalid_params(id, "Field 'entity_id' is required")
                    }
                };
                let material = match args.get("material").and_then(|n| n.as_str()) {
                    Some(v) => v.to_string(),
                    None => {
                        return McpResponse::invalid_params(id, "Field 'material' is required")
                    }
                };
                if material.chars().count() > MAX_ENTITY_NAME_CHARS {
                    return McpResponse::invalid_params(id, "Field 'material' is too long");
                }
                let Some(ent) = scene.entities.iter_mut().find(|e| e.id == entity_id) else {
                    return McpResponse::error(id, -32004, "Entity not found");
                };
                let Some(mesh) = ent.mesh.as_mut() else {
                    return McpResponse::error(
                        id,
                        -32005,
                        "La entidad no tiene componente Mesh",
                    );
                };
                mesh.material = material.clone();
                log::info!(
                    "MCP glass_set_entity_material: entidad #{} material='{}'",
                    entity_id,
                    material
                );
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "Material de la entidad #{} ahora es '{}'",
                                entity_id, material
                            )
                        }]
                    }),
                )
            }

            "glass_set_ui_text" => {
                let entity_id = match args.get("entity_id").and_then(|n| n.as_u64()) {
                    Some(v) => v,
                    None => {
                        return McpResponse::invalid_params(id, "Field 'entity_id' is required")
                    }
                };
                let texto = match args.get("text").and_then(|n| n.as_str()) {
                    Some(v) => v.to_string(),
                    None => {
                        return McpResponse::invalid_params(id, "Field 'text' is required")
                    }
                };
                if texto.chars().count() > 4096 {
                    return McpResponse::invalid_params(id, "Field 'text' is too long");
                }
                let Some(ent) = scene.entities.iter_mut().find(|e| e.id == entity_id) else {
                    return McpResponse::error(id, -32004, "Entity not found");
                };
                match ent.ui_text.as_mut() {
                    Some(t) => {
                        t.text = texto.clone();
                    }
                    None => {
                        let t = glass_core::UITextData {
                            text: texto.clone(),
                            ..Default::default()
                        };
                        ent.ui_text = Some(t);
                        if !ent.components.iter().any(|c| c == "UIText") {
                            ent.components.push("UIText".to_string());
                        }
                    }
                }
                log::info!(
                    "MCP glass_set_ui_text: entidad #{} texto actualizado",
                    entity_id
                );
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Texto UI de la entidad #{}: '{}'", entity_id, texto)
                        }]
                    }),
                )
            }

            tool @ ("glass_mesh_enter_edit"
            | "glass_mesh_apply_op"
            | "glass_mesh_undo"
            | "glass_mesh_redo"
            | "glass_mesh_set_dimension"
            | "glass_mesh_get_stats"
            | "glass_mesh_bake") => {
                // Puerta del complemento: sin él, las tools de malla no se ejecutan.
                if !self.mesh_tools_enabled() {
                    McpResponse::error(
                        id,
                        -32005,
                        "MeshForge está desactivada: instala o activa el complemento «MeshForge» en el Launcher.",
                    )
                } else {
                match mesh_tool(&self.mesh_session, tool, &|k: &str| args.get(k).cloned()) {
                    Ok(out) => McpResponse::success(
                        id,
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string_pretty(&out).unwrap_or_default()
                            }]
                        }),
                    ),
                    Err(MeshToolError::InvalidParams(m)) => McpResponse::invalid_params(id, &m),
                    Err(MeshToolError::Domain(m)) => McpResponse::error(id, -32005, &m),
                    Err(MeshToolError::Internal(m)) => McpResponse::error(id, -32000, &m),
                }
                }
            }
            _ => McpResponse::method_not_found(id, name),
        }
    }

    /// Ejecuta el bucle de transporte stdio JSON-RPC (usado cuando GlassEngine corre como servidor MCP)
    pub fn run_stdio_loop(&self) {
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        let mut reader = stdin.lock();

        // Límite duro por línea (ver MAX_LINE_BYTES): `lines()` dejaba
        // crecer el buffer sin cota hasta OOM si un cliente concatenaba
        // contexto sin '\n'. Cada línea se lee a través de un `take()`
        // acotado; si excede, se descarta el resto sin almacenarlo, se
        // responde -32700 y el bucle sigue vivo.
        let mut line = String::new();
        loop {
            line.clear();
            let read = reader
                .by_ref()
                .take(MAX_LINE_BYTES + 1)
                .read_line(&mut line);
            match read {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if line.len() as u64 > MAX_LINE_BYTES {
                        if !line.ends_with('\n') {
                            // Descartar hasta el fin de la línea sin guardarla.
                            loop {
                                match reader.fill_buf() {
                                    Ok(buf) if !buf.is_empty() => {
                                        match buf.iter().position(|&b| b == b'\n') {
                                            Some(i) => {
                                                reader.consume(i + 1);
                                                break;
                                            }
                                            None => {
                                                let n = buf.len();
                                                reader.consume(n);
                                            }
                                        }
                                    }
                                    _ => break,
                                }
                            }
                        }
                        let err_resp = McpResponse::error(
                            None,
                            -32700,
                            format!(
                                "Request line too long (limit {} bytes)",
                                MAX_LINE_BYTES
                            ),
                        );
                        if let Ok(s) = serde_json::to_string(&err_resp) {
                            let _ = writeln!(stdout, "{}", s);
                            let _ = stdout.flush();
                        }
                        continue;
                    }
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Some(resp) = self.handle_json(trimmed) {
                        let _ = writeln!(stdout, "{}", resp);
                        let _ = stdout.flush();
                    }
                }
                Err(_) => break,
            }
        }
    }
}

/// SHA-256 hexadecimal de un archivo (Fase 6, tools de assets).
fn sha256_hex(path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Some(format!("{:x}", h.finalize()))
}

/// Lee el chunk JSON de un GLB (Fase 6).
fn glb_json(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let data = std::fs::read(path).map_err(|e| format!("No se pudo leer {path:?}: {e}"))?;
    if data.len() < 20 || &data[0..4] != b"glTF" {
        return Err(format!("{path:?} no es un GLB válido"));
    }
    let chunk_len = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
    if &data[16..20] != b"JSON" || 20 + chunk_len > data.len() {
        return Err(format!("{path:?}: chunk JSON ausente o truncado"));
    }
    serde_json::from_slice(&data[20..20 + chunk_len])
        .map_err(|e| format!("{path:?}: JSON ilegible: {e}"))
}

/// Validación de un asset de modelo (Fase 6): sirve a
/// `glass_asset_validate` y a la rama .glb de `glass_asset_reload`.
/// Raíz del proyecto contra la que se resuelven las rutas de assets:
/// la que descubre el VFS subiendo hasta `project.json`; si no hay
/// proyecto, el directorio de trabajo. Las rutas de assets nunca se
/// pasan relativas a procesos hijos (el bridge de Blender): la
/// cadena resolvía contra el `$PWD` del lanzador y la recarga en
/// caliente fallaba al abrir la app desde otro directorio (defecto
/// de la Fase 10, corregido aquí).
fn raiz_proyecto() -> std::path::PathBuf {
    glass_core::AssetVfs::locate()
        .map(|v| v.root().to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

/// Ruta de asset (`assets/...`) en absoluto, contra la raíz.
fn ruta_asset_abs(path: &str) -> std::path::PathBuf {
    raiz_proyecto().join(path)
}

fn validar_asset(path: &str) -> Result<serde_json::Value, String> {
    if !path.starts_with("assets/") || path.contains("..") {
        return Err("La ruta debe estar bajo assets/ y sin '..'".into());
    }
    let ruta = std::path::Path::new(path);
    let ext = ruta
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let glb_path = if ext == "glb" {
        ruta.to_path_buf()
    } else if ext == "blend" || ext == "fbx" {
        ruta.with_extension("glb")
    } else {
        return Err("Extensión no soportada: se espera .blend, .fbx o .glb".into());
    };
    let glb_abs = ruta_asset_abs(&glb_path.to_string_lossy());
    let doc = glb_json(&glb_abs)?;
    let contar = |k: &str| doc.get(k).and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
    // Nodos con malla: instancias (misma malla en ≥2 nodos) y LODs.
    let mut por_malla: std::collections::HashMap<usize, Vec<String>> =
        std::collections::HashMap::new();
    let mut lods: std::collections::HashMap<String, Vec<u32>> = std::collections::HashMap::new();
    if let Some(nodes) = doc.get("nodes").and_then(|v| v.as_array()) {
        for n in nodes {
            let nombre = n.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(mi) = n.get("mesh").and_then(|v| v.as_u64()) {
                por_malla.entry(mi as usize).or_default().push(nombre.to_string());
            }
            if let Some((base, nivel)) = glass_renderer::lod_suffix(nombre) {
                lods.entry(base).or_default().push(nivel);
            }
        }
    }
    let instancias: Vec<serde_json::Value> = por_malla
        .iter()
        .filter(|(_, nombres)| nombres.len() > 1)
        .map(|(mi, nombres)| json!({ "mesh": mi, "nodes": nombres }))
        .collect();
    // Sidecar del bridge (si existe): colecciones/prefabs y frescura.
    let sidecar_path = std::path::PathBuf::from(format!("{}.meta.json", glb_abs.display()));
    let mut sidecar_info = json!({ "present": false });
    if let Ok(texto) = std::fs::read_to_string(&sidecar_path) {
        if let Ok(sc) = serde_json::from_str::<serde_json::Value>(&texto) {
            let mut info = json!({
                "present": true,
                "source": sc.get("source"),
                "exported_at": sc.get("exported_at"),
                "blender_version": sc.get("blender_version"),
                "collections": sc.get("collections"),
            });
            if ext == "blend" || ext == "fbx" {
                let actual = sha256_hex(&ruta_asset_abs(path));
                let registrado = sc.get("source_sha256").and_then(|v| v.as_str()).map(|s| s.to_string());
                info["fresh"] = json!(actual.is_some() && actual == registrado);
                info["source_sha256"] = json!(registrado);
            }
            sidecar_info = info;
        }
    }
    Ok(json!({
        "path": path,
        "glb": glb_path.to_string_lossy(),
        "counts": {
            "meshes": contar("meshes"),
            "nodes": contar("nodes"),
            "materials": contar("materials"),
            "animations": contar("animations"),
            "skins": contar("skins"),
        },
        "instances": instancias,
        "lod_groups": lods,
        "sidecar": sidecar_info,
    }))
}

// ---------------------------------------------------------------------------
// Tools glass_mesh_*: operan sobre la sesión de malla en proceso, no sobre la escena.
// ---------------------------------------------------------------------------

/// Error de una tool de malla antes de convertirlo en respuesta MCP.
enum MeshToolError {
    InvalidParams(String),
    Domain(String),
    Internal(String),
}

type MeshSessionSlot = std::sync::Mutex<Option<glass_mesh_edit::MeshSession>>;

const NO_MESH_SESSION: &str = "No hay sesión de malla activa (usa glass_mesh_enter_edit)";

fn lock_mesh_session(
    slot: &MeshSessionSlot,
) -> Result<std::sync::MutexGuard<'_, Option<glass_mesh_edit::MeshSession>>, MeshToolError> {
    slot.lock()
        .map_err(|_| MeshToolError::Internal("Mutex poison error".to_string()))
}

fn mesh_no_session() -> MeshToolError {
    MeshToolError::Domain(NO_MESH_SESSION.to_string())
}

fn mesh_domain(e: impl std::fmt::Display) -> MeshToolError {
    MeshToolError::Domain(e.to_string())
}

fn mesh_asset_path(arg: Option<serde_json::Value>, key: &str) -> Result<String, MeshToolError> {
    let path = arg
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or_else(|| MeshToolError::InvalidParams(format!("Falta '{key}' (string)")))?;
    if !path.starts_with("assets/") || path.contains("..") {
        return Err(MeshToolError::InvalidParams(
            "La ruta debe estar bajo assets/ y sin '..'".to_string(),
        ));
    }
    Ok(path)
}

fn mesh_size_arg(arg: Option<serde_json::Value>) -> Result<[f32; 3], MeshToolError> {
    let err = || MeshToolError::InvalidParams("Falta 'size' (array de 3 números)".to_string());
    let arr = arg
        .as_ref()
        .and_then(serde_json::Value::as_array)
        .filter(|a| a.len() == 3)
        .ok_or_else(err)?;
    let mut size = [0.0f32; 3];
    for (out, v) in size.iter_mut().zip(arr) {
        *out = v.as_f64().ok_or_else(err)? as f32;
    }
    Ok(size)
}

fn mesh_command_name(cmd: &glass_mesh_edit::commands::MeshCommand) -> &'static str {
    use glass_mesh_edit::commands::MeshCommand;
    match cmd {
        MeshCommand::MoveVerts { .. } => "move_verts",
        MeshCommand::SetVert { .. } => "set_vert",
        MeshCommand::ScaleSelection { .. } => "scale_selection",
        MeshCommand::Mirror { .. } => "mirror",
        MeshCommand::Subdivide { .. } => "subdivide",
        MeshCommand::Array { .. } => "array",
        MeshCommand::Weld { .. } => "weld",
        MeshCommand::DeleteVerts { .. } => "delete_verts",
        MeshCommand::Boolean { .. } => "boolean",
    }
}

/// Ejecuta una tool `glass_mesh_*`; `arg` extrae un argumento por nombre.
fn mesh_tool(
    slot: &MeshSessionSlot,
    tool: &str,
    arg: &dyn Fn(&str) -> Option<serde_json::Value>,
) -> Result<serde_json::Value, MeshToolError> {
    use glass_mesh_edit::commands::MeshCommand;
    use glass_mesh_edit::MeshSession;
    use serde_json::json;

    match tool {
        "glass_mesh_enter_edit" => {
            let path = mesh_asset_path(arg("path"), "path")?;
            let session =
                MeshSession::enter_from_glb(std::path::Path::new(&path)).map_err(mesh_domain)?;
            let out = json!({
                "entered": true,
                "name": session.name,
                "verts": session.mesh.verts.len(),
                "faces": session.mesh.faces.len(),
            });
            *lock_mesh_session(slot)? = Some(session);
            Ok(out)
        }
        "glass_mesh_apply_op" => {
            let raw = arg("command")
                .filter(serde_json::Value::is_object)
                .ok_or_else(|| {
                    MeshToolError::InvalidParams(
                        "Falta 'command' (objeto JSON de MeshCommand)".to_string(),
                    )
                })?;
            let cmd: MeshCommand = serde_json::from_value(raw)
                .map_err(|e| MeshToolError::InvalidParams(format!("command inválido: {e}")))?;
            let applied = mesh_command_name(&cmd);
            let mut guard = lock_mesh_session(slot)?;
            let session = guard.as_mut().ok_or_else(mesh_no_session)?;
            session.apply(cmd).map_err(mesh_domain)?;
            Ok(json!({
                "applied": applied,
                "verts": session.mesh.verts.len(),
                "faces": session.mesh.faces.len(),
            }))
        }
        "glass_mesh_undo" => {
            let mut guard = lock_mesh_session(slot)?;
            let session = guard.as_mut().ok_or_else(mesh_no_session)?;
            let undone = session.undo().map(|c| mesh_command_name(&c));
            Ok(json!({
                "undone": undone,
                "can_undo": session.history.can_undo(),
                "can_redo": session.history.can_redo(),
            }))
        }
        "glass_mesh_redo" => {
            let mut guard = lock_mesh_session(slot)?;
            let session = guard.as_mut().ok_or_else(mesh_no_session)?;
            let redone = session.redo().map(|c| mesh_command_name(&c));
            Ok(json!({
                "redone": redone,
                "can_undo": session.history.can_undo(),
                "can_redo": session.history.can_redo(),
            }))
        }
        "glass_mesh_set_dimension" => {
            let size = mesh_size_arg(arg("size"))?;
            let mut guard = lock_mesh_session(slot)?;
            let session = guard.as_mut().ok_or_else(mesh_no_session)?;
            session.set_dimensions(size).map_err(mesh_domain)?;
            let (stats, _) = session.stats();
            Ok(json!({
                "bbox_min": stats.bbox_min,
                "bbox_max": stats.bbox_max,
            }))
        }
        "glass_mesh_get_stats" => {
            let mut guard = lock_mesh_session(slot)?;
            let session = guard.as_mut().ok_or_else(mesh_no_session)?;
            let (stats, manifold) = session.stats();
            Ok(json!({
                "verts": stats.verts,
                "faces": stats.faces,
                "edges": stats.edges,
                "bbox_min": stats.bbox_min,
                "bbox_max": stats.bbox_max,
                "surface_area": stats.surface_area,
                "boundary_edges": manifold.boundary_edges,
                "non_manifold_edges": manifold.non_manifold_edges,
                "degenerate_faces": manifold.degenerate_faces,
                "is_closed_manifold": manifold.is_closed_manifold,
                "history_len": session.history.len(),
                "can_undo": session.history.can_undo(),
                "can_redo": session.history.can_redo(),
                "recipe_commands": session.recipe.commands.len(),
            }))
        }
        "glass_mesh_bake" => {
            let glb_path = mesh_asset_path(arg("glb_path"), "glb_path")?;
            let recipe_path = mesh_asset_path(arg("recipe_path"), "recipe_path")?;
            let guard = lock_mesh_session(slot)?;
            let session = guard.as_ref().ok_or_else(mesh_no_session)?;
            session
                .save(
                    std::path::Path::new(&glb_path),
                    std::path::Path::new(&recipe_path),
                )
                .map_err(mesh_domain)?;
            Ok(json!({
                "baked": true,
                "glb_path": glb_path,
                "recipe_path": recipe_path,
                "verts": session.mesh.verts.len(),
                "faces": session.mesh.faces.len(),
            }))
        }
        _ => Err(MeshToolError::Internal(format!(
            "tool de malla desconocida: {tool}"
        ))),
    }
}

#[cfg(test)]
mod tests_catchup {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn servidor() -> McpServer {
        McpServer::new(Arc::new(Mutex::new(SceneState::new())))
    }

    fn llamar(srv: &McpServer, tool: &str, args: serde_json::Value) -> String {
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{}","arguments":{}}}}}"#,
            tool, args
        );
        srv.handle_json(&req).unwrap_or_default()
    }

    #[test]
    fn lista_incluye_las_tools_de_prefab() {
        let srv = servidor();
        let resp = srv
            .handle_json(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .unwrap_or_default();
        for tool in [
            "glass_prefab_create",
            "glass_prefab_create_variant",
            "glass_prefab_instantiate",
            "glass_prefab_list",
            "glass_prefab_revert",
            "glass_prefab_overrides",
        ] {
            assert!(resp.contains(tool), "tools/list no declara {tool}");
        }
        assert!(is_mutating_tool("glass_prefab_create"));
        assert!(is_mutating_tool("glass_prefab_create_variant"));
        assert!(is_mutating_tool("glass_prefab_instantiate"));
        assert!(is_mutating_tool("glass_prefab_revert"));
        assert!(!is_mutating_tool("glass_prefab_list"));
        assert!(!is_mutating_tool("glass_prefab_overrides"));
    }

    #[test]
    fn prefab_list_responde_con_el_formato() {
        // El conteo depende de la raíz que localice el VFS en el entorno
        // de test (el repo trae assets/prefabs de ejemplo): lo que se
        // fija aquí es el contrato — éxito y la cabecera del listado.
        let srv = servidor();
        let resp = llamar(&srv, "glass_prefab_list", json!({}));
        assert!(!resp.contains("\"error\""), "respuesta inesperada: {resp}");
        assert!(
            resp.contains("Prefabs en el proyecto ("),
            "respuesta inesperada: {resp}"
        );
    }

    #[test]
    fn prefab_create_rechaza_rutas_invalidas() {
        let srv = servidor();
        let sin_assets = llamar(
            &srv,
            "glass_prefab_create",
            json!({"entity_id": 1, "path": "prefabs/x.prefab.json"}),
        );
        assert!(sin_assets.contains("error"), "respuesta inesperada: {sin_assets}");

        let con_salto = llamar(
            &srv,
            "glass_prefab_create",
            json!({"entity_id": 1, "path": "Assets/../x.prefab.json"}),
        );
        assert!(con_salto.contains("error"), "respuesta inesperada: {con_salto}");
    }

    #[test]
    fn prefab_create_variant_rechaza_rutas_invalidas() {
        let srv = servidor();
        let sin_assets = llamar(
            &srv,
            "glass_prefab_create_variant",
            json!({"path": "Assets/prefabs/base.prefab.json", "variant_path": "prefabs/v.prefab.json"}),
        );
        assert!(sin_assets.contains("error"), "respuesta inesperada: {sin_assets}");

        let con_salto = llamar(
            &srv,
            "glass_prefab_create_variant",
            json!({"path": "Assets/../base.prefab.json", "variant_path": "Assets/prefabs/v.prefab.json"}),
        );
        assert!(con_salto.contains("error"), "respuesta inesperada: {con_salto}");
    }

    #[test]
    fn prefab_create_variant_sin_base_da_32004() {
        let srv = servidor();
        let resp = llamar(
            &srv,
            "glass_prefab_create_variant",
            json!({"path": "Assets/prefabs/no_existe_base.prefab.json", "variant_path": "Assets/prefabs/v_nueva.prefab.json"}),
        );
        assert!(resp.contains("32004"), "respuesta inesperada: {resp}");
    }

    #[test]
    fn prefab_instantiate_inexistente_da_32010() {
        let srv = servidor();
        let resp = llamar(
            &srv,
            "glass_prefab_instantiate",
            json!({"path": "Assets/prefabs/no_existe.prefab.json"}),
        );
        // -32010 = asset ausente, el mismo código que usa glass_prefab_revert.
        assert!(resp.contains("32010"), "respuesta inesperada: {resp}");
    }

    #[test]
    fn prefab_revert_sobre_entidad_normal_falla() {
        let srv = servidor();
        let entidad = {
            let mut sc = srv.scene.lock().unwrap();
            sc.create_entity("Normal".to_string(), vec!["Transform".into()])
        };
        let resp = llamar(&srv, "glass_prefab_revert", json!({"entity_id": entidad}));
        assert!(resp.contains("32013"), "respuesta inesperada: {resp}");

        let resp = llamar(&srv, "glass_prefab_revert", json!({"entity_id": 999_999}));
        assert!(resp.contains("32004"), "respuesta inesperada: {resp}");
    }

    #[test]
    fn prefab_overrides_devuelve_el_diff() {
        let srv = servidor();
        let entidad = {
            let mut sc = srv.scene.lock().unwrap();
            let id = sc.create_entity("Instancia".to_string(), vec!["Transform".into()]);
            let e = sc.get_entity_mut(id).unwrap();
            e.prefab_source = Some("Assets/prefabs/Caja.prefab.json".to_string());
            e.prefab_overrides = Some(glass_core::prefab::PrefabOverrides {
                propiedades: vec![glass_core::prefab::OverridePropiedad {
                    local_id: 6,
                    path: "transform.position".to_string(),
                    valor: serde_json::json!([1.0, 2.0, 3.0]),
                    huerfana: false,
                }],
                anadidas: vec![glass_core::prefab::OverrideAnadida {
                    padre_local_id: Some(5),
                    nombre: "Extra".to_string(),
                    indice: 0,
                    subarbol: None,
                }],
                quitadas: vec![7],
            });
            id
        };
        let resp = llamar(&srv, "glass_prefab_overrides", json!({"entity_id": entidad}));
        assert!(!resp.contains("\"error\""), "respuesta inesperada: {resp}");
        assert!(resp.contains("transform.position"), "respuesta inesperada: {resp}");
        assert!(resp.contains("Extra"), "respuesta inesperada: {resp}");
        assert!(resp.contains("Caja.prefab.json"), "respuesta inesperada: {resp}");

        let normal = {
            let mut sc = srv.scene.lock().unwrap();
            sc.create_entity("Normal".to_string(), vec!["Transform".into()])
        };
        let resp = llamar(&srv, "glass_prefab_overrides", json!({"entity_id": normal}));
        assert!(resp.contains("32013"), "respuesta inesperada: {resp}");
        let resp = llamar(&srv, "glass_prefab_overrides", json!({"entity_id": 999_999}));
        assert!(resp.contains("32004"), "respuesta inesperada: {resp}");
    }

    #[test]
    fn lista_incluye_las_tools_nuevas() {
        let srv = servidor();
        let resp = srv
            .handle_json(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .unwrap_or_default();
        for nombre in [
            "glass_bake_lightmap",
            "glass_get_input_axes",
            "glass_list_materials",
            "glass_set_entity_material",
            "glass_set_ui_text",
        ] {
            assert!(resp.contains(nombre), "falta {nombre} en tools/list");
        }
    }

    #[test]
    fn input_axes_devuelve_los_registrados() {
        glass_scripting::input_axes::set_axis_defs(
            glass_scripting::input_axes::default_input_axes(),
        );
        let srv = servidor();
        let resp = llamar(&srv, "glass_get_input_axes", json!({}));
        assert!(resp.contains("Horizontal"), "resp: {resp}");
        assert!(resp.contains("Jump"), "resp: {resp}");
    }

    #[test]
    fn set_material_y_texto_ui_sobre_entidad() {
        let srv = servidor();
        let id = {
            let mut sc = srv.scene.lock().unwrap();
            sc.create_entity("Cubo MCP".to_string(), vec!["Transform".into()])
        };
        // Sin Mesh, asignar material es un error claro (-32005).
        let resp = llamar(
            &srv,
            "glass_set_entity_material",
            json!({ "entity_id": id, "material": "oro" }),
        );
        assert!(resp.contains("-32005"), "resp: {resp}");
        // El texto UI sí se crea sobre la entidad.
        let resp = llamar(
            &srv,
            "glass_set_ui_text",
            json!({ "entity_id": id, "text": "Hola MCP" }),
        );
        assert!(resp.contains("Hola MCP"), "resp: {resp}");
        let sc = srv.scene.lock().unwrap();
        let ent = sc.entities.iter().find(|e| e.id == id).unwrap();
        assert_eq!(ent.ui_text.as_ref().unwrap().text, "Hola MCP");
    }

    #[test]
    fn bake_escena_vacia_hornea_cero_entidades() {
        // SceneState::new() trae su luz principal por defecto: el
        // horneado de una escena sin estáticos tiene éxito con 0
        // entidades y escribe los dos archivos (se limpian aquí).
        let srv = servidor();
        let resp = llamar(&srv, "glass_bake_lightmap", json!({ "name": "TestMcp" }));
        assert!(resp.contains("0 entidades"), "resp: {resp}");
        let _ = std::fs::remove_file("assets/lightmaps/TestMcp.lm.png");
        let _ = std::fs::remove_file("assets/lightmaps/TestMcp.lm.json");
    }

    /// Nombres de las tools que anuncia tools/list.
    fn nombres_tools_listadas(server: &McpServer) -> Vec<String> {
        let resp = server
            .handle_json(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&resp).expect("respuesta JSON válida");
        v["result"]["tools"]
            .as_array()
            .expect("array tools")
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_owned))
            .collect()
    }

    #[test]
    fn sin_puerta_lista_incluye_mesh() {
        let server = servidor();
        let nombres = nombres_tools_listadas(&server);
        assert!(nombres.iter().any(|n| n == "glass_mesh_enter_edit"));
        // La constante debe coincidir con lo que realmente se anuncia.
        for n in MESH_TOOL_NAMES {
            assert!(nombres.iter().any(|x| x == n), "falta {n} en tools/list");
        }
    }

    #[test]
    fn con_puerta_cerrada_lista_omite_mesh() {
        let server = servidor();
        server.set_mesh_tools_enabled(false);

        let nombres = nombres_tools_listadas(&server);
        assert!(!nombres.iter().any(|n| n.starts_with("glass_mesh_")));
        assert!(nombres.iter().any(|n| n == "glass_get_scene_hierarchy"));

        let resp = server
            .handle_json(
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"glass_mesh_enter_edit","arguments":{}}}"#,
            )
            .unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&resp).expect("respuesta JSON válida");
        assert!(v.get("error").is_some(), "se esperaba error: {resp}");
    }
}
