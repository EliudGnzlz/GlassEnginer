using System;
using GlassEngine.Core;
using GlassEngine.Core.Interop;
using UnityEngine;
using Vector3 = GlassEngine.Core.Interop.Vector3;
using Quaternion = GlassEngine.Core.Interop.Quaternion;

namespace GlassEngine.Core.Scripts
{
    public class PaseoPanteon : GlassBehaviour
    {
        private float _t;

        public override void Update(float deltaTime)
        {
            _t += deltaTime;

            // Recorre la sala de lado a lado para forzar reasignaciones de luces.
            Position = new Vector3(MathF.Sin(_t * 0.22f) * 10.5f, 0.5f, 2.9f);
        }
    }
}
