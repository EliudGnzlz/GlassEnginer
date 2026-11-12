//! Estadísticas del sistema de partículas (Fase 8P): espejo de
//! `transform_stats`. La app escribe vivas/emisores/coste de
//! simulación/overdraw cada frame y el renderer los draws; las
//! tools MCP lo leen con `snapshot()`.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static ALIVE: AtomicU32 = AtomicU32::new(0);
static EMITTERS: AtomicU32 = AtomicU32::new(0);
static DRAWS: AtomicU32 = AtomicU32::new(0);
static SIM_MICROS: AtomicU64 = AtomicU64::new(0);
static OVERDRAW_MILLI: AtomicU32 = AtomicU32::new(0);

/// Registra el estado del frame (lo llama la app tras simular).
pub fn record_frame(alive: u32, emitters: u32, sim_micros: u64, overdraw: f32) {
    ALIVE.store(alive, Ordering::Relaxed);
    EMITTERS.store(emitters, Ordering::Relaxed);
    SIM_MICROS.store(sim_micros, Ordering::Relaxed);
    OVERDRAW_MILLI.store((overdraw.max(0.0) * 1000.0) as u32, Ordering::Relaxed);
}

/// Registra los draws de partículas del frame (el renderer).
pub fn record_draws(draws: u32) {
    DRAWS.store(draws, Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ParticleStatsSnapshot {
    pub alive: u32,
    pub emitters: u32,
    pub draws: u32,
    pub sim_micros: u64,
    pub overdraw: f32,
}

pub fn snapshot() -> ParticleStatsSnapshot {
    ParticleStatsSnapshot {
        alive: ALIVE.load(Ordering::Relaxed),
        emitters: EMITTERS.load(Ordering::Relaxed),
        draws: DRAWS.load(Ordering::Relaxed),
        sim_micros: SIM_MICROS.load(Ordering::Relaxed),
        overdraw: OVERDRAW_MILLI.load(Ordering::Relaxed) as f32 / 1000.0,
    }
}
