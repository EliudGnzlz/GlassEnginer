using System;
using GlassEngine.Core;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;
using UnityEngine;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts;

public class CharacterTest : GlassBehaviour
{
    private CharacterController _cc = null!;
    private int _phase = -1;
    private float _phaseTime;
    private float _vy;
    private bool _finished;
    private bool _vioAire;

    private static string F(float v) => v.ToString("F2", System.Globalization.CultureInfo.InvariantCulture);

    public override void Start()
    {
        _cc = NewController(0.4f, 45f);
        _cc.Initialize(EntityId, Position);
        Debug.Log($"[CHARTEST] inicio entidad {EntityId}");
        EnterPhase(0);
    }

    public override void Update(float deltaTime)
    {
        if (_finished || _cc == null)
            return;

        if (_phase == 5 && !_cc.IsGrounded)
            _vioAire = true;

        // Gravedad y presión de apoyo
        _vy = Math.Max(_vy - 9.81f * deltaTime, -20f);
        if (_cc.IsGrounded && _vy < -1f)
            _vy = -1f;

        Vector3 motion = new Vector3(0f, _vy * deltaTime, 0f);

        switch (_phase)
        {
            case 1:
                motion = new Vector3(0f, _vy * deltaTime, -2f * deltaTime);
                break;
            case 2:
                float d = 2f / (float)Math.Sqrt(2.0);
                motion = new Vector3(-d * deltaTime, _vy * deltaTime, -d * deltaTime);
                break;
            case 3:
            case 4:
                motion = new Vector3(1.6f * deltaTime, _vy * deltaTime, 0f);
                break;
            case 6:
            case 7:
                motion = new Vector3(0f, _vy * deltaTime, -1.6f * deltaTime);
                break;
        }

        _cc.Move(motion, deltaTime);
        Position = _cc.Position;

        if (_phase == 5 && !_cc.IsGrounded)
            _vioAire = true;

        _phaseTime += deltaTime;
        if (PhaseDone(_phase, _phaseTime))
            CompletePhase();
    }

    private CharacterController NewController(float stepOffset, float slopeLimit)
    {
        var cc = new CharacterController();
        cc.Radius = 0.35f;
        cc.Height = 1.8f;
        cc.SkinWidth = 0.02f;
        cc.StepOffset = stepOffset;
        cc.SlopeLimit = slopeLimit;
        cc.SnapDistance = 0.3f;
        return cc;
    }

    private void EnterPhase(int phase)
    {
        _phase = phase;
        _phaseTime = 0f;
        Debug.Log($"[CHARTEST] fase {phase}");

        switch (phase)
        {
            case 3:
                _cc.Teleport(new Vector3(5f, 0.9f, 6f));
                break;
            case 4:
                _cc.Destroy();
                _cc = NewController(0.05f, 45f);
                _cc.Initialize(EntityId, new Vector3(5f, 0.9f, 6f));
                break;
            case 5:
                _cc.Teleport(new Vector3(14.7f, 5.5f, 6f));
                _vy = 0f;
                _vioAire = false;
                break;
            case 6:
                _cc.Destroy();
                _cc = NewController(0.4f, 45f);
                _cc.Initialize(EntityId, new Vector3(0f, 0.9f, -8.6f));
                break;
            case 7:
                _cc.Destroy();
                _cc = NewController(0.4f, 20f);
                _cc.Initialize(EntityId, new Vector3(0f, 0.9f, -8.6f));
                break;
            case 8:
                Debug.Log("[CHARTEST] FIN");
                _finished = true;
                break;
        }
    }

    private static bool PhaseDone(int phase, float t)
    {
        switch (phase)
        {
            case 0: return t >= 1.0f;
            case 1: return t >= 3.0f;
            case 2: return t >= 2.0f;
            case 3: return t >= 6.0f;
            case 4: return t >= 3.0f;
            case 5: return t >= 2.5f;
            case 6: return t >= 3.5f;
            case 7: return t >= 3.5f;
            default: return false;
        }
    }

    private void CompletePhase()
    {
        int p = _phase;

        switch (p)
        {
            case 0:
            {
                bool ok = _cc.IsGrounded && Math.Abs(_cc.Position.Y - 0.90f) < 0.06f;
                Debug.Log($"[CHARTEST] RESULTADO reposo {(ok ? "OK" : "FALLO")} y={F(_cc.Position.Y)} grounded={_cc.IsGrounded}");
                break;
            }
            case 1:
            {
                bool ok = _cc.Position.Z >= 1.80f && _cc.Position.Z <= 1.94f && Math.Abs(_cc.Position.X) < 0.1f;
                Debug.Log($"[CHARTEST] RESULTADO pared {(ok ? "OK" : "FALLO")} z={F(_cc.Position.Z)} x={F(_cc.Position.X)}");
                break;
            }
            case 2:
            {
                bool ok = _cc.Position.Z >= 1.78f && _cc.Position.Z <= 1.96f && _cc.Position.X < -1.5f;
                Debug.Log($"[CHARTEST] RESULTADO desliza {(ok ? "OK" : "FALLO")} z={F(_cc.Position.Z)} x={F(_cc.Position.X)}");
                break;
            }
            case 3:
            {
                bool ok = _cc.Position.Y > 3.5f && _cc.Position.X > 13f;
                Debug.Log($"[CHARTEST] RESULTADO escalera {(ok ? "OK" : "FALLO")} x={F(_cc.Position.X)} y={F(_cc.Position.Y)}");
                break;
            }
            case 4:
            {
                bool ok = _cc.Position.X < 7.5f && _cc.Position.Y < 1.3f;
                Debug.Log($"[CHARTEST] RESULTADO escalon {(ok ? "OK" : "FALLO")} x={F(_cc.Position.X)} y={F(_cc.Position.Y)}");
                break;
            }
            case 5:
            {
                bool ok = _vioAire && _cc.IsGrounded && Math.Abs(_cc.Position.Y - 3.70f) < 0.08f;
                Debug.Log($"[CHARTEST] RESULTADO caida {(ok ? "OK" : "FALLO")} y={F(_cc.Position.Y)} grounded={_cc.IsGrounded}");
                break;
            }
            case 6:
            {
                bool ok = _cc.Position.Y > 1.8f;
                Debug.Log($"[CHARTEST] RESULTADO rampa {(ok ? "OK" : "FALLO")} y={F(_cc.Position.Y)} z={F(_cc.Position.Z)}");
                break;
            }
            case 7:
            {
                bool ok = _cc.Position.Y < 1.35f;
                Debug.Log($"[CHARTEST] RESULTADO rampa_bloquea {(ok ? "OK" : "FALLO")} y={F(_cc.Position.Y)} z={F(_cc.Position.Z)}");
                break;
            }
        }

        if (p < 8)
            EnterPhase(p + 1);
    }
}
