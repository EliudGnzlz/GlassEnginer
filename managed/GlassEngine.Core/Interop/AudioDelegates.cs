using System;
using System.Runtime.InteropServices;

namespace GlassEngine.Core.Interop;

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate ulong PlayAudio2DDelegate(IntPtr clipNameUtf8, float volume, float pitch, bool looping);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate ulong PlayAudio3DDelegate(IntPtr clipNameUtf8, float posX, float posY, float posZ, float volume, float pitch, bool looping);

// Con bus (auditoría C6): espejo de glass_audio_play_2d_on_bus / _3d_on_bus.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate ulong PlayAudio2DOnBusDelegate(IntPtr clipNameUtf8, IntPtr busNameUtf8, float volume, float pitch, bool looping);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate ulong PlayAudio3DOnBusDelegate(IntPtr clipNameUtf8, IntPtr busNameUtf8, float posX, float posY, float posZ, float volume, float pitch, bool looping);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate void SetAudioSourcePositionDelegate(ulong handle, float posX, float posY, float posZ);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate void SetListenerTransformDelegate(float posX, float posY, float posZ, float rotX, float rotY, float rotZ, float rotW);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void StopAudioDelegate(ulong handle);
    public delegate void PauseAudioDelegate(ulong handle);
    public delegate void ResumeAudioDelegate(ulong handle);
