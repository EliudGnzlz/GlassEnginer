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

/// Qué hay que muestrear este frame. Durante un fundido, `clip_a`/`time_a`
/// son el estado origen y `clip_b`/`time_b` el destino, con `weight_b` en
/// [0,1] (peso del destino, listo para `evaluate_skin_matrices_blended`).
#[derive(Debug, Clone, PartialEq)]
pub struct AdvanceOutput {
    pub clip_a: String,
    pub time_a: f32,
    pub clip_b: Option<String>,
    pub time_b: f32,
    /// Peso del clip B (0 = solo A, 1 = solo B). Ignorado si `clip_b` es None.
    pub weight_b: f32,
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
        }

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
            if self.fade_duration <= 0.0 || self.fade_elapsed >= self.fade_duration {
                self.fade_from_state = None;
                self.fade_from_time = 0.0;
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
            let clip_a = controller
                .state(&from)
                .map(|s| s.clip.clone())
                .unwrap_or_default();
            let clip_b = controller.state(&self.current_state).map(|s| s.clip.clone());
            let w = if self.fade_duration > 0.0 {
                (self.fade_elapsed / self.fade_duration).clamp(0.0, 1.0)
            } else {
                1.0
            };
            AdvanceOutput {
                clip_a,
                time_a: self.fade_from_time,
                clip_b,
                time_b: self.state_time,
                weight_b: w,
            }
        } else {
            let clip = controller
                .state(&self.current_state)
                .map(|s| s.clip.clone())
                .unwrap_or_default();
            AdvanceOutput {
                clip_a: clip,
                time_a: self.state_time,
                clip_b: None,
                time_b: 0.0,
                weight_b: 1.0,
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
        } else {
            self.fade_from_state = Some(self.current_state.clone());
            self.fade_from_time = self.state_time;
            self.fade_duration = fade;
            self.fade_elapsed = 0.0;
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
            // exit_time: fracción normalizada sobre la duración del clip origen
            // (para AnyState se toma el clip del estado actual).
            if let Some(et) = tr.exit_time {
                let src_clip = if tr.from == "*" {
                    controller.state(&current).map(|s| s.clip.clone())
                } else {
                    controller.state(&tr.from).map(|s| s.clip.clone())
                };
                let dur = src_clip.as_deref().and_then(|c| clip_duration(c));
                match dur {
                    Some(d) if d > 0.0 => {
                        if (state_time / d) < et {
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
        assert_eq!(out.clip_a, "idle");
        assert!(out.clip_b.is_none());
        assert_eq!(out.weight_b, 1.0);
    }

    #[test]
    fn walk_entra_con_fundido_y_termina_en_walk() {
        let c = controller();
        let mut a = animator();
        a.advance(0.0, &c, clip_duration);
        a.params.insert("walk".to_string(), 1.0);

        // El frame que dispara la transición arranca el fundido (peso 0).
        let out = a.advance(0.125, &c, clip_duration);
        assert_eq!(out.clip_a, "idle");
        assert_eq!(out.clip_b.as_deref(), Some("walk"));
        assert_eq!(out.weight_b, 0.0);
        assert_eq!(a.current_state, "Walk");

        // A mitad del fundido (0,25 s): dos clips y peso intermedio.
        let out = a.advance(0.125, &c, clip_duration);
        assert_eq!(out.clip_a, "idle");
        assert_eq!(out.clip_b.as_deref(), Some("walk"));
        assert!(out.weight_b > 0.0 && out.weight_b < 1.0, "peso {}", out.weight_b);

        // Completado el fundido: solo walk, peso 1.
        let out = a.advance(0.125, &c, clip_duration);
        assert_eq!(out.clip_a, "walk");
        assert!(out.clip_b.is_none());
        assert_eq!(out.weight_b, 1.0);
    }

    #[test]
    fn el_trigger_dispara_una_sola_vez() {
        let c = controller();
        let mut a = animator();
        a.advance(0.0, &c, clip_duration);
        a.params.insert("jump".to_string(), 1.0);

        let out = a.advance(0.01, &c, clip_duration);
        assert_eq!(a.current_state, "Jump");
        assert_eq!(out.clip_b.as_deref(), Some("jump"));
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
        assert!(out.clip_b.is_none());
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
            assert!(out.clip_b.is_none());
            assert_eq!(out.clip_a, "idle");
            assert_eq!(a.current_state, "Idle");
        }
    }
}
