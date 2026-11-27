using System.Collections;

namespace UnityEngine
{
    // ==========================================
    // INSTRUCCIONES DE ESPERA DE CORRUTINAS (§4)
    // ==========================================

    /// <summary>Base de las esperas que interpreta el planificador.</summary>
    public class YieldInstruction
    {
    }

    /// <summary>
    /// Handle de una corrutina en marcha. Envuelve la rutina interna del
    /// planificador; sirve para StopCoroutine(Coroutine) y para
    /// <c>yield return</c> (esperar a que termine).
    /// </summary>
    public sealed class Coroutine : YieldInstruction
    {
        internal Coroutine(GlassEngine.Core.Hosting.CoroutineRoutine routine)
        {
            Routine = routine;
        }

        internal GlassEngine.Core.Hosting.CoroutineRoutine Routine { get; }
    }

    /// <summary>
    /// Espera en tiempo de juego (Time.time). El plazo se calcula al ceder,
    /// no al construir, así que la instancia se puede cachear y reutilizar.
    /// </summary>
    public sealed class WaitForSeconds : YieldInstruction
    {
        public WaitForSeconds(float seconds)
        {
            Seconds = seconds;
        }

        internal float Seconds { get; }
    }

    /// <summary>Reanuda al final del frame (aquí, al inicio del tick siguiente).</summary>
    public sealed class WaitForEndOfFrame : YieldInstruction
    {
    }

    /// <summary>Reanuda tras el siguiente paso fijo.</summary>
    public sealed class WaitForFixedUpdate : YieldInstruction
    {
    }

    /// <summary>
    /// Espera definida por el usuario. El planificador la apila como un
    /// IEnumerator más y llama a MoveNext (es decir, keepWaiting) en cada
    /// bomba de Update hasta que devuelva false.
    /// </summary>
    public abstract class CustomYieldInstruction : IEnumerator
    {
        public abstract bool keepWaiting { get; }

        public object? Current => null;

        public bool MoveNext() => keepWaiting;

        public virtual void Reset()
        {
        }
    }

    /// <summary>
    /// Espera en tiempo real, sin depender del tiempo de juego. El plazo se
    /// fija en la primera consulta de keepWaiting y se rearma al cumplirse,
    /// así que la instancia es reutilizable (igual que en Unity).
    /// </summary>
    public class WaitForSecondsRealtime : CustomYieldInstruction
    {
        // Reloj monótono propio: no consta Time.realtimeSinceStartup en la fachada.
        private static readonly System.Diagnostics.Stopwatch s_reloj = System.Diagnostics.Stopwatch.StartNew();

        // Plazo absoluto en segundos del reloj; negativo = sin fijar.
        private double _plazo = -1;

        public WaitForSecondsRealtime(float time)
        {
            waitTime = time;
        }

        public float waitTime { get; set; }

        public override bool keepWaiting
        {
            get
            {
                double ahora = s_reloj.Elapsed.TotalSeconds;
                if (_plazo < 0)
                    _plazo = ahora + waitTime;

                bool esperar = ahora < _plazo;
                if (!esperar)
                    Reset();
                return esperar;
            }
        }

        public override void Reset() => _plazo = -1;
    }
}
