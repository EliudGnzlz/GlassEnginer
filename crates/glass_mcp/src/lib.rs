//! glass_mcp: Internal lightweight JSON-RPC MCP server for runtime engine introspection.

pub mod protocol;
pub mod scene;
pub mod server;

pub use protocol::{McpError, McpRequest, McpResponse};
pub use scene::{EntityData, MeshData, SceneState, TransformData};
pub use server::McpServer;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_mcp_initialize_and_tools_list() {
        let scene = Arc::new(Mutex::new(SceneState::new()));
        let server = McpServer::new(scene);

        // 1. Initialize
        let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let init_resp = server.handle_json(init_req).expect("Failed to handle initialize");
        assert!(init_resp.contains("GlassEngine-MCP"));
        assert!(init_resp.contains("2024-11-05"));

        // 2. Tools List
        let list_req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
        let list_resp = server.handle_json(list_req).expect("Failed to handle tools/list");
        assert!(list_resp.contains("glass_get_scene_hierarchy"));
        assert!(list_resp.contains("glass_create_entity"));
        assert!(list_resp.contains("glass_modify_component"));
        assert!(list_resp.contains("glass_capture_viewport"));
        assert!(list_resp.contains("glass_reload_shaders"));
    }

    #[test]
    fn test_mcp_entity_manipulation_and_viewport_capture() {
        let scene = Arc::new(Mutex::new(SceneState::new()));
        let server = McpServer::new(scene.clone());

        // 1. Get Scene Hierarchy
        let get_req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"glass_get_scene_hierarchy"}}"#;
        let get_resp = server.handle_json(get_req).expect("Failed get hierarchy");
        assert!(get_resp.contains("Main Camera"));
        assert!(get_resp.contains("Directional Light"));

        // 2. Create Entity
        let create_req = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"glass_create_entity","arguments":{"name":"TestNPC","components":["Transform","Script"]}}}"#;
        let create_resp = server.handle_json(create_req).expect("Failed create entity");
        assert!(create_resp.contains("TestNPC"));

        // Verify entity exists in scene state
        {
            let s = scene.lock().unwrap();
            let npc = s.entities.iter().find(|e| e.name == "TestNPC");
            assert!(npc.is_some());
            assert_eq!(npc.unwrap().id, 5);
        }

        // 3. Modify Component Transform & Material
        let modify_req = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"glass_modify_component","arguments":{"entity_id":5,"position":[10.0,2.5,-3.0],"color":[1.0,0.5,0.0]}}}"#;
        let modify_resp = server.handle_json(modify_req).expect("Failed modify component");
        assert!(modify_resp.contains("modificada con éxito"));

        // Verify updated values in scene state
        {
            let s = scene.lock().unwrap();
            let npc = s.entities.iter().find(|e| e.name == "TestNPC").unwrap();
            assert_eq!(npc.transform.position, [10.0, 2.5, -3.0]);
            assert_eq!(npc.mesh.as_ref().unwrap().color, [1.0, 0.5, 0.0]);
        }

        // 4. Capture Viewport Snapshot
        let capture_req = r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"glass_capture_viewport"}}"#;
        let capture_resp = server.handle_json(capture_req).expect("Failed capture viewport");
        assert!(capture_resp.contains("image/png"));
        assert!(capture_resp.contains("data"));

        // 5. Reload Shaders
        let reload_req = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"glass_reload_shaders"}}"#;
        let reload_resp = server.handle_json(reload_req).expect("Failed reload shaders");
        assert!(reload_resp.contains("recargados exitosamente"));
        assert!(scene.lock().unwrap().shaders_reloaded_flag);
    }
}
