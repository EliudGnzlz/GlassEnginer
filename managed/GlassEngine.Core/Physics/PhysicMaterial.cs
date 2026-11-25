using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Physics
{
    public sealed class PhysicMaterial : PhysicsMaterial
    {
        public float Friction
        {
            get => dynamicFriction;
            set => dynamicFriction = value;
        }

        public float Restitution
        {
            get => bounciness;
            set => bounciness = value;
        }

        public PhysicMaterial()
            : base(0.0f, 0.6f, 0.0f, PhysicsMaterialCombine.Average, PhysicsMaterialCombine.Average)
        {
        }

        public PhysicMaterial(float friction, float restitution)
            : base(friction, 0.6f, restitution, PhysicsMaterialCombine.Average, PhysicsMaterialCombine.Average)
        {
        }

        // Único Clone: el override del base (el runtime sigue siendo
        // PhysicMaterial; quien lo necesite tipado, que haga el cast).
        internal override PhysicsMaterial Clone() => new PhysicMaterial(Friction, Restitution)
        {
            staticFriction = staticFriction,
            frictionCombine = frictionCombine,
            bounceCombine = bounceCombine
        };
    }
}
