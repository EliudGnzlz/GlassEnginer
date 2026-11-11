using UnityEngine;

namespace GlassEngine.Core;

/// <summary>
/// Información de un contacto entre dos entidades (colisión física o
/// trigger), entregada a las callbacks OnCollision*/OnTrigger* de
/// GlassBehaviour. Como en Unity, `gameObject` es la OTRA entidad del par.
/// </summary>
public struct Collision
{
    /// <summary>La otra entidad del contacto, resuelta por su EntityId con
    /// el registro de wrappers (GetOrCreate reutiliza el wrapper vivo).</summary>
    public GameObject gameObject;

    /// <summary>true si el contacto viene de un collider sensor (trigger).</summary>
    public bool isTrigger;

    public Collision(ulong otherEntityId, bool isTrigger, string otherName = null)
    {
        gameObject = GameObject.GetOrCreate(otherEntityId, otherName ?? "Entity");
        this.isTrigger = isTrigger;
    }
}
