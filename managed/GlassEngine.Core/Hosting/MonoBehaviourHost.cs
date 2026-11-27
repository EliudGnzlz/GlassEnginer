using System;
using System.Collections;
using System.Collections.Generic;
using System.Reflection;
using System.Text.Json;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Hosting;

/// <summary>Fase del enlace de un huésped con su script de Unity.</summary>
internal enum HostState
{
    /// <summary>Envuelto instanciado, sin GameObject todavía.</summary>
    Created,

    /// <summary>Enlazado al GameObject; el Awake del usuario está pendiente (D3).</summary>
    Bound,

    /// <summary>Awake del usuario ejecutado.</summary>
    Awake,

    /// <summary>Destruido o desenganchado: no reenvía nada más.</summary>
    Destroyed,
}

/// <summary>
/// Huésped de un script de Unity (D1): es la instancia que vive en
/// <c>_instances</c> y el motor la ticka como cualquier GlassBehaviour;
/// por dentro reenvía el ciclo de vida al MonoBehaviour envuelto. Los
/// métodos del usuario se resuelven por reflexión (D7), así que también
/// recibe los <c>void Update()</c> privados que ocultan los virtuales.
/// Ninguna excepción del usuario sale de aquí (D15).
/// </summary>
internal sealed class MonoBehaviourHost : GlassBehaviour
{
    private const double ErrorSummaryPeriod = 10.0;

    // Cola de Awake diferidos, en orden de creación (D3).
    private static readonly List<MonoBehaviourHost> s_pending = new();
    private static readonly Dictionary<(string Type, string Method, string Exception), ErrorStat> s_errors = new();
    private static readonly HashSet<string> s_warnedOnce = new();
    // Una vez por proceso: ResetAll no lo reinicia a propósito.
    private static bool s_fieldsNoticeLogged;
    private static float s_frameDeltaTime = 1f / 60f;

    private readonly Type _type;
    private readonly MagicMethods _magic;
    private readonly Action? _update;
    private readonly Action? _fixedUpdate;
    private readonly Action? _lateUpdate;
    private bool _userStarted;
    // true entre un OnEnable entregado y su OnDisable.
    private bool _enabledDelivered;

    internal MonoBehaviourHost(Type monoBehaviourType)
    {
        _type = monoBehaviourType;
        _magic = MagicMethods.For(monoBehaviourType);
        // Si el constructor del usuario lanza, CreateScriptInstance lo
        // captura y devuelve -1 (D2).
        Wrapped = (UnityEngine.MonoBehaviour)Activator.CreateInstance(monoBehaviourType, nonPublic: true)!;
        Coroutines = new CoroutineScheduler(this);
        _update = BindAction(_magic.Update);
        _fixedUpdate = BindAction(_magic.FixedUpdate);
        _lateUpdate = BindAction(_magic.LateUpdate);
    }

    internal UnityEngine.MonoBehaviour Wrapped { get; }

    internal HostState State { get; private set; } = HostState.Created;

    internal CoroutineScheduler Coroutines { get; }

    private bool IsActiveAndEnabled => Wrapped.enabled && Wrapped.gameObject.activeSelf;

    // ---------------------------------------------------------------
    // Ciclo de vida que llama el motor
    // ---------------------------------------------------------------

    /// <summary>
    /// Solo enlace (D3): GameObject, componentes, registro y cola de
    /// pendientes. El Awake del usuario corre en el siguiente BeginFrame.
    /// </summary>
    public override void Awake()
    {
        if (State != HostState.Created)
            return;

        var go = UnityEngine.GameObject.GetOrCreate(EntityId, "Entity");
        Wrapped.gameObject = go;
        Wrapped.name = _type.Name;
        Wrapped.Host = this;
        go.AddComponentInternal(Wrapped);
        State = HostState.Bound;
        MonoBehaviourRegistry.Add(this);
        s_pending.Add(this);
    }

    public override void Start() => TryStart();

    public override void FixedUpdate()
    {
        if (State != HostState.Awake)
            return;
        TryStart();
        if (_fixedUpdate is null || !_userStarted || State != HostState.Awake || !IsActiveAndEnabled)
            return;
        UnityEngine.Time.deltaTime = UnityEngine.Time.fixedDeltaTime;
        Run(_fixedUpdate, "FixedUpdate");
    }

    public override void Update(float deltaTime)
    {
        if (State != HostState.Awake)
            return;
        UnityEngine.Time.deltaTime = deltaTime;
        TryStart();
        if (_update is null || !_userStarted || State != HostState.Awake || !IsActiveAndEnabled)
            return;
        Run(_update, "Update");
    }

