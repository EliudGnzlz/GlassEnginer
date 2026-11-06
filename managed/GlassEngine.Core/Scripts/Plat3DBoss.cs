namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Enemigo final del nivel 3: misma patrulla que el enemigo base pero
    /// mucho más duro (v2: 10 de vida, patrulla ±3,0 a 1,35 m/s — cubre
    /// casi toda la arena para que no se le pueda campear desde los
    /// bordes; su tamaño lo pone la escala autorada en la escena).
    /// </summary>
    public class Plat3DBoss : Plat3DEnemy
    {
        protected override int MaxHealth => 10;
        protected override float PatrolRange => 3.0f;
        protected override float PatrolSpeed => 1.35f;
    }
}
