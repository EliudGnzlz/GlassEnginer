//! glass_renderer: Direct Vulkan rendering backend via ash and gpu-allocator.

pub mod asset_cache;
pub mod context;
pub mod mesh;
pub mod offscreen;
pub mod pipeline;
pub mod renderer;
pub mod swapchain;
pub mod watcher;

pub use asset_cache::AssetCache;
pub use context::{VulkanConfig, VulkanContext};
pub use mesh::{GpuMesh, MeshLoader, Vertex};
pub use offscreen::OffscreenTarget;
pub use pipeline::GraphicsPipeline;
pub use renderer::{GlassRenderer, MeshPushConstants};
pub use swapchain::SwapchainContext;
pub use watcher::{AssetReloadEvent, AssetWatcher};
