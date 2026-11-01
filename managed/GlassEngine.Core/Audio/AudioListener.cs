using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Audio;

public static class AudioListener
{
    public static void SetTransform(Vector3 position, Quaternion rotation)
    {
        RuntimeBridge.SetListenerTransform(position, rotation);
    }
}
