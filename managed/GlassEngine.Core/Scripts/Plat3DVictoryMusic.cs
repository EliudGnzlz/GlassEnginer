using GlassEngine.Core;
using GlassEngine.Core.Audio;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Pantalla de victoria: jingle de victoria una vez y la música en
    /// bucle a volumen bajo detrás.
    /// </summary>
    public class Plat3DVictoryMusic : GlassBehaviour
    {
        public override void Start()
        {
            AudioSource.Play2D("plat3d_victory", 0.85f, 1.0f, false);
            AudioSource.Play2D("plat3d_music", 0.22f, 1.0f, true);
        }
    }
}
