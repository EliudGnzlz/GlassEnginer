namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Pinchos: se registran por EntityId. El daño lo aplica el jugador
    /// cuando su raycast de suelo golpea una entidad registrada aquí
    /// (1 de vida con empuje hacia atrás y la invulnerabilidad de 1 s
    /// del jugador como límite de repetición).
    /// </summary>
    public class Plat3DSpikes : GlassBehaviour
    {
        public override void Start()
        {
            Plat3DGameState.RegisterSpike(EntityId);
        }
    }
}
