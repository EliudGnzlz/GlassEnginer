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
    /// Triángulos dibujados en la vista de escena (editor): suma de
    /// `index_count / 3` de cada lote estático y skinned. u64: el máximo
    /// de u32 (~4,29e9) cubre 1M × 320 = 3,2e8, pero bastan ~14M de esas
    /// esferas para desbordarlo; con u64 no hay que saturar ni comprobar.
    pub scene_triangles: u64,
    /// Triángulos dibujados en la vista de juego: suma de
    /// `index_count / 3` de cada lote estático y skinned.
    pub game_triangles: u64,
    /// Draw calls del pase de sombras del sol.
    pub shadow_draw_calls: u32,
    /// Draw calls del pase de sombras con etapa de vértices de grafo
    /// (Fase 2S).
    pub graph_shadow_draw_calls: u32,
    /// Draw calls del pase de contorno toon (Fase 2S, S5).
    pub outline_draw_calls: u32,
    /// Enlaces de pipeline en los pases de vista del frame (Fase 2S, S6).
    pub pipeline_binds: u32,
    /// Enlaces de descriptor set en el bucle de mallas de los pases de
    /// vista del frame (Fase 2S, S6).
    pub descriptor_binds: u32,
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
            scene_triangles: 1_234,
            game_triangles: 5_678,
            shadow_draw_calls: 5,
            graph_shadow_draw_calls: 3,
            outline_draw_calls: 2,
            pipeline_binds: 11,
            descriptor_binds: 23,
        };

        record_render_stats(stats);
        let snapshot = render_stats_snapshot();

        assert_eq!(snapshot.entities_considered, 10);
        assert_eq!(snapshot.entities_culled, 3);
        assert_eq!(snapshot.game_draw_calls, 7);
        assert_eq!(snapshot.scene_draw_calls, 10);
        assert_eq!(snapshot.shadow_draw_calls, 5);
        assert_eq!(snapshot.graph_shadow_draw_calls, 3);
        assert_eq!(snapshot.outline_draw_calls, 2);
        assert_eq!(snapshot.pipeline_binds, 11);
        assert_eq!(snapshot.descriptor_binds, 23);
    }
}
