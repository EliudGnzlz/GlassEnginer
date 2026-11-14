// Numeros de dano flotantes de la demo FPS: al impactar una
// diana (o al enemigo, ampliación del 2026-11-14), el numero nace
// un poco sobre el punto de impacto, sube y se desvanece.
// Pool de 8 textos UI (entidades 221..228, Canvas de la escena)
// proyectados cada frame desde la cámara 1.
using System;
using System.Globalization;
using UnityEngine;
using Vector3 = GlassEngine.Core.Interop.Vector3;
using Quaternion = GlassEngine.Core.Interop.Quaternion;

namespace GlassEngine.Core.Scripts
{
    public class NumerosDanoDianas : GlassBehaviour
    {
        private const ulong PrimeraIdTexto = 221;
        private const int NumTextos = 8;
        private const float Duracion = 0.9f;
        private const float AlturaInicial = 0.35f;
        private const float SubidaTotal = 0.55f;

        private readonly Text[] _textos = new Text[NumTextos];
        private readonly RectTransform[] _rects = new RectTransform[NumTextos];
        private readonly bool[] _activo = new bool[NumTextos];
        private readonly float[] _t = new float[NumTextos];
        private readonly Vector3[] _basePos = new Vector3[NumTextos];

        public override void Start()
        {
            for (int i = 0; i < NumTextos; i++)
            {
                ulong id = PrimeraIdTexto + (ulong)i;
                var go = GameObject.GetOrCreate(id, "NumeroDano" + i);
                var rect = go.GetComponent<RectTransform>() ?? go.AddComponent<RectTransform>();
                var text = go.GetComponent<Text>() ?? go.AddComponent<Text>();
                text.text = "";
                _rects[i] = rect;
                _textos[i] = text;
                _activo[i] = false;
                _t[i] = 0f;
                _basePos[i] = default;
            }

            RegistroImpactos.AlImpactar += OnImpactar;
        }

        public override void OnDestroy()
        {
            RegistroImpactos.AlImpactar -= OnImpactar;
        }

        private void OnImpactar(ulong entidad, float dano, Vector3 punto)
        {
            bool esDiana = false;
            foreach (var diana in Diana.Dianas)
            {
                if (diana.EntityId == entidad)
                {
                    esDiana = true;
                    break;
                }
            }

            // Ampliación del usuario (2026-11-14 07:04): el enemigo
            // de la demo lleva los mismos números que las dianas.
            bool esEnemigo = EnemigoSimple.Instancia != null
                && EnemigoSimple.Instancia.EntityId == entidad;

            if (!esDiana && !esEnemigo)
            {
                return;
            }

            int hueco = -1;
            for (int i = 0; i < NumTextos; i++)
            {
                if (!_activo[i])
                {
                    hueco = i;
                    break;
                }
            }

            if (hueco == -1)
            {
                float mayorT = -1f;
                for (int i = 0; i < NumTextos; i++)
                {
                    if (_t[i] > mayorT)
                    {
                        mayorT = _t[i];
                        hueco = i;
                    }
                }
            }

            _basePos[hueco] = punto + new Vector3(0f, AlturaInicial, 0f);
            _t[hueco] = 0f;
            _activo[hueco] = true;

            string textoDano = MathF.Floor(dano) == dano
                ? ((int)dano).ToString(CultureInfo.InvariantCulture)
                : dano.ToString(CultureInfo.InvariantCulture);

            _textos[hueco].text = textoDano;
            _textos[hueco].color = new Color(1f, 0.85f, 0.25f, 1f);

            Debug.Log("[NUMDANO] " + textoDano + " entidad=" + entidad);
        }

        public override void Update(float deltaTime)
        {
            for (int i = 0; i < NumTextos; i++)
            {
                if (!_activo[i])
                {
                    continue;
                }

                _t[i] += deltaTime;

                if (_t[i] >= Duracion)
                {
                    _activo[i] = false;
                    _textos[i].text = "";
                    continue;
                }

                float p = _t[i] / Duracion;
                Vector3 posMundo = _basePos[i] + new Vector3(0f, SubidaTotal * p, 0f);

                float alfa = p < 0.55f ? 1f : 1f - (p - 0.55f) / 0.45f;

                if (RuntimeBridge.GetTransform(1, out var cam) != 0)
                {
                    _textos[i].color = new Color(1f, 0.85f, 0.25f, 0f);
                    continue;
                }

                Quaternion q = cam.Rotation;
                float qx = q.X, qy = q.Y, qz = q.Z, qw = q.W;

                var right = new Vector3(
                    1f - 2f * (qy * qy + qz * qz),
                    2f * (qx * qy + qw * qz),
                    2f * (qx * qz - qw * qy));

                var up = new Vector3(
                    2f * (qx * qy - qw * qz),
                    1f - 2f * (qx * qx + qz * qz),
                    2f * (qy * qz + qw * qx));

                var forward = new Vector3(
                    -2f * (qx * qz + qw * qy),
                    -2f * (qy * qz - qw * qx),
                    -(1f - 2f * (qx * qx + qy * qy)));

                var rel = new Vector3(
                    posMundo.X - cam.Position.X,
                    posMundo.Y - cam.Position.Y,
                    posMundo.Z - cam.Position.Z);

                float cx = rel.X * right.X + rel.Y * right.Y + rel.Z * right.Z;
                float cy = rel.X * up.X + rel.Y * up.Y + rel.Z * up.Z;
                float cz = rel.X * forward.X + rel.Y * forward.Y + rel.Z * forward.Z;

                if (cz < 0.3f)
                {
                    _textos[i].color = new Color(1f, 0.85f, 0.25f, 0f);
                    continue;
                }

                float fovY = JugadorPersonaje.FovActual;
                float tanY = MathF.Tan(fovY * MathF.PI / 360f);
                float tanX = tanY * (16f / 9f);

                float ndcX = cx / (cz * tanX);
                float ndcY = cy / (cz * tanY);

                _rects[i].anchoredPosition = new Vector2(ndcX * 960f, ndcY * 540f);
                _textos[i].color = new Color(1f, 0.85f, 0.25f, alfa);
            }
        }
    }
}
