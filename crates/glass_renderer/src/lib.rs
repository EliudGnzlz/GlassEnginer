//! glass_renderer: Direct Vulkan rendering backend via ash and gpu-allocator.

pub mod asset_cache;
pub mod context;
pub mod mesh;
pub mod offscreen;
pub mod post;
pub mod pipeline;
pub mod renderer;
pub mod shadow;
pub mod skin;
pub mod swapchain;
pub mod ui;
pub mod sdf_atlas;
pub mod watcher;

pub use asset_cache::{global_asset_cache, register_global_asset_cache, AssetCache, TextureUpload};
pub use context::{VulkanConfig, VulkanContext};
pub use mesh::{
    generate_capsule, generate_cylinder, generate_plane, generate_sphere, GpuMesh, MeshLoader,
    Vertex, PLANE_SEGMENTS,
};
pub use mesh::{lod_level_for_distance, lod_suffix, ModelObject, LOD_FAR, LOD_NEAR};
pub use offscreen::OffscreenTarget;
pub use pipeline::GraphicsPipeline;
pub use post::PostSettings;
pub use renderer::{
    FrameDraws, GlassRenderer, GraphCaster, MeshDrawItem, MeshPushConstants, PointLightsUbo,
    PointShadowLight, PortalDrawItem, PortalPushConstants, PostView, ShadowCasters, SkinnedDrawItem,
    StaticCaster, MAX_POINT_LIGHTS, ParticleBuffers, ParticleVertex,
    EditorLineBuffers, EditorLineVertex,
};
pub use skin::{AnimationClip, JointBuffer, SkinData};
pub use swapchain::SwapchainContext;
pub use sdf_atlas::SdfAtlas;
pub use ui::{
    pack_ui_image_atlas, UiAtlasKind, UiBatch, UiImageAtlas, UiRenderer, UiSegment, UiVertex,
};
pub use watcher::{fijar_bridge_desde_raiz, ruta_bridge_en_raiz, AssetReloadEvent, AssetWatcher};
