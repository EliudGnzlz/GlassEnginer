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
///
/// # Safety
/// `world` debe ser null o un puntero válido a un `PhysicsWorld`
/// vivo entregado por el motor (la función comprueba el null).
#[no_mangle]
pub unsafe extern "C" fn glass_physics_attach_capsule_collider(
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
    world
        .attach_capsule_collider(body_handle, half_height, radius, restitution, friction)
        .unwrap_or_default()
}

/// Adjunta un collider de CILINDRO (eje Y) al cuerpo. `half_height`
/// es la mitad de la altura total. Misma guarda que la cápsula.
///
/// # Safety
/// `world` debe ser null o un puntero válido a un `PhysicsWorld`
/// vivo entregado por el motor (la función comprueba el null).
#[no_mangle]
pub unsafe extern "C" fn glass_physics_attach_cylinder_collider(
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
    world
        .attach_cylinder_collider(body_handle, half_height, radius, restitution, friction)
        .unwrap_or_default()
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

/// # Safety
/// `out_vel` debe ser null o apuntar a memoria válida y escribible
/// para un `[f32; 3]` durante la llamada (la función comprueba el null).
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

/// # Safety
/// `out_pos` y `out_rot` deben ser null o apuntar a memoria válida
/// y escribible para un `[f32; 3]` y un `[f32; 4]` respectivamente
/// durante la llamada (la función comprueba los null).
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
///
/// # Safety
/// Cada puntero de salida debe ser null o apuntar a memoria válida
/// y escribible para un valor de su tipo durante la llamada (la
/// función comprueba cada null antes de escribir).
#[no_mangle]
pub unsafe extern "C" fn glass_physics_cast_ray(
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

/// Handle del cuerpo de una entidad de la escena (Fase 9: los
/// scripts mueven cuerpos de escena por handle, pero no tenían
/// forma de resolver entidad → handle; el mapa ya existía en el
/// mundo para uso de la app). 0 si la entidad no tiene cuerpo.
#[no_mangle]
pub extern "C" fn glass_physics_body_handle(entity_id: u64) -> u64 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world.get_body_by_entity(entity_id).unwrap_or(0);
        }
    }
    0
}

/// Barrido de esfera contra los colliders del mundo (auditoría C5:
/// las consultas extendidas de la Fase 1 solo existían por MCP; esto
/// las abre a los scripts C#). Misma forma y salidas que el raycast.
///
/// # Safety
/// Cada puntero de salida debe ser null o apuntar a memoria válida
/// y escribible para un valor de su tipo durante la llamada (la
/// función comprueba cada null antes de escribir).
#[no_mangle]
pub unsafe extern "C" fn glass_physics_cast_sphere(
    origin_x: f32,
    origin_y: f32,
    origin_z: f32,
    radius: f32,
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
        origin_x, origin_y, origin_z, radius, dir_x, dir_y, dir_z, max_dist,
    ]) {
        log::error!("glass_physics_cast_sphere: valores no finitos; sin impacto");
        return false;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            if let Some(hit) = world.cast_sphere(
                [origin_x, origin_y, origin_z],
                radius,
                [dir_x, dir_y, dir_z],
                max_dist,
                u32::MAX,
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

/// Entidades cuyos colliders intersectan una esfera (auditoría C5).
/// Escribe hasta `capacity` ids en `out_entities` y devuelve cuántos
/// escribió; -1 si los argumentos no son válidos.
///
/// # Safety
/// `out_entities` debe ser null o apuntar a memoria válida y
/// escribible para al menos `capacity` valores `u64` durante la
/// llamada (la función comprueba el null).
#[no_mangle]
pub unsafe extern "C" fn glass_physics_overlap_sphere(
    center_x: f32,
    center_y: f32,
    center_z: f32,
    radius: f32,
    exclude_entity: u64,
    out_entities: *mut u64,
    capacity: u32,
) -> i32 {
    if !all_finite(&[center_x, center_y, center_z, radius]) || out_entities.is_null() {
        log::error!("glass_physics_overlap_sphere: argumentos no válidos");
        return -1;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            let found = world.overlap_sphere(
                [center_x, center_y, center_z],
                radius,
                u32::MAX,
                exclude_entity,
            );
            let n = found.len().min(capacity as usize);
            unsafe {
                for (i, id) in found.iter().take(n).enumerate() {
                    *out_entities.add(i) = *id;
                }
            }
            return n as i32;
        }
    }
    0
}

// ── CharacterController (bloque CC): personajes kinemáticos ─────────
// Espejo de los delegates de PhysicsDelegates.cs (Character*Delegate).

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_character_create(
    entity_id: u64,
    pos_x: f32, pos_y: f32, pos_z: f32,
    radius: f32, height: f32, skin: f32,
    step_offset: f32, slope_limit_deg: f32, snap: f32,
) -> i32 {
    if !all_finite(&[pos_x, pos_y, pos_z, radius, height, skin, step_offset, slope_limit_deg, snap]) {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            let cfg = crate::character::CharacterConfig {
                radius, height, skin, step_offset, slope_limit_deg, snap,
            };
            return world.create_character(entity_id, [pos_x, pos_y, pos_z], cfg) as i32;
        }
    }
    0
}

