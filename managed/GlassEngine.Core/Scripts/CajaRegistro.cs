using System.Collections.Generic;
using UnityEngine;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts;

/// <summary>Compañera de la prueba de huecos beta: publica la
/// posición de su entidad en un registro estático compartido y
/// anota los contactos sólidos que recibe (normal e impulso).</summary>
public class CajaRegistro : GlassBehaviour
{
    public static readonly Dictionary<ulong, Vector3> Posiciones = new();

    public override void Update(float deltaTime)
    {
        Posiciones[EntityId] = Position;
    }

    public override void OnCollisionEnter(Collision collision)
    {
        if (EntityId == 12)
        {
            Debug.Log($"[HUECOS] CONTACTO normal=({collision.normal.x:F2},{collision.normal.y:F2},{collision.normal.z:F2}) impulso={collision.impulse:F2} punto_y={collision.point.y:F2} relvel_y={collision.relativeVelocity.y:F2}");
        }
    }
}
