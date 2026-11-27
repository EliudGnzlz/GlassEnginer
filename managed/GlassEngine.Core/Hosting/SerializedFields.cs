// Aplicador de campos serializados (C0b): vuelca el objeto "fields" del sobre
// ApplyScriptData sobre el MonoBehaviour envuelto, por reflexión, antes de su
// Awake diferido. Elegibilidad y conversión v1 al estilo de Unity.

using System;
using System.Collections;
using System.Collections.Generic;
using System.Reflection;
using System.Text.Json;
using UnityEngine;
using UnityEngine.Serialization;

namespace GlassEngine.Core.Hosting;

internal static class SerializedFields
{
    private const BindingFlags Instancia =
        BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;

    // Tipos de Unity que son referencias a objetos de escena o asset: no convertibles en v1.
    private static readonly HashSet<string> s_referenciasUnity = new(StringComparer.Ordinal)
    {
        "UnityEngine.Object",
        "UnityEngine.GameObject",
        "UnityEngine.Component",
        "UnityEngine.Transform",
        "UnityEngine.MonoBehaviour",
        "UnityEngine.ScriptableObject",
    };

    /// <summary>
    /// Aplica cada propiedad de <paramref name="fields"/> al campo homónimo (o
    /// con alias [FormerlySerializedAs]) de <paramref name="target"/>. Los fallos
    /// se avisan por <paramref name="warnOnce"/> (clave, mensaje) y no detienen el resto.
    /// </summary>
    public static void Apply(object target, JsonElement fields, Action<string, string> warnOnce)
    {
        ArgumentNullException.ThrowIfNull(target);
        ArgumentNullException.ThrowIfNull(warnOnce);
        if (fields.ValueKind != JsonValueKind.Object)
            return;
        AplicarMiembros(target, fields, warnOnce);
    }

    private static void AplicarMiembros(object destino, JsonElement miembros, Action<string, string> warnOnce)
    {
        Type tipo = destino.GetType();
        string nombreTipo = tipo.FullName ?? tipo.Name;

        foreach (JsonProperty propiedad in miembros.EnumerateObject())
        {
            string nombre = propiedad.Name;
            FieldInfo? campo = BuscarCampo(tipo, nombre);
            if (campo is null)
            {
                warnOnce($"fields|{nombreTipo}|{nombre}|sin-campo",
                    $"campos serializados: {nombreTipo} no tiene un campo serializable '{nombre}'; se ignora");
                continue;
            }

            string? razon = RazonNoConvertible(campo.FieldType);
            if (razon is not null)
            {
                warnOnce($"fields|{nombreTipo}|{nombre}|no-convertible",
                    $"campos serializados: {nombreTipo}.{campo.Name} ({campo.FieldType.Name}) no convertible: {razon}; se ignora");
                continue;
            }

            try
            {
                campo.SetValue(destino, Convertir(propiedad.Value, campo.FieldType, warnOnce));
            }
            catch (Exception ex)
            {
                warnOnce($"fields|{nombreTipo}|{nombre}|conversion",
                    $"campos serializados: {nombreTipo}.{campo.Name} ({campo.FieldType.Name}) no admite el valor JSON {propiedad.Value.ValueKind}: {ex.Message}; se ignora");
            }
        }
    }

    // Nombre exacto en toda la jerarquía primero; después, alias [FormerlySerializedAs].
    private static FieldInfo? BuscarCampo(Type tipo, string nombre)
    {
        foreach (FieldInfo campo in CamposSerializables(tipo))
        {
            if (campo.Name == nombre)
                return campo;
        }
        foreach (FieldInfo campo in CamposSerializables(tipo))
        {
            foreach (FormerlySerializedAsAttribute alias in campo.GetCustomAttributes<FormerlySerializedAsAttribute>(inherit: false))
            {
                if (alias.OldName == nombre)
                    return campo;
            }
        }
        return null;
    }