/// # Safety
/// `out_applied_x/y/z`, `out_grounded` y `out_sliding` deben ser null o
/// apuntar a memoria válida y escribible durante la llamada (la función
/// comprueba el null antes de escribir).
#[no_mangle]
pub unsafe extern "C" fn glass_character_move(
    entity_id: u64,
    dx: f32, dy: f32, dz: f32, dt: f32,
    out_ax: *mut f32, out_ay: *mut f32, out_az: *mut f32,
    out_grounded: *mut i32, out_sliding: *mut i32,
) -> i32 {
    if !all_finite(&[dx, dy, dz, dt]) {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            if let Some((applied, grounded, sliding)) =
                world.move_character(entity_id, [dx, dy, dz], dt)
            {
                unsafe {
                    if !out_ax.is_null() { *out_ax = applied[0]; }
                    if !out_ay.is_null() { *out_ay = applied[1]; }
                    if !out_az.is_null() { *out_az = applied[2]; }
                    if !out_grounded.is_null() { *out_grounded = grounded as i32; }
                    if !out_sliding.is_null() { *out_sliding = sliding as i32; }
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_character_teleport(
    entity_id: u64, pos_x: f32, pos_y: f32, pos_z: f32,
) -> i32 {
    if !all_finite(&[pos_x, pos_y, pos_z]) {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.teleport_character(entity_id, [pos_x, pos_y, pos_z]) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_character_destroy(entity_id: u64) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.destroy_character(entity_id) as i32;
        }
    }
    0
}

/// # Safety
/// `out_pos_x/y/z`, `out_grounded` y `out_sliding` deben ser null o
/// apuntar a memoria válida y escribible durante la llamada (la función
/// comprueba el null antes de escribir).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CharacterHitFfi {
    pub entity: u64,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub move_direction: [f32; 3],
    pub move_length: f32,
}

/// Escribe flags y velocidad del último Move, copia hasta `cap` hits y devuelve
/// el total de hits (puede exceder `cap`). Sin personaje: devuelve 0 y flags 0.
///
/// # Safety
/// `out_flags` debe ser válido para escribir un `u32`, `out_vel` para 3 `f32`
/// y `out_hits` para `cap` elementos; cualquiera puede ser nulo («solo contar»).
#[no_mangle]
pub unsafe extern "C" fn glass_character_last_move(
    entity_id: u64,
    out_flags: *mut u32,
    out_vel: *mut f32,
    out_hits: *mut CharacterHitFfi,
    cap: u32,
) -> u32 {
    // Mismo acceso al mundo global que glass_character_info.
    let datos = match get_global_physics_world() {
        Some(sys) => match sys.lock() {
            Ok(world) => world.character_last_move(entity_id),
            Err(_) => None,
        },
        None => None,
    };
    let Some((flags, vel, hits)) = datos else {
        unsafe {
            if !out_flags.is_null() {
                *out_flags = 0;
            }
            if !out_vel.is_null() {
                std::ptr::write_bytes(out_vel, 0, 3);
            }
        }
        return 0;
    };
    unsafe {
        if !out_flags.is_null() {
            *out_flags = flags;
        }
        if !out_vel.is_null() {
            std::ptr::copy_nonoverlapping(vel.as_ptr(), out_vel, 3);
        }
        if !out_hits.is_null() {
            for (i, h) in hits.iter().take(cap as usize).enumerate() {
                out_hits.add(i).write(CharacterHitFfi {
                    entity: h.entity,
                    point: h.point,
                    normal: h.normal,
                    move_direction: h.move_direction,
                    move_length: h.move_length,
                });
            }
        }
    }
    hits.len() as u32
}

/// Posición y estado de apoyo del personaje de una entidad.
///
/// # Safety
/// Los punteros de salida deben ser nulos o apuntar a memoria
/// válida y escribible durante la llamada.
#[no_mangle]
pub unsafe extern "C" fn glass_character_info(
    entity_id: u64,
    out_x: *mut f32, out_y: *mut f32, out_z: *mut f32,
    out_grounded: *mut i32, out_sliding: *mut i32,
) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some((pos, grounded, sliding)) = world.character_info(entity_id) {
                unsafe {
                    if !out_x.is_null() { *out_x = pos[0]; }
                    if !out_y.is_null() { *out_y = pos[1]; }
                    if !out_z.is_null() { *out_z = pos[2]; }
                    if !out_grounded.is_null() { *out_grounded = grounded as i32; }
                    if !out_sliding.is_null() { *out_sliding = sliding as i32; }
                }
                return 1;
            }
        }
    }
    0
}

