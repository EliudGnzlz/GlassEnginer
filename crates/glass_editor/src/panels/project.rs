//! Panel «Proyecto» del editor (equivalente al Project window de Unity).
//!
//! Antes era una pestaña plana y de solo lectura dentro de `ConsolePanel`
//! (`render_project_only`). Ahora es un panel propio con:
//!   * navegación encapsulada en `assets/` (nunca se sale de la raíz),
//!   * creación de assets (carpetas, scripts C#, grafos, escenas),
//!   * renombrado inline, movimiento entre carpetas y papelera segura,
//!   * cola de trabajo (`pending_scene_load` / `pending_prefab_instantiate`)
//!     que consume `EditorApp`; este panel **no** carga nada por su cuenta.
//!
//! Toda la lógica de disco vive en funciones libres para poder testearla
//! sin egui y sin tocar el editor.

use egui::Ui;
use std::path::{Path, PathBuf};

/// Plantilla mínima de escena (misma estructura que `MixamoTest`): una lista
/// raíz de entidades serializadas.
pub const PLANTILLA_ESCENA: &str = r#"[{"active":true,"animator":null,"audio":null,"camera":{"far_clip":1000.0,"fov":60.0,"is_main":true,"near_clip":0.1},"canvas":null,"children":[],"components":["Transform","Camera"],"id":1,"is_static":false,"layer":"Default","light":null,"mesh":null,"name":"Main Camera","parent":null,"physics":null,"rect_transform":null,"script":null,"script_data":null,"tag":"MainCamera","transform":{"position":[0.0,1.6,5.0],"rotation":[-6.0,0.0,0.0],"scale":[1.0,1.0,1.0]},"ui_button":null,"ui_text":null}]"#;

/// Entrada cacheada de un directorio: (nombre, es_carpeta, bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entrada {
    pub nombre: String,
    pub es_dir: bool,
    pub bytes: u64,
}

// ---------------------------------------------------------------------------
// Lógica de disco (testeable, sin egui)
// ---------------------------------------------------------------------------

/// Compara dos rutas con tolerancia (canonicaliza si ambas existen).
pub fn rutas_iguales(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// ¿`ancestro` contiene a `otra` (o es la misma)? Comprueba también con la
/// ruta canónica para no caer en `..` o enlaces.
fn es_ancestro_o_igual(ancestro: &Path, otra: &Path) -> bool {
    if otra.starts_with(ancestro) {
        return true;
    }
    match (std::fs::canonicalize(ancestro), std::fs::canonicalize(otra)) {
        (Ok(a), Ok(b)) => b.starts_with(a),
        _ => false,
    }
}

/// Valida un nombre de asset: no vacío, sin separadores y sin punto inicial.
pub fn validar_nombre(nombre: &str) -> Result<(), String> {
    let n = nombre.trim();
    if n.is_empty() {
        return Err("El nombre no puede estar vacío".to_string());
    }
    if n.contains('/') || n.contains('\\') {
        return Err("El nombre no puede contener '/' ni '\\'".to_string());
    }
    if n.starts_with('.') {
        return Err("El nombre no puede empezar por '.'".to_string());
    }
    Ok(())
}

/// Quita una extensión/sufijo conocido (`.cs`, `.graph.json`, ...).
fn nombre_sin_sufijo(nombre: &str, sufijos: &[&str]) -> String {
    for s in sufijos {
        if let Some(base) = nombre.strip_suffix(s) {
            return base.to_string();
        }
    }
    nombre.to_string()
}

/// Lista un directorio: carpetas primero y luego alfabético (ignora ocultos).
pub fn listar_directorio(dir: &Path) -> Result<Vec<Entrada>, String> {
    let rd = std::fs::read_dir(dir)
        .map_err(|e| format!("No se pudo leer {}: {e}", dir.display()))?;

    let mut entradas = Vec::new();
    for item in rd.flatten() {
        let nombre = item.file_name().to_string_lossy().to_string();
        if nombre.starts_with('.') {
            continue;
        }
        let es_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let bytes = item.metadata().map(|m| m.len()).unwrap_or(0);
        entradas.push(Entrada { nombre, es_dir, bytes });
    }
    entradas.sort_by(|a, b| {
        b.es_dir
            .cmp(&a.es_dir)
            .then_with(|| a.nombre.to_lowercase().cmp(&b.nombre.to_lowercase()))
    });
    Ok(entradas)
}

/// Crea una carpeta dentro de `dir`.
pub fn crear_carpeta(dir: &Path, nombre: &str) -> Result<PathBuf, String> {
    let nombre = nombre.trim();
    validar_nombre(nombre)?;
    let destino = dir.join(nombre);
    if destino.exists() {
        return Err(format!("Ya existe {}", destino.display()));
    }
    std::fs::create_dir_all(&destino)
        .map_err(|e| format!("No se pudo crear {}: {e}", destino.display()))?;
    Ok(destino)
}

/// Plantilla de script C# del runtime.
pub fn plantilla_script(nombre_clase: &str) -> String {
    format!(
        "using UnityEngine;\n\npublic class {nombre_clase} : MonoBehaviour\n{{\n    void Start() {{ }}\n    void Update() {{ }}\n}}"
    )
}

/// Crea `managed/GlassEngine.Core/Scripts/<Nombre>.cs`.
///
/// `managed/` es **hermana** de `assets/`: cuelga de la raíz del proyecto, no
/// del directorio que se esté navegando en el panel.
pub fn crear_script_cs(raiz_proyecto: &Path, nombre: &str) -> Result<PathBuf, String> {
    let base = nombre_sin_sufijo(nombre.trim(), &[".cs"]);
    validar_nombre(&base)?;
    validar_identificador_cs(&base)?;

    let dir = raiz_proyecto
        .join("managed")
        .join("GlassEngine.Core")
        .join("Scripts");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;

    let destino = dir.join(format!("{base}.cs"));
    if destino.exists() {
        return Err(format!("Ya existe {}", destino.display()));
    }
    std::fs::write(&destino, plantilla_script(&base))
        .map_err(|e| format!("No se pudo escribir {}: {e}", destino.display()))?;
    Ok(destino)
}

/// El nombre de un script debe ser un identificador C# válido: empieza
/// por letra o `_`, sigue con letras/dígitos/`_` y no es palabra
/// reservada (si lo fuera, el archivo creado no compilaría).
pub fn validar_identificador_cs(nombre: &str) -> Result<(), String> {
    const RESERVADAS: &[&str] = &[
        "class", "namespace", "public", "private", "static", "void", "int",
        "string", "bool", "float", "double", "new", "using", "return",
        "if", "else", "for", "foreach", "while", "var", "this", "base",
        "event", "operator", "params", "ref", "out", "in", "is", "as",
    ];
    let mut chars = nombre.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return Err(format!("'{nombre}' no es un identificador C# válido")),
    }
    if !nombre.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(format!("'{nombre}' no es un identificador C# válido"));
    }
    if RESERVADAS.contains(&nombre) {
        return Err(format!("'{nombre}' es una palabra reservada de C#"));
    }
    Ok(())
}

/// Primer archivo (ordenado) de `dir` cuyo nombre termina en `sufijo`.
fn primer_archivo_con_sufijo(dir: &Path, sufijo: &str) -> Result<Option<PathBuf>, String> {
    let rd = std::fs::read_dir(dir)
        .map_err(|e| format!("No se pudo leer {}: {e}", dir.display()))?;
    let mut candidatos: Vec<PathBuf> = Vec::new();
    for item in rd.flatten() {
        let nombre = item.file_name().to_string_lossy().to_string();
        if nombre.ends_with(sufijo) && item.file_type().map(|t| t.is_file()).unwrap_or(false) {
            candidatos.push(item.path());
        }
    }
    candidatos.sort();
    Ok(candidatos.into_iter().next())
}

