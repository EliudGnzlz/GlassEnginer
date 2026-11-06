//! glass_scripting: C# CoreCLR scripting host and FFI bindings.

pub mod bindings;
pub mod bridge;
pub mod input_axes;
pub mod host;

pub use bindings::{get_input_state, get_script_params, set_input_state, set_script_params, InputDataFfi, ScriptParamsFfi, TransformFfi};
pub use bridge::{
    register_scene_bridge, SceneBridge, SceneLoadRejection, SpawnSpec, SPAWN_PHYSICS_DYNAMIC,
    SPAWN_PHYSICS_KINEMATIC, SPAWN_PHYSICS_NONE, SPAWN_PHYSICS_STATIC,
};
pub use host::{ScriptError, ScriptHost};
