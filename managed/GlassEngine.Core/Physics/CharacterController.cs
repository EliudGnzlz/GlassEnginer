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
        public float StepOffset
        {
            get => _stepOffset;
            set
            {
                _stepOffset = value;
                if (IsCreated) RuntimeBridge.CharacterSetStepOffset(EntityId, value);
            }
        }
        private float _stepOffset = 0.4f;

        /// <summary>
        /// Unity: minMoveDistance. Si el movimiento pedido a Move es más corto,
        /// se descarta. Defecto 0 (desactivado), a diferencia de Unity (0.001),
        /// para no cambiar el comportamiento de los juegos existentes.
        /// </summary>
        public float MinMoveDistance
        {
            get => _minMoveDistance;
            set
            {
                _minMoveDistance = value;
                if (IsCreated) RuntimeBridge.CharacterSetMinMoveDistance(EntityId, value);
            }
        }
        private float _minMoveDistance = 0.0f;

        /// <summary>
        /// Material compartido (Unity: Collider.sharedMaterial). Guarda la referencia viva
        /// y, al asignarlo, aplica Friction/Restitution al collider de la cápsula.
        /// Nunca es null: asignar null restaura un material 0/0. Cambiar los campos del
        /// objeto después de asignarlo no se reaplica solo; hay que volver a asignarlo.
        /// La cápsula del CharacterController está en un grupo de colisión aislado y no
        /// participa en el solver de contactos, así que hoy estos valores quedan guardados
        /// y aplicados al collider pero no cambian la interacción con otros cuerpos.
        /// </summary>
        public PhysicMaterial SharedMaterial
        {
            get => _sharedMaterial;
            set
            {
                _sharedMaterial = value ?? new PhysicMaterial();
                _materialInstanced = false;
                ApplyMaterial();
            }
        }

        /// <summary>
        /// Material instancia (Unity: Collider.material). La primera lectura crea una copia
        /// del material compartido y la deja como material de este controlador, de modo que
        /// las lecturas siguientes devuelven esa misma copia y el material compartido
        /// original no se modifica. Asignar sustituye el material compartido y lo aplica a
        /// la cápsula. Mismas limitaciones que <see cref="SharedMaterial"/>.
        /// </summary>
        public PhysicMaterial Material
        {
            get
            {
                if (!_materialInstanced)
                {
                    _sharedMaterial = _sharedMaterial.Clone();
                    _materialInstanced = true;
                }
                return _sharedMaterial;
            }
            set
            {
                _sharedMaterial = value ?? new PhysicMaterial();
                _materialInstanced = true;
                ApplyMaterial();
            }
        }
        private PhysicMaterial _sharedMaterial = new PhysicMaterial();
        private bool _materialInstanced;

        private void ApplyMaterial()
        {
            if (IsCreated) RuntimeBridge.CharacterSetMaterial(EntityId, _sharedMaterial.Friction, _sharedMaterial.Restitution);
        }

        /// <summary>
        /// Raycast contra la propia cápsula del personaje (Unity: Collider.Raycast; el motor no
        /// tiene tipo Ray, así que la firma usa origin/direction como Physics.Raycast). No es una
        /// consulta de mundo. Rellena Point, Normal y Distance; si no hay impacto, hit = default.
        /// maxDistance infinito se trata como float.MaxValue.
        /// </summary>
        public bool Raycast(Vector3 origin, Vector3 direction, out RaycastHit hit, float maxDistance)
        {
            hit = default;
            if (!IsCreated) return false;
            float max = float.IsPositiveInfinity(maxDistance) ? float.MaxValue : maxDistance;
            if (!RuntimeBridge.CharacterRaycast(EntityId, origin, direction, max, out float distance, out Vector3 point, out Vector3 normal))
                return false;
            hit = new RaycastHit { EntityId = EntityId, Point = point, Normal = normal, Distance = distance };
            return true;
        }

        /// <summary>
        /// Punto más cercano sobre la cápsula (Unity: Collider.ClosestPoint). Si el punto está
        /// dentro, se devuelve tal cual. Sin personaje o si el puente falla, devuelve position.
        /// </summary>
        public Vector3 ClosestPoint(Vector3 position)
        {
            if (IsCreated && RuntimeBridge.CharacterClosestPoint(EntityId, position, out Vector3 result)) return result;
            return position;
        }

        /// <summary>
        /// Punto más cercano sobre el AABB de la cápsula (Unity: Collider.ClosestPointOnBounds).
        /// Sin personaje o si el puente falla, devuelve position.
        /// </summary>
        public Vector3 ClosestPointOnBounds(Vector3 position)
        {
            if (IsCreated && RuntimeBridge.CharacterClosestPointOnBounds(EntityId, position, out Vector3 result)) return result;
            return position;
        }

        /// <summary>
        /// Unity: center. Desplaza la cápsula respecto a la entidad; al
        /// cambiarlo la cápsula se mueve en (nuevo − anterior). Position sigue
        /// siendo el centro de la cápsula en mundo.
        /// </summary>
        public Vector3 Center
        {
            get => _center;
            set
            {
                Vector3 anterior = _center;
                _center = value;
                if (IsCreated && RuntimeBridge.CharacterSetCenter(EntityId, value))
                {
                    // La cápsula se movió en (nuevo − anterior): la Position
                    // cacheada (centro de la cápsula en mundo) se mueve igual.
                    Position = Position + (value + anterior * -1f);
                }
            }
        }
        private Vector3 _center = Vector3.Zero;

        // Límite de pendiente en grados.
        public float SlopeLimit
        {
            get => _slopeLimit;
            set
            {
                _slopeLimit = value;
                if (IsCreated) RuntimeBridge.CharacterSetSlopeLimit(EntityId, value);
            }
        }
        private float _slopeLimit = 45f;

        // Distancia de ajuste al suelo.
        public float SnapDistance { get; set; } = 0.3f;

        // Detección de colisiones activa (Unity: CharacterController.detectCollisions).
        public bool DetectCollisions
        {
            get => _detectCollisions;
            set
            {
                _detectCollisions = value;
                if (IsCreated) RuntimeBridge.CharacterSetDetectCollisions(EntityId, value);
            }
        }
        private bool _detectCollisions = true;

        // Despenetración automática contra solapes al mover.
        public bool EnableOverlapRecovery
        {
            get => _enableOverlapRecovery;
            set
            {
                _enableOverlapRecovery = value;
                if (IsCreated) RuntimeBridge.CharacterSetOverlapRecovery(EntityId, value);
            }
        }
        private bool _enableOverlapRecovery = true;

        // Controlador habilitado (Unity: CharacterController.enabled).
        public bool Enabled
        {
            get => _enabled;
            set
            {
                _enabled = value;
                if (IsCreated) RuntimeBridge.CharacterSetEnabled(EntityId, value);
            }
        }
        private bool _enabled = true;

        // Máscara de capas que el controlador puede ver en sus sondas.
        // -1 (todos los bits) equivale a todas las capas.
        public int IncludeLayers
        {
            get => _includeLayers;
            set
            {
                _includeLayers = value;
                if (IsCreated) RuntimeBridge.CharacterSetIncludeLayers(EntityId, unchecked((uint)value));
            }
        }
        private int _includeLayers = -1;

        // Caja de la cápsula del personaje en coordenadas de mundo.
        public bool GetBounds(out Vector3 min, out Vector3 max)
        {
            min = Vector3.Zero;
            max = Vector3.Zero;
            if (!IsCreated) return false;
            float[] minArr = new float[3];
            float[] maxArr = new float[3];
            if (!RuntimeBridge.CharacterBounds(EntityId, minArr, maxArr)) return false;
            min = new Vector3(minArr[0], minArr[1], minArr[2]);
            max = new Vector3(maxArr[0], maxArr[1], maxArr[2]);
            return true;
        }

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
            if (IsCreated)
            {
                if (_minMoveDistance != 0f) RuntimeBridge.CharacterSetMinMoveDistance(EntityId, _minMoveDistance);
                if (_center.X != 0f || _center.Y != 0f || _center.Z != 0f)
                {
                    if (RuntimeBridge.CharacterSetCenter(EntityId, _center))
                        Position = Position + _center;
                }
                if (_sharedMaterial.Friction != 0f || _sharedMaterial.Restitution != 0f) ApplyMaterial();
            }
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

    private float _simpleMoveFallSpeed;

    /// <summary>
    /// SimpleMove de Unity: mueve al personaje a la velocidad dada (m/s),
    /// ignorando su componente Y: la caída la lleva este método con la
    /// gravedad del mundo por defecto del motor (0, -9.81, 0). En el
    /// suelo la velocidad vertical se pega a -1 m/s. Devuelve los
    /// CollisionFlags del movimiento. Usa Time.deltaTime.
    /// Simplificación conocida: no hay FFI de gravedad mundial, así que un
    /// cambio de gravedad del mundo no se refleja aquí.
    /// </summary>
    public CollisionFlags SimpleMove(Vector3 speed)
    {
        if (!IsCreated && !Create())
            return CollisionFlags.None;

        float dt = UnityEngine.Time.deltaTime;
        if (IsGrounded)
            _simpleMoveFallSpeed = -1.0f;
        else
            _simpleMoveFallSpeed += -9.81f * dt;

        Vector3 motion = new Vector3(speed.X, _simpleMoveFallSpeed, speed.Z) * dt;
        Move(motion, dt);
        if ((LastFlags & CollisionFlags.Above) != 0 && _simpleMoveFallSpeed > 0f)
            _simpleMoveFallSpeed = 0f;
        return LastFlags;
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
