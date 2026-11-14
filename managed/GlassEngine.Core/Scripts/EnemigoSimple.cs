using GlassEngine.Core.Interop;
using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    public class EnemigoSimple : GlassBehaviour
    {
        public static EnemigoSimple Instancia;
        public float Vida = 100f;

        private bool _muerto;
        private Animator _anim;
        private bool _suscrito;

        public override void Start()
        {
            Instancia = this;
            var goEne = GameObject.GetOrCreate(EntityId, "EnemigoSimple");
            _anim = goEne.GetComponent<Animator>() ?? goEne.AddComponent<Animator>();

            RegistroImpactos.AlImpactar += AlRecibirImpacto;
            _suscrito = true;

            var t = new TransformData
            {
                Position = new Interop.Vector3(Position.X, Position.Y + 2.2f, Position.Z),
                Scale = new Interop.Vector3(1.2f, 0.09f, 0.09f),
                Rotation = Interop.Quaternion.Identity
            };
            RuntimeBridge.SetTransform(211, ref t);
        }

        public override void OnDestroy()
        {
            if (_suscrito)
            {
                RegistroImpactos.AlImpactar -= AlRecibirImpacto;
                _suscrito = false;
            }
        }

        private void AlRecibirImpacto(ulong entidad, float dano, Interop.Vector3 punto)
        {
            if (entidad != EntityId || _muerto) return;

            Vida -= dano;
            float fraccion = MathF.Max(Vida, 0f) / 100f;

            var t = new TransformData
            {
                Position = new Interop.Vector3(Position.X, Position.Y + 2.2f, Position.Z),
                Scale = new Interop.Vector3(1.2f * fraccion, 0.09f, 0.09f),
                Rotation = Interop.Quaternion.Identity
            };
            RuntimeBridge.SetTransform(211, ref t);

            Debug.Log($"[DEMO] ENEMIGO impacto dano={dano} vida={Vida}");

            if (Vida <= 0f)
            {
                _muerto = true;
                _anim.Play("Muerte");

                var tm = new TransformData
                {
                    Position = new Interop.Vector3(Position.X, Position.Y + 2.2f, Position.Z),
                    Scale = new Interop.Vector3(0.001f, 0.09f, 0.09f),
                    Rotation = Interop.Quaternion.Identity
                };
                RuntimeBridge.SetTransform(211, ref tm);

                Debug.Log("[DEMO] ENEMIGO MUERTO");

                RegistroImpactos.AlImpactar -= AlRecibirImpacto;
                _suscrito = false;
            }
        }
    }
}
