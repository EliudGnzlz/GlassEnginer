using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts;

/// <summary>
/// Operaciones de Vector3/cuaterniones que no provee el tipo interop del
/// motor (que solo define operadores + y * escalar, y cuaterniones sin
/// constructores Euler). Base: respuesta de Claude (bloque Plat3D),
/// integrada y ampliada con los cuaterniones de yaw/pitch que necesitan
/// el jugador y la cámara.
/// </summary>
public static class Plat3DMath
{
    public static Vector3 Sub(Vector3 a, Vector3 b)
    {
        return new Vector3
        {
            X = a.X - b.X,
            Y = a.Y - b.Y,
            Z = a.Z - b.Z
        };
    }

    public static float LengthSq(Vector3 v)
    {
        return v.X * v.X + v.Y * v.Y + v.Z * v.Z;
    }

    public static float Length(Vector3 v)
    {
        return MathF.Sqrt(LengthSq(v));
    }

    public static float Distance(Vector3 a, Vector3 b)
    {
        return Length(Sub(a, b));
    }

    public static Vector3 Normalize(Vector3 v)
    {
        float len = Length(v);
        if (len < 1e-6f)
        {
            return new Vector3 { X = 0f, Y = 0f, Z = 0f };
        }
        float inv = 1f / len;
        return v * inv;
    }

    public static Vector3 Lerp(Vector3 a, Vector3 b, float t)
    {
        if (t < 0f) t = 0f;
        if (t > 1f) t = 1f;
        Vector3 delta = Sub(b, a);
        return a + (delta * t);
    }

    /// <summary>Cuaternión de guiñada (eje Y) para un yaw en radianes.
    /// Con yaw = Atan2(dir.X, dir.Z), el cuaternión orienta +Z hacia dir.</summary>
    public static Quaternion YawQuaternion(float yawRad)
    {
        float half = yawRad * 0.5f;
        return new Quaternion
        {
            X = 0f,
            Y = MathF.Sin(half),
            Z = 0f,
            W = MathF.Cos(half)
        };
    }

    /// <summary>Cuaternión de cabeceo (eje X) para un pitch en grados
    /// (negativo = mirar hacia abajo, convención de las escenas del motor).</summary>
    public static Quaternion PitchQuaternion(float pitchDeg)
    {
        float half = pitchDeg * (MathF.PI / 180f) * 0.5f;
        return new Quaternion
        {
            X = MathF.Sin(half),
            Y = 0f,
            Z = 0f,
            W = MathF.Cos(half)
        };
    }

    /// <summary>Producto de Hamilton a*b (aplica primero b, luego a).</summary>
    public static Quaternion Multiply(Quaternion a, Quaternion b)
    {
        return new Quaternion
        {
            W = a.W * b.W - a.X * b.X - a.Y * b.Y - a.Z * b.Z,
            X = a.W * b.X + a.X * b.W + a.Y * b.Z - a.Z * b.Y,
            Y = a.W * b.Y - a.X * b.Z + a.Y * b.W + a.Z * b.X,
            Z = a.W * b.Z + a.X * b.Y - a.Y * b.X + a.Z * b.W
        };
    }
}
