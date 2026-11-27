// Sonda C0b: comprueba que los "fields" del sobre ApplyScriptData llegan
// aplicados antes del Awake diferido.

using System;
using System.Collections.Generic;
using UnityEngine;

public enum Modo { Quieto, Patrulla, Ataque }

[Serializable]
public class Tunel
{
    public float longitud = 0f;
    public int salidas = 0;
}

public class SondaCampos : MonoBehaviour
{
    public float velocidad = 1f;
    public int reintentos = 0;
    public string nombre = "sin-nombre";
    public bool activo = false;
    public Vector3 direccion = Vector3.zero;
    public Modo modo = Modo.Quieto;
    public List<int> patron = new();
    public Tunel tunel = new();

    [SerializeField] private float m_Umbral = 0.5f;

    public float UmbralLeido() => m_Umbral;

    private new void Awake()
    {
        Debug.Log(FormattableString.Invariant(
            $"[SONDA-CAMPOS] velocidad={velocidad} reintentos={reintentos} nombre={nombre} activo={activo} direccion={direccion} modo={modo} umbral={UmbralLeido()} patron={string.Join(",", patron)} tunel=({tunel.longitud},{tunel.salidas})"));
    }
}
