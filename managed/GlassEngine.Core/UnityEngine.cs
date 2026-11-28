using System;
using System.Collections.Generic;
using GlassEngine.Core;
using GlassEngine.Core.Hosting;
using GlassEngine.Core.Interop;
using InteropVector3 = GlassEngine.Core.Interop.Vector3;
using InteropQuaternion = GlassEngine.Core.Interop.Quaternion;

namespace UnityEngine
{
    // ==========================================
    // TIPOS MATEMÁTICOS BÁSICOS
    // ==========================================

    public struct Vector2
    {
        public float x;
        public float y;

        public Vector2(float x, float y) { this.x = x; this.y = y; }

        public static Vector2 zero => new(0f, 0f);
        public static Vector2 one => new(1f, 1f);
        public static Vector2 up => new(0f, 1f);
        public static Vector2 down => new(0f, -1f);
        public static Vector2 left => new(-1f, 0f);
        public static Vector2 right => new(1f, 0f);

        public float magnitude => MathF.Sqrt(x * x + y * y);
        public Vector2 normalized => magnitude > 1e-5f ? this / magnitude : zero;

        public static Vector2 operator +(Vector2 a, Vector2 b) => new(a.x + b.x, a.y + b.y);
        public static Vector2 operator -(Vector2 a, Vector2 b) => new(a.x - b.x, a.y - b.y);
        public static Vector2 operator *(Vector2 a, float d) => new(a.x * d, a.y * d);
        public static Vector2 operator /(Vector2 a, float d) => new(a.x / d, a.y / d);

        public override string ToString() => $"({x:F2}, {y:F2})";
    }

    public struct Vector3
    {
        public float x;
        public float y;
        public float z;

        public Vector3(float x, float y, float z) { this.x = x; this.y = y; this.z = z; }

        public static Vector3 zero => new(0f, 0f, 0f);
        public static Vector3 one => new(1f, 1f, 1f);
        public static Vector3 forward => new(0f, 0f, 1f);
        public static Vector3 back => new(0f, 0f, -1f);
        public static Vector3 up => new(0f, 1f, 0f);
        public static Vector3 down => new(0f, -1f, 0f);
        public static Vector3 left => new(-1f, 0f, 0f);
        public static Vector3 right => new(1f, 0f, 0f);

        public float magnitude => MathF.Sqrt(x * x + y * y + z * z);
        public float sqrMagnitude => x * x + y * y + z * z;
        public Vector3 normalized => magnitude > 1e-5f ? this / magnitude : zero;

        public static Vector3 operator +(Vector3 a, Vector3 b) => new(a.x + b.x, a.y + b.y, a.z + b.z);
        public static Vector3 operator -(Vector3 a, Vector3 b) => new(a.x - b.x, a.y - b.y, a.z - b.z);
        public static Vector3 operator -(Vector3 a) => new(-a.x, -a.y, -a.z);
        public static Vector3 operator *(Vector3 a, float d) => new(a.x * d, a.y * d, a.z * d);
        public static Vector3 operator *(float d, Vector3 a) => new(a.x * d, a.y * d, a.z * d);
        public static Vector3 operator /(Vector3 a, float d) => new(a.x / d, a.y / d, a.z / d);

        public static float Dot(Vector3 a, Vector3 b) => a.x * b.x + a.y * b.y + a.z * b.z;
        public static Vector3 Cross(Vector3 a, Vector3 b) => new(
            a.y * b.z - a.z * b.y,
            a.z * b.x - a.x * b.z,
            a.x * b.y - a.y * b.x
        );

        public static float Distance(Vector3 a, Vector3 b) => (a - b).magnitude;
        public static Vector3 Lerp(Vector3 a, Vector3 b, float t)
        {
            t = Math.Clamp(t, 0f, 1f);
            return new Vector3(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t);
        }

        public static implicit operator InteropVector3(Vector3 v) => new(v.x, v.y, v.z);
        public static implicit operator Vector3(InteropVector3 v) => new(v.X, v.Y, v.Z);

        public override string ToString() => $"({x:F2}, {y:F2}, {z:F2})";
    }

    public struct Vector4
    {
        public float x;
        public float y;
        public float z;
        public float w;

        public Vector4(float x, float y, float z, float w) { this.x = x; this.y = y; this.z = z; this.w = w; }

        public static Vector4 zero => new(0f, 0f, 0f, 0f);
        public static Vector4 one => new(1f, 1f, 1f, 1f);

        public override string ToString() => $"({x:F2}, {y:F2}, {z:F2}, {w:F2})";
    }

    public struct Quaternion
    {
        public float x;
        public float y;
        public float z;
        public float w;

        public Quaternion(float x, float y, float z, float w)
        {
            this.x = x; this.y = y; this.z = z; this.w = w;
        }

        public static Quaternion identity => new(0f, 0f, 0f, 1f);

        public static Quaternion Euler(float pitch, float yaw, float roll)
        {
            float p = pitch * 0.5f * (MathF.PI / 180f);
            float y = yaw * 0.5f * (MathF.PI / 180f);
            float r = roll * 0.5f * (MathF.PI / 180f);

            float sinP = MathF.Sin(p);
            float cosP = MathF.Cos(p);
            float sinY = MathF.Sin(y);
            float cosY = MathF.Cos(y);
            float sinR = MathF.Sin(r);
            float cosR = MathF.Cos(r);

            return new Quaternion(
                sinR * cosP * cosY - cosR * sinP * sinY,
                cosR * sinP * cosY + sinR * cosP * sinY,
                cosR * cosP * sinY - sinR * sinP * cosY,
                cosR * cosP * cosY + sinR * sinP * sinY
            );
        }

        public static Quaternion operator *(Quaternion a, Quaternion b) => new(
            a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
            a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
            a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z
        );

        public static Vector3 operator *(Quaternion rotation, Vector3 point)
        {
            float num1 = rotation.x * 2f;
            float num2 = rotation.y * 2f;
            float num3 = rotation.z * 2f;
            float num4 = rotation.x * num1;
            float num5 = rotation.y * num2;
            float num6 = rotation.z * num3;
            float num7 = rotation.x * num2;
            float num8 = rotation.x * num3;
            float num9 = rotation.y * num3;
            float num10 = rotation.w * num1;
            float num11 = rotation.w * num2;
            float num12 = rotation.w * num3;

            return new Vector3(
                (1f - (num5 + num6)) * point.x + (num7 - num12) * point.y + (num8 + num11) * point.z,
                (num7 + num12) * point.x + (1f - (num4 + num6)) * point.y + (num9 - num10) * point.z,
                (num8 - num11) * point.x + (num9 + num10) * point.y + (1f - (num4 + num5)) * point.z
            );
        }

        public static implicit operator InteropQuaternion(Quaternion q) => new(q.x, q.y, q.z, q.w);
        public static implicit operator Quaternion(InteropQuaternion q) => new(q.X, q.Y, q.Z, q.W);

        public override string ToString() => $"({x:F2}, {y:F2}, {z:F2}, {w:F2})";
    }

    public struct Color
    {
        public float r;
        public float g;
        public float b;
        public float a;

        public Color(float r, float g, float b, float a = 1.0f)
        {
            this.r = r; this.g = g; this.b = b; this.a = a;
        }

        public static Color red => new(1f, 0f, 0f, 1f);
        public static Color green => new(0f, 1f, 0f, 1f);
        public static Color blue => new(0f, 0f, 1f, 1f);
        public static Color white => new(1f, 1f, 1f, 1f);
        public static Color black => new(0f, 0f, 0f, 1f);
        public static Color yellow => new(1f, 0.92f, 0.016f, 1f);
        public static Color clear => new(0f, 0f, 0f, 0f);
    }

    public static class Mathf
    {
        public const float PI = MathF.PI;
        public const float Deg2Rad = PI / 180f;
        public const float Rad2Deg = 180f / PI;

