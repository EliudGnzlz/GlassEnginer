//! glass_mcp: Internal lightweight JSON-RPC MCP server for runtime engine introspection.

// La lista de tools es un json! gigante: la expansión de la macro
// necesita más recursión que el límite por defecto (llegó al techo al
// añadir glass_render_lighting_debug, Fase 2S).
#![recursion_limit = "512"]

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
    fn test_mcp_lote_espacial_y_profiler() {
        let scene = Arc::new(Mutex::new(SceneState::new()));
        let server = McpServer::new(scene.clone());

        // (a) tools/list anuncia las tres tools nuevas (14 -> 17).
        let list_resp = server
            .handle_json(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#)
            .expect("tools/list");
        for nombre in [
            "glass_batch_spawn_entities",
            "glass_inspect_spatial",
            "glass_profiler_snapshot",
        ] {
            assert!(list_resp.contains(nombre), "falta {} en tools/list", nombre);
        }

        // (b) Lote válido: 3 entidades, dos con posición.
        let batch_req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"glass_batch_spawn_entities","arguments":{"entities":[{"name":"Lote Cerca","position":[1.0,0.0,0.0]},{"name":"Lote Lejos","position":[3.0,0.0,0.0]},{"name":"Lote Sin Pos"}]}}}"#;
        let batch_resp = server.handle_json(batch_req).expect("batch spawn");
        assert!(batch_resp.contains("Creadas 3 entidades"), "{}", batch_resp);
        assert!(batch_resp.contains("ids asignados: [5, 6, 7]"), "{}", batch_resp);
        {
            let s = scene.lock().unwrap();
            assert_eq!(s.entities.len(), 7);
            let cerca = s.entities.iter().find(|e| e.name == "Lote Cerca").unwrap();
            assert_eq!(cerca.transform.position, [1.0, 0.0, 0.0]);
            let lejos = s.entities.iter().find(|e| e.name == "Lote Lejos").unwrap();
            assert_eq!(lejos.transform.position, [3.0, 0.0, 0.0]);
        }

        // (c) Lote inválido (segundo spec sin nombre): error y no se crea nada.
        let bad_req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"glass_batch_spawn_entities","arguments":{"entities":[{"name":"Valida"},{"components":["Transform"]}]}}}"#;
        let bad_resp = server.handle_json(bad_req).expect("batch inválido");
        assert!(bad_resp.contains("-32602"), "{}", bad_resp);
        assert_eq!(scene.lock().unwrap().entities.len(), 7);

        // (d) Inspección espacial: radio 2 ve la cercana y no la lejana;
        //     con limit 1 solo vuelve la más cercana.
        let insp_req = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"glass_inspect_spatial","arguments":{"center":[0.0,0.0,0.0],"radius":2.0}}}"#;
        let insp_resp = server.handle_json(insp_req).expect("inspect espacial");
        assert!(insp_resp.contains("Lote Cerca"), "{}", insp_resp);
        assert!(!insp_resp.contains("Lote Lejos"), "{}", insp_resp);
        let lim_req = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"glass_inspect_spatial","arguments":{"center":[0.0,0.0,0.0],"radius":10.0,"limit":1}}}"#;
        let lim_resp = server.handle_json(lim_req).expect("inspect con límite");
        // La respuesta JSON-RPC escapa las comillas del JSON interno:
        // se cuenta la palabra, no el par comilla-nombre.
        assert_eq!(lim_resp.matches("distance").count(), 1, "{}", lim_resp);

        // (e) Profiler: contadores honestos de la escena de prueba.
        let prof_req = r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"glass_profiler_snapshot"}}"#;
        let prof_resp = server.handle_json(prof_req).expect("profiler snapshot");
        assert!(prof_resp.contains("\\\"entities_total\\\": 7"), "{}", prof_resp);
        assert!(prof_resp.contains("\\\"physics_bodies\\\": 0"), "{}", prof_resp);
    }

    #[test]
    fn test_mcp_list_portals() {
        let scene = Arc::new(Mutex::new(SceneState::new()));
        let server = McpServer::new(scene.clone());

        // tools/list anuncia la tool del bloque Portales (17 -> 18).
        let list_resp = server
            .handle_json(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#)
            .expect("tools/list");
        assert!(list_resp.contains("glass_list_portals"), "{}", list_resp);

        // Dos superficies con el mismo pair (la escena base no trae portales).
        {
            let mut s = scene.lock().unwrap();
            let mut a = EntityData::new(50, "Portal A".to_string(), vec!["Transform".into()]);
            a.transform.position = [0.0, 1.1, 0.1];
            a.portal = Some(glass_core::portal::PortalData {
                pair: "sala".to_string(),
                width: 1.2,
                height: 2.2,
            });
            let mut b = EntityData::new(51, "Portal B".to_string(), vec!["Transform".into()]);
            b.transform.position = [0.0, 1.1, -0.1];
            b.portal = Some(glass_core::portal::PortalData {
                pair: "sala".to_string(),
                width: 1.2,
                height: 2.2,
            });
            s.entities.push(a);
            s.entities.push(b);
        }

        let resp = server
            .handle_json(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"glass_list_portals","arguments":{}}}"#)
            .expect("list portals");
        // La respuesta escapa las comillas del JSON interno (patrón de la casa).
        assert!(resp.contains("\\\"count\\\": 2"), "{}", resp);
        assert!(resp.contains("\\\"entity_id\\\": 50"), "{}", resp);
        assert!(resp.contains("\\\"entity_id\\\": 51"), "{}", resp);
        assert!(resp.contains("\\\"partner_id\\\": 51"), "{}", resp);
        assert!(resp.contains("\\\"partner_id\\\": 50"), "{}", resp);
        assert!(resp.contains("Portal A"), "{}", resp);
        assert!(resp.contains("Portal B"), "{}", resp);
    }

        #[test]
    fn test_mcp_physics_cast_y_contact_events() {
        let scene = Arc::new(Mutex::new(SceneState::new()));
        let server = McpServer::new(scene.clone());

        // tools/list anuncia las tools de la Fase 1 (20 -> 22).
        let list_resp = server
            .handle_json(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#)
            .expect("tools/list");
        assert!(list_resp.contains("glass_physics_cast"), "{}", list_resp);
        assert!(list_resp.contains("glass_list_contact_events"), "{}", list_resp);

        // Contactos: sin mundo vivo con cuerpos, la estructura llega vacía.
        let resp = server
            .handle_json(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"glass_list_contact_events","arguments":{}}}"#)
            .expect("contact events");
        assert!(resp.contains("\\\"active_pairs\\\""), "{}", resp);
        assert!(resp.contains("\\\"recent_events\\\""), "{}", resp);
        assert!(resp.contains("\\\"step_count\\\""), "{}", resp);

        // Cast sin mundo poblado: hit null, sin error de despacho.
        let resp = server
            .handle_json(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"glass_physics_cast","arguments":{"kind":"ray","origin":[0,5,0],"direction":[0,-1,0]}}}"#)
            .expect("physics cast");
        assert!(resp.contains("\\\"hit\\\": null"), "{}", resp);
    }