// ── Paquete C: setters/getters de paridad RB/CC sobre CharacterController ──

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_character_set_detect_collisions(entity_id: u64, enabled: i32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_character_detect_collisions(entity_id, enabled != 0) as i32;
        }
    }
    0
}

/// # Safety
/// `out` debe ser null o apuntar a memoria válida y escribible durante la llamada.
#[no_mangle]
pub unsafe extern "C" fn glass_character_get_detect_collisions(entity_id: u64, out: *mut i32) -> i32 {
    if !out.is_null() {
        unsafe { *out = 0; }
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(v) = world.get_character_detect_collisions(entity_id) {
                unsafe {
                    if !out.is_null() { *out = v as i32; }
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_character_set_overlap_recovery(entity_id: u64, enabled: i32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_character_overlap_recovery(entity_id, enabled != 0) as i32;
        }
    }
    0
}

/// # Safety
/// `out` debe ser null o apuntar a memoria válida y escribible durante la llamada.
#[no_mangle]
pub unsafe extern "C" fn glass_character_get_overlap_recovery(entity_id: u64, out: *mut i32) -> i32 {
    if !out.is_null() {
        unsafe { *out = 0; }
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(v) = world.get_character_overlap_recovery(entity_id) {
                unsafe {
                    if !out.is_null() { *out = v as i32; }
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_character_set_step_offset(entity_id: u64, v: f32) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_character_step_offset(entity_id, v) as i32;
        }
    }
    0
}

/// CC-1: minMoveDistance. Devuelve 1 si el personaje existe y el valor es
/// válido (finito y >= 0; 0 desactiva).
///
/// # Safety
/// Sin punteros; se llama desde el host por FFI.
#[no_mangle]
pub unsafe extern "C" fn glass_character_set_min_move_distance(entity_id: u64, v: f32) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_character_min_move_distance(entity_id, v) as i32;
        }
    }
    0
}

/// CC-1: lee minMoveDistance. Escribe 0.0 por defecto y devuelve 1 si
/// escribió el valor del personaje.
///
/// # Safety
/// `out` debe ser nulo o apuntar a un `f32` válido y escribible.
#[no_mangle]
pub unsafe extern "C" fn glass_character_get_min_move_distance(entity_id: u64, out: *mut f32) -> i32 {
    if out.is_null() {
        return 0;
    }
    // SAFETY: `out` no es nulo y el llamador garantiza que es escribible.
    unsafe { *out = 0.0 };
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(v) = world.get_character_min_move_distance(entity_id) {
                // SAFETY: ver arriba.
                unsafe { *out = v };
                return 1;
            }
        }
    }
    0
}

/// CC-1: center. Desplaza la cápsula en (nuevo − anterior). Devuelve 1 si
/// el personaje existe y los valores son finitos.
///
/// # Safety
/// Sin punteros; se llama desde el host por FFI.
#[no_mangle]
pub unsafe extern "C" fn glass_character_set_center(entity_id: u64, x: f32, y: f32, z: f32) -> i32 {
    if !(x.is_finite() && y.is_finite() && z.is_finite()) {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_character_center(entity_id, [x, y, z]) as i32;
        }
    }
    0
}

