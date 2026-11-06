using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Luz de «encendido» nocturno del enemigo del nivel 2 de Plat3D.
    /// Va en una entidad hija que solo tiene una luz Point: de día queda a 0
    /// y de noche sube hasta 26 según <see cref="Plat3DDayNightState.NightFactor"/>.
    /// </summary>
    public class Plat3DNightGlow : GlassBehaviour
    {
        private const float MaxIntensity = 26f;
        private const float ChangeThreshold = 0.05f;

        private Light? _light;
        private float _applied = float.NaN;

        public override void Start()
        {
            var go = GameObject.GetOrCreate(EntityId, "Night Glow");
            _light = go.GetComponent<Light>() ?? go.AddComponent<Light>();

            _light.intensity = 0f;
            _applied = 0f;

            Log($"Luz nocturna lista (entidad {EntityId}); arranca apagada de día.");
        }

        public override void Update(float deltaTime)
        {
            if (_light == null)
            {
                return;
            }

            float target = Plat3DDayNightState.NightFactor * MaxIntensity;
            if (System.Math.Abs(target - _applied) > ChangeThreshold)
            {
                _light.intensity = target;
                _applied = target;
            }
        }
    }
}
