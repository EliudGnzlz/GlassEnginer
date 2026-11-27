using System;
using System.Collections;

namespace GlassEngine.Core.Hosting;

/// <summary>Fase de bombeo de corrutinas dentro del frame (D5).</summary>
internal enum Phase
{
    Fixed = 0,
    Update = 1,
    EndOfFrame = 2,
}

/// <summary>
/// Planificador de corrutinas de un host. En C0 es un esqueleto: no
/// ejecuta nada. Los contadores globales por fase ya avanzan para que C1
/// construya la máquina encima sin cambiar firmas.
/// </summary>
internal sealed class CoroutineScheduler
{
    // Pasadas completadas por fase, indexadas por (int)Phase.
    private static readonly long[] s_tickets = new long[3];

    internal CoroutineScheduler(MonoBehaviourHost host)
    {
        Host = host;
    }

    internal MonoBehaviourHost Host { get; }

    /// <summary>Esqueleto C0: no arranca la rutina.</summary>
    internal UnityEngine.Coroutine? Start(IEnumerator routine, string? methodName) => null;

    /// <summary>Esqueleto C0: no hay rutinas que avanzar.</summary>
    internal void Pump(Phase phase)
    {
    }

    /// <summary>Esqueleto C0: no hay rutinas que parar.</summary>
    internal void StopAll()
    {
    }

    /// <summary>Marca el inicio de una pasada de la fase.</summary>
    internal static void AdvancePhase(Phase phase) => s_tickets[(int)phase]++;

    /// <summary>Contador actual de la fase.</summary>
    internal static long Ticket(Phase phase) => s_tickets[(int)phase];

    internal static void ResetGlobal() => Array.Clear(s_tickets, 0, s_tickets.Length);
}