/// CC-1: lee center. Escribe 0.0 por defecto en los tres ejes y devuelve 1
/// si escribió el valor del personaje.
///
/// # Safety
/// `out_x`, `out_y` y `out_z` deben ser no nulos y apuntar a `f32` válidos
/// y escribibles.
#[no_mangle]
pub unsafe extern "C" fn glass_character_get_center(
    entity_id: u64,
    out_x: *mut f32,
    out_y: *mut f32,
    out_z: *mut f32,
) -> i32 {
    if out_x.is_null() || out_y.is_null() || out_z.is_null() {
        return 0;
    }
    // SAFETY: punteros no nulos y escribibles según el contrato.
    unsafe {
        *out_x = 0.0;
        *out_y = 0.0;
        *out_z = 0.0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(c) = world.get_character_center(entity_id) {
                // SAFETY: ver arriba.
                unsafe {
                    *out_x = c[0];
                    *out_y = c[1];
                    *out_z = c[2];
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// `out` debe ser null o apuntar a memoria válida y escribible durante la llamada.
#[no_mangle]
pub unsafe extern "C" fn glass_character_get_step_offset(entity_id: u64, out: *mut f32) -> i32 {
    if !out.is_null() {
        unsafe { *out = 0.0; }
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(v) = world.get_character_step_offset(entity_id) {
                unsafe {
                    if !out.is_null() { *out = v; }
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_character_set_slope_limit(entity_id: u64, deg: f32) -> i32 {
    if !deg.is_finite() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_character_slope_limit(entity_id, deg) as i32;
        }
    }
    0
}

/// # Safety
/// `out` debe ser null o apuntar a memoria válida y escribible durante la llamada.
#[no_mangle]
pub unsafe extern "C" fn glass_character_get_slope_limit(entity_id: u64, out: *mut f32) -> i32 {
    if !out.is_null() {
        unsafe { *out = 0.0; }
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(v) = world.get_character_slope_limit(entity_id) {
                unsafe {
                    if !out.is_null() { *out = v; }
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_character_set_enabled(entity_id: u64, enabled: i32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_character_enabled(entity_id, enabled != 0) as i32;
        }
    }
    0
}

/// # Safety
/// `out` debe ser null o apuntar a memoria válida y escribible durante la llamada.
#[no_mangle]
pub unsafe extern "C" fn glass_character_get_enabled(entity_id: u64, out: *mut i32) -> i32 {
    if !out.is_null() {
        unsafe { *out = 0; }
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(v) = world.get_character_enabled(entity_id) {
                unsafe {
                    if !out.is_null() { *out = v as i32; }
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_character_set_include_layers(entity_id: u64, mask: u32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_character_include_layers(entity_id, mask) as i32;
        }
    }
    0
}

/// # Safety
/// `out` debe ser null o apuntar a memoria válida y escribible durante la llamada.
#[no_mangle]
pub unsafe extern "C" fn glass_character_get_include_layers(entity_id: u64, out: *mut u32) -> i32 {
    if !out.is_null() {
        unsafe { *out = 0; }
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(v) = world.get_character_include_layers(entity_id) {
                unsafe {
                    if !out.is_null() { *out = v; }
                }
                return 1;
            }
        }
    }
    0
}

/// Caja de la cápsula del personaje: escribe 3 floats en `out_min` y 3 en `out_max`.
///
/// # Safety
/// `out_min` y `out_max` deben ser null o apuntar a memoria válida y
/// escribible para 3 `f32` cada uno durante la llamada.
#[no_mangle]
pub unsafe extern "C" fn glass_character_bounds(
    entity_id: u64,
    out_min: *mut f32,
    out_max: *mut f32,
) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some((min, max)) = world.character_bounds(entity_id) {
                unsafe {
                    if !out_min.is_null() {
                        std::ptr::copy_nonoverlapping(min.as_ptr(), out_min, 3);
                    }
                    if !out_max.is_null() {
                        std::ptr::copy_nonoverlapping(max.as_ptr(), out_max, 3);
                    }
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_locked_axes(body_handle: u64, bits: u32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_locked_axes(body_handle, bits as u8) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_mass(body_handle: u64, mass: f32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_mass(body_handle, mass) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
/// Devuelve -1.0 si el cuerpo no existe.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_mass(body_handle: u64) -> f32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world.get_mass(body_handle).unwrap_or(-1.0);
        }
    }
    -1.0
}

/// # Safety
/// `out` debe ser nulo o apuntar a un `[f32; 3]` válido y escribible.
/// Devuelve 0 y no escribe si `out` es nulo o el cuerpo no existe.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_angular_velocity(body_handle: u64, out: *mut f32) -> i32 {
    if out.is_null() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(angvel) = world.get_angular_velocity(body_handle) {
                let out = std::slice::from_raw_parts_mut(out, 3);
                out.copy_from_slice(&angvel);
                return 1;
            }
        }
    }
    0
}

/// automaticCenterOfMass (RB/CC 100% RB-4). `value != 0` = true. 1 ok, 0 fallo.
#[no_mangle]
pub extern "C" fn glass_physics_set_automatic_center_of_mass(body_handle: u64, value: i32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_automatic_center_of_mass(body_handle, value != 0) as i32;
        }
    }
    0
}

/// automaticCenterOfMass (RB/CC 100% RB-4). -1 no existe, 0 false, 1 true.
#[no_mangle]
pub extern "C" fn glass_physics_get_automatic_center_of_mass(body_handle: u64) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world
                .get_automatic_center_of_mass(body_handle)
                .map_or(-1, i32::from);
        }
    }
    -1
}

/// automaticInertiaTensor (RB/CC 100% RB-4). `value != 0` = true. 1 ok, 0 fallo.
#[no_mangle]
pub extern "C" fn glass_physics_set_automatic_inertia_tensor(body_handle: u64, value: i32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_automatic_inertia_tensor(body_handle, value != 0) as i32;
        }
    }
    0
}

/// automaticInertiaTensor (RB/CC 100% RB-4). -1 no existe, 0 false, 1 true.
#[no_mangle]
pub extern "C" fn glass_physics_get_automatic_inertia_tensor(body_handle: u64) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world
                .get_automatic_inertia_tensor(body_handle)
                .map_or(-1, i32::from);
        }
    }
    -1
}

