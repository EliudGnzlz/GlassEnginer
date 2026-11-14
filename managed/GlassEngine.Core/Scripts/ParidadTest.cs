using System;
using GlassEngine.Core;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;
using UnityEngine;
using Quaternion = GlassEngine.Core.Interop.Quaternion;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts;

/// <summary>
/// Prueba en vivo del bloque de paridad RB/CC (superficie C#
/// de los PAQ-A/B/D sobre el Rigidbody y de los PAQ-C/D sobre
/// el CharacterController): velocidad angular, torque, fuerza
/// en un punto, explosión, centro de masa e inercia por código,
/// velocidad en un punto, Move cinemático verificado por sweep,
/// SweepTest/SweepTestAll, topes de velocidad, Sleep/WakeUp,
/// DetectCollisions de cuerpo y de CC, Enabled, Bounds,
/// StepOffset mutable, overlap recovery y capas del CC.
/// Cada chequeo deja una línea [PARIDAD] RESULTADO y al final
/// [PARIDAD] GLOBAL OK/FALLO.
/// </summary>
public class ParidadTest : GlassBehaviour
{
    private RigidBody _spin = null!;
    private RigidBody _torque = null!;
    private RigidBody _forcePoint = null!;
    private RigidBody _explosion = null!;
    private RigidBody _masa = null!;
    private RigidBody _punto = null!;
    private RigidBody _kin = null!;
    private RigidBody _probe = null!;
    private RigidBody _cap = null!;
    private RigidBody _fantasma = null!;
    private RigidBody _normal = null!;
    private CharacterController _cc = null!;

    private float _t;
    private int _fase;
    private bool _montado;
    private bool _globalOk = true;

    private int _ccFase;
    private float _ccT;
    private float _ccX;
    private float _ccY;
    private float _ccYMax;
    private bool _stepBloqueado;
    private bool _kinMovido;

    private static readonly Vector3 CcInicio = new(0f, 0.92f, 4f);
    private static readonly Vector3 PuntoSpin = new(8f, 6f, 12f);

    private static string F(float v) => v.ToString("F2", System.Globalization.CultureInfo.InvariantCulture);

    private static bool Cerca(float v, float esperado, float tol) => Math.Abs(v - esperado) <= tol;

    private void Resultado(string nombre, bool ok, string detalle)
    {
        if (!ok) _globalOk = false;
        Debug.Log($"[PARIDAD] RESULTADO {nombre} {(ok ? "OK" : "FALLO")} {detalle}");
    }

    private RigidBody CrearCuerpo(ulong id, RigidBodyType tipo, Vector3 pos, bool gravedad)
    {
        var rb = new RigidBody();
        rb.Initialize(id, tipo, pos, Quaternion.Identity);
        rb.AttachBoxCollider(new Vector3(0.5f, 0.5f, 0.5f), restitution: 0f, friction: 0f);
        rb.LinearDamping = 0f;
        rb.UseGravity = gravedad;
        return rb;
    }