/// Crea un grafo copiando el primer `.graph.json` de `assets/shader_graphs/`.
pub fn crear_grafo(assets_root: &Path, nombre: &str) -> Result<PathBuf, String> {
    let base = nombre_sin_sufijo(nombre.trim(), &[".graph.json", ".json"]);
    validar_nombre(&base)?;

    let dir = subcarpeta(assets_root, "shader_graphs", "ShaderGraphs");
    if !dir.is_dir() {
        return Err(format!("No existe la carpeta {}", dir.display()));
    }
    let origen = primer_archivo_con_sufijo(&dir, ".graph.json")?
        .ok_or_else(|| format!("No hay ningún .graph.json en {}", dir.display()))?;

    let destino = dir.join(format!("{base}.graph.json"));
    if destino.exists() {
        return Err(format!("Ya existe {}", destino.display()));
    }
    std::fs::copy(&origen, &destino).map_err(|e| {
        format!(
            "No se pudo copiar {} → {}: {e}",
            origen.display(),
            destino.display()
        )
    })?;
    Ok(destino)
}

/// Subcarpeta de assets respetando la convención del proyecto abierto:
/// si existe la variante con mayúscula inicial (`Scenes`), se usa esa;
/// si no, la de minúsculas (`scenes`).
pub fn subcarpeta(assets_root: &Path, minuscula: &str, mayuscula: &str) -> PathBuf {
    let capitalizada = assets_root.join(mayuscula);
    if capitalizada.is_dir() {
        capitalizada
    } else {
        assets_root.join(minuscula)
    }
}

/// Crea `<assets>/scenes/<Nombre>.scene.json` (o `Scenes/` si el proyecto
/// usa mayúsculas) con la plantilla mínima.
pub fn crear_escena(assets_root: &Path, nombre: &str) -> Result<PathBuf, String> {
    crear_escena_en(&subcarpeta(assets_root, "scenes", "Scenes"), nombre)
}

/// Crea `<dir>/<Nombre>.scene.json` con la plantilla mínima.
pub fn crear_escena_en(dir: &Path, nombre: &str) -> Result<PathBuf, String> {
    let base = nombre_sin_sufijo(nombre.trim(), &[".scene.json", ".json"]);
    validar_nombre(&base)?;

    std::fs::create_dir_all(dir)
        .map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;

    let destino = dir.join(format!("{base}.scene.json"));
    if destino.exists() {
        return Err(format!("Ya existe {}", destino.display()));
    }
    std::fs::write(&destino, PLANTILLA_ESCENA)
        .map_err(|e| format!("No se pudo escribir {}: {e}", destino.display()))?;
    Ok(destino)
}

/// Crea `<assets>/materials/<Nombre>.material.json` (o `Materials/` si el
/// proyecto usa mayúsculas) con un `MaterialData` por defecto.
pub fn crear_material(assets_root: &Path, nombre: &str) -> Result<PathBuf, String> {
    let base = nombre_sin_sufijo(nombre.trim(), &[".material.json", ".json"]);
    validar_nombre(&base)?;

    let dir = subcarpeta(assets_root, "materials", "Materials");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;

    let destino = dir.join(format!("{base}.material.json"));
    if destino.exists() {
        return Err(format!("Ya existe {}", destino.display()));
    }
    let json = serde_json::to_string_pretty(&glass_core::MaterialData::default())
        .map_err(|e| format!("No se pudo serializar el material: {e}"))?;
    std::fs::write(&destino, json)
        .map_err(|e| format!("No se pudo escribir {}: {e}", destino.display()))?;
    Ok(destino)
}

/// Renombra un elemento dentro de su misma carpeta.
pub fn renombrar(ruta: &Path, nuevo_nombre: &str) -> Result<PathBuf, String> {
    let nuevo = nuevo_nombre.trim();
    validar_nombre(nuevo)?;
    if !ruta.exists() {
        return Err(format!("No existe {}", ruta.display()));
    }
    let padre = ruta
        .parent()
        .ok_or_else(|| format!("{} no tiene carpeta padre", ruta.display()))?;
    let destino = padre.join(nuevo);

    if rutas_iguales(ruta, &destino) {
        return Ok(destino); // mismo nombre: no-op
    }
    if destino.exists() {
        return Err(format!("Ya existe {}", destino.display()));
    }
    std::fs::rename(ruta, &destino)
        .map_err(|e| format!("No se pudo renombrar {}: {e}", ruta.display()))?;
    Ok(destino)
}

/// Mueve un elemento a `destino_dir` (dentro de `assets_root`).
pub fn mover_elemento(
    ruta: &Path,
    destino_dir: &Path,
    assets_root: &Path,
) -> Result<PathBuf, String> {
    if !ruta.exists() {
        return Err(format!("No existe {}", ruta.display()));
    }
    if !destino_dir.is_dir() {
        return Err(format!("{} no es una carpeta", destino_dir.display()));
    }
    // La raíz de assets/ no se toca nunca.
    if rutas_iguales(ruta, assets_root) {
        return Err("No se puede mover la raíz de assets/".to_string());
    }
    // Contención: origen y destino deben vivir dentro de assets/ (la
    // función es pública y no puede fiarse de que la UI ya filtró).
    let raiz_canon = std::fs::canonicalize(assets_root)
        .map_err(|e| format!("No se pudo resolver {}: {e}", assets_root.display()))?;
    let origen_canon = std::fs::canonicalize(ruta)
        .map_err(|e| format!("No se pudo resolver {}: {e}", ruta.display()))?;
    let destino_canon = std::fs::canonicalize(destino_dir)
        .map_err(|e| format!("No se pudo resolver {}: {e}", destino_dir.display()))?;
    if !origen_canon.starts_with(&raiz_canon) || !destino_canon.starts_with(&raiz_canon) {
        return Err("Origen y destino deben estar dentro de assets/".to_string());
    }
    // Una carpeta no puede acabar dentro de sí misma.
    if ruta.is_dir() && es_ancestro_o_igual(ruta, destino_dir) {
        return Err("No se puede mover una carpeta dentro de sí misma".to_string());
    }

    let nombre = ruta
        .file_name()
        .ok_or_else(|| format!("{} no tiene nombre de archivo", ruta.display()))?;
    let destino = destino_dir.join(nombre);

    if rutas_iguales(ruta, &destino) {
        return Err("El elemento ya está en esa carpeta".to_string());
    }
    if destino.exists() {
        return Err(format!("Ya existe {}", destino.display()));
    }
    std::fs::rename(ruta, &destino)
        .map_err(|e| format!("No se pudo mover {}: {e}", ruta.display()))?;
    Ok(destino)
}

/// Separa tallo y extensión respetando extensiones compuestas de asset.
fn dividir_nombre(nombre: &str) -> (String, String) {
    for comp in [".scene.json", ".prefab.json", ".graph.json"] {
        if let Some(base) = nombre.strip_suffix(comp) {
            return (base.to_string(), comp.trim_start_matches('.').to_string());
        }
    }
    match nombre.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() => (base.to_string(), ext.to_string()),
        _ => (nombre.to_string(), String::new()),
    }
}

