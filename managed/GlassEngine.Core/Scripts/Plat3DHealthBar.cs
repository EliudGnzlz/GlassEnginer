using GlassEngine.Core;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Fondo de la barra de vida diegética: flota sobre la cabeza del
    /// jugador (los scripts no pueden tocar el Canvas en runtime, así que
    /// el HUD de vida es esta barra 3D). Se oculta en primera persona.
    /// </summary>
    public class Plat3DHealthBar : GlassBehaviour
    {
        private Vector3 _fullScale;
        private bool _init;

        public override void Start()
        {
            _fullScale = Scale;
            _init = true;
        }

        public override void Update(float deltaTime)
        {
            if (!_init || !Plat3DGameState.PlayerPositionValid)
            {
                return;
            }
            if (Plat3DGameState.FirstPerson)
            {
                Scale = new Vector3 { X = 0.001f, Y = 0.001f, Z = 0.001f };
                return;
            }
            Scale = _fullScale;
            Vector3 pp = Plat3DGameState.PlayerPosition;
            Position = new Vector3 { X = pp.X, Y = pp.Y + 1.45f, Z = pp.Z };
        }
    }
}