    public override void OnCollisionEnter(Collision collision) => DeliverContact(_magic.CollisionEnter, collision, "OnCollisionEnter");

    public override void OnCollisionStay(Collision collision) => DeliverContact(_magic.CollisionStay, collision, "OnCollisionStay");

    public override void OnCollisionExit(Collision collision) => DeliverContact(_magic.CollisionExit, collision, "OnCollisionExit");

    public override void OnTriggerEnter(Collision collision) => DeliverContact(_magic.TriggerEnter, collision, "OnTriggerEnter");

    public override void OnTriggerStay(Collision collision) => DeliverContact(_magic.TriggerStay, collision, "OnTriggerStay");

    public override void OnTriggerExit(Collision collision) => DeliverContact(_magic.TriggerExit, collision, "OnTriggerExit");

    public override void OnControllerColliderHit(Physics.ControllerColliderHit hit)
    {
        if (_magic.ControllerHit is not { } m)
            return;
        EnsureAwake();
        if (State != HostState.Awake)
            return;
        object?[]? args = m.Param == ContactParam.None ? null : new object?[] { hit };
        Call(m.Method, args, "OnControllerColliderHit");
    }

    /// <summary>
    /// Evento de animación (D18): llama al método del envuelto con el
    /// nombre del evento. Firmas, en orden: (), (float) si hay valor,
    /// (int) si el valor es entero, (string) si hay texto.
    /// </summary>
    public override void OnAnimationEvent(string name, float? value, string? text)
    {
        if (string.IsNullOrEmpty(name))
            return;
        EnsureAwake();
        if (State != HostState.Awake)
            return;

        MethodInfo? method;
        object?[]? args = null;
        if ((method = _magic.FindByName(name, null)) is not null)
        {
        }
        else if (value.HasValue && (method = _magic.FindByName(name, typeof(float))) is not null)
        {
            args = new object?[] { value.Value };
        }
        else if (value is { } valorEntero && IsInteger(valorEntero) && (method = _magic.FindByName(name, typeof(int))) is not null)
        {
            args = new object?[] { (int)valorEntero };
        }
        else if (text is not null && (method = _magic.FindByName(name, typeof(string))) is not null)
        {
            args = new object?[] { text };
        }
        else
        {
            WarnOnce($"anim|{_type.FullName}|{name}",
                $"El evento de animación '{name}' no tiene receptor en '{_type.Name}' (se admite (), (float), (int) o (string)).");
            return;
        }

        Call(method!, args, name);
    }

    /// <summary>True si el valor no tiene parte fraccionaria (para elegir la firma (int) de un evento).</summary>
    private static bool IsInteger(float v) => float.IsFinite(v) && v == MathF.Truncate(v);

    /// <summary>
    /// OnDisable y OnDestroy del usuario (solo si llegó a despertar,
    /// como en Unity), StopAll y desregistro.
    /// </summary>
    public override void OnDestroy()
    {
        HostState previous = State;
        if (previous == HostState.Destroyed)
            return;
        State = HostState.Destroyed;
        Coroutines.StopAll();

        if (previous == HostState.Awake)
        {
            DeliverDisable();
            if (_magic.OnDestroy is { } onDestroy)
                Call(onDestroy, null, "OnDestroy");
            // Por si OnDestroy arrancó alguna.
            Coroutines.StopAll();
        }

        Unbind(previous);
    }

    // ---------------------------------------------------------------
    // API interna del huésped
    // ---------------------------------------------------------------

