using System;
using GlassEngine.Core.Physics;
using UnityEngine;
using Vector3 = GlassEngine.Core.Interop.Vector3;
using Quaternion = GlassEngine.Core.Interop.Quaternion;

namespace GlassEngine.Core.Scripts
{
    public class JugadorPersonaje : GlassBehaviour
    {
        public static JugadorPersonaje Instancia { get; private set; }

        /// <summary>FOV vertical vigente de la cámara (lo mantiene AplicarFov;
        /// lo lee NumerosDanoDianas para proyectar los números de daño).</summary>
        public static float FovActual { get; private set; } = FovNormal;

        private const float FovNormal = 62f;
        private const float Sensibilidad = 0.15f;
        private const float PitchMax = 60f;
        private const float VelAndar = 2.2f;
        private const float VelCorrer = 4.5f;
        private const float VelAgachado = 1.2f;
        private const float DistOrigen = 0.6f;

        private CharacterController _cc;
        private Animator _anim;

        private float _yaw;
        private float _pitch;
        private float _vy;
        private float _bob;

        private int _armaIdx;
        private int[] _municion = Array.Empty<int>();
        private float _tiempoDesdeDisparo = float.MaxValue;
        private bool _mira;
        private bool _tercera;
        private readonly Random _rng = new Random();

        private bool _ratonInicializado;
        private float _ratonX, _ratonY;
        private bool _teclaRPrev;
        private bool _botonDerPrev;
        private bool _vPrev;

        private Vector3 _camPos;

        public string ArmaActualNombre => ArmaActual?.Nombre ?? "";
        public int MunicionEnCargador => (_armaIdx >= 0 && _armaIdx < _municion.Length) ? _municion[_armaIdx] : 0;
        public int CargadorActual => ArmaActual?.Cargador ?? 0;
        public int DisparosRealizados { get; private set; }

        /// <summary>Origen de disparo actual (posición de la cámara).</summary>
        public Vector3 OrigenDisparo => _camPos;

        public Vector3 DireccionActual
        {
            get
            {
                float y = _yaw * MathF.PI / 180f;
                float p = _pitch * MathF.PI / 180f;
                return new Vector3
                {
                    X = -MathF.Sin(y) * MathF.Cos(p),
                    Y = MathF.Sin(p),
                    Z = -MathF.Cos(y) * MathF.Cos(p)
                };
            }
        }

        private ArmaDef ArmaActual
        {
            get
            {
                var armas = SistemaArmas.Armas;
                if (armas == null || _armaIdx < 0 || _armaIdx >= armas.Count) return null;
                return armas[_armaIdx];
            }
        }

        public override void Start()
        {
            Instancia = this;

            _cc = new CharacterController
            {
                Radius = 0.35f,
                Height = 1.8f,
                SkinWidth = 0.02f,
                StepOffset = 0.4f,
                SlopeLimit = 45f,
                SnapDistance = 0.3f,
            };
            _cc.Initialize(EntityId, new Vector3(0f, 0.92f, 6f));

            var goJug = GameObject.GetOrCreate(EntityId, "JugadorPersonaje");
            _anim = goJug.GetComponent<Animator>() ?? goJug.AddComponent<Animator>();

            _yaw = 0f;
            _pitch = -2f;

            SistemaArmas.Cargar();
            var armas = SistemaArmas.Armas;
            _municion = new int[armas.Count];
            for (int i = 0; i < armas.Count; i++)
                _municion[i] = armas[i].Cargador;

            Equipar(0);
            ActualizarCamara();
            Debug.Log("[DEMO] JUGADOR listo");
        }

        public override void Update(float deltaTime)
        {
            if (_tiempoDesdeDisparo < float.MaxValue - deltaTime)
                _tiempoDesdeDisparo += deltaTime;

            ProcesarRaton();
            ProcesarArmas();
            ProcesarMovimiento(deltaTime);
            ActualizarCamara();
        }

