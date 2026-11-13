//! Máquina de estados de animación v1 (estilo Animator de Unity, reducido).
//! Tipos serializables del controlador + `AnimatorData::advance`, que es la
//! lógica de decisión testeable (sin GPU ni ficheros): avanza el estado,
//! evalúa transiciones y devuelve qué clips/tiempos/peso tocan este frame.

use serde::{Deserialize, Serialize};
use crate::scene::AnimatorData;

// ---------------------------------------------------------------------------
// Tipos del controlador (serializables)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    Bool,
    Float,
    Trigger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDef {
    pub name: String,
    pub kind: ParamKind,
    /// Valor por defecto (bool/trigger se guardan como 0/1).
    #[serde(default)]
    pub default: f32,
}

fn default_speed() -> f32 {
    1.0
}
fn default_loop() -> bool {
    true
}
fn default_fade() -> f32 {
    0.15
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDef {
    pub name: String,
    /// Nombre del clip dentro del .glb del modelo.
    pub clip: String,
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(default = "default_loop", rename = "loop")]
    pub r#loop: bool,
    /// Blend tree 1D opcional (Fase 4): si está presente, `clip` se
    /// ignora y la pose del estado mezcla los clips hijos según el
    /// parámetro.
    #[serde(default)]
    pub blend: Option<BlendTree1D>,
    /// Blend tree 2D opcional: si está presente, manda sobre
    /// `blend` y sobre `clip` (precedencia: blend2d > blend > clip).
    #[serde(default)]
    pub blend2d: Option<BlendTree2D>,
    /// Root motion: el desplazamiento del nodo raíz del clip
    /// mueve el transform de la entidad y la pose fija la raíz
    /// en su traslación de reposo.
    #[serde(default)]
    pub root_motion: bool,
    /// Eventos de animación declarados en este estado.
    #[serde(default)]
    pub events: Vec<AnimEvent>,
}

/// Un hijo de un blend tree 1D: un clip que entra en la mezcla al
/// llegar el parámetro a su umbral.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlendChild {
    /// Nombre del clip dentro del .glb del modelo.
    pub clip: String,
    /// Valor del parámetro al que este hijo pesa 1.
    pub threshold: f32,
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// Eventos de animación declarados en este hijo del blend tree.
    #[serde(default)]
    pub events: Vec<AnimEvent>,
}

/// Blend tree 1D (Fase 4): mezcla clips según un parámetro float,
/// estilo Unity: cada hijo pesa según la distancia del parámetro a
/// su umbral, con reparto lineal entre umbrales vecinos.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlendTree1D {
    /// Nombre del parámetro float que gobierna la mezcla.
    pub param: String,
    pub children: Vec<BlendChild>,
}

/// Pesos de los hijos de un blend tree para un valor del parámetro.
/// Los hijos se ordenan por umbral; fuera de rango manda el hijo
/// extremo (peso 1); entre dos umbrales el reparto es lineal. Solo
/// se devuelven los hijos con peso > 0. Función pura.
pub fn blend_weights(tree: &BlendTree1D, value: f32) -> Vec<(&BlendChild, f32)> {
    let mut sorted: Vec<&BlendChild> = tree.children.iter().collect();
    sorted.sort_by(|a, b| a.threshold.partial_cmp(&b.threshold).unwrap_or(std::cmp::Ordering::Equal));
    // Auditoría M-22: thresholds duplicados se colapsan (manda el
    // PRIMERO del grupo en el JSON). Sin colapsar, justo por encima
    // del punto duplicado la mezcla saltaba de un hijo a otro.
    let mut slots: Vec<&BlendChild> = Vec::with_capacity(sorted.len());
    for c in sorted {
        let duplicado = slots
            .last()
            .map(|p: &&BlendChild| (p.threshold - c.threshold).abs() <= 1e-6)
            .unwrap_or(false);
        if !duplicado {
            slots.push(c);
        }
    }
    let sorted = slots;
    if sorted.is_empty() {
        return Vec::new();
    }
    if sorted.len() == 1 || value <= sorted[0].threshold {
        return vec![(sorted[0], 1.0)];
    }
    let last = sorted.len() - 1;
    if value >= sorted[last].threshold {
        return vec![(sorted[last], 1.0)];
    }
    for i in 0..last {
        let (a, b) = (sorted[i], sorted[i + 1]);
        if value >= a.threshold && value <= b.threshold {
            let span = b.threshold - a.threshold;
            let wb = if span > 1e-6 { (value - a.threshold) / span } else { 0.0 };
            let wa = 1.0 - wb;
            let mut out = Vec::new();
            if wa > 0.0 {
                out.push((a, wa));
            }
            if wb > 0.0 {
                out.push((b, wb));
            }
            return out;
        }
    }
    vec![(sorted[last], 1.0)]
}

/// Un hijo de un blend tree 2D: un clip situado en una posición
/// (x, y) del espacio de los dos parámetros.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlendChild2D {
    /// Nombre del clip dentro del .glb del modelo.
    pub clip: String,
    /// Posición (x, y) en el espacio de los parámetros.
    pub position: [f32; 2],
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// Eventos de animación declarados en este hijo del blend tree.
    #[serde(default)]
    pub events: Vec<AnimEvent>,
}

/// Blend tree 2D: mezcla clips según DOS parámetros float
/// (p. ej. SpeedX/SpeedY para locomoción direccional). Cada hijo
/// pesa por distancia inversa al cuadrado a su posición (Shepard),
/// normalizado a suma 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlendTree2D {
    /// Nombre del parámetro float del eje X.
    pub param_x: String,
    /// Nombre del parámetro float del eje Y.
    pub param_y: String,
    pub children: Vec<BlendChild2D>,
}

/// Pesos de los hijos de un blend tree 2D para el punto (x, y).
/// Coincidencia (casi) exacta con la posición de un hijo ⇒ ese
/// hijo pesa 1. En el resto, peso = (1/d²) normalizado; se podan
/// los pesos <= 1e-3 y se renormaliza. Solo pesos > 0. Función pura.
pub fn blend_weights_2d(tree: &BlendTree2D, x: f32, y: f32) -> Vec<(&BlendChild2D, f32)> {
    if tree.children.is_empty() {
        return Vec::new();
    }
    if tree.children.len() == 1 {
        return vec![(&tree.children[0], 1.0)];
    }
    let mut inv: Vec<f32> = Vec::with_capacity(tree.children.len());
    for c in &tree.children {
        let dx = x - c.position[0];
        let dy = y - c.position[1];
        let d2 = dx * dx + dy * dy;
        if d2 <= 1e-12 {
            return vec![(c, 1.0)];
        }
        inv.push(1.0 / d2);
    }
    let suma: f32 = inv.iter().sum();
    if suma <= 0.0 || !suma.is_finite() {
        return vec![(&tree.children[0], 1.0)];
    }
    let mut out: Vec<(&BlendChild2D, f32)> = tree
        .children
        .iter()
        .zip(inv.iter())
        .map(|(c, i)| (c, i / suma))
        .filter(|(_, w)| *w > 1e-3)
        .collect();
    let s2: f32 = out.iter().map(|(_, w)| *w).sum();
    if s2 > 0.0 {
        for (_, w) in out.iter_mut() {
            *w /= s2;
        }
    }
    out
}

