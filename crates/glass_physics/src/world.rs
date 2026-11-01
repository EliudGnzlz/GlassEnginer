use std::collections::HashMap;
use rapier3d::prelude::*;
use glass_core::TransformFfi;
use crate::types::{BodyHandle, BodyType, ColliderHandle};

pub struct PhysicsWorld {
    pub gravity: Vector<f32>,
    pub integration_parameters: IntegrationParameters,
    pub physics_pipeline: PhysicsPipeline,
    pub island_manager: IslandManager,
    pub broad_phase: DefaultBroadPhase,
    pub narrow_phase: NarrowPhase,
    pub impulse_joint_set: ImpulseJointSet,
    pub multibody_joint_set: MultibodyJointSet,
    pub ccd_solver: CCDSolver,
    pub rigid_body_set: RigidBodySet,
    pub collider_set: ColliderSet,

    next_handle: u64,
    body_map: HashMap<BodyHandle, (RigidBodyHandle, u64)>, // BodyHandle -> (RapierHandle, entity_id)
    handle_lookup: HashMap<RigidBodyHandle, BodyHandle>,
    accumulator: f32,
    pub fixed_timestep: f32,
}

impl PhysicsWorld {
    pub fn new() -> Self {
        Self {
            gravity: vector![0.0, -9.81, 0.0],
            integration_parameters: IntegrationParameters::default(),
            physics_pipeline: PhysicsPipeline::new(),
            island_manager: IslandManager::new(),
            broad_phase: DefaultBroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joint_set: ImpulseJointSet::new(),
            multibody_joint_set: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
            rigid_body_set: RigidBodySet::new(),
            collider_set: ColliderSet::new(),
            next_handle: 1,
            body_map: HashMap::new(),
            handle_lookup: HashMap::new(),
            accumulator: 0.0,
            fixed_timestep: 1.0 / 60.0,
        }
    }

    /// Steps the physics simulation using a fixed timestep accumulator to guarantee stability and prevent jitter.
    pub fn step(&mut self, dt: f32) {
        self.accumulator += dt;
        let fixed_dt = self.fixed_timestep;
        self.integration_parameters.dt = fixed_dt;

        // Prevent spiral of death by capping max substeps per frame
        let mut substeps = 0;
        while self.accumulator >= fixed_dt && substeps < 4 {
            self.physics_pipeline.step(
                &self.gravity,
                &self.integration_parameters,
                &mut self.island_manager,
                &mut self.broad_phase,
                &mut self.narrow_phase,
                &mut self.rigid_body_set,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                &mut self.ccd_solver,
                None,
                &(),
                &(),
            );
            self.accumulator -= fixed_dt;
            substeps += 1;
        }
    }

    /// Creates a rigid body in the physics world and maps it to an entity.
    pub fn create_rigid_body(
        &mut self,
        entity_id: u64,
        body_type: BodyType,
        pos: [f32; 3],
        rot: [f32; 4],
    ) -> BodyHandle {
        let rapier_type = match body_type {
            BodyType::Dynamic => RigidBodyType::Dynamic,
            BodyType::Static => RigidBodyType::Fixed,
            BodyType::Kinematic => RigidBodyType::KinematicPositionBased,
        };

        // Note: nalgebra Quaternion takes (w, i, j, k) where i=x, j=y, k=z
        let quat = nalgebra::Quaternion::new(rot[3], rot[0], rot[1], rot[2]);
        let unit_quat = nalgebra::UnitQuaternion::from_quaternion(quat);

        let rigid_body = RigidBodyBuilder::new(rapier_type)
            .translation(vector![pos[0], pos[1], pos[2]])
            .rotation(unit_quat.scaled_axis())
            .user_data(entity_id as u128)
            .build();

        let rapier_handle = self.rigid_body_set.insert(rigid_body);
        let handle = self.next_handle;
        self.next_handle += 1;

        self.body_map.insert(handle, (rapier_handle, entity_id));
        self.handle_lookup.insert(rapier_handle, handle);

        handle
    }

    /// Attaches an axis-aligned box collider to a rigid body.
    pub fn attach_box_collider(
        &mut self,
        body_handle: BodyHandle,
        half_extents: [f32; 3],
        restitution: f32,
        friction: f32,
    ) -> Option<ColliderHandle> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;

