using System;
using System.Collections;
using System.Collections.Generic;

namespace GlassEngine.Core.Hosting;

/// <summary>Fase de bombeo de corrutinas dentro del frame (D5).</summary>
internal enum Phase
{
    Fixed = 0,
    Update = 1,
    EndOfFrame = 2,
}

/// <summary>Estado de vida de una corrutina (§4).</summary>
internal enum CoroutineStatus
{
    Running,
    Waiting,
    Done,
    Faulted,
    Stopped,
}

/// <summary>
/// Una corrutina (§4): pila de enumeradores (Root abajo, la anidada más
/// interna arriba) y la espera en curso.
/// </summary>
internal sealed class CoroutineRoutine
{
    internal CoroutineRoutine(MonoBehaviourHost owner, IEnumerator root, string? methodName)
    {
        Owner = owner;
        Root = root;
        MethodName = methodName;
        Frames.Push(root);
        Handle = new UnityEngine.Coroutine(this);
    }

    internal Stack<IEnumerator> Frames { get; } = new();

    /// <summary>Identidad para StopCoroutine(IEnumerator).</summary>
    internal IEnumerator Root { get; }

    /// <summary>Solo tiene valor si se arrancó por nombre.</summary>
    internal string? MethodName { get; }

    internal MonoBehaviourHost Owner { get; }

    internal CoroutineStatus Status { get; set; } = CoroutineStatus.Running;

    // Espera en curso; solo tiene sentido con Status == Waiting.
    internal Phase WaitPhase { get; set; }

    internal long WaitTicket { get; set; }

    /// <summary>Plazo en Time.time (WaitForSeconds); null si no hay.</summary>
    internal double? WaitDeadline { get; set; }

    /// <summary>Hija esperada (yield return Coroutine); null si no hay.</summary>
    internal CoroutineRoutine? WaitChild { get; set; }

    /// <summary>Número de bomba en que terminó; -1 mientras vive.</summary>
    internal long FinishSeq { get; set; } = -1;

    /// <summary>Objeto que se devuelve al usuario.</summary>
    internal UnityEngine.Coroutine Handle { get; }

    internal bool IsFinished =>
        Status is CoroutineStatus.Done or CoroutineStatus.Faulted or CoroutineStatus.Stopped;
}

/// <summary>
/// Planificador de corrutinas de un host (§4). Arranca cada rutina de forma
/// síncrona hasta su primer yield y la reanuda en la bomba de la fase que
/// corresponda según la tabla de reanudación. Ninguna excepción del código
/// de usuario sale de aquí: se loguea y muere solo la rutina afectada.
/// </summary>
internal sealed class CoroutineScheduler
{
    // Pasadas completadas por fase, indexadas por (int)Phase.
    private static readonly long[] s_tickets = new long[3];

    // Número de bomba global: avanza en cada Pump de cualquier host. Una
    // hija que termina en la bomba N despierta a su padre (de este o de
    // otro host) a partir de la bomba N + 1.
    private static long s_pumpSeq;

    // Avisos únicos: tipos de yield desconocidos y excepciones ya logueadas.
    private static readonly HashSet<Type> s_tiposAvisados = new();
    private static readonly HashSet<string> s_excepcionesVistas = new(StringComparer.Ordinal);

    private readonly List<CoroutineRoutine> _routines = new();

    // Copia reutilizable para recorrer mientras el código de usuario
    // arranca o para rutinas.
    private readonly List<CoroutineRoutine> _snapshot = new();

    internal CoroutineScheduler(MonoBehaviourHost host)
    {
        Host = host;
    }

    internal MonoBehaviourHost Host { get; }

    /// <summary>
    /// Crea la rutina y la ejecuta de forma síncrona hasta el primer yield.
    /// Si termina o falla en ese tramo, el handle se devuelve ya terminado.
    /// </summary>
    internal UnityEngine.Coroutine? Start(IEnumerator routine, string? methodName)
    {
        if (routine is null)
        {
            UnityEngine.Debug.LogError("StartCoroutine: routine is null");
            return null;
        }

        var c = new CoroutineRoutine(Host, routine, methodName);
        _routines.Add(c);
        Step(c);
        return c.Handle;
    }

