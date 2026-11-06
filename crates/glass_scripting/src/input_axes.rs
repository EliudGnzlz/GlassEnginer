//! Ejes de entrada configurables al estilo Input Manager de Unity.
//!
//! El módulo separa los datos (`InputAxisDef`, serializable) del estado
//! (`AxisBank`, que suaviza los valores frame a frame y calcula los bordes
//! de botón).

use serde::{Deserialize, Serialize};

// --- Valores por defecto usados por serde (valores estándar de Unity) ---

fn d_gravity() -> f32 { 3.0 }
fn d_dead() -> f32 { 0.001 }
fn d_sensitivity() -> f32 { 3.0 }

/// Definición de un eje de entrada (datos puros, serializable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputAxisDef {
    pub name: String,
    #[serde(default)] pub positive: String,
    #[serde(default)] pub negative: String,
    #[serde(default)] pub alt_positive: String,
    #[serde(default)] pub alt_negative: String,
    #[serde(default = "d_gravity")] pub gravity: f32,
    #[serde(default = "d_dead")] pub dead: f32,
    #[serde(default = "d_sensitivity")] pub sensitivity: f32,
    #[serde(default)] pub snap: bool,
    #[serde(default)] pub invert: bool,
}

impl InputAxisDef {
    /// Eje de teclado con los valores por defecto de Unity.
    pub fn key(
        name: &str,
        positive: &str,
        negative: &str,
        alt_positive: &str,
        alt_negative: &str,
    ) -> Self {
        Self {
            name: name.to_string(),
            positive: positive.to_string(),
            negative: negative.to_string(),
            alt_positive: alt_positive.to_string(),
            alt_negative: alt_negative.to_string(),
            gravity: 3.0,
            dead: 0.001,
            sensitivity: 3.0,
            snap: false,
            invert: false,
        }
    }

    /// Eje tipo botón: respuesta inmediata (gravity/sensitivity 1000).
    pub fn button(name: &str, positive: &str, alt_positive: &str) -> Self {
        Self {
            name: name.to_string(),
            positive: positive.to_string(),
            negative: String::new(),
            alt_positive: alt_positive.to_string(),
            alt_negative: String::new(),
            gravity: 1000.0,
            dead: 0.001,
            sensitivity: 1000.0,
            snap: false,
            invert: false,
        }
    }
}

/// Conjunto clásico de ejes de Unity adaptado a nombres de tecla canónicos.
pub fn default_input_axes() -> Vec<InputAxisDef> {
    vec![
        InputAxisDef::key("Horizontal", "d", "a", "right", "left"),
        InputAxisDef::key("Vertical", "w", "s", "up", "down"),
        InputAxisDef::button("Jump", "space", ""),
        InputAxisDef::button("Fire1", "mouse0", "ctrl_left"),
        InputAxisDef::button("Fire2", "mouse1", ""),
        InputAxisDef::button("Fire3", "mouse2", ""),
        InputAxisDef::button("Submit", "return", ""),
        InputAxisDef::button("Cancel", "escape", ""),
    ]
}

/// Estado resultante de un frame de evaluación del banco de ejes.
#[derive(Clone, Debug)]
pub struct AxisFrame {
    pub values: Vec<f32>,
    pub raw: Vec<f32>,
    pub buttons: Vec<bool>,
    pub buttons_down: Vec<bool>,
    pub buttons_up: Vec<bool>,
}

/// Límite duro de ejes que soporta un banco.
const MAX_AXES: usize = 32;

/// Mueve `a` hacia `b` como máximo `max_delta` (nunca se pasa).
fn move_toward(a: f32, b: f32, max_delta: f32) -> f32 {
    let max_delta = max_delta.max(0.0);
    let delta = b - a;
    if delta.abs() <= max_delta {
        b
    } else {
        a + max_delta.copysign(delta)
    }
}

/// Banco con el estado por frame de un conjunto de ejes.
pub struct AxisBank {
    defs: Vec<InputAxisDef>,
    values: Vec<f32>,
    prev_buttons: Vec<bool>,
}

impl AxisBank {
    /// Crea el banco recortando la lista a 32 ejes como máximo.
    pub fn new(defs: Vec<InputAxisDef>) -> Self {
        let mut defs = defs;
        if defs.len() > MAX_AXES {
            defs.truncate(MAX_AXES);
        }
        let n = defs.len();
        Self {
            defs,
            values: vec![0.0; n],
            prev_buttons: vec![false; n],
        }
    }

    /// Definiciones registradas en el banco.
    pub fn defs(&self) -> &[InputAxisDef] {
        &self.defs
    }

