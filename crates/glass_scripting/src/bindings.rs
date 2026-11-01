use serde::{Deserialize, Serialize};

/// Flat C-ABI Transform structure matching C# [StructLayout(LayoutKind.Sequential)].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TransformFfi {
    pub position: [f32; 3],
    pub rotation: [f32; 4], // Quaternion
    pub scale: [f32; 3],
}

impl Default for TransformFfi {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}
