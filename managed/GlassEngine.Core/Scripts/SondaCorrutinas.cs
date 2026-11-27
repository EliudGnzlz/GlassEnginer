using UnityEngine;

/// <summary>
/// Sonda del paquete C1: recorre cada forma de espera de las corrutinas y
/// deja en el log, con [SONDA-C] y Time.frameCount, el frame en que se
/// reanuda cada paso.
/// </summary>
public class SondaCorrutinas : MonoBehaviour
{
    // Cacheadas como en NeoFPS: deben poder reutilizarse.
    private readonly WaitForEndOfFrame _finDeFrame = new WaitForEndOfFrame();
    private readonly WaitForFixedUpdate _pasoFijo = new WaitForFixedUpdate();

    private int _pasosInterrumpible;

    public override void Start()
    {
        Log("Start: arranca la corrutina principal");
        StartCoroutine(Principal());
        Log("Start: StartCoroutine ha vuelto");
    }

    private System.Collections.IEnumerator Principal()
    {
        Log("paso inmediato (síncrono dentro de StartCoroutine)");

        yield return null;
        Log("tras yield null");

        yield return new WaitForSeconds(0.05f);
        Log($"tras WaitForSeconds(0.05) time={Time.time}");

        yield return _finDeFrame;
        Log("tras WaitForEndOfFrame");

        yield return _pasoFijo;
        Log("tras WaitForFixedUpdate");

        yield return Anidada("yield return IEnumerator");
        Log("tras anidada por yield return IEnumerator");

        yield return StartCoroutine(Anidada("yield return StartCoroutine"));
        Log("tras anidada por yield return StartCoroutine");

        // Parada por nombre a mitad: 10 pasos previstos, se para tras 2 frames.
        Coroutine porNombre = StartCoroutine(nameof(Interrumpible), 10);
        if (porNombre is null)
            Log("ERROR: StartCoroutine por nombre devolvió null");

        yield return null;
        yield return null;
        StopCoroutine(nameof(Interrumpible));
        int alParar = _pasosInterrumpible;
        Log($"StopCoroutine(\"{nameof(Interrumpible)}\") tras {alParar} pasos");

        yield return null;
        yield return null;
        yield return null;
        Log(_pasosInterrumpible == alParar
            ? "parada por nombre OK"
            : $"ERROR: Interrumpible siguió tras la parada ({_pasosInterrumpible} pasos)");

        Log("FIN");
    }

    private System.Collections.IEnumerator Anidada(string via)
    {
        Log($"anidada ({via}): entra");
        yield return null;
        Log($"anidada ({via}): sale");
    }

    private System.Collections.IEnumerator Interrumpible(int pasos)
    {
        for (int i = 1; i <= pasos; i++)
        {
            _pasosInterrumpible = i;
            Log($"Interrumpible paso {i}/{pasos}");
            yield return null;
        }

        Log("ERROR: Interrumpible llegó al final; no se detuvo");
    }

    private static void Log(string mensaje) =>
        Debug.Log($"[SONDA-C] frame={Time.frameCount} {mensaje}");
}
