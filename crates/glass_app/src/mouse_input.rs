//! Estado del ratón alimentado por los eventos de winit.
//!
//! Hasta este bloque el motor no registraba el ratón en absoluto: el
//! bucle de eventos solo capturaba teclado. `MouseState` es el acumulador
//! que `window_event` alimenta con `CursorMoved` / `MouseInput` y que
//! `render_frame` consume una vez por frame:
//!
//! - La posición se guarda en píxeles físicos de ventana, tal y como la
//!   entrega winit. La conversión al espacio del Canvas (píxeles del
//!   offscreen de render) se hace al consumir, porque depende del tamaño
//!   actual de ambos.
//! - `left_held` refleja el estado continuo del botón izquierdo.
//! - Los bordes (`left_down_edge` / `left_up_edge`) registran la
//!   transición aunque pulsación y liberación caigan entre dos frames, y
//!   se limpian al consumirlos con `take_edges`: cada transición se
//!   entrega exactamente una vez, que es lo que esperan tanto el
//!   hit-test de botones como `GetMouseButtonDown/Up` de C#.
//!
//! En modo editor los eventos también pasan por aquí (la captura no
//! consume nada: egui-winit recibe su propia copia antes), pero el
//! estado solo se *usa* para el juego en el modo standalone; el editor
//! sigue funcionando exclusivamente con el input de egui, como antes.

/// Estado del ratón entre eventos de winit y frames de render.
#[derive(Debug, Clone, Copy)]
pub struct MouseState {
    /// Última posición conocida, en píxeles físicos de ventana
    /// (origen arriba a la izquierda, como entrega winit).
    pub pos: (f64, f64),
    /// Botón izquierdo actualmente pulsado.
    pub left_held: bool,
    /// Botón derecho actualmente pulsado (Input v2: «mouse1»).
    pub right_held: bool,
    /// Botón central actualmente pulsado (Input v2: «mouse2»).
    pub middle_held: bool,
    down_edge: bool,
    up_edge: bool,
}

impl Default for MouseState {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            pos: (0.0, 0.0),
            left_held: false,
            right_held: false,
            middle_held: false,
            down_edge: false,
            up_edge: false,
        }
    }

    /// Registra un `CursorMoved` de winit.
    pub fn on_cursor_moved(&mut self, x: f64, y: f64) {
        self.pos = (x, y);
    }

    /// Registra un `MouseInput` del botón izquierdo de winit.
    /// Las repeticiones del mismo estado (p. ej. un segundo `Pressed`
    /// sin liberación intermedia) no generan bordes nuevos.
    pub fn on_left_button(&mut self, pressed: bool) {
        if pressed && !self.left_held {
            self.down_edge = true;
        } else if !pressed && self.left_held {
            self.up_edge = true;
        }
        self.left_held = pressed;
    }

    /// Registra un `MouseInput` del botón derecho (sin bordes:
    /// solo lo consume el estado de los ejes de entrada).
    pub fn on_right_button(&mut self, pressed: bool) {
        self.right_held = pressed;
    }

    /// Registra un `MouseInput` del botón central.
    pub fn on_middle_button(&mut self, pressed: bool) {
        self.middle_held = pressed;
    }

    /// Consume los bordes pendientes: los devuelve y los limpia, de modo
    /// que cada transición se procesa una sola vez aunque pulsación y
    /// liberación lleguen antes del mismo frame (clic rápido).
    pub fn take_edges(&mut self) -> (bool, bool) {
        let edges = (self.down_edge, self.up_edge);
        self.down_edge = false;
        self.up_edge = false;
        edges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_posicion_sigue_al_cursor() {
        let mut m = MouseState::new();
        m.on_cursor_moved(120.0, 45.0);
        assert_eq!(m.pos, (120.0, 45.0));
        m.on_cursor_moved(121.5, 46.25);
        assert_eq!(m.pos, (121.5, 46.25));
    }

    #[test]
    fn pulsar_genera_borde_de_bajada_una_sola_vez() {
        let mut m = MouseState::new();
        m.on_left_button(true);
        assert!(m.left_held);
        assert_eq!(m.take_edges(), (true, false));
        // Sin eventos nuevos, el borde no se repite en el frame siguiente.
        assert_eq!(m.take_edges(), (false, false));
        assert!(m.left_held, "el estado continuo sobrevive al consumo de bordes");
    }

    #[test]
    fn soltar_genera_borde_de_subida_y_apaga_held() {
        let mut m = MouseState::new();
        m.on_left_button(true);
        let _ = m.take_edges();
        m.on_left_button(false);
        assert!(!m.left_held);
        assert_eq!(m.take_edges(), (false, true));
    }

    #[test]
    fn clic_rapido_antes_del_frame_entrega_ambos_bordes() {
        let mut m = MouseState::new();
        m.on_left_button(true);
        m.on_left_button(false);
        assert_eq!(m.take_edges(), (true, true));
        assert!(!m.left_held);
    }

    #[test]
    fn estados_repetidos_no_generan_bordes() {
        let mut m = MouseState::new();
        m.on_left_button(true);
        m.on_left_button(true);
        assert_eq!(m.take_edges(), (true, false));
        m.on_left_button(false);
        m.on_left_button(false);
        assert_eq!(m.take_edges(), (false, true));
    }

    #[test]
    fn liberar_sin_pulsar_no_genera_borde() {
        let mut m = MouseState::new();
        m.on_left_button(false);
        assert_eq!(m.take_edges(), (false, false));
        assert!(!m.left_held);
    }
}
