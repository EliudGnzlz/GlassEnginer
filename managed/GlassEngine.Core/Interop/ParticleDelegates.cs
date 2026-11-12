using System;
using System.Runtime.InteropServices;

namespace GlassEngine.Core.Interop;

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate void PlayParticlesDelegate(ulong entityId);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate void StopParticlesDelegate(ulong entityId);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate void BurstParticlesDelegate(ulong entityId, uint count);
