use rapier3d::control::{CharacterLength, KinematicCharacterController};
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::parry::shape::Capsule;
use rapier3d::prelude::*;
use crate::types::{BodyHandle, BodyType};
use crate::world::PhysicsWorld;

#[derive(Clone, Copy, Debug)]
pub struct CharacterConfig {
    pub radius: f32,
    pub height: f32,
    pub skin: f32,
    pub step_offset: f32,
    pub slope_limit_deg: f32,
    pub snap: f32,
}

pub struct CharacterState {
    pub last_hits: Vec<CharacterHit>,
    pub last_flags: u32,
    pub last_velocity: [f32; 3],
    pub controller: KinematicCharacterController,
    pub body: BodyHandle,
    pub rapier_collider: ColliderHandle,
    pub radius: f32,
    pub half_height: f32,
    pub skin: f32,
    pub step_offset: f32,
    pub pos: [f32; 3],
    pub grounded: bool,
    pub sliding: bool,
    /// Cuerpo Rapier del suelo actual (memoria de suelo para plataformas móviles).
    pub ground_body: Option<RigidBodyHandle>,
    /// Translation de `ground_body` cuando se registró.
    pub ground_pos: [f32; 3],
    /// Paquete C (paridad RB/CC): detección de colisiones activa (Unity: detectCollisions).
    pub detect_collisions: bool,
    /// Paquete C: despenetración automática contra solapes al mover.
    pub overlap_recovery: bool,
    /// Paquete C: límite de pendiente en grados, espejo de `controller.max_slope_climb_angle`.
    pub slope_limit_deg: f32,
    /// Paquete C: controlador habilitado (Unity: CharacterController.enabled).
    pub enabled: bool,
    /// Paquete C: máscara de capas que el CC puede ver en sus sondas.
    pub include_mask: u32,
    /// CC-1: distancia mínima de Move (Unity: minMoveDistance). Un movimiento
    /// pedido más corto se descarta. 0.0 = desactivado (Unity usa 0.001).
    pub min_move_distance: f32,
    /// CC-1: desplazamiento de la cápsula respecto a la entidad (Unity: center).
    /// `pos` sigue siendo el centro de la cápsula en mundo.
    pub center: [f32; 3],
    /// CC-2: fricción del collider de la cápsula (Unity: material del CC).
    pub friction: f32,
    /// CC-2: restitución del collider de la cápsula.
    pub restitution: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CharacterHit {
    pub entity: u64,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub move_direction: [f32; 3],
    pub move_length: f32,
}

/// Bits de `CharacterState::last_flags` (espejo de `CollisionFlags` de Unity).
pub const CHARACTER_FLAG_BELOW: u32 = 1;
pub const CHARACTER_FLAG_SIDES: u32 = 2;
pub const CHARACTER_FLAG_ABOVE: u32 = 4;
pub const MAX_CHARACTER_HITS: usize = 16;

impl PhysicsWorld {
    pub fn create_character(&mut self, entity_id: u64, pos: [f32; 3], cfg: CharacterConfig) -> bool {
        if self.characters.contains_key(&entity_id) {
            self.destroy_character(entity_id);
        }

        let body = self.create_rigid_body(entity_id, BodyType::Kinematic, pos, [0.0, 0.0, 0.0, 1.0]);
        let rapier_body = match self.get_rapier_handle(body) {
            Some((h, _)) => h,
            None => return false,
        };

        let half_height = (cfg.height * 0.5 - cfg.radius).max(0.01);
        // La cápsula del personaje es INVISIBLE para el solver y para
        // las consultas del mundo (membresía GROUP_32, filtro NONE):
        // el CharacterController se resuelve a sí mismo con sus propias
        // sondas (que excluyen su collider), así que nadie debe chocar
        // con ella ni verla en un raycast. Sin esto, el jugador se
        // disparaba a sí mismo: el ojo está DENTRO de la cápsula y todo
        // raycast/proyectil desde la cámara impactaba en la propia
        // cápsula a distancia 0 (cazado en la demo FPS, 2026-11-14).
        let builder = ColliderBuilder::capsule_y(half_height, cfg.radius)
            .friction(0.0)
            .restitution(0.0)
            .collision_groups(InteractionGroups::new(Group::GROUP_32, Group::NONE));
        let collider = builder.build();
        let rapier_collider = self
            .collider_set
            .insert_with_parent(collider, rapier_body, &mut self.rigid_body_set);

        // El autostep de Rapier queda APAGADO a propósito: su puerta
        // interna «¿puedo subir?» lanza la cápsula hacia arriba con
        // target_distance = offset y, como el personaje descansa
        // exactamente a esa distancia de la cara del escalón, el
        // barrido cuenta el propio escalón como techo y nunca sube
        // (verificado con sonda de impactos). El escalón lo resuelve
        // `move_character` en esta capa, como Unity.
        let controller = KinematicCharacterController {
            up: UnitVector::new_normalize(vector![0.0, 1.0, 0.0]),
            offset: CharacterLength::Absolute(cfg.skin.max(0.001)),
            slide: true,
            autostep: None,
            max_slope_climb_angle: cfg.slope_limit_deg.to_radians(),
            min_slope_slide_angle: (cfg.slope_limit_deg + 5.0)
                .to_radians()
                .min(89.0f32.to_radians()),
            snap_to_ground: if cfg.snap > 0.001 {
                Some(CharacterLength::Absolute(cfg.snap))
            } else {
                None
            },
            normal_nudge_factor: 1e-4,
        };

        self.characters.insert(
            entity_id,
            CharacterState {
                controller,
                body,
                rapier_collider,
                radius: cfg.radius,
                half_height,
                skin: cfg.skin,
                step_offset: cfg.step_offset,
                pos,
                grounded: false,
                sliding: false,
                ground_body: None,
                ground_pos: [0.0, 0.0, 0.0],
                last_hits: Vec::new(),
                last_flags: 0,
                last_velocity: [0.0, 0.0, 0.0],
                detect_collisions: true,
                overlap_recovery: true,
                slope_limit_deg: cfg.slope_limit_deg,
                enabled: true,
                include_mask: u32::MAX,
                min_move_distance: 0.0,
                center: [0.0, 0.0, 0.0],
                friction: 0.0,
                restitution: 0.0,
            },
        );

        true
    }