/// Hijos ponderados (clip, eventos, peso) de un estado con blend
/// tree, sea 1D o 2D (el 2D manda si están los dos). `None` si el
/// estado no tiene blend de ningún tipo. Solo pesos > 0.
fn weighted_children<'a>(
    st: &'a StateDef,
    params: &std::collections::HashMap<String, f32>,
) -> Option<Vec<(&'a str, &'a Vec<AnimEvent>, f32)>> {
    if let Some(t2) = &st.blend2d {
        let x = params.get(&t2.param_x).copied().unwrap_or(0.0);
        let y = params.get(&t2.param_y).copied().unwrap_or(0.0);
        return Some(
            blend_weights_2d(t2, x, y)
                .into_iter()
                .map(|(c, w)| (c.clip.as_str(), &c.events, w))
                .collect(),
        );
    }
    st.blend.as_ref().map(|t| {
        let v = params.get(&t.param).copied().unwrap_or(0.0);
        blend_weights(t, v)
            .into_iter()
            .map(|(c, w)| (c.clip.as_str(), &c.events, w))
            .collect()
    })
}

/// Clip de referencia de un estado para `exit_time`: el suyo, o el
/// hijo de menor umbral si es un estado con blend tree.
/// Clip de referencia de un estado para medir tiempos (exit_time):
/// en un estado con blend tree, el hijo DOMINANTE según el valor
/// actual del parámetro (auditoría M-23: antes era siempre el de
/// menor threshold, aunque su peso fuese 0); sin blend, su clip.
fn state_reference_clip(
    controller: &AnimatorController,
    state_name: &str,
    params: &std::collections::HashMap<String, f32>,
) -> Option<String> {
    let st = controller.state(state_name)?;
    match weighted_children(st, params) {
        Some(hijos) => hijos
            .iter()
            .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(clip, _, _)| clip.to_string()),
        None => Some(st.clip.clone()),
    }
}

/// Tramo de avance de un clip al recolectar eventos: del instante
/// `prev` al `next` (segundos), sobre un clip de duración `dur`, en
/// el estado `state`. (Agrupa los parámetros de la ventana para no
/// pasarlos sueltos: lint too_many_arguments de Clippy.)
struct TramoClip<'a> {
    state: &'a str,
    clip: &'a str,
    prev: f32,
    next: f32,
    dur: f32,
    bucle: bool,
}

