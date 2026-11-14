//! Paridad RB (paquete B): centro de masa e inercia con Reset,
//! velocidad en un punto, Move/MoveRotation cinemáticos y SweepTest
//! del propio cuerpo.
use glass_physics::PhysicsWorld;

const SUELO: u64 = 1;
const CAJA: u64 = 2;
const OTRA: u64 = 3;
const MURO1: u64 = 4;
const MURO2: u64 = 5;

fn caja_dinamica(w: &mut PhysicsWorld, entity: u64, pos: [f32; 3]) -> u64 {
    let h = w.create_rigid_body(entity, glass_physics::BodyType::Dynamic, pos, [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(h, [0.5, 0.5, 0.5], 0.5, 0.0);
    h
}

fn cerca(a: [f32; 3], b: [f32; 3], tol: f32) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= tol)
}

fn cerca4(a: [f32; 4], b: [f32; 4], tol: f32) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= tol)
}

#[test]
fn centro_de_masa_se_fija_lee_y_restaura() {
    let mut w = PhysicsWorld::new();
    let h = caja_dinamica(&mut w, CAJA, [0.0, 5.0, 0.0]);
    // Por defecto el COM local es el centro geométrico.
    let com0 = w.get_center_of_mass(h).expect("com");
    assert!(cerca(com0, [0.0, 0.0, 0.0], 1e-4), "com inicial {com0:?}");
    // Moverlo y leerlo de vuelta, en local y en mundo.
    assert!(w.set_center_of_mass(h, [0.2, 0.0, 0.0]));
    let com = w.get_center_of_mass(h).expect("com movido");
    assert!(cerca(com, [0.2, 0.0, 0.0], 1e-3), "com movido {com:?}");
    let wcom = w.get_world_center_of_mass(h).expect("wcom");
    assert!(cerca(wcom, [0.2, 5.0, 0.0], 1e-3), "com mundial {wcom:?}");
    // Reset: vuelve al de los colliders.
    assert!(w.reset_center_of_mass(h));
    let com2 = w.get_center_of_mass(h).expect("com tras reset");
    assert!(cerca(com2, [0.0, 0.0, 0.0], 1e-3), "com tras reset {com2:?}");
}

#[test]
fn centro_de_masa_desplazado_cambia_el_giro() {
    let mut w = PhysicsWorld::new();
    let h = caja_dinamica(&mut w, CAJA, [0.0, 5.0, 0.0]);
    // Impulso vertical en el borde: con el COM en el centro gira poco
    // en X; con el COM desplazado en X el mismo impulso gira distinto.
    assert!(w.apply_impulse_at_point(h, [0.0, 4.0, 0.0], [0.25, 5.0, 0.0]));
    let a1 = w.get_angular_velocity(h).expect("angvel");
    let mut w2 = PhysicsWorld::new();
    let h2 = caja_dinamica(&mut w2, CAJA, [0.0, 5.0, 0.0]);
    assert!(w2.set_center_of_mass(h2, [0.2, 0.0, 0.0]));
    assert!(w2.apply_impulse_at_point(h2, [0.0, 4.0, 0.0], [0.25, 5.0, 0.0]));
    let a2 = w2.get_angular_velocity(h2).expect("angvel2");
    // El brazo de palanca efectivo cambia (0.25 vs 0.05): el giro en Z
    // debe ser claramente distinto.
    assert!(
        (a1[2] - a2[2]).abs() > 0.5,
        "el COM debe cambiar el giro ({a1:?} vs {a2:?})"
    );
}

#[test]
fn inercia_se_fija_lee_y_restaura() {
    let mut w = PhysicsWorld::new();
    let h = caja_dinamica(&mut w, CAJA, [0.0, 5.0, 0.0]);
    let (t0, _) = w.get_inertia_tensor(h).expect("inercia inicial");
    // La caja es de 1.0 de lado (half-extents 0.5) y masa 1.0:
    // I = m·(a²+b²)/12 = (1+1)/12 = 1/6 por eje.
    assert!(cerca(t0, [1.0 / 6.0, 1.0 / 6.0, 1.0 / 6.0], 1e-3), "inercia de caja 1.0 {t0:?}");
    assert!(w.set_inertia_tensor(h, [2.0, 2.0, 2.0], [0.0, 0.0, 0.0, 1.0]));
    let (t1, q1) = w.get_inertia_tensor(h).expect("inercia fijada");
    assert!(cerca(t1, [2.0, 2.0, 2.0], 1e-2), "inercia fijada {t1:?}");
    assert!(cerca4(q1, [0.0, 0.0, 0.0, 1.0], 1e-3), "frame identidad {q1:?}");
    assert!(w.reset_inertia_tensor(h));
    let (t2, _) = w.get_inertia_tensor(h).expect("inercia tras reset");
    assert!(cerca(t2, t0, 1e-3), "inercia tras reset {t2:?} vs {t0:?}");
}

#[test]
fn velocidad_en_un_punto_de_cuerpo_girando() {
    let mut w = PhysicsWorld::new();
    let h = caja_dinamica(&mut w, CAJA, [0.0, 5.0, 0.0]);
    assert!(w.set_angular_velocity(h, [0.0, 2.0, 0.0]));
    // Punto desplazado +X del COM: v = ω × r = (0,2,0) × (0.5,0,0) = (0,0,-1).
    let v = w.get_point_velocity(h, [0.5, 5.0, 0.0]).expect("v punto");
    assert!(cerca(v, [0.0, 0.0, -1.0], 1e-3), "v en punto {v:?}");
    // En el eje de giro no hay velocidad tangencial.
    let v0 = w.get_point_velocity(h, [0.0, 6.0, 0.0]).expect("v eje");
    assert!(cerca(v0, [0.0, 0.0, 0.0], 1e-3), "v en el eje {v0:?}");
    // La variante relativa (punto local, resultado local) coincide con
    // rotación identidad.
    let vr = w
        .get_relative_point_velocity(h, [0.5, 0.0, 0.0])
        .expect("v relativa");
    assert!(cerca(vr, [0.0, 0.0, -1.0], 1e-3), "v relativa {vr:?}");
}