    public override void Start()
    {
        // --- Rondas inmediatas (sin esperar pasos): COM, inercia, sueño ---
        _masa = CrearCuerpo(54, RigidBodyType.Dynamic, new Vector3(0f, 8f, 12f), false);
        Vector3 com0 = _masa.CenterOfMass;
        _masa.CenterOfMass = new Vector3(0.3f, 0f, 0f);
        Vector3 com1 = _masa.CenterOfMass;
        Vector3 wcom = _masa.WorldCenterOfMass;
        bool okResetCom = _masa.ResetCenterOfMass();
        Vector3 com2 = _masa.CenterOfMass;
        Resultado("centro_masa",
            Math.Abs(com0.X) < 0.02f && Cerca(com1.X, 0.3f, 0.02f)
            && Cerca(wcom.X, 0.3f, 0.05f) && Cerca(wcom.Y, 8f, 0.05f)
            && okResetCom && Math.Abs(com2.X) < 0.02f,
            $"def=({F(com0.X)}) fijado=({F(com1.X)}) mundo=({F(wcom.X)},{F(wcom.Y)}) reset=({F(com2.X)})");

        Vector3 in0 = _masa.InertiaTensor;
        _masa.InertiaTensor = new Vector3(0.5f, 0.6f, 0.7f);
        Vector3 in1 = _masa.InertiaTensor;
        Quaternion inRot = _masa.InertiaTensorRotation;
        bool okResetIn = _masa.ResetInertiaTensor();
        Vector3 in2 = _masa.InertiaTensor;
        Resultado("inercia",
            Cerca(in0.X, 1f / 6f, 0.02f) && Cerca(in1.X, 0.5f, 0.02f) && Cerca(in1.Y, 0.6f, 0.02f)
            && Math.Abs(inRot.X) < 0.02f && Cerca(Math.Abs(inRot.W), 1f, 0.02f)
            && okResetIn && Cerca(in2.X, 1f / 6f, 0.02f),
            $"def={F(in0.X)} fijada=({F(in1.X)},{F(in1.Y)}) reset={F(in2.X)}");

        bool durmio = _masa.Sleep() && _masa.IsSleeping();
        bool desperto = _masa.WakeUp() && !_masa.IsSleeping();
        Resultado("sueno_explicito", durmio && desperto, $"sleep={durmio} wake={desperto}");

        // --- Cuerpos de las rondas con pasos ---
        _spin = CrearCuerpo(50, RigidBodyType.Dynamic, new Vector3(-8f, 3f, 8f), false);
        _spin.AngularVelocity = new Vector3(0f, 3f, 0f);

        _torque = CrearCuerpo(51, RigidBodyType.Dynamic, new Vector3(-8f, 3f, 4f), false);
        _torque.AddTorque(new Vector3(0f, 0.5f, 0f), ForceMode.Impulse);

        _forcePoint = CrearCuerpo(52, RigidBodyType.Dynamic, new Vector3(0f, 3f, 14f), false);
        _forcePoint.AddForceAtPosition(new Vector3(0f, 0f, 4f), new Vector3(0.5f, 3f, 14f), ForceMode.Impulse);

        _explosion = CrearCuerpo(53, RigidBodyType.Dynamic, new Vector3(3f, 0.5f, -13f), false);
        _explosion.AddExplosionForce(10f, new Vector3(0f, 0.5f, -13f), 6f, 0.5f, ForceMode.Impulse);

        _punto = CrearCuerpo(55, RigidBodyType.Dynamic, PuntoSpin, false);
        _punto.AngularVelocity = new Vector3(0f, 2f, 0f);

        _kin = CrearCuerpo(56, RigidBodyType.Kinematic, new Vector3(0f, 1f, -6f), false);
        _probe = CrearCuerpo(57, RigidBodyType.Dynamic, new Vector3(-5f, 1f, -6f), false);

        _cap = CrearCuerpo(58, RigidBodyType.Dynamic, new Vector3(-12f, 3f, -12f), false);
        _cap.SetLinearVelocity(new Vector3(50f, 0f, 0f));
        _cap.MaxLinearVelocity = 2f;

        _fantasma = CrearCuerpo(60, RigidBodyType.Dynamic, new Vector3(-3f, 2f, 16f), true);
        _fantasma.DetectCollisions = false;
        _normal = CrearCuerpo(61, RigidBodyType.Dynamic, new Vector3(0f, 2f, 16f), true);
        _normal.DetectCollisions = true;

        // --- Personaje para las rondas del CC ---
        _cc = new CharacterController
        {
            Radius = 0.35f,
            Height = 1.8f,
            SkinWidth = 0.02f,
            StepOffset = 0.4f,
            SlopeLimit = 45f,
        };
        _cc.Initialize(EntityId, CcInicio);
        _montado = true;
    }