        public override void OnDestroy()
        {
            if (_mira)
            {
                RuntimeBridge.SetCameraFov(FovNormal);
                _mira = false;
            }
            if (Instancia == this) Instancia = null;
        }

        public override void OnAnimationEvent(string nombre, float? f, string? s)
        {
            if (nombre == "Paso") _bob += 1f;
        }

        // --- API de sesión ---

        public void Equipar(int indice)
        {
            var armas = SistemaArmas.Armas;
            if (armas == null || armas.Count == 0) return;
            if (indice < 0 || indice >= armas.Count) return;

            _armaIdx = indice;
            _tiempoDesdeDisparo = float.MaxValue;

            if (_mira) AplicarFov(armas[indice].ZoomFov);
        }

        public bool DispararUnaVez()
        {
            ArmaDef arma = ArmaActual;
            if (arma == null) return false;
            if (_tiempoDesdeDisparo < arma.CadenciaSeg) return false;
            if (_municion[_armaIdx] <= 0) return false;

            Vector3 dir = DireccionActual;
            Vector3 origen = _camPos;

            int efectivos = SistemaArmas.Disparar(arma, origen, dir, _rng);

            _municion[_armaIdx]--;
            _tiempoDesdeDisparo = 0f;
            DisparosRealizados++;

            Debug.Log($"[DEMO] DISPARO arma={arma.Nombre} efectivos={efectivos} municion={_municion[_armaIdx]}");
            return true;
        }

        public void Mira(bool activa)
        {
            _mira = activa;
            ArmaDef arma = ArmaActual;
            float fov = (activa && arma != null) ? arma.ZoomFov : FovNormal;
            AplicarFov(fov);
        }

        public void Recargar()
        {
            ArmaDef arma = ArmaActual;
            if (arma == null) return;
            _municion[_armaIdx] = arma.Cargador;
        }

        // --- internos ---

        private void AplicarFov(float fov)
        {
            FovActual = fov;
            RuntimeBridge.SetCameraFov(fov);
        }

        private void ProcesarRaton()
        {
            var m = GlassEngine.Core.Input.mousePosition;
            float mx = m.x, my = m.y;
            if (!_ratonInicializado)
            {
                _ratonX = mx; _ratonY = my;
                _ratonInicializado = true;
            }
            float ddx = mx - _ratonX, ddy = my - _ratonY;
            _ratonX = mx; _ratonY = my;

            if (ddx != 0f || ddy != 0f)
            {
                _yaw -= ddx * Sensibilidad;
                _pitch = Math.Clamp(_pitch - ddy * Sensibilidad, -PitchMax, PitchMax);
                if (_yaw > 360f || _yaw < -360f) _yaw %= 360f;
            }
        }

        private void ProcesarArmas()
        {
            if (GlassEngine.Core.Input.GetMouseButtonDown(0))
                DispararUnaVez();

            bool der = GlassEngine.Core.Input.GetMouseButton(1);
            if (der != _botonDerPrev)
            {
                Mira(der);
                _botonDerPrev = der;
            }

            if (GlassEngine.Core.Input.GetKey("1")) EquiparSiDistinta(0);
            else if (GlassEngine.Core.Input.GetKey("2")) EquiparSiDistinta(1);
            else if (GlassEngine.Core.Input.GetKey("3")) EquiparSiDistinta(2);
            else if (GlassEngine.Core.Input.GetKey("4")) EquiparSiDistinta(3);

            bool r = GlassEngine.Core.Input.GetKey("R");
            if (r && !_teclaRPrev) Recargar();
            _teclaRPrev = r;

            bool v = GlassEngine.Core.Input.GetKey("V");
            if (v && !_vPrev) _tercera = !_tercera;
            _vPrev = v;
        }

        private void EquiparSiDistinta(int indice)
        {
            if (indice != _armaIdx) Equipar(indice);
        }

