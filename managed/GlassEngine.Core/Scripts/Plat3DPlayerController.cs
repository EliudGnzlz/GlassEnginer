using System;
using GlassEngine.Core;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;
using GlassEngine.Core.Audio;
using UnityEngine.SceneManagement;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Jugador del plataformas 3D. Base: código de Claude (bloque Plat3D),
    /// contrastado con la API real e integrado por Rayo con: chequeo de
    /// pinchos por el raycast de suelo, zonas de agua con hundimiento y
    /// reaparición en el punto de control, movimiento en primera persona
    /// (giro con A/D) y rotación visual aplicada al cuerpo físico (el
    /// sync de física pisa la rotación de escena con la del cuerpo).
    ///
    /// Convenciones:
    ///  - TakeDamage(amount, attackerPosition): el segundo parámetro es la
    ///    POSICIÓN de quien golpea; el empuje aleja al jugador de ella.
    ///  - La mirada (facing) es un yaw: en 3ª persona mira hacia donde se
    ///    mueve; en 1ª persona gira con A/D y W/S avanzan/retroceden.
    /// </summary>
    public class Plat3DPlayerController : GlassBehaviour
    {
        private RigidBody _body = null!;

        private Vector3 _spawnPosition;
        private float _facingYaw;
        private Vector3 _facing = new Vector3 { X = 0f, Y = 0f, Z = 1f };

        private bool _wasRunPressed;
        private bool _applyKnockbackThisFrame;
        private bool _wasJumpPressed;
        private bool _applyWallPushThisFrame;

        private float _jumpCooldown;
        private float _swordCooldown;
        private float _throwCooldown;
        private float _invulnerability;

        private bool _dead;

        // Agua: mientras se hunde no hay control; al llegar al fondo de la
        // zona pierde 1 de vida y reaparece en el punto de control.
        private bool _sinking;
        private Plat3DWaterZoneData _sinkZone = null!;

        private const float MoveSpeed = 5.2f;
        private const float JumpVelocity = 7.6f;
        private const float JumpCooldownTime = 0.25f;
        private const float SwordCooldownTime = 0.35f;
        private const float ThrowCooldownTime = 0.45f;
        private const float InvulnerabilityTime = 1.0f;
        private const float KnockbackSpeed = 5.5f;
        private const float KnockbackVy = 4.5f;
        private const float FallDeathY = -8.0f;
        private const float TurnSpeedFp = 2.4f; // rad/s en primera persona
        private const float WallCheckDistance = 0.85f;
        private const float WallJumpPushSpeed = 3.4f;

        public override void Awake()
        {
            Plat3DGameState.ResetStatics();
        }

        public override void Start()
        {
            _body = new RigidBody();
            _body.Initialize(EntityId, RigidBodyType.Dynamic, Position, Rotation);

            _spawnPosition = Position;

            Plat3DGameState.PlayerHealth = Plat3DGameState.PlayerMaxHealth;
            Plat3DGameState.Player = this;

            Plat3DGameState.PlayerPosition = Position;
            Plat3DGameState.PlayerPositionValid = true;
        }

        public override void Update(float deltaTime)
        {
            if (_dead || _body == null || !_body.IsValid)
            {
                return;
            }

            TickTimers(deltaTime);

            Vector3 physicsPos;
            Quaternion physicsRot;
            if (!_body.GetTransform(out physicsPos, out physicsRot))
            {
                return;
            }

            PublishState(physicsPos);

            // --- Agua: entrada y hundimiento ---
            if (!_sinking)
            {
                Plat3DWaterZoneData? zone = ZoneAt(physicsPos);
                if (zone != null && physicsPos.Y < zone.SurfaceY - 0.05f)
                {
                    _sinking = true;
                    _sinkZone = zone;
                    Plat3DGameState.RequestSplash(new Vector3
                    {
                        X = physicsPos.X,
                        Y = zone.SurfaceY + 0.04f,
                        Z = physicsPos.Z
                    });
                    AudioSource.PlayClipAtPoint("plat3d_splash", physicsPos, 1.0f, 0.95f);
                }
            }

            if (_sinking)
            {
                // Se hunde sin control hasta el fondo de la zona.
                _body.SetLinearVelocity(new Vector3 { X = 0f, Y = -1.6f, Z = 0f });
                if (physicsPos.Y < _sinkZone.SurfaceY - 0.95f)
                {
                    _sinking = false;
                    Plat3DWaterZoneData zone = _sinkZone;
                    _sinkZone = null!;
                    ApplyDamageInternal(1, withKnockback: false);
                    if (!_dead)
                    {
                        _body.SetTransform(zone.Checkpoint, Plat3DMath.YawQuaternion(0f));
                        _body.SetLinearVelocity(Vector3.Zero);
                        _invulnerability = InvulnerabilityTime;
                        // Republicar ya: el estado de este frame era la
                        // posición del fondo del agua (hallazgo de revisión).
                        PublishState(zone.Checkpoint);
                    }
                }
                return;
            }

            // --- Suelo (raycast) y transporte en plataformas/barca ---
            Vector3 rayOrigin = physicsPos + new Vector3 { X = 0f, Y = 0.1f, Z = 0f };
            Vector3 rayDir = new Vector3 { X = 0f, Y = -1f, Z = 0f };
            RaycastHit groundHit;
            bool grounded = Physics.Physics.Raycast(rayOrigin, rayDir, out groundHit, 0.85f, EntityId);

            if (grounded && Plat3DGameState.PlatformDeltas.TryGetValue(groundHit.EntityId, out Vector3 delta))
            {
                Vector3 transported = physicsPos + delta;
                _body.SetTransform(transported, physicsRot);
                physicsPos = transported;
                PublishState(physicsPos);
            }

            // --- Pinchos: el suelo que pisa daña (empuje hacia atrás) ---
            if (grounded && Plat3DGameState.SpikeIds.Contains(groundHit.EntityId))
            {
                TakeDamage(1, physicsPos + _facing);
            }

            Vector3 currentVelocity;
            _body.GetLinearVelocity(out currentVelocity);

            // --- Entrada y movimiento ---
            float inputX = Input.GetAxisHorizontal();
            float inputZ = Input.GetAxisVertical();

            float vx;
            float vz;
            float vy = currentVelocity.Y;

            if (Plat3DGameState.FirstPerson)
            {
                // 1ª persona: A/D giran, W/S avanzan y retroceden.
                _facingYaw += inputX * TurnSpeedFp * deltaTime;
                UpdateFacingVector();
                vx = _facing.X * inputZ * MoveSpeed;
                vz = _facing.Z * inputZ * MoveSpeed;
            }
            else
            {
                // 3ª persona: movimiento en ejes de mundo; la mirada es
                // la dirección de movimiento.
                vx = inputX * MoveSpeed;
                vz = inputZ * MoveSpeed;
                if (inputX != 0f || inputZ != 0f)
                {
                    _facingYaw = (float)Math.Atan2(inputX, inputZ);
                    UpdateFacingVector();
                }
            }

            // Rotación visual: se aplica al CUERPO (el sync de física la
            // copia a la escena; escribir solo la escena sería pisado).
            Quaternion yawQuat = Plat3DMath.YawQuaternion(_facingYaw);
            _body.SetTransform(physicsPos, yawQuat);

            // --- Salto ---
            bool jumpPressed = Input.GetButton("jump");
            if (jumpPressed && grounded && _jumpCooldown <= 0f)
            {
                vy = JumpVelocity;
                _jumpCooldown = JumpCooldownTime;
                AudioSource.PlayClipAtPoint("plat3d_jump", physicsPos, 1.0f, 1.0f);
            }

            // --- Salto de pared (wall-jump, v2) ---
            // Solo con flanco de subida de la pulsación: mantener espacio
            // tras un salto normal no dispara rebotes accidentales contra
            // cualquier plataforma alta. El rayo sale en la dirección del
            // input (en 1ª persona, en la de la mirada) y no vale contra
            // enemigos, pinchos ni proyectiles: solo geometría del nivel.
            // La velocidad del rebote se aplica ya, como el knockback, y
            // el bloque final de velocidad la respeta este frame.
            if (jumpPressed && !_wasJumpPressed && !grounded && _jumpCooldown <= 0f)
            {
                Vector3 wallOrigin = physicsPos + new Vector3 { X = 0f, Y = 0.2f, Z = 0f };
                Vector3 wallDir = Plat3DGameState.FirstPerson
                    ? _facing
                    : (inputX != 0f || inputZ != 0f)
                        ? Plat3DMath.Normalize(new Vector3 { X = inputX, Y = 0f, Z = inputZ })
                        : _facing;
                RaycastHit wallHit;
                if (Physics.Physics.Raycast(wallOrigin, wallDir, out wallHit, WallCheckDistance, EntityId)
                    && !Plat3DGameState.Enemies.ContainsKey(wallHit.EntityId)
                    && !Plat3DGameState.SpikeIds.Contains(wallHit.EntityId)
                    && !IsProjectile(wallHit.EntityId))
                {
                    _body.SetLinearVelocity(new Vector3
                    {
                        X = -wallDir.X * WallJumpPushSpeed,
                        Y = JumpVelocity,
                        Z = -wallDir.Z * WallJumpPushSpeed
                    });
                    _applyWallPushThisFrame = true;
                    _jumpCooldown = JumpCooldownTime;
                    AudioSource.PlayClipAtPoint("plat3d_jump", physicsPos, 1.0f, 1.05f);
                }
            }
            _wasJumpPressed = jumpPressed;

            // --- Espada (clic izquierdo, raycast frontal) ---
            if (Input.GetMouseButtonDown(0) && _swordCooldown <= 0f)
            {
                _swordCooldown = SwordCooldownTime;
                AudioSource.PlayClipAtPoint("plat3d_sword", physicsPos, 1.0f, 1.0f);

                Vector3 swordOrigin = physicsPos
                    + new Vector3 { X = 0f, Y = 0.35f, Z = 0f }
                    + _facing * 0.55f;

                RaycastHit swordHit;
                if (Physics.Physics.Raycast(swordOrigin, _facing, out swordHit, 2.0f, EntityId))
                {
                    if (Plat3DGameState.Enemies.TryGetValue(swordHit.EntityId, out Plat3DEnemy? enemy))
                    {
                        enemy.TakeDamage(1);
                        AudioSource.PlayClipAtPoint("plat3d_sword_hit", physicsPos, 1.0f, 1.0f);
                    }
                    else
                    {
                        AudioSource.PlayClipAtPoint("plat3d_sword_hit", physicsPos, 0.5f, 1.0f);
                    }
                }
            }

            // --- Proyectil (borde de Shift) ---
            bool runPressed = Input.GetButton("run");
            bool runEdge = runPressed && !_wasRunPressed;
            _wasRunPressed = runPressed;

            if (runEdge && _throwCooldown <= 0f)
            {
                _throwCooldown = ThrowCooldownTime;

                Vector3 throwOrigin = physicsPos
                    + new Vector3 { X = 0f, Y = 0.45f, Z = 0f }
                    + _facing * 0.8f;

                Vector3 throwVelocity = _facing * 10.5f + new Vector3 { X = 0f, Y = 4.2f, Z = 0f };

                if (Plat3DGameState.TryLaunchProjectile(throwOrigin, throwVelocity))
                {
                    AudioSource.PlayClipAtPoint("plat3d_throw", physicsPos, 1.0f, 1.0f);
                }
            }

            // --- Caída al vacío ---
            if (physicsPos.Y < FallDeathY)
            {
                ApplyDamageInternal(1, withKnockback: false);
                if (!_dead)
                {
                    _body.SetTransform(_spawnPosition, Plat3DMath.YawQuaternion(0f));
                    _body.SetLinearVelocity(Vector3.Zero);
                    _invulnerability = InvulnerabilityTime;
                    _applyKnockbackThisFrame = false;
                    // Republicar ya: el estado de este frame era la
                    // posición de la caída (hallazgo de revisión).
                    PublishState(_spawnPosition);
                }
                return;
            }

            // --- Aplicar velocidad final ---
            if (_applyKnockbackThisFrame)
            {
                _applyKnockbackThisFrame = false;
            }
            else if (_applyWallPushThisFrame)
            {
                // La velocidad del rebote ya se aplicó al detectarlo;
                // aquí solo se evita pisarla con la velocidad normal.
                _applyWallPushThisFrame = false;
            }
            else
            {
                _body.SetLinearVelocity(new Vector3 { X = vx, Y = vy, Z = vz });
            }
        }

        public void TakeDamage(int amount, Vector3 fromDirection)
        {
            if (_invulnerability > 0f || _dead || _sinking)
            {
                return;
            }
            ApplyDamageInternal(amount, withKnockback: true, fromDirection);
        }

        private void ApplyDamageInternal(int amount, bool withKnockback, Vector3 fromDirection = default)
        {
            Plat3DGameState.PlayerHealth -= amount;

            if (Plat3DGameState.PlayerHealth <= 0)
            {
                _dead = true;
                AudioSource.PlayClipAtPoint("plat3d_death", Position, 1.0f, 1.0f);
                SceneManager.LoadScene(SceneManager.GetActiveSceneName());
                return;
            }

            _invulnerability = InvulnerabilityTime;
            AudioSource.PlayClipAtPoint("plat3d_hurt", Position, 1.0f, 1.0f);

            if (withKnockback && _body != null && _body.IsValid)
            {
                Vector3 playerPos;
                Quaternion playerRot;
                if (_body.GetTransform(out playerPos, out playerRot))
                {
                    Vector3 diff = new Vector3
                    {
                        X = playerPos.X - fromDirection.X,
                        Y = 0f,
                        Z = playerPos.Z - fromDirection.Z
                    };

                    Vector3 pushDir = Plat3DMath.Length(diff) > 0.0001f
                        ? Plat3DMath.Normalize(diff)
                        : new Vector3 { X = 0f, Y = 0f, Z = 1f };

                    Vector3 knockbackVelocity = pushDir * KnockbackSpeed;
                    knockbackVelocity.Y = KnockbackVy;

                    _body.SetLinearVelocity(knockbackVelocity);
                    _applyKnockbackThisFrame = true;
                }
            }
        }

        private void PublishState(Vector3 physicsPos)
        {
            Plat3DGameState.PlayerPosition = physicsPos;
            Plat3DGameState.PlayerPositionValid = true;
            Plat3DGameState.PlayerFacing = _facing;
            Plat3DGameState.PlayerFacingYaw = _facingYaw;
        }

        private void UpdateFacingVector()
        {
            _facing = new Vector3
            {
                X = MathF.Sin(_facingYaw),
                Y = 0f,
                Z = MathF.Cos(_facingYaw)
            };
        }

        private static bool IsProjectile(ulong entityId)
        {
            var pool = Plat3DGameState.ProjectilePool;
            for (int i = 0; i < pool.Count; i++)
            {
                if (pool[i].EntityId == entityId)
                {
                    return true;
                }
            }
            return false;
        }

        private Plat3DWaterZoneData? ZoneAt(Vector3 pos)
        {
            for (int i = 0; i < Plat3DGameState.WaterZones.Count; i++)
            {
                if (Plat3DGameState.WaterZones[i].ContainsXZ(pos))
                {
                    return Plat3DGameState.WaterZones[i];
                }
            }
            return null;
        }

        private void TickTimers(float deltaTime)
        {
            if (_jumpCooldown > 0f) _jumpCooldown -= deltaTime;
            if (_swordCooldown > 0f) _swordCooldown -= deltaTime;
            if (_throwCooldown > 0f) _throwCooldown -= deltaTime;
            if (_invulnerability > 0f) _invulnerability -= deltaTime;
        }
    }
}