    public override void Update(float deltaTime)
    {
        if (!_montado) return;
        _t += deltaTime;

        if (_fase == 0 && _t > 0.35f)
        {
            _fase = 1;
            Vector3 w = Vector3.Zero, wt = Vector3.Zero, vf = Vector3.Zero, wf = Vector3.Zero;
            Vector3 ve = Vector3.Zero, vp = Vector3.Zero, vc = Vector3.Zero;
            bool okSpin = _spin.GetAngularVelocity(out w) && w.Y > 2.3f && w.Y < 3.7f;
            Resultado("velocidad_angular", okSpin, $"wy={F(w.Y)} fijada 3");
            bool okTor = _torque.GetAngularVelocity(out wt) && wt.Y > 0.8f;
            Resultado("torque", okTor, $"wy={F(wt.Y)} tras impulso 0.5 (esperado ~3)");
            bool okFp = _forcePoint.GetLinearVelocity(out vf) && vf.Z > 3f && vf.Z < 5f
                        && _forcePoint.GetAngularVelocity(out wf) && wf.Y < -1f;
            Resultado("fuerza_en_punto", okFp, $"vz={F(vf.Z)} wy={F(wf.Y)} (impulso descentrado)");
            bool okEx = _explosion.GetLinearVelocity(out ve) && ve.X > 2f && ve.Y > 0.3f;
            Resultado("explosion", okEx, $"v=({F(ve.X)},{F(ve.Y)}) lejos del centro y hacia arriba");
            bool okPv = _punto.GetPointVelocity(new Vector3(PuntoSpin.X + 1f, PuntoSpin.Y, PuntoSpin.Z), out vp)
                        && vp.Z > -2.6f && vp.Z < -1.4f && Math.Abs(vp.X) < 0.4f;
            Resultado("velocidad_en_punto", okPv, $"v_punto=({F(vp.X)},{F(vp.Z)}) esperado (0,-2)");
            bool okCap = _cap.GetLinearVelocity(out vc) && Math.Abs(vc.X) <= 2.1f;
            Resultado("tope_velocidad", okCap, $"vx={F(vc.X)} con tope 2 (arranque 50)");
        }
        else if (_fase == 1 && _t > 0.9f)
        {
            _fase = 2;
            Physics.RaycastHit hit = default;
            bool okSweep = _probe.SweepTest(new Vector3(1f, 0f, 0f), 12f, out hit)
                           && hit.EntityId == 56 && hit.Distance > 3.4f && hit.Distance < 4.6f;
            Resultado("sweep", okSweep, $"entidad={hit.EntityId} distancia={F(hit.Distance)} (esperado 56 a ~4)");
            Physics.RaycastHit[] todos = _probe.SweepTestAll(new Vector3(1f, 0f, 0f), 20f);
            bool hay56 = false, hay21 = false;
            foreach (Physics.RaycastHit h in todos)
            {
                if (h.EntityId == 56) hay56 = true;
                if (h.EntityId == 21) hay21 = true;
            }
            Resultado("sweep_all", todos.Length >= 2 && hay56 && hay21,
                $"impactos={todos.Length} kin56={hay56} pared21={hay21}");
            _kinMovido = _kin.Move(new Vector3(2f, 1f, -6f), Quaternion.Identity);
        }
        else if (_fase == 2 && _t > 1.5f)
        {
            _fase = 3;
            Physics.RaycastHit hit2 = default;
            bool okMove = _kinMovido && _probe.SweepTest(new Vector3(1f, 0f, 0f), 12f, out hit2)
                          && hit2.EntityId == 56 && hit2.Distance > 5.4f && hit2.Distance < 6.6f;
            Resultado("move_kinematico", okMove, $"distancia tras Move={F(hit2.Distance)} (esperado ~6)");
            Vector3 vg = Vector3.Zero, vn = Vector3.Zero;
            bool okDet = _fantasma.GetLinearVelocity(out vg) && vg.Y < -5f
                         && _normal.GetLinearVelocity(out vn) && Math.Abs(vn.Y) < 0.6f;
            Resultado("detect_colisiones", okDet,
                $"fantasma_vy={F(vg.Y)} (atraviesa) normal_vy={F(vn.Y)} (apoyado)");
        }

        ActualizarCc(deltaTime);
    }

    private void MoverCc(float dt, float vx)
    {
        Vector3 a = _cc.Move(new Vector3(vx * dt, -1.0f * dt, 0f), dt);
        _ccX += a.X;
        _ccY += a.Y;
        // El veredicto de subida usa el PICO, no el neto: si la
        // ventana cruza el peldaño entero, baja por el otro lado
        // y el neto vuelve a ~0 habiendo subido bien.
        if (_ccY > _ccYMax) _ccYMax = _ccY;
    }

