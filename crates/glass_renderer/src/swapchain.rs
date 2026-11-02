use ash::vk;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SwapchainError {
    #[error("Vulkan driver error: {0}")]
    Driver(#[from] vk::Result),
    #[error("No suitable surface format found")]
    NoFormatFound,
}

pub struct SwapchainContext {
    pub device: Arc<ash::Device>,
    pub loader: ash::khr::swapchain::Device,
    pub swapchain: vk::SwapchainKHR,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub images: Vec<vk::Image>,
    pub image_views: Vec<vk::ImageView>,
}

impl SwapchainContext {
    pub fn new(
        instance: &ash::Instance,
        device: Arc<ash::Device>,
        surface_loader: &ash::khr::surface::Instance,
        surface: vk::SurfaceKHR,
        physical_device: vk::PhysicalDevice,
        width: u32,
        height: u32,
        old_swapchain: Option<vk::SwapchainKHR>,
    ) -> Result<Self, SwapchainError> {
        let capabilities = unsafe {
            surface_loader.get_physical_device_surface_capabilities(physical_device, surface)?
        };

        // 1. Elegir Extent: según la especificación Vulkan, si capabilities.current_extent.width != u32::MAX,
        // imageExtent DEBE coincidir con current_extent de la superficie nativa del sistema.
        let extent = if capabilities.current_extent.width != u32::MAX && capabilities.current_extent.width > 0 {
            capabilities.current_extent
        } else {
            vk::Extent2D {
                width: width.clamp(
                    capabilities.min_image_extent.width,
                    capabilities.max_image_extent.width,
                ),
                height: height.clamp(
                    capabilities.min_image_extent.height,
                    capabilities.max_image_extent.height,
                ),
            }
        };

        // 2. Elegir Formato
        let formats = unsafe {
            surface_loader.get_physical_device_surface_formats(physical_device, surface)?
        };
        let surface_format = formats
            .iter()
            .find(|f| {
                (f.format == vk::Format::B8G8R8A8_SRGB || f.format == vk::Format::R8G8B8A8_SRGB)
                    && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
            })
            .or_else(|| formats.first())
            .copied()
            .ok_or(SwapchainError::NoFormatFound)?;

        // 3. Elegir Modo de Presentación (Mailbox -> Fifo)
        let present_modes = unsafe {
            surface_loader.get_physical_device_surface_present_modes(physical_device, surface)?
        };
        let present_mode = if present_modes.contains(&vk::PresentModeKHR::MAILBOX) {
            vk::PresentModeKHR::MAILBOX
        } else {
            vk::PresentModeKHR::FIFO
        };

        let mut image_count = capabilities.min_image_count + 1;
        if capabilities.max_image_count > 0 && image_count > capabilities.max_image_count {
            image_count = capabilities.max_image_count;
        }

        // Composite Alpha: evitar problemas de ventana transparente seleccionando OPAQUE si está soportado
        let composite_alpha = if capabilities
            .supported_composite_alpha
            .contains(vk::CompositeAlphaFlagsKHR::OPAQUE)
        {
            vk::CompositeAlphaFlagsKHR::OPAQUE
        } else if capabilities
            .supported_composite_alpha
            .contains(vk::CompositeAlphaFlagsKHR::INHERIT)
        {
            vk::CompositeAlphaFlagsKHR::INHERIT
        } else {
            vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED
        };

        let swapchain_loader = ash::khr::swapchain::Device::new(instance, &device);

        let mut swapchain_create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(image_count)
            .image_format(surface_format.format)
            .image_color_space(surface_format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(capabilities.current_transform)
            .composite_alpha(composite_alpha)
            .present_mode(present_mode)
            .clipped(true);

        if let Some(old) = old_swapchain {
            swapchain_create_info = swapchain_create_info.old_swapchain(old);
        }

        log::info!(
            "Creando Swapchain: {}x{} | Formato: {:?} | Modo: {:?}",
            extent.width,
            extent.height,
            surface_format.format,
            present_mode
        );

        let swapchain = unsafe { swapchain_loader.create_swapchain(&swapchain_create_info, None)? };
        let images = unsafe { swapchain_loader.get_swapchain_images(swapchain)? };

        // 4. Crear ImageViews
        let mut image_views = Vec::with_capacity(images.len());
        for &image in &images {
            let view_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(surface_format.format)
                .components(vk::ComponentMapping::default())
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(1),
                );

            let view = unsafe { device.create_image_view(&view_info, None)? };
            image_views.push(view);
        }

        Ok(Self {
            device,
            loader: swapchain_loader,
            swapchain,
            format: surface_format.format,
            extent,
            images,
            image_views,
        })
    }

    pub fn acquire_next_image(&self, semaphore: vk::Semaphore) -> Result<(u32, bool), vk::Result> {
        unsafe {
            self.loader
                .acquire_next_image(self.swapchain, u64::MAX, semaphore, vk::Fence::null())
        }
    }
}

impl Drop for SwapchainContext {
    fn drop(&mut self) {
        unsafe {
            for &view in &self.image_views {
                self.device.destroy_image_view(view, None);
            }
            self.loader.destroy_swapchain(self.swapchain, None);
        }
    }
}
