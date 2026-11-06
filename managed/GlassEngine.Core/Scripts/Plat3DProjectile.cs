using GlassEngine.Core;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;
using GlassEngine.Core.Audio;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Proyectil del pool (6 por nivel, pre-instanciados en la escena sin
    /// bloque de física: el cuerpo Dynamic y su collider esférico los
    /// crea este script). Inactivo queda anclado y oculto (escala mínima;
    /// el sync de física no toca la escala). Al lanzarse vuela con la
    /// velocidad inicial que le da el jugador y gravedad real de Rapier
    /// (arco parabólico), y daña por proximidad al enemigo (sin eventos
    /// de colisión en el motor, la distancia es la vía honesta).
    /// </summary>
    public class Plat3DProjectile : GlassBehaviour
    {
        private RigidBody _body = null!;
        private Vector3 _anchor;
        private Vector3 _fullScale;
        private bool _active;
        private float _life;

        private const float MaxLife = 2.5f;
        private const float HitDistance = 0.85f;

        public bool IsAvailable => !_active;

        public override void Start()
        {
            _body = new RigidBody();
            _body.Initialize(EntityId, RigidBodyType.Dynamic, Position, Rotation);
            _body.AttachSphereCollider(0.22f, 0.1f, 0.4f);
            _anchor = Position;
            _fullScale = Scale;
            Scale = new Vector3 { X = 0.001f, Y = 0.001f, Z = 0.001f };
            Plat3DGameState.RegisterProjectile(this);
        }

        public void Launch(Vector3 origin, Vector3 velocity)
        {
            if (_body == null || !_body.IsValid)
            {
                return;
            }
            _active = true;
            _life = MaxLife;
            Scale = _fullScale;
            _body.SetTransform(origin, Quaternion.Identity);
            _body.SetLinearVelocity(velocity);
            Position = origin;
        }

        public override void Update(float deltaTime)
        {
            if (_body == null || !_body.IsValid)
            {
                return;
            }

            if (!_active)
            {
                // Anclado y quieto hasta que el jugador lo lance: sin
                // esto, la gravedad lo dejaría caer desde su posición de
                // autoría al cargar la escena.
                _body.SetTransform(_anchor, Quaternion.Identity);
                _body.SetLinearVelocity(Vector3.Zero);
                return;
            }

            _life -= deltaTime;
            if (!_body.GetTransform(out Vector3 pos, out Quaternion _))
            {
                Deactivate();
                return;
            }

            // Impacto contra enemigos por proximidad. TakeDamage puede
            // desregistrar al enemigo al morir: se sale del bucle en
            // cuanto hay impacto, sin seguir enumerando el diccionario.
            foreach (var pair in Plat3DGameState.Enemies)
            {
                Plat3DEnemy enemy = pair.Value;
                if (enemy == null || enemy.IsDead)
                {
                    continue;
                }
                Vector3 ep = enemy.Position;
                float dx = ep.X - pos.X;
                float dy = ep.Y - pos.Y;
                float dz = ep.Z - pos.Z;
                if (dx * dx + dy * dy + dz * dz < HitDistance * HitDistance)
                {
                    enemy.TakeDamage(1);
                    AudioSource.PlayClipAtPoint("plat3d_projectile_hit", pos, 1.0f, 1.0f);
                    Deactivate();
                    return;
                }
            }

            if (_life <= 0f || pos.Y < -12f)
            {
                Deactivate();
            }
        }

        private void Deactivate()
        {
            _active = false;
            Scale = new Vector3 { X = 0.001f, Y = 0.001f, Z = 0.001f };
            _body.SetTransform(_anchor, Quaternion.Identity);
            _body.SetLinearVelocity(Vector3.Zero);
            Position = _anchor;
        }
    }
}
