using System;
using System.Collections.Generic;

namespace UnityEngine;

    /// <summary>
    /// Base de collider de la fachada. Las formas concretas (BoxCollider,
    /// SphereCollider, CapsuleCollider, MeshCollider) derivan de ella, de modo
    /// que GetComponent&lt;Collider&gt;() devuelve la primera forma añadida.
    /// </summary>
    public class Collider : Component
    {
        /// <summary>Si es true, el collider solo detecta solapes (trigger).</summary>
        public bool isTrigger { get; set; }
    }

    /// <summary>Collider esférico. Defectos de Unity: centro cero, radio 0.5.</summary>
    public class SphereCollider : Collider
    {
        public Vector3 center { get; set; } = Vector3.zero;
        public float radius { get; set; } = 0.5f;
    }

    /// <summary>
    /// Collider de cápsula. Defectos de Unity: centro cero, radio 0.5, altura 1,
    /// dirección 1 (eje Y). direction: 0 = X, 1 = Y, 2 = Z.
    /// </summary>
    public class CapsuleCollider : Collider
    {
        public Vector3 center { get; set; } = Vector3.zero;
        public float radius { get; set; } = 0.5f;
        public float height { get; set; } = 1f;
        public int direction { get; set; } = 1;
    }

    /// <summary>Collider de malla.</summary>
    public class MeshCollider : Collider
    {
        /// <summary>
        /// Malla compartida. La fachada no tiene tipo Mesh en v1, por eso se
        /// declara como object; se tipará cuando exista UnityEngine.Mesh.
        /// </summary>
        public object? sharedMesh { get; set; }

        /// <summary>Si es true, la malla se trata como envolvente convexa.</summary>
        public bool convex { get; set; }
    }

    /// <summary>
    /// Datos de un choque del CharacterController durante Move, entregados a
    /// OnControllerColliderHit. Lo construye el huésped a partir del hit del motor.
    /// </summary>
    public class ControllerColliderHit
    {
        internal ControllerColliderHit(Vector3 point, Vector3 normal, GameObject gameObject,
                                       Vector3 moveDirection, float moveLength)
        {
            this.point = point;
            this.normal = normal;
            this.gameObject = gameObject;
            this.moveDirection = moveDirection;
            this.moveLength = moveLength;
        }

        /// <summary>Punto de impacto en espacio de mundo.</summary>
        public Vector3 point { get; }

        /// <summary>Normal de la superficie en el punto de impacto.</summary>
        public Vector3 normal { get; }

        /// <summary>GameObject golpeado.</summary>
        public GameObject gameObject { get; }

        /// <summary>Transform del GameObject golpeado.</summary>
        public Transform transform => gameObject.transform;

        /// <summary>Dirección del movimiento que produjo el choque.</summary>
        public Vector3 moveDirection { get; }

        /// <summary>Distancia recorrida por el controlador hasta el choque.</summary>
        public float moveLength { get; }

        /// <summary>Collider golpeado: null en v1.</summary>
        public Collider? collider => null;

        /// <summary>Collider propio: null en v1.</summary>
        public Collider? otherCollider => null;
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
