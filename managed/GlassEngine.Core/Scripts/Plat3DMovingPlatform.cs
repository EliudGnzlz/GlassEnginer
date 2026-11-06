using GlassEngine.Core;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Plataforma móvil ping-pong (también es la base de la barca).
    /// Cuerpo Kinematic movido por script: escribe el cuerpo físico y el
    /// transform de escena, y publica su desplazamiento del frame en
    /// Plat3DGameState.PlatformDeltas para que el jugador (que lo detecta
    /// con su raycast de suelo) sea transportado. Las variantes por nivel
    /// son subclases: el motor no tiene parámetros por script.
    /// </summary>
    public class Plat3DMovingPlatform : GlassBehaviour
    {
        protected virtual Vector3 Axis => new Vector3 { X = 1f, Y = 0f, Z = 0f };
        protected virtual float Range => 2.2f;
        protected virtual float Speed => 1.15f;
        protected virtual float Dwell => 0f;

        private RigidBody _body = null!;
        private Vector3 _anchor;
        private Quaternion _rotation;
        private float _s;
        private int _dir = 1;
        private float _dwellT;

        public override void Start()
        {
            _body = new RigidBody();
            _body.Initialize(EntityId, RigidBodyType.Kinematic, Position, Rotation);
            _anchor = Position;
            _rotation = Rotation;
            Plat3DGameState.BeginPlatformFrame(EntityId);
        }

        public override void Update(float deltaTime)
        {
            // Contrato de deltas: primero se limpia el del frame anterior
            // y tras mover se escribe el de este frame.
            Plat3DGameState.BeginPlatformFrame(EntityId);
            if (_body == null || !_body.IsValid)
            {
                return;
            }

            Vector3 old = Position;

            if (_dwellT > 0f)
            {
                _dwellT -= deltaTime;
            }
            else
            {
                _s += _dir * Speed * deltaTime;
                if (_s >= Range)
                {
                    _s = Range;
                    _dir = -1;
                    _dwellT = Dwell;
                }
                else if (_s <= -Range)
                {
                    _s = -Range;
                    _dir = 1;
                    _dwellT = Dwell;
                }
            }

            Vector3 axis = Axis;
            Vector3 pos = new Vector3
            {
                X = _anchor.X + axis.X * _s,
                Y = _anchor.Y + axis.Y * _s,
                Z = _anchor.Z + axis.Z * _s
            };
            _body.SetTransform(pos, _rotation);
            Position = pos;
            Plat3DGameState.SetPlatformDelta(EntityId, Plat3DMath.Sub(pos, old));
        }
    }
}
