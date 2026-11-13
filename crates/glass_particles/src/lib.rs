//! Sistema de partículas CPU (Fase 8P): pool SoA acotado,
//! emisión por tasa y ráfagas, formas básicas, simulación Euler
//! con gravedad y drag. Sin dependencias del resto del motor: la
//! app le pasa el origen y la base del emisor por frame y recoge
//! las partículas vivas para expandir los billboards.
//!
//! El layout SoA se fijó midiendo (benchmark en
//! `examples/bench_pool.rs`): el mismo paso de simulación con
//! 50.000 partículas salió un 20–26 % más rápido en SoA que en
//! AoS, con resultados idénticos.

pub mod ffi;

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Forma de emisión. La dirección base de salida es el eje +Y de
/// la base del emisor (su «arriba» local).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum EmitterShape {
    Point,
    Sphere { radius: f32 },
    Box { extents: [f32; 3] },
    Cone { angle_deg: f32, radius: f32 },
}

/// Configuración completa de un emisor. Los rangos (min, max) se
/// muestrean por partícula con el RNG del emisor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmitterConfig {
    pub max_particles: usize,
    /// Partículas por segundo mientras está en Play.
    pub rate: f32,
    pub lifetime: [f32; 2],
    pub start_speed: [f32; 2],
    pub start_size: [f32; 2],
    pub gravity: [f32; 3],
    /// Fracción de velocidad que se pierde por segundo (0 = nada).
    pub drag: f32,
    pub shape: EmitterShape,
    /// true: las partículas siguen el movimiento del emisor.
    pub local_space: bool,
    pub color_start: [f32; 4],
    pub color_end: [f32; 4],
    /// Tamaño final = tamaño inicial × este factor.
    pub size_end_factor: f32,
    /// Velocidad de rotación del billboard, rad/s (rango).
    pub rotation_speed: [f32; 2],
    /// Billboard estirado a lo largo de la velocidad.
    pub stretched: bool,
    /// Longitud del estirado por cada m/s de velocidad.
    pub stretch_factor: f32,
    pub seed: u32,
}

impl Default for EmitterConfig {
    fn default() -> Self {
        // Un fuego razonable: cono hacia arriba, flotación leve,
        // naranja que se apaga a rojo transparente encogiendo.
        Self {
            max_particles: 2000,
            rate: 700.0,
            lifetime: [1.6, 2.8],
            start_speed: [1.2, 2.4],
            start_size: [0.35, 0.6],
            gravity: [0.0, 1.2, 0.0],
            drag: 0.6,
            shape: EmitterShape::Cone {
                angle_deg: 18.0,
                radius: 0.45,
            },
            local_space: false,
            color_start: [1.0, 0.55, 0.12, 0.85],
            color_end: [0.75, 0.08, 0.02, 0.0],
            size_end_factor: 0.25,
            rotation_speed: [-1.2, 1.2],
            stretched: false,
            stretch_factor: 0.08,
            seed: 1,
        }
    }
}

/// RNG xorshift32 determinista (la misma semilla da la misma
/// emisión: importante para pruebas y para que el contenido se
/// vea igual en cada arranque).
#[derive(Debug, Clone)]
pub struct Rng {
    state: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self {
            state: if seed == 0 { 0x9E37_79B9 } else { seed },
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// Uniforme en [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }

    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.unit()
    }

    /// Punto uniforme dentro de la esfera unidad.
    pub fn in_unit_sphere(&mut self) -> [f32; 3] {
        loop {
            let p = [
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
            ];
            if p[0] * p[0] + p[1] * p[1] + p[2] * p[2] <= 1.0 {
                return p;
            }
        }
    }
}

fn v_add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn v_scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn v_len(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn v_norm(a: [f32; 3]) -> [f32; 3] {
    let l = v_len(a);
    if l > 1e-6 {
        v_scale(a, 1.0 / l)
    } else {
        [0.0, 1.0, 0.0]
    }
}
/// Aplica la base (columnas right/up/forward) a un vector local.
fn v_basis(basis: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        basis[0][0] * v[0] + basis[1][0] * v[1] + basis[2][0] * v[2],
        basis[0][1] * v[0] + basis[1][1] * v[1] + basis[2][1] * v[2],
        basis[0][2] * v[0] + basis[1][2] * v[1] + basis[2][2] * v[2],
    ]
}

