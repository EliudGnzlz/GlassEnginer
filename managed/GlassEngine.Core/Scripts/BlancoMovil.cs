using System;
using UnityEngine;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Blanco móvil de la Fase 10: un disco-diana sobre un raíl
    /// lateral que se desplaza en X empujado por su propio cuerpo
    /// físico (velocidad fijada cada frame hacia el extremo activo;
    /// al llegar, cambia de sentido). Hereda de Diana: registra los
    /// impactos igual que una diana fija, y al ser un cuerpo real
    /// los proyectiles colisionan con él y el raycast lo ve donde
    /// está. La entidad se autora CON bloque de física Dynamic.
    /// </summary>
    public class BlancoMovil : Diana
    {
        /// <summary>Registro de blancos móviles vivos (la sesión
        /// apunta a su posición actual).</summary>
        public static readonly System.Collections.Generic.List<BlancoMovil> Moviles =
            new System.Collections.Generic.List<BlancoMovil>();

        /// <summary>Extremos del raíl en X (la sesión/escena los fija
        /// por posición de autoría: centro ± Amplitud).</summary>
        public float Amplitud = 3f;
        public float Velocidad = 1.6f;

        private float _centroX;
        private int _sentido = 1;
        private ulong _handle;

        public override void Start()
        {
            base.Start();
            Moviles.Add(this);
            _centroX = Position.X;
            _handle = RuntimeBridge.GetBodyHandle(EntityId);
        }

        public override void OnDestroy()
        {
            Moviles.Remove(this);
            base.OnDestroy();
        }

        public override void Update(float deltaTime)
        {
            if (_handle == 0)
            {
                _handle = RuntimeBridge.GetBodyHandle(EntityId);
                if (_handle == 0) return;
            }
            float x = Position.X;
            if (x > _centroX + Amplitud) _sentido = -1;
            else if (x < _centroX - Amplitud) _sentido = 1;
            RuntimeBridge.SetLinearVelocity(_handle,
                new Vector3(_sentido * Velocidad, 0f, 0f));
        }
    }
}