        private void ProcesarMovimiento(float deltaTime)
        {
            float h = GlassEngine.Core.Input.GetAxisHorizontal();
            float v = GlassEngine.Core.Input.GetAxisVertical();

            float yr = _yaw * MathF.PI / 180f;
            float fx = -MathF.Sin(yr), fz = -MathF.Cos(yr);
            float rx = MathF.Cos(yr), rz = -MathF.Sin(yr);

            bool corriendo = GlassEngine.Core.Input.GetKey("Shift");
            bool agachado = GlassEngine.Core.Input.GetKey("Ctrl") || GlassEngine.Core.Input.GetKey("C");

            float velocidad = agachado ? VelAgachado : (corriendo ? VelCorrer : VelAndar);

            _vy -= 22f * deltaTime;
            if (_cc.IsGrounded && _vy < -1f) _vy = -1f;
            if (GlassEngine.Core.Input.GetKey("Space") && _cc.IsGrounded)
            {
                _vy = 7.5f;
                _anim.Play("Salto");
            }

            float mvx = (rx * h + fx * v) * velocidad * deltaTime;
            float mvz = (rz * h + fz * v) * velocidad * deltaTime;
            float mvy = _vy * deltaTime;

            Vector3 motion = new Vector3 { X = mvx, Y = mvy, Z = mvz };
            _cc.Move(motion, deltaTime);

            // El .glb del X Bot tiene el origen en los pies: la
            // entidad va media altura por debajo del centro de la
            // cápsula que devuelve el CharacterController.
            var centro = _cc.Position;
            Position = new Vector3(centro.X, centro.Y - 0.9f, centro.Z);

            float hy = _yaw * MathF.PI / 360f;
            Rotation = new Quaternion(0f, MathF.Sin(hy), 0f, MathF.Cos(hy));

            _anim.SetFloat("MoveX", h * (corriendo ? 1f : 1f));
            _anim.SetFloat("MoveZ", v * (corriendo ? 2f : 1f));
            _anim.SetBool("mover", MathF.Abs(h) + MathF.Abs(v) > 0.1f);
            _anim.SetBool("agachado", agachado);
        }

        private void ActualizarCamara()
        {
            Vector3 fwd3 = DireccionActual;
            Vector3 pos = _cc.Position;

            Vector3 ojo;
            if (!_tercera)
            {
                ojo = new Vector3
                {
                    X = pos.X + fwd3.X * 0.25f,
                    Y = pos.Y + 0.65f + fwd3.Y * 0.25f,
                    Z = pos.Z + fwd3.Z * 0.25f
                };

                bool moviendo = MathF.Abs(GlassEngine.Core.Input.GetAxisHorizontal()) + MathF.Abs(GlassEngine.Core.Input.GetAxisVertical()) > 0.1f;
                if (moviendo && _cc.IsGrounded)
                    ojo.Y += MathF.Sin(_bob * MathF.PI) * 0.025f;
            }
            else
            {
                Vector3 ancla = new Vector3 { X = pos.X, Y = pos.Y + 0.7f, Z = pos.Z };
                ojo = new Vector3
                {
                    X = ancla.X - fwd3.X * 4.0f,
                    Y = ancla.Y - fwd3.Y * 4.0f,
                    Z = ancla.Z - fwd3.Z * 4.0f
                };
            }

            _camPos = ojo;

            float hy = _yaw * MathF.PI / 360f;
            float hp = _pitch * MathF.PI / 360f;
            float cy = MathF.Cos(hy), sy = MathF.Sin(hy);
            float cp = MathF.Cos(hp), sp = MathF.Sin(hp);

            var t = new GlassEngine.Core.Interop.TransformData
            {
                Position = ojo,
                Rotation = new Quaternion(cy * sp, sy * cp, -sy * sp, cy * cp),
                Scale = new Vector3(1f, 1f, 1f)
            };
            RuntimeBridge.SetTransform(1, ref t);
        }
    }
}
