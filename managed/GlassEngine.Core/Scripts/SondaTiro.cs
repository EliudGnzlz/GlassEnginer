using System;
using UnityEngine;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts
{
    /// <summary>Sonda de verificación de los números de daño:
    /// dispara la pistola (hitscan) desde el puesto de tiro
    /// (30, 1.6, 0) contra el centro de Diana25 (30, 1.6, -25)
    /// cada 1,4 s de guion, para ver el número flotante sobre la
    /// diana en las capturas. Solo vive en la escena SondaTiro.
    /// </summary>
    public class SondaTiro : GlassBehaviour
    {
        private ArmaDef _pistola;
        private readonly Random _rng = new Random(12345);
        private float _t;
        private int _disparos;

        public override void Start()
        {
            SistemaArmas.Cargar();
            foreach (var arma in SistemaArmas.Armas)
            {
                if (!arma.Proyectil)
                {
                    _pistola = arma;
                    break;
                }
            }
            Debug.Log($"[SONDA] lista: pistola={_pistola?.Nombre} dano={_pistola?.Dano}");
        }

        public override void Update(float deltaTime)
        {
            if (_pistola == null)
                return;

            _t += deltaTime;
            if (_t < 1.4f)
                return;

            _t = 0f;
            _disparos++;
            int efectivos = SistemaArmas.Disparar(
                _pistola,
                new Vector3(30f, 1.6f, 0f),
                new Vector3(0f, 0f, -1f),
                _rng);
            Debug.Log($"[SONDA] disparo {_disparos} efectivos={efectivos}");
        }
    }
}
