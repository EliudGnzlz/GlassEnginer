//! Interpolación de render (hueco beta): la mezcla cae entre el
//! transform previo al substep y el actual según el resto del
//! acumulador; los saltos grandes no se barren; sin activar, el
//! sync interpolado es idéntico al normal.
use glass_physics::world::PhysicsWorld;
use glass_physics::BodyType;

const DT: f32 = 1.0 / 60.0;

fn cuerpo_en_movimiento(w: &mut PhysicsWorld, id: u64) {
    let h = w.create_rigid_body(id, BodyType::Dynamic, [0.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.attach_box_collider(h, [0.5, 0.5, 0.5], 0.0, 0.0);
    assert!(w.set_gravity_scale(h, 0.0));
    assert!(w.set_linear_damping(h, 0.0));
    assert!(w.set_linear_velocity(h, [3.0, 0.0, 0.0]));
}

fn pos_de(lista: &[(u64, glass_core::types::TransformFfi)], id: u64) -> [f32; 3] {
    lista
        .iter()
        .find(|(e, _)| *e == id)
        .map(|(_, t)| t.position)
        .expect("la entidad está en el sync")
}

#[test]
fn la_mezcla_cae_entre_previo_y_actual() {
    let mut w = PhysicsWorld::new();
    cuerpo_en_movimiento(&mut w, 7);
    w.set_interpolation(7, true);

    w.step(DT); // un substep completo: el cuerpo avanza 3 * DT = 0.05
    let actual = pos_de(&w.sync_all_transforms(), 7);
    assert!(actual[0] > 0.04, "actual {actual:?}");

    w.step(DT / 2.0); // medio paso: sin substep nuevo, alpha = 0.5
    let mezcla = pos_de(&w.sync_all_transforms_interpolated(), 7);
    // Entre el previo (0.0) y el actual (0.05): estrictamente dentro.
    assert!(mezcla[0] > 0.001 && mezcla[0] < actual[0] - 0.001, "mezcla {mezcla:?} vs actual {actual:?}");
}

#[test]
fn el_salto_no_se_barre() {
    let mut w = PhysicsWorld::new();
    cuerpo_en_movimiento(&mut w, 8);
    w.set_interpolation(8, true);
    w.step(DT);

    // Teletransporte de 10 m: el sync interpolado da la posición nueva.
    let h = w.get_body_by_entity(8).expect("cuerpo");
    w.set_transform(h, [10.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    w.step(DT / 2.0);
    let mezcla = pos_de(&w.sync_all_transforms_interpolated(), 8);
    assert!(mezcla[0] > 9.9, "mezcla {mezcla:?}");
}

#[test]
fn sin_activar_no_cambia_nada() {
    let mut w = PhysicsWorld::new();
    cuerpo_en_movimiento(&mut w, 9);
    w.step(DT);
    w.step(DT / 2.0);
    let normal = pos_de(&w.sync_all_transforms(), 9);
    let interp = pos_de(&w.sync_all_transforms_interpolated(), 9);
    assert_eq!(normal, interp);
}
