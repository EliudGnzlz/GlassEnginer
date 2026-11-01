use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use crate::backend::{AudioBackend, KiraBackend, NullBackend};
use crate::command::{AudioCommand, CommandQueue, SoundHandle};
use crate::procedural::{beep_440, beep_880};

pub struct AudioSystem {
    command_queue: Arc<CommandQueue>,
    next_handle: Arc<AtomicU64>,
    shutdown_flag: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
    pub is_null_backend: bool,
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
                            AudioCommand::Play2D { name, volume, pitch, looping, handle } => {
                                backend.play_2d(&name, volume, pitch, looping, handle);
                            }
                            AudioCommand::Play3D { name, position, volume, pitch, looping, handle } => {
                                backend.play_3d(&name, position, volume, pitch, looping, handle);
                            }
                            AudioCommand::SetSourcePosition { handle, position } => {
                                backend.set_source_position(handle, position);
                            }
                            AudioCommand::SetListenerTransform { position, orientation } => {
                                backend.set_listener_transform(position, orientation);
                            }
                            AudioCommand::Stop { handle } => {
                                backend.stop(handle);
                            }
                            AudioCommand::StopAll => {
                                backend.stop_all();
                            }
                        }
                    }

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
        let handle = self.generate_handle();
        self.command_queue.push(AudioCommand::Play2D {
            name: name.into(),
            volume,
            pitch,
            looping,
            handle,
        });
        handle
    }

    pub fn play_3d(&self, name: impl Into<String>, position: [f32; 3], volume: f32, pitch: f32, looping: bool) -> SoundHandle {
        let handle = self.generate_handle();
        self.command_queue.push(AudioCommand::Play3D {
            name: name.into(),
            position,
            volume,
            pitch,
            looping,
            handle,
        });
        handle
    }

    pub fn set_source_position(&self, handle: SoundHandle, position: [f32; 3]) {
        self.command_queue.push(AudioCommand::SetSourcePosition { handle, position });
    }

    pub fn set_listener_transform(&self, position: [f32; 3], orientation: [f32; 4]) {
        self.command_queue.push(AudioCommand::SetListenerTransform { position, orientation });
    }

    pub fn stop(&self, handle: SoundHandle) {
        self.command_queue.push(AudioCommand::Stop { handle });
    }

    pub fn stop_all(&self) {
        self.command_queue.push(AudioCommand::StopAll);
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
