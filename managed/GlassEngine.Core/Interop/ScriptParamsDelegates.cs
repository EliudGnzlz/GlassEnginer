using System.Runtime.InteropServices;

namespace GlassEngine.Core.Interop;

[StructLayout(LayoutKind.Sequential)]
public struct ScriptParamsData
{
    public float MoveSpeed;
    public float JumpVelocity;
    public byte AutoDemo;
}

public unsafe delegate int GetScriptParamsDelegate(ScriptParamsData* outParams);
