use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Un evento de animación ya disparado, tal como lo registró la app:
/// alimenta el campo `recent_events` de la tool MCP glass_animator_state.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AnimEventRecord {
    /// Entidad cuyo Animator disparó el evento.
    pub entity: u64,
    /// Estado que lo disparó.
    pub state: String,
    /// Clip cuyo reloj cruzó el evento.
    pub clip: String,
    /// Nombre del evento.
    pub name: String,
    /// Valor numérico opcional del evento.
    pub value: Option<f32>,
    /// Texto opcional del evento.
    pub text: Option<String>,
    /// Instante absoluto (segundos) dentro del clip en que disparó.
    pub time: f32,
    /// Nº de orden global y monótono: ordena eventos entre entidades.
    pub seq: u64,
}

/// Un cambio de estado de la FSM ya ocurrido, tal como lo registró la
/// app: alimenta el campo `recent_state_events` de glass_animator_state.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StateEventRecord {
    /// Entidad cuyo Animator cambió de estado.
    pub entity: u64,
    /// Estado entrado o salido.
    pub state: String,
    /// true = entrada (OnStateEnter); false = salida (OnStateExit).
    pub entered: bool,
    /// Nº de orden global y monótono (compartido con los eventos).
    pub seq: u64,
}

/// Capacidad del anillo global de eventos recientes.
const CAP: usize = 128;

#[derive(Default)]
struct AnimStatsState {
    events: VecDeque<AnimEventRecord>,
    state_events: VecDeque<StateEventRecord>,
    seq: u64,
}

static STATE: OnceLock<Mutex<AnimStatsState>> = OnceLock::new();

fn state() -> &'static Mutex<AnimStatsState> {
    STATE.get_or_init(|| Mutex::new(AnimStatsState::default()))
}

fn lock_state() -> MutexGuard<'static, AnimStatsState> {
    state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Registra un evento disparado (lo llama la app al despacharlo a C#).
pub fn record_anim_event(entity: u64, ev: &crate::animator::FiredAnimEvent) {
    let mut st = lock_state();
    st.seq += 1;
    let seq = st.seq;
    if st.events.len() >= CAP {
        st.events.pop_front();
    }
    st.events.push_back(AnimEventRecord {
        entity,
        state: ev.state.clone(),
        clip: ev.clip.clone(),
        name: ev.name.clone(),
        value: ev.value,
        text: ev.text.clone(),
        time: ev.time,
        seq,
    });
}

/// Los eventos recientes de UNA entidad, del más nuevo al más viejo.
pub fn recent_anim_events(entity: u64, max: usize) -> Vec<AnimEventRecord> {
    let st = lock_state();
    st.events
        .iter()
        .rev()
        .filter(|e| e.entity == entity)
        .take(max)
        .cloned()
        .collect()
}

/// Registra un cambio de estado (lo llama la app al despacharlo a C#).
pub fn record_state_event(entity: u64, change: &crate::animator::StateChangeEvent) {
    let mut st = lock_state();
    st.seq += 1;
    let seq = st.seq;
    if st.state_events.len() >= CAP {
        st.state_events.pop_front();
    }
    st.state_events.push_back(StateEventRecord {
        entity,
        state: change.state.clone(),
        entered: change.entered,
        seq,
    });
}

/// Los cambios de estado recientes de UNA entidad, del más nuevo al
/// más viejo.
pub fn recent_state_events(entity: u64, max: usize) -> Vec<StateEventRecord> {
    let st = lock_state();
    st.state_events
        .iter()
        .rev()
        .filter(|e| e.entity == entity)
        .take(max)
        .cloned()
        .collect()
}
