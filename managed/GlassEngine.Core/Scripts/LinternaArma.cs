using System;
using GlassEngine.Core;
using GlassEngine.Core.Interop;
using UnityEngine;
using Vector3 = GlassEngine.Core.Interop.Vector3;
using Quaternion = GlassEngine.Core.Interop.Quaternion;

namespace GlassEngine.Core.Scripts
{
    public class LinternaArma : GlassBehaviour
    {
        private const ulong ARMA = 200;
        private const ulong LINTERNA_MIN = 201;
        private const ulong LINTERNA_MAX = 206;

        private float _t;

        public override void Start()
        {
            // Configura los seis conos Spot de la linterna del arma.
            for (ulong id = LINTERNA_MIN; id <= LINTERNA_MAX; id++)
            {
                RuntimeBridge.LightSetType(id, 2);
                RuntimeBridge.LightSetRange(id, 26f);
                RuntimeBridge.LightSetSpotAngles(id, 30f, 19f);
                RuntimeBridge.LightSetIntensity(id, 90f);
            }

            Debug.Log("[Panteon] Linternas configuradas desde C# por el puente LightProps");
        }

        public override void Update(float deltaTime)
        {
            _t += deltaTime;

            // Barrido yaw del arma; los conos heredan la rotación.
            float yaw = MathF.Sin(_t * 0.45f) * 26f;
            float half = yaw * MathF.PI / 360f;
            Rotation = new Quaternion(0f, MathF.Sin(half), 0f, MathF.Cos(half));
        }
    }
}