#[test]
fn move_cinematico_empuja_dinamico() {
    let mut w = PhysicsWorld::new();
    let hs = w.create_rigid_body(SUELO, glass_physics::BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(hs, [30.0, 0.5, 30.0], 0.0, 0.5);
    // Empujador cinemático y caja dinámica apoyadas en el suelo.
    let hk = w.create_rigid_body(OTRA, glass_physics::BodyType::Kinematic, [0.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(hk, [1.0, 1.0, 1.0], 0.5, 0.0);
    let hd = caja_dinamica(&mut w, CAJA, [2.0, 0.5, 0.0]);
    // Un dinámico no se puede mover con Move: debe dar false.
    assert!(!w.move_kinematic(hd, [3.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]));
    // Avance gradual del cinemático: 60 pasos de 0.025 hasta x=1.5.
    for i in 1..=60 {
        let x = 0.025 * i as f32;
        assert!(w.move_kinematic(hk, [x, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]));
        w.step(1.0 / 60.0);
    }
    let tk = w.get_transform(hk).expect("transform cinemático");
    assert!((tk.position[0] - 1.5).abs() < 0.05, "cinemático en x={}", tk.position[0]);
    let td = w.get_transform(hd).expect("transform dinámico");
    assert!(td.position[0] > 2.2, "el dinámico debe haber sido empujado (x={})", td.position[0]);
}

#[test]
fn move_rotation_cinematico_gira() {
    let mut w = PhysicsWorld::new();
    let hk = w.create_rigid_body(OTRA, glass_physics::BodyType::Kinematic, [0.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(hk, [1.0, 1.0, 1.0], 0.5, 0.0);
    // 90° alrededor de Y: cuaternión (0, sin45, 0, cos45).
    let s = std::f32::consts::FRAC_1_SQRT_2;
    assert!(w.move_kinematic_rotation(hk, [0.0, s, 0.0, s]));
    w.step(1.0 / 60.0);
    let t = w.get_transform(hk).expect("transform");
    assert!((t.rotation[1].abs() - s).abs() < 0.02, "rotación Y {t:?}");
    assert!((t.rotation[3].abs() - s).abs() < 0.02, "rotación W {t:?}");
}

#[test]
fn masa_despues_de_override_conserva_com_y_escala_inercia() {
    let mut w = PhysicsWorld::new();
    let h = caja_dinamica(&mut w, CAJA, [0.0, 5.0, 0.0]);
    assert!(w.set_center_of_mass(h, [0.2, 0.0, 0.0]));
    assert!(w.set_mass(h, 2.0));
    // El COM fijado sobrevive al cambio de masa...
    let com = w.get_center_of_mass(h).expect("com tras set_mass");
    assert!(cerca(com, [0.2, 0.0, 0.0], 1e-3), "com tras set_mass {com:?}");
    // ...la masa total es la nueva...
    let m = w.get_mass(h).expect("masa");
    assert!((m - 2.0).abs() < 1e-3, "masa {m}");
    // ...y la inercia NO fijada escala con la masa (natural de masa 2).
    let (t, _) = w.get_inertia_tensor(h).expect("inercia tras set_mass");
    assert!(cerca(t, [1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0], 1e-3), "inercia {t:?}");
}

#[test]
fn sweep_detecta_la_pared_y_ordena() {
    let mut w = PhysicsWorld::new();
    let h = caja_dinamica(&mut w, CAJA, [0.0, 0.5, 0.0]);
    let m1 = w.create_rigid_body(MURO1, glass_physics::BodyType::Static, [5.0, 1.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(m1, [0.5, 3.0, 3.0], 0.0, 0.5);
    let m2 = w.create_rigid_body(MURO2, glass_physics::BodyType::Static, [8.0, 1.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(m2, [0.5, 3.0, 3.0], 0.0, 0.5);
    // La caja barre +X: su cara está en 0.5 (half-extents), el muro1 en 4.5.
    let hit = w.sweep_body(h, [1.0, 0.0, 0.0], 10.0).expect("debe haber impacto");
    assert_eq!(hit.entity_id, MURO1, "el primer impacto es el muro1");
    assert!((hit.distance - 4.0).abs() < 0.05, "distancia {}", hit.distance);
    assert!(hit.normal[0] < -0.9, "normal hacia -X {:?}", hit.normal);
    // SweepTestAll: los dos muros, ordenados por distancia.
    let todos = w.sweep_body_all(h, [1.0, 0.0, 0.0], 10.0);
    assert_eq!(todos.len(), 2, "dos impactos {todos:?}");
    assert_eq!(todos[0].entity_id, MURO1);
    assert_eq!(todos[1].entity_id, MURO2);
    assert!(todos[0].distance < todos[1].distance);
    assert!((todos[1].distance - 7.0).abs() < 0.05, "segunda distancia {}", todos[1].distance);
    // Sin obstáculo en esa dirección: nada.
    assert!(w.sweep_body(h, [0.0, 1.0, 0.0], 10.0).is_none());
    assert!(w.sweep_body_all(h, [0.0, 1.0, 0.0], 10.0).is_empty());
}