    /// <summary>Reanuda las rutinas cuya espera se cumple en esta bomba.</summary>
    internal void Pump(Phase phase)
    {
        long seq = ++s_pumpSeq;
        if (_routines.Count == 0)
            return;

        _snapshot.AddRange(_routines);
        try
        {
            foreach (var c in _snapshot)
            {
                // Las paradas o terminadas durante esta misma bomba se saltan.
                if (c.Status != CoroutineStatus.Waiting || !ShouldResume(c, phase, seq))
                    continue;

                c.Status = CoroutineStatus.Running;
                c.WaitChild = null;
                c.WaitDeadline = null;
                Step(c);
            }
        }
        finally
        {
            _snapshot.Clear();
        }
    }

    /// <summary>StopCoroutine(Coroutine): solo si la rutina es de este host.</summary>
    internal void Stop(UnityEngine.Coroutine handle)
    {
        var c = handle.Routine;
        if (!ReferenceEquals(c.Owner, Host) || c.IsFinished)
            return;

        MarkFinished(c, CoroutineStatus.Stopped);
        _routines.Remove(c);
    }

    /// <summary>StopCoroutine(IEnumerator): por identidad de Root.</summary>
    internal void StopByRoot(IEnumerator root)
    {
        for (int i = _routines.Count - 1; i >= 0; i--)
        {
            var c = _routines[i];
            if (!ReferenceEquals(c.Root, root))
                continue;

            MarkFinished(c, CoroutineStatus.Stopped);
            _routines.RemoveAt(i);
        }
    }

    /// <summary>StopCoroutine(string): solo las arrancadas con ese nombre, como en Unity.</summary>
    internal void StopByName(string methodName)
    {
        for (int i = _routines.Count - 1; i >= 0; i--)
        {
            var c = _routines[i];
            if (!string.Equals(c.MethodName, methodName, StringComparison.Ordinal))
                continue;

            MarkFinished(c, CoroutineStatus.Stopped);
            _routines.RemoveAt(i);
        }
    }

    /// <summary>StopAllCoroutines, OnDestroy y SetActive(false).</summary>
    internal void StopAll()
    {
        if (_routines.Count == 0)
            return;

        foreach (var c in _routines)
            MarkFinished(c, CoroutineStatus.Stopped);
        _routines.Clear();
    }

    /// <summary>Marca el inicio de una pasada de la fase.</summary>
    internal static void AdvancePhase(Phase phase) => s_tickets[(int)phase]++;

    /// <summary>Contador actual de la fase.</summary>
    internal static long Ticket(Phase phase) => s_tickets[(int)phase];

    internal static void ResetGlobal()
    {
        Array.Clear(s_tickets, 0, s_tickets.Length);
        s_pumpSeq = 0;
        s_tiposAvisados.Clear();
        s_excepcionesVistas.Clear();
    }

    // ------------------------------------------
    // Máquina de estados
    // ------------------------------------------

