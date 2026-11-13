//! Transforms en estructura de arrays (SoA) — Fase 7.
//!
//! La fuente autoral sigue siendo `EntityData.transform` (escenas,
//! editor y scripts no cambian). Este almacén es la CACHÉ DE
//! TRABAJO de la resolución de matrices de mundo: arrays paralelos
//! contiguos (ids, padres como índices, posiciones, rotaciones como
//! cuaterniones, escalas y mundos) y un orden topológico calculado
//! al reconstruir, para resolver todos los mundos en UNA pasada
//! lineal sin HashMap ni vueltas de barrido.
//!
//! Nada de ECS general: es una optimización localizada a los
//! transforms, como manda la Fase 7.

use crate::scene::SceneState;
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct TransformStore {
    pub ids: Vec<u64>,
    /// Índice del padre dentro de los arrays (−1 = raíz). Un padre
    /// inexistente también cuenta como raíz (paridad histórica).
    pub parent_idx: Vec<i32>,
    /// El padre TAL COMO está autorado en la escena (para detectar
    /// cambios estructurales comparando vectores, sin hash).
    pub parent_ids: Vec<Option<u64>>,
    /// Orden en el que todo padre precede a sus hijos.
    pub depth_order: Vec<usize>,
    pub positions: Vec<[f32; 3]>,
    pub rotations: Vec<[f32; 4]>,
    pub scales: Vec<[f32; 3]>,
    pub world: Vec<[f32; 16]>,
    /// Profundidad máxima de la jerarquía (raíz = 0).
    pub max_depth: u32,
}