        public static float Sin(float f) => MathF.Sin(f);
        public static float Cos(float f) => MathF.Cos(f);
        public static float Sqrt(float f) => MathF.Sqrt(f);
        public static float Clamp(float value, float min, float max) => Math.Clamp(value, min, max);
        public static float Clamp01(float value) => Math.Clamp(value, 0f, 1f);
        public static float Lerp(float a, float b, float t) => a + (b - a) * Clamp01(t);
        public static float Abs(float f) => MathF.Abs(f);
        public static float Max(float a, float b) => MathF.Max(a, b);
        public static float Min(float a, float b) => MathF.Min(a, b);
    }

    // ==========================================
    // SISTEMA DE TIEMPO E INPUT
    // ==========================================

    public static class Time
    {
        public static float deltaTime { get; internal set; } = 0.01667f;
        public static float time { get; internal set; } = 0.0f;
        public static int frameCount { get; internal set; } = 0;

        /// <summary>Paso de FixedUpdate (el mismo FixedStep de RuntimeBridge).</summary>
        public static float fixedDeltaTime { get; internal set; } = 1f / 60f;

        // Reloj de pared: arranca en el primer uso y Reset lo reinicia.
        private static System.Diagnostics.Stopwatch? s_clock;

        /// <summary>Segundos reales desde el primer uso (o desde el último Reset).</summary>
        public static float realtimeSinceStartup
        {
            get
            {
                s_clock ??= System.Diagnostics.Stopwatch.StartNew();
                return (float)s_clock.Elapsed.TotalSeconds;
            }
        }

        /// <summary>Avance de frame que hace el huésped al inicio de cada tick.</summary>
        internal static void BeginFrame(float dt)
        {
            frameCount++;
            time += dt;
            deltaTime = dt;
        }

        /// <summary>Vuelta al estado inicial al sustituir el ensamblado del proyecto.</summary>
        internal static void Reset()
        {
            time = 0f;
            frameCount = 0;
            deltaTime = 1f / 60f;
            if (s_clock is null)
                s_clock = System.Diagnostics.Stopwatch.StartNew();
            else
                s_clock.Restart();
        }
    }

    public enum KeyCode
    {
        None = 0,
        W, A, S, D,
        UpArrow, DownArrow, LeftArrow, RightArrow,
        Space, LeftShift, Return, Escape
    }

    public static class Input
    {
        public static bool GetKey(string keyName) => GlassEngine.Core.Input.GetKey(keyName);

        public static bool GetKey(KeyCode key)
        {
            return key switch
            {
                KeyCode.W => GlassEngine.Core.Input.GetKey("w"),
                KeyCode.S => GlassEngine.Core.Input.GetKey("s"),
                KeyCode.A => GlassEngine.Core.Input.GetKey("a"),
                KeyCode.D => GlassEngine.Core.Input.GetKey("d"),
                KeyCode.Space => GlassEngine.Core.Input.GetKey("space"),
                KeyCode.LeftShift => GlassEngine.Core.Input.GetKey("shift"),
                KeyCode.UpArrow => GlassEngine.Core.Input.GetKey("up"),
                KeyCode.DownArrow => GlassEngine.Core.Input.GetKey("down"),
                KeyCode.LeftArrow => GlassEngine.Core.Input.GetKey("left"),
                KeyCode.RightArrow => GlassEngine.Core.Input.GetKey("right"),
                _ => false
            };
        }

        public static float GetAxis(string axisName)
        {
            if (axisName == "Horizontal") return GlassEngine.Core.Input.GetAxisHorizontal();
            if (axisName == "Vertical") return GlassEngine.Core.Input.GetAxisVertical();
            return 0f;
        }

        public static bool GetButton(string buttonName) => GlassEngine.Core.Input.GetButton(buttonName);

        public static Vector2 mousePosition => GlassEngine.Core.Input.mousePosition;

        public static bool GetMouseButton(int button) => GlassEngine.Core.Input.GetMouseButton(button);

        public static bool GetMouseButtonDown(int button) => GlassEngine.Core.Input.GetMouseButtonDown(button);

        public static bool GetMouseButtonUp(int button) => GlassEngine.Core.Input.GetMouseButtonUp(button);
    }

    public static class Debug
    {
        public static void Log(object message) => RuntimeBridge.Log(2, message?.ToString() ?? "null");
        public static void LogWarning(object message) => RuntimeBridge.Log(1, message?.ToString() ?? "null");
        public static void LogError(object message) => RuntimeBridge.Log(0, message?.ToString() ?? "null");
    }

    // ==========================================
    // JERARQUÍA DE OBJETOS ESTILO UNITY
    // ==========================================

    public class Object
    {
        public string name { get; set; } = "GameObject";

        /// <summary>
        /// Clon histórico SOLO gestionado: crea otro wrapper en C# pero
        /// ninguna entidad en la escena del motor (su EntityId queda a
        /// 0). Se conserva por compatibilidad; la instanciación real es
        /// <see cref="GameObject.Spawn"/> /
        /// <see cref="GameObject.CreatePrimitive"/>.
        /// </summary>
        public static T Instantiate<T>(T original) where T : Object
        {
            if (original is GameObject go)
            {
                var clone = new GameObject($"{go.name} (Clone)");
                clone.transform.position = go.transform.position;
                clone.transform.rotation = go.transform.rotation;
                clone.transform.localScale = go.transform.localScale;
                Debug.Log($"[Instantiate] Clonado GameObject '{clone.name}' en Pos={clone.transform.position}");
                return (T)(object)clone;
            }
            return original;
        }

        public static T Instantiate<T>(T original, Vector3 position, Quaternion rotation) where T : Object
        {
            var res = Instantiate(original);
            if (res is GameObject go)
            {
                go.transform.position = position;
                go.transform.rotation = rotation;
            }
            return res;
        }

        // --- Búsqueda de objetos en escena (C2) ---
        // v1: solo se indexan las instancias ENVUELTAS, es decir, los scripts
        // MonoBehaviour vivos de MonoBehaviourRegistry. Los componentes nativos
        // (Transform, etc.) no están indexados: para esos tipos el resultado es
        // null o un array vacío.
        // Por defecto se excluyen las instancias cuyo gameObject no está activo
        // (activeInHierarchy); includeInactive=true las incluye.
        // La búsqueda lee el registro tal cual. No despierta a nadie ni toca el
        // ciclo de vida. Nunca lanza: un tipo nulo o una entrada incompleta dan
        // null o vacío. Los arrays devueltos son siempre copias nuevas, nunca null.

        /// <summary>
        /// Núcleo común: recorre el registro en orden de creación y filtra por tipo
        /// y actividad. Si soloPrimero es true, corta en la primera coincidencia.
        /// </summary>
        private static System.Collections.Generic.List<Object> RecogerEnvueltos(
            System.Type? type, bool includeInactive, bool soloPrimero)
        {
            var resultado = new System.Collections.Generic.List<Object>();
            if (type is null) return resultado;

            foreach (var host in MonoBehaviourRegistry.All)
            {
                var w = host?.Wrapped;
                if (w is null) continue;
                if (!type.IsInstanceOfType(w)) continue;

                if (!includeInactive)
                {
                    var go = w.gameObject;
                    if (go is null || !go.activeInHierarchy) continue;
                }

                resultado.Add(w);
                if (soloPrimero) break;
            }
            return resultado;
        }

        private static Object? PrimeroEnvuelto(System.Type? type, bool includeInactive)
        {
            var lista = RecogerEnvueltos(type, includeInactive, soloPrimero: true);
            return lista.Count > 0 ? lista[0] : null;
        }

        /// <summary>Copia tipada de la lista (array nuevo, nunca null).</summary>
        private static T[] CopiaTipada<T>(System.Collections.Generic.List<Object> lista) where T : Object
        {
            var arr = new T[lista.Count];
            for (int i = 0; i < lista.Count; i++) arr[i] = (T)lista[i];
            return arr;
        }