    /// <summary>
    /// Avanza la rutina hasta su siguiente espera o hasta que termine. Las
    /// anidadas IEnumerator se apilan y siguen en el mismo paso; al agotarse
    /// una, el padre continúa también en el mismo paso.
    /// </summary>
    private void Step(CoroutineRoutine c)
    {
        while (true)
        {
            var top = c.Frames.Peek();
            bool ok;
            object? current = null;
            try
            {
                ok = top.MoveNext();
                if (ok)
                    current = top.Current;
            }
            catch (Exception ex)
            {
                Fail(c, ex);
                return;
            }

            // Se paró a sí misma (o la paró otra) dentro de MoveNext.
            if (c.Status == CoroutineStatus.Stopped)
                return;

            if (!ok)
            {
                c.Frames.Pop();
                if (c.Frames.Count == 0)
                {
                    MarkFinished(c, CoroutineStatus.Done);
                    _routines.Remove(c);
                    return;
                }
                continue;
            }

            switch (current)
            {
                case null:
                    Wait(c, Phase.Update, null);
                    return;
                case UnityEngine.WaitForSeconds w:
                    // El plazo se calcula al ceder, no al construir.
                    Wait(c, Phase.Update, UnityEngine.Time.time + w.Seconds);
                    return;
                case UnityEngine.WaitForFixedUpdate:
                    Wait(c, Phase.Fixed, null);
                    return;
                case UnityEngine.WaitForEndOfFrame:
                    Wait(c, Phase.EndOfFrame, null);
                    return;
                case UnityEngine.Coroutine h:
                    c.Status = CoroutineStatus.Waiting;
                    c.WaitChild = h.Routine;
                    c.WaitDeadline = null;
                    return;
                case IEnumerator e:
                    // Incluye CustomYieldInstruction y WaitForSecondsRealtime.
                    c.Frames.Push(e);
                    continue;
                default:
                    // Unity trata lo desconocido como una espera de un frame.
                    WarnUnknownYield(current.GetType());
                    Wait(c, Phase.Update, null);
                    return;
            }
        }
    }

    /// <summary>Guarda la espera con el ticket de la fase en el momento de ceder.</summary>
    private static void Wait(CoroutineRoutine c, Phase phase, double? deadline)
    {
        c.Status = CoroutineStatus.Waiting;
        c.WaitPhase = phase;
        c.WaitTicket = Ticket(phase);
        c.WaitDeadline = deadline;
        c.WaitChild = null;
    }

    /// <summary>
    /// Tabla de reanudación (§4). Con la regla del ticket una espera nunca
    /// se cumple en la misma bomba en que se emitió.
    /// </summary>
    private static bool ShouldResume(CoroutineRoutine c, Phase phase, long seq)
    {
        // Hija: en cualquier bomba, una vez terminada (Done, Faulted o
        // Stopped) en una bomba anterior. [SV] Reanudar al padre cuando la
        // hija se para o falla puede divergir de Unity; así no quedan
        // rutinas esperando para siempre.
        if (c.WaitChild is { } child)
            return child.IsFinished && child.FinishSeq < seq;

        if (c.WaitPhase != phase || Ticket(phase) <= c.WaitTicket)
            return false;

        return c.WaitDeadline is not { } plazo || UnityEngine.Time.time >= plazo;
    }

    /// <summary>
    /// Fija el estado final. No llama a Dispose: Unity no ejecuta los
    /// bloques finally al parar ([SV] contrastar con Unity). Vaciar la pila
    /// solo suelta referencias.
    /// </summary>
    private static void MarkFinished(CoroutineRoutine c, CoroutineStatus status)
    {
        c.Status = status;
        c.FinishSeq = s_pumpSeq;
        c.WaitChild = null;
        c.WaitDeadline = null;
        c.Frames.Clear();
    }

    /// <summary>Mata solo esta rutina y loguea la excepción con dedup (patrón D15).</summary>
    private void Fail(CoroutineRoutine c, Exception ex)
    {
        string nombre = Describe(c);
        MarkFinished(c, CoroutineStatus.Faulted);
        _routines.Remove(c);

        string clave = $"{nombre}|{ex.GetType().FullName}|{ex.Message}";
        if (!s_excepcionesVistas.Add(clave))
            return;

        UnityEngine.Debug.LogError(
            $"[Corrutinas] Excepción en la corrutina {nombre}; se detiene solo esta rutina " +
            $"(las repeticiones idénticas no se vuelven a loguear):\n{ex}");
    }

    private static void WarnUnknownYield(Type tipo)
    {
        if (!s_tiposAvisados.Add(tipo))
            return;

        UnityEngine.Debug.LogWarning(
            $"[Corrutinas] yield return de tipo '{tipo.FullName}' no soportado; " +
            "se trata como una espera de un frame (aviso único por tipo).");
    }

    // El tipo del iterador generado incluye clase y método ("Clase+<Metodo>d__N").
    private static string Describe(CoroutineRoutine c)
    {
        var tipo = c.Root.GetType();
        return tipo.FullName ?? tipo.Name;
    }
}