/// Primera ruta libre dentro de `dir` con el nombre dado (` (2)`, ` (3)`...).
///
/// Nunca devuelve una ruta que ya exista: esa garantía es la que hace
/// segura la papelera (un `rename` sobre una ruta existente la
/// reemplazaría en Unix). Si se agotan los sufijos razonables, error.
pub fn ruta_libre_en(dir: &Path, nombre: &str) -> Result<PathBuf, String> {
    let candidato = dir.join(nombre);
    if !candidato.exists() {
        return Ok(candidato);
    }
    let (tallo, ext) = dividir_nombre(nombre);
    let mut n = 2u32;
    loop {
        let nuevo = if ext.is_empty() {
            format!("{tallo} ({n})")
        } else {
            format!("{tallo} ({n}).{ext}")
        };
        let candidato = dir.join(nuevo);
        if !candidato.exists() {
            return Ok(candidato);
        }
        n += 1;
        if n > 10_000 {
            // Salvaguarda: nunca colgar el editor en un bucle infinito,
            // y jamás devolver una ruta existente (ver la doc de arriba).
            return Err(format!(
                "No hay nombre libre para '{nombre}' en {} tras 10.000 intentos",
                dir.display()
            ));
        }
    }
}

/// Copia un archivo soltado desde el SO a `dir_destino`, con
/// nombre libre si hay colisión (vía `ruta_libre_en`). Devuelve la
/// ruta creada. Los directorios se rechazan.
pub fn importar_archivo(origen: &Path, dir_destino: &Path) -> Result<PathBuf, String> {
    if !origen.exists() {
        return Err(format!("No existe {}", origen.display()));
    }
    if origen.is_dir() {
        return Err(format!(
            "{} es una carpeta; la importación por arrastre acepta archivos",
            origen.display()
        ));
    }
    let nombre = origen
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| format!("El origen no tiene nombre de archivo: {}", origen.display()))?;
    let destino = ruta_libre_en(dir_destino, &nombre)?;
    std::fs::copy(origen, &destino)
        .map_err(|e| format!("No se pudo importar {}: {e}", origen.display()))?;
    Ok(destino)
}

/// Mueve un elemento a `.project_trash/` (hermana de `assets/`). Nada se borra.
pub fn mover_a_papelera(raiz_proyecto: &Path, ruta: &Path) -> Result<PathBuf, String> {
    if !ruta.exists() {
        return Err(format!("No existe {}", ruta.display()));
    }
    if rutas_iguales(ruta, raiz_proyecto) {
        return Err("No se puede eliminar la raíz del proyecto".to_string());
    }
    // Solo se manda a la papelera lo que vive dentro de assets/ (la
    // función es pública; la UI ya filtra, pero la garantía va aquí).
    let assets_canon = std::fs::canonicalize(raiz_proyecto.join("assets"))
        .map_err(|e| format!("No se pudo resolver assets/: {e}"))?;
    let ruta_canon = std::fs::canonicalize(ruta)
        .map_err(|e| format!("No se pudo resolver {}: {e}", ruta.display()))?;
    if ruta_canon == assets_canon || !ruta_canon.starts_with(&assets_canon) {
        return Err("Solo se puede eliminar contenido dentro de assets/".to_string());
    }

    let papelera = raiz_proyecto.join(".project_trash");
    std::fs::create_dir_all(&papelera)
        .map_err(|e| format!("No se pudo crear {}: {e}", papelera.display()))?;

    let nombre = ruta
        .file_name()
        .ok_or_else(|| format!("{} no tiene nombre de archivo", ruta.display()))?
        .to_string_lossy()
        .to_string();

    let destino = ruta_libre_en(&papelera, &nombre)?;
    std::fs::rename(ruta, &destino)
        .map_err(|e| format!("No se pudo mover a la papelera {}: {e}", ruta.display()))?;
    Ok(destino)
}

/// Todas las carpetas bajo `assets_root` (incluida ella misma), en orden.
pub fn listar_carpetas(assets_root: &Path) -> Vec<PathBuf> {
    let mut carpetas = Vec::new();
    if assets_root.is_dir() {
        carpetas.push(assets_root.to_path_buf());
    }
    recorrer_carpetas(assets_root, &mut carpetas, 0);
    carpetas
}

fn recorrer_carpetas(dir: &Path, out: &mut Vec<PathBuf>, profundidad: usize) {
    if profundidad > 32 {
        return; // cortafuegos ante árboles absurdos o enlaces
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut hijos: Vec<PathBuf> = Vec::new();
    for item in rd.flatten() {
        let nombre = item.file_name().to_string_lossy().to_string();
        if nombre.starts_with('.') {
            continue;
        }
        if item.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            hijos.push(item.path());
        }
    }
    hijos.sort();
    for hijo in hijos {
        out.push(hijo.clone());
        recorrer_carpetas(&hijo, out, profundidad + 1);
    }
}

/// Ruta relativa a `assets_root` en formato legible (`assets/scenes`).
pub fn ruta_legible(assets_root: &Path, ruta: &Path) -> String {
    let rel = ruta.strip_prefix(assets_root).unwrap_or(ruta);
    let s = rel.to_string_lossy().replace('\\', "/");
    if s.is_empty() {
        "assets".to_string()
    } else {
        format!("assets/{s}")
    }
}

pub fn es_escena(nombre: &str) -> bool {
    nombre.ends_with(".scene.json")
}

pub fn es_prefab(nombre: &str) -> bool {
    nombre.ends_with(".prefab.json")
}

pub fn es_modelo(nombre: &str) -> bool {
    let ext = nombre.rsplit('.').next().unwrap_or("").to_lowercase();
    matches!(ext.as_str(), "glb" | "gltf" | "blend" | "fbx")
}

pub fn es_instanciable(nombre: &str, es_dir: bool) -> bool {
    !es_dir && (es_prefab(nombre) || es_modelo(nombre))
}

/// Carga útil del arrastre de un asset del panel Proyecto hacia la escena o la jerarquía.
#[derive(Debug, Clone)]
pub struct ArrastreAsset {
    pub ruta: std::path::PathBuf,
}

/// Instancia un modelo o prefab en `posicion` y devuelve el id de la entidad raíz.
///
/// Para .blend/.fbx el modelo es el stem del archivo porque el motor los convierte
/// a un .glb hermano (misma carpeta) con ese mismo nombre.
pub fn instanciar_asset(
    state: &mut glass_core::SceneState,
    ruta: &std::path::Path,
    posicion: [f32; 3],
) -> Result<u64, String> {
    let nombre = ruta.file_name().and_then(|n| n.to_str()).unwrap_or("");

    if es_prefab(nombre) {
        let prefab = glass_core::Prefab::load_from_file(ruta).map_err(|e| e.to_string())?;
        return Ok(state.instantiate_prefab(&prefab, None, Some(posicion)));
    }

    if es_modelo(nombre) {
        let stem = ruta
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if stem.is_empty() {
            return Err(format!(
                "El modelo '{}' no tiene un nombre válido",
                ruta.display()
            ));
        }
        let id = state.create_entity(
            stem.clone(),
            vec!["Transform".to_string(), "MeshRenderer".to_string()],
        );
        if let Some(entidad) = state.get_entity_mut(id) {
            entidad.transform.position = posicion;
            if let Some(mesh) = entidad.mesh.as_mut() {
                mesh.model = stem;
                mesh.shader = "pbr.vert.spv".to_string();
                mesh.color = [1.0, 1.0, 1.0];
            }
        }
        return Ok(id);
    }

    Err(format!("El asset '{nombre}' no se puede instanciar en la escena"))
}

