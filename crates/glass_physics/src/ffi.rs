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

/// Frontera FFI: ningún float no finito (NaN/inf) debe llegar a Rapier,
/// donde envenenaría la simulación entera. Cada función que recibe floats
/// valida con esto y rechaza devolviendo su valor de fallo (0/false).
fn all_finite(vals: &[f32]) -> bool {
    vals.iter().all(|v| v.is_finite())
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
    if !all_finite(&[pos_x, pos_y, pos_z, rot_x, rot_y, rot_z, rot_w]) {
        log::error!("glass_physics_create_body: posición/rotación no finitas; cuerpo no creado");
        return 0;
    }
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
    if !all_finite(&[half_x, half_y, half_z, restitution, friction]) {
        log::error!("glass_physics_attach_box_collider: valores no finitos; collider no creado");
        return 0;
    }
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
    if !all_finite(&[radius, restitution, friction]) {
        log::error!("glass_physics_attach_sphere_collider: valores no finitos; collider no creado");
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world
                .attach_sphere_collider(body_handle, radius, restitution, friction)
                .unwrap_or(0);
        }
    }
    0
}

/// Adjunta un collider de CÁPSULA (eje Y) al cuerpo. `half_height` es
/// la mitad del tramo recto, sin casquetes (la semántica de Rapier).
/// 0 si el mundo o el cuerpo no existen o algún valor no es finito.
#[no_mangle]
pub extern "C" fn glass_physics_attach_capsule_collider(
    world: *mut PhysicsWorld,
    body_handle: BodyHandle,
    half_height: f32,
    radius: f32,
    restitution: f32,
    friction: f32,
) -> ColliderHandle {
    if world.is_null() || !all_finite(&[half_height, radius, restitution, friction]) || radius < 0.0 {
        return 0;
    }
    let world = unsafe { &mut *world };
    match world.attach_capsule_collider(body_handle, half_height, radius, restitution, friction) {
        Some(h) => h,
        None => 0,
    }
}

/// Adjunta un collider de CILINDRO (eje Y) al cuerpo. `half_height`
/// es la mitad de la altura total. Misma guarda que la cápsula.
#[no_mangle]
pub extern "C" fn glass_physics_attach_cylinder_collider(
    world: *mut PhysicsWorld,
    body_handle: BodyHandle,
    half_height: f32,
    radius: f32,
    restitution: f32,
    friction: f32,
) -> ColliderHandle {
    if world.is_null() || !all_finite(&[half_height, radius, restitution, friction]) || radius < 0.0 {
        return 0;
    }
    let world = unsafe { &mut *world };
    match world.attach_cylinder_collider(body_handle, half_height, radius, restitution, friction) {
        Some(h) => h,
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn glass_physics_apply_impulse(
    body_handle: BodyHandle,
    imp_x: f32,
    imp_y: f32,
    imp_z: f32,
) -> bool {
    if !all_finite(&[imp_x, imp_y, imp_z]) {
        log::error!("glass_physics_apply_impulse: impulso no finito; ignorado");
        return false;
    }
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
    if !all_finite(&[force_x, force_y, force_z]) {
        log::error!("glass_physics_apply_force: fuerza no finita; ignorada");
        return false;
    }
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
    if !all_finite(&[vel_x, vel_y, vel_z]) {
        log::error!("glass_physics_set_linear_velocity: velocidad no finita; ignorada");
        return false;
    }
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
    if !all_finite(&[pos_x, pos_y, pos_z, rot_x, rot_y, rot_z, rot_w]) {
        log::error!("glass_physics_set_transform: posición/rotación no finitas; ignorado");
        return false;
    }
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

/// Raycast contra los colliders del mundo (añadido para el juego
/// Plat3D: chequeo de suelo del jugador y golpe de espada). Devuelve
/// `true` si hubo impacto y rellena las salidas (cualquiera puede ser
/// null si al llamante no le interesa). `exclude_entity` = 0 no excluye.
#[no_mangle]
pub extern "C" fn glass_physics_cast_ray(
    origin_x: f32,
    origin_y: f32,
    origin_z: f32,
    dir_x: f32,
    dir_y: f32,
    dir_z: f32,
    max_dist: f32,
    exclude_entity: u64,
    out_entity: *mut u64,
    out_point_x: *mut f32,
    out_point_y: *mut f32,
    out_point_z: *mut f32,
    out_normal_x: *mut f32,
    out_normal_y: *mut f32,
    out_normal_z: *mut f32,
    out_distance: *mut f32,
) -> bool {
    if !all_finite(&[
        origin_x, origin_y, origin_z, dir_x, dir_y, dir_z, max_dist,
    ]) {
        log::error!("glass_physics_cast_ray: valores no finitos; sin impacto");
        return false;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            if let Some(hit) = world.cast_ray(
                [origin_x, origin_y, origin_z],
                [dir_x, dir_y, dir_z],
                max_dist,
                exclude_entity,
            ) {
                unsafe {
                    if !out_entity.is_null() {
                        *out_entity = hit.entity_id;
                    }
                    if !out_point_x.is_null() {
                        *out_point_x = hit.point[0];
                    }
                    if !out_point_y.is_null() {
                        *out_point_y = hit.point[1];
                    }
                    if !out_point_z.is_null() {
                        *out_point_z = hit.point[2];
                    }
                    if !out_normal_x.is_null() {
                        *out_normal_x = hit.normal[0];
                    }
                    if !out_normal_y.is_null() {
                        *out_normal_y = hit.normal[1];
                    }
                    if !out_normal_z.is_null() {
                        *out_normal_z = hit.normal[2];
                    }
                    if !out_distance.is_null() {
                        *out_distance = hit.distance;
                    }
                }
                return true;
            }
        }
    }
    false
}
