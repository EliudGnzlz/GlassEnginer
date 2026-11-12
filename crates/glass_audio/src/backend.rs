use std::collections::HashMap;
use std::io::Cursor;
use kira::{
    listener::ListenerHandle,
    sound::{
        static_sound::{StaticSoundData, StaticSoundHandle},
        PlaybackState,
    },
    track::{SpatialTrackBuilder, SpatialTrackHandle, TrackBuilder},
    effect::filter::{FilterBuilder, FilterMode},
    AudioManager, AudioManagerSettings, Capacities, Decibels, DefaultBackend, PlaybackRate, Tween,
};
use crate::buses::{BusId, BusMixer};
use crate::command::SoundHandle;

pub trait AudioBackend: Send {
    fn load_clip(&mut self, name: &str, wav_bytes: Vec<u8>) -> bool;
    fn play_2d(&mut self, name: &str, volume: f32, pitch: f32, looping: bool, bus: BusId, handle: SoundHandle) -> bool;
    fn play_3d(&mut self, name: &str, position: [f32; 3], volume: f32, pitch: f32, looping: bool, bus: BusId, handle: SoundHandle) -> bool;
    fn set_source_position(&mut self, handle: SoundHandle, position: [f32; 3]);
    fn set_listener_transform(&mut self, position: [f32; 3], orientation: [f32; 4]);
    fn stop(&mut self, handle: SoundHandle);
    fn stop_all(&mut self);
    fn set_bus_volume(&mut self, bus: BusId, volume: f32);
    fn duck_bus(&mut self, bus: BusId, amount: f32, duration_secs: f32, now: f64);
    fn set_source_occlusion(&mut self, handle: SoundHandle, occluded: bool);
    fn tick(&mut self, now: f64);
}

/// Fuente 3D viva en el backend Kira: su pista espacial, el sonido,
/// el filtro low-pass de oclusión y los datos para reescalar su
/// volumen cuando cambia el bus.
struct SpatialSource {
    track: SpatialTrackHandle,
    sound: StaticSoundHandle,
    filter: kira::effect::filter::FilterHandle,
    bus: BusId,
    base_volume: f32,
}

pub struct KiraBackend {
    manager: AudioManager<DefaultBackend>,
    listener: ListenerHandle,
    clips: HashMap<String, StaticSoundData>,
    active_2d: HashMap<SoundHandle, StaticSoundHandle>,
    active_3d: HashMap<SoundHandle, SpatialSource>,
    music_track: kira::track::TrackHandle,
    sfx_track: kira::track::TrackHandle,
    mixer: BusMixer,
    last_effective: [f32; 3],
}

