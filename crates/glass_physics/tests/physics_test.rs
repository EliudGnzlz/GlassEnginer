use glass_physics::{BodyType, PhysicsWorld};

#[test]
fn test_physics_world_simulation_and_collision() {
    let mut world = PhysicsWorld::new();

    // 1. Create a static floor at y = 0.0
    // Floor center at (0, 0, 0), half_extents = [10.0, 0.5, 10.0] -> top surface at y = 0.5
    let floor_body = world.create_rigid_body(
        1001,
        BodyType::Static,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    );
    let floor_collider = world.attach_box_collider(floor_body, [10.0, 0.5, 10.0], 0.1, 0.7);
    assert!(floor_collider.is_some(), "Static floor collider must be created");

    // 2. Create a dynamic falling cube at y = 5.0
    // Half extents [0.5, 0.5, 0.5]
    let cube_body = world.create_rigid_body(
        1002,
        BodyType::Dynamic,
        [0.0, 5.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    );
    let cube_collider = world.attach_box_collider(cube_body, [0.5, 0.5, 0.5], 0.2, 0.5);
    assert!(cube_collider.is_some(), "Dynamic cube collider must be created");

    let initial_tf = world.get_transform(cube_body).expect("Initial transform");
    assert_eq!(initial_tf.position[1], 5.0);

    // 3. Step physics simulation for 120 ticks (2.0 seconds at 60 Hz)
    for _ in 0..120 {
        world.step(1.0 / 60.0);
    }

    // 4. Cube should have fallen and collided with the floor.
    // Floor top is at y = 0.5, cube half-height is 0.5.
    // Cube center should rest around y = 1.0 (+/- tolerance for penetration / bouncing)
    let rested_tf = world.get_transform(cube_body).expect("Transform after resting");
    println!("Cube rested at y = {:.3}", rested_tf.position[1]);
    assert!(
        rested_tf.position[1] < 4.0,
        "Cube must fall below initial height (y={})",
        rested_tf.position[1]
    );
    assert!(
        (rested_tf.position[1] - 1.0).abs() < 0.2,
        "Cube must rest on top of the floor around y=1.0 (got y={})",
        rested_tf.position[1]
    );

    // 5. Test impulse application (apply upward impulse to jump)
    let applied = world.apply_impulse(cube_body, [0.0, 10.0, 0.0]);
    assert!(applied, "Impulse should succeed");

    // Step a few frames
    for _ in 0..10 {
        world.step(1.0 / 60.0);
    }

    let jumped_tf = world.get_transform(cube_body).expect("Transform after jump");
    println!("Cube jumped to y = {:.3}", jumped_tf.position[1]);
    assert!(
        jumped_tf.position[1] > rested_tf.position[1],
        "Cube should have moved upward after positive Y impulse"
    );

    // 6. Test sync_all_transforms
    let dynamic_transforms = world.sync_all_transforms();
    assert_eq!(dynamic_transforms.len(), 1, "Only the 1 dynamic body should be returned");
    assert_eq!(dynamic_transforms[0].0, 1002);

    // 7. Test removing body
    let removed = world.remove_rigid_body(cube_body);
    assert!(removed, "Body removal should succeed");
    assert_eq!(world.sync_all_transforms().len(), 0);
}
