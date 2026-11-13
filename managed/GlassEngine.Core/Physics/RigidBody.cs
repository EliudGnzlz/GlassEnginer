using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Physics;

public enum RigidBodyType
{
    Dynamic = 0,
    Static = 1,
    Kinematic = 2
}

public enum ForceMode
{
    Force = 0,
    Impulse = 1,
    Acceleration = 2,
    VelocityChange = 3
}

public enum CollisionDetectionMode
{
    Discrete = 0,
    Continuous = 1
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
        _isKinematic = type == RigidBodyType.Kinematic;
        if (_constraints != RigidBodyConstraints.None)
        {
            RuntimeBridge.SetLockedAxes(Handle, (int)_constraints);
        }
    }

    private float _mass = 1.0f;

    public float Mass
    {
        get => RuntimeBridge.GetMass(Handle);
        set
        {
            _mass = value;
            if (IsValid)
            {
                RuntimeBridge.SetMass(Handle, value);
            }
        }
    }

    public float LinearDamping
    {
        get => RuntimeBridge.GetLinearDamping(Handle);
        set
        {
            if (IsValid)
            {
                RuntimeBridge.SetLinearDamping(Handle, value);
            }
        }
    }

    private bool _useGravity = true;

    public bool UseGravity
    {
        get => _useGravity;
        set
        {
            _useGravity = value;
            if (IsValid)
            {
                RuntimeBridge.SetGravityScale(Handle, value ? 1f : 0f);
            }
        }
    }

    public float GravityScale
    {
        get => RuntimeBridge.GetGravityScale(Handle);
        set
        {
            if (IsValid)
            {
                RuntimeBridge.SetGravityScale(Handle, value);
            }
        }
    }

    public CollisionDetectionMode CollisionDetection
    {
        get => RuntimeBridge.GetCcdEnabled(Handle) == 1 ? CollisionDetectionMode.Continuous : CollisionDetectionMode.Discrete;
        set
        {
            if (IsValid)
            {
                RuntimeBridge.SetCcdEnabled(Handle, value == CollisionDetectionMode.Continuous);
            }
        }
    }

    private bool _isKinematic;

    public bool IsKinematic
    {
        get => _isKinematic;
        set
        {
            _isKinematic = value;
            if (IsValid)
            {
                RuntimeBridge.SetBodyType(Handle, value ? 2 : 0);
            }
        }
    }

    public void MovePosition(Vector3 position)
    {
        if (!IsValid) return;
        RuntimeBridge.MovePosition(Handle, position);
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
        ulong result = RuntimeBridge.AttachBoxCollider(Handle, halfExtents, restitution, friction);
        if (_mass > 0f) RuntimeBridge.SetMass(Handle, _mass);
        return result;
    }

    public ulong AttachSphereCollider(float radius, float restitution = 0.0f, float friction = 0.5f)
    {
        if (!IsValid) return 0;
        ulong result = RuntimeBridge.AttachSphereCollider(Handle, radius, restitution, friction);
        if (_mass > 0f) RuntimeBridge.SetMass(Handle, _mass);
        return result;
    }

    /// <summary>Cápsula sobre el eje Y: halfHeight = mitad del tramo recto
    /// (sin casquetes); la altura total es 2·(halfHeight + radius).</summary>
    public ulong AttachCapsuleCollider(float halfHeight, float radius, float restitution = 0.0f, float friction = 0.5f)
    {
        if (!IsValid) return 0;
        ulong result = RuntimeBridge.AttachCapsuleCollider(Handle, halfHeight, radius, restitution, friction);
        if (_mass > 0f) RuntimeBridge.SetMass(Handle, _mass);
        return result;
    }

    /// <summary>Cilindro sobre el eje Y: halfHeight = mitad de la altura total.</summary>
    public ulong AttachCylinderCollider(float halfHeight, float radius, float restitution = 0.0f, float friction = 0.5f)
    {
        if (!IsValid) return 0;
        ulong result = RuntimeBridge.AttachCylinderCollider(Handle, halfHeight, radius, restitution, friction);
        if (_mass > 0f) RuntimeBridge.SetMass(Handle, _mass);
        return result;
    }

    public bool ApplyImpulse(Vector3 impulse, ForceMode mode = ForceMode.Impulse)
    {
        if (!IsValid) return false;
        switch (mode)
        {
            case ForceMode.VelocityChange:
                if (!GetLinearVelocity(out Vector3 currentVelocity)) return false;
                return RuntimeBridge.SetLinearVelocity(Handle, currentVelocity + impulse);
            case ForceMode.Impulse:
            default:
                return RuntimeBridge.ApplyImpulse(Handle, impulse);
        }
    }

    public bool ApplyForce(Vector3 force, ForceMode mode = ForceMode.Force)
    {
        if (!IsValid) return false;
        switch (mode)
        {
            case ForceMode.Acceleration:
                return RuntimeBridge.ApplyForce(Handle, force * Mass);
            case ForceMode.Impulse:
                return RuntimeBridge.ApplyImpulse(Handle, force);
            case ForceMode.VelocityChange:
                if (!GetLinearVelocity(out Vector3 currentVelocity)) return false;
                return RuntimeBridge.SetLinearVelocity(Handle, currentVelocity + force);
            case ForceMode.Force:
            default:
                return RuntimeBridge.ApplyForce(Handle, force);
        }
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
