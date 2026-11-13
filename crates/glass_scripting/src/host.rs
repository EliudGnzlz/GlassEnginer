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
// El puente de audio viaja como STRUCT de callbacks (como el de
// física): la resolución de delegados de netcorehost tiene un tope
// de aridad que los 7 punteros sueltos superaban (auditoría C6).
type InitAudioBridgeFn = unsafe extern "system" fn(*const AudioBridgeCallbacks);

#[repr(C)]
struct AudioBridgeCallbacks {
    play_2d: *const c_void,
    play_3d: *const c_void,
    set_source_position: *const c_void,
    set_listener_transform: *const c_void,
    stop: *const c_void,
    // Añadidos AL FINAL (auditoría C6): play 2D/3D con bus.
    play_2d_on_bus: *const c_void,
    play_3d_on_bus: *const c_void,
}

type InitParticleBridgeFn = unsafe extern "system" fn(
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
    get_velocity: *const c_void,
    set_velocity: *const c_void,
    // Añadido AL FINAL (juego Plat3D): un ensamblado managed anterior lee
    // solo los 10 punteros originales; este undécimo queda fuera de su
    // vista y no desplaza ningún offset existente.
    cast_ray: *const c_void,
    // Añadidos AL FINAL (bloque de primitivas y colliders por forma):
    // attach de cápsula y cilindro, duodécimo y decimotercer puntero,
    // con la misma convención: un ensamblado anterior no los lee y el
    // lado C# nuevo los tolera a null si un host viejo no los entrega.
    attach_capsule: *const c_void,
    attach_cylinder: *const c_void,
    // Añadidos AL FINAL (auditoría C5): consultas extendidas para C#.
    cast_sphere: *const c_void,
    overlap_sphere: *const c_void,
    // Añadido AL FINAL (Fase 9): resolver entidad → handle de cuerpo.
    get_body_handle: *const c_void,
}

type InitPhysicsBridgeFn = unsafe extern "system" fn(*const PhysicsBridgeCallbacks);
type InitInputBridgeFn = unsafe extern "system" fn(*const c_void);
type InitScriptParamsBridgeFn = unsafe extern "system" fn(*const c_void);
type InitSceneBridgeFn = unsafe extern "system" fn(
    *const c_void, // scene_load
    *const c_void, // scene_active_name
    *const c_void, // scene_count
    *const c_void, // scene_name_at
);
/// Puente de animación con piel (bloque de skinning): entrega a C# el
/// callback `animator_play` para que `Animator.Play` llegue al motor. Es
/// un export NUEVO y separado a propósito: `InitSceneBridge` ya fijó sus
/// 4 punteros posicionales y tocarlo rompería los ensamblados viejos.
type InitAnimationBridgeFn = unsafe extern "system" fn(
    *const c_void, // animator_play
);
/// Puente de la máquina de estados de animación (bloque FSM): entrega a
/// C# los callbacks de parámetros, estado actual y crossfade del
/// Animator (`Animator.SetBool/SetFloat/SetTrigger`,
/// `Animator.GetCurrentState`, `Animator.CrossFade`). Export NUEVO y
/// separado, como los demás.
type InitAnimatorFsmBridgeFn = unsafe extern "system" fn(
    *const c_void, // animator_set_param
    *const c_void, // animator_current_state
    *const c_void, // animator_crossfade
);

/// Puente de luces (bloque de luces): entrega a C# los callbacks para
/// cambiar el color y la intensidad de la luz de una entidad
/// (`Light.color`, `Light.intensity`). Export NUEVO y separado, como los
/// demás.
type InitLightBridgeFn = unsafe extern "system" fn(
    *const c_void, // light_set_color
    *const c_void, // light_set_intensity
);

/// Puente de propiedades de luz (integración de iluminación): tipo,
/// rango y ángulos Spot de `Light`. Export NUEVO y separado, como los
/// demás.
type InitLightPropsBridgeFn = unsafe extern "system" fn(
    *const c_void, // light_set_type
    *const c_void, // light_set_range
    *const c_void, // light_set_spot_angles
);
/// Puente de UI dinámica (Canvas v2): entrega a C# los callbacks
/// para mutar texto, imagen y rect de las entidades con UI
/// (clases Text, Image y RectTransform). Export NUEVO y separado.
#[repr(C)]
struct UiBridgeCallbacks {
    ui_set_text: *const c_void,
    ui_set_font_size: *const c_void,
    ui_set_text_color: *const c_void,
    ui_set_image_sprite: *const c_void,
    ui_set_image_color: *const c_void,
    ui_set_button_color: *const c_void,
    ui_set_anchored_pos: *const c_void,
    ui_set_size_delta: *const c_void,
    // Añadidos AL FINAL (drag & drop): getters del último gesto.
    ui_get_dragged_id: *const c_void,
    ui_get_drop_target_id: *const c_void,
}

