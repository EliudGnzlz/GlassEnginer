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

    /// <summary>Punto de contacto medio en mundo (cero si no hay datos).</summary>
    public Vector3 point;

    /// <summary>Normal del contacto en mundo (cero si no hay datos).</summary>
    public Vector3 normal;

    /// <summary>Máximo impulso normal del manifold (0 si no hay datos).</summary>
    public float impulse;

    /// <summary>Velocidad lineal relativa del par (cero si no hay datos).</summary>
    public Vector3 relativeVelocity;

    public Collision(
        ulong otherEntityId,
        bool isTrigger,
        string? otherName = null,
        Vector3 point = default,
        Vector3 normal = default,
        float impulse = 0f,
        Vector3 relativeVelocity = default)
    {
        gameObject = GameObject.GetOrCreate(otherEntityId, otherName ?? "Entity");
        this.isTrigger = isTrigger;
        this.point = point;
        this.normal = normal;
        this.impulse = impulse;
        this.relativeVelocity = relativeVelocity;
    }
}
