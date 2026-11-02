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

        public static void Destroy(Object obj)
        {
            if (obj is GameObject go)
            {
                // Podría enviar señal de destrucción de la entidad
                Debug.Log($"Destruyendo objeto {go.name} (EntityId: {go.EntityId})");
            }
        }
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
        public bool enabled { get; set; } = true;
    }

    public class Transform : Component
    {
        public Vector3 position
        {
            get
            {
                if (RuntimeBridge.GetTransform(EntityId, out TransformData t) == 0)
                    return t.Position;
                return Vector3.zero;
            }
            set
            {
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
                if (RuntimeBridge.GetTransform(EntityId, out TransformData t) == 0)
                    return t.Rotation;
                return Quaternion.identity;
            }
            set
            {
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
                if (RuntimeBridge.GetTransform(EntityId, out TransformData t) == 0)
                    return t.Scale;
                return Vector3.one;
            }
            set
            {
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

        public void SetActive(bool active)
        {
            activeSelf = active;
        }

        public T AddComponent<T>() where T : Component, new()
        {
            var comp = new T { gameObject = this, name = typeof(T).Name };
            _components.Add(comp);
            return comp;
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

    public class BoxCollider : Component
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

    public class Light : Component
    {
        public Color color { get; set; } = Color.white;
        public float intensity { get; set; } = 1.0f;
    }

    // ==========================================
    // COMPONENTES DE UI ESTILO UNITY (CANVAS, RECTTRANSFORM)
    // ==========================================

    public class RectTransform : Transform
    {
        public Vector2 anchoredPosition { get; set; } = Vector2.zero;
        public Vector2 sizeDelta { get; set; } = new Vector2(100f, 100f);
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

namespace UnityEngine.UI
{
    public class UIBehaviour : MonoBehaviour { }

    public class Text : UIBehaviour
    {
        public string text { get; set; } = "";
        public int fontSize { get; set; } = 14;
        public Color color { get; set; } = Color.white;
    }

    public class Button : UIBehaviour
    {
        public Action? onClick;

        public void Press()
        {
            onClick?.Invoke();
        }
    }
}