type InitUiBridgeFn = unsafe extern "system" fn(*const UiBridgeCallbacks);

/// Puente de consulta de ejes de entrada (Input v2): entrega al
/// managed los callbacks para contar los ejes configurados y
/// leer su nombre por índice (su caché nombre → índice). Export
/// NUEVO y separado, como los demás.
#[repr(C)]
pub struct InputAxesCallbacks {
    /// `crate::bindings::input_axis_count`
    input_axis_count: *const c_void,
    /// `crate::bindings::input_axis_name`
    input_axis_name: *const c_void,
}

type InitInputAxesBridgeFn = unsafe extern "system" fn(*const InputAxesCallbacks);
/// Puente del punto de interacción de los grafos (bloque homónimo):
/// entrega a C# los callbacks para publicar y limpiar el vec4 global que
/// leen los shaders de grafo (`Shader.SetInteractionPoint`). Export NUEVO
/// y separado, como el de animación.
type InitInteractionBridgeFn = unsafe extern "system" fn(
    *const c_void, // graph_set_interaction_point
    *const c_void, // graph_clear_interaction_point
);
/// Puente de la huella de caja del canal de interacción (bloque
/// «huella con forma»): entrega a C# el callback para publicar la
/// interacción con forma de caja (`Shader.SetInteractionBox`). Export
/// NUEVO y separado, como los demás: `InitInteractionBridge` ya fijó
/// sus 2 punteros posicionales y tocarlo rompería los ensamblados que
/// lo importan.
type InitInteractionBoxBridgeFn = unsafe extern "system" fn(
    *const c_void, // graph_set_interaction_box
);
/// Puente de instanciación real de entidades (bloque homónimo):
/// entrega a C# los callbacks para crear, destruir, activar/desactivar
/// entidades de la escena viva y consultar sus conteos
/// (`GameObject.Spawn`, `Object.Destroy`, `EntityPool`). Export NUEVO y
/// separado, como los demás: tocar uno existente rompería los
/// ensamblados que ya lo importan con sus punteros posicionales.
type InitSpawnBridgeFn = unsafe extern "system" fn(
    *const c_void, // entity_spawn
    *const c_void, // entity_destroy
    *const c_void, // entity_set_active
    *const c_void, // entity_count
    *const c_void, // entity_physics_counts
);
/// Puente de instanciación de Prefabs desde C# (bloque homónimo):
/// export nuevo y separado, como los demás; entrega un único callback
/// (`entity_instantiate_prefab`) al ensamblado managed.
type InitPrefabBridgeFn = unsafe extern "system" fn(
    *const c_void, // entity_instantiate_prefab
);
/// Puente de niebla en vivo (afinado visual de la Fase 9): entrega a
/// C# el callback para fijar la niebla de la escena desde un script
/// (`RuntimeBridge.SetFog`, sistemas de clima). Export NUEVO y
/// separado, como los demás.
type InitFogBridgeFn = unsafe extern "system" fn(
    *const c_void, // scene_set_fog
);
/// Puente de FOV de la cámara principal (Fase 10): entrega a C# el
/// callback para fijar el FOV desde un script
/// (`RuntimeBridge.SetCameraFov`, miras con zoom). Export NUEVO y
/// separado, como los demás.
type InitCameraFovBridgeFn = unsafe extern "system" fn(
    *const c_void, // scene_set_camera_fov
);
type CreateScriptInstanceFn = unsafe extern "system" fn(u64, *const c_char) -> i32;
type UpdateScriptInstanceFn = unsafe extern "system" fn(i32, f32);
type DestroyScriptInstanceFn = unsafe extern "system" fn(i32);
/// Entrega el clic de un botón del Canvas al runtime C#: entidad pulsada y
/// nombre de la acción (`UIButtonData.on_click_action`). Devuelve 0 si el
/// método nombrado se invocó; códigos negativos según el motivo (ver
/// `RuntimeBridge.InvokeButtonClick` en el lado C#).
type InvokeButtonClickFn = unsafe extern "system" fn(u64, *const c_char) -> i32;
/// Tick gestionado por lotes (bloque «FFI por lotes»): una sola
/// llamada por frame para todos los scripts; el gestionado itera
/// sus instancias y trabaja sobre la caché de transforms del frame.
type TickAllScriptsFn = unsafe extern "system" fn(f32);
/// Export managed de la Fase 1: (entidad, otra entidad, kind, es_trigger).
type DispatchContactEventFn = unsafe extern "system" fn(u64, u64, i32, i32, *const std::ffi::c_char);

