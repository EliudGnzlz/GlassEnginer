using GlassEngine.Core;
using GlassEngine.Core.Interop;
using UnityEngine;
// Vector3 sin ambigüedad: el de Interop, que es el que usan
// RuntimeBridge y TransformData (el de UnityEngine chocaba, CS0104).
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts
{
    public class EscaparateControl : GlassBehaviour
    {
        // Ids de las entidades de la escena escaparate
        private const ulong SOL = 2;
        private const ulong EMISOR_LLUVIA = 50;
        private const ulong EMISOR_LLUVIA_FINA = 54;
        private const ulong EMISOR_ARENA = 51;
        private const ulong EMISOR_POLVO = 55;
        private const ulong MURO_POLVO = 52;
        private const ulong BOLA = 9;

        // El portal está en z = -9
        private const float PORTAL_Z = -9f;
        private const float DURACION_FASE = 24f;
        private const float INTERVALO_SONDA = 7f;
        private const float UMBRAL_QUIETA = 0.5f;

        private float _tiempoFase;
        private float _tiempoSonda;
        private bool _tormenta;
        private bool _ladoConocido;
        private int _ladoAnterior;
        // Handle del cuerpo de la bola: las operaciones de velocidad
        // van por handle, no por id de entidad (Fase 9).
        private ulong _bolaHandle;

        public override void Start()
        {
            // Arranque en clima LLUVIA
            _tormenta = false;
            _tiempoFase = 0f;
            _tiempoSonda = 0f;
            AplicarClima(false);
            Debug.Log("[ESCAPARATE] clima: LLUVIA");
        }

        public override void Update(float deltaTime)
        {
            // Alternancia de clima cada 24 s
            _tiempoFase += deltaTime;
            if (_tiempoFase >= DURACION_FASE)
            {
                _tiempoFase -= DURACION_FASE;
                _tormenta = !_tormenta;
                AplicarClima(_tormenta);
                Debug.Log(_tormenta ? "[ESCAPARATE] clima: TORMENTA" : "[ESCAPARATE] clima: LLUVIA");
            }

            // Posición de la bola: se lee cada Update para detectar el cruce del portal
            TransformData transform;
            if (RuntimeBridge.GetTransform(BOLA, out transform) != 0)
            {
                return; // sin datos válidos este ciclo
            }

            float z = transform.Position.Z;

            // Lado del portal según el signo de (z + 9)
            int lado = (z - PORTAL_Z) >= 0f ? 1 : -1;
            if (_ladoConocido && lado != _ladoAnterior)
            {
                Debug.Log("[ESCAPARATE] sonda cruzó el portal");
            }
            _ladoAnterior = lado;
            _ladoConocido = true;

            // Relanzamiento de la sonda cada 7 s (temporizador propio)
            _tiempoSonda += deltaTime;
            if (_tiempoSonda < INTERVALO_SONDA)
            {
                return;
            }
            _tiempoSonda -= INTERVALO_SONDA;

            // Solo se relanza si la bola está casi quieta (o si no se pudo comprobar)
            if (_bolaHandle == 0)
            {
                _bolaHandle = RuntimeBridge.GetBodyHandle(BOLA);
                if (_bolaHandle == 0)
                {
                    return; // la bola aún no tiene cuerpo; se reintenta
                }
            }
            bool relanzar = true;
            Vector3 velocidadActual;
            if (RuntimeBridge.GetLinearVelocity(_bolaHandle, out velocidadActual))
            {
                float vx = velocidadActual.X;
                float vy = velocidadActual.Y;
                float vz = velocidadActual.Z;
                float modulo = (float)System.Math.Sqrt(vx * vx + vy * vy + vz * vz);
                relanzar = modulo < UMBRAL_QUIETA;
            }

            if (!relanzar)
            {
                return;
            }

            // Dirección hacia el portal según el lado actual. 8 m/s:
            // con el damping del motor, a 6 m/s la bola llegaba al
            // plano del portal casi parada (medido en la sonda).
            float vzPortal = z >= PORTAL_Z ? -8f : 8f;
            Vector3 nuevaVelocidad = new Vector3 { X = 0f, Y = 0.5f, Z = vzPortal };
            RuntimeBridge.SetLinearVelocity(_bolaHandle, nuevaVelocidad);
        }

        private void AplicarClima(bool tormenta)
        {
            RuntimeBridge.SetEntityActive(EMISOR_LLUVIA, !tormenta);
            RuntimeBridge.SetEntityActive(EMISOR_LLUVIA_FINA, !tormenta);
            RuntimeBridge.SetEntityActive(EMISOR_ARENA, tormenta);
            RuntimeBridge.SetEntityActive(EMISOR_POLVO, tormenta);
            RuntimeBridge.SetEntityActive(MURO_POLVO, tormenta);
            // Sol atenuado de verdad en tormenta y niebla a juego:
            // arena cálida y cercana; en lluvia, la niebla fría
            // autorada de la escena.
            RuntimeBridge.LightSetIntensity(SOL, tormenta ? 0.35f : 1.0f);
            if (tormenta)
            {
                RuntimeBridge.SetFog(0.66f, 0.52f, 0.32f, 9f, 48f);
            }
            else
            {
                RuntimeBridge.SetFog(0.55f, 0.62f, 0.72f, 20f, 60f);
            }
        }
    }
}
