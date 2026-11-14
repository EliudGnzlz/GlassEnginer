using System;
using GlassEngine.Core;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;
using UnityEngine;
using Quaternion = GlassEngine.Core.Interop.Quaternion;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts;

/// <summary>
/// Prueba en vivo del bloque de huecos beta RB/CC:
/// masa autorada en escena que se nota en un choque, CCD de
/// script contra pared fina (con gemelo discreto que la tunela),
/// ForceMode VelocityChange exacto con masa 5, CharacterController
/// que recibe OnControllerColliderHit contra la pared, e
/// interpolación activada sin romper el sync.
/// </summary>
public class HuecosBetaTest : GlassBehaviour
{
    private CharacterController _cc = null!;
    private RigidBody _ccd = null!;
    private RigidBody _tunel = null!;
    private RigidBody _velChange = null!;
    private float _t;
    private bool _montado;
    private bool _hitPared;
    private bool _globalOk = true;
    private int _fase;

    private static string F(float v) => v.ToString("F2", System.Globalization.CultureInfo.InvariantCulture);

    private void Resultado(string nombre, bool ok, string detalle)
    {
        if (!ok) _globalOk = false;
        Debug.Log($"[HUECOS] RESULTADO {nombre} {(ok ? "OK" : "FALLO")} {detalle}");
    }

    public override void Start()
    {
        // Proyectiles del carril CCD (z=-3), pared fina en x=8.
        _ccd = new RigidBody();
        _ccd.Initialize(30, RigidBodyType.Dynamic, new Vector3(5f, 0.5f, -3f), Quaternion.Identity);
        _ccd.AttachSphereCollider(0.05f, restitution: 0f, friction: 0f);
        _ccd.Mass = 0.1f;
        _ccd.LinearDamping = 0f;
        _ccd.UseGravity = false;
        _ccd.CollisionDetection = CollisionDetectionMode.Continuous;
        _ccd.Interpolate = true;
        _ccd.SetLinearVelocity(new Vector3(80f, 0f, 0f));

        _tunel = new RigidBody();
        _tunel.Initialize(31, RigidBodyType.Dynamic, new Vector3(5f, 0.5f, -4.5f), Quaternion.Identity);
        _tunel.AttachSphereCollider(0.05f, restitution: 0f, friction: 0f);
        _tunel.Mass = 0.1f;
        _tunel.LinearDamping = 0f;
        _tunel.UseGravity = false;
        _tunel.SetLinearVelocity(new Vector3(80f, 0f, 0f));

        // VelocityChange: la velocidad queda en 3 m/s aunque pese 5.
        _velChange = new RigidBody();
        _velChange.Initialize(32, RigidBodyType.Dynamic, new Vector3(0f, 5f, -10f), Quaternion.Identity);
        _velChange.AttachBoxCollider(new Vector3(0.5f, 0.5f, 0.5f), restitution: 0f, friction: 0f);
        _velChange.Mass = 5f;
        _velChange.LinearDamping = 0f;
        _velChange.UseGravity = false;
        _velChange.ApplyImpulse(new Vector3(3f, 0f, 0f), ForceMode.VelocityChange);

        // Personaje caminando hacia la pared del carril z=+4 (x=3).
        _cc = new CharacterController
        {
            Radius = 0.35f,
            Height = 1.8f,
            SkinWidth = 0.02f,
            StepOffset = 0.4f,
            SlopeLimit = 45f,
        };
        _cc.Initialize(EntityId, new Vector3(0f, 0.92f, 4f));
        _montado = true;
    }

    public override void OnControllerColliderHit(ControllerColliderHit hit)
    {
        if (Math.Abs(hit.normal.y) < 0.3f && !_hitPared)
        {
            _hitPared = true;
            Debug.Log($"[HUECOS] HIT_CC normal=({F(hit.normal.x)},{F(hit.normal.y)},{F(hit.normal.z)}) entidad={hit.gameObject.EntityId}");
        }
    }

    public override void Update(float deltaTime)
    {
        if (!_montado) return;
        _t += deltaTime;

        // El personaje avanza con presión de gravedad, como un juego.
        _cc.Move(new Vector3(2.2f * deltaTime, -1.0f * deltaTime, 0f), deltaTime);

        if (_fase == 0 && _t > 0.3f)
        {
            _fase = 1;
            bool okV = _velChange.GetLinearVelocity(out Vector3 v) && Math.Abs(v.X - 3f) < 0.15f;
            Resultado("velocity_change", okV, $"v_x={F(v.X)} con masa 5");
        }
        else if (_fase == 1 && _t > 1.2f)
        {
            _fase = 2;
            // Los cuerpos creados por script no viven en la escena:
            // el veredicto va por velocidad (el CCD queda frenado
            // contra la pared; el discreto sigue volando).
            bool okCcd = _ccd.GetLinearVelocity(out Vector3 vc) && Math.Abs(vc.X) < 5f;
            Resultado("ccd", okCcd, $"proyectil_ccd_vx={F(vc.X)} (pared en 8)");
            bool okTunel = _tunel.GetLinearVelocity(out Vector3 vt) && vt.X > 40f;
            Resultado("tunel_sin_ccd", okTunel, $"proyectil_discreto_vx={F(vt.X)}");
            Resultado("hit_cc", _hitPared, "OnControllerColliderHit con normal horizontal");
            bool okFlags = (_cc.LastFlags & CollisionFlags.Sides) != 0 || _hitPared;
            Resultado("flags_cc", okFlags, $"ultimos_flags={_cc.LastFlags} vel_cc=({F(_cc.Velocity.X)},{F(_cc.Velocity.Y)})");
        }
        else if (_fase == 2 && _t > 2.4f)
        {
            _fase = 3;
            Vector3 pp = Vector3.Zero, pl = Vector3.Zero;
            bool okPos = CajaRegistro.Posiciones.TryGetValue(10, out pp) && CajaRegistro.Posiciones.TryGetValue(11, out pl);
            if (okPos)
            {
                Resultado("masa_escena", pl.X > 0.5f && pp.X > -2.6f,
                    $"pesada_x={F(pp.X)} ligera_x={F(pl.X)} (arranque -4 y -1)");
            }
            else
            {
                Resultado("masa_escena", false, "sin posiciones registradas");
            }
            Debug.Log($"[HUECOS] GLOBAL {(_globalOk ? "OK" : "FALLO")}");
        }
    }
}
