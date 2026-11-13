//! glass_physics: 3D Rigid Body and Collision Physics Engine for GlassEngine.

pub mod ffi;
pub mod types;
pub mod world;

pub mod character;
pub mod ragdoll;
pub use ragdoll::{RagdollBoneSpec, RagdollState};
pub use types::*;
pub use world::*;
pub use ffi::*;
pub use character::{CharacterConfig, CharacterState};

pub fn version() -> &'static str {
    "0.23"
}
