using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using System.Runtime.InteropServices;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core;

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

    private static readonly Dictionary<int, GlassBehaviour> _instances = new();
    private static int _nextHandle = 1;
    private static readonly object _sync = new();

    [UnmanagedCallersOnly(EntryPoint = "InitAudioBridge")]
    public static void InitAudioBridge(
        IntPtr play2dFn,
        IntPtr play3dFn,
        IntPtr setPosFn,
        IntPtr setListenerFn,
        IntPtr stopFn)
    {
        if (play2dFn != IntPtr.Zero)
            _playAudio2D = Marshal.GetDelegateForFunctionPointer<PlayAudio2DDelegate>(play2dFn);
        if (play3dFn != IntPtr.Zero)
            _playAudio3D = Marshal.GetDelegateForFunctionPointer<PlayAudio3DDelegate>(play3dFn);
        if (setPosFn != IntPtr.Zero)
            _setAudioSourcePos = Marshal.GetDelegateForFunctionPointer<SetAudioSourcePositionDelegate>(setPosFn);
        if (setListenerFn != IntPtr.Zero)
            _setListenerTransform = Marshal.GetDelegateForFunctionPointer<SetListenerTransformDelegate>(setListenerFn);
        if (stopFn != IntPtr.Zero)
            _stopAudio = Marshal.GetDelegateForFunctionPointer<StopAudioDelegate>(stopFn);

        Log(2, "GlassEngine.Core AudioBridge inicializado exitosamente desde Rust.");
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
        instance.Awake();

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

    [UnmanagedCallersOnly(EntryPoint = "DestroyScriptInstance")]
    public static void DestroyScriptInstance(int handle)
    {
        lock (_sync)
        {
            if (_instances.Remove(handle))
            {
                Log(2, $"Instancia de script handle {handle} destruida.");
            }
        }
    }

    private static Type? ResolveType(string typeName)
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
