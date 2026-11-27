use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use crate::backend::{AudioBackend, KiraBackend, NullBackend};
use crate::buses::{BusId, BusMixer, BusSnapshot};
use crate::command::{AudioCommand, CommandQueue, SoundHandle};
use crate::procedural::{beep_440, beep_880};

/// Espejo en el hilo principal de una fuente viva: lo mantiene el
/// propio AudioSystem al encolar comandos, para consultar buses y
/// oclusión sin pasar por la cola de audio.
#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub bus: BusId,
    pub position: Option<[f32; 3]>,
    pub occluded: bool,
}

pub struct AudioSystem {
    command_queue: Arc<CommandQueue>,
    next_handle: Arc<AtomicU64>,
    shutdown_flag: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
    pub is_null_backend: bool,
    mixer: Arc<Mutex<BusMixer>>,
    sources: Arc<Mutex<HashMap<SoundHandle, SourceInfo>>>,
    clock_start: Instant,
}

impl Default for AudioSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSystem {
    pub fn new() -> Self {
        let command_queue = Arc::new(CommandQueue::new(512));
        let next_handle = Arc::new(AtomicU64::new(1));
        let shutdown_flag = Arc::new(AtomicBool::new(false));

        let queue_clone = command_queue.clone();
        let shutdown_clone = shutdown_flag.clone();

        // Attempt to initialize Kira backend with CPAL
        let (mut backend, is_null): (Box<dyn AudioBackend>, bool) = match KiraBackend::try_new() {
            Ok(kira) => {
                log::info!("glass_audio: Backend Kira (CPAL) inicializado exitosamente.");
                (Box::new(kira), false)
            }
            Err(e) => {
                log::warn!("glass_audio: Dispositivo de audio hardware no disponible ({}). Usando NullBackend headless.", e);
                (Box::new(NullBackend::new()), true)
            }
        };

        // Preload standard procedural tones
        backend.load_clip("beep_440", beep_440());
        backend.load_clip("beep_880", beep_880());

        // Spawn high-priority decoupled audio thread
        let mixer = Arc::new(Mutex::new(BusMixer::new()));
        let sources: Arc<Mutex<HashMap<SoundHandle, SourceInfo>>> = Arc::new(Mutex::new(HashMap::new()));
        let clock_start = Instant::now();
        let backend_clock = Instant::now();
        let thread_handle = thread::Builder::new()
            .name("glass_audio_thread".to_string())
            .spawn(move || {
                log::info!("glass_audio: Hilo de audio desacoplado iniciado.");
                while !shutdown_clone.load(Ordering::Relaxed) {
                    let mut processed = 0;
                    while let Some(cmd) = queue_clone.pop() {
                        processed += 1;
                        match cmd {
                            AudioCommand::LoadClip { name, wav_bytes } => {
                                backend.load_clip(&name, wav_bytes);
                            }
                            AudioCommand::Play2D { name, volume, pitch, looping, bus, handle } => {
                                backend.play_2d(&name, volume, pitch, looping, bus, handle);
                            }
                            AudioCommand::Play3D { name, position, volume, pitch, looping, bus, handle } => {
                                backend.play_3d(&name, crate::backend::Play3dParams { position, volume, pitch, looping, bus }, handle);
                            }
                            AudioCommand::SetSourcePosition { handle, position } => {
                                backend.set_source_position(handle, position);
                            }
                            AudioCommand::SetListenerTransform { position, orientation } => {
                                backend.set_listener_transform(position, orientation);
                            }
                            AudioCommand::Pause { handle } => {
                                backend.pause(handle);
                            }
                            AudioCommand::Resume { handle } => {
                                backend.resume(handle);
                            }
                            AudioCommand::Stop { handle } => {
                                backend.stop(handle);
                            }
                            AudioCommand::StopAll => {
                                backend.stop_all();
                            }
                            AudioCommand::SetBusVolume { bus, volume } => {
                                backend.set_bus_volume(bus, volume);
                            }
                            AudioCommand::DuckBus { bus, amount, duration_secs } => {
                                backend.duck_bus(bus, amount, duration_secs, backend_clock.elapsed().as_secs_f64());
                            }
                            AudioCommand::SetSourceOcclusion { handle, occluded } => {
                                backend.set_source_occlusion(handle, occluded);
                            }
                        }
                    }

                    backend.tick(backend_clock.elapsed().as_secs_f64());
                    if processed == 0 {
                        thread::sleep(Duration::from_millis(5));
                    }
                }
                backend.stop_all();
                log::info!("glass_audio: Hilo de audio desacoplado finalizado.");
            })
            .expect("Fallo al crear hilo de audio");

        Self {
            command_queue,
            next_handle,
            shutdown_flag,
            thread_handle: Some(thread_handle),
            is_null_backend: is_null,
            mixer,
            sources,
            clock_start,
        }
    }

    fn generate_handle(&self) -> SoundHandle {
        self.next_handle.fetch_add(1, Ordering::Relaxed)
    }

    pub fn load_clip(&self, name: impl Into<String>, wav_bytes: Vec<u8>) {
        self.command_queue.push(AudioCommand::LoadClip {
            name: name.into(),
            wav_bytes,
        });
    }

