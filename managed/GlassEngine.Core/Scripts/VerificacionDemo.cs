using System;
using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Verificación automática de la demo FPS: dispara el arma del
    /// jugador (vía su propia API pública) hasta matar al enemigo,
    /// sin depender del input X11 (a ~1 fps el muestreo pierde
    /// pulsaciones). Solo se usa en la escena DemoVerif.
    /// </summary>
    public class VerificacionDemo : GlassBehaviour
    {
        private float _t;
        private float _desdeDisparo;
        private int _disparos;
        private bool _fin;
        private bool _arEquipada;

        public override void Update(float dt)
        {
            if (_fin) return;
            _t += dt;
            _desdeDisparo += dt;
            if (_t < 1.0f) return;
            var jug = JugadorPersonaje.Instancia;
            if (jug == null) return;
            // Fase 1: pistola (proyectil). Fase 2: si sobrevive a 24
            // disparos, AR hitscan. Se anota que arma lo mato.
            if (_disparos == 24 && !_arEquipada)
            {
                _arEquipada = true;
                jug.Equipar(1);
                Debug.Log("[VERIF] la pistola no basto; cambio al AR");
            }
            var ene = EnemigoSimple.Instancia;
            if (ene != null && ene.Vida <= 0f)
            {
                Debug.Log($"[VERIF] enemigo muerto tras {_disparos} disparos (fase AR: {_arEquipada})");
                _fin = true;
                return;
            }
            if (_desdeDisparo >= 0.5f && _disparos < 60)
            {
                _desdeDisparo = 0f;
                if (jug.DispararUnaVez()) _disparos++;
                else jug.Recargar();
            }
            if (_disparos >= 90)
            {
                Debug.Log("[VERIF] agotados los disparos sin matar al enemigo");
                _fin = true;
            }
        }
    }
}
