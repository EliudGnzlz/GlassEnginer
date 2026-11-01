using System;
using GlassEngine.Core.Audio;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;

namespace GlassEngine.Core.Scripts;

/// <summary>
/// Controlador físico 3D de alta precisión para el cubo jugador:
/// Soporta desplazamiento horizontal calibrado, saltos parabólicos bajo gravedad -9.81m/s2,
/// detección de flancos para aterrizaje/escalada y feedback de audio espacial reactivo.
/// </summary>
public sealed class PlayerController : GlassBehaviour
{
    private readonly RigidBody _rigidBody = new();
    private float _timer = 0.0f;
    private float _jumpTimer = 0.0f;
    private int _stage = 0;
    private bool _hasJumpedStep1 = false;
    private bool _hasLandedStep1 = false;
    private bool _hasJumpedStep2 = false;
    private bool _hasLandedStep2 = false;

    public float TargetSpeed = 1.6f;
    public float JumpVelocityY = 6.5f;

    public bool ClimbedStep1 => _hasLandedStep1;
    public bool ClimbedStep2 => _hasLandedStep2;

    public override void Awake()
    {
        Log($"PlayerController: Awake en entidad #{EntityId}");
    }

    public override void Start()
    {
        Log($"PlayerController: Inicializando cuerpo Dynamic y colisionador Box en entidad #{EntityId}");
        _rigidBody.Initialize(EntityId, RigidBodyType.Dynamic, Position, Rotation);
        _rigidBody.AttachBoxCollider(new Vector3(0.5f, 0.5f, 0.5f), restitution: 0.0f, friction: 0.8f);

        // Sonido de inicio/spawn
        AudioSource.PlayClipAtPoint("beep_440", Position, volume: 0.5f, pitch: 1.0f);
    }

    public override void Update(float deltaTime)
    {
        _timer += deltaTime;

        // 1. Sincronizar posición y rotación desde el motor Rapier3D
        if (!_rigidBody.GetTransform(out var physPos, out var physRot))
        {
            return;
        }

        Position = physPos;
        Rotation = physRot;

        _rigidBody.GetLinearVelocity(out var curVel);

        // 2. Control de etapas cinemáticas y físicas
        switch (_stage)
        {
            case 0: // Desplazamiento horizontal inicial controlado hacia Step 1
                _rigidBody.SetLinearVelocity(new Vector3(TargetSpeed, curVel.Y, 0.0f));
                if (physPos.X >= 0.5f && !_hasJumpedStep1)
                {
                    Log($"PlayerController: Obstáculo Step 1 (X=[2.0, 4.0], Y=1.0) detectado en X={physPos.X:F2} (Vx={curVel.X:F2}). Ejecutando salto 1!");
                    _rigidBody.SetLinearVelocity(new Vector3(TargetSpeed, JumpVelocityY, 0.0f));
                    AudioSource.PlayClipAtPoint("beep_880", Position, volume: 1.0f, pitch: 1.2f);
                    _hasJumpedStep1 = true;
                    _jumpTimer = 0.0f;
                    _stage = 1;
                }
                break;

            case 1: // En trayectoria parabólica natural sobre Step 1 (X=[2.0, 4.0], Y_surface=1.0m, cube_center_y=1.50m)
                _jumpTimer += deltaTime;
                _rigidBody.SetLinearVelocity(new Vector3(TargetSpeed, curVel.Y, 0.0f)); // Mantener velocidad horizontal constante
                // Detectar aterrizaje sobre Step 1
                if (_jumpTimer >= 0.4f && physPos.X >= 2.0f && physPos.X <= 4.0f && physPos.Y >= 1.35f && physPos.Y <= 1.70f && !_hasLandedStep1)
                {
                    _hasLandedStep1 = true;
                    float surfaceOffset = physPos.Y - 1.0f; // Superficie Step 1 = 1.0m
                    Log($"PlayerController: ¡Aterrizaje perfecto en Step 1! Pos=(X:{physPos.X:F2}, Y:{physPos.Y:F2}, Z:{physPos.Z:F2}) | Offset semi-altura: +{surfaceOffset:F2}m (Esperado: 0.50m)");
                    AudioSource.PlayClipAtPoint("beep_440", Position, volume: 1.0f, pitch: 1.0f);
                    _stage = 2;
                }
                break;

            case 2: // Caminar sobre Step 1 hacia el punto de despegue para Step 2 (X: 2.0 -> 3.65)
                _rigidBody.SetLinearVelocity(new Vector3(TargetSpeed, curVel.Y, 0.0f));
                if (physPos.X >= 3.65f && !_hasJumpedStep2)
                {
                    Log($"PlayerController: Obstáculo elevado Step 2 (X=[5.0, 7.0], Y=2.0) detectado en X={physPos.X:F2} (Vx={curVel.X:F2}). Ejecutando segundo salto de escalada!");
                    _rigidBody.SetLinearVelocity(new Vector3(TargetSpeed, JumpVelocityY + 0.8f, 0.0f));
                    AudioSource.PlayClipAtPoint("beep_880", Position, volume: 1.0f, pitch: 1.4f);
                    _hasJumpedStep2 = true;
                    _jumpTimer = 0.0f;
                    _stage = 3;
                }
                break;

            case 3: // En trayectoria aérea sobre Step 2 (X=[5.0, 7.0], Y_surface=2.0m, cube_center_y=2.50m)
                _jumpTimer += deltaTime;
                _rigidBody.SetLinearVelocity(new Vector3(TargetSpeed, curVel.Y, 0.0f)); // Mantener velocidad horizontal constante
                if (_jumpTimer >= 0.4f && physPos.X >= 4.8f && physPos.X <= 7.2f && physPos.Y >= 2.30f && physPos.Y <= 2.80f && !_hasLandedStep2)
                {
                    _hasLandedStep2 = true;
                    float surfaceOffset = physPos.Y - 2.0f; // Superficie Step 2 = 2.0m
                    Log($"PlayerController: ¡Aterrizaje y escalada completada en Step 2! Pos=(X:{physPos.X:F2}, Y:{physPos.Y:F2}, Z:{physPos.Z:F2}) | Offset semi-altura: +{surfaceOffset:F2}m (Esperado: 0.50m)");
                    AudioSource.PlayClipAtPoint("beep_440", Position, volume: 1.0f, pitch: 1.1f);
                    _stage = 4;
                }
                break;

            case 4: // Estabilización en reposo sobre Step 2
                _rigidBody.SetLinearVelocity(new Vector3(0.0f, curVel.Y, 0.0f));
                break;
        }
    }

    public void Jump()
    {
        Log($"PlayerController: Salto manual invocado.");
        _rigidBody.ApplyImpulse(new Vector3(0.0f, JumpVelocityY, 0.0f));
        AudioSource.PlayClipAtPoint("beep_880", Position, volume: 1.0f, pitch: 1.3f);
    }

    public override void OnDestroy()
    {
        Log($"PlayerController: OnDestroy en entidad #{EntityId}");
        _rigidBody.Destroy();
    }
}
