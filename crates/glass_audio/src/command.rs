use crossbeam_queue::ArrayQueue;

use crate::buses::BusId;

pub type SoundHandle = u64;

#[derive(Debug)]
pub enum AudioCommand {
    LoadClip {
        name: String,
        wav_bytes: Vec<u8>,
    },
    Play2D {
        name: String,
        volume: f32,
        pitch: f32,
        looping: bool,
        bus: BusId,
        handle: SoundHandle,
    },
    Play3D {
        name: String,
        position: [f32; 3],
        volume: f32,
        pitch: f32,
        looping: bool,
        bus: BusId,
        handle: SoundHandle,
    },
    SetSourcePosition {
        handle: SoundHandle,
        position: [f32; 3],
    },
    SetListenerTransform {
        position: [f32; 3],
        orientation: [f32; 4],
    },
    Stop {
        handle: SoundHandle,
    },
    /// Pausa una fuente sin retirarla; se reanuda con `Resume`.
    Pause {
        handle: SoundHandle,
    },
    /// Reanuda una fuente pausada con `Pause`.
    Resume {
        handle: SoundHandle,
    },
    SetBusVolume {
        bus: BusId,
        volume: f32,
    },
    DuckBus {
        bus: BusId,
        amount: f32,
        duration_secs: f32,
    },
    SetSourceOcclusion {
        handle: SoundHandle,
        occluded: bool,
    },
    StopAll,
}

/// Thread-safe lock-free SPSC / MPSC command queue for audio operations.
pub struct CommandQueue {
    inner: ArrayQueue<AudioCommand>,
}

impl CommandQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: ArrayQueue::new(capacity),
        }
    }

    /// Pushes an audio command non-blockingly. If the queue is full, drops the command with a warning.
    pub fn push(&self, cmd: AudioCommand) -> bool {
        if let Err(dropped) = self.inner.push(cmd) {
            log::warn!("glass_audio: Command queue llena, descartando comando: {:?}", dropped);
            false
        } else {
            true
        }
    }

    pub fn pop(&self) -> Option<AudioCommand> {
        self.inner.pop()
    }
}