/// Pool de partículas en arrays paralelos (SoA). Las vivas son
/// `0..alive`; al morir una, la última ocupa su hueco.
#[derive(Debug, Default)]
pub struct ParticlePool {
    pub pos: Vec<[f32; 3]>,
    pub vel: Vec<[f32; 3]>,
    pub age: Vec<f32>,
    pub life: Vec<f32>,
    pub size0: Vec<f32>,
    pub rot0: Vec<f32>,
    pub rot_speed: Vec<f32>,
    pub alive: usize,
}

impl ParticlePool {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            pos: Vec::with_capacity(cap),
            vel: Vec::with_capacity(cap),
            age: Vec::with_capacity(cap),
            life: Vec::with_capacity(cap),
            size0: Vec::with_capacity(cap),
            rot0: Vec::with_capacity(cap),
            rot_speed: Vec::with_capacity(cap),
            alive: 0,
        }
    }

    fn spawn(
        &mut self,
        cap: usize,
        pos: [f32; 3],
        vel: [f32; 3],
        life: f32,
        size: f32,
        rot0: f32,
        rot_speed: f32,
    ) -> bool {
        if self.alive >= cap {
            return false;
        }
        if self.alive == self.pos.len() {
            self.pos.push(pos);
            self.vel.push(vel);
            self.age.push(0.0);
            self.life.push(life);
            self.size0.push(size);
            self.rot0.push(rot0);
            self.rot_speed.push(rot_speed);
        } else {
            let i = self.alive;
            self.pos[i] = pos;
            self.vel[i] = vel;
            self.age[i] = 0.0;
            self.life[i] = life;
            self.size0[i] = size;
            self.rot0[i] = rot0;
            self.rot_speed[i] = rot_speed;
        }
        self.alive += 1;
        true
    }

    fn kill(&mut self, i: usize) {
        let last = self.alive - 1;
        if i != last {
            self.pos[i] = self.pos[last];
            self.vel[i] = self.vel[last];
            self.age[i] = self.age[last];
            self.life[i] = self.life[last];
            self.size0[i] = self.size0[last];
            self.rot0[i] = self.rot0[last];
            self.rot_speed[i] = self.rot_speed[last];
        }
        self.alive -= 1;
    }

    /// Fracción de vida recorrida de la partícula `i` (0..1).
    pub fn life_t(&self, i: usize) -> f32 {
        if self.life[i] > 0.0 {
            (self.age[i] / self.life[i]).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }
}

/// Un emisor vivo: configuración + pool + estado de emisión.
#[derive(Debug)]
pub struct Emitter {
    pub config: EmitterConfig,
    pub pool: ParticlePool,
    pub playing: bool,
    accumulator: f32,
    rng: Rng,
}

impl Emitter {
    pub fn new(config: EmitterConfig) -> Self {
        let cap = config.max_particles.max(1);
        let rng = Rng::new(config.seed);
        Self {
            pool: ParticlePool::with_capacity(cap),
            config,
            playing: true,
            accumulator: 0.0,
            rng,
        }
    }

    pub fn play(&mut self) {
        self.playing = true;
    }

    pub fn stop(&mut self) {
        self.playing = false;
    }

    pub fn clear(&mut self) {
        self.pool.alive = 0;
        self.accumulator = 0.0;
    }

