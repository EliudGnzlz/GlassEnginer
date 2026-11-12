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
    match &st.blend {
        None => Some(st.clip.clone()),
        Some(tree) => {
            let value = params.get(&tree.param).copied().unwrap_or(0.0);
            let pesos = blend_weights(tree, value);
            pesos
                .iter()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(c, _)| c.clip.clone())
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

/// Qué hay que muestrear este frame: la lista de clips con sus
/// tiempos y pesos. Un estado normal aporta un sample a peso 1; un
/// estado con blend tree aporta sus hijos; durante un fundido van
/// los samples del origen escalados por (1−w) y los del destino por w.
#[derive(Debug, Clone, PartialEq)]
pub struct AdvanceOutput {
    pub samples: Vec<ClipSample>,
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
        let Some(tree) = &st.blend else {
            return;
        };
        for child in &tree.children {
            *times.entry(child.clip.clone()).or_insert(0.0) += dt * st.speed * child.speed;
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
        match &st.blend {
            None => vec![ClipSample {
                clip: st.clip.clone(),
                time: self.state_time_for(state_name),
                weight: 1.0,
            }],
            Some(tree) => {
                let v = self.params.get(&tree.param).copied().unwrap_or(0.0);
                blend_weights(tree, v)
                    .into_iter()
                    .map(|(child, w)| ClipSample {
                        clip: child.clip.clone(),
                        time: times.get(&child.clip).copied().unwrap_or(0.0),
                        weight: w,
                    })
                    .collect()
            }
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
            .map(|s| s.blend.is_none())
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

        if self.fade_from_state.is_some() {
            // 2a) Fundido en curso: avanzar temporizadores y tiempo del origen.
            self.fade_elapsed += dt;
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
            AdvanceOutput { samples }
        } else {
            let actual = self.current_state.clone();
            AdvanceOutput {
                samples: self.state_samples(controller, &actual, &self.blend_times),
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
                    BlendChild { clip: "idle".to_string(), threshold: 0.0, speed: 1.0 },
                    BlendChild { clip: "walk".to_string(), threshold: 2.5, speed: 1.0 },
                    BlendChild { clip: "jump".to_string(), threshold: 5.0, speed: 1.0 },
                ],
            }),
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
                BlendChild { clip: "idle".to_string(), threshold: 0.0, speed: 1.0 },
                BlendChild { clip: "walk".to_string(), threshold: 0.0, speed: 1.0 },
                BlendChild { clip: "jump".to_string(), threshold: 1.0, speed: 1.0 },
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
                    BlendChild { clip: "idle".to_string(), threshold: 0.0, speed: 1.0 },
                    BlendChild { clip: "jump".to_string(), threshold: 5.0, speed: 1.0 },
                ],
            }),
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
