using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts
{
    /// <summary>Plataforma móvil vertical (sube y baja).</summary>
    public class Plat3DMovingPlatformVertical : Plat3DMovingPlatform
    {
        protected override Vector3 Axis => new Vector3 { X = 0f, Y = 1f, Z = 0f };
        protected override float Range => 1.5f;
        protected override float Speed => 0.9f;
    }
}
