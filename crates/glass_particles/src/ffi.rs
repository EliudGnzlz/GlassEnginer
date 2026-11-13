//! Puente FFI para scripts C# (Fase 8P): las llamadas gestionadas
//! se convierten en comandos de la cola global, que la app drena
//! una vez por frame (mismo patrón que el audio).

/// `ParticleSystem.Play()` desde C#.
///
/// # Safety
/// No dereferencia punteros: solo encola un comando en la cola
/// global de partículas. Forma parte de la superficie FFI (por eso
/// es `unsafe extern`), pero admite cualquier `entity_id`.
#[no_mangle]
pub unsafe extern "C" fn glass_particles_play(entity_id: u64) {
    crate::push_command(crate::ParticleCommand::Play(entity_id));
}

/// `ParticleSystem.Stop()` desde C#.
///
/// # Safety
/// No dereferencia punteros: solo encola un comando en la cola
/// global de partículas. Forma parte de la superficie FFI (por eso
/// es `unsafe extern`), pero admite cualquier `entity_id`.
#[no_mangle]
pub unsafe extern "C" fn glass_particles_stop(entity_id: u64) {
    crate::push_command(crate::ParticleCommand::Stop(entity_id));
}

/// `ParticleSystem.Emit(count)` desde C#.
///
/// # Safety
/// No dereferencia punteros: solo encola un comando en la cola
/// global de partículas. Forma parte de la superficie FFI (por eso
/// es `unsafe extern`), pero admite cualquier valor.
#[no_mangle]
pub unsafe extern "C" fn glass_particles_burst(entity_id: u64, count: u32) {
    crate::push_command(crate::ParticleCommand::Burst(entity_id, count));
}
