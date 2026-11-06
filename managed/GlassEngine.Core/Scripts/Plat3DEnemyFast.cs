namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Enemigo rápido (v2): patrulla más larga y paso vivo; castiga
    /// quedarse quieto en su cubierta. Misma vida que el base.
    /// </summary>
    public class Plat3DEnemyFast : Plat3DEnemy
    {
        protected override int MaxHealth => 2;
        protected override float PatrolRange => 2.2f;
        protected override float PatrolSpeed => 1.9f;
    }
}
