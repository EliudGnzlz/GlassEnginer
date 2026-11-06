using System;
using System.Collections.Generic;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts;

/// <summary>
/// Prueba de aceptación del puente de instanciación de Prefabs: en
/// cada ciclo instancia el prefab «Floating_Platform» 20 veces (posiciones
/// repartidas en una línea sobre X) con
/// <see cref="UnityEngine.GameObject.InstantiatePrefab"/>, deja asentar, destruye
/// TODAS las raíces con <see cref="UnityEngine.Object.Destroy"/> y verifica que el
/// conteo de entidades vuelve exacto a la base de la escena. 5 ciclos;
/// mensaje final «PrefabPeakTest OK» si los 5 pasan.
///
/// Calcada del patrón de <see cref="RegenRoomTest"/>: base tomada en
/// <c>Start</c>, fases por temporizador en <c>Update</c>, veredicto
/// comparando contra esa base.
/// </summary>
public class PrefabPeakTest : GlassBehaviour
{
    private const int CoinsPerCycle = 20;
    private const int Cycles = 5;
    private const int SettleFrames = 30;
    private const int EmptyFrames = 5;

    private enum Phase { Spawning, Settling, Destroying, Emptying, Done }

    private Phase _phase = Phase.Spawning;
    private int _framesInPhase;
    private int _cycle;

    private int _baseEntities;
    private int _baseBodies;
    private int _baseColliders;
    private bool _verdict = true;
    private string _failReason = "";

    private readonly List<UnityEngine.GameObject> _coins = new();

    public override void Start()
    {
        _baseEntities = RuntimeBridge.GetEntityCount();
        RuntimeBridge.GetPhysicsCounts(out _baseBodies, out _baseColliders);
        Log($"PREFABPEAK base entities={_baseEntities} bodies={_baseBodies} colliders={_baseColliders}");
    }

    public override void Update(float deltaTime)
    {
        switch (_phase)
        {
            case Phase.Spawning:
                SpawnCoins();
                _framesInPhase = 0;
                _phase = Phase.Settling;
                break;

            case Phase.Settling:
                if (++_framesInPhase >= SettleFrames)
                {
                    RuntimeBridge.GetPhysicsCounts(out int peakBodies, out int peakColliders);
                    Log($"PREFABPEAK ciclo {_cycle} spawneadas={_coins.Count} entities={RuntimeBridge.GetEntityCount()} bodies={peakBodies} colliders={peakColliders} baseBodies={_baseBodies}");
                    _phase = Phase.Destroying;
                }
                break;

            case Phase.Destroying:
                DestroyCoins();
                _framesInPhase = 0;
                _phase = Phase.Emptying;
                break;

            case Phase.Emptying:
                if (++_framesInPhase >= EmptyFrames)
                {
                    CheckEmptyCycle();
                    _cycle++;
                    if (_cycle >= Cycles)
                    {
                        _phase = Phase.Done;
                    }
                    else
                    {
                        _phase = Phase.Spawning;
                    }
                }
                break;

            case Phase.Done:
                Log(_verdict
                    ? "PrefabPeakTest OK"
                    : $"PrefabPeakTest FAIL: {_failReason}");
                _phase = Phase.Done + 1; // fuera de rango: no vuelve a entrar al switch útil
                break;
        }
    }

    private void SpawnCoins()
    {
        for (int i = 0; i < CoinsPerCycle; i++)
        {
            var pos = new UnityEngine.Vector3(i * 2.0f, 0f, 0f);
            var go = UnityEngine.GameObject.InstantiatePrefab("Floating_Platform", pos);
            if (go is null)
            {
                _verdict = false;
                _failReason = $"ciclo {_cycle}: InstantiatePrefab devolvió null en la moneda {i}";
                continue;
            }
            _coins.Add(go);
        }
    }

    private void DestroyCoins()
    {
        foreach (var coin in _coins)
        {
            UnityEngine.Object.Destroy(coin);
        }
        _coins.Clear();
    }

    private void CheckEmptyCycle()
    {
        int entities = RuntimeBridge.GetEntityCount();
        RuntimeBridge.GetPhysicsCounts(out int bodies, out int colliders);
        if (entities != _baseEntities)
        {
            _verdict = false;
            _failReason = $"ciclo {_cycle}: entities={entities} esperado={_baseEntities}";
        }
        if (bodies != _baseBodies || colliders != _baseColliders)
        {
            _verdict = false;
            _failReason = $"ciclo {_cycle}: bodies={bodies}/{colliders} esperado={_baseBodies}/{_baseColliders} (cuerpos huérfanos)";
        }
        Log($"PREFABPEAK ciclo {_cycle} vacío entities={entities} base={_baseEntities} bodies={bodies} baseBodies={_baseBodies}");
    }

    private static void Log(string message) => UnityEngine.Debug.Log(message);
}
