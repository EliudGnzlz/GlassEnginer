//! glass_physics: 3D Rigid Body and Collision Physics Engine for GlassEngine.

pub mod ffi;
pub mod types;
pub mod world;

pub use types::*;
pub use world::*;
pub use ffi::*;

pub fn version() -> &'static str {
    "0.23"
}
