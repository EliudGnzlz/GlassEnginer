//! glass_scripting: C# CoreCLR scripting host and FFI bindings.

pub mod bindings;
pub mod bridge;
pub mod host;

pub use bindings::{get_input_state, get_script_params, set_input_state, set_script_params, InputDataFfi, ScriptParamsFfi, TransformFfi};
pub use bridge::{register_scene_bridge, SceneBridge};
pub use host::{ScriptError, ScriptHost};
