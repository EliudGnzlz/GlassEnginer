using System;
using GlassEngine.Core.Audio;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;

namespace GlassEngine.Core.Scripts;

/// <summary>
/// Controlador físico 3D interactivo para el cubo jugador:
/// - Control interactivo completo por teclado: WASD / Flechas para desplazarse y [Espacio] para saltar.
/// - Colisión y escalada sobre plataformas con Rapier3D.
/// - Feedback reactivo de audio 3D (beep_880 en salto, beep_440 en aterrizaje).
/// - Modo AutoDemo para suites de prueba automatizadas.
/// </summary>
public sealed class PlayerController : GlassBehaviour
{
    private readonly RigidBody _rigidBody = new();
    private float _timer = 0.0f;
    private float _jumpTimer = 0.0f;
    private float _jumpCooldown = 0.0f;
    private bool _wasGrounded = true;

    // Estado para pruebas automatizadas (--test-platformer)
    public static bool GlobalAutoDemo = false;
    public bool AutoDemo = false;
    private int _stage = 0;
    private bool _hasJumpedStep1 = false;
    private bool _hasLandedStep1 = false;
    private bool _hasJumpedStep2 = false;
    private bool _hasLandedStep2 = false;

    public float MoveSpeed = 3.8f;
    public float JumpVelocity = 7.2f;

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

