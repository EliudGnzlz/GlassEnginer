use std::ffi::{c_char, CStr};

pub use glass_core::TransformFfi;

// Compile-time sanity check for memory alignment and size
const _: () = {
    assert!(std::mem::size_of::<TransformFfi>() == 40);
    assert!(std::mem::align_of::<TransformFfi>() == 4);
};

pub const BRIDGE_OK: i32 = 0;
pub const BRIDGE_ERR_NOT_FOUND: i32 = -1;
pub const BRIDGE_ERR_NULL_PTR: i32 = -2;

/// Native callback invoked by C# to query an entity's transform.
pub unsafe extern "C" fn get_transform(entity_id: u64, out: *mut TransformFfi) -> i32 {
    if out.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    match crate::bridge::read_entity_transform(entity_id) {
        Some(t) => {
            unsafe { *out = t };
            BRIDGE_OK
        }
        None => BRIDGE_ERR_NOT_FOUND,
    }
}

/// Native callback invoked by C# to mutate an entity's transform.
pub unsafe extern "C" fn set_transform(entity_id: u64, input: *const TransformFfi) -> i32 {
    if input.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    let transform = unsafe { *input };
    if crate::bridge::write_entity_transform(entity_id, transform) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

/// Native callback invoked by C# to pipe logs directly into Rust log subsystem.
pub unsafe extern "C" fn log_message(level: i32, msg: *const c_char) {
    if msg.is_null() {
        return;
    }
    let s = unsafe { CStr::from_ptr(msg) };
    let text = s.to_string_lossy();
    match level {
        0 => log::trace!("[C#] {}", text),
        1 => log::debug!("[C#] {}", text),
        2 => log::info!("[C#] {}", text),
        3 => log::warn!("[C#] {}", text),
        _ => log::error!("[C#] {}", text),
    }
}

use std::sync::Mutex;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct InputDataFfi {
    pub move_x: f32, // -1.0 (A) .. +1.0 (D)
    pub move_z: f32, // -1.0 (S) .. +1.0 (W)
    pub jump: u8,
    pub run: u8,
}

static GLOBAL_INPUT: Mutex<InputDataFfi> = Mutex::new(InputDataFfi {
    move_x: 0.0,
    move_z: 0.0,
    jump: 0,
    run: 0,
});

pub fn set_input_state(input: InputDataFfi) {
    if let Ok(mut g) = GLOBAL_INPUT.lock() {
        *g = input;
    }
}

pub fn get_input_state() -> InputDataFfi {
    GLOBAL_INPUT.lock().map(|g| *g).unwrap_or_default()
}

pub unsafe extern "C" fn get_input(out: *mut InputDataFfi) -> i32 {
    if out.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    unsafe { *out = get_input_state() };
    BRIDGE_OK
}
