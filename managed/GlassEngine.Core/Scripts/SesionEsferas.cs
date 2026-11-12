using System;
using System.Collections.Generic;
using UnityEngine;
using Debug = UnityEngine.Debug;
using Stopwatch = System.Diagnostics.Stopwatch;
using Vector3 = GlassEngine.Core.Interop.Vector3;
using Quaternion = GlassEngine.Core.Interop.Quaternion;

namespace GlassEngine.Core.Scripts
{
public class SesionEsferas : GlassBehaviour
{
    enum Fase { Generando, EsperaTrasGenerar, Borrando, EsperaTrasBorrar, Fin }

    int N, F, C, LOTE, ESPERA;
    int lado;
    Fase fase = Fase.Generando;
    List<ulong> ids = new List<ulong>();
    int creados = 0;
    int fallos = 0;
    int ciclo = 0;
    int esperaFrames = 0;
    Stopwatch sw = new Stopwatch();
    Vector3 colorEsfera = new Vector3(0.4f, 0.6f, 0.9f);

    public override void Start()
    {
        N = LeerEnv("GLASS_ESF_N", 1000);
        F = LeerEnv("GLASS_ESF_FISICA", 2);
        C = LeerEnv("GLASS_ESF_CICLOS", 3);
        LOTE = LeerEnv("GLASS_ESF_LOTE", 4000);
        ESPERA = LeerEnv("GLASS_ESF_ESPERA", 90);
        lado = (int)Math.Ceiling(Math.Pow(N, 1.0 / 3.0));
        Debug.Log("[ESF] listo n=" + N + " fisica=" + F + " ciclos=" + C);
    }

    int LeerEnv(string nombre, int def)
    {
        var v = Environment.GetEnvironmentVariable(nombre);
        return int.TryParse(v, out int r) ? r : def;
    }

    public override void Update(float deltaTime)
    {
        switch (fase)
        {
            case Fase.Generando:
                Generando();
                break;
            case Fase.EsperaTrasGenerar:
                if (++esperaFrames >= ESPERA)
                {
                    esperaFrames = 0;
                    fase = Fase.Borrando;
                    sw.Restart();
                }
                break;
            case Fase.Borrando:
                Borrando();
                break;
            case Fase.EsperaTrasBorrar:
                if (++esperaFrames >= ESPERA)
                {
                    esperaFrames = 0;
                    ciclo++;
                    if (ciclo >= C)
                    {
                        Debug.Log("[ESF] FIN ciclos=" + C);
                        fase = Fase.Fin;
                    }
                    else
                    {
                        creados = 0;
                        fallos = 0;
                        fase = Fase.Generando;
                    }
                }
                break;
            case Fase.Fin:
                break;
        }
    }

    void Generando()
    {
        if (creados == 0 && fallos == 0)
            sw.Restart();

        int restantes = N - (creados + fallos);
        int enEsteLote = Math.Min(LOTE, restantes);

        var physics = (SpawnPhysicsKind)F;

        for (int k = 0; k < enEsteLote; k++)
        {
            int i = creados + fallos;
            int x = i % lado;
            int y = (i / lado) % lado;
            int z = i / (lado * lado);
            Vector3 pos = new Vector3(
                (x - lado / 2) * 1.3f,
                (y - lado / 2) * 1.3f,
                300f + (z - lado / 2) * 1.3f
            );

            ulong id = RuntimeBridge.SpawnEntity(
                "Esf", "sphere", null, 0,
                pos, Quaternion.Identity, new Vector3(1, 1, 1),
                physics, colorEsfera
            );

            if (id == 0)
                fallos++;
            else
            {
                ids.Add(id);
                creados++;
            }
        }

        if (creados + fallos >= N)
        {
            sw.Stop();
            Debug.Log("[ESF] GENERADAS n=" + creados + " fallos=" + fallos + " ms=" + sw.ElapsedMilliseconds + " ciclo=" + ciclo);
            fase = Fase.EsperaTrasGenerar;
        }
    }

    void Borrando()
    {
        if (ids.Count > 0 && sw.ElapsedMilliseconds == 0 && !sw.IsRunning)
            sw.Restart();

        int enEsteLote = Math.Min(LOTE, ids.Count);

        for (int k = 0; k < enEsteLote; k++)
        {
            int idx = ids.Count - 1;
            RuntimeBridge.DestroyEntity(ids[idx]);
            ids.RemoveAt(idx);
        }

        if (ids.Count == 0)
        {
            sw.Stop();
            Debug.Log("[ESF] BORRADAS ms=" + sw.ElapsedMilliseconds + " ciclo=" + ciclo);
            fase = Fase.EsperaTrasBorrar;
        }
    }
}
}
