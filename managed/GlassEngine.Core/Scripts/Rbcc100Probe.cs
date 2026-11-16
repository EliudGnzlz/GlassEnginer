using System;
using GlassEngine.Core;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;
using UnityEngine;
using Vector3 = GlassEngine.Core.Interop.Vector3;
using Quaternion = GlassEngine.Core.Interop.Quaternion;
using RaycastHit = GlassEngine.Core.Physics.RaycastHit;

namespace GlassEngine.Core.Scripts;

/// <summary>
/// Sonda física del bloque RB/CC 100% (RB-1..RB-4 + CC-1/CC-2): ejerce
/// las APIs nuevas por el camino real (standalone) y deja el veredicto
/// en el log con la marca [RBCC100]. Escena: PruebaRbcc100.
/// </summary>
public class Rbcc100Probe : GlassBehaviour
{
    private RigidBody _rb = null!;
    private CharacterController _cc = null!;
    private int _frame;
    private bool _fin;

    private static string F(float v) => v.ToString("F3", System.Globalization.CultureInfo.InvariantCulture);

    private void Check(string nombre, bool ok, string detalle = "")
        => Debug.Log($"[RBCC100] {nombre} {(ok ? "OK" : "FALLO")} {detalle}");

    public override void Start()
    {
        // --- Rigidbody (RB-1..RB-4): set/get inmediatos ---
        _rb = new RigidBody();
        _rb.Initialize(9101, RigidBodyType.Dynamic, new Vector3(0f, 5f, -6f), Quaternion.Identity);
        _rb.AttachBoxCollider(new Vector3(0.5f, 0.5f, 0.5f), restitution: 0f, friction: 0.5f);
        _rb.UseGravity = false;

        _rb.AngularDamping = 0.9f;
        Check("RB angularDamping", Math.Abs(_rb.AngularDamping - 0.9f) < 0.001f, F(_rb.AngularDamping));
        _rb.SolverIterations = 7;
        Check("RB solverIterations", _rb.SolverIterations == 7, _rb.SolverIterations.ToString());
        _rb.SolverVelocityIterations = 9;
        Check("RB solverVelocityIterations", _rb.SolverVelocityIterations == 9, _rb.SolverVelocityIterations.ToString());
        _rb.MaxDepenetrationVelocity = 2.5f;
        Check("RB maxDepenetrationVelocity", Math.Abs(_rb.MaxDepenetrationVelocity - 2.5f) < 0.001f, F(_rb.MaxDepenetrationVelocity));
        _rb.IncludeLayers = 0xFFu;
        _rb.ExcludeLayers = 0x2u;
        Check("RB capas", _rb.IncludeLayers == 0xFFu && _rb.ExcludeLayers == 0x2u,
            $"inc={_rb.IncludeLayers} exc={_rb.ExcludeLayers}");

        Check("RB automaticCOM defecto", _rb.AutomaticCenterOfMass && _rb.AutomaticInertiaTensor);
        _rb.CenterOfMass = new Vector3(0.3f, -0.2f, 0.1f);
        Check("RB fijar COM apaga automatico", !_rb.AutomaticCenterOfMass);
        Check("RB resetCOM", _rb.ResetCenterOfMass() && _rb.AutomaticCenterOfMass);
        _rb.InertiaTensor = new Vector3(2f, 2f, 2f);
        Check("RB fijar inercia apaga automatico", !_rb.AutomaticInertiaTensor);
        Check("RB resetInertia", _rb.ResetInertiaTensor() && _rb.AutomaticInertiaTensor);

        // La entidad 9101 no existe en la escena: no hay Transform que
        // publicar y la llamada debe fallar SIN tocar el cuerpo.
        Check("RB publishTransform sin escena = false", !_rb.PublishTransform());
        var rb2 = new RigidBody();
        rb2.Initialize(EntityId, RigidBodyType.Dynamic, Position, Quaternion.Identity);
        rb2.AttachBoxCollider(new Vector3(0.5f, 0.5f, 0.5f), restitution: 0f, friction: 0.5f);
        Check("RB publishTransform con escena", rb2.PublishTransform());
        bool rechazado = !_rb.SetTransform(new Vector3(0f, 5f, -6f), default);
        _rb.GetTransform(out _, out Quaternion rotTras);
        Check("RB setTransform cuaternion cero rechazado",
            rechazado && Math.Abs(rotTras.W - 1f) < 0.001f,
            $"w={F(rotTras.W)}");
        Vector3 cp = _rb.ClosestPointOnBounds(new Vector3(3f, 5.5f, -6f));
        Check("RB closestPointOnBounds", Math.Abs(cp.X - 0.5f) < 0.01f && Math.Abs(cp.Y - 5.5f) < 0.01f && Math.Abs(cp.Z + 6f) < 0.01f,
            $"({F(cp.X)},{F(cp.Y)},{F(cp.Z)})");

        _rb.AddRelativeForce(new Vector3(10f, 0f, 0f), ForceMode.Force);
        bool hayFuerza = _rb.GetAccumulatedForce(out Vector3 fz) && Math.Abs(fz.X) > 5f;
        Check("RB addRelativeForce+acumulada", hayFuerza, $"({F(fz.X)},{F(fz.Y)},{F(fz.Z)})");
        _rb.GetAccumulatedTorque(out Vector3 tq);
        Check("RB accumulatedTorque lee", true, $"({F(tq.X)},{F(tq.Y)},{F(tq.Z)})");

        // --- CharacterController (CC-1/CC-2) ---
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
        _cc.Create();
        Check("CC creado", _cc.IsCreated, $"pos=({F(_cc.Position.X)},{F(_cc.Position.Y)},{F(_cc.Position.Z)})");
    }

