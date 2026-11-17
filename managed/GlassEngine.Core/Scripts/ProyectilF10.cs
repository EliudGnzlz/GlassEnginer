using System;
using System.Collections.Generic;
using GlassEngine.Core.Interop;
using GlassEngine.Core.Physics;
using Vector3 = GlassEngine.Core.Interop.Vector3;
using Quaternion = GlassEngine.Core.Interop.Quaternion;

namespace GlassEngine.Core.Scripts
{
    /// <summary>Proyectil con pool estático; crea su propio cuerpo dinámico al arrancar.</summary>
    public class ProyectilF10 : GlassBehaviour
    {
        private static readonly List<ProyectilF10> Pool = new List<ProyectilF10>();

        private RigidBody _body;
        private Vector3 _ancla;
        private Vector3 _escalaAutorada;
        private bool _activo;
        private Vector3 _direccion;
        private float _velocidad;
        private float _alcance;
        private float _dano;
        private float _recorrida;

        public override void Start()
        {
            _ancla = Position;
            _escalaAutorada = Scale;

            _body = new RigidBody();
            _body.Initialize(EntityId, RigidBodyType.Dynamic, _ancla, Quaternion.Identity);
            _body.AttachSphereCollider(0.06f, 0.1f, 0.2f);
            _body.CollisionDetection = CollisionDetectionMode.Continuous;
            _body.SetTransform(_ancla, Quaternion.Identity);
            _body.SetLinearVelocity(new Vector3(0f, 0f, 0f));

            Scale = new Vector3(0.001f, 0.001f, 0.001f);
            _activo = false;

            if (!Pool.Contains(this)) Pool.Add(this);
        }

        public override void OnDestroy()
        {
            Pool.Remove(this);
        }

        /// <summary>Toma el primer proyectil libre del pool y lo lanza.</summary>
        public static bool DispararDesdePool(Vector3 origen, Vector3 direccion, float velocidad, float alcance, float dano)
        {
            for (int i = 0; i < Pool.Count; i++)
            {
                var p = Pool[i];
                if (p == null || p._activo) continue;
                p.Lanzar(origen, direccion, velocidad, alcance, dano);
                return true;
            }
            return false;
        }

        private void Lanzar(Vector3 origen, Vector3 direccion, float velocidad, float alcance, float dano)
        {
            float l = (float)Math.Sqrt(direccion.X * direccion.X + direccion.Y * direccion.Y + direccion.Z * direccion.Z);
            if (l < 0.0001f) return;

            _direccion = new Vector3(direccion.X / l, direccion.Y / l, direccion.Z / l);
            _velocidad = velocidad;
            _alcance = alcance;
            _dano = dano;
            _recorrida = 0f;
            _activo = true;

            Scale = _escalaAutorada;
            if (_body != null)
            {
                _body.SetTransform(origen, Quaternion.Identity);
                _body.SetLinearVelocity(new Vector3(_direccion.X * _velocidad, _direccion.Y * _velocidad, _direccion.Z * _velocidad));
            }
        }

        public override void Update(float dt)
        {
            if (!_activo)
            {
                // Inactivo: anclado en su posición de autoría, escala mínima y sin velocidad.
                Scale = new Vector3(0.001f, 0.001f, 0.001f);
                if (_body != null)
                {
                    _body.SetTransform(_ancla, Quaternion.Identity);
                    _body.SetLinearVelocity(new Vector3(0f, 0f, 0f));
                }
                return;
            }

            if (_body != null)
            {
                Vector3 v;
                _body.GetLinearVelocity(out v);
                float sp = (float)Math.Sqrt(v.X * v.X + v.Y * v.Y + v.Z * v.Z);
                _recorrida += sp * dt;
            }

            if (_recorrida > _alcance) Retirar();
        }

        public override void OnCollisionEnter(Collision c)
        {
            if (!_activo) return;
            if (_body == null) return;

            ulong otro = c.gameObject.EntityId;
            Vector3 punto = Position;
            if (c.point.x != 0f || c.point.y != 0f || c.point.z != 0f)
                punto = new Vector3(c.point.x, c.point.y, c.point.z);
            RegistroImpactos.Registrar(otro, _dano, punto);
            Retirar();
        }

        private void Retirar()
        {
            _activo = false;
            _recorrida = 0f;
            Scale = new Vector3(0.001f, 0.001f, 0.001f);

            if (_body != null)
            {
                _body.SetLinearVelocity(new Vector3(0f, 0f, 0f));
                _body.SetTransform(_ancla, Quaternion.Identity);
            }
        }
    }
}
