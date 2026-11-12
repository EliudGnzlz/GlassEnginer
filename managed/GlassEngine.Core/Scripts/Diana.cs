using System;
using System.Globalization;
using UnityEngine;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts
{
    public static class RegistroImpactos
    {
        public static event Action<ulong, float, Vector3> AlImpactar;

        public static void Registrar(ulong entidad, float dano, Vector3 punto)
        {
            AlImpactar?.Invoke(entidad, dano, punto);
        }
    }

    public class Diana : GlassBehaviour
    {
        /// <summary>Registro de dianas vivas (la sesión las configura
        /// por cercanía a los centros autorados).</summary>
        public static readonly System.Collections.Generic.List<Diana> Dianas = new();

        private float _distancia;
        private Vector3 _centro;
        private Vector3 _puntoImpacto;

        public int Impactos { get; private set; }
        public float Distancia => _distancia;

        public void Configurar(float distancia, Vector3 centro)
        {
            _distancia = distancia;
            _centro = centro;
        }

        public override void Start()
        {
            if (!Dianas.Contains(this)) Dianas.Add(this);
            RegistroImpactos.AlImpactar += AlRecibirImpacto;
        }

        public override void OnDestroy()
        {
            Dianas.Remove(this);
            RegistroImpactos.AlImpactar -= AlRecibirImpacto;
        }

        private void AlRecibirImpacto(ulong entidad, float dano, Vector3 punto)
        {
            if (entidad != EntityId)
                return;

            Impactos++;
            _puntoImpacto = punto;

            var handle = RuntimeBridge.GetBodyHandle(EntityId);
            if (handle > 0)
            {
                RuntimeBridge.ApplyImpulse(handle, new Vector3(0f, 0.6f, 0.4f));
            }

            Debug.Log(FormattableString.Invariant(
                $"[F10] DIANA d={_distancia:F2} impactos={Impactos} punto=({_puntoImpacto.X:F2},{_puntoImpacto.Y:F2},{_puntoImpacto.Z:F2}) centro_y={_centro.Y:F2} caida={(_centro.Y - _puntoImpacto.Y):F2}"));
        }
    }
}