    /// Filtro de consulta de las sondas del CharacterController: excluye su
    /// propia cápsula (invisible para todo lo demás) y aplica la máscara de
    /// capas que puede ver (`include_mask`); la cápsula del CC lleva filter
    /// NONE en su propio collider, así que sigue invisible a otras sondas
    /// aunque esta máscara sea todo-unos. Función asociada sin `&self` para
    /// que el filtro salga con vida `'static` y no arrastre préstamos de
    /// `self.characters` a través de `move_character`.
    fn cc_filter(rapier_collider: ColliderHandle, include_mask: u32) -> QueryFilter<'static> {
        QueryFilter::new()
            .exclude_collider(rapier_collider)
            .groups(InteractionGroups::new(
                Group::all(),
                Group::from_bits_truncate(include_mask),
            ))
    }

    /// Pasada de despenetración (paquete C, overlap_recovery): hasta 4
    /// iteraciones de "detecta solapes y empuja fuera" con la cápsula del
    /// personaje en `*pos`. Si `overlap_recovery` está desactivado, el
    /// personaje puede quedar atascado en geometría, como en Unity.
    fn recover_overlap(&mut self, entity_id: u64, pos: &mut [f32; 3]) {
        let Some(state) = self.characters.get(&entity_id) else { return };
        let half_height = state.half_height;
        let radius = state.radius;
        let skin = state.skin;
        let filter = Self::cc_filter(state.rapier_collider, state.include_mask);
        let capsula = Capsule::new_y(half_height, radius);

        for _ in 0..4 {
            let shape_pos = Isometry::translation(pos[0], pos[1], pos[2]);
            let mut solapados: Vec<ColliderHandle> = Vec::new();
            self.query_pipeline.intersections_with_shape(
                &self.rigid_body_set,
                &self.collider_set,
                &shape_pos,
                &capsula,
                filter,
                |h| {
                    solapados.push(h);
                    true
                },
            );
            if solapados.is_empty() {
                break;
            }

            let mut empuje = [0.0f32; 3];
            for h in solapados {
                let Some(otra) = self.collider_set.get(h) else { continue };
                let otra_pose = *otra.position();
                if let Ok(Some(contact)) = rapier3d::parry::query::contact(
                    &shape_pos,
                    &capsula,
                    &otra_pose,
                    otra.shape(),
                    skin + 0.01,
                ) {
                    if contact.dist < 0.0 {
                        // SIGNO VERIFICADO contra parry 0.18 (`Contact`):
                        // `normal1` apunta hacia el exterior de la PRIMERA
                        // forma (la cápsula) — en su punta inferior es -Y,
                        // o sea HACIA el obstáculo. La dirección de
                        // expulsión de la cápsula es `normal2` (exterior
                        // de la SEGUNDA forma, el obstáculo): en la cara
                        // superior de una caja es +Y. Con `normal1` el
                        // ascensor se hundía en la plataforma (regresión
                        // character_platforms 2026-11-14).
                        let n = contact.normal2.into_inner();
                        let mag = -contact.dist + 0.001;
                        empuje[0] += n.x * mag;
                        empuje[1] += n.y * mag;
                        empuje[2] += n.z * mag;
                    }
                }
            }
            pos[0] += empuje[0];
            pos[1] += empuje[1];
            pos[2] += empuje[2];
        }
    }