    // Del tipo concreto hacia arriba, sin llegar a MonoBehaviour (ni a object/ValueType
    // en los tipos anidados). Públicos, o no públicos con [SerializeField]; sin
    // readonly ni [NonSerialized]. Los estáticos quedan fuera por BindingFlags.Instance.
    private static IEnumerable<FieldInfo> CamposSerializables(Type tipo)
    {
        for (Type? nivel = tipo;
             nivel is not null && nivel != typeof(MonoBehaviour) && nivel != typeof(object) && nivel != typeof(ValueType);
             nivel = nivel.BaseType)
        {
            foreach (FieldInfo campo in nivel.GetFields(Instancia | BindingFlags.DeclaredOnly))
            {
                if (campo.IsInitOnly || campo.IsDefined(typeof(NonSerializedAttribute), inherit: false))
                    continue;
                if (campo.IsPublic || campo.IsDefined(typeof(SerializeField), inherit: false))
                    yield return campo;
            }
        }
    }

    // null si el tipo es convertible; si no, la razón.
    private static string? RazonNoConvertible(Type tipo)
    {
        if (tipo.IsEnum || EsEscalar(tipo) || EsMatematicaUnity(tipo))
            return null;
        if (tipo.IsArray)
        {
            if (tipo.GetArrayRank() != 1)
                return "arreglo multidimensional";
            return RazonElemento(tipo.GetElementType()!);
        }
        if (EsLista(tipo, out Type? elemento))
            return RazonElemento(elemento);
        if (tipo.IsPointer || tipo.IsByRef)
            return "puntero o referencia";
        if (EsReferenciaUnity(tipo))
            return "referencia a objeto de Unity (no soportada en v1)";
        if (EsDeSystemOUnity(tipo))
            return "tipo de System/UnityEngine no soportado en v1";
        if (tipo.IsInterface || tipo.IsAbstract)
            return "interfaz o tipo abstracto";
        if (tipo.ContainsGenericParameters)
            return "tipo genérico abierto";
        if (!tipo.IsValueType && tipo.GetConstructor(Instancia, null, Type.EmptyTypes, null) is null)
            return "sin constructor sin parámetros";
        return null;
    }

    private static string? RazonElemento(Type elemento)
    {
        string? razon = RazonNoConvertible(elemento);
        return razon is null ? null : $"elemento {elemento.Name}: {razon}";
    }

    private static object? Convertir(JsonElement valor, Type tipo, Action<string, string> warnOnce)
    {
        if (valor.ValueKind == JsonValueKind.Null && !tipo.IsValueType)
            return null;

        if (tipo.IsEnum)
        {
            if (valor.ValueKind == JsonValueKind.String)
                return Enum.Parse(tipo, valor.GetString()!, ignoreCase: false);
            return Enum.ToObject(tipo, Convertir(valor, Enum.GetUnderlyingType(tipo), warnOnce)!);
        }

        switch (Type.GetTypeCode(tipo))
        {
            case TypeCode.Boolean: return valor.GetBoolean();
            case TypeCode.String: return valor.GetString();
            case TypeCode.Char: return Caracter(valor);
            case TypeCode.SByte: return (sbyte)Entero(valor, sbyte.MinValue, sbyte.MaxValue);
            case TypeCode.Byte: return (byte)Entero(valor, byte.MinValue, byte.MaxValue);
            case TypeCode.Int16: return (short)Entero(valor, short.MinValue, short.MaxValue);
            case TypeCode.UInt16: return (ushort)Entero(valor, ushort.MinValue, ushort.MaxValue);
            case TypeCode.Int32: return (int)Entero(valor, int.MinValue, int.MaxValue);
            case TypeCode.UInt32: return (uint)Entero(valor, uint.MinValue, uint.MaxValue);
            case TypeCode.Int64: return Entero(valor, long.MinValue, long.MaxValue);
            case TypeCode.UInt64:
                return valor.TryGetUInt64(out ulong sinSigno)
                    ? sinSigno
                    : throw new OverflowException("se esperaba un entero sin signo de 64 bits");
            case TypeCode.Single: return valor.GetSingle();
            case TypeCode.Double: return valor.GetDouble();
            case TypeCode.Decimal: return valor.GetDecimal();
        }