/// Evento de animación hacia un script C#: (entidad, nombre del evento,
/// valor numérico, hay_valor, texto opcional).
type DispatchAnimEventFn = unsafe extern "system" fn(u64, *const std::ffi::c_char, f32, i32, *const std::ffi::c_char);
/// Cambio de estado de la FSM: (entidad, nombre del estado, entró 1/0).
type DispatchStateEventFn = unsafe extern "system" fn(u64, *const std::ffi::c_char, i32);
/// Init dedicado del lote de transforms (patrón de la casa: Init
/// nuevo y separado; nunca se toca un Init existente). Recibe los
/// dos punteros que C# usará para pedir y devolver el lote completo
/// de transforms en un solo cruce.
type InitTransformBatchBridgeFn = unsafe extern "system" fn(*const c_void, *const c_void);

pub struct ScriptHost {
    pub is_initialized: bool,
    _context: Option<HostfxrContext<InitializedForRuntimeConfig>>,
    create_instance_fn: Option<CreateScriptInstanceFn>,
    update_instance_fn: Option<UpdateScriptInstanceFn>,
    destroy_instance_fn: Option<DestroyScriptInstanceFn>,
    invoke_click_fn: Option<InvokeButtonClickFn>,
    /// Si existe, `update_all` hace UNA llamada gestionada por
    /// frame en lugar de una por instancia activa.
    tick_all_fn: Option<TickAllScriptsFn>,
    contact_event_fn: Option<DispatchContactEventFn>,
    anim_event_fn: Option<DispatchAnimEventFn>,
    state_event_fn: Option<DispatchStateEventFn>,
    active_instances: HashMap<i32, (u64, String)>, // handle -> (entity_id, script_name)
}

