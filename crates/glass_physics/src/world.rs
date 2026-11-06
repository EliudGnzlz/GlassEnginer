use std::collections::HashMap;
use rapier3d::prelude::*;
use glass_core::TransformFfi;
use crate::types::{BodyHandle, BodyType, ColliderHandle, ColliderShape};

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
    pub query_pipeline: QueryPipeline,

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
            query_pipeline: QueryPipeline::new(),
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

        let mut builder = RigidBodyBuilder::new(rapier_type)
            .translation(vector![pos[0], pos[1], pos[2]])
            .rotation(unit_quat.scaled_axis())
            .user_data(entity_id as u128);

        if rapier_type == RigidBodyType::Dynamic {
            builder = builder.lock_rotations().linear_damping(1.5);
        }

        let rigid_body = builder.build();

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

    /// Attaches a capsule collider to a rigid body. `half_height` es
    /// la mitad del TRAMO RECTO (sin casquetes), como en Rapier.
    pub fn attach_capsule_collider(
        &mut self,
        body_handle: BodyHandle,
        half_height: f32,
        radius: f32,
        restitution: f32,
        friction: f32,
    ) -> Option<ColliderHandle> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;

        let collider = ColliderBuilder::capsule_y(half_height, radius)
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

    /// Attaches a cylinder collider to a rigid body. `half_height` es
    /// la mitad de la altura total.
    pub fn attach_cylinder_collider(
        &mut self,
        body_handle: BodyHandle,
        half_height: f32,
        radius: f32,
        restitution: f32,
        friction: f32,
    ) -> Option<ColliderHandle> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;

        let collider = ColliderBuilder::cylinder(half_height, radius)
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

    /// Adjunta el collider descrito por un `ColliderShape` (la vía
    /// que usa la reconstrucción de escena: la forma ya viene derivada
    /// de la malla por `collider_shape_for_builtin`). Delega en los
    /// attach específicos para que exista una sola construcción de
    /// cada forma en el crate.
    pub fn attach_collider(
        &mut self,
        body_handle: BodyHandle,
        shape: ColliderShape,
        restitution: f32,
        friction: f32,
    ) -> Option<ColliderHandle> {
        match shape {
            ColliderShape::Cuboid { half_extents } => {
                self.attach_box_collider(body_handle, half_extents, restitution, friction)
            }
            ColliderShape::Ball { radius } => {
                self.attach_sphere_collider(body_handle, radius, restitution, friction)
            }
            ColliderShape::Capsule { half_height, radius } => {
                self.attach_capsule_collider(body_handle, half_height, radius, restitution, friction)
            }
            ColliderShape::Cylinder { half_height, radius } => {
                self.attach_cylinder_collider(body_handle, half_height, radius, restitution, friction)
            }
        }
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

    /// Gets the linear velocity of a rigid body.
    pub fn get_linear_velocity(&self, body_handle: BodyHandle) -> Option<[f32; 3]> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;
        let rb = self.rigid_body_set.get(rapier_handle)?;
        let linvel = rb.linvel();
        Some([linvel.x, linvel.y, linvel.z])
    }

    /// Sets the linear velocity of a rigid body directly.
    pub fn set_linear_velocity(&mut self, body_handle: BodyHandle, linvel: [f32; 3]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.set_linvel(vector![linvel[0], linvel[1], linvel[2]], true);
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

    /// Finds an existing rigid body handle by entity ID if registered.
    pub fn get_body_by_entity(&self, entity_id: u64) -> Option<BodyHandle> {
        self.body_map.iter().find(|(_, &(_, eid))| eid == entity_id).map(|(&h, _)| h)
    }

    /// Returns the internal Rapier handle and entity_id for a BodyHandle.
    pub fn get_rapier_handle(&self, body_handle: BodyHandle) -> Option<(RigidBodyHandle, u64)> {
        self.body_map.get(&body_handle).copied()
    }

    /// Lanza un rayo contra los colliders del mundo y devuelve el primer
    /// impacto (entidad, punto, normal y distancia). La QueryPipeline se
    /// actualiza en cada llamada: el mundo se reconstruye al cambiar de
    /// escena y los scripts crean/destruyen cuerpos en runtime, así que una
    /// pipeline cacheada quedaría obsoleta; el coste es asumible para los
    /// pocos raycasts por frame que hace el gameplay (suelo, espada).
    /// `exclude_entity` (0 = ninguna) ignora los colliders de esa entidad.
    pub fn cast_ray(
        &mut self,
        origin: [f32; 3],
        dir: [f32; 3],
        max_dist: f32,
        exclude_entity: u64,
    ) -> Option<RayHit> {
        if !origin.iter().chain(dir.iter()).all(|v| v.is_finite())
            || !max_dist.is_finite()
            || max_dist <= 0.0
        {
            return None;
        }
        let dir_v = vector![dir[0], dir[1], dir[2]];
        let len = dir_v.magnitude();
        if !len.is_finite() || len < 1e-8 {
            return None;
        }
        let ray = Ray::new(point![origin[0], origin[1], origin[2]], dir_v / len);
        self.query_pipeline.update(&self.collider_set);
        let excluded_rb = if exclude_entity != 0 {
            self.get_body_by_entity(exclude_entity)
                .and_then(|bh| self.get_rapier_handle(bh))
                .map(|(rh, _)| rh)
        } else {
            None
        };
        // Ojo: el handle aquí es el de Rapier, no el alias propio del crate.
        let pred = |_handle: rapier3d::geometry::ColliderHandle, collider: &Collider| -> bool {
            match (excluded_rb, collider.parent()) {
                (Some(ex), Some(parent)) => parent != ex,
                _ => true,
            }
        };
        let filter = QueryFilter::default().predicate(&pred);
        let (handle, intersection) = self.query_pipeline.cast_ray_and_get_normal(
            &self.rigid_body_set,
            &self.collider_set,
            &ray,
            max_dist,
            true,
            filter,
        )?;
        let collider = self.collider_set.get(handle)?;
        let rb_handle = collider.parent()?;
        let body_handle = self.handle_lookup.get(&rb_handle)?;
        let &(_, entity_id) = self.body_map.get(body_handle)?;
        let point = ray.point_at(intersection.time_of_impact);
        Some(RayHit {
            entity_id,
            point: [point.x, point.y, point.z],
            normal: [
                intersection.normal.x,
                intersection.normal.y,
                intersection.normal.z,
            ],
            distance: intersection.time_of_impact,
        })
    }
}