    pub fn move_character(
        &mut self,
        entity_id: u64,
        desired: [f32; 3],
        dt: f32,
    ) -> Option<([f32; 3], bool, bool)> {
        // Sincroniza los colliders del cuerpo suelo con su cuerpo ANTES
        // de consultar: las posiciones de collider solo se propagan
        // desde los cuerpos al hacer step(), y el personaje se mueve
        // desde los scripts, antes del step. Sin esto, la geometría de
        // la plataforma iría siempre un paso por detrás del arrastre
        // (y en un mundo sin steps quedaría congelada en su posición
        // de creación, cancelando el carry vertical en cada frame).
        let suelo = self
            .characters
            .get(&entity_id)
            .and_then(|s| s.ground_body)
            .and_then(|gb| self.rigid_body_set.get(gb))
            .map(|rb| (*rb.position(), rb.colliders().to_vec()));
        if let Some((iso, handles)) = suelo {
            for h in handles {
                if let Some(col) = self.collider_set.get_mut(h) {
                    if let Some(local) = col.position_wrt_parent() {
                        let local = *local;
                        col.set_position(iso * local);
                    }
                }
            }
        }

        self.query_pipeline.update(&self.collider_set);

        let state = self.characters.get(&entity_id)?;
        let controller = state.controller;
        let half_height = state.half_height;
        let radius = state.radius;
        let mut pos = state.pos;
        let rapier_collider = state.rapier_collider;
        let body = state.body;
        let skin = state.skin;
        let grounded_prev = state.grounded;
        let ground_body = state.ground_body;
        let ground_pos = state.ground_pos;
        let enabled = state.enabled;
        let detect_collisions = state.detect_collisions;
        let overlap_recovery = state.overlap_recovery;
        let step_offset = state.step_offset;
        let include_mask = state.include_mask;
        let min_move_distance = state.min_move_distance;
        let pos_entrada = pos;

        // Paquete C: un CharacterController deshabilitado no se mueve ni
        // colisiona (Unity: CharacterController.enabled = false).
        if !enabled {
            if let Some(state) = self.characters.get_mut(&entity_id) {
                state.last_velocity = [0.0, 0.0, 0.0];
            }
            return Some(([0.0, 0.0, 0.0], self.characters.get(&entity_id)?.grounded, false));
        }

        // CC-1: minMoveDistance (Unity). Si el movimiento pedido es más corto
        // que el umbral se descarta la llamada entera, incluido el arrastre de
        // plataforma de este frame (en Unity el descarte es de la llamada a
        // Move completa). 0.0 = desactivado.
        let magnitud_pedida =
            (desired[0] * desired[0] + desired[1] * desired[1] + desired[2] * desired[2]).sqrt();
        if min_move_distance > 0.0 && magnitud_pedida < min_move_distance {
            if let Some(state) = self.characters.get_mut(&entity_id) {
                state.last_velocity = [0.0, 0.0, 0.0];
            }
            return Some(([0.0, 0.0, 0.0], self.characters.get(&entity_id)?.grounded, false));
        }

        // Paquete C: modo fantasma (detectCollisions = false). La cápsula ya
        // es invisible al solver; aquí el MOVIMIENTO también atraviesa todo,
        // sin arrastre de suelo (sin suelo no hay carry).
        if !detect_collisions {
            let nueva_pos = [pos[0] + desired[0], pos[1] + desired[1], pos[2] + desired[2]];
            let velocidad = if dt > 1e-6 {
                [desired[0] / dt, desired[1] / dt, desired[2] / dt]
            } else {
                [0.0, 0.0, 0.0]
            };
            if let Some(state) = self.characters.get_mut(&entity_id) {
                state.pos = nueva_pos;
                state.grounded = false;
                state.sliding = false;
                state.ground_body = None;
                state.ground_pos = [0.0, 0.0, 0.0];
                state.last_hits = Vec::new();
                state.last_flags = 0;
                state.last_velocity = velocidad;
            }
            self.set_transform(body, nueva_pos, [0.0, 0.0, 0.0, 1.0]);
            return Some((desired, false, false));
        }

        if overlap_recovery {
            self.recover_overlap(entity_id, &mut pos);
        }

        let filtro_sondas = Self::cc_filter(rapier_collider, include_mask);

        // Una pasada de move_shape desde `desde` con el deseo dado.
        let hits: std::cell::RefCell<Vec<rapier3d::control::CharacterCollision>> =
            std::cell::RefCell::new(Vec::new());
        let pasada = |ctrl: &KinematicCharacterController, desde: [f32; 3], deseo: [f32; 3]| {
            ctrl.move_shape(
                dt.max(1e-4),
                &self.rigid_body_set,
                &self.collider_set,
                &self.query_pipeline,
                &Capsule::new_y(half_height, radius),
                &Isometry::translation(desde[0], desde[1], desde[2]),
                vector![deseo[0], deseo[1], deseo[2]],
                filtro_sondas,
                |c| hits.borrow_mut().push(c),
            )
        };

        // Con los pies en el suelo y deseo descendente (la presión de
        // gravedad típica de un script), el movimiento se separa como
        // en Unity: primero el horizontal (pendientes y deslizamiento)
        // y luego el vertical (gravedad y snap); si el horizontal se
        // bloquea contra un escalón, esta capa lo sube a mano. En el
        // aire o saltando, una sola pasada con el vector completo.
        // Arrastre por plataforma: el personaje se mueve CON su suelo antes de
        // aplicar su propio deseo. Solo translation: el controller no rota, así
        // que una plataforma que gira no arrastra la rotación (fuera de v1).
        // Si el cuerpo ya no existe, carry = 0; la sonda de abajo reescribe el
        // suelo (y lo olvida si no hay impacto).
        let mut carry = [0.0f32; 3];
        if grounded_prev {
            if let Some(gb) = ground_body {
                if let Some(rb) = self.rigid_body_set.get(gb) {
                    let t = rb.translation();
                    carry = [
                        t.x - ground_pos[0],
                        t.y - ground_pos[1],
                        t.z - ground_pos[2],
                    ];
                }
            }
        }
        pos = [pos[0] + carry[0], pos[1] + carry[1], pos[2] + carry[2]];

        let separar = grounded_prev
            && desired[1] < 0.0
            && (desired[0] != 0.0 || desired[2] != 0.0);
        let movement = if separar {
            let step = step_offset;
            // Sobre suelo CINEMÁTICO, la pasada horizontal sale una
            // piel por encima: la fricción cinemática de Rapier
            // proyecta el movimiento restante sobre la normal del
            // suelo y anula el empuje normal minúsculo con el que
            // move_shape se separa de la superficie; sin él, el
            // barrido horizontal nace dentro de la distancia
            // objetivo (offset) y se auto-bloquea en un bucle de
            // impactos a toi 0 — el personaje no puede caminar
            // sobre plataformas cinemáticas (verificado contra
            // Rapier 0.23.1: sobre cuerpo Fixed la misma pasada
            // mueve; sobre Kinematic devuelve [0,0,0]). Saliendo
            // una piel por encima, el barrido nace fuera de esa
            // distancia y la pasada vertical posterior asienta.
            let suelo_kinematico = ground_body
                .and_then(|gb| self.rigid_body_set.get(gb))
                .map(|rb| rb.is_kinematic())
                .unwrap_or(false);
            let lift = if suelo_kinematico { skin } else { 0.0 };
            let pos_h = [pos[0], pos[1] + lift, pos[2]];
            let m1 = pasada(&controller, pos_h, [desired[0], 0.0, desired[2]]);
            // ¿Bloqueado contra una pared/escalón? Se intenta el
            // escalón propio: subir, avanzar y asentar (ver la nota
            // en create_character sobre el autostep de Rapier).
            let deseado_h = (desired[0] * desired[0] + desired[2] * desired[2]).sqrt();
            let aplicado_h =
                (m1.translation.x * m1.translation.x + m1.translation.z * m1.translation.z)
                    .sqrt();
            let mut escalon: Option<rapier3d::control::EffectiveCharacterMovement> = None;
            if step > 0.001 && deseado_h > 1e-6 && aplicado_h < 0.85 * deseado_h {
                // Muestra adelantada (la receta del propio Rapier en
                // su handle_stairs): desde step_offset por encima y
                // 0,30 m por delante, ¿hay suelo a una altura
                // subible? Si lo hay, el personaje se alza a esa
                // altura y reintenta el avance; si desde ahí tampoco
                // avanza (pared alta), se descarta y manda m1.
                let dir = [desired[0] / deseado_h, desired[2] / deseado_h];
                let muestra = [
                    pos[0] + dir[0] * 0.30,
                    pos[1] + step,
                    pos[2] + dir[1] * 0.30,
                ];
                let shape = Capsule::new_y(half_height, radius);
                let impacto = self.query_pipeline.cast_shape(
                    &self.rigid_body_set,
                    &self.collider_set,
                    &Isometry::translation(muestra[0], muestra[1], muestra[2]),
                    &vector![0.0, -1.0, 0.0],
                    &shape,
                    ShapeCastOptions {
                        target_distance: skin,
                        stop_at_penetration: false,
                        max_time_of_impact: step + 0.1,
                        compute_impact_geometry_on_penetration: true,
                    },
                    filtro_sondas,
                );
                // Puerta de discontinuidad: un escalón de verdad
                // tiene una cara casi vertical, así que el suelo a
                // 0,05 m por delante está bajo y a radio+piel ya está
                // arriba; en una rampa ambos rayos caen sobre la
                // misma superficie inclinada y la diferencia es
                // pequeña. Sin esta puerta, la muestra «sube» rampas
                // que el slopeLimit debería bloquear.
                let discontinuidad = {
                    let filtro = filtro_sondas;
                    let origen_y = pos[1] + step + 0.5;
                    let suelo_en = |adelante: f32| -> Option<f32> {
                        let rayo = Ray::new(
                            point![
                                pos[0] + dir[0] * adelante,
                                origen_y,
                                pos[2] + dir[1] * adelante
                            ],
                            vector![0.0, -1.0, 0.0],
                        );
                        self.query_pipeline
                            .cast_ray(
                                &self.rigid_body_set,
                                &self.collider_set,
                                &rayo,
                                step + 2.0,
                                true,
                                filtro,
                            )
                            .map(|(_, toi)| origen_y - toi)
                    };
                    match (suelo_en(0.05), suelo_en(radius + skin + 0.03)) {
                        (Some(cerca), Some(lejos)) => lejos - cerca > step * 0.6,
                        _ => false,
                    }
                };
                if let Some((_, hit)) = impacto {
                    // Un impacto a toi≈0 es la muestra NACIENDO dentro
                    // del escalón (step_offset menor que su altura):
                    // no es un aterrizaje, es penetración; sin este
                    // corte el personaje treparía step_offset por
                    // frame como una cremallera.
                    let aterriza_y = if hit.time_of_impact > 1e-4 && discontinuidad {
                        muestra[1] - hit.time_of_impact
                    } else {
                        pos[1]
                    };
                    if aterriza_y > pos[1] + 0.02 && aterriza_y <= pos[1] + step + 0.03 {
                        let pos_alto = [pos[0], aterriza_y, pos[2]];
                        let m1b = pasada(&controller, pos_alto, [desired[0], 0.0, desired[2]]);
                        let aplicado_h_b = (m1b.translation.x * m1b.translation.x
                            + m1b.translation.z * m1b.translation.z)
                            .sqrt();
                        if aplicado_h_b > aplicado_h + 1e-5 {
                            let pos_sobre = [
                                pos_alto[0] + m1b.translation.x,
                                pos_alto[1] + m1b.translation.y,
                                pos_alto[2] + m1b.translation.z,
                            ];
                            let m2b = pasada(&controller, pos_sobre, [0.0, desired[1], 0.0]);
                            escalon =
                                Some(rapier3d::control::EffectiveCharacterMovement {
                                    translation: vector![
                                        m1b.translation.x,
                                        (aterriza_y - pos[1])
                                            + m1b.translation.y
                                            + m2b.translation.y,
                                        m1b.translation.z
                                    ],
                                    grounded: m2b.grounded,
                                    is_sliding_down_slope: m1b.is_sliding_down_slope
                                        || m2b.is_sliding_down_slope,
                                });
                        }
                    }
                }
            }
            match escalon {
                Some(m) => m,
                None => {
                    let pos1 = [
                        pos_h[0] + m1.translation.x,
                        pos_h[1] + m1.translation.y,
                        pos_h[2] + m1.translation.z,
                    ];
                    let m2 = pasada(&controller, pos1, [0.0, desired[1], 0.0]);
                    rapier3d::control::EffectiveCharacterMovement {
                        // El lift se devuelve aquí: la posición final
                        // es pos_h + m1 + m2, y el movimiento se mide
                        // desde la `pos` sin levantar.
                        translation: m1.translation
                            + m2.translation
                            + vector![0.0, lift, 0.0],
                        grounded: m2.grounded,
                        is_sliding_down_slope: m1.is_sliding_down_slope
                            || m2.is_sliding_down_slope,
                    }
                }
            }
        } else {
            pasada(&controller, pos, desired)
        };

        // `pos` ya incluye el carry y el empuje de recover_overlap: se
        // suma una sola vez a new_pos.
        let new_pos = [
            pos[0] + movement.translation.x,
            pos[1] + movement.translation.y,
            pos[2] + movement.translation.z,
        ];
        // Desplazamiento real del frame: diferencia entre la posición
        // final y la de entrada; incluye el empuje de recover_overlap
        // (antes solo sumaba carry + movimiento y el llamante divergía
        // de state.pos cuando había despenetración).
        let applied = [
            new_pos[0] - pos_entrada[0],
            new_pos[1] - pos_entrada[1],
            new_pos[2] - pos_entrada[2],
        ];

        // Sonda de suelo: resuelve (padre, translation) a valores propios antes
        // de mutar el estado.
        let (nuevo_suelo, nuevo_suelo_pos) = if movement.grounded {
            let shape_pos = Isometry::translation(new_pos[0], new_pos[1], new_pos[2]);
            let filter = filtro_sondas;
            let options = ShapeCastOptions {
                max_time_of_impact: skin + 0.06,
                target_distance: skin,
                stop_at_penetration: true,
                compute_impact_geometry_on_penetration: true,
            };
            self.query_pipeline
                .cast_shape(
                    &self.rigid_body_set,
                    &self.collider_set,
                    &shape_pos,
                    &Vector::new(0.0, -1.0, 0.0),
                    self.collider_set[rapier_collider].shape(),
                    options,
                    filter,
                )
                .and_then(|(hit, _)| self.collider_set[hit].parent())
                .and_then(|padre| {
                    let t = self.rigid_body_set.get(padre)?.translation();
                    Some((padre, [t.x, t.y, t.z]))
                })
                .map_or((None, [0.0, 0.0, 0.0]), |(padre, t)| (Some(padre), t))
        } else {
            (None, [0.0, 0.0, 0.0])
        };

        // Ojos del controlador: convierte las colisiones recogidas por
        // las pasadas en hits con flags estilo Unity y la velocidad
        // aplicada, y lo guarda para character_last_move.
        let deseado_len = (desired[0] * desired[0] + desired[1] * desired[1] + desired[2] * desired[2]).sqrt();
        let dir = if deseado_len > 1e-6 {
            [desired[0] / deseado_len, desired[1] / deseado_len, desired[2] / deseado_len]
        } else {
            [0.0, 0.0, 0.0]
        };
        let mut flags = if movement.grounded { CHARACTER_FLAG_BELOW } else { 0 };
        let mut hits_out: Vec<CharacterHit> = Vec::new();
        for c in hits.borrow().iter() {
            let Some(ent) = self.entity_of_collider(c.handle) else { continue };
            if ent == entity_id {
                continue;
            }
            let n = c.hit.normal1;
            if n.y > 0.6 {
                flags |= CHARACTER_FLAG_BELOW;
            } else if n.y < -0.6 {
                flags |= CHARACTER_FLAG_ABOVE;
            } else {
                flags |= CHARACTER_FLAG_SIDES;
            }
            let ta = c.translation_applied;
            hits_out.push(CharacterHit {
                entity: ent,
                point: [c.hit.witness2.x, c.hit.witness2.y, c.hit.witness2.z],
                normal: [n.x, n.y, n.z],
                move_direction: dir,
                move_length: (ta.x * ta.x + ta.y * ta.y + ta.z * ta.z).sqrt(),
            });
            if hits_out.len() >= MAX_CHARACTER_HITS {
                break;
            }
        }
        let velocidad = if dt > 1e-6 {
            [applied[0] / dt, applied[1] / dt, applied[2] / dt]
        } else {
            [0.0, 0.0, 0.0]
        };

        if let Some(state) = self.characters.get_mut(&entity_id) {
            state.pos = new_pos;
            state.grounded = movement.grounded;
            state.sliding = movement.is_sliding_down_slope;
            state.ground_body = nuevo_suelo;
            state.ground_pos = nuevo_suelo_pos;
            state.last_hits = hits_out;
            state.last_flags = flags;
            state.last_velocity = velocidad;
        }

        self.set_transform(body, new_pos, [0.0, 0.0, 0.0, 1.0]);

        Some((applied, movement.grounded, movement.is_sliding_down_slope))
    }