        /// <summary>
        /// Ordena por EntityId del GameObject. Los empates (varios scripts en el
        /// mismo GameObject) conservan el orden de creación. Las instancias sin
        /// gameObject van al final.
        /// </summary>
        private static void OrdenarPorEntityId(System.Collections.Generic.List<Object> lista)
        {
            var claves = new (ulong id, int orden, Object obj)[lista.Count];
            for (int i = 0; i < lista.Count; i++)
            {
                ulong id = ulong.MaxValue;
                if (lista[i] is MonoBehaviour mb && mb.gameObject is not null)
                    id = mb.gameObject.EntityId;
                claves[i] = (id, i, lista[i]);
            }

            System.Array.Sort(claves, (a, b) =>
            {
                int c = a.id.CompareTo(b.id);
                return c != 0 ? c : a.orden.CompareTo(b.orden);
            });

            for (int i = 0; i < claves.Length; i++) lista[i] = claves[i].obj;
        }

        // FindObjectOfType: primera instancia en orden de creación, o null.

        public static T FindObjectOfType<T>() where T : Object
            => (T)PrimeroEnvuelto(typeof(T), false)!;

        public static T FindObjectOfType<T>(bool includeInactive) where T : Object
            => (T)PrimeroEnvuelto(typeof(T), includeInactive)!;

        public static Object? FindObjectOfType(System.Type type)
            => PrimeroEnvuelto(type, false);

        public static Object? FindObjectOfType(System.Type type, bool includeInactive)
            => PrimeroEnvuelto(type, includeInactive);

        // FindObjectsOfType: todas en orden de creación (array vacío si no hay).

        public static T[] FindObjectsOfType<T>() where T : Object
            => CopiaTipada<T>(RecogerEnvueltos(typeof(T), false, soloPrimero: false));

        public static T[] FindObjectsOfType<T>(bool includeInactive) where T : Object
            => CopiaTipada<T>(RecogerEnvueltos(typeof(T), includeInactive, soloPrimero: false));

        public static Object[] FindObjectsOfType(System.Type type)
            => RecogerEnvueltos(type, false, soloPrimero: false).ToArray();

        public static Object[] FindObjectsOfType(System.Type type, bool includeInactive)
            => RecogerEnvueltos(type, includeInactive, soloPrimero: false).ToArray();

        // FindFirstObjectByType / FindAnyObjectByType: en v1 equivalen a FindObjectOfType.

        public static T FindFirstObjectByType<T>() where T : Object
            => FindObjectOfType<T>(false);

        public static T FindFirstObjectByType<T>(bool includeInactive) where T : Object
            => FindObjectOfType<T>(includeInactive);

        public static Object? FindFirstObjectByType(System.Type type)
            => FindObjectOfType(type, false);

        public static Object? FindFirstObjectByType(System.Type type, bool includeInactive)
            => FindObjectOfType(type, includeInactive);

        public static T FindAnyObjectByType<T>() where T : Object
            => FindObjectOfType<T>(false);

        public static T FindAnyObjectByType<T>(bool includeInactive) where T : Object
            => FindObjectOfType<T>(includeInactive);

        public static Object? FindAnyObjectByType(System.Type type)
            => FindObjectOfType(type, false);

        public static Object? FindAnyObjectByType(System.Type type, bool includeInactive)
            => FindObjectOfType(type, includeInactive);

        // FindObjectsByType: None = orden de creación; InstanceID = por EntityId.
        // Un valor de sortMode desconocido se trata como None.

        public static T[] FindObjectsByType<T>(FindObjectsSortMode sortMode) where T : Object
            => FindObjectsByType<T>(false, sortMode);

        public static T[] FindObjectsByType<T>(bool includeInactive, FindObjectsSortMode sortMode) where T : Object
        {
            var lista = RecogerEnvueltos(typeof(T), includeInactive, soloPrimero: false);
            if (sortMode == FindObjectsSortMode.InstanceID) OrdenarPorEntityId(lista);
            return CopiaTipada<T>(lista);
        }

        /// <summary>
        /// Destruye de verdad (bloque de instanciación): si el objeto
        /// envuelve una entidad del motor (EntityId != 0), la entidad
        /// sale de la escena y su cuerpo y colliders salen del mundo
        /// físico en la misma llamada; sus scripts reciben OnDestroy en
        /// este frame. Antes de este bloque, Destroy solo escribía en el
        /// log. Para un objeto solo-gestionado (EntityId == 0, p. ej. un
        /// clon de Instantiate) sigue sin haber nada nativo que liberar.
        /// </summary>
        public static void Destroy(Object obj)
        {
            if (obj is GameObject go)
            {
                if (go.EntityId != 0 && RuntimeBridge.DestroyEntity(go.EntityId))
                {
                    GameObject.UnregisterEntity(go.EntityId);
                    Debug.Log($"[Destroy] Entidad destruida: '{go.name}' (EntityId: {go.EntityId})");
                    go.EntityId = 0;
                }
                else
                {
                    Debug.Log($"[Destroy] Destruyendo objeto '{go.name}' (EntityId: {go.EntityId})");
                }
            }
        }
    }

    /// <summary>
    /// Orden de los resultados de FindObjectsByType.
    /// None = orden de creación; InstanceID = orden por EntityId del GameObject.
    /// </summary>
    public enum FindObjectsSortMode
    {
        None,
        InstanceID
    }

    /// <summary>
    /// Primitivas integradas del motor: cada valor nombra el modelo del
    /// AssetCache con esa forma y su collider inferido (la esfera choca
    /// como esfera, el cilindro como cilindro...).
    /// </summary>
    public enum PrimitiveType
    {
        Cube,
        Sphere,
        Capsule,
        Cylinder,
        Plane,
    }

    public class Component : Object
    {
        public GameObject gameObject { get; internal set; } = null!;
        public Transform transform => gameObject.transform;
        public ulong EntityId => gameObject.EntityId;

        public T? GetComponent<T>() where T : Component => gameObject.GetComponent<T>();
    }

    public class Behaviour : Component
    {
        private bool _enabled = true;

        public bool enabled
        {
            get => _enabled;
            set
            {
                if (_enabled == value)
                    return;
                _enabled = value;
                OnEnabledChanged(value);
            }
        }

        /// <summary>Aviso de cambio efectivo de <see cref="enabled"/> (MonoBehaviour lo reenvía a su huésped, D13).</summary>
        internal virtual void OnEnabledChanged(bool value) { }
    }

    public class Transform : Component
    {
        // Durante el tick (bloque «FFI por lotes») estas tres
        // propiedades trabajan sobre la caché del frame: leer y
        // escribir no cruza la frontera. Fuera del tick, o si la
        // entidad no está en la caché (p. ej. creada a mitad de
        // frame), se usa la ruta FFI clásica por entidad, con los
        // mismos valores de reserva de siempre.
        public Vector3 position
        {
            get
            {
                if (TransformCache.TryRead(EntityId, out TransformData tc))
                    return tc.Position;
                if (RuntimeBridge.GetTransform(EntityId, out TransformData t) == 0)
                    return t.Position;
                return Vector3.zero;
            }
            set
            {
                if (TransformCache.TryRead(EntityId, out TransformData tc))
                {
                    tc.Position = value;
                    TransformCache.Write(EntityId, tc);
                    return;
                }
                if (RuntimeBridge.GetTransform(EntityId, out TransformData t) == 0)
                {
                    t.Position = value;
                    RuntimeBridge.SetTransform(EntityId, ref t);
                }
            }
        }

