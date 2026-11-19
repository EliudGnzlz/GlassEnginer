//! Herramienta de comparación F1: convierte un `.fbx` por la vía nativa
//! e imprime los conteos y avisos como JSON por stdout.
//!
//! Uso: `cargo run -p glass_fbx --example convertir -- <entrada.fbx> <salida.glb>`

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("uso: convertir <entrada.fbx> <salida.glb>");
        std::process::exit(2);
    }
    let fuente = PathBuf::from(&args[1]);
    let destino = PathBuf::from(&args[2]);
    match glass_fbx::convertir_fbx(&fuente, &destino) {
        Ok(informe) => {
            println!(
                "{}",
                serde_json::json!({
                    "ok": true,
                    "counts": {
                        "meshes": informe.conteos.meshes,
                        "nodes": informe.conteos.nodes,
                        "materials": informe.conteos.materials,
                        "animations": informe.conteos.animations,
                        "skins": informe.conteos.skins,
                        "bytes": informe.conteos.bytes,
                    },
                    "avisos": informe.avisos,
                })
            );
        }
        Err(e) => {
            println!("{}", serde_json::json!({ "ok": false, "error": e.to_string() }));
            std::process::exit(1);
        }
    }
}