    /// Índice del eje por nombre (insensible a mayúsculas).
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.defs
            .iter()
            .position(|d| d.name.eq_ignore_ascii_case(name))
    }

    /// Valor suavizado actual del eje (0 si no existe).
    pub fn value_of(&self, name: &str) -> f32 {
        match self.index_of(name) {
            Some(i) => self.values[i],
            None => 0.0,
        }
    }

    /// Avanza el estado de todos los ejes y devuelve el frame resultante.
    pub fn update(
        &mut self,
        dt: f32,
        down: &dyn Fn(&str) -> bool,
        analog: &dyn Fn(usize) -> f32,
    ) -> AxisFrame {
        self.update_with_pad(dt, down, analog, &|_| false, &|_| 0.0)
    }

    /// Como `update`, con dos fuentes más del MANDO (gamepad):
    /// `pad_axis(i)` se suma al objetivo antes del clamp ( el
    /// llamante decide el signo, p. ej. el stick izquierdo para
    /// Horizontal/Vertical) y `pad_button(i)` se suma al botón
    /// del eje (p. ej. el botón A para Jump).
    pub fn update_with_pad(
        &mut self,
        dt: f32,
        down: &dyn Fn(&str) -> bool,
        analog: &dyn Fn(usize) -> f32,
        pad_button: &dyn Fn(usize) -> bool,
        pad_axis: &dyn Fn(usize) -> f32,
    ) -> AxisFrame {
        let n = self.defs.len();
        let mut frame = AxisFrame {
            values: vec![0.0; n],
            raw: vec![0.0; n],
            buttons: vec![false; n],
            buttons_down: vec![false; n],
            buttons_up: vec![false; n],
        };

        for i in 0..n {
            let def = &self.defs[i];

            // Lado positivo / negativo del eje (teclas o botones)
            let pos = (!def.positive.is_empty() && down(&def.positive))
                || (!def.alt_positive.is_empty() && down(&def.alt_positive));
            let neg = (!def.negative.is_empty() && down(&def.negative))
                || (!def.alt_negative.is_empty() && down(&def.alt_negative));

            let a = analog(i);
            // El botón del mando suma como el lado positivo (una
            // pulsación de A en Jump también lleva su valor a 1).
            let pad_btn = pad_button(i);
            let mut target = ((pos as i32 - neg as i32) as f32
                + a
                + pad_axis(i)
                + if pad_btn { 1.0 } else { 0.0 })
            .clamp(-1.0, 1.0);
            if def.invert {
                target = -target;
            }

            // Valor crudo tras aplicar zona muerta
            let raw = if target.abs() < def.dead { 0.0 } else { target };

            // El botón del eje mira el lado positivo ANTES de invertir
            let button = pos || a > def.dead || pad_btn;

            // Suavizado con snap opcional al cambiar de signo
            let mut v = self.values[i];
            if def.snap && target != 0.0 && v != 0.0 && target.signum() != v.signum() {
                v = 0.0;
            }
            let rate = if target != 0.0 { def.sensitivity } else { def.gravity };
            v = move_toward(v, target, rate * dt.max(0.0));
            if target == 0.0 && v.abs() < def.dead {
                v = 0.0;
            }
            self.values[i] = v;

            // Bordes de botón
            let prev = self.prev_buttons[i];
            frame.buttons_down[i] = button && !prev;
            frame.buttons_up[i] = !button && prev;
            self.prev_buttons[i] = button;

            frame.values[i] = v;
            frame.raw[i] = raw;
            frame.buttons[i] = button;
        }

        frame
    }
}

// --- Registro global de definiciones ---

static DEFS: std::sync::RwLock<Vec<InputAxisDef>> = std::sync::RwLock::new(Vec::new());

/// Escribe el registro global de definiciones (lo hace la app al arrancar).
pub fn set_axis_defs(defs: Vec<InputAxisDef>) {
    if let Ok(mut w) = DEFS.write() {
        *w = defs;
    }
}

