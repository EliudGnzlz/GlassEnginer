//! glass_scripting: C# CoreCLR scripting host and FFI bindings.

pub mod bindings;
pub mod bridge;
pub mod host;

pub use bindings::TransformFfi;
pub use bridge::{register_scene_bridge, SceneBridge};
pub use host::{ScriptError, ScriptHost};
