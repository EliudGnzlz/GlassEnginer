use std::collections::{HashMap, HashSet, VecDeque};
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::pipeline::ChannelEventCollector;
use rapier3d::prelude::*;
use glass_core::TransformFfi;
use crate::types::{
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
                        self.push_contact_event(pair, ContactKind::Enter, is_trigger);
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
                    self.push_contact_event(pair, ContactKind::Exit, is_trigger);
                }
            }
        }
    }

    /// Registra un evento agregado (por par de entidades) en `pending` y en
    /// el anillo de inspección.
    fn push_contact_event(&mut self, pair: (u64, u64), kind: ContactKind, is_trigger: bool) {
        let ev = ContactEvent {
            a: pair.0,
            b: pair.1,
            kind,
            is_trigger,
            step: self.step_count,
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
    fn entity_of_collider(&self, h: rapier3d::geometry::ColliderHandle) -> Option<u64> {
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
        let quat = nalgebra::Quaternion::new(rot[3], rot[0], rot[1], rot[2]);
        let unit_quat = nalgebra::UnitQuaternion::from_quaternion(quat);

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

        Some(collider_handle.into_raw_parts().0 as u64)
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

        Some(collider_handle.into_raw_parts().0 as u64)
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

        Some(collider_handle.into_raw_parts().0 as u64)
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

        Some(collider_handle.into_raw_parts().0 as u64)
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
        Some(collider_handle.into_raw_parts().0 as u64)
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
        Some(collider_handle.into_raw_parts().0 as u64)
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
        Some(collider_handle.into_raw_parts().0 as u64)
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
        true
    }

    pub fn get_mass(&self, handle: BodyHandle) -> Option<f32> {
        let (rapier, _) = self.get_rapier_handle(handle)?;
        self.rigid_body_set.get(rapier).map(|rb| rb.mass())
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
