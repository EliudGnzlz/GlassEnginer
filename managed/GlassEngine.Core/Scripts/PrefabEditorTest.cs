using System;
using UnityEngine;

namespace GlassEngine.Core.Scripts;

/// <summary>
/// Verificación en vivo (2026-11-14) del prefab «Enemigo_Suelo_Este»,
/// creado minutos antes DESDE EL EDITOR con Assets → Save Selection as
/// Prefab sobre la entidad 126 de Plat3D_L2. Lo instancia desde C# con
/// GameObject.InstantiatePrefab y comprueba: raíz no nula, +3 entidades
/// (raíz + 2 hijos) y +1 cuerpo físico (la raíz Kinematic).
/// </summary>
public class PrefabEditorTest : GlassBehaviour
{
    private int _frame;
    private int _baseEntities;
    private int _baseBodies;
    private ulong _rootId;
    private bool _spawned;
    private bool _done;

    public override void Start()
    {
        _baseEntities = RuntimeBridge.GetEntityCount();
        RuntimeBridge.GetPhysicsCounts(out _baseBodies, out int col);
        Debug.Log($"[PREFABTEST] base entidades={_baseEntities} cuerpos={_baseBodies} colliders={col}");
    }

    public override void Update(float deltaTime)
    {
        if (_done) return;
        _frame++;
        if (!_spawned && _frame >= 30)
        {
            _spawned = true;
            var go = GameObject.InstantiatePrefab("Enemigo_Suelo_Este",
                new Vector3(0f, 1.5f, 0f));
            if (go is null)
            {
                Debug.Log("[PREFABTEST] RESULTADO instantiate=FALLO (null)");
                Debug.Log("[PREFABTEST] GLOBAL FALLO");
                _done = true;
                return;
            }
            _rootId = go.EntityId;
            Debug.Log($"[PREFABTEST] RESULTADO instantiate=OK raiz={_rootId}");
        }
        else if (_spawned && _frame >= 100)
        {
            int entities = RuntimeBridge.GetEntityCount();
            RuntimeBridge.GetPhysicsCounts(out int bodies, out int colliders);
            bool okEnt = entities == _baseEntities + 3;
            bool okBody = bodies == _baseBodies + 1;
            Debug.Log($"[PREFABTEST] RESULTADO entidades={entities} (esperado {_baseEntities + 3}) {(okEnt ? "OK" : "FALLO")}");
            Debug.Log($"[PREFABTEST] RESULTADO cuerpos={bodies} (esperado {_baseBodies + 1}) {(okBody ? "OK" : "FALLO")}");
            Debug.Log($"[PREFABTEST] RESULTADO colliders={colliders}");
            Debug.Log(okEnt && okBody ? "[PREFABTEST] GLOBAL OK" : "[PREFABTEST] GLOBAL FALLO");
            _done = true;
        }
    }
}
