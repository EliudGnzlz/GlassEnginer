using System;
using GlassEngine.Core;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;
using UnityEngine;
using Quaternion = GlassEngine.Core.Interop.Quaternion;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts;

/// <summary>
/// Prueba en vivo de los criterios «mejor que Unity» del
/// CharacterController: viaje en plataforma horizontal, bajada
/// caminando con suelta natural, teletransporte directo al ascensor,
/// viaje en ascensor sin despegarse y teletransporte limpio de vuelta
/// al suelo. Las plataformas son cuerpos cinemáticos creados aquí y
/// movidos por SetTransform cada frame, como haría un script de juego.
/// </summary>
public class PlataformasCCTest : GlassBehaviour
{
    private const ulong IdPlataformaH = 40;
    private const ulong IdAscensor = 41;

    private CharacterController _cc = null!;
    private RigidBody _platH = null!;
    private RigidBody _ascensor = null!;
    private float _t;
    private float _vy;
    private float _platHX;
    private float _ascY = 0.75f;
    private bool _vioAire;
    private bool _globalOk = true;
    private int _fase;
    private bool _fin;

    private static string F(float v) => v.ToString("F2", System.Globalization.CultureInfo.InvariantCulture);

    private void Resultado(string nombre, bool ok, string detalle)
    {
        if (!ok) _globalOk = false;
        Debug.Log($"[PLATFTEST] RESULTADO {nombre} {(ok ? "OK" : "FALLO")} {detalle}");
    }

    public override void Start()
    {
        _platH = new RigidBody();
        _platH.Initialize(IdPlataformaH, RigidBodyType.Kinematic, new Vector3(0f, 0.75f, 0f), Quaternion.Identity);
        _platH.AttachBoxCollider(new Vector3(1.5f, 0.25f, 1.5f), restitution: 0f, friction: 0.5f);

        _ascensor = new RigidBody();
        _ascensor.Initialize(IdAscensor, RigidBodyType.Kinematic, new Vector3(9f, 0.75f, 0f), Quaternion.Identity);
        _ascensor.AttachBoxCollider(new Vector3(1.5f, 0.25f, 1.5f), restitution: 0f, friction: 0.5f);

        _cc = new CharacterController
        {
            Radius = 0.35f,
            Height = 1.8f,
            SkinWidth = 0.02f,
            StepOffset = 0.4f,
            SlopeLimit = 45f,
            SnapDistance = 0.3f,
        };
        _cc.Initialize(EntityId, Position);
        Debug.Log($"[PLATFTEST] inicio entidad {EntityId}");
    }

    public override void Update(float deltaTime)
    {
        if (_fin || _cc == null)
            return;
        _t += deltaTime;

        // Las plataformas se mueven ANTES del Move del personaje,
        // como en un juego (otros scripts actualizan el mundo primero).
        if (_t < 4.5f)
        {
            _platHX += 1.2f * deltaTime;
            _platH.SetTransform(new Vector3(_platHX, 0.75f, 0f), Quaternion.Identity);
        }
        if (_t >= 4.5f && _t < 6.0f)
        {
            _ascY += 1.2f * deltaTime;
            _ascensor.SetTransform(new Vector3(9f, _ascY, 0f), Quaternion.Identity);
        }

        _vy = Math.Max(_vy - 9.81f * deltaTime, -20f);
        if (_cc.IsGrounded && _vy < -1f)
            _vy = -1f;

        Vector3 motion = new Vector3(0f, _vy * deltaTime, 0f);
        if (_fase == 2)
            motion = new Vector3(-1.6f * deltaTime, _vy * deltaTime, 0f);

        _cc.Move(motion, deltaTime);
        Position = _cc.Position;

        if (_fase == 3 && _t > 4.6f && !_cc.IsGrounded)
            _vioAire = true;

        switch (_fase)
        {
            case 0 when _t >= 0.5f:
                _fase = 1;
                break;
            case 1 when _t >= 2.5f:
                Resultado("viaje_horizontal",
                    Math.Abs(_cc.Position.X - _platHX) < 0.08f && _cc.IsGrounded,
                    $"charX={F(_cc.Position.X)} platX={F(_platHX)}");
                _fase = 2;
                break;
            case 2 when _t >= 4.5f:
                Resultado("bajada_caminando",
                    _cc.IsGrounded && Math.Abs(_cc.Position.Y - 0.92f) < 0.06f,
                    $"y={F(_cc.Position.Y)} grounded={_cc.IsGrounded}");
                _cc.Teleport(new Vector3(9f, 1.91f, 0f));
                _fase = 3;
                break;
            case 3 when _t >= 6.0f:
                Resultado("ascensor",
                    !_vioAire && Math.Abs(_cc.Position.Y - 3.71f) < 0.08f && _cc.IsGrounded,
                    $"y={F(_cc.Position.Y)} vioAire={_vioAire}");
                _cc.Teleport(new Vector3(-6f, 0.92f, 0f));
                _fase = 4;
                break;
            case 4 when _t >= 7.0f:
                Resultado("teletransporte_limpio",
                    _cc.IsGrounded && Math.Abs(_cc.Position.X + 6f) < 0.03f,
                    $"x={F(_cc.Position.X)} grounded={_cc.IsGrounded}");
                Debug.Log($"[PLATFTEST] RESULTADO GLOBAL {(_globalOk ? "OK" : "FALLO")}");
                Debug.Log("[PLATFTEST] FIN");
                _fin = true;
                break;
        }
    }
}
