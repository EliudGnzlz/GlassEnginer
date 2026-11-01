//! glass_scripting: C# CoreCLR scripting host and FFI bindings.

pub mod bindings;
pub mod host;

pub use bindings::TransformFfi;
pub use host::ScriptHost;