    pub fn teleport_character(&mut self, entity_id: u64, pos: [f32; 3]) -> bool {
        let body = if let Some(state) = self.characters.get_mut(&entity_id) {
            state.pos = pos;
            state.grounded = false;
            // Sin arrastre residual tras teletransportar.
            state.ground_body = None;
            state.body
        } else {
            return false;
        };

        self.set_transform(body, pos, [0.0, 0.0, 0.0, 1.0]);
        true
    }

    pub fn destroy_character(&mut self, entity_id: u64) -> bool {
        if let Some(state) = self.characters.remove(&entity_id) {
            self.remove_rigid_body(state.body);
            true
        } else {
            false
        }
    }

    /// (flags, velocidad, hits) del último `move_character` de la entidad.
    pub fn character_last_move(
        &self,
        entity_id: u64,
    ) -> Option<(u32, [f32; 3], Vec<CharacterHit>)> {
        let state = self.characters.get(&entity_id)?;
        Some((state.last_flags, state.last_velocity, state.last_hits.clone()))
    }

    pub fn character_info(&self, entity_id: u64) -> Option<([f32; 3], bool, bool)> {
        self.characters
            .get(&entity_id)
            .map(|s| (s.pos, s.grounded, s.sliding))
    }

    // ── Paquete C: setters/getters de paridad RB/CC ─────────────────

