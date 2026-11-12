using System.Collections.Generic;
using UnityEngine;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts
{
    public class PruebaIntegralControl : GlassBehaviour
    {
        const int LuzBase = 30;
        const int LuzTotal = 16;
        const int FrameInicioCiclos = 2600;
        const int FrameSpawnEspera = 300;
        const int FrameDespawnEspera = 150;
        const int TotalCiclos = 5;

        int _f;
        int _rayA, _rayT, _sphA, _sphT, _ovA, _ovT;
        int _pasoLuces;

        // Fases: 0 idle, 1 espera tras spawn, 2 espera tras despawn, 3 fin
        int _fase;
        int _faseFrame;
        int _ciclo;
        readonly List<GameObject> _coins = new List<GameObject>();

        public override void Update(float deltaTime)
        {
            Consultas();
            EscalonesLuces();
            Ciclos();
            _f++;
        }

        void Consultas()
        {
            if (_f % 30 != 0) return;

            _rayT++;
            if (Physics.Physics.Raycast(new Vector3(-10f, 6f, -2f), new Vector3(0f, -1f, 0f), out GlassEngine.Core.Physics.RaycastHit hit, 20f))
                _rayA++;

            _sphT++;
            if (Physics.Physics.SphereCast(new Vector3(-14f, 1f, -2f), 0.5f, new Vector3(1f, 0f, 0f), out GlassEngine.Core.Physics.RaycastHit hit2, 12f))
                _sphA++;

            _ovT++;
            ulong[] ov = Physics.Physics.OverlapSphere(new Vector3(-10f, 1f, -2f), 2.5f);
            if (ov != null && ov.Length >= 1)
                _ovA++;

            if (_f > 0 && _f % 900 == 0)
            {
                Debug.Log($"[PRUEBA] CONSULTAS rayo={_rayA}/{_rayT} esfera={_sphA}/{_sphT} overlap={_ovA}/{_ovT}");
            }
        }

        void EscalonesLuces()
        {
            if (_pasoLuces >= 5) return;
            int frameObjetivo = 300 + _pasoLuces * 500;
            if (_f < frameObjetivo) return;

            int k = _pasoLuces * 4;
            for (int i = 0; i < LuzTotal; i++)
            {
                RuntimeBridge.SetEntityActive((ulong)(LuzBase + i), i < k);
            }
            Debug.Log("[PRUEBA] LUCES " + k);
            _pasoLuces++;
        }

        void Ciclos()
        {
            switch (_fase)
            {
                case 0:
                    if (_f >= FrameInicioCiclos)
                    {
                        _ciclo = 1;
                        Spawn();
                        _fase = 1;
                        _faseFrame = _f;
                    }
                    break;

                case 1:
                    if (_f - _faseFrame >= FrameSpawnEspera)
                    {
                        Despawn();
                        if (_ciclo >= TotalCiclos)
                        {
                            Debug.Log("[PRUEBA] CONTROL FIN");
                            _fase = 3;
                        }
                        else
                        {
                            _fase = 2;
                            _faseFrame = _f;
                        }
                    }
                    break;

                case 2:
                    if (_f - _faseFrame >= FrameDespawnEspera)
                    {
                        _ciclo++;
                        Spawn();
                        _fase = 1;
                        _faseFrame = _f;
                    }
                    break;
            }
        }

        void Spawn()
        {
            _coins.Clear();
            for (int fila = 0; fila < 5; fila++)
            {
                for (int col = 0; col < 8; col++)
                {
                    var pos = new UnityEngine.Vector3(8f + col * 0.8f, 0.6f, -6f - fila * 0.8f);
                    UnityEngine.GameObject go = UnityEngine.GameObject.InstantiatePrefab("Gold_Coin", pos);
                    if (go != null) _coins.Add(go);
                }
            }
            Debug.Log($"[PRUEBA] CICLO_CS {_ciclo} SPAWN 40");
        }

        void Despawn()
        {
            for (int i = 0; i < _coins.Count; i++)
            {
                if (_coins[i] != null) UnityEngine.Object.Destroy(_coins[i]);
            }
            _coins.Clear();
            Debug.Log($"[PRUEBA] CICLO_CS {_ciclo} DESPAWN");
        }
    }
}
