using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Physics;

public enum ColliderType
{
    Box,
    Sphere,
    Capsule,
    Cylinder
}

public sealed class Collider
{
    private PhysicsMaterial _sharedMaterial;
    private PhysicsMaterial _instanceMaterial;
    private bool _materialInstanced;

    public ColliderType Type { get; set; } = ColliderType.Box;
    public Vector3 HalfExtents { get; set; } = new(0.5f, 0.5f, 0.5f);
    public float Radius { get; set; } = 0.5f;
    public ulong Handle { get; internal set; }
    public ulong EntityId { get; internal set; }
    public uint Index { get; internal set; }

    public PhysicsMaterial sharedMaterial
    {
        get
        {
            if (EntityId == 0)
                return _sharedMaterial;

            uint materialId = RuntimeBridge.ColliderGetSharedMaterial(EntityId, Index);
            if (materialId == 0)
            {
                _sharedMaterial = null;
                return null;
            }

            if (_sharedMaterial != null && _sharedMaterial.MaterialId == materialId)
            {
                // El id es el mismo pero los valores pueden haber cambiado
                // (refresh_asset en Play): se refrescan en el objeto en caché.
                if (RuntimeBridge.MaterialGet(
                    materialId,
                    out float df,
                    out float sf,
                    out float b,
                    out int fc,
                    out int bc))
                {
                    _sharedMaterial.RefreshValues(
                        df, sf, b,
                        PhysicsMaterial.CombineFromNative(fc),
                        PhysicsMaterial.CombineFromNative(bc));
                }
                return _sharedMaterial;
            }

            _sharedMaterial = PhysicsMaterial.FromNative(materialId);
            return _sharedMaterial;
        }
        set
        {
            _sharedMaterial = value;
            _instanceMaterial = null;
            _materialInstanced = false;

            if (EntityId == 0)
                return;

            uint materialId = value == null ? 0 : value.EnsureRegistered();
            RuntimeBridge.ColliderSetSharedMaterial(EntityId, Index, materialId);
        }
    }

    public PhysicsMaterial material
    {
        get
        {
            if (_materialInstanced)
                return _instanceMaterial;

            if (EntityId == 0)
            {
                _instanceMaterial = _sharedMaterial == null
                    ? new PhysicsMaterial()
                    : _sharedMaterial.Clone();
                _materialInstanced = true;
                return _instanceMaterial;
            }

            uint materialId = RuntimeBridge.ColliderGetMaterial(EntityId, Index);
            if (materialId == 0)
                return null;

            _instanceMaterial = PhysicsMaterial.FromNative(materialId);
            _materialInstanced = _instanceMaterial != null;
            return _instanceMaterial;
        }
        set
        {
            if (EntityId == 0)
            {
                _instanceMaterial = value;
                _materialInstanced = true;
                return;
            }

            uint instanceId = RuntimeBridge.ColliderGetMaterial(EntityId, Index);
            if (instanceId == 0)
                return;

            PhysicsMaterial source = value ?? sharedMaterial ?? new PhysicsMaterial();
            RuntimeBridge.MaterialSet(
                instanceId,
                source.dynamicFriction,
                source.staticFriction,
                source.bounciness,
                PhysicsMaterial.CombineToNative(source.frictionCombine),
                PhysicsMaterial.CombineToNative(source.bounceCombine));

            _instanceMaterial = source.Clone();
            _instanceMaterial.AdoptNativeId(instanceId);
            _materialInstanced = true;
        }
    }
}
