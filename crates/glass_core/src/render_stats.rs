use std::sync::{Mutex, MutexGuard, OnceLock};

/// Estadísticas del último frame recolectado (Fase 2): las rellena la app
/// al construir las listas de dibujo y las lee la tool MCP
/// glass_render_stats.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct RenderStatsSnapshot {
    /// Entidades con malla consideradas para la vista de juego.
    pub entities_considered: u32,
    /// Entidades descartadas por frustum culling en la vista de juego.
    pub entities_culled: u32,
    /// Draw calls emitidos para la vista de juego (lotes por primitiva).
    pub game_draw_calls: u32,
    /// Draw calls emitidos para la vista de escena (editor).
    pub scene_draw_calls: u32,
    /// Draw calls del pase de sombras del sol.
    pub shadow_draw_calls: u32,
}

static STATE: OnceLock<Mutex<RenderStatsSnapshot>> = OnceLock::new();

fn state() -> &'static Mutex<RenderStatsSnapshot> {
    STATE.get_or_init(|| Mutex::new(RenderStatsSnapshot::default()))
}

/// Un pánico ocurrido con el lock tomado no debe dejar la telemetría
/// inservible: recuperamos el estado interno envenenado en lugar de
/// propagar el pánico.
fn lock_state() -> MutexGuard<'static, RenderStatsSnapshot> {
    state().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn record_render_stats(stats: RenderStatsSnapshot) {
    *lock_state() = stats;
}

pub fn render_stats_snapshot() -> RenderStatsSnapshot {
    *lock_state()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_then_snapshot_returns_stored_value() {
        let stats = RenderStatsSnapshot {
            entities_considered: 10,
            entities_culled: 3,
            game_draw_calls: 7,
            scene_draw_calls: 10,
            shadow_draw_calls: 5,
        };

        record_render_stats(stats);
        let snapshot = render_stats_snapshot();

        assert_eq!(snapshot.entities_considered, 10);
        assert_eq!(snapshot.entities_culled, 3);
        assert_eq!(snapshot.game_draw_calls, 7);
        assert_eq!(snapshot.scene_draw_calls, 10);
        assert_eq!(snapshot.shadow_draw_calls, 5);
    }
}
