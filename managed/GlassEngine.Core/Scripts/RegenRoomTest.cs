using System;
using System.Collections.Generic;
using System.IO;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts;

/// <summary>
/// Prueba de aceptación del bloque de instanciación real: genera una
/// «habitación» de 600 objetos mezclando primitivas con sus colliders
/// (400 por spawn directo y 200 prestados de un EntityPool creciente),
/// la deja asentarse, la destruye ENTERA y la regenera, durante 10
/// ciclos. En cada ciclo registra entidades, cuerpos y colliders vivos
/// (puente de instanciación: son los conteos del propio Rapier) y el
/// RSS del proceso leído de /proc/self/status.
///
/// Veredicto (línea REGEN DONE): PASS si tras cada borrado los tres
/// conteos vuelven EXACTO a la base de la escena y el RSS del último
/// ciclo vacío queda a menos de un 5 % del primero (sin tendencia
/// creciente; el RSS absoluto puede variar por el runtime, por eso se
/// informa también la serie completa). Antes de medir el RSS del ciclo
/// vacío se fuerza el GC: sin eso, el heap gestionado mediría lo que el
/// recolector aún no ha querido soltar, no una fuga.
///
/// También ejercita en el prólogo el pool FIJO (64 piezas: la 65.ª
/// petición devuelve 0) y la fachada Unity (CreatePrimitive + Destroy).
/// La escena es InstantiateRegenTest.scene.json.
/// </summary>
public class RegenRoomTest : GlassBehaviour
{
    private const int DirectObjects = 400;
    private const int PoolObjects = 200;
    private const int Cycles = 10;
    private const int SettleFrames = 45;
    private const int EmptyFrames = 5;

    private static readonly string[] Models = { "sphere", "capsule", "cylinder", "cube", "plane" };

    private enum Phase { Prologue, ProbeWait, Generating, Settling, Destroying, Emptying, Done }

    private Phase _phase = Phase.Prologue;
    private int _framesInPhase;
    private int _cycle;

    private int _baseEntities;
    private int _baseBodies;
    private int _baseColliders;
    private long _firstEmptyRssKb = -1;
    private long _lastEmptyRssKb = -1;
    private bool _verdict = true;
    private string _failReason = "";

    private readonly List<ulong> _direct = new();
    private readonly List<ulong> _pooledOut = new();
    private EntityPool? _growingPool;

    // Sonda de la revisión de Kimi: un pool DINÁMICO creciente. La pieza
    // precalentada y las que el pool hace crecer deben caer en el sitio
    // del Get: si una crecida naciese con el cuerpo en el origen, la
    // sincronización física la teletransportaría allí (x -> 0).
    private EntityPool? _probePool;
    private readonly List<(ulong Id, float ExpectedX)> _probePieces = new();

    public override void Start()
    {
        _baseEntities = RuntimeBridge.GetEntityCount();
        RuntimeBridge.GetPhysicsCounts(out _baseBodies, out _baseColliders);
        Log($"REGEN base entities={_baseEntities} bodies={_baseBodies} colliders={_baseColliders} rss_kb={ReadRssKb()}");
    }

    public override void Update(float deltaTime)
    {
        switch (_phase)
        {
            case Phase.Prologue:
                RunPoolPrologue();
                _framesInPhase = 0;
                _phase = Phase.ProbeWait;
                break;

            case Phase.ProbeWait:
                if (++_framesInPhase >= 90)
                {
                    CheckProbe();
                    _phase = Phase.Generating;
                }
                break;

            case Phase.Generating:
                GenerateRoom();
                _framesInPhase = 0;
                _phase = Phase.Settling;
                break;

            case Phase.Settling:
                if (++_framesInPhase >= SettleFrames)
                {
                    ReportCounts("full");
                    _phase = Phase.Destroying;
                }
                break;

            case Phase.Destroying:
                DestroyRoom();
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
                        float driftPct = _firstEmptyRssKb > 0
                            ? 100f * (_lastEmptyRssKb - _firstEmptyRssKb) / _firstEmptyRssKb
                            : 0f;
                        if (_verdict && driftPct > 5f)
                        {
                            Fail($"RSS con tendencia creciente: +{driftPct:F1} % entre el primer y el último ciclo vacío");
                        }
                        Log($"REGEN DONE cycles={Cycles} verdict={(_verdict ? "PASS" : "FAIL")} reason=\"{_failReason}\" " +
                            $"first_empty_rss_kb={_firstEmptyRssKb} last_empty_rss_kb={_lastEmptyRssKb} rss_drift_pct={driftPct:F2}");
                        _phase = Phase.Done;
                    }
                    else
                    {
                        _phase = Phase.Generating;
                    }
                }
                break;

