//! Historial de deshacer basado en instantáneas del estado autoral del editor.
//!
//! El historial captura pares `(entities, next_entity_id)` mediante `Snapshot` y usa
//! una huella FNV-1a del JSON serializado para detectar cambios de forma barata.
//! Un vigía (`observe`) consolida un nuevo paso únicamente cuando el estado ha
//! cambiado, no hay una edición en curso y el cambio se mantiene estable durante
//! un frame completo.

use glass_core::scene::EntityData;

/// Número máximo de pasos que conserva la pila de deshacer.
pub const MAX_PASOS: usize = 64;

/// Instantánea del estado autoral del editor.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub entities: Vec<EntityData>,
    pub next_entity_id: u64,
    pub hash: u64,
}

impl Snapshot {
    pub fn new(entities: &[EntityData], next_entity_id: u64) -> Snapshot {
        let entities = entities.to_vec();
        let hash = scene_hash(&entities, next_entity_id);
        Snapshot {
            entities,
            next_entity_id,
            hash,
        }
    }
}

/// Huella FNV-1a de 64 bits sobre el JSON de `(entities, next_entity_id)`.
pub fn scene_hash(entities: &[EntityData], next_entity_id: u64) -> u64 {
    let bytes = match serde_json::to_vec(&(entities, next_entity_id)) {
        Ok(bytes) => bytes,
        Err(_) => return 0,
    };

    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Historial de deshacer/rehacer por instantáneas.
#[derive(Debug)]
pub struct UndoHistory {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    settled: Option<Snapshot>,
    last_seen: u64,
}

impl UndoHistory {
    pub fn new() -> Self {
        UndoHistory {
            undo: Vec::new(),
            redo: Vec::new(),
            settled: None,
            last_seen: 0,
        }
    }

    /// Observa el estado actual y consolida un paso cuando el cambio es estable.
    pub fn observe(&mut self, entities: &[EntityData], next_entity_id: u64, editando: bool) {
        let h = scene_hash(entities, next_entity_id);

        let Some(base_hash) = self.settled.as_ref().map(|snapshot| snapshot.hash) else {
            self.settled = Some(Snapshot::new(entities, next_entity_id));
            self.last_seen = h;
            return;
        };

        if h == base_hash {
            self.last_seen = h;
            return;
        }

        if editando {
            self.last_seen = h;
            return;
        }

        if h != self.last_seen {
            // El cambio se ha visto por primera vez en este frame: aún no es estable.
            self.last_seen = h;
            return;
        }

        // Estable y distinto del estado consolidado: consolidar nuevo paso.
        let anterior = self.settled.take().unwrap();
        self.undo.push(anterior);
        if self.undo.len() > MAX_PASOS {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.settled = Some(Snapshot::new(entities, next_entity_id));
        self.last_seen = h;
    }

    /// Deshace un paso. Devuelve la instantánea que debe aplicarse.
    pub fn undo(&mut self, entities: &[EntityData], next_entity_id: u64) -> Option<Snapshot> {
        let snapshot = self.undo.pop()?;

        self.redo.push(Snapshot::new(entities, next_entity_id));
        self.settled = Some(snapshot.clone());
        self.last_seen = snapshot.hash;

        Some(snapshot)
    }

    /// Rehace un paso. Devuelve la instantánea que debe aplicarse.
    pub fn redo(&mut self, entities: &[EntityData], next_entity_id: u64) -> Option<Snapshot> {
        let snapshot = self.redo.pop()?;

        self.undo.push(Snapshot::new(entities, next_entity_id));
        if self.undo.len() > MAX_PASOS {
            self.undo.remove(0);
        }

        self.settled = Some(snapshot.clone());
        self.last_seen = snapshot.hash;

        Some(snapshot)
    }

    /// Reinicia el historial tomando el estado actual como base limpia.
    pub fn reset(&mut self, entities: &[EntityData], next_entity_id: u64) {
        self.undo.clear();
        self.redo.clear();
        self.settled = Some(Snapshot::new(entities, next_entity_id));
        self.last_seen = self.settled.as_ref().map(|snapshot| snapshot.hash).unwrap_or(0);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn pasos(&self) -> usize {
        self.undo.len()
    }
}

impl Default for UndoHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ent(id: u64, nombre: &str) -> EntityData {
        EntityData::new(id, nombre.to_string(), vec!["Transform".to_string()])
    }

    #[test]
    fn consolida_tras_cambio_estable() {
        let mut historia = UndoHistory::new();
        let a = vec![ent(1, "A")];
        let b = vec![ent(1, "B")];

        historia.observe(&a, 10, false);
        assert!(!historia.can_undo());

        // Primer frame con el cambio: se registra como "visto", aún no consolida.
        historia.observe(&b, 10, false);
        assert!(!historia.can_undo());

        // Segundo frame con el mismo cambio: ya es estable y se consolida.
        historia.observe(&b, 10, false);
        assert!(historia.can_undo());
        assert_eq!(historia.pasos(), 1);
    }

    #[test]
    fn no_consolida_mientras_edita() {
        let mut historia = UndoHistory::new();
        let a = vec![ent(1, "A")];
        let b = vec![ent(1, "B")];

        historia.observe(&a, 10, false);
        assert!(!historia.can_undo());

        // Mientras se edita no se consolida, pero el estado visto sí se actualiza.
        historia.observe(&b, 10, true);
        historia.observe(&b, 10, true);
        assert!(!historia.can_undo());

        // Al dejar de editar, el frame actual ya coincide con `last_seen`:
        // el cambio se considera estable y se consolida en este primer observe estable.
        historia.observe(&b, 10, false);
        assert!(historia.can_undo());
        assert_eq!(historia.pasos(), 1);
    }

    #[test]
    fn deshacer_y_rehacer() {
        let mut historia = UndoHistory::new();
        let a = vec![ent(1, "A")];
        let b = vec![ent(1, "B")];
        let c = vec![ent(1, "C")];
        let next_id = 10;

        historia.observe(&a, next_id, false);

        historia.observe(&b, next_id, false);
        historia.observe(&b, next_id, false);
        assert!(historia.can_undo());

        historia.observe(&c, next_id, false);
        historia.observe(&c, next_id, false);
        assert_eq!(historia.pasos(), 2);

        let b_devuelto = historia.undo(&c, next_id).unwrap();
        assert_eq!(b_devuelto.entities[0].name, "B");
        assert!(historia.can_undo());
        assert!(historia.can_redo());

        let a_devuelto = historia.undo(&b_devuelto.entities, next_id).unwrap();
        assert_eq!(a_devuelto.entities[0].name, "A");
        assert!(!historia.can_undo());
        assert!(historia.can_redo());

        let pasos_antes = historia.pasos();
        let b_rehecho = historia.redo(&a_devuelto.entities, next_id).unwrap();
        assert_eq!(b_rehecho.entities[0].name, "B");
        assert!(historia.can_redo());
        assert_eq!(historia.pasos(), pasos_antes + 1);

        // Observar el estado recién rehecho no debe crear pasos nuevos.
        let pasos_tras_redo = historia.pasos();
        historia.observe(&b_rehecho.entities, next_id, false);
        assert!(historia.can_redo());
        assert_eq!(historia.pasos(), pasos_tras_redo);
    }

    #[test]
    fn tope_de_pasos() {
        let mut historia = UndoHistory::new();
        let next_id = 10;

        let base = vec![ent(1, "Base")];
        historia.observe(&base, next_id, false);

        for i in 0..(MAX_PASOS + 5) {
            let nombre = format!("Estado{}", i);
            let estado = vec![ent(1, &nombre)];
            historia.observe(&estado, next_id, false);
            historia.observe(&estado, next_id, false);
        }

        assert_eq!(historia.pasos(), MAX_PASOS);
    }

    #[test]
    fn reset_limpia() {
        let mut historia = UndoHistory::new();
        let next_id = 10;
        let a = vec![ent(1, "A")];
        let b = vec![ent(1, "B")];

        historia.observe(&a, next_id, false);
        historia.observe(&b, next_id, false);
        historia.observe(&b, next_id, false);

        assert!(historia.can_undo());

        historia.reset(&b, next_id);

        assert!(!historia.can_undo());
        assert!(!historia.can_redo());
    }
}
