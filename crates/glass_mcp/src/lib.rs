//! glass_mcp: Internal lightweight JSON-RPC MCP server for runtime engine introspection.

pub mod protocol;
pub mod scene;
pub mod server;

pub use protocol::{McpError, McpRequest, McpResponse};
pub use scene::{AudioSourceData, EntityData, MeshData, PhysicsData, SceneState, ScriptComponentData, TransformData};
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

    #[test]
    fn test_mcp_platformer_scene_and_physics_climbing() {
        let scene = Arc::new(Mutex::new(SceneState::new()));
        let server = McpServer::new(scene.clone());

        // 1. Setup Platformer Scene via MCP
        let setup_req = r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"glass_setup_platformer_scene","arguments":{"player_initial_y":1.0}}}"#;
        let setup_resp = server.handle_json(setup_req).expect("Failed setup platformer");
        assert!(setup_resp.contains("Suelo Estático"), "Got response: {}", setup_resp);
        assert!(setup_resp.contains("Escalón 1"));
        assert!(setup_resp.contains("Escalón 2"));
        assert!(setup_resp.contains("Cubo Jugador 3D"));

        // 2. Query Hierarchy
        let hier_req = r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"glass_get_scene_hierarchy"}}"#;
        let hier_resp = server.handle_json(hier_req).expect("Failed get hierarchy");
        assert!(hier_resp.contains("Static Floor"));
        assert!(hier_resp.contains("Low Platform Step 1"));
        assert!(hier_resp.contains("High Platform Step 2"));
        assert!(hier_resp.contains("Player 3D Cube"));
        assert!(hier_resp.contains("PlayerController"));

        // 3. Audio status query via MCP
        let audio_status_req = r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"glass_get_audio_status"}}"#;
        let audio_status_resp = server.handle_json(audio_status_req).expect("Failed get audio status");
        assert!(audio_status_resp.contains("beep_440"));
        assert!(audio_status_resp.contains("beep_880"));

        // 4. Play audio clip via MCP
        let play_audio_req = r#"{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"glass_play_audio","arguments":{"clip":"beep_880","volume":0.8,"pitch":1.2}}}"#;
        let play_audio_resp = server.handle_json(play_audio_req).expect("Failed play audio");
        assert!(play_audio_resp.contains("reproducido con éxito"));

        // 5. Simulate Jump
        let jump_req = r#"{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"glass_simulate_player","arguments":{"action":"jump","steps":15}}}"#;
        let jump_resp = server.handle_json(jump_req).expect("Failed jump");
        assert!(jump_resp.contains("beep_880 (Salto)"));

        // 6. Simulate Climbing onto Step 1
        let climb1_req = r#"{"jsonrpc":"2.0","id":15,"method":"tools/call","params":{"name":"glass_simulate_player","arguments":{"action":"step_climb","steps":35}}}"#;
        let climb1_resp = server.handle_json(climb1_req).expect("Failed climb 1");
        assert!(climb1_resp.contains("climbed_step1") || climb1_resp.contains("position"));
    }
}