        public Quaternion rotation
        {
            get
            {
                if (TransformCache.TryRead(EntityId, out TransformData tc))
                    return tc.Rotation;
                if (RuntimeBridge.GetTransform(EntityId, out TransformData t) == 0)
                    return t.Rotation;
                return Quaternion.identity;
            }
            set
            {
                if (TransformCache.TryRead(EntityId, out TransformData tc))
                {
                    tc.Rotation = value;
                    TransformCache.Write(EntityId, tc);
                    return;
                }
                if (RuntimeBridge.GetTransform(EntityId, out TransformData t) == 0)
                {
                    t.Rotation = value;
                    RuntimeBridge.SetTransform(EntityId, ref t);
                }
            }
        }

        public Vector3 localScale
        {
            get
            {
                if (TransformCache.TryRead(EntityId, out TransformData tc))
                    return tc.Scale;
                if (RuntimeBridge.GetTransform(EntityId, out TransformData t) == 0)
                    return t.Scale;
                return Vector3.one;
            }
            set
            {
                if (TransformCache.TryRead(EntityId, out TransformData tc))
                {
                    tc.Scale = value;
                    TransformCache.Write(EntityId, tc);
                    return;
                }
                if (RuntimeBridge.GetTransform(EntityId, out TransformData t) == 0)
                {
                    t.Scale = value;
                    RuntimeBridge.SetTransform(EntityId, ref t);
                }
            }
        }

        public Vector3 forward => rotation * Vector3.forward;
        public Vector3 up => rotation * Vector3.up;
        public Vector3 right => rotation * Vector3.right;

        public void Translate(Vector3 translation)
        {
            position += translation;
        }

        public void Rotate(Vector3 eulers)
        {
            rotation = rotation * Quaternion.Euler(eulers.x, eulers.y, eulers.z);
        }
    }

    public class GameObject : Object
    {
        public ulong EntityId { get; internal set; }
        public Transform transform { get; }
        public string tag { get; set; } = "Untagged";
        public bool activeSelf { get; private set; } = true;

        // v1: sin propagación de actividad por la jerarquía de padres,
        // activeInHierarchy equivale a activeSelf.
        public bool activeInHierarchy => activeSelf;

        private readonly List<Component> _components = new();
        private static readonly Dictionary<ulong, GameObject> _entityMap = new();

        // --- Búsqueda de GameObjects (C2) ---
        // Solo se consideran wrappers ACTIVOS (activeInHierarchy), como en Unity.
        // Comparación de name y tag exacta (ordinal). Find no interpreta rutas con '/'.
        // Para que el resultado sea determinista ante un Dictionary sin orden
        // garantizado, se elige o se ordena por EntityId ascendente.
        // Nunca lanza: name o tag nulos dan null o un array vacío.

        private static GameObject? PrimeroActivo(System.Func<GameObject, bool> criterio)
        {
            GameObject? mejor = null;
            ulong mejorId = 0;
            foreach (var par in _entityMap)
            {
                var go = par.Value;
                if (go is null || !go.activeInHierarchy) continue;
                if (!criterio(go)) continue;
                if (mejor is null || par.Key < mejorId)
                {
                    mejor = go;
                    mejorId = par.Key;
                }
            }
            return mejor;
        }

        public static GameObject? Find(string name)
        {
            if (name is null) return null;
            return PrimeroActivo(go => string.Equals(go.name, name, System.StringComparison.Ordinal));
        }

        public static GameObject? FindWithTag(string tag)
        {
            if (tag is null) return null;
            return PrimeroActivo(go => string.Equals(go.tag, tag, System.StringComparison.Ordinal));
        }

        public static GameObject[] FindGameObjectsWithTag(string tag)
        {
            if (tag is null) return System.Array.Empty<GameObject>();

            var encontrados = new System.Collections.Generic.List<(ulong id, GameObject go)>();
            foreach (var par in _entityMap)
            {
                var go = par.Value;
                if (go is null || !go.activeInHierarchy) continue;
                if (!string.Equals(go.tag, tag, System.StringComparison.Ordinal)) continue;
                encontrados.Add((par.Key, go));
            }

            encontrados.Sort((a, b) => a.id.CompareTo(b.id));

            var arr = new GameObject[encontrados.Count];
            for (int i = 0; i < arr.Length; i++) arr[i] = encontrados[i].go;
            return arr;
        }

        public GameObject(string name = "GameObject", ulong entityId = 0)
        {
            this.name = name;
            this.EntityId = entityId;
            transform = new Transform { gameObject = this, name = "Transform" };
            _components.Add(transform);
            if (entityId != 0)
            {
                _entityMap[entityId] = this;
            }
        }

        public static GameObject GetOrCreate(ulong entityId, string name = "Entity")
        {
            if (_entityMap.TryGetValue(entityId, out var existing))
                return existing;
            var go = new GameObject(name, entityId);
            return go;
        }

        /// <summary>
        /// Vacía el mapa id -> wrapper (MonoBehaviourHost.ResetAll, al
        /// sustituir el ensamblado del proyecto).
        /// </summary>
        internal static void ClearEntityMap()
        {
            _entityMap.Clear();
        }

        /// <summary>
        /// Quita a la entidad del mapa id -> wrapper (al destruirla):
        /// sin esto, el mapa retendría un wrapper por cada entidad
        /// creada en la sesión y un EntityId reutilizado resucitaría el
        /// wrapper viejo.
        /// </summary>
        internal static void UnregisterEntity(ulong entityId)
        {
            _entityMap.Remove(entityId);
        }

        /// <summary>
        /// Nombre de modelo integrado de cada primitiva (la clave del
        /// AssetCache y la que decide el collider inferido).
        /// </summary>
        internal static string ModelNameFor(PrimitiveType type) => type switch
        {
            PrimitiveType.Sphere => "sphere",
            PrimitiveType.Capsule => "capsule",
            PrimitiveType.Cylinder => "cylinder",
            PrimitiveType.Plane => "plane",
            _ => "cube",
        };

        /// <summary>
        /// Crea una entidad REAL en la escena viva (bloque de
        /// instanciación), la vía que le faltaba a
        /// <see cref="Object.Instantiate{T}"/>: aquel sigue clonando
        /// solo el wrapper gestionado (comportamiento histórico), pero
        /// Spawn llega al motor — malla, cuerpo físico con la forma
        /// inferida de la malla, y script si se pide (su instancia nace
        /// antes del siguiente Update). La transformada es la LOCAL de
        /// la entidad (relativa a <paramref name="parentId"/>; 0 =
        /// raíz). Devuelve null si el motor rechazó el spawn (ver el
        /// log). Una rotación toda a cero se lee como identidad y una
        /// escala toda a cero como uno, para que los valores por
        /// defecto de C# sean utilizables tal cual.
        /// </summary>
        public static GameObject? Spawn(
            string model,
            Vector3 position,
            Quaternion rotation,
            Vector3 scale,
            SpawnPhysicsKind physics = SpawnPhysicsKind.None,
            string name = "Entity",
            Color? color = null,
            string? scriptClass = null,
            ulong parentId = 0)
        {
            if (rotation.x == 0f && rotation.y == 0f && rotation.z == 0f && rotation.w == 0f)
                rotation = Quaternion.identity;
            if (scale.x == 0f && scale.y == 0f && scale.z == 0f)
                scale = Vector3.one;
            InteropVector3? nativeColor = color.HasValue
                ? new InteropVector3(color.Value.r, color.Value.g, color.Value.b)
                : null;
            ulong id = RuntimeBridge.SpawnEntity(
                name, model, scriptClass, parentId,
                position, rotation, scale, physics, nativeColor);
            if (id == 0)
            {
                Debug.LogError($"[Spawn] El motor rechazó crear '{name}' (modelo '{model}').");
                return null;
            }
            // D10: si un huésped ya enlazó esta entidad, se reutiliza su wrapper.
            return GetOrCreate(id, name);
        }

