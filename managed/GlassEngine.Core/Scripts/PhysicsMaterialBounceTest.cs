using GlassEngine.Core;
using GlassEngine.Core.Physics;

/// <summary>
/// Verificación de C6 por el camino real: una esfera con bounciness=1 y
/// bounceCombine=Maximum debe rebotar cerca de su altura de caída cuando
/// el suelo tiene el mismo material. El veredicto es informativo: el test
/// registra el ratio y deja el diagnóstico en el Log, no fuerza el número.
/// </summary>
public class PhysicsMaterialBounceTest : GlassBehaviour
{
    /// Id de la entidad del suelo estático, asignado por el harness en el
    /// Inspector. El suelo debe ser una caja con la cara superior en y=0.5
    /// (la escena de prueba lo autoró así). Si queda en 0, el suelo conserva
    /// su material y el test solo mide el rebote contra lo que haya.
    public ulong FloorEntityId;

    private RigidBody _ball;
    private Collider _ballCollider;

    private float _startY;
    private const float ContactY = 1.0f; // centro de la esfera r=0.5 sobre la cara superior del suelo (y=0.5)
    private bool _impacted;
    private float _apexY;
    private float _timeSinceImpact;
    private bool _finished;

    public override void Start()
    {
        var bounce = new PhysicsMaterial
        {
            dynamicFriction = 0.0f,
            staticFriction = 0.0f,
            bounciness = 1.0f,
            frictionCombine = PhysicsMaterialCombine.Average,
            bounceCombine = PhysicsMaterialCombine.Maximum
        };

        if (FloorEntityId != 0)
        {
            var floorCollider = new Collider { EntityId = FloorEntityId, Index = 0 };
            floorCollider.sharedMaterial = bounce;
            Log("PhysicsMaterialBounceTest: floor.sharedMaterial bounciness=1.0 bounceCombine=Maximum");
        }
        else
        {
            Log("PhysicsMaterialBounceTest: FloorEntityId no asignado; el suelo conserva su material.");
        }

        _ball = new RigidBody();
        _ball.Initialize(EntityId, RigidBodyType.Dynamic, Position, Rotation);
        // El motor crea los cuerpos dinámicos con linear_damping 1.5 por
        // defecto: sin esto la caída y la subida pierden velocidad y el
        // ratio mide el freno, no el material. A 0, el rebote es solo
        // restitution + combine (el veredicto 0.95 lo supone).
        _ball.LinearDamping = 0.0f;
        _ball.AttachSphereCollider(0.5f, restitution: 0.0f, friction: 0.0f);
        _ballCollider = _ball.GetCollider(0);
        if (_ballCollider != null)
        {
            _ballCollider.sharedMaterial = bounce;
        }

        _startY = Position.Y;
        Log($"PhysicsMaterialBounceTest: startY={_startY:F4} contactY={ContactY:F4} ball.sharedMaterial bounciness=1.0 bounceCombine=Maximum");
    }

    public override void Update(float deltaTime)
    {
        if (_ball == null)
        {
            return;
        }

        _ball.GetTransform(out var physPos, out var physRot);

        if (!_impacted)
        {
            if (physPos.Y <= ContactY + 0.02f)
            {
                _impacted = true;
                _apexY = physPos.Y;
                _timeSinceImpact = 0.0f;
                Log($"PhysicsMaterialBounceTest: impacto y={physPos.Y:F4}");
            }
            return;
        }

        _timeSinceImpact += deltaTime;
        if (physPos.Y > _apexY)
        {
            _apexY = physPos.Y;
        }

        if (!_finished && _timeSinceImpact >= 2.0f)
        {
            _finished = true;
            float drop = _startY - ContactY;
            float rebound = _apexY - ContactY;
            float ratio = drop > 1e-5f ? rebound / drop : 0.0f;

            Log($"PhysicsMaterialBounceTest: drop={drop:F4}m apex={rebound:F4}m ratio={ratio:F4} (target>=0.95)");

            if (ratio < 0.95f)
            {
                Log("PhysicsMaterialBounceTest DIAGNOSTICO: ratio<0.95. Revisar bounceCombine=Maximum, bounciness=1 en ambos colliders, damping/umbral de restitucion y velocidad/altura de caida.");
            }
        }
    }
}
