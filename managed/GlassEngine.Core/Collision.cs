using System;
using System.Collections.Generic;
using UnityEngine;

namespace GlassEngine.Core;

/// <summary>
/// Punto de contacto individual de una colisión: lo que Unity expone
/// en `Collision.contacts`. `separation` negativa indica penetración
/// (convención Unity/Rapier). El `impulse` por punto puede ser 0 en
/// el Enter: el solver aún no lo ha escrito cuando el evento se
/// recoge; el impulso agregado de `Collision` conserva la estimación.
/// `thisCollider`/`otherCollider` quedan a null por ahora: la fachada
/// no tiene un registro entidad→collider consultable (los RigidBody
/// de script son instancias por script, no registradas); resolverlos
/// es trabajo del puente de componente Collider, no de este tipo.
/// </summary>
public struct ContactPoint
{
    /// <summary>Punto de contacto en mundo.</summary>
    public Vector3 point;

    /// <summary>Normal del manifold de este punto, en mundo.</summary>
    public Vector3 normal;

    /// <summary>Distancia entre los colliders en el punto (negativa si penetran).</summary>
    public float separation;

    /// <summary>Impulso normal del solver en este punto.</summary>
    public float impulse;

    /// <summary>Collider de la entidad que recibe el evento (ver la nota del tipo).</summary>
    public Physics.Collider? thisCollider;

    /// <summary>Collider de la otra entidad (ver la nota del tipo).</summary>
    public Physics.Collider? otherCollider;
}

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

    /// <summary>Puntos de contacto individuales (nunca null: vacío si no hay datos).</summary>
    public ContactPoint[] contacts;

    /// <summary>Número de puntos de contacto.</summary>
    public int contactCount => contacts.Length;

    /// <summary>El punto de contacto en `index` (fuera de rango lanza, como en Unity).</summary>
    public ContactPoint GetContact(int index) => contacts[index];

    /// <summary>Copia los puntos en `buffer` y devuelve cuántos copió (semántica Unity).</summary>
    public int GetContacts(ContactPoint[] buffer)
    {
        int n = Math.Min(buffer.Length, contacts.Length);
        Array.Copy(contacts, buffer, n);
        return n;
    }

    /// <summary>Sustituye el contenido de `buffer` por los puntos y devuelve cuántos hay (semántica Unity).</summary>
    public int GetContacts(List<ContactPoint> buffer)
    {
        buffer.Clear();
        buffer.AddRange(contacts);
        return contacts.Length;
    }

    public Collision(
        ulong otherEntityId,
        bool isTrigger,
        string? otherName = null,
        Vector3 point = default,
        Vector3 normal = default,
        float impulse = 0f,
        Vector3 relativeVelocity = default,
        ContactPoint[]? contacts = null)
    {
        gameObject = GameObject.GetOrCreate(otherEntityId, otherName ?? "Entity");
        this.isTrigger = isTrigger;
        this.point = point;
        this.normal = normal;
        this.impulse = impulse;
        this.relativeVelocity = relativeVelocity;
        this.contacts = contacts ?? Array.Empty<ContactPoint>();
    }
}