/// Impacto de un raycast: entidad golpeada y geometría del contacto.
#[derive(Debug, Clone, Copy)]
pub struct RayHit {
    pub entity_id: u64,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub distance: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::collider_shape_for_builtin;

    /// Mundo mínimo: suelo fijo (caja ancha con la superficie en
    /// y = 0) y pasos de 1/60 hasta que el cuerpo de prueba se asiente.
    fn mundo_con_suelo() -> (PhysicsWorld, u64) {
        let mut world = PhysicsWorld::new();
        let suelo = world.create_rigid_body(10, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world
            .attach_box_collider(suelo, [10.0, 0.5, 10.0], 0.0, 0.9)
            .expect("collider suelo");
        (world, suelo)
    }

    fn asentar(world: &mut PhysicsWorld, pasos: usize) {
        for _ in 0..pasos {
            world.step(1.0 / 60.0);
        }
    }

    #[test]
    fn bola_por_inferencia_descansa_a_la_altura_de_su_radio() {
        // Esfera de escala 2 (radio mundial 1) soltada sobre el suelo:
        // con Ball descansa con el centro a y ≈ 1; con la caja de
        // siempre (box_extents 0,5) quedaría a y ≈ 0,5, medio cuerpo
        // hundido. Es el test que distingue la forma nueva de la vieja.
        let (mut world, _suelo) = mundo_con_suelo();
        let shape = collider_shape_for_builtin("sphere", [2.0, 2.0, 2.0], [0.5, 0.5, 0.5]);
        assert_eq!(shape, ColliderShape::Ball { radius: 1.0 });
        let bola = world.create_rigid_body(20, BodyType::Dynamic, [0.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world.attach_collider(bola, shape, 0.0, 0.9).expect("collider bola");
        asentar(&mut world, 180);
        let t = world.get_transform(bola).expect("transform bola");
        assert!(
            (t.position[1] - 1.0).abs() < 0.05,
            "la bola debía descansar con el centro a su radio (1,0); está a {}",
            t.position[1]
        );
    }

    #[test]
    fn capsula_por_inferencia_se_apoya_de_pie_estable() {
        // Cápsula de preset (escala [1,2,1]): half_height 1,5 + radio
        // 0,5 → apoyada de pie el centro queda a 2,0, y quieta.
        let (mut world, _suelo) = mundo_con_suelo();
        let shape = collider_shape_for_builtin("capsule", [1.0, 2.0, 1.0], [0.5, 0.5, 0.5]);
        let capsula = world.create_rigid_body(30, BodyType::Dynamic, [0.0, 4.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world.attach_collider(capsula, shape, 0.0, 0.9).expect("collider cápsula");
        asentar(&mut world, 240);
        let t = world.get_transform(capsula).expect("transform cápsula");
        assert!(
            (t.position[1] - 2.0).abs() < 0.05,
            "la cápsula debía apoyar de pie con el centro a 2,0; está a {}",
            t.position[1]
        );
        // Y sigue ahí tras más pasos (apoyo estable, no equilibrio fugaz).
        let y1 = t.position[1];
        asentar(&mut world, 60);
        let t2 = world.get_transform(capsula).expect("transform cápsula 2");
        assert!((t2.position[1] - y1).abs() < 0.01, "la cápsula no quedó estable");
    }

    #[test]
    fn cilindro_por_inferencia_se_apoya_sobre_su_tapa() {
        let (mut world, _suelo) = mundo_con_suelo();
        let shape = collider_shape_for_builtin("cylinder", [1.0, 1.0, 1.0], [0.5, 0.5, 0.5]);
        let cilindro = world.create_rigid_body(40, BodyType::Dynamic, [2.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world.attach_collider(cilindro, shape, 0.0, 0.9).expect("collider cilindro");
        asentar(&mut world, 240);
        let t = world.get_transform(cilindro).expect("transform cilindro");
        assert!(
            (t.position[1] - 1.0).abs() < 0.05,
            "el cilindro (half_height 1) debía descansar con el centro a 1,0; está a {}",
            t.position[1]
        );
    }
}
