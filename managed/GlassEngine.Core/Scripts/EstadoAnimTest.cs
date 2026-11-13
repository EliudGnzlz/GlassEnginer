using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Guión de la escena de prueba de StateMachineBehaviours
    /// (PruebaEstadosAnim.scene.json). Ejercita la FSM del X Bot como
    /// AnimFsmTest (walk a ~0,8 s, vuelta a ~2,0 s, CrossFade a Walk a
    /// ~3,2 s y Play a Idle a ~4,4 s) y anota en el log cada
    /// OnStateEnter/OnStateExit que el motor le entrega.
    /// </summary>
    public class EstadoAnimTest : GlassBehaviour
    {
        private float _elapsed;
        private int _phase;
        private Animator? _animator;

        public override void Start()
        {
            var go = GameObject.GetOrCreate(EntityId, "X Bot FSM");
            _animator = go.GetComponent<Animator>() ?? go.AddComponent<Animator>();
            Debug.Log($"[AnimEstados] Script activo en la entidad {EntityId}; estado inicial: '{_animator?.GetCurrentState()}'.");
        }

        public override void OnStateEnter(string state)
        {
            Debug.Log($"[AnimEstados] ent={EntityId} ENTER {state}");
        }

        public override void OnStateExit(string state)
        {
            Debug.Log($"[AnimEstados] ent={EntityId} EXIT {state}");
        }

        public override void Update(float deltaTime)
        {
            _elapsed += deltaTime;
            if (_phase == 0 && _elapsed >= 0.8f)
            {
                _phase = 1;
                _animator?.SetBool("walk", true);
            }
            else if (_phase == 1 && _elapsed >= 2.0f)
            {
                _phase = 2;
                _animator?.SetBool("walk", false);
            }
            else if (_phase == 2 && _elapsed >= 3.2f)
            {
                _phase = 3;
                _animator?.CrossFade("Walk", 0.5f);
            }
            else if (_phase == 3 && _elapsed >= 4.4f)
            {
                _phase = 4;
                _animator?.Play("Idle");
            }
        }
    }
}
