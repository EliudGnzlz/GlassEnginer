using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts;

public sealed class PlayerMovement : GlassBehaviour
{
    public float Speed = 2.0f;
    private float _timer = 0.0f;

    public override void Awake()
    {
        Log("PlayerMovement Awake iniciado.");
    }

    public override void Start()
    {
        Log($"PlayerMovement iniciado en entidad {EntityId}.");
    }

    public override void Update(float deltaTime)
    {
        _timer += deltaTime;

        // Movimiento oscilatorio suave en el eje X y Z
        Vector3 pos = Position;
        pos.X = MathF.Sin(_timer * Speed) * 2.0f;
        pos.Z = MathF.Cos(_timer * Speed) * 2.0f;
        Position = pos;
    }
}
