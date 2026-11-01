use crate::protocol::{McpRequest, McpResponse};
use serde_json::json;

pub struct McpRouter;

impl McpRouter {
    pub fn handle_request(req: McpRequest) -> McpResponse {
        match req.method.as_str() {
            "glass_get_scene_hierarchy" => McpResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(json!({
                    "entities": [
                        {"id": 1, "name": "Main Camera", "components": ["Transform", "Camera"]},
                        {"id": 2, "name": "Directional Light", "components": ["Transform", "Light"]}
                    ]
                })),
                error: None,
            },
            "glass_create_entity" => McpResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(json!({"status": "created", "entity_id": 3})),
                error: None,
            },
            _ => McpResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: None,
                error: Some(json!({"code": -32601, "message": "Method not found"})),
            },
        }
    }
}
