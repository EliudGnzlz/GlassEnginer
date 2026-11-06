using GlassEngine.Core;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;
using GlassEngine.Core.Audio;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Enemigo del plataformas 3D: patrulla sobre su plataforma en el eje
    /// X entre ±PatrolRange desde su posición de autoría, daña al jugador
    /// por contacto (distancia) y muere con espada/proyectiles. Es un
    /// cuerpo Kinematic movido por script: escribe a la vez el cuerpo
    /// físico y su transform de escena (el sync solo copia los Dynamic).
    /// </summary>
    public class Plat3DEnemy : GlassBehaviour
    {
        protected virtual int MaxHealth => 2;
        protected virtual float PatrolRange => 1.7f;
        protected virtual float PatrolSpeed => 1.3f;

        public bool IsDead { get; private set; }

        private RigidBody _body = null!;
        private Vector3 _anchor;
        private Quaternion _rotation;
        private float _s;
        private int _dir = 1;
        private int _health;

        public override void Start()
        {
            _body = new RigidBody();
            _body.Initialize(EntityId, RigidBodyType.Kinematic, Position, Rotation);
            _anchor = Position;
            _rotation = Rotation;
            _health = MaxHealth;
            Plat3DGameState.RegisterEnemy(EntityId, this);
        }

        public override void Update(float deltaTime)
        {
            if (IsDead || _body == null || !_body.IsValid)
            {
                return;
            }

            _s += _dir * PatrolSpeed * deltaTime;
            if (_s > PatrolRange)
            {
                _s = PatrolRange;
                _dir = -1;
            }
            else if (_s < -PatrolRange)
            {
                _s = -PatrolRange;
                _dir = 1;
            }

            Vector3 pos = new Vector3
            {
                X = _anchor.X + _s,
                Y = _anchor.Y,
                Z = _anchor.Z
            };
            _body.SetTransform(pos, _rotation);
            Position = pos;

            // Daño por contacto al jugador.
            if (Plat3DGameState.PlayerPositionValid)
            {
                Vector3 pp = Plat3DGameState.PlayerPosition;
                float dx = pp.X - pos.X;
                float dy = pp.Y - pos.Y;
                float dz = pp.Z - pos.Z;
                if (dx * dx + dy * dy + dz * dz < 0.95f * 0.95f)
                {
                    Plat3DGameState.DamagePlayer(1, pos);
                }
            }
        }

        public void TakeDamage(int amount)
        {
            if (IsDead)
            {
                return;
            }
            _health -= amount;
            if (_health <= 0)
            {
                IsDead = true;
                Plat3DGameState.UnregisterEnemy(EntityId);
                AudioSource.PlayClipAtPoint("plat3d_enemy_die", Position, 1.0f, 1.0f);
                _body.Destroy();
                // Sin Instantiate/Destroy de entidades: el cadáver sale
                // del mundo (bajo el nivel) y su cuerpo físico se borra.
                Position = new Vector3 { X = _anchor.X, Y = -60f, Z = _anchor.Z };
            }
            else
            {
                AudioSource.PlayClipAtPoint("plat3d_sword_hit", Position, 0.7f, 1.5f);
            }
        }
    }
}
