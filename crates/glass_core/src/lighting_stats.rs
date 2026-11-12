use std::sync::{Mutex, MutexGuard, OnceLock};

/// Luz puntual publicada en el UBO del último frame (Fase 2S).
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct PointLightSnapshot {
    pub position: [f32; 3],
    pub range: f32,
    pub color: [f32; 3],
    pub intensity: f32,
    /// Índice de cubo en el atlas de sombras puntuales; -1 = sin sombra.
    pub shadow_cube: i32,
}

/// Estadísticas de iluminación del último frame (Fase 2S): luz direccional,
/// luces puntuales publicadas en el UBO y niebla de la escena. Las rellena
/// la app y las lee la tool MCP glass_render_lighting_debug.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct LightingStatsSnapshot {
    pub has_directional: bool,
    pub directional_dir: [f32; 3],
    pub directional_color: [f32; 3],
    /// Nº de luces puntuales válidas en `points` [0..8].
    pub point_count: u32,
    pub points: [PointLightSnapshot; 8],
    pub fog_enabled: bool,
    pub fog_color: [f32; 3],
    pub fog_near: f32,
    pub fog_far: f32,
}

static STATE: OnceLock<Mutex<LightingStatsSnapshot>> = OnceLock::new();

fn state() -> &'static Mutex<LightingStatsSnapshot> {
    STATE.get_or_init(|| Mutex::new(LightingStatsSnapshot::default()))
}

fn lock_state() -> MutexGuard<'static, LightingStatsSnapshot> {
    state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn record_lighting_stats(stats: LightingStatsSnapshot) {
    *lock_state() = stats;
}

pub fn lighting_stats_snapshot() -> LightingStatsSnapshot {
    *lock_state()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_then_snapshot_returns_stored_value() {
        let mut points = [PointLightSnapshot::default(); 8];
        points[0] = PointLightSnapshot {
            position: [1.0, 2.0, 3.0],
            range: 5.0,
            color: [1.0, 0.0, 0.0],
            intensity: 2.0,
            shadow_cube: 0,
        };
        points[1] = PointLightSnapshot {
            position: [-1.0, 0.5, 2.0],
            range: 4.0,
            color: [0.0, 1.0, 0.0],
            intensity: 1.5,
            shadow_cube: -1,
        };

        let stats = LightingStatsSnapshot {
            has_directional: true,
            directional_dir: [0.0, -1.0, 0.0],
            directional_color: [1.0, 0.9, 0.8],
            point_count: 2,
            points,
            fog_enabled: true,
            fog_color: [0.2, 0.3, 0.4],
            fog_near: 1.0,
            fog_far: 50.0,
        };

        record_lighting_stats(stats);
        let snapshot = lighting_stats_snapshot();
        assert!(snapshot.has_directional);
        assert_eq!(snapshot.point_count, 2);
        assert_eq!(snapshot.points[0].shadow_cube, 0);
        assert_eq!(snapshot.fog_far, 50.0);
    }
}
