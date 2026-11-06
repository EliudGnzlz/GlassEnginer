using System;
using System.Collections.Generic;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core;

/// <summary>
/// Pool de entidades sobre el spawn real del motor (bloque de
/// instanciación), pensado para generadores procedurales que crean y
/// retiran las mismas piezas sin parar: en vez de destruir y volver a
/// crear, las piezas se DESACTIVAN (salen del render y su cuerpo sale
/// del mundo físico) y se reactivan al pedirlas otra vez.
///
/// Cubre los dos sabores que un generador necesita:
/// <list type="bullet">
/// <item><b>Fijo</b> (<c>grow: false</c>): una cantidad cerrada creada
/// al construir el pool; cuando se agota, <see cref="Get(Vector3, Quaternion)"/>
/// devuelve 0 y el llamante decide (esperar, reciclar la más vieja...).</item>
/// <item><b>Creciente</b> (<c>grow: true</c>): si no queda ninguna libre,
/// se spawnea una nueva y el pool crece; el total solo baja con
/// <see cref="DestroyAll"/>.</item>
/// </list>
///
/// Todas las piezas de un pool comparten modelo, física, escala y color
/// (un pool por tipo de pieza, como los ObjectPool de Unity). Los ids
/// que entrega son ids de entidad del motor: valen para
/// <see cref="RuntimeBridge.SetTransform"/>, la física FFI y
/// <see cref="RuntimeBridge.DestroyEntity"/>. El pool no es seguro
/// entre hilos: como todo el scripting del motor, se usa desde el hilo
/// de los scripts.
/// </summary>
public sealed class EntityPool
{
    private readonly string _model;
    private readonly SpawnPhysicsKind _physics;
    private readonly Vector3 _scale;
    private readonly Vector3? _color;
    private readonly string _namePrefix;
    private readonly bool _grow;

    private readonly Stack<ulong> _inactive = new();
    private readonly HashSet<ulong> _active = new();
    private readonly HashSet<ulong> _all = new();

    /// <summary>
    /// Crea el pool y precalienta <paramref name="prewarm"/> entidades
    /// ya inactivas (sin cuerpo ni coste de render, listas para el
    /// primer Get). Si el motor rechaza algún spawn del precalentado
    /// (ver el log), el pool nace con menos piezas: los contadores
    /// dicen siempre la verdad.
    /// </summary>
    public EntityPool(
        string model,
        int prewarm,
        bool grow,
        SpawnPhysicsKind physics = SpawnPhysicsKind.None,
        Vector3? scale = null,
        Vector3? color = null,
        string namePrefix = "Pooled")
    {
        _model = model ?? throw new ArgumentNullException(nameof(model));
        if (prewarm < 0) throw new ArgumentOutOfRangeException(nameof(prewarm));
        _grow = grow;
        _physics = physics;
        _scale = scale ?? new Vector3(1f, 1f, 1f);
        _color = color;
        _namePrefix = namePrefix;
        for (int i = 0; i < prewarm; i++)
        {
            ulong id = SpawnOne();
            if (id == 0) break;
            // Inactiva desde el nacimiento: nunca llega a dibujarse ni
            // a tener cuerpo entre el spawn y esta llamada (ambas son
            // síncronas dentro del mismo Update del llamante).
            RuntimeBridge.SetEntityActive(id, false);
            _inactive.Push(id);
        }
    }

    /// <summary>Piezas que el pool posee en total (activas + libres).</summary>
    public int TotalCount => _all.Count;

    /// <summary>Piezas prestadas ahora mismo.</summary>
    public int ActiveCount => _active.Count;

    /// <summary>Piezas libres, listas para un Get inmediato.</summary>
    public int AvailableCount => _inactive.Count;

    /// <summary>True si el pool crece bajo demanda cuando se agota.</summary>
    public bool CanGrow => _grow;

