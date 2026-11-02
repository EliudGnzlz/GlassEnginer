//! glass_editor: Unity-style docking editor layout powered by egui.

pub mod docking;
pub mod panels;

pub use docking::EditorDockingLayout;
pub use panels::{GameResolution, PlayState, ViewportTab};