#[test]
    fn test_mcp_frame_timing_y_memory_stats() {
        let scene = Arc::new(Mutex::new(SceneState::new()));
        let server = McpServer::new(scene.clone());

        // tools/list anuncia las tools de la Fase 0 (18 -> 20).
        let list_resp = server
            .handle_json(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#)
            .expect("tools/list");
        assert!(list_resp.contains("glass_frame_timing"), "{}", list_resp);
        assert!(list_resp.contains("glass_memory_stats"), "{}", list_resp);

        // Sin motor en el proceso puede no haber muestras (otro test del
        // mismo binario sí registra): lo que no debe faltar es la estructura,
        // y si samples es 0, la nota que lo declara.
        let resp = server
            .handle_json(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"glass_frame_timing","arguments":{}}}"#)
            .expect("frame timing");
        assert!(resp.contains("\\\"samples\\\""), "{}", resp);
        assert!(resp.contains("\\\"gpu_samples\\\""), "{}", resp);
        if resp.contains("\\\"samples\\\": 0") {
            assert!(resp.contains("sin muestras"), "{}", resp);
        }

        // Memoria: process siempre presente; gpu puede ser snapshot o null.
        let resp = server
            .handle_json(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"glass_memory_stats","arguments":{}}}"#)
            .expect("memory stats");
        assert!(resp.contains("\\\"process\\\""), "{}", resp);
        assert!(resp.contains("\\\"rss_bytes\\\""), "{}", resp);
        assert!(resp.contains("\\\"gpu\\\""), "{}", resp);
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
