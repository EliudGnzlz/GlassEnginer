use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use glass_scripting::{register_scene_bridge, SceneBridge, ScriptHost, TransformFfi};

#[derive(Default, Clone)]
struct TestBridge {
    transforms: Arc<Mutex<HashMap<u64, TransformFfi>>>,
}

impl SceneBridge for TestBridge {
    fn get_transform(&self, entity_id: u64) -> Option<TransformFfi> {
        let lock = self.transforms.lock().unwrap();
        lock.get(&entity_id).copied()
    }

    fn set_transform(&self, entity_id: u64, transform: TransformFfi) -> bool {
        let mut lock = self.transforms.lock().unwrap();
        lock.insert(entity_id, transform);
        true
    }
}

#[test]
fn test_coreclr_host_and_scripts() {
    let _ = env_logger::builder().is_test(true).try_init();

    // 1. Setup scene bridge with test entities
    let bridge = TestBridge::default();
    let initial_transform_42 = TransformFfi {
        position: [0.0, 1.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0], // Identity quaternion
        scale: [1.0, 1.0, 1.0],
    };
    let initial_transform_43 = TransformFfi {
        position: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0, 1.0, 1.0],
    };

    bridge.transforms.lock().unwrap().insert(42, initial_transform_42);
    bridge.transforms.lock().unwrap().insert(43, initial_transform_43);

    register_scene_bridge(Box::new(bridge.clone()));

    // 2. Locate managed DLL directory
    let assembly_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../managed/GlassEngine.Core/bin/Debug/net8.0");

    assert!(assembly_dir.exists(), "Directorio de ensamblado C# debe existir: {}", assembly_dir.display());

    // 3. Initialize ScriptHost
    let mut host = ScriptHost::new();
    let init_res = host.initialize(&assembly_dir);
    assert!(init_res.is_ok(), "CoreCLR debe inicializar correctamente: {:?}", init_res.err());
    assert!(host.is_initialized);

    // 4. Instantiate Rotator on entity 42
    let rotator_handle = host.create_instance(42, "GlassEngine.Core.Scripts.Rotator")
        .expect("Debe instanciar Rotator");
    assert!(rotator_handle > 0);

    // 5. Instantiate PlayerMovement on entity 43
    let mover_handle = host.create_instance(43, "GlassEngine.Core.Scripts.PlayerMovement")
        .expect("Debe instanciar PlayerMovement");
    assert!(mover_handle > 0);
    assert_eq!(host.active_instance_count(), 2);

    // 6. Step several frames (e.g. 5 frames of 16.6ms)
    for _ in 0..5 {
        host.update_all(0.016667);
    }

    // 7. Verify entity 42 was rotated by C# script
    let updated_t42 = bridge.get_transform(42).expect("Entidad 42 debe existir");
    println!("Entidad 42 Rotación tras Update C#: {:?}", updated_t42.rotation);
    // After rotation around Y, rotation.y or rotation.w should differ from initial [0, 0, 0, 1]
    assert_ne!(updated_t42.rotation, initial_transform_42.rotation, "Rotator debió modificar la rotación");

    // 8. Verify entity 43 was moved by C# script
    let updated_t43 = bridge.get_transform(43).expect("Entidad 43 debe existir");
    println!("Entidad 43 Posición tras Update C#: {:?}", updated_t43.position);
    assert_ne!(updated_t43.position, initial_transform_43.position, "PlayerMovement debió mover la posición");

    // 9. Cleanup
    host.destroy_instance(rotator_handle);
    host.destroy_instance(mover_handle);
    assert_eq!(host.active_instance_count(), 0);
}
