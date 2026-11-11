use std::ffi::{c_char, CStr};

pub use glass_core::TransformFfi;

// Compile-time sanity check for memory alignment and size
const _: () = {
    assert!(std::mem::size_of::<TransformFfi>() == 40);
    assert!(std::mem::align_of::<TransformFfi>() == 4);
};

pub const BRIDGE_OK: i32 = 0;
pub const BRIDGE_ERR_NOT_FOUND: i32 = -1;
pub const BRIDGE_ERR_NULL_PTR: i32 = -2;
pub const BRIDGE_ERR_BUFFER_TOO_SMALL: i32 = -3;
pub const BRIDGE_ERR_BRIDGE: i32 = -4;

/// Copia `text` en el buffer C de `cap` bytes **incluyendo el terminador NUL**.
///
/// Devuelve la longitud escrita sin el NUL, o un código de error negativo. El
/// caso `cap == 0` no es un error: significa "sólo dime cuánto necesito", lo que
/// permite a C# dimensionar el buffer sin adivinar. Por eso `cap == 0` se
/// comprueba **antes** que el puntero nulo: en modo consulta el llamante pasa
/// legítimamente `NULL`, y responderle `BRIDGE_ERR_NULL_PTR` le dejaría sin
/// forma de calcular el tamaño.
unsafe fn copy_str_to_buf(text: &str, out: *mut c_char, cap: usize) -> i32 {
    let bytes = text.as_bytes();
    if cap == 0 {
        return bytes.len() as i32;
    }
    if out.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    let needed = bytes.len() + 1; // + NUL
    if cap < needed {
        return BRIDGE_ERR_BUFFER_TOO_SMALL;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, bytes.len());
        *out.add(bytes.len()) = 0;
    }
    bytes.len() as i32
}

/// Native callback invoked by C# to query an entity's transform.
pub unsafe extern "C" fn get_transform(entity_id: u64, out: *mut TransformFfi) -> i32 {
    if out.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    match crate::bridge::read_entity_transform(entity_id) {
        Some(t) => {
            unsafe { *out = t };
            BRIDGE_OK
        }
        None => BRIDGE_ERR_NOT_FOUND,
    }
}

/// Native callback invoked by C# to mutate an entity's transform.
pub unsafe extern "C" fn set_transform(entity_id: u64, input: *const TransformFfi) -> i32 {
    if input.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    let transform = unsafe { *input };
    if crate::bridge::write_entity_transform(entity_id, transform) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

/// Exporta el lote COMPLETO de transforms en un solo cruce FFI.
///
/// `ids` y `data` son arrays paralelos cuyo tamaño en elementos es
/// `cap`. Se sigue el patrón de consulta de la casa (igual que
/// `copy_str_to_buf`):
///   * Si `cap == 0` o cualquiera de los dos punteros es nulo → modo
///     consulta: se devuelve N (total de entidades con transform)
///     SIN tocar los buffers. Un puntero nulo en consulta NUNCA es
///     error.
///   * Si no → se copian `min(cap, N)` pares y se devuelve N, para
///     que C# pueda comparar N con su capacidad y reintentar con un
///     buffer mayor.
pub unsafe extern "C" fn get_transforms_batch(
    ids: *mut u64,
    data: *mut TransformFfi,
    cap: usize,
) -> i32 {
    let lote = crate::bridge::read_all_transforms();
    let total = lote.len();

    // Modo consulta: sólo se informa del tamaño necesario.
    if ids.is_null() || data.is_null() || cap == 0 {
        return total as i32;
    }

    // Copia parcial acotada por la capacidad del llamante.
    let a_copiar = total.min(cap);
    for (i, (id, transform)) in lote.iter().take(a_copiar).enumerate() {
        unsafe {
            *ids.add(i) = *id;
            *data.add(i) = *transform;
        }
    }

    total as i32
}

/// Aplica un lote de transforms venido de C# en un solo cruce FFI.
///
/// `ids` y `data` son arrays paralelos de `count` elementos.
/// `count == 0` es una llamada vacía legítima (devuelve 0); un
/// puntero nulo con `count > 0` es fallo de contrato y devuelve
/// `BRIDGE_ERR_NULL_PTR`. Devuelve cuántos pares se aplicaron: un
/// id desconocido o muerto no aborta el resto del lote.
pub unsafe extern "C" fn set_transforms_batch(
    ids: *const u64,
    data: *const TransformFfi,
    count: usize,
) -> i32 {
    if count == 0 {
        return 0;
    }
    if ids.is_null() || data.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }

    let mut aplicados: i32 = 0;
    for i in 0..count {
        let (id, transform) = unsafe { (*ids.add(i), *data.add(i)) };
        if crate::bridge::write_entity_transform(id, transform) {
            aplicados += 1;
        }
    }

    aplicados
}

/// Native callback invoked by C# to pipe logs directly into Rust log subsystem.
pub unsafe extern "C" fn log_message(level: i32, msg: *const c_char) {
    if msg.is_null() {
        return;
    }
    let s = unsafe { CStr::from_ptr(msg) };
    let text = s.to_string_lossy();
    match level {
        0 => log::trace!("[C#] {}", text),
        1 => log::debug!("[C#] {}", text),
        2 => log::info!("[C#] {}", text),
        3 => log::warn!("[C#] {}", text),
        _ => log::error!("[C#] {}", text),
    }
}

use std::sync::Mutex;

/// Estado de input que la app publica cada frame y C# consulta con
/// `get_input`. El layout `#[repr(C)]` es un contrato con la estructura
/// espejo `InputData` de `managed/GlassEngine.Core/Interop/InputDelegates.cs`
/// (mismo orden y mismos tipos; el test `input_data_ffi_layout_es_el_contrato`
/// fija offsets y tamaño para que un cambio accidental rompa el test y no
/// el puente).
///
/// Los campos de ratón viajan en píxeles del *offscreen* de render (el
/// espacio del Canvas, origen arriba a la izquierda): la app convierte la
/// posición de ventana antes de publicar, así un script ve el ratón en el
/// mismo sistema en el que se resuelven los rectángulos de UI.
/// `mouse_left_down` / `mouse_left_up` son bordes: valen 1 solo en el
/// frame de la pulsación / liberación; `mouse_left_held` vale 1 mientras
/// el botón izquierdo permanece pulsado.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct InputDataFfi {
    pub move_x: f32, // -1.0 (A) .. +1.0 (D)
    pub move_z: f32, // -1.0 (S) .. +1.0 (W)
    pub jump: u8,
    pub run: u8,
    // Tecla V (cambio de cámara en Plat3D). Ocupa el byte de RELLENO de
    // alineación que ya existía entre `run` y `mouse_x`: el tamaño del
    // struct (24) y todos los offsets previos quedan intactos, y el
    // prefijo heredado de 12 bytes lo incluye sin desbordar nada (en un
    // managed viejo ese byte es su propio relleno y lo ignora).
    pub camera: u8,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub mouse_left_held: u8,
    pub mouse_left_down: u8,
    pub mouse_left_up: u8,
    // Input v2 (ejes configurables): campos AÑADIDOS al final; el
    // prefijo de 24 bytes queda intacto y un managed viejo sigue
    // leyendo su parte (el mecanismo LEGACY_INPUT_DATA_LEN manda).
    /// Nº de ejes válidos en los arrays (0..=32).
    pub axis_count: u32,
    /// Valor suavizado por índice de eje.
    pub axes: [f32; 32],
    /// Valor crudo (sin suavizar) por índice de eje.
    pub axes_raw: [f32; 32],
    /// Bit i = el botón del eje i está pulsado.
    pub buttons: u32,
    /// Bit i = el botón del eje i se pulsó ESTE frame.
    pub buttons_down: u32,
    /// Bit i = el botón del eje i se soltó ESTE frame.
    pub buttons_up: u32,
}

