// Supuestos de API (AJUSTAR SOLO EN LOS HELPERS si difieren):
// - glass_physics::world::PhysicsWorld::new() con gravedad por defecto (0, -9.81, 0)
// - create_rigid_body(BodyType, [f32; 3]) -> BodyHandle (= u64)
// - attach_box_collider(handle, [f32; 3] half_extents, restitution, friction)

use glass_physics::types::BodyType;
use glass_physics::world::PhysicsWorld;

const DT: f32 = 1.0 / 60.0;

fn mundo() -> PhysicsWorld {
    PhysicsWorld::new()
}

fn cuerpo(w: &mut PhysicsWorld, tipo: BodyType, pos: [f32; 3]) -> u64 {
    w.create_rigid_body(0, tipo, pos, [0.0, 0.0, 0.0, 1.0])
}

fn caja(w: &mut PhysicsWorld, tipo: BodyType, pos: [f32; 3], half: [f32; 3], friction: f32) -> u64 {
    let h = cuerpo(w, tipo, pos);
    let _ = w.attach_box_collider(h, half, 0.0, friction);
    h
}

fn suelo(w: &mut PhysicsWorld) -> u64 {
    caja(w, BodyType::Static, [0.0, -0.5, 0.0], [50.0, 0.5, 50.0], 0.5)
}

fn pasos(w: &mut PhysicsWorld, n: usize) {
    for _ in 0..n {
        w.step(DT);
    }
}

fn pos(w: &PhysicsWorld, h: u64) -> [f32; 3] {
    w.get_transform(h).expect("cuerpo inexistente").position
}

fn vel(w: &PhysicsWorld, h: u64) -> [f32; 3] {
    w.get_linear_velocity(h).expect("cuerpo inexistente")
}

