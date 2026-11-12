//! Benchmark de `compute_world_matrices` (Fase 7): mide el coste de
//! resolver las matrices de mundo en tres escenarios sintéticos.
//! Uso: cargo run --release -p glass_core --example bench_transforms

use glass_core::SceneState;
use std::time::Instant;

fn poblar(scene: &mut SceneState, n: usize, modo: &str) {
    scene.entities.clear();
    let mut ids: Vec<u64> = Vec::new();
    let mut padre: Option<u64> = None;
    let mut en_cadena = 0;
    for creado in 0..n {
        let parent = match modo {
            "cadenas" => {
                let p = if en_cadena == 0 { None } else { padre };
                padre = None; // se fija tras crear
                p
            }
            "anidado" => {
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
        e.transform.position = [creado as f32 * 0.01, (creado % 7) as f32, 0.0];
        e.transform.set_rotation_euler([0.0, (creado % 13) as f32 * 3.0, 0.0]);
        e.transform.scale = [1.0, 1.0, 1.0];
        e.parent = parent;
        ids.push(id);
        if modo == "cadenas" {
            padre = Some(id);
            en_cadena = (en_cadena + 1) % 10;
        }
    }
}

fn medir(nombre: &str, n: usize, modo: &str, iteraciones: u32) {
    let mut scene = SceneState::new();
    poblar(&mut scene, n, modo);
    // Calentamiento
    let _ = scene.compute_world_matrices();
    let t0 = Instant::now();
    let mut suma = 0.0f32;
    for _ in 0..iteraciones {
        let m = scene.compute_world_matrices();
        if let Some((_, w)) = m.iter().next() {
            suma += w[12];
        }
    }
    let total = t0.elapsed();
    println!(
        "{nombre} (n={n}, {modo}): {:.3} ms/iteración ({} iteraciones, checksum {:.1})",
        total.as_secs_f64() * 1000.0 / iteraciones as f64,
        iteraciones,
        suma
    );
}

fn main() {
    medir("plano", 5000, "plano", 200);
    medir("cadenas", 5000, "cadenas", 200);
    medir("anidado", 2000, "anidado", 200);
    medir("escena", 600, "anidado", 1000);
}
