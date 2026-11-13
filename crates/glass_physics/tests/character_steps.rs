//! Regresión del autostep del CharacterController: suelo plano y un
//! peldaño de 0,35 m; el personaje avanza hacia +X CON la presión de
//! gravedad que usa un script (vy = -1 m/s), el caso que dejó a
//! Rapier sin disparar el autostep hasta que el mundo separó el
//! movimiento en pasada horizontal y vertical.
use glass_physics::{BodyType, CharacterConfig, PhysicsWorld};

fn mundo_con_peldano(altura_peldano: f32) -> PhysicsWorld {
    let mut w = PhysicsWorld::new();
    let suelo = w.create_rigid_body(1, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(suelo, [30.0, 0.5, 10.0], 0.0, 0.5);
    let paso = w.create_rigid_body(
        2,
        BodyType::Static,
        [6.45, altura_peldano / 2.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    );
    w.attach_box_collider(paso, [0.45, altura_peldano / 2.0, 1.5], 0.0, 0.5);
    w
}

fn cfg(step: f32) -> CharacterConfig {
    CharacterConfig {
        radius: 0.35,
        height: 1.8,
        skin: 0.02,
        step_offset: step,
        slope_limit_deg: 45.0,
        snap: 0.3,
    }
}

/// Avanza 240 frames a 1,6 m/s con vy=-1; devuelve (pos final, y máx).
fn avanza(w: &mut PhysicsWorld, step: f32) -> ([f32; 3], f32) {
    assert!(w.create_character(15, [5.0, 0.91, 0.0], cfg(step)));
    let mut pos = [5.0f32, 0.91, 0.0];
    let mut max_y = pos[1];
    for _ in 0..240 {
        let (a, _, _) = w
            .move_character(15, [1.6 / 60.0, -1.0 / 60.0, 0.0], 1.0 / 60.0)
            .expect("el personaje existe");
        pos = [pos[0] + a[0], pos[1] + a[1], pos[2] + a[2]];
        max_y = max_y.max(pos[1]);
    }
    (pos, max_y)
}

#[test]
fn autostep_con_presion_de_gravedad() {
    let mut w = mundo_con_peldano(0.35);
    let (pos, max_y) = avanza(&mut w, 0.4);
    // Subió el peldaño (centro a 1,25+ sobre la cima de 0,35)…
    assert!(max_y > 1.2, "debió pisar la cima del peldaño; y máx {max_y}");
    // …lo cruzó y volvió a caer al suelo del otro lado.
    assert!(pos[0] > 10.0, "debió cruzar el peldaño; x final {}", pos[0]);
    assert!((pos[1] - 0.91).abs() < 0.06, "debió aterrizar a 0,91; y {}", pos[1]);
}

#[test]
fn sin_step_offset_se_bloquea() {
    let mut w = mundo_con_peldano(0.35);
    let (pos, _) = avanza(&mut w, 0.05);
    assert!(pos[0] < 5.8, "con stepOffset 0,05 debe bloquearse; x {}", pos[0]);
    assert!((pos[1] - 0.92).abs() < 0.06, "y debe quedar a ras de suelo; y {}", pos[1]);
}

/// Mundo con la rampa del 30° de la escena de prueba (caja rotada).
fn mundo_con_rampa(slope_limit_deg: f32) -> (PhysicsWorld, CharacterConfig) {
    let mut w = PhysicsWorld::new();
    let suelo = w.create_rigid_body(1, BodyType::Static, [0.0, -0.5, -2.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(suelo, [30.0, 0.5, 35.0], 0.0, 0.5);
    // Rotación de +30° sobre X (cuaternión [x,y,z,w]).
    let s = (15.0f32).to_radians().sin();
    let c = (15.0f32).to_radians().cos();
    let rampa = w.create_rigid_body(2, BodyType::Static, [0.0, 1.35, -13.5], [s, 0.0, 0.0, c]);
    w.attach_box_collider(rampa, [2.0, 0.15, 3.5], 0.0, 0.5);
    (
        w,
        CharacterConfig {
            radius: 0.35,
            height: 1.8,
            skin: 0.02,
            step_offset: 0.4,
            slope_limit_deg,
            snap: 0.3,
        },
    )
}

fn sube_rampa(slope: f32) -> [f32; 3] {
    let (mut w, cfg) = mundo_con_rampa(slope);
    assert!(w.create_character(15, [0.0, 0.91, -8.6], cfg));
    let mut pos = [0.0f32, 0.91, -8.6];
    for _ in 0..210 {
        let (a, _, _) = w
            .move_character(15, [0.0, -1.0 / 60.0, -1.6 / 60.0], 1.0 / 60.0)
            .expect("el personaje existe");
        pos = [pos[0] + a[0], pos[1] + a[1], pos[2] + a[2]];
    }
    pos
}

#[test]
fn rampa_30_con_limite_45_sube() {
    let pos = sube_rampa(45.0);
    assert!(pos[1] > 1.8, "con slopeLimit 45 debe subir la rampa; y {}", pos[1]);
}

#[test]
fn rampa_30_con_limite_20_no_sube() {
    // La puerta de discontinuidad impide que la muestra de escalón
    // «suba» la rampa a saltos cuando el límite la veta.
    let pos = sube_rampa(20.0);
    assert!(pos[1] < 1.35, "con slopeLimit 20 no debe subir; y {}", pos[1]);
}