fn modulo(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

#[test]
fn la_masa_se_aplica_y_se_lee() {
    let mut w = mundo();
    let h = caja(&mut w, BodyType::Dynamic, [0.0, 5.0, 0.0], [0.5; 3], 0.5);
    assert!(w.set_gravity_scale(h, 0.0));
    pasos(&mut w, 1);
    let m0 = w.get_mass(h).expect("sin masa");
    assert!((m0 - 1.0).abs() < 0.01, "masa inicial {m0}");

    assert!(w.set_mass(h, 5.0));
    pasos(&mut w, 1);
    let m1 = w.get_mass(h).expect("sin masa");
    assert!((m1 - 5.0).abs() < 0.01, "masa tras set_mass {m1}");

    // Choque: la pesada (10) a 2 m/s contra la ligera (1) en reposo.
    let mut w = mundo();
    let pesada = caja(&mut w, BodyType::Dynamic, [-2.0, 5.0, 0.0], [0.5; 3], 0.0);
    let ligera = caja(&mut w, BodyType::Dynamic, [0.0, 5.0, 0.0], [0.5; 3], 0.0);
    for h in [pesada, ligera] {
        assert!(w.set_gravity_scale(h, 0.0));
        // Sin el damping histórico (1.5 fijo): mide el choque, no el freno.
        assert!(w.set_linear_damping(h, 0.0));
    }
    assert!(w.set_mass(pesada, 10.0));
    assert!(w.set_mass(ligera, 1.0));
    assert!(w.set_linear_velocity(pesada, [2.0, 0.0, 0.0]));
    pasos(&mut w, 90);

    let v_ligera = modulo(vel(&w, ligera));
    let v_pesada = modulo(vel(&w, pesada));
    assert!(v_ligera > 1.0, "ligera {v_ligera}");
    assert!(v_pesada > 0.5, "pesada {v_pesada}");
    assert!(v_ligera >= v_pesada, "ligera {v_ligera} < pesada {v_pesada}");
}

#[test]
fn el_damping_frena() {
    let mut w = mundo();
    let frenada = caja(&mut w, BodyType::Dynamic, [0.0, 5.0, -3.0], [0.5; 3], 0.5);
    let libre = caja(&mut w, BodyType::Dynamic, [0.0, 5.0, 3.0], [0.5; 3], 0.5);
    for h in [frenada, libre] {
        assert!(w.set_gravity_scale(h, 0.0));
        assert!(w.set_linear_velocity(h, [4.0, 0.0, 0.0]));
    }
    assert!(w.set_linear_damping(frenada, 3.0));
    assert!(w.set_linear_damping(libre, 0.0));
    assert!((w.get_linear_damping(frenada).unwrap() - 3.0).abs() < 1e-5);
    pasos(&mut w, 60);

    let v_frenada = modulo(vel(&w, frenada));
    let v_libre = modulo(vel(&w, libre));
    assert!((v_libre - 4.0).abs() < 0.05, "libre {v_libre}");
    assert!(v_frenada < v_libre, "frenada {v_frenada} >= libre {v_libre}");
}

#[test]
fn sin_gravedad_flota() {
    let mut w = mundo();
    suelo(&mut w);
    let flota = caja(&mut w, BodyType::Dynamic, [-2.0, 3.0, 0.0], [0.5; 3], 0.5);
    let cae = caja(&mut w, BodyType::Dynamic, [2.0, 3.0, 0.0], [0.5; 3], 0.5);
    assert!(w.set_gravity_scale(flota, 0.0));
    assert!(w.set_gravity_scale(cae, 1.0));
    assert!((w.get_gravity_scale(flota).unwrap()).abs() < 1e-6);
    pasos(&mut w, 120);

    let y_flota = pos(&w, flota)[1];
    let y_cae = pos(&w, cae)[1];
    assert!((y_flota - 3.0).abs() < 0.05, "flota y={y_flota}");
    assert!(y_cae < 2.5, "cae y={y_cae}");
}

#[test]
fn el_ccd_evita_el_tunel() {
    const PARED_X: f32 = 5.0;
    let mut w = mundo();
    caja(&mut w, BodyType::Static, [PARED_X, 0.0, 0.0], [0.05, 2.0, 2.0], 0.5);

    let proyectil = |w: &mut PhysicsWorld, z: f32| {
        let h = cuerpo(w, BodyType::Dynamic, [0.0, 0.0, z]);
        let _ = w.attach_sphere_collider(h, 0.05, 0.0, 0.5);
        assert!(w.set_gravity_scale(h, 0.0));
        assert!(w.set_linear_velocity(h, [80.0, 0.0, 0.0]));
        h
    };
    let sin_ccd = proyectil(&mut w, -1.0);
    let con_ccd = proyectil(&mut w, 1.0);
    assert!(w.set_ccd_enabled(sin_ccd, false));
    assert!(w.set_ccd_enabled(con_ccd, true));
    assert_eq!(w.ccd_enabled(con_ccd), Some(true));
    pasos(&mut w, 30);

    let x_sin = pos(&w, sin_ccd)[0];
    let x_con = pos(&w, con_ccd)[0];
    assert!(x_sin > PARED_X + 1.0, "sin CCD x={x_sin}");
    assert!(x_con < PARED_X + 0.3, "con CCD x={x_con}");
}

#[test]
fn mover_posicion_cinematica_empuja() {
    let mut w = mundo();
    suelo(&mut w);
    let inicio = [-1.1, 0.5, 0.0];
    let plataforma = caja(&mut w, BodyType::Kinematic, inicio, [0.5; 3], 0.5);
    let empujada = caja(&mut w, BodyType::Dynamic, [0.0, 0.5, 0.0], [0.5; 3], 0.5);
    pasos(&mut w, 1);
    let x0 = pos(&w, empujada)[0];

    for i in 1..=90 {
        let objetivo = [inicio[0] + 0.02 * i as f32, inicio[1], inicio[2]];
        assert!(w.move_position(plataforma, objetivo));
        w.step(DT);
    }

    let dx = pos(&w, empujada)[0] - x0;
    assert!(dx > 0.5, "desplazamiento {dx}");
}

#[test]
fn cambiar_el_tipo_a_kinematico_lo_detiene() {
    let mut w = mundo();
    let h = caja(&mut w, BodyType::Dynamic, [0.0, 20.0, 0.0], [0.5; 3], 0.5);
    pasos(&mut w, 30);
    let y_cayendo = pos(&w, h)[1];
    assert!(y_cayendo < 19.5, "no cae y={y_cayendo}");

    assert!(w.set_body_type(h, BodyType::Kinematic));
    let y_congelada = pos(&w, h)[1];
    pasos(&mut w, 30);
    let y_tras = pos(&w, h)[1];
    assert!((y_tras - y_congelada).abs() < 0.01, "kinemático se movió {y_congelada} -> {y_tras}");

    assert!(w.set_body_type(h, BodyType::Dynamic));
    pasos(&mut w, 30);
    let y_reanuda = pos(&w, h)[1];
    assert!(y_reanuda < y_tras - 0.1, "no reanuda la caída {y_tras} -> {y_reanuda}");
}
