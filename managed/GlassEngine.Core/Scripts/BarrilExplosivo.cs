using System;
using System.Collections.Generic;
using UnityEngine;
using GlassEngine.Core.Audio;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts
{
    /// <summary>Barril explosivo: acumula daño, detona con impulso radial y daño en cadena.</summary>
    public class BarrilExplosivo : GlassBehaviour
    {
        // Configuración que rellena la sesión.
        public ulong EmisorFuegoId;
        public ulong EmisorHumoId;
        public float RadioImpulso = 6f;
        public float FuerzaImpulso = 9f;
        public float Vida = 20f;

        /// <summary>Vecinos a los que empujar (cajas y barriles cercanos); los rellena la sesión.</summary>
        public List<ulong> VecinosImpulso = new List<ulong>();

        /// <summary>Posiciones conocidas de los vecinos (la sesión las aporta; sin posición no hay impulso radial).</summary>
        public static readonly Dictionary<ulong, Vector3> PosicionesVecinos = new Dictionary<ulong, Vector3>();

        /// <summary>Barriles vivos (id) para el daño en cadena; los marca la sesión/el propio barril.</summary>
        public static readonly HashSet<ulong> BarrilesVivos = new HashSet<ulong>();

        /// <summary>Instancias vivas (la sesión las cablea en su Start).</summary>
        public static readonly List<BarrilExplosivo> Barriles = new List<BarrilExplosivo>();

        private bool _detonado;
        private bool _emisoresApagados;
        private bool _autoApagado;
        private float _t;

        public override void Start()
        {
            BarrilesVivos.Add(EntityId);
            if (!Barriles.Contains(this)) Barriles.Add(this);
            RegistroImpactos.AlImpactar += OnImpacto;
        }

        public override void OnDestroy()
        {
            RegistroImpactos.AlImpactar -= OnImpacto;
            BarrilesVivos.Remove(EntityId);
            Barriles.Remove(this);
        }

        public override void Update(float dt)
        {
            if (!_detonado) return;

            _t += dt;

            // Apagado de emisores a los 1,2 s.
            if (!_emisoresApagados && _t >= 1.2f)
            {
                _emisoresApagados = true;
                if (EmisorFuegoId != 0) RuntimeBridge.SetEntityActive(EmisorFuegoId, false);
                if (EmisorHumoId != 0) RuntimeBridge.SetEntityActive(EmisorHumoId, false);
            }

            // Autodesactivación a los 0,1 s.
            if (!_autoApagado && _t >= 0.1f)
            {
                _autoApagado = true;
                RuntimeBridge.SetEntityActive(EntityId, false);
            }
        }

        private void OnImpacto(ulong entidad, float dano, Vector3 punto)
        {
            if (entidad != EntityId) return;
            if (_detonado) return;

            Vida -= dano;
            if (Vida > 0f)
            {
                Debug.Log($"[F10] BARRIL golpeado vida={Vida:F2}");
                return;
            }

            Detonar();
        }

        private void Detonar()
        {
            _detonado = true;
            _t = 0f;
            BarrilesVivos.Remove(EntityId);

            Debug.Log($"[F10] BARRIL detona id={EntityId}");

            if (EmisorFuegoId != 0) RuntimeBridge.SetEntityActive(EmisorFuegoId, true);
            if (EmisorHumoId != 0) RuntimeBridge.SetEntityActive(EmisorHumoId, true);

            // Impulso radial sobre los vecinos con cuerpo propio en la escena.
            for (int i = 0; i < VecinosImpulso.Count; i++)
            {
                ulong id = VecinosImpulso[i];
                if (id == EntityId) continue;

                var handle = RuntimeBridge.GetBodyHandle(id);
                if (handle <= 0) continue;

                Vector3 posV;
                if (!PosicionesVecinos.TryGetValue(id, out posV)) continue;

                float dx = posV.X - Position.X;
                float dy = posV.Y - Position.Y;
                float dz = posV.Z - Position.Z;
                float dist = (float)Math.Sqrt(dx * dx + dy * dy + dz * dz);
                if (dist > RadioImpulso) continue;
                if (dist < 0.0001f) dist = 0.0001f;

                Vector3 dir = new Vector3(dx / dist, dy / dist, dz / dist);
                float f = FuerzaImpulso * (1f - dist / RadioImpulso);
                Vector3 impulso = new Vector3(
                    dir.X * f,
                    dir.Y * f + 0.35f * FuerzaImpulso,
                    dir.Z * f);
                RuntimeBridge.ApplyImpulse(handle, impulso);
            }

            // Daño en cadena solo a otros barriles.
            for (int i = 0; i < VecinosImpulso.Count; i++)
            {
                ulong id = VecinosImpulso[i];
                if (id == EntityId) continue;
                if (!BarrilesVivos.Contains(id)) continue;

                Vector3 posV;
                if (!PosicionesVecinos.TryGetValue(id, out posV)) posV = Position;
                RegistroImpactos.Registrar(id, 25f, posV);
            }

            var audio = new GlassEngine.Core.Audio.AudioSource
            {
                EntityId = EntityId,
                ClipName = "beep_880",
                Volume = 0.8f,
                Pitch = 1f,
                Looping = false,
                Spatial3D = true,
                Bus = "Sfx"
            };
            audio.Play(Position);
        }
    }
}