    private ulong SpawnOne()
    {
        ulong id = RuntimeBridge.SpawnEntity(
            $"{_namePrefix}_{_all.Count}", _model, null, 0,
            Vector3.Zero, Quaternion.Identity, _scale, _physics, _color);
        if (id != 0)
        {
            _all.Add(id);
        }
        return id;
    }

    /// <summary>
    /// Presta una pieza y la deja ACTIVA en la posición y rotación
    /// dadas: primero se escribe su transformada (sin cuerpo: está
    /// inactiva) y después se activa, de modo que el cuerpo nace ya en
    /// su sitio y en su forma. Devuelve el id de la entidad, o 0 si el
    /// pool es fijo y está agotado (o si el motor rechazó el spawn al
    /// crecer).
    ///
    /// Las piezas que el pool hace CRECER en este Get pasan por el
    /// mismo camino que las reutilizadas: el spawn las crea activas y
    /// con el cuerpo en el origen, así que se desactivan en el acto y
    /// el cuerpo definitivo nace al activarlas ya colocadas. Sin esa
    /// desactivación, la sincronización física teletransportaría la
    /// pieza al origen en el primer paso (lo cazó la revisión de Kimi
    /// y lo fija la sonda dinámica de RegenRoomTest). Si una pieza
    /// prestable ya no existe en el motor (destruida por fuera del
    /// pool), se olvida y se prueba con la siguiente.
    /// </summary>
    public ulong Get(Vector3 position, Quaternion rotation)
    {
        for (int attempt = 0; attempt < 4; attempt++)
        {
            ulong id;
            bool grown = false;
            if (_inactive.Count > 0)
            {
                id = _inactive.Pop();
            }
            else if (_grow)
            {
                id = SpawnOne();
                if (id == 0) return 0;
                grown = true;
                RuntimeBridge.SetEntityActive(id, false);
            }
            else
            {
                return 0;
            }
            var transform = TransformData.Default;
            transform.Position = position;
            transform.Rotation = rotation;
            transform.Scale = _scale;
            RuntimeBridge.SetTransform(id, ref transform);
            if (RuntimeBridge.SetEntityActive(id, true))
            {
                _active.Add(id);
                return id;
            }
            // La pieza ya no existe en el motor: olvidarla y reintentar.
            _all.Remove(id);
            if (grown) return 0;
        }
        return 0;
    }

    /// <summary>Presta una pieza sin rotación (identidad).</summary>
    public ulong Get(Vector3 position) => Get(position, Quaternion.Identity);

    /// <summary>
    /// Devuelve una pieza prestada: se desactiva (fuera del render y
    /// sin cuerpo) y queda libre para el próximo Get. Devuelve false si
    /// el id no es una pieza activa de este pool, o si la pieza ya no
    /// existe en el motor (destruida por fuera): en ese caso el pool la
    /// olvida en vez de guardar un id muerto como si estuviera libre.
    /// </summary>
    public bool Release(ulong entityId)
    {
        if (!_active.Remove(entityId))
        {
            return false;
        }
        if (!RuntimeBridge.SetEntityActive(entityId, false))
        {
            _all.Remove(entityId);
            return false;
        }
        _inactive.Push(entityId);
        return true;
    }

    /// <summary>Devuelve todas las piezas prestadas.</summary>
    public void ReleaseAll()
    {
        foreach (ulong id in new List<ulong>(_active))
        {
            Release(id);
        }
    }

    /// <summary>
    /// Destruye TODAS las piezas en el motor (activas y libres): la
    /// escena y el mundo físico vuelven a como estaban antes del pool.
    /// Es el final de ciclo de un generador que descarta una sala
    /// entera en vez de reciclarla. El pool queda vacío y reutilizable
    /// (con grow, el próximo Get vuelve a spawnear).
    /// </summary>
    public void DestroyAll()
    {
        foreach (ulong id in _all)
        {
            RuntimeBridge.DestroyEntity(id);
        }
        _all.Clear();
        _active.Clear();
        _inactive.Clear();
    }
}