    private void ActualizarCc(float dt)
    {
        if (_fase < 1) return;
        _ccT += dt;

        switch (_ccFase)
        {
            case 0: // Andar contra la pared con colisiones: bloquea.
                MoverCc(dt, 3.0f);
                if (_ccT > 2.6f)
                {
                    Resultado("cc_detect_bloquea", _ccX > 4.6f && _ccX < 5.5f,
                        $"avance={F(_ccX)} (la pared está a 5.5, cápsula 0.35)");
                    _cc.DetectCollisions = false;
                    _cc.Teleport(CcInicio);
                    _ccX = 0f; _ccT = 0f; _ccFase = 1;
                }
                break;

            case 1: // Sin detect: atraviesa la pared (fantasma).
                MoverCc(dt, 3.0f);
                if (_ccT > 2.4f)
                {
                    Resultado("cc_detect_fantasma", _ccX > 6.5f,
                        $"avance={F(_ccX)} (más allá de la pared a 5.5)");
                    _ccT = 0f; _ccFase = 2;
                }
                break;

            case 2: // Enabled y Bounds, instantáneos.
                _cc.Enabled = false;
                Vector3 aplicado = _cc.Move(new Vector3(0.5f, 0f, 0f), dt);
                Resultado("cc_enabled", Math.Abs(aplicado.X) < 1e-5f && Math.Abs(aplicado.Z) < 1e-5f,
                    $"aplicado con Enabled=false: ({F(aplicado.X)},{F(aplicado.Z)})");
                _cc.Enabled = true;
                bool okB = _cc.GetBounds(out Vector3 mn, out Vector3 mx);
                Vector3 tam = new(mx.X - mn.X, mx.Y - mn.Y, mx.Z - mn.Z);
                Resultado("cc_bounds", okB && Cerca(tam.X, 0.7f, 0.06f) && Cerca(tam.Y, 1.8f, 0.06f) && Cerca(tam.Z, 0.7f, 0.06f),
                    $"tamaño=({F(tam.X)},{F(tam.Y)},{F(tam.Z)}) esperado (0.7,1.8,0.7)");
                _cc.DetectCollisions = true;
                _cc.Teleport(new Vector3(-7.5f, 0.92f, -2f));
                _cc.StepOffset = 0.05f;
                _ccX = 0f; _ccY = 0f; _ccYMax = 0f; _ccT = 0f; _ccFase = 3;
                break;

            case 3: // Peldaño de 0.35 con StepOffset 0.05: bloquea.
                MoverCc(dt, 3.0f);
                if (_ccT > 1.6f)
                {
                    _stepBloqueado = _ccX < 1.6f;
                    _cc.StepOffset = 0.4f;
                    _ccT = 0f; _ccFase = 4;
                }
                break;

            case 4: // Con StepOffset 0.4 en caliente: sube y cruza.
                MoverCc(dt, 3.0f);
                if (_ccT > 1.6f)
                {
                    Resultado("cc_step_offset", _stepBloqueado && _ccX > 2.6f && _ccYMax > 0.12f,
                        $"bloqueado_con_0.05={_stepBloqueado} avance_total={F(_ccX)} pico_subida={F(_ccYMax)}");
                    _cc.EnableOverlapRecovery = true;
                    _cc.Teleport(new Vector3(6f, 0.92f, 4f));
                    _ccX = 0f; _ccT = 0f; _ccFase = 5;
                }
                break;

            case 5: // Solapado dentro de la pared: la recuperación expulsa.
                Vector3 a = _cc.Move(new Vector3(0f, -1.0f * dt, 0f), dt);
                _ccX += Math.Abs(a.X);
                if (_ccT > 0.6f)
                {
                    Resultado("cc_overlap_recovery", _ccX > 0.08f,
                        $"expulsión_acumulada={F(_ccX)} desde el centro de la pared");
                    _cc.Teleport(CcInicio);
                    _cc.IncludeLayers = 1 << 5;
                    _ccX = 0f; _ccT = 0f; _ccFase = 6;
                }
                break;

            case 6: // La pared es de capa 0 y la máscara solo admite la 5: pasa.
                MoverCc(dt, 3.0f);
                if (_ccT > 2.4f)
                {
                    Resultado("cc_include_layers", _ccX > 6.5f,
                        $"avance={F(_ccX)} con IncludeLayers=(1<<5) y pared en capa 0");
                    _cc.IncludeLayers = -1;
                    _ccFase = 7;
                }
                break;

            case 7:
                _ccFase = 8;
                Debug.Log($"[PARIDAD] GLOBAL {(_globalOk ? "OK" : "FALLO")}");
                break;
        }
    }
}
