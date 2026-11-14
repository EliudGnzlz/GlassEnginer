//! La cápsula del CharacterController es invisible para las
//! consultas del mundo y para el solver: el personaje se resuelve
//! con sus propias sondas. Sin esto, un raycast desde la cámara del
//! jugador (que está DENTRO de su cápsula) impacta en sí mismo a
//! distancia 0 y las armas hitscan/proyectil nunca llegan al mundo
//! (cazado en la demo FPS, 2026-11-14).
use glass_physics::character::CharacterConfig;
use glass_physics::PhysicsWorld;

const SUELO: u64 = 1;
const PARED: u64 = 2;
const PJ: u64 = 10;
const BALA: u64 = 20;

fn caja_estatica(w: &mut PhysicsWorld, entity: u64, centro: [f32; 3], semiejes: [f32; 3]) {
    let h = w.create_rigid_body(
        entity,
        glass_physics::BodyType::Static,
        centro,
        [0.0, 0.0, 0.0, 1.0],
    );
    w.attach_box_collider(h, semiejes, 0.0, 0.5);
}

fn config() -> CharacterConfig {
    CharacterConfig {
        radius: 0.35,
        height: 1.8,
        skin: 0.02,
        step_offset: 0.4,
        slope_limit_deg: 45.0,
        snap: 0.3,
    }
}

#[test]
fn el_raycast_desde_el_ojo_no_se_impacta_a_si_mismo() {
    let mut w = PhysicsWorld::new();
    caja_estatica(&mut w, SUELO, [0.0, -0.5, 0.0], [20.0, 0.5, 20.0]);
    caja_estatica(&mut w, PARED, [0.0, 1.5, -14.0], [5.0, 1.5, 0.35]);
    w.create_character(PJ, [0.0, 0.92, 6.0], config());

    // Ojo del jugador: centro de la cápsula + 0,65 Y + 0,25 adelante.
    // Está DENTRO del volumen de la cápsula (radio 0,35).
    let hit = w
        .cast_ray([0.0, 1.57, 5.75], [0.0, -0.035, -1.0], 130.0, 0)
        .expect("el rayo debe llegar a la pared");
    assert_eq!(hit.entity_id, PARED, "el primer impacto debe ser la pared, no el personaje");
    assert!(hit.distance > 15.0, "distancia plausible: {}", hit.distance);
}

#[test]
fn un_dinamico_atraviesa_la_capsula_sin_chocar() {
    let mut w = PhysicsWorld::new();
    caja_estatica(&mut w, SUELO, [0.0, -0.5, 0.0], [20.0, 0.5, 20.0]);
    w.create_character(PJ, [0.0, 0.92, 0.0], config());

    // Bala dinámica lanzada contra el pecho del personaje: con la
    // cápsula invisible al solver debe seguir de largo y caer al
    // suelo tras él, sin quedarse pegada ni desviarse.
    let bala = w.create_rigid_body(
        BALA,
        glass_physics::BodyType::Dynamic,
        [0.0, 1.2, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    );
    w.attach_sphere_collider(bala, 0.06, 0.1, 0.0);
    w.set_linear_velocity(bala, [0.0, 0.0, -30.0]);

    let mut z_min = f32::MAX;
    for _ in 0..90 {
        w.step(1.0 / 60.0);
        if let Some(t) = w.get_transform(bala) {
            z_min = z_min.min(t.position[2]);
        }
    }
    assert!(z_min < -1.0, "la bala debió pasar de largo del personaje (z_min={z_min})");
}
