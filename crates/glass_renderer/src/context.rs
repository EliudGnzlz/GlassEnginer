use ash::vk;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::borrow::Cow;
use std::ffi::{c_char, CStr, CString};
use thiserror::Error;

const VALIDATION_LAYER_NAME: &CStr = c"VK_LAYER_KHRONOS_validation";

#[derive(Error, Debug)]
pub enum VulkanError {
    #[error("Vulkan loading error: {0}")]
    Loading(#[from] ash::LoadingError),
    #[error("Vulkan driver error: {0}")]
    Driver(#[from] vk::Result),
    #[error("No suitable physical device (GPU) found matching requirements")]
    NoSuitableGpu,
    #[error("Window handle error: {0}")]
    WindowHandle(String),
    #[error("Missing required Vulkan extension or layer: {0}")]
    MissingRequirement(String),
    #[error(
        "GPU '{device}' sólo admite {available} bytes de push constants, pero el motor requiere {required}"
    )]
    InsufficientPushConstantSpace {
        device: String,
        available: u32,
        required: u32,
    },
}

unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    let callback_data = *p_callback_data;
    let message = if callback_data.p_message.is_null() {
        Cow::Borrowed("")
    } else {
        CStr::from_ptr(callback_data.p_message).to_string_lossy()
    };

    match message_severity {
        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => {
            log::error!("[Vulkan Validation - {:?}] {}", message_type, message);
        }
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => {
            log::warn!("[Vulkan Validation - {:?}] {}", message_type, message);
        }
        vk::DebugUtilsMessageSeverityFlagsEXT::INFO => {
            log::info!("[Vulkan Validation - {:?}] {}", message_type, message);
        }
        _ => {
            log::trace!("[Vulkan Validation - {:?}] {}", message_type, message);
        }
    }

    vk::FALSE
}

#[derive(Debug, Clone)]
pub struct VulkanConfig {
    pub app_name: String,
    pub enable_validation: bool,
    pub preferred_gpu_name: Option<String>,
}

impl Default for VulkanConfig {
    fn default() -> Self {
        Self {
            app_name: "GlassEngine App".to_string(),
            enable_validation: cfg!(debug_assertions),
            preferred_gpu_name: std::env::var("GLASS_GPU_NAME").ok(),
        }
    }
}

pub struct QueueFamilyIndices {
    pub graphics_family: u32,
    pub present_family: u32,
}

pub struct VulkanContext {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub debug_utils: Option<ash::ext::debug_utils::Instance>,
    pub debug_messenger: Option<vk::DebugUtilsMessengerEXT>,
    pub surface_loader: ash::khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
    pub physical_device: vk::PhysicalDevice,
    pub physical_device_properties: vk::PhysicalDeviceProperties,
    pub device: ash::Device,
    pub graphics_queue: vk::Queue,
    pub present_queue: vk::Queue,
    pub queue_families: QueueFamilyIndices,
}

struct GpuCandidate {
    device: vk::PhysicalDevice,
    properties: vk::PhysicalDeviceProperties,
    indices: QueueFamilyIndices,
    score: u32,
}

impl VulkanContext {
    pub fn new<W>(window: &W, config: VulkanConfig) -> Result<Self, VulkanError>
    where
        W: HasDisplayHandle + HasWindowHandle,
    {
        log::info!("Cargando punto de entrada Vulkan (ash::Entry)...");
        let entry = unsafe { ash::Entry::load()? };

        let display_handle = window
            .display_handle()
            .map_err(|e| VulkanError::WindowHandle(e.to_string()))?
            .as_raw();
        let window_handle = window
            .window_handle()
            .map_err(|e| VulkanError::WindowHandle(e.to_string()))?
            .as_raw();

        // 1. Capas y Extensiones de Instancia
        let mut enabled_layers: Vec<*const c_char> = Vec::new();
        if config.enable_validation {
            let available_layers = unsafe { entry.enumerate_instance_layer_properties()? };
            let has_validation = available_layers.iter().any(|layer| {
                let name = unsafe { CStr::from_ptr(layer.layer_name.as_ptr()) };
                name == VALIDATION_LAYER_NAME
            });

            if has_validation {
                log::info!("Capa de validación habilitada: {:?}", VALIDATION_LAYER_NAME);
                enabled_layers.push(VALIDATION_LAYER_NAME.as_ptr());
            } else {
                log::warn!("Capa de validación solicitada pero no disponible en el sistema.");
            }
        }

        let required_surface_extensions =
            ash_window::enumerate_required_extensions(display_handle)?;
        let mut enabled_extensions: Vec<*const c_char> = required_surface_extensions.to_vec();

        if config.enable_validation {
            enabled_extensions.push(ash::ext::debug_utils::NAME.as_ptr());
        }

        let app_name = CString::new(config.app_name.clone()).unwrap_or_default();
        let engine_name = c"GlassEngine";

        let app_info = vk::ApplicationInfo::default()
            .application_name(&app_name)
            .application_version(vk::make_api_version(0, 1, 0, 0))
            .engine_name(engine_name)
            .engine_version(vk::make_api_version(0, 1, 0, 0))
            .api_version(vk::API_VERSION_1_3);

        let mut debug_create_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(
                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                    | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            )
            .pfn_user_callback(Some(vulkan_debug_callback));

        let mut instance_create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_layer_names(&enabled_layers)
            .enabled_extension_names(&enabled_extensions);

        if config.enable_validation {
            instance_create_info = instance_create_info.push_next(&mut debug_create_info);
        }

        log::info!("Creando ash::Instance de Vulkan...");
        let instance = unsafe { entry.create_instance(&instance_create_info, None)? };

        // 2. Debug Messenger
        let (debug_utils, debug_messenger) = if config.enable_validation {
            let loader = ash::ext::debug_utils::Instance::new(&entry, &instance);
            let messenger = unsafe { loader.create_debug_utils_messenger(&debug_create_info, None)? };
            (Some(loader), Some(messenger))
        } else {
            (None, None)
        };

        // 3. Superficie Windowing
        log::info!("Creando VkSurfaceKHR para la ventana nativa...");
        let surface = unsafe {
            ash_window::create_surface(&entry, &instance, display_handle, window_handle, None)?
        };
        let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);

