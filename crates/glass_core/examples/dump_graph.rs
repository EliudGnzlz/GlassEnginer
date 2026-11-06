//! Compila UN grafo guardado (JSON) a GLSL: escribe `<nombre>.frag` y,
//! si el grafo declara etapa de vértices, `<nombre>.vert` en el directorio
//! destino. Sirve para regenerar los .spv de un solo grafo tras editarlo
//! sin tocar los demás (los binarios de los otros grafos siguen siendo
//! válidos aunque cambien las plantillas del compilador: el layout nuevo
//! declara un set que sus shaders no usan).
//! Uso: cargo run -p glass_core --example dump_graph -- <grafo.graph.json> <directorio>
use glass_core::shader_graph::ShaderGraph;
use std::path::PathBuf;

fn main() {
    let src = std::env::args().nth(1).expect("falta la ruta del grafo .graph.json");
    let dir = std::env::args().nth(2).expect("falta el directorio destino");
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();

    let text = std::fs::read_to_string(&src).expect("no se pudo leer el grafo");
    let graph = ShaderGraph::from_json(&text).expect("el JSON del grafo no parsea");
    let frag = graph.compile_fragment().expect("el fragmento debe compilar");
    std::fs::write(dir.join(format!("{}.frag", graph.name)), frag).unwrap();
    match graph.compile_vertex().expect("la etapa de vértices debe compilar") {
        Some(vert) => {
            std::fs::write(dir.join(format!("{}.vert", graph.name)), vert).unwrap();
            println!("{}: frag + vert ok", graph.name);
        }
        None => println!("{}: frag ok (sin etapa de vértices)", graph.name),
    }
}
