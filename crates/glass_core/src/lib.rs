//! glass_core: Window management, OS event abstractions and lifecycle contracts.

pub mod events;
pub mod window;

pub use events::EngineEvent;
pub use window::{GlassWindow, WindowConfig};

/// Trait representing an engine subsystem lifecycle.
pub trait Subsystem {
    fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn update(&mut self, delta_time: f32) -> Result<(), Box<dyn std::error::Error>>;
    fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error>>;
}
