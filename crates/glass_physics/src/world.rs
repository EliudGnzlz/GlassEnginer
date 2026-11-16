use std::collections::{HashMap, HashSet, VecDeque};
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::pipeline::ChannelEventCollector;
use rapier3d::prelude::*;
use glass_core::TransformFfi;
use crate::types::{ContactInfo, 
    BodyHandle, BodyType, ColliderConfig, ColliderHandle, ColliderShape, ContactEvent,
    ContactKind,
};

pub struct PhysicsWorld {
    pub gravity: Vector<f32>,
    pub integration_parameters: IntegrationParameters,
    pub physics_pipeline: PhysicsPipeline,
    pub island_manager: IslandManager,
    pub broad_phase: DefaultBroadPhase,
    pub narrow_phase: NarrowPhase,
    pub impulse_joint_set: ImpulseJointSet,
    pub multibody_joint_set: MultibodyJointSet,
    pub ccd_solver: CCDSolver,
    pub rigid_body_set: RigidBodySet,
    pub collider_set: ColliderSet,
    pub query_pipeline: QueryPipeline,
    pub characters: HashMap<u64, crate::character::CharacterState>,
    pub ragdolls: std::collections::HashMap<u64, crate::ragdoll::RagdollState>,
    /// Transform de cada entidad interpolada justo antes del último substep fijo.
    prev_transforms: std::collections::HashMap<u64, TransformFfi>,
    /// Entidades (no handles de cuerpo) con interpolación de render activada.
    interp_bodies: std::collections::HashSet<u64>,
    /// Topes de velocidad (lineal, angular) por cuerpo registrado. Solo
    /// afecta a los cuerpos presentes en el mapa.
    velocity_caps: std::collections::HashMap<u64, (f32, f32)>,
    /// Límite de velocidad de despenetración por cuerpo (m/s). Sin entrada = sin límite.
    depenetration_caps: std::collections::HashMap<u64, f32>,
    /// Posiciones al inicio del substep de los cuerpos con límite de
    /// despenetración: la separación de Rapier es POSICIONAL (medido: el
    /// cuerpo sale de la penetración con vy ≈ 0), así que el límite se
    /// aplica sobre el desplazamiento por substep, no sobre la velocidad.
    depenetration_prev: std::collections::HashMap<u64, [f32; 3]>,
    /// includeLayers por cuerpo (estilo Unity). Sin entrada = u32::MAX (todas).
    rb_include_layers: std::collections::HashMap<u64, u32>,
    /// excludeLayers por cuerpo (estilo Unity). Sin entrada = 0 (ninguna).
    rb_exclude_layers: std::collections::HashMap<u64, u32>,
    /// Máscara base de cada collider (su cfg.mask), clave = índice raw del
    /// ColliderHandle. Filter efectivo = base & include & !exclude del cuerpo.
    layer_base_masks: std::collections::HashMap<u64, u32>,
    /// solverIterations TOTALES guardadas por cuerpo. Sin entrada = 4.
    solver_iterations: std::collections::HashMap<u64, i32>,
    /// solverVelocityIterations TOTALES guardadas por cuerpo. Sin entrada = 4.
    solver_velocity_iterations: std::collections::HashMap<u64, i32>,
    /// automaticCenterOfMass por cuerpo (Unity). Sin entrada = true.
    automatic_center_of_mass: std::collections::HashMap<u64, bool>,
    /// automaticInertiaTensor por cuerpo (Unity). Sin entrada = true.
    automatic_inertia_tensor: std::collections::HashMap<u64, bool>,
    /// Grupos de colisión originales de los colliders de un cuerpo con
    /// detectCollisions desactivado, guardados para poder restaurarlos.
    detect_disabled: std::collections::HashMap<u64, Vec<(rapier3d::geometry::ColliderHandle, rapier3d::geometry::InteractionGroups)>>,
    /// Override de centro de masa / inercia por cuerpo (paridad RB):
    /// objetivos (com local, inercia) y los colliders cuyas densidades se
    /// redujeron a la mitad para poder aplicar el override (ver
    /// `apply_mprops_override`).
    mprops_override: std::collections::HashMap<u64, MpropsOverride>,

    /// Handler de eventos de colisión de Rapier: vuelca los CollisionEvent
    /// de cada substep en el canal que drena `step()`.
    event_handler: ChannelEventCollector,
    event_receiver: crossbeam::channel::Receiver<CollisionEvent>,
    /// Receptor del canal de fuerzas de contacto: el colector de Rapier
    /// exige ambos canales; las fuerzas no se usan aún, se drena y descarta.
    force_event_receiver: crossbeam::channel::Receiver<ContactForceEvent>,
    /// Substeps totales ejecutados (reloj de los ContactEvent).
    step_count: u64,
    /// Eventos desde el último `drain_contact_events`.
    pending_contact_events: Vec<ContactEvent>,
    /// Anillo de los últimos 256 eventos, para inspección (MCP).
    contact_ring: VecDeque<ContactEvent>,
    /// Par de entidades ordenado -> pares de colliders crudos en contacto.
    pair_colliders: std::collections::HashMap<(u64, u64), std::collections::HashSet<(u32, u32)>>,
    /// Par de colliders crudo ordenado -> par de entidades dueño (registrado al Started).
    pair_owner: std::collections::HashMap<(u32, u32), (u64, u64)>,
    /// Par de entidades -> is_trigger con el que se emitió su Enter.
    pair_trigger: std::collections::HashMap<(u64, u64), bool>,
    /// Pares (a, b, is_trigger) en contacto o intersección tras el
    /// último substep; de aquí se deriva Stay.
    active_pairs: HashSet<(u64, u64, bool)>,

    next_handle: u64,
    body_map: HashMap<BodyHandle, (RigidBodyHandle, u64)>, // BodyHandle -> (RapierHandle, entity_id)
    handle_lookup: HashMap<RigidBodyHandle, BodyHandle>,
    accumulator: f32,
    pub fixed_timestep: f32,

    /// Pool propio de la física: aísla el paralelismo de Rapier del pool
    /// global de rayon. Solo se usa dentro de `step()`.
    #[cfg(feature = "parallel")]
    pool: rayon::ThreadPool,
}

/// Aplica la configuración de la Fase 1 a un builder de collider: grupos
/// de interacción (capa propia + máscara), eventos de colisión activos
/// (los consume el registro de contactos del mundo) y modo sensor.
fn apply_config(builder: ColliderBuilder, cfg: ColliderConfig) -> ColliderBuilder {
    // Auditoría (menor): una capa ≥ 32 no cabe en los 32 grupos de
    // Rapier; se mapea con & 31 pero AVISANDO (antes era silencioso
    // y dos capas distintas acababan compartiendo grupo).
    if cfg.layer > 31 {
        log::warn!(
            "Física: capa {} fuera de rango (0..=31); se mapea a {} y puede compartir grupo con ella.",
            cfg.layer,
            cfg.layer & 31
        );
    }
    let groups = InteractionGroups::new(
        Group::from_bits_truncate(1u32 << (cfg.layer & 31)),
        Group::from_bits_truncate(cfg.mask),
    );
    builder
        .collision_groups(groups)
        .active_events(ActiveEvents::COLLISION_EVENTS)
        .active_collision_types(rapier3d::geometry::ActiveCollisionTypes::all())
        .sensor(cfg.is_trigger)
}

/// Hilos del pool de física: GLASS_PHYSICS_THREADS si parsea a un valor >= 1;
/// si no, (available_parallelism - 1) acotado a [1, 4].
#[cfg(feature = "parallel")]
fn physics_thread_count() -> usize {
    std::env::var("GLASS_PHYSICS_THREADS")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                .saturating_sub(1)
                .clamp(1, 4)
        })
}

impl Default for PhysicsWorld {
    fn default() -> Self {
        Self::new()
    }
}

/// Override de centro de masa / inercia de un cuerpo, por componentes
/// separados (como en Unity: fijar solo el COM no congela la inercia,
/// y fijar solo la inercia no congela el COM): `com`/`inertia` son
/// `None` mientras ese componente siga siendo el natural de los
/// colliders. `halved` guarda los colliders cuyas densidades se
/// redujeron al aplicar el override (ver `apply_current_override`).
struct MpropsOverride {
    com: Option<[f32; 3]>,
    inertia: Option<([f32; 3], [f32; 4])>,
    halved: Vec<rapier3d::geometry::ColliderHandle>,
}

/// Factor por el que se reducen las densidades de los colliders al
/// aplicar un override de masa/inercia (ver `apply_mprops_override`).
const MPROPS_DENSITY_SCALE: f32 = 0.5;

impl PhysicsWorld {
    pub fn new() -> Self {
        let (event_sender, event_receiver) = crossbeam::channel::unbounded();
        let (force_sender, force_event_receiver) = crossbeam::channel::unbounded();

        #[cfg(feature = "parallel")]
        let pool = {
            let threads = physics_thread_count();
            log::info!("Física: pool paralelo con {threads} hilo(s).");
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("glass-phys-{i}"))
                .build()
                .expect("Física: no se pudo crear el pool de hilos de rayon")
        };

