using GlassEngine.Core;
using GlassEngine.Core.Audio;
using UnityEngine.SceneManagement;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Script de las entidades de botón de la pantalla de victoria: el
    /// Canvas invoca por nombre (UIButtonData.on_click_action) el método
    /// público sin parámetros que nombre cada botón: «JUGAR DE NUEVO»
    /// (RestartGame, pasa por la pantalla de carga del nivel 1) y
    /// «VOLVER AL MENU» (GoMenu).
    /// </summary>
    public class Plat3DRestartButton : GlassBehaviour
    {
        public void RestartGame()
        {
            AudioSource.Play2D("plat3d_click", 0.8f, 1.0f, false);
            SceneManager.LoadScene("Plat3D_Loading_L1");
        }

        public void GoMenu()
        {
            AudioSource.Play2D("plat3d_click", 0.8f, 1.0f, false);
            SceneManager.LoadScene("Plat3D_Menu");
        }
    }
}
