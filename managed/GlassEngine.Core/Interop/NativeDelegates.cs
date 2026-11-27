using System;
using System.Runtime.InteropServices;

namespace GlassEngine.Core.Interop;

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int GetTransformDelegate(ulong entityId, out TransformData transform);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SetTransformDelegate(ulong entityId, ref TransformData transform);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int GetTransformsBatchDelegate(IntPtr ids, IntPtr data, int cap);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SetTransformsBatchDelegate(IntPtr ids, IntPtr data, int count);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate void LogMessageDelegate(int level, IntPtr utf8Message);

// ---------------------------------------------------------------------------
// Carga de escenas: mismo patron reverse-P/Invoke que las demas fronteras.
// ---------------------------------------------------------------------------

/// <summary>Encola la carga de una escena. Devuelve 0 si quedo encolada.</summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SceneLoadDelegate(IntPtr sceneNameUtf8);

/// <summary>Escribe el nombre de la escena activa; con cap=0 solo informa el tamano.</summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SceneActiveNameDelegate(IntPtr buffer, UIntPtr capacity);

/// <summary>Numero de escenas del proyecto.</summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SceneCountDelegate();

/// <summary>Escribe el nombre de la escena en la posicion indicada.</summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SceneNameAtDelegate(int index, IntPtr buffer, UIntPtr capacity);

// ---------------------------------------------------------------------------
// Animacion con piel (Animator.Play, bloque de skinning).
// ---------------------------------------------------------------------------

/// <summary>
/// Pide que la entidad reproduzca el clip indicado. Devuelve 0 si el motor
/// acepto la peticion, -1 si la entidad no existe.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int AnimatorPlayDelegate(ulong entityId, IntPtr clipNameUtf8);

/// <summary>
/// Fija un parámetro del controlador de animación (bloque FSM):
/// kind 0 = bool, 1 = float, 2 = trigger. Devuelve 0 si el motor aceptó.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int AnimatorSetParamDelegate(ulong entityId, IntPtr nameUtf8, int kind, float value);

/// <summary>
/// Escribe el estado actual de la máquina en el búfer (protocolo de dos
/// llamadas: con búfer nulo devuelve los bytes necesarios). Sin
/// controlador, el estado es el clip en curso.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int AnimatorCurrentStateDelegate(ulong entityId, IntPtr buffer, UIntPtr capacity);

/// <summary>
/// Salto con fundido (bloque FSM): a un estado con controlador, o
/// crossfade libre entre clips sin él. Devuelve 0 si el motor aceptó.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int AnimatorCrossFadeDelegate(ulong entityId, IntPtr nameUtf8, float fade);

// ---------------------------------------------------------------------------
// Punto de interacción de los grafos (Shader.SetInteractionPoint).
// ---------------------------------------------------------------------------

/// <summary>
/// Publica el punto de interacción de los grafos: posición de mundo del
/// intruso y su radio de influencia (el vec4 global GraphGlobals).
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate void SetInteractionPointDelegate(float x, float y, float z, float radius);

/// <summary>
/// Limpia el punto de interacción: vuelve a su valor inactivo y los
/// grafos dejan de reaccionar.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate void ClearInteractionPointDelegate();

/// <summary>
/// Publica la interacción con huella de caja en planta: centro de mundo,
/// semiejes de la caja en su marco local (halfX a lo largo de su eje X,
/// halfZ a lo largo de su eje Z) y su rotación Y en radianes. Viaja por
/// su propio puente (<c>InitInteractionBoxBridge</c>), separado del del
/// punto esférico.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate void SetInteractionBoxDelegate(float x, float y, float z, float halfX, float halfZ, float yaw);

// ---------------------------------------------------------------------------
// Instanciación real de entidades (GameObject.Spawn / EntityPool).
// ---------------------------------------------------------------------------

/// <summary>
/// Crea una entidad en la escena viva desde la especificación. Devuelve
/// el id asignado, o 0 si el motor rechazó el spawn.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate ulong EntitySpawnDelegate(ref SpawnSpecData spec);

