using GlassEngine.Core.Interop;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Control de la escena de estrés de cubos: equipa el RPG, dispara dos veces
    /// sobre el cúmulo y va reportando los conteos físicos.
    /// </summary>
    public class SesionCubos : GlassBehaviour
    {
        // Ids autorados por el generador de la escena
        const ulong JUGADOR          = 1;
        const ulong EMISOR_FOGONAZO  = 40;
        const ulong EMISOR_ONDA      = 41;
        const ulong EMISOR_HUMO      = 42;
        const ulong LUZ_FLASH        = 43;

        const int RPG_INDICE = 4;

        static readonly Vector3 CENTRO_CUMULO = new Vector3(0f, 2f, 0f);

        int frame;

        // Programa de la sesión. Los valores por defecto son los de la
        // prueba de 1.000 cubos; en los escalones grandes el driver
        // puede adelantar los disparos por variable de entorno (con
        // pilas de 5.000+ el frame tarda segundos y esperar al frame
        // 660 no aporta más señal de asiento).
        int fEquipar = 600;
        int fDisparo2 = 2400;
        int fFin = 3600;

        static int EnvEntero(string nombre, int defecto)
        {
            var v = System.Environment.GetEnvironmentVariable(nombre);
            return int.TryParse(v, out int n) && n > 0 ? n : defecto;
        }

        public override void Start()
        {
            fEquipar = EnvEntero("GLASS_CUBOS_F1", 600);
            fDisparo2 = EnvEntero("GLASS_CUBOS_F2", 2400);
            fFin = EnvEntero("GLASS_CUBOS_FIN", 3600);

            ProyectilRpg.EmisorFogonazoId = EMISOR_FOGONAZO;
            ProyectilRpg.EmisorOndaId     = EMISOR_ONDA;
            ProyectilRpg.EmisorHumoId     = EMISOR_HUMO;
            ProyectilRpg.LuzFlashId       = LUZ_FLASH;

            RuntimeBridge.GetPhysicsCounts(out int cuerpos, out int colliders);
            UnityEngine.Debug.Log($"[CUBOS] listo cuerpos={cuerpos} colliders={colliders}");
        }

        public override void Update(float dt)
        {
            frame++;
            var jugador = JugadorFps.Instancia;

            if (frame == fEquipar)
            {
                if (jugador != null)
                {
                    jugador.Equipar(RPG_INDICE);
                    jugador.ApuntarA(CENTRO_CUMULO);
                }
            }
            else if (frame == fEquipar + 60)
            {
                if (jugador != null) jugador.DispararUnaVez();
                UnityEngine.Debug.Log("[CUBOS] DISPARO 1");
            }
            else if (frame == fDisparo2)
            {
                if (jugador != null)
                {
                    jugador.Equipar(RPG_INDICE);
                    jugador.ApuntarA(CENTRO_CUMULO);
                    jugador.DispararUnaVez();
                }
                UnityEngine.Debug.Log("[CUBOS] DISPARO 2");
            }
            else if (frame == fFin)
            {
                RuntimeBridge.GetPhysicsCounts(out int cuerposFin, out int collidersFin);
                UnityEngine.Debug.Log($"[CUBOS] FIN cuerpos={cuerposFin}");
            }

            if (frame % 600 == 0)
            {
                RuntimeBridge.GetPhysicsCounts(out int cuerpos, out int colliders);
                UnityEngine.Debug.Log($"[CUBOS] t={frame} cuerpos={cuerpos}");
            }
        }
    }
}
