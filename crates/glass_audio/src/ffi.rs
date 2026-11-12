use std::ffi::{c_char, CStr};
use std::sync::{Arc, OnceLock, RwLock};
use crate::buses::BusId;
use crate::command::SoundHandle;
use crate::manager::AudioSystem;

static GLOBAL_AUDIO: OnceLock<RwLock<Option<Arc<AudioSystem>>>> = OnceLock::new();

fn get_audio_cell() -> &'static RwLock<Option<Arc<AudioSystem>>> {
    GLOBAL_AUDIO.get_or_init(|| RwLock::new(None))
}

pub fn register_global_audio_system(system: Arc<AudioSystem>) {
    if let Ok(mut cell) = get_audio_cell().write() {
        *cell = Some(system);
    }
}

pub fn get_global_audio_system() -> Option<Arc<AudioSystem>> {
    get_audio_cell().read().ok().and_then(|c| c.clone())
}

#[no_mangle]
pub unsafe extern "C" fn glass_audio_play_2d(
    clip_utf8: *const c_char,
    volume: f32,
    pitch: f32,
    looping: bool,
) -> SoundHandle {
    if clip_utf8.is_null() {
        return 0;
    }
    let clip_str = match CStr::from_ptr(clip_utf8).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    if let Some(sys) = get_global_audio_system() {
        sys.play_2d(clip_str, volume, pitch, looping)
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn glass_audio_play_3d(
    clip_utf8: *const c_char,
    pos_x: f32,
    pos_y: f32,
    pos_z: f32,
    volume: f32,
    pitch: f32,
    looping: bool,
) -> SoundHandle {
    if clip_utf8.is_null() {
        return 0;
    }
    let clip_str = match CStr::from_ptr(clip_utf8).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    if let Some(sys) = get_global_audio_system() {
        sys.play_3d(clip_str, [pos_x, pos_y, pos_z], volume, pitch, looping)
    } else {
        0
    }
}

/// Resuelve un nombre de bus del puente C# (auditoría C6): un nombre
/// desconocido avisa y cae a Sfx (antes el bus ni siquiera cruzaba).
fn bus_desde_puente(bus_utf8: *const c_char) -> BusId {
    if bus_utf8.is_null() {
        return BusId::Sfx;
    }
    match unsafe { CStr::from_ptr(bus_utf8) }.to_str() {
        Ok(nombre) => BusId::from_name(nombre).unwrap_or_else(|| {
            log::warn!("Audio: bus desconocido '{nombre}' desde C#; se usa Sfx.");
            BusId::Sfx
        }),
        Err(_) => BusId::Sfx,
    }
}

#[no_mangle]
pub unsafe extern "C" fn glass_audio_play_2d_on_bus(
    clip_utf8: *const c_char,
    bus_utf8: *const c_char,
    volume: f32,
    pitch: f32,
    looping: bool,
) -> SoundHandle {
    if clip_utf8.is_null() {
        return 0;
    }
    let clip_str = match CStr::from_ptr(clip_utf8).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let bus = bus_desde_puente(bus_utf8);
    if let Some(sys) = get_global_audio_system() {
        sys.play_2d_on_bus(clip_str, volume, pitch, looping, bus)
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn glass_audio_play_3d_on_bus(
    clip_utf8: *const c_char,
    bus_utf8: *const c_char,
    pos_x: f32,
    pos_y: f32,
    pos_z: f32,
    volume: f32,
    pitch: f32,
    looping: bool,
) -> SoundHandle {
    if clip_utf8.is_null() {
        return 0;
    }
    let clip_str = match CStr::from_ptr(clip_utf8).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let bus = bus_desde_puente(bus_utf8);
    if let Some(sys) = get_global_audio_system() {
        sys.play_3d_on_bus(clip_str, [pos_x, pos_y, pos_z], volume, pitch, looping, bus)
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn glass_audio_set_source_position(
    handle: SoundHandle,
    pos_x: f32,
    pos_y: f32,
    pos_z: f32,
) {
    if let Some(sys) = get_global_audio_system() {
        sys.set_source_position(handle, [pos_x, pos_y, pos_z]);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glass_audio_set_listener_transform(
    pos_x: f32,
    pos_y: f32,
    pos_z: f32,
    rot_x: f32,
    rot_y: f32,
    rot_z: f32,
    rot_w: f32,
) {
    if let Some(sys) = get_global_audio_system() {
        sys.set_listener_transform([pos_x, pos_y, pos_z], [rot_x, rot_y, rot_z, rot_w]);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glass_audio_stop(handle: SoundHandle) {
    if let Some(sys) = get_global_audio_system() {
        sys.stop(handle);
    }
}
