using GlassEngine.Core;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Audio;
using UnityEngine.SceneManagement;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Meta del nivel: al acercarse el jugador, suena el jingle de meta y,
    /// tras 1,3 s (para que el sonido se oiga antes del stop_all del
    /// cambio de escena), pasa a la pantalla de carga del siguiente
    /// nivel según la escena activa: L1 → carga L2, L2 → carga L3,
    /// L3 → pantalla de victoria (directa).
    /// </summary>
    public class Plat3DGoal : GlassBehaviour
    {
        private bool _triggered;
        private float _timer;
        private string _next = "Plat3D_Victory";

        private const float TriggerDistance = 1.5f;
        private const float LoadDelay = 1.3f;

        public override void Update(float deltaTime)
        {
            if (_triggered)
            {
                _timer -= deltaTime;
                if (_timer <= 0f)
                {
                    SceneManager.LoadScene(_next);
                }
                return;
            }

            if (!Plat3DGameState.PlayerPositionValid)
            {
                return;
            }

            Vector3 pp = Plat3DGameState.PlayerPosition;
            Vector3 mp = Position;
            float dx = pp.X - mp.X;
            float dy = pp.Y - mp.Y;
            float dz = pp.Z - mp.Z;
            if (dx * dx + dy * dy + dz * dz < TriggerDistance * TriggerDistance)
            {
                _triggered = true;
                _timer = LoadDelay;
                string scene = SceneManager.GetActiveSceneName();
                _next = scene == "Plat3D_L1"
                    ? "Plat3D_Loading_L2"
                    : scene == "Plat3D_L2"
                        ? "Plat3D_Loading_L3"
                        : "Plat3D_Victory";
                AudioSource.Play2D("plat3d_goal", 0.9f, 1.0f, false);
            }
        }
    }
}
