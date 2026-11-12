using System;
using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    public class SolDiaNoche : GlassBehaviour
    {
        const int CicloFrames = 3600;

        Light _light;
        int _f;
        bool _logMediodia;

        public override void Awake()
        {
            var go = GameObject.GetOrCreate(EntityId, "Sol");
            _light = go.GetComponent<Light>() ?? go.AddComponent<Light>();
        }

        public override void Update(float deltaTime)
        {
            int frame = _f;
            float fase = (frame % CicloFrames) / (float)CicloFrames;

            // Rotación: baja y reinicia de golpe al cambiar de ciclo
            float pitch = -50f - fase * 180f;
            Rotation = Quaternion.Euler(pitch, 30f, 0f);

            // Intensidad
            float dia = MathF.Sin(fase * MathF.PI);
            if (_light != null)
                _light.intensity = 0.12f + 0.93f * dia;

            // Medianoche: inicio de ciclo
            if (frame > 0 && frame % CicloFrames == 0)
            {
                Debug.Log("[PRUEBA] SOL medianoche");
                _logMediodia = false;
            }

            // Mediodía: un log por ciclo al cruzar 0.98
            if (!_logMediodia && dia >= 0.98f)
            {
                _logMediodia = true;
                Debug.Log("[PRUEBA] SOL mediodia");
            }

            _f++;
        }
    }
}
