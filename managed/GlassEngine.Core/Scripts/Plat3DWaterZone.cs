using GlassEngine.Core;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Zona de agua: se adjunta a la entidad de la superficie (la
    /// primitiva Plano del motor con el grafo de shader «agua», sin
    /// física: el jugador la atraviesa y se hunde). Registra en
    /// Plat3DGameState los límites derivados de su posición/escala de
    /// autoría, la altura de la superficie y el punto de control: la
    /// orilla de partida está en el lado -X (la barca cruza hacia +X),
    /// 2,2 m más allá del borde y 2,4 m sobre la superficie, para caer
    /// sobre la plataforma de la orilla al reaparecer.
    ///
    /// La superficie ES la posición Y de la entidad: el plano es plano
    /// (antes la superficie era un cubo aplastado y había que sumar
    /// media escala Y; con el plano ese cálculo daría una superficie
    /// fantasma por encima del agua visible).
    /// </summary>
    public class Plat3DWaterZone : GlassBehaviour
    {
        public override void Start()
        {
            Vector3 p = Position;
            Vector3 s = Scale;
            float surfaceY = p.Y;
            Plat3DGameState.RegisterWaterZone(new Plat3DWaterZoneData
            {
                MinX = p.X - s.X / 2f,
                MaxX = p.X + s.X / 2f,
                MinZ = p.Z - s.Z / 2f,
                MaxZ = p.Z + s.Z / 2f,
                SurfaceY = surfaceY,
                Checkpoint = new Vector3
                {
                    X = p.X - s.X / 2f - 2.2f,
                    Y = surfaceY + 2.4f,
                    Z = p.Z
                }
            });
        }
    }
}
