//! Instrumentación de Fase 0: timers por fase de frame, memoria de GPU y de proceso.
//!
//! El objetivo es medir sin coste apreciable: el registro de frames usa un buffer
//! circular de tamaño fijo reservado una sola vez, y las estadísticas se calculan
//! únicamente cuando alguien pide un snapshot (HUD, telemetría, tests).

use std::sync::{Mutex, MutexGuard, OnceLock};

use serde::Serialize;

/// Número de frames que se conservan para las estadísticas. Suficiente para una
/// ventana de ~4 s a 60 fps sin que el coste de ordenar crezca.
const FRAME_SAMPLES_CAP: usize = 240;

// ---------------------------------------------------------------------------
// Muestras de frame
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
pub struct FramePhaseSample {
    pub update_us: u64, pub physics_us: u64, pub render_us: u64,
    pub present_us: u64, pub total_us: u64, pub gpu_render_us: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct PhaseStats { pub avg_us: f64, pub p95_us: f64, pub max_us: u64 }

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct FrameTimingSnapshot {
    pub samples: usize,
    pub total: PhaseStats, pub update: PhaseStats, pub physics: PhaseStats,
    pub render: PhaseStats, pub present: PhaseStats,
    pub gpu_render: PhaseStats, pub gpu_samples: usize,
    pub last_gpu_render_us: Option<u64>,
    /// Antigüedad de `last_gpu_render_us` en frames dentro de la
    /// ventana (0 = es del último frame). Auditoría Fase 0 (menor):
    /// sin esto, una marca vieja se leía como actual.
    pub last_gpu_age_frames: Option<usize>,
}

/// Buffer circular de muestras en orden cronológico.
///
/// Se implementa con un `Vec` contiguo (no `VecDeque`) para poder pasar un slice
/// directo a `compute_snapshot` y poder testear las estadísticas sin tocar el
/// registro global. Al estar lleno, desplazar 239 elementos es un memmove trivial
/// comparado con el coste de un frame, y evita cualquier realloc.
struct FrameBuffer {
    samples: Vec<FramePhaseSample>,
}

impl FrameBuffer {
    fn with_capacity(capacity: usize) -> Self {
        Self { samples: Vec::with_capacity(capacity) }
    }

    fn push(&mut self, sample: FramePhaseSample) {
        if self.samples.len() >= FRAME_SAMPLES_CAP {
            // Descartamos la muestra más antigua sin tocar la capacidad reservada.
            self.samples.copy_within(1.., 0);
            self.samples.pop();
        }
        self.samples.push(sample);
    }

