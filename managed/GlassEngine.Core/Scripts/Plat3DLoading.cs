using GlassEngine.Core;
using UnityEngine.SceneManagement;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Control de la pantalla de carga (escenas Plat3D_Loading_L1/L2/L3).
    /// La carga de escenas del motor es SÍNCRONA y rápida: no hay pasos
    /// asíncronos que seguir con una barra. Lo que hace esta pantalla es
    /// un intersticial honesto: la escena de carga se renderiza de verdad
    /// (Canvas «Cargando...» + nombre del nivel destino, autorados en el
    /// JSON de cada escena de carga, y un cubo girando) durante un mínimo
    /// de 1,1 s de frames reales, y SOLO entonces se ejecuta la carga
    /// síncrona del nivel destino, derivado del nombre de la escena de
    /// carga activa (Plat3D_Loading_L2 → Plat3D_L2).
    /// </summary>
    public class Plat3DLoading : GlassBehaviour
    {
        private float _elapsed;
        private bool _launched;

        private const float MinVisibleSeconds = 1.1f;

        public override void Update(float deltaTime)
        {
            if (_launched)
            {
                return;
            }
            _elapsed += deltaTime;
            if (_elapsed < MinVisibleSeconds)
            {
                return;
            }
            _launched = true;
            string active = SceneManager.GetActiveSceneName();
            string target = active == "Plat3D_Loading_L2"
                ? "Plat3D_L2"
                : active == "Plat3D_Loading_L3"
                    ? "Plat3D_L3"
                    : "Plat3D_L1";
            SceneManager.LoadScene(target);
        }
    }
}
