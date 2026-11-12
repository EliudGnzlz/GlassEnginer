using System;
using System.Runtime.InteropServices;

namespace GlassEngine.Core.Interop;

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate ulong CreateRigidBodyDelegate(
    ulong entityId,
    int bodyType,
    float posX,
    float posY,
    float posZ,
    float rotX,
    float rotY,
    float rotZ,
    float rotW);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate ulong AttachBoxColliderDelegate(
    ulong bodyHandle,
    float halfX,
    float halfY,
    float halfZ,
    float restitution,
    float friction);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate ulong AttachSphereColliderDelegate(
    ulong bodyHandle,
    float radius,
    float restitution,
    float friction);

// Cápsula (bloque de primitivas y colliders por forma): espejo de
// `glass_physics_attach_capsule_collider`. halfHeight es la mitad del
// tramo recto, sin casquetes (semántica de Rapier).
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate ulong AttachCapsuleColliderDelegate(
    ulong bodyHandle,
    float halfHeight,
    float radius,
    float restitution,
    float friction);

// Cilindro: espejo de `glass_physics_attach_cylinder_collider`.
// halfHeight es la mitad de la altura total.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate ulong AttachCylinderColliderDelegate(
    ulong bodyHandle,
    float halfHeight,
    float radius,
    float restitution,
    float friction);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool ApplyImpulseDelegate(
    ulong bodyHandle,
    float impX,
    float impY,
    float impZ);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool ApplyForceDelegate(
    ulong bodyHandle,
    float forceX,
    float forceY,
    float forceZ);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool GetPhysicsTransformDelegate(
    ulong bodyHandle,
    out Vector3 position,
    out Quaternion rotation);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool SetPhysicsTransformDelegate(
    ulong bodyHandle,
    float posX,
    float posY,
    float posZ,
    float rotX,
    float rotY,
    float rotZ,
    float rotW);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool RemoveRigidBodyDelegate(ulong bodyHandle);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool GetLinearVelocityDelegate(
    ulong bodyHandle,
    out Vector3 velocity);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool SetLinearVelocityDelegate(
    ulong bodyHandle,
    float velX,
    float velY,
    float velZ);

// Raycast (juego Plat3D): espejo exacto de `glass_physics_cast_ray` en
// glass_physics/src/ffi.rs. Devuelve true si hubo impacto.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool CastRayDelegate(
    float originX,
    float originY,
    float originZ,
    float dirX,
    float dirY,
    float dirZ,
    float maxDistance,
    ulong excludeEntityId,
    out ulong hitEntityId,
    out float hitPointX,
    out float hitPointY,
    out float hitPointZ,
    out float hitNormalX,
    out float hitNormalY,
    out float hitNormalZ,
    out float hitDistance);

// Barrido de esfera (auditoría C5): espejo exacto de
// `glass_physics_cast_sphere` en glass_physics/src/ffi.rs.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool CastSphereDelegate(
    float originX,
    float originY,
    float originZ,
    float radius,
    float dirX,
    float dirY,
    float dirZ,
    float maxDistance,
    ulong excludeEntityId,
    out ulong hitEntityId,
    out float hitPointX,
    out float hitPointY,
    out float hitPointZ,
    out float hitNormalX,
    out float hitNormalY,
    out float hitNormalZ,
    out float hitDistance);

// Solape de esfera (auditoría C5): espejo de
// `glass_physics_overlap_sphere`. Escribe hasta `capacity` ids en el
// búfer y devuelve cuántos escribió (-1 si algo va mal).
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int OverlapSphereDelegate(
    float centerX,
    float centerY,
    float centerZ,
    float radius,
    ulong excludeEntityId,
    IntPtr outEntities,
    uint capacity);
