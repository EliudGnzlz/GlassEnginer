using System;
using System.Collections.Generic;
using System.Reflection;
using System.Runtime.CompilerServices;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Hosting;

/// <summary>Parámetro que espera un método de contacto del usuario (D9).</summary>
internal enum ContactParam
{
    /// <summary>Firma sin parámetros: ().</summary>
    None,

    /// <summary>(UnityEngine.Collision).</summary>
    Collision,

    /// <summary>(UnityEngine.Collider).</summary>
    Collider,

    /// <summary>(GlassEngine.Core.Physics.ControllerColliderHit).</summary>
    ControllerHit,
}

/// <summary>Método de contacto resuelto y la forma de su parámetro.</summary>
internal sealed class ContactMethod
{
    internal ContactMethod(MethodInfo method, ContactParam param)
    {
        Method = method;
        Param = param;
    }

    internal MethodInfo Method { get; }

    internal ContactParam Param { get; }
}

/// <summary>
/// Métodos mágicos de un tipo de MonoBehaviour (D7/D8), resueltos por
/// reflexión una vez por tipo. Se recorre la jerarquía desde el tipo
/// envuelto hasta UnityEngine.MonoBehaviour (sin incluirlo) con
/// DeclaredOnly, así que gana la declaración más derivada, sea pública
/// o privada. Las firmas de contacto no admitidas (D9) dejan la
/// propiedad a null y se avisan una vez: el aviso se emite al construir
/// la entrada de la caché, que existe una sola vez por tipo.
/// </summary>
internal sealed class MagicMethods
{
    private const BindingFlags Flags =
        BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly;

    // Nivel de aviso de RuntimeBridge.Log: debe coincidir con el que usa LogWarn del host.
    private const int LogLevelWarning = 1;

    private static readonly ConditionalWeakTable<Type, MagicMethods> s_cache = new();

    private readonly Type _type;
    private readonly Dictionary<(string Name, Type? Arg), MethodInfo?> _byName = new();

    private MagicMethods(Type type)
    {
        _type = type;

        Awake = FindByName("Awake", null);
        OnEnable = FindByName("OnEnable", null);
        Start = FindByName("Start", null);
        OnDisable = FindByName("OnDisable", null);
        OnDestroy = FindByName("OnDestroy", null);
        Update = FindByName("Update", null);
        FixedUpdate = FindByName("FixedUpdate", null);
        LateUpdate = FindByName("LateUpdate", null);

        CollisionEnter = FindContact("OnCollisionEnter", typeof(UnityEngine.Collision), ContactParam.Collision);
        CollisionStay = FindContact("OnCollisionStay", typeof(UnityEngine.Collision), ContactParam.Collision);
        CollisionExit = FindContact("OnCollisionExit", typeof(UnityEngine.Collision), ContactParam.Collision);
        TriggerEnter = FindContact("OnTriggerEnter", typeof(UnityEngine.Collider), ContactParam.Collider);
        TriggerStay = FindContact("OnTriggerStay", typeof(UnityEngine.Collider), ContactParam.Collider);
        TriggerExit = FindContact("OnTriggerExit", typeof(UnityEngine.Collider), ContactParam.Collider);
        ControllerHit = FindContact("OnControllerColliderHit",
            typeof(UnityEngine.ControllerColliderHit), ContactParam.ControllerHit);
    }

    internal MethodInfo? Awake { get; }

    internal MethodInfo? OnEnable { get; }

    internal MethodInfo? Start { get; }

    internal MethodInfo? OnDisable { get; }

    internal MethodInfo? OnDestroy { get; }

    internal MethodInfo? Update { get; }

    internal MethodInfo? FixedUpdate { get; }

    internal MethodInfo? LateUpdate { get; }

    internal ContactMethod? CollisionEnter { get; }

    internal ContactMethod? CollisionStay { get; }

    internal ContactMethod? CollisionExit { get; }

    internal ContactMethod? TriggerEnter { get; }

    internal ContactMethod? TriggerStay { get; }

    internal ContactMethod? TriggerExit { get; }

    internal ContactMethod? ControllerHit { get; }

    /// <summary>Entrada de caché del tipo (se calcula la primera vez).</summary>
    internal static MagicMethods For(Type type) => s_cache.GetValue(type, static t => new MagicMethods(t));

    /// <summary>
    /// Método de instancia <paramref name="name"/> con firma () si
    /// <paramref name="argType"/> es null o (argType) si no. Gana el más
    /// derivado. El resultado, también el null, queda en caché.
    /// </summary>
    internal MethodInfo? FindByName(string name, Type? argType)
    {
        var key = (name, argType);
        if (_byName.TryGetValue(key, out MethodInfo? cached))
            return cached;

        MethodInfo? found = null;
        for (Type? t = _type; t is not null && t != typeof(UnityEngine.MonoBehaviour); t = t.BaseType)
        {
            foreach (MethodInfo m in t.GetMethods(Flags))
            {
                if (m.Name != name || m.IsGenericMethodDefinition)
                    continue;
                ParameterInfo[] ps = m.GetParameters();
                bool matches = argType is null
                    ? ps.Length == 0
                    : ps.Length == 1 && ps[0].ParameterType == argType;
                if (matches)
                {
                    found = m;
                    break;
                }
            }
            if (found is not null)
                break;
        }

        _byName[key] = found;
        return found;
    }

    /// <summary>
    /// Decide el primer nivel de la jerarquía que declara el nombre. En
    /// ese nivel, (argType) gana sobre (); si solo hay firmas no
    /// admitidas, devuelve null y avisa.
    /// </summary>
    private ContactMethod? FindContact(string name, Type argType, ContactParam param)
    {
        for (Type? t = _type; t is not null && t != typeof(UnityEngine.MonoBehaviour); t = t.BaseType)
        {
            MethodInfo? typed = null;
            MethodInfo? bare = null;
            MethodInfo? wrong = null;
            foreach (MethodInfo m in t.GetMethods(Flags))
            {
                if (m.Name != name || m.IsGenericMethodDefinition)
                    continue;
                ParameterInfo[] ps = m.GetParameters();
                if (ps.Length == 0)
                    bare ??= m;
                else if (ps.Length == 1 && ps[0].ParameterType == argType)
                    typed ??= m;
                else
                    wrong ??= m;
            }

            if (typed is not null)
                return new ContactMethod(typed, param);
            if (bare is not null)
                return new ContactMethod(bare, ContactParam.None);
            if (wrong is not null)
            {
                RuntimeBridge.Log(LogLevelWarning,
                    $"MonoBehaviour: '{_type.FullName}.{name}({Describe(wrong)})' tiene una firma no admitida; " +
                    $"se espera ({argType.FullName}) o (). No se llamará.");
                return null;
            }
        }
        return null;
    }

    private static string Describe(MethodInfo method)
    {
        ParameterInfo[] ps = method.GetParameters();
        var names = new string[ps.Length];
        for (int i = 0; i < ps.Length; i++)
            names[i] = ps[i].ParameterType.FullName ?? ps[i].ParameterType.Name;
        return string.Join(", ", names);
    }
}
