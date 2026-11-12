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

        public override void Start()
        {
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

            if (frame == 600)
            {
                if (jugador != null)
                {
                    jugador.Equipar(RPG_INDICE);
                    jugador.ApuntarA(CENTRO_CUMULO);
                }
            }
            else if (frame == 660)
            {
                if (jugador != null) jugador.DispararUnaVez();
                UnityEngine.Debug.Log("[CUBOS] DISPARO 1");
            }
            else if (frame == 2400)
            {
                if (jugador != null)
                {
                    jugador.Equipar(RPG_INDICE);
                    jugador.ApuntarA(CENTRO_CUMULO);
                    jugador.DispararUnaVez();
                }
                UnityEngine.Debug.Log("[CUBOS] DISPARO 2");
            }
            else if (frame == 3600)
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
