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
            },
        );

        true
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
                QueryFilter::new().exclude_collider(rapier_collider),
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
            let step = state.step_offset;
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
                    QueryFilter::new().exclude_collider(rapier_collider),
                );
                // Puerta de discontinuidad: un escalón de verdad
                // tiene una cara casi vertical, así que el suelo a
                // 0,05 m por delante está bajo y a radio+piel ya está
                // arriba; en una rampa ambos rayos caen sobre la
                // misma superficie inclinada y la diferencia es
                // pequeña. Sin esta puerta, la muestra «sube» rampas
                // que el slopeLimit debería bloquear.
                let discontinuidad = {
                    let filtro = QueryFilter::new().exclude_collider(rapier_collider);
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

        // `pos` ya incluye el carry: se suma una sola vez a new_pos.
        let new_pos = [
            pos[0] + movement.translation.x,
            pos[1] + movement.translation.y,
            pos[2] + movement.translation.z,
        ];
        // Desplazamiento real del frame: carry + movimiento propio.
        let applied = [
            carry[0] + movement.translation.x,
            carry[1] + movement.translation.y,
            carry[2] + movement.translation.z,
        ];

        // Sonda de suelo: resuelve (padre, translation) a valores propios antes
        // de mutar el estado.
        let (nuevo_suelo, nuevo_suelo_pos) = if movement.grounded {
            let shape_pos = Isometry::translation(new_pos[0], new_pos[1], new_pos[2]);
            let filter = QueryFilter::default().exclude_collider(rapier_collider);
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
}