    /// Muestra (posición local, dirección local) según la forma.
    fn sample_spawn(&mut self) -> ([f32; 3], [f32; 3]) {
        match self.config.shape {
            EmitterShape::Point => ([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            EmitterShape::Sphere { radius } => {
                let p = v_scale(self.rng.in_unit_sphere(), radius);
                (p, v_norm(p))
            }
            EmitterShape::Box { extents } => {
                let p = [
                    self.rng.range(-extents[0], extents[0]),
                    self.rng.range(-extents[1], extents[1]),
                    self.rng.range(-extents[2], extents[2]),
                ];
                (p, [0.0, 1.0, 0.0])
            }
            EmitterShape::Cone { angle_deg, radius } => {
                // Origen en un disco de radio `radius` (plano XZ).
                let a = self.rng.unit() * std::f32::consts::TAU;
                let r = radius * self.rng.unit().sqrt();
                let origin = [r * a.cos(), 0.0, r * a.sin()];
                // Dirección dentro del cono alrededor de +Y.
                let tilt = (angle_deg.to_radians()) * self.rng.unit().sqrt();
                let dir = [
                    tilt.sin() * a.cos(),
                    tilt.cos(),
                    tilt.sin() * a.sin(),
                ];
                (origin, dir)
            }
        }
    }

    fn spawn_one(&mut self, origin: [f32; 3], basis: &[[f32; 3]; 3]) {
        let (local_pos, local_dir) = self.sample_spawn();
        let pos = v_add(origin, v_basis(basis, local_pos));
        let speed = self
            .rng
            .range(self.config.start_speed[0], self.config.start_speed[1]);
        let vel = v_scale(v_norm(v_basis(basis, local_dir)), speed);
        let life = self
            .rng
            .range(self.config.lifetime[0], self.config.lifetime[1]);
        let size = self
            .rng
            .range(self.config.start_size[0], self.config.start_size[1]);
        let rot0 = self.rng.range(0.0, std::f32::consts::TAU);
        let rot_speed = self
            .rng
            .range(self.config.rotation_speed[0], self.config.rotation_speed[1]);
        self.pool.spawn(
            self.config.max_particles.max(1),
            pos,
            vel,
            life.max(0.01),
            size,
            rot0,
            rot_speed,
        );
    }

    /// Emite `n` partículas al momento.
    pub fn burst(&mut self, n: usize, origin: [f32; 3], basis: &[[f32; 3]; 3]) {
        for _ in 0..n {
            if self.pool.alive >= self.config.max_particles.max(1) {
                break;
            }
            self.spawn_one(origin, basis);
        }
    }

    /// Avanza la simulación un paso. `origin` y `basis` son la
    /// posición y orientación actuales del emisor en el mundo;
    /// `prev_origin` es la del frame anterior (para el espacio
    /// local: las partículas siguen el delta del emisor).
    pub fn update(
        &mut self,
        dt: f32,
        origin: [f32; 3],
        basis: &[[f32; 3]; 3],
        prev_origin: [f32; 3],
    ) {
        const UMBRAL_PAR: usize = 8192;

        if dt <= 0.0 {
            return;
        }
        // Espacio local: arrastrar las vivas con el emisor.
        if self.config.local_space {
            let delta = [
                origin[0] - prev_origin[0],
                origin[1] - prev_origin[1],
                origin[2] - prev_origin[2],
            ];
            if delta != [0.0, 0.0, 0.0] {
                if self.pool.alive >= UMBRAL_PAR {
                    use rayon::prelude::*;
                    self.pool.pos[..self.pool.alive]
                        .par_iter_mut()
                        .for_each(|p| *p = v_add(*p, delta));
                } else {
                    for i in 0..self.pool.alive {
                        self.pool.pos[i] = v_add(self.pool.pos[i], delta);
                    }
                }
            }
        }
        // Emisión por tasa, con arrastre fraccional.
        if self.playing && self.config.rate > 0.0 {
            self.accumulator += self.config.rate * dt;
            let mut n = self.accumulator.floor() as usize;
            self.accumulator -= n as f32;
            while n > 0 && self.pool.alive < self.config.max_particles.max(1) {
                self.spawn_one(origin, basis);
                n -= 1;
            }
        }
        // Integración Euler: gravedad, drag, posición, edad.
        let drag_factor = (1.0 - self.config.drag * dt).max(0.0);
        let g = self.config.gravity;
        if self.pool.alive >= UMBRAL_PAR {
            use rayon::prelude::*;
            let alive = self.pool.alive;
            self.pool.pos[..alive]
                .par_iter_mut()
                .zip(self.pool.vel[..alive].par_iter_mut())
                .zip(self.pool.age[..alive].par_iter_mut())
                .for_each(|((pos, vel), age)| {
                    *age += dt;
                    let v = *vel;
                    let v = [
                        (v[0] + g[0] * dt) * drag_factor,
                        (v[1] + g[1] * dt) * drag_factor,
                        (v[2] + g[2] * dt) * drag_factor,
                    ];
                    *vel = v;
                    *pos = v_add(*pos, v_scale(v, dt));
                });
            let mut i = 0;
            while i < self.pool.alive {
                if self.pool.age[i] >= self.pool.life[i] {
                    self.pool.kill(i);
                    continue;
                }
                i += 1;
            }
        } else {
            let mut i = 0;
            while i < self.pool.alive {
                self.pool.age[i] += dt;
                if self.pool.age[i] >= self.pool.life[i] {
                    self.pool.kill(i);
                    continue;
                }
                let v = self.pool.vel[i];
                let v = [
                    (v[0] + g[0] * dt) * drag_factor,
                    (v[1] + g[1] * dt) * drag_factor,
                    (v[2] + g[2] * dt) * drag_factor,
                ];
                self.pool.vel[i] = v;
                self.pool.pos[i] = v_add(self.pool.pos[i], v_scale(v, dt));
                i += 1;
            }
        }
    }

    /// Tamaño actual de la partícula `i` (curva lineal por vida).
    pub fn size_at(&self, i: usize) -> f32 {
        let t = self.pool.life_t(i);
        self.pool.size0[i] * (1.0 + (self.config.size_end_factor - 1.0) * t)
    }

    /// Color actual de la partícula `i`: gradiente lineal con el
    /// alfa modulado por una entrada y salida suaves (fade).
    pub fn color_at(&self, i: usize) -> [f32; 4] {
        let t = self.pool.life_t(i);
        let mut c = [0.0; 4];
        for k in 0..4 {
            c[k] = self.config.color_start[k]
                + (self.config.color_end[k] - self.config.color_start[k]) * t;
        }
        let fade_in = (t / 0.12).min(1.0);
        let fade_out = ((1.0 - t) / 0.25).min(1.0);
        c[3] *= fade_in.min(fade_out);
        c
    }

    /// Ángulo actual del billboard de la partícula `i`.
    pub fn rotation_at(&self, i: usize) -> f32 {
        self.pool.rot0[i] + self.pool.rot_speed[i] * self.pool.age[i]
    }
}

/// Comandos de control que llegan de fuera del bucle (MCP o
/// scripts C#): la app los drena una vez por frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleCommand {
    Play(u64),
    Stop(u64),
    Burst(u64, u32),
}

static COMMANDS: Mutex<Vec<ParticleCommand>> = Mutex::new(Vec::new());

/// Encola un comando de control para la app.
pub fn push_command(cmd: ParticleCommand) {
    if let Ok(mut q) = COMMANDS.lock() {
        q.push(cmd);
    }
}

/// La app recoge los comandos pendientes (y vacía la cola).
pub fn drain_commands() -> Vec<ParticleCommand> {
    if let Ok(mut q) = COMMANDS.lock() {
        std::mem::take(&mut *q)
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIS: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    fn config_base() -> EmitterConfig {
        EmitterConfig {
            rate: 60.0,
            lifetime: [10.0, 10.0],
            shape: EmitterShape::Point,
            gravity: [0.0, 0.0, 0.0],
            drag: 0.0,
            ..EmitterConfig::default()
        }
    }

    #[test]
    fn la_tasa_acumula_con_arrastre() {
        let mut e = Emitter::new(config_base());
        for _ in 0..60 {
            e.update(1.0 / 60.0, [0.0, 0.0, 0.0], &BASIS, [0.0, 0.0, 0.0]);
        }
        assert_eq!(e.pool.alive, 60);
    }

    #[test]
    fn paralelo_equivale_al_secuencial_con_mas_de_8192() {
        // Por encima de UMBRAL_PAR el update integra en paralelo y
        // compacta después: el conjunto de supervivientes y sus
        // valores deben ser bit a bit los del modelo secuencial.
        let mut cfg = config_base();
        cfg.rate = 0.0;
        cfg.max_particles = 20_000;
        cfg.gravity = [0.0, -9.8, 0.0];
        cfg.drag = 0.5;
        cfg.local_space = true;
        let mut e = Emitter::new(cfg);
        e.burst(20_000, [0.0, 0.0, 0.0], &BASIS);
        assert_eq!(e.pool.alive, 20_000);
        // La mitad muere en este paso (vida 0,05 < dt 0,1).
        for i in 0..e.pool.alive {
            e.pool.life[i] = if i % 2 == 0 { 0.05 } else { 10.0 };
            e.pool.vel[i] = [i as f32 * 0.01, 1.0, -2.0];
        }
        let pos0: Vec<[f32; 3]> = e.pool.pos[..e.pool.alive].to_vec();
        let vel0: Vec<[f32; 3]> = e.pool.vel[..e.pool.alive].to_vec();
        let dt = 0.1f32;
        let delta = [1.0, 0.0, 0.0];
        e.update(dt, delta, &BASIS, [0.0, 0.0, 0.0]);
        // Modelo secuencial: arrastre, edad, muerte o integración.
        let drag_factor = (1.0f32 - 0.5 * dt).max(0.0);
        let mut esperado: Vec<([f32; 3], [f32; 3])> = Vec::new();
        for i in (0..pos0.len()).step_by(2) {
            let _ = i; // las pares murieron; solo sobreviven las impares
        }
        for i in (1..pos0.len()).step_by(2) {
            let pos = v_add(pos0[i], delta);
            let v = vel0[i];
            let v = [
                (v[0] + 0.0 * dt) * drag_factor,
                (v[1] + -9.8 * dt) * drag_factor,
                (v[2] + 0.0 * dt) * drag_factor,
            ];
            esperado.push((v_add(pos, v_scale(v, dt)), v));
        }
        let mut obtenido: Vec<([f32; 3], [f32; 3])> = (0..e.pool.alive)
            .map(|i| (e.pool.pos[i], e.pool.vel[i]))
            .collect();
        let clave = |x: &([f32; 3], [f32; 3])| {
            (x.0[0].to_bits(), x.0[1].to_bits(), x.0[2].to_bits())
        };
        esperado.sort_by_key(clave);
        obtenido.sort_by_key(clave);
        assert_eq!(obtenido.len(), esperado.len());
        assert_eq!(obtenido, esperado);
    }

    #[test]
    fn la_vida_mata_y_libera() {
        let mut cfg = config_base();
        cfg.lifetime = [0.05, 0.05];
        let mut e = Emitter::new(cfg);
        e.burst(5, [0.0, 0.0, 0.0], &BASIS);
        assert_eq!(e.pool.alive, 5);
        e.update(0.1, [0.0, 0.0, 0.0], &BASIS, [0.0, 0.0, 0.0]);
        // La emisión por tasa añade más, pero las 5 de la ráfaga
        // murieron: se comprueba que ninguna edad supera su vida.
        for i in 0..e.pool.alive {
            assert!(e.pool.age[i] <= e.pool.life[i]);
        }
    }

    #[test]
    fn el_tope_se_respeta() {
        let mut cfg = config_base();
        cfg.max_particles = 10;
        let mut e = Emitter::new(cfg);
        e.burst(50, [0.0, 0.0, 0.0], &BASIS);
        assert_eq!(e.pool.alive, 10);
    }

    #[test]
    fn la_gravedad_curva_la_trayectoria() {
        let mut cfg = config_base();
        cfg.rate = 0.0;
        cfg.gravity = [0.0, -9.8, 0.0];
        cfg.start_speed = [0.0, 0.0];
        let mut e = Emitter::new(cfg);
        e.burst(1, [0.0, 10.0, 0.0], &BASIS);
        e.update(0.5, [0.0, 10.0, 0.0], &BASIS, [0.0, 10.0, 0.0]);
        assert!(e.pool.vel[0][1] < -4.0);
        assert!(e.pool.pos[0][1] < 10.0);
    }

    #[test]
    fn misma_semilla_misma_emision() {
        let mut a = Emitter::new(config_base());
        let mut b = Emitter::new(config_base());
        a.burst(8, [1.0, 2.0, 3.0], &BASIS);
        b.burst(8, [1.0, 2.0, 3.0], &BASIS);
        assert_eq!(a.pool.pos, b.pool.pos);
        assert_eq!(a.pool.vel, b.pool.vel);
    }

    #[test]
    fn espacio_local_sigue_al_emisor() {
        let mut cfg = config_base();
        cfg.rate = 0.0;
        cfg.local_space = true;
        cfg.start_speed = [0.0, 0.0];
        let mut e = Emitter::new(cfg);
        e.burst(1, [0.0, 0.0, 0.0], &BASIS);
        e.update(0.016, [5.0, 0.0, 0.0], &BASIS, [0.0, 0.0, 0.0]);
        assert!((e.pool.pos[0][0] - 5.0).abs() < 1e-4);
    }

    #[test]
    fn comandos_se_drenan() {
        push_command(ParticleCommand::Play(7));
        push_command(ParticleCommand::Burst(7, 3));
        let cmds = drain_commands();
        assert_eq!(cmds.len(), 2);
        assert!(drain_commands().is_empty());
    }
}

pub use ffi::{glass_particles_burst, glass_particles_play, glass_particles_stop};
