//! glass_renderer: Direct Vulkan rendering backend via ash and gpu-allocator.

pub mod context;
pub mod offscreen;
pub mod pipeline;
pub mod renderer;
pub mod swapchain;

pub use context::{VulkanConfig, VulkanContext};
pub use offscreen::OffscreenTarget;
pub use pipeline::GraphicsPipeline;
pub use renderer::GlassRenderer;
pub use swapchain::SwapchainContext;
