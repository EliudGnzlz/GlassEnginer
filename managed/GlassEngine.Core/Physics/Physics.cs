using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Physics;

/// <summary>
/// Resultado de un raycast: entidad golpeada y geometría del contacto.
/// </summary>
public struct RaycastHit
{
    public ulong EntityId;
    public Vector3 Point;
    public Vector3 Normal;
    public float Distance;
}

/// <summary>
/// Consultas al mundo físico. El raycast lo resuelve Rapier en el host
/// (glass_physics_cast_ray) a través del puente de física; la
/// QueryPipeline se actualiza en cada llamada, así que el resultado
/// refleja los cuerpos creados/destruidos por scripts hasta ese momento
/// (la física del frame en curso aún no ha dado su paso cuando los
/// scripts corren: el raycast ve el estado al cierre del frame anterior).
/// </summary>
public static class Physics
{
    /// <summary>
    /// Lanza un rayo y devuelve el primer collider golpeado.
    /// `excludeEntityId` (0 = ninguna) ignora los colliders de esa entidad
    /// (típicamente, el propio lanzador).
    /// </summary>
    public static bool Raycast(Vector3 origin, Vector3 direction, out RaycastHit hit, float maxDistance, ulong excludeEntityId = 0)
    {
        hit = default;
        if (!RuntimeBridge.CastRay(origin, direction, maxDistance, excludeEntityId,
                out ulong entityId, out Vector3 point, out Vector3 normal, out float distance))
        {
            return false;
        }
        hit = new RaycastHit
        {
            EntityId = entityId,
            Point = point,
            Normal = normal,
            Distance = distance,
        };
        return true;
    }

    /// <summary>
    /// Barrido de esfera: como un raycast con grosor, para chequeos de
    /// suelo y de proximidad que un rayo fino fallaría (auditoría C5).
    /// </summary>
    public static bool SphereCast(Vector3 origin, float radius, Vector3 direction, out RaycastHit hit, float maxDistance, ulong excludeEntityId = 0)
    {
        hit = default;
        if (!RuntimeBridge.CastSphere(origin, radius, direction, maxDistance, excludeEntityId,
                out ulong entityId, out Vector3 point, out Vector3 normal, out float distance))
        {
            return false;
        }
        hit = new RaycastHit
        {
            EntityId = entityId,
            Point = point,
            Normal = normal,
            Distance = distance,
        };
        return true;
    }

    /// <summary>
    /// Ids de las entidades cuyos colliders intersectan la esfera
    /// (auditoría C5). Sin duplicados por entidad.
    /// </summary>
    public static ulong[] OverlapSphere(Vector3 center, float radius, ulong excludeEntityId = 0)
    {
        return RuntimeBridge.OverlapSphere(center, radius, excludeEntityId);
    }
}