            case Phase.Done:
                break;
        }
    }

    /// <summary>
    /// Prólogo de pools: el fijo se agota exactamente en su tamaño y el
    /// creciente nace con su precalentado; la fachada Unity crea y
    /// destruye una primitiva. Todo esto ocurre UNA vez, antes de los
    /// ciclos, y deja los conteos en la base al terminar.
    /// </summary>
    private void RunPoolPrologue()
    {
        // Pool fijo de 64 cubos estáticos.
        var fixedPool = new EntityPool("cube", prewarm: 64, grow: false,
            SpawnPhysicsKind.Static, namePrefix: "FixedProbe");
        var handed = new List<ulong>();
        for (int i = 0; i < 64; i++)
        {
            ulong id = fixedPool.Get(new Vector3(i % 8, 30f, i / 8));
            if (id == 0) Fail($"pool fijo: el Get {i + 1} de 64 devolvió 0");
            else handed.Add(id);
        }
        ulong sixtyFifth = fixedPool.Get(new Vector3(0f, 30f, 0f));
        if (sixtyFifth != 0) Fail("pool fijo: el Get 65 no devolvió 0 estando agotado");
        fixedPool.ReleaseAll();
        if (fixedPool.AvailableCount != 64) Fail($"pool fijo: tras ReleaseAll hay {fixedPool.AvailableCount} libres, no 64");
        // Segundo préstamo completo tras liberar: la reutilización real.
        for (int i = 0; i < 64; i++)
        {
            if (fixedPool.Get(new Vector3(i % 8, 30f, i / 8)) == 0)
                Fail($"pool fijo: el Get {i + 1} tras ReleaseAll devolvió 0");
        }
        fixedPool.DestroyAll();
        if (fixedPool.TotalCount != 0) Fail("pool fijo: DestroyAll no vació el pool");

        // Pool creciente de los ciclos: nace con 40 y crecerá hasta 200.
        _growingPool = new EntityPool("cube", prewarm: 40, grow: true,
            SpawnPhysicsKind.Static, scale: new Vector3(1.2f, 1.2f, 1.2f),
            color: new Vector3(0.25f, 0.55f, 0.85f), namePrefix: "RoomCube");

        // Fachada Unity: CreatePrimitive crea de verdad y Destroy quita.
        var probe = UnityEngine.GameObject.CreatePrimitive(UnityEngine.PrimitiveType.Sphere);
        if (probe is null || probe.EntityId == 0)
        {
            Fail("fachada Unity: CreatePrimitive no creó entidad");
        }
        else
        {
            UnityEngine.Object.Destroy(probe);
            if (probe.EntityId != 0) Fail("fachada Unity: Destroy no soltó el EntityId");
        }

        // Tras el prólogo, la escena debe estar otra vez en la base
        // (salvo las 40 piezas precalentadas del pool creciente, que son
        // entidades inactivas sin cuerpo: +40 entidades, +0 cuerpos).
        int entities = RuntimeBridge.GetEntityCount();
        RuntimeBridge.GetPhysicsCounts(out int bodies, out int colliders);
        if (entities != _baseEntities + 40 || bodies != _baseBodies || colliders != _baseColliders)
        {
            Fail($"tras el prólogo: entities={entities} (base+40={_baseEntities + 40}), bodies={bodies} (base={_baseBodies}), colliders={colliders} (base={_baseColliders})");
        }
        Log($"REGEN pools fixed=64/64+0 growing_prewarmed=40 facade=ok entities={entities} bodies={bodies} colliders={colliders}");

        // Sonda dinámica creciente (ver CheckProbe): 1 precalentada +
        // 2 crecidas, soltadas desde y=6 en x=6/9/12.
        _probePool = new EntityPool("sphere", prewarm: 1, grow: true,
            SpawnPhysicsKind.Dynamic, namePrefix: "ProbeSphere");
        _probePieces.Clear();
        foreach (float x in new[] { 6f, 9f, 12f })
        {
            ulong id = _probePool.Get(new Vector3(x, 6f, -20f));
            if (id == 0) Fail($"sonda: el Get en x={x} devolvió 0");
            else _probePieces.Add((id, x));
        }
    }

    /// <summary>
    /// Tras 40 frames, las tres esferas de la sonda deben haber caído
    /// (y entre 0,2 y 1,2) y seguir sobre su x del Get (±0,75): la
    /// precalentada y, sobre todo, las dos crecidas en caliente, cuyo
    /// cuerpo debe nacer en el sitio del Get y no en el origen.
    /// </summary>
    private void CheckProbe()
    {
        foreach (var (id, expectedX) in _probePieces)
        {
            if (RuntimeBridge.GetTransform(id, out TransformData t) != 0)
            {
                Fail($"sonda: GetTransform({id}) falló");
                continue;
            }
            bool ok = Math.Abs(t.Position.X - expectedX) < 0.75f
                      && t.Position.Y > 0.2f && t.Position.Y < 1.2f;
            Log($"REGEN probe id={id} x={t.Position.X:F2} y={t.Position.Y:F2} expected_x={expectedX} ok={ok}");
            if (!ok)
            {
                Fail($"sonda: la esfera {id} acabó en ({t.Position.X:F2}, {t.Position.Y:F2}) en vez de caer sobre x={expectedX}");
            }
        }
        _probePool?.DestroyAll();
        _probePool = null;
        _probePieces.Clear();
    }

    /// <summary>
    /// Genera la habitación del ciclo: 400 spawns directos (rejilla de
    /// 25×16, las cinco primitivas, dinámicos y estáticos alternados,
    /// escalas 0,8–1,6) + 200 cubos del pool creciente en otra rejilla.
    /// Todo en un solo frame: es el caso de ráfaga de un generador.
    /// </summary>
    private void GenerateRoom()
    {
        _direct.Clear();
        for (int i = 0; i < DirectObjects; i++)
        {
            int gx = i % 25;
            int gz = i / 25;
            bool dynamic = i % 3 == 0;
            var position = new Vector3(
                (gx - 12) * 1.4f,
                dynamic ? 4f + (i % 4) : 0.6f,
                (gz - 8) * 1.4f);
            float s = 0.8f + (i * 37 % 5) * 0.2f;
            var color = new Vector3(
                0.35f + (i % 4) * 0.18f,
                0.45f + (i % 3) * 0.2f,
                0.75f - (i % 5) * 0.1f);
            ulong id = RuntimeBridge.SpawnEntity(
                $"Room_{_cycle}_{i}", Models[i % Models.Length], null, 0,
                position, Quaternion.Identity, new Vector3(s, s, s),
                dynamic ? SpawnPhysicsKind.Dynamic : SpawnPhysicsKind.Static,
                color);
            if (id == 0) Fail($"ciclo {_cycle}: el spawn directo {i} devolvió 0");
            else _direct.Add(id);
        }

        _pooledOut.Clear();
        if (_growingPool is not null)
        {
            for (int i = 0; i < PoolObjects; i++)
            {
                int gx = i % 25;
                int gz = i / 25;
                ulong id = _growingPool.Get(new Vector3((gx - 12) * 1.4f, 0.6f, 26f + gz * 1.4f));
                if (id == 0) Fail($"ciclo {_cycle}: el Get {i} del pool creciente devolvió 0");
                else _pooledOut.Add(id);
            }
        }
    }

    /// <summary>Destruye la habitación entera en un solo frame.</summary>
    private void DestroyRoom()
    {
        foreach (ulong id in _direct)
        {
            if (!RuntimeBridge.DestroyEntity(id))
                Fail($"ciclo {_cycle}: DestroyEntity({id}) devolvió false");
        }
        _direct.Clear();
        // El pool se destruye entero (no ReleaseAll): el ciclo siguiente
        // lo vuelve a hacer crecer desde su precalentado, que es el
        // patrón «descartar la sala y regenerarla» del generador.
        _growingPool?.DestroyAll();
        _pooledOut.Clear();
    }

    private void ReportCounts(string phase)
    {
        int entities = RuntimeBridge.GetEntityCount();
        RuntimeBridge.GetPhysicsCounts(out int bodies, out int colliders);
        int expected = _baseEntities + DirectObjects + PoolObjects;
        if (phase == "full" && (entities != expected || bodies != _baseBodies + DirectObjects + PoolObjects))
        {
            Fail($"ciclo {_cycle} lleno: entities={entities} (esperadas {expected}), bodies={bodies} (esperados {_baseBodies + DirectObjects + PoolObjects})");
        }
        Log($"REGEN cycle={_cycle} phase={phase} entities={entities} bodies={bodies} colliders={colliders} rss_kb={ReadRssKb()}");
    }

    private void CheckEmptyCycle()
    {
        // El GC forzado separa «memoria retenida por el recolector» de
        // «memoria que el proceso ya no puede soltar»: solo lo segundo
        // es una fuga atribuible al motor o al puente.
        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();

        int entities = RuntimeBridge.GetEntityCount();
        RuntimeBridge.GetPhysicsCounts(out int bodies, out int colliders);
        long rss = ReadRssKb();
        if (_firstEmptyRssKb < 0) _firstEmptyRssKb = rss;
        _lastEmptyRssKb = rss;
        // Las 40 piezas precalentadas del pool creciente se destruyeron
        // con DestroyAll y el pool las recrea al generar: en vacío la
        // escena está EXACTAMENTE en la base.
        if (entities != _baseEntities || bodies != _baseBodies || colliders != _baseColliders)
        {
            Fail($"ciclo {_cycle} vacío: entities={entities} (base {_baseEntities}), bodies={bodies} (base {_baseBodies}), colliders={colliders} (base {_baseColliders})");
        }
        Log($"REGEN cycle={_cycle} phase=empty entities={entities} bodies={bodies} colliders={colliders} rss_kb={rss}");
    }

    private void Fail(string reason)
    {
        if (_verdict)
        {
            _failReason = reason;
        }
        _verdict = false;
        Log($"REGEN FAIL {reason}");
    }

    private static long ReadRssKb()
    {
        try
        {
            foreach (string line in File.ReadLines("/proc/self/status"))
            {
                if (line.StartsWith("VmRSS:"))
                {
                    string[] parts = line.Split(new[] { ' ', '\t' }, StringSplitOptions.RemoveEmptyEntries);
                    return long.Parse(parts[1]);
                }
            }
        }
        catch
        {
            // Sin /proc (no-Linux): la medición no aplica.
        }
        return -1;
    }
}