impl TransformStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// true si la estructura cacheada ya no describe a la escena
    /// (cambió el número de entidades o algún id/padre).
    pub fn structure_differs(&self, scene: &SceneState) -> bool {
        // Comparación directa de vectores (sin hash): la caché
        // guarda los ids y los padres autorados en el orden de la
        // escena, así que basta un barrido lineal de enteros.
        if self.ids.len() != scene.entities.len() {
            return true;
        }
        scene
            .entities
            .iter()
            .enumerate()
            .any(|(i, e)| self.ids[i] != e.id || self.parent_ids[i] != e.parent)
    }

    /// Reconstruye los arrays desde la escena y calcula el orden
    /// topológico y la profundidad máxima.
    pub fn rebuild(&mut self, scene: &SceneState) {
        let n = scene.entities.len();
        self.ids.clear();
        self.parent_idx.clear();
        self.parent_ids.clear();
        self.positions.clear();
        self.rotations.clear();
        self.scales.clear();
        self.world.clear();
        self.depth_order.clear();
        self.ids.reserve(n);
        let pos_de: HashMap<u64, usize> = scene
            .entities
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        for e in &scene.entities {
            self.ids.push(e.id);
            self.parent_ids.push(e.parent);
            self.parent_idx.push(match e.parent {
                None => -1,
                Some(pid) => pos_de.get(&pid).map(|p| *p as i32).unwrap_or(-1),
            });
            self.positions.push(e.transform.position);
            self.rotations.push(e.transform.rotation_quat());
            self.scales.push(e.transform.scale);
            self.world.push(crate::math::mat4_identity());
        }
        // Orden topológico por niveles: raíces primero; en cada
        // nivel, los hijos de los ya colocados. Lo que quede fuera
        // (ciclos) se añade al final: su padre efectivo ya es raíz
        // en parent_idx solo si no existe; un ciclo real se trata
        // como raíz al resolver (paridad con el histórico).
        let mut profundidad: Vec<i32> = vec![-1; n];
        let mut orden: Vec<usize> = Vec::with_capacity(n);
        let mut nivel: Vec<usize> = (0..n).filter(|&i| self.parent_idx[i] < 0).collect();
        let mut prof = 0i32;
        let mut hijos: Vec<Vec<usize>> = vec![Vec::new(); n];
        for i in 0..n {
            if self.parent_idx[i] >= 0 {
                hijos[self.parent_idx[i] as usize].push(i);
            }
        }
        while !nivel.is_empty() {
            let mut siguiente: Vec<usize> = Vec::new();
            for &i in &nivel {
                profundidad[i] = prof;
                orden.push(i);
                siguiente.extend_from_slice(&hijos[i]);
            }
            nivel = siguiente;
            prof += 1;
        }
        self.max_depth = orden
            .iter()
            .map(|&i| profundidad[i].max(0) as u32)
            .max()
            .unwrap_or(0);
        // Ciclos (nunca alcanzados desde una raíz): al final, como
        // raíces (su mundo = su local, igual que el histórico).
        if orden.len() < n {
            let mut en_orden = vec![false; n];
            for &i in &orden {
                en_orden[i] = true;
            }
            for (i, &en_orden_i) in en_orden.iter().enumerate() {
                if !en_orden_i {
                    // Se marca su padre como raíz para resolverlo
                    // sin depender de otro miembro del ciclo.
                    self.parent_idx[i] = -1;
                    orden.push(i);
                }
            }
        }
        self.depth_order = orden;
    }

    /// Refresca los arrays locales desde las entidades (la
    /// estructura ya se validó con `structure_differs`/`rebuild`).
    pub fn refresh_locals(&mut self, scene: &SceneState) {
        for (i, e) in scene.entities.iter().enumerate() {
            self.positions[i] = e.transform.position;
            self.rotations[i] = e.transform.rotation_quat();
            self.scales[i] = e.transform.scale;
        }
    }

    /// Resuelve todos los mundos. En jerarquías grandes y anchas
    /// paraleliza por niveles de `depth_order` (contiguo por
    /// profundidad por construcción en rebuild): dentro de un nivel
    /// ningún elemento es padre de otro, así que cada mundo se
    /// calcula en paralelo leyendo el `world` del padre (de un nivel
    /// ya cerrado) y el resultado se vuelca al cerrar el nivel, sin
    /// leer y escribir `world` a la vez. Si la estructura no
    /// garantiza esa propiedad (restos de ciclos con el padre dentro
    /// del mismo nivel), o por debajo de los umbrales, se usa la
    /// pasada secuencial histórica. Devuelve cuántas matrices se
    /// calcularon.
    pub fn compute_world(&mut self) -> usize {
        let n = self.depth_order.len();
        if n < 8192 {
            return self.compute_world_secuencial();
        }
        // Profundidad de cada posición de depth_order, derivada de
        // parent_idx en una pasada (el padre precede al hijo).
        let mut pos_en_orden = vec![usize::MAX; n];
        for (k, &i) in self.depth_order.iter().enumerate() {
            pos_en_orden[i] = k;
        }
        let mut profundidad = vec![0u32; n];
        for (k, &i) in self.depth_order.iter().enumerate() {
            let p = self.parent_idx[i];
            profundidad[k] = if p >= 0 {
                let pk = pos_en_orden[p as usize];
                if pk == usize::MAX {
                    0
                } else {
                    profundidad[pk] + 1
                }
            } else {
                0
            };
        }
        let mut rangos: Vec<(usize, usize)> = Vec::new();
        let mut inicio = 0usize;
        for k in 1..n {
            if profundidad[k] != profundidad[inicio] {
                rangos.push((inicio, k));
                inicio = k;
            }
        }
        rangos.push((inicio, n));
        let ancho_max = rangos.iter().map(|(a, b)| b - a).max().unwrap_or(0);
        if ancho_max < 512 {
            return self.compute_world_secuencial();
        }
        // Garantía de nivel: ningún padre dentro del mismo rango que
        // su hijo (solo podría pasar con restos de ciclos).
        for &(a, b) in &rangos {
            for k in a..b {
                let p = self.parent_idx[self.depth_order[k]];
                if p >= 0 {
                    let pk = pos_en_orden[p as usize];
                    if pk >= a && pk < b {
                        return self.compute_world_secuencial();
                    }
                }
            }
        }
        use rayon::prelude::*;
        let mut hechas = 0usize;
        for (a, b) in rangos {
            let indices = &self.depth_order[a..b];
            let positions = &self.positions;
            let rotations = &self.rotations;
            let scales = &self.scales;
            let parent_idx = &self.parent_idx;
            let world = &self.world;
            let calculadas: Vec<[f32; 16]> = indices
                .par_iter()
                .map(|&i| {
                    let local = crate::math::mat4_from_trs_quat(
                        positions[i],
                        rotations[i],
                        scales[i],
                    );
                    let p = parent_idx[i];
                    if p >= 0 {
                        crate::math::mat4_mul(world[p as usize], local)
                    } else {
                        local
                    }
                })
                .collect();
            for (j, &i) in indices.iter().enumerate() {
                self.world[i] = calculadas[j];
            }
            hechas += indices.len();
        }
        hechas
    }

    /// La pasada histórica: lineal en orden topológico.
    fn compute_world_secuencial(&mut self) -> usize {
        let mut hechas = 0usize;
        for k in 0..self.depth_order.len() {
            let i = self.depth_order[k];
            let local = crate::math::mat4_from_trs_quat(
                self.positions[i],
                self.rotations[i],
                self.scales[i],
            );
            let p = self.parent_idx[i];
            self.world[i] = if p >= 0 {
                crate::math::mat4_mul(self.world[p as usize], local)
            } else {
                local
            };
            hechas += 1;
        }
        hechas
    }

    /// El resultado en el formato histórico: id → matriz de mundo.
    pub fn world_map(&self) -> HashMap<u64, [f32; 16]> {
        self.ids
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, self.world[i]))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::EntityData;

    /// Algoritmo histórico (referencia): pasada topológica por
    /// vueltas con HashMap; lo irresoluble va como raíz.
    fn mundos_referencia(scene: &SceneState) -> HashMap<u64, [f32; 16]> {
        let mut out: HashMap<u64, [f32; 16]> = HashMap::new();
        let mut pending: Vec<&EntityData> = scene.entities.iter().collect();
        let mut last_len = pending.len() + 1;
        while !pending.is_empty() && pending.len() < last_len {
            last_len = pending.len();
            let mut still: Vec<&EntityData> = Vec::new();
            for ent in pending {
                let pw = match ent.parent {
                    None => Some(crate::math::mat4_identity()),
                    Some(pid) => out.get(&pid).copied(),
                };
                match pw {
                    Some(pw) => {
                        let local = crate::math::mat4_from_trs_quat(
                            ent.transform.position,
                            ent.transform.rotation_quat(),
                            ent.transform.scale,
                        );
                        out.insert(ent.id, crate::math::mat4_mul(pw, local));
                    }
                    None => still.push(ent),
                }
            }
            pending = still;
        }
        for ent in pending {
            out.insert(
                ent.id,
                crate::math::mat4_from_trs_quat(
                    ent.transform.position,
                    ent.transform.rotation_quat(),
                    ent.transform.scale,
                ),
            );
        }
        out
    }

    fn escena_con(modo: &str, n: usize) -> SceneState {
        let mut scene = SceneState::new();
        scene.entities.clear();
        let mut ids: Vec<u64> = Vec::new();
        let mut padre: Option<u64> = None;
        let mut en_cadena = 0;
        for creado in 0..n {
            let parent = match modo {
                "cadenas" => {
                    if en_cadena > 0 {
                        padre
                    } else {
                        None
                    }
                }
                "arbol" => {
                    if creado > 0 {
                        Some(ids[(creado - 1) / 2])
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let id = scene.create_entity(format!("E{creado}"), vec!["Transform".into()]);
            let e = scene.entities.iter_mut().find(|e| e.id == id).unwrap();
            e.transform.position = [creado as f32 * 0.25, (creado % 5) as f32, 1.0];
            e.transform.set_rotation_euler([0.0, (creado % 9) as f32 * 10.0, 0.0]);
            e.parent = parent;
            ids.push(id);
            if modo == "cadenas" {
                padre = Some(id);
                en_cadena = (en_cadena + 1) % 7;
            }
        }
        scene
    }

    fn casi_iguales(a: &[f32; 16], b: &[f32; 16]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn soa_equivale_al_historico() {
        for modo in ["plano", "cadenas", "arbol"] {
            let scene = escena_con(modo, 120);
            let referencia = mundos_referencia(&scene);
            let mut store = TransformStore::new();
            store.rebuild(&scene);
            store.compute_world();
            let mapa = store.world_map();
            assert_eq!(mapa.len(), referencia.len(), "modo {modo}");
            for (id, w) in &referencia {
                let otro = mapa.get(id).expect("falta id");
                assert!(casi_iguales(w, otro), "modo {modo}, id {id}");
            }
        }
    }

    #[test]
    fn paralelo_equivale_a_la_referencia_en_escenas_grandes() {
        // Por encima del umbral (8192) y con niveles anchos (>= 512)
        // compute_world toma el camino paralelo por niveles: el
        // resultado debe coincidir con el algoritmo histórico.
        for (modo, n) in [("plano", 9_000usize), ("arbol", 10_000usize)] {
            let scene = escena_con(modo, n);
            let referencia = mundos_referencia(&scene);
            let mut store = TransformStore::new();
            store.rebuild(&scene);
            let hechas = store.compute_world();
            assert_eq!(hechas, n, "modo {modo}");
            let mapa = store.world_map();
            assert_eq!(mapa.len(), referencia.len(), "modo {modo}");
            for (id, w) in &referencia {
                let otro = mapa.get(id).expect("falta id");
                assert!(casi_iguales(w, otro), "modo {modo}, id {id}");
            }
        }
    }

    #[test]
    fn el_resultado_no_depende_del_orden_en_el_vector() {
        let mut scene = escena_con("cadenas", 60);
        let referencia = mundos_referencia(&scene);
        scene.entities.reverse();
        let mut store = TransformStore::new();
        store.rebuild(&scene);
        store.compute_world();
        let mapa = store.world_map();
        for (id, w) in &referencia {
            assert!(casi_iguales(w, mapa.get(id).unwrap()), "id {id}");
        }
    }

    #[test]
    fn ciclo_se_resuelve_como_raiz() {
        let mut scene = escena_con("plano", 3);
        // Ciclo 5 → 7 → 5 (ids según creación; tomamos los reales).
        let a = scene.entities[0].id;
        let b = scene.entities[1].id;
        scene.entities[0].parent = Some(b);
        scene.entities[1].parent = Some(a);
        let mut store = TransformStore::new();
        store.rebuild(&scene);
        store.compute_world();
        let mapa = store.world_map();
        assert_eq!(mapa.len(), 3);
        // Como raíz: su mundo es su local.
        let ea = &scene.entities[0].transform;
        let local = crate::math::mat4_from_trs_quat(ea.position, ea.rotation_quat(), ea.scale);
        assert!(casi_iguales(&local, mapa.get(&a).unwrap()));
    }
}
