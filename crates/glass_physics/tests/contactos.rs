// Supuestos de API a verificar: PhysicsWorld::new(), BodyType::{Dynamic, Fixed},
// attach_box_collider -> ColliderHandle, set_collider_sensor, destroy_rigid_body.
use glass_physics::types::{ContactEvent, ContactKind};
use glass_physics::{BodyType, PhysicsWorld};

const DT: f32 = 1.0 / 60.0;
const CAJA: u64 = 1; // entidad menor => `a` del par
const SUELO: u64 = 2;

fn suelo(w: &mut PhysicsWorld) -> glass_physics::BodyHandle {
    let h = w.create_rigid_body(SUELO, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(h, [10.0, 0.5, 10.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.5, bounciness: 0.0, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
    h
}

fn caja(w: &mut PhysicsWorld, y: f32) -> glass_physics::BodyHandle {
    let h = w.create_rigid_body(CAJA, BodyType::Dynamic, [0.0, y, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(h, [0.5, 0.5, 0.5], &glass_core::PhysicsMaterial { dynamic_friction: 0.5, bounciness: 0.0, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
    h
}

fn primer(w: &mut PhysicsWorld, max_steps: usize, f: impl Fn(&ContactEvent) -> bool) -> Option<ContactEvent> {
    for _ in 0..max_steps {
        w.step(DT);
        if let Some(e) = w.drain_contact_events().into_iter().find(|e| f(e)) {
            return Some(e);
        }
    }
    None
}

#[test]
fn el_impacto_trae_punto_normal_e_impulso() {
    let mut w = PhysicsWorld::new();
    suelo(&mut w);
    caja(&mut w, 3.0);

    let ev = primer(&mut w, 240, |e| matches!(e.kind, ContactKind::Enter) && !e.is_trigger)
        .expect("no llegó Started no-trigger");
    assert_eq!((ev.a, ev.b), (CAJA, SUELO));
    let c = ev.contact.expect("Started sin contact");

    assert!(c.normal[1].abs() > 0.9, "normal no vertical: {:?}", c.normal);
    assert!(c.normal[0].abs() < 0.1 && c.normal[2].abs() < 0.1, "normal: {:?}", c.normal);
    assert!(c.point[1].abs() < 0.3, "punto.y: {}", c.point[1]);
    assert!(c.impulse >= 0.0, "impulso: {}", c.impulse);
    // En este arnés par.0 es la caja: su velocidad relativa al
    // suelo es su propia velocidad de caída.
    assert!(c.relative_velocity[1] < -1.0, "rv.y: {}", c.relative_velocity[1]);
}

#[test]
fn el_trigger_no_trae_contacto() {
    let mut w = PhysicsWorld::new();
    let s = w.create_rigid_body(SUELO, BodyType::Static, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let sensor = glass_physics::types::ColliderConfig { is_trigger: true, ..Default::default() };
    w.attach_box_collider_with_config(s, [2.0, 0.5, 2.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.5, bounciness: 0.0, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "", sensor);
    caja(&mut w, 3.0);

    let ev = primer(&mut w, 240, |e| matches!(e.kind, ContactKind::Enter))
        .expect("no llegó Started del sensor");
    assert!(ev.is_trigger);
    assert!(ev.contact.is_none());
}

#[test]
fn sin_contacto_en_el_stopped() {
    let mut w = PhysicsWorld::new();
    let s = suelo(&mut w);
    caja(&mut w, 3.0);

    primer(&mut w, 240, |e| matches!(e.kind, ContactKind::Enter) && !e.is_trigger)
        .expect("no llegó Started");
    w.remove_rigid_body(s);

    let ev = primer(&mut w, 10, |e| matches!(e.kind, ContactKind::Exit))
        .expect("no llegó Stopped");
    assert!(ev.contact.is_none());
}

#[test]
fn la_normal_no_depende_del_orden_de_creacion() {
    // La caja se crea ANTES que el suelo: la normal entregada a la
    // caja (par b del evento) debe seguir siendo +Y tras el espejo.
    let mut w = PhysicsWorld::new();
    let caja = w.create_rigid_body(2, BodyType::Dynamic, [0.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(caja, [0.5, 0.5, 0.5], &glass_core::PhysicsMaterial { dynamic_friction: 0.5, bounciness: 0.0, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
    let suelo = w.create_rigid_body(1, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(suelo, [10.0, 0.5, 10.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.5, bounciness: 0.0, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
    let mut contacto = None;
    for _ in 0..240 {
        w.step(DT);
        for e in w.drain_contact_events() {
            if e.contact.is_some() {
                contacto = e.contact;
            }
        }
        if contacto.is_some() {
            break;
        }
    }
    let c = contacto.expect("hubo contacto");
    // El evento llega como (a=suelo, b=caja): la normal de `a` es la
    // del suelo (-Y) y el espejo da a la caja +Y.
    assert!(c.normal[1] < -0.9, "normal del suelo {c:?}");
    assert!(c.impulse > 1.0, "impulso estimado del impacto: {c:?}");
}

#[test]
fn el_impacto_trae_puntos_individuales() {
    let mut w = PhysicsWorld::new();
    suelo(&mut w);
    caja(&mut w, 3.0);

    let ev = primer(&mut w, 240, |e| matches!(e.kind, ContactKind::Enter) && !e.is_trigger)
        .expect("no llegó Started no-trigger");
    let c = ev.contact.expect("Started sin contact");

    // La cara superior del suelo está en y = 0 (centro -0,5, semialtura 0,5).
    assert!(c.point_count >= 1, "point_count = {}", c.point_count);
    assert!(
        (c.point_count as usize) <= glass_physics::MAX_PUNTOS_CONTACTO,
        "point_count = {}",
        c.point_count
    );
    for (i, p) in c.points[..c.point_count as usize].iter().enumerate() {
        let len = (p.normal[0].powi(2) + p.normal[1].powi(2) + p.normal[2].powi(2)).sqrt();
        assert!((len - 1.0).abs() <= 0.01, "punto {i}: |normal| = {len}");
        assert!(p.separation <= 0.05, "punto {i}: separation = {}", p.separation);
        assert!(p.point[1].abs() <= 0.15, "punto {i}: y = {} (cara en 0)", p.point[1]);
        // La normal de cada punto coincide con la agregada del par.
        assert!(
            (p.normal[1] - c.normal[1]).abs() <= 0.01,
            "punto {i}: normal.y {} vs agregada {}",
            p.normal[1],
            c.normal[1]
        );
    }
}
