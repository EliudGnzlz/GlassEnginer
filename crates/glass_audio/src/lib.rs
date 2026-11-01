//! glass_audio: Decoupled 2D/3D Audio Subsystem for GlassEngine.

pub mod backend;
pub mod command;
pub mod ffi;
pub mod manager;
pub mod procedural;

pub use command::SoundHandle;
pub use ffi::{
    get_global_audio_system, glass_audio_play_2d, glass_audio_play_3d,
    glass_audio_set_listener_transform, glass_audio_set_source_position, glass_audio_stop,
    register_global_audio_system,
};
pub use manager::AudioSystem;
pub use procedural::{beep_440, beep_880, generate_tone};