/// Icono por extensión (mismo criterio que el explorador clásico).
pub fn icono_para(nombre: &str, es_dir: bool) -> &'static str {
    if es_dir {
        return "📁";
    }
    if es_escena(nombre) {
        return "🎬";
    }
    if es_prefab(nombre) {
        return "🧩";
    }
    if nombre.ends_with(".graph.json") {
        return "🎨";
    }
    if nombre.ends_with(".material.json") {
        return "🖌";
    }
    let ext = nombre
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "cs" => "📜",
        "glb" | "gltf" | "fbx" | "blend" => "🗿",
        "png" | "jpg" | "jpeg" => "🖼",
        "wav" | "ogg" | "mp3" => "🔊",
        _ => "📄",
    }
}

// ---------------------------------------------------------------------------
// Panel
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TipoCreacion {
    Carpeta,
    Script,
    Grafo,
    Escena,
    Material,
}

impl TipoCreacion {
    /// Nombre usado al crear desde un menú contextual con el campo
    /// «Nombre:» vacío.
    fn nombre_por_defecto(self) -> &'static str {
        match self {
            TipoCreacion::Carpeta => "NuevaCarpeta",
            TipoCreacion::Script => "NuevoScript",
            TipoCreacion::Grafo => "NuevoGrafo",
            TipoCreacion::Escena => "NuevaEscena",
            TipoCreacion::Material => "NuevoMaterial",
        }
    }
}

/// Submenú «➕ Crear» compartido por los menús contextuales del panel.
/// Devuelve el tipo pulsado (si alguno) y cierra el menú.
fn submenu_crear(ui: &mut egui::Ui) -> Option<TipoCreacion> {
    ui.menu_button("➕ Crear", |ui| {
        let mut elegido = None;
        for (tipo, etiqueta) in [
            (TipoCreacion::Carpeta, "📁 Carpeta"),
            (TipoCreacion::Escena, "🎬 Escena"),
            (TipoCreacion::Material, "🖌 Material"),
            (TipoCreacion::Grafo, "🎨 Grafo de shaders"),
            (TipoCreacion::Script, "📜 Script C#"),
        ] {
            if ui.button(etiqueta).clicked() {
                elegido = Some(tipo);
                ui.close();
            }
        }
        elegido
    })
    .inner
    .flatten()
}

/// Panel «Proyecto / Assets».
pub struct ProjectPanel {
    /// Raíz navegable: `assets`. Nunca se sale de aquí.
    pub assets_root: PathBuf,
    /// Directorio visible actualmente (siempre dentro de `assets_root`).
    pub current_dir: PathBuf,
    /// Entradas cacheadas del directorio actual.
    pub entradas: Vec<Entrada>,
    /// Elemento seleccionado.
    pub seleccion: Option<PathBuf>,
    /// Buffer del campo «Nombre» de la barra de creación.
    pub nuevo_nombre: String,
    /// Elemento en edición inline y su buffer.
    pub rename_target: Option<PathBuf>,
    pub rename_buffer: String,
    rename_focus: bool,
    /// Última línea de estado: (mensaje, es_error).
    pub status: Option<(String, bool)>,
    /// Cola consumida por `EditorApp`: cargar escena.
    pub pending_scene_load: Option<PathBuf>,
    /// Cola consumida por `EditorApp`: instanciar prefab.
    pub pending_prefab_instantiate: Option<PathBuf>,
    /// Asset cuyo arrastre hacia la escena o la jerarquía está en curso.
    pub arrastre_asset: Option<PathBuf>,
}

impl Default for ProjectPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectPanel {
    pub fn new() -> Self {
        let assets_root = PathBuf::from("assets");
        let mut panel = Self {
            current_dir: assets_root.clone(),
            assets_root,
            entradas: Vec::new(),
            seleccion: None,
            nuevo_nombre: String::new(),
            rename_target: None,
            rename_buffer: String::new(),
            rename_focus: false,
            status: None,
            pending_scene_load: None,
            pending_prefab_instantiate: None,
            arrastre_asset: None,
        };
        panel.refrescar();
        panel
    }

    /// Re-enraíza el panel a la carpeta de assets del proyecto abierto
    /// (`<proyecto>/Assets` o `<proyecto>/assets`). Crea la carpeta si
    /// no existe, vuelve a la raíz y refresca la lista.
    pub fn set_assets_root(&mut self, raiz: PathBuf) {
        let _ = std::fs::create_dir_all(&raiz);
        self.current_dir = raiz.clone();
        self.assets_root = raiz;
        self.seleccion = None;
        self.rename_target = None;
        self.arrastre_asset = None;
        self.refrescar();
    }

    // --- utilidades internas -------------------------------------------------

