using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Physics;

public enum RigidBodyType
{
    Dynamic = 0,
    Static = 1,
    Kinematic = 2
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

    public void Destroy()
    {
        if (IsValid)
        {
            RuntimeBridge.RemoveRigidBody(Handle);
            Handle = 0;
        }
    }
}
