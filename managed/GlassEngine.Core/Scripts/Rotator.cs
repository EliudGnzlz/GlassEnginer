using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts;

public sealed class Rotator : GlassBehaviour
{
    /// <summary>Velocidad de rotación en radianes por segundo.</summary>
    public float Speed = 1.5f;

    public override void Awake()
    {
        Log("Rotator Awake iniciado.");
    }

    public override void Start()
    {
        Log($"Rotator Start activado en entidad {EntityId}.");
    }

    public override void Update(float deltaTime)
    {
        Quaternion current = Rotation;

        float angle = Speed * deltaTime;
        float halfAngle = angle * 0.5f;
        Quaternion delta = new Quaternion(0f, MathF.Sin(halfAngle), 0f, MathF.Cos(halfAngle));

        Rotation = current * delta;
    }
}