/// ClosestPointOnBounds (RB/CC 100% RB-4): escribe en `out` el punto más
/// cercano a (x, y, z) sobre los AABB de los colliders del cuerpo. 1 ok, 0 fallo.
///
/// # Safety
/// `out` debe ser nulo o apuntar a un buffer válido y escribible de al menos 3 `f32`.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_closest_point_on_bounds(
    body_handle: u64,
    x: f32,
    y: f32,
    z: f32,
    out: *mut f32,
) -> i32 {
    if out.is_null() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(punto) = world.closest_point_on_bounds(body_handle, [x, y, z]) {
                let out = std::slice::from_raw_parts_mut(out, 3);
                out.copy_from_slice(&punto);
                return 1;
            }
        }
    }
    0
}

/// Fuerza en coordenadas locales del cuerpo, rotada a mundo con su rotación actual.
/// Devuelve true si el cuerpo existe.
/// # Safety
/// (Sin punteros: las tres componentes van por valor, como `glass_physics_apply_force`.)
#[no_mangle]
pub extern "C" fn glass_physics_add_relative_force(
    body_handle: u64,
    force_x: f32,
    force_y: f32,
    force_z: f32,
) -> bool {
    if !all_finite(&[force_x, force_y, force_z]) {
        log::error!("glass_physics_add_relative_force: fuerza no finita; ignorada");
        return false;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.add_relative_force(body_handle, [force_x, force_y, force_z]);
        }
    }
    false
}

/// Impulso en coordenadas locales del cuerpo, rotado a mundo con su rotación actual.
/// Devuelve true si el cuerpo existe.
/// # Safety
/// (Sin punteros: las tres componentes van por valor.)
#[no_mangle]
pub extern "C" fn glass_physics_apply_relative_impulse(
    body_handle: u64,
    impulse_x: f32,
    impulse_y: f32,
    impulse_z: f32,
) -> bool {
    if !all_finite(&[impulse_x, impulse_y, impulse_z]) {
        log::error!("glass_physics_apply_relative_impulse: impulso no finito; ignorado");
        return false;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.apply_relative_impulse(body_handle, [impulse_x, impulse_y, impulse_z]);
        }
    }
    false
}

/// Fuerza de usuario acumulada (mundo). Escribe [0,0,0] y devuelve 0 si no existe el cuerpo.
/// # Safety
/// `out` debe ser null o apuntar a 3 `f32` escribibles.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_accumulated_force(body_handle: u64, out: *mut f32) -> i32 {
    if out.is_null() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            let out = std::slice::from_raw_parts_mut(out, 3);
            match world.get_accumulated_force(body_handle) {
                Some(v) => {
                    out.copy_from_slice(&v);
                    return 1;
                }
                None => {
                    out.copy_from_slice(&[0.0; 3]);
                    return 0;
                }
            }
        }
    }
    0
}

