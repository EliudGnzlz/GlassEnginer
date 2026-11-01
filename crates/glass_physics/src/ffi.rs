use std::sync::{Arc, Mutex, OnceLock, RwLock};
use crate::types::{BodyHandle, BodyType, ColliderHandle};
use crate::world::PhysicsWorld;

static GLOBAL_PHYSICS: OnceLock<RwLock<Option<Arc<Mutex<PhysicsWorld>>>>> = OnceLock::new();

fn get_physics_cell() -> &'static RwLock<Option<Arc<Mutex<PhysicsWorld>>>> {
    GLOBAL_PHYSICS.get_or_init(|| RwLock::new(None))
}

pub fn register_global_physics_world(world: Arc<Mutex<PhysicsWorld>>) {
    if let Ok(mut cell) = get_physics_cell().write() {
        *cell = Some(world);
    }
}

pub fn get_global_physics_world() -> Option<Arc<Mutex<PhysicsWorld>>> {
    get_physics_cell().read().ok().and_then(|c| c.clone())
}

#[no_mangle]
pub extern "C" fn glass_physics_create_body(
    entity_id: u64,
    body_type: i32,
    pos_x: f32,
    pos_y: f32,
    pos_z: f32,
    rot_x: f32,
    rot_y: f32,
    rot_z: f32,
    rot_w: f32,
) -> BodyHandle {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            // If body already exists for this entity, return existing handle
            if let Some(existing) = world.get_body_by_entity(entity_id) {
                return existing;
            }
            return world.create_rigid_body(
                entity_id,
                BodyType::from_i32(body_type),
                [pos_x, pos_y, pos_z],
                [rot_x, rot_y, rot_z, rot_w],
            );
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn glass_physics_attach_box_collider(
    body_handle: BodyHandle,
    half_x: f32,
    half_y: f32,
    half_z: f32,
    restitution: f32,
    friction: f32,
) -> ColliderHandle {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            if let Some((rapier_handle, _)) = world.get_rapier_handle(body_handle) {
                if let Some(rb) = world.rigid_body_set.get(rapier_handle) {
                    if !rb.colliders().is_empty() {
                        return rb.colliders()[0].into_raw_parts().0 as u64;
                    }
                }
            }
            return world
                .attach_box_collider(body_handle, [half_x, half_y, half_z], restitution, friction)
                .unwrap_or(0);
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn glass_physics_attach_sphere_collider(
    body_handle: BodyHandle,
    radius: f32,
    restitution: f32,
    friction: f32,
) -> ColliderHandle {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world
                .attach_sphere_collider(body_handle, radius, restitution, friction)
                .unwrap_or(0);
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn glass_physics_apply_impulse(
    body_handle: BodyHandle,
    imp_x: f32,
    imp_y: f32,
    imp_z: f32,
) -> bool {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.apply_impulse(body_handle, [imp_x, imp_y, imp_z]);
        }
    }
    false
}

#[no_mangle]
pub extern "C" fn glass_physics_apply_force(
    body_handle: BodyHandle,
    force_x: f32,
    force_y: f32,
    force_z: f32,
) -> bool {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.apply_force(body_handle, [force_x, force_y, force_z]);
        }
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_linear_velocity(
    body_handle: BodyHandle,
    out_vel: *mut [f32; 3],
) -> bool {
    if out_vel.is_null() {
        return false;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(vel) = world.get_linear_velocity(body_handle) {
                *out_vel = vel;
                return true;
            }
        }
    }
    false
}

#[no_mangle]
pub extern "C" fn glass_physics_set_linear_velocity(
    body_handle: BodyHandle,
    vel_x: f32,
    vel_y: f32,
    vel_z: f32,
) -> bool {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_linear_velocity(body_handle, [vel_x, vel_y, vel_z]);
        }
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_transform(
    body_handle: BodyHandle,
    out_pos: *mut [f32; 3],
    out_rot: *mut [f32; 4],
) -> bool {
    if out_pos.is_null() || out_rot.is_null() {
        return false;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(tf) = world.get_transform(body_handle) {
                *out_pos = tf.position;
                *out_rot = tf.rotation;
                return true;
            }
        }
    }
    false
}

#[no_mangle]
pub extern "C" fn glass_physics_set_transform(
    body_handle: BodyHandle,
    pos_x: f32,
    pos_y: f32,
    pos_z: f32,
    rot_x: f32,
    rot_y: f32,
    rot_z: f32,
    rot_w: f32,
) -> bool {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_transform(
                body_handle,
                [pos_x, pos_y, pos_z],
                [rot_x, rot_y, rot_z, rot_w],
            );
        }
    }
    false
}

#[no_mangle]
pub extern "C" fn glass_physics_remove_body(body_handle: BodyHandle) -> bool {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.remove_rigid_body(body_handle);
        }
    }
    false
}
