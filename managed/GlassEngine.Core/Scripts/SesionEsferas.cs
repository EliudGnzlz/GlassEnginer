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
        modoJerarquia = Environment.GetEnvironmentVariable("GLASS_ESF_MODO") == "JERARQUIA";
        if (modoJerarquia)
        {
            jBaseCount = RuntimeBridge.GetEntityCount();
            Debug.Log("[ESF] JERARQUIA listo raices=" + J_RAICES + " hijos=" + J_HIJOS + " nietos=" + J_NIETOS + " total=" + (J_RAICES + J_RAICES * J_HIJOS + J_RAICES * J_HIJOS * J_NIETOS) + " ciclos=" + C + " base=" + jBaseCount);
        }
        else
        {
            Debug.Log("[ESF] listo n=" + N + " fisica=" + F + " ciclos=" + C);
        }
    }

    // ---- Modo JERARQUIA (escalón 6 de la escalera de esferas) ----
    // 100 raíces x 10 hijos x 10 nietos = 11.100 entidades, profundidad
    // 3, sin física. Comprueba y loguea: (1) al mover una raíz el nieto
    // queda en la posición mundial esperada (compuesta de los locales
    // leídos por el puente: el puente C# lee transformadas LOCALES, así
    // que la comprobación verifica que ningún local se corrompe al mover
    // el padre y que la cadena compone el mundo esperado); (2) al borrar
    // solo las raíces el subárbol entero desaparece y el conteo vuelve a
    // la base sin huérfanos; (3) tiempo de frame con el árbol completo.
    bool modoJerarquia = false;
    enum FaseJ { Raices, Hijos, Nietos, EsperaTrasGenerar, BorrandoRaices, EsperaTrasBorrar, Fin }
    FaseJ faseJ = FaseJ.Raices;
    const int J_RAICES = 100, J_HIJOS = 10, J_NIETOS = 10;
    List<ulong> jRaices = new List<ulong>();
    List<ulong> jHijos = new List<ulong>();
    int jCursor = 0;
    int jBaseCount = -1;
    bool jChequeoHecho = false;
    int jFrameN = 0;
    Stopwatch jFrameSw = new Stopwatch();

    int LeerEnv(string nombre, int def)
    {
        var v = Environment.GetEnvironmentVariable(nombre);
        return int.TryParse(v, out int r) ? r : def;
    }

    public override void Update(float deltaTime)
    {
        if (modoJerarquia)
        {
            UpdateJerarquia(deltaTime);
            return;
        }
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

    void UpdateJerarquia(float dt)
    {
        switch (faseJ)
        {
            case FaseJ.Raices:
            {
                if (jCursor == 0 && jRaices.Count == 0) sw.Restart();
                int lote = Math.Min(LOTE, J_RAICES - jRaices.Count);
                for (int k = 0; k < lote; k++)
                {
                    int i = jRaices.Count;
                    Vector3 pos = new Vector3((i % 10) * 4f, 0f, 300f + (i / 10) * 4f);
                    ulong id = RuntimeBridge.SpawnEntity("RaizJ", "sphere", null, 0,
                        pos, Quaternion.Identity, new Vector3(1, 1, 1),
                        SpawnPhysicsKind.None, new Vector3(0.9f, 0.5f, 0.3f));
                    if (id == 0) fallos++; else jRaices.Add(id);
                }
                if (jRaices.Count + fallos >= J_RAICES) { jCursor = 0; faseJ = FaseJ.Hijos; }
                break;
            }
            case FaseJ.Hijos:
            {
                int total = J_RAICES * J_HIJOS;
                int lote = Math.Min(LOTE, total - jHijos.Count);
                for (int k = 0; k < lote; k++)
                {
                    int i = jHijos.Count;
                    ulong padre = jRaices[i / J_HIJOS];
                    Vector3 local = new Vector3((i % J_HIJOS) * 0.35f, 0f, 0f);
                    ulong id = RuntimeBridge.SpawnEntity("HijoJ", "sphere", null, padre,
                        local, Quaternion.Identity, new Vector3(0.6f, 0.6f, 0.6f),
                        SpawnPhysicsKind.None, new Vector3(0.4f, 0.9f, 0.5f));
                    if (id == 0) fallos++; else jHijos.Add(id);
                }
                if (jHijos.Count + fallos >= total) { jCursor = 0; faseJ = FaseJ.Nietos; }
                break;
            }
            case FaseJ.Nietos:
            {
                int total = J_RAICES * J_HIJOS * J_NIETOS;
                int hechos = jCursor;
                int lote = Math.Min(LOTE, total - hechos);
                for (int k = 0; k < lote; k++)
                {
                    int i = hechos + k;
                    ulong padre = jHijos[i / J_NIETOS];
                    Vector3 local = new Vector3(0f, (i % J_NIETOS) * 0.35f, 0f);
                    ulong id = RuntimeBridge.SpawnEntity("NietoJ", "sphere", null, padre,
                        local, Quaternion.Identity, new Vector3(0.35f, 0.35f, 0.35f),
                        SpawnPhysicsKind.None, colorEsfera);
                    if (id == 0) fallos++;
                    if (i == 34) jNietoSonda = id;
                }
                jCursor += lote;
                if (jCursor >= total)
                {
                    sw.Stop();
                    int cuenta = RuntimeBridge.GetEntityCount();
                    int totalArbol = cuenta - jBaseCount;
                    Debug.Log("[ESF] JERARQUIA GENERADO n=" + totalArbol + " fallos=" + fallos + " ms=" + sw.ElapsedMilliseconds + " cuenta=" + cuenta + " esperada=" + (jBaseCount + J_RAICES + J_RAICES * J_HIJOS + J_RAICES * J_HIJOS * J_NIETOS) + " ciclo=" + ciclo);
                    esperaFrames = 0; jFrameN = 0; jChequeoHecho = false;
                    jFrameSw.Restart();
                    faseJ = FaseJ.EsperaTrasGenerar;
                }
                break;
            }
            case FaseJ.EsperaTrasGenerar:
                // Tiempo de frame REAL (reloj de pared entre Updates): el
                // deltaTime que recibe el script viene limitado por el
                // motor y no refleja el coste verdadero con el árbol
                // completo, que es lo que este escalón debe medir.
                jFrameN++;
                if (!jChequeoHecho)
                {
                    jChequeoHecho = true;
                    ChequeoHijoSigue();
                }
                if (++esperaFrames >= ESPERA)
                {
                    jFrameSw.Stop();
                    float media = jFrameN > 0 ? (float)jFrameSw.ElapsedMilliseconds / jFrameN : 0f;
                    Debug.Log("[ESF] JERARQUIA frame_ms=" + media.ToString("F2") + " frames=" + jFrameN + " ciclo=" + ciclo);
                    esperaFrames = 0;
                    sw.Restart();
                    faseJ = FaseJ.BorrandoRaices;
                }
                break;
            case FaseJ.BorrandoRaices:
            {
                int lote = Math.Min(LOTE, jRaices.Count);
                for (int k = 0; k < lote; k++)
                {
                    int idx = jRaices.Count - 1;
                    RuntimeBridge.DestroyEntity(jRaices[idx]);
                    jRaices.RemoveAt(idx);
                }
                if (jRaices.Count == 0)
                {
                    sw.Stop();
                    Debug.Log("[ESF] JERARQUIA BORRADAS_RAICES ms=" + sw.ElapsedMilliseconds + " ciclo=" + ciclo);
                    esperaFrames = 0;
                    faseJ = FaseJ.EsperaTrasBorrar;
                }
                break;
            }
            case FaseJ.EsperaTrasBorrar:
                if (++esperaFrames >= ESPERA)
                {
                    esperaFrames = 0;
                    int cuenta = RuntimeBridge.GetEntityCount();
                    bool ok = cuenta == jBaseCount;
                    Debug.Log("[ESF] JERARQUIA sin_huerfanos ok=" + ok + " cuenta=" + cuenta + " base=" + jBaseCount + " ciclo=" + ciclo);
                    jHijos.Clear(); jCursor = 0; fallos = 0; jNietoSonda = 0;
                    ciclo++;
                    if (ciclo >= C)
                    {
                        Debug.Log("[ESF] FIN ciclos=" + C);
                        faseJ = FaseJ.Fin;
                    }
                    else faseJ = FaseJ.Raices;
                }
                break;
            case FaseJ.Fin:
                break;
        }
    }

    ulong jNietoSonda = 0;

    // Comprobación (1): el nieto sonda es el nº 34 (hijo 3 de la raíz 0,
    // nieto 4 de ese hijo): local del hijo = (3*0,35, 0, 0), local del
    // nieto = (0, 4*0,35, 0). Se mueve la raíz 0 en Δ y se relee la
    // cadena de locales por el puente: ningún local debe cambiar y el
    // mundo compuesto (raíz + hijo + nieto, rotaciones identidad y
    // escalas sin rotación de por medio en la cadena de traslación) debe
    // ser el esperado + Δ.
    void ChequeoHijoSigue()
    {
        ulong raiz = jRaices[0];
        ulong hijo = jHijos[3];
        ulong nieto = jNietoSonda;
        if (nieto == 0) { Debug.Log("[ESF] JERARQUIA hijo_sigue ok=False motivo=sin_sonda"); return; }
        if (RuntimeBridge.GetTransform(raiz, out var tR) != 0 ||
            RuntimeBridge.GetTransform(hijo, out var tH) != 0 ||
            RuntimeBridge.GetTransform(nieto, out var tN) != 0)
        {
            Debug.Log("[ESF] JERARQUIA hijo_sigue ok=False motivo=puente");
            return;
        }
        Vector3 antes = new Vector3(
            tR.Position.X + tH.Position.X + tN.Position.X,
            tR.Position.Y + tH.Position.Y + tN.Position.Y,
            tR.Position.Z + tH.Position.Z + tN.Position.Z);
        Vector3 delta = new Vector3(7f, 3f, -5f);
        tR.Position = new Vector3(tR.Position.X + delta.X, tR.Position.Y + delta.Y, tR.Position.Z + delta.Z);
        RuntimeBridge.SetTransform(raiz, ref tR);
        RuntimeBridge.GetTransform(raiz, out var tR2);
        RuntimeBridge.GetTransform(hijo, out var tH2);
        RuntimeBridge.GetTransform(nieto, out var tN2);
        Vector3 despues = new Vector3(
            tR2.Position.X + tH2.Position.X + tN2.Position.X,
            tR2.Position.Y + tH2.Position.Y + tN2.Position.Y,
            tR2.Position.Z + tH2.Position.Z + tN2.Position.Z);
        Vector3 esperado = new Vector3(antes.X + delta.X, antes.Y + delta.Y, antes.Z + delta.Z);
        bool localesIntactos =
            tH2.Position.X == tH.Position.X && tH2.Position.Y == tH.Position.Y && tH2.Position.Z == tH.Position.Z &&
            tN2.Position.X == tN.Position.X && tN2.Position.Y == tN.Position.Y && tN2.Position.Z == tN.Position.Z;
        bool ok = localesIntactos &&
            Math.Abs(despues.X - esperado.X) < 0.001f &&
            Math.Abs(despues.Y - esperado.Y) < 0.001f &&
            Math.Abs(despues.Z - esperado.Z) < 0.001f;
        Debug.Log("[ESF] JERARQUIA hijo_sigue ok=" + ok + " locales_intactos=" + localesIntactos +
            " nieto_mundo=(" + despues.X.ToString("F2") + "," + despues.Y.ToString("F2") + "," + despues.Z.ToString("F2") + ")" +
            " esperado=(" + esperado.X.ToString("F2") + "," + esperado.Y.ToString("F2") + "," + esperado.Z.ToString("F2") + ")");
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