    /// <summary>
    /// Aplica el sobre de ApplyScriptData (D16): nombre, tag y estado
    /// activo. Los "fields" son de C0b: se ignoran con un aviso único.
    /// </summary>
    internal int ApplyScriptData(ReadOnlySpan<byte> jsonUtf8)
    {
        if (State == HostState.Awake)
            return -3;
        if (State != HostState.Bound)
            return -1;

        string? name = null;
        string? tag = null;
        bool? active = null;
        bool hasFields;
        try
        {
            var reader = new Utf8JsonReader(jsonUtf8);
            using JsonDocument doc = JsonDocument.ParseValue(ref reader);
            JsonElement root = doc.RootElement;
            if (root.ValueKind != JsonValueKind.Object)
                return -2;
            if (root.TryGetProperty("v", out JsonElement v)
                && (v.ValueKind != JsonValueKind.Number || !v.TryGetInt32(out int version) || version != 1))
                return -2;
            if (root.TryGetProperty("name", out JsonElement n) && n.ValueKind == JsonValueKind.String)
                name = n.GetString();
            if (root.TryGetProperty("tag", out JsonElement t) && t.ValueKind == JsonValueKind.String)
                tag = t.GetString();
            if (root.TryGetProperty("active", out JsonElement a))
            {
                if (a.ValueKind == JsonValueKind.True)
                    active = true;
                else if (a.ValueKind == JsonValueKind.False)
                    active = false;
            }
            hasFields = root.TryGetProperty("fields", out JsonElement f) && f.ValueKind != JsonValueKind.Null;
        }
        catch (JsonException)
        {
            return -2;
        }

        Wrapped.gameObject.ApplyHostInit(name, tag, active);

        if (hasFields && !s_fieldsNoticeLogged)
        {
            s_fieldsNoticeLogged = true;
            LogWarn("campos serializados: pendientes de C0b");
        }
        return 0;
    }

    /// <summary>
    /// Despierta ya al usuario si sigue pendiente y su GameObject está
    /// activo. Lo usan las entradas que llegan antes del BeginFrame
    /// (contacto, clic, evento de animación) y SetActive(true).
    /// </summary>
    internal void EnsureAwake()
    {
        if (State == HostState.Bound && Wrapped.gameObject.activeSelf)
            RunAwake();
    }

    /// <summary>Start del usuario si está despierto, activo y habilitado (D4); si no, se reintenta.</summary>
    internal void TryStart()
    {
        if (_userStarted || State != HostState.Awake || !IsActiveAndEnabled)
            return;
        _userStarted = true;
        if (_magic.Start is not { } start)
            return;
        object? result = Call(start, null, "Start");
        if (result is IEnumerator routine && State == HostState.Awake)
            Coroutines.Start(routine, null);
    }

    internal void RunLateUpdate()
    {
        if (State != HostState.Awake)
            return;
        TryStart();
        if (_lateUpdate is null || !_userStarted || State != HostState.Awake || !IsActiveAndEnabled)
            return;
        Run(_lateUpdate, "LateUpdate");
    }

    /// <summary>SetActive del GameObject (D13): OnEnable/OnDisable y StopAll al desactivar.</summary>
    internal void NotifyActiveChanged(bool active)
    {
        if (State == HostState.Destroyed)
            return;

        if (active)
        {
            if (State == HostState.Bound)
            {
                // Awake y OnEnable síncronos, como al activar en Unity.
                EnsureAwake();
                return;
            }
            if (State == HostState.Awake && Wrapped.enabled)
                DeliverEnable();
        }
        else
        {
            Coroutines.StopAll();
            if (State == HostState.Awake)
                DeliverDisable();
        }
    }

    /// <summary>Behaviour.enabled (D13): las corrutinas siguen vivas.</summary>
    internal void NotifyEnabledChanged(bool enabled)
    {
        if (State != HostState.Awake || !Wrapped.gameObject.activeSelf)
            return;
        if (enabled)
            DeliverEnable();
        else
            DeliverDisable();
    }

    // ---------------------------------------------------------------
    // Ganchos de TickAllScripts (D5, §3)
    // ---------------------------------------------------------------

    /// <summary>
    /// Gancho 1: EndOfFrame del frame anterior, avance de Time, Awake
    /// diferidos en orden de creación y Start de los hosts de la foto.
    /// </summary>
    internal static void BeginFrame(IReadOnlyList<GlassBehaviour> snapshot, float dt)
    {
        CoroutineScheduler.AdvancePhase(Phase.EndOfFrame);
        for (int i = 0; i < snapshot.Count; i++)
        {
            if (snapshot[i] is MonoBehaviourHost h && h.State != HostState.Destroyed)
                h.SafePump(Phase.EndOfFrame);
        }

        UnityEngine.Time.BeginFrame(dt);
        s_frameDeltaTime = dt;

        // Por índice: un Awake puede crear hosts nuevos, que se despiertan
        // en esta misma pasada (como un Instantiate en Unity).
        for (int i = 0; i < s_pending.Count; i++)
        {
            MonoBehaviourHost h = s_pending[i];
            if (h.State != HostState.Bound || !h.Wrapped.gameObject.activeSelf)
                continue;
            try
            {
                h.RunAwake();
            }
            catch (Exception ex)
            {
                LogError($"Error interno despertando '{h._type.Name}': {ex}");
            }
        }
        s_pending.RemoveAll(static h => h.State != HostState.Bound);

        for (int i = 0; i < snapshot.Count; i++)
        {
            if (snapshot[i] is not MonoBehaviourHost h || h.State == HostState.Destroyed)
                continue;
            try
            {
                h.StartIfNeeded();
            }
            catch (Exception ex)
            {
                LogError($"Error interno en Start de '{h._type.Name}': {ex}");
            }
        }
    }

