using UnityEngine;
using AudioSource = GlassEngine.Core.Audio.AudioSource;

namespace GlassEngine.Core.Scripts
{
    public class AudioPrueba2D : GlassBehaviour
    {
        AudioSource _audio;

        public override void Awake()
        {
            _audio = new AudioSource();
            _audio.EntityId = EntityId;
            _audio.ClipName = "plat3d_music";
            _audio.Volume = 0.5f;
            _audio.Looping = true;
            _audio.Spatial3D = false;
            _audio.Bus = "Music";
        }

        public override void Start()
        {
            _audio.Play(Position);
        }
    }
}
