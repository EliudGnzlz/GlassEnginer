//! Paridad RB (paquete A): velocidad angular, torque, fuerza en un
//! punto, explosión, topes de velocidad, sueño y detectCollisions.
use glass_physics::PhysicsWorld;

const SUELO: u64 = 1;
const CAJA: u64 = 2;
const OTRA: u64 = 3;

fn suelo(w: &mut PhysicsWorld) {
    let h = w.create_rigid_body(SUELO, glass_physics::BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(h, [30.0, 0.5, 30.0], 0.0, 0.5);
}

fn caja_dinamica(w: &mut PhysicsWorld, entity: u64, pos: [f32; 3]) -> u64 {
    let h = w.create_rigid_body(entity, glass_physics::BodyType::Dynamic, pos, [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(h, [0.5, 0.5, 0.5], 0.5, 0.0);
    h
}

#[test]
fn velocidad_angular_se_fija_y_se_lee() {
    let mut w = PhysicsWorld::new();
    let h = caja_dinamica(&mut w, CAJA, [0.0, 5.0, 0.0]);
    assert!(w.set_angular_velocity(h, [0.0, 2.0, 0.0]));
    let v = w.get_angular_velocity(h).expect("velocidad angular");
    assert!((v[1] - 2.0).abs() < 1e-4, "angvel Y = {}", v[1]);
}

#[test]
fn fuerza_fuera_del_centro_produce_giro() {
    let mut w = PhysicsWorld::new();
    let h = caja_dinamica(&mut w, CAJA, [0.0, 5.0, 0.0]);
    // Impulso horizontal aplicado en el borde superior: debe girar.
    assert!(w.apply_impulse_at_point(h, [4.0, 0.0, 0.0], [0.0, 5.5, 0.0]));
    let v = w.get_angular_velocity(h).expect("angvel");
    let giro = v[0].abs() + v[1].abs() + v[2].abs();
    assert!(giro > 0.1, "el impulso descentrado debe producir giro (angvel={v:?})");
    // Y el mismo impulso en el centro NO gira.
    let h2 = caja_dinamica(&mut w, OTRA, [5.0, 5.0, 0.0]);
    assert!(w.apply_impulse_at_point(h2, [4.0, 0.0, 0.0], [5.0, 5.0, 0.0]));
    let v2 = w.get_angular_velocity(h2).expect("angvel2");
    assert!(v2.iter().all(|c| c.abs() < 1e-3), "en el centro no debe girar ({v2:?})");
}

#[test]
fn torque_impulsivo_hace_girar() {
    let mut w = PhysicsWorld::new();
    let h = caja_dinamica(&mut w, CAJA, [0.0, 5.0, 0.0]);
    assert!(w.apply_torque_impulse(h, [0.0, 3.0, 0.0]));
    let v = w.get_angular_velocity(h).expect("angvel");
    assert!(v[1] > 0.5, "torque en Y debe girar en Y ({v:?})");
}

#[test]
fn explosion_empuja_segun_distancia() {
    let mut w = PhysicsWorld::new();
    let cerca = caja_dinamica(&mut w, CAJA, [1.0, 5.0, 0.0]);
    let lejos = caja_dinamica(&mut w, OTRA, [8.0, 5.0, 0.0]);
    assert!(w.explosion_impulse(cerca, [0.0, 5.0, 0.0], 10.0, 20.0, 0.0));
    assert!(w.explosion_impulse(lejos, [0.0, 5.0, 0.0], 10.0, 20.0, 0.0));
    let vc = w.get_linear_velocity(cerca).expect("v cerca");
    let vl = w.get_linear_velocity(lejos).expect("v lejos");
    assert!(vc[0] > vl[0] && vl[0] > 0.0, "más cerca, más empuje ({vc:?} vs {vl:?})");
    // Fuera del radio: sin efecto.
    let fuera = caja_dinamica(&mut w, 9, [30.0, 5.0, 0.0]);
    assert!(!w.explosion_impulse(fuera, [0.0, 5.0, 0.0], 10.0, 20.0, 0.0));
}

#[test]
fn topes_de_velocidad_se_respetan() {
    let mut w = PhysicsWorld::new();
    suelo(&mut w);
    let h = caja_dinamica(&mut w, CAJA, [0.0, 5.0, 0.0]);
    w.set_velocity_caps(h, 1.0, 0.5);
    w.set_linear_velocity(h, [50.0, 0.0, 0.0]);
    w.set_angular_velocity(h, [0.0, 9.0, 0.0]);
    for _ in 0..10 {
        w.step(1.0 / 60.0);
    }
    let v = w.get_linear_velocity(h).expect("v");
    let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    assert!(mag <= 1.05, "la velocidad lineal queda capada ({mag})");
    let a = w.get_angular_velocity(h).expect("a");
    let maga = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    assert!(maga <= 0.55, "la velocidad angular queda capada ({maga})");
}

#[test]
fn dormir_y_despertar() {
    let mut w = PhysicsWorld::new();
    let h = caja_dinamica(&mut w, CAJA, [0.0, 5.0, 0.0]);
    assert!(w.sleep_body(h));
    assert_eq!(w.is_sleeping(h), Some(true));
    assert!(w.wake_body(h));
    assert_eq!(w.is_sleeping(h), Some(false));
    assert!(w.set_sleep_threshold(h, 0.005));
    assert!(w.set_sleep_threshold(h, 0.0)); // nunca duerme
}

#[test]
fn sin_deteccion_la_caja_cae_a_traves_del_suelo() {
    let mut w = PhysicsWorld::new();
    suelo(&mut w);
    let h = caja_dinamica(&mut w, CAJA, [0.0, 1.2, 0.0]);
    assert!(w.set_detect_collisions(h, false));
    for _ in 0..90 {
        w.step(1.0 / 60.0);
    }
    let t = w.get_transform(h).expect("transform");
    assert!(t.position[1] < 0.0, "sin detección debe atravesar el suelo (y={})", t.position[1]);
    // Al reactivar la detección, vuelve a apoyarse si cae de nuevo.
    assert!(w.set_detect_collisions(h, true));
}
