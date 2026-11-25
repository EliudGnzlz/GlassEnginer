using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Physics;

public enum PhysicsMaterialCombine
{
    Average = 0,
    Multiply = 1,
    Minimum = 2,
    Maximum = 3
}

/// <summary>
/// Espejo administrado de un material nativo. Los materiales registrados no
/// persisten a disco: la tabla nativa se destruye al salir de Play.
/// </summary>
public class PhysicsMaterial
{
    private float _dynamicFriction;
    private float _staticFriction;
    private float _bounciness;
    private PhysicsMaterialCombine _frictionCombine;
    private PhysicsMaterialCombine _bounceCombine;

    internal uint MaterialId { get; set; }

    public float dynamicFriction
    {
        get => _dynamicFriction;
        set
        {
            _dynamicFriction = value;
            PushToNative();
        }
    }

    public float staticFriction
    {
        get => _staticFriction;
        set
        {
            _staticFriction = value;
            PushToNative();
        }
    }

    public float bounciness
    {
        get => _bounciness;
        set
        {
            _bounciness = value;
            PushToNative();
        }
    }

    public PhysicsMaterialCombine frictionCombine
    {
        get => _frictionCombine;
        set
        {
            _frictionCombine = value;
            PushToNative();
        }
    }

    public PhysicsMaterialCombine bounceCombine
    {
        get => _bounceCombine;
        set
        {
            _bounceCombine = value;
            PushToNative();
        }
    }

    public PhysicsMaterial()
        : this(0.6f, 0.6f, 0.0f, PhysicsMaterialCombine.Average, PhysicsMaterialCombine.Average)
    {
    }

    public PhysicsMaterial(
        float dynamicFriction,
        float staticFriction,
        float bounciness,
        PhysicsMaterialCombine frictionCombine,
        PhysicsMaterialCombine bounceCombine)
    {
        _dynamicFriction = dynamicFriction;
        _staticFriction = staticFriction;
        _bounciness = bounciness;
        _frictionCombine = frictionCombine;
        _bounceCombine = bounceCombine;
    }

    internal virtual PhysicsMaterial Clone() => new(
        _dynamicFriction,
        _staticFriction,
        _bounciness,
        _frictionCombine,
        _bounceCombine);

    internal void RefreshValues(
        float dynamicFriction,
        float staticFriction,
        float bounciness,
        PhysicsMaterialCombine frictionCombine,
        PhysicsMaterialCombine bounceCombine)
    {
        // Asignación directa a los campos: sin PushToNative, porque los
        // valores vienen del nativo y ya están allí.
        _dynamicFriction = dynamicFriction;
        _staticFriction = staticFriction;
        _bounciness = bounciness;
        _frictionCombine = frictionCombine;
        _bounceCombine = bounceCombine;
    }

    internal uint EnsureRegistered()
    {
        if (MaterialId == 0)
        {
            MaterialId = RuntimeBridge.MaterialCreate(
                _dynamicFriction,
                _staticFriction,
                _bounciness,
                CombineToNative(_frictionCombine),
                CombineToNative(_bounceCombine));
        }

        return MaterialId;
    }

    internal void AdoptNativeId(uint materialId)
    {
        MaterialId = materialId;
    }

    internal static PhysicsMaterial FromNative(uint materialId)
    {
        if (materialId == 0 ||
            !RuntimeBridge.MaterialGet(
                materialId,
                out float dynamicFriction,
                out float staticFriction,
                out float bounciness,
                out int frictionCombine,
                out int bounceCombine))
        {
            return null;
        }

        return new PhysicsMaterial(
            dynamicFriction,
            staticFriction,
            bounciness,
            CombineFromNative(frictionCombine),
            CombineFromNative(bounceCombine))
        {
            MaterialId = materialId
        };
    }

    internal static int CombineToNative(PhysicsMaterialCombine combine)
    {
        switch (combine)
        {
            case PhysicsMaterialCombine.Average:
                return 0;
            case PhysicsMaterialCombine.Multiply:
                return 1;
            case PhysicsMaterialCombine.Minimum:
                return 2;
            case PhysicsMaterialCombine.Maximum:
                return 3;
            default:
                return 0;
        }
    }

    internal static PhysicsMaterialCombine CombineFromNative(int combine)
    {
        switch (combine)
        {
            case 0:
                return PhysicsMaterialCombine.Average;
            case 1:
                return PhysicsMaterialCombine.Multiply;
            case 2:
                return PhysicsMaterialCombine.Minimum;
            case 3:
                return PhysicsMaterialCombine.Maximum;
            default:
                return PhysicsMaterialCombine.Average;
        }
    }

    private void PushToNative()
    {
        if (MaterialId == 0)
            return;

        RuntimeBridge.MaterialSet(
            MaterialId,
            _dynamicFriction,
            _staticFriction,
            _bounciness,
            CombineToNative(_frictionCombine),
            CombineToNative(_bounceCombine));
    }
}
