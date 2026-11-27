// Atributos de Unity para que los scripts convertidos compilen.
// Tipos sin semántica en v1, salvo SerializeField y FormerlySerializedAs, que
// reconoce el aplicador de campos serializados (Hosting/SerializedFields.cs).

using System;

namespace UnityEngine
{
    [AttributeUsage(AttributeTargets.Field)]
    public sealed class SerializeField : Attribute { }

    [AttributeUsage(AttributeTargets.Field)]
    public sealed class SerializeReference : Attribute { }

    [AttributeUsage(AttributeTargets.Field)]
    public sealed class TooltipAttribute : Attribute
    {
        public readonly string tooltip;
        public TooltipAttribute(string tooltip) => this.tooltip = tooltip;
    }

    [AttributeUsage(AttributeTargets.Field, AllowMultiple = true)]
    public sealed class HeaderAttribute : Attribute
    {
        public readonly string header;
        public HeaderAttribute(string header) => this.header = header;
    }

    [AttributeUsage(AttributeTargets.Field)]
    public sealed class RangeAttribute : Attribute
    {
        public readonly float min;
        public readonly float max;
        public RangeAttribute(float min, float max)
        {
            this.min = min;
            this.max = max;
        }
    }

    [AttributeUsage(AttributeTargets.Field, AllowMultiple = true)]
    public sealed class SpaceAttribute : Attribute
    {
        public readonly float height;
        public SpaceAttribute(float height) => this.height = height;
    }

    [AttributeUsage(AttributeTargets.Field)]
    public sealed class HideInInspector : Attribute { }

    [AttributeUsage(AttributeTargets.Class, AllowMultiple = true)]
    public sealed class RequireComponent : Attribute
    {
        public Type Type { get; }
        public RequireComponent(Type requiredComponent) => Type = requiredComponent;
    }

    [AttributeUsage(AttributeTargets.Class)]
    public sealed class AddComponentMenuAttribute : Attribute
    {
        public string componentMenu { get; }
        public int componentOrder { get; }
        public AddComponentMenuAttribute(string menuName) => componentMenu = menuName;
        public AddComponentMenuAttribute(string menuName, int order)
        {
            componentMenu = menuName;
            componentOrder = order;
        }
    }

    [AttributeUsage(AttributeTargets.Class, AllowMultiple = true)]
    public sealed class HelpURLAttribute : Attribute
    {
        public string URL { get; }
        public HelpURLAttribute(string url) => URL = url;
    }

    [AttributeUsage(AttributeTargets.Class)]
    public sealed class CreateAssetMenuAttribute : Attribute
    {
        public string fileName { get; set; } = "";
        public string menuName { get; set; } = "";
        public int order { get; set; }
    }

    [AttributeUsage(AttributeTargets.Field)]
    public sealed class MultilineAttribute : Attribute
    {
        public readonly int lines;
        public MultilineAttribute(int lines) => this.lines = lines;
    }

    [AttributeUsage(AttributeTargets.Field)]
    public sealed class TextAreaAttribute : Attribute
    {
        public readonly int minLines;
        public readonly int maxLines;
        public TextAreaAttribute(int minLines, int maxLines)
        {
            this.minLines = minLines;
            this.maxLines = maxLines;
        }
    }

    [AttributeUsage(AttributeTargets.Field)]
    public sealed class MinAttribute : Attribute
    {
        public readonly float min;
        public MinAttribute(float min) => this.min = min;
    }

    [AttributeUsage(AttributeTargets.Field)]
    public sealed class ColorUsageAttribute : Attribute
    {
        public readonly bool showAlpha;
        public readonly bool hdr;
        public ColorUsageAttribute(bool showAlpha) => this.showAlpha = showAlpha;
        public ColorUsageAttribute(bool showAlpha, bool hdr)
        {
            this.showAlpha = showAlpha;
            this.hdr = hdr;
        }
    }

    [AttributeUsage(AttributeTargets.Class)]
    public sealed class DisallowMultipleComponent : Attribute { }

    [AttributeUsage(AttributeTargets.Class)]
    public sealed class ExecuteInEditMode : Attribute { }

    [AttributeUsage(AttributeTargets.Class)]
    public sealed class ExecuteAlways : Attribute { }

    [AttributeUsage(AttributeTargets.Class)]
    public sealed class SelectionBase : Attribute { }

    [AttributeUsage(AttributeTargets.Method | AttributeTargets.Field)]
    public sealed class ContextMenuAttribute : Attribute
    {
        public readonly string menuItem;
        public ContextMenuAttribute(string itemName) => menuItem = itemName;
    }

    public enum RuntimeInitializeLoadType
    {
        AfterSceneLoad,
        BeforeSceneLoad,
        AfterAssembliesLoaded,
        BeforeSplashScreen,
        SubsystemRegistration,
    }

    [AttributeUsage(AttributeTargets.Method)]
    public sealed class RuntimeInitializeOnLoadMethodAttribute : Attribute
    {
        public RuntimeInitializeLoadType loadType { get; }
        public RuntimeInitializeOnLoadMethodAttribute() => loadType = RuntimeInitializeLoadType.AfterSceneLoad;
        public RuntimeInitializeOnLoadMethodAttribute(RuntimeInitializeLoadType loadType) => this.loadType = loadType;
    }
}

namespace UnityEngine.Serialization
{
    [AttributeUsage(AttributeTargets.Field, AllowMultiple = true)]
    public sealed class FormerlySerializedAsAttribute : Attribute
    {
        public string OldName { get; }
        public FormerlySerializedAsAttribute(string oldName) => OldName = oldName;
    }
}
