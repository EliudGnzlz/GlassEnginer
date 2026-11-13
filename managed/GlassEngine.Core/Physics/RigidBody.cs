using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Physics;

public enum RigidBodyType
{
    Dynamic = 0,
    Static = 1,
    Kinematic = 2
}

/// <summary>Restricciones por ejes, con los mismos bits que Unity
/// (RigidbodyConstraints). Por defecto None: el cuerpo se mueve y
/// rota libre; el bloqueo es una opción por cuerpo, no global.</summary>
[Flags]
public enum RigidBodyConstraints
{
    None = 0,
    FreezePositionX = 1,
    FreezePositionY = 2,
    FreezePositionZ = 4,
    FreezeRotationX = 8,
    FreezeRotationY = 16,
    FreezeRotationZ = 32,
    FreezePosition = FreezePositionX | FreezePositionY | FreezePositionZ,
    FreezeRotation = FreezeRotationX | FreezeRotationY | FreezeRotationZ,
    FreezeAll = FreezePosition | FreezeRotation
}

public sealed class RigidBody
{
    public ulong EntityId { get; private set; }
    public RigidBodyType BodyType { get; private set; }
    public ulong Handle { get; private set; }

    public bool IsValid => Handle != 0;

    public void Initialize(ulong entityId, RigidBodyType type, Vector3 position, Quaternion rotation)
    {
        EntityId = entityId;
        BodyType = type;
        Handle = RuntimeBridge.CreateRigidBody(entityId, (int)type, position, rotation);
        if (_constraints != RigidBodyConstraints.None)
        {
            RuntimeBridge.SetLockedAxes(Handle, (int)_constraints);
        }
    }

    private RigidBodyConstraints _constraints;

    public RigidBodyConstraints Constraints
    {
        get => _constraints;
        set
        {
            _constraints = value;
            if (IsValid)
            {
                RuntimeBridge.SetLockedAxes(Handle, (int)value);
            }
        }
    }

    public bool FreezeRotation
    {
        get => (Constraints & RigidBodyConstraints.FreezeRotation) == RigidBodyConstraints.FreezeRotation;
        set => Constraints = value
            ? Constraints | RigidBodyConstraints.FreezeRotation
            : Constraints & ~RigidBodyConstraints.FreezeRotation;
    }

    public ulong AttachBoxCollider(Vector3 halfExtents, float restitution = 0.0f, float friction = 0.5f)
    {
        if (!IsValid) return 0;
        return RuntimeBridge.AttachBoxCollider(Handle, halfExtents, restitution, friction);
    }

    public ulong AttachSphereCollider(float radius, float restitution = 0.0f, float friction = 0.5f)
    {
        if (!IsValid) return 0;
        return RuntimeBridge.AttachSphereCollider(Handle, radius, restitution, friction);
    }

    /// <summary>Cápsula sobre el eje Y: halfHeight = mitad del tramo recto
    /// (sin casquetes); la altura total es 2·(halfHeight + radius).</summary>
    public ulong AttachCapsuleCollider(float halfHeight, float radius, float restitution = 0.0f, float friction = 0.5f)
    {
        if (!IsValid) return 0;
        return RuntimeBridge.AttachCapsuleCollider(Handle, halfHeight, radius, restitution, friction);
    }

    /// <summary>Cilindro sobre el eje Y: halfHeight = mitad de la altura total.</summary>
    public ulong AttachCylinderCollider(float halfHeight, float radius, float restitution = 0.0f, float friction = 0.5f)
    {
        if (!IsValid) return 0;
        return RuntimeBridge.AttachCylinderCollider(Handle, halfHeight, radius, restitution, friction);
    }

    public bool ApplyImpulse(Vector3 impulse)
    {
        if (!IsValid) return false;
        return RuntimeBridge.ApplyImpulse(Handle, impulse);
    }

    public bool ApplyForce(Vector3 force)
    {
        if (!IsValid) return false;
        return RuntimeBridge.ApplyForce(Handle, force);
    }

    public bool GetTransform(out Vector3 position, out Quaternion rotation)
    {
        if (!IsValid)
        {
            position = Vector3.Zero;
            rotation = Quaternion.Identity;
            return false;
        }
        return RuntimeBridge.GetPhysicsTransform(Handle, out position, out rotation);
    }

    public bool SetTransform(Vector3 position, Quaternion rotation)
    {
        if (!IsValid) return false;
        return RuntimeBridge.SetPhysicsTransform(Handle, position, rotation);
    }

    public bool GetLinearVelocity(out Vector3 velocity)
    {
        if (!IsValid)
        {
            velocity = Vector3.Zero;
            return false;
        }
        return RuntimeBridge.GetLinearVelocity(Handle, out velocity);
    }

    public bool SetLinearVelocity(Vector3 velocity)
    {
        if (!IsValid) return false;
        return RuntimeBridge.SetLinearVelocity(Handle, velocity);
    }

    public void Destroy()
    {
        if (IsValid)
        {
            RuntimeBridge.RemoveRigidBody(Handle);
            Handle = 0;
        }
    }
}
