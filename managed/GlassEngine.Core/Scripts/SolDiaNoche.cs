using System;
using System.Globalization;
using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    // Ciclo día/noche del sol. El pico de mediodía se lee de la escena
    // (Light.intensity autorado en Awake, antes de que Update lo pise);
    // el piso nocturno sigue siendo una constante (0,12). La modulación
    // del ambiente queda fuera de este script: no hay API de ambiente
    // en scripting.
    //
    // Variable de entorno GLASS_SOL_FASE (0..1, solo pruebas): si está
    // puesta y es un float válido en ese rango, la fase del ciclo se
    // congela en ese valor (el contador de frames no avanza); si no
    // está puesta o no es válida, el ciclo corre como siempre.
    public class SolDiaNoche : GlassBehaviour
    {
        const int CicloFrames = 3600;
        const float PisoNocturno = 0.12f;

        Light _light;
        int _f;
        bool _logMediodia;
        float _pico;
        bool _faseCongelada;
        float _faseFija;

        public override void Awake()
        {
            var go = GameObject.GetOrCreate(EntityId, "Sol");
            _light = go.GetComponent<Light>() ?? go.AddComponent<Light>();

            float picoAutorado = _light != null ? _light.intensity : float.NaN;
            _pico = (float.IsFinite(picoAutorado) && picoAutorado > 0f)
                ? picoAutorado
                : 1.0f;

            var raw = Environment.GetEnvironmentVariable("GLASS_SOL_FASE");
            if (!string.IsNullOrEmpty(raw)
                && float.TryParse(raw, NumberStyles.Float, CultureInfo.InvariantCulture, out var fase)
                && float.IsFinite(fase)
                && fase >= 0f && fase <= 1f)
            {
                _faseCongelada = true;
                _faseFija = fase;
            }
        }

        public override void Update(float deltaTime)
        {
            int frame = _f;
            float fase = _faseCongelada
                ? _faseFija
                : (frame % CicloFrames) / (float)CicloFrames;

            // Rotación: baja y reinicia de golpe al cambiar de ciclo
            float pitch = -50f - fase * 180f;
            Rotation = Quaternion.Euler(pitch, 30f, 0f);

            // Intensidad: modula el pico autorado de escena, no una constante
            float dia = MathF.Sin(fase * MathF.PI);
            if (_light != null)
                _light.intensity = PisoNocturno + (_pico - PisoNocturno) * dia;

            if (!_faseCongelada)
            {
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
            else
            {
                // Fase fija: un solo log si la fase congelada cae en mediodía,
                // sin repetirse en bucle.
                if (!_logMediodia && dia >= 0.98f)
                {
                    _logMediodia = true;
                    Debug.Log("[PRUEBA] SOL mediodia");
                }
            }
        }
    }
}
