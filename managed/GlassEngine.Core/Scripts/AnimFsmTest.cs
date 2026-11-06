using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Guión de la escena de prueba de la máquina de estados de animación
    /// (AnimFsmTest.scene.json). El X Bot arranca en «Idle» (controlador
    /// `xbot`); el guión ejercita la API nueva con marcas en el log:
    /// a los ~0,8 s pone el bool `walk` (transición Idle→Walk con
    /// fundido), a los ~2,0 s lo quita (Walk→Idle), a los ~3,2 s pide un
    /// CrossFade directo a «Walk» y a los ~4,4 s un Play a «Idle»
    /// (salto de estado). Tras cada paso anota `GetCurrentState()`.
    /// </summary>
    public class AnimFsmTest : GlassBehaviour
    {
        private float _elapsed;
        private int _phase;
        private Animator? _animator;

        public override void Start()
        {
            var go = GameObject.GetOrCreate(EntityId, "X Bot FSM");
            _animator = go.GetComponent<Animator>() ?? go.AddComponent<Animator>();
            Debug.Log($"[AnimFsmTest] Script activo en la entidad {EntityId}; estado inicial: '{_animator?.GetCurrentState()}'.");
        }

        public override void Update(float deltaTime)
        {
            _elapsed += deltaTime;
            if (_phase == 0 && _elapsed >= 0.8f)
            {
                _phase = 1;
                _animator?.SetBool("walk", true);
                Debug.Log($"[AnimFsmTest] SetBool(walk, true); estado: '{_animator?.GetCurrentState()}'.");
            }
            else if (_phase == 1 && _elapsed >= 2.0f)
            {
                _phase = 2;
                _animator?.SetBool("walk", false);
                Debug.Log($"[AnimFsmTest] SetBool(walk, false); estado: '{_animator?.GetCurrentState()}'.");
            }
            else if (_phase == 2 && _elapsed >= 3.2f)
            {
                _phase = 3;
                _animator?.CrossFade("Walk", 0.5f);
                Debug.Log($"[AnimFsmTest] CrossFade(Walk, 0.5); estado: '{_animator?.GetCurrentState()}'.");
            }
            else if (_phase == 3 && _elapsed >= 4.4f)
            {
                _phase = 4;
                _animator?.Play("Idle");
                Debug.Log($"[AnimFsmTest] Play(Idle); estado: '{_animator?.GetCurrentState()}'.");
            }
        }
    }
}
