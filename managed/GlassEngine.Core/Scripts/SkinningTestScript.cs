using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Guión de la escena de prueba del bloque de skinning
    /// (SkinningTest.scene.json). La serpiente arranca sola con el clip por
    /// defecto del componente Animator («Ondular»); este script demuestra
    /// la API C# <c>Animator.Play</c>: a los ~3 s pide «Saludar» y a los
    /// ~6 s vuelve a «Ondular». Cada cambio queda en el log del motor, así
    /// la verificación en vivo puede cruzar capturas y marcas de tiempo.
    /// </summary>
    public class SkinningTestScript : GlassBehaviour
    {
        private float _elapsed;
        private int _phase;
        private Animator? _animator;

        public override void Start()
        {
            var go = GameObject.GetOrCreate(EntityId, "Serpiente");
            _animator = go.GetComponent<Animator>() ?? go.AddComponent<Animator>();
            Debug.Log($"[SkinningTest] Script activo en la entidad {EntityId}; la serpiente ondula sola y saludará a los 3 s.");
        }

        public override void Update(float deltaTime)
        {
            _elapsed += deltaTime;
            if (_phase == 0 && _elapsed >= 3.0f)
            {
                _phase = 1;
                _animator?.Play("Saludar");
                Debug.Log("[SkinningTest] Animator.Play(\"Saludar\") pedido a los ~3 s.");
            }
            else if (_phase == 1 && _elapsed >= 6.0f)
            {
                _phase = 2;
                _animator?.Play("Ondular");
                Debug.Log("[SkinningTest] Animator.Play(\"Ondular\") pedido a los ~6 s.");
            }
        }
    }
}
