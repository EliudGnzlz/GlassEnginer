using System;
using UnityEngine;
using GlassEngine.Core.Interop;
using System.Collections.Generic;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts
{
    /// <summary>Guion de la demo FPS de la Fase 10: puntería, disparos, luces y ciclos C#.</summary>
    public class SesionFps : GlassBehaviour
    {
        // Coordenadas constantes (la escena se autora para que coincidan).
        static readonly Vector3 Diana10 = new Vector3(0f, 1.6f, -10f);
        static readonly Vector3 Diana25 = new Vector3(-4f, 1.6f, -25f);
        static readonly Vector3 Diana50 = new Vector3(5f, 1.6f, -50f);
        static readonly Vector3 Diana100 = new Vector3(0f, 1.8f, -100f);
        static readonly Vector3 Barril = new Vector3(3f, 0.9f, -15f);
        static readonly Vector3 BlancoA = new Vector3(-20f, 1.6f, -30f); // extremo del raíl
        static readonly Vector3 BlancoB = new Vector3(20f, 1.6f, -30f);  // extremo del raíl

        static readonly Vector3[] CentrosDiana = { Diana10, Diana25, Diana50, Diana100 };
        static readonly float[] DistanciasDiana = { 10f, 25f, 50f, 100f };

        private int _frame;
        private int _ciclo;
        private readonly List<ulong> _monedas = new List<ulong>();

        public override void Start()
        {
            ConfigurarDianas();
            CablearBarriles();
        }

        /// <summary>Cableado de la escena: emisores compartidos de la
        /// explosión (ids 40/41, junto al barril principal), vecinos
        /// de impulso por barril y posiciones autoradas de todos los
        /// cuerpos empujables. Los ids salen del generador de la
        /// escena Fase10CampoTiro.</summary>
        private static void CablearBarriles()
        {
            BarrilExplosivo.PosicionesVecinos[30] = new Vector3(3f, 0.7f, -15f);
            BarrilExplosivo.PosicionesVecinos[31] = new Vector3(-1f, 0.7f, -19f);
            BarrilExplosivo.PosicionesVecinos[32] = new Vector3(5f, 0.7f, -60f);
            BarrilExplosivo.PosicionesVecinos[44] = new Vector3(4.5f, 0.4f, -14f);
            BarrilExplosivo.PosicionesVecinos[45] = new Vector3(1.8f, 0.4f, -16.5f);
            BarrilExplosivo.PosicionesVecinos[46] = new Vector3(4.6f, 0.4f, -17.5f);
            BarrilExplosivo.PosicionesVecinos[47] = new Vector3(-0.5f, 0.4f, -20.5f);
            BarrilExplosivo.PosicionesVecinos[48] = new Vector3(5.8f, 0.4f, -58f);
            BarrilExplosivo.PosicionesVecinos[49] = new Vector3(4f, 0.4f, -61.5f);
            for (int i = 0; i < BarrilExplosivo.Barriles.Count; i++)
            {
                var b = BarrilExplosivo.Barriles[i];
                b.EmisorFuegoId = 40;
                b.EmisorHumoId = 41;
                if (b.EntityId == 30) b.VecinosImpulso.AddRange(new ulong[] { 44, 45, 46, 31, 47 });
                else if (b.EntityId == 31) b.VecinosImpulso.AddRange(new ulong[] { 47, 44, 30 });
                else if (b.EntityId == 32) b.VecinosImpulso.AddRange(new ulong[] { 48, 49 });
            }
        }

        private static void ConfigurarDianas()
        {
            // La sesión conoce la escena: asigna a cada Diana su distancia nominal y su centro.
            for (int i = 0; i < Diana.Dianas.Count; i++)
            {
                var d = Diana.Dianas[i];
                if (d == null) continue;

                int mejor = 0;
                float mejorD = float.MaxValue;
                for (int k = 0; k < CentrosDiana.Length; k++)
                {
                    float dx = d.Position.X - CentrosDiana[k].X;
                    float dy = d.Position.Y - CentrosDiana[k].Y;
                    float dz = d.Position.Z - CentrosDiana[k].Z;
                    float dd = dx * dx + dy * dy + dz * dz;
                    if (dd < mejorD) { mejorD = dd; mejor = k; }
                }

                // Solo se configura la diana FIJA cercana a un centro
                // autorado; los blancos móviles quedan con distancia
                // 0 (se identifican por su posición en el log).
                if (mejorD < 4f)
                    d.Configurar(DistanciasDiana[mejor], CentrosDiana[mejor]);
            }
        }

        public override void Update(float dt)
        {
            _frame++;
            int f = _frame;
            // Reintento de configuración: el Start de la sesión puede
            // correr antes que el de las dianas (orden no garantizado).
            if (f == 60) ConfigurarDianas();
            var j = JugadorFps.Instancia;

            if (j != null)
            {
                switch (f)
                {
                    // Pistola a 10 m
                    case 120: j.Equipar(0); j.ApuntarA(Diana10); break;
                    case 150: case 210: case 270: j.DispararUnaVez(); break;

                    // Escopeta a 10 m
                    case 400: j.Equipar(2); j.ApuntarA(Diana10); break;
                    case 430: case 520: j.DispararUnaVez(); break;

                    // AR a 25 m y a 50 m
                    case 700: j.Equipar(1); j.ApuntarA(Diana25); break;
                    case 730: case 760: case 790: j.DispararUnaVez(); break;
                    case 900: j.ApuntarA(Diana50); break;
                    case 930: case 960: j.DispararUnaVez(); break;

                    // Pistola a 100 m: aquí se mide la caída
                    case 1100: j.Equipar(0); j.ApuntarA(Diana100); break;
                    case 1130: case 1230: case 1330: j.DispararUnaVez(); break;

                    // Francotirador con mira a 100 m
                    case 1500: j.Equipar(3); j.Mira(true); j.ApuntarA(Diana100); break;
                    case 1560: case 1700: j.DispararUnaVez(); break;
                    case 1760: j.Mira(false); break;

                    // AR contra el barril
                    case 1900: j.Equipar(1); j.ApuntarA(Barril); break;
                    case 1930: case 1990: case 2050: j.DispararUnaVez(); break;

                    // Blancos móviles: se apunta a los extremos del raíl (puntos fijos)
                    case 2300: if (BlancoMovil.Moviles.Count > 0) { j.ApuntarA(BlancoMovil.Moviles[0].Position); j.DispararUnaVez(); } break;
                    case 2330: j.DispararUnaVez(); break;
                    case 2600: if (BlancoMovil.Moviles.Count > 1) { j.ApuntarA(BlancoMovil.Moviles[1].Position); j.DispararUnaVez(); } break;
                    case 2630: j.DispararUnaVez(); break;
                }
            }

            // Escalones de luces: 16 luces puntuales ids 130..145.
            for (int paso = 0; paso < 5; paso++)
            {
                if (f == 3000 + paso * 500)
                {
                    int k = paso * 4;
                    for (int i = 0; i < 16; i++)
                        RuntimeBridge.SetEntityActive((ulong)(130 + i), i < k);
                    Debug.Log($"[F10] LUCES {k}");
                }
            }

            // Ciclos C# de monedas: 5 ciclos de 40 prefabs (300 frames de vida).
            if (f >= 5600 && _ciclo < 5)
            {
                int inicio = 5600 + _ciclo * 340;
                if (f == inicio) SpawnCiclo(_ciclo + 1);
                else if (f == inicio + 300) { DespawnCiclo(_ciclo + 1); _ciclo++; }
            }

            if (f == 9000)
            {
                int total = JugadorFps.Instancia != null ? JugadorFps.Instancia.DisparosRealizados : 0;
                Debug.Log($"[F10] SESION FIN disparos={total}");
            }
        }

        private void SpawnCiclo(int n)
        {
            _monedas.Clear();
            for (int i = 0; i < 40; i++)
            {
                int col = i % 8;
                int fila = i / 8;
                float x = -5.25f + col * 1.5f;
                float z = -20f + fila * 1.5f;

                var go = UnityEngine.GameObject.InstantiatePrefab(
                    "Gold_Coin", new UnityEngine.Vector3(x, 1f, z));
                if (go != null) _monedas.Add(go.EntityId);
            }
            Debug.Log($"[F10] CICLO_CS {n} SPAWN 40");
        }

        private void DespawnCiclo(int n)
        {
            for (int i = 0; i < _monedas.Count; i++)
                RuntimeBridge.DestroyEntity(_monedas[i]);
            _monedas.Clear();
            Debug.Log($"[F10] CICLO_CS {n} DESPAWN");
        }
    }
}
