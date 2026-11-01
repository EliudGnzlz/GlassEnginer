use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::path::Path;
use netcorehost::{
    hostfxr::{HostfxrContext, InitializedForRuntimeConfig},
    nethost,
    pdcstr,
    pdcstring::PdCString,
};
use thiserror::Error;
use crate::bindings::{get_transform, set_transform, log_message};

#[derive(Error, Debug)]
pub enum ScriptError {
    #[error("CoreCLR initialization failed: {0}")]
    InitFailed(String),
    #[error("Assembly not found: {0}")]
    AssemblyNotFound(String),
    #[error("Delegate resolution failed: {0}")]
    DelegateResolutionFailed(String),
    #[error("Script instantiation failed: {0}")]
    InstantiationFailed(String),
    #[error("Invalid C-String: {0}")]
    InvalidCString(String),
}

type InitBridgeFn = unsafe extern "system" fn(*const c_void, *const c_void, *const c_void);
type InitAudioBridgeFn = unsafe extern "system" fn(
    *const c_void,
    *const c_void,
    *const c_void,
    *const c_void,
    *const c_void,
);

#[repr(C)]
struct PhysicsBridgeCallbacks {
    create_body: *const c_void,
    attach_box: *const c_void,
    attach_sphere: *const c_void,
    apply_impulse: *const c_void,
    apply_force: *const c_void,
    get_transform: *const c_void,
    set_transform: *const c_void,
    remove_body: *const c_void,
}

type InitPhysicsBridgeFn = unsafe extern "system" fn(*const PhysicsBridgeCallbacks);
type CreateScriptInstanceFn = unsafe extern "system" fn(u64, *const c_char) -> i32;
type UpdateScriptInstanceFn = unsafe extern "system" fn(i32, f32);
type DestroyScriptInstanceFn = unsafe extern "system" fn(i32);

pub struct ScriptHost {
    pub is_initialized: bool,
    _context: Option<HostfxrContext<InitializedForRuntimeConfig>>,
    create_instance_fn: Option<CreateScriptInstanceFn>,
    update_instance_fn: Option<UpdateScriptInstanceFn>,
    destroy_instance_fn: Option<DestroyScriptInstanceFn>,
    active_instances: HashMap<i32, (u64, String)>, // handle -> (entity_id, script_name)
}

impl ScriptHost {
    pub fn new() -> Self {
        Self {
            is_initialized: false,
            _context: None,
            create_instance_fn: None,
            update_instance_fn: None,
            destroy_instance_fn: None,
            active_instances: HashMap::new(),
        }
    }

