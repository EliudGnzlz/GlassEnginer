using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Sonda mínima de animación (escena MicroAnim): obtiene el
    /// Animator de una entidad de escena y alimenta sus parámetros.
    ///
    /// HISTORIA (2026-11-14): esta sonda es la repro del segfault que
    /// bloqueó la demo FPS. `GetComponent&lt;Animator&gt;()` devuelve
    /// null para componentes que existen en la escena pero nunca se
    /// añadieron desde C#; desreferenciar ese null en este host mata
    /// el proceso con SIGSEGV sin pasar por los try/catch. El patrón
    /// obligatorio es el de abajo: `?? AddComponent&lt;T&gt;()`.
    /// </summary>
    public class MicroAnimTest : GlassBehaviour
    {
        private Animator _anim;
        private float _t;

        public override void Start()
        {
            var go = GameObject.GetOrCreate(EntityId, "MicroAnimTest");
            _anim = go.GetComponent<Animator>() ?? go.AddComponent<Animator>();
            Debug.Log("[MICRO] listo (Animator obtenido con fallback)");
        }

        public override void Update(float deltaTime)
        {
            _t += deltaTime;
            // Ejercicio continuo de los parámetros del controlador.
            _anim.SetBool("mover", _t % 4f >= 2f);
            _anim.SetFloat("MoveZ", _t % 4f >= 2f ? 1f : 0f);
        }
    }
}
