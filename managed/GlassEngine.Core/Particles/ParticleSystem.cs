using System;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Particles;

/// <summary>
/// API C# del emisor de partículas de una entidad (Fase 8P).
/// Play/Stop/Emit no actúan al momento: encolan un comando que el
/// bucle del motor aplica en el siguiente frame, en orden con el
/// resto de comandos (el mismo camino que usa el MCP).
/// </summary>
public sealed class ParticleSystem
{
    public ulong EntityId { get; set; }

    public void Play()
    {
        RuntimeBridge.PlayParticles(EntityId);
    }

    public void Stop()
    {
        RuntimeBridge.StopParticles(EntityId);
    }

    /// <summary>Emite una ráfaga de <paramref name="count"/> partículas ya.</summary>
    public void Emit(int count)
    {
        if (count > 0)
        {
            RuntimeBridge.BurstParticles(EntityId, (uint)count);
        }
    }
}
