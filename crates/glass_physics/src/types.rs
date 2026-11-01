use serde::{Deserialize, Serialize};

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyType {
    Dynamic = 0,
    Static = 1,
    Kinematic = 2,
}

impl BodyType {
    pub fn from_i32(val: i32) -> Self {
        match val {
            0 => Self::Dynamic,
            1 => Self::Static,
            _ => Self::Kinematic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ColliderShape {
    Cuboid { half_extents: [f32; 3] },
    Ball { radius: f32 },
    Capsule { half_height: f32, radius: f32 },
}

pub type BodyHandle = u64;
pub type ColliderHandle = u64;
