using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    public class EventosDemo : GlassBehaviour
    {
        public override void OnCollisionEnter(Collision collision)
        {
            Debug.Log("[EVENTOS] CollisionEnter con " + collision.gameObject.name);
        }

        public override void OnTriggerEnter(Collision collision)
        {
            Debug.Log("[EVENTOS] TriggerEnter con " + collision.gameObject.name);
        }

        public override void OnTriggerExit(Collision collision)
        {
            Debug.Log("[EVENTOS] TriggerExit con " + collision.gameObject.name);
        }
    }
}
