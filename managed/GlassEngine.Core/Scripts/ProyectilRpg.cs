using System;
using System.Collections.Generic;
using GlassEngine.Core.Physics;
using Vector3 = GlassEngine.Core.Interop.Vector3;
using UnityEngine;

namespace GlassEngine.Core.Scripts
{
    /// <summary>
    /// Proyectil de RPG con pool estatico, vuelo con fisica dinamica y explosion con impulso radial.
    /// </summary>
    public class ProyectilRpg : GlassBehaviour
    {
        private static readonly List<ProyectilRpg> pool = new List<ProyectilRpg>();

        public static ulong EmisorFogonazoId;
        public static ulong EmisorOndaId;
        public static ulong EmisorHumoId;
        public static ulong LuzFlashId;

        private RigidBody cuerpo;
        private Vector3 posicionAutoria;
        private Vector3 escalaAutorada;
        private bool enVuelo;

        private float alcance;
        private float dano;
        private float radioExplosion;
        private float distanciaRecorrida;

        // temporizadores de apagado de efectos tras explosion
        private bool explotando;
        private float temporizadorExplosion;

        public override void Start()
        {
            posicionAutoria = Position;
            escalaAutorada = Scale;
            cuerpo = new RigidBody();
            cuerpo.Initialize(EntityId, RigidBodyType.Dynamic, Position, Rotation);
            cuerpo.AttachSphereCollider(0.16f, 0.1f, 0.2f);
            cuerpo.CollisionDetection = CollisionDetectionMode.Continuous;

            // inactivo anclado, escala minima, sin velocidad
            Scale = new Vector3(0.001f, 0.001f, 0.001f);
            cuerpo.SetTransform(posicionAutoria, Rotation);
            cuerpo.SetLinearVelocity(Vector3.Zero);
            enVuelo = false;
            explotando = false;

            pool.Add(this);
        }

        public override void Update(float deltaTime)
        {
            if (explotando)
            {
                temporizadorExplosion += deltaTime;

                if (temporizadorExplosion >= 0.25f)
                {
                    RuntimeBridge.SetEntityActive(EmisorFogonazoId, false);
                }
                if (temporizadorExplosion >= 0.5f)
                {
                    RuntimeBridge.LightSetIntensity(LuzFlashId, 0f);
                }
                if (temporizadorExplosion >= 0.6f)
                {
                    RuntimeBridge.SetEntityActive(EmisorOndaId, false);
                }
                if (temporizadorExplosion >= 1.6f)
                {
                    RuntimeBridge.SetEntityActive(EmisorHumoId, false);
                    explotando = false;
                }
                return;
            }

            if (!enVuelo)
            {
                // inactivo, anclado en su sitio de autoria
                cuerpo.SetTransform(posicionAutoria, Rotation);
                cuerpo.SetLinearVelocity(Vector3.Zero);
                return;
            }

            cuerpo.GetLinearVelocity(out Vector3 velocidad);
            float desplazamiento = (float)Math.Sqrt(
                velocidad.X * velocidad.X + velocidad.Y * velocidad.Y + velocidad.Z * velocidad.Z) * deltaTime;
            distanciaRecorrida += desplazamiento;

            if (distanciaRecorrida >= alcance)
            {
                Explotar(Position);
            }
        }

        public override void OnCollisionEnter(Collision c)
        {
            if (enVuelo)
            {
                Explotar(Position);
            }
        }

        public static bool DispararDesdePool(Vector3 origen, Vector3 direccion, float velocidad, float alcance, float dano, float radio)
        {
            foreach (var p in pool)
            {
                if (!p.enVuelo && !p.explotando)
                {
                    p.Scale = p.escalaAutorada;
                    p.cuerpo.SetTransform(origen, p.Rotation);
                    p.cuerpo.SetLinearVelocity(direccion * velocidad);

                    p.alcance = alcance;
                    p.dano = dano;
                    p.radioExplosion = radio;
                    p.distanciaRecorrida = 0f;
                    p.enVuelo = true;

                    return true;
                }
            }

            Debug.LogWarning("[RPG] Pool de proyectiles agotado");
            return false;
        }

        private void Explotar(Vector3 punto)
        {
            enVuelo = false;

            ulong[] afectados = GlassEngine.Core.Physics.Physics.OverlapSphere(punto, radioExplosion);

            Debug.Log($"[RPG] EXPLOSION punto=({punto.X},{punto.Y},{punto.Z}) radio={radioExplosion} afectados={afectados.Length}");

            foreach (var id in afectados)
            {
                if (id == EntityId) continue;

                UnityEngine.Vector3 posObjetivoUnity = GameObject.GetOrCreate(id, "objetivo").transform.position;
                Vector3 posObjetivo = new Vector3(posObjetivoUnity.x, posObjetivoUnity.y, posObjetivoUnity.z);

                Vector3 direccion = new Vector3(
                    posObjetivo.X - punto.X, posObjetivo.Y - punto.Y, posObjetivo.Z - punto.Z);
                float dist = (float)Math.Sqrt(
                    direccion.X * direccion.X + direccion.Y * direccion.Y + direccion.Z * direccion.Z);
                Vector3 dirNormalizada = dist > 0.0001f
                    ? new Vector3(direccion.X / dist, direccion.Y / dist, direccion.Z / dist)
                    : Vector3.Zero;

                float falloff = Math.Max(0f, 1f - dist / radioExplosion);
                Vector3 impulso = dirNormalizada * (14f * falloff) + new Vector3(0f, 5f, 0f);

                ulong handle = RuntimeBridge.GetBodyHandle(id);
                if (handle != 0)
                {
                    RuntimeBridge.ApplyImpulse(handle, impulso);
                }

                RegistroImpactos.Registrar(id, dano, punto);
            }

            // teletransporta emisores de efectos
            var fogonazoGo = GameObject.GetOrCreate(EmisorFogonazoId, "fogonazo");
            fogonazoGo.transform.position = new UnityEngine.Vector3(punto.X, punto.Y, punto.Z);

            var ondaGo = GameObject.GetOrCreate(EmisorOndaId, "onda");
            ondaGo.transform.position = new UnityEngine.Vector3(punto.X, punto.Y, punto.Z);

            var humoGo = GameObject.GetOrCreate(EmisorHumoId, "humo");
            humoGo.transform.position = new UnityEngine.Vector3(punto.X, punto.Y + 0.5f, punto.Z);

            var luzGo = GameObject.GetOrCreate(LuzFlashId, "luz");
            luzGo.transform.position = new UnityEngine.Vector3(punto.X, punto.Y + 1f, punto.Z);

            RuntimeBridge.SetEntityActive(EmisorFogonazoId, true);
            RuntimeBridge.SetEntityActive(EmisorOndaId, true);
            RuntimeBridge.SetEntityActive(EmisorHumoId, true);

            RuntimeBridge.LightSetIntensity(LuzFlashId, 3.5f);

            var sonido = new GlassEngine.Core.Audio.AudioSource
            {
                ClipName = "plat3d_projectile_hit",
                Volume = 1f,
                Pitch = 0.5f,
                Looping = false,
                Spatial3D = true,
                Bus = "Sfx"
            };
            sonido.Play(punto);

            // se retira el proyectil al ancla
            Scale = new Vector3(0.001f, 0.001f, 0.001f);
            cuerpo.SetTransform(posicionAutoria, Rotation);
            cuerpo.SetLinearVelocity(Vector3.Zero);

            explotando = true;
            temporizadorExplosion = 0f;
        }
    }
}