static GLOBAL_INPUT: Mutex<InputDataFfi> = Mutex::new(InputDataFfi {
    move_x: 0.0,
    move_z: 0.0,
    jump: 0,
    run: 0,
    camera: 0,
    mouse_x: 0.0,
    mouse_y: 0.0,
    mouse_left_held: 0,
    mouse_left_down: 0,
    mouse_left_up: 0,
    axis_count: 0,
    axes: [0.0; 32],
    axes_raw: [0.0; 32],
    buttons: 0,
    buttons_down: 0,
    buttons_up: 0,
});

pub fn set_input_state(input: InputDataFfi) {
    if let Ok(mut g) = GLOBAL_INPUT.lock() {
        *g = input;
    }
}

pub fn get_input_state() -> InputDataFfi {
    GLOBAL_INPUT.lock().map(|g| *g).unwrap_or_default()
}

/// Tamaño del `InputData` de C# anterior al bloque de input de ratón:
/// `move_x`, `move_z`, `jump`, `run` y el relleno de alineación. Es un
/// prefijo EXACTO del layout actual (mismos offsets 0/4/8/9), así que un
/// ensamblado managed viejo puede seguir leyendo ejes y botones si solo
/// se le escriben estos bytes.
pub const LEGACY_INPUT_DATA_LEN: usize = 12;

/// El host lo activa cuando el ensamblado managed cargado es anterior al
/// contrato de input con ratón (marcador: no exporta `InvokeButtonClick`,
/// que entró en el mismo bloque que los campos de ratón de `InputData`).
/// Escribir los 24 bytes actuales en el buffer de 12 que reserva un C#
/// viejo desborda su pila y corrompe el frame gestionado: el binario de
/// juego puro lanzado sin argumentos moría así, con un segfault dentro
/// del código JIT al desreferenciar un puntero pisado por los propios
/// bytes del ratón (ver REGISTRO_FIX_SIN_ARGS.md).
static LEGACY_INPUT_LAYOUT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn set_legacy_input_layout(legacy: bool) {
    LEGACY_INPUT_LAYOUT.store(legacy, std::sync::atomic::Ordering::Relaxed);
}

/// Cuántos bytes escribe `get_input` según la generación del managed.
fn input_write_len() -> usize {
    if LEGACY_INPUT_LAYOUT.load(std::sync::atomic::Ordering::Relaxed) {
        LEGACY_INPUT_DATA_LEN
    } else {
        std::mem::size_of::<InputDataFfi>()
    }
}

pub unsafe extern "C" fn get_input(out: *mut InputDataFfi) -> i32 {
    if out.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    let state = get_input_state();
    // No escribir nunca más bytes de los que el lado C# reserva para su
    // generación del struct: ver `LEGACY_INPUT_DATA_LEN`.
    unsafe {
        std::ptr::copy_nonoverlapping(
            &state as *const InputDataFfi as *const u8,
            out as *mut u8,
            input_write_len(),
        );
    }
    BRIDGE_OK
}

/// Nº de ejes de entrada configurados (Input v2). Lo consulta el
/// managed para construir su caché nombre → índice.
#[no_mangle]
pub unsafe extern "C" fn input_axis_count() -> u32 {
    crate::input_axes::axis_defs().len() as u32
}

/// Escribe el nombre UTF-8 del eje `index` en `buf` (sin
/// terminador) y devuelve los bytes escritos; -1 si el índice
/// no existe o el buffer es nulo o no cabe.
#[no_mangle]
pub unsafe extern "C" fn input_axis_name(index: u32, buf: *mut u8, buf_len: i32) -> i32 {
    if buf.is_null() || buf_len <= 0 {
        return -1;
    }
    let defs = crate::input_axes::axis_defs();
    let Some(def) = defs.get(index as usize) else {
        return -1;
    };
    let bytes = def.name.as_bytes();
    if bytes.len() > buf_len as usize {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
    }
    bytes.len() as i32
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScriptParamsFfi {
    pub move_speed: f32,
    pub jump_velocity: f32,
    pub auto_demo: u8,
}

static GLOBAL_SCRIPT_PARAMS: Mutex<ScriptParamsFfi> = Mutex::new(ScriptParamsFfi {
    move_speed: 3.8,
    jump_velocity: 7.2,
    auto_demo: 0,
});

pub fn set_script_params(params: ScriptParamsFfi) {
    if let Ok(mut g) = GLOBAL_SCRIPT_PARAMS.lock() {
        *g = params;
    }
}

pub fn get_script_params() -> ScriptParamsFfi {
    GLOBAL_SCRIPT_PARAMS.lock().map(|g| *g).unwrap_or(ScriptParamsFfi {
        move_speed: 3.8,
        jump_velocity: 7.2,
        auto_demo: 0,
    })
}

pub unsafe extern "C" fn get_script_params_ffi(out: *mut ScriptParamsFfi) -> i32 {
    if out.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    unsafe { *out = get_script_params() };
    BRIDGE_OK
}

// ---------------------------------------------------------------------------
// Carga de escenas (SceneManager.LoadScene desde C#)
// ---------------------------------------------------------------------------

/// Encola la carga de una escena para el final del frame.
///
/// Devuelve `BRIDGE_OK` (0) si la petición quedó encolada, `BRIDGE_ERR_NOT_FOUND`
/// (-1) si la escena no existe o la ruta se sale de la raíz del proyecto, y
/// `BRIDGE_ERR_BRIDGE` (-4) si el motor rechazó la petición por otro motivo.
///
/// Igual que en Unity, el éxito **no** significa que la escena ya esté cargada:
/// sólo que se cargará cuando el frame termine. Para observar el resultado real,
/// consultar [`scene_active_name`] en frames posteriores.
pub unsafe extern "C" fn scene_load(name: *const c_char) -> i32 {
    if name.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    let requested = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return BRIDGE_ERR_BRIDGE, // no es UTF-8 válido
    };
    match crate::bridge::request_scene_load(requested) {
        Ok(()) => BRIDGE_OK,
        Err(rejection) => {
            // El motivo se registra en el log del motor para que el editor lo
            // muestre; el código de retorno basta para que C# decida.
            log::warn!("[C#] SceneManager.LoadScene('{requested}') rechazado: {rejection}");
            match rejection {
                crate::bridge::SceneLoadRejection::NotFound(_) => BRIDGE_ERR_NOT_FOUND,
                crate::bridge::SceneLoadRejection::Invalid(_) => BRIDGE_ERR_BRIDGE,
                crate::bridge::SceneLoadRejection::Unavailable(_) => BRIDGE_ERR_BRIDGE,
            }
        }
    }
}

/// Escribe en `out` el nombre de la escena activa. Con `cap == 0` sólo informa
/// del tamaño necesario. Devuelve la longitud escrita, o un código negativo.
pub unsafe extern "C" fn scene_active_name(out: *mut c_char, cap: usize) -> i32 {
    match crate::bridge::read_active_scene_name() {
        Some(name) => unsafe { copy_str_to_buf(&name, out, cap) },
        None => BRIDGE_ERR_NOT_FOUND,
    }
}

/// Número de escenas descubiertas en el proyecto. `-1` si no hay puente.
pub unsafe extern "C" fn scene_count() -> i32 {
    let scenes = crate::bridge::read_available_scenes();
    if scenes.is_empty() {
        BRIDGE_ERR_NOT_FOUND
    } else {
        scenes.len() as i32
    }
}

// ---------------------------------------------------------------------------
// Animación con piel (Animator.Play desde C#)
// ---------------------------------------------------------------------------