        Self {
            gravity: vector![0.0, -9.81, 0.0],
            integration_parameters: IntegrationParameters::default(),
            physics_pipeline: PhysicsPipeline::new(),
            island_manager: IslandManager::new(),
            broad_phase: DefaultBroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joint_set: ImpulseJointSet::new(),
            multibody_joint_set: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
            rigid_body_set: RigidBodySet::new(),
            collider_set: ColliderSet::new(),
            query_pipeline: QueryPipeline::new(),
            characters: HashMap::new(),
            ragdolls: HashMap::new(),
            prev_transforms: std::collections::HashMap::new(),
            interp_bodies: std::collections::HashSet::new(),
            velocity_caps: HashMap::new(),
            depenetration_caps: HashMap::new(),
            depenetration_prev: HashMap::new(),
            rb_include_layers: HashMap::new(),
            rb_exclude_layers: HashMap::new(),
            layer_base_masks: HashMap::new(),
            solver_iterations: HashMap::new(),
            solver_velocity_iterations: HashMap::new(),
            automatic_center_of_mass: HashMap::new(),
            automatic_inertia_tensor: HashMap::new(),
            detect_disabled: HashMap::new(),
            mprops_override: HashMap::new(),
            event_handler: ChannelEventCollector::new(event_sender, force_sender),
            event_receiver,
            force_event_receiver,
            step_count: 0,
            pending_contact_events: Vec::new(),
            contact_ring: VecDeque::new(),
            pair_colliders: std::collections::HashMap::new(),
            pair_owner: std::collections::HashMap::new(),
            pair_trigger: std::collections::HashMap::new(),
            active_pairs: HashSet::new(),
            next_handle: 1,
            body_map: HashMap::new(),
            handle_lookup: HashMap::new(),
            accumulator: 0.0,
            fixed_timestep: 1.0 / 60.0,
            #[cfg(feature = "parallel")]
            pool,
        }
    }

    /// Steps the physics simulation using a fixed timestep accumulator to guarantee stability and prevent jitter.
    ///
    /// REGLA (feature `parallel`): `physics_pipeline.step` corre dentro del
    /// pool de rayon. Ningún PhysicsHooks ni EventHandler propio puede llamar
    /// a C# (ni tocar estado no Sync) porque podría ejecutarse en un hilo
    /// `glass-phys-*`. Los eventos solo se encolan y se procesan aquí, en el
    /// hilo llamante, en `collect_contact_events`.
    pub fn step(&mut self, dt: f32) {
        let dt = if dt.is_finite() && dt >= 0.0 { dt } else { 0.0 };
        if !self.accumulator.is_finite() {
            self.accumulator = 0.0;
        }
        self.accumulator += dt;
        let fixed_dt = self.fixed_timestep;
        self.integration_parameters.dt = fixed_dt;

        // Prevent spiral of death by capping max substeps per frame
        let mut substeps = 0;
        while self.accumulator >= fixed_dt && substeps < 4 {
            // Captura por substep: prev queda exactamente un paso fijo detrás
            // de curr, coherente con alpha = acumulador / fixed_timestep. Si el
            // frame no ejecuta substeps, prev se conserva y la mezcla avanza.
            self.capture_prev_transforms();
            self.capture_depenetration_prev();
            #[cfg(feature = "parallel")]
            {
                // Préstamo disjunto explícito: el closure no captura `self` entero.
                let Self {
                    pool,
                    physics_pipeline,
                    gravity,
                    integration_parameters,
                    island_manager,
                    broad_phase,
                    narrow_phase,
                    rigid_body_set,
                    collider_set,
                    impulse_joint_set,
                    multibody_joint_set,
                    ccd_solver,
                    event_handler,
                    ..
                } = self;
                pool.install(|| {
                    physics_pipeline.step(
                        &*gravity,
                        &*integration_parameters,
                        island_manager,
                        broad_phase,
                        narrow_phase,
                        rigid_body_set,
                        collider_set,
                        impulse_joint_set,
                        multibody_joint_set,
                        ccd_solver,
                        None,
                        &(),
                        &*event_handler,
                    )
                });
            }
            #[cfg(not(feature = "parallel"))]
            self.physics_pipeline.step(
                &self.gravity,
                &self.integration_parameters,
                &mut self.island_manager,
                &mut self.broad_phase,
                &mut self.narrow_phase,
                &mut self.rigid_body_set,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                &mut self.ccd_solver,
                None,
                &(),
                &self.event_handler,
            );
            self.accumulator -= fixed_dt;
            // Topes DESPUÉS del step: si se recorta antes, la gravedad
            // del propio substep vuelve a superar el tope (medido:
            // cap 1,0 → 1,12 tras un paso).
            self.apply_velocity_caps();
            // Despenetración también DESPUÉS: el solver del propio
            // substep volvería a superar el límite.
            self.apply_depenetration_caps(fixed_dt);
            substeps += 1;
            self.step_count += 1;
            self.collect_contact_events();
            self.rebuild_active_pairs();
        }
        // Tope de substeps alcanzado: se descarta el exceso para no arrastrar deuda.
        if substeps == 4 && self.accumulator >= fixed_dt {
            self.accumulator = 0.0;
        }
    }

    /// Drena el canal de eventos de Rapier tras un substep: convierte los
    /// CollisionEvent en ContactEvent de entidades (par ordenado), los
    /// acumula para `drain_contact_events` y los guarda en el anillo de
    /// inspección. Los eventos entre colliders del mismo cuerpo se
    /// descartan (no son contacto entre entidades).
    ///
    /// Determinismo: con la narrow phase paralela el orden de llegada al
    /// canal varía. Se drena todo y se ordena con clave total (Started
    /// antes que Stopped; dentro de cada grupo, por par crudo ordenado
    /// (índice, generación)). Un Stopped procesado antes que un Started
    /// del mismo substep generaría Enter/Exit espurios en entidades
    /// multi-collider.
    fn collect_contact_events(&mut self) {
        // Las fuerzas de contacto no se consumen: se drena su canal para
        // que no crezca sin límite.
        while self.force_event_receiver.try_recv().is_ok() {}

        let mut events: Vec<CollisionEvent> = self.event_receiver.try_iter().collect();
        // Los empates (mismo rango y mismo par) son indistinguibles para la
        // máquina de estados, así que el resultado es determinista.
        events.sort_unstable_by_key(|event| {
            let (rank, h1, h2) = match *event {
                CollisionEvent::Started(h1, h2, _) => (0u8, h1, h2),
                CollisionEvent::Stopped(h1, h2, _) => (1u8, h1, h2),
            };
            let (a, b) = (h1.into_raw_parts(), h2.into_raw_parts());
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            (rank, lo, hi)
        });

        for event in events {
            match event {
                CollisionEvent::Started(h1, h2, _) => {
                    let (Some(e1), Some(e2)) =
                        (self.entity_of_collider(h1), self.entity_of_collider(h2))
                    else {
                        continue;
                    };
                    if e1 == e2 {
                        continue;
                    }
                    let pair = if e1 < e2 { (e1, e2) } else { (e2, e1) };
                    let (r1, r2) = (h1.into_raw_parts().0, h2.into_raw_parts().0);
                    let key = if r1 < r2 { (r1, r2) } else { (r2, r1) };
                    self.pair_owner.insert(key, pair);
                    let set = self.pair_colliders.entry(pair).or_default();
                    let first = set.is_empty();
                    set.insert(key);
                    if first {
                        let is_trigger =
                            self.collider_set.get(h1).map(|c| c.is_sensor()).unwrap_or(false)
                                || self.collider_set.get(h2).map(|c| c.is_sensor()).unwrap_or(false);
                        self.pair_trigger.insert(pair, is_trigger);
                        let contact = if is_trigger {
                            None
                        } else {
                            self.contact_info(h1, h2, pair)
                        };
                        self.push_contact_event(pair, ContactKind::Enter, is_trigger, contact);
                    }
                }
                CollisionEvent::Stopped(h1, h2, _) => {
                    let (r1, r2) = (h1.into_raw_parts().0, h2.into_raw_parts().0);
                    let key = if r1 < r2 { (r1, r2) } else { (r2, r1) };
                    let pair = match self.pair_owner.remove(&key) {
                        Some(pair) => pair,
                        None => {
                            let (Some(e1), Some(e2)) =
                                (self.entity_of_collider(h1), self.entity_of_collider(h2))
                            else {
                                continue;
                            };
                            if e1 == e2 {
                                continue;
                            }
                            if e1 < e2 { (e1, e2) } else { (e2, e1) }
                        }
                    };
                    let now_empty = match self.pair_colliders.get_mut(&pair) {
                        Some(set) => {
                            set.remove(&key);
                            set.is_empty()
                        }
                        None => true,
                    };
                    if !now_empty {
                        continue;
                    }
                    self.pair_colliders.remove(&pair);
                    let is_trigger = match self.pair_trigger.remove(&pair) {
                        Some(t) => t,
                        None => {
                            self.collider_set.get(h1).map(|c| c.is_sensor()).unwrap_or(false)
                                || self.collider_set.get(h2).map(|c| c.is_sensor()).unwrap_or(false)
                        }
                    };
                    self.push_contact_event(pair, ContactKind::Exit, is_trigger, None);
                }
            }
        }
    }

    /// Datos del impacto de un par sólido: normal del primer manifold con
    /// puntos, punto medio en mundo, impulso máximo y velocidad relativa
    /// (pair.0 - pair.1). `None` si no hay manifold con puntos.
    fn contact_info(
        &self,
        h1: rapier3d::geometry::ColliderHandle,
        h2: rapier3d::geometry::ColliderHandle,
        pair: (u64, u64),
    ) -> Option<ContactInfo> {
        let pair_np = self.narrow_phase.contact_pair(h1, h2)?;
        let manifold = pair_np.manifolds.iter().find(|m| !m.points.is_empty())?;
        // local_p1 está en el espacio de collider1 del ContactPair, que puede
        // ser h2 si Rapier guardó el par en orden inverso.
        let c1 = self.collider_set.get(pair_np.collider1)?;
        let (t, r) = (c1.translation(), c1.rotation());

        let mut sum = [0.0f32; 3];
        let mut impulse = 0.0f32;
        for p in &manifold.points {
            let w = t + r * p.local_p1.coords;
            sum[0] += w.x;
            sum[1] += w.y;
            sum[2] += w.z;
            impulse = impulse.max(p.data.impulse);
        }
        let n = manifold.points.len() as f32;
        // La normal del manifold apunta de collider1 a collider2. La que
        // se entrega a `pair.0` debe apuntar DEL OTRO HACIA pair.0
        // (semántica Unity): si pair.0 es el dueño de collider1, se
        // invierte. El despacho espejo de la app la vuelve a invertir.
        let cruda = manifold.data.normal;
        let dueno1 = self
            .collider_set
            .get(pair_np.collider1)
            .and_then(|c| c.parent())
            .and_then(|p| self.rigid_body_set.get(p))
            .map(|rb| rb.user_data as u64);
        let signo = if dueno1 == Some(pair.0) { -1.0 } else { 1.0 };
        let normal = [cruda.x * signo, cruda.y * signo, cruda.z * signo];

        let vel = |e: u64| {
            self.get_body_by_entity(e)
                .and_then(|b| self.get_linear_velocity(b))
                .unwrap_or([0.0; 3])
        };
        let (va, vb) = (vel(pair.0), vel(pair.1));
        let relative_velocity = [va[0] - vb[0], va[1] - vb[1], va[2] - vb[2]];

        // El solver de Rapier aún no ha escrito el impulso en el
        // manifold cuando el evento Enter se recoge (mide 0): si es
        // cero, se estima el impulso del impacto como masa reducida
        // por velocidad de aproximación a lo largo de la normal.
        let impulse = if impulse > 0.0 {
            impulse
        } else {
            let masa = |e: u64| -> Option<f32> {
                let h = self.get_body_by_entity(e)?;
                let rb = self.rigid_body_set.get(
                    *self.body_map.get(&h).map(|(r, _)| r)?,
                )?;
                if rb.is_fixed() || rb.is_kinematic() {
                    None
                } else {
                    Some(rb.mass())
                }
            };
            let reducida = match (masa(pair.0), masa(pair.1)) {
                (Some(a), Some(b)) => a * b / (a + b),
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (None, None) => 0.0,
            };
            let acercamiento = -(relative_velocity[0] * normal[0]
                + relative_velocity[1] * normal[1]
                + relative_velocity[2] * normal[2]);
            if acercamiento > 0.0 {
                reducida * acercamiento
            } else {
                0.0
            }
        };

        Some(ContactInfo {
            point: [sum[0] / n, sum[1] / n, sum[2] / n],
            normal,
            impulse,
            relative_velocity,
        })
    }

    /// Registra un evento agregado (por par de entidades) en `pending` y en
    /// el anillo de inspección.
    fn push_contact_event(
        &mut self,
        pair: (u64, u64),
        kind: ContactKind,
        is_trigger: bool,
        contact: Option<ContactInfo>,
    ) {
        let ev = ContactEvent {
            a: pair.0,
            b: pair.1,
            kind,
            is_trigger,
            step: self.step_count,
            contact,
        };
        self.pending_contact_events.push(ev);
        if self.contact_ring.len() >= 256 {
            self.contact_ring.pop_front();
        }
        self.contact_ring.push_back(ev);
    }

    /// Reconstruye los pares activos desde la narrow phase: contactos con
    /// contacto real y pares de intersección (sensores) en verdadera
    /// intersección. Es la foto con la que los consumidores derivan Stay.
    fn rebuild_active_pairs(&mut self) {
        self.active_pairs.clear();
        for pair in self.narrow_phase.contact_pairs() {
            if !pair.has_any_active_contact {
                continue;
            }
            let (Some(e1), Some(e2)) = (
                self.entity_of_collider(pair.collider1),
                self.entity_of_collider(pair.collider2),
            ) else {
                continue;
            };
            if e1 == e2 {
                continue;
            }
            let is_trigger = self
                .collider_set
                .get(pair.collider1)
                .map(|c| c.is_sensor())
                .unwrap_or(false)
                || self
                    .collider_set
                    .get(pair.collider2)
                    .map(|c| c.is_sensor())
                    .unwrap_or(false);
            let (a, b) = if e1 < e2 { (e1, e2) } else { (e2, e1) };
            self.active_pairs.insert((a, b, is_trigger));
        }
        for (h1, h2, intersecting) in self.narrow_phase.intersection_pairs() {
            if !intersecting {
                continue;
            }
            let (Some(e1), Some(e2)) = (self.entity_of_collider(h1), self.entity_of_collider(h2))
            else {
                continue;
            };
            if e1 == e2 {
                continue;
            }
            let (a, b) = if e1 < e2 { (e1, e2) } else { (e2, e1) };
            self.active_pairs.insert((a, b, true));
        }
    }

    /// Entidad propietaria de un collider, vía el `user_data` de su cuerpo.
    pub(crate) fn entity_of_collider(&self, h: rapier3d::geometry::ColliderHandle) -> Option<u64> {
        let collider = self.collider_set.get(h)?;
        let body = self.rigid_body_set.get(collider.parent()?)?;
        Some(body.user_data as u64)
    }

    /// Eventos de contacto desde el último drenado (Enter/Exit; Stay se
    /// deriva de `contact_pairs`).
    pub fn drain_contact_events(&mut self) -> Vec<ContactEvent> {
        std::mem::take(&mut self.pending_contact_events)
    }

    /// Pares en contacto o intersección tras el último substep, ordenados:
    /// (entidad_a, entidad_b, es_trigger).
    pub fn contact_pairs(&self) -> Vec<(u64, u64, bool)> {
        // Auditoría (menor): el mismo par puede estar como contacto
        // sólido (false) y como intersección de sensor (true) a la
        // vez; se fusiona en una sola entrada y manda el contacto.
        let mut fusion: std::collections::HashMap<(u64, u64), bool> =
            std::collections::HashMap::new();
        for &(a, b, is_trigger) in self.active_pairs.iter() {
            fusion
                .entry((a, b))
                .and_modify(|t| *t = *t && is_trigger)
                .or_insert(is_trigger);
        }
        let mut pairs: Vec<(u64, u64, bool)> =
            fusion.into_iter().map(|((a, b), t)| (a, b, t)).collect();
        pairs.sort_unstable();
        pairs
    }

    /// Últimos `limit` eventos del anillo de inspección, en orden cronológico.
    pub fn recent_contact_events(&self, limit: usize) -> Vec<ContactEvent> {
        let skip = self.contact_ring.len().saturating_sub(limit);
        self.contact_ring.iter().skip(skip).copied().collect()
    }

    /// Substeps totales ejecutados por el mundo.
    pub fn step_count(&self) -> u64 {
        self.step_count
    }

    /// Creates a rigid body in the physics world and maps it to an entity.
    pub fn create_rigid_body(
        &mut self,
        entity_id: u64,
        body_type: BodyType,
        pos: [f32; 3],
        rot: [f32; 4],
    ) -> BodyHandle {
        let rapier_type = match body_type {
            BodyType::Dynamic => RigidBodyType::Dynamic,
            BodyType::Static => RigidBodyType::Fixed,
            BodyType::Kinematic => RigidBodyType::KinematicPositionBased,
        };

        // Note: nalgebra Quaternion takes (w, i, j, k) where i=x, j=y, k=z
        // Cuaternión inválido (norma ~0 o no finita) al crear: identidad,
        // nunca NaN (normalizar un cuaternión cero envenena el cuerpo).
        let norma2 = rot[0] * rot[0] + rot[1] * rot[1] + rot[2] * rot[2] + rot[3] * rot[3];
        let unit_quat = if norma2.is_finite() && norma2 >= 1e-12 {
            let quat = nalgebra::Quaternion::new(rot[3], rot[0], rot[1], rot[2]);
            nalgebra::UnitQuaternion::from_quaternion(quat)
        } else {
            nalgebra::UnitQuaternion::identity()
        };

        let mut builder = RigidBodyBuilder::new(rapier_type)
            .translation(vector![pos[0], pos[1], pos[2]])
            .rotation(unit_quat.scaled_axis())
            .user_data(entity_id as u128);

        if rapier_type == RigidBodyType::Dynamic {
            // Las rotaciones quedan libres por defecto (estilo Unity);
            // el bloqueo pasa a ser opción por cuerpo vía set_locked_axes.
            builder = builder.linear_damping(1.5);
        }

        let rigid_body = builder.build();

        let rapier_handle = self.rigid_body_set.insert(rigid_body);
        let handle = self.next_handle;
        self.next_handle += 1;

        self.body_map.insert(handle, (rapier_handle, entity_id));
        self.handle_lookup.insert(rapier_handle, handle);

        handle
    }

    /// Attaches an axis-aligned box collider to a rigid body.
    pub fn attach_box_collider_with_config(
        &mut self,
        body_handle: BodyHandle,
        half_extents: [f32; 3],
        restitution: f32,
        friction: f32,
        cfg: ColliderConfig,
    ) -> Option<ColliderHandle> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;

        let builder = ColliderBuilder::cuboid(half_extents[0], half_extents[1], half_extents[2])
            .restitution(restitution)
            .friction(friction);
        let collider = apply_config(builder, cfg).build();

        let collider_handle = self.collider_set.insert_with_parent(
            collider,
            rapier_handle,
            &mut self.rigid_body_set,
        );

        Some(self.register_collider_layers(collider_handle))
    }

    /// Sin configuración (capa 0, máscara completa, no trigger): la firma
    /// histórica delega en la versión con `ColliderConfig`.
    pub fn attach_box_collider(
        &mut self,
        body_handle: BodyHandle,
        half_extents: [f32; 3],
        restitution: f32,
        friction: f32,
    ) -> Option<ColliderHandle> {
        self.attach_box_collider_with_config(body_handle, half_extents, restitution, friction, ColliderConfig::default())
    }

    /// Attaches a sphere collider to a rigid body.
    pub fn attach_sphere_collider_with_config(
        &mut self,
        body_handle: BodyHandle,
        radius: f32,
        restitution: f32,
        friction: f32,
        cfg: ColliderConfig,
    ) -> Option<ColliderHandle> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;

        let builder = ColliderBuilder::ball(radius)
            .restitution(restitution)
            .friction(friction);
        let collider = apply_config(builder, cfg).build();

        let collider_handle = self.collider_set.insert_with_parent(
            collider,
            rapier_handle,
            &mut self.rigid_body_set,
        );

        Some(self.register_collider_layers(collider_handle))
    }

    /// Sin configuración (capa 0, máscara completa, no trigger): la firma
    /// histórica delega en la versión con `ColliderConfig`.
    pub fn attach_sphere_collider(
        &mut self,
        body_handle: BodyHandle,
        radius: f32,
        restitution: f32,
        friction: f32,
    ) -> Option<ColliderHandle> {
        self.attach_sphere_collider_with_config(body_handle, radius, restitution, friction, ColliderConfig::default())
    }

    /// Attaches a capsule collider to a rigid body. `half_height` es
    /// la mitad del TRAMO RECTO (sin casquetes), como en Rapier.
    pub fn attach_capsule_collider_with_config(
        &mut self,
        body_handle: BodyHandle,
        half_height: f32,
        radius: f32,
        restitution: f32,
        friction: f32,
        cfg: ColliderConfig,
    ) -> Option<ColliderHandle> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;

        let builder = ColliderBuilder::capsule_y(half_height, radius)
            .restitution(restitution)
            .friction(friction);
        let collider = apply_config(builder, cfg).build();

        let collider_handle = self.collider_set.insert_with_parent(
            collider,
            rapier_handle,
            &mut self.rigid_body_set,
        );

        Some(self.register_collider_layers(collider_handle))
    }

    /// Sin configuración (capa 0, máscara completa, no trigger): la firma
    /// histórica delega en la versión con `ColliderConfig`.
    pub fn attach_capsule_collider(
        &mut self,
        body_handle: BodyHandle,
        half_height: f32,
        radius: f32,
        restitution: f32,
        friction: f32,
    ) -> Option<ColliderHandle> {
        self.attach_capsule_collider_with_config(body_handle, half_height, radius, restitution, friction, ColliderConfig::default())
    }

    /// Attaches a cylinder collider to a rigid body. `half_height` es
    /// la mitad de la altura total.
    pub fn attach_cylinder_collider_with_config(
        &mut self,
        body_handle: BodyHandle,
        half_height: f32,
        radius: f32,
        restitution: f32,
        friction: f32,
        cfg: ColliderConfig,
    ) -> Option<ColliderHandle> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;

        let builder = ColliderBuilder::cylinder(half_height, radius)
            .restitution(restitution)
            .friction(friction);
        let collider = apply_config(builder, cfg).build();

        let collider_handle = self.collider_set.insert_with_parent(
            collider,
            rapier_handle,
            &mut self.rigid_body_set,
        );

        Some(self.register_collider_layers(collider_handle))
    }

    /// Sin configuración (capa 0, máscara completa, no trigger): la firma
    /// histórica delega en la versión con `ColliderConfig`.
    pub fn attach_cylinder_collider(
        &mut self,
        body_handle: BodyHandle,
        half_height: f32,
        radius: f32,
        restitution: f32,
        friction: f32,
    ) -> Option<ColliderHandle> {
        self.attach_cylinder_collider_with_config(body_handle, half_height, radius, restitution, friction, ColliderConfig::default())
    }

    /// Adjunta el collider descrito por un `ColliderShape` (la vía
    /// que usa la reconstrucción de escena: la forma ya viene derivada
    /// de la malla por `collider_shape_for_builtin`). Delega en los
    /// attach específicos para que exista una sola construcción de
    /// cada forma en el crate.
    pub fn attach_collider(
        &mut self,
        body_handle: BodyHandle,
        shape: ColliderShape,
        restitution: f32,
        friction: f32,
    ) -> Option<ColliderHandle> {
        match shape {
            ColliderShape::Cuboid { half_extents } => {
                self.attach_box_collider(body_handle, half_extents, restitution, friction)
            }
            ColliderShape::Ball { radius } => {
                self.attach_sphere_collider(body_handle, radius, restitution, friction)
            }
            ColliderShape::Capsule { half_height, radius } => {
                self.attach_capsule_collider(body_handle, half_height, radius, restitution, friction)
            }
            ColliderShape::Cylinder { half_height, radius } => {
                self.attach_cylinder_collider(body_handle, half_height, radius, restitution, friction)
            }
        }
    }

    /// Como `attach_collider` pero con la configuración de la Fase 1
    /// (capa, máscara y trigger) aplicada al collider creado.
    pub fn attach_collider_with_config(
        &mut self,
        body_handle: BodyHandle,
        shape: ColliderShape,
        restitution: f32,
        friction: f32,
        cfg: ColliderConfig,
    ) -> Option<ColliderHandle> {
        match shape {
            ColliderShape::Cuboid { half_extents } => self.attach_box_collider_with_config(
                body_handle, half_extents, restitution, friction, cfg,
            ),
            ColliderShape::Ball { radius } => {
                self.attach_sphere_collider_with_config(body_handle, radius, restitution, friction, cfg)
            }
            ColliderShape::Capsule { half_height, radius } => self
                .attach_capsule_collider_with_config(body_handle, half_height, radius, restitution, friction, cfg),
            ColliderShape::Cylinder { half_height, radius } => self
                .attach_cylinder_collider_with_config(body_handle, half_height, radius, restitution, friction, cfg),
        }
    }

    /// Collider convexo a partir de una nube de puntos (el «mesh collider»
    /// asequible de la caja de herramientas: Rapier calcula el hull). Si los
    /// puntos son degenerados (coplanarios o repetidos), devuelve None y no
    /// crea nada.
    pub fn attach_convex_collider(
        &mut self,
        body_handle: BodyHandle,
        points: &[[f32; 3]],
        restitution: f32,
        friction: f32,
        cfg: ColliderConfig,
    ) -> Option<ColliderHandle> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;
        if points.is_empty()
            || points.len() < 4
            || points.iter().any(|p| !p.iter().all(|c| c.is_finite()))
        {
            return None;
        }
        let rapier_points: Vec<Point<f32>> =
            points.iter().map(|p| Point::new(p[0], p[1], p[2])).collect();
        let hull = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ColliderBuilder::convex_hull(&rapier_points)
        })) {
            Ok(hull) => hull?,
            Err(_) => {
                log::warn!(
                    "attach_convex_collider: convex_hull entró en pánico ({} puntos); collider descartado",
                    rapier_points.len()
                );
                return None;
            }
        };
        let builder = hull.restitution(restitution).friction(friction);
        let collider = apply_config(builder, cfg).build();
        let collider_handle = self.collider_set.insert_with_parent(
            collider,
            rapier_handle,
            &mut self.rigid_body_set,
        );
        Some(self.register_collider_layers(collider_handle))
    }

    /// Igual que `attach_collider_with_config`, pero con un desplazamiento local
    /// `offset` respecto al origen del cuerpo (el collider se inserta con parent,
    /// de modo que Rapier interpreta la `translation` como relativa al cuerpo).
    pub fn attach_collider_with_config_at(
        &mut self,
        body_handle: BodyHandle,
        shape: ColliderShape,
        restitution: f32,
        friction: f32,
        cfg: ColliderConfig,
        offset: [f32; 3],
    ) -> Option<ColliderHandle> {
        // El cuerpo debe existir; si no, no hay nada que hacer.
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;

        // Builder según la forma geométrica.
        let builder = match shape {
            ColliderShape::Cuboid { half_extents } => {
                ColliderBuilder::cuboid(half_extents[0], half_extents[1], half_extents[2])
            }
            ColliderShape::Ball { radius } => ColliderBuilder::ball(radius),
            ColliderShape::Capsule {
                half_height,
                radius,
            } => ColliderBuilder::capsule_y(half_height, radius),
            ColliderShape::Cylinder {
                half_height,
                radius,
            } => ColliderBuilder::cylinder(half_height, radius),
        }
        .restitution(restitution)
        .friction(friction)
        // Offset relativo al cuerpo padre (se respeta al usar insert_with_parent).
        .translation(vector![offset[0], offset[1], offset[2]]);

        let collider = apply_config(builder, cfg).build();
        let collider_handle =
            self.collider_set
                .insert_with_parent(collider, rapier_handle, &mut self.rigid_body_set);
        Some(self.register_collider_layers(collider_handle))
    }

    /// Añade un collider de malla triangular (trimesh) al cuerpo indicado.
    /// La malla es estática en el espacio local del cuerpo (no hay offset:
    /// los vértices vienen ya en coordenadas locales del cuerpo).
    pub fn attach_trimesh_collider(
        &mut self,
        body_handle: BodyHandle,
        vertices: &[[f32; 3]],
        indices: &[[u32; 3]],
        restitution: f32,
        friction: f32,
        cfg: ColliderConfig,
    ) -> Option<ColliderHandle> {
        // El cuerpo debe existir.
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;

        // Validación previa OBLIGATORIA: parry 0.18 no devuelve Err ante
        // índices fuera de rango — indexa los vértices al reconstruir su
        // QBVH y entra en pánico. Sin esta guarda, una malla corrupta
        // tumbaría el motor en vez de devolver None.
        if vertices.is_empty() || indices.is_empty() {
            return None;
        }
        let n_vertices = vertices.len() as u32;
        if indices.iter().flatten().any(|&i| i >= n_vertices) {
            return None;
        }
        if vertices.iter().flatten().any(|c| !c.is_finite()) {
            return None;
        }
        // Una malla cuyos triángulos son TODOS degenerados (índices
        // repetidos o área cero) no colisiona con nada útil: se rechaza
        // entera. Los triángulos degenerados sueltos dentro de una malla
        // válida se dejan pasar (Rapier los tolera).
        // Auditoría (menor): el juicio es RELATIVO a la propia malla
        // (antes, un umbral absoluto de 1e-12 sobre el área² rechazaba
        // mallas válidas en escala milimétrica y aceptaba degeneradas
        // en modelos enormes): basta que exista UN triángulo con área
        // real frente al mayor de la malla.
        let mut max_cruz2 = 0.0f32;
        for t in indices.iter() {
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                continue;
            }
            let a = vertices[t[0] as usize];
            let b = vertices[t[1] as usize];
            let c = vertices[t[2] as usize];
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let cruz = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let cruz2: f32 = cruz.iter().map(|v| v * v).sum();
            if cruz2 > max_cruz2 {
                max_cruz2 = cruz2;
            }
        }
        if !matches!(
            max_cruz2.partial_cmp(&0.0),
            Some(std::cmp::Ordering::Greater)
        ) {
            return None;
        }

        // Conversión a los tipos de Rapier.
        let puntos: Vec<Point<f32>> = vertices
            .iter()
            .map(|v| Point::new(v[0], v[1], v[2]))
            .collect();
        let triangulos: Vec<[u32; 3]> = indices.to_vec();

        // `trimesh` devuelve Result; este método devuelve Option -> .ok()?
        // (malla degenerada, índices fuera de rango, etc. -> None, sin pánico).
        let builder = ColliderBuilder::trimesh(puntos, triangulos)
            .ok()?
            .restitution(restitution)
            .friction(friction);

        let collider = apply_config(builder, cfg).build();
        let collider_handle =
            self.collider_set
                .insert_with_parent(collider, rapier_handle, &mut self.rigid_body_set);
        Some(self.register_collider_layers(collider_handle))
    }

    /// Gets the current transform of a rigid body.
    pub fn get_transform(&self, body_handle: BodyHandle) -> Option<TransformFfi> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;
        let rb = self.rigid_body_set.get(rapier_handle)?;
        let translation = rb.translation();
        let rotation = rb.rotation();
        let quat = rotation.quaternion();

        Some(TransformFfi {
            position: [translation.x, translation.y, translation.z],
            rotation: [quat.i, quat.j, quat.k, quat.w],
            scale: [1.0, 1.0, 1.0],
        })
    }

    /// Directly sets the transform of a rigid body (e.g. for kinematic bodies or teleporting).
    pub fn set_transform(&mut self, body_handle: BodyHandle, pos: [f32; 3], rot: [f32; 4]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        // Un cuaternión de norma ~0 o no finita no es una rotación válida:
        // normalizarlo produciría NaN y envenenaría el cuerpo (hallado por
        // la sonda física RBCC100: PublishTransform con el transform por
        // defecto (0,0,0,0) dejaba el cuerpo con rotación NaN).
        let norma2 = rot[0] * rot[0] + rot[1] * rot[1] + rot[2] * rot[2] + rot[3] * rot[3];
        if !norma2.is_finite() || norma2 < 1e-12 {
            return false;
        }

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            let quat = nalgebra::Quaternion::new(rot[3], rot[0], rot[1], rot[2]);
            let unit_quat = nalgebra::UnitQuaternion::from_quaternion(quat);
            rb.set_translation(vector![pos[0], pos[1], pos[2]], true);
            rb.set_rotation(unit_quat, true);
            true
        } else {
            false
        }
    }

    /// Applies an instantaneous linear impulse to a rigid body.
    pub fn apply_impulse(&mut self, body_handle: BodyHandle, impulse: [f32; 3]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.apply_impulse(vector![impulse[0], impulse[1], impulse[2]], true);
            true
        } else {
            false
        }
    }

    /// Applies a continuous force to a rigid body.
    pub fn apply_force(&mut self, body_handle: BodyHandle, force: [f32; 3]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.add_force(vector![force[0], force[1], force[2]], true);
            true
        } else {
            false
        }
    }

    /// Fuerza en coordenadas locales del cuerpo, rotada a mundo con la rotación actual.
    pub fn add_relative_force(&mut self, body_handle: BodyHandle, force: [f32; 3]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            let world_force = *rb.rotation() * vector![force[0], force[1], force[2]];
            rb.add_force(world_force, true);
            true
        } else {
            false
        }
    }

    /// Impulso en coordenadas locales del cuerpo, rotado a mundo con la rotación actual.
    pub fn apply_relative_impulse(&mut self, body_handle: BodyHandle, impulse: [f32; 3]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            let world_impulse = *rb.rotation() * vector![impulse[0], impulse[1], impulse[2]];
            rb.apply_impulse(world_impulse, true);
            true
        } else {
            false
        }
    }

    /// Suma viva de fuerzas de usuario desde el último `reset_forces` (Rapier no resetea por paso).
    pub fn get_accumulated_force(&self, body_handle: BodyHandle) -> Option<[f32; 3]> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;
        let f = self.rigid_body_set.get(rapier_handle)?.user_force();
        Some([f.x, f.y, f.z])
    }

    /// Suma viva de torques de usuario desde el último `reset_torques` (Rapier no resetea por paso).
    pub fn get_accumulated_torque(&self, body_handle: BodyHandle) -> Option<[f32; 3]> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;
        let t = self.rigid_body_set.get(rapier_handle)?.user_torque();
        Some([t.x, t.y, t.z])
    }

    /// Gets the linear velocity of a rigid body.
    pub fn get_linear_velocity(&self, body_handle: BodyHandle) -> Option<[f32; 3]> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;
        let rb = self.rigid_body_set.get(rapier_handle)?;
        let linvel = rb.linvel();
        Some([linvel.x, linvel.y, linvel.z])
    }

    /// Sets the linear velocity of a rigid body directly.
    pub fn set_linear_velocity(&mut self, body_handle: BodyHandle, linvel: [f32; 3]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.set_linvel(vector![linvel[0], linvel[1], linvel[2]], true);
            true
        } else {
            false
        }
    }

    /// Gets the angular velocity of a rigid body.
    pub fn get_angular_velocity(&self, body_handle: BodyHandle) -> Option<[f32; 3]> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;
        let rb = self.rigid_body_set.get(rapier_handle)?;
        let angvel = rb.angvel();
        Some([angvel.x, angvel.y, angvel.z])
    }

    /// Sets the angular velocity of a rigid body directly.
    pub fn set_angular_velocity(&mut self, body_handle: BodyHandle, angvel: [f32; 3]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.set_angvel(vector![angvel[0], angvel[1], angvel[2]], true);
            true
        } else {
            false
        }
    }

    /// Applies a continuous torque to a rigid body.
    pub fn add_torque(&mut self, body_handle: BodyHandle, torque: [f32; 3]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.add_torque(vector![torque[0], torque[1], torque[2]], true);
            true
        } else {
            false
        }
    }

    /// Applies an instantaneous torque impulse to a rigid body.
    pub fn apply_torque_impulse(&mut self, body_handle: BodyHandle, impulse: [f32; 3]) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.apply_torque_impulse(vector![impulse[0], impulse[1], impulse[2]], true);
            true
        } else {
            false
        }
    }

    /// Applies a continuous force at a specific world point of a rigid body.
    pub fn add_force_at_point(
        &mut self,
        body_handle: BodyHandle,
        force: [f32; 3],
        point: [f32; 3],
    ) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.add_force_at_point(
                vector![force[0], force[1], force[2]],
                point![point[0], point[1], point[2]],
                true,
            );
            true
        } else {
            false
        }
    }

    /// Applies an instantaneous impulse at a specific world point of a rigid body.
    pub fn apply_impulse_at_point(
        &mut self,
        body_handle: BodyHandle,
        impulse: [f32; 3],
        point: [f32; 3],
    ) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.apply_impulse_at_point(
                vector![impulse[0], impulse[1], impulse[2]],
                point![point[0], point[1], point[2]],
                true,
            );
            true
        } else {
            false
        }
    }

    /// Applies an instantaneous explosion impulse to a rigid body.
    ///
    /// `center` es el centro de la explosión; `upwards` desplaza el centro
    /// efectivo hacia arriba antes de calcular la dirección (pero no afecta
    /// a la distancia usada para la atenuación, que siempre usa `center`
    /// sin desplazar). Si `distancia >= radius` o `radius <= 0` no hace nada
    /// y devuelve `false`.
    pub fn explosion_impulse(
        &mut self,
        body_handle: BodyHandle,
        center: [f32; 3],
        radius: f32,
        force: f32,
        upwards: f32,
    ) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if radius <= 0.0 {
            return false;
        }

        let rb = match self.rigid_body_set.get_mut(rapier_handle) {
            Some(rb) => rb,
            None => return false,
        };

        let world_com = rb.position() * rb.mass_properties().local_mprops.local_com;
        let center_point = point![center[0], center[1], center[2]];
        let distance = (world_com - center_point).norm();
        if distance >= radius {
            return false;
        }

        let effective_center = point![center[0], center[1] - upwards, center[2]];
        let mut dir = world_com - effective_center;
        if distance < 1e-5 {
            dir = vector![0.0, 1.0, 0.0];
        } else {
            dir = dir.normalize();
        }

        let factor = 1.0 - distance / radius;
        let impulse = dir * (force * factor);
        rb.apply_impulse_at_point(impulse, world_com, true);
        true
    }

    /// Applies a continuous explosion force to a rigid body. Misma semántica
    /// que [`PhysicsWorld::explosion_impulse`] pero usando `add_force_at_point`.
    pub fn explosion_force(
        &mut self,
        body_handle: BodyHandle,
        center: [f32; 3],
        radius: f32,
        force: f32,
        upwards: f32,
    ) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if radius <= 0.0 {
            return false;
        }

        let rb = match self.rigid_body_set.get_mut(rapier_handle) {
            Some(rb) => rb,
            None => return false,
        };

        let world_com = rb.position() * rb.mass_properties().local_mprops.local_com;
        let center_point = point![center[0], center[1], center[2]];
        let distance = (world_com - center_point).norm();
        if distance >= radius {
            return false;
        }

        let effective_center = point![center[0], center[1] - upwards, center[2]];
        let mut dir = world_com - effective_center;
        if distance < 1e-5 {
            dir = vector![0.0, 1.0, 0.0];
        } else {
            dir = dir.normalize();
        }

        let factor = 1.0 - distance / radius;
        let force_vec = dir * (force * factor);
        rb.add_force_at_point(force_vec, world_com, true);
        true
    }

    /// Sets the velocity caps (max linear, max angular) for a registered
    /// rigid body. Los valores no finitos o negativos se guardan como
    /// `f32::INFINITY` (sin tope).
    pub fn set_velocity_caps(&mut self, body_handle: BodyHandle, max_linear: f32, max_angular: f32) {
        let lin = if max_linear.is_finite() && max_linear >= 0.0 {
            max_linear
        } else {
            f32::INFINITY
        };
        let ang = if max_angular.is_finite() && max_angular >= 0.0 {
            max_angular
        } else {
            f32::INFINITY
        };
        self.velocity_caps.insert(body_handle, (lin, ang));
    }

    /// Fija la velocidad máxima (m/s) con la que un contacto penetrante puede
    /// separar el cuerpo. No finito o negativo = sin límite. `false` si el cuerpo no existe.
    pub fn set_max_depenetration_velocity(&mut self, body_handle: BodyHandle, max: f32) -> bool {
        if !self.body_map.contains_key(&body_handle) {
            return false;
        }
        let v = if max.is_finite() && max >= 0.0 {
            max
        } else {
            f32::INFINITY
        };
        self.depenetration_caps.insert(body_handle, v);
        true
    }

    /// `None` si el cuerpo no existe; `Some(INFINITY)` si no tiene límite.
    pub fn get_max_depenetration_velocity(&self, body_handle: BodyHandle) -> Option<f32> {
        if !self.body_map.contains_key(&body_handle) {
            return None;
        }
        Some(
            self.depenetration_caps
                .get(&body_handle)
                .copied()
                .unwrap_or(f32::INFINITY),
        )
    }

    pub fn clear_max_depenetration_velocity(&mut self, body_handle: BodyHandle) {
        self.depenetration_caps.remove(&body_handle);
    }

    /// Guarda la posición de los cuerpos con límite finito al inicio del
    /// substep (antes de que corra el pipeline).
    fn capture_depenetration_prev(&mut self) {
        self.depenetration_prev.clear();
        if self.depenetration_caps.is_empty() {
            return;
        }
        for (&bh, &limite) in self.depenetration_caps.iter() {
            if !limite.is_finite() {
                continue;
            }
            let Some(&(rh, _)) = self.body_map.get(&bh) else {
                continue;
            };
            let Some(rb) = self.rigid_body_set.get(rh) else {
                continue;
            };
            let tr = rb.translation();
            self.depenetration_prev.insert(bh, [tr.x, tr.y, tr.z]);
        }
    }

    /// Aplica el límite de velocidad de despenetración tras cada substep.
    /// La separación de Rapier es posicional (el solver corrige la posición
    /// sin dejar velocidad medible: verificado con sonda, vy ≈ 0 mientras
    /// el cuerpo sale), así que el límite se hace cumplir sobre el
    /// DESPLAZAMIENTO de separación del substep: si el cuerpo se separó
    /// más de `limite * dt` a lo largo de la normal de un contacto
    /// penetrante, se devuelve el exceso. Como guarda adicional se recorta
    /// también la componente de velocidad lineal en esa dirección.
    fn apply_depenetration_caps(&mut self, dt: f32) {
        if self.depenetration_caps.is_empty() {
            return;
        }
        let limitados: Vec<(u64, RigidBodyHandle, f32)> = self
            .depenetration_caps
            .iter()
            .filter(|(_, limite)| limite.is_finite())
            .filter_map(|(bh, limite)| self.body_map.get(bh).map(|&(rh, _)| (*bh, rh, *limite)))
            .collect();

        for (bh, rh, limite) in limitados {
            let Some(prev) = self.depenetration_prev.get(&bh).copied() else {
                continue;
            };
            let Some(rb) = self.rigid_body_set.get(rh) else {
                continue;
            };
            if rb.is_sleeping() {
                continue;
            }
            let tr = rb.translation();
            let mut pos = [tr.x, tr.y, tr.z];
            let mut linvel = *rb.linvel();
            let mut movido = false;
            let mut velocidad_tocada = false;

            for pair in self.narrow_phase.contact_pairs() {
                if !pair.has_any_active_contact {
                    continue;
                }
                for manifold in &pair.manifolds {
                    let signo = if manifold.data.rigid_body2 == Some(rh) {
                        1.0
                    } else if manifold.data.rigid_body1 == Some(rh) {
                        -1.0
                    } else {
                        continue;
                    };
                    if !manifold.points.iter().any(|pt| pt.dist < 0.0) {
                        continue;
                    }
                    let dir = manifold.data.normal * signo;
                    // Desplazamiento de separación de este substep.
                    let disp = (pos[0] - prev[0]) * dir.x
                        + (pos[1] - prev[1]) * dir.y
                        + (pos[2] - prev[2]) * dir.z;
                    let max_disp = limite * dt;
                    if disp > max_disp {
                        let exceso = disp - max_disp;
                        pos[0] -= dir.x * exceso;
                        pos[1] -= dir.y * exceso;
                        pos[2] -= dir.z * exceso;
                        movido = true;
                    }
                    // Guarda de velocidad: misma cota sobre la velocidad viva.
                    let v_sep = linvel.dot(&dir);
                    if v_sep > limite {
                        linvel -= dir * (v_sep - limite);
                        velocidad_tocada = true;
                    }
                }
            }

            if movido || velocidad_tocada {
                if let Some(rb) = self.rigid_body_set.get_mut(rh) {
                    if movido {
                        rb.set_translation(vector![pos[0], pos[1], pos[2]], true);
                    }
                    if velocidad_tocada {
                        rb.set_linvel(linvel, true);
                    }
                }
            }
        }
    }

    /// Clears the velocity caps for a rigid body.
    pub fn clear_velocity_caps(&mut self, body_handle: BodyHandle) {
        self.velocity_caps.remove(&body_handle);
    }

    /// Recorta la velocidad lineal y angular de todos los cuerpos con topes
    /// configurados. Se invoca una vez por substep.
    fn apply_velocity_caps(&mut self) {
        for (&body_handle, &(max_linear, max_angular)) in self.velocity_caps.iter() {
            let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
                Some(v) => v,
                None => continue,
            };
            if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
                if max_linear.is_finite() {
                    let linvel = *rb.linvel();
                    let speed = linvel.norm();
                    if speed > max_linear && speed > 0.0 {
                        rb.set_linvel(linvel * (max_linear / speed), true);
                    }
                }
                if max_angular.is_finite() {
                    let angvel = *rb.angvel();
                    let speed = angvel.norm();
                    if speed > max_angular && speed > 0.0 {
                        rb.set_angvel(angvel * (max_angular / speed), true);
                    }
                }
            }
        }
    }

    /// Pone a dormir un cuerpo rígido.
    pub fn sleep_body(&mut self, body_handle: BodyHandle) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.sleep();
            true
        } else {
            false
        }
    }

    /// Despierta un cuerpo rígido.
    pub fn wake_body(&mut self, body_handle: BodyHandle) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            rb.wake_up(true);
            true
        } else {
            false
        }
    }

    /// Indica si un cuerpo rígido está durmiendo.
    pub fn is_sleeping(&self, body_handle: BodyHandle) -> Option<bool> {
        let &(rapier_handle, _) = self.body_map.get(&body_handle)?;
        let rb = self.rigid_body_set.get(rapier_handle)?;
        Some(rb.is_sleeping())
    }

    /// Fija el umbral de sueño de un cuerpo con la semántica de energía
    /// específica al estilo Unity (por defecto `0.005`). Se traduce a los
    /// umbrales de velocidad lineal/angular de Rapier con:
    /// `lin = sqrt(2.0 * threshold)`, `ang = lin * 1.25` (la razón 1.25 viene
    /// de los valores por defecto de Rapier, 0.5 lineal / 0.4 angular). Si
    /// `threshold <= 0.0` se fija `lin = -1.0` y `ang = -1.0`, lo que en
    /// Rapier significa que el cuerpo nunca duerme automáticamente.
    pub fn set_sleep_threshold(&mut self, body_handle: BodyHandle, threshold: f32) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        if let Some(rb) = self.rigid_body_set.get_mut(rapier_handle) {
            let (lin, ang) = if threshold > 0.0 {
                let lin = (2.0 * threshold).sqrt();
                (lin, lin * 1.25)
            } else {
                (-1.0, -1.0)
            };
            let act = rb.activation_mut();
            act.normalized_linear_threshold = lin;
            act.angular_threshold = ang;
            true
        } else {
            false
        }
    }

    /// Activa o desactiva la detección de colisiones de un cuerpo sin
    /// eliminarlo de la simulación. Internamente fija el filtro de los
    /// colliders del cuerpo a `Group::NONE` (conservando la membresía
    /// original) para que no participen en el solver ni en consultas con
    /// grupos, y restaura los grupos originales al reactivar.
    ///
    /// Si el cuerpo se elimina mientras tiene la detección desactivada, la
    /// entrada de `detect_disabled` queda huérfana; esto es inofensivo
    /// porque los handles de cuerpo no se reutilizan sin volver a registrar
    /// una entrada nueva.
    pub fn set_detect_collisions(&mut self, body_handle: BodyHandle, enabled: bool) -> bool {
        let &(rapier_handle, _) = match self.body_map.get(&body_handle) {
            Some(v) => v,
            None => return false,
        };

        let collider_handles: Vec<rapier3d::geometry::ColliderHandle> = match self
            .rigid_body_set
            .get(rapier_handle)
        {
            Some(rb) => rb.colliders().to_vec(),
            None => return false,
        };

        if !enabled {
            if self.detect_disabled.contains_key(&body_handle) {
                return true;
            }
            let mut saved = Vec::with_capacity(collider_handles.len());
            for handle in collider_handles {
                if let Some(collider) = self.collider_set.get_mut(handle) {
                    let original_groups = collider.collision_groups();
                    saved.push((handle, original_groups));
                    collider.set_collision_groups(rapier3d::geometry::InteractionGroups::new(
                        original_groups.memberships,
                        rapier3d::geometry::Group::NONE,
                    ));
                }
            }
            self.detect_disabled.insert(body_handle, saved);
        } else if let Some(saved) = self.detect_disabled.remove(&body_handle) {
            for (handle, original_groups) in saved {
                if let Some(collider) = self.collider_set.get_mut(handle) {
                    collider.set_collision_groups(original_groups);
                }
            }
        }

        true
    }

    /// Removes a rigid body and its attached colliders.
    pub fn set_locked_axes(&mut self, body_handle: BodyHandle, bits: u8) -> bool {
        if let Some((handle, _)) = self.get_rapier_handle(body_handle) {
            if let Some(body) = self.rigid_body_set.get_mut(handle) {
                let mut axes = rapier3d::dynamics::LockedAxes::empty();
                if bits & 1 != 0 {
                    axes |= rapier3d::dynamics::LockedAxes::TRANSLATION_LOCKED_X;
                }
                if bits & 2 != 0 {
                    axes |= rapier3d::dynamics::LockedAxes::TRANSLATION_LOCKED_Y;
                }
                if bits & 4 != 0 {
                    axes |= rapier3d::dynamics::LockedAxes::TRANSLATION_LOCKED_Z;
                }
                if bits & 8 != 0 {
                    axes |= rapier3d::dynamics::LockedAxes::ROTATION_LOCKED_X;
                }
                if bits & 16 != 0 {
                    axes |= rapier3d::dynamics::LockedAxes::ROTATION_LOCKED_Y;
                }
                if bits & 32 != 0 {
                    axes |= rapier3d::dynamics::LockedAxes::ROTATION_LOCKED_Z;
                }
                body.set_locked_axes(axes, true);
                return true;
            }
        }
        false
    }

    pub fn set_mass(&mut self, handle: BodyHandle, mass: f32) -> bool {
        if !mass.is_finite() || mass <= 0.0 {
            return false;
        }
        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return false;
        };
        // Con un override de COM/inercia activo las densidades están a
        // la mitad: restaurarlas primero para que `current` sea la masa
        // natural y el factor salga bien (la reaplicación del override
        // al final vuelve a reducirlas).
        self.restore_mprops_densities(handle);
        let Some(rb) = self.rigid_body_set.get(rapier) else {
            return false;
        };
        // Se suma la masa de los colliders (no rb.mass()) porque rb.mass() no se
        // actualiza hasta el siguiente step: dos set_mass seguidos acumularían el factor.
        let colliders: Vec<(rapier3d::geometry::ColliderHandle, f32, f32)> = rb
            .colliders()
            .iter()
            .filter_map(|&ch| self.collider_set.get(ch).map(|c| (ch, c.density(), c.mass())))
            .collect();
        let current: f32 = colliders.iter().map(|&(_, _, m)| m).sum();
        if current <= 1e-6 {
            return false;
        }
        let factor = mass / current;
        for (ch, density, _) in colliders {
            if let Some(c) = self.collider_set.get_mut(ch) {
                c.set_density(density * factor);
            }
        }
        // Con un override de COM/inercia activo, el reescalado de
        // densidades mueve S y el override quedaría obsoleto: se
        // reaplica con los mismos componentes fijados (apply restaura
        // las densidades a su natural —ahora la nueva masa— y vuelve a
        // reducirlas a la mitad; lo no fijado toma el natural nuevo).
        if self.mprops_override.contains_key(&handle) {
            return self.apply_current_override(handle);
        }
        true
    }

    pub fn get_mass(&self, handle: BodyHandle) -> Option<f32> {
        let (rapier, _) = self.get_rapier_handle(handle)?;
        self.rigid_body_set.get(rapier).map(|rb| rb.mass())
    }

    /// Centro de masa en el espacio LOCAL del cuerpo. Si hay un override
    /// de COM activo se devuelve su objetivo (siempre fresco, aunque las
    /// `local_mprops` cacheadas de Rapier aún no se hayan actualizado).
    pub fn get_center_of_mass(&self, handle: BodyHandle) -> Option<[f32; 3]> {
        if let Some(com) = self.com_override_activo(handle) {
            return Some(com);
        }
        let (rapier, _) = self.get_rapier_handle(handle)?;
        let rb = self.rigid_body_set.get(rapier)?;
        let com = rb.mass_properties().local_mprops.local_com;
        Some(com.coords.into())
    }

    /// Centro de masa en espacio MUNDIAL (fresco, recalculado desde la
    /// pose actual, no desde el campo `world_com` que puede desactualizarse).
    pub fn get_world_center_of_mass(&self, handle: BodyHandle) -> Option<[f32; 3]> {
        let (rapier, _) = self.get_rapier_handle(handle)?;
        let rb = self.rigid_body_set.get(rapier)?;
        let local_com = self.effective_local_com(handle, rb);
        let world_com = rb.position() * local_com;
        Some(world_com.coords.into())
    }

    /// COM local efectivo: el objetivo del override de COM si lo hay;
    /// si no, el de las `local_mprops` de Rapier.
    fn effective_local_com(&self, handle: BodyHandle, rb: &RigidBody) -> Point<f32> {
        if let Some(com) = self.com_override_activo(handle) {
            return Point::new(com[0], com[1], com[2]);
        }
        rb.mass_properties().local_mprops.local_com
    }

    /// COM manual del override, solo si automaticCenterOfMass está en false.
    fn com_override_activo(&self, handle: BodyHandle) -> Option<[f32; 3]> {
        if self
            .automatic_center_of_mass
            .get(&handle)
            .copied()
            .unwrap_or(true)
        {
            return None;
        }
        self.mprops_override.get(&handle).and_then(|ov| ov.com)
    }

    /// Inercia manual del override, solo si automaticInertiaTensor está en false.
    fn inertia_override_activo(&self, handle: BodyHandle) -> Option<([f32; 3], [f32; 4])> {
        if self
            .automatic_inertia_tensor
            .get(&handle)
            .copied()
            .unwrap_or(true)
        {
            return None;
        }
        self.mprops_override.get(&handle).and_then(|ov| ov.inertia)
    }

    /// Suma de las propiedades de masa de SOLO los colliders del cuerpo
    /// (sin la masa adicional establecida vía `set_additional_mass_properties`),
    /// cada una transformada a la pose local del cuerpo.
    fn colliders_mass_properties(&self, rb: &RigidBody) -> MassProperties {
        let mut total = MassProperties::default();
        let inv_pose = rb.position().inverse();
        for &h in rb.colliders() {
            if let Some(co) = self.collider_set.get(h) {
                let local = inv_pose * co.position();
                total += co.mass_properties().transform_by(&local);
            }
        }
        total
    }

    /// Inercia principal saneada desde `inv_principal_inertia_sqrt`: si
    /// alguna componente es 0 (eje sin inercia, p. ej. masa cero), se
    /// sustituye por una inercia enorme (1e9) para evitar una división
    /// por cero al invertir.
    fn inercia_principal_saneada(inv_sqrt: Vector<f32>) -> [f32; 3] {
        let mut out = [0.0f32; 3];
        for i in 0..3 {
            let s = inv_sqrt[i];
            out[i] = if s > 0.0 { 1.0 / (s * s) } else { 1.0e9 };
        }
        out
    }

    /// Restaura las densidades de los colliders reducidos por un override
    /// anterior a su valor natural (× 1/0.5) y vacía la lista de
    /// reducidos, para que una reaplicación parta de densidades naturales.
    /// Un collider añadido después del override no está en la lista y no
    /// se toca.
    fn restore_mprops_densities(&mut self, handle: BodyHandle) {
        if let Some(ov) = self.mprops_override.get_mut(&handle) {
            let halved = std::mem::take(&mut ov.halved);
            for ch in halved {
                if let Some(co) = self.collider_set.get_mut(ch) {
                    let d = co.density();
                    co.set_density(d / MPROPS_DENSITY_SCALE);
                }
            }
        }
    }

    /// Aplica (o limpia) el override de COM/inercia guardado en
    /// `mprops_override` para el cuerpo.
    ///
    /// POR QUÉ NO VALE `adicional = D - S` A SECAS: la suma/resta de
    /// `MassProperties` de parry es ponderada por masa. Cuando el total
    /// deseado D y la suma de colliders S tienen la MISMA masa (el caso
    /// normal al fijar solo el centro de masa o solo la inercia), la
    /// resta da una masa adicional de masa CERO, cuyo centro de masa es
    /// degenerado (0) y al sumarla de vuelta no mueve el COM: el override
    /// no se aplica y el getter lo lee sin cambiar (verificado en tests:
    /// `set_center_of_mass([0.2, 0, 0])` se leía `[0, 0, 0]`). La
    /// solución es reducir primero a la mitad la contribución de los
    /// colliders (densidades × 0.5, sin tocar la geometría): la adicional
    /// pasa a tener masa M/2 > 0 y el álgebra vuelve a estar bien
    /// definida. La masa total sigue siendo M y el COM/inercia totales
    /// quedan exactamente en el objetivo.
    ///
    /// Los componentes no fijados (`None`) toman su valor natural de los
    /// colliders, así que un `set_mass` posterior (que reescala las
    /// densidades) fluye a la inercia si solo se fijó el COM, como en
    /// Unity. Si no queda ningún componente fijado, el override se
    /// LIMPIA del todo (densidades restauradas, adicional a cero,
    /// entrada borrada) en vez de dejar un override fantasma.
    fn apply_current_override(&mut self, handle: BodyHandle) -> bool {
        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return false;
        };
        // 1. Restaurar las densidades a su valor natural (deshacer el
        // halve de un override anterior, solo en los colliders que se
        // redujeron entonces; un collider añadido después no se toca).
        self.restore_mprops_densities(handle);
        let Some(rb) = self.rigid_body_set.get(rapier) else {
            return false;
        };
        // 2. S natural (fresco, del ColliderSet) y masa total M: se suma
        // de los colliders, NO se lee rb.mass() (desactualizado hasta el
        // siguiente step tras un cambio de densidad, como en set_mass).
        let natural = self.colliders_mass_properties(rb);
        let masa_total = natural.mass();
        if !masa_total.is_finite() || masa_total <= 1e-6 {
            // Sin masa no se puede sostener un override: limpiar el
            // estado para no dejar la entrada huérfana con densidades
            // naturales y una adicional obsoleta.
            if let Some(rb) = self.rigid_body_set.get_mut(rapier) {
                rb.set_additional_mass_properties(MassProperties::default(), true);
                rb.recompute_mass_properties_from_colliders(&self.collider_set);
            }
            self.mprops_override.remove(&handle);
            return false;
        }
        let com_natural: [f32; 3] = natural.local_com.coords.into();
        let inercia_natural =
            Self::inercia_principal_saneada(natural.inv_principal_inertia_sqrt);
        let qn = natural.principal_inertia_local_frame.quaternion();
        let frame_natural = [qn.i, qn.j, qn.k, qn.w];
        // 3. Resolver objetivos: lo fijado manda; lo demás, natural.
        // El valor manual solo manda si el toggle automático está en false.
        let com_obj = self.com_override_activo(handle);
        let inertia_obj = self.inertia_override_activo(handle);
        // Sin componentes fijados no hay nada que aplicar: limpiar.
        if com_obj.is_none() && inertia_obj.is_none() {
            let Some(rb) = self.rigid_body_set.get_mut(rapier) else {
                return false;
            };
            rb.set_additional_mass_properties(MassProperties::default(), true);
            rb.recompute_mass_properties_from_colliders(&self.collider_set);
            self.mprops_override.remove(&handle);
            return true;
        }
        let com_objetivo = com_obj.unwrap_or(com_natural);
        let (tensor, rot) = inertia_obj.unwrap_or((inercia_natural, frame_natural));
        if !com_objetivo.iter().all(|v| v.is_finite())
            || !tensor.iter().all(|v| v.is_finite())
            || !rot.iter().all(|v| v.is_finite())
            || tensor.iter().any(|v| *v <= 0.0)
        {
            return false;
        }
        let frame_objetivo_q = nalgebra::UnitQuaternion::from_quaternion(
            nalgebra::Quaternion::new(rot[3], rot[0], rot[1], rot[2]),
        );
        // 4. Reducir los colliders a la mitad (densidades × 0.5).
        let mut halved_nuevo = Vec::new();
        for &ch in rb.colliders() {
            if let Some(co) = self.collider_set.get_mut(ch) {
                let d = co.density();
                co.set_density(d * MPROPS_DENSITY_SCALE);
                halved_nuevo.push(ch);
            }
        }
        // S' = S/2: misma COM, mitad de masa e inercia. Se recalcula
        // desde el ColliderSet para no depender de cachés.
        let Some(rb) = self.rigid_body_set.get(rapier) else {
            return false;
        };
        let mitad = self.colliders_mass_properties(rb);
        // 5. D con masa M y objetivos; adicional = D - S' (masa M/2).
        let total_deseado = MassProperties::with_principal_inertia_frame(
            Point::new(com_objetivo[0], com_objetivo[1], com_objetivo[2]),
            masa_total,
            Vector::new(tensor[0], tensor[1], tensor[2]),
            frame_objetivo_q,
        );
        let adicional = total_deseado - mitad;
        let Some(rb) = self.rigid_body_set.get_mut(rapier) else {
            return false;
        };
        rb.set_additional_mass_properties(adicional, true);
        rb.recompute_mass_properties_from_colliders(&self.collider_set);
        if let Some(ov) = self.mprops_override.get_mut(&handle) {
            ov.halved = halved_nuevo;
        }
        true
    }

    /// Total actual (inercia principal + frame) del cuerpo: la fijada si
    /// la hay; si no, la natural de los colliders (fresca). OJO: con un
    /// override activo las densidades están a la mitad y la inercia de S
    /// también; la natural es el doble (la escala uniforme de densidades
    /// escala la inercia linealmente).
    fn total_inertia(&self, handle: BodyHandle) -> Option<([f32; 3], [f32; 4])> {
        let halved = self.mprops_override.contains_key(&handle);
        if halved {
            if let Some(inertia) = self.inertia_override_activo(handle) {
                return Some(inertia);
            }
        }
        let (rapier, _) = self.get_rapier_handle(handle)?;
        let rb = self.rigid_body_set.get(rapier)?;
        let s = self.colliders_mass_properties(rb);
        let mut inercia = Self::inercia_principal_saneada(s.inv_principal_inertia_sqrt);
        if halved {
            for v in inercia.iter_mut() {
                *v /= MPROPS_DENSITY_SCALE;
            }
        }
        let q = s.principal_inertia_local_frame.quaternion();
        Some((inercia, [q.i, q.j, q.k, q.w]))
    }

    /// Garantiza que existe una entrada de override para el cuerpo (con
    /// ambos componentes en `None`) y la devuelve para fijar uno de ellos.
    fn mprops_entry(&mut self, handle: BodyHandle) -> &mut MpropsOverride {
        self.mprops_override.entry(handle).or_insert(MpropsOverride {
            com: None,
            inertia: None,
            halved: Vec::new(),
        })
    }

    /// Fija el centro de masa del cuerpo, conservando su inercia actual
    /// (fijada o natural).
    pub fn set_center_of_mass(&mut self, handle: BodyHandle, com: [f32; 3]) -> bool {
        if !com.iter().all(|v| v.is_finite()) {
            return false;
        }
        if self.get_rapier_handle(handle).is_none() {
            return false;
        }
        self.mprops_entry(handle).com = Some(com);
        self.automatic_center_of_mass.insert(handle, false);
        self.apply_current_override(handle)
    }

    /// Restaura el centro de masa al que derivan únicamente los colliders,
    /// conservando la inercia actual (fijada o natural).
    pub fn reset_center_of_mass(&mut self, handle: BodyHandle) -> bool {
        if self.get_rapier_handle(handle).is_none() {
            return false;
        }
        self.automatic_center_of_mass.insert(handle, true);
        if let Some(ov) = self.mprops_override.get_mut(&handle) {
            ov.com = None;
        } else {
            return true; // Sin override no hay nada que restaurar.
        }
        self.apply_current_override(handle)
    }

    /// Tensor de inercia principal actual (diagonal, en el frame devuelto)
    /// y el frame como cuaternión (x, y, z, w).
    pub fn get_inertia_tensor(&self, handle: BodyHandle) -> Option<([f32; 3], [f32; 4])> {
        self.total_inertia(handle)
    }

    /// Fija el tensor de inercia principal (diagonal) y su frame,
    /// conservando el centro de masa actual (fijado o natural).
    pub fn set_inertia_tensor(
        &mut self,
        handle: BodyHandle,
        tensor: [f32; 3],
        rotation: [f32; 4],
    ) -> bool {
        if !tensor.iter().all(|v| v.is_finite() && *v > 0.0)
            || !rotation.iter().all(|v| v.is_finite())
        {
            return false;
        }
        if self.get_rapier_handle(handle).is_none() {
            return false;
        }
        self.mprops_entry(handle).inertia = Some((tensor, rotation));
        self.automatic_inertia_tensor.insert(handle, false);
        self.apply_current_override(handle)
    }

    /// Restaura la inercia principal y su frame a los que derivan
    /// únicamente de los colliders, conservando el centro de masa actual
    /// (fijado o natural).
    pub fn reset_inertia_tensor(&mut self, handle: BodyHandle) -> bool {
        if self.get_rapier_handle(handle).is_none() {
            return false;
        }
        self.automatic_inertia_tensor.insert(handle, true);
        if let Some(ov) = self.mprops_override.get_mut(&handle) {
            ov.inertia = None;
        } else {
            return true; // Sin override no hay nada que restaurar.
        }
        self.apply_current_override(handle)
    }

    /// automaticCenterOfMass (Unity). A false sin COM manual guardado congela
    /// el COM efectivo actual como manual. A true el natural vuelve a mandar;
    /// el manual guardado se ignora (y se descarta si tampoco queda inercia manual activa).
    pub fn set_automatic_center_of_mass(&mut self, handle: BodyHandle, automatic: bool) -> bool {
        if self.get_rapier_handle(handle).is_none() {
            return false;
        }
        if !automatic {
            let tiene_manual = self
                .mprops_override
                .get(&handle)
                .is_some_and(|ov| ov.com.is_some());
            if !tiene_manual {
                let Some(com) = self.get_center_of_mass(handle) else {
                    return false;
                };
                self.mprops_entry(handle).com = Some(com);
            }
        }
        self.automatic_center_of_mass.insert(handle, automatic);
        self.apply_current_override(handle)
    }

    /// automaticCenterOfMass del cuerpo (defecto true). None si no existe.
    pub fn get_automatic_center_of_mass(&self, handle: BodyHandle) -> Option<bool> {
        self.get_rapier_handle(handle)?;
        Some(
            self.automatic_center_of_mass
                .get(&handle)
                .copied()
                .unwrap_or(true),
        )
    }

    /// automaticInertiaTensor (Unity). Misma semántica que
    /// set_automatic_center_of_mass, con snapshot de la inercia efectiva.
    pub fn set_automatic_inertia_tensor(&mut self, handle: BodyHandle, automatic: bool) -> bool {
        if self.get_rapier_handle(handle).is_none() {
            return false;
        }
        if !automatic {
            let tiene_manual = self
                .mprops_override
                .get(&handle)
                .is_some_and(|ov| ov.inertia.is_some());
            if !tiene_manual {
                let Some(inertia) = self.total_inertia(handle) else {
                    return false;
                };
                self.mprops_entry(handle).inertia = Some(inertia);
            }
        }
        self.automatic_inertia_tensor.insert(handle, automatic);
        self.apply_current_override(handle)
    }

    /// automaticInertiaTensor del cuerpo (defecto true). None si no existe.
    pub fn get_automatic_inertia_tensor(&self, handle: BodyHandle) -> Option<bool> {
        self.get_rapier_handle(handle)?;
        Some(
            self.automatic_inertia_tensor
                .get(&handle)
                .copied()
                .unwrap_or(true),
        )
    }

    /// ClosestPointOnBounds (Unity): punto más cercano a `point` sobre los AABB
    /// mundiales de los colliders del cuerpo. Punto interior → el propio punto.
    /// None si el cuerpo no existe, no tiene colliders o el punto no es finito.
    pub fn closest_point_on_bounds(&self, handle: BodyHandle, point: [f32; 3]) -> Option<[f32; 3]> {
        if !point.iter().all(|v| v.is_finite()) {
            return None;
        }
        let (rapier, _) = self.get_rapier_handle(handle)?;
        let rb = self.rigid_body_set.get(rapier)?;
        rb.colliders()
            .iter()
            .filter_map(|&ch| self.collider_set.get(ch))
            .map(|collider| {
                let aabb = collider.compute_aabb();
                let candidato: [f32; 3] = std::array::from_fn(|i| {
                    point[i].clamp(aabb.mins.coords[i], aabb.maxs.coords[i])
                });
                let d2: f32 = (0..3).map(|i| (candidato[i] - point[i]).powi(2)).sum();
                (candidato, d2)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(candidato, _)| candidato)
    }

    /// Velocidad lineal de un punto del cuerpo, dado en coordenadas MUNDIALES.
    pub fn get_point_velocity(&self, handle: BodyHandle, point: [f32; 3]) -> Option<[f32; 3]> {
        let (rapier, _) = self.get_rapier_handle(handle)?;
        let rb = self.rigid_body_set.get(rapier)?;
        let world_com = rb.position() * self.effective_local_com(handle, rb);
        let p = Point::new(point[0], point[1], point[2]);
        let r = p - world_com;
        let v = rb.linvel() + rb.angvel().cross(&r);
        Some([v.x, v.y, v.z])
    }

    /// Velocidad lineal de un punto del cuerpo, dado en coordenadas LOCALES,
    /// devuelta también en coordenadas LOCALES del cuerpo.
    pub fn get_relative_point_velocity(
        &self,
        handle: BodyHandle,
        local_point: [f32; 3],
    ) -> Option<[f32; 3]> {
        let (rapier, _) = self.get_rapier_handle(handle)?;
        let rb = self.rigid_body_set.get(rapier)?;
        let local_p = Point::new(local_point[0], local_point[1], local_point[2]);
        let world_p = rb.position() * local_p;
        let world_com = rb.position() * self.effective_local_com(handle, rb);
        let r = world_p - world_com;
        let v_world = rb.linvel() + rb.angvel().cross(&r);
        let v_local = rb.rotation().inverse_transform_vector(&v_world);
        Some([v_local.x, v_local.y, v_local.z])
    }

    /// Mueve un cuerpo CINEMÁTICO a la pose indicada (posición + rotación)
    /// para el siguiente step. Devuelve false si el cuerpo no es cinemático.
    pub fn move_kinematic(&mut self, handle: BodyHandle, pos: [f32; 3], quat: [f32; 4]) -> bool {
        if !pos.iter().all(|v| v.is_finite()) || !quat.iter().all(|v| v.is_finite()) {
            return false;
        }
        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return false;
        };
        let Some(rb) = self.rigid_body_set.get_mut(rapier) else {
            return false;
        };
        if rb.body_type() != RigidBodyType::KinematicPositionBased {
            return false;
        }
        let rotation = nalgebra::UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(
            quat[3], quat[0], quat[1], quat[2],
        ));
        let translation = Translation::from(vector![pos[0], pos[1], pos[2]]);
        rb.set_next_kinematic_position(Isometry::from_parts(translation, rotation));
        true
    }

    /// Mueve solo la rotación de un cuerpo CINEMÁTICO para el siguiente
    /// step, conservando la traslación programada. Devuelve false si el
    /// cuerpo no es cinemático.
    pub fn move_kinematic_rotation(&mut self, handle: BodyHandle, quat: [f32; 4]) -> bool {
        if !quat.iter().all(|v| v.is_finite()) {
            return false;
        }
        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return false;
        };
        let Some(rb) = self.rigid_body_set.get_mut(rapier) else {
            return false;
        };
        if rb.body_type() != RigidBodyType::KinematicPositionBased {
            return false;
        }
        let rotation = nalgebra::UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(
            quat[3], quat[0], quat[1], quat[2],
        ));
        rb.set_next_kinematic_rotation(rotation);
        true
    }

    /// Barre todos los colliders de un cuerpo en su pose actual, en la
    /// dirección `dir`, hasta `max_dist`, excluyendo el propio cuerpo.
    /// Devuelve el impacto más cercano entre todos sus colliders.
    pub fn sweep_body(&mut self, handle: BodyHandle, dir: [f32; 3], max_dist: f32) -> Option<ShapeHit> {
        if !dir.iter().all(|v| v.is_finite()) || !max_dist.is_finite() || max_dist <= 0.0 {
            return None;
        }
        let dir_vec = Vector::new(dir[0], dir[1], dir[2]);
        if dir_vec.norm_squared() < f32::EPSILON {
            return None;
        }
        let dir_vec = dir_vec.normalize();

        let (rapier, _) = self.get_rapier_handle(handle)?;
        let collider_handles: Vec<rapier3d::geometry::ColliderHandle> =
            self.rigid_body_set.get(rapier)?.colliders().to_vec();
        if collider_handles.is_empty() {
            return None;
        }

        self.query_pipeline.update(&self.collider_set);

        let pred = move |_h: rapier3d::geometry::ColliderHandle, collider: &Collider| -> bool {
            collider.parent() != Some(rapier)
        };
        let filter = QueryFilter::default().predicate(&pred).exclude_sensors();
        let options = ShapeCastOptions::with_max_time_of_impact(max_dist);

        let mut mejor: Option<ShapeHit> = None;
        for ch in collider_handles {
            let Some(co) = self.collider_set.get(ch) else { continue };
            let shape = co.shape();
            let pose = *co.position();
            if let Some((hit_handle, hit)) = self.query_pipeline.cast_shape(
                &self.rigid_body_set,
                &self.collider_set,
                &pose,
                &dir_vec,
                shape,
                options,
                filter,
            ) {
                if let Some(entity_id) = self.entity_of_collider(hit_handle) {
                    let candidato = ShapeHit {
                        entity_id,
                        point: [hit.witness1.x, hit.witness1.y, hit.witness1.z],
                        normal: [hit.normal1.x, hit.normal1.y, hit.normal1.z],
                        distance: hit.time_of_impact,
                    };
                    if mejor.map(|m| candidato.distance < m.distance).unwrap_or(true) {
                        mejor = Some(candidato);
                    }
                }
            }
        }
        mejor
    }

    /// Como `sweep_body`, pero recoge el impacto más cercano POR ENTIDAD
    /// golpeada entre todos los colliders del cuerpo, ordenado por
    /// distancia ascendente.
    ///
    /// `cast_shape` devuelve solo el PRIMER impacto de cada barrido, así
    /// que se itera: en cada vuelta se toma el impacto más cercano entre
    /// todos los colliders del cuerpo, se anota su entidad y se repite el
    /// barrido excluyendo el cuerpo de esa entidad (por su handle de
    /// Rapier, sin tocar `self` dentro del predicado), hasta que no
    /// quede nada o se llegue al tope de iteraciones.
    pub fn sweep_body_all(&mut self, handle: BodyHandle, dir: [f32; 3], max_dist: f32) -> Vec<ShapeHit> {
        if !dir.iter().all(|v| v.is_finite()) || !max_dist.is_finite() || max_dist <= 0.0 {
            return Vec::new();
        }
        let dir_vec = Vector::new(dir[0], dir[1], dir[2]);
        if dir_vec.norm_squared() < f32::EPSILON {
            return Vec::new();
        }
        let dir_vec = dir_vec.normalize();

        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return Vec::new();
        };
        let collider_handles: Vec<rapier3d::geometry::ColliderHandle> = match self
            .rigid_body_set
            .get(rapier)
        {
            Some(rb) => rb.colliders().to_vec(),
            None => return Vec::new(),
        };
        if collider_handles.is_empty() {
            return Vec::new();
        }

        self.query_pipeline.update(&self.collider_set);
        let options = ShapeCastOptions::with_max_time_of_impact(max_dist);

        let mut resultado: Vec<ShapeHit> = Vec::new();
        let mut excluidos: std::collections::HashSet<rapier3d::dynamics::RigidBodyHandle> =
            std::collections::HashSet::new();
        for _ in 0..64 {
            // Mejor impacto de esta vuelta entre todos los colliders.
            let mut mejor: Option<ShapeHit> = None;
            let mut mejor_cuerpo: Option<rapier3d::dynamics::RigidBodyHandle> = None;
            for ch in &collider_handles {
                let Some(co) = self.collider_set.get(*ch) else { continue };
                let shape = co.shape();
                let pose = *co.position();
                let pred = |_h: rapier3d::geometry::ColliderHandle, collider: &Collider| -> bool {
                    match collider.parent() {
                        Some(p) => p != rapier && !excluidos.contains(&p),
                        None => true,
                    }
                };
                let filter = QueryFilter::default().predicate(&pred).exclude_sensors();
                if let Some((hit_handle, hit)) = self.query_pipeline.cast_shape(
                    &self.rigid_body_set,
                    &self.collider_set,
                    &pose,
                    &dir_vec,
                    shape,
                    options,
                    filter,
                ) {
                    if let Some(entity_id) = self.entity_of_collider(hit_handle) {
                        let candidato = ShapeHit {
                            entity_id,
                            point: [hit.witness1.x, hit.witness1.y, hit.witness1.z],
                            normal: [hit.normal1.x, hit.normal1.y, hit.normal1.z],
                            distance: hit.time_of_impact,
                        };
                        if mejor.map(|m| candidato.distance < m.distance).unwrap_or(true) {
                            mejor = Some(candidato);
                            mejor_cuerpo = self
                                .collider_set
                                .get(hit_handle)
                                .and_then(|c| c.parent());
                        }
                    }
                }
            }
            match (mejor, mejor_cuerpo) {
                (Some(h), Some(cuerpo)) => {
                    excluidos.insert(cuerpo);
                    resultado.push(h);
                }
                _ => break,
            }
        }

        resultado.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(std::cmp::Ordering::Equal));
        resultado
    }

    pub fn set_linear_damping(&mut self, handle: BodyHandle, damping: f32) -> bool {
        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return false;
        };
        match self.rigid_body_set.get_mut(rapier) {
            Some(rb) => {
                rb.set_linear_damping(damping);
                true
            }
            None => false,
        }
    }

    pub fn get_linear_damping(&self, handle: BodyHandle) -> Option<f32> {
        let (rapier, _) = self.get_rapier_handle(handle)?;
        self.rigid_body_set.get(rapier).map(|rb| rb.linear_damping())
    }

    pub fn set_angular_damping(&mut self, handle: BodyHandle, damping: f32) -> bool {
        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return false;
        };
        match self.rigid_body_set.get_mut(rapier) {
            Some(rb) => {
                rb.set_angular_damping(damping);
                true
            }
            None => false,
        }
    }

    pub fn get_angular_damping(&self, handle: BodyHandle) -> Option<f32> {
        let (rapier, _) = self.get_rapier_handle(handle)?;
        self.rigid_body_set.get(rapier).map(|rb| rb.angular_damping())
    }

    /// Fija las iteraciones TOTALES del solver para el cuerpo. Rapier solo admite
    /// iteraciones adicionales sobre la base global, así que se guarda
    /// `n - base` saturado a 0. Los valores negativos cuentan como 0.
    /// Defecto de solverIterations y solverVelocityIterations: la base de Rapier en este motor.
    const DEFAULT_SOLVER_ITERATIONS: i32 = 4;

    /// Guarda solverIterations (TOTAL) del cuerpo y aplica el contador de Rapier.
    /// Rapier 0.23 tiene un único contador adicional por cuerpo, compartido con
    /// solverVelocityIterations: manda el máximo de ambas (ver apply_solver_iterations).
    pub fn set_solver_iterations(&mut self, handle: BodyHandle, iterations: i32) -> bool {
        if self.get_rapier_handle(handle).is_none() {
            return false;
        }
        self.solver_iterations.insert(handle, iterations);
        self.apply_solver_iterations(handle)
    }

    /// Guarda solverVelocityIterations (TOTAL) del cuerpo. Rapier 0.23 solo tiene
    /// un contador adicional por cuerpo (set_additional_solver_iterations), compartido
    /// con solverIterations: se aplica
    /// additional = max(solver_iterations, solver_velocity_iterations) - base global,
    /// saturado a 0.
    pub fn set_solver_velocity_iterations(&mut self, handle: BodyHandle, iterations: i32) -> bool {
        if self.get_rapier_handle(handle).is_none() {
            return false;
        }
        self.solver_velocity_iterations.insert(handle, iterations);
        self.apply_solver_iterations(handle)
    }

    /// Devuelve el valor GUARDADO de solverVelocityIterations (4 si nunca se fijó),
    /// no el efectivo. El efectivo lo da get_solver_iterations.
    pub fn get_solver_velocity_iterations(&self, handle: BodyHandle) -> Option<i32> {
        self.get_rapier_handle(handle)?;
        Some(
            self.solver_velocity_iterations
                .get(&handle)
                .copied()
                .unwrap_or(Self::DEFAULT_SOLVER_ITERATIONS),
        )
    }

    /// additional = max(solver, velocity) - base global, saturado a 0.
    fn apply_solver_iterations(&mut self, handle: BodyHandle) -> bool {
        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return false;
        };
        let solver = self
            .solver_iterations
            .get(&handle)
            .copied()
            .unwrap_or(Self::DEFAULT_SOLVER_ITERATIONS);
        let velocity = self
            .solver_velocity_iterations
            .get(&handle)
            .copied()
            .unwrap_or(Self::DEFAULT_SOLVER_ITERATIONS);
        let base = self.integration_parameters.num_solver_iterations.get();
        let additional = (solver.max(velocity).max(0) as usize).saturating_sub(base);
        match self.rigid_body_set.get_mut(rapier) {
            Some(rb) => {
                rb.set_additional_solver_iterations(additional);
                true
            }
            None => false,
        }
    }

    /// Devuelve las iteraciones TOTALES (base global + adicionales del cuerpo).
    pub fn get_solver_iterations(&self, handle: BodyHandle) -> Option<i32> {
        let (rapier, _) = self.get_rapier_handle(handle)?;
        let base = self.integration_parameters.num_solver_iterations.get();
        self.rigid_body_set.get(rapier).map(|rb| {
            i32::try_from(base.saturating_add(rb.additional_solver_iterations()))
                .unwrap_or(i32::MAX)
        })
    }

    /// includeLayers (Unity): capas con las que el cuerpo SÍ colisiona.
    /// Reescribe al momento el filter de TODOS sus colliders
    /// (base & include & !exclude); el membership no cambia.
    pub fn set_include_layers(&mut self, handle: BodyHandle, mask: u32) -> bool {
        if self.get_rapier_handle(handle).is_none() {
            return false;
        }
        self.rb_include_layers.insert(handle, mask);
        self.apply_layer_filter(handle)
    }

    /// None si el cuerpo no existe; Some(u32::MAX) por defecto.
    pub fn get_include_layers(&self, handle: BodyHandle) -> Option<u32> {
        self.get_rapier_handle(handle)?;
        Some(self.rb_include_layers.get(&handle).copied().unwrap_or(u32::MAX))
    }

    /// excludeLayers (Unity): capas con las que el cuerpo NO colisiona.
    /// Misma reescritura de filter que set_include_layers.
    pub fn set_exclude_layers(&mut self, handle: BodyHandle, mask: u32) -> bool {
        if self.get_rapier_handle(handle).is_none() {
            return false;
        }
        self.rb_exclude_layers.insert(handle, mask);
        self.apply_layer_filter(handle)
    }

    /// None si el cuerpo no existe; Some(0) por defecto.
    pub fn get_exclude_layers(&self, handle: BodyHandle) -> Option<u32> {
        self.get_rapier_handle(handle)?;
        Some(self.rb_exclude_layers.get(&handle).copied().unwrap_or(0))
    }

    /// include & !exclude vigente del cuerpo.
    fn body_layer_filter(&self, handle: BodyHandle) -> u32 {
        let include = self.rb_include_layers.get(&handle).copied().unwrap_or(u32::MAX);
        let exclude = self.rb_exclude_layers.get(&handle).copied().unwrap_or(0);
        include & !exclude
    }

    /// Reescribe el filter de todos los colliders del cuerpo. Si un collider no
    /// tiene base registrada (cápsula del CC, ragdoll), su filter actual pasa a
    /// ser su base y se registra, para no estrechar sobre lo ya estrechado.
    fn apply_layer_filter(&mut self, handle: BodyHandle) -> bool {
        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return false;
        };
        let Some(rb) = self.rigid_body_set.get(rapier) else {
            return false;
        };
        let colliders = rb.colliders().to_vec();
        let filter = self.body_layer_filter(handle);
        for ch in colliders {
            let Some(collider) = self.collider_set.get_mut(ch) else {
                continue;
            };
            let groups = collider.collision_groups();
            let key = ch.into_raw_parts().0 as u64;
            let base = *self
                .layer_base_masks
                .entry(key)
                .or_insert(groups.filter.bits());
            collider.set_collision_groups(InteractionGroups::new(
                groups.memberships,
                Group::from_bits_truncate(base & filter),
            ));
        }
        true
    }

    /// Registra la base de un collider recién insertado (su filter actual, que
    /// es el cfg.mask aplicado) y le aplica el filtro include/exclude vigente
    /// de su cuerpo padre. Devuelve el raw u64 del handle.
    fn register_collider_layers(&mut self, collider_handle: rapier3d::prelude::ColliderHandle) -> u64 {
        let key = collider_handle.into_raw_parts().0 as u64;
        let Some((groups, parent)) = self
            .collider_set
            .get(collider_handle)
            .map(|c| (c.collision_groups(), c.parent()))
        else {
            return key;
        };
        let base = groups.filter.bits();
        self.layer_base_masks.insert(key, base);
        let body_filter = self
            .rb_include_layers
            .keys()
            .chain(self.rb_exclude_layers.keys())
            .find(|&&h| parent.is_some() && self.get_rapier_handle(h).map(|(r, _)| r) == parent)
            .map_or(u32::MAX, |&h| self.body_layer_filter(h));
        if body_filter == u32::MAX {
            return key;
        }
        if let Some(c) = self.collider_set.get_mut(collider_handle) {
            c.set_collision_groups(InteractionGroups::new(
                groups.memberships,
                Group::from_bits_truncate(base & body_filter),
            ));
        }
        key
    }

    pub fn set_gravity_scale(&mut self, handle: BodyHandle, scale: f32) -> bool {
        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return false;
        };
        match self.rigid_body_set.get_mut(rapier) {
            Some(rb) => {
                rb.set_gravity_scale(scale, true);
                true
            }
            None => false,
        }
    }

    pub fn get_gravity_scale(&self, handle: BodyHandle) -> Option<f32> {
        let (rapier, _) = self.get_rapier_handle(handle)?;
        self.rigid_body_set.get(rapier).map(|rb| rb.gravity_scale())
    }

    pub fn set_ccd_enabled(&mut self, handle: BodyHandle, enabled: bool) -> bool {
        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return false;
        };
        match self.rigid_body_set.get_mut(rapier) {
            Some(rb) => {
                rb.enable_ccd(enabled);
                true
            }
            None => false,
        }
    }

    pub fn ccd_enabled(&self, handle: BodyHandle) -> Option<bool> {
        let (rapier, _) = self.get_rapier_handle(handle)?;
        self.rigid_body_set.get(rapier).map(|rb| rb.is_ccd_enabled())
    }

    pub fn set_body_type(&mut self, handle: BodyHandle, tipo: BodyType) -> bool {
        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return false;
        };
        let rapier_type = match tipo {
            BodyType::Dynamic => RigidBodyType::Dynamic,
            BodyType::Static => RigidBodyType::Fixed,
            BodyType::Kinematic => RigidBodyType::KinematicPositionBased,
        };
        match self.rigid_body_set.get_mut(rapier) {
            Some(rb) => {
                rb.set_body_type(rapier_type, true);
                true
            }
            None => false,
        }
    }

    pub fn move_position(&mut self, handle: BodyHandle, pos: [f32; 3]) -> bool {
        let Some((rapier, _)) = self.get_rapier_handle(handle) else {
            return false;
        };
        match self.rigid_body_set.get_mut(rapier) {
            Some(rb) if rb.body_type() == RigidBodyType::KinematicPositionBased => {
                rb.set_next_kinematic_translation(vector![pos[0], pos[1], pos[2]]);
                true
            }
            _ => false,
        }
    }

    pub fn get_locked_axes(&self, body_handle: BodyHandle) -> Option<u8> {
        if let Some((handle, _)) = self.get_rapier_handle(body_handle) {
            if let Some(body) = self.rigid_body_set.get(handle) {
                let axes = body.locked_axes();
                let mut bits: u8 = 0;
                if axes.contains(rapier3d::dynamics::LockedAxes::TRANSLATION_LOCKED_X) {
                    bits |= 1;
                }
                if axes.contains(rapier3d::dynamics::LockedAxes::TRANSLATION_LOCKED_Y) {
                    bits |= 2;
                }
                if axes.contains(rapier3d::dynamics::LockedAxes::TRANSLATION_LOCKED_Z) {
                    bits |= 4;
                }
                if axes.contains(rapier3d::dynamics::LockedAxes::ROTATION_LOCKED_X) {
                    bits |= 8;
                }
                if axes.contains(rapier3d::dynamics::LockedAxes::ROTATION_LOCKED_Y) {
                    bits |= 16;
                }
                if axes.contains(rapier3d::dynamics::LockedAxes::ROTATION_LOCKED_Z) {
                    bits |= 32;
                }
                return Some(bits);
            }
        }
        None
    }

    pub fn remove_rigid_body(&mut self, body_handle: BodyHandle) -> bool {
        if let Some((rapier_handle, _)) = self.body_map.remove(&body_handle) {
            self.handle_lookup.remove(&rapier_handle);
            self.mprops_override.remove(&body_handle);
            self.rigid_body_set.remove(
                rapier_handle,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
            true
        } else {
            false
        }
    }

    /// Returns a bulk list of (entity_id, TransformFfi) for all active dynamic bodies to synchronize back to SceneState.
    pub fn sync_all_transforms(&self) -> Vec<(u64, TransformFfi)> {
        let mut results = Vec::with_capacity(self.body_map.len());
        for (&_body_handle, &(rapier_handle, entity_id)) in &self.body_map {
            if let Some(rb) = self.rigid_body_set.get(rapier_handle) {
                if rb.is_dynamic() {
                    let translation = rb.translation();
                    let quat = rb.rotation().quaternion();
                    results.push((
                        entity_id,
                        TransformFfi {
                            position: [translation.x, translation.y, translation.z],
                            rotation: [quat.i, quat.j, quat.k, quat.w],
                            scale: [1.0, 1.0, 1.0],
                        },
                    ));
                }
            }
        }
        results
    }

    /// Fracción del paso fijo acumulada sin simular, en [0, 1].
    pub fn interpolation_alpha(&self) -> f32 {
        if self.fixed_timestep <= 0.0 {
            return 1.0;
        }
        (self.accumulator / self.fixed_timestep).clamp(0.0, 1.0)
    }

    /// Activa o desactiva la interpolación de render. La clave es la ENTIDAD.
    pub fn set_interpolation(&mut self, entity_id: u64, enabled: bool) {
        if enabled {
            self.interp_bodies.insert(entity_id);
        } else {
            self.interp_bodies.remove(&entity_id);
            self.prev_transforms.remove(&entity_id);
        }
    }

    /// Guarda el transform actual de los cuerpos dinámicos interpolados.
    /// Reconstruye el mapa entero: las entidades sin cuerpo dinámico vivo
    /// pierden su entrada.
    fn capture_prev_transforms(&mut self) {
        self.prev_transforms.clear();
        if self.interp_bodies.is_empty() {
            return;
        }
        for &(rapier_handle, entity_id) in self.body_map.values() {
            if !self.interp_bodies.contains(&entity_id) {
                continue;
            }
            let Some(rb) = self.rigid_body_set.get(rapier_handle) else {
                continue;
            };
            if !rb.is_dynamic() {
                continue;
            }
            let translation = rb.translation();
            let quat = rb.rotation().quaternion();
            self.prev_transforms.insert(
                entity_id,
                TransformFfi {
                    position: [translation.x, translation.y, translation.z],
                    rotation: [quat.i, quat.j, quat.k, quat.w],
                    scale: [1.0, 1.0, 1.0],
                },
            );
        }
    }

    /// Como `sync_all_transforms`, pero mezcla prev→curr con `interpolation_alpha`
    /// en las entidades interpoladas. Saltos de más de 2 m se devuelven sin mezclar.
    pub fn sync_all_transforms_interpolated(&self) -> Vec<(u64, TransformFfi)> {
        use rapier3d::na::{Quaternion, UnitQuaternion};
        const SALTO_MAX: f32 = 2.0;

        let mut results = self.sync_all_transforms();
        if self.prev_transforms.is_empty() {
            return results;
        }
        let alpha = self.interpolation_alpha();
        let to_uq = |r: [f32; 4]| {
            UnitQuaternion::from_quaternion(Quaternion::new(r[3], r[0], r[1], r[2]))
        };

        for (entity_id, curr) in results.iter_mut() {
            if !self.interp_bodies.contains(entity_id) {
                continue;
            }
            let Some(prev) = self.prev_transforms.get(entity_id) else {
                continue;
            };
            let p = prev.position;
            let c = curr.position;
            let d = [c[0] - p[0], c[1] - p[1], c[2] - p[2]];
            if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > SALTO_MAX * SALTO_MAX {
                continue;
            }
            curr.position = [
                p[0] + d[0] * alpha,
                p[1] + d[1] * alpha,
                p[2] + d[2] * alpha,
            ];
            let q0 = to_uq(prev.rotation);
            let q1 = to_uq(curr.rotation);
            // try_slerp: slerp hace panic con cuaterniones casi idénticos.
            let q = q0.try_slerp(&q1, alpha, 1.0e-6).unwrap_or(q1);
            let q = q.quaternion();
            curr.rotation = [q.i, q.j, q.k, q.w];
        }
        results
    }

    pub fn body_count(&self) -> usize {
        self.rigid_body_set.len()
    }

    pub fn collider_count(&self) -> usize {
        self.collider_set.len()
    }

    /// Finds an existing rigid body handle by entity ID if registered.
    pub fn get_body_by_entity(&self, entity_id: u64) -> Option<BodyHandle> {
        self.body_map.iter().find(|(_, &(_, eid))| eid == entity_id).map(|(&h, _)| h)
    }

    /// Returns the internal Rapier handle and entity_id for a BodyHandle.
    pub fn get_rapier_handle(&self, body_handle: BodyHandle) -> Option<(RigidBodyHandle, u64)> {
        self.body_map.get(&body_handle).copied()
    }

    /// Lanza un rayo contra los colliders del mundo y devuelve el primer
    /// impacto (entidad, punto, normal y distancia). La QueryPipeline se
    /// actualiza en cada llamada: el mundo se reconstruye al cambiar de
    /// escena y los scripts crean/destruyen cuerpos en runtime, así que una
    /// pipeline cacheada quedaría obsoleta; el coste es asumible para los
    /// pocos raycasts por frame que hace el gameplay (suelo, espada).
    /// `exclude_entity` (0 = ninguna) ignora los colliders de esa entidad.
    pub fn cast_ray_masked(
        &mut self,
        origin: [f32; 3],
        dir: [f32; 3],
        max_dist: f32,
        mask: u32,
        exclude_entity: u64,
    ) -> Option<RayHit> {
        if !origin.iter().chain(dir.iter()).all(|v| v.is_finite())
            || !max_dist.is_finite()
            || max_dist <= 0.0
        {
            return None;
        }
        let dir_v = vector![dir[0], dir[1], dir[2]];
        let len = dir_v.magnitude();
        if !len.is_finite() || len < 1e-8 {
            return None;
        }
        let ray = Ray::new(point![origin[0], origin[1], origin[2]], dir_v / len);
        self.query_pipeline.update(&self.collider_set);
        let excluded_rb = if exclude_entity != 0 {
            self.get_body_by_entity(exclude_entity)
                .and_then(|bh| self.get_rapier_handle(bh))
                .map(|(rh, _)| rh)
        } else {
            None
        };
        // Ojo: el handle aquí es el de Rapier, no el alias propio del crate.
        let pred = |_handle: rapier3d::geometry::ColliderHandle, collider: &Collider| -> bool {
            match (excluded_rb, collider.parent()) {
                (Some(ex), Some(parent)) => parent != ex,
                _ => true,
            }
        };
        let filter = QueryFilter::default()
            .groups(InteractionGroups::new(
                Group::all(),
                Group::from_bits_truncate(mask),
            ))
            .predicate(&pred)
            .exclude_sensors();
        let (handle, intersection) = self.query_pipeline.cast_ray_and_get_normal(
            &self.rigid_body_set,
            &self.collider_set,
            &ray,
            max_dist,
            true,
            filter,
        )?;
        let collider = self.collider_set.get(handle)?;
        let rb_handle = collider.parent()?;
        let body_handle = self.handle_lookup.get(&rb_handle)?;
        let &(_, entity_id) = self.body_map.get(body_handle)?;
        let point = ray.point_at(intersection.time_of_impact);
        Some(RayHit {
            entity_id,
            point: [point.x, point.y, point.z],
            normal: [
                intersection.normal.x,
                intersection.normal.y,
                intersection.normal.z,
            ],
            distance: intersection.time_of_impact,
        })
    }

    /// Lanza un rayo contra los colliders del mundo y devuelve el primer
    /// impacto (entidad, punto, normal y distancia). La QueryPipeline se
    /// actualiza en cada llamada: el mundo se reconstruye al cambiar de
    /// escena y los scripts crean/destruyen cuerpos en runtime, así que una
    /// pipeline cacheada quedaría obsoleta; el coste es asumible para los
    /// pocos raycasts por frame que hace el gameplay (suelo, espada).
    /// `exclude_entity` (0 = ninguna) ignora los colliders de esa entidad.
    /// Sin máscara: delega en `cast_ray_masked` con todas las capas.
    pub fn cast_ray(
        &mut self,
        origin: [f32; 3],
        dir: [f32; 3],
        max_dist: f32,
        exclude_entity: u64,
    ) -> Option<RayHit> {
        self.cast_ray_masked(origin, dir, max_dist, u32::MAX, exclude_entity)
    }

    /// pertenezca a esa entidad.
    pub fn cast_sphere(
        &mut self,
        origin: [f32; 3],
        radius: f32,
        dir: [f32; 3],
        max_dist: f32,
        mask: u32,
        exclude_entity: u64,
    ) -> Option<ShapeHit> {
        if !origin.iter().all(|v| v.is_finite())
            || !dir.iter().all(|v| v.is_finite())
            || !radius.is_finite()
            || radius <= 0.0
            || !max_dist.is_finite()
            || max_dist <= 0.0
        {
            return None;
        }
        let dir_vec = Vector::new(dir[0], dir[1], dir[2]);
        if dir_vec.norm_squared() < f32::EPSILON {
            return None;
        }
        let dir_vec = dir_vec.normalize();

        self.query_pipeline.update(&self.collider_set);

        let excluded_rb = if exclude_entity != 0 {
            self.get_body_by_entity(exclude_entity)
                .and_then(|bh| self.get_rapier_handle(bh))
                .map(|(rh, _)| rh)
        } else {
            None
        };
        let pred = |_handle: rapier3d::geometry::ColliderHandle, collider: &Collider| -> bool {
            match (excluded_rb, collider.parent()) {
                (Some(ex), Some(parent)) => parent != ex,
                _ => true,
            }
        };
        let filter = QueryFilter::default()
            .groups(InteractionGroups::new(
                Group::all(),
                Group::from_bits_truncate(mask),
            ))
            .predicate(&pred)
            .exclude_sensors();

        let shape_pos = Isometry::translation(origin[0], origin[1], origin[2]);
        let ball = Ball::new(radius);
        let options = ShapeCastOptions::with_max_time_of_impact(max_dist);

        self.query_pipeline
            .cast_shape(
                &self.rigid_body_set,
                &self.collider_set,
                &shape_pos,
                &dir_vec,
                &ball,
                options,
                filter,
            )
            .and_then(|(handle, hit)| {
                self.entity_of_collider(handle).map(|entity_id| ShapeHit {
                    entity_id,
                    point: [hit.witness1.x, hit.witness1.y, hit.witness1.z],
                    normal: [hit.normal1.x, hit.normal1.y, hit.normal1.z],
                    distance: hit.time_of_impact,
                })
            })
    }

    /// pertenezca a esa entidad.
    pub fn cast_capsule(
        &mut self,
        barrido: BarridoCapsula,
        mask: u32,
        exclude_entity: u64,
    ) -> Option<ShapeHit> {
        let BarridoCapsula { origin, half_height, radius, dir, max_dist } = barrido;
        if !origin.iter().all(|v| v.is_finite())
            || !dir.iter().all(|v| v.is_finite())
            || !radius.is_finite()
            || radius <= 0.0
            || !half_height.is_finite()
            || half_height < 0.0
            || !max_dist.is_finite()
            || max_dist <= 0.0
        {
            return None;
        }
        let dir_vec = Vector::new(dir[0], dir[1], dir[2]);
        if dir_vec.norm_squared() < f32::EPSILON {
            return None;
        }
        let dir_vec = dir_vec.normalize();

        self.query_pipeline.update(&self.collider_set);

        let excluded_rb = if exclude_entity != 0 {
            self.get_body_by_entity(exclude_entity)
                .and_then(|bh| self.get_rapier_handle(bh))
                .map(|(rh, _)| rh)
        } else {
            None
        };
        let pred = |_handle: rapier3d::geometry::ColliderHandle, collider: &Collider| -> bool {
            match (excluded_rb, collider.parent()) {
                (Some(ex), Some(parent)) => parent != ex,
                _ => true,
            }
        };
        let filter = QueryFilter::default()
            .groups(InteractionGroups::new(
                Group::all(),
                Group::from_bits_truncate(mask),
            ))
            .predicate(&pred)
            .exclude_sensors();

        let shape_pos = Isometry::translation(origin[0], origin[1], origin[2]);
        let capsule = Capsule::new_y(half_height, radius);
        let options = ShapeCastOptions::with_max_time_of_impact(max_dist);

        self.query_pipeline
            .cast_shape(
                &self.rigid_body_set,
                &self.collider_set,
                &shape_pos,
                &dir_vec,
                &capsule,
                options,
                filter,
            )
            .and_then(|(handle, hit)| {
                self.entity_of_collider(handle).map(|entity_id| ShapeHit {
                    entity_id,
                    point: [hit.witness1.x, hit.witness1.y, hit.witness1.z],
                    normal: [hit.normal1.x, hit.normal1.y, hit.normal1.z],
                    distance: hit.time_of_impact,
                })
            })
    }

    /// cuerpo rígido padre pertenezca a esa entidad.
    pub fn overlap_sphere(
        &mut self,
        center: [f32; 3],
        radius: f32,
        mask: u32,
        exclude_entity: u64,
    ) -> Vec<u64> {
        if !center.iter().all(|v| v.is_finite()) || !radius.is_finite() || radius <= 0.0 {
            return Vec::new();
        }

        self.query_pipeline.update(&self.collider_set);

        let excluded_rb = if exclude_entity != 0 {
            self.get_body_by_entity(exclude_entity)
                .and_then(|bh| self.get_rapier_handle(bh))
                .map(|(rh, _)| rh)
        } else {
            None
        };
        let pred = |_handle: rapier3d::geometry::ColliderHandle, collider: &Collider| -> bool {
            match (excluded_rb, collider.parent()) {
                (Some(ex), Some(parent)) => parent != ex,
                _ => true,
            }
        };
        let filter = QueryFilter::default()
            .groups(InteractionGroups::new(
                Group::all(),
                Group::from_bits_truncate(mask),
            ))
            .predicate(&pred)
            .exclude_sensors();

        let shape_pos = Isometry::translation(center[0], center[1], center[2]);
        let ball = Ball::new(radius);

        let mut entities = Vec::new();
        self.query_pipeline.intersections_with_shape(
            &self.rigid_body_set,
            &self.collider_set,
            &shape_pos,
            &ball,
            filter,
            |handle| {
                if let Some(entity_id) = self.entity_of_collider(handle) {
                    entities.push(entity_id);
                }
                true
            },
        );

        entities.sort_unstable();
        entities.dedup();
        entities
    }

    /// cuerpo rígido padre pertenezca a esa entidad.
    pub fn overlap_box(
        &mut self,
        center: [f32; 3],
        half_extents: [f32; 3],
        mask: u32,
        exclude_entity: u64,
    ) -> Vec<u64> {
        if !center.iter().all(|v| v.is_finite())
            || !half_extents.iter().all(|v| v.is_finite() && *v > 0.0)
        {
            return Vec::new();
        }

        self.query_pipeline.update(&self.collider_set);

        let excluded_rb = if exclude_entity != 0 {
            self.get_body_by_entity(exclude_entity)
                .and_then(|bh| self.get_rapier_handle(bh))
                .map(|(rh, _)| rh)
        } else {
            None
        };
        let pred = |_handle: rapier3d::geometry::ColliderHandle, collider: &Collider| -> bool {
            match (excluded_rb, collider.parent()) {
                (Some(ex), Some(parent)) => parent != ex,
                _ => true,
            }
        };
        let filter = QueryFilter::default()
            .groups(InteractionGroups::new(
                Group::all(),
                Group::from_bits_truncate(mask),
            ))
            .predicate(&pred)
            .exclude_sensors();

        let shape_pos = Isometry::translation(center[0], center[1], center[2]);
        let cuboid = Cuboid::new(Vector::new(
            half_extents[0],
            half_extents[1],
            half_extents[2],
        ));

        let mut entities = Vec::new();
        self.query_pipeline.intersections_with_shape(
            &self.rigid_body_set,
            &self.collider_set,
            &shape_pos,
            &cuboid,
            filter,
            |handle| {
                if let Some(entity_id) = self.entity_of_collider(handle) {
                    entities.push(entity_id);
                }
                true
            },
        );

        entities.sort_unstable();
        entities.dedup();
        entities
    }

}

