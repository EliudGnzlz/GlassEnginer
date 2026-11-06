namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Enemigo tanque (v2): aguanta un golpe más que el base y patrulla
    /// despacio; es un muro de carne en las cubiertas de paso obligado.
    /// </summary>
    public class Plat3DEnemyTank : Plat3DEnemy
    {
        protected override int MaxHealth => 3;
        protected override float PatrolRange => 1.7f;
        protected override float PatrolSpeed => 1.1f;
    }
}
