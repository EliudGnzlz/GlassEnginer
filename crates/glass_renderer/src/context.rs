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
    /// Aviso para el usuario cuando la GPU elegida no es la dedicada
    /// habiendo una dedicada enumerada (ver `aviso_gpu`); la app lo
    /// muestra en el Hub. None si no hay nada que avisar.
    pub gpu_aviso: Option<String>,
}

/// Datos de un dispositivo físico ya consultados al ICD, en forma plana
/// para que la selección de GPU sea una función pura y testeable con
/// listas simuladas (en un equipo híbrido real no hay forma de probar
/// aquí la enumeración: la VM solo ofrece lavapipe).
#[derive(Debug, Clone)]
pub struct GpuDeviceInfo {
    pub name: String,
    pub device_type: vk::PhysicalDeviceType,
    pub vendor_id: u32,
    pub driver_version: u32,
    pub local_vram_mb: u64,
    pub graphics_family: Option<u32>,
    pub present_family: Option<u32>,
    pub swapchain_supported: bool,
}

impl GpuDeviceInfo {
    pub fn es_discreta(&self) -> bool {
        self.device_type == vk::PhysicalDeviceType::DISCRETE_GPU
    }

    /// Por qué el dispositivo no puede ser candidato (None = puede).
    pub fn motivo_rechazo(&self) -> Option<&'static str> {
        if self.graphics_family.is_none() {
            Some("sin cola de gráficos")
        } else if self.present_family.is_none() {
            Some("sin cola de presentación para la superficie")
        } else if !self.swapchain_supported {
            Some("sin la extensión VK_KHR_swapchain")
        } else {
            None
        }
    }
}

/// Puntuación de preferencia de una GPU candidata (None si no lo es).
/// Orden de tipos: discreta → integrada → virtual → CPU/software, como
/// en Unity y demás motores; dentro del tipo manda la VRAM local, y el
/// nombre preferido (GLASS_GPU_NAME) se impone a todo. La escala hace
/// imposible que una integrada (máx. 5000+3000) supere a una discreta
/// (mín. 10000).
pub fn gpu_score(info: &GpuDeviceInfo, preferred_gpu_name: Option<&str>) -> Option<u32> {
    if info.motivo_rechazo().is_some() {
        return None;
    }
    let mut score = match info.device_type {
        vk::PhysicalDeviceType::DISCRETE_GPU => 10000,
        vk::PhysicalDeviceType::INTEGRATED_GPU => 5000,
        vk::PhysicalDeviceType::VIRTUAL_GPU => 2000,
        vk::PhysicalDeviceType::CPU => 1000,
        _ => 500,
    };
    if let Some(pref) = preferred_gpu_name {
        if info.name.to_lowercase().contains(&pref.to_lowercase()) {
            score += 50000;
        }
    }
    score += (info.local_vram_mb / 100).min(3000) as u32;
    Some(score)
}

/// Índice de la GPU elegida entre las enumeradas (None si ninguna es
/// candidata). Empate: gana la primera enumerada (sort estable).
pub fn select_gpu(devices: &[GpuDeviceInfo], preferred_gpu_name: Option<&str>) -> Option<usize> {
    let mut best: Option<(usize, u32)> = None;
    for (idx, info) in devices.iter().enumerate() {
        if let Some(score) = gpu_score(info, preferred_gpu_name) {
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((idx, score));
            }
        }
    }
    best.map(|(idx, _)| idx)
}

