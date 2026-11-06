using System.Collections.Generic;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts;

/// <summary>
/// Zona de agua registrada por un Plat3DWaterZone: límites XZ, altura de
/// la superficie y punto de control (orilla de partida) donde reaparece
/// el jugador si se ahoga.
/// </summary>
public sealed class Plat3DWaterZoneData
{
    public float MinX;
    public float MaxX;
    public float MinZ;
    public float MaxZ;
    public float SurfaceY;
    public Vector3 Checkpoint;

    public bool ContainsXZ(Vector3 p)
    {
        return p.X >= MinX && p.X <= MaxX && p.Z >= MinZ && p.Z <= MaxZ;
    }
}

/// <summary>
/// Único punto de coordinación entre scripts del plataformas 3D.
/// Contrato de uso (diseño revisado por Claude en el bloque Plat3D):
///  - El Awake del jugador llama a ResetStatics() (los Awake corren al
///    instanciar la escena, antes que cualquier Update).
///  - TODOS los demás scripts se registran en su propio Start (Start es
///    diferido al primer Update de cada instancia: puede intercalarse
///    con Updates ajenos del frame 1, así que todo consumidor tolera
///    registros aún ausentes en ese primer frame).
///  - Cada "dueño" de un dato es el único que lo escribe y limpia; los
///    consumidores solo leen y toleran un frame de desfase por el orden
///    no garantizado de Update (HashMap).
/// </summary>
public static class Plat3DGameState
{
    // --- Jugador ---
    public static Vector3 PlayerPosition;
    public static bool PlayerPositionValid;
    public static Vector3 PlayerFacing = new Vector3 { X = 0f, Y = 0f, Z = 1f };
    public static float PlayerFacingYaw;
    public static int PlayerHealth;
    public const int PlayerMaxHealth = 5;
    public static Plat3DPlayerController Player = null!;

    // --- Cámara ---
    public static bool FirstPerson;

    // --- Enemigos ---
    public static readonly Dictionary<ulong, Plat3DEnemy> Enemies =
        new Dictionary<ulong, Plat3DEnemy>();

    // --- Pinchos (solo necesitamos el id de la entidad) ---
    public static readonly HashSet<ulong> SpikeIds = new HashSet<ulong>();

    // --- Plataformas móviles: delta de desplazamiento de ESTE frame ---
    public static readonly Dictionary<ulong, Vector3> PlatformDeltas =
        new Dictionary<ulong, Vector3>();

    // --- Proyectiles (pool pre-instanciado en escena) ---
    public static readonly List<Plat3DProjectile> ProjectilePool =
        new List<Plat3DProjectile>();

    // --- Zonas de agua ---
    public static readonly List<Plat3DWaterZoneData> WaterZones =
        new List<Plat3DWaterZoneData>();

    // --- Efecto de chapoteo pendiente (lo consume Plat3DSplash) ---
    public static Vector3 SplashPosition;
    public static bool SplashPending;

    // --- Estela de la barca: eventos circulares que consumen los
    // anillos Plat3DWakeRing por huecos (índice % 8) ---
    private static readonly Vector3[] WakePositions = new Vector3[64];
    public static int WakeSpawnCount;
    private static int _wakeRingRegistrations;

    // --- Relleno de vida diegético (barra 3D sobre el jugador) ---
    public static ulong HealthFillId;
    public static bool HealthFillRegistered;

    /// <summary>
    /// Resetea todo el estado estático. Debe llamarse SOLO desde el Awake
    /// del jugador, una vez por carga de escena.
    /// </summary>
    public static void ResetStatics()
    {
        PlayerPosition = new Vector3 { X = 0f, Y = 0f, Z = 0f };
        PlayerPositionValid = false;
        PlayerFacing = new Vector3 { X = 0f, Y = 0f, Z = 1f };
        PlayerFacingYaw = 0f;
        PlayerHealth = PlayerMaxHealth;
        Player = null!;
        FirstPerson = false;

        Enemies.Clear();
        SpikeIds.Clear();
        PlatformDeltas.Clear();
        ProjectilePool.Clear();
        WaterZones.Clear();
        SplashPending = false;
        WakeSpawnCount = 0;
        _wakeRingRegistrations = 0;

        HealthFillId = 0;
        HealthFillRegistered = false;
    }

