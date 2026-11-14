using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Physics
{
    // Personaje kinemático de Rapier.
    public sealed class CharacterController
    {
        // Radio de la cápsula.
        public float Radius { get; set; } = 0.35f;

        // Altura total de la cápsula.
        public float Height { get; set; } = 1.8f;

        // Margen de piel de la cápsula.
        public float SkinWidth { get; set; } = 0.02f;

        // Altura máxima de escalón.
        public float StepOffset { get; set; } = 0.4f;

        // Límite de pendiente en grados.
        public float SlopeLimit { get; set; } = 45f;

        // Distancia de ajuste al suelo.
        public float SnapDistance { get; set; } = 0.3f;

        // Entidad asociada al personaje.
        public ulong EntityId { get; private set; }

        // Indica si el personaje fue creado en el motor.
        public bool IsCreated { get; private set; }

        // Indica si el personaje está en el suelo.
        public bool IsGrounded { get; private set; }

        // Indica si el personaje está deslizando.
        public bool IsSliding { get; private set; }

        // Centro de la cápsula, último conocido.
        public Vector3 Position { get; private set; }

        // Inicializa el personaje y lo crea en el motor.
        public void Initialize(ulong entityId, Vector3 centerPosition)
        {
            EntityId = entityId;
            Position = centerPosition;
            Create();
        }

        // Crea el personaje con la configuración actual.
        public bool Create()
        {
            if (IsCreated) return true;
            IsCreated = RuntimeBridge.CharacterCreate(EntityId, Position, Radius, Height, SkinWidth, StepOffset, SlopeLimit, SnapDistance);
            return IsCreated;
        }

        // Mueve el personaje y devuelve el desplazamiento aplicado.
        private const int MaxHits = 16;
    private readonly CharacterHitFfi[] _hitBuffer = new CharacterHitFfi[MaxHits];

    public Vector3 Velocity { get; private set; }
    public CollisionFlags LastFlags { get; private set; }

    public Vector3 Move(Vector3 motion, float dt)
    {
        Vector3 applied = MoveCore(motion, dt);
        ReadLastMove();
        return applied;
    }

    private void ReadLastMove()
    {
        if (!RuntimeBridge.CharacterLastMove(EntityId, out uint flags, out Vector3 velocity, _hitBuffer, out int total))
            return;
        Velocity = velocity;
        LastFlags = (CollisionFlags)flags;
        int count = Math.Min(total, _hitBuffer.Length);
        for (int i = 0; i < count; i++)
            RuntimeBridge.DispatchControllerHit(EntityId, ControllerColliderHit.FromFfi(in _hitBuffer[i]));
    }

    private Vector3 MoveCore(Vector3 motion, float dt)
        {
            if (!IsCreated && !Create())
                return Vector3.Zero;

            bool ok = RuntimeBridge.CharacterMove(EntityId, motion, dt, out Vector3 applied, out bool grounded, out bool sliding);
            if (!ok)
                return Vector3.Zero;

            Position = new Vector3(Position.X + applied.X, Position.Y + applied.Y, Position.Z + applied.Z);
            IsGrounded = grounded;
            IsSliding = sliding;
            return applied;
        }

        // Teletransporta el personaje a la posición indicada.
        public void Teleport(Vector3 centerPosition)
        {
            if (RuntimeBridge.CharacterTeleport(EntityId, centerPosition))
            {
                Position = centerPosition;
                IsGrounded = false;
            }
        }

        // Destruye el personaje en el motor.
        public void Destroy()
        {
            RuntimeBridge.CharacterDestroy(EntityId);
            IsCreated = false;
        }

        // Actualiza posición y estado desde el motor.
        public bool RefreshInfo()
        {
            bool ok = RuntimeBridge.CharacterInfo(EntityId, out Vector3 position, out bool grounded, out bool sliding);
            if (ok)
            {
                Position = position;
                IsGrounded = grounded;
                IsSliding = sliding;
            }
            return ok;
        }
    }
}
