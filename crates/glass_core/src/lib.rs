//! glass_core: Window management, OS event abstractions and lifecycle contracts.

pub mod events;
pub mod prefab;
pub mod project;
pub mod scene;
pub mod types;
pub mod window;

pub use events::EngineEvent;
pub use prefab::Prefab;
pub use project::ProjectSettings;
pub use scene::{
    AudioSourceData, CameraData, CanvasData, EntityData, LightData, MeshData, PhysicsData,
    RectTransformData, SceneState, ScriptComponentData, TransformData, UIButtonData, UITextData,
};
pub use types::TransformFfi;
pub use window::{GlassWindow, WindowConfig};

/// Trait representing an engine subsystem lifecycle.
pub trait Subsystem {
    fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn update(&mut self, delta_time: f32) -> Result<(), Box<dyn std::error::Error>>;
    fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error>>;
}
