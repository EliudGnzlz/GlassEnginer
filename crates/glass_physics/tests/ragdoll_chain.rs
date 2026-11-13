use glass_physics::ragdoll::RagdollBoneSpec;
use glass_physics::world::PhysicsWorld;
use rapier3d::prelude::*;

const OWNER: u64 = 42;
const DT: f32 = 1.0 / 60.0;
const ID: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn world_with_ground() -> PhysicsWorld {
    let mut w = PhysicsWorld::new();
    let ground = ColliderBuilder::cuboid(20.0, 0.5, 20.0)
        .translation(vector![0.0, -0.5, 0.0])
        .friction(0.7)
        .build();
    w.collider_set.insert(ground);
    w
}

fn chain() -> Vec<RagdollBoneSpec> {
    vec![
        RagdollBoneSpec {
            pos: [0.0, 1.0, 0.0],
            quat: ID,
            parent: None,
            seg_dir_local: [0.0, 1.0, 0.0],
            seg_len: 0.4,
            radius: 0.09,
        },
        RagdollBoneSpec {
            pos: [0.0, 1.4, 0.0],
            quat: ID,
            parent: Some(0),
            seg_dir_local: [0.0, 1.0, 0.0],
            seg_len: 0.4,
            radius: 0.09,
        },
        RagdollBoneSpec {
            pos: [0.0, 1.8, 0.0],
            quat: ID,
            parent: Some(1),
            seg_dir_local: [0.0, 0.0, 0.0],
            seg_len: 0.0,
            radius: 0.09,
        },
        RagdollBoneSpec {
            pos: [0.35, 1.4, 0.0],
            quat: ID,
            parent: Some(1),
            seg_dir_local: [0.0, 0.0, 0.0],
            seg_len: 0.0,
            radius: 0.09,
        },
    ]
}

fn steps(w: &mut PhysicsWorld, n: usize) {
    for _ in 0..n {
        w.step(DT);
    }
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[test]
fn la_cadena_cae_y_queda_en_el_suelo() {
    let mut w = world_with_ground();
    assert!(w.create_ragdoll(OWNER, &chain()));
    steps(&mut w, 240);
    let t = w.ragdoll_bone_transforms(OWNER).expect("transforms");
    assert_eq!(t.len(), 4);
    for (i, (p, _)) in t.iter().enumerate() {
        assert!(p[1] < 0.6, "hueso {i} no cayó: y = {}", p[1]);
        assert!(p[1] > -0.1, "hueso {i} atravesó el suelo: y = {}", p[1]);
    }
}

#[test]
fn las_articulaciones_mantienen_las_distancias() {
    let mut w = world_with_ground();
    let specs = chain();
    assert!(w.create_ragdoll(OWNER, &specs));
    steps(&mut w, 300);
    let t = w.ragdoll_bone_transforms(OWNER).expect("transforms");
    for (parent, child) in [(0usize, 1usize), (1, 2), (1, 3)] {
        let d = dist(t[parent].0, t[child].0);
        let expected = specs[parent].seg_len;
        assert!(
            (d - expected).abs() <= 0.12,
            "par {parent}-{child}: distancia {d}, esperada {expected} ± 0.12"
        );
    }
}

#[test]
fn el_impulso_desplaza_la_cadena() {
    let mut w = world_with_ground();
    assert!(w.create_ragdoll(OWNER, &chain()));
    let x0 = w.ragdoll_bone_transforms(OWNER).expect("transforms")[1].0[0];
    assert!(w.apply_ragdoll_impulse(OWNER, 1, [6.0, 2.0, 0.0]));
    steps(&mut w, 30);
    let x1 = w.ragdoll_bone_transforms(OWNER).expect("transforms")[1].0[0];
    assert!(x1 - x0 > 0.3, "desplazamiento en X insuficiente: {}", x1 - x0);
}

#[test]
fn destruir_libera_el_ragdoll() {
    let mut w = world_with_ground();
    assert!(w.create_ragdoll(OWNER, &chain()));
    assert!(w.ragdoll_active(OWNER));
    assert!(w.destroy_ragdoll(OWNER));
    assert!(!w.ragdoll_active(OWNER));
    assert!(w.ragdoll_bone_transforms(OWNER).is_none());
}
