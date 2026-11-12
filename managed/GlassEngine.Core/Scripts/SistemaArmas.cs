using System;
using System.Collections.Generic;
using System.IO;
using System.Text.Json;
using UnityEngine;
using GlassEngine.Core.Physics;
using Vector3 = GlassEngine.Core.Interop.Vector3;

namespace GlassEngine.Core.Scripts
{
    /// <summary>Definición de un arma de la demo FPS (Fase 10).</summary>
    public sealed class ArmaDef
    {
        public string Nombre = "";
        public bool Proyectil;
        public float Velocidad;
        public float DispersionGrados;
        public float Alcance;
        public float Dano;
        public int Perdigones = 1;
        public float CadenciaSeg;
        public float ZoomFov;
        public int Cargador;
        /// <summary>Radio de explosión (0 = sin explosión; el RPG
        /// usa 9). Con radio &gt; 0 el disparo va al pool del RPG.</summary>
        public float RadioExplosion;
    }

    /// <summary>Carga de armas y resolución de disparos (hitscan o proyectil).</summary>
    public static class SistemaArmas
    {
        private const string RutaJson = "assets/data/armas.json";

        public static List<ArmaDef> Armas { get; private set; } = new List<ArmaDef>();

        public static void Cargar()
        {
            try
            {
                string texto = File.ReadAllText(RutaJson);
                var lista = new List<ArmaDef>();

                using (JsonDocument doc = JsonDocument.Parse(texto))
                {
                    JsonElement raiz = doc.RootElement;
                    JsonElement arreglo;

                    // Se admite un arreglo en la raíz o un objeto con la clave "armas".
                    if (raiz.ValueKind == JsonValueKind.Array)
                        arreglo = raiz;
                    else if (raiz.ValueKind == JsonValueKind.Object && raiz.TryGetProperty("armas", out JsonElement a) && a.ValueKind == JsonValueKind.Array)
                        arreglo = a;
                    else
                        throw new InvalidDataException("formato JSON no reconocido");

                    foreach (JsonElement e in arreglo.EnumerateArray())
                    {
                        if (e.ValueKind != JsonValueKind.Object) continue;
                        var def = new ArmaDef
                        {
                            Nombre = LeerTexto(e, "nombre", "Arma"),
                            Proyectil = LeerBool(e, "proyectil", false),
                            Velocidad = LeerFloat(e, "velocidad", 0f),
                            DispersionGrados = LeerFloat(e, "dispersion_grados", 0f),
                            Alcance = LeerFloat(e, "alcance", 100f),
                            Dano = LeerFloat(e, "dano", 10f),
                            Perdigones = Math.Max(1, LeerInt(e, "perdigones", 1)),
                            CadenciaSeg = Math.Max(0f, LeerFloat(e, "cadencia_seg", 0.2f)),
                            ZoomFov = LeerFloat(e, "zoom_fov", 50f),
                            Cargador = Math.Max(1, LeerInt(e, "cargador", 10)),
                            RadioExplosion = LeerFloat(e, "radio_explosion", 0f)
                        };
                        lista.Add(def);
                    }
                }

                if (lista.Count == 0)
                    throw new InvalidDataException("JSON sin armas");

                Armas = lista;
                Debug.Log($"[F10] ARMAS: {lista.Count} leidas de JSON");
            }
            catch (Exception ex)
            {
                Armas = TablaEspejo();
                Debug.Log($"[F10] ARMAS: tabla espejo (motivo: {ex.Message})");
            }
        }

        private static List<ArmaDef> TablaEspejo()
        {
            return new List<ArmaDef>
            {
                new ArmaDef { Nombre = "Pistola 9mm", Proyectil = true, Velocidad = 120f, DispersionGrados = 0.4f, Alcance = 60f, Dano = 10f, Perdigones = 1, CadenciaSeg = 0.35f, ZoomFov = 50f, Cargador = 12 },
                new ArmaDef { Nombre = "AR 5.56", Proyectil = false, Velocidad = 0f, DispersionGrados = 0.15f, Alcance = 120f, Dano = 12f, Perdigones = 1, CadenciaSeg = 0.12f, ZoomFov = 35f, Cargador = 30 },
                new ArmaDef { Nombre = "Escopeta 12", Proyectil = true, Velocidad = 90f, DispersionGrados = 3.5f, Alcance = 30f, Dano = 6f, Perdigones = 8, CadenciaSeg = 0.9f, ZoomFov = 55f, Cargador = 6 },
                new ArmaDef { Nombre = "Francotirador .308", Proyectil = true, Velocidad = 260f, DispersionGrados = 0.02f, Alcance = 220f, Dano = 50f, Perdigones = 1, CadenciaSeg = 1.4f, ZoomFov = 18f, Cargador = 5 },
                new ArmaDef { Nombre = "RPG-7", Proyectil = true, Velocidad = 55f, DispersionGrados = 0.1f, Alcance = 150f, Dano = 100f, Perdigones = 1, CadenciaSeg = 2.2f, ZoomFov = 45f, Cargador = 4, RadioExplosion = 9f }
            };
        }

