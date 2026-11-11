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
                let mut datos = serde_json::to_value(&snap).unwrap_or(json!({}));
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
                    "process": serde_json::to_value(&pm).unwrap_or(json!({})),
                    "gpu": serde_json::to_value(&gpu).unwrap_or(serde_json::Value::Null)
                });
                let texto = serde_json::to_string_pretty(&datos).unwrap_or_default();
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
                    });
                }

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
                if !(densidad > 0.0) || densidad > 256.0 {
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
                        let mut t = glass_core::UITextData::default();
                        t.text = texto.clone();
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
}
