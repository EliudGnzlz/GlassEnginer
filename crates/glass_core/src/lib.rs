//! glass_core: Window management, OS event abstractions and lifecycle contracts.

pub mod animator;
pub mod lightmap;
pub mod events;
pub mod math;
pub mod prefab;
pub mod primitives;
pub mod project;
pub mod scenes;
pub mod portal;
pub mod scene;
pub mod shader_graph;
pub mod types;
pub mod vfs;
pub mod window;

pub use events::EngineEvent;
pub use math::{
    directional_shadow_vp, euler_deg_to_quat, mat4_from_trs_euler, mat4_from_trs_quat, mat4_identity, mat4_inverse_affine, mat4_mul,
    mat4_scale, mat4_to_quat, mat4_transform_point, mat4_translation, quat_mul, quat_normalize,
    quat_rotate_vec3, quat_to_euler_deg, quat_to_mat3,
};
pub use prefab::Prefab;
pub use project::ProjectSettings;
pub use scene::{
    resolve_ui_rect, AudioSourceData, CameraData, CanvasData, EntityData, LightData,
    MaterialData, MeshData,
    PhysicsData, RectTransformData, SceneState, ScriptComponentData, TransformData, UIButtonData,
    UITextData,
    UIImageData,
    UIDragData,
};
pub use scenes::{SceneError, SceneManager, SceneLoaded};
pub use types::TransformFfi;
pub use vfs::{AssetVfs, VfsError, normalize_asset_path};
pub use window::{GlassWindow, WindowConfig};

/// Trait representing an engine subsystem lifecycle.
pub trait Subsystem {
    fn initialize(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn update(&mut self, delta_time: f32) -> Result<(), Box<dyn std::error::Error>>;
    fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error>>;
}