    pub fn play_2d(&self, name: impl Into<String>, volume: f32, pitch: f32, looping: bool) -> SoundHandle {
        self.play_2d_on_bus(name, volume, pitch, looping, BusId::Sfx)
    }

    pub fn play_2d_on_bus(&self, name: impl Into<String>, volume: f32, pitch: f32, looping: bool, bus: BusId) -> SoundHandle {
        let handle = self.generate_handle();
        self.command_queue.push(AudioCommand::Play2D {
            name: name.into(),
            volume,
            pitch,
            looping,
            bus,
            handle,
        });
        if let Ok(mut sources) = self.sources.lock() {
            sources.insert(handle, SourceInfo { bus, position: None, occluded: false });
        }
        handle
    }

    pub fn play_3d(&self, name: impl Into<String>, position: [f32; 3], volume: f32, pitch: f32, looping: bool) -> SoundHandle {
        self.play_3d_on_bus(name, position, volume, pitch, looping, BusId::Sfx)
    }

    pub fn play_3d_on_bus(&self, name: impl Into<String>, position: [f32; 3], volume: f32, pitch: f32, looping: bool, bus: BusId) -> SoundHandle {
        let handle = self.generate_handle();
        self.command_queue.push(AudioCommand::Play3D {
            name: name.into(),
            position,
            volume,
            pitch,
            looping,
            bus,
            handle,
        });
        if let Ok(mut sources) = self.sources.lock() {
            sources.insert(handle, SourceInfo { bus, position: Some(position), occluded: false });
        }
        handle
    }

    pub fn set_source_position(&self, handle: SoundHandle, position: [f32; 3]) {
        self.command_queue.push(AudioCommand::SetSourcePosition { handle, position });
        if let Ok(mut sources) = self.sources.lock() {
            if let Some(info) = sources.get_mut(&handle) {
                if info.position.is_some() {
                    info.position = Some(position);
                }
            }
        }
    }

    pub fn set_listener_transform(&self, position: [f32; 3], orientation: [f32; 4]) {
        self.command_queue.push(AudioCommand::SetListenerTransform { position, orientation });
    }

    pub fn stop(&self, handle: SoundHandle) {
        self.command_queue.push(AudioCommand::Stop { handle });
        if let Ok(mut sources) = self.sources.lock() {
            sources.remove(&handle);
        }
    }

    /// Pausa la fuente. Sigue registrada en `sources`: pausar no la retira.
    pub fn pause(&self, handle: SoundHandle) {
        self.command_queue.push(AudioCommand::Pause { handle });
    }

    /// Reanuda una fuente pausada.
    pub fn resume(&self, handle: SoundHandle) {
        self.command_queue.push(AudioCommand::Resume { handle });
    }

    pub fn stop_all(&self) {
        self.command_queue.push(AudioCommand::StopAll);
        if let Ok(mut sources) = self.sources.lock() {
            sources.clear();
        }
    }

    /// Fija el volumen base de un bus (0.0..=1.0) y lo refleja en el espejo.
    pub fn set_bus_volume(&self, bus: BusId, volume: f32) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.set_volume(bus, volume);
        }
        self.command_queue.push(AudioCommand::SetBusVolume { bus, volume });
    }

    /// Dispara el ducking de un bus: baja su volumen efectivo al factor
    /// `amount` durante `duration_secs` segundos.
    pub fn duck_bus(&self, bus: BusId, amount: f32, duration_secs: f32) {
        let now = self.clock_start.elapsed().as_secs_f64();
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.duck(bus, amount, duration_secs, now);
        }
        self.command_queue.push(AudioCommand::DuckBus { bus, amount, duration_secs });
    }

    /// Marca la oclusión de una fuente 3D (low-pass en el backend).
    pub fn set_source_occlusion(&self, handle: SoundHandle, occluded: bool) {
        if let Ok(mut sources) = self.sources.lock() {
            if let Some(info) = sources.get_mut(&handle) {
                info.occluded = occluded;
            }
        }
        self.command_queue.push(AudioCommand::SetSourceOcclusion { handle, occluded });
    }

    /// Foto de los tres buses según el espejo del manager.
    pub fn bus_snapshot(&self) -> [BusSnapshot; 3] {
        let now = self.clock_start.elapsed().as_secs_f64();
        match self.mixer.lock() {
            Ok(mixer) => mixer.snapshot(now),
            Err(_) => BusMixer::new().snapshot(now),
        }
    }

    /// Fuentes 3D vivas según el espejo: (handle, posición).
    pub fn live_3d_sources(&self) -> Vec<(SoundHandle, [f32; 3])> {
        match self.sources.lock() {
            Ok(sources) => sources
                .iter()
                .filter_map(|(h, info)| info.position.map(|p| (*h, p)))
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Copia del espejo de fuentes vivas para inspección (MCP).
    pub fn sources_snapshot(&self) -> Vec<(SoundHandle, SourceInfo)> {
        match self.sources.lock() {
            Ok(sources) => sources.iter().map(|(h, info)| (*h, info.clone())).collect(),
            Err(_) => Vec::new(),
        }
    }
}

impl Drop for AudioSystem {
    fn drop(&mut self) {
        self.shutdown_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}