        // 4. Selección Universal de GPU Física (Agnóstica de fabricante y tipo)
        log::info!("Enumerando y puntuando dispositivos físicos (NVIDIA, AMD, Intel, Integradas, Dedicadas)...");
        let physical_devices = unsafe { instance.enumerate_physical_devices()? };
        let mut candidates: Vec<GpuCandidate> = Vec::new();

        for pdevice in physical_devices {
            let properties = unsafe { instance.get_physical_device_properties(pdevice) };
            let queue_families =
                unsafe { instance.get_physical_device_queue_family_properties(pdevice) };

            let mut graphics_index = None;
            let mut present_index = None;

            for (index, family) in queue_families.iter().enumerate() {
                let idx = index as u32;
                if family.queue_flags.contains(vk::QueueFlags::GRAPHICS) && graphics_index.is_none() {
                    graphics_index = Some(idx);
                }

                let supports_present = unsafe {
                    surface_loader.get_physical_device_surface_support(pdevice, idx, surface)?
                };
                if supports_present && present_index.is_none() {
                    present_index = Some(idx);
                }

                if graphics_index.is_some() && present_index.is_some() {
                    break;
                }
            }

            // Verificar soporte de extensión swapchain
            let available_extensions =
                unsafe { instance.enumerate_device_extension_properties(pdevice)? };
            let supports_swapchain = available_extensions.iter().any(|ext| {
                let name = unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) };
                name == ash::khr::swapchain::NAME
            });

            if let (Some(g_idx), Some(p_idx)) = (graphics_index, present_index) {
                if supports_swapchain {
                    let device_name = unsafe { CStr::from_ptr(properties.device_name.as_ptr()) }
                        .to_string_lossy()
                        .to_string();

                    // Puntuación heurística:
                    // 1. Dedicada (Discrete GPU): NVIDIA, AMD Radeon, Intel Arc -> Base 10000
                    // 2. Integrada (Integrated GPU): Intel Iris/UHD, AMD Vega/RDNA APU -> Base 5000
                    // 3. Virtual GPU (VM, Cloud) -> Base 2000
                    // 4. CPU (Software rasterizer, llvmpipe, lavapipe) -> Base 1000
                    let mut score = match properties.device_type {
                        vk::PhysicalDeviceType::DISCRETE_GPU => 10000,
                        vk::PhysicalDeviceType::INTEGRATED_GPU => 5000,
                        vk::PhysicalDeviceType::VIRTUAL_GPU => 2000,
                        vk::PhysicalDeviceType::CPU => 1000,
                        _ => 500,
                    };

                    // Bonus si coincide con el nombre preferido por el usuario en configuración o variable de entorno
                    if let Some(pref) = &config.preferred_gpu_name {
                        if device_name.to_lowercase().contains(&pref.to_lowercase()) {
                            score += 50000;
                        }
                    }

                    // Bonus por memoria VRAM local disponible
                    let mem_props = unsafe { instance.get_physical_device_memory_properties(pdevice) };
                    let mut local_vram_mb = 0u64;
                    for i in 0..mem_props.memory_heap_count as usize {
                        if mem_props.memory_heaps[i]
                            .flags
                            .contains(vk::MemoryHeapFlags::DEVICE_LOCAL)
                        {
                            local_vram_mb += mem_props.memory_heaps[i].size / (1024 * 1024);
                        }
                    }
                    score += (local_vram_mb / 100).min(3000) as u32;

                    log::info!(
                        "GPU detectada: '{}' | Tipo: {:?} | VRAM: {} MB | Score: {} | Colas [G:{}, P:{}]",
                        device_name,
                        properties.device_type,
                        local_vram_mb,
                        score,
                        g_idx,
                        p_idx
                    );

                    candidates.push(GpuCandidate {
                        device: pdevice,
                        properties,
                        indices: QueueFamilyIndices {
                            graphics_family: g_idx,
                            present_family: p_idx,
                        },
                        score,
                    });
                }
            }
        }

        if candidates.is_empty() {
            return Err(VulkanError::NoSuitableGpu);
        }

        // Ordenar candidatos de mayor a menor puntuación
        candidates.sort_by_key(|c| std::cmp::Reverse(c.score));
        let best_gpu = candidates.remove(0);

        let chosen_name =
            unsafe { CStr::from_ptr(best_gpu.properties.device_name.as_ptr()) }
                .to_string_lossy();
        log::info!("GPU seleccionada (Score máximo {}): {}", best_gpu.score, chosen_name);

        let physical_device = best_gpu.device;
        let physical_device_properties = best_gpu.properties;
        let queue_families = best_gpu.indices;

        // 4.b Verificación de capacidad de push constants.
        // El pipeline declara MESH_PUSH_CONSTANTS_SIZE bytes; si el dispositivo
        // ofrece menos, `create_pipeline_layout` fallaría más tarde (o peor, se
        // leería basura). Se comprueba aquí para dar un error explícito.
        let max_push = physical_device_properties.limits.max_push_constants_size;
        let required_push = crate::renderer::MESH_PUSH_CONSTANTS_SIZE;
        if max_push < required_push {
            let name = unsafe { CStr::from_ptr(physical_device_properties.device_name.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            log::error!(
                "GPU '{}' expone maxPushConstantsSize={} pero el motor necesita {} bytes.",
                name,
                max_push,
                required_push
            );
            return Err(VulkanError::InsufficientPushConstantSpace {
                device: name,
                available: max_push,
                required: required_push,
            });
        }
        log::info!(
            "Push constants: {} bytes requeridos / {} disponibles.",
            required_push,
            max_push
        );

        // 5. Creación de Dispositivo Lógico (Device) y Colas
        let mut unique_queue_families = vec![queue_families.graphics_family];
        if queue_families.present_family != queue_families.graphics_family {
            unique_queue_families.push(queue_families.present_family);
        }

        let queue_priorities = [1.0f32];
        let queue_create_infos: Vec<vk::DeviceQueueCreateInfo> = unique_queue_families
            .iter()
            .map(|&family_index| {
                vk::DeviceQueueCreateInfo::default()
                    .queue_family_index(family_index)
                    .queue_priorities(&queue_priorities)
            })
            .collect();

        let device_extensions = [ash::khr::swapchain::NAME.as_ptr()];
        // Fase 2: compresión BC de texturas solo si el dispositivo la
        // soporta; el feature hay que ACTIVARLO en la creación y avisar a
        // la ruta de subida, que si no sigue en RGBA8.
        let supported_features = unsafe { instance.get_physical_device_features(physical_device) };
        let bc_ok = supported_features.texture_compression_bc != vk::FALSE;
        let mut device_features = vk::PhysicalDeviceFeatures::default();
        if bc_ok {
            device_features.texture_compression_bc = vk::TRUE;
            crate::mesh::set_texture_bc_enabled(true);
        }
        log::info!(
            "Compresión BC de texturas: {}",
            if bc_ok { "activada" } else { "no soportada; texturas en RGBA8" }
        );

        let device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_create_infos)
            .enabled_extension_names(&device_extensions)
            .enabled_features(&device_features);

        log::info!("Creando ash::Device lógico...");
        let device = unsafe { instance.create_device(physical_device, &device_create_info, None)? };

        let graphics_queue =
            unsafe { device.get_device_queue(queue_families.graphics_family, 0) };
        let present_queue =
            unsafe { device.get_device_queue(queue_families.present_family, 0) };

        log::info!("Contexto Vulkan inicializado con éxito.");

        Ok(Self {
            entry,
            instance,
            debug_utils,
            debug_messenger,
            surface_loader,
            surface,
            physical_device,
            physical_device_properties,
            device,
            graphics_queue,
            present_queue,
            queue_families,
        })
    }
}

impl Drop for VulkanContext {
    fn drop(&mut self) {
        unsafe {
            log::info!("Destruyendo VulkanContext en orden inverso seguro...");
            self.device.device_wait_idle().unwrap_or(());
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);

            if let (Some(loader), Some(messenger)) = (&self.debug_utils, self.debug_messenger) {
                loader.destroy_debug_utils_messenger(messenger, None);
            }

            self.instance.destroy_instance(None);
            log::info!("Contexto Vulkan liberado correctamente.");
        }
    }
}