        /// <summary>
        /// Instancia un Prefab (<c>.prefab.json</c>) en la escena viva,
        /// por nombre ("Gold_Coin") o por ruta explícita. Devuelve null
        /// si el motor rechazó la instanciación (ver el log), igual que
        /// <see cref="Spawn"/>.
        /// </summary>
        public static GameObject? InstantiatePrefab(string name, Vector3 position = default)
        {
            ulong id = RuntimeBridge.InstantiatePrefab(name, position);
            if (id == 0)
            {
                Debug.LogError($"[InstantiatePrefab] El motor rechazó instanciar '{name}'.");
                return null;
            }
            // D10: si un huésped ya enlazó esta entidad, se reutiliza su wrapper.
            return GetOrCreate(id, name);
        }


        /// <summary>
        /// Como el CreatePrimitive de Unity: crea la primitiva en la
        /// escena, con su collider — aquí un cuerpo ESTÁTICO con la
        /// forma inferida (el suelo y los muros de un generador son el
        /// caso típico). Para piezas dinámicas o sin física, usa
        /// <see cref="Spawn"/> con el nombre de modelo de la primitiva
        /// ("sphere", "capsule", "cylinder", "plane", "cube").
        /// </summary>
        public static GameObject? CreatePrimitive(
            PrimitiveType type,
            Vector3 position = default,
            Quaternion rotation = default,
            Vector3 scale = default)
        {
            return Spawn(
                ModelNameFor(type), position, rotation, scale,
                SpawnPhysicsKind.Static, type.ToString());
        }

        /// <summary>
        /// Activa o desactiva la entidad en el motor: la inactiva no se
        /// dibuja y su cuerpo sale del mundo físico; al reactivarla el
        /// cuerpo se recrea desde su PhysicsData. Es la operación de
        /// préstamo de los pools. Sin entidad (EntityId == 0) solo se
        /// actualiza la bandera local, como antes.
        /// </summary>
        public void SetActive(bool active)
        {
            bool changed = activeSelf != active;
            activeSelf = active;
            if (EntityId != 0)
            {
                RuntimeBridge.SetEntityActive(EntityId, active);
                // D13: OnEnable/OnDisable de los huéspedes, solo si el estado cambió.
                if (changed)
                    GlassEngine.Core.Hosting.MonoBehaviourRegistry.NotifyActiveChanged(EntityId, active);
            }
        }

        public T AddComponent<T>() where T : Component, new()
        {
            var comp = new T { gameObject = this, name = typeof(T).Name };
            _components.Add(comp);
            return comp;
        }

        /// <summary>Enlaza un componente ya instanciado (el MonoBehaviour de un huésped).</summary>
        internal void AddComponentInternal(Component c)
        {
            if (!_components.Contains(c))
                _components.Add(c);
            c.gameObject = this;
        }

        internal void RemoveComponentInternal(Component c)
        {
            _components.Remove(c);
        }

        /// <summary>
        /// Sobre de ApplyScriptData (D12): name y tag solo se escriben si
        /// el wrapper conserva su valor por defecto; active se aplica con
        /// SetActive si difiere del actual.
        /// </summary>
        internal void ApplyHostInit(string? name, string? tag, bool? active)
        {
            if (!string.IsNullOrEmpty(name)
                && (string.IsNullOrEmpty(this.name) || this.name == "GameObject" || this.name == "Entity"))
            {
                this.name = name;
            }

            if (!string.IsNullOrEmpty(tag)
                && (string.IsNullOrEmpty(this.tag) || this.tag == "Untagged"))
            {
                this.tag = tag;
            }

            if (active.HasValue && active.Value != activeSelf)
                SetActive(active.Value);
        }

        public T? GetComponent<T>() where T : Component
        {
            foreach (var c in _components)
            {
                if (c is T match) return match;
            }
            return null;
        }
    }

    public class MonoBehaviour : Behaviour
        {
            public virtual void Awake() { }
            public virtual void Start() { }
            public virtual void Update() { }
            public virtual void FixedUpdate() { }
            public virtual void LateUpdate() { }
            public virtual void OnDestroy() { }

            /// <summary>Huésped que ticka a este script (null si no está alojado).</summary>
            internal GlassEngine.Core.Hosting.MonoBehaviourHost? Host { get; set; }

            internal override void OnEnabledChanged(bool value) => Host?.NotifyEnabledChanged(value);

            // ------------------------------------------
            // Corrutinas (§4). enabled = false no las detiene, como en Unity;
            // OnDestroy y SetActive(false) hacen StopAll desde el huésped.
            // Los StartCoroutine devuelven null si no se puede arrancar, pero se
            // anotan no anulables como en Unity.
            // ------------------------------------------

            public Coroutine StartCoroutine(System.Collections.IEnumerator routine)
            {
                if (routine is null)
                {
                    Debug.LogError("StartCoroutine: routine is null");
                    return null!;
                }

                var host = HostParaCorrutinas();
                if (host is null)
                    return null!;

                return host.Coroutines.Start(routine, null)!;
            }

            public Coroutine StartCoroutine(string methodName) => StartCoroutine(methodName, null);

            public Coroutine StartCoroutine(string methodName, object? value)
            {
                if (string.IsNullOrEmpty(methodName))
                {
                    Debug.LogError("StartCoroutine: methodName is null or empty");
                    return null!;
                }

                // Se comprueba antes de invocar: con el objeto inactivo no corre nada.
                var host = HostParaCorrutinas();
                if (host is null)
                    return null!;

                var metodo = BuscarCorrutinaPorNombre(methodName, value);
                if (metodo is null)
                {
                    Debug.LogError($"Coroutine '{methodName}' couldn't be started");
                    return null!;
                }

                System.Collections.IEnumerator? rutina;
                try
                {
                    object?[]? args = metodo.GetParameters().Length == 0 ? null : new[] { value };
                    rutina = metodo.Invoke(this, args) as System.Collections.IEnumerator;
                }
                catch (System.Exception ex)
                {
                    // Ninguna excepción cruza la FFI: un método no iterador puede lanzar al invocarse.
                    var real = (ex as System.Reflection.TargetInvocationException)?.InnerException ?? ex;
                    Debug.LogError($"Coroutine '{methodName}' couldn't be started:\n{real}");
                    return null!;
                }

                if (rutina is null)
                {
                    Debug.LogError($"Coroutine '{methodName}' couldn't be started");
                    return null!;
                }

                return host.Coroutines.Start(rutina, methodName)!;
            }

            /// <summary>Para la rutina cuyo Root es este enumerador (solo las de este script).</summary>
            public void StopCoroutine(System.Collections.IEnumerator routine)
            {
                if (routine is null)
                    return;
                Host?.Coroutines.StopByRoot(routine);
            }

            /// <summary>Para la rutina de este handle si pertenece a este script.</summary>
            public void StopCoroutine(Coroutine routine)
            {
                if (routine is null)
                    return;
                Host?.Coroutines.Stop(routine);
            }

            /// <summary>Para solo las rutinas arrancadas con StartCoroutine(string), como en Unity.</summary>
            public void StopCoroutine(string methodName)
            {
                if (methodName is null)
                    return;
                Host?.Coroutines.StopByName(methodName);
            }

            public void StopAllCoroutines() => Host?.Coroutines.StopAll();

            /// <summary>Host válido para arrancar corrutinas; loguea y devuelve null si no lo hay.</summary>
            private GlassEngine.Core.Hosting.MonoBehaviourHost? HostParaCorrutinas()
            {
                var host = Host;
                if (host is null)
                {
                    Debug.LogError(
                        $"Coroutine couldn't be started because the script '{GetType().Name}' is not hosted by the engine");
                    return null;
                }

                if (!gameObject.activeInHierarchy)
                {
                    Debug.LogError(
                        $"Coroutine couldn't be started because the game object '{gameObject.name}' is inactive!");
                    return null;
                }

                return host;
            }