    pub fn set_character_detect_collisions(&mut self, entity_id: u64, v: bool) -> bool {
        if let Some(state) = self.characters.get_mut(&entity_id) {
            state.detect_collisions = v;
            true
        } else {
            false
        }
    }

    pub fn get_character_detect_collisions(&self, entity_id: u64) -> Option<bool> {
        self.characters.get(&entity_id).map(|s| s.detect_collisions)
    }

    pub fn set_character_overlap_recovery(&mut self, entity_id: u64, v: bool) -> bool {
        if let Some(state) = self.characters.get_mut(&entity_id) {
            state.overlap_recovery = v;
            true
        } else {
            false
        }
    }

    pub fn get_character_overlap_recovery(&self, entity_id: u64) -> Option<bool> {
        self.characters.get(&entity_id).map(|s| s.overlap_recovery)
    }

    pub fn set_character_step_offset(&mut self, entity_id: u64, v: f32) -> bool {
        if !v.is_finite() || v <= 0.0 {
            return false;
        }
        if let Some(state) = self.characters.get_mut(&entity_id) {
            state.step_offset = v;
            true
        } else {
            false
        }
    }

    pub fn get_character_step_offset(&self, entity_id: u64) -> Option<f32> {
        self.characters.get(&entity_id).map(|s| s.step_offset)
    }

