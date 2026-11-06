//! Vuelca los prefabricados del grafo de shaders como JSON editable y como
//! .frag compilado, para validarlos con glslangValidator y para copiarlos a
//! assets/shader_graphs/.
//! Uso: cargo run -p glass_core --example dump_shader_presets -- <directorio>
use glass_core::shader_graph::presets;
use std::path::PathBuf;

fn main() {
    let dir = std::env::args().nth(1).expect("falta el directorio destino");
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    for g in presets::all() {
        std::fs::write(dir.join(format!("{}.graph.json", g.name)), g.to_json()).unwrap();
        let glsl = g.compile_fragment().expect("el prefabricado debe compilar");
        std::fs::write(dir.join(format!("{}.frag", g.name)), glsl).unwrap();
        println!("{}: ok", g.name);
    }
}
