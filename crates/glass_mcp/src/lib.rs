//! glass_mcp: Internal lightweight JSON-RPC MCP server for runtime engine introspection.

pub mod bake;
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

    #[test]
    fn test_mcp_shared_scene_matches_editor_view() {
        // Réplica del cableado de la app en modo editor: UN
        // `Arc<Mutex<SceneState>>` y dos consumidores — el servidor MCP
        // (el hilo stdio) y el "editor" (el otro clon del Arc, que muta
        // directo como hace glass_editor). Lo que escribe uno lo lee el
        // otro sin copia intermedia: es la propiedad que define el MCP
        // sobre la escena viva.
        let scene = Arc::new(Mutex::new(SceneState::new()));
        let server = McpServer::new(scene.clone());
        let editor_side = scene.clone();

        // 1. Escritura por MCP -> visible desde el Arc del "editor".
        let create_req = r#"{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"glass_create_entity","arguments":{"name":"Entidad MCP Viva","components":["Transform","MeshRenderer"]}}}"#;
        let create_resp = server.handle_json(create_req).expect("create via MCP");
        assert!(create_resp.contains("Entidad MCP Viva"));
        {
            let s = editor_side.lock().unwrap();
            let ent = s
                .entities
                .iter()
                .find(|e| e.name == "Entidad MCP Viva")
                .expect("el editor debe ver la entidad creada por MCP");
            assert!(ent.components.iter().any(|c| c == "MeshRenderer"));
        }

        // 2. Escritura directa en el Arc ("editor") -> visible por MCP.
        let editor_entity_id = {
            let mut s = editor_side.lock().unwrap();
            let id = s.create_entity(
                "Entidad del Editor".into(),
                vec!["Transform".into(), "MeshRenderer".into()],
            );
            assert!(s.modify_transform(id, Some([7.0, 8.0, 9.0]), None, None));
            id
        };
        let hier_req = r#"{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"glass_get_scene_hierarchy"}}"#;
        let hier_resp = server.handle_json(hier_req).expect("hierarchy via MCP");
        assert!(
            hier_resp.contains("Entidad del Editor"),
            "el MCP debe ver la entidad del editor: {}",
            hier_resp
        );
        assert!(hier_resp.contains("Entidad MCP Viva"));

        // 3. Modificación por MCP de la entidad del "editor" -> el Arc
        // refleja el cambio.
        let modify_req = format!(
            r#"{{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{{"name":"glass_modify_component","arguments":{{"entity_id":{},"position":[1.0,2.0,3.0]}}}}}}"#,
            editor_entity_id
        );
        let modify_resp = server.handle_json(&modify_req).expect("modify via MCP");
        assert!(modify_resp.contains("modificada con éxito"));
        let s = editor_side.lock().unwrap();
        let ent = s.entities.iter().find(|e| e.id == editor_entity_id).unwrap();
        assert_eq!(ent.transform.position, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_mcp_shared_scene_concurrent_editor_and_mcp_writes() {
        use std::collections::HashSet;
        // El hilo MCP y el "editor" comparten el Mutex de la escena:
        // escrituras concurrentes no deben interbloquearse ni duplicar
        // ids (la asignación vive dentro del lock, en `next_entity_id`).
        let scene = Arc::new(Mutex::new(SceneState::new()));
        let server = Arc::new(McpServer::new(scene.clone()));
        let initial = scene.lock().unwrap().entities.len();

        const N: usize = 60; // creadas por MCP desde el hilo secundario
        const M: usize = 60; // creadas directo en el Arc, hilo principal

        let server_thread = server.clone();
        let scene_thread_side = scene.clone();
        // Señal de fin con timeout: si una regresión metiera un deadlock,
        // el test debe FALLAR con diagnóstico, no colgar la suite entera
        // en un `join()` sin límite (apunte de revisión externa).
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let mcp_thread = std::thread::spawn(move || {
            for i in 0..N {
                let req = format!(
                    r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"glass_create_entity","arguments":{{"name":"Concurrente MCP {}"}}}}}}"#,
                    100 + i,
                    i
                );
                let resp = server_thread
                    .handle_json(&req)
                    .expect("create concurrente MCP");
                assert!(resp.contains("Entidad creada exitosamente"), "{}", resp);
            }
            // El hilo también lee el Arc directo: sin deadlock cruzado.
            let _ = scene_thread_side.lock().unwrap().entities.len();
            let _ = done_tx.send(());
        });

        // "Editor": escrituras directas y lecturas por MCP entrelazadas,
        // para forzar contención real sobre el mismo Mutex.
        for i in 0..M {
            {
                let mut s = scene.lock().unwrap();
                s.create_entity(
                    format!("Concurrente Editor {}", i),
                    vec!["Transform".into()],
                );
            }
            let hier_req = r#"{"jsonrpc":"2.0","id":200,"method":"tools/call","params":{"name":"glass_get_scene_hierarchy"}}"#;
            assert!(server.handle_json(hier_req).is_some());
        }
        done_rx
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("el hilo MCP no terminó en 60 s: posible deadlock");
        mcp_thread
            .join()
            .expect("el hilo MCP no debe panicar ni interbloquearse");

        let s = scene.lock().unwrap();
        assert_eq!(s.entities.len(), initial + N + M);
        let ids: HashSet<u64> = s.entities.iter().map(|e| e.id).collect();
        assert_eq!(ids.len(), s.entities.len(), "ids duplicados bajo concurrencia");
    }
}