        if (tipo == typeof(Vector2))
            return new Vector2(Componente(valor, "x"), Componente(valor, "y"));
        if (tipo == typeof(Vector3))
            return new Vector3(Componente(valor, "x"), Componente(valor, "y"), Componente(valor, "z"));
        if (tipo == typeof(Vector4))
            return new Vector4(Componente(valor, "x"), Componente(valor, "y"), Componente(valor, "z"), Componente(valor, "w"));
        if (tipo == typeof(Quaternion))
            return new Quaternion(Componente(valor, "x"), Componente(valor, "y"), Componente(valor, "z"), Componente(valor, "w"));
        if (tipo == typeof(Color))
            return new Color(Componente(valor, "r"), Componente(valor, "g"), Componente(valor, "b"), Componente(valor, "a"));

        if (tipo.IsArray)
        {
            Type elemento = tipo.GetElementType()!;
            Array arreglo = Array.CreateInstance(elemento, valor.GetArrayLength());
            int i = 0;
            foreach (JsonElement item in valor.EnumerateArray())
                arreglo.SetValue(Convertir(item, elemento, warnOnce), i++);
            return arreglo;
        }

        if (EsLista(tipo, out Type? elementoLista))
        {
            var lista = (IList)Activator.CreateInstance(tipo)!;
            foreach (JsonElement item in valor.EnumerateArray())
                lista.Add(Convertir(item, elementoLista, warnOnce));
            return lista;
        }

        // Tipo propio (clase o struct): instancia nueva y mismas reglas de campo.
        ExigirObjeto(valor);
        object instancia = Activator.CreateInstance(tipo, nonPublic: true)!;
        AplicarMiembros(instancia, valor, warnOnce);
        return instancia;
    }

    private static long Entero(JsonElement valor, long min, long max)
    {
        if (!valor.TryGetInt64(out long n) || n < min || n > max)
            throw new OverflowException($"se esperaba un entero en [{min}, {max}]");
        return n;
    }

    private static char Caracter(JsonElement valor)
    {
        if (valor.ValueKind == JsonValueKind.String)
        {
            string s = valor.GetString()!;
            if (s.Length != 1)
                throw new FormatException("se esperaba una cadena de un carácter");
            return s[0];
        }
        return (char)Entero(valor, char.MinValue, char.MaxValue);
    }

    private static float Componente(JsonElement valor, string miembro)
    {
        ExigirObjeto(valor);
        return valor.TryGetProperty(miembro, out JsonElement c) ? c.GetSingle() : 0f;
    }

    private static void ExigirObjeto(JsonElement valor)
    {
        if (valor.ValueKind != JsonValueKind.Object)
            throw new FormatException("se esperaba un objeto JSON");
    }

    private static bool EsEscalar(Type tipo) =>
        Type.GetTypeCode(tipo) is TypeCode.Boolean or TypeCode.Char or TypeCode.String
            or TypeCode.SByte or TypeCode.Byte or TypeCode.Int16 or TypeCode.UInt16
            or TypeCode.Int32 or TypeCode.UInt32 or TypeCode.Int64 or TypeCode.UInt64
            or TypeCode.Single or TypeCode.Double or TypeCode.Decimal;

    private static bool EsMatematicaUnity(Type tipo) =>
        tipo == typeof(Vector2) || tipo == typeof(Vector3) || tipo == typeof(Vector4)
        || tipo == typeof(Quaternion) || tipo == typeof(Color);

    private static bool EsLista(Type tipo, out Type elemento)
    {
        if (tipo.IsGenericType && tipo.GetGenericTypeDefinition() == typeof(List<>))
        {
            elemento = tipo.GetGenericArguments()[0];
            return true;
        }
        elemento = null!;
        return false;
    }

    private static bool EsReferenciaUnity(Type tipo)
    {
        for (Type? t = tipo; t is not null; t = t.BaseType)
        {
            if (t.FullName is string nombre && s_referenciasUnity.Contains(nombre))
                return true;
        }
        return false;
    }

    private static bool EsDeSystemOUnity(Type tipo)
    {
        string? ns = tipo.Namespace;
        return ns is not null
            && (ns == "System" || ns.StartsWith("System.", StringComparison.Ordinal)
                || ns == "UnityEngine" || ns.StartsWith("UnityEngine.", StringComparison.Ordinal));
    }
}
