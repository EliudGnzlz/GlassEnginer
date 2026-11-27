using UnityEngine;

public class SondaAudio : MonoBehaviour
{
    public override void Start()
    {
        var fuente = gameObject.AddComponent<AudioSource>();
        fuente.clip = "beep_440";

        fuente.Play();
        Debug.Log("[SONDA-A] tras Play isPlaying=" + fuente.isPlaying);

        fuente.Pause();
        Debug.Log("[SONDA-A] tras Pause isPlaying=" + fuente.isPlaying);

        fuente.UnPause();
        Debug.Log("[SONDA-A] tras UnPause isPlaying=" + fuente.isPlaying);

        fuente.Stop();
        Debug.Log("[SONDA-A] tras Stop isPlaying=" + fuente.isPlaying);
    }
}