/// Torque de usuario acumulado (mundo). Escribe [0,0,0] y devuelve 0 si no existe el cuerpo.
/// # Safety
/// `out` debe ser null o apuntar a 3 `f32` escribibles.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_accumulated_torque(body_handle: u64, out: *mut f32) -> i32 {
    if out.is_null() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            let out = std::slice::from_raw_parts_mut(out, 3);
            match world.get_accumulated_torque(body_handle) {
                Some(v) => {
                    out.copy_from_slice(&v);
                    return 1;
                }
                None => {
                    out.copy_from_slice(&[0.0; 3]);
                    return 0;
                }
            }
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_angular_velocity(
    body_handle: u64,
    x: f32,
    y: f32,
    z: f32,
) -> i32 {
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_angular_velocity(body_handle, [x, y, z]) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_add_torque(body_handle: u64, x: f32, y: f32, z: f32) -> i32 {
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.add_torque(body_handle, [x, y, z]) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_apply_torque_impulse(
    body_handle: u64,
    x: f32,
    y: f32,
    z: f32,
) -> i32 {
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.apply_torque_impulse(body_handle, [x, y, z]) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_add_force_at_point(
    body_handle: u64,
    fx: f32,
    fy: f32,
    fz: f32,
    px: f32,
    py: f32,
    pz: f32,
) -> i32 {
    if !fx.is_finite() || !fy.is_finite() || !fz.is_finite()
        || !px.is_finite() || !py.is_finite() || !pz.is_finite()
    {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.add_force_at_point(body_handle, [fx, fy, fz], [px, py, pz]) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_apply_impulse_at_point(
    body_handle: u64,
    ix: f32,
    iy: f32,
    iz: f32,
    px: f32,
    py: f32,
    pz: f32,
) -> i32 {
    if !ix.is_finite() || !iy.is_finite() || !iz.is_finite()
        || !px.is_finite() || !py.is_finite() || !pz.is_finite()
    {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.apply_impulse_at_point(body_handle, [ix, iy, iz], [px, py, pz]) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_explosion_impulse(
    body_handle: u64,
    cx: f32,
    cy: f32,
    cz: f32,
    radius: f32,
    force: f32,
    upwards: f32,
) -> i32 {
    if !cx.is_finite() || !cy.is_finite() || !cz.is_finite()
        || !radius.is_finite() || !force.is_finite() || !upwards.is_finite()
    {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.explosion_impulse(body_handle, [cx, cy, cz], radius, force, upwards) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_explosion_force(
    body_handle: u64,
    cx: f32,
    cy: f32,
    cz: f32,
    radius: f32,
    force: f32,
    upwards: f32,
) -> i32 {
    if !cx.is_finite() || !cy.is_finite() || !cz.is_finite()
        || !radius.is_finite() || !force.is_finite() || !upwards.is_finite()
    {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.explosion_force(body_handle, [cx, cy, cz], radius, force, upwards) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_velocity_caps(
    body_handle: u64,
    max_linear: f32,
    max_angular: f32,
) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            world.set_velocity_caps(body_handle, max_linear, max_angular);
            return 1;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_sleep(body_handle: u64) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.sleep_body(body_handle) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_wake(body_handle: u64) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.wake_body(body_handle) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
/// Devuelve 1 si el cuerpo está durmiendo, 0 si está despierto o si el
/// cuerpo no existe (ambos casos son indistinguibles con este valor de
/// retorno).
#[no_mangle]
pub unsafe extern "C" fn glass_physics_is_sleeping(body_handle: u64) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world.is_sleeping(body_handle).unwrap_or(false) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_sleep_threshold(body_handle: u64, threshold: f32) -> i32 {
    if !threshold.is_finite() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_sleep_threshold(body_handle, threshold) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_detect_collisions(body_handle: u64, enabled: i32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_detect_collisions(body_handle, enabled != 0) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_linear_damping(body_handle: u64, damping: f32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_linear_damping(body_handle, damping) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
/// Devuelve -1.0 si el cuerpo no existe.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_linear_damping(body_handle: u64) -> f32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world.get_linear_damping(body_handle).unwrap_or(-1.0);
        }
    }
    -1.0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_angular_damping(body_handle: u64, damping: f32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_angular_damping(body_handle, damping) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
/// Devuelve -1.0 si el cuerpo no existe.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_angular_damping(body_handle: u64) -> f32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world.get_angular_damping(body_handle).unwrap_or(-1.0);
        }
    }
    -1.0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
/// Fija la velocidad máxima de despenetración (m/s); no finito o negativo = sin límite.
/// Devuelve 1 si el cuerpo existe, 0 si no.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_max_depenetration_velocity(body_handle: u64, max: f32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_max_depenetration_velocity(body_handle, max) as i32;
        }
    }
    0
}

/// Fija includeLayers del cuerpo. 1 si existe, 0 si no.
#[no_mangle]
pub extern "C" fn glass_physics_set_include_layers(body_handle: u64, mask: u32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_include_layers(body_handle, mask) as i32;
        }
    }
    0
}

/// includeLayers del cuerpo extendido a i64 (conserva el bit 31). -1 si no existe.
#[no_mangle]
pub extern "C" fn glass_physics_get_include_layers(body_handle: u64) -> i64 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world.get_include_layers(body_handle).map_or(-1, i64::from);
        }
    }
    -1
}

/// Fija excludeLayers del cuerpo. 1 si existe, 0 si no.
#[no_mangle]
pub extern "C" fn glass_physics_set_exclude_layers(body_handle: u64, mask: u32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_exclude_layers(body_handle, mask) as i32;
        }
    }
    0
}

/// excludeLayers del cuerpo extendido a i64 (conserva el bit 31). -1 si no existe.
#[no_mangle]
pub extern "C" fn glass_physics_get_exclude_layers(body_handle: u64) -> i64 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world.get_exclude_layers(body_handle).map_or(-1, i64::from);
        }
    }
    -1
}

/// Fija solverVelocityIterations (comparte contador con solverIterations; manda el máximo).
#[no_mangle]
pub extern "C" fn glass_physics_set_solver_velocity_iterations(body_handle: u64, iterations: i32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_solver_velocity_iterations(body_handle, iterations) as i32;
        }
    }
    0
}

/// solverVelocityIterations guardado del cuerpo. -1 si no existe.
#[no_mangle]
pub extern "C" fn glass_physics_get_solver_velocity_iterations(body_handle: u64) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world.get_solver_velocity_iterations(body_handle).unwrap_or(-1);
        }
    }
    -1
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
/// Devuelve la velocidad máxima de despenetración (INFINITY = sin límite) o -1.0 si no existe el cuerpo.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_max_depenetration_velocity(body_handle: u64) -> f32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world.get_max_depenetration_velocity(body_handle).unwrap_or(-1.0);
        }
    }
    -1.0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
