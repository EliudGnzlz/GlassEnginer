using UnityEngine;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Sonda del puente de luces sobre la DIRECCIONAL: alterna la
    /// intensidad del sol entre 1.0 y 0.1 cada ~1,2 s y anota el
    /// código del motor.
    /// </summary>
    public class ToggleSunTest : GlassBehaviour
    {
        private float _elapsed;
        private bool _dim;

        public override void Start()
        {
            int rc = RuntimeBridge.LightSetIntensity(EntityId, 1.0f);
            Debug.Log($"[ToggleSunTest] Start en entidad {EntityId}; LightSetIntensity(1.0) -> {rc}.");
        }

        public override void Update(float deltaTime)
        {
            _elapsed += deltaTime;
            if (_elapsed >= 1.2f)
            {
                _elapsed = 0f;
                _dim = !_dim;
                float v = _dim ? 0.1f : 1.0f;
                int rc = RuntimeBridge.LightSetIntensity(EntityId, v);
                Debug.Log($"[ToggleSunTest] LightSetIntensity({v}) -> {rc}.");
            }
        }
    }
}