            /// <summary>
            /// Método de instancia (de cualquier visibilidad, en la clase o sus
            /// bases) que devuelve IEnumerator y tiene 0 parámetros, o 1 si el
            /// valor no es nulo y su tipo es asignable. Con valor se prefiere el
            /// de 1 parámetro.
            /// </summary>
            private System.Reflection.MethodInfo? BuscarCorrutinaPorNombre(string nombre, object? valor)
            {
                const System.Reflection.BindingFlags flags =
                    System.Reflection.BindingFlags.Instance |
                    System.Reflection.BindingFlags.Public |
                    System.Reflection.BindingFlags.NonPublic |
                    System.Reflection.BindingFlags.DeclaredOnly;

                System.Reflection.MethodInfo? sinParametros = null;
                for (var tipo = GetType(); tipo is not null && tipo != typeof(MonoBehaviour); tipo = tipo.BaseType)
                {
                    foreach (var m in tipo.GetMethods(flags))
                    {
                        if (m.Name != nombre ||
                            m.IsGenericMethodDefinition ||
                            m.ReturnType != typeof(System.Collections.IEnumerator))
                            continue;

                        var ps = m.GetParameters();
                        if (ps.Length == 0)
                            sinParametros ??= m;
                        else if (ps.Length == 1 && valor is not null && ps[0].ParameterType.IsInstanceOfType(valor))
                            return m;
                    }
                }

                return sinParametros;
            }
        }

    // COMPONENTES DE FÍSICA Y AUDIO COMPATIBLES
    // ==========================================

    public class Rigidbody : Component
    {
        public void AddForce(Vector3 force)
        {
            RuntimeBridge.ApplyForce(EntityId, force);
        }

        public void AddImpulse(Vector3 impulse)
        {
            RuntimeBridge.ApplyImpulse(EntityId, impulse);
        }

        public Vector3 velocity
        {
            get
            {
                if (RuntimeBridge.GetLinearVelocity(EntityId, out var vel))
                    return vel;
                return Vector3.zero;
            }
            set
            {
                RuntimeBridge.SetLinearVelocity(EntityId, value);
            }
        }
    }

    /// <summary>Collider de caja. Defectos de Unity: centro cero, tamaño uno.</summary>
    public class BoxCollider : Collider
    {
        public Vector3 center { get; set; } = Vector3.zero;
        public Vector3 size { get; set; } = Vector3.one;
    }

    public class AudioSource : Component
    {
        public string clip { get; set; } = "";
        public float volume { get; set; } = 1.0f;
        public float pitch { get; set; } = 1.0f;
        public bool loop { get; set; } = false;

        private ulong _handle;
        private bool _paused;

        public bool isPlaying => _handle != 0 && !_paused;

        public void Play()
        {
            // En esta v1 del motor no hay reanudación desde el punto de reinicio:
            // volver a llamar a Play() sobre una fuente ya activa la detiene y
            // la arranca de nuevo desde el principio, como Unity al re-disparar Play.
            if (_handle != 0)
            {
                Stop();
            }

            if (!string.IsNullOrEmpty(clip))
            {
                _handle = RuntimeBridge.PlayAudio3D(clip, transform.position, volume, pitch, loop);
                _paused = false;
            }
        }

        public void Pause()
        {
            if (_handle != 0 && !_paused)
            {
                RuntimeBridge.PauseAudio(_handle);
                _paused = true;
            }
        }

        public void UnPause()
        {
            if (_handle != 0 && _paused)
            {
                RuntimeBridge.ResumeAudio(_handle);
                _paused = false;
            }
        }

        public void Stop()
        {
            if (_handle != 0)
            {
                RuntimeBridge.StopAudio(_handle);
                _handle = 0;
                _paused = false;
            }
        }

        public static void PlayClipAtPoint(string clipName, Vector3 position, float volume = 1.0f)
        {
            RuntimeBridge.PlayAudio3D(clipName, position, volume, 1.0f, false);
        }
    }
    /// <summary>
    /// Reproductor de animaciones con piel (bloque de skinning), espejo
    /// mínimo del <c>Animator</c> de Unity. La pose la evalúa el motor en
    /// Rust cada frame (muestreo del clip, matrices de articulación,
    /// deformación en GPU); desde C# solo se pide el clip por nombre,
    /// igual que en Unity. Sin llamadas, la entidad con componente
    /// Animator en la escena ya reproduce su clip por defecto sola.
    /// </summary>
    public class Animator : Component
    {
        /// <summary>Clip aceptado por el último <see cref="Play"/> ("" = ninguno).</summary>
        public string CurrentClip { get; private set; } = "";

        /// <summary>True si el último <see cref="Play"/> quedó aceptado por el motor.</summary>
        public bool IsPlaying { get; private set; }

        /// <summary>
        /// Reproduce el clip indicado en bucle desde el principio. Si el
        /// modelo no tiene un clip con ese nombre, el motor usa el primer
        /// clip del modelo.
        /// </summary>
        public void Play(string stateName)
        {
            int rc = RuntimeBridge.AnimatorPlay(EntityId, stateName);
            if (rc == 0)
            {
                CurrentClip = stateName;
                IsPlaying = true;
            }
            else
            {
                Debug.LogError($"[Animator] Play('{stateName}') devolvió {rc} en la entidad {EntityId}.");
            }
        }
        /// <summary>Fija un parámetro bool del controlador (true → 1, false → 0).</summary>
        public void SetBool(string name, bool value)
        {
            int rc = RuntimeBridge.AnimatorSetParam(EntityId, name, 0, value ? 1f : 0f);
            if (rc != 0)
                Debug.LogError($"[Animator] SetBool('{name}') devolvió {rc} en la entidad {EntityId}.");
        }

        /// <summary>Fija un parámetro float del controlador.</summary>
        public void SetFloat(string name, float value)
        {
            int rc = RuntimeBridge.AnimatorSetParam(EntityId, name, 1, value);
            if (rc != 0)
                Debug.LogError($"[Animator] SetFloat('{name}') devolvió {rc} en la entidad {EntityId}.");
        }

        /// <summary>Dispara un trigger del controlador (se consume al dispararse).</summary>
        public void SetTrigger(string name)
        {
            int rc = RuntimeBridge.AnimatorSetParam(EntityId, name, 2, 1f);
            if (rc != 0)
                Debug.LogError($"[Animator] SetTrigger('{name}') devolvió {rc} en la entidad {EntityId}.");
        }

        /// <summary>
        /// Salta al estado/clip indicado con un fundido de <paramref name="fadeSeconds"/>.
        /// Con controlador lo resuelve la FSM; sin controlador es un crossfade libre.
        /// </summary>
        public void CrossFade(string stateName, float fadeSeconds)
        {
            int rc = RuntimeBridge.AnimatorCrossFade(EntityId, stateName, fadeSeconds);
            if (rc == 0)
            {
                CurrentClip = stateName;
                IsPlaying = true;
            }
            else
            {
                Debug.LogError($"[Animator] CrossFade('{stateName}') devolvió {rc} en la entidad {EntityId}.");
            }
        }

        /// <summary>
        /// Estado actual de la FSM. Sin controlador devuelve el clip en curso;
        /// si el puente no conoce nada, el clip aceptado por el último Play.
        /// </summary>
        public string GetCurrentState()
        {
            string state = RuntimeBridge.AnimatorCurrentState(EntityId);
            if (!string.IsNullOrEmpty(state))
                return state;
            return CurrentClip;
        }
    }

