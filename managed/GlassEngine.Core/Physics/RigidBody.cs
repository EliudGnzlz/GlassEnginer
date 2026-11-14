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

    private bool _interpolate;

    /// <summary>Interpolación de render: el transform visible se
    /// mezcla entre el paso físico previo y el actual. La clave en
    /// el motor es la ENTIDAD, no el handle de cuerpo.</summary>
    public bool Interpolate
    {
        get => _interpolate;
        set
        {
            _interpolate = value;
            if (EntityId != 0)
            {
                RuntimeBridge.SetInterpolation(EntityId, value);
            }
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

    public bool GetAngularVelocity(out Vector3 velocity)
    {
        if (!IsValid)
        {
            velocity = Vector3.Zero;
            return false;
        }
        return RuntimeBridge.GetAngularVelocity(Handle, out velocity);
    }

    public Vector3 AngularVelocity
    {
        get
        {
            GetAngularVelocity(out Vector3 velocity);
            return velocity;
        }
        set
        {
            if (IsValid) RuntimeBridge.SetAngularVelocity(Handle, value);
        }
    }

    public bool AddForceAtPosition(Vector3 force, Vector3 point, ForceMode mode = ForceMode.Force)
    {
        if (!IsValid) return false;
        switch (mode)
        {
            case ForceMode.Impulse:
                return RuntimeBridge.ApplyImpulseAtPoint(Handle, force, point);
            case ForceMode.Acceleration:
                return RuntimeBridge.AddForceAtPoint(Handle, force * Mass, point);
            case ForceMode.VelocityChange:
                return RuntimeBridge.ApplyImpulseAtPoint(Handle, force * Mass, point);
            case ForceMode.Force:
            default:
                return RuntimeBridge.AddForceAtPoint(Handle, force, point);
        }
    }

    /// <summary>Acceleration/VelocityChange escalan por masa igual que
    /// `ApplyForce`, no por el tensor de inercia: el FFI `add_torque` /
    /// `apply_torque_impulse` ya aplica la inercia internamente.</summary>
    public bool AddTorque(Vector3 torque, ForceMode mode = ForceMode.Force)
    {
        if (!IsValid) return false;
        switch (mode)
        {
            case ForceMode.Impulse:
                return RuntimeBridge.ApplyTorqueImpulse(Handle, torque);
            case ForceMode.Acceleration:
                return RuntimeBridge.AddTorque(Handle, torque * Mass);
            case ForceMode.VelocityChange:
                return RuntimeBridge.ApplyTorqueImpulse(Handle, torque * Mass);
            case ForceMode.Force:
            default:
                return RuntimeBridge.AddTorque(Handle, torque);
        }
    }

    /// <summary>Convierte el torque del espacio local al mundo con la
    /// rotación actual del cuerpo antes de llamar al puente. Igual que
    /// `AddTorque`, Acceleration/VelocityChange escalan por masa, no por
    /// el tensor de inercia.</summary>
    public bool AddRelativeTorque(Vector3 torque, ForceMode mode = ForceMode.Force)
    {
        if (!IsValid) return false;
        if (!GetTransform(out _, out Quaternion rotation)) return false;
        Vector3 worldTorque = RotateVector(rotation, torque);
        return AddTorque(worldTorque, mode);
    }

    public bool AddExplosionForce(float force, Vector3 position, float radius, float upwardsModifier = 0f, ForceMode mode = ForceMode.Force)
    {
        if (!IsValid) return false;
        switch (mode)
        {
            case ForceMode.Impulse:
                return RuntimeBridge.ExplosionImpulse(Handle, position, radius, force, upwardsModifier);
            case ForceMode.Acceleration:
                return RuntimeBridge.ExplosionForce(Handle, position, radius, force * Mass, upwardsModifier);
            case ForceMode.VelocityChange:
                return RuntimeBridge.ExplosionImpulse(Handle, position, radius, force * Mass, upwardsModifier);
            case ForceMode.Force:
            default:
                return RuntimeBridge.ExplosionForce(Handle, position, radius, force, upwardsModifier);
        }
    }

    private bool _detectCollisions = true;

    /// <summary>No hay getter en Rust para esta propiedad: el get
    /// devuelve el campo local, el set empuja al motor si IsValid.</summary>
    public bool DetectCollisions
    {
        get => _detectCollisions;
        set
        {
            _detectCollisions = value;
            if (IsValid) RuntimeBridge.SetDetectCollisions(Handle, value);
        }
    }

    private float _maxLinearVelocity = float.PositiveInfinity;
    private float _maxAngularVelocity = float.PositiveInfinity;

    /// <summary>&lt;= 0 o no finito = sin tope (infinito), igual que el FFI.</summary>
    public float MaxLinearVelocity
    {
        get => _maxLinearVelocity;
        set
        {
            _maxLinearVelocity = value;
            if (IsValid) RuntimeBridge.SetVelocityCaps(Handle, _maxLinearVelocity, _maxAngularVelocity);
        }
    }

    /// <summary>&lt;= 0 o no finito = sin tope (infinito), igual que el FFI.</summary>
    public float MaxAngularVelocity
    {
        get => _maxAngularVelocity;
        set
        {
            _maxAngularVelocity = value;
            if (IsValid) RuntimeBridge.SetVelocityCaps(Handle, _maxLinearVelocity, _maxAngularVelocity);
        }
    }

    public Vector3 CenterOfMass
    {
        get
        {
            RuntimeBridge.GetCenterOfMass(Handle, out Vector3 center);
            return center;
        }
        set
        {
            if (IsValid) RuntimeBridge.SetCenterOfMass(Handle, value);
        }
    }

    public Vector3 WorldCenterOfMass
    {
        get
        {
            RuntimeBridge.GetWorldCenterOfMass(Handle, out Vector3 center);
            return center;
        }
    }

    public bool ResetCenterOfMass()
    {
        if (!IsValid) return false;
        return RuntimeBridge.ResetCenterOfMass(Handle);
    }

    public Vector3 InertiaTensor
    {
        get
        {
            RuntimeBridge.GetInertiaTensor(Handle, out Vector3 tensor, out _);
            return tensor;
        }
        set
        {
            if (!IsValid) return;
            RuntimeBridge.GetInertiaTensor(Handle, out _, out Quaternion rotation);
            RuntimeBridge.SetInertiaTensor(Handle, value, rotation);
        }
    }

    public Quaternion InertiaTensorRotation
    {
        get
        {
            RuntimeBridge.GetInertiaTensor(Handle, out _, out Quaternion rotation);
            return rotation;
        }
        set
        {
            if (!IsValid) return;
            RuntimeBridge.GetInertiaTensor(Handle, out Vector3 tensor, out _);
            RuntimeBridge.SetInertiaTensor(Handle, tensor, value);
        }
    }

    public bool ResetInertiaTensor()
    {
        if (!IsValid) return false;
        return RuntimeBridge.ResetInertiaTensor(Handle);
    }

    public bool GetPointVelocity(Vector3 point, out Vector3 velocity)
    {
        if (!IsValid)
        {
            velocity = Vector3.Zero;
            return false;
        }
        return RuntimeBridge.GetPointVelocity(Handle, point, out velocity);
    }

    public bool GetRelativePointVelocity(Vector3 point, out Vector3 velocity)
    {
        if (!IsValid)
        {
            velocity = Vector3.Zero;
            return false;
        }
        return RuntimeBridge.GetRelativePointVelocity(Handle, point, out velocity);
    }

    /// <summary>Solo tiene efecto en cuerpos cinemáticos; el motor
    /// devuelve false en otro caso.</summary>
    public bool MoveRotation(Quaternion rotation)
    {
        if (!IsValid) return false;
        return RuntimeBridge.MoveKinematicRotation(Handle, rotation);
    }

    /// <summary>Solo tiene efecto en cuerpos cinemáticos; el motor
    /// devuelve false en otro caso.</summary>
    public bool Move(Vector3 position, Quaternion rotation)
    {
        if (!IsValid) return false;
        return RuntimeBridge.MoveKinematic(Handle, position, rotation);
    }

    public bool SweepTest(Vector3 direction, float maxDistance, out RaycastHit hit)
    {
        hit = default;
        if (!IsValid) return false;
        return RuntimeBridge.SweepBody(Handle, direction, maxDistance, out hit);
    }

    public RaycastHit[] SweepTestAll(Vector3 direction, float maxDistance, int maxHits = 16)
    {
        if (!IsValid) return Array.Empty<RaycastHit>();
        int cap = Math.Clamp(maxHits, 1, 64);
        RaycastHit[] buffer = new RaycastHit[cap];
        uint count = RuntimeBridge.SweepBodyAll(Handle, direction, maxDistance, buffer);
        if (count == 0) return Array.Empty<RaycastHit>();
        if (count == buffer.Length) return buffer;
        RaycastHit[] result = new RaycastHit[count];
        Array.Copy(buffer, result, (int)count);
        return result;
    }

    public bool Sleep()
    {
        if (!IsValid) return false;
        return RuntimeBridge.Sleep(Handle);
    }

    public bool WakeUp()
    {
        if (!IsValid) return false;
        return RuntimeBridge.Wake(Handle);
    }

    public bool IsSleeping()
    {
        if (!IsValid) return false;
        return RuntimeBridge.IsSleeping(Handle);
    }

    private float _sleepThreshold = 0.005f;

    public float SleepThreshold
    {
        get => _sleepThreshold;
        set
        {
            _sleepThreshold = value;
            if (IsValid) RuntimeBridge.SetSleepThreshold(Handle, value);
        }
    }

    /// <summary>Rota un vector por un cuaternión con la fórmula estándar
    /// q·v: Quaternion de Interop no tiene rotación de vectores.
    /// t = 2·(qv × v); v' = v + w·t + qv × t.</summary>
    private static Vector3 RotateVector(Quaternion q, Vector3 v)
    {
        float tx = 2f * (q.Y * v.Z - q.Z * v.Y);
        float ty = 2f * (q.Z * v.X - q.X * v.Z);
        float tz = 2f * (q.X * v.Y - q.Y * v.X);
        return new Vector3(
            v.X + q.W * tx + (q.Y * tz - q.Z * ty),
            v.Y + q.W * ty + (q.Z * tx - q.X * tz),
            v.Z + q.W * tz + (q.X * ty - q.Y * tx));
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
