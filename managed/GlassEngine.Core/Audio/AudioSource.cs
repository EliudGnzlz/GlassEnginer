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

    public ulong ActiveHandle { get; private set; } = 0;
    public bool IsPlaying => ActiveHandle != 0;

    public void Play(Vector3 position)
    {
        if (Spatial3D)
        {
            ActiveHandle = RuntimeBridge.PlayAudio3D(ClipName, position, Volume, Pitch, Looping);
        }
        else
        {
            ActiveHandle = RuntimeBridge.PlayAudio2D(ClipName, Volume, Pitch, Looping);
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

    public static ulong PlayClipAtPoint(string clipName, Vector3 position, float volume = 1.0f, float pitch = 1.0f)
    {
        return RuntimeBridge.PlayAudio3D(clipName, position, volume, pitch, false);
    }

    public static ulong Play2D(string clipName, float volume = 1.0f, float pitch = 1.0f, bool looping = false)
    {
        return RuntimeBridge.PlayAudio2D(clipName, volume, pitch, looping);
    }
}