/// Pide al motor que la entidad reproduzca el clip indicado (bloque de
/// skinning): crea o actualiza su componente `Animator` con ese clip en
/// reproducción. El cambio se ve en el frame siguiente, cuando el bucle de
/// dibujo muestrea el clip.
///
/// Devuelve `BRIDGE_OK` (0) si la entidad existe, `BRIDGE_ERR_NOT_FOUND`
/// (-1) si no existe (o si el puente del motor no atiende animaciones) y
/// `BRIDGE_ERR_NULL_PTR` (-2) si el nombre del clip es nulo.
pub unsafe extern "C" fn animator_play(entity_id: u64, clip: *const c_char) -> i32 {
    if clip.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    let clip_name = match unsafe { CStr::from_ptr(clip) }.to_str() {
        Ok(s) => s,
        Err(_) => return BRIDGE_ERR_BRIDGE, // no es UTF-8 válido
    };
    if crate::bridge::animator_play(entity_id, clip_name) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

// ---------------------------------------------------------------------------
// Máquina de estados de animación (Animator.SetBool/SetFloat/SetTrigger,
// Animator.CrossFade y Animator.GetCurrentState desde C#)
// ---------------------------------------------------------------------------

/// Fija un parámetro del controlador de la entidad. `kind`: 0 = bool,
/// 1 = float, 2 = trigger. Devuelve `BRIDGE_OK` si la entidad existe y
/// tiene componente Animator, `BRIDGE_ERR_NOT_FOUND` si no, y
/// `BRIDGE_ERR_NULL_PTR` si el nombre es nulo.
#[no_mangle]
pub unsafe extern "C" fn animator_set_param(
    entity_id: u64,
    name: *const c_char,
    kind: i32,
    value: f32,
) -> i32 {
    if name.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    let param = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return BRIDGE_ERR_BRIDGE,
    };
    if crate::bridge::animator_set_param(entity_id, param, kind, value) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

/// Escribe el estado actual de la máquina en `out` con el protocolo de
/// dos llamadas de `copy_str_to_buf` (con `cap = 0` devuelve los bytes
/// que hacen falta). Sin controlador, el estado es el clip en curso.
#[no_mangle]
pub unsafe extern "C" fn animator_current_state(
    entity_id: u64,
    out: *mut c_char,
    cap: usize,
) -> i32 {
    let s = crate::bridge::animator_current_state(entity_id);
    unsafe { copy_str_to_buf(&s, out, cap) }
}

/// Salto con fundido: a un estado si la entidad tiene controlador, o
/// crossfade libre entre clips si no lo tiene. Devuelve `BRIDGE_OK` si
/// la entidad existe y `BRIDGE_ERR_NOT_FOUND` si no.
#[no_mangle]
pub unsafe extern "C" fn animator_crossfade(
    entity_id: u64,
    name: *const c_char,
    fade: f32,
) -> i32 {
    if name.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    let state = match unsafe { CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return BRIDGE_ERR_BRIDGE,
    };
    if crate::bridge::animator_crossfade(entity_id, state, fade) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

// ---------------------------------------------------------------------------
// Luces (Light.color / Light.intensity desde C#)
// ---------------------------------------------------------------------------

/// Fija el color de la luz de la entidad. Devuelve `BRIDGE_OK` si la
/// entidad existe y tiene componente Light, y `BRIDGE_ERR_NOT_FOUND` si
/// no.
#[no_mangle]
pub unsafe extern "C" fn light_set_color(entity_id: u64, r: f32, g: f32, b: f32) -> i32 {
    if crate::bridge::light_set_color(entity_id, r, g, b) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

/// Fija la intensidad de la luz de la entidad (acotada a >= 0.0 en el
/// motor). Mismos códigos que `light_set_color`.
#[no_mangle]
pub unsafe extern "C" fn light_set_intensity(entity_id: u64, intensity: f32) -> i32 {
    if crate::bridge::light_set_intensity(entity_id, intensity) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

#[no_mangle]
pub unsafe extern "C" fn ui_set_text(entity_id: u64, text: *const c_char) -> i32 {
    if text.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    let text = match CStr::from_ptr(text).to_str() {
        Ok(s) => s,
        Err(_) => return BRIDGE_ERR_BRIDGE,
    };
    if crate::bridge::ui_set_text(entity_id, text) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

#[no_mangle]
pub unsafe extern "C" fn ui_set_font_size(entity_id: u64, size: f32) -> i32 {
    if crate::bridge::ui_set_font_size(entity_id, size) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

#[no_mangle]
pub unsafe extern "C" fn ui_set_text_color(entity_id: u64, r: f32, g: f32, b: f32, a: f32) -> i32 {
    if crate::bridge::ui_set_text_color(entity_id, r, g, b, a) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

#[no_mangle]
pub unsafe extern "C" fn ui_set_image_sprite(entity_id: u64, sprite: *const c_char) -> i32 {
    if sprite.is_null() {
        return BRIDGE_ERR_NULL_PTR;
    }
    let sprite = match CStr::from_ptr(sprite).to_str() {
        Ok(s) => s,
        Err(_) => return BRIDGE_ERR_BRIDGE,
    };
    if crate::bridge::ui_set_image_sprite(entity_id, sprite) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

#[no_mangle]
pub unsafe extern "C" fn ui_set_image_color(entity_id: u64, r: f32, g: f32, b: f32, a: f32) -> i32 {
    if crate::bridge::ui_set_image_color(entity_id, r, g, b, a) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

#[no_mangle]
pub unsafe extern "C" fn ui_set_button_color(entity_id: u64, r: f32, g: f32, b: f32, a: f32) -> i32 {
    if crate::bridge::ui_set_button_color(entity_id, r, g, b, a) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

#[no_mangle]
pub unsafe extern "C" fn ui_set_anchored_pos(entity_id: u64, x: f32, y: f32) -> i32 {
    if crate::bridge::ui_set_anchored_pos(entity_id, x, y) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

#[no_mangle]
pub unsafe extern "C" fn ui_set_size_delta(entity_id: u64, w: f32, h: f32) -> i32 {
    if crate::bridge::ui_set_size_delta(entity_id, w, h) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

/// Entidad del último arrastre del Canvas (drag & drop, UI v2).
#[no_mangle]
pub unsafe extern "C" fn ui_get_dragged_id() -> u64 {
    crate::bridge::ui_dragged_id()
}

/// Casilla que recibió la última suelta (0 = en vacío).
#[no_mangle]
pub unsafe extern "C" fn ui_get_drop_target_id() -> u64 {
    crate::bridge::ui_drop_target_id()
}

// ---------------------------------------------------------------------------
// Instanciación real de entidades (GameObject.Spawn / EntityPool desde C#)
// ---------------------------------------------------------------------------

/// Especificación FFI de un spawn de entidad: el espejo `#[repr(C)]` de
/// `SpawnSpecData` en `managed/GlassEngine.Core/Interop/Structs.cs`
/// (mismo orden y mismos tipos; el test
/// `spawn_spec_ffi_layout_es_el_contrato` fija offsets y tamaño).
///
/// Los campos se ordenan de mayor a menor alineación (punteros, u64,
/// arrays de f32, i32, u8) para que el padding sea el explícito del
/// final y no una sorpresa entre plataformas. Las cadenas son UTF-8 con
/// terminador NUL y pueden ser NULL (= ausente); su longitud máxima
/// aceptada es [`MAX_SPAWN_STR_LEN`] bytes, la misma cota que el MCP
/// aplica a los nombres.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SpawnSpecFfi {
    pub name: *const c_char,
    pub model: *const c_char,
    pub script: *const c_char,
    pub parent_id: u64,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub color: [f32; 3],
    /// 0 = sin física, 1 = estático, 2 = dinámico, 3 = cinemático
    /// (constantes `SPAWN_PHYSICS_*` de `bridge`).
    pub physics: i32,
    /// 1 si `color` debe aplicarse; 0 deja el color por defecto de la
    /// malla, como una entidad autorada sin tocar.
    pub has_color: u8,
}

/// Cota de longitud (bytes, sin el NUL) para nombre/modelo/script de un
/// spawn: la misma que el servidor MCP impone a los nombres de entidad.
pub const MAX_SPAWN_STR_LEN: usize = 256;

/// Lee una cadena FFI acotada: NULL -> `None` (ausente); sin NUL dentro
/// de la cota o con UTF-8 inválido -> `Err(())` (spec rechazado entero:
/// aceptar un nombre truncado en silencio crearía entidades imposibles
/// de encontrar después por su nombre real).
unsafe fn read_spawn_str(ptr: *const c_char) -> Result<Option<String>, ()> {
    if ptr.is_null() {
        return Ok(None);
    }
    let mut len = 0usize;
    loop {
        if len > MAX_SPAWN_STR_LEN {
            return Err(());
        }
        if unsafe { *ptr.add(len) } == 0 {
            break;
        }
        len += 1;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(Some(s.to_string())),
        Err(_) => Err(()),
    }
}

/// Convierte el spec FFI en el `SpawnSpec` del puente, saneando como el
/// resto de la casa: cualquier float no finito (o una escala con un eje
/// a cero, que produciría un collider degenerado) rechaza el spawn
/// entero, y un cuaternión degenerado se sustituye por la identidad en
/// vez de propagar NaN a la escena.
fn sanitize_spawn_spec(ffi: &SpawnSpecFfi) -> Option<crate::bridge::SpawnSpec> {
    let finite = ffi.position.iter().chain(&ffi.rotation).chain(&ffi.scale).chain(&ffi.color).all(|v| v.is_finite());
    if !finite {
        return None;
    }
    if ffi.scale.iter().any(|&s| s == 0.0) {
        return None;
    }
    if !(crate::bridge::SPAWN_PHYSICS_NONE..=crate::bridge::SPAWN_PHYSICS_KINEMATIC).contains(&ffi.physics) {
        return None;
    }
    let rotation = {
        let q = ffi.rotation;
        let len2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
        if len2 < 1e-6 {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            q
        }
    };
    Some(crate::bridge::SpawnSpec {
        name: unsafe { read_spawn_str(ffi.name) }.ok()?.unwrap_or_default(),
        model: unsafe { read_spawn_str(ffi.model) }.ok()?.unwrap_or_default(),
        script: unsafe { read_spawn_str(ffi.script) }.ok()?.unwrap_or_default(),
        parent_id: ffi.parent_id,
        position: ffi.position,
        rotation,
        scale: ffi.scale,
        color: ffi.color,
        has_color: ffi.has_color != 0,
        physics: ffi.physics,
    })
}

/// Callback invocado por C# (`GameObject.Spawn`, `EntityPool`) para
/// crear una entidad en la escena viva. Devuelve el id asignado, o 0 si
/// el spec es nulo/inválido o el motor no pudo crearla (el motivo va al
/// log). El id 0 nunca es una entidad válida en este motor (la escena
/// por defecto empieza en 1), así que sirve de centinela sin ambigüedad.
pub unsafe extern "C" fn entity_spawn(spec: *const SpawnSpecFfi) -> u64 {
    if spec.is_null() {
        return 0;
    }
    let Some(spec) = sanitize_spawn_spec(unsafe { &*spec }) else {
        log::warn!("[C#] Spawn rechazado: spec inválido (no finito, escala a cero, física fuera de rango o cadena ilegible).");
        return 0;
    };
    match crate::bridge::spawn_entity(&spec) {
        Some(id) => id,
        None => {
            log::warn!("[C#] Spawn rechazado por el motor (sin puente o padre inexistente).");
            0
        }
    }
}

/// Callback invocado por C# (`Object.Destroy`, `EntityPool.DestroyAll`)
/// para destruir una entidad: sale de la escena y su cuerpo y colliders
/// salen del mundo físico. Devuelve `BRIDGE_OK` si existía y
/// `BRIDGE_ERR_NOT_FOUND` si no (o si no hay puente).
pub unsafe extern "C" fn entity_destroy(entity_id: u64) -> i32 {
    if crate::bridge::destroy_entity(entity_id) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

/// Callback invocado por C# (`GameObject.InstantiatePrefab`) para crear
/// una instancia de un Prefab (`.prefab.json`) en la escena viva.
///
/// `path` es UTF-8 con NUL final, acotado como el resto de cadenas de
/// este puente (ver [`read_spawn_str`]): NULL, vacía o no UTF-8 se
/// rechaza aquí mismo con 0, sin llegar al motor. x/y/z deben ser
/// finitos (una posición con NaN/inf produciría una entidad imposible
/// de localizar después). La resolución de "nombre" vs "ruta" y la
/// carga del `.prefab.json` las hace el lado Rust del motor
/// (`crate::bridge::instantiate_prefab`), igual que `entity_spawn`
/// delega en `crate::bridge::spawn_entity`. Devuelve el id de la
/// entidad raíz creada, o 0 si el puente no está, la ruta no se pudo
/// leer o el motor rechazó la instanciación (el motivo va al log).
pub unsafe extern "C" fn entity_instantiate_prefab(path: *const c_char, x: f32, y: f32, z: f32) -> u64 {
    let path = match unsafe { read_spawn_str(path) } {
        Ok(Some(p)) if !p.is_empty() => p,
        Ok(Some(_)) => {
            log::warn!("[C#] InstantiatePrefab rechazado: ruta/nombre vacío.");
            return 0;
        }
        Ok(None) => {
            log::warn!("[C#] InstantiatePrefab rechazado: puntero de ruta nulo.");
            return 0;
        }
        Err(()) => {
            log::warn!("[C#] InstantiatePrefab rechazado: ruta ilegible (no UTF-8 o demasiado larga).");
            return 0;
        }
    };
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        log::warn!("[C#] InstantiatePrefab rechazado: posición no finita.");
        return 0;
    }
    match crate::bridge::instantiate_prefab(&path, [x, y, z]) {
        Some(id) => id,
        None => {
            log::warn!("[C#] InstantiatePrefab rechazado por el motor (prefab inexistente o JSON inválido).");
            0
        }
    }
}

// REGISTRO-PREFAB: `entity_instantiate_prefab` se registra en la tabla
// de callbacks del host junto a `entity_spawn` (el excerpt de este
// archivo no incluye el punto donde se construye esa tabla de punteros
// de función que viaja a `InitSpawnBridge`/`InitPrefabBridge`; debe
// añadirse ahí un puntero nuevo a esta función, expuesto a C# con el
// export `InitPrefabBridge` descrito en `managed/GlassEngine.Core/RuntimeBridge.cs`).


/// Callback invocado por C# (`EntityPool.Get/Release`) para activar o
/// desactivar una entidad: la inactiva no se dibuja y no tiene cuerpo;
/// al reactivarla el cuerpo se recrea desde su `PhysicsData`. Devuelve
/// `BRIDGE_OK` / `BRIDGE_ERR_NOT_FOUND` como `entity_destroy`.
pub unsafe extern "C" fn entity_set_active(entity_id: u64, active: u8) -> i32 {
    if crate::bridge::set_entity_active(entity_id, active != 0) {
        BRIDGE_OK
    } else {
        BRIDGE_ERR_NOT_FOUND
    }
}

/// Número de entidades de la escena viva; `BRIDGE_ERR_NOT_FOUND` si el
/// puente no está disponible (p. ej. antes de cargar el proyecto).
pub unsafe extern "C" fn entity_count() -> i32 {
    match crate::bridge::entity_count() {
        Some(n) => n as i32,
        None => BRIDGE_ERR_NOT_FOUND,
    }
}

/// Escribe los conteos (cuerpos, colliders) del mundo físico vivo en
/// las salidas (cualquiera puede ser NULL si no interesa). Devuelve
/// `BRIDGE_OK` o `BRIDGE_ERR_NOT_FOUND` si no hay puente.
pub unsafe extern "C" fn entity_physics_counts(out_bodies: *mut i32, out_colliders: *mut i32) -> i32 {
    match crate::bridge::physics_counts() {
        Some((bodies, colliders)) => {
            unsafe {
                if !out_bodies.is_null() {
                    *out_bodies = bodies as i32;
                }
                if !out_colliders.is_null() {
                    *out_colliders = colliders as i32;
                }
            }
            BRIDGE_OK
        }
        None => BRIDGE_ERR_NOT_FOUND,
    }
}

// ---------------------------------------------------------------------------
// Punto de interacción de los grafos (Shader.SetInteractionPoint desde C#)
// ---------------------------------------------------------------------------

/// El canal global de interacción vigente: los dos vec4 del UBO
/// `GraphGlobals`, seguidos — primero el punto (posición de mundo del
/// intruso, radio en el `w`) y después la forma de su huella (semiejes
/// X/Z de la caja, rotación Y en radianes y discriminador 0 = esfera /
/// 1 = caja) —. Es el espejo en el lado Rust del UBO de los pipelines
/// de grafo: C# lo escribe por el FFI de este módulo y la app lo lee
/// cada frame para pasarlo al Renderer. Un solo intruso para todo el
/// motor (v1), como el `ObjectPosition` global del tutorial de
/// referencia. Sin publicador vale
/// [`glass_core::shader_graph::INTERACTION_GLOBALS_INACTIVE`] y los
/// grafos que leen el nodo no hacen nada.
static GLOBAL_INTERACTION_POINT: Mutex<[f32; 8]> =
    Mutex::new(glass_core::shader_graph::INTERACTION_GLOBALS_INACTIVE);

/// Fija el canal desde Rust (la app lo usa al cambiar de escena, cuando
/// el script que publicaba desaparece con la anterior).
pub fn set_interaction_point_state(point: [f32; 8]) {
    if let Ok(mut g) = GLOBAL_INTERACTION_POINT.lock() {
        *g = point;
    }
}

/// El canal vigente, para que la app lo entregue al Renderer por frame.
pub fn get_interaction_point_state() -> [f32; 8] {
    GLOBAL_INTERACTION_POINT
        .lock()
        .map(|g| *g)
        .unwrap_or(glass_core::shader_graph::INTERACTION_GLOBALS_INACTIVE)
}

/// Vuelve el canal a su valor inactivo (nadie interactúa).
pub fn clear_interaction_point_state() {
    set_interaction_point_state(glass_core::shader_graph::INTERACTION_GLOBALS_INACTIVE);
}

/// Callback invocado por C# (`Shader.SetInteractionPoint`) para publicar
/// el punto de interacción del frame: posición de mundo del intruso y su
/// radio. La publicación fija también la forma a ESFÉRICA (el segundo
/// vec4 a cero): la huella es el disco del radio. Los valores no finitos
/// se descartan enteros (un NaN en el UBO envenenaría todas las
/// máscaras de distancia sin error visible) y el radio negativo se
/// clampa a cero; con radio cero el punto sigue siendo válido pero su
/// disco de influencia es un punto.
pub unsafe extern "C" fn graph_set_interaction_point(x: f32, y: f32, z: f32, radius: f32) {
    if !(x.is_finite() && y.is_finite() && z.is_finite() && radius.is_finite()) {
        return;
    }
    set_interaction_point_state([x, y, z, radius.max(0.0), 0.0, 0.0, 0.0, 0.0]);
}

/// Callback invocado por C# (`Shader.SetInteractionBox`) para publicar
/// la interacción del frame con huella de CAJA en planta: centro de
/// mundo, semiejes de la caja en su marco local (`half_x` a lo largo de
/// su eje X, `half_z` a lo largo de su eje Z — los de su collider en
/// planta) y su rotación Y en radianes. El `w` del punto (el radio) no
/// se usa con esta forma y queda a cero. Misma higiene que la esférica:
/// no finitos descartados enteros y semiejes negativos clampados a cero.
pub unsafe extern "C" fn graph_set_interaction_box(
    x: f32,
    y: f32,
    z: f32,
    half_x: f32,
    half_z: f32,
    yaw: f32,
) {
    if !(x.is_finite()
        && y.is_finite()
        && z.is_finite()
        && half_x.is_finite()
        && half_z.is_finite()
        && yaw.is_finite())
    {
        return;
    }
    set_interaction_point_state([
        x,
        y,
        z,
        0.0,
        half_x.max(0.0),
        half_z.max(0.0),
        yaw,
        1.0,
    ]);
}

/// Callback invocado por C# (`Shader.ClearInteractionPoint`): el canal
/// vuelve a su valor inactivo y los grafos dejan de reaccionar.
pub unsafe extern "C" fn graph_clear_interaction_point() {
    clear_interaction_point_state();
}

/// Escribe en `out` el nombre de la escena en la posición `index`.
pub unsafe extern "C" fn scene_name_at(index: i32, out: *mut c_char, cap: usize) -> i32 {
    if index < 0 {
        return BRIDGE_ERR_NOT_FOUND;
    }
    let scenes = crate::bridge::read_available_scenes();
    match scenes.get(index as usize) {
        Some(name) => unsafe { copy_str_to_buf(name, out, cap) },
        None => BRIDGE_ERR_NOT_FOUND,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::SceneLoadRejection;
    use std::sync::{Mutex, MutexGuard};

    /// El puente es un `OnceLock` global del proceso, así que estos tests se
    /// serializan entre sí y restituyen `NullBridge` al terminar. Sin esto, un
    /// test dejaría su puente falso instalado para el resto del binario.
    static FFI_LOCK: Mutex<()> = Mutex::new(());

    /// Puente de prueba que **almacena de verdad** la transformada recibida, en
    /// lugar de devolver una constante. Un puente que sólo ecoa una constante no
    /// probaría la ida y vuelta, que es precisamente lo que hay que verificar:
    /// que los 40 bytes que C# escribe en el struct llegan intactos a Rust y
    /// vuelven intactos a C#.
    struct FakeBridge {
        /// Nombre de escena que acepta la carga; cualquier otro es NotFound.
        acepta: &'static str,
        almacen: Mutex<Option<TransformFfi>>,
        /// Peticiones `Animator.Play` recibidas: (entidad, clip).
        animaciones: Mutex<Vec<(u64, String)>>,
        /// Entidades creadas por spawn: id -> spec recibido.
        spawns: Mutex<std::collections::HashMap<u64, crate::bridge::SpawnSpec>>,
        /// Siguiente id de spawn (empieza lejos de los ids de escena).
        next_spawn_id: Mutex<u64>,
        /// Entidades desactivadas por set_active.
        inactivas: Mutex<std::collections::HashSet<u64>>,
    }

    impl crate::bridge::SceneBridge for FakeBridge {
        fn get_transform(&self, entity_id: u64) -> Option<TransformFfi> {
            if entity_id != 7 {
                return None;
            }
            *self.almacen.lock().unwrap_or_else(|e| e.into_inner())
        }

        fn set_transform(&self, entity_id: u64, transform: TransformFfi) -> bool {
            if entity_id != 7 {
                return false;
            }
            *self.almacen.lock().unwrap_or_else(|e| e.into_inner()) = Some(transform);
            true
        }

        /// El lote del puente falso: la única entidad que conoce (la
        /// 7) y solo si ya tiene transformada guardada.
        fn all_transforms(&self) -> Vec<(u64, TransformFfi)> {
            match *self.almacen.lock().unwrap_or_else(|e| e.into_inner()) {
                Some(t) => vec![(7, t)],
                None => Vec::new(),
            }
        }

        fn request_scene_load(&self, name: &str) -> Result<(), SceneLoadRejection> {
            if name == self.acepta {
                Ok(())
            } else {
                Err(SceneLoadRejection::NotFound(format!(
                    "escena no encontrada: '{name}'"
                )))
            }
        }

        fn active_scene_name(&self) -> Option<String> {
            Some(self.acepta.to_string())
        }

        fn available_scenes(&self) -> Vec<String> {
            vec!["Nivel1".to_string(), "Nivel2".to_string()]
        }

        fn animator_play(&self, entity_id: u64, clip: &str) -> bool {
            if entity_id != 7 {
                return false;
            }
            self.animaciones
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((entity_id, clip.to_string()));
            true
        }

        fn spawn_entity(&self, spec: &crate::bridge::SpawnSpec) -> Option<u64> {
            if spec.parent_id != 0 && spec.parent_id != 7 {
                return None; // padre inexistente en este mundo de juguete
            }
            let mut next = self.next_spawn_id.lock().unwrap_or_else(|e| e.into_inner());
            let id = *next;
            *next += 1;
            self.spawns
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id, spec.clone());
            Some(id)
        }

        fn destroy_entity(&self, entity_id: u64) -> bool {
            self.spawns
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&entity_id)
                .is_some()
        }

        fn set_entity_active(&self, entity_id: u64, active: bool) -> bool {
            if !self.spawns.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&entity_id) {
                return false;
            }
            let mut inactivas = self.inactivas.lock().unwrap_or_else(|e| e.into_inner());
            if active {
                inactivas.remove(&entity_id);
            } else {
                inactivas.insert(entity_id);
            }
            true
        }

        fn entity_count(&self) -> Option<usize> {
            Some(self.spawns.lock().unwrap_or_else(|e| e.into_inner()).len())
        }

        fn physics_counts(&self) -> Option<(usize, usize)> {
            let n = self.spawns.lock().unwrap_or_else(|e| e.into_inner()).len();
            Some((n, n))
        }
    }

    fn install_fake() -> MutexGuard<'static, ()> {
        let guard = FFI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::bridge::register_scene_bridge(Box::new(FakeBridge {
            acepta: "Nivel1",
            almacen: Mutex::new(None),
            animaciones: Mutex::new(Vec::new()),
            spawns: Mutex::new(std::collections::HashMap::new()),
            next_spawn_id: Mutex::new(100),
            inactivas: Mutex::new(std::collections::HashSet::new()),
        }));
        guard
    }

    fn restore() {
        crate::bridge::register_scene_bridge(Box::new(crate::bridge::NullBridge));
    }

    /// Ida y vuelta completa del contrato FFI: un puntero a `TransformFfi`
    /// escrito por Rust debe leerse de vuelta idéntico desde el lado C#.
    #[test]
    fn get_y_set_transform_round_trip_por_ffi() {
        let _g = install_fake();

        let original = TransformFfi {
            position: [4.0, 5.0, 6.0],
            rotation: [0.1, 0.2, 0.3, 0.9],
            scale: [2.0, 2.0, 2.0],
        };
        let rc = unsafe { set_transform(7, &original as *const TransformFfi) };
        assert_eq!(rc, BRIDGE_OK);

        let mut leido = TransformFfi {
            position: [0.0; 3],
            rotation: [0.0; 4],
            scale: [0.0; 3],
        };
        let rc = unsafe { get_transform(7, &mut leido as *mut TransformFfi) };
        assert_eq!(rc, BRIDGE_OK);
        // Comparación byte a byte: cualquier hueco de padding introducido por
        // un `#[repr(C)]` mal puesto rompería esta igualdad.
        assert_eq!(
            unsafe { std::slice::from_raw_parts(&original as *const _ as *const u8, 40) },
            unsafe { std::slice::from_raw_parts(&leido as *const _ as *const u8, 40) }
        );

        // Entidad inexistente: error tipado, no basura en el buffer.
        let mut no_tocada = TransformFfi {
            position: [9.0; 3],
            rotation: [0.0; 4],
            scale: [1.0; 3],
        };
        let rc = unsafe { get_transform(999, &mut no_tocada as *mut TransformFfi) };
        assert_eq!(rc, BRIDGE_ERR_NOT_FOUND);
        assert_eq!(no_tocada.position, [9.0; 3], "el buffer no debe modificarse");

        restore();
    }

    /// El lote completo por FFI: consulta de tamaño, lectura del
    /// lote, devolución modificada y lectura por la vía individual.
    #[test]
    fn lote_transforms_round_trip_por_ffi() {
        let _g = install_fake();

        let original = TransformFfi {
            position: [4.0, 5.0, 6.0],
            rotation: [0.1, 0.2, 0.3, 0.9],
            scale: [2.0, 2.0, 2.0],
        };
        assert_eq!(
            unsafe { set_transform(7, &original as *const TransformFfi) },
            BRIDGE_OK
        );

        // 1) Consulta: cap == 0 devuelve el total sin tocar punteros.
        let total = unsafe {
            get_transforms_batch(std::ptr::null_mut(), std::ptr::null_mut(), 0)
        };
        assert_eq!(total, 1, "el puente falso solo conoce la entidad 7");

        // 2) Lectura real del lote.
        let mut ids: [u64; 4] = [0; 4];
        let mut data: [TransformFfi; 4] = [TransformFfi {
            position: [0.0; 3],
            rotation: [0.0; 4],
            scale: [0.0; 3],
        }; 4];
        let total = unsafe {
            get_transforms_batch(ids.as_mut_ptr(), data.as_mut_ptr(), ids.len())
        };
        assert_eq!(total, 1);
        assert_eq!(ids[0], 7);
        assert_eq!(data[0], original);

        // 3) Devolución del lote con la posición movida.
        data[0].position = [40.0, 50.0, 60.0];
        let aplicados = unsafe { set_transforms_batch(ids.as_ptr(), data.as_ptr(), 1) };
        assert_eq!(aplicados, 1);

        // 4) Lo escrito se ve por la vía de una sola entidad.
        let mut leido = TransformFfi {
            position: [0.0; 3],
            rotation: [0.0; 4],
            scale: [0.0; 3],
        };
        assert_eq!(
            unsafe { get_transform(7, &mut leido as *mut TransformFfi) },
            BRIDGE_OK
        );
        assert_eq!(leido, data[0]);

        // 5) Contrato: puntero nulo con count > 0 es error; count
        //    == 0 es llamada vacía legítima.
        assert_eq!(
            unsafe { set_transforms_batch(std::ptr::null(), data.as_ptr(), 1) },
            BRIDGE_ERR_NULL_PTR
        );
        assert_eq!(
            unsafe { set_transforms_batch(ids.as_ptr(), data.as_ptr(), 0) },
            0
        );

        restore();
    }

    #[test]
    fn punteros_nulos_no_provocan_segfault() {
        let _g = install_fake();
        assert_eq!(unsafe { get_transform(7, std::ptr::null_mut()) }, BRIDGE_ERR_NULL_PTR);
        assert_eq!(unsafe { set_transform(7, std::ptr::null()) }, BRIDGE_ERR_NULL_PTR);
        assert_eq!(unsafe { scene_load(std::ptr::null()) }, BRIDGE_ERR_NULL_PTR);
        assert_eq!(unsafe { scene_active_name(std::ptr::null_mut(), 64) }, BRIDGE_ERR_NULL_PTR);
        assert_eq!(unsafe { scene_name_at(0, std::ptr::null_mut(), 64) }, BRIDGE_ERR_NULL_PTR);
        assert_eq!(unsafe { animator_play(7, std::ptr::null()) }, BRIDGE_ERR_NULL_PTR);
        restore();
    }

    #[test]
    fn animator_play_llega_al_puente_con_entidad_y_clip() {
        let _g = install_fake();

        let clip = std::ffi::CString::new("Saludar").unwrap();
        assert_eq!(unsafe { animator_play(7, clip.as_ptr()) }, BRIDGE_OK);
        // Entidad desconocida para el puente: NOT_FOUND, sin registrar nada.
        assert_eq!(unsafe { animator_play(999, clip.as_ptr()) }, BRIDGE_ERR_NOT_FOUND);

        restore();
    }

    /// El layout de `SpawnSpecFfi` es el contrato con `SpawnSpecData` en
    /// C# (`LayoutKind.Sequential`): punteros primero, u64 después, los
    /// arrays de f32, el i32 y el u8 al final; si alguien reordena un
    /// campo, este test rompe antes de que C# lea basura en silencio.
    #[test]
    fn spawn_spec_ffi_layout_es_el_contrato() {
        use std::mem::{offset_of, size_of};
        assert_eq!(offset_of!(SpawnSpecFfi, name), 0);
        assert_eq!(offset_of!(SpawnSpecFfi, model), 8);
        assert_eq!(offset_of!(SpawnSpecFfi, script), 16);
        assert_eq!(offset_of!(SpawnSpecFfi, parent_id), 24);
        assert_eq!(offset_of!(SpawnSpecFfi, position), 32);
        assert_eq!(offset_of!(SpawnSpecFfi, rotation), 44);
        assert_eq!(offset_of!(SpawnSpecFfi, scale), 60);
        assert_eq!(offset_of!(SpawnSpecFfi, color), 72);
        assert_eq!(offset_of!(SpawnSpecFfi, physics), 84);
        assert_eq!(offset_of!(SpawnSpecFfi, has_color), 88);
        assert_eq!(size_of::<SpawnSpecFfi>(), 96);
    }

    fn spec_valido() -> SpawnSpecFfi {
        SpawnSpecFfi {
            name: std::ptr::null(),
            model: std::ptr::null(),
            script: std::ptr::null(),
            parent_id: 0,
            position: [1.0, 2.0, 3.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
            color: [0.0; 3],
            physics: crate::bridge::SPAWN_PHYSICS_DYNAMIC,
            has_color: 0,
        }
    }

    /// El saneamiento del spec: lo no finito y la escala a cero se
    /// rechazan enteros; el cuaternión degenerado se sustituye por la
    /// identidad; las cadenas ilegibles o sin NUL en la cota rechazan.
    #[test]
    fn spawn_sanea_el_spec_antes_del_puente() {
        let ok = sanitize_spawn_spec(&spec_valido()).expect("spec válido");
        assert_eq!(ok.position, [1.0, 2.0, 3.0]);
        assert_eq!(ok.name, "");
        assert_eq!(ok.physics, crate::bridge::SPAWN_PHYSICS_DYNAMIC);

        let mut nan = spec_valido();
        nan.position[1] = f32::NAN;
        assert!(sanitize_spawn_spec(&nan).is_none());

        let mut escala_cero = spec_valido();
        escala_cero.scale[0] = 0.0;
        assert!(sanitize_spawn_spec(&escala_cero).is_none());

        let mut fisica_rara = spec_valido();
        fisica_rara.physics = 9;
        assert!(sanitize_spawn_spec(&fisica_rara).is_none());

        let mut quat_roto = spec_valido();
        quat_roto.rotation = [0.0; 4];
        let saneado = sanitize_spawn_spec(&quat_roto).expect("quat degenerado se sustituye");
        assert_eq!(saneado.rotation, [0.0, 0.0, 0.0, 1.0]);

        // Cadena sin NUL dentro de la cota: ilegible, no truncable.
        let larga = vec![b'a'; MAX_SPAWN_STR_LEN + 10];
        assert!(unsafe { read_spawn_str(larga.as_ptr() as *const c_char) }.is_err());
        // NULL = ausente, no error.
        assert_eq!(unsafe { read_spawn_str(std::ptr::null()) }, Ok(None));
        let nombre = std::ffi::CString::new("Muro").unwrap();
        assert_eq!(
            unsafe { read_spawn_str(nombre.as_ptr()) },
            Ok(Some("Muro".to_string()))
        );
    }

    /// Ida y vuelta del spawn por el FFI: crea, cuenta, desactiva,
    /// reactiva y destruye contra el puente falso, con los códigos de la
    /// casa en cada paso y en cada ausencia.
    #[test]
    fn entity_spawn_ciclo_completo_por_ffi() {
        let _g = install_fake();

        assert_eq!(unsafe { entity_spawn(std::ptr::null()) }, 0);
        let mut invalido = spec_valido();
        invalido.scale = [0.0, 1.0, 1.0];
        assert_eq!(unsafe { entity_spawn(&invalido) }, 0);

        let nombre = std::ffi::CString::new("Muro_3").unwrap();
        let modelo = std::ffi::CString::new("sphere").unwrap();
        let mut spec = spec_valido();
        spec.name = nombre.as_ptr();
        spec.model = modelo.as_ptr();
        let id = unsafe { entity_spawn(&spec) };
        assert_eq!(id, 100, "el puente falso asigna desde 100");
        assert_eq!(unsafe { entity_count() }, 1);

        let (mut bodies, mut colliders) = (-1, -1);
        assert_eq!(
            unsafe { entity_physics_counts(&mut bodies, &mut colliders) },
            BRIDGE_OK
        );
        assert_eq!((bodies, colliders), (1, 1));
        // Salidas NULL toleradas: solo se escribe la que se pasa.
        assert_eq!(
            unsafe { entity_physics_counts(std::ptr::null_mut(), &mut colliders) },
            BRIDGE_OK
        );

        assert_eq!(unsafe { entity_set_active(id, 0) }, BRIDGE_OK);
        assert_eq!(unsafe { entity_set_active(id, 1) }, BRIDGE_OK);
        assert_eq!(unsafe { entity_set_active(999, 1) }, BRIDGE_ERR_NOT_FOUND);

        assert_eq!(unsafe { entity_destroy(id) }, BRIDGE_OK);
        assert_eq!(unsafe { entity_destroy(id) }, BRIDGE_ERR_NOT_FOUND);
        assert_eq!(unsafe { entity_count() }, 0);

        // Padre inexistente: el puente lo rechaza y el FFI devuelve 0.
        let mut huerfano = spec_valido();
        huerfano.parent_id = 4242;
        assert_eq!(unsafe { entity_spawn(&huerfano) }, 0);

        restore();
    }

    #[test]
    fn scene_load_acepta_y_rechaza_con_codigos_distintos() {
        let _g = install_fake();

        let ok = std::ffi::CString::new("Nivel1").unwrap();
        assert_eq!(unsafe { scene_load(ok.as_ptr()) }, BRIDGE_OK);

        let falta = std::ffi::CString::new("NoExiste").unwrap();
        assert_eq!(unsafe { scene_load(falta.as_ptr()) }, BRIDGE_ERR_NOT_FOUND);

        restore();
    }

    /// El bucle real que usará C#: preguntar tamaño con `cap = 0`, reservar y
    /// volver a pedir. Comprueba que el tamaño anunciado es suficiente.
    #[test]
    fn scene_active_name_negocia_el_tamano_del_buffer() {
        let _g = install_fake();

        let needed = unsafe { scene_active_name(std::ptr::null_mut(), 0) };
        assert_eq!(needed, "Nivel1".len() as i32);

        let mut chico = [0i8; 3];
        assert_eq!(
            unsafe { scene_active_name(chico.as_mut_ptr(), chico.len()) },
            BRIDGE_ERR_BUFFER_TOO_SMALL
        );

        let mut buf = vec![0i8; needed as usize + 1];
        let written = unsafe { scene_active_name(buf.as_mut_ptr(), buf.len()) };
        assert_eq!(written, needed);
        let texto = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_str().unwrap();
        assert_eq!(texto, "Nivel1", "el buffer debe quedar terminado en NUL");

        restore();
    }

    #[test]
    fn scene_count_y_scene_name_at_enumeran_el_proyecto() {
        let _g = install_fake();

        assert_eq!(unsafe { scene_count() }, 2);

        let mut buf = vec![0i8; 64];
        let written = unsafe { scene_name_at(1, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(written, "Nivel2".len() as i32);
        let texto = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_str().unwrap();
        assert_eq!(texto, "Nivel2");

        assert_eq!(
            unsafe { scene_name_at(9, buf.as_mut_ptr(), buf.len()) },
            BRIDGE_ERR_NOT_FOUND
        );
        assert_eq!(
            unsafe { scene_name_at(-1, buf.as_mut_ptr(), buf.len()) },
            BRIDGE_ERR_NOT_FOUND
        );

        restore();
    }

    /// El layout de `InputDataFfi` es el contrato con `InputData` en C#
    /// (`LayoutKind.Sequential`): si alguien reordena o cambia un tipo, este
    /// test rompe antes de que el puente lea basura en silencio.
    #[test]
    fn input_data_ffi_layout_es_el_contrato() {
        use std::mem::{offset_of, size_of};
        assert_eq!(offset_of!(InputDataFfi, move_x), 0);
        assert_eq!(offset_of!(InputDataFfi, move_z), 4);
        assert_eq!(offset_of!(InputDataFfi, jump), 8);
        assert_eq!(offset_of!(InputDataFfi, run), 9);
        // `camera` vive en el relleno histórico: no desplaza a nadie.
        assert_eq!(offset_of!(InputDataFfi, camera), 10);
        assert_eq!(offset_of!(InputDataFfi, mouse_x), 12);
        assert_eq!(offset_of!(InputDataFfi, mouse_y), 16);
        assert_eq!(offset_of!(InputDataFfi, mouse_left_held), 20);
        assert_eq!(offset_of!(InputDataFfi, mouse_left_down), 21);
        assert_eq!(offset_of!(InputDataFfi, mouse_left_up), 22);
        assert_eq!(size_of::<InputDataFfi>(), 296);
    }

    /// Con el layout heredado activo, `get_input` escribe SOLO el prefijo
    /// de 12 bytes: el resto del buffer (que en un C# viejo es pila
    /// gestionada ajena al struct) queda intacto. Es la regresión del
    /// segfault del binario puro sin argumentos: el `InputData` viejo de
    /// la DLL recibía los 24 bytes actuales y el frame de `Update` quedaba
    /// corrupto.
    #[test]
    fn get_input_heredado_no_desborda_el_buffer() {
        let _g = FFI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previo = get_input_state();
        set_input_state(InputDataFfi {
            move_x: 1.0,
            move_z: -1.0,
            jump: 1,
            run: 0,
            camera: 0,
            mouse_x: 800.0,
            mouse_y: 500.0,
            mouse_left_held: 1,
            mouse_left_down: 1,
            mouse_left_up: 0,
            ..Default::default()
        });
        set_legacy_input_layout(true);
        // Canario 0xAA en todo el destino: solo deben cambiar los 12
        // primeros bytes, y con los valores del prefijo heredado.
        let mut buf = [0xAAu8; 24];
        let rc = unsafe { get_input(buf.as_mut_ptr() as *mut InputDataFfi) };
        set_legacy_input_layout(false);
        set_input_state(previo);
        assert_eq!(rc, BRIDGE_OK);
        assert_eq!(&buf[0..4], &1.0f32.to_ne_bytes());
        assert_eq!(&buf[4..8], &(-1.0f32).to_ne_bytes());
        assert_eq!(buf[8], 1);
        assert_eq!(buf[9], 0);
        assert!(
            buf[LEGACY_INPUT_DATA_LEN..].iter().all(|&b| b == 0xAA),
            "get_input heredado escribió más allá del prefijo de {LEGACY_INPUT_DATA_LEN} bytes"
        );
    }

    /// Sin el modo heredado, `get_input` escribe el struct completo de
    /// 24 bytes, ratón incluido.
    #[test]
    fn get_input_actual_escribe_el_struct_completo() {
        let _g = FFI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previo = get_input_state();
        set_input_state(InputDataFfi {
            move_x: 0.0,
            move_z: 0.0,
            jump: 0,
            run: 0,
            camera: 0,
            mouse_x: 800.0,
            mouse_y: 500.0,
            mouse_left_held: 1,
            mouse_left_down: 0,
            mouse_left_up: 1,
            ..Default::default()
        });
        set_legacy_input_layout(false);
        // El buffer destino es el struct COMPLETO actual (296 B):
        // con el layout moderno get_input escribe el tamaño entero.
        let mut buf = [0u8; 296];
        let rc = unsafe { get_input(buf.as_mut_ptr() as *mut InputDataFfi) };
        set_input_state(previo);
        assert_eq!(rc, BRIDGE_OK);
        let written = unsafe { *(buf.as_ptr() as *const InputDataFfi) };
        assert_eq!(written.mouse_x, 800.0);
        assert_eq!(written.mouse_y, 500.0);
        assert_eq!(written.mouse_left_held, 1);
        assert_eq!(written.mouse_left_up, 1);
    }

    /// El FFI del punto de interacción publica y limpia el global que la
    /// app entrega al Renderer: ida y vuelta exacta de los dos vec4,
    /// radio negativo clampado y no finitos descartados sin tocar el
    /// valor anterior. La publicación esférica deja la forma a cero
    /// (discriminador 0); la de caja la rellena con el discriminador 1.
    #[test]
    fn interaccion_publica_limpia_y_sanea() {
        let _g = FFI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previo = get_interaction_point_state();

        unsafe { graph_set_interaction_point(37.5, 0.3, -1.25, 1.9) };
        assert_eq!(
            get_interaction_point_state(),
            [37.5, 0.3, -1.25, 1.9, 0.0, 0.0, 0.0, 0.0]
        );

        // Radio negativo: se clampa a cero, la posición sí se acepta.
        unsafe { graph_set_interaction_point(1.0, 2.0, 3.0, -2.0) };
        assert_eq!(
            get_interaction_point_state(),
            [1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );

        // Un NaN no debe envenenar el global: se descarta la llamada.
        unsafe { graph_set_interaction_point(f32::NAN, 0.0, 0.0, 1.0) };
        assert_eq!(
            get_interaction_point_state(),
            [1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );

        // Caja: el punto lleva el centro (radio a cero) y la forma los
        // semiejes, el yaw y el discriminador 1.
        unsafe { graph_set_interaction_box(37.5, 0.3, -1.25, 2.1, 1.6, 0.5) };
        assert_eq!(
            get_interaction_point_state(),
            [37.5, 0.3, -1.25, 0.0, 2.1, 1.6, 0.5, 1.0]
        );

        // Semieje negativo clampado; NaN en el yaw descarta la llamada.
        unsafe { graph_set_interaction_box(1.0, 2.0, 3.0, -2.0, 1.0, 0.0) };
        assert_eq!(
            get_interaction_point_state(),
            [1.0, 2.0, 3.0, 0.0, 0.0, 1.0, 0.0, 1.0]
        );
        unsafe { graph_set_interaction_box(9.0, 9.0, 9.0, 1.0, 1.0, f32::NAN) };
        assert_eq!(
            get_interaction_point_state(),
            [1.0, 2.0, 3.0, 0.0, 0.0, 1.0, 0.0, 1.0]
        );

        // Y publicar la esfera después de la caja apaga la caja: el
        // último publicador define la forma, sin residuos.
        unsafe { graph_set_interaction_point(5.0, 0.0, 5.0, 1.0) };
        assert_eq!(
            get_interaction_point_state(),
            [5.0, 0.0, 5.0, 1.0, 0.0, 0.0, 0.0, 0.0]
        );

        unsafe { graph_clear_interaction_point() };
        assert_eq!(
            get_interaction_point_state(),
            glass_core::shader_graph::INTERACTION_GLOBALS_INACTIVE
        );

        set_interaction_point_state(previo);
    }

    /// Sin puente instalado, `NullBridge` debe degradar a "no disponible" y no
    /// a un pánico que tumbe el proceso entero desde código C#.
    #[test]
    fn null_bridge_degrada_sin_panico() {
        let _g = FFI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        restore();

        let nombre = std::ffi::CString::new("Nivel1").unwrap();
        assert_eq!(unsafe { scene_load(nombre.as_ptr()) }, BRIDGE_ERR_BRIDGE);
        assert_eq!(unsafe { scene_active_name(std::ptr::null_mut(), 0) }, BRIDGE_ERR_NOT_FOUND);
        assert_eq!(unsafe { scene_count() }, BRIDGE_ERR_NOT_FOUND);

        // Instanciación sin puente: degrada a 0 / NOT_FOUND, sin pánico.
        assert_eq!(unsafe { entity_spawn(&spec_valido()) }, 0);
        assert_eq!(unsafe { entity_destroy(7) }, BRIDGE_ERR_NOT_FOUND);
        assert_eq!(unsafe { entity_set_active(7, 1) }, BRIDGE_ERR_NOT_FOUND);
        assert_eq!(unsafe { entity_count() }, BRIDGE_ERR_NOT_FOUND);
        let (mut b, mut c) = (-7, -7);
        assert_eq!(
            unsafe { entity_physics_counts(&mut b, &mut c) },
            BRIDGE_ERR_NOT_FOUND
        );
        assert_eq!((b, c), (-7, -7), "sin puente no se tocan las salidas");
    }
}