    pub fn set_character_slope_limit(&mut self, entity_id: u64, deg: f32) -> bool {
        if !deg.is_finite() {
            return false;
        }
        let deg = deg.clamp(0.0, 89.0);
        if let Some(state) = self.characters.get_mut(&entity_id) {
            state.slope_limit_deg = deg;
            state.controller.max_slope_climb_angle = deg.to_radians();
            state.controller.min_slope_slide_angle =
                (deg + 5.0).to_radians().min(89.0f32.to_radians());
            true
        } else {
            false
        }
    }

    pub fn get_character_slope_limit(&self, entity_id: u64) -> Option<f32> {
        self.characters.get(&entity_id).map(|s| s.slope_limit_deg)
    }

    pub fn set_character_enabled(&mut self, entity_id: u64, v: bool) -> bool {
        if let Some(state) = self.characters.get_mut(&entity_id) {
            state.enabled = v;
            true
        } else {
            false
        }
    }

    pub fn get_character_enabled(&self, entity_id: u64) -> Option<bool> {
        self.characters.get(&entity_id).map(|s| s.enabled)
    }

    pub fn set_character_include_layers(&mut self, entity_id: u64, mask: u32) -> bool {
        if let Some(state) = self.characters.get_mut(&entity_id) {
            state.include_mask = mask;
            true
        } else {
            false
        }
    }

    pub fn get_character_include_layers(&self, entity_id: u64) -> Option<u32> {
        self.characters.get(&entity_id).map(|s| s.include_mask)
    }

    /// CC-1: minMoveDistance. Rechaza no finitos y negativos; 0.0 = desactivado.
    pub fn set_character_min_move_distance(&mut self, entity_id: u64, v: f32) -> bool {
        if !v.is_finite() || v < 0.0 {
            return false;
        }
        if let Some(state) = self.characters.get_mut(&entity_id) {
            state.min_move_distance = v;
            true
        } else {
            false
        }
    }

    pub fn get_character_min_move_distance(&self, entity_id: u64) -> Option<f32> {
        self.characters.get(&entity_id).map(|s| s.min_move_distance)
    }

    pub fn set_character_material(&mut self, entity_id: u64, friction: f32, restitution: f32) -> bool {
        if !friction.is_finite() || friction < 0.0 || !restitution.is_finite() || restitution < 0.0 {
            return false;
        }
        let Some(state) = self.characters.get_mut(&entity_id) else {
            return false;
        };
        state.friction = friction;
        state.restitution = restitution;
        let handle = state.rapier_collider;
        if let Some(collider) = self.collider_set.get_mut(handle) {
            collider.set_friction(friction);
            collider.set_restitution(restitution);
        }
        true
    }

    pub fn get_character_material(&self, entity_id: u64) -> Option<(f32, f32)> {
        self.characters.get(&entity_id).map(|s| (s.friction, s.restitution))
    }

    /// Raycast analítico contra la cápsula del personaje (Y-arriba, centrada en `pos`).
    /// Devuelve (distancia, punto, normal). `dir` se normaliza aquí.
    pub fn character_raycast(
        &self,
        entity_id: u64,
        origin: [f32; 3],
        dir: [f32; 3],
        max_dist: f32,
    ) -> Option<(f32, [f32; 3], [f32; 3])> {
        use rapier3d::parry::query::RayCast;

        if !max_dist.is_finite() || max_dist < 0.0 {
            return None;
        }
        if origin.iter().chain(dir.iter()).any(|v| !v.is_finite()) {
            return None;
        }
        let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        if !len.is_finite() || len <= f32::EPSILON {
            return None;
        }
        let d = [dir[0] / len, dir[1] / len, dir[2] / len];

        let state = self.characters.get(&entity_id)?;
        let capsule = Capsule::new_y(state.half_height, state.radius);
        let iso = Isometry::translation(state.pos[0], state.pos[1], state.pos[2]);
        let ray = rapier3d::parry::query::Ray::new(
            Point::new(origin[0], origin[1], origin[2]),
            Vector::new(d[0], d[1], d[2]),
        );
        let hit = capsule.cast_ray_and_get_normal(&iso, &ray, max_dist, true)?;
        let t = hit.time_of_impact;
        let point = [origin[0] + d[0] * t, origin[1] + d[1] * t, origin[2] + d[2] * t];
        Some((t, point, [hit.normal.x, hit.normal.y, hit.normal.z]))
    }

    /// Punto más cercano sobre la cápsula. Con solid=true, un punto interior se devuelve tal cual.
    pub fn character_closest_point(&self, entity_id: u64, point: [f32; 3]) -> Option<[f32; 3]> {
        use rapier3d::parry::query::PointQuery;

        if point.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let state = self.characters.get(&entity_id)?;
        let capsule = Capsule::new_y(state.half_height, state.radius);
        let iso = Isometry::translation(state.pos[0], state.pos[1], state.pos[2]);
        let proj = capsule.project_point(&iso, &Point::new(point[0], point[1], point[2]), true);
        Some([proj.point.x, proj.point.y, proj.point.z])
    }

    /// Punto más cercano sobre el AABB de `character_bounds` (clamp por componente).
    pub fn character_closest_point_on_bounds(&self, entity_id: u64, point: [f32; 3]) -> Option<[f32; 3]> {
        if point.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let (min, max) = self.character_bounds(entity_id)?;
        Some([
            point[0].clamp(min[0], max[0]),
            point[1].clamp(min[1], max[1]),
            point[2].clamp(min[2], max[2]),
        ])
    }

