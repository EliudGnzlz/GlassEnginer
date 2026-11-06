using GlassEngine.Core;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Onda de espuma al tocar el agua: una entidad plana con el grafo
    /// de shader «espuma» que vive oculta (escala mínima) y, cuando el
    /// jugador pide un chapoteo (Plat3DGameState.RequestSplash), aparece
    /// en el punto de contacto y se expande 0,85 s hasta desvanecerse.
    /// Es el efecto de contacto en la capa de juego: el runtime de
    /// grafos no expone uniforms a medida para pasar la posición del
    /// jugador al shader de agua.
    /// </summary>
    public class Plat3DSplash : GlassBehaviour
    {
        private float _t = -1f;
        private Vector3 _pos;

        private const float Duration = 0.85f;

        private static readonly Vector3 Hidden =
            new Vector3 { X = 0.001f, Y = 0.001f, Z = 0.001f };

        public override void Start()
        {
            Scale = Hidden;
        }

        public override void Update(float deltaTime)
        {
            if (Plat3DGameState.SplashPending)
            {
                _pos = Plat3DGameState.SplashPosition;
                Plat3DGameState.SplashPending = false;
                _t = 0f;
            }
            if (_t < 0f)
            {
                return;
            }
            _t += deltaTime;
            float k = _t / Duration;
            if (k >= 1f)
            {
                _t = -1f;
                Scale = Hidden;
                return;
            }
            float r = 0.7f + 2.6f * k;
            Position = _pos;
            Scale = new Vector3 { X = r, Y = 0.07f, Z = r };
        }
    }
}
