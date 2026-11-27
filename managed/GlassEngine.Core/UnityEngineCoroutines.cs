namespace UnityEngine;

/// <summary>Base de las instrucciones de espera de corrutinas.</summary>
public class YieldInstruction
{
}

/// <summary>Asa de una corrutina en marcha (la crea el planificador en C1).</summary>
public sealed class Coroutine : YieldInstruction
{
    internal Coroutine(object? routine)
    {
        Routine = routine;
    }

    /// <summary>Rutina interna del planificador.</summary>
    internal object? Routine { get; }
}
