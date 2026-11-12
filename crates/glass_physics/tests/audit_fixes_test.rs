//! Tests de los arreglos de la auditoría Opus (Fases 1 y 1.5):
//! eventos agregados por par de entidades, Exit al despawnear,
//! acumulador sin deuda, sensores fuera de las consultas y eventos
//! de sensores frente a cuerpos cinemáticos.

use glass_physics::{BodyType, ColliderConfig, ColliderShape, ContactKind, PhysicsWorld};

const DT: f32 = 1.0 / 60.0;

fn suelo(w: &mut PhysicsWorld) {
    let b = w.create_rigid_body(1, BodyType::Static, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(b, [10.0, 0.5, 10.0], 0.0, 0.5);
}

#[test]
fn eventos_agregados_por_par_y_exit_al_despawnear() {
    let mut w = PhysicsWorld::new();
    suelo(&mut w);
    // Cuerpo compuesto: DOS colliders que tocan el suelo a la vez.
    let c = w.create_rigid_body(2, BodyType::Dynamic, [0.0, 1.05, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let cfg = ColliderConfig::default();
    w.attach_collider_with_config_at(c, ColliderShape::Cuboid { half_extents: [0.5, 0.5, 0.5] }, 0.0, 0.5, cfg, [-0.6, 0.0, 0.0]);
    w.attach_collider_with_config_at(c, ColliderShape::Cuboid { half_extents: [0.5, 0.5, 0.5] }, 0.0, 0.5, cfg, [0.6, 0.0, 0.0]);
    for _ in 0..30 {
        w.step(DT);
    }
    let eventos = w.drain_contact_events();
    let enters: Vec<_> = eventos
        .iter()
        .filter(|e| e.kind == ContactKind::Enter && e.a == 1 && e.b == 2)
        .collect();
    assert_eq!(enters.len(), 1, "un solo Enter por par de entidades: {eventos:?}");
    // Despawn en contacto: debe llegar exactamente un Exit del par.
    assert!(w.remove_rigid_body(c));
    for _ in 0..5 {
        w.step(DT);
    }
    let eventos = w.drain_contact_events();
    let exits: Vec<_> = eventos
        .iter()
        .filter(|e| e.kind == ContactKind::Exit && e.a == 1 && e.b == 2)
        .collect();
    assert_eq!(exits.len(), 1, "un solo Exit al despawnear en contacto: {eventos:?}");
}

#[test]
fn acumulador_descarta_el_exceso() {
    let mut w = PhysicsWorld::new();
    w.step(10.0); // tirón enorme: tope de 4 substeps y deuda descartada
    assert_eq!(w.step_count(), 4, "el tope es 4 substeps por llamada");
    w.step(DT);
    assert_eq!(w.step_count(), 5, "sin deuda arrastrada: un paso normal");
    w.step(f32::NAN);
    assert_eq!(w.step_count(), 5, "un dt NaN no envenena el acumulador");
    w.step(DT);
    assert_eq!(w.step_count(), 6);
}

#[test]
fn las_consultas_excluyen_sensores() {
    let mut w = PhysicsWorld::new();
    suelo(&mut w);
    // Sensor grande entre el origen del rayo y el suelo.
    let s = w.create_rigid_body(3, BodyType::Static, [0.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let cfg = ColliderConfig { is_trigger: true, ..Default::default() };
    w.attach_collider_with_config_at(s, ColliderShape::Cuboid { half_extents: [2.0, 2.0, 2.0] }, 0.0, 0.0, cfg, [0.0, 0.0, 0.0]);
    w.step(DT);
    let hit = w.cast_ray_masked([0.0, 8.0, 0.0], [0.0, -1.0, 0.0], 20.0, u32::MAX, 0);
    let hit = hit.expect("el rayo debe llegar al suelo atravesando el sensor");
    assert_eq!(hit.entity_id, 1, "el impacto es el suelo, no el sensor: {hit:?}");
}

#[test]
fn sensor_estatico_emite_eventos_frente_a_cinematico() {
    let mut w = PhysicsWorld::new();
    let s = w.create_rigid_body(4, BodyType::Static, [0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let cfg = ColliderConfig { is_trigger: true, ..Default::default() };
    w.attach_collider_with_config_at(s, ColliderShape::Cuboid { half_extents: [1.0, 1.0, 1.0] }, 0.0, 0.0, cfg, [0.0, 0.0, 0.0]);
    let k = w.create_rigid_body(5, BodyType::Kinematic, [0.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(k, [0.5, 0.5, 0.5], 0.0, 0.0);
    // El cinemático se teletransporta dentro del sensor.
    w.set_transform(k, [0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    for _ in 0..5 {
        w.step(DT);
    }
    let eventos = w.drain_contact_events();
    assert!(
        eventos.iter().any(|e| e.kind == ContactKind::Enter && e.is_trigger && e.a == 4 && e.b == 5),
        "Enter de sensor frente a cinemático: {eventos:?}"
    );
}
