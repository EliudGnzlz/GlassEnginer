using System.Globalization;
using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Estado compartido del ciclo día/noche del nivel 2 de Plat3D.
    /// Lo escribe <see cref="Plat3DDayNight"/> y lo leen otros guiones
    /// (p. ej. <see cref="Plat3DNightGlow"/>) para reaccionar a la noche.
    /// </summary>
    public static class Plat3DDayNightState
    {
        /// <summary>0 = día pleno, 1 = noche cerrada.</summary>
        public static float NightFactor;

        /// <summary>Si es &gt;= 0 el ciclo se congela en esa fase (0..1). Útil para pruebas.</summary>
        public static float PhaseOverride = -1f;
    }

    /// <summary>
    /// Ciclo día/noche del nivel 2 de Plat3D. Va en la entidad del sol
    /// (luz direccional): gira el sol por tramos, cambia su color/intensidad
    /// y publica <see cref="Plat3DDayNightState.NightFactor"/> para el resto
    /// de guiones. Ciclo completo: 150 s.
    /// <para>
    /// La curva de intensidad es relativa: el pico se autoría en la escena y
    /// este guion lo lee en el arranque (la curva se escribió contra
    /// <see cref="PicoReferencia"/>). Re-autorar el sol en la escena escala el
    /// ciclo entero en vez de ser pisado por constantes; con el sol autorado a
    /// 1,05 (Plat3D L2 y DayNightTest hoy) el resultado es idéntico al de antes.
    /// </para>
    /// <para>
    /// Para las capturas de verificación se puede congelar la fase desde fuera
    /// con <see cref="Plat3DDayNightState.PhaseOverride"/> o, si nadie lo ha
    /// puesto, con la variable de entorno <c>GLASS_SOL_FASE</c> (0..1), la
    /// misma que honra <c>SolDiaNoche</c> de la Demo FPS. Un PhaseOverride
    /// puesto por script manda sobre el entorno.
    /// </para>
    /// <para>
    /// La modulación del ambiente queda fuera: no hay API de ambiente en
    /// scripting (misma nota que dejó G5-S1 en SolDiaNoche).
    /// </para>
    /// </summary>
    public class Plat3DDayNight : GlassBehaviour
    {
        private const float CycleDuration = 150f;
        private const float Yaw = 30f;

        // Pico de intensidad con el que la curva fue autorada.
        private const float PicoReferencia = 1.05f;

        // Fases de los tramos.
        private const float SunsetStart = 0.55f;
        private const float NightStart = 0.65f;
        private const float DawnStart = 0.93f;

        // Claves de color.
        private static readonly Color DayColor = new Color(1f, 0.96f, 0.88f);
        private static readonly Color NoonColor = new Color(1f, 0.85f, 0.62f);
        private static readonly Color SunsetColor = new Color(1f, 0.42f, 0.18f);
        private static readonly Color NightColor = new Color(0.32f, 0.42f, 0.68f);

        private Light? _light;
        private float _escala = 1f;
        private float _phase;
        private int _segment = -1;

        public override void Start()
        {
            var go = GameObject.GetOrCreate(EntityId, "Sun");
            _light = go.GetComponent<Light>() ?? go.AddComponent<Light>();

            // El pico sale de la escena: la curva se escala respecto a la
            // intensidad autorada en la luz. Se lee el componente real a
            // través del puente; si no responde, el valor local de C#.
            float autorada = _light != null && _light.TryGetEngineIntensity(out float delMotor)
                ? delMotor
                : (_light?.intensity ?? 0f);
            _escala = EsFinita(autorada) && autorada > 0f
                ? autorada / PicoReferencia
                : 1f;

            // Si nadie ha congelado la fase por script, se admite la variable
            // de entorno compartida con SolDiaNoche (útil para capturas).
            if (Plat3DDayNightState.PhaseOverride < 0f)
            {
                string? texto = System.Environment.GetEnvironmentVariable("GLASS_SOL_FASE");
                if (!string.IsNullOrEmpty(texto)
                    && float.TryParse(texto, NumberStyles.Float, CultureInfo.InvariantCulture, out float desdeEntorno)
                    && EsFinita(desdeEntorno)
                    && desdeEntorno >= 0f
                    && desdeEntorno <= 1f)
                {
                    Plat3DDayNightState.PhaseOverride = desdeEntorno;
                }
            }

            float overridePhase = Plat3DDayNightState.PhaseOverride;
            _phase = overridePhase >= 0f ? Clamp01(overridePhase) : 0f;

            Log($"Ciclo día/noche listo (entidad {EntityId}, {CycleDuration:0} s por vuelta, pico {autorada:0.00}, escala {_escala:0.00}).");
        }

        public override void Update(float deltaTime)
        {
            float overridePhase = Plat3DDayNightState.PhaseOverride;
            if (overridePhase >= 0f)
            {
                _phase = Clamp01(overridePhase);
            }
            else
            {
                _phase += deltaTime / CycleDuration;
                if (_phase >= 1f)
                {
                    _phase -= (float)System.Math.Floor(_phase);
                }
            }

            float pitch;
            float intensity;
            Color color;

            if (_phase < SunsetStart)
            {
                // Día: -30 -> -150, con bache de mediodía en la intensidad.
                EnterSegment(0, "día");
                float t = _phase / SunsetStart;
                pitch = Lerp(-30f, -150f, t);
                color = LerpColor(DayColor, NoonColor, t);
                intensity = Lerp(1.05f, 0.95f, t)
                            + 0.15f * (float)System.Math.Sin(System.Math.PI * _phase / SunsetStart);
            }
            else if (_phase < NightStart)
            {
                // Atardecer: -150 -> -172, el sol se vuelve naranja y cae a 0.5.
                EnterSegment(1, "atardecer");
                float t = (_phase - SunsetStart) / (NightStart - SunsetStart);
                pitch = Lerp(-150f, -172f, t);
                color = LerpColor(NoonColor, SunsetColor, t);
                intensity = Lerp(0.95f, 0.5f, t);
            }
            else if (_phase < DawnStart)
            {
                // Noche: -172 -> -188, luz de luna fría y muy tenue.
                EnterSegment(2, "noche");
                float t = (_phase - NightStart) / (DawnStart - NightStart);
                pitch = Lerp(-172f, -188f, t);
                color = NightColor;
                intensity = 0.13f;
            }
            else
            {
                // Amanecer rápido (time-lapse de ~10 s): barrido directo -188 -> -30.
                EnterSegment(3, "amanecer");
                float t = (_phase - DawnStart) / (1f - DawnStart);
                pitch = Lerp(-188f, -30f, t);
                color = LerpColor(NightColor, DayColor, t);
                intensity = Lerp(0.13f, 1.05f, t);
            }

            Rotation = Quaternion.Euler(pitch, Yaw, 0f);

            if (_light != null)
            {
                _light.color = color;

                // La curva es relativa: se escala al pico autorado en la escena.
                _light.intensity = intensity * _escala;
            }

            Plat3DDayNightState.NightFactor = ComputeNightFactor(_phase);
        }

        private static float ComputeNightFactor(float phase)
        {
            if (phase < SunsetStart)
            {
                return 0f;
            }

            if (phase < 0.70f)
            {
                return Clamp01((phase - SunsetStart) / (0.70f - SunsetStart));
            }

            if (phase < DawnStart)
            {
                return 1f;
            }

            return Clamp01(1f - (phase - DawnStart) / (1f - DawnStart));
        }

        private void EnterSegment(int index, string name)
        {
            if (_segment == index)
            {
                return;
            }

            _segment = index;
            Log($"Tramo «{name}» (fase {_phase:0.000}, luz {_light?.intensity:0.00}).");
        }

        private static bool EsFinita(float value)
            => !float.IsNaN(value) && !float.IsInfinity(value);

        private static float Lerp(float a, float b, float t) => a + (b - a) * t;

        private static Color LerpColor(Color a, Color b, float t)
            => new Color(Lerp(a.r, b.r, t), Lerp(a.g, b.g, t), Lerp(a.b, b.b, t));

        private static float Clamp01(float value)
            => value < 0f ? 0f : (value > 1f ? 1f : value);
    }
}
