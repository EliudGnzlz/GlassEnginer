using System;
using System.Runtime.InteropServices;

namespace GlassEngine.Core.Interop;

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int GetTransformDelegate(ulong entityId, out TransformData transform);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SetTransformDelegate(ulong entityId, ref TransformData transform);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate void LogMessageDelegate(int level, IntPtr utf8Message);