    public override void Update(float deltaTime)
    {
        if (_fin || _cc == null)
            return;
        _frame++;

        // Fase 1 (frames 1..90): SimpleMove sin velocidad horizontal:
        // cae por su acumulador de gravedad y aterriza sobre el suelo.
        if (_frame <= 90)
        {
            _cc.SimpleMove(Vector3.Zero);
            if (_frame == 90)
            {
                Check("CC simpleMove cae y aterriza", _cc.IsGrounded && _cc.Position.Y > 0.8f && _cc.Position.Y < 1.05f,
                    $"grounded={_cc.IsGrounded} y={F(_cc.Position.Y)}");
                FaseConsultas();
                _fin = true;
                Debug.Log("[RBCC100] FIN");
            }
        }
    }

    private void FaseConsultas()
    {
        float dt = 1f / 60f;

        // minMoveDistance: 0.1 con umbral 0.5 se descarta; sin umbral pasa.
        float x0 = _cc.Position.X;
        _cc.MinMoveDistance = 0.5f;
        _cc.Move(new Vector3(0.1f, 0f, 0f), dt);
        Check("CC minMoveDistance descarta", Math.Abs(_cc.Position.X - x0) < 0.001f, $"x={F(_cc.Position.X)}");
        _cc.MinMoveDistance = 0f;
        _cc.Move(new Vector3(0.3f, 0f, 0f), dt);
        Check("CC minMoveDistance cero pasa", _cc.Position.X - x0 > 0.2f, $"x={F(_cc.Position.X)}");

        // center: desplaza la cápsula en el delta.
        float x1 = _cc.Position.X;
        _cc.Center = new Vector3(0.5f, 0f, 0f);
        Check("CC center desplaza", Math.Abs(_cc.Position.X - (x1 + 0.5f)) < 0.01f, $"x={F(_cc.Position.X)}");
        _cc.Center = Vector3.Zero;
        Check("CC center vuelve", Math.Abs(_cc.Position.X - x1) < 0.01f, $"x={F(_cc.Position.X)}");

        // material / sharedMaterial.
        _cc.SharedMaterial = new PhysicMaterial(0.8f, 0.3f);
        Check("CC sharedMaterial", Math.Abs(_cc.SharedMaterial.Friction - 0.8f) < 0.001f
            && Math.Abs(_cc.SharedMaterial.Restitution - 0.3f) < 0.001f);
        Check("CC material instancia", Math.Abs(_cc.Material.Friction - 0.8f) < 0.001f);

        // Raycast contra la propia cápsula y puntos más cercanos.
        Vector3 p = _cc.Position;
        bool hit = _cc.Raycast(new Vector3(p.X - 5f, p.Y, p.Z), new Vector3(1f, 0f, 0f), out RaycastHit rh, 100f);
        Check("CC raycast capsula", hit && Math.Abs(rh.Distance - 4.65f) < 0.05f && rh.EntityId == EntityId,
            hit ? $"dist={F(rh.Distance)}" : "sin impacto");
        Vector3 cercano = _cc.ClosestPoint(new Vector3(p.X + 3f, p.Y, p.Z));
        Check("CC closestPoint", Math.Abs(cercano.X - (p.X + 0.35f)) < 0.01f, $"x={F(cercano.X)}");
        Vector3 enBounds = _cc.ClosestPointOnBounds(new Vector3(p.X + 3f, p.Y, p.Z));
        Check("CC closestPointOnBounds", Math.Abs(enBounds.X - (p.X + 0.35f)) < 0.01f, $"x={F(enBounds.X)}");
    }
}