    /// CC-1: center. Desplaza la cápsula en (nuevo − anterior) y mueve el
    /// cuerpo cinemático (patrón teleport_character). No se tocan grounded ni
    /// ground_body: la cápsula se mueve con su entidad y el siguiente Move
    /// reevalúa el suelo con sus sondas.
    pub fn set_character_center(&mut self, entity_id: u64, center: [f32; 3]) -> bool {
        if !center.iter().all(|c| c.is_finite()) {
            return false;
        }
        let (body, pos) = if let Some(state) = self.characters.get_mut(&entity_id) {
            let delta = [
                center[0] - state.center[0],
                center[1] - state.center[1],
                center[2] - state.center[2],
            ];
            state.center = center;
            if delta == [0.0, 0.0, 0.0] {
                return true;
            }
            state.pos = [
                state.pos[0] + delta[0],
                state.pos[1] + delta[1],
                state.pos[2] + delta[2],
            ];
            (state.body, state.pos)
        } else {
            return false;
        };

        self.set_transform(body, pos, [0.0, 0.0, 0.0, 1.0]);
        true
    }

    pub fn get_character_center(&self, entity_id: u64) -> Option<[f32; 3]> {
        self.characters.get(&entity_id).map(|s| s.center)
    }

    pub fn character_bounds(&self, entity_id: u64) -> Option<([f32; 3], [f32; 3])> {
        let state = self.characters.get(&entity_id)?;
        let pos = state.pos;
        let radius = state.radius;
        let half_height = state.half_height;
        let min = [pos[0] - radius, pos[1] - (half_height + radius), pos[2] - radius];
        let max = [pos[0] + radius, pos[1] + (half_height + radius), pos[2] + radius];
        Some((min, max))
    }
}

#[cfg(test)]
mod cc1_tests {
    use super::*;

    const ID: u64 = 4242;
    const DT: f32 = 1.0 / 60.0;

    // Mismo patrón de mundo y CharacterConfig que los tests existentes de
    // create_character (mundo vacío: no hace falta suelo para estos casos).
    fn mundo_con_personaje(pos: [f32; 3]) -> PhysicsWorld {
        let mut world = PhysicsWorld::new();
        let cfg = CharacterConfig {
            radius: 0.5,
            height: 2.0,
            skin: 0.02,
            step_offset: 0.4,
            slope_limit_deg: 45.0,
            snap: 0.2,
        };
        assert!(world.create_character(ID, pos, cfg));
        world
    }

    fn cerca(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn min_move_distance_descarta_movimiento_corto() {
        let mut world = mundo_con_personaje([0.0, 1.0, 0.0]);
        assert!(world.set_character_min_move_distance(ID, 0.5));
        let (aplicado, _, _) = world
            .move_character(ID, [0.1, 0.0, 0.0], DT)
            .expect("personaje existente");
        assert!(cerca(aplicado, [0.0, 0.0, 0.0]));
        assert!(cerca(world.characters[&ID].pos, [0.0, 1.0, 0.0]));
    }

    #[test]
    fn min_move_distance_deja_pasar_movimiento_largo() {
        let mut world = mundo_con_personaje([0.0, 1.0, 0.0]);
        assert!(world.set_character_min_move_distance(ID, 0.5));
        let (aplicado, _, _) = world
            .move_character(ID, [1.0, 0.0, 0.0], DT)
            .expect("personaje existente");
        assert!(aplicado[0] > 0.9);
        assert!(world.characters[&ID].pos[0] > 0.9);
    }

    #[test]
    fn min_move_distance_cero_por_defecto_no_descarta() {
        let mut world = mundo_con_personaje([0.0, 1.0, 0.0]);
        assert_eq!(world.get_character_min_move_distance(ID), Some(0.0));
        let (aplicado, _, _) = world
            .move_character(ID, [0.1, 0.0, 0.0], DT)
            .expect("personaje existente");
        assert!(aplicado[0] > 0.09);
        assert!(world.characters[&ID].pos[0] > 0.09);
    }

    #[test]
    fn min_move_distance_set_get_e_inexistente() {
        let mut world = mundo_con_personaje([0.0, 1.0, 0.0]);
        assert!(world.set_character_min_move_distance(ID, 0.25));
        assert_eq!(world.get_character_min_move_distance(ID), Some(0.25));
        assert!(world.set_character_min_move_distance(ID, 0.0));
        assert_eq!(world.get_character_min_move_distance(ID), Some(0.0));
        assert!(!world.set_character_min_move_distance(ID + 1, 0.25));
        assert_eq!(world.get_character_min_move_distance(ID + 1), None);
    }

    #[test]
    fn min_move_distance_rechaza_negativo_y_no_finito() {
        let mut world = mundo_con_personaje([0.0, 1.0, 0.0]);
        assert!(world.set_character_min_move_distance(ID, 0.25));
        assert!(!world.set_character_min_move_distance(ID, -0.1));
        assert!(!world.set_character_min_move_distance(ID, f32::NAN));
        assert_eq!(world.get_character_min_move_distance(ID), Some(0.25));
    }

    #[test]
    fn center_desplaza_la_capsula_y_vuelve() {
        let mut world = mundo_con_personaje([0.0, 1.0, 0.0]);
        assert!(world.set_character_center(ID, [0.5, 0.0, 0.0]));
        assert!(cerca(world.characters[&ID].pos, [0.5, 1.0, 0.0]));
        assert!(cerca(
            world.get_character_center(ID).expect("personaje existente"),
            [0.5, 0.0, 0.0]
        ));

        assert!(world.set_character_center(ID, [0.0, 0.0, 0.0]));
        assert!(cerca(world.characters[&ID].pos, [0.0, 1.0, 0.0]));
        assert!(cerca(
            world.get_character_center(ID).expect("personaje existente"),
            [0.0, 0.0, 0.0]
        ));
    }

    #[test]
    fn center_inexistente_y_no_finito() {
        let mut world = mundo_con_personaje([0.0, 1.0, 0.0]);
        assert!(!world.set_character_center(ID + 1, [0.5, 0.0, 0.0]));
        assert_eq!(world.get_character_center(ID + 1), None);

        assert!(!world.set_character_center(ID, [f32::INFINITY, 0.0, 0.0]));
        assert!(!world.set_character_center(ID, [0.0, f32::NAN, 0.0]));
        assert!(cerca(world.characters[&ID].pos, [0.0, 1.0, 0.0]));
        assert!(cerca(
            world.get_character_center(ID).expect("personaje existente"),
            [0.0, 0.0, 0.0]
        ));
    }
}

#[cfg(test)]
mod cc2_tests {
    use super::*;