    fn set_info(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), false));
    }

    fn set_error(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), true));
    }

    /// Raíz del proyecto: carpeta hermana de `assets/`.
    fn raiz_proyecto(&self) -> PathBuf {
        match self.assets_root.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        }
    }

    fn en_raiz(&self) -> bool {
        rutas_iguales(&self.current_dir, &self.assets_root)
    }

    fn ruta_actual_legible(&self) -> String {
        ruta_legible(&self.assets_root, &self.current_dir)
    }

    /// Relee el directorio actual.
    fn refrescar(&mut self) {
        match listar_directorio(&self.current_dir) {
            Ok(entradas) => self.entradas = entradas,
            Err(e) => {
                self.entradas.clear();
                self.set_error(e);
            }
        }
    }

    /// Importa los archivos soltados desde el SO al directorio
    /// visible: copia cada uno con nombre libre si colisiona, sin
    /// que un error detenga el resto; anota el resultado en la
    /// línea de estado y refresca la lista si entró algo.
    pub fn importar_soltados(&mut self, rutas: &[PathBuf]) {
        let mut importados: Vec<PathBuf> = Vec::new();
        let mut errores: Vec<String> = Vec::new();
        for ruta in rutas {
            match importar_archivo(ruta, &self.current_dir) {
                Ok(destino) => importados.push(destino),
                Err(e) => errores.push(e),
            }
        }
        if !importados.is_empty() {
            self.refrescar();
            self.seleccion = importados.last().cloned();
        }
        if let Some(primero) = errores.into_iter().next() {
            self.set_error(primero);
        } else if importados.len() == 1 {
            self.set_info(format!(
                "Importado: {}",
                ruta_legible(&self.assets_root, &importados[0])
            ));
        } else if importados.len() > 1 {
            self.set_info(format!(
                "Importados {} archivos en {}",
                importados.len(),
                self.ruta_actual_legible()
            ));
        }
    }

    /// Entra en una carpeta, sin poder escapar de `assets/`.
    fn entrar_en(&mut self, dir: PathBuf) {
        if !dir.starts_with(&self.assets_root) {
            self.set_error(format!("Ruta fuera de assets/: {}", dir.display()));
            return;
        }
        self.current_dir = dir;
        self.seleccion = None;
        self.rename_target = None;
        self.rename_focus = false;
        self.refrescar();
    }

    /// Sube un nivel (deshabilitado en la raíz).
    fn subir(&mut self) {
        if self.en_raiz() {
            return;
        }
        let destino = match self.current_dir.parent() {
            Some(p) if p.starts_with(&self.assets_root) => p.to_path_buf(),
            _ => self.assets_root.clone(),
        };
        self.current_dir = destino;
        self.seleccion = None;
        self.rename_target = None;
        self.refrescar();
    }

    fn iniciar_renombrado(&mut self, ruta: PathBuf) {
        if rutas_iguales(&ruta, &self.assets_root) {
            self.set_error("No se puede renombrar la raíz de assets/");
            return;
        }
        let nombre = ruta
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        self.rename_buffer = nombre;
        self.rename_target = Some(ruta);
        self.rename_focus = true;
    }

    fn confirmar_renombrado(&mut self) {
        let Some(origen) = self.rename_target.clone() else {
            return;
        };
        if rutas_iguales(&origen, &self.assets_root) {
            self.rename_target = None;
            self.set_error("No se puede renombrar la raíz de assets/");
            return;
        }
        let nuevo = self.rename_buffer.trim().to_string();
        match renombrar(&origen, &nuevo) {
            Ok(destino) => {
                self.rename_target = None;
                self.rename_focus = false;
                self.seleccion = Some(destino.clone());
                self.refrescar();
                self.set_info(format!("Renombrado → {}", destino.display()));
            }
            // Se mantiene la edición abierta para corregir el nombre.
            Err(e) => self.set_error(e),
        }
    }

    fn eliminar(&mut self, ruta: PathBuf) {
        if rutas_iguales(&ruta, &self.assets_root) {
            self.set_error("No se puede eliminar la raíz de assets/");
            return;
        }
        let raiz = self.raiz_proyecto();
        match mover_a_papelera(&raiz, &ruta) {
            Ok(destino) => {
                self.seleccion = None;
                self.refrescar();
                self.set_info(format!("Movido a la papelera: {}", destino.display()));
            }
            Err(e) => self.set_error(e),
        }
    }

    fn ejecutar_movimiento(&mut self, item: PathBuf, destino_dir: PathBuf) {
        match mover_elemento(&item, &destino_dir, &self.assets_root) {
            Ok(nuevo) => {
                self.seleccion = Some(nuevo.clone());
                self.refrescar();
                self.set_info(format!("Movido a {}", nuevo.display()));
            }
            Err(e) => self.set_error(e),
        }
    }

    fn ejecutar_creacion(&mut self, tipo: TipoCreacion) {
        let nombre = self.nuevo_nombre.trim().to_string();
        if nombre.is_empty() {
            self.set_error("Escribe un nombre antes de crear el asset");
            return;
        }
        if self.ejecutar_creacion_con(tipo, nombre, None) {
            self.nuevo_nombre.clear();
        }
    }

    /// Crea el asset `tipo` con `nombre` (ya validado como no vacío).
    /// `destino` solo se aplica a carpetas y escenas: materiales, grafos y
    /// scripts viven en su carpeta canónica. Selecciona el resultado,
    /// refresca y deja el mensaje de estado. Devuelve `true` si se creó.
    fn ejecutar_creacion_con(
        &mut self,
        tipo: TipoCreacion,
        nombre: String,
        destino: Option<PathBuf>,
    ) -> bool {
        let raiz_proyecto = self.raiz_proyecto();
        let resultado = match tipo {
            TipoCreacion::Carpeta => crear_carpeta(
                destino.as_deref().unwrap_or(self.current_dir.as_path()),
                &nombre,
            ),
            TipoCreacion::Script => crear_script_cs(&raiz_proyecto, &nombre),
            TipoCreacion::Grafo => crear_grafo(&self.assets_root, &nombre),
            TipoCreacion::Escena => match destino {
                Some(dir) => crear_escena_en(&dir, &nombre),
                None => crear_escena(&self.assets_root, &nombre),
            },
            TipoCreacion::Material => crear_material(&self.assets_root, &nombre),
        };
        match resultado {
            Ok(ruta) => {
                self.seleccion = Some(ruta.clone());
                self.refrescar();
                self.set_info(format!("Creado: {}", ruta.display()));
                true
            }
            Err(e) => {
                self.set_error(e);
                false
            }
        }
    }

    /// Nombre para una creación desde menú contextual: el del campo
    /// «Nombre:» si hay algo escrito; si no, el nombre por defecto del tipo.
    fn nombre_para_creacion(&self, tipo: TipoCreacion) -> String {
        let escrito = self.nuevo_nombre.trim();
        if escrito.is_empty() {
            tipo.nombre_por_defecto().to_string()
        } else {
            escrito.to_string()
        }
    }

    // --- dibujo --------------------------------------------------------------

    pub fn render(&mut self, ui: &mut Ui) {
        // --- Barra superior ---------------------------------------------------
        let en_raiz = self.en_raiz();
        let mut pedir_refresco = false;
        let mut pedir_subir = false;
        ui.horizontal(|ui| {
            ui.heading("📁 Proyecto / Assets");
            ui.separator();
            ui.label(format!("Ruta: {}", self.ruta_actual_legible()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🔄 Refrescar").clicked() {
                    pedir_refresco = true;
                }
                if ui
                    .add_enabled(!en_raiz, egui::Button::new("⬆ Subir"))
                    .clicked()
                {
                    pedir_subir = true;
                }
            });
        });
        if pedir_refresco {
            self.refrescar();
            self.set_info("Proyecto refrescado");
        }
        if pedir_subir {
            self.subir();
        }

        // --- Barra de creación ------------------------------------------------
        let mut accion_crear: Option<TipoCreacion> = None;
        ui.horizontal(|ui| {
            ui.label("Nombre:");
            ui.add(
                egui::TextEdit::singleline(&mut self.nuevo_nombre)
                    .desired_width(180.0)
                    .hint_text("Nuevo asset…"),
            );
            if ui.button("📁 Carpeta").clicked() {
                accion_crear = Some(TipoCreacion::Carpeta);
            }
            if ui.button("📜 Script C#").clicked() {
                accion_crear = Some(TipoCreacion::Script);
            }
            if ui.button("🎨 Grafo").clicked() {
                accion_crear = Some(TipoCreacion::Grafo);
            }
            if ui.button("🎬 Escena").clicked() {
                accion_crear = Some(TipoCreacion::Escena);
            }
            if ui.button("🖌 Material").clicked() {
                accion_crear = Some(TipoCreacion::Material);
            }
        });
        if let Some(tipo) = accion_crear {
            self.ejecutar_creacion(tipo);
        }

        ui.separator();

        // --- Línea de estado --------------------------------------------------
        match &self.status {
            Some((msg, true)) => {
                ui.colored_label(egui::Color32::from_rgb(255, 110, 110), format!("⛔ {msg}"));
            }
            Some((msg, false)) => {
                ui.colored_label(egui::Color32::from_rgb(150, 200, 255), format!("ℹ {msg}"));
            }
            None => {
                ui.label(egui::RichText::new("Listo").italics().weak());
            }
        }

        ui.separator();

        // --- Lista de assets --------------------------------------------------
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Fondo clicable de la zona visible. Se registra ANTES que los
                // elementos para que estos queden encima y conserven su propio
                // menú contextual; el clic derecho en hueco vacío cae aquí.
                let fondo = ui.interact(
                    ui.clip_rect(),
                    ui.make_persistent_id("fondo_lista_assets"),
                    egui::Sense::click(),
                );
                fondo.context_menu(|ui| {
                    if let Some(tipo) = submenu_crear(ui) {
                        let nombre = self.nombre_para_creacion(tipo);
                        let destino = self.current_dir.clone();
                        self.ejecutar_creacion_con(tipo, nombre, Some(destino));
                    }
                });
                if self.entradas.is_empty() {
                    ui.label(egui::RichText::new("(carpeta vacía)").italics().weak());
                }

                let entradas = self.entradas.clone();

                // Acciones diferidas: durante el dibujo sólo se leen campos.
                let mut seleccionar: Option<PathBuf> = None;
                let mut entrar: Option<PathBuf> = None;
                let mut abrir_escena: Option<PathBuf> = None;
                let mut instanciar_prefab: Option<PathBuf> = None;
                let mut info: Option<String> = None;
                let mut empezar_renombrado: Option<PathBuf> = None;
                let mut eliminar: Option<PathBuf> = None;
                let mut crear_contextual: Option<(TipoCreacion, PathBuf)> = None;
                let mut accion_mover: Option<(PathBuf, PathBuf)> = None;
                let mut confirmar_renombrado = false;
                let mut cancelar_renombrado = false;
                let mut nuevo_arrastre: Option<PathBuf> = None;

                for entrada in &entradas {
                    let Entrada {
                        nombre,
                        es_dir,
                        bytes,
                    } = entrada;
                    let ruta = self.current_dir.join(nombre);
                    let icono = icono_para(nombre, *es_dir);

                    // --- Renombrado inline ---
                    if self.rename_target.as_deref() == Some(ruta.as_path()) {
                        let mut confirmar = false;
                        let mut cancelar = false;
                        ui.horizontal(|ui| {
                            ui.label(icono);
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.rename_buffer)
                                    .desired_width(200.0),
                            );
                            if self.rename_focus {
                                resp.request_focus();
                                self.rename_focus = false;
                            }
                            let enter = resp.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if ui.button("✓").clicked() || enter {
                                confirmar = true;
                            }
                            if ui.button("✖").clicked() {
                                cancelar = true;
                            }
                        });
                        if confirmar {
                            confirmar_renombrado = true;
                        }
                        if cancelar {
                            cancelar_renombrado = true;
                        }
                        continue;
                    }

                    // --- Fila normal ---
                    let seleccionada = self.seleccion.as_deref() == Some(ruta.as_path());
                    let etiqueta = format!("{icono} {nombre}");
                    let pista = if *es_dir {
                        format!("Carpeta: {}", ruta.display())
                    } else {
                        format!("{} — {} bytes", ruta.display(), bytes)
                    };
                    let resp = ui.selectable_label(seleccionada, etiqueta).on_hover_text(pista);

                    // Inicio del arrastre de assets instanciables (modelos y
                    // prefabs) hacia la escena o la jerarquía. No se usa
                    // `ui.dnd_drag_source`: su interact interno con
                    // Sense::drag() se queda con la pulsación y la fila deja
                    // de responder a clic, doble clic y menú contextual
                    // (regresión comprobada en vivo). En su lugar se observa
                    // el puntero sobre la propia respuesta de la etiqueta —
                    // que conserva intacto su comportamiento — y cuando el
                    // gesto ya es un arrastre decidido se anota el origen;
                    // la carga útil se publica tras el bucle.
                    // El origen se detecta con `press_origin` dentro del
                    // rect de la fila, no con `is_pointer_button_down_on`:
                    // la interacción de arrastre del ScrollArea se queda con
                    // los ids potenciales de clic/arrastre y la etiqueta
                    // nunca lo reporta (comprobado en vivo).
                    let origen_en_fila = ui
                        .input(|i| i.pointer.press_origin())
                        .is_some_and(|o| resp.rect.contains(o));
                    if es_instanciable(nombre, *es_dir)
                        && origen_en_fila
                        && ui.input(|i| i.pointer.is_decidedly_dragging())
                    {
                        nuevo_arrastre = Some(ruta.clone());
                    }

                    if resp.clicked() {
                        seleccionar = Some(ruta.clone());
                    }
                    if resp.double_clicked() {
                        if *es_dir {
                            entrar = Some(ruta.clone());
                        } else if es_escena(nombre) {
                            abrir_escena = Some(ruta.clone());
                        } else if es_prefab(nombre) {
                            instanciar_prefab = Some(ruta.clone());
                        } else {
                            info = Some(format!("Archivo seleccionado: {}", ruta.display()));
                        }
                    }

                    // --- Menú contextual estilo Unity ---
                    resp.context_menu(|ui| {
                        ui.label(egui::RichText::new(nombre.as_str()).strong());
                        ui.separator();

                        if let Some(tipo) = submenu_crear(ui) {
                            // Carpeta → se crea dentro de ella; archivo → en
                            // la carpeta visible.
                            let destino = if *es_dir {
                                ruta.clone()
                            } else {
                                self.current_dir.clone()
                            };
                            crear_contextual = Some((tipo, destino));
                        }
                        ui.separator();

                        if ui.button("✏ Renombrar").clicked() {
                            empezar_renombrado = Some(ruta.clone());
                            ui.close();
                        }

                        ui.menu_button("📦 Mover a…", |ui| {
                            // La lista se calcula al desplegar el submenú.
                            let destinos = listar_carpetas(&self.assets_root);
                            if destinos.is_empty() {
                                ui.label(egui::RichText::new("(sin carpetas)").italics().weak());
                            }
                            for destino in destinos {
                                // La carpeta actual y todo lo que cuelga del
                                // propio elemento no son destinos válidos.
                                if rutas_iguales(&destino, &self.current_dir) {
                                    continue;
                                }
                                if *es_dir && es_ancestro_o_igual(&ruta, &destino) {
                                    continue;
                                }
                                let etiqueta = ruta_legible(&self.assets_root, &destino);
                                if ui.button(etiqueta).clicked() {
                                    accion_mover = Some((ruta.clone(), destino.clone()));
                                    ui.close();
                                }
                            }
                        });

                        ui.separator();
                        if ui.button("🗑 Eliminar").clicked() {
                            eliminar = Some(ruta.clone());
                            ui.close();
                        }
                    });
                }

                // --- Aplicar acciones diferidas ---
                if let Some(r) = seleccionar {
                    self.seleccion = Some(r);
                }
                if let Some(r) = entrar {
                    self.entrar_en(r);
                }
                if let Some(r) = abrir_escena {
                    self.seleccion = Some(r.clone());
                    self.set_info(format!("Carga de escena encolada: {}", r.display()));
                    self.pending_scene_load = Some(r);
                }
                if let Some(r) = instanciar_prefab {
                    self.seleccion = Some(r.clone());
                    self.set_info(format!("Instanciación de prefab encolada: {}", r.display()));
                    self.pending_prefab_instantiate = Some(r);
                }
                if let Some(msg) = info {
                    self.set_info(msg);
                }
                if let Some(r) = empezar_renombrado {
                    self.iniciar_renombrado(r);
                }
                if let Some((tipo, destino)) = crear_contextual {
                    let nombre = self.nombre_para_creacion(tipo);
                    self.ejecutar_creacion_con(tipo, nombre, Some(destino));
                }
                if let Some(r) = eliminar {
                    self.eliminar(r);
                }
                if let Some((item, destino)) = accion_mover {
                    self.ejecutar_movimiento(item, destino);
                }
                if confirmar_renombrado {
                    self.confirmar_renombrado();
                }
                if cancelar_renombrado {
                    self.rename_target = None;
                    self.rename_focus = false;
                    self.set_info("Renombrado cancelado");
                }
                if let Some(r) = nuevo_arrastre {
                    self.arrastre_asset = Some(r);
                }

                // Arrastre de asset en curso: publicar la carga útil en cada
                // fotograma y dibujar su fantasma junto al cursor (mismo
                // recurso visual que el fantasma de la jerarquía). El plugin
                // DragAndDrop de egui limpia la carga al soltar el botón;
                // los destinos (viewport y jerarquía) la leen en ese mismo
                // fotograma, antes de la limpieza de fin de pasada.
                if self.arrastre_asset.is_some() {
                    if ui.input(|i| i.pointer.any_released()) {
                        self.arrastre_asset = None;
                    } else if let Some(ruta_arrastre) = self.arrastre_asset.clone() {
                        egui::DragAndDrop::set_payload(
                            ui.ctx(),
                            ArrastreAsset {
                                ruta: ruta_arrastre.clone(),
                            },
                        );
                        if let Some(mp) = ui.input(|i| i.pointer.interact_pos()) {
                            let nombre_arrastre = ruta_arrastre
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let texto = format!(
                                "{} {}",
                                icono_para(&nombre_arrastre, false),
                                nombre_arrastre
                            );
                            let pintor = ui.ctx().layer_painter(egui::LayerId::new(
                                egui::Order::Foreground,
                                egui::Id::new("fantasma_asset"),
                            ));
                            let ancho = (texto.chars().count() as f32 * 7.0 + 24.0).max(90.0);
                            let rect_fantasma = egui::Rect::from_min_size(
                                mp + egui::vec2(12.0, 10.0),
                                egui::vec2(ancho, 22.0),
                            );
                            let sel = ui.visuals().selection.bg_fill;
                            pintor.rect_filled(
                                rect_fantasma,
                                2.0,
                                egui::Color32::from_rgba_unmultiplied(
                                    sel.r(),
                                    sel.g(),
                                    sel.b(),
                                    225,
                                ),
                            );
                            pintor.rect_stroke(
                                rect_fantasma,
                                2.0,
                                egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 160, 255)),
                                egui::StrokeKind::Inside,
                            );
                            pintor.text(
                                rect_fantasma.min + egui::vec2(10.0, 11.0),
                                egui::Align2::LEFT_CENTER,
                                texto,
                                egui::FontId::proportional(12.0),
                                egui::Color32::WHITE,
                            );
                        }
                    }
                }
            });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    /// Carpeta temporal única por test (proceso + instante).
    fn dir_temporal_creacion(etiqueta: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "glass_editor_{etiqueta}_{}_{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("debe crearse el dir temporal");
        dir
    }

    #[test]
    fn crear_material_escribe_json_valido() {
        let assets = dir_temporal_creacion("material");
        let ruta = super::crear_material(&assets, "Rojo").expect("debe crear el material");
        assert_eq!(ruta, assets.join("materials").join("Rojo.material.json"));

        let texto = std::fs::read_to_string(&ruta).expect("debe poder leerse");
        let _: glass_core::MaterialData =
            serde_json::from_str(&texto).expect("debe ser un MaterialData válido");

        assert!(super::crear_material(&assets, "Rojo").is_err());
        let _ = std::fs::remove_dir_all(&assets);
    }

    #[test]
    fn subcarpeta_respeta_la_convencion_existente() {
        let assets = dir_temporal_creacion("subcarpeta");
        assert_eq!(
            super::subcarpeta(&assets, "scenes", "Scenes"),
            assets.join("scenes")
        );

        std::fs::create_dir_all(assets.join("Scenes")).expect("debe crearse Scenes");
        assert_eq!(
            super::subcarpeta(&assets, "scenes", "Scenes"),
            assets.join("Scenes")
        );
        let _ = std::fs::remove_dir_all(&assets);
    }

    #[test]
    fn set_assets_root_reenraiza_y_refresca() {
        let assets = dir_temporal_creacion("reenraizar");
        std::fs::write(assets.join("Prueba.scene.json"), "{}").expect("debe escribirse");
        std::fs::create_dir_all(assets.join("Scenes")).expect("debe crearse Scenes");

        let mut panel = super::ProjectPanel::new();
        panel.set_assets_root(assets.clone());

        assert_eq!(panel.assets_root, assets);
        assert_eq!(panel.current_dir, assets);
        assert!(!panel.entradas.is_empty());
        let _ = std::fs::remove_dir_all(&assets);
    }
    use super::*;

    /// Directorio temporal único (sin crates externas).
    fn temp_dir_unico(etiqueta: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CONTADOR: AtomicU64 = AtomicU64::new(0);

        let n = CONTADOR.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "glass_project_{etiqueta}_{}_{}_{}",
            std::process::id(),
            nanos,
            n
        ));
        std::fs::create_dir_all(&dir).expect("crear directorio temporal");
        dir
    }

    #[test]
    fn validar_nombre_rechaza_invalidos() {
        assert!(validar_nombre("").is_err());
        assert!(validar_nombre("   ").is_err());
        assert!(validar_nombre("a/b").is_err());
        assert!(validar_nombre("a\\b").is_err());
        assert!(validar_nombre(".oculto").is_err());
        assert!(validar_nombre("Heroe").is_ok());
        assert!(validar_nombre("Heroe_01").is_ok());
    }

    #[test]
    fn listar_directorio_ordena_carpetas_primero() {
        let raiz = temp_dir_unico("listado");
        std::fs::create_dir_all(raiz.join("Zeta")).unwrap();
        std::fs::write(raiz.join("alfa.txt"), "x").unwrap();
        std::fs::write(raiz.join("Beta.txt"), "x").unwrap();
        std::fs::write(raiz.join(".oculto"), "x").unwrap();

        let entradas = listar_directorio(&raiz).unwrap();
        assert_eq!(entradas.len(), 3);
        assert_eq!(entradas[0].nombre, "Zeta");
        assert!(entradas[0].es_dir);
        assert_eq!(entradas[1].nombre, "alfa.txt");
        assert_eq!(entradas[2].nombre, "Beta.txt");

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn importar_archivo_copia_y_resuelve_colision() {
        let raiz = temp_dir_unico("importar");
        let origen_dir = raiz.join("fuera");
        std::fs::create_dir_all(&origen_dir).unwrap();
        let origen = origen_dir.join("modelo.glb");
        std::fs::write(&origen, b"glb-falso").unwrap();
        let destino_dir = raiz.join("assets");
        std::fs::create_dir_all(&destino_dir).unwrap();

        let creado = importar_archivo(&origen, &destino_dir).unwrap();
        assert_eq!(creado, destino_dir.join("modelo.glb"));
        assert_eq!(std::fs::read(&creado).unwrap(), b"glb-falso");

        let creado2 = importar_archivo(&origen, &destino_dir).unwrap();
        assert_eq!(creado2, destino_dir.join("modelo (2).glb"));

        assert!(importar_archivo(&origen_dir, &destino_dir).is_err());
        assert!(importar_archivo(&raiz.join("nope.glb"), &destino_dir).is_err());

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn crear_y_renombrar_carpeta() {
        let raiz = temp_dir_unico("renombrar");
        std::fs::create_dir_all(&raiz).unwrap();

        let nueva = crear_carpeta(&raiz, "Escenas").unwrap();
        assert!(nueva.is_dir());
        assert!(crear_carpeta(&raiz, "Escenas").is_err());

        let renombrada = renombrar(&nueva, "Niveles").unwrap();
        assert!(renombrada.is_dir());
        assert!(!nueva.exists());

        assert!(renombrar(&renombrada, "mal/nombre").is_err());
        assert!(renombrar(&renombrada, ".oculto").is_err());

        std::fs::write(raiz.join("otro.txt"), "x").unwrap();
        assert!(renombrar(&renombrada, "otro.txt").is_err());

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn crear_script_en_managed() {
        let raiz = temp_dir_unico("script");
        std::fs::create_dir_all(raiz.join("assets")).unwrap();

        let ruta = crear_script_cs(&raiz, "Jugador").unwrap();
        assert!(ruta.ends_with("managed/GlassEngine.Core/Scripts/Jugador.cs"));
        let texto = std::fs::read_to_string(&ruta).unwrap();
        assert!(texto.contains("public class Jugador : MonoBehaviour"));
        assert!(texto.contains("void Update() { }"));

        // No se duplica la extensión ni se sobrescribe.
        assert!(crear_script_cs(&raiz, "Jugador.cs").is_err());

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn crear_escena_escribe_plantilla() {
        let raiz = temp_dir_unico("escena");
        let assets = raiz.join("assets");
        std::fs::create_dir_all(&assets).unwrap();

        let ruta = crear_escena(&assets, "Nivel1").unwrap();
        assert_eq!(
            ruta.file_name().unwrap().to_string_lossy(),
            "Nivel1.scene.json"
        );
        let texto = std::fs::read_to_string(&ruta).unwrap();
        assert!(texto.starts_with('['));
        assert!(texto.contains("\"Main Camera\""));
        assert!(crear_escena(&assets, "Nivel1").is_err());

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn crear_grafo_copia_el_primero() {
        let raiz = temp_dir_unico("grafo");
        let assets = raiz.join("assets");
        std::fs::create_dir_all(assets.join("shader_graphs")).unwrap();

        // Sin fuente: debe fallar con mensaje.
        assert!(crear_grafo(&assets, "brillo").is_err());

        std::fs::write(
            assets.join("shader_graphs/laser.graph.json"),
            "{\"name\":\"laser\"}",
        )
        .unwrap();
        let nuevo = crear_grafo(&assets, "brillo").unwrap();
        assert_eq!(
            nuevo.file_name().unwrap().to_string_lossy(),
            "brillo.graph.json"
        );
        assert_eq!(
            std::fs::read_to_string(&nuevo).unwrap(),
            "{\"name\":\"laser\"}"
        );

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn mover_elemento_valida_destinos() {
        let raiz = temp_dir_unico("mover");
        let assets = raiz.join("assets");
        std::fs::create_dir_all(assets.join("sub")).unwrap();
        std::fs::create_dir_all(assets.join("otra")).unwrap();
        std::fs::write(assets.join("a.txt"), "x").unwrap();

        let nuevo = mover_elemento(&assets.join("a.txt"), &assets.join("sub"), &assets).unwrap();
        assert!(nuevo.exists());
        assert!(!assets.join("a.txt").exists());

        // Una carpeta no puede acabar dentro de sí misma.
        std::fs::create_dir_all(assets.join("sub/hija")).unwrap();
        assert!(mover_elemento(&assets.join("sub"), &assets.join("sub/hija"), &assets).is_err());

        // La raíz de assets/ tampoco se mueve.
        assert!(mover_elemento(&assets, &assets.join("otra"), &assets).is_err());

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn papelera_no_borra_y_desambigua() {
        let raiz = temp_dir_unico("papelera");
        let assets = raiz.join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(assets.join("dup.txt"), "hola").unwrap();

        let d1 = mover_a_papelera(&raiz, &assets.join("dup.txt")).unwrap();
        assert!(!assets.join("dup.txt").exists());
        assert!(d1.exists());
        assert_eq!(std::fs::read_to_string(&d1).unwrap(), "hola");

        std::fs::write(assets.join("dup.txt"), "otra vez").unwrap();
        let d2 = mover_a_papelera(&raiz, &assets.join("dup.txt")).unwrap();
        assert_ne!(d1, d2);
        assert!(d2.exists());
        assert!(d2
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("(2)"));

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn listar_carpetas_recorre_el_arbol() {
        let raiz = temp_dir_unico("carpetas");
        let assets = raiz.join("assets");
        std::fs::create_dir_all(assets.join("a/b")).unwrap();
        std::fs::create_dir_all(assets.join("c")).unwrap();
        std::fs::create_dir_all(assets.join(".oculta")).unwrap();

        let carpetas = listar_carpetas(&assets);
        assert!(carpetas.contains(&assets));
        assert!(carpetas.iter().any(|p| p.ends_with("a/b")));
        assert!(!carpetas.iter().any(|p| p.ends_with(".oculta")));
        assert_eq!(carpetas.len(), 4); // assets, a, a/b, c

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn iconos_por_extension() {
        assert_eq!(icono_para("Nivel.scene.json", false), "🎬");
        assert_eq!(icono_para("Enemigo.prefab.json", false), "🧩");
        assert_eq!(icono_para("Heroe.cs", false), "📜");
        assert_eq!(icono_para("laser.graph.json", false), "🎨");
        assert_eq!(icono_para("xbot.glb", false), "🗿");
        assert_eq!(icono_para("piel.png", false), "🖼");
        assert_eq!(icono_para("golpe.wav", false), "🔊");
        assert_eq!(icono_para("notas.txt", false), "📄");
        assert_eq!(icono_para("cualquiera", true), "📁");
    }

    #[test]
    fn instanciables_solo_modelos_y_prefabs() {
        assert!(es_instanciable("Heroe.blend", false));
        assert!(es_instanciable("Heroe.fbx", false));
        assert!(es_instanciable("Heroe.glb", false));
        assert!(es_instanciable("Enemigo.prefab.json", false));
        assert!(!es_instanciable("Nivel.scene.json", false));
        assert!(!es_instanciable("Heroe.cs", false));
        assert!(!es_instanciable("modelos", true));
    }

    #[test]
    fn instanciar_modelo_crea_entidad_con_su_stem() {
        let mut escena = glass_core::SceneState::new();
        let id = instanciar_asset(
            &mut escena,
            std::path::Path::new("assets/models/Heroe.blend"),
            [1.0, 0.0, 2.0],
        )
        .expect("el modelo debe instanciarse");

        let entidad = escena.get_entity_mut(id).expect("la entidad debe existir");
        assert_eq!(entidad.name, "Heroe");
        assert_eq!(entidad.transform.position, [1.0, 0.0, 2.0]);
        let mesh = entidad.mesh.as_ref().expect("debe tener mesh");
        assert_eq!(mesh.model, "Heroe");
        assert_eq!(mesh.shader, "pbr.vert.spv");

        assert!(instanciar_asset(
            &mut escena,
            std::path::Path::new("assets/Heroe.cs"),
            [0.0, 0.0, 0.0],
        )
        .is_err());
    }

    #[test]
    fn instanciar_prefab_desde_archivo() {
        let dir = temp_dir_unico("prefab_drop");
        std::fs::create_dir_all(&dir).unwrap();

        let mut origen = glass_core::SceneState::new();
        let raiz_id = origen.create_entity(
            "Caja".to_string(),
            vec!["Transform".to_string(), "MeshRenderer".to_string()],
        );
        let prefab = glass_core::Prefab::from_entity(&origen, raiz_id).unwrap();
        let ruta = dir.join("Caja.prefab.json");
        std::fs::write(&ruta, serde_json::to_string(&prefab).unwrap()).unwrap();

        let mut escena = glass_core::SceneState::new();
        let id = instanciar_asset(&mut escena, &ruta, [3.0, 0.0, 4.0]).unwrap();
        let entidad = escena.get_entity_mut(id).expect("la entidad debe existir");
        assert_eq!(entidad.name, "Caja");
        assert_eq!(entidad.transform.position, [3.0, 0.0, 4.0]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
