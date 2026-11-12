//! Command Queue del MCP (Fase 8): los comandos mutantes se
//! encolan y los ejecuta el bucle del motor en orden de envío, en
//! un punto fijo del frame (nunca a mitad de frame ni en el hilo
//! del cliente MCP). Cada comando guarda su estado y su resultado
//! para poder consultarlos (`glass_queue_status`), y quien llama
//! de forma síncrona espera su resultado por la Condvar.

use serde_json::Value;
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Estado de un comando encolado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdState {
    Pending,
    Running,
    Done,
    Error,
}

impl CmdState {
    pub fn as_str(&self) -> &'static str {
        match self {
            CmdState::Pending => "pending",
            CmdState::Running => "running",
            CmdState::Done => "done",
            CmdState::Error => "error",
        }
    }
}

/// Un comando de la cola, con su desenlace.
#[derive(Debug, Clone)]
pub struct QueuedCommand {
    pub id: u64,
    pub tool: String,
    pub args: Value,
    pub state: CmdState,
    pub result: Option<Value>,
    pub error: Option<String>,
    /// Nº de bombeo en el que se ejecutó (0 = aún no).
    pub pass: u64,
    /// true si se ejecutó al momento por no haber bombeo vivo.
    pub direct: bool,
}

struct Inner {
    next_id: u64,
    pass: u64,
    commands: VecDeque<QueuedCommand>,
    last_pump: Option<Instant>,
}

/// La cola compartida entre el servidor MCP (productor y
/// consultante) y el bucle del motor (consumidor vía bombeo).
pub struct CommandQueue {
    inner: Mutex<Inner>,
    changed: Condvar,
}

/// Cuántos comandos se conservan en el historial.
const HISTORY: usize = 200;

impl CommandQueue {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                next_id: 1,
                pass: 0,
                commands: VecDeque::new(),
                last_pump: None,
            }),
            changed: Condvar::new(),
        }
    }

    /// Anota un bombeo del bucle (latido + contador de pasadas).
    pub fn note_pump(&self) -> u64 {
        let mut g = self.inner.lock().unwrap();
        g.pass += 1;
        g.last_pump = Some(Instant::now());
        g.pass
    }

    /// true si el bucle bombeó hace menos de 2 s.
    pub fn pump_alive(&self) -> bool {
        let g = self.inner.lock().unwrap();
        g.last_pump
            .map(|t| t.elapsed() < Duration::from_secs(2))
            .unwrap_or(false)
    }

    /// Encola un comando y devuelve su id.
    pub fn submit(&self, tool: &str, args: Value) -> u64 {
        let mut g = self.inner.lock().unwrap();
        let id = g.next_id;
        g.next_id += 1;
        g.commands.push_back(QueuedCommand {
            id,
            tool: tool.to_string(),
            args,
            state: CmdState::Pending,
            result: None,
            error: None,
            pass: 0,
            direct: false,
        });
        while g.commands.len() > HISTORY {
            g.commands.pop_front();
        }
        id
    }

    /// Extrae los pendientes en orden de envío, marcándolos Running.
    pub fn take_pending(&self) -> Vec<QueuedCommand> {
        let mut g = self.inner.lock().unwrap();
        let mut out = Vec::new();
        for c in g.commands.iter_mut() {
            if c.state == CmdState::Pending {
                c.state = CmdState::Running;
                out.push(c.clone());
            }
        }
        out
    }

    /// Cierra un comando con su desenlace y despierta a quien espera.
    pub fn complete(&self, id: u64, result: Result<Value, String>, pass: u64, direct: bool) {
        {
            let mut g = self.inner.lock().unwrap();
            if let Some(c) = g.commands.iter_mut().find(|c| c.id == id) {
                match result {
                    Ok(v) => {
                        c.state = CmdState::Done;
                        c.result = Some(v);
                    }
                    Err(e) => {
                        c.state = CmdState::Error;
                        c.error = Some(e);
                    }
                }
                c.pass = pass;
                c.direct = direct;
            }
        }
        self.changed.notify_all();
    }

    /// Espera a que un comando termine (o venza el timeout).
    /// Devuelve el estado final conocido, o None si venció.
    pub fn wait(&self, id: u64, timeout: Duration) -> Option<CmdState> {
        let inicio = Instant::now();
        let mut g = self.inner.lock().unwrap();
        loop {
            if let Some(c) = g.commands.iter().find(|c| c.id == id) {
                if c.state == CmdState::Done || c.state == CmdState::Error {
                    return Some(c.state);
                }
            } else {
                return None;
            }
            if inicio.elapsed() >= timeout {
                return None;
            }
            let restante = timeout - inicio.elapsed();
            let (g2, _) = self
                .changed
                .wait_timeout(g, restante.min(Duration::from_millis(250)))
                .unwrap();
            g = g2;
        }
    }

    /// El registro de un comando.
    pub fn status(&self, id: u64) -> Option<QueuedCommand> {
        let g = self.inner.lock().unwrap();
        g.commands.iter().find(|c| c.id == id).cloned()
    }

    /// Los últimos `n` comandos, del más reciente al más antiguo.
    pub fn recent(&self, n: usize) -> Vec<QueuedCommand> {
        let g = self.inner.lock().unwrap();
        g.commands.iter().rev().take(n).cloned().collect()
    }

    /// (pendientes, completados, errores, total en historial, pasada actual)
    pub fn summary(&self) -> (usize, usize, usize, usize, u64) {
        let g = self.inner.lock().unwrap();
        let mut p = 0;
        let mut d = 0;
        let mut e = 0;
        for c in &g.commands {
            match c.state {
                CmdState::Pending | CmdState::Running => p += 1,
                CmdState::Done => d += 1,
                CmdState::Error => e += 1,
            }
        }
        (p, d, e, g.commands.len(), g.pass)
    }
}

impl Default for CommandQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializa un comando para las respuestas MCP.
pub fn command_json(c: &QueuedCommand) -> Value {
    serde_json::json!({
        "command_id": c.id,
        "tool": c.tool,
        "state": c.state.as_str(),
        "pass": c.pass,
        "direct": c.direct,
        "result": c.result,
        "error": c.error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn orden_y_ciclo_de_vida() {
        let q = CommandQueue::new();
        let a = q.submit("glass_create_entity", json!({}));
        let b = q.submit("glass_modify_component", json!({}));
        assert_eq!((a, b), (1, 2));
        let pend = q.take_pending();
        assert_eq!(pend.len(), 2);
        assert_eq!(pend[0].id, 1);
        assert_eq!(q.take_pending().len(), 0);
        q.complete(a, Ok(json!({"ok": true})), 1, false);
        assert_eq!(q.wait(a, Duration::from_millis(50)), Some(CmdState::Done));
        q.complete(b, Err("fallo".into()), 1, false);
        assert_eq!(q.status(b).unwrap().state, CmdState::Error);
        let (p, d, e, total, _) = q.summary();
        assert_eq!((p, d, e, total), (0, 1, 1, 2));
    }

    #[test]
    fn espera_con_timeout_devuelve_none() {
        let q = CommandQueue::new();
        let a = q.submit("glass_create_entity", json!({}));
        assert_eq!(q.wait(a, Duration::from_millis(30)), None);
    }
}
