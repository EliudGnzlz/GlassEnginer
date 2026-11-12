using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Audio;

public sealed class AudioSource
{
    public ulong EntityId { get; set; }
    public string ClipName { get; set; } = "beep_440";
    public float Volume { get; set; } = 1.0f;
    public float Pitch { get; set; } = 1.0f;
    public bool Looping { get; set; } = false;
    public bool Spatial3D { get; set; } = false;
    /// <summary>
    /// Bus de mezcla por el que suena este AudioSource: "Master",
    /// "Music" o "Sfx" (auditoría C6; antes los scripts no podían
    /// elegir bus y todo caía en el implícito del host). Un nombre
    /// desconocido avisa en el log y suena por Sfx.
    /// </summary>
    public string Bus { get; set; } = "Sfx";

    public ulong ActiveHandle { get; private set; } = 0;
    public bool IsPlaying => ActiveHandle != 0;

    public void Play(Vector3 position)
    {
        if (Spatial3D)
        {
            ActiveHandle = RuntimeBridge.PlayAudio3DOnBus(ClipName, Bus, position, Volume, Pitch, Looping);
        }
        else
        {
            ActiveHandle = RuntimeBridge.PlayAudio2DOnBus(ClipName, Bus, Volume, Pitch, Looping);
        }
    }

    public void UpdatePosition(Vector3 newPosition)
    {
        if (Spatial3D && IsPlaying)
        {
            RuntimeBridge.SetAudioSourcePosition(ActiveHandle, newPosition);
        }
    }

    public void Stop()
    {
        if (IsPlaying)
        {
            RuntimeBridge.StopAudio(ActiveHandle);
            ActiveHandle = 0;
        }
    }

    public static ulong PlayClipAtPoint(string clipName, Vector3 position, float volume = 1.0f, float pitch = 1.0f, string bus = "Sfx")
    {
        return RuntimeBridge.PlayAudio3DOnBus(clipName, bus, position, volume, pitch, false);
    }

    public static ulong Play2D(string clipName, float volume = 1.0f, float pitch = 1.0f, bool looping = false, string bus = "Sfx")
    {
        return RuntimeBridge.PlayAudio2DOnBus(clipName, bus, volume, pitch, looping);
    }
}
