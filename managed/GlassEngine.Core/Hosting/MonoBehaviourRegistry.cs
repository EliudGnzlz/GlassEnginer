using System;
using System.Collections.Generic;

namespace GlassEngine.Core.Hosting;

/// <summary>
/// Hosts vivos, en orden de alta, con índice por entidad. Las lecturas
/// devuelven fotos (arrays): ResetAll recorre <see cref="All"/> mientras
/// cada Unbind llama a <see cref="Remove"/>, y los OnEnable/OnDisable de
/// <see cref="NotifyActiveChanged"/> pueden crear o destruir hosts.
/// </summary>
internal static class MonoBehaviourRegistry
{
    private static readonly List<MonoBehaviourHost> s_hosts = new();
    private static readonly Dictionary<ulong, List<MonoBehaviourHost>> s_byEntity = new();

    // Foto de s_hosts; se invalida en cada alta o baja.
    private static MonoBehaviourHost[]? s_snapshot;

    /// <summary>Foto de los hosts vivos; se puede recorrer aunque el registro cambie.</summary>
    internal static IReadOnlyList<MonoBehaviourHost> All => s_snapshot ??= s_hosts.ToArray();

    internal static void Add(MonoBehaviourHost host)
    {
        ulong entityId = host.EntityId;
        if (!s_byEntity.TryGetValue(entityId, out List<MonoBehaviourHost>? list))
        {
            list = new List<MonoBehaviourHost>();
            s_byEntity.Add(entityId, list);
        }
        else if (list.Contains(host))
        {
            return;
        }

        list.Add(host);
        s_hosts.Add(host);
        s_snapshot = null;
    }

    internal static void Remove(MonoBehaviourHost host)
    {
        if (!s_hosts.Remove(host))
            return;
        s_snapshot = null;

        ulong entityId = host.EntityId;
        if (s_byEntity.TryGetValue(entityId, out List<MonoBehaviourHost>? list))
        {
            list.Remove(host);
            if (list.Count == 0)
                s_byEntity.Remove(entityId);
        }
    }

    /// <summary>Foto de los hosts de la entidad, en orden de alta (vacía si no hay).</summary>
    internal static IReadOnlyList<MonoBehaviourHost> ForEntity(ulong entityId)
        => s_byEntity.TryGetValue(entityId, out List<MonoBehaviourHost>? list)
            ? list.ToArray()
            : Array.Empty<MonoBehaviourHost>();

    /// <summary>SetActive de la entidad (D13): reenvía a cada host suyo.</summary>
    internal static void NotifyActiveChanged(ulong entityId, bool active)
    {
        IReadOnlyList<MonoBehaviourHost> hosts = ForEntity(entityId);
        for (int i = 0; i < hosts.Count; i++)
            hosts[i].NotifyActiveChanged(active);
    }

    internal static void Clear()
    {
        s_hosts.Clear();
        s_byEntity.Clear();
        s_snapshot = null;
    }
}
