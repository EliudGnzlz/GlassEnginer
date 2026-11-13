using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using System.Runtime.InteropServices;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core;

/// <summary>
/// Tipo de física con el que nace una entidad spawneada. Los valores son
/// el contrato con el FFI (campo <c>Physics</c> de
/// <see cref="SpawnSpecData"/>): no reordenar ni renumerar.
/// </summary>
public enum SpawnPhysicsKind
{
    /// <summary>Solo visual: sin cuerpo ni collider.</summary>
    None = 0,
    /// <summary>Cuerpo fijo de Rapier: suelos, muros, piezas de una sala.</summary>
    Static = 1,
    /// <summary>Cuerpo dinámico: cae, choca y la física escribe su transform.</summary>
    Dynamic = 2,
    /// <summary>Cuerpo cinemático: lo mueve el script y empuja a los dinámicos.</summary>
    Kinematic = 3,
}

public static class RuntimeBridge
{
    private static GetTransformDelegate? _getTransform;
    private static SetTransformDelegate? _setTransform;
    private static LogMessageDelegate? _logMessage;

    private static PlayAudio2DDelegate? _playAudio2D;
    private static PlayAudio3DDelegate? _playAudio3D;
    private static SetAudioSourcePositionDelegate? _setAudioSourcePos;
    private static SetListenerTransformDelegate? _setListenerTransform;
    private static StopAudioDelegate? _stopAudio;
    private static PlayAudio2DOnBusDelegate? _playAudio2DOnBus;
    private static PlayAudio3DOnBusDelegate? _playAudio3DOnBus;

    private static PlayParticlesDelegate? _playParticles;
    private static StopParticlesDelegate? _stopParticles;
    private static BurstParticlesDelegate? _burstParticles;

    private static CreateRigidBodyDelegate? _createRigidBody;
    private static AttachBoxColliderDelegate? _attachBoxCollider;
    private static AttachSphereColliderDelegate? _attachSphereCollider;
    private static ApplyImpulseDelegate? _applyImpulse;
    private static ApplyForceDelegate? _applyForce;
    private static GetPhysicsTransformDelegate? _getPhysicsTransform;
    private static SetPhysicsTransformDelegate? _setPhysicsTransform;
    private static RemoveRigidBodyDelegate? _removeRigidBody;
    private static GetLinearVelocityDelegate? _getLinearVelocity;
    private static SetLinearVelocityDelegate? _setLinearVelocity;
    private static CastRayDelegate? _castRay;
    private static CastSphereDelegate? _castSphere;
    private static OverlapSphereDelegate? _overlapSphere;
    private static GetBodyHandleDelegate? _getBodyHandle;
    private static CharacterCreateDelegate? _characterCreate;
    private static CharacterMoveDelegate? _characterMove;
    private static CharacterTeleportDelegate? _characterTeleport;
    private static CharacterDestroyDelegate? _characterDestroy;
    private static CharacterInfoDelegate? _characterInfo;
    private static AttachCapsuleColliderDelegate? _attachCapsuleCollider;
    private static AttachCylinderColliderDelegate? _attachCylinderCollider;

    private static readonly Dictionary<int, GlassBehaviour> _instances = new();
    private static int _nextHandle = 1;

    // Lote de transforms (bloque «FFI por lotes»): los dos exports
    // que Rust entrega con InitTransformBatchBridge. Con ellos, el
    // tick trabaja sobre TransformCache (una lectura y una
    // escritura por frame) en vez de cruzar la frontera por acceso.
    private static GetTransformsBatchDelegate? _getTransformsBatch;
    private static SetTransformsBatchDelegate? _setTransformsBatch;

    public static bool TransformBatchAvailable =>
        _getTransformsBatch != null && _setTransformsBatch != null;

    [UnmanagedCallersOnly(EntryPoint = "InitTransformBatchBridge")]
    public static void InitTransformBatchBridge(IntPtr getFn, IntPtr setFn)
    {
        _getTransformsBatch = Marshal.GetDelegateForFunctionPointer<GetTransformsBatchDelegate>(getFn);
        _setTransformsBatch = Marshal.GetDelegateForFunctionPointer<SetTransformsBatchDelegate>(setFn);
    }

    public static int GetTransformsBatch(IntPtr ids, IntPtr data, int cap)
    {
        return _getTransformsBatch?.Invoke(ids, data, cap) ?? -1;
    }

    public static int SetTransformsBatch(IntPtr ids, IntPtr data, int count)
    {
        return _setTransformsBatch?.Invoke(ids, data, count) ?? -1;
    }
    private static readonly object _sync = new();

    // Caché de resolución de tipos, con negativos memorizados: sin ella, cada
    // creación de instancia recorría todos los ensamblados del dominio.
    private static readonly Dictionary<string, Type?> _typeCache = new();

    [StructLayout(LayoutKind.Sequential)]
    public struct AudioBridgeCallbacks
    {
        public IntPtr Play2D;
        public IntPtr Play3D;
        public IntPtr SetSourcePosition;
        public IntPtr SetListenerTransform;
        public IntPtr Stop;
        // Añadidos AL FINAL (auditoría C6): play con bus.
        public IntPtr Play2DOnBus;
        public IntPtr Play3DOnBus;
    }

    [UnmanagedCallersOnly(EntryPoint = "InitAudioBridge")]
    public static unsafe void InitAudioBridge(AudioBridgeCallbacks* callbacks)
    {
        if (callbacks == null) return;
        if (callbacks->Play2D != IntPtr.Zero)
            _playAudio2D = Marshal.GetDelegateForFunctionPointer<PlayAudio2DDelegate>(callbacks->Play2D);
        if (callbacks->Play3D != IntPtr.Zero)
            _playAudio3D = Marshal.GetDelegateForFunctionPointer<PlayAudio3DDelegate>(callbacks->Play3D);
        if (callbacks->SetSourcePosition != IntPtr.Zero)
            _setAudioSourcePos = Marshal.GetDelegateForFunctionPointer<SetAudioSourcePositionDelegate>(callbacks->SetSourcePosition);
        if (callbacks->SetListenerTransform != IntPtr.Zero)
            _setListenerTransform = Marshal.GetDelegateForFunctionPointer<SetListenerTransformDelegate>(callbacks->SetListenerTransform);
        if (callbacks->Stop != IntPtr.Zero)
            _stopAudio = Marshal.GetDelegateForFunctionPointer<StopAudioDelegate>(callbacks->Stop);
        if (callbacks->Play2DOnBus != IntPtr.Zero)
            _playAudio2DOnBus = Marshal.GetDelegateForFunctionPointer<PlayAudio2DOnBusDelegate>(callbacks->Play2DOnBus);
        if (callbacks->Play3DOnBus != IntPtr.Zero)
            _playAudio3DOnBus = Marshal.GetDelegateForFunctionPointer<PlayAudio3DOnBusDelegate>(callbacks->Play3DOnBus);

        Log(2, "GlassEngine.Core AudioBridge inicializado exitosamente desde Rust.");
    }

    public static ulong PlayAudio2DOnBus(string clipName, string busName, float volume, float pitch, bool looping)
    {
        if (_playAudio2DOnBus is null) return PlayAudio2D(clipName, volume, pitch, looping);
        IntPtr utf8 = Marshal.StringToCoTaskMemUTF8(clipName);
        IntPtr busUtf8 = Marshal.StringToCoTaskMemUTF8(busName);
        try
        {
            return _playAudio2DOnBus(utf8, busUtf8, volume, pitch, looping);
        }
        finally
        {
            Marshal.FreeCoTaskMem(utf8);
            Marshal.FreeCoTaskMem(busUtf8);
        }
    }

    public static ulong PlayAudio3DOnBus(string clipName, string busName, Vector3 position, float volume, float pitch, bool looping)
    {
        if (_playAudio3DOnBus is null) return PlayAudio3D(clipName, position, volume, pitch, looping);
        IntPtr utf8 = Marshal.StringToCoTaskMemUTF8(clipName);
        IntPtr busUtf8 = Marshal.StringToCoTaskMemUTF8(busName);
        try
        {
            return _playAudio3DOnBus(utf8, busUtf8, position.X, position.Y, position.Z, volume, pitch, looping);
        }
        finally
        {
            Marshal.FreeCoTaskMem(utf8);
            Marshal.FreeCoTaskMem(busUtf8);
        }
    }

    public static ulong PlayAudio2D(string clipName, float volume, float pitch, bool looping)
    {
        if (_playAudio2D is null) return 0;
        IntPtr utf8 = Marshal.StringToCoTaskMemUTF8(clipName);
        try
        {
            return _playAudio2D(utf8, volume, pitch, looping);
        }
        finally
        {
            Marshal.FreeCoTaskMem(utf8);
        }
    }

    public static ulong PlayAudio3D(string clipName, Vector3 position, float volume, float pitch, bool looping)
    {
        if (_playAudio3D is null) return 0;
        IntPtr utf8 = Marshal.StringToCoTaskMemUTF8(clipName);
        try
        {
            return _playAudio3D(utf8, position.X, position.Y, position.Z, volume, pitch, looping);
        }
        finally
        {
            Marshal.FreeCoTaskMem(utf8);
        }
    }

    public static void SetAudioSourcePosition(ulong handle, Vector3 position)
    {
        _setAudioSourcePos?.Invoke(handle, position.X, position.Y, position.Z);
    }

    public static void SetListenerTransform(Vector3 position, Quaternion rotation)
    {
        _setListenerTransform?.Invoke(position.X, position.Y, position.Z, rotation.X, rotation.Y, rotation.Z, rotation.W);
    }

    public static void StopAudio(ulong handle)
    {
        _stopAudio?.Invoke(handle);
    }

