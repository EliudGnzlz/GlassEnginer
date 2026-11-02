using System.Runtime.InteropServices;

namespace GlassEngine.Core.Interop;

[StructLayout(LayoutKind.Sequential)]
public struct InputData
{
    public float MoveX; // -1.0 (A/Left) .. +1.0 (D/Right)
    public float MoveZ; // -1.0 (S/Down) .. +1.0 (W/Up)
    public byte Jump;   // 1 if Space pressed
    public byte Run;    // 1 if Shift pressed
}

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int GetInputDelegate(out InputData input);
