using UnityEngine;

// Sonda C2: objetivo de búsqueda.
public class SondaBuscada : MonoBehaviour
{
    public int marca = 42;
}

// Sonda C2: ejercita FindObjectOfType y familia, y GameObject.Find y familia.
public class SondaBuscadora : MonoBehaviour
{
    public override void Start()
    {
        // 1. Primera SondaBuscada activa.
        var b = FindObjectOfType<SondaBuscada>();
        Debug.Log($"[SONDA-B] FindObjectOfType: {(b == null ? "null" : b.gameObject.name + " marca=" + b.marca)}");

        // 2. Conteo de todos los MonoBehaviour activos indexados.
        Debug.Log($"[SONDA-B] FindObjectsOfType<MonoBehaviour>: {FindObjectsOfType<MonoBehaviour>().Length}");

        // 3. GameObject activo por nombre exacto.
        var go = GameObject.Find("EntidadBuscada");
        Debug.Log($"[SONDA-B] GameObject.Find: {(go == null ? "null" : "encontrado " + go.name)}");

        // 4. GameObjects activos con el tag "Buscada".
        Debug.Log($"[SONDA-B] FindGameObjectsWithTag: {GameObject.FindGameObjectsWithTag("Buscada").Length}");

        // 5. Autobúsqueda.
        var yo = FindObjectOfType<SondaBuscadora>();
        Debug.Log($"[SONDA-B] FindObjectOfType<SondaBuscadora>: {(yo == null ? "null" : yo.gameObject.name)}");

        // 6. Con inactivos: debe coincidir con 1 si nada está inactivo.
        var bi = FindObjectOfType<SondaBuscada>(true);
        Debug.Log($"[SONDA-B] con inactivos: {(bi == null ? "null" : bi.gameObject.name)}");
    }
}