    [UnmanagedCallersOnly(EntryPoint = "InitParticleBridge")]
    public static void InitParticleBridge(
        IntPtr playFn,
        IntPtr stopFn,
        IntPtr burstFn)
    {
        if (playFn != IntPtr.Zero)
            _playParticles = Marshal.GetDelegateForFunctionPointer<PlayParticlesDelegate>(playFn);
        if (stopFn != IntPtr.Zero)
            _stopParticles = Marshal.GetDelegateForFunctionPointer<StopParticlesDelegate>(stopFn);
        if (burstFn != IntPtr.Zero)
            _burstParticles = Marshal.GetDelegateForFunctionPointer<BurstParticlesDelegate>(burstFn);

        Log(2, "GlassEngine.Core ParticleBridge inicializado exitosamente desde Rust.");
    }

    public static void PlayParticles(ulong entityId)
    {
        _playParticles?.Invoke(entityId);
    }

    public static void StopParticles(ulong entityId)
    {
        _stopParticles?.Invoke(entityId);
    }

    public static void BurstParticles(ulong entityId, uint count)
    {
        _burstParticles?.Invoke(entityId, count);
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct PhysicsBridgeCallbacks
    {
        public IntPtr CreateBody;
        public IntPtr AttachBox;
        public IntPtr AttachSphere;
        public IntPtr ApplyImpulse;
        public IntPtr ApplyForce;
        public IntPtr GetTransform;
        public IntPtr SetTransform;
        public IntPtr RemoveBody;
        public IntPtr GetVelocity;
        public IntPtr SetVelocity;
        // Undécimo puntero, AL FINAL (juego Plat3D): un host anterior no lo
        // escribe y un managed anterior no lo lee; ningún offset se mueve.
        public IntPtr CastRay;
        // Duodécimo y decimotercero, AL FINAL (bloque de primitivas y
        // colliders por forma): attach de cápsula y cilindro, con la
        // misma convención que CastRay.
        public IntPtr AttachCapsule;
        public IntPtr AttachCylinder;
        // Decimocuarto y decimoquinto, AL FINAL (auditoría C5):
        // barrido y solape de esfera para los scripts.
        public IntPtr CastSphere;
        public IntPtr OverlapSphere;
        // Decimosexto, AL FINAL (Fase 9): entidad → handle de cuerpo.
        public IntPtr GetBodyHandle;

        // Decimoséptimo a vigesimoprimero, AL FINAL (bloque
        // CharacterController): personaje kinemático de Rapier.
        public IntPtr CharacterCreate;
        public IntPtr CharacterMove;
        public IntPtr CharacterTeleport;
        public IntPtr CharacterDestroy;
        public IntPtr CharacterInfo;
    }

    [UnmanagedCallersOnly(EntryPoint = "InitPhysicsBridge")]
    public static unsafe void InitPhysicsBridge(PhysicsBridgeCallbacks* callbacks)
    {
        if (callbacks == null) return;
        if (callbacks->CreateBody != IntPtr.Zero)
            _createRigidBody = Marshal.GetDelegateForFunctionPointer<CreateRigidBodyDelegate>(callbacks->CreateBody);
        if (callbacks->AttachBox != IntPtr.Zero)
            _attachBoxCollider = Marshal.GetDelegateForFunctionPointer<AttachBoxColliderDelegate>(callbacks->AttachBox);
        if (callbacks->AttachSphere != IntPtr.Zero)
            _attachSphereCollider = Marshal.GetDelegateForFunctionPointer<AttachSphereColliderDelegate>(callbacks->AttachSphere);
        if (callbacks->ApplyImpulse != IntPtr.Zero)
            _applyImpulse = Marshal.GetDelegateForFunctionPointer<ApplyImpulseDelegate>(callbacks->ApplyImpulse);
        if (callbacks->ApplyForce != IntPtr.Zero)
            _applyForce = Marshal.GetDelegateForFunctionPointer<ApplyForceDelegate>(callbacks->ApplyForce);
        if (callbacks->GetTransform != IntPtr.Zero)
            _getPhysicsTransform = Marshal.GetDelegateForFunctionPointer<GetPhysicsTransformDelegate>(callbacks->GetTransform);
        if (callbacks->SetTransform != IntPtr.Zero)
            _setPhysicsTransform = Marshal.GetDelegateForFunctionPointer<SetPhysicsTransformDelegate>(callbacks->SetTransform);
        if (callbacks->RemoveBody != IntPtr.Zero)
            _removeRigidBody = Marshal.GetDelegateForFunctionPointer<RemoveRigidBodyDelegate>(callbacks->RemoveBody);
        if (callbacks->GetVelocity != IntPtr.Zero)
            _getLinearVelocity = Marshal.GetDelegateForFunctionPointer<GetLinearVelocityDelegate>(callbacks->GetVelocity);
        if (callbacks->SetVelocity != IntPtr.Zero)
            _setLinearVelocity = Marshal.GetDelegateForFunctionPointer<SetLinearVelocityDelegate>(callbacks->SetVelocity);
        if (callbacks->CastRay != IntPtr.Zero)
            _castRay = Marshal.GetDelegateForFunctionPointer<CastRayDelegate>(callbacks->CastRay);
        if (callbacks->AttachCapsule != IntPtr.Zero)
            _attachCapsuleCollider = Marshal.GetDelegateForFunctionPointer<AttachCapsuleColliderDelegate>(callbacks->AttachCapsule);
        if (callbacks->AttachCylinder != IntPtr.Zero)
            _attachCylinderCollider = Marshal.GetDelegateForFunctionPointer<AttachCylinderColliderDelegate>(callbacks->AttachCylinder);
        if (callbacks->CastSphere != IntPtr.Zero)
            _castSphere = Marshal.GetDelegateForFunctionPointer<CastSphereDelegate>(callbacks->CastSphere);
        if (callbacks->OverlapSphere != IntPtr.Zero)
            _overlapSphere = Marshal.GetDelegateForFunctionPointer<OverlapSphereDelegate>(callbacks->OverlapSphere);
        if (callbacks->GetBodyHandle != IntPtr.Zero)
            _getBodyHandle = Marshal.GetDelegateForFunctionPointer<GetBodyHandleDelegate>(callbacks->GetBodyHandle);

        if (callbacks->CharacterCreate != IntPtr.Zero)
            _characterCreate = Marshal.GetDelegateForFunctionPointer<CharacterCreateDelegate>(callbacks->CharacterCreate);

        if (callbacks->CharacterMove != IntPtr.Zero)
            _characterMove = Marshal.GetDelegateForFunctionPointer<CharacterMoveDelegate>(callbacks->CharacterMove);

        if (callbacks->CharacterTeleport != IntPtr.Zero)
            _characterTeleport = Marshal.GetDelegateForFunctionPointer<CharacterTeleportDelegate>(callbacks->CharacterTeleport);

        if (callbacks->CharacterDestroy != IntPtr.Zero)
            _characterDestroy = Marshal.GetDelegateForFunctionPointer<CharacterDestroyDelegate>(callbacks->CharacterDestroy);

        if (callbacks->CharacterInfo != IntPtr.Zero)
            _characterInfo = Marshal.GetDelegateForFunctionPointer<CharacterInfoDelegate>(callbacks->CharacterInfo);

        Log(2, "GlassEngine.Core PhysicsBridge inicializado exitosamente desde Rust.");
    }

    [UnmanagedCallersOnly(EntryPoint = "InitInputBridge")]
    public static void InitInputBridge(IntPtr getInputFn)
    {
        if (getInputFn != IntPtr.Zero)
        {
            var del = Marshal.GetDelegateForFunctionPointer<GetInputDelegate>(getInputFn);
            Input.Initialize(del);
        }
        Log(2, "GlassEngine.Core InputBridge inicializado exitosamente desde Rust.");
    }

private static InputAxisCountDelegate? _inputAxisCount;
    private static InputAxisNameDelegate? _inputAxisName;

    /// <summary>
    /// Inicializa el puente de metadatos de ejes configurables (Input v2)
    /// desde Rust.
    /// </summary>
    [UnmanagedCallersOnly(EntryPoint = "InitInputAxesBridge")]
    public static unsafe void InitInputAxesBridge(InputAxesCallbacks* callbacks)
    {
        if (callbacks == null) return;

        if (callbacks->Count != IntPtr.Zero)
            _inputAxisCount = Marshal.GetDelegateForFunctionPointer<InputAxisCountDelegate>(callbacks->Count);

        if (callbacks->Name != IntPtr.Zero)
            _inputAxisName = Marshal.GetDelegateForFunctionPointer<InputAxisNameDelegate>(callbacks->Name);

        Log(2, "GlassEngine.Core InputAxesBridge inicializado exitosamente desde Rust.");
    }

    /// <summary>
    /// Devuelve el número de ejes configurables publicados por el host.
    /// </summary>
    public static int AxisCount()
    {
        try
        {
            return _inputAxisCount?.Invoke() ?? 0;
        }
        catch
        {
            return 0;
        }
    }

    /// <summary>
    /// Devuelve el nombre del eje configurable en el índice dado, o null
    /// si no existe o el host no lo soporta.
    /// </summary>
    public static string? AxisName(int index)
    {
        if (_inputAxisName == null) return null;

        var buffer = new byte[128];
        int n;
        try
        {
            n = _inputAxisName(index, buffer, buffer.Length);
        }
        catch
        {
            return null;
        }

        if (n <= 0) return null;

        return System.Text.Encoding.UTF8.GetString(buffer, 0, Math.Min(n, buffer.Length));
    }

    public static unsafe GetScriptParamsDelegate? GetScriptParamsNative;

    [UnmanagedCallersOnly(EntryPoint = "InitScriptParamsBridge")]
    public static unsafe void InitScriptParamsBridge(IntPtr getParamsFn)
    {
        if (getParamsFn != IntPtr.Zero)
        {
            GetScriptParamsNative = Marshal.GetDelegateForFunctionPointer<GetScriptParamsDelegate>(getParamsFn);
        }
        Log(2, "GlassEngine.Core ScriptParamsBridge inicializado exitosamente desde Rust.");
    }

    // -----------------------------------------------------------------------
    // Puente de escenas (SceneManager.LoadScene)
    // -----------------------------------------------------------------------

    private static SceneLoadDelegate? _sceneLoad;
    private static SceneActiveNameDelegate? _sceneActiveName;
    private static SceneCountDelegate? _sceneCount;
    private static SceneNameAtDelegate? _sceneNameAt;

    /// <summary>True cuando Rust conecto el puente de escenas.</summary>
    public static bool ScenesAvailable => _sceneLoad is not null;

    [UnmanagedCallersOnly(EntryPoint = "InitSceneBridge")]
    public static void InitSceneBridge(
        IntPtr sceneLoadFn,
        IntPtr activeNameFn,
        IntPtr countFn,
        IntPtr nameAtFn)
    {
        if (sceneLoadFn != IntPtr.Zero)
            _sceneLoad = Marshal.GetDelegateForFunctionPointer<SceneLoadDelegate>(sceneLoadFn);
        if (activeNameFn != IntPtr.Zero)
            _sceneActiveName = Marshal.GetDelegateForFunctionPointer<SceneActiveNameDelegate>(activeNameFn);
        if (countFn != IntPtr.Zero)
            _sceneCount = Marshal.GetDelegateForFunctionPointer<SceneCountDelegate>(countFn);
        if (nameAtFn != IntPtr.Zero)
            _sceneNameAt = Marshal.GetDelegateForFunctionPointer<SceneNameAtDelegate>(nameAtFn);

        Log(2, "GlassEngine.Core SceneBridge inicializado exitosamente desde Rust.");
    }

    /// <summary>
    /// Pide la carga de una escena. Devuelve 0 si quedo encolada, -1 si no
    /// existe, -4 si el puente no esta conectado. No bloquea: la escena cambia
    /// al final del frame, igual que en Unity.
    /// </summary>
    public static int SceneLoad(string sceneName)
    {
        if (_sceneLoad is null) return -4;
        if (sceneName is null) return -2;
        IntPtr utf8 = Marshal.StringToCoTaskMemUTF8(sceneName);
        try
        {
            return _sceneLoad(utf8);
        }
        finally
        {
            Marshal.FreeCoTaskMem(utf8);
        }
    }

    /// <summary>
    /// Pide a Rust un string con el patron de dos pasos (cap=0 informa tamano),
    /// para no adivinar el largo ni arriesgar un buffer corto.
    /// </summary>
    private static string? ReadNativeString(Func<IntPtr, UIntPtr, int> fetch)
    {
        int needed = fetch(IntPtr.Zero, UIntPtr.Zero);
        if (needed < 0) return null;

        var buffer = new byte[needed + 1];
        unsafe
        {
            fixed (byte* ptr = buffer)
            {
                int written = fetch((IntPtr)ptr, (UIntPtr)buffer.Length);
                if (written < 0) return null;
            }
        }
        return System.Text.Encoding.UTF8.GetString(buffer, 0, needed);
    }

    public static string? GetActiveSceneName()
    {
        if (_sceneActiveName is null) return null;
        return ReadNativeString((buf, cap) => _sceneActiveName(buf, cap));
    }

    public static int GetSceneCount()
    {
        if (_sceneCount is null) return 0;
        int count = _sceneCount();
        return count < 0 ? 0 : count;
    }

    public static string? GetSceneNameAt(int index)
    {
        if (_sceneNameAt is null) return null;
        return ReadNativeString((buf, cap) => _sceneNameAt(index, buf, cap));
    }

    // -----------------------------------------------------------------------
    // Puente de animación (Animator.Play, bloque de skinning)
    // -----------------------------------------------------------------------
    //
    // Export NUEVO y separado a propósito, como InvokeButtonClick en su
    // día: InitSceneBridge ya fijó sus punteros posicionales y los
    // ensamblados viejos deben seguir arrancando. Si el motor no encuentra
    // InitAnimationBridge, este delegado queda nulo y AnimatorPlay
    // devuelve -4 en vez de tumbar el proceso.

    private static AnimatorPlayDelegate? _animatorPlay;

    /// <summary>True cuando Rust conectó el puente de animación.</summary>
    public static bool AnimationAvailable => _animatorPlay is not null;

    [UnmanagedCallersOnly(EntryPoint = "InitAnimationBridge")]
    public static void InitAnimationBridge(IntPtr animatorPlayFn)
    {
        if (animatorPlayFn != IntPtr.Zero)
            _animatorPlay = Marshal.GetDelegateForFunctionPointer<AnimatorPlayDelegate>(animatorPlayFn);

        Log(2, "GlassEngine.Core AnimationBridge inicializado exitosamente desde Rust.");
    }

    /// <summary>
    /// Pide que la entidad reproduzca un clip de animación con piel.
    /// Devuelve 0 si el motor aceptó (la entidad existe y su componente
    /// Animator quedó con ese clip en reproducción), -1 si la entidad no
    /// existe, -2 si el nombre es nulo y -4 si el puente no está conectado.
    /// </summary>
    public static int AnimatorPlay(ulong entityId, string clipName)
    {
        if (_animatorPlay is null) return -4;
        if (clipName is null) return -2;
        IntPtr utf8 = Marshal.StringToCoTaskMemUTF8(clipName);
        try
        {
            return _animatorPlay(entityId, utf8);
        }
        finally
        {
            Marshal.FreeCoTaskMem(utf8);
        }
    }

    // -----------------------------------------------------------------------
    // Puente de la máquina de estados de animación (bloque FSM)
    // -----------------------------------------------------------------------
    //
    // Export NUEVO y separado, con el mismo patrón que el de animación:
    // si el motor no encuentra InitAnimatorFsmBridge (un binario anterior
    // a este bloque), los delegados quedan nulos y los métodos FSM
    // devuelven -4 en vez de tumbar el proceso. Animator.Play no pasa
    // por aquí: viaja por el puente de animación de siempre.

    private static AnimatorSetParamDelegate? _animatorSetParam;
    private static AnimatorCurrentStateDelegate? _animatorCurrentState;
    private static AnimatorCrossFadeDelegate? _animatorCrossFade;

    /// <summary>True cuando Rust conectó el puente FSM del Animator.</summary>
    public static bool AnimatorFsmAvailable => _animatorSetParam is not null;

    [UnmanagedCallersOnly(EntryPoint = "InitAnimatorFsmBridge")]
    public static void InitAnimatorFsmBridge(IntPtr setParamFn, IntPtr currentStateFn, IntPtr crossFadeFn)
    {
        if (setParamFn != IntPtr.Zero)
            _animatorSetParam = Marshal.GetDelegateForFunctionPointer<AnimatorSetParamDelegate>(setParamFn);
        if (currentStateFn != IntPtr.Zero)
            _animatorCurrentState = Marshal.GetDelegateForFunctionPointer<AnimatorCurrentStateDelegate>(currentStateFn);
        if (crossFadeFn != IntPtr.Zero)
            _animatorCrossFade = Marshal.GetDelegateForFunctionPointer<AnimatorCrossFadeDelegate>(crossFadeFn);

        Log(2, "GlassEngine.Core AnimatorFsmBridge inicializado exitosamente desde Rust.");
    }

    /// <summary>
    /// Fija un parámetro del controlador: kind 0 = bool, 1 = float,
    /// 2 = trigger. Devuelve 0 si el motor aceptó, -1 si la entidad no
    /// existe o no tiene Animator, -2 si el nombre es nulo y -4 si el
    /// puente no está conectado.
    /// </summary>
    public static int AnimatorSetParam(ulong entityId, string name, int kind, float value)
    {
        if (_animatorSetParam is null) return -4;
        if (name is null) return -2;
        IntPtr utf8 = Marshal.StringToCoTaskMemUTF8(name);
        try
        {
            return _animatorSetParam(entityId, utf8, kind, value);
        }
        finally
        {
            Marshal.FreeCoTaskMem(utf8);
        }
    }

    /// <summary>
    /// Salto con fundido: a un estado si la entidad tiene controlador, o
    /// crossfade libre entre clips si no lo tiene. Mismos códigos que
    /// <see cref="AnimatorSetParam"/>.
    /// </summary>
    public static int AnimatorCrossFade(ulong entityId, string name, float fade)
    {
        if (_animatorCrossFade is null) return -4;
        if (name is null) return -2;
        IntPtr utf8 = Marshal.StringToCoTaskMemUTF8(name);
        try
        {
            return _animatorCrossFade(entityId, utf8, fade);
        }
        finally
        {
            Marshal.FreeCoTaskMem(utf8);
        }
    }

    // -----------------------------------------------------------------------
    // Puente de luces (Light.color / Light.intensity)
    // -----------------------------------------------------------------------
    //
    // Export NUEVO y separado, con el mismo patrón que el FSM: si el
    // motor no encuentra InitLightBridge (un binario anterior a este
    // bloque), los delegados quedan nulos y los setters devuelven -4 en
    // vez de tumbar el proceso.

    private static LightSetColorDelegate? _lightSetColor;
    private static LightSetIntensityDelegate? _lightSetIntensity;

    /// <summary>True cuando Rust conectó el puente de luces.</summary>
    public static bool LightBridgeAvailable => _lightSetColor is not null;

    [UnmanagedCallersOnly(EntryPoint = "InitLightBridge")]
    public static void InitLightBridge(IntPtr setColorFn, IntPtr setIntensityFn)
    {
        if (setColorFn != IntPtr.Zero)
            _lightSetColor = Marshal.GetDelegateForFunctionPointer<LightSetColorDelegate>(setColorFn);
        if (setIntensityFn != IntPtr.Zero)
            _lightSetIntensity = Marshal.GetDelegateForFunctionPointer<LightSetIntensityDelegate>(setIntensityFn);

        Log(2, "GlassEngine.Core LightBridge inicializado exitosamente desde Rust.");
    }

    /// <summary>
    /// Fija el color de la luz de la entidad. Devuelve 0 si el motor
    /// aceptó, -1 si la entidad no existe o no tiene luz y -4 si el
    /// puente no está conectado.
    /// </summary>
    public static int LightSetColor(ulong entityId, float r, float g, float b)
    {
        if (_lightSetColor is null) return -4;
        return _lightSetColor(entityId, r, g, b);
    }

    /// <summary>
    /// Fija la intensidad de la luz de la entidad (el motor la acota a
    /// >= 0.0). Mismos códigos que <see cref="LightSetColor"/>.
    /// </summary>
    public static int LightSetIntensity(ulong entityId, float intensity)
    {
        if (_lightSetIntensity is null) return -4;
        return _lightSetIntensity(entityId, intensity);
    }

// Export NUEVO y separado del puente de luces original (InitLightBridge).
    // Rust lo registra de forma independiente mediante InitLightPropsBridge; si el
    // motor no encuentra InitLightPropsBridge, los delegados quedan nulos y los
    // setters devuelven -4.
    private static LightSetTypeDelegate? _lightSetType;
    private static LightSetRangeDelegate? _lightSetRange;
    private static LightSetSpotAnglesDelegate? _lightSetSpotAngles;

    /// <summary>
    /// Indica si el puente de propiedades extendidas de luz (tipo, rango, ángulos) está conectado.
    /// </summary>
    public static bool LightPropsBridgeAvailable => _lightSetType is not null;

    [UnmanagedCallersOnly(EntryPoint = "InitLightPropsBridge")]
    public static void InitLightPropsBridge(IntPtr setTypeFn, IntPtr setRangeFn, IntPtr setSpotAnglesFn)
    {
        if (setTypeFn != IntPtr.Zero)
        {
            _lightSetType = Marshal.GetDelegateForFunctionPointer<LightSetTypeDelegate>(setTypeFn);
        }

        if (setRangeFn != IntPtr.Zero)
        {
            _lightSetRange = Marshal.GetDelegateForFunctionPointer<LightSetRangeDelegate>(setRangeFn);
        }

        if (setSpotAnglesFn != IntPtr.Zero)
        {
            _lightSetSpotAngles = Marshal.GetDelegateForFunctionPointer<LightSetSpotAnglesDelegate>(setSpotAnglesFn);
        }

        Log(2, "GlassEngine.Core LightPropsBridge inicializado exitosamente desde Rust.");
    }

    /// <summary>
    /// Establece el tipo de una luz. Devuelve 0 si tuvo éxito, -1 si la entidad no existe
    /// o no tiene componente de luz, -4 si el puente no está conectado.
    /// </summary>
    public static int LightSetType(ulong entityId, int type)
    {
        return _lightSetType is null ? -4 : _lightSetType(entityId, type);
    }

    /// <summary>
    /// Establece el rango de una luz. Devuelve 0 si tuvo éxito, -1 si la entidad no existe
    /// o no tiene componente de luz, -4 si el puente no está conectado.
    /// </summary>
    public static int LightSetRange(ulong entityId, float range)
    {
        return _lightSetRange is null ? -4 : _lightSetRange(entityId, range);
    }

    /// <summary>
    /// Establece los ángulos del cono (exterior e interior) de una luz tipo Spot. Devuelve 0
    /// si tuvo éxito, -1 si la entidad no existe o no tiene componente de luz, -4 si el
    /// puente no está conectado.
    /// </summary>
    public static int LightSetSpotAngles(ulong entityId, float outerDeg, float innerDeg)
    {
        return _lightSetSpotAngles is null ? -4 : _lightSetSpotAngles(entityId, outerDeg, innerDeg);
    }

[StructLayout(LayoutKind.Sequential)]
public struct UiBridgeCallbacks
{
    public IntPtr SetText;
    public IntPtr SetFontSize;
    public IntPtr SetTextColor;
    public IntPtr SetImageSprite;
    public IntPtr SetImageColor;
    public IntPtr SetButtonColor;
    public IntPtr SetAnchoredPos;
    public IntPtr SetSizeDelta;
        // Añadidos AL FINAL (drag & drop): getters del último gesto.
        public IntPtr GetDraggedId;
        public IntPtr GetDropTargetId;
}

private static UiSetTextDelegate? _uiSetText;
private static UiSetFontSizeDelegate? _uiSetFontSize;
private static UiSetTextColorDelegate? _uiSetTextColor;
private static UiSetImageSpriteDelegate? _uiSetImageSprite;
private static UiSetImageColorDelegate? _uiSetImageColor;
private static UiSetButtonColorDelegate? _uiSetButtonColor;
private static UiSetAnchoredPosDelegate? _uiSetAnchoredPos;
private static UiSetSizeDeltaDelegate? _uiSetSizeDelta;
    private static UiGetDraggedIdDelegate? _uiGetDraggedId;
    private static UiGetDropTargetIdDelegate? _uiGetDropTargetId;

public static bool UiBridgeAvailable => _uiSetText != null;

[UnmanagedCallersOnly(EntryPoint = "InitUiBridge")]
public static unsafe void InitUiBridge(UiBridgeCallbacks* callbacks)
{
    if (callbacks == null) return;

    if (callbacks->SetText != IntPtr.Zero)
        _uiSetText = Marshal.GetDelegateForFunctionPointer<UiSetTextDelegate>(callbacks->SetText);
    if (callbacks->SetFontSize != IntPtr.Zero)
        _uiSetFontSize = Marshal.GetDelegateForFunctionPointer<UiSetFontSizeDelegate>(callbacks->SetFontSize);
    if (callbacks->SetTextColor != IntPtr.Zero)
        _uiSetTextColor = Marshal.GetDelegateForFunctionPointer<UiSetTextColorDelegate>(callbacks->SetTextColor);
    if (callbacks->SetImageSprite != IntPtr.Zero)
        _uiSetImageSprite = Marshal.GetDelegateForFunctionPointer<UiSetImageSpriteDelegate>(callbacks->SetImageSprite);
    if (callbacks->SetImageColor != IntPtr.Zero)
        _uiSetImageColor = Marshal.GetDelegateForFunctionPointer<UiSetImageColorDelegate>(callbacks->SetImageColor);
    if (callbacks->SetButtonColor != IntPtr.Zero)
        _uiSetButtonColor = Marshal.GetDelegateForFunctionPointer<UiSetButtonColorDelegate>(callbacks->SetButtonColor);
    if (callbacks->SetAnchoredPos != IntPtr.Zero)
        _uiSetAnchoredPos = Marshal.GetDelegateForFunctionPointer<UiSetAnchoredPosDelegate>(callbacks->SetAnchoredPos);
    if (callbacks->SetSizeDelta != IntPtr.Zero)
        _uiSetSizeDelta = Marshal.GetDelegateForFunctionPointer<UiSetSizeDeltaDelegate>(callbacks->SetSizeDelta);
        if (callbacks->GetDraggedId != IntPtr.Zero)
            _uiGetDraggedId = Marshal.GetDelegateForFunctionPointer<UiGetDraggedIdDelegate>(callbacks->GetDraggedId);
        if (callbacks->GetDropTargetId != IntPtr.Zero)
            _uiGetDropTargetId = Marshal.GetDelegateForFunctionPointer<UiGetDropTargetIdDelegate>(callbacks->GetDropTargetId);

    Log(2, "GlassEngine.Core UiBridge inicializado exitosamente desde Rust.");
}

public static int UiSetText(ulong entityId, string text)
{
    if (_uiSetText is null) return -4;
    IntPtr textUtf8 = Marshal.StringToCoTaskMemUTF8(text);
    try
    {
        return _uiSetText(entityId, textUtf8);
    }
    finally
    {
        Marshal.FreeCoTaskMem(textUtf8);
    }
}

public static int UiSetFontSize(ulong entityId, float fontSize)
{
    if (_uiSetFontSize is null) return -4;
    return _uiSetFontSize(entityId, fontSize);
}

public static int UiSetTextColor(ulong entityId, float r, float g, float b, float a)
{
    if (_uiSetTextColor is null) return -4;
    return _uiSetTextColor(entityId, r, g, b, a);
}

public static int UiSetImageSprite(ulong entityId, string sprite)
{
    if (_uiSetImageSprite is null) return -4;
    IntPtr spriteUtf8 = Marshal.StringToCoTaskMemUTF8(sprite);
    try
    {
        return _uiSetImageSprite(entityId, spriteUtf8);
    }
    finally
    {
        Marshal.FreeCoTaskMem(spriteUtf8);
    }
}

public static int UiSetImageColor(ulong entityId, float r, float g, float b, float a)
{
    if (_uiSetImageColor is null) return -4;
    return _uiSetImageColor(entityId, r, g, b, a);
}

public static int UiSetButtonColor(ulong entityId, float r, float g, float b, float a)
{
    if (_uiSetButtonColor is null) return -4;
    return _uiSetButtonColor(entityId, r, g, b, a);
}

public static int UiSetAnchoredPos(ulong entityId, float x, float y)
{
    if (_uiSetAnchoredPos is null) return -4;
    return _uiSetAnchoredPos(entityId, x, y);
}

public static int UiSetSizeDelta(ulong entityId, float x, float y)
{
    if (_uiSetSizeDelta is null) return -4;
    return _uiSetSizeDelta(entityId, x, y);
}

    /// <summary>Entidad del último arrastre del Canvas (0 si aún no hubo ninguno).</summary>
    public static ulong UiGetDraggedId()
    {
        if (_uiGetDraggedId is null) return 0;
        return _uiGetDraggedId();
    }

    /// <summary>Casilla que recibió la última suelta (0 si se soltó en vacío o no hubo suelta).</summary>
    public static ulong UiGetDropTargetId()
    {
        if (_uiGetDropTargetId is null) return 0;
        return _uiGetDropTargetId();
    }
    /// <summary>
    /// Estado actual de la máquina: con controlador, el nombre del
    /// estado; sin controlador, el clip en curso. Cadena vacía si el
    /// puente no está conectado o la entidad no tiene Animator.
    /// </summary>
    public static string AnimatorCurrentState(ulong entityId)
    {
        if (_animatorCurrentState is null) return "";
        return ReadNativeString((buf, cap) => _animatorCurrentState(entityId, buf, cap)) ?? "";
    }

    // -----------------------------------------------------------------------
    // Puente del punto de interacción (Shader.SetInteractionPoint)
    // -----------------------------------------------------------------------
    //
    // Export NUEVO y separado, con el mismo patrón que el de animación:
    // si el motor no encuentra InitInteractionBridge (un binario anterior
    // a este bloque), los delegados quedan nulos y la publicación es un
    // no-op silencioso en vez de tumbar el proceso.

    private static SetInteractionPointDelegate? _setInteractionPoint;
    private static ClearInteractionPointDelegate? _clearInteractionPoint;

    /// <summary>True cuando Rust conectó el puente de interacción.</summary>
    public static bool InteractionAvailable => _setInteractionPoint is not null;

    [UnmanagedCallersOnly(EntryPoint = "InitInteractionBridge")]
    public static void InitInteractionBridge(IntPtr setFn, IntPtr clearFn)
    {
        if (setFn != IntPtr.Zero)
            _setInteractionPoint = Marshal.GetDelegateForFunctionPointer<SetInteractionPointDelegate>(setFn);
        if (clearFn != IntPtr.Zero)
            _clearInteractionPoint = Marshal.GetDelegateForFunctionPointer<ClearInteractionPointDelegate>(clearFn);

        Log(2, "GlassEngine.Core InteractionBridge inicializado exitosamente desde Rust.");
    }

    /// <summary>
    /// Publica el punto de interacción que leerán los grafos este frame
    /// (posición de mundo del intruso + radio en el vec4 global).
    /// Sin puente conectado no hace nada.
    /// </summary>
    public static void SetInteractionPoint(float x, float y, float z, float radius)
    {
        _setInteractionPoint?.Invoke(x, y, z, radius);
    }

    /// <summary>
    /// Limpia el punto de interacción (vuelve a su valor inactivo).
    /// Sin puente conectado no hace nada.
    /// </summary>
    public static void ClearInteractionPoint()
    {
        _clearInteractionPoint?.Invoke();
    }

    // -----------------------------------------------------------------------
    // Puente de la huella de caja (Shader.SetInteractionBox)
    // -----------------------------------------------------------------------
    //
    // Export NUEVO y separado, como el de animación: InitInteractionBridge
    // ya fijó sus punteros posicionales y no se toca. Si el motor no
    // encuentra InitInteractionBoxBridge (un binario anterior a este
    // bloque), el delegado queda nulo y publicar la caja es un no-op
    // silencioso; la publicación esférica sigue funcionando por su
    // propio puente.

    private static SetInteractionBoxDelegate? _setInteractionBox;

    /// <summary>True cuando Rust conectó el puente de la huella de caja.</summary>
    public static bool InteractionBoxAvailable => _setInteractionBox is not null;

    [UnmanagedCallersOnly(EntryPoint = "InitInteractionBoxBridge")]
    public static void InitInteractionBoxBridge(IntPtr boxFn)
    {
        if (boxFn != IntPtr.Zero)
            _setInteractionBox = Marshal.GetDelegateForFunctionPointer<SetInteractionBoxDelegate>(boxFn);

        Log(2, "GlassEngine.Core InteractionBoxBridge inicializado exitosamente desde Rust.");
    }

    /// <summary>
    /// Publica la interacción con huella de caja que leerán los grafos
    /// este frame (centro + forma en el canal global GraphGlobals).
    /// Sin puente conectado no hace nada.
    /// </summary>
    public static void SetInteractionBox(float x, float y, float z, float halfX, float halfZ, float yaw)
    {
        _setInteractionBox?.Invoke(x, y, z, halfX, halfZ, yaw);
    }

    // -----------------------------------------------------------------------
    // Puente de instanciación real (GameObject.Spawn / EntityPool)
    // -----------------------------------------------------------------------
    //
    // Export NUEVO y separado, como el de animación: los Init anteriores
    // ya fijaron sus punteros posicionales y no se tocan. Si el motor no
    // encuentra InitSpawnBridge (un binario anterior a este bloque), los
    // delegados quedan nulos y todas las operaciones de este puente
    // devuelven el error controlado de la casa (0 / false / -1).

    private static EntitySpawnDelegate? _entitySpawn;
    private static EntityDestroyDelegate? _entityDestroy;
    private static EntitySetActiveDelegate? _entitySetActive;
    private static EntityCountDelegate? _entityCount;
    private static EntityPhysicsCountsDelegate? _entityPhysicsCounts;

    /// <summary>
    /// True cuando Rust conectó el puente de instanciación COMPLETO:
    /// los cinco delegados, no solo el de spawn (una conexión parcial
    /// dejaría Spawn funcionando y Destroy mintiendo en silencio).
    /// </summary>
    public static bool SpawnAvailable =>
        _entitySpawn is not null && _entityDestroy is not null && _entitySetActive is not null
        && _entityCount is not null && _entityPhysicsCounts is not null;

    [UnmanagedCallersOnly(EntryPoint = "InitSpawnBridge")]
    public static void InitSpawnBridge(
        IntPtr spawnFn, IntPtr destroyFn, IntPtr setActiveFn, IntPtr countFn, IntPtr physicsCountsFn)
    {
        if (spawnFn != IntPtr.Zero)
            _entitySpawn = Marshal.GetDelegateForFunctionPointer<EntitySpawnDelegate>(spawnFn);
        if (destroyFn != IntPtr.Zero)
            _entityDestroy = Marshal.GetDelegateForFunctionPointer<EntityDestroyDelegate>(destroyFn);
        if (setActiveFn != IntPtr.Zero)
            _entitySetActive = Marshal.GetDelegateForFunctionPointer<EntitySetActiveDelegate>(setActiveFn);
        if (countFn != IntPtr.Zero)
            _entityCount = Marshal.GetDelegateForFunctionPointer<EntityCountDelegate>(countFn);
        if (physicsCountsFn != IntPtr.Zero)
            _entityPhysicsCounts = Marshal.GetDelegateForFunctionPointer<EntityPhysicsCountsDelegate>(physicsCountsFn);

        Log(2, "GlassEngine.Core SpawnBridge inicializado exitosamente desde Rust.");
    }

    // -----------------------------------------------------------------------
    // Puente de instanciación de Prefabs (GameObject.InstantiatePrefab)
    // -----------------------------------------------------------------------
    //
    // Export NUEVO e independiente del de Spawn: si Rust es un binario
    // anterior a este bloque y no expone InitPrefabBridge, el delegado
    // queda nulo y InstantiatePrefab devuelve 0 con log, como el resto
    // del puente de instanciación ante un motor viejo.

    private static InstantiatePrefabNative? _instantiatePrefab;

    /// <summary>
    /// True cuando Rust conectó el puente de instanciación de prefabs.
    /// </summary>
    public static bool PrefabBridgeAvailable => _instantiatePrefab is not null;

    [UnmanagedCallersOnly(EntryPoint = "InitPrefabBridge")]
    public static void InitPrefabBridge(IntPtr instantiatePrefabFn)
    {
        if (instantiatePrefabFn != IntPtr.Zero)
            _instantiatePrefab = Marshal.GetDelegateForFunctionPointer<InstantiatePrefabNative>(instantiatePrefabFn);

        Log(2, "GlassEngine.Core PrefabBridge inicializado exitosamente desde Rust.");
    }

    /// <summary>
    /// Instancia un Prefab (`.prefab.json`) en la escena viva, por
    /// nombre ("Gold_Coin" -> assets/prefabs/Gold_Coin.prefab.json) o
    /// por ruta explícita (contiene '/' o termina en .prefab.json; se
    /// usa tal cual). Devuelve el id de la entidad raíz creada, o 0 si
    /// el puente no está disponible o el motor rechazó la
    /// instanciación (ver el log).
    /// </summary>
    public static ulong InstantiatePrefab(string nameOrPath, Vector3 position)
    {
        if (_instantiatePrefab is null)
        {
            Log(0, "[InstantiatePrefab] Puente de prefabs no inicializado (falta InitPrefabBridge).");
            return 0;
        }
        IntPtr pathPtr = Marshal.StringToCoTaskMemUTF8(nameOrPath);
        try
        {
            return _instantiatePrefab(pathPtr, position.X, position.Y, position.Z);
        }
        finally
        {
            Marshal.FreeCoTaskMem(pathPtr);
        }
    }


    /// <summary>
    /// Crea una entidad REAL en la escena viva: malla por nombre
    /// ("cube", "sphere", "capsule", "cylinder", "plane" o un modelo
    /// importado), transformada local (relativa a
    /// <paramref name="parentId"/>; 0 = raíz), física cuya forma se
    /// infiere de la malla como en las escenas autoradas, y clase de
    /// script opcional (su instancia nace antes del siguiente Update).
    /// Devuelve el id de la entidad, o 0 si el puente no está o el
    /// motor rechazó el spawn (ver el log). La transformada es la LOCAL
    /// de la entidad, como el Transform de Unity.
    /// </summary>
    public static ulong SpawnEntity(
        string? name, string? model, string? scriptClass, ulong parentId,
        Vector3 position, Quaternion rotation, Vector3 scale,
        SpawnPhysicsKind physics, Vector3? color)
    {
        if (_entitySpawn is null) return 0;
        IntPtr namePtr = name is null ? IntPtr.Zero : Marshal.StringToCoTaskMemUTF8(name);
        IntPtr modelPtr = model is null ? IntPtr.Zero : Marshal.StringToCoTaskMemUTF8(model);
        IntPtr scriptPtr = scriptClass is null ? IntPtr.Zero : Marshal.StringToCoTaskMemUTF8(scriptClass);
        try
        {
            var spec = new SpawnSpecData
            {
                Name = namePtr,
                Model = modelPtr,
                Script = scriptPtr,
                ParentId = parentId,
                Position = position,
                Rotation = rotation,
                Scale = scale,
                Color = color ?? Vector3.Zero,
                Physics = (int)physics,
                HasColor = (byte)(color.HasValue ? 1 : 0),
            };
            return _entitySpawn(ref spec);
        }
        finally
        {
            if (namePtr != IntPtr.Zero) Marshal.FreeCoTaskMem(namePtr);
            if (modelPtr != IntPtr.Zero) Marshal.FreeCoTaskMem(modelPtr);
            if (scriptPtr != IntPtr.Zero) Marshal.FreeCoTaskMem(scriptPtr);
        }
    }

    /// <summary>
    /// Destruye una entidad: sale de la escena y su cuerpo y colliders
    /// salen del mundo físico en la misma llamada; las instancias de
    /// script que tuviera reciben su OnDestroy en este frame (en la
    /// reconciliación del motor, nunca dentro de esta llamada).
    /// Devuelve true si la entidad existía.
    /// </summary>
    public static bool DestroyEntity(ulong entityId)
    {
        if (_entityDestroy is null) return false;
        return _entityDestroy(entityId) == 0;
    }

    /// <summary>
    /// Activa o desactiva una entidad. La inactiva no se dibuja y su
    /// cuerpo sale del mundo físico; al reactivarla, el cuerpo se
    /// recrea desde su PhysicsData y la forma inferida de su malla.
    /// Es la operación de préstamo de los pools. Devuelve true si la
    /// entidad existe.
    /// </summary>
    public static bool SetEntityActive(ulong entityId, bool active)
    {
        if (_entitySetActive is null) return false;
        return _entitySetActive(entityId, (byte)(active ? 1 : 0)) == 0;
    }

    // -----------------------------------------------------------------------
    // Puente de niebla en vivo (RuntimeBridge.SetFog)
    // -----------------------------------------------------------------------
    // Export NUEVO e independiente (InitFogBridge): si el motor es un
    // binario anterior a este bloque, el delegado queda nulo y SetFog
    // devuelve false, como el resto de puentes ante un motor viejo.

    private static SceneSetFogDelegate? _sceneSetFog;

    /// <summary>True cuando Rust conectó el puente de niebla.</summary>
    public static bool FogBridgeAvailable => _sceneSetFog is not null;

    [UnmanagedCallersOnly(EntryPoint = "InitFogBridge")]
    public static void InitFogBridge(IntPtr fogFn)
    {
        if (fogFn != IntPtr.Zero)
            _sceneSetFog = Marshal.GetDelegateForFunctionPointer<SceneSetFogDelegate>(fogFn);

        Log(2, "GlassEngine.Core FogBridge inicializado exitosamente desde Rust.");
    }

    /// <summary>
    /// Fija la niebla de la escena en vivo: color RGB lineal y
    /// distancias de inicio (near) y niebla total (far), en metros.
    /// Pensado para sistemas de clima. Devuelve true si la escena
    /// tenía una entidad con niebla que actualizar.
    /// </summary>
    public static bool SetFog(float r, float g, float b, float near, float far)
    {
        if (_sceneSetFog is null) return false;
        return _sceneSetFog(r, g, b, near, far) == 0;
    }

    // -----------------------------------------------------------------------
    // Puente de FOV de cámara (RuntimeBridge.SetCameraFov, Fase 10)
    // -----------------------------------------------------------------------
    // Export NUEVO e independiente (InitCameraFovBridge): si el motor
    // es un binario anterior a este bloque, el delegado queda nulo y
    // SetCameraFov devuelve false.

    private static SceneSetCameraFovDelegate? _sceneSetCameraFov;

    /// <summary>True cuando Rust conectó el puente de FOV.</summary>
    public static bool CameraFovBridgeAvailable => _sceneSetCameraFov is not null;

    [UnmanagedCallersOnly(EntryPoint = "InitCameraFovBridge")]
    public static void InitCameraFovBridge(IntPtr fovFn)
    {
        if (fovFn != IntPtr.Zero)
            _sceneSetCameraFov = Marshal.GetDelegateForFunctionPointer<SceneSetCameraFovDelegate>(fovFn);

        Log(2, "GlassEngine.Core CameraFovBridge inicializado exitosamente desde Rust.");
    }

    /// <summary>
    /// Fija el FOV (grados) de la cámara principal en vivo. Pensado
    /// para miras con zoom. Devuelve true si había cámara principal.
    /// </summary>
    public static bool SetCameraFov(float fov)
    {
        if (_sceneSetCameraFov is null) return false;
        return _sceneSetCameraFov(fov) == 0;
    }

    /// <summary>Número de entidades de la escena viva, o -1 sin puente.</summary>
    public static int GetEntityCount()
    {
        if (_entityCount is null) return -1;
        return _entityCount();
    }

    /// <summary>
    /// Conteos (cuerpos, colliders) del mundo físico vivo, contados por
    /// el propio Rapier: la cifra con la que un generador verifica que
    /// tras borrar una sala todo volvió a la base. Devuelve false sin
    /// puente (las salidas quedan a 0).
    /// </summary>
    public static bool GetPhysicsCounts(out int bodies, out int colliders)
    {
        bodies = 0;
        colliders = 0;
        if (_entityPhysicsCounts is null) return false;
        return _entityPhysicsCounts(out bodies, out colliders) == 0;
    }

    public static ulong CreateRigidBody(ulong entityId, int bodyType, Vector3 position, Quaternion rotation)
    {
        if (_createRigidBody is null) return 0;
        return _createRigidBody(entityId, bodyType, position.X, position.Y, position.Z, rotation.X, rotation.Y, rotation.Z, rotation.W);
    }

    public static ulong AttachBoxCollider(ulong bodyHandle, Vector3 halfExtents, float restitution, float friction)
    {
        if (_attachBoxCollider is null) return 0;
        return _attachBoxCollider(bodyHandle, halfExtents.X, halfExtents.Y, halfExtents.Z, restitution, friction);
    }

    public static ulong AttachSphereCollider(ulong bodyHandle, float radius, float restitution, float friction)
    {
        if (_attachSphereCollider is null) return 0;
        return _attachSphereCollider(bodyHandle, radius, restitution, friction);
    }

    public static ulong AttachCapsuleCollider(ulong bodyHandle, float halfHeight, float radius, float restitution, float friction)
    {
        if (_attachCapsuleCollider is null) return 0;
        return _attachCapsuleCollider(bodyHandle, halfHeight, radius, restitution, friction);
    }

    public static ulong AttachCylinderCollider(ulong bodyHandle, float halfHeight, float radius, float restitution, float friction)
    {
        if (_attachCylinderCollider is null) return 0;
        return _attachCylinderCollider(bodyHandle, halfHeight, radius, restitution, friction);
    }

    public static bool ApplyImpulse(ulong bodyHandle, Vector3 impulse)
    {
        if (_applyImpulse is null) return false;
        return _applyImpulse(bodyHandle, impulse.X, impulse.Y, impulse.Z);
    }

    public static bool ApplyForce(ulong bodyHandle, Vector3 force)
    {
        if (_applyForce is null) return false;
        return _applyForce(bodyHandle, force.X, force.Y, force.Z);
    }

    public static bool GetPhysicsTransform(ulong bodyHandle, out Vector3 position, out Quaternion rotation)
    {
        if (_getPhysicsTransform is null)
        {
            position = Vector3.Zero;
            rotation = Quaternion.Identity;
            return false;
        }
        return _getPhysicsTransform(bodyHandle, out position, out rotation);
    }

    public static bool SetPhysicsTransform(ulong bodyHandle, Vector3 position, Quaternion rotation)
    {
        if (_setPhysicsTransform is null) return false;
        return _setPhysicsTransform(bodyHandle, position.X, position.Y, position.Z, rotation.X, rotation.Y, rotation.Z, rotation.W);
    }

    public static bool RemoveRigidBody(ulong bodyHandle)
    {
        if (_removeRigidBody is null) return false;
        return _removeRigidBody(bodyHandle);
    }

    public static bool GetLinearVelocity(ulong bodyHandle, out Vector3 velocity)
    {
        if (_getLinearVelocity is null)
        {
            velocity = Vector3.Zero;
            return false;
        }
        return _getLinearVelocity(bodyHandle, out velocity);
    }

    public static bool SetLinearVelocity(ulong bodyHandle, Vector3 velocity)
    {
        if (_setLinearVelocity is null) return false;
        return _setLinearVelocity(bodyHandle, velocity.X, velocity.Y, velocity.Z);
    }

    /// <summary>
    /// Raycast contra los colliders del mundo físico. Devuelve false sin
    /// impacto o si el host no enlazó el callback (degradación segura).
    /// </summary>
    public static bool CastRay(Vector3 origin, Vector3 direction, float maxDistance, ulong excludeEntityId,
        out ulong hitEntityId, out Vector3 hitPoint, out Vector3 hitNormal, out float hitDistance)
    {
        hitEntityId = 0;
        hitPoint = Vector3.Zero;
        hitNormal = Vector3.Zero;
        hitDistance = 0f;
        if (_castRay is null) return false;
        bool ok = _castRay(
            origin.X, origin.Y, origin.Z,
            direction.X, direction.Y, direction.Z,
            maxDistance, excludeEntityId,
            out hitEntityId,
            out float px, out float py, out float pz,
            out float nx, out float ny, out float nz,
            out hitDistance);
        if (ok)
        {
            hitPoint = new Vector3(px, py, pz);
            hitNormal = new Vector3(nx, ny, nz);
        }
        return ok;
    }

    /// <summary>
    /// Barrido de esfera contra los colliders del mundo (auditoría
    /// C5). Devuelve false sin impacto o sin callback enlazado.
    /// </summary>
    public static bool CastSphere(Vector3 origin, float radius, Vector3 direction, float maxDistance, ulong excludeEntityId,
        out ulong hitEntityId, out Vector3 hitPoint, out Vector3 hitNormal, out float hitDistance)
    {
        hitEntityId = 0;
        hitPoint = Vector3.Zero;
        hitNormal = Vector3.Zero;
        hitDistance = 0f;
        if (_castSphere is null) return false;
        bool ok = _castSphere(
            origin.X, origin.Y, origin.Z, radius,
            direction.X, direction.Y, direction.Z,
            maxDistance, excludeEntityId,
            out hitEntityId,
            out float px, out float py, out float pz,
            out float nx, out float ny, out float nz,
            out hitDistance);
        if (ok)
        {
            hitPoint = new Vector3(px, py, pz);
            hitNormal = new Vector3(nx, ny, nz);
        }
        return ok;
    }

    /// <summary>
    /// Handle del cuerpo físico de una entidad de la escena (Fase
    /// 9). Las operaciones de velocidad/impulso van por handle; sin
    /// esto un script no podía mover un cuerpo creado por la escena.
    /// 0 si la entidad no tiene cuerpo o el host no lo enlazó.
    /// </summary>
    public static ulong GetBodyHandle(ulong entityId)
    {
        if (_getBodyHandle is null) return 0;
        return _getBodyHandle(entityId);
    }

    public static bool CharacterCreate(ulong entityId, Vector3 position, float radius, float height, float skin, float stepOffset, float slopeLimitDeg, float snap)
    {
        if (_characterCreate is null) return false;
        return _characterCreate(entityId, position.X, position.Y, position.Z, radius, height, skin, stepOffset, slopeLimitDeg, snap);
    }

    public static bool CharacterMove(ulong entityId, Vector3 motion, float dt, out Vector3 applied, out bool grounded, out bool sliding)
    {
        if (_characterMove is null)
        {
            applied = Vector3.Zero;
            grounded = false;
            sliding = false;
            return false;
        }

        bool ok = _characterMove(entityId, motion.X, motion.Y, motion.Z, dt,
            out float appliedX, out float appliedY, out float appliedZ,
            out int groundedI32, out int slidingI32);

        applied = new Vector3(appliedX, appliedY, appliedZ);
        grounded = groundedI32 != 0;
        sliding = slidingI32 != 0;
        return ok;
    }

    public static bool CharacterTeleport(ulong entityId, Vector3 position)
    {
        if (_characterTeleport is null) return false;
        return _characterTeleport(entityId, position.X, position.Y, position.Z);
    }

    public static bool CharacterDestroy(ulong entityId)
    {
        if (_characterDestroy is null) return false;
        return _characterDestroy(entityId);
    }

    public static bool CharacterInfo(ulong entityId, out Vector3 position, out bool grounded, out bool sliding)
    {
        if (_characterInfo is null)
        {
            position = Vector3.Zero;
            grounded = false;
            sliding = false;
            return false;
        }

        bool ok = _characterInfo(entityId,
            out float posX, out float posY, out float posZ,
            out int groundedI32, out int slidingI32);

        position = new Vector3(posX, posY, posZ);
        grounded = groundedI32 != 0;
        sliding = slidingI32 != 0;
        return ok;
    }

    /// <summary>
    /// Entidades cuyos colliders intersectan la esfera (auditoría
    /// C5). El buffer crece ×4 mientras la consulta lo llene (tope
    /// 65536): antes era fijo de 64 y truncaba en silencio — la
    /// explosión del RPG de la Fase 10 solo afectaba a 64 cuerpos
    /// de una pila de 1.000. Array vacío si no hay callback.
    /// </summary>
    public static ulong[] OverlapSphere(Vector3 center, float radius, ulong excludeEntityId)
    {
        if (_overlapSphere is null) return Array.Empty<ulong>();
        int capacidad = 256;
        while (true)
        {
            var buffer = new ulong[capacidad];
            var handle = System.Runtime.InteropServices.GCHandle.Alloc(buffer, System.Runtime.InteropServices.GCHandleType.Pinned);
            try
            {
                int n = _overlapSphere(center.X, center.Y, center.Z, radius, excludeEntityId,
                    handle.AddrOfPinnedObject(), (uint)buffer.Length);
                if (n <= 0) return Array.Empty<ulong>();
                if (n < buffer.Length || capacidad >= 65536)
                {
                    var result = new ulong[n];
                    Array.Copy(buffer, result, n);
                    return result;
                }
            }
            finally
            {
                handle.Free();
            }
            capacidad *= 4;
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "InitBridge")]
    public static void InitBridge(IntPtr getTransformFn, IntPtr setTransformFn, IntPtr logFn)
    {
        if (getTransformFn != IntPtr.Zero)
            _getTransform = Marshal.GetDelegateForFunctionPointer<GetTransformDelegate>(getTransformFn);
        if (setTransformFn != IntPtr.Zero)
            _setTransform = Marshal.GetDelegateForFunctionPointer<SetTransformDelegate>(setTransformFn);
        if (logFn != IntPtr.Zero)
            _logMessage = Marshal.GetDelegateForFunctionPointer<LogMessageDelegate>(logFn);

        Log(2, "GlassEngine.Core RuntimeBridge inicializado exitosamente desde Rust.");
    }

    public static int GetTransform(ulong entityId, out TransformData transform)
    {
        if (_getTransform is null)
        {
            transform = TransformData.Default;
            return -1;
        }

        return _getTransform(entityId, out transform);
    }

    public static int SetTransform(ulong entityId, ref TransformData transform)
    {
        if (_setTransform is null)
        {
            return -1;
        }

        return _setTransform(entityId, ref transform);
    }

    public static void Log(int level, string message)
    {
        if (_logMessage is null)
        {
            return;
        }

        IntPtr ptr = Marshal.StringToCoTaskMemUTF8(message);
        try
        {
            _logMessage(level, ptr);
        }
        finally
        {
            Marshal.FreeCoTaskMem(ptr);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "CreateScriptInstance")]
    public static int CreateScriptInstance(ulong entityId, IntPtr typeNameUtf8)
    {
        string? typeName = Marshal.PtrToStringUTF8(typeNameUtf8);
        if (string.IsNullOrWhiteSpace(typeName))
        {
            Log(4, "CreateScriptInstance falló: typeName vacío.");
            return -1;
        }

        Type? type = ResolveType(typeName);
        if (type is null || !typeof(GlassBehaviour).IsAssignableFrom(type) || type.IsAbstract)
        {
            Log(4, $"CreateScriptInstance falló: No se pudo resolver la clase GlassBehaviour '{typeName}'.");
            return -1;
        }

        GlassBehaviour instance;
        try
        {
            instance = (GlassBehaviour)Activator.CreateInstance(type)!;
        }
        catch (Exception ex)
        {
            Log(4, $"CreateScriptInstance falló al instanciar '{typeName}': {ex.Message}");
            return -1;
        }

        instance.EntityId = entityId;
        try
        {
            instance.Awake();
        }
        catch (Exception ex)
        {
            // Una excepción en Awake() no puede cruzar [UnmanagedCallersOnly]:
            // CoreCLR la trataría como fail-fast y mataría el proceso. Como en
            // Update y OnDestroy: se loguea y, al fallar, la instancia no llega
            // a registrarse.
            Log(4, $"CreateScriptInstance falló en Awake de '{typeName}': {ex.Message}");
            return -1;
        }

        lock (_sync)
        {
            int handle = _nextHandle++;
            _instances[handle] = instance;
            Log(2, $"Instancia de script C# '{type.Name}' creada con handle {handle} para entidad {entityId}.");
            return handle;
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "UpdateScriptInstance")]
    public static void UpdateScriptInstance(int handle, float deltaTime)
    {
        GlassBehaviour? instance;
        lock (_sync)
        {
            if (!_instances.TryGetValue(handle, out instance))
            {
                return;
            }
        }

        try
        {
            instance.StartIfNeeded();
            instance.Update(deltaTime);
        }
        catch (Exception ex)
        {
            Log(4, $"Error ejecutando Update en script handle {handle}: {ex.Message}");
        }
    }

    private const float FixedStep = 1f / 60f;
    private static float _fixedAccumulator;

    /// Tick completo de scripts en UNA llamada por frame (bloque
    /// «FFI por lotes»): Rust llama aquí en lugar de llamar
    /// UpdateScriptInstance por instancia. Se abre la caché de
    /// transforms del frame, se despachan los pasos fijos
    /// (FixedUpdate, que hasta este bloque nadie llamaba) y los
    /// Update sobre una foto de las instancias, y al salir se
    /// vuelcan los transforms sucios de vuelta a Rust en un solo
    /// cruce. Las instancias creadas a mitad de tick entran en el
    /// siguiente frame.
    [UnmanagedCallersOnly(EntryPoint = "TickAllScripts")]
    public static void TickAllScripts(float deltaTime)
    {
        TransformCache.BeginFrame();

        try
        {
            KeyValuePair<int, GlassBehaviour>[] instances;
            lock (_sync)
            {
                instances = new KeyValuePair<int, GlassBehaviour>[_instances.Count];
                int i = 0;
                foreach (KeyValuePair<int, GlassBehaviour> instance in _instances)
                {
                    instances[i++] = instance;
                }
            }

            _fixedAccumulator += deltaTime;
            int pasos = 0;

            // Los pasos fijos van antes de Update y se limitan a 3
            // para evitar la espiral de retraso.
            while (_fixedAccumulator >= FixedStep && pasos < 3)
            {
                foreach (KeyValuePair<int, GlassBehaviour> instance in instances)
                {
                    try
                    {
                        instance.Value.FixedUpdate();
                    }
                    catch (Exception ex)
                    {
                        Log(4, $"Error ejecutando FixedUpdate en script handle {instance.Key}: {ex.Message}");
                    }
                }

                _fixedAccumulator -= FixedStep;
                pasos++;
            }

            if (pasos == 3 && _fixedAccumulator >= FixedStep)
            {
                _fixedAccumulator %= FixedStep;
            }

            foreach (KeyValuePair<int, GlassBehaviour> instance in instances)
            {
                try
                {
                    instance.Value.StartIfNeeded();
                    instance.Value.Update(deltaTime);
                }
                catch (Exception ex)
                {
                    Log(4, $"Error ejecutando Update en script handle {instance.Key}: {ex.Message}");
                }
            }
        }
        finally
        {
            TransformCache.Flush();
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "DestroyScriptInstance")]
    public static void DestroyScriptInstance(int handle)
    {
        GlassBehaviour? inst;
        lock (_sync)
        {
            if (_instances.Remove(handle, out inst))
            {
                try
                {
                    inst?.OnDestroy();
                }
                catch (Exception ex)
                {
                    Log(4, $"Error en OnDestroy para handle {handle}: {ex.Message}");
                }
                Log(2, $"Instancia de script handle {handle} destruida.");
            }
        }
    }

    // Punto de entrada del clic de botón del Canvas: Rust (glass_scripting)
    // lo resuelve de forma tolerante — un ensamblado sin este método arranca
    // igual y los clics solo quedan en el log de Rust.
    // Semántica: `action` es el nombre (campo «On Click Event» del Button)
    // de un método público sin parámetros del script adjunto a la entidad
    // del PROPIO botón, al estilo del UnityEvent de un Button de Unity.
    // Códigos de retorno: 0 = invocado; -1 = la entidad no tiene instancia
    // de script; -2 = el script no tiene ese método sin parámetros;
    // -3 = acción vacía; -4 = el método lanzó una excepción (ya logueada).
    [UnmanagedCallersOnly(EntryPoint = "InvokeButtonClick")]
    public static int InvokeButtonClick(ulong entityId, IntPtr actionUtf8)
    {
        string? action = Marshal.PtrToStringUTF8(actionUtf8);
        if (string.IsNullOrWhiteSpace(action))
        {
            return -3;
        }

        GlassBehaviour? target = null;
        lock (_sync)
        {
            foreach (var inst in _instances.Values)
            {
                if (inst.EntityId == entityId)
                {
                    target = inst;
                    break;
                }
            }
        }

        if (target is null)
        {
            Log(3, $"[Canvas UI] Clic en la entidad {entityId} (acción '{action}'): la entidad no tiene script; nada que invocar.");
            return -1;
        }

        var method = target.GetType().GetMethod(
            action,
            BindingFlags.Public | BindingFlags.Instance,
            binder: null,
            types: Type.EmptyTypes,
            modifiers: null);
        if (method is null)
        {
            Log(3, $"[Canvas UI] Clic en la entidad {entityId}: el script '{target.GetType().Name}' no tiene un método público sin parámetros llamado '{action}'.");
            return -2;
        }

        try
        {
            method.Invoke(target, null);
            Log(2, $"[Canvas UI] Botón de la entidad {entityId}: método '{action}' invocado en '{target.GetType().Name}'.");
            return 0;
        }
        catch (Exception ex)
        {
            // Como en Update: una excepción de script jamás cruza la frontera
            // [UnmanagedCallersOnly]; se loguea y se devuelve código de error.
            var inner = ex is TargetInvocationException && ex.InnerException is not null ? ex.InnerException : ex;
            Log(4, $"[Canvas UI] Error invocando '{action}' en la entidad {entityId}: {inner.Message}");
            return -4;
        }
    }

    /// <summary>
    /// Entrega un evento de contacto a la instancia de script de entityId
    /// (Fase 1). kind: 0 = Enter, 1 = Stay, 2 = Exit; isTrigger: 0/1.
    /// Sin instancia en la entidad no hace nada (no es un error). Las
    /// excepciones del callback se capturan y se registran: un script roto
    /// no debe tumbar el motor.
    /// </summary>
    /// <summary>
    /// Punto de entrada nativo de los eventos de animación: el motor lo
    /// llama cuando el Animator de una entidad cruza un evento declarado
    /// en su controlador. Busca el behaviour de la entidad y le invoca
    /// OnAnimationEvent. Sin instancia no hace nada. Las excepciones se
    /// capturan y se registran: un script roto no debe tumbar el motor.
    /// </summary>
    /// <summary>
    /// Punto de entrada nativo de los cambios de estado de la FSM: el
    /// motor lo llama cuando el Animator de una entidad entra o sale
    /// de un estado. Busca el behaviour de la entidad y le invoca
    /// OnStateEnter u OnStateExit. Sin instancia no hace nada. Las
    /// excepciones se capturan y se registran.
    /// </summary>
    [UnmanagedCallersOnly(EntryPoint = "DispatchStateEvent")]
    public static void DispatchStateEvent(ulong entityId, IntPtr statePtr, int entered)
    {
        GlassBehaviour? instance = null;
        lock (_sync)
        {
            foreach (var inst in _instances.Values)
            {
                if (inst.EntityId == entityId)
                {
                    instance = inst;
                    break;
                }
            }
        }
        if (instance is null)
        {
            return;
        }
        string state = statePtr == IntPtr.Zero ? string.Empty : Marshal.PtrToStringUTF8(statePtr) ?? string.Empty;
        try
        {
            if (entered != 0) instance.OnStateEnter(state);
            else instance.OnStateExit(state);
        }
        catch (Exception ex)
        {
            Log(4, $"[{instance.GetType().Name}] Excepción en OnStateEnter/OnStateExit: {ex}");
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "DispatchAnimEvent")]
    public static void DispatchAnimEvent(ulong entityId, IntPtr namePtr, float value, int hasValue, IntPtr textPtr)
    {
        GlassBehaviour? instance = null;
        lock (_sync)
        {
            foreach (var inst in _instances.Values)
            {
                if (inst.EntityId == entityId)
                {
                    instance = inst;
                    break;
                }
            }
        }
        if (instance is null)
        {
            return;
        }
        string name = namePtr == IntPtr.Zero ? string.Empty : Marshal.PtrToStringUTF8(namePtr) ?? string.Empty;
        string? text = textPtr == IntPtr.Zero ? null : Marshal.PtrToStringUTF8(textPtr);
        try
        {
            instance.OnAnimationEvent(name, hasValue != 0 ? value : null, text);
        }
        catch (Exception ex)
        {
            Log(4, $"[{instance.GetType().Name}] Excepción en OnAnimationEvent: {ex}");
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "DispatchContactEvent")]
    public static void DispatchContactEvent(ulong entityId, ulong otherEntityId, int kind, int isTrigger, IntPtr otherNamePtr)
    {
        GlassBehaviour? instance = null;
        lock (_sync)
        {
            foreach (var inst in _instances.Values)
            {
                if (inst.EntityId == entityId)
                {
                    instance = inst;
                    break;
                }
            }
        }
        if (instance is null)
        {
            return;
        }
        bool trigger = isTrigger != 0;
        string otherName = otherNamePtr == IntPtr.Zero ? null : Marshal.PtrToStringUTF8(otherNamePtr);
        var collision = new Collision(otherEntityId, trigger, otherName);
        try
        {
            switch (kind)
            {
                case 0:
                    if (trigger) instance.OnTriggerEnter(collision);
                    else instance.OnCollisionEnter(collision);
                    break;
                case 1:
                    if (trigger) instance.OnTriggerStay(collision);
                    else instance.OnCollisionStay(collision);
                    break;
                case 2:
                    if (trigger) instance.OnTriggerExit(collision);
                    else instance.OnCollisionExit(collision);
                    break;
                default:
                    Log(3, $"[RuntimeBridge] DispatchContactEvent: kind fuera de rango ({kind})");
                    break;
            }
        }
        catch (Exception ex)
        {
            Log(4, $"[{instance.GetType().Name}] Excepción en callback de contacto: {ex}");
        }
    }

    private static Type? ResolveType(string typeName)
    {
        lock (_sync)
        {
            if (_typeCache.TryGetValue(typeName, out Type? cached))
            {
                return cached;
            }
        }

        Type? resolved = ResolveTypeUncached(typeName);

        lock (_sync)
        {
            _typeCache[typeName] = resolved;
        }
        return resolved;
    }

    private static Type? ResolveTypeUncached(string typeName)
    {
        Type? type = Type.GetType(typeName);
        if (type is not null)
        {
            return type;
        }

        foreach (var assembly in AppDomain.CurrentDomain.GetAssemblies())
        {
            type = assembly.GetType(typeName);
            if (type is not null)
            {
                return type;
            }
        }

        foreach (var assembly in AppDomain.CurrentDomain.GetAssemblies())
        {
            Type[] types;
            try
            {
                types = assembly.GetTypes();
            }
            catch (ReflectionTypeLoadException ex)
            {
                types = ex.Types.Where(t => t is not null).Cast<Type>().ToArray();
            }

            foreach (var candidate in types)
            {
                if (candidate.FullName == typeName || candidate.Name == typeName)
                {
                    return candidate;
                }
            }
        }

        return null;
    }
}