        let collider = ColliderBuilder::cuboid(half_extents[0], half_extents[1], half_extents[2])
            .restitution(restitution)
            .friction(friction)
            .build();

        let collider_handle = self.collider_set.insert_with_parent(
            collider,
            rapier_handle,
            &mut self.rigid_body_set,
        );

        Some(collider_handle.into_raw_parts().0 as u64)
    }

    /// Attaches a sphere collider to a rigid body.
    pub fn attach_sphere_collider(
        &mut self,
        body_handle: BodyHandle,
        radius: f32,
        restitution: f32,
        friction: f32,
    ) -> Option<ColliderHandle> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;

        let collider = ColliderBuilder::ball(radius)
            .restitution(restitution)
            .friction(friction)
            .build();

        let collider_handle = self.collider_set.insert_with_parent(
            collider,
            rapier_handle,
            &mut self.rigid_body_set,
        );

        Some(collider_handle.into_raw_parts().0 as u64)
    }

    /// Gets the current transform of a rigid body.
    pub fn get_transform(&self, body_handle: BodyHandle) -> Option<TransformFfi> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;
        let rb = self.rigid_body_set.get(rapier_handle)?;
        let translation = rb.translation();
        let rotation = rb.rotation();
        let quat = rotation.quaternion();

        Some(TransformFfi {
            position: [translation.x, translation.y, translation.z],
            rotation: [quat.i, quat.j, quat.k, quat.w],
            scale: [1.0, 1.0, 1.0],
        })
    }

    /// Directly sets the transform of a rigid body (e.g. for kinematic bodies or teleporting).
    pub fn set_transform(&mut self, body_handle: BodyHandle, pos: [f32; 3], rot: [f32; 4]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            let quat = nalgebra::Quaternion::new(rot[3], rot[0], rot[1], rot[2]);
            let unit_quat = nalgebra::UnitQuaternion::from_quaternion(quat);
            rb.set_translation(vector![pos[0], pos[1], pos[2]], true);
            rb.set_rotation(unit_quat, true);
            true
        } else {
            false
        }
    }

    /// Applies an instantaneous linear impulse to a rigid body.
    pub fn apply_impulse(&mut self, body_handle: BodyHandle, impulse: [f32; 3]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.apply_impulse(vector![impulse[0], impulse[1], impulse[2]], true);
            true
        } else {
            false
        }
    }

    /// Applies a continuous force to a rigid body.
    pub fn apply_force(&mut self, body_handle: BodyHandle, force: [f32; 3]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.add_force(vector![force[0], force[1], force[2]], true);
            true
        } else {
            false
        }
    }

    /// Removes a rigid body and its attached colliders.
    pub fn remove_rigid_body(&mut self, body_handle: BodyHandle) -> bool {
        if let Some((rapier_handle, _)) = self.body_map.remove(&body_handle) {
            self.handle_lookup.remove(&rapier_handle);
            self.rigid_body_set.remove(
                rapier_handle,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
            true
        } else {
            false
        }
    }

    /// Returns a bulk list of (entity_id, TransformFfi) for all active dynamic bodies to synchronize back to SceneState.
    pub fn sync_all_transforms(&self) -> Vec<(u64, TransformFfi)> {
        let mut results = Vec::with_capacity(self.body_map.len());
        for (&_body_handle, &(rapier_handle, entity_id)) in &self.body_map {
            if let Some(rb) = self.rigid_body_set.get(rapier_handle) {
                if rb.is_dynamic() {
                    let translation = rb.translation();
                    let quat = rb.rotation().quaternion();
                    results.push((
                        entity_id,
                        TransformFfi {
                            position: [translation.x, translation.y, translation.z],
                            rotation: [quat.i, quat.j, quat.k, quat.w],
                            scale: [1.0, 1.0, 1.0],
                        },
                    ));
                }
            }
        }
        results
    }

    pub fn body_count(&self) -> usize {
        self.rigid_body_set.len()
    }

    pub fn collider_count(&self) -> usize {
        self.collider_set.len()
    }
}
