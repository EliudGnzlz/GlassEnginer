use std::sync::Arc;
use std::thread;
use std::time::Duration;
use glass_audio::{
    generate_tone, glass_audio_play_2d, glass_audio_play_3d,
    glass_audio_set_listener_transform, glass_audio_set_source_position, glass_audio_stop,
    register_global_audio_system, AudioSystem,
};

#[test]
fn test_procedural_wav_generation() {
    let tone = generate_tone(440.0, 0.1, 0.5);
    assert!(tone.len() > 44);
    assert_eq!(&tone[0..4], b"RIFF");
    assert_eq!(&tone[8..12], b"WAVE");
}

#[test]
fn test_audio_system_lifecycle_and_playback() {
    let _ = env_logger::builder().is_test(true).try_init();

    let audio = Arc::new(AudioSystem::new());
    register_global_audio_system(audio.clone());

    // 1. Play 2D audio
    let h2d = audio.play_2d("beep_440", 0.5, 1.0, false);
    assert!(h2d > 0);

    // 2. Play 3D spatial audio
    let h3d = audio.play_3d("beep_880", [2.0, 0.0, 1.0], 0.8, 1.0, true);
    assert!(h3d > 0);

    // 3. Move listener and source
    audio.set_listener_transform([0.0, 1.5, -3.0], [0.0, 0.0, 0.0, 1.0]);
    audio.set_source_position(h3d, [3.0, 0.0, 2.0]);

    // Give decoupled audio thread time to process command queue
    thread::sleep(Duration::from_millis(50));

    // 4. Test C-ABI FFI functions
    unsafe {
        let c_clip = std::ffi::CString::new("beep_440").unwrap();
        let ffi_handle = glass_audio_play_2d(c_clip.as_ptr(), 0.6, 1.0, false);
        assert!(ffi_handle > 0);

        let ffi_3d = glass_audio_play_3d(c_clip.as_ptr(), 1.0, 0.0, 0.0, 0.7, 1.0, false);
        assert!(ffi_3d > 0);

        glass_audio_set_source_position(ffi_3d, 1.5, 0.0, 0.5);
        glass_audio_set_listener_transform(0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0);

        glass_audio_stop(ffi_handle);
        glass_audio_stop(ffi_3d);
    }

    // 5. Stop audio sources
    audio.stop(h2d);
    audio.stop(h3d);
    audio.stop_all();

    thread::sleep(Duration::from_millis(30));
}
