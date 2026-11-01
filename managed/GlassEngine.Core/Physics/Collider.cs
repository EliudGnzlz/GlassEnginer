using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Physics;

public enum ColliderType
{
    Box,
    Sphere
}

public sealed class Collider
{
    public ColliderType Type { get; set; } = ColliderType.Box;
    public Vector3 HalfExtents { get; set; } = new(0.5f, 0.5f, 0.5f);
    public float Radius { get; set; } = 0.5f;
    public float Restitution { get; set; } = 0.0f;
    public float Friction { get; set; } = 0.5f;
    public ulong Handle { get; internal set; } = 0;
}
