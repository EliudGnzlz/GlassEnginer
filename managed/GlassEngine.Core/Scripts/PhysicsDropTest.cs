using System;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;

namespace GlassEngine.Core.Scripts;

public class PhysicsDropTest : GlassBehaviour
{
    private RigidBody _rigidBody = new();
    private float _timer = 0.0f;
    private bool _bounced = false;

    public override void Start()
    {
        Log($"PhysicsDropTest: Inicializando RigidBody Dynamic en entidad {EntityId}");
        _rigidBody.Initialize(EntityId, RigidBodyType.Dynamic, Position, Rotation);
        _rigidBody.AttachBoxCollider(new Vector3(0.5f, 0.5f, 0.5f), restitution: 0.3f, friction: 0.5f);
    }

    public override void Update(float deltaTime)
    {
        _timer += deltaTime;

        // Query physics position
        if (_rigidBody.GetTransform(out var physPos, out var physRot))
        {
            // Sync internal script transform
            Position = physPos;
            Rotation = physRot;

            if (_timer > 0.5f && !_bounced)
            {
                Log($"PhysicsDropTest: Entidad {EntityId} cayendo bajo gravedad, PosY={physPos.Y:F2}. Aplicando impulso de salto!");
                _rigidBody.ApplyImpulse(new Vector3(0.0f, 8.0f, 0.0f));
                _bounced = true;
            }
        }
    }
}
