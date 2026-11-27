using System;
using System.Collections.Generic;

namespace UnityEngine;

/// <summary>Base mínima de collider de la fachada (C3 la especializa).</summary>
public class Collider : Component
{
}

/// <summary>Punto de contacto tal como lo ve un script de Unity.</summary>
public struct ContactPoint
{
    /// <summary>Punto de contacto en mundo.</summary>
    public Vector3 point;

    /// <summary>Normal del contacto en mundo.</summary>
    public Vector3 normal;

    /// <summary>Distancia entre los colliders (negativa si penetran).</summary>
    public float separation;

    /// <summary>Impulso del solver en este punto (normal × impulso escalar).</summary>
    public Vector3 impulse;

    /// <summary>null en v1.</summary>
    public Collider? thisCollider;

    /// <summary>null en v1.</summary>
    public Collider? otherCollider;
}

/// <summary>
/// Contacto entregado a OnCollision*(Collision). Como en Unity,
/// <see cref="gameObject"/> es la otra entidad del par.
/// </summary>
public class Collision
{
    internal Collision(GameObject gameObject, ContactPoint[] contacts, Vector3 impulse, Vector3 relativeVelocity)
    {
        this.gameObject = gameObject;
        this.contacts = contacts;
        this.impulse = impulse;
        this.relativeVelocity = relativeVelocity;
    }

    /// <summary>La otra entidad del contacto.</summary>
    public GameObject gameObject { get; }

    public Transform transform => gameObject.transform;

    /// <summary>null en v1.</summary>
    public Collider? collider => null;

    /// <summary>null en v1.</summary>
    public Collider? otherCollider => null;

    /// <summary>Puntos de contacto (nunca null).</summary>
    public ContactPoint[] contacts { get; }

    public int contactCount => contacts.Length;

    /// <summary>Impulso total del contacto.</summary>
    public Vector3 impulse { get; }

    public Vector3 relativeVelocity { get; }

    /// <summary>El punto en <paramref name="index"/> (fuera de rango lanza, como en Unity).</summary>
    public ContactPoint GetContact(int index) => contacts[index];

    /// <summary>Sustituye el contenido de <paramref name="contacts"/> y devuelve cuántos hay.</summary>
    public int GetContacts(List<ContactPoint> contacts)
    {
        if (contacts is null)
            throw new ArgumentNullException(nameof(contacts));
        contacts.Clear();
        contacts.AddRange(this.contacts);
        return this.contacts.Length;
    }
}