impl Default for ScriptHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptHost {
    pub fn new() -> Self {
        Self {
            is_initialized: false,
            _context: None,
            create_instance_fn: None,
            update_instance_fn: None,
            destroy_instance_fn: None,
            invoke_click_fn: None,
            tick_all_fn: None,
            contact_event_fn: None,
            anim_event_fn: None,
            state_event_fn: None,
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
            let audio_callbacks = AudioBridgeCallbacks {
                play_2d: glass_audio::glass_audio_play_2d as *const c_void,
                play_3d: glass_audio::glass_audio_play_3d as *const c_void,
                set_source_position: glass_audio::glass_audio_set_source_position as *const c_void,
                set_listener_transform: glass_audio::glass_audio_set_listener_transform as *const c_void,
                stop: glass_audio::glass_audio_stop as *const c_void,
                play_2d_on_bus: glass_audio::glass_audio_play_2d_on_bus as *const c_void,
                play_3d_on_bus: glass_audio::glass_audio_play_3d_on_bus as *const c_void,
            };
            unsafe {
                init_audio_fn(&audio_callbacks as *const AudioBridgeCallbacks);
            }
        }

        if let Ok(init_particles_fn) = loader
            .get_function_with_unmanaged_callers_only::<InitParticleBridgeFn>(
                bridge_type,
                pdcstr!("InitParticleBridge"),
            )
        {
            log::info!("Vinculando callbacks Rust <-> C# Particles FFI...");
            unsafe {
                init_particles_fn(
                    glass_particles::glass_particles_play as *const c_void,
                    glass_particles::glass_particles_stop as *const c_void,
                    glass_particles::glass_particles_burst as *const c_void,
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
                get_velocity: glass_physics::glass_physics_get_linear_velocity as *const c_void,
                set_velocity: glass_physics::glass_physics_set_linear_velocity as *const c_void,
                cast_ray: glass_physics::glass_physics_cast_ray as *const c_void,
                attach_capsule: glass_physics::glass_physics_attach_capsule_collider as *const c_void,
                attach_cylinder: glass_physics::glass_physics_attach_cylinder_collider as *const c_void,
                cast_sphere: glass_physics::glass_physics_cast_sphere as *const c_void,
                overlap_sphere: glass_physics::glass_physics_overlap_sphere as *const c_void,
                get_body_handle: glass_physics::glass_physics_body_handle as *const c_void,
            };
            unsafe {
                init_physics_fn(&callbacks as *const PhysicsBridgeCallbacks);
            }
        }

        if let Ok(init_input_fn) = loader.get_function_with_unmanaged_callers_only::<InitInputBridgeFn>(
            bridge_type,
            pdcstr!("InitInputBridge"),
        ) {
            log::info!("Vinculando callbacks Rust <-> C# Input FFI...");
            unsafe {
                init_input_fn(crate::bindings::get_input as *const c_void);
            }
        }
        // Consulta de ejes (Input v2): opcional como los demás;
        // sin ella el managed no resuelve nombres y GetAxis cae a
        // los ejes heredados.
        if let Ok(init_axes_fn) = loader
            .get_function_with_unmanaged_callers_only::<InitInputAxesBridgeFn>(
                bridge_type,
                pdcstr!("InitInputAxesBridge"),
            )
        {
            log::info!("Vinculando callbacks Rust <-> C# InputAxes FFI...");
            unsafe {
                init_axes_fn(&InputAxesCallbacks {
                    input_axis_count: crate::bindings::input_axis_count as *const c_void,
                    input_axis_name: crate::bindings::input_axis_name as *const c_void,
                });
            }
        } else {
            log::warn!(
                "InputAxes no estará disponible: el ensamblado managed \
                 no exporta InitInputAxesBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }

        if let Ok(init_params_fn) = loader.get_function_with_unmanaged_callers_only::<InitScriptParamsBridgeFn>(
            bridge_type,
            pdcstr!("InitScriptParamsBridge"),
        ) {
            log::info!("Vinculando callbacks Rust <-> C# ScriptParams FFI...");
            unsafe {
                init_params_fn(crate::bindings::get_script_params_ffi as *const c_void);
            }
        }

        // Puente de escenas: permite que `SceneManager.LoadScene` de C# pida una
        // carga real. Es opcional a propósito: si el ensamblado managed es viejo
        // y no exporta `InitSceneBridge`, el motor arranca igual y C# recibe
        // -4 (puente no disponible) en lugar de tumbar el proceso.
        if let Ok(init_scene_fn) = loader.get_function_with_unmanaged_callers_only::<InitSceneBridgeFn>(
            bridge_type,
            pdcstr!("InitSceneBridge"),
        ) {
            log::info!("Vinculando callbacks Rust <-> C# Scene FFI...");
            unsafe {
                init_scene_fn(
                    crate::bindings::scene_load as *const c_void,
                    crate::bindings::scene_active_name as *const c_void,
                    crate::bindings::scene_count as *const c_void,
                    crate::bindings::scene_name_at as *const c_void,
                );
            }
        } else {
            log::warn!(
                "SceneManager.LoadScene no estara disponible: el ensamblado managed \
                 no exporta InitSceneBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }

        // Puente de animación: opcional como los demás. Un ensamblado
        // anterior al bloque de skinning no exporta `InitAnimationBridge`;
        // el motor arranca igual y `Animator.Play` de C# responderá -4
        // (puente no conectado) hasta recompilar el managed.
        if let Ok(init_anim_fn) = loader.get_function_with_unmanaged_callers_only::<InitAnimationBridgeFn>(
            bridge_type,
            pdcstr!("InitAnimationBridge"),
        ) {
            log::info!("Vinculando callbacks Rust <-> C# Animation FFI...");
            unsafe {
                init_anim_fn(crate::bindings::animator_play as *const c_void);
            }
        } else {
            log::warn!(
                "Animator.Play no estara disponible: el ensamblado managed \
                 no exporta InitAnimationBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }

        // Puente de la máquina de estados de animación: opcional como
        // los demás. Un ensamblado anterior a este bloque no exporta
        // `InitAnimatorFsmBridge`; el motor arranca igual y los métodos
        // FSM del Animator responderán -4 (puente no conectado) hasta
        // recompilar el managed. `Animator.Play` no se ve afectado:
        // viaja por el puente de animación de siempre.
        if let Ok(init_fsm_fn) = loader.get_function_with_unmanaged_callers_only::<InitAnimatorFsmBridgeFn>(
            bridge_type,
            pdcstr!("InitAnimatorFsmBridge"),
        ) {
            log::info!("Vinculando callbacks Rust <-> C# Animator FSM FFI...");
            unsafe {
                init_fsm_fn(
                    crate::bindings::animator_set_param as *const c_void,
                    crate::bindings::animator_current_state as *const c_void,
                    crate::bindings::animator_crossfade as *const c_void,
                );
            }
        } else {
            log::warn!(
                "Animator FSM no estara disponible: el ensamblado managed \
                 no exporta InitAnimatorFsmBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }

        // Puente de luces: opcional como los demás. Un ensamblado
        // anterior a este bloque no exporta `InitLightBridge`; el motor
        // arranca igual y los setters de `Light` devolverán -4 (puente
        // no conectado) hasta recompilar el managed.
        if let Ok(init_light_fn) = loader
            .get_function_with_unmanaged_callers_only::<InitLightBridgeFn>(
                bridge_type,
                pdcstr!("InitLightBridge"),
            )
        {
            log::info!("Vinculando callbacks Rust <-> C# Light FFI...");
            unsafe {
                init_light_fn(
                    crate::bindings::light_set_color as *const c_void,
                    crate::bindings::light_set_intensity as *const c_void,
                );
            }
        } else {
            log::warn!(
                "Luces no estará disponible: el ensamblado managed \
                 no exporta InitLightBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }

        if let Ok(init_light_props_fn) = loader.get_function_with_unmanaged_callers_only::<InitLightPropsBridgeFn>(
            bridge_type,
            pdcstr!("InitLightPropsBridge"),
        ) {
            log::info!("Vinculando callbacks Rust <-> C# LightProps FFI...");
            unsafe {
                init_light_props_fn(
                    crate::bindings::light_set_type as *const c_void,
                    crate::bindings::light_set_range as *const c_void,
                    crate::bindings::light_set_spot_angles as *const c_void,
                );
            }
        } else {
            log::warn!(
                "Propiedades de luz no estarán disponibles: el ensamblado managed \
                 no exporta InitLightPropsBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }
        // Puente de UI dinámica: opcional como los demás; los
        // setters devolverán -4 hasta recompilar el managed.
        if let Ok(init_ui_fn) = loader
            .get_function_with_unmanaged_callers_only::<InitUiBridgeFn>(
                bridge_type,
                pdcstr!("InitUiBridge"),
            )
        {
            log::info!("Vinculando callbacks Rust <-> C# UI dinámica FFI...");
            unsafe {
                init_ui_fn(&UiBridgeCallbacks {
                    ui_set_text: crate::bindings::ui_set_text as *const c_void,
                    ui_set_font_size: crate::bindings::ui_set_font_size as *const c_void,
                    ui_set_text_color: crate::bindings::ui_set_text_color as *const c_void,
                    ui_set_image_sprite: crate::bindings::ui_set_image_sprite as *const c_void,
                    ui_set_image_color: crate::bindings::ui_set_image_color as *const c_void,
                    ui_set_button_color: crate::bindings::ui_set_button_color as *const c_void,
                    ui_set_anchored_pos: crate::bindings::ui_set_anchored_pos as *const c_void,
                    ui_set_size_delta: crate::bindings::ui_set_size_delta as *const c_void,
                    ui_get_dragged_id: crate::bindings::ui_get_dragged_id as *const c_void,
                    ui_get_drop_target_id: crate::bindings::ui_get_drop_target_id
                        as *const c_void,
                });
            }
        } else {
            log::warn!(
                "UI dinámica no estará disponible: el ensamblado managed \
                 no exporta InitUiBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }
        // Puente del punto de interacción: opcional como los demás. Un
        // ensamblado anterior a este bloque no exporta
        // `InitInteractionBridge`; el motor arranca igual y
        // `Shader.SetInteractionPoint` de C# será un no-op silencioso
        // hasta recompilar el managed.
        if let Ok(init_interaction_fn) = loader
            .get_function_with_unmanaged_callers_only::<InitInteractionBridgeFn>(
                bridge_type,
                pdcstr!("InitInteractionBridge"),
            )
        {
            log::info!("Vinculando callbacks Rust <-> C# Interaction FFI...");
            unsafe {
                init_interaction_fn(
                    crate::bindings::graph_set_interaction_point as *const c_void,
                    crate::bindings::graph_clear_interaction_point as *const c_void,
                );
            }
        } else {
            log::warn!(
                "Shader.SetInteractionPoint no estara disponible: el ensamblado managed \
                 no exporta InitInteractionBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }

        // Puente de la huella de caja: opcional e independiente del
        // anterior. Un ensamblado anterior a este bloque no exporta
        // `InitInteractionBoxBridge`; el motor arranca igual y
        // `Shader.SetInteractionBox` de C# será un no-op silencioso
        // hasta recompilar el managed (la publicación esférica, que
        // viaja por el puente anterior, no se ve afectada).
        if let Ok(init_interaction_box_fn) = loader
            .get_function_with_unmanaged_callers_only::<InitInteractionBoxBridgeFn>(
                bridge_type,
                pdcstr!("InitInteractionBoxBridge"),
            )
        {
            log::info!("Vinculando callbacks Rust <-> C# InteractionBox FFI...");
            unsafe {
                init_interaction_box_fn(
                    crate::bindings::graph_set_interaction_box as *const c_void,
                );
            }
        } else {
            log::warn!(
                "Shader.SetInteractionBox no estara disponible: el ensamblado managed \
                 no exporta InitInteractionBoxBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }

        // Puente de instanciación real: opcional e independiente, como
        // los demás. Un ensamblado anterior a este bloque no exporta
        // `InitSpawnBridge`; el motor arranca igual y
        // `GameObject.Spawn`/`Object.Destroy`/`EntityPool` de C#
        // responderán error controlado hasta recompilar el managed.
        if let Ok(init_spawn_fn) = loader
            .get_function_with_unmanaged_callers_only::<InitSpawnBridgeFn>(
                bridge_type,
                pdcstr!("InitSpawnBridge"),
            )
        {
            log::info!("Vinculando callbacks Rust <-> C# Spawn FFI...");
            unsafe {
                init_spawn_fn(
                    crate::bindings::entity_spawn as *const c_void,
                    crate::bindings::entity_destroy as *const c_void,
                    crate::bindings::entity_set_active as *const c_void,
                    crate::bindings::entity_count as *const c_void,
                    crate::bindings::entity_physics_counts as *const c_void,
                );
            }
        } else {
            log::warn!(
                "GameObject.Spawn/Destroy no estaran disponibles: el ensamblado managed \
                 no exporta InitSpawnBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }

        // Puente de prefabs: opcional e independiente, como los demás.
        // Un ensamblado anterior a este bloque no exporta
        // `InitPrefabBridge`; el motor arranca igual y
        // `GameObject.InstantiatePrefab` responderá error controlado
        // hasta recompilar el managed.
        if let Ok(init_prefab_fn) = loader
            .get_function_with_unmanaged_callers_only::<InitPrefabBridgeFn>(
                bridge_type,
                pdcstr!("InitPrefabBridge"),
            )
        {
            log::info!("Vinculando callbacks Rust <-> C# Prefab FFI...");
            unsafe {
                init_prefab_fn(
                    crate::bindings::entity_instantiate_prefab as *const c_void,
                );
            }
        } else {
            log::warn!(
                "GameObject.InstantiatePrefab no estara disponible: el ensamblado managed \
                 no exporta InitPrefabBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }

        // Puente de niebla en vivo: opcional e independiente, como los
        // demás. Un ensamblado anterior no exporta `InitFogBridge`; el
        // motor arranca igual y `RuntimeBridge.SetFog` devolverá false.
        if let Ok(init_fog_fn) = loader
            .get_function_with_unmanaged_callers_only::<InitFogBridgeFn>(
                bridge_type,
                pdcstr!("InitFogBridge"),
            )
        {
            log::info!("Vinculando callbacks Rust <-> C# Fog FFI...");
            unsafe {
                init_fog_fn(
                    crate::bindings::scene_set_fog as *const c_void,
                );
            }
        } else {
            log::warn!(
                "RuntimeBridge.SetFog no estara disponible: el ensamblado managed \
                 no exporta InitFogBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }

        // Puente de FOV de cámara: opcional e independiente, como los
        // demás. Un ensamblado anterior no exporta
        // `InitCameraFovBridge`; el motor arranca igual y
        // `RuntimeBridge.SetCameraFov` devolverá false.
        if let Ok(init_fov_fn) = loader
            .get_function_with_unmanaged_callers_only::<InitCameraFovBridgeFn>(
                bridge_type,
                pdcstr!("InitCameraFovBridge"),
            )
        {
            log::info!("Vinculando callbacks Rust <-> C# CameraFov FFI...");
            unsafe {
                init_fov_fn(
                    crate::bindings::scene_set_camera_fov as *const c_void,
                );
            }
        } else {
            log::warn!(
                "RuntimeBridge.SetCameraFov no estara disponible: el ensamblado managed \
                 no exporta InitCameraFovBridge (¿GlassEngine.Core.dll desactualizado?)."
            );
        }

        // Delegado opcional, como el puente de escenas: un ensamblado managed
        // anterior a este bloque no exporta `InvokeButtonClick`, y en ese caso
        // el motor debe arrancar igual — los clics de botón se registran en el
        // log desde Rust pero no llegan a C# hasta recompilar el managed.
        let invoke_click_fn = loader
            .get_function_with_unmanaged_callers_only::<InvokeButtonClickFn>(
                bridge_type,
                pdcstr!("InvokeButtonClick"),
            )
            .ok();
        if invoke_click_fn.is_none() {
            log::warn!(
                "Los onClick de los botones del Canvas no llegaran a C#: el ensamblado \
                 managed no exporta InvokeButtonClick (¿GlassEngine.Core.dll desactualizado?)."
            );
            // Ese mismo ensamblado trae el `InputData` anterior al bloque
            // de ratón (12 bytes, sin campos de ratón): si `get_input`
            // escribiera ahí los 24 bytes actuales desbordaría el buffer
            // que C# reserva en su pila y corrompería el frame gestionado
            // (segfault real cazado en el binario puro sin argumentos).
            // Con el marcador ausente se sirve solo el prefijo heredado,
            // byte a byte idéntico al layout viejo.
            crate::bindings::set_legacy_input_layout(true);
            log::warn!(
                "Puente de input en modo heredado: el ensamblado managed es anterior \
                 al contrato de input con ratón; se serviran solo los primeros {} bytes \
                 de InputDataFfi (ejes y botones, sin ratón) hasta recompilar GlassEngine.Core.",
                crate::bindings::LEGACY_INPUT_DATA_LEN
            );
        }

        // Tick por lotes e Init del lote de transforms (bloque «FFI
        // por lotes»): resolución tolerante como el resto de
        // opcionales. Un ensamblado anterior a este bloque no los
        // exporta y el motor arranca igual con el bucle por
        // instancia y el FFI de transform por entidad.
        let tick_all_fn = loader
            .get_function_with_unmanaged_callers_only::<TickAllScriptsFn>(
                bridge_type,
                pdcstr!("TickAllScripts"),
            )
            .ok();
        if tick_all_fn.is_none() {
            log::warn!(
                "El ensamblado managed no exporta TickAllScripts: se mantiene \
                 el tick por instancia (¿GlassEngine.Core.dll desactualizado?)."
            );
        }
        let init_transform_batch_fn = loader
            .get_function_with_unmanaged_callers_only::<InitTransformBatchBridgeFn>(
                bridge_type,
                pdcstr!("InitTransformBatchBridge"),
            )
            .ok();
        match init_transform_batch_fn {
            Some(init_fn) => unsafe {
                init_fn(
                    crate::bindings::get_transforms_batch as *const c_void,
                    crate::bindings::set_transforms_batch as *const c_void,
                );
            },
            None => {
                log::warn!(
                    "El ensamblado managed no exporta InitTransformBatchBridge: \
                     el lote de transforms queda inactivo."
                );
            }
        }

        // Eventos de contacto (Fase 1): resolución tolerante — un
        // ensamblado anterior no exporta DispatchContactEvent y el motor
        // arranca igual, simplemente sin callbacks de contacto.
        let contact_event_fn = loader
            .get_function_with_unmanaged_callers_only::<DispatchContactEventFn>(
                bridge_type,
                pdcstr!("DispatchContactEvent"),
            )
            .ok();
        if contact_event_fn.is_none() {
            log::warn!(
                "El ensamblado managed no exporta DispatchContactEvent: los \
                 callbacks OnCollision*/OnTrigger* quedan inactivos."
            );
        }
        self.contact_event_fn = contact_event_fn.map(|f| *f);

        // Eventos de animación: misma resolución tolerante — si el
        // ensamblado no los exporta, el motor arranca igual y los
        // eventos simplemente no llegan a los scripts.
        let anim_event_fn = loader
            .get_function_with_unmanaged_callers_only::<DispatchAnimEventFn>(
                bridge_type,
                pdcstr!("DispatchAnimEvent"),
            )
            .ok();
        if anim_event_fn.is_none() {
            log::warn!(
                "El ensamblado managed no exporta DispatchAnimEvent: los eventos de animación no llegarán a los scripts."
            );
        }
        self.anim_event_fn = anim_event_fn.map(|f| *f);

        // Cambios de estado de la FSM: misma resolución tolerante — si el
        // ensamblado no los exporta, el motor arranca igual y los
        // cambios simplemente no llegan a los scripts.
        let state_event_fn = loader
            .get_function_with_unmanaged_callers_only::<DispatchStateEventFn>(
                bridge_type,
                pdcstr!("DispatchStateEvent"),
            )
            .ok();
        if state_event_fn.is_none() {
            log::warn!(
                "El ensamblado managed no exporta DispatchStateEvent: los cambios de estado no llegarán a los scripts."
            );
        }
        self.state_event_fn = state_event_fn.map(|f| *f);
        self.tick_all_fn = tick_all_fn.map(|f| *f);
        self.invoke_click_fn = invoke_click_fn.map(|f| *f);
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
    ///
    /// Camino nuevo (bloque «FFI por lotes»): si el gestionado
    /// exporta `TickAllScripts`, UNA sola llamada por frame y C#
    /// itera sus instancias trabajando sobre la caché de transforms
    /// del frame. Si no, el bucle clásico por instancia.
    pub fn update_all(&self, delta_time: f32) {
        if let Some(tick_all_fn) = self.tick_all_fn {
            unsafe {
                tick_all_fn(delta_time);
            }
            return;
        }
        if let Some(update_fn) = self.update_instance_fn {
            for &handle in self.active_instances.keys() {
                unsafe {
                    update_fn(handle, delta_time);
                }
            }
        }
    }

    /// Entrega un evento de contacto a los scripts de UNA entidad (la
    /// otra va como «other», con su nombre de escena para que el
    /// `Collision.gameObject` gestionado nazca con el nombre real).
    /// kind: 0 = Enter, 1 = Stay, 2 = Exit.
    /// Devuelve false si el ensamblado no exporta DispatchContactEvent.
    pub fn dispatch_contact_event(
        &self,
        entity_id: u64,
        other_entity_id: u64,
        kind: i32,
        is_trigger: bool,
        other_name: &str,
    ) -> bool {
        match self.contact_event_fn {
            Some(f) => {
                let name_c = std::ffi::CString::new(other_name).unwrap_or_default();
                unsafe { f(entity_id, other_entity_id, kind, is_trigger as i32, name_c.as_ptr()) };
                true
            }
            None => false,
        }
    }

    /// Entrega un cambio de estado de la FSM a los scripts de UNA
    /// entidad (StateMachineBehaviours: OnStateEnter/OnStateExit).
    /// Devuelve false si el ensamblado no exporta DispatchStateEvent.
    pub fn dispatch_state_event(&self, entity_id: u64, state: &str, entered: bool) -> bool {
        match self.state_event_fn {
            Some(f) => {
                let state_c = std::ffi::CString::new(state).unwrap_or_default();
                unsafe { f(entity_id, state_c.as_ptr(), entered as i32) };
                true
            }
            None => false,
        }
    }

    /// Entrega un evento de animación a los scripts de UNA entidad.
    /// Devuelve false si el ensamblado no exporta DispatchAnimEvent.
    pub fn dispatch_anim_event(
        &self,
        entity_id: u64,
        name: &str,
        value: Option<f32>,
        text: Option<&str>,
    ) -> bool {
        match self.anim_event_fn {
            Some(f) => {
                let name_c = std::ffi::CString::new(name).unwrap_or_default();
                let text_c = text.map(|t| std::ffi::CString::new(t).unwrap_or_default());
                let (v, has) = match value {
                    Some(v) => (v, 1),
                    None => (0.0, 0),
                };
                unsafe {
                    f(
                        entity_id,
                        name_c.as_ptr(),
                        v,
                        has,
                        text_c
                            .as_ref()
                            .map(|c| c.as_ptr())
                            .unwrap_or(std::ptr::null()),
                    )
                };
                true
            }
            None => false,
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

    /// Destruye todas las instancias activas.
    ///
    /// Lo usa el cambio de escena: al sustituir el conjunto de entidades, los
    /// ids viejos dejan de existir, así que cualquier `GlassBehaviour` que
    /// sobreviva queda con un `entityId` colgando y sus `GetTransform` fallan en
    /// silencio. Se destruyen antes de instanciar los scripts de la escena nueva.
    pub fn destroy_all_instances(&mut self) {
        let handles: Vec<i32> = self.active_instances.keys().copied().collect();
        for handle in handles {
            self.destroy_instance(handle);
        }
    }

    /// Entrega al runtime C# el clic de un botón del Canvas: la entidad del
    /// botón y el nombre de acción de su `UIButtonData.on_click_action`.
    /// El lado C# busca la instancia de script de ESA entidad e invoca el
    /// método sin parámetros con ese nombre.
    ///
    /// Devuelve `true` si el delegado managed existe y la llamada se hizo
    /// (el código de retorno de C# se registra aquí si no es 0: -1 sin
    /// instancia en la entidad, -2 método no encontrado, -3 acción vacía,
    /// -4 excepción en el método). Devuelve `false` si el ensamblado no
    /// exporta el delegado o la acción no es una cadena C válida: el
    /// llamante ya registra el clic en el log, así que nada se pierde en
    /// silencio.
    pub fn invoke_button_click(&self, entity_id: u64, action: &str) -> bool {
        let Some(invoke_fn) = self.invoke_click_fn else {
            return false;
        };
        let Ok(c_action) = CString::new(action) else {
            log::warn!(
                "[Canvas UI] on_click_action de la entidad {entity_id} no es una cadena C válida: {action:?}"
            );
            return false;
        };
        let rc = unsafe { invoke_fn(entity_id, c_action.as_ptr()) };
        if rc != 0 {
            log::warn!(
                "[Canvas UI] InvokeButtonClick(entidad {entity_id}, acción '{action}') devolvió {rc}."
            );
        }
        true
    }

    /// Destruye las instancias cuya entidad ya no está en `alive`
    /// (reconciliación del bloque de instanciación): una entidad
    /// destruida desde C# —o borrada en el editor— deja su
    /// `GlassBehaviour` huérfano si nadie lo dispone, y su `OnDestroy`
    /// nunca correría. La app llama a esto cada frame antes de
    /// `update_all`, con el conjunto de ids de la escena viva.
    ///
    /// La lista de huérfanos se calcula ANTES de destruir ninguna:
    /// `OnDestroy` puede a su vez destruir otras entidades, y esas se
    /// recogen en la reconciliación del frame siguiente, no en una
    /// cascada recursiva dentro de esta. Devuelve cuántas destruyó.
    pub fn destroy_instances_not_in(&mut self, alive: &std::collections::HashSet<u64>) -> usize {
        let orphans: Vec<i32> = self
            .active_instances
            .iter()
            .filter(|(_, (entity_id, _))| !alive.contains(entity_id))
            .map(|(handle, _)| *handle)
            .collect();
        let n = orphans.len();
        for handle in orphans {
            self.destroy_instance(handle);
        }
        n
    }

    /// Id de entidad asociado a una instancia, si sigue viva.
    pub fn entity_of_instance(&self, handle: i32) -> Option<u64> {
        self.active_instances.get(&handle).map(|(id, _)| *id)
    }

    pub fn active_instance_count(&self) -> usize {
        self.active_instances.len()
    }
}
