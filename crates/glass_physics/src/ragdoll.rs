use crate::types::{BodyHandle, BodyType};
use crate::world::*;
use rapier3d::dynamics::ImpulseJointHandle;
use rapier3d::na::{Isometry3, Point3, Quaternion, Translation3, UnitQuaternion, Vector3};
use rapier3d::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct RagdollBoneSpec {
    pub pos: [f32; 3],
    pub quat: [f32; 4],
    pub parent: Option<usize>,
    pub seg_dir_local: [f32; 3],
    pub seg_len: f32,
    pub radius: f32,
}

pub struct RagdollState {
    pub bodies: Vec<BodyHandle>,
    pub joints: Vec<ImpulseJointHandle>,
}

fn spec_iso(s: &RagdollBoneSpec) -> Isometry3<f32> {
    let rot = UnitQuaternion::from_quaternion(Quaternion::new(
        s.quat[3], s.quat[0], s.quat[1], s.quat[2],
    ));
    Isometry3::from_parts(Translation3::new(s.pos[0], s.pos[1], s.pos[2]), rot)
}

fn arc_rotation(dir: &Vector3<f32>) -> UnitQuaternion<f32> {
    let y = Vector3::y();
    UnitQuaternion::rotation_between(&y, dir).unwrap_or_else(|| {
        // Antiparalelo a +Y: media vuelta alrededor de X.
        UnitQuaternion::from_axis_angle(&Vector3::x_axis(), std::f32::consts::PI)
    })
}

impl PhysicsWorld {
    pub fn create_ragdoll(&mut self, owner: u64, specs: &[RagdollBoneSpec]) -> bool {
        if self.ragdolls.contains_key(&owner) {
            self.destroy_ragdoll(owner);
        }
        if specs.is_empty() {
            return false;
        }
        for (i, s) in specs.iter().enumerate() {
            if let Some(p) = s.parent {
                if p >= specs.len() || p == i {
                    return false;
                }
            }
        }

        let mut bodies: Vec<BodyHandle> = Vec::with_capacity(specs.len());
        let mut rapier_bodies: Vec<RigidBodyHandle> = Vec::with_capacity(specs.len());

        for s in specs {
            let body = self.create_rigid_body(owner, BodyType::Dynamic, s.pos, s.quat);
            bodies.push(body);
            let Some((rb, _)) = self.get_rapier_handle(body) else {
                for b in bodies {
                    self.remove_rigid_body(b);
                }
                return false;
            };
            rapier_bodies.push(rb);

            let dir = Vector3::new(s.seg_dir_local[0], s.seg_dir_local[1], s.seg_dir_local[2]);
            let norm = dir.norm();
            let col = if s.seg_len > 0.01 && norm > 1e-6 {
                let dir = dir / norm;
                let half_height = (s.seg_len * 0.5 - s.radius).max(0.01);
                let iso = Isometry3::from_parts(
                    Translation3::from(dir * (s.seg_len * 0.5)),
                    arc_rotation(&dir),
                );
                ColliderBuilder::capsule_y(half_height, s.radius)
                    .position(iso)
                    .friction(0.7)
                    .build()
            } else {
                ColliderBuilder::ball(s.radius).friction(0.7).build()
            };
            self.collider_set
                .insert_with_parent(col, rb, &mut self.rigid_body_set);
        }

        let mut joints = Vec::new();
        for (i, s) in specs.iter().enumerate() {
            let Some(p) = s.parent else { continue };
            let parent_iso = spec_iso(&specs[p]);
            let anchor1 =
                parent_iso.inverse_transform_point(&Point3::new(s.pos[0], s.pos[1], s.pos[2]));
            let joint = SphericalJointBuilder::new()
                .local_anchor1(anchor1)
                .local_anchor2(Point::origin())
                .contacts_enabled(false);
            let h = self
                .impulse_joint_set
                .insert(rapier_bodies[p], rapier_bodies[i], joint, true);
            joints.push(h);
        }

        self.ragdolls.insert(owner, RagdollState { bodies, joints });
        true
    }

    pub fn ragdoll_bone_transforms(&self, owner: u64) -> Option<Vec<([f32; 3], [f32; 4])>> {
        let state = self.ragdolls.get(&owner)?;
        let mut out = Vec::with_capacity(state.bodies.len());
        for &body in &state.bodies {
            let (rb, _) = self.get_rapier_handle(body)?;
            let rb = self.rigid_body_set.get(rb)?;
            let t = rb.translation();
            let q = rb.rotation().coords;
            out.push(([t.x, t.y, t.z], [q.x, q.y, q.z, q.w]));
        }
        Some(out)
    }

    pub fn apply_ragdoll_impulse(&mut self, owner: u64, bone: usize, impulse: [f32; 3]) -> bool {
        let Some(body) = self
            .ragdolls
            .get(&owner)
            .and_then(|s| s.bodies.get(bone).copied())
        else {
            return false;
        };
        self.apply_impulse(body, impulse);
        true
    }

    pub fn destroy_ragdoll(&mut self, owner: u64) -> bool {
        let Some(state) = self.ragdolls.remove(&owner) else {
            return false;
        };
        for body in state.bodies {
            self.remove_rigid_body(body);
        }
        true
    }

    pub fn ragdoll_active(&self, owner: u64) -> bool {
        self.ragdolls.contains_key(&owner)
    }
}