    /// Initializes .NET CoreCLR host from the managed assembly output directory.
    pub fn initialize(&mut self, assembly_dir: &Path) -> Result<(), ScriptError> {
        let dll_path = assembly_dir.join("GlassEngine.Core.dll");
        let config_path = assembly_dir.join("GlassEngine.Core.runtimeconfig.json");

        if !dll_path.exists() {
            return Err(ScriptError::AssemblyNotFound(dll_path.display().to_string()));
        }
        if !config_path.exists() {
            return Err(ScriptError::AssemblyNotFound(config_path.display().to_string()));
        }

        log::info!("Cargando hostfxr para .NET CoreCLR...");
        let hostfxr = nethost::load_hostfxr()
            .map_err(|e| ScriptError::InitFailed(format!("Error cargando libhostfxr: {e:?}")))?;

        let config_pdc = PdCString::from_os_str(config_path.as_os_str())
            .map_err(|e| ScriptError::InitFailed(format!("Error en config path: {e:?}")))?;
        let dll_pdc = PdCString::from_os_str(dll_path.as_os_str())
            .map_err(|e| ScriptError::InitFailed(format!("Error en dll path: {e:?}")))?;

        log::info!("Inicializando runtime CoreCLR desde: {}", config_path.display());
        let context = hostfxr.initialize_for_runtime_config(&config_pdc)
            .map_err(|e| ScriptError::InitFailed(format!("Error inicializando config: {e:?}")))?;

        let loader = context.get_delegate_loader_for_assembly(dll_pdc)
            .map_err(|e| ScriptError::InitFailed(format!("Error obteniendo loader: {e:?}")))?;

        log::info!("Resolviendo delegados [UnmanagedCallersOnly] en GlassEngine.Core...");
        let bridge_type = pdcstr!("GlassEngine.Core.RuntimeBridge, GlassEngine.Core");

        let init_fn = loader.get_function_with_unmanaged_callers_only::<InitBridgeFn>(
            bridge_type,
            pdcstr!("InitBridge"),
        ).map_err(|e| ScriptError::DelegateResolutionFailed(format!("InitBridge: {e:?}")))?;

        let create_fn = loader.get_function_with_unmanaged_callers_only::<CreateScriptInstanceFn>(
            bridge_type,
            pdcstr!("CreateScriptInstance"),
        ).map_err(|e| ScriptError::DelegateResolutionFailed(format!("CreateScriptInstance: {e:?}")))?;

        let update_fn = loader.get_function_with_unmanaged_callers_only::<UpdateScriptInstanceFn>(
            bridge_type,
            pdcstr!("UpdateScriptInstance"),
        ).map_err(|e| ScriptError::DelegateResolutionFailed(format!("UpdateScriptInstance: {e:?}")))?;

        let destroy_fn = loader.get_function_with_unmanaged_callers_only::<DestroyScriptInstanceFn>(
            bridge_type,
            pdcstr!("DestroyScriptInstance"),
        ).map_err(|e| ScriptError::DelegateResolutionFailed(format!("DestroyScriptInstance: {e:?}")))?;

        // Connect Rust callbacks to C# RuntimeBridge
        log::info!("Vinculando callbacks Rust <-> C# FFI...");
        unsafe {
            init_fn(
                get_transform as *const c_void,
                set_transform as *const c_void,
                log_message as *const c_void,
            );
        }

        if let Ok(init_audio_fn) = loader.get_function_with_unmanaged_callers_only::<InitAudioBridgeFn>(
            bridge_type,
            pdcstr!("InitAudioBridge"),
        ) {
            log::info!("Vinculando callbacks Rust <-> C# Audio FFI...");
            unsafe {
                init_audio_fn(
                    glass_audio::glass_audio_play_2d as *const c_void,
                    glass_audio::glass_audio_play_3d as *const c_void,
                    glass_audio::glass_audio_set_source_position as *const c_void,
                    glass_audio::glass_audio_set_listener_transform as *const c_void,
                    glass_audio::glass_audio_stop as *const c_void,
                );
            }
        }

        if let Ok(init_physics_fn) = loader.get_function_with_unmanaged_callers_only::<InitPhysicsBridgeFn>(
            bridge_type,
            pdcstr!("InitPhysicsBridge"),
        ) {
            log::info!("Vinculando callbacks Rust <-> C# Physics FFI...");
            let callbacks = PhysicsBridgeCallbacks {
                create_body: glass_physics::glass_physics_create_body as *const c_void,
                attach_box: glass_physics::glass_physics_attach_box_collider as *const c_void,
                attach_sphere: glass_physics::glass_physics_attach_sphere_collider as *const c_void,
                apply_impulse: glass_physics::glass_physics_apply_impulse as *const c_void,
                apply_force: glass_physics::glass_physics_apply_force as *const c_void,
                get_transform: glass_physics::glass_physics_get_transform as *const c_void,
                set_transform: glass_physics::glass_physics_set_transform as *const c_void,
                remove_body: glass_physics::glass_physics_remove_body as *const c_void,
            };
            unsafe {
                init_physics_fn(&callbacks as *const PhysicsBridgeCallbacks);
            }
        }

        self.create_instance_fn = Some(*create_fn);
        self.update_instance_fn = Some(*update_fn);
        self.destroy_instance_fn = Some(*destroy_fn);
        self._context = Some(context);
        self.is_initialized = true;

        log::info!("Host de Scripting .NET CoreCLR inicializado exitosamente.");
        Ok(())
    }

    /// Instantiates a C# script (e.g. "GlassEngine.Core.Scripts.Rotator") attached to an entity.
    pub fn create_instance(&mut self, entity_id: u64, script_name: &str) -> Result<i32, ScriptError> {
        let create_fn = self.create_instance_fn.ok_or_else(|| {
            ScriptError::InitFailed("Host no inicializado".to_string())
        })?;

        let c_script_name = CString::new(script_name)
            .map_err(|e| ScriptError::InvalidCString(e.to_string()))?;

        let handle = unsafe { create_fn(entity_id, c_script_name.as_ptr()) };
        if handle < 0 {
            return Err(ScriptError::InstantiationFailed(format!(
                "Fallo instanciando script '{script_name}' para entidad {entity_id}"
            )));
        }

        self.active_instances.insert(handle, (entity_id, script_name.to_string()));
        Ok(handle)
    }

    /// Dispatches an Update tick for a specific script instance.
    pub fn update_instance(&self, handle: i32, delta_time: f32) {
        if let Some(update_fn) = self.update_instance_fn {
            unsafe {
                update_fn(handle, delta_time);
            }
        }
    }

    /// Dispatches an Update tick for all registered active script instances.
    pub fn update_all(&self, delta_time: f32) {
        if let Some(update_fn) = self.update_instance_fn {
            for &handle in self.active_instances.keys() {
                unsafe {
                    update_fn(handle, delta_time);
                }
            }
        }
    }

    /// Destroys a script instance in the managed runtime.
    pub fn destroy_instance(&mut self, handle: i32) {
        if let Some(destroy_fn) = self.destroy_instance_fn {
            unsafe {
                destroy_fn(handle);
            }
        }
        self.active_instances.remove(&handle);
    }

    pub fn active_instance_count(&self) -> usize {
        self.active_instances.len()
    }
}
