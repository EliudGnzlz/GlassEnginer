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

// Entidad → handle de cuerpo (Fase 9): espejo de
// `glass_physics_body_handle`. Devuelve 0 si no hay cuerpo.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate ulong GetBodyHandleDelegate(ulong entityId);

// Crea un personaje kinemático: espejo de `glass_character_create`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool CharacterCreateDelegate(ulong entityId, float posX, float posY, float posZ, float radius, float height, float skin, float stepOffset, float slopeLimitDeg, float snap);

// Mueve un personaje kinemático: espejo de `glass_character_move`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool CharacterMoveDelegate(ulong entityId, float dx, float dy, float dz, float dt, out float appliedX, out float appliedY, out float appliedZ, out int grounded, out int sliding);

// Teletransporta un personaje kinemático: espejo de `glass_character_teleport`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool CharacterTeleportDelegate(ulong entityId, float posX, float posY, float posZ);

// Destruye un personaje kinemático: espejo de `glass_character_destroy`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool CharacterDestroyDelegate(ulong entityId);

// Consulta un personaje kinemático: espejo de `glass_character_info`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate bool CharacterInfoDelegate(ulong entityId, out float posX, out float posY, out float posZ, out int grounded, out int sliding);

// Masa del cuerpo: espejo de `glass_physics_set_mass`/`glass_physics_get_mass`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SetMassDelegate(ulong handle, float value);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate float GetMassDelegate(ulong handle);

// Amortiguación lineal: espejo de `glass_physics_set_linear_damping`/`glass_physics_get_linear_damping`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SetLinearDampingDelegate(ulong handle, float value);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate float GetLinearDampingDelegate(ulong handle);

// Escala de gravedad: espejo de `glass_physics_set_gravity_scale`/`glass_physics_get_gravity_scale`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SetGravityScaleDelegate(ulong handle, float value);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate float GetGravityScaleDelegate(ulong handle);

// CCD: espejo de `glass_physics_set_ccd_enabled`/`glass_physics_get_ccd_enabled`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SetCcdEnabledDelegate(ulong handle, int enabled);

[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int GetCcdEnabledDelegate(ulong handle);

// Tipo de cuerpo: espejo de `glass_physics_set_body_type`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SetBodyTypeDelegate(ulong handle, int bodyType);

// Teletransporte de posición: espejo de `glass_physics_move_position`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int MovePositionDelegate(ulong handle, float x, float y, float z);

// Impacto del CharacterController en bruto (espejo de CharacterHitFfi de Rust).
[StructLayout(LayoutKind.Sequential)]
public struct CharacterHitFfi
{
    public ulong Entity;
    public float PointX, PointY, PointZ;
    public float NormalX, NormalY, NormalZ;
    public float MoveDirX, MoveDirY, MoveDirZ;
    public float MoveLength;
}

// Último Move del personaje: espejo de `glass_character_last_move`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate uint CharacterLastMoveDelegate(ulong entityId, out uint flags, [In, Out] float[] velocity, [In, Out] CharacterHitFfi[] hits, uint cap);

// Interpolación de render por entidad: espejo de `glass_physics_set_interpolation`.
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate int SetInterpolationDelegate(ulong entityId, int enabled);