/// `iterations` es el total del solver para el cuerpo; negativos cuentan como 0.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_solver_iterations(body_handle: u64, iterations: i32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_solver_iterations(body_handle, iterations) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
/// Devuelve -1 si el cuerpo no existe.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_solver_iterations(body_handle: u64) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world.get_solver_iterations(body_handle).unwrap_or(-1);
        }
    }
    -1
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_gravity_scale(body_handle: u64, scale: f32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_gravity_scale(body_handle, scale) as i32;
        }
    }
    0
}

/// # Safety
/// Requiere el mundo global inicializado. `entity_id` es la ENTIDAD, no el
/// handle de cuerpo. `enabled != 0` activa la interpolación.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_interpolation(entity_id: u64, enabled: i32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            world.set_interpolation(entity_id, enabled != 0);
            return 1;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
/// Devuelve -1.0 si el cuerpo no existe.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_gravity_scale(body_handle: u64) -> f32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return world.get_gravity_scale(body_handle).unwrap_or(-1.0);
        }
    }
    -1.0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
/// `enabled` distinto de 0 activa el CCD.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_ccd_enabled(body_handle: u64, enabled: i32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_ccd_enabled(body_handle, enabled != 0) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
/// Devuelve 1 si el CCD está activo, 0 si no, -1 si el cuerpo no existe.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_ccd_enabled(body_handle: u64) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            return match world.ccd_enabled(body_handle) {
                Some(true) => 1,
                Some(false) => 0,
                None => -1,
            };
        }
    }
    -1
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
/// `body_type`: 0 = Dynamic, 1 = Static, 2 = Kinematic (enum C# RigidBodyType).
/// Devuelve 0 si el valor no es válido.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_body_type(body_handle: u64, body_type: i32) -> i32 {
    let tipo = match body_type {
        0 => crate::types::BodyType::Dynamic,
        1 => crate::types::BodyType::Static,
        2 => crate::types::BodyType::Kinematic,
        _ => return 0,
    };
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_body_type(body_handle, tipo) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_move_position(body_handle: u64, x: f32, y: f32, z: f32) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.move_position(body_handle, [x, y, z]) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_locked_axes(body_handle: u64) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(bits) = world.get_locked_axes(body_handle) {
                return bits as i32;
            }
        }
    }
    -1
}

