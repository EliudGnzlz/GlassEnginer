using System;
using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    public class VaivenLod : GlassBehaviour
    {
        static readonly Vector3 CAM = new Vector3(0f, 7f, 16f);

        float _t;
        int _banda = -1;

        public override void Update(float deltaTime)
        {
            _t += deltaTime;

            float z = -14f + 20f * MathF.Sin(_t * 0.25f);
            Position = new Vector3(10f, 1f, z);

            float d = Vector3.Distance(Position, CAM);
            int banda = d < 20f ? 0 : (d < 45f ? 1 : 2);

            if (banda != _banda)
            {
                _banda = banda;
                Debug.Log("[PRUEBA] LOD banda " + banda + " d=" + d.ToString("F1"));
            }
        }
    }
}
