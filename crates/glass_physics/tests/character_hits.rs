use glass_physics::character::{
    CharacterConfig, CHARACTER_FLAG_ABOVE, CHARACTER_FLAG_BELOW, CHARACTER_FLAG_SIDES,
};
use glass_physics::PhysicsWorld;

const DT: f32 = 1.0 / 60.0;
const SUELO: u64 = 1;
const PARED: u64 = 2;
const TECHO: u64 = 3;
const PJ: u64 = 10;

// Arnés: misma creación de cuerpos estáticos que character_steps.rs.
fn caja_estatica(w: &mut PhysicsWorld, entity: u64, centro: [f32; 3], semiejes: [f32; 3]) {
    let h = w.create_rigid_body(entity, glass_physics::BodyType::Static, centro, [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(h, semiejes, &glass_core::PhysicsMaterial { dynamic_friction: 0.5, bounciness: 0.0, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
}

fn mundo_con_suelo() -> PhysicsWorld {
    let mut w = PhysicsWorld::new();
    caja_estatica(&mut w, SUELO, [0.0, -0.5, 0.0], [20.0, 0.5, 20.0]);
    w
}

fn config() -> CharacterConfig {
    CharacterConfig {
        radius: 0.3,
        height: 1.8,
        skin: 0.02,
        step_offset: 0.4,
        slope_limit_deg: 45.0,
        snap: 0.3,
    }
}

#[test]
fn caminar_contra_la_pared_da_sides() {
    let mut w = mundo_con_suelo();
    caja_estatica(&mut w, PARED, [2.0, 1.5, 0.0], [0.1, 1.5, 5.0]);
    w.create_character(PJ, [0.0, 0.95, 0.0], config());

    let mut visto = false;
    for _ in 0..120 {
        w.move_character(PJ, [0.1, -0.01, 0.0], DT).expect("personaje");
        let (flags, _, hits) = w.character_last_move(PJ).expect("estado");
        let contra_pared = hits
            .iter()
            .any(|h| h.entity == PARED && h.normal[1].abs() < 0.3 && h.normal[0] < 0.0);
        if flags & CHARACTER_FLAG_SIDES != 0 && contra_pared {
            visto = true;
            break;
        }
    }
    assert!(visto, "nunca se registró Sides con hit en la pared");
}

#[test]
fn caer_al_suelo_da_below_y_velocidad() {
    let mut w = mundo_con_suelo();
    w.create_character(PJ, [0.0, 4.0, 0.0], config());

    let vy = -5.0;
    w.move_character(PJ, [0.0, vy * DT, 0.0], DT).expect("personaje");
    let (_, vel, _) = w.character_last_move(PJ).expect("estado");
    assert!(vel[1] < -1.0, "velocidad Y en caída: {}", vel[1]);

    for _ in 0..180 {
        w.move_character(PJ, [0.0, vy * DT, 0.0], DT).expect("personaje");
    }
    let (flags, _, _) = w.character_last_move(PJ).expect("estado");
    assert!(flags & CHARACTER_FLAG_BELOW != 0, "flags tras asentarse: {flags:#b}");
}

#[test]
fn golpearse_el_techo_da_above() {
    let mut w = mundo_con_suelo();
    caja_estatica(&mut w, TECHO, [0.0, 2.6, 0.0], [5.0, 0.2, 5.0]);
    w.create_character(PJ, [0.0, 0.95, 0.0], config());

    let mut visto = false;
    for _ in 0..60 {
        w.move_character(PJ, [0.0, 0.15, 0.0], DT).expect("personaje");
        let (flags, _, _) = w.character_last_move(PJ).expect("estado");
        if flags & CHARACTER_FLAG_ABOVE != 0 {
            visto = true;
            break;
        }
    }
    assert!(visto, "nunca se registró Above bajo el techo");
}