    fn snapshot(&self) -> FrameTimingSnapshot {
        compute_snapshot(&self.samples)
    }
}

/// Calcula avg/p95/max sobre un conjunto de valores ya materializado.
///
/// p95 = percentil con índice `ceil(0.95 * n) - 1` (en enteros, para no depender
/// de la precisión de coma flotante). Un slice vacío devuelve todo a cero.
fn compute_phase_stats(values: &mut [u64]) -> PhaseStats {
    let n = values.len();
    if n == 0 {
        return PhaseStats::default();
    }

    let avg_us = values.iter().map(|&v| v as f64).sum::<f64>() / n as f64;

    values.sort_unstable();
    let rank = (95 * n + 99) / 100; // ceil(0.95 * n) en aritmética entera
    let idx = rank.saturating_sub(1).min(n - 1);

    PhaseStats {
        avg_us,
        p95_us: values[idx] as f64,
        max_us: values[n - 1],
    }
}

/// Función pura: deriva el snapshot completo a partir de un slice de muestras.
/// Al no depender del estado global se puede testear de forma determinista.
fn compute_snapshot(samples: &[FramePhaseSample]) -> FrameTimingSnapshot {
    let n = samples.len();

    let mut total = Vec::with_capacity(n);
    let mut update = Vec::with_capacity(n);
    let mut physics = Vec::with_capacity(n);
    let mut render = Vec::with_capacity(n);
    let mut present = Vec::with_capacity(n);
    // El número de muestras con GPU no se conoce a priori: dejamos que crezca.
    let mut gpu = Vec::new();
    let mut last_gpu = None;
    let mut last_gpu_idx: Option<usize> = None;

    for (i, sample) in samples.iter().enumerate() {
        total.push(sample.total_us);
        update.push(sample.update_us);
        physics.push(sample.physics_us);
        render.push(sample.render_us);
        present.push(sample.present_us);
        if let Some(gpu_us) = sample.gpu_render_us {
            gpu.push(gpu_us);
            // Al recorrer en orden cronológico, el último que asignemos es el más reciente.
            last_gpu = Some(gpu_us);
            last_gpu_idx = Some(i);
        }
    }

    let gpu_render = compute_phase_stats(&mut gpu);

    FrameTimingSnapshot {
        samples: n,
        total: compute_phase_stats(&mut total),
        update: compute_phase_stats(&mut update),
        physics: compute_phase_stats(&mut physics),
        render: compute_phase_stats(&mut render),
        present: compute_phase_stats(&mut present),
        gpu_render,
        gpu_samples: gpu.len(),
        last_gpu_render_us: last_gpu,
        last_gpu_age_frames: last_gpu_idx.map(|i| n.saturating_sub(1 + i)),
    }
}

// ---------------------------------------------------------------------------
// Estado global
// ---------------------------------------------------------------------------

struct ProfilingState {
    frames: FrameBuffer,
    gpu_memory: Option<GpuMemorySnapshot>,
}

static STATE: OnceLock<Mutex<ProfilingState>> = OnceLock::new();

/// Inicialización perezosa: el buffer reserva su capacidad exacta una sola vez,
/// antes de que nadie registre frames, para que `record_frame_timing` no aloque.
fn state() -> &'static Mutex<ProfilingState> {
    STATE.get_or_init(|| {
        Mutex::new(ProfilingState {
            frames: FrameBuffer::with_capacity(FRAME_SAMPLES_CAP),
            gpu_memory: None,
        })
    })
}

/// Un pánico ocurrido con el lock tomado no debe dejar la telemetría inservible:
/// recuperamos el estado interno envenenado en lugar de propagar el pánico.
fn lock_state() -> MutexGuard<'static, ProfilingState> {
    state().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn record_frame_timing(sample: FramePhaseSample) {
    lock_state().frames.push(sample);
}

pub fn frame_timing_snapshot() -> FrameTimingSnapshot {
    lock_state().frames.snapshot()
}

// ---------------------------------------------------------------------------
// Memoria de GPU
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct GpuMemorySnapshot { pub allocated_bytes: u64, pub capacity_bytes: u64 }

pub fn record_gpu_memory(allocated_bytes: u64, capacity_bytes: u64) {
    lock_state().gpu_memory = Some(GpuMemorySnapshot { allocated_bytes, capacity_bytes });
}

pub fn gpu_memory_snapshot() -> Option<GpuMemorySnapshot> {
    // `Option<GpuMemorySnapshot>` es `Copy`: el lock se libera al salir.
    lock_state().gpu_memory
}

// ---------------------------------------------------------------------------
// Memoria de proceso
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct ProcessMemorySnapshot { pub rss_bytes: Option<u64>, pub peak_rss_bytes: Option<u64> }