/// # Safety
/// `out` debe ser null o apuntar a memoria válida y escribible para 3 floats.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_center_of_mass(body_handle: u64, out: *mut f32) -> i32 {
    if out.is_null() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(com) = world.get_center_of_mass(body_handle) {
                unsafe {
                    *out.add(0) = com[0];
                    *out.add(1) = com[1];
                    *out.add(2) = com[2];
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// `out` debe ser null o apuntar a memoria válida y escribible para 3 floats.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_world_center_of_mass(body_handle: u64, out: *mut f32) -> i32 {
    if out.is_null() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(com) = world.get_world_center_of_mass(body_handle) {
                unsafe {
                    *out.add(0) = com[0];
                    *out.add(1) = com[1];
                    *out.add(2) = com[2];
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_center_of_mass(
    body_handle: u64,
    x: f32,
    y: f32,
    z: f32,
) -> i32 {
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_center_of_mass(body_handle, [x, y, z]) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_reset_center_of_mass(body_handle: u64) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.reset_center_of_mass(body_handle) as i32;
        }
    }
    0
}

/// # Safety
/// `out_tensor` debe ser null o apuntar a memoria válida y escribible para 3
/// floats; `out_rot` debe ser null o apuntar a memoria válida y escribible
/// para 4 floats.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_inertia_tensor(
    body_handle: u64,
    out_tensor: *mut f32,
    out_rot: *mut f32,
) -> i32 {
    if out_tensor.is_null() || out_rot.is_null() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some((tensor, rot)) = world.get_inertia_tensor(body_handle) {
                unsafe {
                    *out_tensor.add(0) = tensor[0];
                    *out_tensor.add(1) = tensor[1];
                    *out_tensor.add(2) = tensor[2];
                    *out_rot.add(0) = rot[0];
                    *out_rot.add(1) = rot[1];
                    *out_rot.add(2) = rot[2];
                    *out_rot.add(3) = rot[3];
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_set_inertia_tensor(
    body_handle: u64,
    ix: f32,
    iy: f32,
    iz: f32,
    qx: f32,
    qy: f32,
    qz: f32,
    qw: f32,
) -> i32 {
    if !ix.is_finite() || !iy.is_finite() || !iz.is_finite()
        || !qx.is_finite() || !qy.is_finite() || !qz.is_finite() || !qw.is_finite()
    {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.set_inertia_tensor(body_handle, [ix, iy, iz], [qx, qy, qz, qw]) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_reset_inertia_tensor(body_handle: u64) -> i32 {
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.reset_inertia_tensor(body_handle) as i32;
        }
    }
    0
}

/// # Safety
/// `out` debe ser null o apuntar a memoria válida y escribible para 3 floats.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_point_velocity(
    body_handle: u64,
    px: f32,
    py: f32,
    pz: f32,
    out: *mut f32,
) -> i32 {
    if out.is_null() || !px.is_finite() || !py.is_finite() || !pz.is_finite() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(v) = world.get_point_velocity(body_handle, [px, py, pz]) {
                unsafe {
                    *out.add(0) = v[0];
                    *out.add(1) = v[1];
                    *out.add(2) = v[2];
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// `out` debe ser null o apuntar a memoria válida y escribible para 3 floats.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_get_relative_point_velocity(
    body_handle: u64,
    px: f32,
    py: f32,
    pz: f32,
    out: *mut f32,
) -> i32 {
    if out.is_null() || !px.is_finite() || !py.is_finite() || !pz.is_finite() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(world) = sys.lock() {
            if let Some(v) = world.get_relative_point_velocity(body_handle, [px, py, pz]) {
                unsafe {
                    *out.add(0) = v[0];
                    *out.add(1) = v[1];
                    *out.add(2) = v[2];
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_move_kinematic(
    body_handle: u64,
    px: f32,
    py: f32,
    pz: f32,
    qx: f32,
    qy: f32,
    qz: f32,
    qw: f32,
) -> i32 {
    if !px.is_finite() || !py.is_finite() || !pz.is_finite()
        || !qx.is_finite() || !qy.is_finite() || !qz.is_finite() || !qw.is_finite()
    {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.move_kinematic(body_handle, [px, py, pz], [qx, qy, qz, qw]) as i32;
        }
    }
    0
}

/// # Safety
/// No recibe punteros; es segura de llamar con cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_physics_move_kinematic_rotation(
    body_handle: u64,
    qx: f32,
    qy: f32,
    qz: f32,
    qw: f32,
) -> i32 {
    if !qx.is_finite() || !qy.is_finite() || !qz.is_finite() || !qw.is_finite() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            return world.move_kinematic_rotation(body_handle, [qx, qy, qz, qw]) as i32;
        }
    }
    0
}

/// # Safety
/// `out_entity`, `out_point`, `out_normal` y `out_dist` deben ser null o
/// apuntar a memoria válida y escribible (u64, 3 floats, 3 floats, 1 float
/// respectivamente).
#[no_mangle]
pub unsafe extern "C" fn glass_physics_sweep_body(
    body_handle: u64,
    dx: f32,
    dy: f32,
    dz: f32,
    max_dist: f32,
    out_entity: *mut u64,
    out_point: *mut f32,
    out_normal: *mut f32,
    out_dist: *mut f32,
) -> i32 {
    if !dx.is_finite() || !dy.is_finite() || !dz.is_finite() || !max_dist.is_finite() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            if let Some(hit) = world.sweep_body(body_handle, [dx, dy, dz], max_dist) {
                unsafe {
                    if !out_entity.is_null() {
                        *out_entity = hit.entity_id;
                    }
                    if !out_point.is_null() {
                        *out_point.add(0) = hit.point[0];
                        *out_point.add(1) = hit.point[1];
                        *out_point.add(2) = hit.point[2];
                    }
                    if !out_normal.is_null() {
                        *out_normal.add(0) = hit.normal[0];
                        *out_normal.add(1) = hit.normal[1];
                        *out_normal.add(2) = hit.normal[2];
                    }
                    if !out_dist.is_null() {
                        *out_dist = hit.distance;
                    }
                }
                return 1;
            }
        }
    }
    0
}

/// # Safety
/// `out_entities` y `out_dists` deben ser null o apuntar a memoria válida y
/// escribible para al menos `cap` elementos (u64 y f32 respectivamente).
#[no_mangle]
pub unsafe extern "C" fn glass_physics_sweep_body_all(
    body_handle: u64,
    dx: f32,
    dy: f32,
    dz: f32,
    max_dist: f32,
    out_entities: *mut u64,
    out_dists: *mut f32,
    cap: u32,
) -> u32 {
    if !dx.is_finite() || !dy.is_finite() || !dz.is_finite() || !max_dist.is_finite() {
        return 0;
    }
    if out_entities.is_null() || out_dists.is_null() {
        return 0;
    }
    if let Some(sys) = get_global_physics_world() {
        if let Ok(mut world) = sys.lock() {
            let hits = world.sweep_body_all(body_handle, [dx, dy, dz], max_dist);
            let n = hits.len().min(cap as usize);
            unsafe {
                for (i, hit) in hits.iter().take(n).enumerate() {
                    *out_entities.add(i) = hit.entity_id;
                    *out_dists.add(i) = hit.distance;
                }
            }
            return n as u32;
        }
    }
    0
}