        /// <summary>Desvía la dirección dentro de un cono uniforme de semiángulo 'grados'.</summary>
        public static Vector3 AplicarDispersion(Vector3 dir, float grados, Random rng)
        {
            Vector3 d = Normalizar(dir);
            if (grados <= 0f || rng == null) return d;

            // Perpendiculares: se elige un eje auxiliar no paralelo a d.
            Vector3 aux = MathF.Abs(d.Y) < 0.99f
                ? new Vector3 { X = 0f, Y = 1f, Z = 0f }
                : new Vector3 { X = 1f, Y = 0f, Z = 0f };
            Vector3 p1 = Normalizar(Cruz(d, aux));
            Vector3 p2 = Cruz(d, p1);

            // Muestreo uniforme sobre el casquete esférico.
            float cosMax = MathF.Cos(grados * MathF.PI / 180f);
            float cosT = 1f - (float)rng.NextDouble() * (1f - cosMax);
            float sinT = MathF.Sqrt(MathF.Max(0f, 1f - cosT * cosT));
            float phi = (float)rng.NextDouble() * 2f * MathF.PI;
            float a = sinT * MathF.Cos(phi);
            float b = sinT * MathF.Sin(phi);

            return Normalizar(new Vector3
            {
                X = d.X * cosT + p1.X * a + p2.X * b,
                Y = d.Y * cosT + p1.Y * a + p2.Y * b,
                Z = d.Z * cosT + p1.Z * a + p2.Z * b
            });
        }

        /// <summary>Resuelve un gatillazo. Devuelve el número de disparos efectivos.</summary>
        public static int Disparar(ArmaDef arma, Vector3 origen, Vector3 direccion, Random rng)
        {
            if (arma == null) return 0;

            int efectivos = 0;
            int perdigones = Math.Max(1, arma.Perdigones);

            for (int i = 0; i < perdigones; i++)
            {
                Vector3 dir = AplicarDispersion(direccion, arma.DispersionGrados, rng);

                if (arma.Proyectil)
                {
                    if (arma.RadioExplosion > 0f)
                    {
                        if (ProyectilRpg.DispararDesdePool(origen, dir, arma.Velocidad, arma.Alcance, arma.Dano, arma.RadioExplosion))
                            efectivos++;
                    }
                    else if (ProyectilF10.DispararDesdePool(origen, dir, arma.Velocidad, arma.Alcance, arma.Dano))
                        efectivos++;
                }
                else
                {
                    if (Physics.Physics.Raycast(origen, dir, out GlassEngine.Core.Physics.RaycastHit hit, arma.Alcance))
                    {
                        RegistroImpactos.Registrar(hit.EntityId, arma.Dano, hit.Point);
                        efectivos++;
                    }
                }
            }

            return efectivos;
        }

        // --- utilidades ---

        private static Vector3 Cruz(Vector3 a, Vector3 b)
        {
            return new Vector3
            {
                X = a.Y * b.Z - a.Z * b.Y,
                Y = a.Z * b.X - a.X * b.Z,
                Z = a.X * b.Y - a.Y * b.X
            };
        }

        private static Vector3 Normalizar(Vector3 v)
        {
            float l = MathF.Sqrt(v.X * v.X + v.Y * v.Y + v.Z * v.Z);
            if (l < 1e-6f) return new Vector3 { X = 0f, Y = 0f, Z = -1f };
            return new Vector3 { X = v.X / l, Y = v.Y / l, Z = v.Z / l };
        }

        private static string LeerTexto(JsonElement e, string clave, string def)
        {
            if (e.TryGetProperty(clave, out JsonElement v) && v.ValueKind == JsonValueKind.String)
                return v.GetString() ?? def;
            return def;
        }

        private static bool LeerBool(JsonElement e, string clave, bool def)
        {
            if (!e.TryGetProperty(clave, out JsonElement v)) return def;
            if (v.ValueKind == JsonValueKind.True) return true;
            if (v.ValueKind == JsonValueKind.False) return false;
            return def;
        }

        private static float LeerFloat(JsonElement e, string clave, float def)
        {
            if (e.TryGetProperty(clave, out JsonElement v) && v.ValueKind == JsonValueKind.Number && v.TryGetDouble(out double d))
                return (float)d;
            return def;
        }

        private static int LeerInt(JsonElement e, string clave, int def)
        {
            if (e.TryGetProperty(clave, out JsonElement v) && v.ValueKind == JsonValueKind.Number)
            {
                if (v.TryGetInt32(out int i)) return i;
                if (v.TryGetDouble(out double d)) return (int)Math.Round(d);
            }
            return def;
        }
    }
}
