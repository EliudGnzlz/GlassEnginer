using UnityEngine;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Sonda del puente de luces: alterna la intensidad de SU luz
    /// puntual entre 0 y 25 cada ~1,2 s y anota el código que
    /// devuelve el motor en cada llamada.
    /// </summary>
    public class ToggleLightTest : GlassBehaviour
    {
        private float _elapsed;
        private bool _on;

        public override void Start()
        {
            int rc = RuntimeBridge.LightSetIntensity(EntityId, 0f);
            Debug.Log($"[ToggleLightTest] Start en entidad {EntityId}; LightSetIntensity(0) -> {rc}.");
        }

        public override void Update(float deltaTime)
        {
            _elapsed += deltaTime;
            if (_elapsed >= 1.2f)
            {
                _elapsed = 0f;
                _on = !_on;
                float v = _on ? 25f : 0f;
                int rc = RuntimeBridge.LightSetIntensity(EntityId, v);
                Debug.Log($"[ToggleLightTest] LightSetIntensity({v}) -> {rc}.");
            }
        }
    }
}
