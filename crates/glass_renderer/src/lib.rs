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
pub mod watcher;

pub use asset_cache::{global_asset_cache, register_global_asset_cache, AssetCache};
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
    GlassRenderer, MeshDrawItem, MeshPushConstants, PointLightsUbo, PointShadowLight, PortalDrawItem,
    PortalPushConstants, PostView, ShadowCasters, SkinnedDrawItem, MAX_POINT_LIGHTS,
    ParticleBuffers, ParticleVertex,
};
pub use skin::{AnimationClip, JointBuffer, SkinData};
pub use swapchain::SwapchainContext;
pub use ui::{
    pack_ui_image_atlas, FontAtlas, UiAtlasKind, UiBatch, UiImageAtlas, UiRenderer, UiSegment,
    UiVertex,
};
pub use watcher::{AssetReloadEvent, AssetWatcher};