    // ---------------- Jugador ----------------

    /// <summary>
    /// Reenvía daño al jugador. No-op seguro si no hay jugador vivo.
    /// `fromDirection` es la POSICIÓN del atacante: el empuje aleja al
    /// jugador de ese punto. La invulnerabilidad la gestiona el jugador.
    /// </summary>
    public static void DamagePlayer(int amount, Vector3 fromDirection)
    {
        if (Player == null)
        {
            return;
        }
        Player.TakeDamage(amount, fromDirection);
    }

    // ---------------- Enemigos ----------------

    public static void RegisterEnemy(ulong entityId, Plat3DEnemy enemy)
    {
        Enemies[entityId] = enemy;
    }

    public static void UnregisterEnemy(ulong entityId)
    {
        Enemies.Remove(entityId);
    }

    // ---------------- Pinchos ----------------

    public static void RegisterSpike(ulong entityId)
    {
        SpikeIds.Add(entityId);
    }

    // ---------------- Plataformas móviles ----------------

    /// <summary>
    /// Debe llamarse una vez por frame, al INICIO del Update de la propia
    /// plataforma, antes de moverla. Deja el delta en cero hasta que la
    /// plataforma escriba el real con SetPlatformDelta: un pasajero que
    /// lea antes obtiene el delta del frame anterior, nunca basura ni
    /// doble acumulación.
    /// </summary>
    public static void BeginPlatformFrame(ulong entityId)
    {
        PlatformDeltas[entityId] = new Vector3 { X = 0f, Y = 0f, Z = 0f };
    }

    public static void SetPlatformDelta(ulong entityId, Vector3 delta)
    {
        PlatformDeltas[entityId] = delta;
    }

    public static void UnregisterPlatform(ulong entityId)
    {
        PlatformDeltas.Remove(entityId);
    }

    // ---------------- Proyectiles ----------------

    public static void RegisterProjectile(Plat3DProjectile projectile)
    {
        ProjectilePool.Add(projectile);
    }

    /// <summary>
    /// Busca un proyectil libre en el pool y lo lanza. Devuelve false sin
    /// lanzar nada si no hay ninguno disponible (pool vacío o agotado);
    /// nunca lanza excepción ni asume que el pool ya está poblado.
    /// </summary>
    public static bool TryLaunchProjectile(Vector3 origin, Vector3 velocity)
    {
        for (int i = 0; i < ProjectilePool.Count; i++)
        {
            Plat3DProjectile candidate = ProjectilePool[i];
            if (candidate != null && candidate.IsAvailable)
            {
                candidate.Launch(origin, velocity);
                return true;
            }
        }
        return false;
    }

    // ---------------- Agua y chapoteo ----------------

    public static void RegisterWaterZone(Plat3DWaterZoneData zone)
    {
        WaterZones.Add(zone);
    }

    /// <summary>Pide a Plat3DSplash que muestre la onda de espuma en un punto.</summary>
    public static void RequestSplash(Vector3 position)
    {
        SplashPosition = position;
        SplashPending = true;
    }

    /// <summary>
    /// Registra un anillo de estela y devuelve su hueco (0..7 en la
    /// escena del lago). Los anillos se registran en su Start, después
    /// del ResetStatics del Awake del jugador.
    /// </summary>
    public static int RegisterWakeRing()
    {
        return _wakeRingRegistrations++;
    }

    /// <summary>Pide un anillo de espuma expansivo en un punto (estela de la barca).</summary>
    public static void RequestWake(Vector3 position)
    {
        WakePositions[WakeSpawnCount % 64] = position;
        WakeSpawnCount++;
    }

    public static Vector3 WakePositionAt(int index)
    {
        return WakePositions[index % 64];
    }

    // ---------------- UI: relleno de vida ----------------

    /// <summary>
    /// Registra la entidad del relleno de vida. Si ya hay uno registrado
    /// se ignora el nuevo (un solo registrador válido por escena).
    /// </summary>
    public static bool RegisterHealthFill(ulong entityId)
    {
        if (HealthFillRegistered)
        {
            return false;
        }
        HealthFillId = entityId;
        HealthFillRegistered = true;
        return true;
    }
}