        AudioSource.PlayClipAtPoint("beep_440", Position, volume: 0.5f, pitch: 1.0f);
    }

    public override void Update(float deltaTime)
    {
        _timer += deltaTime;
        if (_jumpCooldown > 0f) _jumpCooldown -= deltaTime;

        // 0. Sincronizar parámetros expuestos del script en tiempo real desde el Inspector
        if (RuntimeBridge.GetScriptParamsNative != null)
        {
            unsafe
            {
                ScriptParamsData p;
                if (RuntimeBridge.GetScriptParamsNative(&p) == 0)
                {
                    if (p.MoveSpeed > 0.1f) MoveSpeed = p.MoveSpeed;
                    if (p.JumpVelocity > 0.1f) JumpVelocity = p.JumpVelocity;
                    AutoDemo = p.AutoDemo != 0;
                }
            }
        }

        // 1. Sincronizar posición y rotación desde Rapier3D
        if (!_rigidBody.GetTransform(out var physPos, out var physRot))
        {
            return;
        }

        Position = physPos;
        Rotation = physRot;
        _rigidBody.GetLinearVelocity(out var curVel);

        // 2. Comprobar si el suelo está bajo el cubo (Suelo, Step 1 o Step 2)
        bool onGround = physPos.Y >= 0.40f && physPos.Y <= 0.65f;
        bool onStep1 = physPos.X >= 1.8f && physPos.X <= 4.2f && physPos.Y >= 1.35f && physPos.Y <= 1.70f;
        bool onStep2 = physPos.X >= 4.8f && physPos.X <= 7.2f && physPos.Y >= 2.35f && physPos.Y <= 2.75f;
        bool isGrounded = (onGround || onStep1 || onStep2) && Math.Abs(curVel.Y) < 0.4f;

        // Detectar si aterrizó para audio feedback
        if (!_wasGrounded && isGrounded && curVel.Y <= 0.1f)
        {
            AudioSource.PlayClipAtPoint("beep_440", Position, volume: 0.8f, pitch: 1.0f);
            Log($"[PlayerController] Aterrizaje en superficie detectado en Pos=({physPos.X:F2}, {physPos.Y:F2}, {physPos.Z:F2})");
        }
        _wasGrounded = isGrounded;

        // 3. Control de modo: Si se ejecuta suite de prueba (--test-platformer) o AutoDemo, simulación guiada
        bool isTestMode = GlobalAutoDemo || AutoDemo || Environment.GetEnvironmentVariable("GLASS_AUTO_DEMO") == "1";
        if (isTestMode)
        {
            UpdateAutoDemo(deltaTime, physPos, curVel);
            return;
        }

        // 4. CONTROL INTERACTIVO DEL USUARIO (WASD + Espacio)
        float moveX = 0f;
        if (Input.GetKey("d") || Input.GetKey("right")) moveX += 1f;
        if (Input.GetKey("a") || Input.GetKey("left")) moveX -= 1f;

        float moveZ = 0f;
        if (Input.GetKey("w") || Input.GetKey("up")) moveZ += 1f;
        if (Input.GetKey("s") || Input.GetKey("down")) moveZ -= 1f;

        bool jumpPressed = Input.GetKey("space") || Input.GetButton("jump");

        float targetVx = moveX * MoveSpeed;
        float targetVz = moveZ * MoveSpeed;
        float targetVy = curVel.Y;

        if (jumpPressed && isGrounded && _jumpCooldown <= 0f)
        {
            targetVy = JumpVelocity;
            _jumpCooldown = 0.35f;
            _wasGrounded = false;
            AudioSource.PlayClipAtPoint("beep_880", Position, volume: 1.0f, pitch: 1.25f);
            Log($"[PlayerController] ¡Salto interactivo accionado por el usuario [Espacio]! (Vy={JumpVelocity:F1} m/s)");
        }

        // Si el usuario se cae del suelo por los lados, reposicionar
        if (physPos.Y < -5.0f)
        {
            _rigidBody.SetTransform(new Vector3(0.0f, 0.5f, 0.0f), Quaternion.Identity);
            _rigidBody.SetLinearVelocity(Vector3.Zero);
            Log("[PlayerController] Jugador caído al vacío. Respawn en origen.");
            return;
        }

        _rigidBody.SetLinearVelocity(new Vector3(targetVx, targetVy, targetVz));
    }

    private void UpdateAutoDemo(float deltaTime, Vector3 physPos, Vector3 curVel)
    {
        switch (_stage)
        {
            case 0:
                _rigidBody.SetLinearVelocity(new Vector3(1.6f, curVel.Y, 0.0f));
                if (physPos.X >= 0.5f && !_hasJumpedStep1)
                {
                    _rigidBody.SetLinearVelocity(new Vector3(1.6f, 6.5f, 0.0f));
                    AudioSource.PlayClipAtPoint("beep_880", Position, 1.0f, 1.2f);
                    _hasJumpedStep1 = true;
                    _jumpTimer = 0.0f;
                    _stage = 1;
                }
                break;

            case 1:
                _jumpTimer += deltaTime;
                _rigidBody.SetLinearVelocity(new Vector3(1.6f, curVel.Y, 0.0f));
                if (_jumpTimer >= 0.4f && physPos.X >= 2.0f && physPos.X <= 4.0f && physPos.Y >= 1.35f && physPos.Y <= 1.70f && !_hasLandedStep1)
                {
                    _hasLandedStep1 = true;
                    AudioSource.PlayClipAtPoint("beep_440", Position, 1.0f, 1.0f);
                    _stage = 2;
                }
                break;

            case 2:
                _rigidBody.SetLinearVelocity(new Vector3(1.6f, curVel.Y, 0.0f));
                if (physPos.X >= 3.65f && !_hasJumpedStep2)
                {
                    _rigidBody.SetLinearVelocity(new Vector3(1.6f, 7.3f, 0.0f));
                    AudioSource.PlayClipAtPoint("beep_880", Position, 1.0f, 1.4f);
                    _hasJumpedStep2 = true;
                    _jumpTimer = 0.0f;
                    _stage = 3;
                }
                break;

            case 3:
                _jumpTimer += deltaTime;
                _rigidBody.SetLinearVelocity(new Vector3(1.6f, curVel.Y, 0.0f));
                if (_jumpTimer >= 0.4f && physPos.X >= 4.8f && physPos.X <= 7.2f && physPos.Y >= 2.30f && physPos.Y <= 2.80f && !_hasLandedStep2)
                {
                    _hasLandedStep2 = true;
                    AudioSource.PlayClipAtPoint("beep_440", Position, 1.0f, 1.1f);
                    _stage = 4;
                }
                break;

            case 4:
                _rigidBody.SetLinearVelocity(new Vector3(0.0f, curVel.Y, 0.0f));
                _jumpTimer += deltaTime;
                if (_jumpTimer >= 2.5f)
                {
                    _rigidBody.SetTransform(new Vector3(0.0f, 0.5f, 0.0f), Quaternion.Identity);
                    _rigidBody.SetLinearVelocity(Vector3.Zero);
                    _hasJumpedStep1 = false;
                    _hasLandedStep1 = false;
                    _hasJumpedStep2 = false;
                    _hasLandedStep2 = false;
                    _jumpTimer = 0.0f;
                    _stage = 0;
                }
                break;
        }
    }
}
