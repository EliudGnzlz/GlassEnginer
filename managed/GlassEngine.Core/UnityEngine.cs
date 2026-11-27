using System;
using System.Collections.Generic;
using GlassEngine.Core;
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

        private readonly List<Component> _components = new();
        private static readonly Dictionary<ulong, GameObject> _entityMap = new();

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
    }

    // ==========================================
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

    public class BoxCollider : Collider
    {
        public Vector3 size { get; set; } = Vector3.one;
    }

    public class AudioSource : Component
    {
        public string clip { get; set; } = "";
        public float volume { get; set; } = 1.0f;
        public float pitch { get; set; } = 1.0f;
        public bool loop { get; set; } = false;

        public void Play()
        {
            if (!string.IsNullOrEmpty(clip))
            {
                RuntimeBridge.PlayAudio3D(clip, transform.position, volume, pitch, loop);
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
        // pivote quedan locales en v1 (el motor los lee de la
        // escena, no del script).
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
        public Vector2 anchorMin { get; set; } = new Vector2(0.5f, 0.5f);
        public Vector2 anchorMax { get; set; } = new Vector2(0.5f, 0.5f);
        public Vector2 pivot { get; set; } = new Vector2(0.5f, 0.5f);
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
