using System;

namespace GlassEngine.Core.Hosting;

/// <summary>
/// Conversión de los contactos del núcleo a los tipos de la fachada
/// UnityEngine que reciben los OnCollision*/OnTrigger* del usuario (D9).
/// </summary>
internal static class FacadeContact
{
    /// <summary>
    /// Collision del núcleo → UnityEngine.Collision. El gameObject se
    /// reutiliza tal cual: en ambos tipos es la OTRA entidad del par.
    /// impulse = normal × impulso escalar, también por punto.
    /// </summary>
    internal static UnityEngine.Collision ToFacade(GlassEngine.Core.Collision collision)
    {
        GlassEngine.Core.ContactPoint[] source = collision.contacts ?? Array.Empty<GlassEngine.Core.ContactPoint>();
        UnityEngine.ContactPoint[] contacts = source.Length == 0
            ? Array.Empty<UnityEngine.ContactPoint>()
            : new UnityEngine.ContactPoint[source.Length];

        for (int i = 0; i < source.Length; i++)
        {
            GlassEngine.Core.ContactPoint c = source[i];
            contacts[i] = new UnityEngine.ContactPoint
            {
                point = c.point,
                normal = c.normal,
                separation = c.separation,
                impulse = c.normal * c.impulse,
                // thisCollider/otherCollider: null en v1.
            };
        }

        return new UnityEngine.Collision(
            collision.gameObject,
            contacts,
            collision.normal * collision.impulse,
            collision.relativeVelocity);
    }

    /// <summary>
    /// Vista mínima del collider de la otra entidad para OnTrigger*(Collider).
    /// No se registra en los componentes del wrapper: solo sirve para
    /// <c>other.gameObject</c> y <c>other.transform</c>.
    /// </summary>
    internal static UnityEngine.Collider ColliderOf(ulong otherEntityId, string? otherName)
    {
        UnityEngine.GameObject go = UnityEngine.GameObject.GetOrCreate(otherEntityId, otherName ?? "Entity");
        return new UnityEngine.Collider { gameObject = go };
    }

    /// <summary>Sin estado en v1: no hay caché que limpiar.</summary>
    internal static void Reset()
    {
    }
}