/// <summary>
/// Destruye una entidad: sale de la escena y su cuerpo y colliders salen
/// del mundo físico. Devuelve 0 si existía, -1 si no.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int EntityDestroyDelegate(ulong entityId);

/// <summary>
/// Activa (1) o desactiva (0) una entidad: la inactiva no se dibuja y no
/// tiene cuerpo; al reactivarla el cuerpo se recrea. Devuelve 0 si la
/// entidad existe, -1 si no.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int EntitySetActiveDelegate(ulong entityId, byte active);

/// <summary>
/// Fija la niebla de la escena en vivo (color RGB lineal, cerca y
/// lejos en metros). Devuelve 0 si la escena tenía niebla que fijar.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SceneSetFogDelegate(float r, float g, float b, float near, float far);

/// <summary>
/// Fija el FOV (grados) de la cámara principal en vivo. Devuelve 0
/// si la escena tenía cámara principal que ajustar.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SceneSetCameraFovDelegate(float fov);

/// <summary>Número de entidades de la escena viva, o -1 sin puente.</summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int EntityCountDelegate();

/// <summary>
/// Escribe los conteos (cuerpos, colliders) del mundo físico vivo.
/// Devuelve 0 si el puente respondió, -1 si no.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int EntityPhysicsCountsDelegate(out int bodies, out int colliders);

/// <summary>
/// Instancia un Prefab (.prefab.json) por nombre o ruta UTF-8 y
/// posición de mundo; devuelve el id de la raíz creada o 0 si el
/// motor lo rechazó. Puente <c>InitPrefabBridge</c>.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate ulong InstantiatePrefabNative(IntPtr path, float x, float y, float z);// ---------------------------------------------------------------------------
// Luces: mismo patron reverse-P/Invoke que las demas fronteras.
// ---------------------------------------------------------------------------

/// <summary>Fija el color de la luz de la entidad. Devuelve 0 si el motor acepto.</summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int LightSetColorDelegate(ulong entityId, float r, float g, float b);

/// <summary>Fija la intensidad de la luz de la entidad. Devuelve 0 si el motor acepto.</summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int LightSetIntensityDelegate(ulong entityId, float intensity);
public delegate int LightGetIntensityDelegate(ulong entityId, out float intensity);

/// <summary>
/// Delegado para establecer el tipo de una luz (Directional/Point/Spot) desde Rust.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int LightSetTypeDelegate(ulong entityId, int type);

/// <summary>
/// Delegado para establecer el rango (radio de influencia) de una luz desde Rust.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int LightSetRangeDelegate(ulong entityId, float range);

/// <summary>
/// Delegado para establecer los ángulos del cono de una luz tipo Spot (exterior e interior) desde Rust.
/// </summary>
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int LightSetSpotAnglesDelegate(ulong entityId, float outerDeg, float innerDeg);

public delegate int UiSetTextDelegate(ulong entityId, IntPtr textUtf8);
public delegate int UiSetFontSizeDelegate(ulong entityId, float fontSize);
public delegate int UiSetTextColorDelegate(ulong entityId, float r, float g, float b, float a);
public delegate int UiSetImageSpriteDelegate(ulong entityId, IntPtr spriteUtf8);
public delegate int UiSetImageColorDelegate(ulong entityId, float r, float g, float b, float a);
public delegate int UiSetButtonColorDelegate(ulong entityId, float r, float g, float b, float a);
public delegate int UiSetAnchoredPosDelegate(ulong entityId, float x, float y);
public delegate int UiSetSizeDeltaDelegate(ulong entityId, float x, float y);

/// <summary>Entidad del último arrastre del Canvas (drag &amp; drop).</summary>
public delegate ulong UiGetDraggedIdDelegate();

/// <summary>Casilla que recibió la última suelta del Canvas (0 = en vacío).</summary>
public delegate ulong UiGetDropTargetIdDelegate();
