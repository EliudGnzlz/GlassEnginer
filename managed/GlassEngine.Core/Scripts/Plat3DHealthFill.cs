using System;
using GlassEngine.Core;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Relleno de la barra de vida: sigue al jugador como el fondo y se
    /// encoge desde la derecha según la vida (anclado a la izquierda:
    /// el centro se desplaza la mitad de lo perdido). Va 0,06 hacia la
    /// cámara (+Z, donde está la cámara en 3ª persona) para dibujarse
    /// delante del fondo. Se oculta en primera persona.
    /// </summary>
    public class Plat3DHealthFill : GlassBehaviour
    {
        private float _fullScaleX;
        private float _fullScaleY;
        private float _fullScaleZ;
        private bool _init;

        public override void Start()
        {
            Vector3 s = Scale;
            _fullScaleX = s.X;
            _fullScaleY = s.Y;
            _fullScaleZ = s.Z;
            Plat3DGameState.RegisterHealthFill(EntityId);
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

            float frac = Math.Clamp(
                Plat3DGameState.PlayerHealth / (float)Plat3DGameState.PlayerMaxHealth,
                0f, 1f);
            float w = Math.Max(_fullScaleX * frac, 0.001f);
            float lost = _fullScaleX - w;

            Scale = new Vector3 { X = w, Y = _fullScaleY, Z = _fullScaleZ };
            Vector3 pp = Plat3DGameState.PlayerPosition;
            Position = new Vector3
            {
                X = pp.X - lost / 2f,
                Y = pp.Y + 1.45f,
                Z = pp.Z + 0.06f
            };
        }
    }
}
