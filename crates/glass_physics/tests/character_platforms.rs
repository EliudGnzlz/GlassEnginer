use glass_physics::{BodyType, CharacterConfig, PhysicsWorld};

const PJ: u64 = 15;
const DT: f32 = 1.0 / 60.0;

fn cfg() -> CharacterConfig {
    CharacterConfig {
        radius: 0.35,
        height: 1.8,
        skin: 0.02,
        step_offset: 0.4,
        slope_limit_deg: 45.0,
        snap: 0.3,
    }
}

/// Mundo con suelo estático grande (cima en y=0).
fn mundo() -> PhysicsWorld {
    let mut w = PhysicsWorld::new();
    let suelo = w.create_rigid_body(1, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(suelo, [30.0, 0.5, 10.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.5, bounciness: 0.0, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
    w
}

/// Crea la plataforma cinemática (cima en y = centro_y + 0.25) y devuelve su handle.
macro_rules! plataforma {
    ($w:expr, $c:expr) => {{
        let h = $w.create_rigid_body(2, BodyType::Kinematic, $c, [0.0, 0.0, 0.0, 1.0]);
        $w.attach_box_collider(h, [1.5, 0.25, 1.5], &glass_core::PhysicsMaterial { dynamic_friction: 0.5, bounciness: 0.0, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
        h
    }};
}

/// Mueve el personaje, acumula `applied` en `pos` y devuelve `grounded`.
macro_rules! mover {
    ($w:expr, $pos:expr, $deseo:expr) => {{
        let (applied, grounded, _sliding) = $w
            .move_character(PJ, $deseo, DT)
            .expect("move_character falló");
        $pos[0] += applied[0];
        $pos[1] += applied[1];
        $pos[2] += applied[2];
        grounded
    }};
}

#[test]
fn plataforma_horizontal_arrastra() {
    let mut w = mundo();
    let mut px = 0.0;
    let plat = plataforma!(w, [px, 0.75, 0.0]);
    let _ = w.create_character(PJ, [0.0, 1.91, 0.0], cfg());

    let mut pos = [0.0, 1.91, 0.0];
    let mut grounded = false;
    for _ in 0..60 {
        // La plataforma avanza antes del move del personaje.
        px += 0.02;
        w.set_transform(plat, [px, 0.75, 0.0], [0.0, 0.0, 0.0, 1.0]);
        grounded = mover!(w, pos, [0.0, -DT, 0.0]);
    }

    assert!(
        (pos[0] - 1.2).abs() < 0.05,
        "x final {} esperada ≈ 1.2 (arrastre)",
        pos[0]
    );
    assert!(grounded, "debe seguir grounded en el último frame");
}

#[test]
fn ascensor_sube_y_baja_sin_despegarse() {
    let mut w = mundo();
    let mut py = 0.75;
    let plat = plataforma!(w, [0.0, py, 0.0]);
    let _ = w.create_character(PJ, [0.0, 1.91, 0.0], cfg());

    let y_inicial = 1.91;
    let mut pos = [0.0, y_inicial, 0.0];
    let mut frame = 0;

    // Subida.
    for i in 0..40 {
        py += 0.02;
        w.set_transform(plat, [0.0, py, 0.0], [0.0, 0.0, 0.0, 1.0]);
        let grounded = mover!(w, pos, [0.0, -DT, 0.0]);
        assert!(
            grounded || frame == 0,
            "subida: despegado en frame {} (y={})",
            frame,
            pos[1]
        );
        if i == 19 {
            let esperada = y_inicial + 20.0 * 0.02;
            assert!(
                (pos[1] - esperada).abs() < 0.06,
                "mitad de subida: y={} esperada ≈ {}",
                pos[1],
                esperada
            );
        }
        frame += 1;
    }
    let esperada = y_inicial + 40.0 * 0.02;
    assert!(
        (pos[1] - esperada).abs() < 0.06,
        "fin de subida: y={} esperada ≈ {}",
        pos[1],
        esperada
    );

    // Bajada.
    for _ in 0..40 {
        py -= 0.02;
        w.set_transform(plat, [0.0, py, 0.0], [0.0, 0.0, 0.0, 1.0]);
        let grounded = mover!(w, pos, [0.0, -DT, 0.0]);
        assert!(
            grounded,
            "bajada: despegado en frame {} (y={})",
            frame,
            pos[1]
        );
        frame += 1;
    }

    assert!(
        (pos[1] - y_inicial).abs() < 0.06,
        "y final {} esperada ≈ {}",
        pos[1],
        y_inicial
    );
}

#[test]
fn bajar_caminando_suelta_la_plataforma() {
    let mut w = mundo();
    let mut px = 0.0;
    let plat = plataforma!(w, [px, 0.75, 0.0]);
    let _ = w.create_character(PJ, [0.0, 1.91, 0.0], cfg());

    let mut pos = [0.0, 1.91, 0.0];
    let mut en_suelo = false;

    // Camina −X hasta caer al suelo estático.
    for _ in 0..600 {
        px += 0.02;
        w.set_transform(plat, [px, 0.75, 0.0], [0.0, 0.0, 0.0, 1.0]);
        let grounded = mover!(w, pos, [-1.6 / 60.0, -DT, 0.0]);
        if grounded && (pos[1] - 0.91).abs() < 0.05 {
            en_suelo = true;
            break;
        }
    }
    assert!(en_suelo, "no llegó a caer al suelo (pos={:?})", pos);

    // Solo se mueve la plataforma: el personaje no debe ser arrastrado.
    let x_ref = pos[0];
    for f in 0..30 {
        px += 0.02;
        w.set_transform(plat, [px, 0.75, 0.0], [0.0, 0.0, 0.0, 1.0]);
        mover!(w, pos, [0.0, -DT, 0.0]);
        assert!(
            (pos[0] - x_ref).abs() <= 0.01,
            "frame {}: x={} se movió desde {} (arrastre residual)",
            f,
            pos[0],
            x_ref
        );
    }
}

#[test]
fn teletransporte_limpia_el_arrastre() {
    let mut w = mundo();
    let mut px = 0.0;
    let plat = plataforma!(w, [px, 0.75, 0.0]);
    let _ = w.create_character(PJ, [0.0, 1.91, 0.0], cfg());

    let mut pos = [0.0, 1.91, 0.0];
    for _ in 0..20 {
        px += 0.02;
        w.set_transform(plat, [px, 0.75, 0.0], [0.0, 0.0, 0.0, 1.0]);
        mover!(w, pos, [0.0, -DT, 0.0]);
    }

    // Teletransporte lejos, sobre el suelo estático.
    assert!(
        w.teleport_character(PJ, [-10.0, 0.91, 0.0]),
        "teleport_character devolvió false"
    );
    pos = [-10.0, 0.91, 0.0];

    let mut grounded = false;
    for f in 0..30 {
        px += 0.02;
        w.set_transform(plat, [px, 0.75, 0.0], [0.0, 0.0, 0.0, 1.0]);
        grounded = mover!(w, pos, [0.0, -DT, 0.0]);
        assert!(
            (pos[0] + 10.0).abs() <= 0.01,
            "frame {} tras teleport: x={} esperada −10 (memoria de suelo no limpiada)",
            f,
            pos[0]
        );
    }
    assert!(grounded, "debe terminar grounded sobre el suelo estático");
}
