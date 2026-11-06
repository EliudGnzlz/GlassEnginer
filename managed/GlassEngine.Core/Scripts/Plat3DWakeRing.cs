using GlassEngine.Core;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Anillo de espuma de la estela de la barca (v2): entidad plana con
    /// el grafo de shader «espuma», una de 8 en pool en la escena del
    /// lago. Cada anillo tiene un hueco (slot) y consume los eventos de
    /// estela cuyo índice % 8 coincide con su hueco: al arrancar se
    /// coloca en el punto pedido y se expande de 1,2 a 5,2 m de diámetro
    /// en 1,4 s; después se oculta. Mismo patrón que Plat3DSplash pero
    /// con estado propio por anillo, porque la barca genera ondas sin
    /// parar mientras se mueve y un solo slot no daría abasto.
    /// </summary>
    public class Plat3DWakeRing : GlassBehaviour
    {
        private int _slot = -1;
        private int _nextEvent;
        private float _t = -1f;
        private Vector3 _pos;

        private const float Duration = 1.4f;
        private const int RingSlots = 8;

        private static readonly Vector3 Hidden =
            new Vector3 { X = 0.001f, Y = 0.001f, Z = 0.001f };

        public override void Start()
        {
            _slot = Plat3DGameState.RegisterWakeRing();
            Scale = Hidden;
        }

        public override void Update(float deltaTime)
        {
            // Consumir los eventos de estela pendientes: cada anillo
            // atiende uno de cada RingSlots (reparto circular).
            while (_nextEvent < Plat3DGameState.WakeSpawnCount)
            {
                int idx = _nextEvent++;
                if (idx % RingSlots == _slot)
                {
                    _pos = Plat3DGameState.WakePositionAt(idx);
                    _t = 0f;
                }
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
            float d = 1.2f + 4.0f * k;
            Position = _pos;
            Scale = new Vector3 { X = d, Y = 0.07f, Z = d };
        }
    }
}
