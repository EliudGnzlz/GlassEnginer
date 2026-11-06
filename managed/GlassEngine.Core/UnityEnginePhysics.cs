using System;
using CorePhysics = GlassEngine.Core.Physics.Physics;
using InteropVector3 = GlassEngine.Core.Interop.Vector3;

namespace UnityEngine
{
    /// <summary>
    /// Resultado de un raycast en la fachada estilo Unity. `EntityId` es el
    /// identificador de la entidad golpeada (la fachada no tiene un tipo
    /// Collider propio que devolver).
    /// </summary>
    public struct RaycastHit
    {
        public ulong EntityId;
        public Vector3 point;
        public Vector3 normal;
        public float distance;
    }

    /// <summary>
    /// Fachada estilo Unity sobre GlassEngine.Core.Physics.Physics.
    /// </summary>
    public static class Physics
    {
        public static bool Raycast(Vector3 origin, Vector3 direction, out RaycastHit hit, float maxDistance)
        {
            return Raycast(origin, direction, out hit, maxDistance, 0);
        }

        public static bool Raycast(Vector3 origin, Vector3 direction, out RaycastHit hit, float maxDistance, ulong excludeEntityId)
        {
            hit = default;
            var o = new InteropVector3(origin.x, origin.y, origin.z);
            var d = new InteropVector3(direction.x, direction.y, direction.z);
            if (!CorePhysics.Raycast(o, d, out var coreHit, maxDistance, excludeEntityId))
            {
                return false;
            }
            hit = new RaycastHit
            {
                EntityId = coreHit.EntityId,
                point = new Vector3(coreHit.Point.X, coreHit.Point.Y, coreHit.Point.Z),
                normal = new Vector3(coreHit.Normal.X, coreHit.Normal.Y, coreHit.Normal.Z),
                distance = coreHit.Distance,
            };
            return true;
        }
    }
}