/// Amplitud lineal (0.0..=1.0) a decibelios, el mismo criterio que
/// usa el backend al configurar los sonidos.
fn amplitud_a_db(v: f32) -> Decibels {
    if v <= 0.0001 {
        Decibels::SILENCE
    } else {
        Decibels(20.0 * v.log10())
    }
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

        let music_track = manager
            .add_sub_track(TrackBuilder::new())
            .map_err(|e| format!("Fallo al crear el bus Music: {:?}", e))?;
        let sfx_track = manager
            .add_sub_track(TrackBuilder::new())
            .map_err(|e| format!("Fallo al crear el bus SFX: {:?}", e))?;

        Ok(Self {
            manager,
            listener,
            clips: HashMap::new(),
            active_2d: HashMap::new(),
            active_3d: HashMap::new(),
            music_track,
            sfx_track,
            mixer: BusMixer::new(),
            last_effective: [1.0, 1.0, 1.0],
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

    fn play_2d(&mut self, name: &str, volume: f32, pitch: f32, looping: bool, bus: BusId, handle: SoundHandle) -> bool {
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

        let resultado = match bus {
            BusId::Music => self.music_track.play(configured),
            BusId::Sfx => self.sfx_track.play(configured),
            BusId::Master => self.manager.play(configured),
        };
        match resultado {
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

    fn play_3d(&mut self, name: &str, position: [f32; 3], volume: f32, pitch: f32, looping: bool, bus: BusId, handle: SoundHandle) -> bool {
        self.active_3d.retain(|_, src| src.sound.state() != PlaybackState::Stopped);
        let sound_data = match self.clips.get(name) {
            Some(d) => d.clone(),
            None => {
                log::warn!("glass_audio: Clip '{}' no encontrado para reproducir en 3D", name);
                return false;
            }
        };

        // El bus no es una pista para las fuentes 3D: su volumen
        // efectivo escala el del sonido al nacer (el tick lo mantiene).
        let volumen_efectivo = volume * self.last_effective[bus.index()];
        let db = amplitud_a_db(volumen_efectivo);

        let mut configured = sound_data
            .volume(db)
            .playback_rate(PlaybackRate(pitch.max(0.05) as f64));

        if looping {
            configured = configured.loop_region(..);
        }

        // Create spatial sub-track linked to listener, con un low-pass
        // por fuente para la oclusión (Fase 3): nace abierto a 18 kHz.
        let mut track_builder = SpatialTrackBuilder::new();
        let filter = track_builder.add_effect(
            FilterBuilder::new()
                .mode(FilterMode::LowPass)
                .cutoff(18_000.0),
        );
        match self.manager.add_spatial_sub_track(&self.listener, position, track_builder) {
            Ok(mut spatial_track) => {
                match spatial_track.play(configured) {
                    Ok(sound_handle) => {
                        self.active_3d.insert(handle, SpatialSource {
                            track: spatial_track,
                            sound: sound_handle,
                            filter,
                            bus,
                            base_volume: volume,
                        });
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
        if let Some(src) = self.active_3d.get_mut(&handle) {
            src.track.set_position(position, Tween::default());
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
        if let Some(mut src) = self.active_3d.remove(&handle) {
            src.sound.stop(Tween::default());
        }
    }

    fn stop_all(&mut self) {
        for (_, mut sound_handle) in self.active_2d.drain() {
            sound_handle.stop(Tween::default());
        }
        for (_, mut src) in self.active_3d.drain() {
            src.sound.stop(Tween::default());
        }
    }

    fn set_bus_volume(&mut self, bus: BusId, volume: f32) {
        self.mixer.set_volume(bus, volume);
    }

    fn duck_bus(&mut self, bus: BusId, amount: f32, duration_secs: f32, now: f64) {
        self.mixer.duck(bus, amount, duration_secs, now);
    }

    fn set_source_occlusion(&mut self, handle: SoundHandle, occluded: bool) {
        if let Some(src) = self.active_3d.get_mut(&handle) {
            let cutoff = if occluded { 700.0 } else { 18_000.0 };
            src.filter.set_cutoff(cutoff, Tween {
                duration: std::time::Duration::from_millis(150),
                ..Default::default()
            });
        }
    }

    fn tick(&mut self, now: f64) {
        let efectivos = [
            self.mixer.effective(BusId::Master, now),
            self.mixer.effective(BusId::Music, now),
            self.mixer.effective(BusId::Sfx, now),
        ];
        for (i, bus) in [BusId::Master, BusId::Music, BusId::Sfx].into_iter().enumerate() {
            if (efectivos[i] - self.last_effective[i]).abs() <= 0.001 {
                continue;
            }
            self.last_effective[i] = efectivos[i];
            let db = amplitud_a_db(efectivos[i]);
            let tween = Tween {
                duration: std::time::Duration::from_millis(50),
                ..Default::default()
            };
            match bus {
                BusId::Master => self.manager.main_track().set_volume(db, tween),
                BusId::Music => self.music_track.set_volume(db, tween),
                BusId::Sfx => self.sfx_track.set_volume(db, tween),
            }
        }
        for src in self.active_3d.values_mut() {
            let objetivo = src.base_volume * efectivos[src.bus.index()];
            src.sound.set_volume(amplitud_a_db(objetivo), Tween::default());
        }
    }
}

pub struct NullBackend {
    clips: HashMap<String, usize>,
    active_sources: HashMap<SoundHandle, [f32; 3]>,
    listener_pos: [f32; 3],
    listener_rot: [f32; 4],
    mixer: BusMixer,
    occluded: HashMap<SoundHandle, bool>,
}

impl NullBackend {
    pub fn new() -> Self {
        Self {
            clips: HashMap::new(),
            active_sources: HashMap::new(),
            listener_pos: [0.0; 3],
            listener_rot: [0.0, 0.0, 0.0, 1.0],
            mixer: BusMixer::new(),
            occluded: HashMap::new(),
        }
    }
}

impl AudioBackend for NullBackend {
    fn load_clip(&mut self, name: &str, wav_bytes: Vec<u8>) -> bool {
        self.clips.insert(name.to_string(), wav_bytes.len());
        true
    }

    fn play_2d(&mut self, _name: &str, _volume: f32, _pitch: f32, _looping: bool, _bus: BusId, handle: SoundHandle) -> bool {
        self.active_sources.insert(handle, [0.0, 0.0, 0.0]);
        true
    }

    fn play_3d(&mut self, _name: &str, position: [f32; 3], _volume: f32, _pitch: f32, _looping: bool, _bus: BusId, handle: SoundHandle) -> bool {
        self.active_sources.insert(handle, position);
        self.occluded.insert(handle, false);
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
        self.occluded.remove(&handle);
    }

    fn stop_all(&mut self) {
        self.active_sources.clear();
        self.occluded.clear();
    }

    fn set_bus_volume(&mut self, bus: BusId, volume: f32) {
        self.mixer.set_volume(bus, volume);
    }

    fn duck_bus(&mut self, bus: BusId, amount: f32, duration_secs: f32, now: f64) {
        self.mixer.duck(bus, amount, duration_secs, now);
    }

    fn set_source_occlusion(&mut self, handle: SoundHandle, occluded: bool) {
        self.occluded.insert(handle, occluded);
    }

    fn tick(&mut self, _now: f64) {}
}
