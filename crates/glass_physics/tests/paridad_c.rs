//! Regresión del paquete C de paridad RB/CC: detectCollisions (fantasma),
//! enabled, stepOffset/slopeLimit mutables, overlap recovery, includeLayers
//! y bounds de la cápsula del CharacterController.
use glass_physics::{BodyType, CharacterConfig, ColliderConfig, PhysicsWorld};

fn cfg(step: f32, slope: f32) -> CharacterConfig {
    CharacterConfig {
        radius: 0.35,
        height: 1.8,
        skin: 0.02,
        step_offset: step,
        slope_limit_deg: slope,
        snap: 0.3,
    }
}

/// Suelo plano + muro vertical en x [6.0, 6.9], altura 3 m.
fn mundo_con_muro() -> PhysicsWorld {
    let mut w = PhysicsWorld::new();
    let suelo = w.create_rigid_body(1, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(suelo, [30.0, 0.5, 10.0], 0.0, 0.5);
    let muro = w.create_rigid_body(2, BodyType::Static, [6.45, 1.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(muro, [0.45, 1.5, 1.5], 0.0, 0.5);
    w
}

/// Como `mundo_con_muro`, pero el muro vive en la capa 2.
fn mundo_con_muro_capa2() -> PhysicsWorld {
    let mut w = PhysicsWorld::new();
    let suelo = w.create_rigid_body(1, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(suelo, [30.0, 0.5, 10.0], 0.0, 0.5);
    let muro = w.create_rigid_body(2, BodyType::Static, [6.45, 1.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider_with_config(
        muro,
        [0.45, 1.5, 1.5],
        0.0,
        0.5,
        ColliderConfig {
            layer: 2,
            mask: u32::MAX,
            is_trigger: false,
        },
    );
    w
}

/// Avanza N frames a 1,6 m/s con presión de gravedad; devuelve la pos final.
fn avanza(w: &mut PhysicsWorld, frames: usize) -> [f32; 3] {
    let mut pos = w.character_info(15).expect("el personaje existe").0;
    for _ in 0..frames {
        let (a, _, _) = w
            .move_character(15, [1.6 / 60.0, -1.0 / 60.0, 0.0], 1.0 / 60.0)
            .expect("el personaje existe");
        pos = [pos[0] + a[0], pos[1] + a[1], pos[2] + a[2]];
    }
    pos
}

#[test]
fn fantasma_atraviesa_la_pared() {
    let mut w = mundo_con_muro();
    assert!(w.create_character(15, [5.0, 0.91, 0.0], cfg(0.4, 45.0)));
    assert!(w.set_character_detect_collisions(15, false));
    let pos = avanza(&mut w, 240);
    assert!(pos[0] > 6.9, "en modo fantasma debe atravesar el muro; x {}", pos[0]);
}

#[test]
fn con_colisiones_el_muro_bloquea() {
    let mut w = mundo_con_muro();
    assert!(w.create_character(15, [5.0, 0.91, 0.0], cfg(0.4, 45.0)));
    let pos = avanza(&mut w, 240);
    assert!(pos[0] < 5.8, "con colisiones el muro debe bloquear; x {}", pos[0]);
}

#[test]
fn deshabilitado_no_se_mueve() {
    let mut w = mundo_con_muro();
    assert!(w.create_character(15, [5.0, 0.91, 0.0], cfg(0.4, 45.0)));
    assert!(w.set_character_enabled(15, false));
    let inicio = w.character_info(15).unwrap().0;
    for _ in 0..10 {
        let (a, _, _) = w
            .move_character(15, [1.6 / 60.0, -1.0 / 60.0, 0.0], 1.0 / 60.0)
            .unwrap();
        assert_eq!(a, [0.0, 0.0, 0.0], "deshabilitado no debe aplicar nada");
    }
    let pos = w.character_info(15).unwrap().0;
    assert_eq!(pos, inicio, "deshabilitado no debe moverse");
    // Y al reactivarlo vuelve a andar.
    assert!(w.set_character_enabled(15, true));
    let (a, _, _) = w
        .move_character(15, [1.6 / 60.0, -1.0 / 60.0, 0.0], 1.0 / 60.0)
        .unwrap();
    assert!(a[0] > 0.0, "reactivado debe moverse; aplicado {:?}", a);
}

#[test]
fn step_offset_mutable_surte_efecto() {
    // Peldaño de 0,35 m (misma geometría que character_steps).
    let mut w = PhysicsWorld::new();
    let suelo = w.create_rigid_body(1, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(suelo, [30.0, 0.5, 10.0], 0.0, 0.5);
    let paso = w.create_rigid_body(2, BodyType::Static, [6.45, 0.175, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(paso, [0.45, 0.175, 1.5], 0.0, 0.5);
    assert!(w.create_character(15, [5.0, 0.91, 0.0], cfg(0.05, 45.0)));
    let pos = avanza(&mut w, 120);
    assert!(pos[0] < 5.8, "con stepOffset 0,05 debe bloquearse; x {}", pos[0]);
    // Subir el tope en caliente y seguir andando: ahora lo cruza.
    assert!(w.set_character_step_offset(15, 0.4));
    assert!((w.get_character_step_offset(15).unwrap() - 0.4).abs() < 1e-6);
    let pos = avanza(&mut w, 240);
    assert!(pos[0] > 10.0, "con stepOffset 0,4 debe cruzar el peldaño; x {}", pos[0]);
}

#[test]
fn slope_limit_mutable_surte_efecto() {
    // Rampa de 30° (misma geometría que character_steps).
    let mut w = PhysicsWorld::new();
    let suelo = w.create_rigid_body(1, BodyType::Static, [0.0, -0.5, -2.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(suelo, [30.0, 0.5, 35.0], 0.0, 0.5);
    let s = (15.0f32).to_radians().sin();
    let c = (15.0f32).to_radians().cos();
    let rampa = w.create_rigid_body(2, BodyType::Static, [0.0, 1.35, -13.5], [s, 0.0, 0.0, c]);
    w.attach_box_collider(rampa, [2.0, 0.15, 3.5], 0.0, 0.5);
    assert!(w.create_character(15, [0.0, 0.91, -8.6], cfg(0.4, 20.0)));
    let mut pos = [0.0f32, 0.91, -8.6];
    for _ in 0..210 {
        let (a, _, _) = w
            .move_character(15, [0.0, -1.0 / 60.0, -1.6 / 60.0], 1.0 / 60.0)
            .unwrap();
        pos = [pos[0] + a[0], pos[1] + a[1], pos[2] + a[2]];
    }
    assert!(pos[1] < 1.35, "con slopeLimit 20 no debe subir; y {}", pos[1]);
    assert!(w.set_character_slope_limit(15, 45.0));
    assert!((w.get_character_slope_limit(15).unwrap() - 45.0).abs() < 1e-6);
    for _ in 0..210 {
        let (a, _, _) = w
            .move_character(15, [0.0, -1.0 / 60.0, -1.6 / 60.0], 1.0 / 60.0)
            .unwrap();
        pos = [pos[0] + a[0], pos[1] + a[1], pos[2] + a[2]];
    }
    assert!(pos[1] > 1.8, "con slopeLimit 45 debe subir la rampa; y {}", pos[1]);
}

#[test]
fn overlap_recovery_expulsa_de_la_pared() {
    // El personaje nace DENTRO del muro (x=6,45, centro del muro).
    let mut w = mundo_con_muro();
    assert!(w.create_character(15, [6.45, 0.91, 0.0], cfg(0.4, 45.0)));
    let (a, _, _) = w
        .move_character(15, [0.01, -0.01, 0.0], 1.0 / 60.0)
        .unwrap();
    let _ = a;
    let pos = w.character_info(15).unwrap().0;
    // La cápsula (radio 0,35) debe quedar FUERA del muro x [6,0, 6,9]:
    // x + 0,35 <= 6,0 (pegada a la cara izquierda) o x - 0,35 >= 6,9.
    assert!(
        pos[0] < 5.7 || pos[0] > 7.2,
        "con overlap recovery debe salir del muro; x {}",
        pos[0]
    );
}

#[test]
fn sin_overlap_recovery_queda_atrapado() {
    let mut w = mundo_con_muro();
    assert!(w.create_character(15, [6.45, 0.91, 0.0], cfg(0.4, 45.0)));
    assert!(w.set_character_overlap_recovery(15, false));
    for _ in 0..10 {
        let _ = w.move_character(15, [0.01, -0.01, 0.0], 1.0 / 60.0);
    }
    let pos = w.character_info(15).unwrap().0;
    // Sigue solapando el muro x [6,0, 6,9] con la cápsula (radio 0,35).
    assert!(
        (5.7..7.2).contains(&pos[0]),
        "sin overlap recovery debe seguir dentro del muro; x {}",
        pos[0]
    );
}

#[test]
fn bounds_caja_de_la_capsula() {
    let mut w = PhysicsWorld::new();
    assert!(w.create_character(15, [1.0, 0.91, 2.0], cfg(0.4, 45.0)));
    let (min, max) = w.character_bounds(15).expect("bounds");
    // radio 0,35; media altura total 0,9 (half_height 0,55 + radio 0,35).
    assert!((min[0] - 0.65).abs() < 1e-6, "min.x {:?}", min);
    assert!((min[1] - 0.01).abs() < 1e-6, "min.y {:?}", min);
    assert!((min[2] - 1.65).abs() < 1e-6, "min.z {:?}", min);
    assert!((max[0] - 1.35).abs() < 1e-6, "max.x {:?}", max);
    assert!((max[1] - 1.81).abs() < 1e-6, "max.y {:?}", max);
    assert!((max[2] - 2.35).abs() < 1e-6, "max.z {:?}", max);
}

#[test]
fn include_layers_filtra_la_capa_del_muro() {
    let mut w = mundo_con_muro_capa2();
    assert!(w.create_character(15, [5.0, 0.91, 0.0], cfg(0.4, 45.0)));
    // Sin la capa 2 en la máscara: el muro no existe para las sondas.
    assert!(w.set_character_include_layers(15, !(1u32 << 2)));
    let pos = avanza(&mut w, 240);
    assert!(pos[0] > 6.9, "sin capa 2 debe atravesar el muro; x {}", pos[0]);

    // Con todas las capas: el muro bloquea.
    let mut w = mundo_con_muro_capa2();
    assert!(w.create_character(15, [5.0, 0.91, 0.0], cfg(0.4, 45.0)));
    assert!(w.set_character_include_layers(15, u32::MAX));
    let pos = avanza(&mut w, 240);
    assert!(pos[0] < 5.8, "con capa 2 debe bloquear; x {}", pos[0]);
}

#[test]
fn setters_getters_redondos() {
    let mut w = mundo_con_muro();
    assert!(w.create_character(15, [5.0, 0.91, 0.0], cfg(0.4, 45.0)));
    assert!(w.set_character_detect_collisions(15, false));
    assert!(!w.get_character_detect_collisions(15).unwrap());
    assert!(w.set_character_overlap_recovery(15, false));
    assert!(!w.get_character_overlap_recovery(15).unwrap());
    assert!(w.set_character_enabled(15, false));
    assert!(!w.get_character_enabled(15).unwrap());
    assert!(w.set_character_include_layers(15, 0b101));
    assert_eq!(w.get_character_include_layers(15).unwrap(), 0b101);
    assert!(!w.set_character_step_offset(15, -1.0), "step negativo no válido");
    assert!(!w.set_character_detect_collisions(99, true), "entidad inexistente");
}
