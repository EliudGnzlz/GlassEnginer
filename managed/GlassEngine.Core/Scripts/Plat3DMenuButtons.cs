using GlassEngine.Core;
using GlassEngine.Core.Audio;
using UnityEngine.SceneManagement;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Script de la entidad del botón JUGAR del menú principal: el Canvas
    /// invoca por nombre (UIButtonData.on_click_action = "PlayGame") este
    /// método público sin parámetros. Pasa por la pantalla de carga del
    /// nivel 1, que es la que acaba cargando la escena del nivel.
    /// </summary>
    public class Plat3DMenuButtons : GlassBehaviour
    {
        public void PlayGame()
        {
            AudioSource.Play2D("plat3d_click", 0.8f, 1.0f, false);
            SceneManager.LoadScene("Plat3D_Loading_L1");
        }
    }
}
