use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
    pub vsync: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "GlassEngine - Milestone 2".to_string(),
            width: 1280,
            height: 720,
            resizable: true,
            vsync: true,
        }
    }
}

impl WindowConfig {
    pub fn to_window_attributes(&self) -> WindowAttributes {
        Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(LogicalSize::new(self.width as f64, self.height as f64))
            .with_resizable(self.resizable)
    }
}

pub struct GlassWindow {
    pub inner: Arc<Window>,
}

impl GlassWindow {
    pub fn new(event_loop: &ActiveEventLoop, config: &WindowConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let attrs = config.to_window_attributes();
        let window = Arc::new(event_loop.create_window(attrs)?);
        Ok(Self { inner: window })
    }

    pub fn id(&self) -> WindowId {
        self.inner.id()
    }

    pub fn request_redraw(&self) {
        self.inner.request_redraw();
    }
}

impl HasWindowHandle for GlassWindow {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        self.inner.window_handle()
    }
}

impl HasDisplayHandle for GlassWindow {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        self.inner.display_handle()
    }
}