/// Aviso para el usuario cuando se va a trabajar con una GPU que no es
/// la dedicada habiendo una dedicada enumerada: en un portátil híbrido
/// la dedicada puede no reportar salida de pantalla y la integrada se
/// queda el trabajo — con un driver viejo eso ha cerrado el motor sin
/// mensaje (igvk64.dll, 2026-11-17). None si no hay nada que avisar.
pub fn aviso_gpu(devices: &[GpuDeviceInfo], elegida: usize) -> Option<String> {
    let ganadora = devices.get(elegida)?;
    if ganadora.es_discreta() {
        return None;
    }
    let discreta = devices.iter().find(|d| d.es_discreta())?;
    let motivo = discreta
        .motivo_rechazo()
        .unwrap_or("quedó por debajo en la puntuación");
    Some(format!(
        "Se usará la gráfica «{}» y no la dedicada «{}»: la dedicada no puede usarse ({}). \
         Si el motor se cierra sin aviso o va lento, fuerza la dedicada en la configuración \
         de gráficos de Windows (Configuración > Sistema > Pantalla > Gráficos) o actualiza \
         el driver de la integrada: un driver antiguo puede fallar con Vulkan.",
        ganadora.name, discreta.name, motivo
    ))
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

        // 4. Selección de GPU física: se enumeran TODOS los dispositivos
        // y se anota el veredicto de cada uno — una dedicada descartada
        // en silencio dejó sin diagnóstico el cierre en un equipo
        // híbrido (2026-11-17) — y la elección la hace `select_gpu`,
        // función pura cubierta por tests con listas simuladas.
        log::info!("Enumerando dispositivos físicos Vulkan...");
        let physical_devices = unsafe { instance.enumerate_physical_devices()? };
        let mut dispositivos: Vec<(
            vk::PhysicalDevice,
            vk::PhysicalDeviceProperties,
            GpuDeviceInfo,
        )> = Vec::new();

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

            // Memoria VRAM local disponible
            let mem_props = unsafe { instance.get_physical_device_memory_properties(pdevice) };
            let mut local_vram_mb = 0u64;
            for heap in mem_props
                .memory_heaps
                .iter()
                .take(mem_props.memory_heap_count as usize)
            {
                if heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL) {
                    local_vram_mb += heap.size / (1024 * 1024);
                }
            }

            let device_name = unsafe { CStr::from_ptr(properties.device_name.as_ptr()) }
                .to_string_lossy()
                .to_string();
            let info = GpuDeviceInfo {
                name: device_name,
                device_type: properties.device_type,
                vendor_id: properties.vendor_id,
                driver_version: properties.driver_version,
                local_vram_mb,
                graphics_family: graphics_index,
                present_family: present_index,
                swapchain_supported: supports_swapchain,
            };
            match info.motivo_rechazo() {
                None => log::info!(
                    "GPU enumerada: '{}' | Tipo: {:?} | Vendor: {:#06x} | Driver: {} ({:#010x}) | VRAM local: {} MB | candidata (score {})",
                    info.name,
                    info.device_type,
                    info.vendor_id,
                    info.driver_version,
                    info.driver_version,
                    info.local_vram_mb,
                    gpu_score(&info, config.preferred_gpu_name.as_deref()).unwrap_or(0)
                ),
                Some(motivo) => log::info!(
                    "GPU enumerada: '{}' | Tipo: {:?} | Vendor: {:#06x} | Driver: {} ({:#010x}) | VRAM local: {} MB | NO candidata: {}",
                    info.name,
                    info.device_type,
                    info.vendor_id,
                    info.driver_version,
                    info.driver_version,
                    info.local_vram_mb,
                    motivo
                ),
            }
            dispositivos.push((pdevice, properties, info));
        }

        let infos: Vec<GpuDeviceInfo> =
            dispositivos.iter().map(|(_, _, i)| i.clone()).collect();
        let elegida = select_gpu(&infos, config.preferred_gpu_name.as_deref())
            .ok_or(VulkanError::NoSuitableGpu)?;
        let gpu_aviso = aviso_gpu(&infos, elegida);
        if let Some(aviso) = &gpu_aviso {
            log::warn!("{}", aviso);
        }
        let info_elegida = infos[elegida].clone();
        log::info!(
            "GPU seleccionada: '{}' | Tipo: {:?} | Vendor: {:#06x} | Driver: {} ({:#010x}) | Score: {}",
            info_elegida.name,
            info_elegida.device_type,
            info_elegida.vendor_id,
            info_elegida.driver_version,
            info_elegida.driver_version,
            gpu_score(&info_elegida, config.preferred_gpu_name.as_deref()).unwrap_or(0)
        );

        let (physical_device, physical_device_properties, _) =
            dispositivos.into_iter().nth(elegida).unwrap();
        let queue_families = QueueFamilyIndices {
            graphics_family: info_elegida.graphics_family.unwrap(),
            present_family: info_elegida.present_family.unwrap(),
        };

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
            gpu_aviso,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(
        nombre: &str,
        tipo: vk::PhysicalDeviceType,
        vram_mb: u64,
        present: bool,
    ) -> GpuDeviceInfo {
        GpuDeviceInfo {
            name: nombre.to_string(),
            device_type: tipo,
            vendor_id: 0,
            driver_version: 0,
            local_vram_mb: vram_mb,
            graphics_family: Some(0),
            present_family: if present { Some(0) } else { None },
            swapchain_supported: true,
        }
    }

    fn intel() -> GpuDeviceInfo {
        gpu(
            "Intel(R) UHD Graphics",
            vk::PhysicalDeviceType::INTEGRATED_GPU,
            8192,
            true,
        )
    }

    fn nvidia() -> GpuDeviceInfo {
        gpu(
            "NVIDIA GeForce RTX 3060",
            vk::PhysicalDeviceType::DISCRETE_GPU,
            8192,
            true,
        )
    }

    #[test]
    fn hibrido_elige_la_discreta_en_ambos_ordenes() {
        assert_eq!(select_gpu(&[intel(), nvidia()], None), Some(1));
        assert_eq!(select_gpu(&[nvidia(), intel()], None), Some(0));
    }

    #[test]
    fn discreta_sin_vram_gana_a_integrada_con_mucha() {
        // La escala lo garantiza: discreta mín. 10000 > integrada máx. 8000.
        let discreta_pobre = gpu(
            "NVIDIA GeForce GTX 1050",
            vk::PhysicalDeviceType::DISCRETE_GPU,
            0,
            true,
        );
        let integrada_rica = gpu(
            "Intel(R) Iris Xe",
            vk::PhysicalDeviceType::INTEGRATED_GPU,
            16384,
            true,
        );
        assert_eq!(select_gpu(&[integrada_rica, discreta_pobre], None), Some(1));
    }

    #[test]
    fn discreta_sin_presentacion_no_es_candidata_y_hay_aviso() {
        // El caso del equipo híbrido real: la dedicada no reporta cola de
        // presentación, gana la integrada y el aviso nombra a ambas.
        let mut discreta_sorda = nvidia();
        discreta_sorda.present_family = None;
        let devices = vec![intel(), discreta_sorda];
        let elegida = select_gpu(&devices, None).unwrap();
        assert_eq!(elegida, 0);
        let aviso = aviso_gpu(&devices, elegida).expect("debe haber aviso");
        assert!(aviso.contains("Intel(R) UHD Graphics"));
        assert!(aviso.contains("NVIDIA GeForce RTX 3060"));
        assert!(aviso.contains("sin cola de presentación"));
    }

    #[test]
    fn hibrido_sano_no_avisa() {
        let devices = vec![intel(), nvidia()];
        let elegida = select_gpu(&devices, None).unwrap();
        assert_eq!(aviso_gpu(&devices, elegida), None);
    }

    #[test]
    fn orden_integrada_antes_que_virtual_y_cpu() {
        let cpu = gpu("llvmpipe", vk::PhysicalDeviceType::CPU, 0, true);
        let virtual_gpu = gpu(
            "VirtualBox Graphics",
            vk::PhysicalDeviceType::VIRTUAL_GPU,
            0,
            true,
        );
        assert_eq!(select_gpu(&[cpu.clone(), virtual_gpu], None), Some(1));
        assert_eq!(select_gpu(&[cpu, intel()], None), Some(1));
    }

    #[test]
    fn solo_cpu_se_elige_y_lista_vacia_no() {
        let cpu = gpu("llvmpipe", vk::PhysicalDeviceType::CPU, 0, true);
        assert_eq!(select_gpu(&[cpu], None), Some(0));
        assert_eq!(select_gpu(&[], None), None);
    }

    #[test]
    fn sin_cola_de_graficos_no_es_candidata() {
        let mut rota = nvidia();
        rota.graphics_family = None;
        assert_eq!(select_gpu(&[rota], None), None);
    }

    #[test]
    fn sin_swapchain_no_es_candidata() {
        let mut sin_swap = nvidia();
        sin_swap.swapchain_supported = false;
        assert_eq!(select_gpu(&[sin_swap], None), None);
    }

    #[test]
    fn nombre_preferido_se_impone_a_la_discreta() {
        let devices = vec![intel(), nvidia()];
        assert_eq!(select_gpu(&devices, Some("intel")), Some(0));
        assert_eq!(select_gpu(&devices, Some("geforce")), Some(1));
    }
}