/// Parámetros de un barrido de cápsula (eje Y) en `cast_capsule`:
/// origen, media altura del tramo cilíndrico, radio, dirección y
/// distancia máxima. (Agrupados para no pasarlos sueltos: lint
/// too_many_arguments de Clippy.)
#[derive(Debug, Clone, Copy)]
pub struct BarridoCapsula {
    pub origin: [f32; 3],
    pub half_height: f32,
    pub radius: f32,
    pub dir: [f32; 3],
    pub max_dist: f32,
}

/// Resultado de una consulta de forma (shape cast): como `RayHit` pero el
/// punto y la normal son los del contacto de la forma barrida (witness del
/// collider golpeado).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeHit {
    pub entity_id: u64,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub distance: f32,
}

/// Impacto de un raycast: entidad golpeada y geometría del contacto.
#[derive(Debug, Clone, Copy)]
pub struct RayHit {
    pub entity_id: u64,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub distance: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::collider_shape_for_builtin;

    /// Mundo mínimo: suelo fijo (caja ancha con la superficie en
    /// y = 0) y pasos de 1/60 hasta que el cuerpo de prueba se asiente.
    #[test]
    fn angular_damping_set_get() {
        let (mut world, _suelo) = mundo_con_suelo();
        let bola = world.create_rigid_body(20, BodyType::Dynamic, [0.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0]);

        assert!(world.set_angular_damping(bola, 2.5));
        assert_eq!(world.get_angular_damping(bola), Some(2.5));

        assert!(world.set_angular_damping(bola, 0.0));
        assert_eq!(world.get_angular_damping(bola), Some(0.0));

        assert!(!world.set_angular_damping(9999, 1.0));
        assert_eq!(world.get_angular_damping(9999), None);
    }

