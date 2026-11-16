namespace GlassEngine.Core.Physics
{
    /// <summary>
    /// Material físico mínimo (Unity: PhysicMaterial). El motor no tiene
    /// sistema de assets de materiales físicos: es un objeto de datos con
    /// fricción y restitución que el CharacterController aplica a su
    /// cápsula al asignarlo a Material/SharedMaterial.
    /// </summary>
    public sealed class PhysicMaterial
    {
        public float Friction { get; set; } = 0.0f;
        public float Restitution { get; set; } = 0.0f;

        public PhysicMaterial() { }

        public PhysicMaterial(float friction, float restitution)
        {
            Friction = friction;
            Restitution = restitution;
        }

        internal PhysicMaterial Clone() => new PhysicMaterial(Friction, Restitution);
    }
}