    /// <summary>Gancho 2, tras cada paso fijo: bomba Fixed con deltaTime = fixedDeltaTime.</summary>
    internal static void AfterFixedStep(IReadOnlyList<GlassBehaviour> snapshot)
    {
        UnityEngine.Time.deltaTime = UnityEngine.Time.fixedDeltaTime;
        CoroutineScheduler.AdvancePhase(Phase.Fixed);
        for (int i = 0; i < snapshot.Count; i++)
        {
            if (snapshot[i] is MonoBehaviourHost h && h.State != HostState.Destroyed)
                h.SafePump(Phase.Fixed);
        }
        UnityEngine.Time.deltaTime = s_frameDeltaTime;
    }

    /// <summary>Gancho 3, tras todos los Update: bomba Update y luego LateUpdate de todos.</summary>
    internal static void AfterUpdates(IReadOnlyList<GlassBehaviour> snapshot)
    {
        CoroutineScheduler.AdvancePhase(Phase.Update);
        for (int i = 0; i < snapshot.Count; i++)
        {
            if (snapshot[i] is MonoBehaviourHost h && h.State != HostState.Destroyed)
                h.SafePump(Phase.Update);
        }

        for (int i = 0; i < snapshot.Count; i++)
        {
            if (snapshot[i] is not MonoBehaviourHost h || h.State == HostState.Destroyed)
                continue;
            try
            {
                h.RunLateUpdate();
            }
            catch (Exception ex)
            {
                LogError($"Error interno en LateUpdate de '{h._type.Name}': {ex}");
            }
        }
    }

    /// <summary>
    /// Reinicia el estado estático del huésped (al sustituir el
    /// ensamblado del proyecto). Los hosts que sigan vivos se desenganchan
    /// sin callbacks de usuario: sus tipos pertenecen al ensamblado viejo.
    /// </summary>
    internal static void ResetAll()
    {
        int detached = 0;
        foreach (MonoBehaviourHost h in MonoBehaviourRegistry.All)
        {
            if (h.State == HostState.Destroyed)
                continue;
            HostState previous = h.State;
            h.State = HostState.Destroyed;
            h.Coroutines.StopAll();
            h.Unbind(previous);
            detached++;
        }
        if (detached > 0)
            LogWarn($"ResetAll: {detached} huéspedes seguían vivos y se han desenganchado sin OnDestroy.");

        s_pending.Clear();
        MonoBehaviourRegistry.Clear();
        s_errors.Clear();
        s_warnedOnce.Clear();
        s_frameDeltaTime = 1f / 60f;
        FacadeContact.Reset();
        CoroutineScheduler.ResetGlobal();
        UnityEngine.GameObject.ClearEntityMap();
    }

    // ---------------------------------------------------------------
    // Internos
    // ---------------------------------------------------------------

    private void RunAwake()
    {
        // El estado cambia antes de llamar: un EnsureAwake reentrante no repite.
        State = HostState.Awake;
        if (_magic.Awake is { } awake)
            Call(awake, null, "Awake");
        if (State == HostState.Awake && IsActiveAndEnabled)
            DeliverEnable();
    }

    private void DeliverEnable()
    {
        if (_enabledDelivered)
            return;
        _enabledDelivered = true;
        if (_magic.OnEnable is { } onEnable)
            Call(onEnable, null, "OnEnable");
    }

    private void DeliverDisable()
    {
        if (!_enabledDelivered)
            return;
        _enabledDelivered = false;
        if (_magic.OnDisable is { } onDisable)
            Call(onDisable, null, "OnDisable");
    }

    private void DeliverContact(ContactMethod? contact, Collision collision, string message)
    {
        if (contact is not { } m)
            return;
        EnsureAwake();
        if (State != HostState.Awake)
            return;

        object?[]? args;
        switch (m.Param)
        {
            case ContactParam.Collision:
                args = new object?[] { FacadeContact.ToFacade(collision) };
                break;
            case ContactParam.Collider:
            {
                UnityEngine.GameObject? other = collision.gameObject;
                args = new object?[] { FacadeContact.ColliderOf(other?.EntityId ?? 0, other?.name) };
                break;
            }
            default:
                args = null;
                break;
        }

        object? result = Call(m.Method, args, message);
        if (result is IEnumerator routine && State == HostState.Awake)
            Coroutines.Start(routine, null);
    }

