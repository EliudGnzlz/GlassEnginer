//! Estadísticas de la resolución de transforms (Fase 7): espejo
//! global de la última pasada de `TransformStore`, con el mismo
//! patrón que `render_stats` y `lighting_stats` — la app (o el
//! editor) resuelve, y cualquier consumidor (p. ej. el servidor
//! MCP) lee la foto sin tocar la escena.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static ENTITIES: AtomicU32 = AtomicU32::new(0);
static RECOMPUTED: AtomicU32 = AtomicU32::new(0);
static MAX_DEPTH: AtomicU32 = AtomicU32::new(0);
static LAST_MICROS: AtomicU64 = AtomicU64::new(0);
static TOTAL_PASSES: AtomicU64 = AtomicU64::new(0);

/// Registra una pasada de resolución de mundos.
pub fn record_pass(entities: u32, recomputed: u32, max_depth: u32, micros: u64) {
    ENTITIES.store(entities, Ordering::Relaxed);
    RECOMPUTED.store(recomputed, Ordering::Relaxed);
    MAX_DEPTH.store(max_depth, Ordering::Relaxed);
    LAST_MICROS.store(micros, Ordering::Relaxed);
    TOTAL_PASSES.fetch_add(1, Ordering::Relaxed);
}

/// Foto de la última pasada registrada.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransformStatsSnapshot {
    pub entities: u32,
    pub recomputed: u32,
    pub max_depth: u32,
    pub last_micros: u64,
    pub total_passes: u64,
}

pub fn snapshot() -> TransformStatsSnapshot {
    TransformStatsSnapshot {
        entities: ENTITIES.load(Ordering::Relaxed),
        recomputed: RECOMPUTED.load(Ordering::Relaxed),
        max_depth: MAX_DEPTH.load(Ordering::Relaxed),
        last_micros: LAST_MICROS.load(Ordering::Relaxed),
        total_passes: TOTAL_PASSES.load(Ordering::Relaxed),
    }
}
