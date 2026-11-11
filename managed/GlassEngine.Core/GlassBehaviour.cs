using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core;

public abstract class GlassBehaviour
{
    public ulong EntityId { get; set; }

    private bool _started;

    public virtual void Awake() { }

    public virtual void Start() { }

    public virtual void Update(float deltaTime) { }

    /// Paso fijo (1/60 s, con acumulador): lo despacha
    /// <c>RuntimeBridge.TickAllScripts</c> antes de Update desde el
    /// bloque «FFI por lotes». Antes de ese bloque la jerarquía que
    /// el motor ticka no tenía este gancho (solo existía en la
    /// fachada <c>MonoBehaviour</c>, que no es la que se ticka).
    public virtual void FixedUpdate() { }

    /// <summary>Un collider no-trigger de esta entidad empieza a tocar otro.</summary>
    public virtual void OnCollisionEnter(Collision collision) { }

    /// <summary>Cada frame mientras dos colliders no-trigger siguen en contacto.</summary>
    public virtual void OnCollisionStay(Collision collision) { }

    /// <summary>Dos colliders no-trigger dejan de tocarse.</summary>
    public virtual void OnCollisionExit(Collision collision) { }

    /// <summary>Otro collider empieza a solapar el trigger de esta entidad.</summary>
    public virtual void OnTriggerEnter(Collision collision) { }

    /// <summary>Cada frame mientras otro collider sigue dentro del trigger.</summary>
    public virtual void OnTriggerStay(Collision collision) { }

    /// <summary>Otro collider deja de solapar el trigger de esta entidad.</summary>
    public virtual void OnTriggerExit(Collision collision) { }

    public virtual void OnDestroy() { }

    internal void StartIfNeeded()
    {
        if (!_started)
        {
            _started = true;
            Start();
        }
    }

    public Vector3 Position
    {
        get
        {
            RuntimeBridge.GetTransform(EntityId, out TransformData transform);
            return transform.Position;
        }
        set
        {
            if (RuntimeBridge.GetTransform(EntityId, out TransformData transform) == 0)
            {
                transform.Position = value;
                RuntimeBridge.SetTransform(EntityId, ref transform);
            }
        }
    }

    public Quaternion Rotation
    {
        get
        {
            RuntimeBridge.GetTransform(EntityId, out TransformData transform);
            return transform.Rotation;
        }
        set
        {
            if (RuntimeBridge.GetTransform(EntityId, out TransformData transform) == 0)
            {
                transform.Rotation = value;
                RuntimeBridge.SetTransform(EntityId, ref transform);
            }
        }
    }

    public Vector3 Scale
    {
        get
        {
            RuntimeBridge.GetTransform(EntityId, out TransformData transform);
            return transform.Scale;
        }
        set
        {
            if (RuntimeBridge.GetTransform(EntityId, out TransformData transform) == 0)
            {
                transform.Scale = value;
                RuntimeBridge.SetTransform(EntityId, ref transform);
            }
        }
    }

    protected void Log(string message)
    {
        RuntimeBridge.Log(2, $"[{GetType().Name}] {message}");
    }
}
