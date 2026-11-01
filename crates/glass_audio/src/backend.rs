use std::collections::HashMap;
use std::io::Cursor;
use kira::{
    listener::ListenerHandle,
    sound::{
        static_sound::{StaticSoundData, StaticSoundHandle},
        PlaybackState,
    },
    track::{SpatialTrackBuilder, SpatialTrackHandle},
    AudioManager, AudioManagerSettings, Capacities, Decibels, DefaultBackend, PlaybackRate, Tween,
};
use crate::command::SoundHandle;

pub trait AudioBackend: Send {
    fn load_clip(&mut self, name: &str, wav_bytes: Vec<u8>) -> bool;
    fn play_2d(&mut self, name: &str, volume: f32, pitch: f32, looping: bool, handle: SoundHandle) -> bool;
    fn play_3d(&mut self, name: &str, position: [f32; 3], volume: f32, pitch: f32, looping: bool, handle: SoundHandle) -> bool;
    fn set_source_position(&mut self, handle: SoundHandle, position: [f32; 3]);
    fn set_listener_transform(&mut self, position: [f32; 3], orientation: [f32; 4]);
    fn stop(&mut self, handle: SoundHandle);
    fn stop_all(&mut self);
}

pub struct KiraBackend {
    manager: AudioManager<DefaultBackend>,
    listener: ListenerHandle,
    clips: HashMap<String, StaticSoundData>,
    active_2d: HashMap<SoundHandle, StaticSoundHandle>,
    active_3d: HashMap<SoundHandle, (SpatialTrackHandle, StaticSoundHandle)>,
}