pub fn process_memory_snapshot() -> ProcessMemorySnapshot {
    #[cfg(target_os = "linux")]
    {
        // `/proc/self/status` es la fuente más barata y siempre disponible en Linux.
        match std::fs::read_to_string("/proc/self/status") {
            Ok(text) => {
                let (rss_bytes, peak_rss_bytes) = parse_status_memory(&text);
                ProcessMemorySnapshot { rss_bytes, peak_rss_bytes }
            }
            // Sin /proc (contenedores restringidos, sandboxes...) devolvemos "desconocido".
            Err(_) => ProcessMemorySnapshot::default(),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Fuera de Linux no hay fuente portátil equivalente en Fase 0.
        ProcessMemorySnapshot::default()
    }
}

/// Parsea `VmRSS:` (residente actual) y `VmHWM:` (pico residente) de
/// `/proc/self/status`. Ambas líneas vienen en kB; se convierten a bytes.
/// Es una función pura para poder probarla sin depender del proceso real.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // testeable en cualquier plataforma
fn parse_status_memory(text: &str) -> (Option<u64>, Option<u64>) {
    let mut rss_bytes = None;
    let mut peak_bytes = None;

    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else { continue };
        // Formato: "\tVmRSS:\t  123456 kB" -> primer token tras ':' es el número.
        let kb = rest
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<u64>().ok());
        let bytes = kb.map(|kb| kb.saturating_mul(1024));

        match key.trim() {
            "VmRSS" => rss_bytes = bytes,
            "VmHWM" => peak_bytes = bytes,
            _ => {}
        }
    }

    (rss_bytes, peak_bytes)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Muestra sintética donde todas las fases valen `v`.
    fn muestra(v: u64) -> FramePhaseSample {
        FramePhaseSample {
            update_us: v, physics_us: v, render_us: v, present_us: v, total_us: v,
            gpu_render_us: None,
        }
    }

    #[test]
    fn estadisticas_con_valores_conocidos() {
        // 20 muestras con total 1..=20: avg 10.5; p95 -> ceil(19)-1 = idx 18 -> 19; max 20.
        // GPU solo en las pares: 4, 8, ..., 40 (10 muestras) -> avg 22, p95 idx 9 -> 40.
        let samples: Vec<FramePhaseSample> = (1..=20u64)
            .map(|v| FramePhaseSample {
                gpu_render_us: if v % 2 == 0 { Some(v * 2) } else { None },
                ..muestra(v)
            })
            .collect();

        let snap = compute_snapshot(&samples);
        assert_eq!(snap.samples, 20);
        assert!((snap.total.avg_us - 10.5).abs() < 1e-9);
        assert!((snap.total.p95_us - 19.0).abs() < 1e-9);
        assert_eq!(snap.total.max_us, 20);

        assert_eq!(snap.gpu_samples, 10);
        assert_eq!(snap.last_gpu_render_us, Some(40));
        // La última muestra GPU es la del índice 19 (v=20): edad 0.
        assert_eq!(snap.last_gpu_age_frames, Some(0));
        assert!((snap.gpu_render.avg_us - 22.0).abs() < 1e-9);
        assert!((snap.gpu_render.p95_us - 40.0).abs() < 1e-9);
        assert_eq!(snap.gpu_render.max_us, 40);

        // Si la GPU solo aparece en la primera de 5, su edad es 4.
        let samples: Vec<FramePhaseSample> = (0..5u64)
            .map(|i| FramePhaseSample {
                gpu_render_us: if i == 0 { Some(99) } else { None },
                ..muestra(10)
            })
            .collect();
        let snap = compute_snapshot(&samples);
        assert_eq!(snap.last_gpu_render_us, Some(99));
        assert_eq!(snap.last_gpu_age_frames, Some(4));
    }

    #[test]
    fn buffer_circular_conserva_las_ultimas_240() {
        // Buffer local: no comparte estado con el registro global ni con otros tests.
        let mut buffer = FrameBuffer::with_capacity(FRAME_SAMPLES_CAP);
        for i in 0..300u64 {
            buffer.push(muestra(i));
        }

        assert_eq!(buffer.samples.len(), FRAME_SAMPLES_CAP);
        assert_eq!(buffer.samples[0].total_us, 60); // 300 - 240: la más antigua superviviente
        assert_eq!(buffer.samples[FRAME_SAMPLES_CAP - 1].total_us, 299);
        assert_eq!(buffer.snapshot().samples, FRAME_SAMPLES_CAP);
    }

    #[test]
    fn parseo_de_proc_status() {
        let text = "Name:\tglass\n\
                    VmPeak:\t  900000 kB\n\
                    VmRSS:\t  123456 kB\n\
                    VmHWM:\t  234567 kB\n\
                    Threads:\t4\n";
        let (rss, peak) = parse_status_memory(text);
        assert_eq!(rss, Some(123_456 * 1024));
        assert_eq!(peak, Some(234_567 * 1024));

        // Sin las líneas esperadas no inventamos valores.
        let (rss, peak) = parse_status_memory("Name:\tglass\nThreads:\t4\n");
        assert_eq!(rss, None);
        assert_eq!(peak, None);
    }

    #[test]
    fn memoria_gpu_ida_y_vuelta() {
        record_gpu_memory(1024, 4096);
        let snap = gpu_memory_snapshot().expect("tras grabar debe existir un snapshot");
        assert_eq!(snap.allocated_bytes, 1024);
        assert_eq!(snap.capacity_bytes, 4096);
    }
}
