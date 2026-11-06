using GlassEngine.Core;
using GlassEngine.Core.Audio;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Música de fondo del nivel. Al cambiar de escena el motor hace
    /// stop_all(), así que cada escena de nivel trae una entidad con
    /// este script que arranca el bucle en su Start.
    /// </summary>
    public class Plat3DMusic : GlassBehaviour
    {
        public override void Start()
        {
            AudioSource.Play2D("plat3d_music", 0.32f, 1.0f, true);
        }
    }
}