impl KiraBackend {
    pub fn try_new() -> Result<Self, String> {
        let settings = AudioManagerSettings {
            capacities: Capacities {
                sub_track_capacity: 512,
                listener_capacity: 16,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut manager = AudioManager::<DefaultBackend>::new(settings)
            .map_err(|e| format!("Fallo al inicializar CPAL AudioManager: {:?}", e))?;

        let listener = manager
            .add_listener([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0])
            .map_err(|e| format!("Fallo al crear ListenerHandle: {:?}", e))?;

        Ok(Self {
            manager,
            listener,
            clips: HashMap::new(),
            active_2d: HashMap::new(),
            active_3d: HashMap::new(),
        })
    }
}

impl AudioBackend for KiraBackend {
    fn load_clip(&mut self, name: &str, wav_bytes: Vec<u8>) -> bool {
        match StaticSoundData::from_cursor(Cursor::new(wav_bytes)) {
            Ok(data) => {
                log::info!("glass_audio: Clip '{}' cargado ({:.2} s)", name, data.duration().as_secs_f32());
                self.clips.insert(name.to_string(), data);
                true
            }
            Err(e) => {
                log::error!("glass_audio: Error decodificando WAV '{}': {:?}", name, e);
                false
            }
        }
    }

    fn play_2d(&mut self, name: &str, volume: f32, pitch: f32, looping: bool, handle: SoundHandle) -> bool {
        self.active_2d.retain(|_, sound| sound.state() != PlaybackState::Stopped);
        let sound_data = match self.clips.get(name) {
            Some(d) => d.clone(),
            None => {
                log::warn!("glass_audio: Clip '{}' no encontrado para reproducir en 2D", name);
                return false;
            }
        };

        // Convert amplitude (0.0..1.0) to decibels
        let db = if volume <= 0.0001 {
            Decibels::SILENCE
        } else {
            Decibels(20.0 * volume.log10())
        };

        let mut configured = sound_data
            .volume(db)
            .playback_rate(PlaybackRate(pitch.max(0.05) as f64));

        if looping {
            configured = configured.loop_region(..);
        }

        match self.manager.play(configured) {
            Ok(sound_handle) => {
                self.active_2d.insert(handle, sound_handle);
                true
            }
            Err(e) => {
                log::error!("glass_audio: Error al reproducir sonido 2D '{}': {:?}", name, e);
                false
            }
        }
    }

    fn play_3d(&mut self, name: &str, position: [f32; 3], volume: f32, pitch: f32, looping: bool, handle: SoundHandle) -> bool {
        self.active_3d.retain(|_, (_, sound)| sound.state() != PlaybackState::Stopped);
        let sound_data = match self.clips.get(name) {
            Some(d) => d.clone(),
            None => {
                log::warn!("glass_audio: Clip '{}' no encontrado para reproducir en 3D", name);
                return false;
            }
        };

        let db = if volume <= 0.0001 {
            Decibels::SILENCE
        } else {
            Decibels(20.0 * volume.log10())
        };

        let mut configured = sound_data
            .volume(db)
            .playback_rate(PlaybackRate(pitch.max(0.05) as f64));

        if looping {
            configured = configured.loop_region(..);
        }

        // Create spatial sub-track linked to listener
        let track_builder = SpatialTrackBuilder::new();
        match self.manager.add_spatial_sub_track(&self.listener, position, track_builder) {
            Ok(mut spatial_track) => {
                match spatial_track.play(configured) {
                    Ok(sound_handle) => {
                        self.active_3d.insert(handle, (spatial_track, sound_handle));
                        true
                    }
                    Err(e) => {
                        log::error!("glass_audio: Error al reproducir en pista espacial: {:?}", e);
                        false
                    }
                }
            }
            Err(e) => {
                log::error!("glass_audio: Error creando pista espacial para sonido 3D: {:?}", e);
                false
            }
        }
    }

    fn set_source_position(&mut self, handle: SoundHandle, position: [f32; 3]) {
        if let Some((spatial_track, _)) = self.active_3d.get_mut(&handle) {
            spatial_track.set_position(position, Tween::default());
        }
    }

    fn set_listener_transform(&mut self, position: [f32; 3], orientation: [f32; 4]) {
        self.listener.set_position(position, Tween::default());
        self.listener.set_orientation(orientation, Tween::default());
    }

    fn stop(&mut self, handle: SoundHandle) {
        if let Some(mut sound_handle) = self.active_2d.remove(&handle) {
            sound_handle.stop(Tween::default());
        }
        if let Some((_, mut sound_handle)) = self.active_3d.remove(&handle) {
            sound_handle.stop(Tween::default());
        }
    }

    fn stop_all(&mut self) {
        for (_, mut sound_handle) in self.active_2d.drain() {
            sound_handle.stop(Tween::default());
        }
        for (_, (_, mut sound_handle)) in self.active_3d.drain() {
            sound_handle.stop(Tween::default());
        }
    }
}

pub struct NullBackend {
    clips: HashMap<String, usize>,
    active_sources: HashMap<SoundHandle, [f32; 3]>,
    listener_pos: [f32; 3],
    listener_rot: [f32; 4],
}

impl NullBackend {
    pub fn new() -> Self {
        Self {
            clips: HashMap::new(),
            active_sources: HashMap::new(),
            listener_pos: [0.0; 3],
            listener_rot: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

impl AudioBackend for NullBackend {
    fn load_clip(&mut self, name: &str, wav_bytes: Vec<u8>) -> bool {
        self.clips.insert(name.to_string(), wav_bytes.len());
        true
    }

    fn play_2d(&mut self, _name: &str, _volume: f32, _pitch: f32, _looping: bool, handle: SoundHandle) -> bool {
        self.active_sources.insert(handle, [0.0, 0.0, 0.0]);
        true
    }

    fn play_3d(&mut self, _name: &str, position: [f32; 3], _volume: f32, _pitch: f32, _looping: bool, handle: SoundHandle) -> bool {
        self.active_sources.insert(handle, position);
        true
    }

    fn set_source_position(&mut self, handle: SoundHandle, position: [f32; 3]) {
        if let Some(pos) = self.active_sources.get_mut(&handle) {
            *pos = position;
        }
    }

    fn set_listener_transform(&mut self, position: [f32; 3], orientation: [f32; 4]) {
        self.listener_pos = position;
        self.listener_rot = orientation;
    }

    fn stop(&mut self, handle: SoundHandle) {
        self.active_sources.remove(&handle);
    }

    fn stop_all(&mut self) {
        self.active_sources.clear();
    }
}
