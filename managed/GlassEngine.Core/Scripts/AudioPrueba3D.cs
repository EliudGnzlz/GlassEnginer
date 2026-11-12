using System;
using UnityEngine;
using AudioSource = GlassEngine.Core.Audio.AudioSource;

namespace GlassEngine.Core.Scripts
{
    public class AudioPrueba3D : GlassBehaviour
    {
        static readonly Vector3 Centro = new Vector3(6f, 1f, 2f);
        const float Radio = 2.5f;
        const float Omega = 3f;

        AudioSource _audio;
        float _t;

        public override void Awake()
        {
            _audio = new AudioSource();
            _audio.EntityId = EntityId;
            _audio.ClipName = "beep_440";
            _audio.Volume = 0.8f;
            _audio.Pitch = 1.0f;
            _audio.Looping = true;
            _audio.Spatial3D = true;
            _audio.Bus = "Sfx";
        }

        public override void Start()
        {
            _audio.Play(Position);
        }

        public override void Update(float deltaTime)
        {
            _t += deltaTime;
            float a = _t * Omega;
            Position = new Vector3(
                Centro.x + MathF.Cos(a) * Radio,
                Centro.y,
                Centro.z + MathF.Sin(a) * Radio);
            _audio.UpdatePosition(Position);
        }
    }
}