    #[test]
    fn solver_iterations_total() {
        let (mut world, _suelo) = mundo_con_suelo();
        let bola = world.create_rigid_body(20, BodyType::Dynamic, [0.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0]);

        assert!(world.set_solver_iterations(bola, 6));
        assert_eq!(world.get_solver_iterations(bola), Some(6));

        assert!(world.set_solver_iterations(bola, 4));
        assert_eq!(world.get_solver_iterations(bola), Some(4));

        // 0 adicionales: queda la base de Rapier (4).
        assert!(world.set_solver_iterations(bola, 0));
        assert_eq!(world.get_solver_iterations(bola), Some(4));

        assert!(world.set_solver_iterations(bola, 10));
        assert_eq!(world.get_solver_iterations(bola), Some(10));

        assert!(!world.set_solver_iterations(9999, 6));
        assert_eq!(world.get_solver_iterations(9999), None);
    }

    #[test]
    fn angular_damping_frena_rotacion() {
        let (mut world, _suelo) = mundo_con_suelo();
        // Con collider: sin él el cuerpo no tiene masa/inercia y Rapier
        // anula la velocidad angular en el primer paso. A y = 8 no tocan
        // el suelo en los 60 pasos (caerían ~4,9 m en 1 s).
        let frenado = world.create_rigid_body(30, BodyType::Dynamic, [0.0, 8.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        let libre = world.create_rigid_body(31, BodyType::Dynamic, [5.0, 8.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world
            .attach_box_collider(frenado, [1.0, 1.0, 1.0], 0.0, 0.0)
            .expect("collider frenado");
        world
            .attach_box_collider(libre, [1.0, 1.0, 1.0], 0.0, 0.0)
            .expect("collider libre");

        assert!(world.set_angular_damping(frenado, 5.0));
        assert!(world.set_angular_damping(libre, 0.0));
        assert!(world.set_angular_velocity(frenado, [0.0, 10.0, 0.0]));
        assert!(world.set_angular_velocity(libre, [0.0, 10.0, 0.0]));

        for _ in 0..60 {
            world.step(1.0 / 60.0);
        }

        let v1 = world.get_angular_velocity(frenado).expect("cuerpo frenado debe existir");
        let v2 = world.get_angular_velocity(libre).expect("cuerpo libre debe existir");
        let n1 = (v1[0] * v1[0] + v1[1] * v1[1] + v1[2] * v1[2]).sqrt();
        let n2 = (v2[0] * v2[0] + v2[1] * v2[1] + v2[2] * v2[2]).sqrt();

        assert!(n1 < n2, "con damping 5.0 ({n1}) debe girar menos que con 0.0 ({n2})");
    }

    fn cuerpo_dinamico_box(world: &mut PhysicsWorld, id: u64) -> u64 {
        let h = world.create_rigid_body(id, BodyType::Dynamic, [0.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world
            .attach_box_collider(h, [0.5, 0.5, 0.5], 0.0, 0.0)
            .expect("collider box");
        h
    }

    fn aprox(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4)
    }

    #[test]
    fn set_transform_rechaza_cuaternion_cero() {
        // Regresión sonda RBCC100: un cuaternión (0,0,0,0) normalizado
        // produce NaN y envenena el cuerpo; set_transform debe rechazarlo.
        let (mut world, _suelo) = mundo_con_suelo();
        let h = world.create_rigid_body(77, BodyType::Dynamic, [0.0, 8.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        assert!(!world.set_transform(h, [0.0, 8.0, 0.0], [0.0, 0.0, 0.0, 0.0]));
        assert!(world.set_transform(h, [0.0, 9.0, 0.0], [0.0, 0.0, 0.0, 1.0]));
        // Crear con cuaternión cero cae a identidad, nunca a NaN.
        let h2 = world.create_rigid_body(78, BodyType::Dynamic, [0.0, 8.0, 0.0], [0.0, 0.0, 0.0, 0.0]);
        let t2 = world.get_transform(h2).expect("transform h2");
        assert!(t2.rotation.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn add_relative_force_rota_con_el_cuerpo() {
        let (mut world, _suelo) = mundo_con_suelo();

        let rotado = cuerpo_dinamico_box(&mut world, 40);
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert!(world.set_transform(rotado, [0.0, 5.0, 0.0], [0.0, s, 0.0, s]));
        assert!(world.add_relative_force(rotado, [0.0, 0.0, 10.0]));
        let f = world.get_accumulated_force(rotado).unwrap();
        assert!(aprox(f, [10.0, 0.0, 0.0]), "rotado: {f:?}");

        let sin_rotar = cuerpo_dinamico_box(&mut world, 41);
        assert!(world.add_relative_force(sin_rotar, [0.0, 0.0, 10.0]));
        let f = world.get_accumulated_force(sin_rotar).unwrap();
        assert!(aprox(f, [0.0, 0.0, 10.0]), "sin rotar: {f:?}");
    }

    #[test]
    fn accumulated_force_y_torque() {
        let (mut world, _suelo) = mundo_con_suelo();
        let h = cuerpo_dinamico_box(&mut world, 42);

        assert!(world.apply_force(h, [1.0, 2.0, 3.0]));
        assert!(world.add_torque(h, [0.0, 5.0, 0.0]));

        assert!(aprox(world.get_accumulated_force(h).unwrap(), [1.0, 2.0, 3.0]));
        assert!(aprox(world.get_accumulated_torque(h).unwrap(), [0.0, 5.0, 0.0]));

        assert!(world.get_accumulated_force(9999).is_none());
        assert!(world.get_accumulated_torque(9999).is_none());
    }

    #[test]
    fn add_relative_force_handle_invalido() {
        let (mut world, _suelo) = mundo_con_suelo();
        assert!(!world.add_relative_force(9999, [0.0, 0.0, 10.0]));
        assert!(!world.apply_relative_impulse(9999, [0.0, 0.0, 10.0]));
    }

    #[test]
    fn max_depenetration_velocity_frena_salida() {
        let (mut world, _suelo) = mundo_con_suelo();
        let limitada =
            world.create_rigid_body(100, BodyType::Dynamic, [-3.0, 0.2, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world
            .attach_box_collider(limitada, [0.5; 3], 0.0, 0.5)
            .expect("collider limitada");
        let libre =
            world.create_rigid_body(101, BodyType::Dynamic, [3.0, 0.2, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world
            .attach_box_collider(libre, [0.5; 3], 0.0, 0.5)
            .expect("collider libre");
        assert!(world.set_max_depenetration_velocity(limitada, 0.2));

        for _ in 0..30 {
            world.step(1.0 / 60.0);
        }

        let y_limitada = world.get_transform(limitada).expect("transform limitada").position[1];
        let y_libre = world.get_transform(libre).expect("transform libre").position[1];
        assert!(y_limitada < 0.4, "limitada salio demasiado: y = {y_limitada}");
        assert!(y_libre > 0.45, "libre no salio: y = {y_libre}");
    }

    #[test]
    fn max_depenetration_velocity_no_frena_caida_libre() {
        let (mut world, _suelo) = mundo_con_suelo();
        let h = world.create_rigid_body(102, BodyType::Dynamic, [0.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world.attach_box_collider(h, [0.5; 3], 0.0, 0.5).expect("collider");
        assert!(world.set_max_depenetration_velocity(h, 0.2));

        for _ in 0..30 {
            world.step(1.0 / 60.0);
        }

        let vy = world.get_linear_velocity(h).expect("velocidad")[1];
        assert!(vy < -3.0, "la caida libre se freno: vy = {vy}");
    }

    #[test]
    fn max_depenetration_velocity_set_get() {
        let (mut world, _suelo) = mundo_con_suelo();
        let h = world.create_rigid_body(103, BodyType::Dynamic, [0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);

        assert!(world.set_max_depenetration_velocity(h, 1.5));
        assert_eq!(world.get_max_depenetration_velocity(h), Some(1.5));

        assert!(world.set_max_depenetration_velocity(h, -1.0));
        assert_eq!(world.get_max_depenetration_velocity(h), Some(f32::INFINITY));

        assert!(!world.set_max_depenetration_velocity(9999, 1.0));
        assert_eq!(world.get_max_depenetration_velocity(9999), None);
    }

    fn grupos_rb3(world: &PhysicsWorld, h: BodyHandle, i: usize) -> InteractionGroups {
        let (rapier, _) = world.get_rapier_handle(h).expect("cuerpo");
        let ch = world.rigid_body_set[rapier].colliders()[i];
        world.collider_set[ch].collision_groups()
    }

    /// Añade una caja al cuerpo y pone su membership en `capa` (filter intacto).
    fn caja_en_capa_rb3(world: &mut PhysicsWorld, h: BodyHandle, capa: u32) {
        world
            .attach_box_collider(h, [0.5, 0.5, 0.5], 0.0, 0.5)
            .expect("collider");
        let (rapier, _) = world.get_rapier_handle(h).expect("cuerpo");
        let ch = world.rigid_body_set[rapier].colliders()[0];
        let collider = world.collider_set.get_mut(ch).expect("collider");
        let g = collider.collision_groups();
        collider.set_collision_groups(InteractionGroups::new(
            Group::from_bits_truncate(1u32 << capa),
            g.filter,
        ));
    }

    fn colisionan_rb3(world: &PhysicsWorld, a: BodyHandle, b: BodyHandle) -> bool {
        grupos_rb3(world, a, 0).test(grupos_rb3(world, b, 0))
    }

    #[test]
    fn rb3_include_layers_solo_colisiona_con_capas_incluidas() {
        let (mut world, _) = mundo_con_suelo();
        let a = world.create_rigid_body(9101, BodyType::Dynamic, [0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        let b = world.create_rigid_body(9102, BodyType::Dynamic, [2.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        let c = world.create_rigid_body(9103, BodyType::Dynamic, [4.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        caja_en_capa_rb3(&mut world, a, 1);
        caja_en_capa_rb3(&mut world, b, 2);
        caja_en_capa_rb3(&mut world, c, 3);
        assert!(colisionan_rb3(&world, a, c));
        assert!(world.set_include_layers(a, 1u32 << 2));
        assert!(colisionan_rb3(&world, a, b));
        assert!(!colisionan_rb3(&world, a, c));
        assert_eq!(grupos_rb3(&world, a, 0).memberships.bits(), 1u32 << 1);
        assert_eq!(world.get_include_layers(a), Some(1u32 << 2));
    }

    #[test]
    fn rb3_exclude_layers_no_colisiona_con_capa_excluida() {
        let (mut world, _) = mundo_con_suelo();
        let a = world.create_rigid_body(9111, BodyType::Dynamic, [0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        let b = world.create_rigid_body(9112, BodyType::Dynamic, [2.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        caja_en_capa_rb3(&mut world, a, 1);
        caja_en_capa_rb3(&mut world, b, 2);
        assert!(colisionan_rb3(&world, a, b));
        assert!(world.set_exclude_layers(a, 1u32 << 2));
        assert!(!colisionan_rb3(&world, a, b));
        assert_eq!(world.get_exclude_layers(a), Some(1u32 << 2));
    }

    #[test]
    fn rb3_quitar_exclude_restaura_la_colision_y_la_base() {
        let (mut world, _) = mundo_con_suelo();
        let a = world.create_rigid_body(9121, BodyType::Dynamic, [0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        let b = world.create_rigid_body(9122, BodyType::Dynamic, [2.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        caja_en_capa_rb3(&mut world, a, 1);
        caja_en_capa_rb3(&mut world, b, 2);
        assert!(world.set_exclude_layers(a, 1u32 << 2));
        assert!(world.set_exclude_layers(a, 1u32 << 2));
        assert!(!colisionan_rb3(&world, a, b));
        // Un collider añadido después nace ya filtrado.
        world
            .attach_box_collider(a, [0.25, 0.25, 0.25], 0.0, 0.5)
            .expect("collider");
        assert_eq!(grupos_rb3(&world, a, 1).filter.bits(), !(1u32 << 2));
        assert!(world.set_exclude_layers(a, 0));
        assert!(colisionan_rb3(&world, a, b));
        assert_eq!(grupos_rb3(&world, a, 0).filter.bits(), u32::MAX);
        assert_eq!(grupos_rb3(&world, a, 1).filter.bits(), u32::MAX);
    }

    #[test]
    fn rb3_layers_handle_invalido_y_defectos() {
        let (mut world, _) = mundo_con_suelo();
        assert!(!world.set_include_layers(u64::MAX, 1));
        assert!(!world.set_exclude_layers(u64::MAX, 1));
        assert_eq!(world.get_include_layers(u64::MAX), None);
        assert_eq!(world.get_exclude_layers(u64::MAX), None);
        let a = world.create_rigid_body(9131, BodyType::Dynamic, [0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(world.get_include_layers(a), Some(u32::MAX));
        assert_eq!(world.get_exclude_layers(a), Some(0));
    }

    #[test]
    fn rb3_solver_velocity_manda_el_maximo() {
        let (mut world, _) = mundo_con_suelo();
        let a = world.create_rigid_body(9141, BodyType::Dynamic, [0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        assert!(world.set_solver_iterations(a, 6));
        assert!(world.set_solver_velocity_iterations(a, 10));
        assert_eq!(world.get_solver_iterations(a), Some(10));
        assert_eq!(world.get_solver_velocity_iterations(a), Some(10));
    }

    #[test]
    fn rb3_solver_velocity_menor_no_baja_solver() {
        let (mut world, _) = mundo_con_suelo();
        let a = world.create_rigid_body(9151, BodyType::Dynamic, [0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        assert!(world.set_solver_iterations(a, 6));
        assert!(world.set_solver_velocity_iterations(a, 3));
        assert_eq!(world.get_solver_iterations(a), Some(6));
        assert_eq!(world.get_solver_velocity_iterations(a), Some(3));
    }

    #[test]
    fn rb3_solver_velocity_handle_invalido_y_defecto() {
        let (mut world, _) = mundo_con_suelo();
        assert!(!world.set_solver_velocity_iterations(u64::MAX, 8));
        assert_eq!(world.get_solver_velocity_iterations(u64::MAX), None);
        let a = world.create_rigid_body(9161, BodyType::Dynamic, [0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(world.get_solver_velocity_iterations(a), Some(4));
    }

    fn rb4_cerca(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    fn rb4_cuerpo_caja(world: &mut PhysicsWorld, id: u64) -> BodyHandle {
        let h = world.create_rigid_body(id, BodyType::Dynamic, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world
            .attach_box_collider(h, [0.5, 0.5, 0.5], 0.0, 0.5)
            .expect("collider rb4");
        h
    }

    #[test]
    fn rb4_fijar_com_desactiva_automatico_y_true_vuelve_al_natural() {
        let (mut world, _) = mundo_con_suelo();
        let h = rb4_cuerpo_caja(&mut world, 9401);
        let natural = world.get_center_of_mass(h).expect("COM natural");
        let manual = [0.3, -0.2, 0.1];
        assert!(!rb4_cerca(natural, manual));

        assert!(world.set_center_of_mass(h, manual));
        assert_eq!(world.get_automatic_center_of_mass(h), Some(false));
        assert!(rb4_cerca(world.get_center_of_mass(h).unwrap(), manual));

        assert!(world.set_automatic_center_of_mass(h, true));
        assert_eq!(world.get_automatic_center_of_mass(h), Some(true));
        let vuelto = world.get_center_of_mass(h).unwrap();
        assert!(rb4_cerca(vuelto, natural));
        assert!(!rb4_cerca(vuelto, manual));

        assert!(world.set_center_of_mass(h, manual));
        assert!(world.reset_center_of_mass(h));
        assert_eq!(world.get_automatic_center_of_mass(h), Some(true));
        assert!(rb4_cerca(world.get_center_of_mass(h).unwrap(), natural));
    }

    #[test]
    fn rb4_automatico_com_a_false_congela_el_valor_actual() {
        let (mut world, _) = mundo_con_suelo();
        let h = rb4_cuerpo_caja(&mut world, 9402);
        let antes = world.get_center_of_mass(h).expect("COM natural");
        assert!(world.set_automatic_center_of_mass(h, false));
        assert_eq!(world.get_automatic_center_of_mass(h), Some(false));
        assert!(rb4_cerca(world.get_center_of_mass(h).unwrap(), antes));
    }

    #[test]
    fn rb4_fijar_inercia_desactiva_automatico_y_true_vuelve_a_la_natural() {
        let (mut world, _) = mundo_con_suelo();
        let h = rb4_cuerpo_caja(&mut world, 9403);
        let natural = world.total_inertia(h).expect("inercia natural").0;

        assert!(world.set_inertia_tensor(h, [2.0, 2.0, 2.0], [0.0, 0.0, 0.0, 1.0]));
        assert_eq!(world.get_automatic_inertia_tensor(h), Some(false));
        assert!(rb4_cerca(world.total_inertia(h).unwrap().0, [2.0, 2.0, 2.0]));

        assert!(world.set_automatic_inertia_tensor(h, true));
        assert_eq!(world.get_automatic_inertia_tensor(h), Some(true));
        let vuelta = world.total_inertia(h).unwrap().0;
        if !rb4_cerca(natural, [2.0, 2.0, 2.0]) {
            assert!(!rb4_cerca(vuelta, [2.0, 2.0, 2.0]));
        }

        assert!(world.set_inertia_tensor(h, [2.0, 2.0, 2.0], [0.0, 0.0, 0.0, 1.0]));
        assert!(world.reset_inertia_tensor(h));
        assert_eq!(world.get_automatic_inertia_tensor(h), Some(true));
    }

    #[test]
    fn rb4_automaticos_handle_invalido_y_defecto() {
        let (mut world, _) = mundo_con_suelo();
        let invalido: BodyHandle = u64::MAX;
        assert!(!world.set_automatic_center_of_mass(invalido, false));
        assert!(!world.set_automatic_inertia_tensor(invalido, false));
        assert_eq!(world.get_automatic_center_of_mass(invalido), None);
        assert_eq!(world.get_automatic_inertia_tensor(invalido), None);

        let h = rb4_cuerpo_caja(&mut world, 9405);
        assert_eq!(world.get_automatic_center_of_mass(h), Some(true));
        assert_eq!(world.get_automatic_inertia_tensor(h), Some(true));
    }

    #[test]
    fn rb4_closest_point_on_bounds() {
        let (mut world, _) = mundo_con_suelo();
        let h = rb4_cuerpo_caja(&mut world, 9406);
        assert!(rb4_cerca(
            world.closest_point_on_bounds(h, [3.0, 0.25, 0.0]).unwrap(),
            [0.5, 0.25, 0.0]
        ));
        assert!(rb4_cerca(
            world.closest_point_on_bounds(h, [2.0, 2.0, 2.0]).unwrap(),
            [0.5, 0.5, 0.5]
        ));
        assert!(rb4_cerca(
            world.closest_point_on_bounds(h, [0.1, 0.0, 0.0]).unwrap(),
            [0.1, 0.0, 0.0]
        ));

        let sin_colliders =
            world.create_rigid_body(9407, BodyType::Dynamic, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        assert!(world.closest_point_on_bounds(sin_colliders, [1.0, 0.0, 0.0]).is_none());
        assert!(world.closest_point_on_bounds(u64::MAX, [1.0, 0.0, 0.0]).is_none());
    }

    fn mundo_con_suelo() -> (PhysicsWorld, u64) {
        let mut world = PhysicsWorld::new();
        let suelo = world.create_rigid_body(10, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world
            .attach_box_collider(suelo, [10.0, 0.5, 10.0], 0.0, 0.9)
            .expect("collider suelo");
        (world, suelo)
    }

    fn asentar(world: &mut PhysicsWorld, pasos: usize) {
        for _ in 0..pasos {
            world.step(1.0 / 60.0);
        }
    }

    #[test]
    fn bola_por_inferencia_descansa_a_la_altura_de_su_radio() {
        // Esfera de escala 2 (radio mundial 1) soltada sobre el suelo:
        // con Ball descansa con el centro a y ≈ 1; con la caja de
        // siempre (box_extents 0,5) quedaría a y ≈ 0,5, medio cuerpo
        // hundido. Es el test que distingue la forma nueva de la vieja.
        let (mut world, _suelo) = mundo_con_suelo();
        let shape = collider_shape_for_builtin("sphere", [2.0, 2.0, 2.0], [0.5, 0.5, 0.5]);
        assert_eq!(shape, ColliderShape::Ball { radius: 1.0 });
        let bola = world.create_rigid_body(20, BodyType::Dynamic, [0.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world.attach_collider(bola, shape, 0.0, 0.9).expect("collider bola");
        asentar(&mut world, 180);
        let t = world.get_transform(bola).expect("transform bola");
        assert!(
            (t.position[1] - 1.0).abs() < 0.05,
            "la bola debía descansar con el centro a su radio (1,0); está a {}",
            t.position[1]
        );
    }

    #[test]
    fn capsula_por_inferencia_se_apoya_de_pie_estable() {
        // Cápsula de preset (escala [1,2,1]): half_height 1,5 + radio
        // 0,5 → apoyada de pie el centro queda a 2,0, y quieta.
        let (mut world, _suelo) = mundo_con_suelo();
        let shape = collider_shape_for_builtin("capsule", [1.0, 2.0, 1.0], [0.5, 0.5, 0.5]);
        let capsula = world.create_rigid_body(30, BodyType::Dynamic, [0.0, 4.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world.attach_collider(capsula, shape, 0.0, 0.9).expect("collider cápsula");
        asentar(&mut world, 240);
        let t = world.get_transform(capsula).expect("transform cápsula");
        assert!(
            (t.position[1] - 2.0).abs() < 0.05,
            "la cápsula debía apoyar de pie con el centro a 2,0; está a {}",
            t.position[1]
        );
        // Y sigue ahí tras más pasos (apoyo estable, no equilibrio fugaz).
        let y1 = t.position[1];
        asentar(&mut world, 60);
        let t2 = world.get_transform(capsula).expect("transform cápsula 2");
        assert!((t2.position[1] - y1).abs() < 0.01, "la cápsula no quedó estable");
    }

    #[test]
    fn cilindro_por_inferencia_se_apoya_sobre_su_tapa() {
        let (mut world, _suelo) = mundo_con_suelo();
        let shape = collider_shape_for_builtin("cylinder", [1.0, 1.0, 1.0], [0.5, 0.5, 0.5]);
        let cilindro = world.create_rigid_body(40, BodyType::Dynamic, [2.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        world.attach_collider(cilindro, shape, 0.0, 0.9).expect("collider cilindro");
        asentar(&mut world, 240);
        let t = world.get_transform(cilindro).expect("transform cilindro");
        assert!(
            (t.position[1] - 1.0).abs() < 0.05,
            "el cilindro (half_height 1) debía descansar con el centro a 1,0; está a {}",
            t.position[1]
        );
    }
#[test]
fn capas_cruzadas_no_colisionan_entre_si() {
    let mut world = PhysicsWorld::new();

    // Suelo estático (entidad 1, capa 0 por defecto).
    let ground = world.create_rigid_body(1, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_box_collider(ground, [10.0, 0.5, 10.0], 0.0, 0.7);

    // Dos esferas dinámicas en la MISMA posición, en capas que se ignoran mutuamente.
    // Ambas ven la capa 0 (suelo) porque el bit 0 sigue puesto.
    let bh2 = world.create_rigid_body(2, BodyType::Dynamic, [0.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_sphere_collider_with_config(
        bh2, 0.5, 0.0, 0.7,
        ColliderConfig { layer: 1, mask: !(1u32 << 2), is_trigger: false },
    );

    let bh3 = world.create_rigid_body(3, BodyType::Dynamic, [0.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_sphere_collider_with_config(
        bh3, 0.5, 0.0, 0.7,
        ColliderConfig { layer: 2, mask: !(1u32 << 1), is_trigger: false },
    );

    // Dejar caer ambas hasta que reposen.
    for _ in 0..240 {
        world.step(1.0 / 60.0);
    }

    let y2 = world.get_transform(bh2).unwrap().position[1];
    let y3 = world.get_transform(bh3).unwrap().position[1];

    // Ambas deben descansar sobre el suelo (si colisionaran, una quedaría ~1.0 más alta).
    assert!((y2 - 0.5).abs() < 0.15, "esfera 2 fuera de rango: y = {}", y2);
    assert!((y3 - 0.5).abs() < 0.15, "esfera 3 fuera de rango: y = {}", y3);

    // El par (2,3) no debe aparecer como contacto.
    let pairs = world.contact_pairs();
    assert!(
        !pairs.iter().any(|&(a, b, _)| (a == 2 && b == 3) || (a == 3 && b == 2)),
        "las esferas de capas cruzadas no deberían colisionar entre sí",
    );
}

#[test]
fn trigger_no_bloquea_y_genera_eventos() {
    let mut world = PhysicsWorld::new();

    // Suelo estático (entidad 1).
    let ground = world.create_rigid_body(1, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_box_collider(ground, [10.0, 0.5, 10.0], 0.0, 0.7);

    // Caja sensor estática (entidad 4): no debe bloquear.
    let sensor = world.create_rigid_body(4, BodyType::Static, [0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_box_collider_with_config(
        sensor, [1.0, 1.0, 1.0], 0.0, 0.7,
        ColliderConfig { layer: 0, mask: u32::MAX, is_trigger: true },
    );

    // Esfera dinámica que atraviesa el sensor (entidad 5).
    let sphere = world.create_rigid_body(5, BodyType::Dynamic, [0.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_sphere_collider(sphere, 0.5, 0.0, 0.7);

    for _ in 0..300 {
        world.step(1.0 / 60.0);
    }

    // La esfera debe haber atravesado el sensor y reposar sobre el suelo.
    let y = world.get_transform(sphere).unwrap().position[1];
    assert!(y < 1.0, "la esfera debería estar por debajo del sensor: y = {}", y);

    // Drenar una vez al final: deben aparecer un Enter y un Exit del par (4,5) como trigger
let eventos = world.drain_contact_events();
    // Debe registrarse el ciclo completo del par (4,5) como trigger
    let mut hubo_enter = false;
    let mut hubo_exit = false;
    for ev in &eventos {
        if ((ev.a == 4 && ev.b == 5) || (ev.a == 5 && ev.b == 4)) && ev.is_trigger {
            match ev.kind {
                ContactKind::Enter => hubo_enter = true,
                ContactKind::Exit => hubo_exit = true,
            }
        }
    }
    assert!(hubo_enter, "Debe existir un Enter del par (4,5) con is_trigger=true");
    assert!(hubo_exit, "Debe existir un Exit del par (4,5) con is_trigger=true");
}

#[test]
fn colision_normal_genera_enter_y_par_activo() {
    let mut world = PhysicsWorld::new();
    // Suelo estático (entidad 1): superficie en y=0
    let suelo = world.create_rigid_body(1, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_box_collider(suelo, [10.0, 0.5, 10.0], 0.0, 0.7);
    // Esfera dinámica (entidad 2) que caerá sobre el suelo
    let esfera = world.create_rigid_body(2, BodyType::Dynamic, [0.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_sphere_collider(esfera, 0.5, 0.0, 0.7);

    // Simular la caída drenando eventos en cada step para capturar el Enter
    let mut hubo_enter = false;
    for _ in 0..300 {
        world.step(1.0 / 60.0);
        for ev in world.drain_contact_events() {
            let es_par = (ev.a == 1 && ev.b == 2) || (ev.a == 2 && ev.b == 1);
            if es_par && !ev.is_trigger && matches!(ev.kind, ContactKind::Enter) {
                hubo_enter = true;
            }
        }
    }
    assert!(hubo_enter, "Debe generarse un Enter normal entre suelo (1) y esfera (2)");

    // Tras depositarse, el par debe figurar como contacto activo
    let pares = world.contact_pairs();
    let activo = pares.iter().any(|&(a, b, _)| (a == 1 && b == 2) || (a == 2 && b == 1));
    assert!(activo, "El par (1,2) debe figurar como activo en contact_pairs()");
}

#[test]
fn exit_al_teletransportar() {
    let mut world = PhysicsWorld::new();
    // Suelo estático (entidad 1)
    let suelo = world.create_rigid_body(1, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_box_collider(suelo, [10.0, 0.5, 10.0], 0.0, 0.7);
    // Esfera dinámica (entidad 2) que cae y reposa
    let esfera = world.create_rigid_body(2, BodyType::Dynamic, [0.0, 3.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_sphere_collider(esfera, 0.5, 0.0, 0.7);

    // Dejar caer hasta reposar y limpiar los eventos de la caída
    for _ in 0..300 {
        world.step(1.0 / 60.0);
        world.drain_contact_events();
    }
    // Confirmar que hay contacto activo antes de teletransportar
    let pares = world.contact_pairs();
    assert!(
        pares.iter().any(|&(a, b, _)| (a == 1 && b == 2) || (a == 2 && b == 1)),
        "La esfera debe estar en contacto con el suelo antes de teletransportar"
    );

    // Teletransportar la esfera muy lejos y avanzar unos steps
    world.set_transform(esfera, [0.0, 100.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let mut hubo_exit = false;
    for _ in 0..3 {
        world.step(1.0 / 60.0);
        for ev in world.drain_contact_events() {
            let es_par = (ev.a == 1 && ev.b == 2) || (ev.a == 2 && ev.b == 1);
            if es_par && !ev.is_trigger && matches!(ev.kind, ContactKind::Exit) {
                hubo_exit = true;
            }
        }
        if hubo_exit {
            break;
        }
    }
    assert!(hubo_exit, "Debe emitirse un Exit normal al teletransportar la esfera fuera del contacto");
}

#[test]
fn cast_sphere_acierta_suelo_con_normal_arriba() {
    let mut world = PhysicsWorld::new();
    // Suelo estático (entidad 1): superficie en y=0
    let suelo = world.create_rigid_body(1, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_box_collider(suelo, [10.0, 0.5, 10.0], 0.0, 0.7);
    // Un step para que se actualice el pipeline de consultas
    world.step(1.0 / 60.0);

    // Lanzar una esfera hacia abajo desde por encima del suelo
    let hit = world
        .cast_sphere([0.0, 2.0, 0.0], 0.1, [0.0, -1.0, 0.0], 5.0, u32::MAX, 0)
        .expect("El cast_sphere debe acertar el suelo");

    // La cara superior del suelo tiene normal hacia arriba (~+y)
    assert!(
        hit.normal[1] > 0.9,
        "La normal del impacto debe apuntar hacia arriba (y≈1), se obtuvo {:?}",
        hit.normal
    );
    assert!(
        (hit.distance - 1.9).abs() < 0.05,
        "La distancia de contacto debe ser ≈1.9, se obtuvo {}",
        hit.distance
    );
}

#[test]
fn cast_ray_masked_respeta_mascara() {
    // Dos esferas estáticas en la misma posición en el eje del rayo,
    // pero en capas distintas: la máscara solo debe permitir impactar una.
    let mut world = PhysicsWorld::new();

    let bh_capa0 = world.create_rigid_body(1, BodyType::Static, [0.0, 0.0, 5.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_sphere_collider_with_config(
        bh_capa0,
        0.5,
        0.0,
        0.5,
        ColliderConfig { layer: 0, mask: u32::MAX, is_trigger: false },
    );

    let bh_capa1 = world.create_rigid_body(2, BodyType::Static, [0.0, 0.0, 10.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_sphere_collider_with_config(
        bh_capa1,
        0.5,
        0.0,
        0.5,
        ColliderConfig { layer: 1, mask: u32::MAX, is_trigger: false },
    );

    // Máscara que solo incluye la capa 1 (bit 1): debe impactar la esfera lejana,
    // ignorando la cercana que está en capa 0.
    let mascara_capa1 = 1u32 << 1;
    let hit = world.cast_ray_masked([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 100.0, mascara_capa1, 0);

    assert!(hit.is_some());
    let hit = hit.unwrap();
    assert_eq!(hit.entity_id, 2);
}

#[test]
fn overlap_sphere_lista_solo_lo_de_dentro() {
    // Tres cuerpos: dos dentro del radio de la esfera de consulta, uno fuera.
    let mut world = PhysicsWorld::new();

    let bh_dentro1 = world.create_rigid_body(1, BodyType::Static, [1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_sphere_collider(bh_dentro1, 0.3, 0.0, 0.5);

    let bh_dentro2 = world.create_rigid_body(2, BodyType::Static, [-1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_sphere_collider(bh_dentro2, 0.3, 0.0, 0.5);

    let bh_fuera = world.create_rigid_body(3, BodyType::Static, [50.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_sphere_collider(bh_fuera, 0.3, 0.0, 0.5);

    let resultado = world.overlap_sphere([0.0, 0.0, 0.0], 5.0, u32::MAX, 0);

    assert_eq!(resultado.len(), 2);
    assert!(resultado.contains(&1));
    assert!(resultado.contains(&2));
    assert!(!resultado.contains(&3));
}

#[test]
fn convex_hull_sostiene_como_caja() {
    // Un casco convexo con forma de caja debe comportarse como un suelo sólido:
    // una esfera dinámica que cae sobre él debe quedar apoyada en su superficie.
    let mut world = PhysicsWorld::new();

    let puntos_caja = [
        [-10.0, -0.5, -10.0],
        [10.0, -0.5, -10.0],
        [-10.0, -0.5, 10.0],
        [10.0, -0.5, 10.0],
        [-10.0, 0.5, -10.0],
        [10.0, 0.5, -10.0],
        [-10.0, 0.5, 10.0],
        [10.0, 0.5, 10.0],
    ];

    let bh_suelo = world.create_rigid_body(1, BodyType::Static, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    let collider = world.attach_convex_collider(
        bh_suelo,
        &puntos_caja,
        0.0,
        0.5,
        ColliderConfig { layer: 0, mask: u32::MAX, is_trigger: false },
    );
    assert!(collider.is_some());

    let bh_esfera = world.create_rigid_body(2, BodyType::Dynamic, [0.0, 5.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
    world.attach_sphere_collider(bh_esfera, 0.5, 0.0, 0.5);

    // Suficientes substeps para que la esfera caiga y se asiente sobre el casco convexo.
    for _ in 0..200 {
        world.step(1.0 / 60.0);
    }

    let transform = world.get_transform(bh_esfera).unwrap();
    let altura_final = transform.position[1];

    // La superficie del "suelo" convexo está en y=0.5, y el radio de la esfera es 0.5,
    // por lo que debe reposar cerca de y=1.0, sin atravesarlo ni quedar flotando.
    assert!(altura_final > 0.9 && altura_final < 1.1);
}




    // ------------------------------------------------------------------
    // Helpers DS-1.5a
    // ------------------------------------------------------------------

    /// Crea un cuerpo con la API de Fase 1. Los dinámicos van con rotaciones
    /// bloqueadas y damping 1.5 para que caigan frenados (lo aplica
    /// create_rigid_body a todo dinámico del mundo).
    fn crear_cuerpo(
        world: &mut PhysicsWorld,
        entidad: u64,
        dinamico: bool,
        pos: [f32; 3],
    ) -> BodyHandle {
        world.create_rigid_body(
            entidad,
            if dinamico { BodyType::Dynamic } else { BodyType::Static },
            pos,
            [0.0, 0.0, 0.0, 1.0],
        )
    }

    /// Lanza un rayo y devuelve la entidad impactada (si la hay).
    fn rayo_entidad(
        world: &mut PhysicsWorld,
        origen: [f32; 3],
        dir: [f32; 3],
        max_dist: f32,
    ) -> Option<u64> {
        world
            .cast_ray(origen, dir, max_dist, 0)
            .map(|hit| hit.entity_id)
    }

    /// Añade a `verts`/`idx` una caja alineada a ejes: 8 vértices y 12 triángulos.
    fn anadir_caja(
        min: [f32; 3],
        max: [f32; 3],
        verts: &mut Vec<[f32; 3]>,
        idx: &mut Vec<[u32; 3]>,
    ) {
        let base = verts.len() as u32;
        verts.extend_from_slice(&[
            [min[0], min[1], min[2]], // 0
            [max[0], min[1], min[2]], // 1
            [max[0], max[1], min[2]], // 2
            [min[0], max[1], min[2]], // 3
            [min[0], min[1], max[2]], // 4
            [max[0], min[1], max[2]], // 5
            [max[0], max[1], max[2]], // 6
            [min[0], max[1], max[2]], // 7
        ]);
        let caras: [[u32; 3]; 12] = [
            [0, 2, 1],
            [0, 3, 2], // -Z
            [4, 5, 6],
            [4, 6, 7], // +Z
            [0, 4, 7],
            [0, 7, 3], // -X
            [1, 2, 6],
            [1, 6, 5], // +X
            [0, 1, 5],
            [0, 5, 4], // -Y
            [3, 7, 6],
            [3, 6, 2], // +Y
        ];
        for c in caras {
            idx.push([base + c[0], base + c[1], base + c[2]]);
        }
    }

    /// Canal en U abierto por arriba, a lo largo de Z:
    /// fondo x[-1.2,1.2] y[-0.2,0] + paredes x[±1.0,±1.2] y[0,1], z[-2,2].
    /// 24 vértices y 36 triángulos en total.
    fn canal_en_u() -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let mut verts = Vec::with_capacity(24);
        let mut idx = Vec::with_capacity(36);
        // Fondo
        anadir_caja([-1.2, -0.2, -2.0], [1.2, 0.0, 2.0], &mut verts, &mut idx);
        // Pared izquierda
        anadir_caja([-1.2, 0.0, -2.0], [-1.0, 1.0, 2.0], &mut verts, &mut idx);
        // Pared derecha
        anadir_caja([1.0, 0.0, -2.0], [1.2, 1.0, 2.0], &mut verts, &mut idx);
        assert_eq!(verts.len(), 24);
        assert_eq!(idx.len(), 36);
        (verts, idx)
    }

    // ------------------------------------------------------------------
    // Tests DS-1.5a
    // ------------------------------------------------------------------

    #[test]
    fn offset_dos_esferas_reposa_y_rayo_localiza() {
        let mut world = PhysicsWorld::new();

        // Suelo estático: caja half [10, 0.5, 10] en y = -0.5 (superficie en y = 0).
        let suelo = crear_cuerpo(&mut world, 1, false, [0.0, -0.5, 0.0]);
        world
            .attach_collider_with_config(
                suelo,
                ColliderShape::Cuboid {
                    half_extents: [10.0, 0.5, 10.0],
                },
                0.0,
                0.8,
                ColliderConfig::default(),
            )
            .expect("collider del suelo");

        // Cuerpo dinámico con DOS esferas desplazadas en x.
        let esfera = crear_cuerpo(&mut world, 2, true, [0.0, 3.0, 0.0]);
        world
            .attach_collider_with_config_at(
                esfera,
                ColliderShape::Ball { radius: 0.5 },
                0.0,
                0.8,
                ColliderConfig::default(),
                [-1.2, 0.0, 0.0],
            )
            .expect("esfera izquierda");
        world
            .attach_collider_with_config_at(
                esfera,
                ColliderShape::Ball { radius: 0.5 },
                0.0,
                0.8,
                ColliderConfig::default(),
                [1.2, 0.0, 0.0],
            )
            .expect("esfera derecha");

        for _ in 0..300 {
            world.step(1.0 / 60.0);
        }

        // Reposa apoyado en ambas esferas: centro a y ≈ 0.5.
        let y = world.get_transform(esfera).unwrap().position[1];
        assert!((y - 0.5).abs() < 0.1, "y del cuerpo = {y}");

        // Rayo por el centro: pasa entre las dos esferas, NO toca la entidad 2.
        let central = rayo_entidad(&mut world, [0.0, 5.0, 0.0], [0.0, -1.0, 0.0], 10.0);
        assert_ne!(central, Some(2), "el rayo central tocó las esferas: {central:?}");

        // Rayo sobre la esfera derecha: SÍ toca la entidad 2.
        let lateral = rayo_entidad(&mut world, [1.2, 5.0, 0.0], [0.0, -1.0, 0.0], 10.0);
        assert_eq!(lateral, Some(2), "el rayo lateral no tocó las esferas: {lateral:?}");
    }

    #[test]
    fn trimesh_canal_recibe_esfera_en_el_fondo() {
        let mut world = PhysicsWorld::new();

        let (verts, idx) = canal_en_u();

        // Canal estático (UN solo cuerpo con el trimesh completo).
        let canal = crear_cuerpo(&mut world, 1, false, [0.0, 0.0, 0.0]);
        world
            .attach_trimesh_collider(canal, &verts, &idx, 0.0, 0.8, ColliderConfig::default())
            .expect("trimesh del canal");

        // Esfera dinámica que debe caer DENTRO del canal.
        let esfera = crear_cuerpo(&mut world, 2, true, [0.0, 3.0, 0.0]);
        world
            .attach_collider_with_config(
                esfera,
                ColliderShape::Ball { radius: 0.4 },
                0.0,
                0.8,
                ColliderConfig::default(),
            )
            .expect("esfera");

        for _ in 0..300 {
            world.step(1.0 / 60.0);
        }

        let pos = world.get_transform(esfera).unwrap().position;
        assert!((pos[1] - 0.4).abs() < 0.1, "la esfera no reposa en el fondo: y = {}", pos[1]);
        assert!(pos[0].abs() < 0.3, "la esfera se salió del canal: x = {}", pos[0]);
    }

    #[test]
    fn hull_del_canal_deja_la_esfera_sobre_el_borde() {
        let mut world = PhysicsWorld::new();

        let (verts, _idx) = canal_en_u();

        // Mismo canal, pero como nube de puntos convexa: el hull cierra la boca a y = 1.
        let canal = crear_cuerpo(&mut world, 1, false, [0.0, 0.0, 0.0]);
        world
            .attach_convex_collider(canal, &verts, 0.0, 0.8, ColliderConfig::default())
            .expect("hull del canal");

        let esfera = crear_cuerpo(&mut world, 2, true, [0.0, 3.0, 0.0]);
        world
            .attach_collider_with_config(
                esfera,
                ColliderShape::Ball { radius: 0.4 },
                0.0,
                0.8,
                ColliderConfig::default(),
            )
            .expect("esfera");

        for _ in 0..300 {
            world.step(1.0 / 60.0);
        }

        // Contraste clave: con hull la esfera NO entra, reposa sobre la tapa a y ≈ 1.4.
        let pos = world.get_transform(esfera).unwrap().position;
        assert!(
            (pos[1] - 1.4).abs() < 0.15,
            "la esfera no reposa sobre el hull: y = {}",
            pos[1]
        );
    }

    #[test]
    fn trimesh_degenerado_devuelve_none() {
        let mut world = PhysicsWorld::new();

        let cuerpo = crear_cuerpo(&mut world, 1, false, [0.0, 0.0, 0.0]);

        // Índices que referencian vértices inexistentes -> Err en Rapier -> None.
        let verts = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let idx = vec![[0u32, 1u32, 7u32]];

        let resultado =
            world.attach_trimesh_collider(cuerpo, &verts, &idx, 0.0, 0.8, ColliderConfig::default());
        assert!(resultado.is_none(), "un trimesh inválido debe devolver None");

        // Caso adicional: todos los vértices idénticos (malla degenerada).
        let verts_rep = vec![
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        ];
        let idx_rep = vec![[0u32, 1u32, 2u32]];
        let resultado_rep = world.attach_trimesh_collider(
            cuerpo,
            &verts_rep,
            &idx_rep,
            0.0,
            0.8,
            ColliderConfig::default(),
        );
        assert!(resultado_rep.is_none(), "una malla degenerada debe devolver None");
    }


    /// Puerta multinúcleo (Fase B): determinismo entre ejecuciones.
    /// 10 mundos idénticos (pila de cajas + una bola, una sola isla
    /// grande de contactos) corren 600 pasos; el hash de los bits de
    /// posiciones y velocidades finales debe ser idéntico en los 10.
    /// Con la feature `parallel` esto valida que el solver paralelo
    /// no introduce deriva entre ejecuciones en esta escena.
    #[test]
    fn determinismo_entre_ejecuciones_x10() {
        fn correr() -> u64 {
            let (mut world, _suelo) = mundo_con_suelo();
            let mut cuerpos = Vec::new();
            let mut id = 100u64;
            for x in 0..3 {
                for z in 0..2 {
                    for y in 0..2 {
                        id += 1;
                        let b = world.create_rigid_body(
                            id,
                            BodyType::Dynamic,
                            [x as f32 * 1.1 - 1.1, 2.0 + y as f32 * 1.05, z as f32 * 1.1 - 0.55],
                            [0.0, 0.0, 0.0, 1.0],
                        );
                        world
                            .attach_box_collider(b, [0.5, 0.5, 0.5], 0.0, 0.9)
                            .expect("collider caja");
                        cuerpos.push(b);
                    }
                }
            }
            id += 1;
            let bola = world.create_rigid_body(id, BodyType::Dynamic, [0.0, 6.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
            world
                .attach_collider(bola, ColliderShape::Ball { radius: 0.5 }, 0.0, 0.9)
                .expect("collider bola");
            cuerpos.push(bola);
            asentar(&mut world, 600);
            let mut hash: u64 = 0xcbf29ce484222325;
            for b in &cuerpos {
                let t = world.get_transform(*b).expect("transform");
                let v = world.get_linear_velocity(*b).expect("velocidad");
                for x in t.position.iter().chain(v.iter()) {
                    for byte in x.to_bits().to_le_bytes() {
                        hash ^= byte as u64;
                        hash = hash.wrapping_mul(0x100000001b3);
                    }
                }
            }
            hash
        }
        let referencia = correr();
        for _ in 1..10 {
            assert_eq!(correr(), referencia);
        }
    }

    /// Puerta multinúcleo (Fase B): la secuencia de ContactEvent que
    /// ve C# debe ser idéntica entre ejecuciones aunque la narrow
    /// phase emita en paralelo. Escena con entidades multi-collider
    /// (el caso del Enter/Exit espurio): dos cuerpos con dos
    /// colliders cada uno caen sobre el suelo y luego se retira uno
    /// (Exit). 20 mundos frescos, secuencias idénticas.
    #[test]
    fn secuencia_de_eventos_identica_x20() {
        fn correr() -> Vec<(u64, u64, u8, bool, u64)> {
            let (mut world, _suelo) = mundo_con_suelo();
            let cfg = ColliderConfig::default();
            let mut cuerpos = Vec::new();
            for (n, x) in [(200u64, -2.0f32), (300u64, 2.0f32)] {
                let b = world.create_rigid_body(n, BodyType::Dynamic, [x, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
                for dx in [-0.6f32, 0.6] {
                    world
                        .attach_collider_with_config_at(
                            b,
                            ColliderShape::Cuboid { half_extents: [0.5, 0.5, 0.5] },
                            0.0,
                            0.9,
                            cfg,
                            [dx, 0.0, 0.0],
                        )
                        .expect("collider offset");
                }
                cuerpos.push(b);
            }
            let mut secuencia = Vec::new();
            for paso in 0..240 {
                world.step(1.0 / 60.0);
                if paso == 150 {
                    world.remove_rigid_body(cuerpos[0]);
                }
                for ev in world.drain_contact_events() {
                    let kind = match ev.kind {
                        ContactKind::Enter => 0u8,
                        ContactKind::Exit => 1u8,
                    };
                    secuencia.push((ev.a, ev.b, kind, ev.is_trigger, ev.step));
                }
            }
            secuencia
        }
        let referencia = correr();
        assert!(!referencia.is_empty());
        for _ in 1..20 {
            assert_eq!(correr(), referencia);
        }
    }
}
