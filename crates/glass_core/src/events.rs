use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EngineEvent {
    WindowResized { width: u32, height: u32 },
    WindowClosed,
    KeyboardInput { key: String, pressed: bool },
    CursorMoved { x: f64, y: f64 },
    MouseButton { button: u32, pressed: bool },
    FileDropped(String),
}
