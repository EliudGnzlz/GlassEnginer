using System.Runtime.InteropServices;

namespace GlassEngine.Core.Interop;

[StructLayout(LayoutKind.Sequential)]
public struct Vector3
{
    public float X;
    public float Y;
    public float Z;

    public Vector3(float x, float y, float z)
    {
        X = x;
        Y = y;
        Z = z;
    }

    public static Vector3 Zero => new(0f, 0f, 0f);
    public static Vector3 UnitY => new(0f, 1f, 0f);

    public static Vector3 operator +(Vector3 a, Vector3 b) =>
        new(a.X + b.X, a.Y + b.Y, a.Z + b.Z);

    public static Vector3 operator *(Vector3 v, float s) =>
        new(v.X * s, v.Y * s, v.Z * s);

    public override string ToString() => $"({X:F2}, {Y:F2}, {Z:F2})";
}

[StructLayout(LayoutKind.Sequential)]
public struct Quaternion
{
    public float X;
    public float Y;
    public float Z;
    public float W;

    public Quaternion(float x, float y, float z, float w)
    {
        X = x;
        Y = y;
        Z = z;
        W = w;
    }

    public static Quaternion Identity => new(0f, 0f, 0f, 1f);

    public static Quaternion operator *(Quaternion a, Quaternion b) =>
        new(
            a.W * b.X + a.X * b.W + a.Y * b.Z - a.Z * b.Y,
            a.W * b.Y - a.X * b.Z + a.Y * b.W + a.Z * b.X,
            a.W * b.Z + a.X * b.Y - a.Y * b.X + a.Z * b.W,
            a.W * b.W - a.X * b.X - a.Y * b.Y - a.Z * b.Z);

    public override string ToString() => $"({X:F2}, {Y:F2}, {Z:F2}, {W:F2})";
}

[StructLayout(LayoutKind.Sequential)]
public struct TransformData
{
    public Vector3 Position;
    public Quaternion Rotation;
    public Vector3 Scale;

    public static TransformData Default => new()
    {
        Position = Vector3.Zero,
        Rotation = Quaternion.Identity,
        Scale = new Vector3(1f, 1f, 1f)
    };
}

/// <summary>
/// Especificación de spawn de una entidad: el espejo gestionado de
/// <c>SpawnSpecFfi</c> en Rust (glass_scripting::bindings). El layout
/// secuencial ES el contrato: punteros primero, el u64 del padre, los
/// vectores, el i32 de física y el byte de color; cualquier cambio de
/// orden o de tipo rompe la frontera en silencio, y en Rust hay un test
/// de offsets que fija el otro lado. Las cadenas viajan como punteros
/// UTF-8 con NUL (IntPtr.Zero = ausente) y son propiedad del llamante:
/// el motor las copia durante la llamada.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public struct SpawnSpecData
{
    public IntPtr Name;
    public IntPtr Model;
    public IntPtr Script;
    public ulong ParentId;
    public Vector3 Position;
    public Quaternion Rotation;
    public Vector3 Scale;
    public Vector3 Color;
    /// <summary>Ver <c>SpawnPhysicsKind</c>: 0 sin física, 1 estático, 2 dinámico, 3 cinemático.</summary>
    public int Physics;
    /// <summary>1 si Color debe aplicarse; 0 deja el color por defecto de la malla.</summary>
    public byte HasColor;
}