    const ID: u64 = 4242;

    fn world_at(pos: [f32; 3]) -> PhysicsWorld {
        let mut w = PhysicsWorld::new();
        assert!(w.create_character(
            ID,
            pos,
            CharacterConfig {
                radius: 0.5,
                height: 2.0,
                skin: 0.02,
                step_offset: 0.4,
                slope_limit_deg: 45.0,
                snap: 0.2,
            },
        ));
        w
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn approx3(a: [f32; 3], b: [f32; 3]) -> bool {
        approx(a[0], b[0]) && approx(a[1], b[1]) && approx(a[2], b[2])
    }

    #[test]
    fn material_set_get_and_applied_to_collider() {
        let mut w = world_at([0.0, 1.0, 0.0]);
        assert_eq!(w.get_character_material(ID), Some((0.0, 0.0)));

        assert!(w.set_character_material(ID, 0.8, 0.3));
        let (f, r) = w.get_character_material(ID).unwrap();
        assert!(approx(f, 0.8));
        assert!(approx(r, 0.3));

        let handle = w.characters.get(&ID).map(|s| s.rapier_collider).unwrap();
        let collider = w.collider_set.get(handle).unwrap();
        assert!(approx(collider.friction(), 0.8));
        assert!(approx(collider.restitution(), 0.3));

        assert!(!w.set_character_material(ID, -0.1, 0.0));
        assert!(!w.set_character_material(ID, 0.0, -0.1));
        assert!(!w.set_character_material(ID, f32::NAN, 0.0));
        let (f, r) = w.get_character_material(ID).unwrap();
        assert!(approx(f, 0.8));
        assert!(approx(r, 0.3));

        assert!(!w.set_character_material(9999, 0.5, 0.5));
        assert_eq!(w.get_character_material(9999), None);
    }

    #[test]
    fn raycast_front_hit() {
        let w = world_at([0.0, 1.0, 0.0]);
        let (dist, point, normal) = w
            .character_raycast(ID, [-5.0, 1.0, 0.0], [1.0, 0.0, 0.0], 100.0)
            .unwrap();
        assert!(approx(dist, 4.5));
        assert!(approx3(point, [-0.5, 1.0, 0.0]));
        assert!(approx3(normal, [-1.0, 0.0, 0.0]));

        // Dirección no unitaria: se normaliza y da el mismo impacto.
        let (dist2, point2, _) = w
            .character_raycast(ID, [-5.0, 1.0, 0.0], [3.0, 0.0, 0.0], 100.0)
            .unwrap();
        assert!(approx(dist2, 4.5));
        assert!(approx3(point2, [-0.5, 1.0, 0.0]));
    }

    #[test]
    fn raycast_miss_and_invalid() {
        let w = world_at([0.0, 1.0, 0.0]);
        assert!(w.character_raycast(ID, [-5.0, 5.0, 0.0], [1.0, 0.0, 0.0], 100.0).is_none());
        assert!(w.character_raycast(ID, [-5.0, 1.0, 0.0], [0.0, 0.0, 0.0], 100.0).is_none());
        assert!(w.character_raycast(ID, [-5.0, 1.0, 0.0], [1.0, 0.0, 0.0], -1.0).is_none());
        assert!(w.character_raycast(ID, [-5.0, 1.0, 0.0], [1.0, 0.0, 0.0], f32::INFINITY).is_none());
        assert!(w.character_raycast(ID, [-5.0, 1.0, 0.0], [1.0, 0.0, 0.0], 3.0).is_none());
        assert!(w.character_raycast(9999, [-5.0, 1.0, 0.0], [1.0, 0.0, 0.0], 100.0).is_none());
    }

    #[test]
    fn closest_point_on_capsule() {
        let w = world_at([0.0, 1.0, 0.0]);
        let p = w.character_closest_point(ID, [3.0, 1.0, 0.0]).unwrap();
        assert!(approx3(p, [0.5, 1.0, 0.0]));
        let inside = w.character_closest_point(ID, [0.0, 1.0, 0.0]).unwrap();
        assert!(approx3(inside, [0.0, 1.0, 0.0]));
        assert!(w.character_closest_point(9999, [3.0, 1.0, 0.0]).is_none());
    }

    #[test]
    fn closest_point_on_bounds_clamps() {
        let w = world_at([0.0, 1.0, 0.0]);
        let p = w.character_closest_point_on_bounds(ID, [3.0, 1.0, 0.0]).unwrap();
        assert!(approx3(p, [0.5, 1.0, 0.0]));
        let inside = w.character_closest_point_on_bounds(ID, [0.0, 1.0, 0.0]).unwrap();
        assert!(approx3(inside, [0.0, 1.0, 0.0]));
        let corner = w.character_closest_point_on_bounds(ID, [3.0, 5.0, 0.0]).unwrap();
        assert!(approx3(corner, [0.5, 2.0, 0.0]));
        assert!(w.character_closest_point_on_bounds(9999, [3.0, 1.0, 0.0]).is_none());
    }
}
