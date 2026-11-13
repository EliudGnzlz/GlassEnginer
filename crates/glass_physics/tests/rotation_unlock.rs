//! Regresión del desbloqueo de rotaciones (orden 2026-11-13): el
//! bloqueo era global (lock_rotations al crear todo dinámico) y pasa a
//! ser opción por cuerpo estilo Unity (set_locked_axes). Una tabla
//! larga apoyada descentrada en un pedestal estrecho debe VOLCAR por
//! gravedad con las rotaciones libres, y NO debe rotar con
//! FreezeRotation (bits 8|16|32 = 56).
use glass_physics::{BodyType, PhysicsWorld};

const DT: f32 = 1.0 / 60.0;

/// Pedestal estrecho con la tabla encima, descentrada +0,25 en X.
fn mundo(freeze_rotation: bool) -> (PhysicsWorld, u64) {
    let mut w = PhysicsWorld::new();
    let pedestal = w.create_rigid_body(1, BodyType::Static, [0.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(pedestal, [0.15, 0.5, 0.15], 0.0, 0.5);
    let tabla = w.create_rigid_body(2, BodyType::Dynamic, [0.25, 1.09, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(tabla, [1.2, 0.08, 0.2], 0.0, 0.5);
    if freeze_rotation {
        assert!(w.set_locked_axes(tabla, 56));
        assert_eq!(w.get_locked_axes(tabla), Some(56));
    } else {
        assert_eq!(w.get_locked_axes(tabla), Some(0));
    }
    (w, tabla)
}

/// Componente mayor de rotación del cuaternión (0 = identidad).
fn rot_max(w: &PhysicsWorld, h: u64) -> f32 {
    let t = w.get_transform(h).expect("el cuerpo existe");
    let q = t.rotation;
    q[0].abs().max(q[1].abs()).max(q[2].abs())
}

#[test]
fn la_tabla_vuelca_con_rotaciones_libres() {
    let (mut w, tabla) = mundo(false);
    for _ in 0..180 {
        w.step(DT);
    }
    let r = rot_max(&w, tabla);
    assert!(r > 0.15, "la tabla debió volcar (rotación apreciable); q máx {r}");
}

#[test]
fn con_freeze_rotation_no_rota() {
    let (mut w, tabla) = mundo(true);
    for _ in 0..180 {
        w.step(DT);
    }
    let r = rot_max(&w, tabla);
    assert!(r < 0.02, "con FreezeRotation no debe rotar; q máx {r}");
}