/// Devuelve una copia de las definiciones registradas globalmente.
pub fn axis_defs() -> Vec<InputAxisDef> {
    DEFS.read().map(|g| g.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_suma_al_boton() {
        let mut b = AxisBank::new(vec![InputAxisDef::button("Jump", "space", "")]);
        let f1 = b.update_with_pad(0.016, &|_| false, &|_| 0.0, &|_| true, &|_| 0.0);
        assert!(f1.buttons[0] && f1.buttons_down[0]);
        assert!((f1.values[0] - 1.0).abs() < 1e-5);
        let f2 = b.update_with_pad(0.016, &|_| false, &|_| 0.0, &|_| false, &|_| 0.0);
        assert!(!f2.buttons[0] && f2.buttons_up[0]);
    }

    #[test]
    fn pad_axis_mueve_el_eje() {
        let mut b = AxisBank::new(vec![InputAxisDef::key(
            "Horizontal", "d", "a", "right", "left",
        )]);
        let f = b.update_with_pad(1.0, &|_| false, &|_| 0.0, &|_| false, &|_| 0.5);
        assert!((f.raw[0] - 0.5).abs() < 1e-5);
        // La rampa (sensibilidad 3 en 1 s) alcanza el objetivo 0,5.
        assert!((f.values[0] - 0.5).abs() < 1e-5);
    }

    fn banco_default() -> AxisBank {
        AxisBank::new(default_input_axes())
    }

    #[test]
    fn rampa_horizontal() {
        let mut bank = banco_default();
        let h = bank.index_of("horizontal").unwrap();
        let k = |n: &str| n == "d";

        // 0.1 s con sensibilidad 3 => ~0.3
        let f = bank.update(0.1, &k, &|_| 0.0);
        assert!((f.values[h] - 0.3).abs() < 1e-4);

        // Un segundo completo satura en 1.0 y no lo sobrepasa
        let f = bank.update(1.0, &k, &|_| 0.0);
        assert!((f.values[h] - 1.0).abs() < 1e-6);
        let f = bank.update(1.0, &k, &|_| 0.0);
        assert!(f.values[h] <= 1.0 + 1e-6);
    }

    #[test]
    fn retorno_con_gravedad() {
        let mut bank = banco_default();
        let h = bank.index_of("Horizontal").unwrap();

        bank.update(1.0, &|n| n == "d", &|_| 0.0);
        assert!((bank.value_of("Horizontal") - 1.0).abs() < 1e-6);

        // Al soltar decae con gravity 3: de 1.0 a ~0.7 en 0.1 s
        let f = bank.update(0.1, &|_| false, &|_| 0.0);
        assert!((f.values[h] - 0.7).abs() < 1e-4);
    }

    #[test]
    fn jump_es_inmediato() {
        let mut bank = banco_default();
        let j = bank.index_of("Jump").unwrap();

        // Con dt 0.016 y sensibilidad 1000 llega a 1.0 en un solo frame
        let f = bank.update(0.016, &|n| n == "space", &|_| 0.0);
        assert!((f.values[j] - 1.0).abs() < 1e-6);
        assert!(f.buttons[j]);
        assert!(f.buttons_down[j]);
        assert!(!f.buttons_up[j]);

        // Al soltar, el borde de subida aparece una sola vez
        let f = bank.update(0.016, &|_| false, &|_| 0.0);
        assert!(!f.buttons_down[j]);
        assert!(f.buttons_up[j]);
    }

    #[test]
    fn snap_cambia_de_signo() {
        let mut def = InputAxisDef::key("Test", "d", "a", "", "");
        def.snap = true;
        let mut bank = AxisBank::new(vec![def]);

        // Llevar el eje a -0.8 (3 * 0.2666... = 0.8)
        let f = bank.update(0.266_666_68, &|n| n == "a", &|_| 0.0);
        assert!((f.values[0] + 0.8).abs() < 1e-4);

        // Con snap, pulsar el lado positivo salta a >= 0 en un frame
        let f = bank.update(0.016, &|n| n == "d", &|_| 0.0);
        assert!(f.values[0] >= 0.0);
    }

    #[test]
    fn dead_analogico() {
        let def = InputAxisDef::key("Analog", "", "", "", "");
        let mut bank = AxisBank::new(vec![def]);

        // Por debajo de la zona muerta: raw 0 y botón apagado
        let f = bank.update(0.016, &|_| false, &|_| 0.0005);
        assert_eq!(f.raw[0], 0.0);
        assert!(!f.buttons[0]);

        // Por encima: el valor crudo pasa tal cual
        let f = bank.update(0.016, &|_| false, &|_| 0.5);
        assert!((f.raw[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn eje_invertido() {
        let mut def = InputAxisDef::key("Inv", "d", "a", "", "");
        def.invert = true;
        let mut bank = AxisBank::new(vec![def]);

        let f = bank.update(0.016, &|n| n == "d", &|_| 0.0);
        assert!((f.raw[0] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn serde_rellena_defaults() {
        // JSON parcial: solo el nombre, el resto debe tomar los defaults
        let json = r#"{ "name": "Prueba" }"#;
        let def: InputAxisDef = serde_json::from_str(json).unwrap();
        assert!((def.gravity - 3.0).abs() < 1e-6);
        assert!((def.dead - 0.001).abs() < 1e-6);
        assert!((def.sensitivity - 3.0).abs() < 1e-6);
        assert!(def.positive.is_empty());
        assert!(def.negative.is_empty());
        assert!(!def.snap);
        assert!(!def.invert);
    }
}
