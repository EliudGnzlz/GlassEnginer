//! glass_core: Window management, OS event abstractions and lifecycle contracts.

pub mod animator;
pub mod animator_stats;
pub mod profiling;
pub mod render_stats;
pub mod lighting_stats;
pub mod lightmap;
pub mod events;
pub mod math;
pub mod prefab;
pub mod componentes;
pub mod primitives;
pub mod project;
pub mod scenes;
pub mod portal;
pub mod proceso;
pub mod scene;
pub mod particle_stats;
pub mod physics_material;
pub mod transform_stats;
pub mod transforms;
pub mod shader_graph;
pub mod types;
pub mod vfs;
pub mod window;

pub use events::EngineEvent;
pub use math::{
    directional_shadow_vp, euler_deg_to_quat, mat4_from_trs_euler, mat4_from_trs_quat, mat4_identity, mat4_inverse_affine, mat4_mul,
    mat4_scale, mat4_sin_escala, mat4_to_quat, mat4_transform_point, mat4_translation, quat_mul,
    quat_normalize,
    quat_rotate_vec3, quat_to_euler_deg, quat_to_mat3,
};
pub use prefab::Prefab;
pub use physics_material::PhysicsMaterial;
pub use project::ProjectSettings;
pub use scene::{
    canvas_scale_factor, layout_group_rects, resolve_ui_rect,
    resolve_ui_rect_in, spec_por_defecto, AudioSourceData, CameraData, CanvasData, CharacterData,
    ColliderSpec, EntityData,
    LayoutGroupData, LightData, LightProfileData, MaterialData, MeshColliderMode, MeshData,
    PhysicsData,
    RectTransformData, SceneState, ScriptComponentData, TransformData, UIButtonData, UITextData,
    UIImageData, UIInputFieldData, UIDragData,
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
