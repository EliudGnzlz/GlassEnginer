using System;
using GlassEngine.Core.Audio;
using GlassEngine.Core.Interop;

namespace GlassEngine.Core.Scripts;

public sealed class AudioEmitterTest : GlassBehaviour
{
    private AudioSource? _audioSource;
    private float _timer = 0.0f;

    public override void Awake()
    {
        Log("AudioEmitterTest Awake iniciado.");
        _audioSource = new AudioSource
        {
            EntityId = EntityId,
            ClipName = "beep_440",
            Volume = 0.8f,
            Pitch = 1.0f,
            Looping = true,
            Spatial3D = true
        };
    }

    public override void Start()
    {
        Log($"AudioEmitterTest Start: reproduciendo sonido 3D en entidad {EntityId}...");
        _audioSource?.Play(Position);
    }

    public override void Update(float deltaTime)
    {
        _timer += deltaTime;

        // Mover la posición de la entidad y actualizar la fuente de audio espacial 3D
        Vector3 pos = Position;
        pos.X = MathF.Cos(_timer * 3.0f) * 2.5f;
        pos.Z = MathF.Sin(_timer * 3.0f) * 2.5f;
        Position = pos;

        _audioSource?.UpdatePosition(pos);
    }

    public void StopAudio()
    {
        _audioSource?.Stop();
        Log("AudioEmitterTest: audio detenido.");
    }
}
