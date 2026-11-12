using System;
using System.Globalization;
using UnityEngine;
using Vector3 = GlassEngine.Core.Interop.Vector3;
using Quaternion = GlassEngine.Core.Interop.Quaternion;

namespace GlassEngine.Core.Scripts
{
    /// <summary>Jugador FPS montado sobre la entidad cámara del puesto (Fase 10).</summary>
    public class JugadorFps : GlassBehaviour
    {
        public static JugadorFps Instancia { get; private set; }

        private const float FovNormal = 62f;
        private const float Sensibilidad = 0.15f;
        private const float PitchMax = 60f;
        private const float VelocidadMov = 2f;
        private const float DistOrigen = 0.6f;
        private const float MinX = -2.5f, MaxX = 2.5f;
        private const float MinZ = 6.5f, MaxZ = 9.5f;

        private float _yaw;
        private float _pitch;

        private int _armaIdx;
        private int[] _municion = Array.Empty<int>();
        private float _tiempoDesdeDisparo = float.MaxValue;
        private bool _mira;
        private readonly Random _rng = new Random();

        // Estado de entrada entre frames.
        private bool _ratonInicializado;
        private float _ratonX, _ratonY;
        private bool _teclaRPrev;
        private bool _botonDerPrev;

        public string ArmaActualNombre => ArmaActual?.Nombre ?? "";
        public int MunicionEnCargador => (_armaIdx >= 0 && _armaIdx < _municion.Length) ? _municion[_armaIdx] : 0;
        public int DisparosRealizados { get; private set; }

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

            _yaw = 0f;
            _pitch = -2f;
            AplicarRotacion();

            SistemaArmas.Cargar();
            var armas = SistemaArmas.Armas;
            _municion = new int[armas.Count];
            for (int i = 0; i < armas.Count; i++)
                _municion[i] = armas[i].Cargador;

            Equipar(0);
            Debug.Log("[F10] JUGADOR listo");
        }

        public override void Update(float deltaTime)
        {
            if (_tiempoDesdeDisparo < float.MaxValue - deltaTime)
                _tiempoDesdeDisparo += deltaTime;

            ProcesarEntradaHumana(deltaTime);
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

        // --- API de sesión ---

        public void Equipar(int indice)
        {
            var armas = SistemaArmas.Armas;
            if (armas == null || armas.Count == 0) return;
            if (indice < 0 || indice >= armas.Count) return;

            _armaIdx = indice;
            _tiempoDesdeDisparo = float.MaxValue;

            // Si la mira estaba activa, se ajusta al zoom de la nueva arma.
            if (_mira) AplicarFov(armas[indice].ZoomFov);
        }

        public void ApuntarA(Vector3 punto)
        {
            Vector3 pos = Position;
            float dx = punto.X - pos.X, dy = punto.Y - pos.Y, dz = punto.Z - pos.Z;
            float l = MathF.Sqrt(dx * dx + dy * dy + dz * dz);
            if (l < 1e-5f) return;
            dx /= l; dy /= l; dz /= l;

            _yaw = MathF.Atan2(-dx, -dz) * 180f / MathF.PI;
            _pitch = MathF.Asin(Math.Clamp(dy, -1f, 1f)) * 180f / MathF.PI;
            AplicarRotacion();
        }

        public bool DispararUnaVez()
        {
            ArmaDef arma = ArmaActual;
            if (arma == null) return false;
            if (_tiempoDesdeDisparo < arma.CadenciaSeg) return false;
            if (_municion[_armaIdx] <= 0) return false;

            Vector3 dir = DireccionActual;
            Vector3 pos = Position;
            Vector3 origen = new Vector3
            {
                X = pos.X + dir.X * DistOrigen,
                Y = pos.Y + dir.Y * DistOrigen,
                Z = pos.Z + dir.Z * DistOrigen
            };

            int efectivos = SistemaArmas.Disparar(arma, origen, dir, _rng);

            // Un cartucho por gatillazo, independientemente de los perdigones.
            _municion[_armaIdx]--;
            _tiempoDesdeDisparo = 0f;
            DisparosRealizados++;

            Debug.Log($"[F10] DISPARO arma={arma.Nombre} efectivos={efectivos} municion={_municion[_armaIdx]}");
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
            RuntimeBridge.SetCameraFov(fov);
            Debug.Log("[F10] MIRA fov=" + fov.ToString(CultureInfo.InvariantCulture));
        }

        private void AplicarRotacion()
        {
            // Quaternión YXZ desde yaw/pitch en grados.
            float hy = _yaw * MathF.PI / 360f;
            float hp = _pitch * MathF.PI / 360f;
            float cy = MathF.Cos(hy), sy = MathF.Sin(hy);
            float cp = MathF.Cos(hp), sp = MathF.Sin(hp);

            Rotation = new Quaternion(cy * sp, sy * cp, -sy * sp, cy * cp);
        }

        private void ProcesarEntradaHumana(float deltaTime)
        {
            // Rotación por delta del ratón.
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
                _pitch = Math.Clamp(_pitch + ddy * Sensibilidad, -PitchMax, PitchMax);
                if (_yaw > 360f || _yaw < -360f) _yaw %= 360f;
                AplicarRotacion();
            }

            // Disparo.
            if (GlassEngine.Core.Input.GetMouseButtonDown(0))
                DispararUnaVez();

            // Mira: solo se notifica en los cambios de estado.
            bool der = GlassEngine.Core.Input.GetMouseButton(1);
            if (der != _botonDerPrev)
            {
                Mira(der);
                _botonDerPrev = der;
            }

            // Selección de arma.
            if (GlassEngine.Core.Input.GetKey("1")) EquiparSiDistinta(0);
            else if (GlassEngine.Core.Input.GetKey("2")) EquiparSiDistinta(1);
            else if (GlassEngine.Core.Input.GetKey("3")) EquiparSiDistinta(2);
            else if (GlassEngine.Core.Input.GetKey("4")) EquiparSiDistinta(3);

            // Recarga por flanco de pulsación.
            bool r = GlassEngine.Core.Input.GetKey("R");
            if (r && !_teclaRPrev) Recargar();
            _teclaRPrev = r;

            // Movimiento en el plano XZ relativo al yaw, limitado al puesto.
            float h = GlassEngine.Core.Input.GetAxisHorizontal();
            float v = GlassEngine.Core.Input.GetAxisVertical();
            if (h != 0f || v != 0f)
            {
                float yr = _yaw * MathF.PI / 180f;
                float fx = -MathF.Sin(yr), fz = -MathF.Cos(yr);
                float rx = MathF.Cos(yr), rz = -MathF.Sin(yr);

                float mvx = fx * v + rx * h;
                float mvz = fz * v + rz * h;
                float l = MathF.Sqrt(mvx * mvx + mvz * mvz);
                if (l > 1f) { mvx /= l; mvz /= l; }

                Vector3 pos = Position;
                pos.X = Math.Clamp(pos.X + mvx * VelocidadMov * deltaTime, MinX, MaxX);
                pos.Z = Math.Clamp(pos.Z + mvz * VelocidadMov * deltaTime, MinZ, MaxZ);
                Position = pos;
            }
        }

        private void EquiparSiDistinta(int indice)
        {
            if (indice != _armaIdx) Equipar(indice);
        }
    }
}
