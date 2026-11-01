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