    /// <summary>
    /// Publicación de valores globales para los shaders de grafo. En esta
    /// versión el motor expone UN solo global: el punto de interacción
    /// que leen los grafos con el nodo «Punto de interacción» — la
    /// posición de un intruso (una barca, el jugador) y la FORMA de su
    /// huella: un disco de radio dado con
    /// <see cref="SetInteractionPoint"/> o la caja de su collider en
    /// planta con <see cref="SetInteractionBox"/> — para que materiales
    /// como el agua reaccionen a su paso abrazando su forma real, no un
    /// círculo genérico. Es el espíritu del Shader.SetGlobalVector del
    /// tutorial de referencia, con un nombre propio y honesto: no hay
    /// más globales que este.
    /// </summary>
    public static class Shader
    {
        /// <summary>
        /// Publica el punto de interacción de este frame con huella
        /// ESFÉRICA: un disco del radio dado alrededor de la posición.
        /// Hay que llamarlo cada frame mientras el objeto interactúa
        /// (normalmente desde su Update): al cambiar de escena el motor
        /// lo limpia solo; si el publicador desaparece sin limpiar, el
        /// último valor publicado queda fijado. Si el objeto tiene forma
        /// de caja, publica su huella con
        /// <see cref="SetInteractionBox"/> en su lugar: el último que
        /// publica define la forma activa.
        /// </summary>
        public static void SetInteractionPoint(Vector3 position, float radius)
        {
            RuntimeBridge.SetInteractionPoint(position.x, position.y, position.z, radius);
        }

        /// <summary>
        /// Publica la interacción de este frame con huella de CAJA en
        /// planta (XZ): la del collider del objeto proyectado al suelo.
        /// <paramref name="halfX"/> y <paramref name="halfZ"/> son los
        /// semiejes de la caja en su marco local (halfX a lo largo de su
        /// eje X, halfZ a lo largo de su eje Z) y
        /// <paramref name="yawRadians"/> su rotación Y en radianes. La
        /// misma disciplina que la esférica: cada frame desde el Update,
        /// el motor lo limpia al cambiar de escena y el último que
        /// publica define la forma activa.
        /// </summary>
        public static void SetInteractionBox(Vector3 center, float halfX, float halfZ, float yawRadians)
        {
            RuntimeBridge.SetInteractionBox(center.x, center.y, center.z, halfX, halfZ, yawRadians);
        }

        /// <summary>
        /// Deja de publicar: el punto vuelve a su valor inactivo y los
        /// grafos que lo leen se comportan como si nadie interactuara.
        /// </summary>
        public static void ClearInteractionPoint()
        {
            RuntimeBridge.ClearInteractionPoint();
        }
    }

    public struct Rect
    {
        public float x;
        public float y;
        public float width;
        public float height;

        public Rect(float x, float y, float width, float height)
        {
            this.x = x;
            this.y = y;
            this.width = width;
            this.height = height;
        }

        public static Rect zero => new(0f, 0f, 0f, 0f);
        public static Rect one => new(0f, 0f, 1f, 1f);
    }

    public class Camera : Component
    {
        public float fieldOfView { get; set; } = 60.0f;
        public float nearClipPlane { get; set; } = 0.1f;
        public float farClipPlane { get; set; } = 1000.0f;
        public float depth { get; set; } = 0.0f;
        public Rect rect { get; set; } = Rect.one;
        public bool is_main { get; set; } = false;
        public static Camera? main { get; set; }
        public static readonly List<Camera> allCameras = new();

        public Camera()
        {
            allCameras.Add(this);
            if (main == null) main = this;
        }
    }

public enum LightType
    {
        Directional = 0,
        Point = 1,
        Spot = 2,
        Area = 3
    }

    public class Light : Component
    {
        private Color _color = Color.white;
        private float _intensity = 1.0f;
        private LightType _type = LightType.Directional;
        private float _range = 10.0f;
        private float _spotAngle = 45.0f;
        private float _innerSpotAngle = 35.0f;

        // El constructor NO llama al puente: la instancia C# puede
        // crearse antes de que la entidad esté viva en la escena (o con
        // el puente aún sin enlazar). Los valores locales son la verdad
        // hasta el primer setter.
        public Light() { }

        public Color color
        {
            get => _color;
            set
            {
                _color = value;
                RuntimeBridge.LightSetColor(EntityId, value.r, value.g, value.b);
            }
        }

        public float intensity
        {
            get => _intensity;
            set
            {
                _intensity = value;
                RuntimeBridge.LightSetIntensity(EntityId, value);
            }
        }

        /// <summary>
        /// Lee la intensidad del componente de luz REAL de la escena a través
        /// del puente. El getter <see cref="intensity"/> devuelve el campo
        /// local, que es la verdad hasta el primer setter; este método existe
        /// para que los scripts lean el valor autorado al arrancar, antes de
        /// escribir nada. Si el puente no responde, deja el valor local en
        /// <paramref name="value"/> y devuelve false.
        /// </summary>
        public bool TryGetEngineIntensity(out float value)
        {
            if (RuntimeBridge.LightGetIntensity(EntityId, out float delMotor) == 0)
            {
                value = delMotor;
                return true;
            }
            value = _intensity;
            return false;
        }

        public LightType type
        {
            get => _type;
            set
            {
                _type = value;
                RuntimeBridge.LightSetType(EntityId, (int)value);
            }
        }

        public float range
        {
            get => _range;
            set
            {
                _range = value;
                RuntimeBridge.LightSetRange(EntityId, value);
            }
        }

        public float spotAngle
        {
            get => _spotAngle;
            set
            {
                _spotAngle = value;
                RuntimeBridge.LightSetSpotAngles(EntityId, _spotAngle, _innerSpotAngle);
            }
        }

        public float innerSpotAngle
        {
            get => _innerSpotAngle;
            set
            {
                _innerSpotAngle = value;
                RuntimeBridge.LightSetSpotAngles(EntityId, _spotAngle, _innerSpotAngle);
            }
        }
    }
public class Text : Component
{
    private string _text = "";
    private float _fontSize = 18f;
    private Color _color = Color.white;

    public Text() { }

    public string text
    {
        get => _text;
        set
        {
            _text = value;
            RuntimeBridge.UiSetText(EntityId, value);
        }
    }

    public float fontSize
    {
        get => _fontSize;
        set
        {
            _fontSize = value;
            RuntimeBridge.UiSetFontSize(EntityId, value);
        }
    }

    public Color color
    {
        get => _color;
        set
        {
            _color = value;
            RuntimeBridge.UiSetTextColor(EntityId, value.r, value.g, value.b, value.a);
        }
    }

        // --- Propiedades de estilo V4d ---
        private float _opacity = 1f;
        public float opacity
        {
            get => _opacity;
            set
            {
                _opacity = value;
                RuntimeBridge.UiSetOpacity(EntityId, value);
            }
        }

        private string _overflow = "visible";
        public string overflow
        {
            get => _overflow;
            set
            {
                _overflow = value;
                RuntimeBridge.UiSetOverflow(EntityId, value);
            }
        }

        private float _letterSpacing;
        public float letterSpacing
        {
            get => _letterSpacing;
            set
            {
                _letterSpacing = value;
                RuntimeBridge.UiSetLetterSpacing(EntityId, value);
            }
        }
}

public class Image : Component
{
    private string _sprite = "";
    private Color _color = Color.white;

    public Image() { }

    public string sprite
    {
        get => _sprite;
        set
        {
            _sprite = value;
            RuntimeBridge.UiSetImageSprite(EntityId, value);
        }
    }

    public Color color
    {
        get => _color;
        set
        {
            _color = value;
            RuntimeBridge.UiSetImageColor(EntityId, value.r, value.g, value.b, value.a);
        }
    }

        // --- Propiedades de estilo V4d ---
        private Color _backgroundColor = Color.white;
        public Color backgroundColor
        {
            get => _backgroundColor;
            set
            {
                _backgroundColor = value;
                RuntimeBridge.UiSetBackgroundColor(EntityId, value.r, value.g, value.b, value.a);
            }
        }

        private float _borderRadiusTopLeft;
        public float borderRadiusTopLeft
        {
            get => _borderRadiusTopLeft;
            set
            {
                _borderRadiusTopLeft = value;
                RuntimeBridge.UiSetBorderRadius(EntityId, _borderRadiusTopLeft, _borderRadiusTopRight, _borderRadiusBottomRight, _borderRadiusBottomLeft);
            }
        }

