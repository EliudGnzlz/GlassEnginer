//! Benchmark del paso de simulación: AoS contra SoA con 50.000
//! partículas × 600 pasos. Fijó el layout del pool (SoA) en la
//! Fase 8P: SoA salió un 20–26 % más rápido con resultados
//! idénticos. `cargo run --release -p glass_particles --example bench_pool`.

use std::time::Instant;

const N: usize = 50_000;
const PASOS: usize = 600;
const DT: f32 = 1.0 / 60.0;

#[derive(Clone, Copy)]
struct P {
    pos: [f32; 3],
    vel: [f32; 3],
    age: f32,
    life: f32,
    size: f32,
}

fn bench_aos() -> f32 {
    let mut ps: Vec<P> = (0..N)
        .map(|i| P {
            pos: [i as f32 * 0.01, 0.0, 0.0],
            vel: [0.1, 2.0, 0.0],
            age: 0.0,
            life: 2.0 + (i % 10) as f32 * 0.1,
            size: 0.3,
        })
        .collect();
    let mut out: Vec<[f32; 4]> = vec![[0.0; 4]; N];
    let t = Instant::now();
    for _ in 0..PASOS {
        for p in ps.iter_mut() {
            p.vel[1] += -3.0 * DT;
            let d = 1.0 - 0.4 * DT;
            p.vel = [p.vel[0] * d, p.vel[1] * d, p.vel[2] * d];
            p.pos = [
                p.pos[0] + p.vel[0] * DT,
                p.pos[1] + p.vel[1] * DT,
                p.pos[2] + p.vel[2] * DT,
            ];
            p.age += DT;
            if p.age >= p.life {
                p.age = 0.0;
                p.pos[1] = 0.0;
                p.vel[1] = 2.0;
            }
        }
        for (i, p) in ps.iter().enumerate() {
            let k = p.age / p.life;
            out[i] = [p.pos[0], p.pos[1], p.size * (1.0 - k), 1.0 - k];
        }
    }
    let suma: f32 = out.iter().map(|o| o[0] + o[3]).sum();
    println!("AoS: {:?} (suma {suma:.1})", t.elapsed());
    suma
}

fn bench_soa() -> f32 {
    let mut pos: Vec<[f32; 3]> = (0..N).map(|i| [i as f32 * 0.01, 0.0, 0.0]).collect();
    let mut vel: Vec<[f32; 3]> = vec![[0.1, 2.0, 0.0]; N];
    let mut age: Vec<f32> = vec![0.0; N];
    let life: Vec<f32> = (0..N).map(|i| 2.0 + (i % 10) as f32 * 0.1).collect();
    let mut out: Vec<[f32; 4]> = vec![[0.0; 4]; N];
    let t = Instant::now();
    for _ in 0..PASOS {
        for i in 0..N {
            vel[i][1] += -3.0 * DT;
            let d = 1.0 - 0.4 * DT;
            vel[i] = [vel[i][0] * d, vel[i][1] * d, vel[i][2] * d];
            pos[i] = [
                pos[i][0] + vel[i][0] * DT,
                pos[i][1] + vel[i][1] * DT,
                pos[i][2] + vel[i][2] * DT,
            ];
            age[i] += DT;
            if age[i] >= life[i] {
                age[i] = 0.0;
                pos[i][1] = 0.0;
                vel[i][1] = 2.0;
            }
        }
        for i in 0..N {
            let k = age[i] / life[i];
            out[i] = [pos[i][0], pos[i][1], 0.3 * (1.0 - k), 1.0 - k];
        }
    }
    let suma: f32 = out.iter().map(|o| o[0] + o[3]).sum();
    println!("SoA: {:?} (suma {suma:.1})", t.elapsed());
    suma
}

fn main() {
    bench_aos();
    bench_soa();
    bench_aos();
    bench_soa();
}