/// Recolecta los eventos de animación disparados al avanzar el
/// tramo de clip dado.
fn recolectar_eventos(
    defs: &[AnimEvent],
    tramo: TramoClip<'_>,
    out: &mut Vec<FiredAnimEvent>,
) {
    let TramoClip { state, clip, prev, next, dur, bucle } = tramo;
    if dur <= 0.0 || defs.is_empty() || next <= prev {
        return;
    }
    for def in defs {
        let at = def.time.clamp(0.0, 1.0) * dur;
        let dispara = if bucle {
            // En bucle: subir el instante del evento hasta el primer ciclo
            // posterior a `prev`; como mucho un disparo por evento.
            let mut t = at;
            if t <= prev {
                t += (((prev - t) / dur).floor() + 1.0) * dur;
            }
            t <= next
        } else {
            prev < at && at <= next
        };
        if dispara {
            out.push(FiredAnimEvent {
                state: state.to_string(),
                clip: clip.to_string(),
                name: def.name.clone(),
                value: def.value,
                text: def.text.clone(),
                time: at,
            });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    Eq,
    NotEq,
    Greater,
    Less,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub param: String,
    pub op: ConditionOp,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionDef {
    /// Nombre de estado origen, o "*" para AnyState.
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(default = "default_fade")]
    pub fade: f32,
    /// Fracción normalizada 0..1 sobre la duración del clip origen; None = inmediata.
    #[serde(default)]
    pub exit_time: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimatorController {
    #[serde(default)]
    pub parameters: Vec<ParamDef>,
    #[serde(default)]
    pub states: Vec<StateDef>,
    #[serde(default)]
    pub transitions: Vec<TransitionDef>,
    #[serde(default)]
    pub default_state: String,
}

impl AnimatorController {
    pub fn state(&self, name: &str) -> Option<&StateDef> {
        self.states.iter().find(|s| s.name == name)
    }

    pub fn param(&self, name: &str) -> Option<&ParamDef> {
        self.parameters.iter().find(|p| p.name == name)
    }

    /// Carga desde JSON, con el mismo patrón que `Prefab::load_from_file`:
    /// devuelve `Err(String)` con contexto legible.
    pub fn load_from_file(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("No se pudo leer '{}': {}", path.display(), e))?;
        serde_json::from_str(&text)
            .map_err(|e| format!("JSON inválido en '{}': {}", path.display(), e))
    }
}

// ---------------------------------------------------------------------------
// Salida de un paso de simulación
// ---------------------------------------------------------------------------

/// Una muestra de clip con su reloj y su peso en la pose del frame.
/// La suma de pesos de la salida es 1 (salvo pesos descartados por
/// redondeo al filtrar los ~0).
#[derive(Debug, Clone, PartialEq)]
pub struct ClipSample {
    pub clip: String,
    pub time: f32,
    pub weight: f32,
}

/// Evento de animación declarado en un estado o en un hijo de blend tree.
/// `time` está normalizado en 0..1 sobre la duración del clip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimEvent {
    /// Nombre del evento.
    pub name: String,
    /// Instante normalizado (0..1) dentro de la duración del clip.
    pub time: f32,
    /// Valor numérico opcional asociado al evento.
    #[serde(default)]
    pub value: Option<f32>,
    /// Texto opcional asociado al evento.
    #[serde(default)]
    pub text: Option<String>,
}

/// Evento de animación ya disparado en un frame concreto.
#[derive(Debug, Clone, PartialEq)]
pub struct FiredAnimEvent {
    /// Estado que lo disparó.
    pub state: String,
    /// Nombre del clip dentro del .glb del modelo.
    pub clip: String,
    /// Nombre del evento.
    pub name: String,
    /// Valor numérico opcional asociado al evento.
    pub value: Option<f32>,
    /// Texto opcional asociado al evento.
    pub text: Option<String>,
    /// Instante absoluto (segundos) dentro del clip.
    pub time: f32,
}

/// Qué hay que muestrear este frame: la lista de clips con sus
/// tiempos y pesos. Un estado normal aporta un sample a peso 1; un
/// estado con blend tree aporta sus hijos; durante un fundido van
/// los samples del origen escalados por (1−w) y los del destino por w.
#[derive(Debug, Clone, PartialEq)]
pub struct StateChangeEvent {
    /// Estado que se entra o se sale.
    pub state: String,
    /// true = entrada (OnStateEnter); false = salida (OnStateExit).
    pub entered: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdvanceOutput {
    pub samples: Vec<ClipSample>,
    /// Eventos de animación disparados en este frame.
    pub fired_events: Vec<FiredAnimEvent>,
    /// Cambios de estado del frame — entradas y salidas — para StateMachineBehaviours.
    pub state_events: Vec<StateChangeEvent>,
}

// ---------------------------------------------------------------------------
// Runtime: avance de la máquina de estados sobre `AnimatorData`
// ---------------------------------------------------------------------------

impl AnimatorData {
    /// Avanza un frame del controlador y devuelve qué muestrear.
    ///
    /// - Inicializa el estado actual y los parámetros por defecto si hace falta.
    /// - Avanza el tiempo del estado (× speed; clampa al final si no-loop).
    /// - Si hay fundido en curso, avanza temporizadores y el tiempo del origen.
    /// - Si no hay fundido, procesa una petición pendiente (Play/CrossFade) o,
    ///   en su defecto, evalúa transiciones del estado y de AnyState en orden.
    ///
    /// No conoce clips reales: la duración se pide por `clip_duration(clip)`.
    /// Avanza los relojes de los hijos del blend tree de un estado.
    fn advance_blend_clocks(
        controller: &AnimatorController,
        state_name: &str,
        times: &mut std::collections::HashMap<String, f32>,
        dt: f32,
    ) {
        let Some(st) = controller.state(state_name) else {
            return;
        };
        if let Some(tree) = &st.blend {
            for child in &tree.children {
                *times.entry(child.clip.clone()).or_insert(0.0) += dt * st.speed * child.speed;
            }
        }
        if let Some(tree) = &st.blend2d {
            for child in &tree.children {
                *times.entry(child.clip.clone()).or_insert(0.0) += dt * st.speed * child.speed;
            }
        }
    }

    /// Samples de un estado (sin fundido): su clip, o los hijos de su
    /// blend tree con los pesos del parámetro y sus relojes propios.
    fn state_samples(
        &self,
        controller: &AnimatorController,
        state_name: &str,
        times: &std::collections::HashMap<String, f32>,
    ) -> Vec<ClipSample> {
        let Some(st) = controller.state(state_name) else {
            return Vec::new();
        };
        match weighted_children(st, &self.params) {
            None => vec![ClipSample {
                clip: st.clip.clone(),
                time: self.state_time_for(state_name),
                weight: 1.0,
            }],
            Some(hijos) => hijos
                .into_iter()
                .map(|(clip, _, w)| ClipSample {
                    clip: clip.to_string(),
                    time: times.get(clip).copied().unwrap_or(0.0),
                    weight: w,
                })
                .collect(),
        }
    }

    /// El reloj «del estado» usado para el sample de un estado sin
    /// blend: state_time si es el actual, fade_from_time si es el
    /// origen del fundido.
    fn state_time_for(&self, state_name: &str) -> f32 {
        if self.fade_from_state.as_deref() == Some(state_name) {
            self.fade_from_time
        } else {
            self.state_time
        }
    }

    pub fn advance<F>(
        &mut self,
        dt: f32,
        controller: &AnimatorController,
        clip_duration: F,
    ) -> AdvanceOutput
    where
        F: Fn(&str) -> Option<f32>,
    {
        // Estado a la entrada del frame (para StateMachineBehaviours):
        // si no existe en el controlador (p. ej. el primer frame, con
        // current_state vacío), no cuenta como «salida» de nada.
        let estado_previo = self.current_state.clone();
        let previo_valido = controller.state(&estado_previo).is_some();

        // Inicializar estado actual si hace falta.
        if controller.state(&self.current_state).is_none() {
            self.current_state = controller.default_state.clone();
            self.state_time = 0.0;
        }
        // Higiene ante un cambio de controlador en caliente (auditoría,
        // menor): un fundido cuyo origen ya no existe en el
        // controlador vigente se descarta, y los relojes de blend de
        // clips ajenos a este controlador se podan.
        if let Some(from) = &self.fade_from_state {
            if controller.state(from).is_none() {
                self.fade_from_state = None;
                self.fade_from_time = 0.0;
                self.fade_blend_times.clear();
            }
        }
        {
            let mut validos: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for st in &controller.states {
                if !st.clip.is_empty() {
                    validos.insert(st.clip.as_str());
                }
                if let Some(tree) = &st.blend {
                    for ch in &tree.children {
                        validos.insert(ch.clip.as_str());
                    }
                }
                if let Some(tree) = &st.blend2d {
                    for ch in &tree.children {
                        validos.insert(ch.clip.as_str());
                    }
                }
            }
            self.blend_times.retain(|k, _| validos.contains(k.as_str()));
            self.fade_blend_times.retain(|k, _| validos.contains(k.as_str()));
        }
        // Inicializar parámetros que falten con sus valores por defecto.
        for p in &controller.parameters {
            let init = match p.kind {
                ParamKind::Bool | ParamKind::Trigger => {
                    if p.default != 0.0 { 1.0 } else { 0.0 }
                }
                ParamKind::Float => p.default,
            };
            self.params.entry(p.name.clone()).or_insert(init);
        }

        // Acumulador de eventos de animación disparados en este frame.
        let mut eventos: Vec<FiredAnimEvent> = Vec::new();
        let prev_state_time = self.state_time;
        let prev_blend_times = self.blend_times.clone();
        // 1) Avanzar el tiempo del estado actual (× speed; clamp si no-loop).
        let (cur_speed, cur_loop) = controller
            .state(&self.current_state)
            .map(|s| (s.speed, s.r#loop))
            .unwrap_or((1.0, true));
        self.state_time += dt * cur_speed;
        if !cur_loop {
            if let Some(d) = controller
                .state(&self.current_state)
                .and_then(|s| clip_duration(&s.clip))
            {
                if d > 0.0 && self.state_time > d {
                    self.state_time = d;
                }
            }
        } else if controller
            .state(&self.current_state)
            .map(|s| s.blend.is_none() && s.blend2d.is_none())
            .unwrap_or(false)
        {
            // Auditoría (menor): en loop, el reloj del estado se
            // envuelve por la duración de su clip para no perder
            // precisión f32 en sesiones largas (la muestra envuelve
            // igual al muestrear; el exit_time usa el ciclo en curso).
            if let Some(d) = controller
                .state(&self.current_state)
                .and_then(|s| clip_duration(&s.clip))
            {
                if d > 0.0 && self.state_time > d {
                    self.state_time %= d;
                }
            }
        }

        // Relojes de los hijos del blend tree del estado actual.
        Self::advance_blend_clocks(controller, &self.current_state.clone(), &mut self.blend_times, dt);

        // Evaluar los eventos del estado actual.
        if let Some(st) = controller.state(&self.current_state).cloned() {
            match weighted_children(&st, &self.params) {
                Some(hijos) => {
                    for (clip, evts, w) in hijos {
                        if w <= 0.0 {
                            continue;
                        }
                        if let Some(d) = clip_duration(clip) {
                            let prev = prev_blend_times.get(clip).copied().unwrap_or(0.0);
                            let next = self.blend_times.get(clip).copied().unwrap_or(prev);
                            recolectar_eventos(
                                evts,
                                TramoClip {
                                    state: &st.name,
                                    clip,
                                    prev,
                                    next,
                                    dur: d,
                                    bucle: true,
                                },
                                &mut eventos,
                            );
                        }
                    }
                }
                None => {
                    if let Some(d) = clip_duration(&st.clip) {
                        let next_eval = if cur_loop {
                            prev_state_time + dt * cur_speed
                        } else {
                            (prev_state_time + dt * cur_speed).min(d)
                        };
                        recolectar_eventos(
                            &st.events,
                            TramoClip {
                                state: &st.name,
                                clip: &st.clip,
                                prev: prev_state_time,
                                next: next_eval,
                                dur: d,
                                bucle: cur_loop,
                            },
                            &mut eventos,
                        );
                    }
                }
            }
        }

        if self.fade_from_state.is_some() {
            // 2a) Fundido en curso: avanzar temporizadores y tiempo del origen.
            self.fade_elapsed += dt;
            let prev_fade_from_time = self.fade_from_time;
            let prev_fade_blend_times = self.fade_blend_times.clone();
            if let Some(from) = self.fade_from_state.clone() {
                let (fs_speed, fs_loop) = controller
                    .state(&from)
                    .map(|s| (s.speed, s.r#loop))
                    .unwrap_or((1.0, true));
                self.fade_from_time += dt * fs_speed;
                if !fs_loop {
                    if let Some(d) = controller.state(&from).and_then(|s| clip_duration(&s.clip)) {
                        if d > 0.0 && self.fade_from_time > d {
                            self.fade_from_time = d;
                        }
                    }
                }
            }
            if let Some(from) = self.fade_from_state.clone() {
                Self::advance_blend_clocks(controller, &from, &mut self.fade_blend_times, dt);

                // Evaluar los eventos del estado origen del fundido.
                if let Some(st) = controller.state(&from).cloned() {
                    let (fs_speed, fs_loop) = (st.speed, st.r#loop);
                    match weighted_children(&st, &self.params) {
                        Some(hijos) => {
                            for (clip, evts, w) in hijos {
                                if w <= 0.0 {
                                    continue;
                                }
                                if let Some(d) = clip_duration(clip) {
                                    let prev = prev_fade_blend_times
                                        .get(clip)
                                        .copied()
                                        .unwrap_or(0.0);
                                    let next = self
                                        .fade_blend_times
                                        .get(clip)
                                        .copied()
                                        .unwrap_or(prev);
                                    recolectar_eventos(
                                        evts,
                                        TramoClip {
                                            state: &st.name,
                                            clip,
                                            prev,
                                            next,
                                            dur: d,
                                            bucle: true,
                                        },
                                        &mut eventos,
                                    );
                                }
                            }
                        }
                        None => {
                            if let Some(d) = clip_duration(&st.clip) {
                                let next_eval = if fs_loop {
                                    prev_fade_from_time + dt * fs_speed
                                } else {
                                    (prev_fade_from_time + dt * fs_speed).min(d)
                                };
                                recolectar_eventos(
                                    &st.events,
                                    TramoClip {
                                        state: &st.name,
                                        clip: &st.clip,
                                        prev: prev_fade_from_time,
                                        next: next_eval,
                                        dur: d,
                                        bucle: fs_loop,
                                    },
                                    &mut eventos,
                                );
                            }
                        }
                    }
                }
            }
            if self.fade_duration <= 0.0 || self.fade_elapsed >= self.fade_duration {
                self.fade_from_state = None;
                self.fade_from_time = 0.0;
                self.fade_blend_times.clear();
            } else {
                // Auditoría (menor): durante el fundido TAMBIÉN se
                // atienden las peticiones y las transiciones (antes
                // esperaban a que acabase y los triggers disparaban
                // tarde). Si algo dispara, el fundido en curso se
                // completa en el acto y el nuevo arranca desde el
                // estado destino (el salto de pose queda acotado al
                // peso que le quedaba al origen saliente).
                let mut fired: Option<(String, f32)> = None;
                if let Some((name, fade)) = self.pending_play.take() {
                    if controller.state(&name).is_some() && name != self.current_state {
                        fired = Some((name, fade.max(0.0)));
                    }
                }
                if fired.is_none() {
                    fired = self.eval_transitions(controller, &clip_duration);
                }
                if let Some((to, fade)) = fired {
                    self.fade_from_state = None;
                    self.fade_from_time = 0.0;
                    self.fade_blend_times.clear();
                    self.fade_elapsed = 0.0;
                    self.fade_duration = 0.0;
                    self.start_fade(to, fade);
                }
            }
        } else {
            // 2b) Sin fundido: petición pendiente (Play/CrossFade) o transiciones.
            let mut fired: Option<(String, f32)> = None;
            if let Some((name, fade)) = self.pending_play.take() {
                if controller.state(&name).is_some() && name != self.current_state {
                    fired = Some((name, fade.max(0.0)));
                }
            }
            if fired.is_none() {
                fired = self.eval_transitions(controller, &clip_duration);
            }
            if let Some((to, fade)) = fired {
                self.start_fade(to, fade);
            }
        }

        // Cambios de estado del frame (StateMachineBehaviours):
        // comparación entrada/salida de advance — cubre transiciones,
        // peticiones Play/CrossFade y el alta inicial. En un fundido,
        // la entrada/salida se notifica al ARRANCAR el fundido (el
        // estado destino ya es el actual), no al completarse.
        let mut cambios_estado: Vec<StateChangeEvent> = Vec::new();
        if estado_previo != self.current_state {
            if previo_valido {
                cambios_estado.push(StateChangeEvent {
                    state: estado_previo.clone(),
                    entered: false,
                });
            }
            cambios_estado.push(StateChangeEvent {
                state: self.current_state.clone(),
                entered: true,
            });
        }

        // 3) Construir la muestra del frame.
        if let Some(from) = self.fade_from_state.clone() {
            let w = if self.fade_duration > 0.0 {
                (self.fade_elapsed / self.fade_duration).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let mut samples = Vec::new();
            for mut s in self.state_samples(controller, &from, &self.fade_blend_times) {
                s.weight *= 1.0 - w;
                if s.weight > 0.0001 {
                    samples.push(s);
                }
            }
            let destino = self.current_state.clone();
            for mut s in self.state_samples(controller, &destino, &self.blend_times) {
                s.weight *= w;
                if s.weight > 0.0001 {
                    samples.push(s);
                }
            }
            AdvanceOutput {
                samples,
                fired_events: eventos,
                state_events: cambios_estado,
            }
        } else {
            let actual = self.current_state.clone();
            AdvanceOutput {
                samples: self.state_samples(controller, &actual, &self.blend_times),
                fired_events: eventos,
                state_events: cambios_estado,
            }
        }
    }

    /// Inicia un fundido hacia `to`. Con `fade <= 0` el salto es instantáneo.
    fn start_fade(&mut self, to: String, fade: f32) {
        if fade <= 0.0 {
            self.current_state = to;
            self.state_time = 0.0;
            self.fade_from_state = None;
            self.fade_from_time = 0.0;
            self.blend_times.clear();
            self.fade_blend_times.clear();
        } else {
            self.fade_from_state = Some(self.current_state.clone());
            self.fade_from_time = self.state_time;
            self.fade_duration = fade;
            self.fade_elapsed = 0.0;
            // Los relojes de blend del estado saliente pasan a ser
            // los del origen del fundido; el destino arranca de cero.
            self.fade_blend_times = std::mem::take(&mut self.blend_times);
            self.current_state = to;
            self.state_time = 0.0;
        }
    }

    /// Evalúa transiciones del estado actual y de AnyState en orden de
    /// declaración. Devuelve el destino (con su fundido) y consume los
    /// triggers usados por la transición que dispara.
    fn eval_transitions<F>(
        &mut self,
        controller: &AnimatorController,
        clip_duration: &F,
    ) -> Option<(String, f32)>
    where
        F: Fn(&str) -> Option<f32>,
    {
        let current = self.current_state.clone();
        let state_time = self.state_time;
        let mut chosen: Option<(String, f32, Vec<String>)> = None;

        for tr in &controller.transitions {
            // Solo transiciones desde el estado actual o AnyState.
            if tr.from != "*" && tr.from != current {
                continue;
            }
            // Nada de auto-retransiciones (evita re-disparar al llegar).
            if tr.to == current {
                continue;
            }
            // Condiciones.
            let mut ok = true;
            for cond in &tr.conditions {
                let v = self.params.get(&cond.param).copied().unwrap_or(0.0);
                let pass = match cond.op {
                    ConditionOp::Eq => (v - cond.value).abs() < 1e-5,
                    ConditionOp::NotEq => (v - cond.value).abs() >= 1e-5,
                    ConditionOp::Greater => v > cond.value,
                    ConditionOp::Less => v < cond.value,
                };
                if !pass {
                    ok = false;
                    break;
                }
            }
            if !ok {
                continue;
            }
            // exit_time: fracción normalizada sobre la duración del
            // clip de referencia del estado actual (en blend, el hijo
            // dominante — M-23). En estados con loop la fracción es la
            // del ciclo en curso (state_time % duración); sin loop, la
            // del tiempo clamped.
            if let Some(et) = tr.exit_time {
                let src_clip = state_reference_clip(controller, &current, &self.params);
                let dur = src_clip.as_deref().and_then(clip_duration);
                match dur {
                    Some(d) if d > 0.0 => {
                        let en_loop = controller
                            .state(&current)
                            .map(|s| s.r#loop)
                            .unwrap_or(true);
                        let fraccion = if en_loop {
                            (state_time % d) / d
                        } else {
                            state_time / d
                        };
                        if fraccion < et {
                            continue;
                        }
                    }
                    _ => continue,
                }
            }
            // Triggers referenciados por la transición (se consumen al disparar).
            let mut triggers = Vec::new();
            for cond in &tr.conditions {
                if let Some(pd) = controller.param(&cond.param) {
                    if pd.kind == ParamKind::Trigger {
                        triggers.push(cond.param.clone());
                    }
                }
            }
            chosen = Some((tr.to.clone(), tr.fade, triggers));
            break;
        }

        if let Some((to, fade, triggers)) = chosen {
            for t in triggers {
                if let Some(v) = self.params.get_mut(&t) {
                    *v = 0.0; // trigger consumido
                }
            }
            Some((to, fade))
        } else {
            None
        }
    }
}#[cfg(test)]
mod fsm_tests {
    use super::*;
    use std::collections::HashMap;

    fn condition(param: &str, op: ConditionOp, value: f32) -> Condition {
        Condition {
            param: param.to_string(),
            op,
            value,
        }
    }

    fn transition(
        from: &str,
        to: &str,
        conditions: Vec<Condition>,
        fade: f32,
        exit_time: Option<f32>,
    ) -> TransitionDef {
        TransitionDef {
            from: from.to_string(),
            to: to.to_string(),
            conditions,
            fade,
            exit_time,
        }
    }

    fn state(name: &str, clip: &str, looping: bool) -> StateDef {
        StateDef {
            name: name.to_string(),
            clip: clip.to_string(),
            speed: 1.0,
            r#loop: looping,
            blend: None,
            blend2d: None,
            root_motion: false,
            events: Vec::new(),
        }
    }

    /// Idle (loop) / Walk (loop) / Jump (no loop), con las transiciones
    /// del enunciado: Idle<->Walk por el bool `walk` con fundido 0,25;
    /// AnyState->Jump por el trigger `jump`; Jump->Idle al agotar el
    /// clip (exit_time 1,0, sin fundido).
    fn controller() -> AnimatorController {
        AnimatorController {
            default_state: "Idle".to_string(),
            parameters: vec![
                ParamDef { name: "walk".to_string(), kind: ParamKind::Bool, default: 0.0 },
                ParamDef { name: "jump".to_string(), kind: ParamKind::Trigger, default: 0.0 },
            ],
            states: vec![
                state("Idle", "idle", true),
                state("Walk", "walk", true),
                state("Jump", "jump", false),
            ],
            transitions: vec![
                transition("Idle", "Walk", vec![condition("walk", ConditionOp::Eq, 1.0)], 0.25, None),
                transition("Walk", "Idle", vec![condition("walk", ConditionOp::Eq, 0.0)], 0.25, None),
                transition("*", "Jump", vec![condition("jump", ConditionOp::Eq, 1.0)], 0.1, None),
                transition("Jump", "Idle", vec![], 0.0, Some(1.0)),
            ],
        }
    }

    fn clip_duration(clip: &str) -> Option<f32> {
        match clip {
            "idle" | "walk" => Some(2.0),
            "jump" => Some(0.8),
            _ => None,
        }
    }

    fn animator() -> AnimatorData {
        AnimatorData {
            controller: "fsm".to_string(),
            playing: true,
            params: HashMap::new(),
            ..Default::default()
        }
    }

    #[test]
    fn arranca_en_el_estado_por_defecto() {
        let c = controller();
        let mut a = animator();
        let out = a.advance(0.0, &c, clip_duration);
        assert_eq!(a.current_state, "Idle");
        assert_eq!(out.samples.len(), 1);
        assert_eq!(out.samples[0].clip, "idle");
        assert_eq!(out.samples[0].weight, 1.0);
    }

    #[test]
    fn walk_entra_con_fundido_y_termina_en_walk() {
        let c = controller();
        let mut a = animator();
        a.advance(0.0, &c, clip_duration);
        a.params.insert("walk".to_string(), 1.0);

        // El frame que dispara la transición arranca el fundido (peso 0).
        let out = a.advance(0.125, &c, clip_duration);
        assert_eq!(out.samples.len(), 1);
        assert_eq!(out.samples[0].clip, "idle");
        assert_eq!(a.current_state, "Walk");

        // A mitad del fundido (0,25 s): dos clips y peso intermedio.
        let out = a.advance(0.125, &c, clip_duration);
        assert_eq!(out.samples.len(), 2);
        let w_walk = out.samples.iter().find(|s| s.clip == "walk").map(|s| s.weight).unwrap_or(0.0);
        assert!(w_walk > 0.0 && w_walk < 1.0, "peso walk {w_walk}");
        let suma: f32 = out.samples.iter().map(|s| s.weight).sum();
        assert!((suma - 1.0).abs() < 1e-4, "suma {suma}");

        // Completado el fundido: solo walk, peso 1.
        let out = a.advance(0.125, &c, clip_duration);
        assert_eq!(out.samples.len(), 1);
        assert_eq!(out.samples[0].clip, "walk");
        assert_eq!(out.samples[0].weight, 1.0);
    }

    #[test]
    fn el_trigger_dispara_una_sola_vez() {
        let c = controller();
        let mut a = animator();
        a.advance(0.0, &c, clip_duration);
        a.params.insert("jump".to_string(), 1.0);

        let out = a.advance(0.01, &c, clip_duration);
        assert_eq!(a.current_state, "Jump");
        // El frame que dispara abre el fundido con peso 0 del destino:
        // la muestra sigue siendo solo idle.
        assert_eq!(out.samples.len(), 1);
        assert_eq!(out.samples[0].clip, "idle");
        // A mitad del fundido de entrada, jump ya pesa.
        let out = a.advance(0.05, &c, clip_duration);
        let w_jump = out.samples.iter().find(|s| s.clip == "jump").map(|s| s.weight).unwrap_or(0.0);
        assert!(w_jump > 0.0 && w_jump < 1.0, "peso jump {w_jump}");
        // El trigger quedó consumido al disparar.
        assert_eq!(a.params.get("jump").copied().unwrap_or(0.0), 0.0);

        // El fundido de entrada se completa; la salida por exit_time se
        // evalúa en un frame posterior y vuelve a Idle...
        a.advance(1.0, &c, clip_duration);
        assert_eq!(a.current_state, "Jump");
        a.advance(0.02, &c, clip_duration);
        assert_eq!(a.current_state, "Idle");
        // ...y no se re-dispara solo: el trigger ya no está.
        let out = a.advance(0.02, &c, clip_duration);
        assert_eq!(a.current_state, "Idle");
        assert_eq!(out.samples.len(), 1);
        assert_eq!(out.samples[0].clip, "idle");
    }

    #[test]
    fn jump_vuelve_a_idle_al_agotar_exit_time() {
        let c = controller();
        let mut a = animator();
        a.advance(0.0, &c, clip_duration);
        a.params.insert("jump".to_string(), 1.0);
        a.advance(0.01, &c, clip_duration);
        assert_eq!(a.current_state, "Jump");

        // A mitad del clip (y con el fundido de entrada ya cerrado) sigue.
        a.advance(0.5, &c, clip_duration);
        assert_eq!(a.current_state, "Jump");

        // Al agotar la duración (exit_time 1,0) vuelve a Idle.
        a.advance(0.6, &c, clip_duration);
        assert_eq!(a.current_state, "Idle");
    }

    #[test]
    fn sin_parametros_no_hay_transiciones_espontaneas() {
        let c = controller();
        let mut a = animator();
        for _ in 0..20 {
            let out = a.advance(0.1, &c, clip_duration);
            assert_eq!(out.samples.len(), 1);
            assert_eq!(out.samples[0].clip, "idle");
            assert_eq!(a.current_state, "Idle");
        }
    }

    #[test]
    fn blend_tree_mezcla_por_parametro() {
        // Controlador con un único estado Locomocion (blend por Speed:
        // idle@0, walk@2.5, jump@5 como tercer clip) y sin transiciones.
        let mut c = controller();
        c.default_state = "Locomocion".to_string();
        c.parameters.push(ParamDef { name: "Speed".to_string(), kind: ParamKind::Float, default: 0.0 });
        c.states.push(StateDef {
            name: "Locomocion".to_string(),
            clip: String::new(),
            speed: 1.0,
            r#loop: true,
            blend: Some(BlendTree1D {
                param: "Speed".to_string(),
                children: vec![
                    BlendChild { clip: "idle".to_string(), threshold: 0.0, speed: 1.0, events: Vec::new() },
                    BlendChild { clip: "walk".to_string(), threshold: 2.5, speed: 1.0, events: Vec::new() },
                    BlendChild { clip: "jump".to_string(), threshold: 5.0, speed: 1.0, events: Vec::new() },
                ],
            }),
            blend2d: None,
            root_motion: false,
            events: Vec::new(),
        });
        c.transitions.clear();
        let mut a = animator();
        let out = a.advance(0.1, &c, clip_duration);
        assert_eq!(out.samples.len(), 1);
        assert_eq!(out.samples[0].clip, "idle");
        a.params.insert("Speed".to_string(), 1.25);
        let out = a.advance(0.1, &c, clip_duration);
        assert_eq!(out.samples.len(), 2);
        let w_idle = out.samples.iter().find(|s| s.clip == "idle").unwrap().weight;
        let w_walk = out.samples.iter().find(|s| s.clip == "walk").unwrap().weight;
        assert!((w_idle - 0.5).abs() < 1e-4, "idle {w_idle}");
        assert!((w_walk - 0.5).abs() < 1e-4, "walk {w_walk}");
        a.params.insert("Speed".to_string(), 9.0);
        let out = a.advance(0.1, &c, clip_duration);
        assert_eq!(out.samples.len(), 1);
        assert_eq!(out.samples[0].clip, "jump");
        assert_eq!(out.samples[0].weight, 1.0);
    }

    #[test]
    fn blend_con_thresholds_duplicados_no_salta() {
        // Auditoría M-22: [idle@0, walk@0, jump@1] — el duplicado se
        // colapsa (manda idle) y justo por encima de 0 no hay salto.
        let tree = BlendTree1D {
            param: "Speed".to_string(),
            children: vec![
                BlendChild { clip: "idle".to_string(), threshold: 0.0, speed: 1.0, events: Vec::new() },
                BlendChild { clip: "walk".to_string(), threshold: 0.0, speed: 1.0, events: Vec::new() },
                BlendChild { clip: "jump".to_string(), threshold: 1.0, speed: 1.0, events: Vec::new() },
            ],
        };
        let w0 = blend_weights(&tree, 0.0);
        assert_eq!(w0.len(), 1);
        assert_eq!(w0[0].0.clip, "idle");
        let w = blend_weights(&tree, 0.001);
        assert!(w.iter().all(|(c, _)| c.clip != "walk"), "walk no debe aparecer: {w:?}");
        let dominante = w.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).map(|(c, _)| c).unwrap();
        assert_eq!(dominante.clip, "idle");
        let mitad = blend_weights(&tree, 0.5);
        assert_eq!(mitad.len(), 2);
        assert!((mitad.iter().map(|(_, x)| x).sum::<f32>() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn exit_time_en_blend_usa_el_clip_dominante() {
        // Auditoría M-23: con Speed alta manda jump (0,8 s), no idle
        // (2,0 s): a 0,5 s de estado la fracción ya supera 0,5.
        let mut c = controller();
        c.default_state = "Locomocion".to_string();
        c.parameters.push(ParamDef { name: "Speed".to_string(), kind: ParamKind::Float, default: 0.0 });
        c.states.push(StateDef {
            name: "Locomocion".to_string(),
            clip: String::new(),
            speed: 1.0,
            r#loop: true,
            blend: Some(BlendTree1D {
                param: "Speed".to_string(),
                children: vec![
                    BlendChild { clip: "idle".to_string(), threshold: 0.0, speed: 1.0, events: Vec::new() },
                    BlendChild { clip: "jump".to_string(), threshold: 5.0, speed: 1.0, events: Vec::new() },
                ],
            }),
            blend2d: None,
            root_motion: false,
            events: Vec::new(),
        });
        c.transitions.push(transition("Locomocion", "Idle", vec![], 0.0, Some(0.5)));

        let mut a = animator();
        a.current_state = "Locomocion".to_string();
        a.params.insert("Speed".to_string(), 5.0);
        a.state_time = 0.5;
        a.advance(0.0, &c, clip_duration);
        assert_eq!(a.current_state, "Idle", "con el dominante (jump, 0,8 s) la fracción 0,625 dispara");

        // Control: con Speed 0 manda idle (2,0 s) y 0,5 s no basta.
        let mut b = animator();
        b.current_state = "Locomocion".to_string();
        b.params.insert("Speed".to_string(), 0.0);
        b.state_time = 0.5;
        b.advance(0.0, &c, clip_duration);
        assert_eq!(b.current_state, "Locomocion");
    }

    #[test]
    fn transicion_durante_el_fundido_no_espera() {
        // Auditoría (menor): un trigger a mitad de fundido dispara ya.
        let c = controller();
        let mut a = animator();
        a.advance(0.0, &c, clip_duration);
        a.params.insert("walk".to_string(), 1.0);
        a.advance(0.016, &c, clip_duration);
        assert_eq!(a.current_state, "Walk");
        assert!(a.fade_from_state.is_some(), "el fundido Idle->Walk sigue en curso");
        a.params.insert("jump".to_string(), 1.0);
        a.advance(0.016, &c, clip_duration);
        assert_eq!(a.current_state, "Jump", "el trigger no espera al fin del fundido");
    }

    #[test]
    fn el_reloj_en_loop_se_envuelve() {
        // Auditoría (menor): state_time no crece sin límite en loop.
        let c = controller();
        let mut a = animator();
        a.advance(0.0, &c, clip_duration);
        a.state_time = 5.0; // idle dura 2,0 s
        a.advance(0.0, &c, clip_duration);
        assert!((a.state_time - 1.0).abs() < 1e-4, "state_time={}", a.state_time);
    }
}

#[cfg(test)]
mod tests_eventos {
    use super::*;
    use crate::scene::AnimatorData;

    fn ev(name: &str, time: f32) -> AnimEvent {
        AnimEvent {
            name: name.to_string(),
            time,
            value: None,
            text: None,
        }
    }

    fn state_def(
        name: &str,
        clip: &str,
        speed: f32,
        bucle: bool,
        blend: Option<BlendTree1D>,
        events: Vec<AnimEvent>,
    ) -> StateDef {
        StateDef {
            name: name.to_string(),
            clip: clip.to_string(),
            speed,
            r#loop: bucle,
            blend,
            blend2d: None,
            root_motion: false,
            events,
        }
    }

    fn child_def(clip: &str, threshold: f32, events: Vec<AnimEvent>) -> BlendChild {
        BlendChild {
            clip: clip.to_string(),
            threshold,
            speed: 1.0,
            events,
        }
    }

    fn controller(states: Vec<StateDef>, default_state: &str) -> AnimatorController {
        AnimatorController {
            parameters: vec![],
            states,
            transitions: vec![],
            default_state: default_state.to_string(),
        }
    }

    fn animator(current_state: &str) -> AnimatorData {
        AnimatorData {
            current_state: current_state.to_string(),
            ..Default::default()
        }
    }

    fn dur(clip: &str) -> Option<f32> {
        match clip {
            "idle" | "walk" => Some(2.0),
            "jump" => Some(0.8),
            _ => None,
        }
    }

    #[test]
    fn evento_simple_loop_dispara_al_cruzar() {
        let c = controller(
            vec![state_def(
                "Walk",
                "walk",
                1.0,
                true,
                None,
                vec![ev("Paso", 0.5)],
            )],
            "Walk",
        );
        let mut a = animator("Walk");

        let out = a.advance(0.6, &c, dur);
        assert!(out.fired_events.is_empty());

        let out = a.advance(0.6, &c, dur);
        assert_eq!(out.fired_events.len(), 1);
        let e = &out.fired_events[0];
        assert_eq!(e.name.as_str(), "Paso");
        assert_eq!(e.state.as_str(), "Walk");
        assert!((e.time - 1.0).abs() < 1e-4, "time={}", e.time);
    }

    #[test]
    fn evento_al_envolver_el_ciclo() {
        let c = controller(
            vec![state_def(
                "Walk",
                "walk",
                1.0,
                true,
                None,
                vec![ev("Paso", 0.9)],
            )],
            "Walk",
        );
        let mut a = animator("Walk");
        a.state_time = 1.5;

        let out = a.advance(0.6, &c, dur);
        assert_eq!(out.fired_events.len(), 1);
    }

    #[test]
    fn evento_no_loop_una_sola_vez() {
        let c = controller(
            vec![state_def(
                "Jump",
                "jump",
                1.0,
                false,
                None,
                vec![ev("Aterrizar", 0.75)],
            )],
            "Jump",
        );
        let mut a = animator("Jump");

        let out = a.advance(0.5, &c, dur);
        assert!(out.fired_events.is_empty());

        let out = a.advance(0.5, &c, dur);
        assert_eq!(out.fired_events.len(), 1);

        let out = a.advance(0.5, &c, dur);
        assert!(out.fired_events.is_empty());
    }

    #[test]
    fn evento_en_hijo_de_blend() {
        let tree = BlendTree1D {
            param: "Speed".to_string(),
            children: vec![
                child_def("idle", 0.0, vec![]),
                child_def("walk", 2.5, vec![ev("PasoWalk", 0.5)]),
            ],
        };
        let c = controller(
            vec![state_def(
                "Locomocion",
                "walk",
                1.0,
                true,
                Some(tree),
                vec![],
            )],
            "Locomocion",
        );
        let mut a = animator("Locomocion");
        a.params.insert("Speed".to_string(), 2.5);

        let out = a.advance(1.2, &c, dur);
        assert_eq!(out.fired_events.len(), 1);
        assert_eq!(out.fired_events[0].clip.as_str(), "walk");
    }

    #[test]
    fn blend2d_pesos_basicos() {
        let tree = BlendTree2D {
            param_x: "X".to_string(),
            param_y: "Y".to_string(),
            children: vec![
                BlendChild2D { clip: "a".to_string(), position: [-1.0, -1.0], speed: 1.0, events: vec![] },
                BlendChild2D { clip: "b".to_string(), position: [1.0, -1.0], speed: 1.0, events: vec![] },
                BlendChild2D { clip: "c".to_string(), position: [-1.0, 1.0], speed: 1.0, events: vec![] },
                BlendChild2D { clip: "d".to_string(), position: [1.0, 1.0], speed: 1.0, events: vec![] },
            ],
        };
        // En el centro, las cuatro esquinas pesan lo mismo.
        let w = blend_weights_2d(&tree, 0.0, 0.0);
        assert_eq!(w.len(), 4);
        for (_, peso) in &w {
            assert!((peso - 0.25).abs() < 1e-4, "peso {peso}");
        }
        // Sobre la posición de un hijo, ese hijo pesa 1.
        let w = blend_weights_2d(&tree, 1.0, 1.0);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].0.clip, "d");
        assert_eq!(w[0].1, 1.0);
        // La suma es 1 en un punto arbitrario.
        let w = blend_weights_2d(&tree, 0.3, -0.7);
        let suma: f32 = w.iter().map(|(_, p)| p).sum();
        assert!((suma - 1.0).abs() < 1e-5, "suma {suma}");
        // Árbol vacío y de un solo hijo.
        let vacio = BlendTree2D { param_x: "X".to_string(), param_y: "Y".to_string(), children: vec![] };
        assert!(blend_weights_2d(&vacio, 0.0, 0.0).is_empty());
        let uno = BlendTree2D {
            param_x: "X".to_string(),
            param_y: "Y".to_string(),
            children: vec![BlendChild2D { clip: "a".to_string(), position: [3.0, 4.0], speed: 1.0, events: vec![] }],
        };
        let w = blend_weights_2d(&uno, 0.0, 0.0);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].1, 1.0);
    }

    #[test]
    fn blend2d_mezcla_por_parametros() {
        let st = StateDef {
            name: "Movimiento".to_string(),
            clip: String::new(),
            speed: 1.0,
            r#loop: true,
            blend: None,
            blend2d: Some(BlendTree2D {
                param_x: "SpeedX".to_string(),
                param_y: "SpeedY".to_string(),
                children: vec![
                    BlendChild2D { clip: "idle".to_string(), position: [0.0, 0.0], speed: 1.0, events: vec![] },
                    BlendChild2D { clip: "walk".to_string(), position: [2.0, 0.0], speed: 1.0, events: vec![] },
                    BlendChild2D { clip: "jump".to_string(), position: [0.0, 2.0], speed: 1.0, events: vec![] },
                ],
            }),
            root_motion: false,
            events: vec![],
        };
        let c = controller(vec![st], "Movimiento");
        let mut a = animator("Movimiento");
        // En (0,0) solo idle.
        let out = a.advance(0.1, &c, dur);
        assert_eq!(out.samples.len(), 1);
        assert_eq!(out.samples[0].clip, "idle");
        assert_eq!(out.samples[0].weight, 1.0);
        // En (1,1) los tres están a la misma distancia² (2): tercios.
        a.params.insert("SpeedX".to_string(), 1.0);
        a.params.insert("SpeedY".to_string(), 1.0);
        let out = a.advance(0.1, &c, dur);
        assert_eq!(out.samples.len(), 3);
        for s in &out.samples {
            assert!((s.weight - 1.0 / 3.0).abs() < 1e-3, "{} {}", s.clip, s.weight);
        }
        // Sobre walk, solo walk.
        a.params.insert("SpeedX".to_string(), 2.0);
        a.params.insert("SpeedY".to_string(), 0.0);
        let out = a.advance(0.1, &c, dur);
        assert_eq!(out.samples.len(), 1);
        assert_eq!(out.samples[0].clip, "walk");
    }

    #[test]
    fn evento_en_hijo_de_blend2d() {
        let st = StateDef {
            name: "Movimiento".to_string(),
            clip: String::new(),
            speed: 1.0,
            r#loop: true,
            blend: None,
            blend2d: Some(BlendTree2D {
                param_x: "SpeedX".to_string(),
                param_y: "SpeedY".to_string(),
                children: vec![
                    BlendChild2D { clip: "idle".to_string(), position: [0.0, 0.0], speed: 1.0, events: vec![] },
                    BlendChild2D { clip: "walk".to_string(), position: [2.5, 0.0], speed: 1.0, events: vec![ev("PasoWalk", 0.5)] },
                ],
            }),
            root_motion: false,
            events: vec![],
        };
        let c = controller(vec![st], "Movimiento");
        let mut a = animator("Movimiento");
        a.params.insert("SpeedX".to_string(), 2.5);
        a.params.insert("SpeedY".to_string(), 0.0);

        let out = a.advance(1.2, &c, dur);
        assert_eq!(out.fired_events.len(), 1);
        assert_eq!(out.fired_events[0].clip.as_str(), "walk");
    }

    #[test]
    fn hijo_sin_peso_no_dispara() {
        let tree = BlendTree1D {
            param: "Speed".to_string(),
            children: vec![
                child_def("idle", 0.0, vec![]),
                child_def("walk", 2.5, vec![ev("PasoWalk", 0.5)]),
            ],
        };
        let c = controller(
            vec![state_def(
                "Locomocion",
                "walk",
                1.0,
                true,
                Some(tree),
                vec![],
            )],
            "Locomocion",
        );
        let mut a = animator("Locomocion");
        a.params.insert("Speed".to_string(), 0.0);

        let out = a.advance(1.2, &c, dur);
        assert!(out.fired_events.is_empty());
    }

    #[test]
    fn dt_gigante_una_sola_ocurrencia() {
        let c = controller(
            vec![state_def(
                "Walk",
                "walk",
                1.0,
                true,
                None,
                vec![ev("Paso", 0.5)],
            )],
            "Walk",
        );
        let mut a = animator("Walk");

        let out = a.advance(5.5, &c, dur);
        assert_eq!(out.fired_events.len(), 1);
    }

    #[test]
    fn speed_doble_llega_antes() {
        let c = controller(
            vec![state_def(
                "Walk",
                "walk",
                2.0,
                true,
                None,
                vec![ev("Paso", 0.5)],
            )],
            "Walk",
        );
        let mut a = animator("Walk");

        let out = a.advance(0.6, &c, dur);
        assert_eq!(out.fired_events.len(), 1);
    }

    #[test]
    fn origen_del_fundido_sigue_disparando() {
        let c = controller(
            vec![
                state_def("Walk", "walk", 1.0, true, None, vec![]),
                state_def("Idle", "idle", 1.0, true, None, vec![ev("PasoIdle", 0.5)]),
            ],
            "Walk",
        );
        let mut a = animator("Walk");
        a.fade_from_state = Some("Idle".to_string());
        a.fade_from_time = 0.9;
        a.fade_duration = 1.0;
        a.fade_elapsed = 0.0;

        let out = a.advance(0.2, &c, dur);
        assert_eq!(out.fired_events.len(), 1);
        assert_eq!(out.fired_events[0].state.as_str(), "Idle");
    }

    #[test]
    fn sin_cruce_no_hay_eventos() {
        let c = controller(
            vec![state_def(
                "Walk",
                "walk",
                1.0,
                true,
                None,
                vec![ev("Paso", 0.5)],
            )],
            "Walk",
        );
        let mut a = animator("Walk");

        let out = a.advance(0.3, &c, dur);
        assert!(out.fired_events.is_empty());
    }
}

#[cfg(test)]
mod tests_estados {
    use super::*;
    use crate::scene::AnimatorData;

    fn dur(clip: &str) -> Option<f32> {
        match clip {
            "idle" | "walk" => Some(2.0),
            "jump" => Some(0.8),
            _ => None,
        }
    }

    fn estado(name: &str, clip: &str, bucle: bool) -> StateDef {
        StateDef {
            name: name.to_string(),
            clip: clip.to_string(),
            speed: 1.0,
            r#loop: bucle,
            blend: None,
            blend2d: None,
            root_motion: false,
            events: Vec::new(),
        }
    }

    fn ctrl() -> AnimatorController {
        AnimatorController {
            parameters: vec![
                ParamDef { name: "walk".to_string(), kind: ParamKind::Bool, default: 0.0 },
                ParamDef { name: "jump".to_string(), kind: ParamKind::Trigger, default: 0.0 },
            ],
            states: vec![
                estado("Idle", "idle", true),
                estado("Walk", "walk", true),
                estado("Jump", "jump", false),
            ],
            transitions: vec![
                TransitionDef {
                    from: "Idle".to_string(),
                    to: "Walk".to_string(),
                    conditions: vec![Condition {
                        param: "walk".to_string(),
                        op: ConditionOp::Eq,
                        value: 1.0,
                    }],
                    fade: 0.25,
                    exit_time: None,
                },
                TransitionDef {
                    from: "Walk".to_string(),
                    to: "Idle".to_string(),
                    conditions: vec![Condition {
                        param: "walk".to_string(),
                        op: ConditionOp::Eq,
                        value: 0.0,
                    }],
                    fade: 0.25,
                    exit_time: None,
                },
                TransitionDef {
                    from: "*".to_string(),
                    to: "Jump".to_string(),
                    conditions: vec![Condition {
                        param: "jump".to_string(),
                        op: ConditionOp::Eq,
                        value: 1.0,
                    }],
                    fade: 0.1,
                    exit_time: None,
                },
            ],
            default_state: "Idle".to_string(),
        }
    }

    fn anim(current: &str) -> AnimatorData {
        AnimatorData {
            current_state: current.to_string(),
            ..Default::default()
        }
    }

    fn resumen(out: &AdvanceOutput) -> Vec<(&str, bool)> {
        out.state_events.iter().map(|e| (e.state.as_str(), e.entered)).collect()
    }

    #[test]
    fn primer_avance_solo_enter_sin_exit() {
        let c = ctrl();
        let mut a = anim("");
        let out = a.advance(0.1, &c, dur);
        assert_eq!(resumen(&out), vec![("Idle", true)]);
    }

    #[test]
    fn ya_en_estado_no_emite_enter() {
        let c = ctrl();
        let mut a = anim("Idle");
        let out = a.advance(0.1, &c, dur);
        assert!(out.state_events.is_empty());
    }

    #[test]
    fn transicion_exit_luego_enter() {
        let c = ctrl();
        let mut a = anim("Idle");
        a.params.insert("walk".to_string(), 1.0);
        let out = a.advance(0.1, &c, dur);
        assert_eq!(resumen(&out), vec![("Idle", false), ("Walk", true)]);
    }

    #[test]
    fn sin_cambio_frames_siguientes_vacios() {
        let c = ctrl();
        let mut a = anim("Idle");
        a.params.insert("walk".to_string(), 1.0);
        let _ = a.advance(0.1, &c, dur);
        // El fundido (0,25 s) se completa en estos frames: sin eventos.
        assert!(a.advance(0.1, &c, dur).state_events.is_empty());
        assert!(a.advance(0.3, &c, dur).state_events.is_empty());
    }

    #[test]
    fn crossfade_notifica_al_arrancar_no_al_completar() {
        let c = ctrl();
        let mut a = anim("Idle");
        a.pending_play = Some(("Walk".to_string(), 0.5));
        let out = a.advance(0.1, &c, dur);
        assert_eq!(resumen(&out), vec![("Idle", false), ("Walk", true)]);
        assert!(a.advance(0.6, &c, dur).state_events.is_empty());
    }

    #[test]
    fn play_instantaneo_mismo_frame() {
        let c = ctrl();
        let mut a = anim("Idle");
        a.pending_play = Some(("Walk".to_string(), 0.0));
        let out = a.advance(0.1, &c, dur);
        assert_eq!(resumen(&out), vec![("Idle", false), ("Walk", true)]);
    }

    #[test]
    fn anystate_trigger_cambia_desde_walk() {
        let c = ctrl();
        let mut a = anim("Walk");
        // walk=1 mantiene falsa la condición de vuelta a Idle: así la
        // transición que dispara es la de AnyState (orden de declaración).
        a.params.insert("walk".to_string(), 1.0);
        a.params.insert("jump".to_string(), 1.0);
        let out = a.advance(0.1, &c, dur);
        assert_eq!(resumen(&out), vec![("Walk", false), ("Jump", true)]);
    }
}