        private float _borderRadiusTopRight;
        public float borderRadiusTopRight
        {
            get => _borderRadiusTopRight;
            set
            {
                _borderRadiusTopRight = value;
                RuntimeBridge.UiSetBorderRadius(EntityId, _borderRadiusTopLeft, _borderRadiusTopRight, _borderRadiusBottomRight, _borderRadiusBottomLeft);
            }
        }

        private float _borderRadiusBottomRight;
        public float borderRadiusBottomRight
        {
            get => _borderRadiusBottomRight;
            set
            {
                _borderRadiusBottomRight = value;
                RuntimeBridge.UiSetBorderRadius(EntityId, _borderRadiusTopLeft, _borderRadiusTopRight, _borderRadiusBottomRight, _borderRadiusBottomLeft);
            }
        }

        private float _borderRadiusBottomLeft;
        public float borderRadiusBottomLeft
        {
            get => _borderRadiusBottomLeft;
            set
            {
                _borderRadiusBottomLeft = value;
                RuntimeBridge.UiSetBorderRadius(EntityId, _borderRadiusTopLeft, _borderRadiusTopRight, _borderRadiusBottomRight, _borderRadiusBottomLeft);
            }
        }

        private float _borderWidth;
        public float borderWidth
        {
            get => _borderWidth;
            set
            {
                _borderWidth = value;
                RuntimeBridge.UiSetBorder(EntityId, _borderWidth, _borderColor.r, _borderColor.g, _borderColor.b, _borderColor.a);
            }
        }

        private Color _borderColor = Color.white;
        public Color borderColor
        {
            get => _borderColor;
            set
            {
                _borderColor = value;
                RuntimeBridge.UiSetBorder(EntityId, _borderWidth, _borderColor.r, _borderColor.g, _borderColor.b, _borderColor.a);
            }
        }

        private float _opacity = 1f;
        public float opacity
        {
            get => _opacity;
            set
            {
                _opacity = value;
                RuntimeBridge.UiSetOpacity(EntityId, value);
            }
        }
}


public class Button : Component
{
    private Color _color = new Color(0.25f, 0.55f, 0.85f, 1f);

    public Button() { }

    public Color color
    {
        get => _color;
        set
        {
            _color = value;
            RuntimeBridge.UiSetButtonColor(EntityId, value.r, value.g, value.b, value.a);
        }
    }

        // --- Propiedades de estilo V4d ---
        private float _opacity = 1f;
        public float opacity
        {
            get => _opacity;
            set
            {
                _opacity = value;
                RuntimeBridge.UiSetOpacity(EntityId, value);
            }
        }
}

/// <summary>
/// Contexto del último gesto de drag &amp; drop del Canvas: qué
/// entidad se arrastró y sobre qué casilla se soltó (0 = en
/// vacío). Los métodos on_drop/on_drag_end de los scripts lo
/// consultan para saber la pareja del gesto.
/// </summary>
public static class DragDrop
{
    public static ulong DraggedEntityId => RuntimeBridge.UiGetDraggedId();
    public static ulong DropTargetEntityId => RuntimeBridge.UiGetDropTargetId();
}

    // ==========================================
    // COMPONENTES DE UI ESTILO UNITY (CANVAS, RECTTRANSFORM)
    // ==========================================

    public class RectTransform : Transform
    {
        private Vector2 _anchoredPosition = Vector2.zero;
        private Vector2 _sizeDelta = new Vector2(100f, 100f);

        // anchoredPosition y sizeDelta escriben en el motor por el
        // puente de UI dinámica: el lote del Canvas se reconstruye
        // cada frame y el cambio se ve al frame siguiente. Anclas y
        // pivote: desde V4d también escriben en el motor por el puente.
        public Vector2 anchoredPosition
        {
            get => _anchoredPosition;
            set
            {
                _anchoredPosition = value;
                RuntimeBridge.UiSetAnchoredPos(EntityId, value.x, value.y);
            }
        }
        public Vector2 sizeDelta
        {
            get => _sizeDelta;
            set
            {
                _sizeDelta = value;
                RuntimeBridge.UiSetSizeDelta(EntityId, value.x, value.y);
            }
        }
        private Vector2 _anchorMin = new Vector2(0.5f, 0.5f);
        public Vector2 anchorMin
        {
            get => _anchorMin;
            set
            {
                _anchorMin = value;
                RuntimeBridge.UiSetAnchorMin(EntityId, value.x, value.y);
            }
        }

        private Vector2 _anchorMax = new Vector2(0.5f, 0.5f);
        public Vector2 anchorMax
        {
            get => _anchorMax;
            set
            {
                _anchorMax = value;
                RuntimeBridge.UiSetAnchorMax(EntityId, value.x, value.y);
            }
        }

        private Vector2 _pivot = new Vector2(0.5f, 0.5f);
        public Vector2 pivot
        {
            get => _pivot;
            set
            {
                _pivot = value;
                RuntimeBridge.UiSetPivot(EntityId, value.x, value.y);
            }
        }
    }

    public class Canvas : Component
    {
        public enum RenderMode { ScreenSpaceOverlay, ScreenSpaceCamera, WorldSpace }
        public RenderMode renderMode { get; set; } = RenderMode.ScreenSpaceOverlay;
    }
}


namespace UnityEngine.SceneManagement
{
    /// <summary>
    /// API de escenas compatible con Unity sobre el SceneManager nativo del motor.
    /// La carga es diferida: <see cref="LoadScene"/> retorna de inmediato y la
    /// escena cambia al final del frame, nunca a mitad de el.
    /// </summary>
    public static class SceneManager
    {
        /// <summary>Codigos devueltos por el motor (espejo de bindings.rs).</summary>
        private const int Ok = 0;
        private const int NotFound = -1;
        private const int NullPtr = -2;
        private const int BridgeUnavailable = -4;

        /// <summary>
        /// Encola la carga de <paramref name="sceneName"/>.
        /// </summary>
        /// <returns>
        /// <c>true</c> si la peticion quedo encolada. <c>false</c> si la escena no
        /// existe o el puente nativo no esta conectado; en ese caso el log del
        /// motor indica el motivo exacto y la escena actual no se toca.
        /// </returns>
        public static bool LoadScene(string sceneName)
        {
            int rc = RuntimeBridge.SceneLoad(sceneName);
            switch (rc)
            {
                case Ok:
                    Debug.Log($"[SceneManager] Carga de escena '{sceneName}' encolada para el final del frame.");
                    return true;
                case NotFound:
                    Debug.LogError($"[SceneManager] La escena '{sceneName}' no existe en el proyecto.");
                    return false;
                case NullPtr:
                    Debug.LogError("[SceneManager] LoadScene recibio un nombre nulo.");
                    return false;
                case BridgeUnavailable:
                    Debug.LogError("[SceneManager] El puente nativo de escenas no esta conectado (recompilar GlassEngine.Core).");
                    return false;
                default:
                    Debug.LogError($"[SceneManager] Fallo al cargar '{sceneName}' (codigo {rc}).");
                    return false;
            }
        }

        /// <summary>Nombre corto de la escena activa, o cadena vacia si no hay ninguna.</summary>
        public static string GetActiveSceneName() => RuntimeBridge.GetActiveSceneName() ?? string.Empty;

        /// <summary>Numero de escenas descubiertas en <c>assets/scenes</c>.</summary>
        public static int GetSceneCount() => RuntimeBridge.GetSceneCount();

        /// <summary>Nombre corto de la escena en la posicion indicada, o cadena vacia.</summary>
        public static string GetSceneNameAt(int index) => RuntimeBridge.GetSceneNameAt(index) ?? string.Empty;
    }
}