    private void Unbind(HostState previous)
    {
        MonoBehaviourRegistry.Remove(this);
        if (ReferenceEquals(Wrapped.Host, this))
            Wrapped.Host = null;
        if (previous != HostState.Created)
            Wrapped.gameObject.RemoveComponentInternal(Wrapped);
    }

    private void SafePump(Phase phase)
    {
        try
        {
            Coroutines.Pump(phase);
        }
        catch (Exception ex)
        {
            ReportException($"corrutinas ({phase})", ex);
        }
    }

    private Action? BindAction(MethodInfo? method)
    {
        if (method is null)
            return null;
        if (method.ReturnType == typeof(void))
        {
            try
            {
                return (Action)Delegate.CreateDelegate(typeof(Action), Wrapped, method);
            }
            catch (Exception ex) when (ex is ArgumentException or MethodAccessException)
            {
                // Se cae a Invoke.
            }
        }
        UnityEngine.MonoBehaviour target = Wrapped;
        return () => method.Invoke(target, null);
    }

    private object? Call(MethodInfo method, object?[]? args, string what)
    {
        try
        {
            return method.Invoke(Wrapped, args);
        }
        catch (Exception ex)
        {
            ReportException(what, ex);
            return null;
        }
    }

    private void Run(Action action, string what)
    {
        try
        {
            action();
        }
        catch (Exception ex)
        {
            ReportException(what, ex);
        }
    }

    /// <summary>
    /// D15: la primera aparición de (tipo, método, excepción) sale
    /// completa; las repeticiones se resumen como mucho cada <see cref="ErrorSummaryPeriod"/> segundos de reloj real,
    /// con un único resumen por clave que acumula las repeticiones.
    /// </summary>
    private void ReportException(string what, Exception ex)
    {
        // Las excepciones de método invocado por reflexión llegan envueltas.
        if (ex is System.Reflection.TargetInvocationException { InnerException: { } inner })
            ex = inner;

        string typeName = _type.FullName ?? _type.Name;
        Type exType = ex.GetType();
        var key = (Type: typeName, Method: what, Exception: exType.FullName ?? exType.Name);
        long now = System.Diagnostics.Stopwatch.GetTimestamp();

        string message = string.Empty;
        lock (s_errors)
        {
            if (!s_errors.TryGetValue(key, out ErrorStat? stat))
            {
                // Primera aparición: se emite completa y se abre la ventana de resumen.
                s_errors[key] = new ErrorStat { Count = 0, LastSummaryTimestamp = now };
                message = $"[{typeName}] excepción en {what}: {ex}";
            }
            else
            {
                stat.Count++;
                double elapsed = (now - stat.LastSummaryTimestamp)
                    / (double)System.Diagnostics.Stopwatch.Frequency;
                if (elapsed >= ErrorSummaryPeriod)
                {
                    message = $"[{typeName}] {key.Exception} en {what} repetida "
                        + $"{stat.Count} veces en los últimos {elapsed:F1} s";
                    stat.Count = 0;
                    stat.LastSummaryTimestamp = now;
                }
            }
        }

        // Se emite fuera del lock para no retener s_errors durante el log.
        if (message.Length > 0)
            LogError(message);
    }

    /// <summary>
    /// Emite <paramref name="mensaje"/> como aviso solo la primera vez que
    /// aparece <paramref name="clave"/> en todo el proceso.
    /// </summary>
    private static void WarnOnce(string clave, string mensaje)
    {
        bool nueva;
        lock (s_warnedOnce)
        {
            nueva = s_warnedOnce.Add(clave);
        }

        if (nueva)
            RuntimeBridge.Log(3, mensaje);
    }

    private static void LogError(string mensaje)
    {
        RuntimeBridge.Log(4, mensaje);
    }

    private static void LogWarn(string mensaje)
    {
        RuntimeBridge.Log(3, mensaje);
    }

    /// <summary>
    /// Estado de deduplicación de una clave (tipo, método, excepción):
    /// repeticiones desde el último resumen y marca de tiempo de ese resumen
    /// (en ticks de <see cref="System.Diagnostics.Stopwatch"/>).
    /// </summary>
    private sealed class ErrorStat
    {
        public int Count;
        public long LastSummaryTimestamp;
    }
}
