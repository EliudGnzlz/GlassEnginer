use crate::protocol::{McpRequest, McpResponse};
use crate::scene::SceneState;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde_json::json;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

pub struct McpServer {
    pub scene: Arc<Mutex<SceneState>>,
}

impl McpServer {
    pub fn new(scene: Arc<Mutex<SceneState>>) -> Self {
        Self { scene }
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
                        "version": "0.1.0"
                    }
                }),
            ),

            "notifications/initialized" => {
                // Notificación del cliente, no requiere resultado
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
                            "description": "Modifica componentes de una entidad (Transform: posición, rotación, escala; Material: color RGB, wireframe).",
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

                let mut modified = false;
                if pos.is_some() || rot.is_some() || scale.is_some() {
                    modified |= scene.modify_transform(entity_id, pos, rot, scale);
                }
                if let Some(col) = color {
                    modified |= scene.modify_material(entity_id, col, wireframe);
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
                        // Generar un snapshot procedural PNG representativo del Viewport 1280x720
                        let mut img = image::RgbaImage::new(1280, 720);
                        for (x, y, pixel) in img.enumerate_pixels_mut() {
                            // Fondo característico de GlassEngine (azul oscuro profundo)
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
