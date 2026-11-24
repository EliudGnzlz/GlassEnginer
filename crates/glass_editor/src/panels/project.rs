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

/// Indica si `nombre` es un `.glb` generado automáticamente a partir de una
/// fuente (`.fbx` / `.blend`) presente en la misma carpeta, y que por tanto
/// debe ocultarse en el panel para no mostrar el mismo modelo dos veces.
///
/// Regla (deben cumplirse todas):
/// - `nombre` termina en `.glb` (extensión sin distinguir mayúsculas).
/// - El stem no termina en `_edited`: los `*_edited.glb` de MeshForge son
///   copias deliberadas del usuario y se muestran siempre.
/// - Existe `dir/<nombre>.meta.json`, se lee y parsea como JSON, y su campo
///   `source` (string) es exactamente `<stem>.fbx` o `<stem>.blend`.
/// - La fuente indicada existe como fichero en el mismo `dir`.
///
/// Es segura porque cualquier fallo devuelve `false` y el `.glb` se muestra.
/// Un `.glb` sin sidecar se ve siempre, y si el usuario borra la fuente el
/// `.glb` reaparece. Nunca entra en pánico.
pub fn es_glb_gemelo_oculto(dir: &Path, nombre: &str) -> bool {
    let ruta = Path::new(nombre);

    let es_glb = ruta
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("glb"));
    if !es_glb {
        return false;
    }

    let Some(stem) = ruta.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    if stem.ends_with("_edited") {
        return false;
    }

    let sidecar = dir.join(format!("{nombre}.meta.json"));
    let Ok(texto) = std::fs::read_to_string(&sidecar) else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&texto) else {
        return false;
    };
    let Some(source) = json.get("source").and_then(|v| v.as_str()) else {
        return false;
    };

    let es_su_fuente = source == format!("{stem}.fbx") || source == format!("{stem}.blend");
    es_su_fuente && dir.join(source).is_file()
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
        if nombre.ends_with(".meta.json") {
            continue;
        }
        let es_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
        // Oculta el .glb generado cuando su fuente (.fbx/.blend) está al lado.
        if !es_dir && es_glb_gemelo_oculto(dir, &nombre) {
            continue;
        }
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

/// Firma barata del contenido de un directorio (nombres, tipo, tamaño y
/// mtime de cada entrada). Sirve para detectar cambios, no para mostrar:
/// incluye también ocultos y `.meta.json`. Devuelve 0 si no se puede leer.
pub fn firma_directorio(dir: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut items: Vec<std::fs::DirEntry> = rd.flatten().collect();
    items.sort_by_key(|item| item.file_name());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for item in &items {
        item.file_name().hash(&mut hasher);
        let es_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
        es_dir.hash(&mut hasher);
        let meta = item.metadata().ok();
        meta.as_ref().map(|m| m.len()).unwrap_or(0).hash(&mut hasher);
        let mtime = meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .unwrap_or_default();
        mtime.as_secs().hash(&mut hasher);
        mtime.subsec_nanos().hash(&mut hasher);
    }
    hasher.finish()
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

/// Crea `<dir>/<Nombre>.physmat.json` con la plantilla de asset nuevo
/// de Physics Material (0.6/0.6/0.0, §2 del diseño de colliders).
pub fn crear_physics_material_en(dir: &Path, nombre: &str) -> Result<PathBuf, String> {
    let base = nombre_sin_sufijo(nombre.trim(), &[".physmat.json", ".json"]);
    validar_nombre(&base)?;

    std::fs::create_dir_all(dir)
        .map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;

    let destino = dir.join(format!("{base}.physmat.json"));
    if destino.exists() {
        return Err(format!("Ya existe {}", destino.display()));
    }
    let json = serde_json::to_string_pretty(&glass_core::PhysicsMaterial::NEW_ASSET)
        .map_err(|e| format!("No se pudo serializar el material físico: {e}"))?;
    std::fs::write(&destino, json)
        .map_err(|e| format!("No se pudo escribir {}: {e}", destino.display()))?;
    Ok(destino)
}

/// Crea `<assets>/materials/<Nombre>.physmat.json` (o `Materials/` si el
/// proyecto usa mayúsculas): la carpeta tipo Materiales, como el material
/// visual.
pub fn crear_physics_material(assets_root: &Path, nombre: &str) -> Result<PathBuf, String> {
    crear_physics_material_en(&subcarpeta(assets_root, "materials", "Materials"), nombre)
}

/// Crea `<assets>/prefabs/<Nombre>.prefab.json` (o `Prefabs/` si el
/// proyecto usa mayúsculas) con un prefab vacío: una sola entidad raíz
/// con Transform y el nombre dado.
pub fn crear_prefab(assets_root: &Path, nombre: &str) -> Result<PathBuf, String> {
    crear_prefab_en(&subcarpeta(assets_root, "prefabs", "Prefabs"), nombre)
}

/// Crea `<dir>/<Nombre>.prefab.json` con un prefab vacío válido.
pub fn crear_prefab_en(dir: &Path, nombre: &str) -> Result<PathBuf, String> {
    let base = nombre_sin_sufijo(nombre.trim(), &[".prefab.json", ".json"]);
    validar_nombre(&base)?;
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    let destino = dir.join(format!("{base}.prefab.json"));
    if destino.exists() {
        return Err(format!("Ya existe {}", destino.display()));
    }
    let mut escena = glass_core::SceneState::new();
    let raiz = escena.create_entity(base.clone(), vec!["Transform".to_string()]);
    let prefab = glass_core::Prefab::from_entity(&escena, raiz)?;
    prefab
        .save_to_file(&destino)
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
    for comp in [
        ".scene.json",
        ".prefab.json",
        ".graph.json",
        ".physmat.json",
        ".material.json",
    ] {
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

/// Nombres de archivo (último componente) de un lote de rutas.
fn nombres_de(rutas: &[PathBuf]) -> Vec<String> {
    rutas
        .iter()
        .map(|r| {
            r.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| r.to_string_lossy().into_owned())
        })
        .collect()
}

/// Describe un lote para la banda y el log: hasta 3 nombres separados por
/// comas; con más de 3, «N archivos».
fn describir_lote(nombres: &[String]) -> String {
    if nombres.len() > 3 {
        format!("{} archivos", nombres.len())
    } else {
        nombres.join(", ")
    }
}

/// Normaliza una ruta para compararla: separadores '/', sin prefijo
/// verbatim inicial, que tras cambiar separadores es «//?/» y en
/// minúsculas.
fn normalizar_ruta_conversion(ruta: &str) -> String {
    let con_barras = ruta.replace('\\', "/");
    let sin_prefijo = con_barras.strip_prefix("//?/").unwrap_or(&con_barras);
    sin_prefijo.to_lowercase()
}

/// Nombres de los modelos (.blend/.fbx, sin distinguir mayúsculas) de la
/// carpeta visible cuya conversión está en curso.
///
/// Regla de correspondencia (no conocemos la raíz del watcher, así que no
/// se comparan rutas completas): se forma el sufijo
/// «<carpeta visible>/<nombre>» (p. ej. «models/casa.blend» para
/// «…/Assets/Models/casa.blend»), o solo «<nombre>» si la carpeta visible
/// no tiene último componente con nombre. Hay coincidencia si la ruta en
/// curso, normalizada (minúsculas, '/', sin «\\?\»), es igual al sufijo o
/// termina en «/» + sufijo. La carpeta visible se extrae de la ruta YA
/// normalizada, para que el cálculo sea idéntico con separadores de
/// Windows o de Unix sea cual sea la plataforma que compila.
pub fn nombres_convirtiendo(
    entradas: &[Entrada],
    en_curso: &[PathBuf],
    current_dir: &Path,
) -> Vec<String> {
    if en_curso.is_empty() {
        return Vec::new();
    }
    let en_curso_norm: Vec<String> = en_curso
        .iter()
        .map(|p| normalizar_ruta_conversion(&p.to_string_lossy()))
        .collect();
    let dir_norm = normalizar_ruta_conversion(&current_dir.to_string_lossy());
    let carpeta = dir_norm
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let mut nombres = Vec::new();
    for entrada in entradas {
        if entrada.es_dir {
            continue;
        }
        let nombre_min = entrada.nombre.to_lowercase();
        if !(nombre_min.ends_with(".blend") || nombre_min.ends_with(".fbx")) {
            continue;
        }
        let sufijo = match &carpeta {
            Some(c) => format!("{c}/{nombre_min}"),
            None => nombre_min,
        };
        let con_barra = format!("/{sufijo}");
        if en_curso_norm
            .iter()
            .any(|r| *r == sufijo || r.ends_with(&con_barra))
        {
            nombres.push(entrada.nombre.clone());
        }
    }
    nombres
}

/// Mueve un elemento a `.project_trash/` (hermana de la raíz de
/// assets). Nada se borra. Recibe la raíz de assets REAL del proyecto
/// (con la capitalización que tenga en disco: `Assets` o `assets`):
/// reconstruirla como `raíz.join("assets")` rompía el borrado en
/// sistemas de archivos sensibles a mayúsculas cuando la carpeta se
/// llama `Assets` (el caso de las plantillas), porque la resolución
/// canónica fallaba y ningún asset se podía mandar a la papelera.
pub fn mover_a_papelera(assets_root: &Path, ruta: &Path) -> Result<PathBuf, String> {
    if !ruta.exists() {
        return Err(format!("No existe {}", ruta.display()));
    }
    // La raíz del proyecto es la carpeta madre de la de assets.
    let raiz_proyecto = assets_root.parent().unwrap_or(assets_root);
    if rutas_iguales(ruta, raiz_proyecto) {
        return Err("No se puede eliminar la raíz del proyecto".to_string());
    }
    // Solo se manda a la papelera lo que vive dentro de assets/ (la
    // función es pública; la UI ya filtra, pero la garantía va aquí).
    let assets_canon = std::fs::canonicalize(assets_root)
        .map_err(|e| format!("No se pudo resolver {}: {e}", assets_root.display()))?;
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

/// Productos de conversión que acompañan a una fuente convertible
/// (`.blend`/`.fbx`): su `.glb` gemelo y el sidecar de ese `.glb`,
/// SOLO si existen en disco. Para cualquier otra extensión, vacío.
/// Duplica un archivo o carpeta en su misma carpeta usando la
/// convención ` (2)`, ` (3)`... de `ruta_libre_en`. Devuelve la ruta
/// de la copia. Cada archivo se duplica por su cuenta: duplicar una
/// fuente .blend/.fbx NO arrastra su .glb convertido (el vigilante
/// convertirá la copia).
pub fn duplicar_elemento(ruta: &Path) -> Result<PathBuf, String> {
    if !ruta.exists() {
        return Err(format!(
            "No se pudo duplicar {}: la ruta no existe",
            ruta.display()
        ));
    }
    let dir_padre = ruta.parent().ok_or_else(|| {
        format!(
            "No se pudo duplicar {}: no tiene carpeta padre",
            ruta.display()
        )
    })?;
    let nombre = ruta.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        format!(
            "No se pudo duplicar {}: nombre de archivo no válido",
            ruta.display()
        )
    })?;

    let destino = ruta_libre_en(dir_padre, nombre)?;

    if ruta.is_dir() {
        if let Err(e) = copiar_dir_recursivo(ruta, &destino) {
            // El destino no existía antes (ruta_libre_en lo garantiza):
            // se limpia la copia parcial.
            let _ = std::fs::remove_dir_all(&destino);
            return Err(e);
        }
    } else {
        std::fs::copy(ruta, &destino)
            .map_err(|e| format!("No se pudo copiar {}: {e}", ruta.display()))?;
    }

    Ok(destino)
}

/// Crea `destino` y copia en él todo el contenido de `origen`,
/// recursivamente.
fn copiar_dir_recursivo(origen: &Path, destino: &Path) -> Result<(), String> {
    std::fs::create_dir(destino)
        .map_err(|e| format!("No se pudo crear la carpeta {}: {e}", destino.display()))?;

    let entradas = std::fs::read_dir(origen)
        .map_err(|e| format!("No se pudo leer la carpeta {}: {e}", origen.display()))?;

    for entrada in entradas {
        let entrada =
            entrada.map_err(|e| format!("No se pudo leer una entrada de {}: {e}", origen.display()))?;
        let ruta_origen = entrada.path();
        let ruta_destino = destino.join(entrada.file_name());
        let tipo = entrada
            .file_type()
            .map_err(|e| format!("No se pudo obtener el tipo de {}: {e}", ruta_origen.display()))?;

        if tipo.is_dir() {
            copiar_dir_recursivo(&ruta_origen, &ruta_destino)?;
        } else {
            std::fs::copy(&ruta_origen, &ruta_destino)
                .map_err(|e| format!("No se pudo copiar {}: {e}", ruta_origen.display()))?;
        }
    }
    Ok(())
}

pub fn gemelos_convertidos(ruta: &Path) -> Vec<PathBuf> {
    let convertible = ruta
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|extension| extension == "blend" || extension == "fbx");

    if !convertible {
        return vec![];
    }

    let glb = ruta.with_extension("glb");
    if !glb.exists() {
        return vec![];
    }

    let mut gemelos = vec![glb.clone()];
    let Some(nombre) = glb.file_name() else {
        return gemelos;
    };

    let mut nombre_sidecar = nombre.to_os_string();
    nombre_sidecar.push(".meta.json");
    let sidecar = glb.with_file_name(nombre_sidecar);

    if sidecar.exists() {
        gemelos.push(sidecar);
    }

    gemelos
}

/// Migas de pan desde la raíz de assets hasta `current_dir`: pares
/// (etiqueta visible, ruta acumulada). La primera etiqueta es el nombre
/// de la propia raíz. Si `current_dir` no cuelga de `assets_root`,
/// devuelve una sola miga con la raíz.
pub fn migas(assets_root: &Path, current_dir: &Path) -> Vec<(String, PathBuf)> {
    let nombre_raiz = assets_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "assets".to_string());
    let mut salida = vec![(nombre_raiz, assets_root.to_path_buf())];
    if let Ok(rel) = current_dir.strip_prefix(assets_root) {
        let mut acumulada = assets_root.to_path_buf();
        for componente in rel.components() {
            acumulada.push(componente.as_os_str());
            salida.push((
                componente.as_os_str().to_string_lossy().into_owned(),
                acumulada.clone(),
            ));
        }
    }
    salida
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

/// Ruta del .glb que alimenta la miniatura de un modelo: el propio
/// archivo si es .glb; el .glb hermano si es .blend/.fbx y existe;
/// None en otro caso. El .gltf (JSON) queda fuera: el cargador solo
/// acepta GLB binario y un .gltf puede depender de recursos externos.
pub fn ruta_glb_miniatura(ruta: &Path) -> Option<PathBuf> {
    let ext = ruta
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "glb" => Some(ruta.to_path_buf()),
        "blend" | "fbx" => {
            let hermano = ruta.with_extension("glb");
            hermano.exists().then_some(hermano)
        }
        _ => None,
    }
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
    if glass_core::physics_material::es_physmat(nombre) {
        return "🧲";
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

/// Accesos rápidos de «Favoritos»: búsquedas por tipo sobre todo el árbol
/// de assets, como las búsquedas guardadas de la ventana Proyecto de Unity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipoRapido {
    Escenas,
    Materiales,
    Modelos,
    Prefabs,
    Scripts,
}

impl TipoRapido {
    pub const TODOS: [TipoRapido; 5] = [
        TipoRapido::Escenas,
        TipoRapido::Materiales,
        TipoRapido::Modelos,
        TipoRapido::Prefabs,
        TipoRapido::Scripts,
    ];

    pub fn etiqueta(&self) -> &'static str {
        match self {
            TipoRapido::Escenas => "🎬 Escenas",
            TipoRapido::Materiales => "🖌 Materiales",
            TipoRapido::Modelos => "🗿 Modelos",
            TipoRapido::Prefabs => "🧩 Prefabs",
            TipoRapido::Scripts => "📜 Scripts",
        }
    }

    pub fn coincide(&self, nombre: &str) -> bool {
        match self {
            TipoRapido::Escenas => es_escena(nombre),
            TipoRapido::Materiales => nombre.ends_with(".material.json"),
            TipoRapido::Modelos => es_modelo(nombre),
            TipoRapido::Prefabs => es_prefab(nombre),
            TipoRapido::Scripts => nombre.ends_with(".cs"),
        }
    }
}

/// Búsqueda recursiva por tipo desde `raiz`: devuelve las rutas completas
/// de los archivos que coinciden, en orden de recorrido (carpetas primero
/// en cada nivel, como `listar_directorio`, que además ya oculta los
/// `.meta.json` y los nombres con punto inicial). Tope de profundidad 32.
pub fn buscar_por_tipo(raiz: &std::path::Path, tipo: TipoRapido) -> Vec<std::path::PathBuf> {
    let mut salida = Vec::new();
    let mut pila: Vec<(std::path::PathBuf, usize)> = vec![(raiz.to_path_buf(), 0)];
    while let Some((dir, profundidad)) = pila.pop() {
        let Ok(entradas) = listar_directorio(&dir) else {
            continue;
        };
        let mut subcarpetas = Vec::new();
        for entrada in entradas {
            let ruta = dir.join(&entrada.nombre);
            if entrada.es_dir {
                subcarpetas.push(ruta);
            } else if tipo.coincide(&entrada.nombre) {
                salida.push(ruta);
            }
        }
        if profundidad < 32 {
            // Al apilar al revés, el recorrido visita las carpetas en orden.
            for sub in subcarpetas.into_iter().rev() {
                pila.push((sub, profundidad + 1));
            }
        }
    }
    salida
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
    PhysicsMaterial,
    Prefab,
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
            TipoCreacion::PhysicsMaterial => "Nuevo Physics Material",
            TipoCreacion::Prefab => "NuevoPrefab",
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
            (TipoCreacion::PhysicsMaterial, "🧲 Physics Material"),
            (TipoCreacion::Prefab, "🧩 Prefab"),
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
    /// Rect del panel en el último frame renderizado: el docking lo
    /// usa para saber que un clic cayó dentro del panel (contexto
    /// de Supr aunque la selección no cambie de valor).
    pub rect_ultimo: egui::Rect,
    /// Elemento en edición inline y su buffer.
    pub rename_target: Option<PathBuf>,
    pub rename_buffer: String,
    rename_focus: bool,
    /// Última línea de estado: (mensaje, es_error).
    pub status: Option<(String, bool)>,
    /// Rutas FUENTE (.blend/.fbx) con conversión en vuelo. Las rellena el
    /// docking cada frame (absolutas, quizá con prefijo verbatim).
    pub conversiones_en_curso: Vec<PathBuf>,
    /// Instante en que se aplicó el resultado de la última importación por
    /// arrastre (con éxito o con errores). Lo consume la sonda de main.rs.
    pub importacion_completada_en: Option<std::time::Instant>,
    /// Receptor del lote que se está copiando en segundo plano: entrega
    /// (importados, errores) cuando el hilo termina.
    import_rx: Option<std::sync::mpsc::Receiver<(Vec<PathBuf>, Vec<String>)>>,
    /// Nombres de archivo del lote en curso (para la banda de progreso).
    importando: Vec<String>,
    /// Lotes soltados mientras otro se importaba: (rutas, destino), con el
    /// destino capturado en el momento del soltado.
    importaciones_encoladas: Vec<(Vec<PathBuf>, PathBuf)>,
    /// Cola consumida por `EditorApp`: cargar escena.
    pub pending_scene_load: Option<PathBuf>,
    /// Cola consumida por `EditorApp`: instanciar prefab.
    pub pending_prefab_instantiate: Option<PathBuf>,
    /// Cola consumida por `EditorApp`: reconvertir un .blend/.fbx a .glb
    /// (la misma maquinaria que la conversión al soltar en la escena).
    pub pending_conversion: Option<PathBuf>,
    /// Asset cuyo arrastre hacia la escena o la jerarquía está en curso.
    pub arrastre_asset: Option<PathBuf>,
    /// Texto del buscador del panel (filtra el listado por nombre).
    pub busqueda: String,
    /// Árbol de carpetas cacheado (raíz primero); se renueva en `refrescar`.
    pub arbol_carpetas: Vec<PathBuf>,
    /// Vista rápida activa (Favoritos): búsqueda por tipo en todo el árbol.
    pub vista_rapida: Option<TipoRapido>,
    /// Resultados cacheados de la vista rápida (rutas completas).
    pub entradas_rapidas: Vec<PathBuf>,
    /// Tamaño de iconos de la barra inferior: 0 = lista; >0 = rejilla.
    pub tamano_iconos: f32,
    /// Caché de miniaturas de imágenes (clave: ruta del archivo).
    miniaturas: std::collections::HashMap<PathBuf, Option<egui::TextureHandle>>,
    /// Caché de miniaturas de modelos 3D renderizadas (clave: ruta del
    /// archivo; valor: textura y mtime del .glb fuente al calcularla —
    /// si el .glb cambia o aparece, la miniatura se rehace).
    miniaturas_modelo:
        std::collections::HashMap<PathBuf, (Option<egui::TextureHandle>, Option<std::time::SystemTime>)>,
    /// Miniaturas de modelo que aún se pueden generar en este frame:
    /// la carga y el rasterizado son síncronos y no deben congelar la
    /// UI cuando una carpeta trae muchos modelos.
    presupuesto_miniaturas_modelo: usize,
    /// Caché de colores de material ya leídos (clave: ruta del .material.json).
    colores_material: std::collections::HashMap<PathBuf, Option<egui::Color32>>,
    /// Firma del directorio visible en el último `refrescar()`.
    firma_directorio_actual: u64,
    /// Momento de la última comprobación de cambios externos.
    ultima_comprobacion_externa: std::time::Instant,
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
            rect_ultimo: egui::Rect::NOTHING,
            rename_target: None,
            rename_buffer: String::new(),
            rename_focus: false,
            status: None,
            conversiones_en_curso: Vec::new(),
            importacion_completada_en: None,
            import_rx: None,
            importando: Vec::new(),
            importaciones_encoladas: Vec::new(),
            pending_scene_load: None,
            pending_prefab_instantiate: None,
            pending_conversion: None,
            arrastre_asset: None,
            busqueda: String::new(),
            arbol_carpetas: Vec::new(),
            vista_rapida: None,
            entradas_rapidas: Vec::new(),
            tamano_iconos: 0.0,
            miniaturas: std::collections::HashMap::new(),
            miniaturas_modelo: std::collections::HashMap::new(),
            presupuesto_miniaturas_modelo: 0,
            colores_material: std::collections::HashMap::new(),
            firma_directorio_actual: 0,
            ultima_comprobacion_externa: std::time::Instant::now(),
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
        let msg = msg.into();
        // La línea de estado permanente se retiró de la UI a petición
        // del usuario (2026-11-18): los mensajes viven en la Consola.
        log::info!("Proyecto: {}", msg);
        self.status = Some((msg, false));
    }

    fn set_error(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        log::error!("Proyecto: {}", msg);
        self.status = Some((msg, true));
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
        self.arbol_carpetas = listar_carpetas(&self.assets_root);
        self.entradas_rapidas = match self.vista_rapida {
            Some(tipo) => buscar_por_tipo(&self.assets_root, tipo),
            None => Vec::new(),
        };
        // Las operaciones internas dejan la firma al día: no disparan un
        // segundo refresco en `comprobar_cambios_externos`.
        self.firma_directorio_actual = firma_directorio(&self.current_dir);
    }

    /// Comprueba si el directorio visible cambió por fuera del panel
    /// (una conversión terminada, una copia desde el explorador del
    /// SO…) y refresca solo en ese caso. Barata: como mucho relee los
    /// metadatos del directorio visible dos veces por segundo.
    pub fn comprobar_cambios_externos(&mut self) {
        if self.ultima_comprobacion_externa.elapsed() < std::time::Duration::from_millis(500) {
            return;
        }
        self.ultima_comprobacion_externa = std::time::Instant::now();
        if firma_directorio(&self.current_dir) != self.firma_directorio_actual {
            self.refrescar();
        }
    }

    /// Importa los archivos soltados desde el SO al directorio visible
    /// SIN bloquear el frame: captura el destino ahora (la carpeta visible
    /// en el momento del soltado) y copia en un hilo aparte. Si ya hay un
    /// lote en curso, este se encola y se lanza al terminar el anterior.
    /// El resultado se aplica en `recoger_importacion` (cada frame).
    pub fn importar_soltados(&mut self, rutas: &[PathBuf]) {
        if rutas.is_empty() {
            return;
        }
        let destino = self.current_dir.clone();
        if self.import_rx.is_some() {
            let nombres = nombres_de(rutas);
            self.importaciones_encoladas.push((rutas.to_vec(), destino));
            self.set_info(format!(
                "Importación en cola: {}",
                describir_lote(&nombres)
            ));
            return;
        }
        self.lanzar_importacion(rutas.to_vec(), destino);
    }

    /// Lanza un lote en segundo plano. El hilo ejecuta el mismo bucle que
    /// la versión síncrona (un error no detiene el resto) y solo envía el
    /// resultado por el canal; nunca toca el panel.
    fn lanzar_importacion(&mut self, rutas: Vec<PathBuf>, destino: PathBuf) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.importando = nombres_de(&rutas);
        let lanzado = std::thread::Builder::new()
            .name("glass-importar-soltados".into())
            .spawn(move || {
                let mut importados: Vec<PathBuf> = Vec::new();
                let mut errores: Vec<String> = Vec::new();
                for ruta in &rutas {
                    match importar_archivo(ruta, &destino) {
                        Ok(destino) => importados.push(destino),
                        Err(e) => errores.push(e),
                    }
                }
                // Si el panel ya no existe, el envío falla y no importa.
                let _ = tx.send((importados, errores));
            });
        match lanzado {
            Ok(_) => {
                self.import_rx = Some(rx);
                let msg = format!("Importando {}…", describir_lote(&self.importando));
                self.set_info(msg);
            }
            Err(e) => {
                self.importando.clear();
                self.set_error(format!("No se pudo lanzar la importación: {e}"));
            }
        }
    }

    /// Recoge, sin bloquear, el resultado del lote en curso y lo aplica con
    /// la misma semántica que tenía la importación síncrona. Si el hilo
    /// murió sin enviar nada, limpia el estado y lo anota como error. Al
    /// terminar un lote, lanza el siguiente de la cola (con su destino).
    fn recoger_importacion(&mut self) {
        use std::sync::mpsc::TryRecvError;
        let resultado = match self.import_rx.as_ref() {
            None => return,
            Some(rx) => match rx.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => None,
            },
        };
        self.import_rx = None;
        self.importando.clear();

        match resultado {
            None => {
                self.set_error("La importación se interrumpió".to_string());
            }
            Some((importados, errores)) => {
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
        }
        // La sonda de main.rs mide desde el fin de la operación, también
        // cuando el hilo murió sin entregar resultado.
        self.importacion_completada_en = Some(std::time::Instant::now());

        if !self.importaciones_encoladas.is_empty() {
            let (rutas, destino) = self.importaciones_encoladas.remove(0);
            self.lanzar_importacion(rutas, destino);
        }
    }

    /// Entra en una carpeta, sin poder escapar de `assets/`.
    fn entrar_en(&mut self, dir: PathBuf) {
        if !dir.starts_with(&self.assets_root) {
            self.set_error(format!("Ruta fuera de assets/: {}", dir.display()));
            return;
        }
        self.current_dir = dir;
        self.vista_rapida = None;
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
        self.vista_rapida = None;
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
        // Antes de mover a la papelera (después la ruta ya no existe): si
        // es un Physics Material, el aviso de éxito advierte de las
        // referencias rotas (§6.1). El panel no tiene diálogo de
        // confirmación: el borrado va a la papelera, recuperable.
        let era_physmat = ruta
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(glass_core::physics_material::es_physmat);
        if rutas_iguales(&ruta, &self.assets_root) {
            self.set_error("No se puede eliminar la raíz de assets/");
            return;
        }

        let gemelos = gemelos_convertidos(&ruta);

        match mover_a_papelera(&self.assets_root, &ruta) {
            Ok(destino) => {
                let mut fallos = Vec::new();

                for gemelo in gemelos {
                    if let Err(error) = mover_a_papelera(&self.assets_root, &gemelo) {
                        fallos.push(format!("{}: {error}", gemelo.display()));
                    }
                }

                self.seleccion = None;
                self.refrescar();

                if fallos.is_empty() {
                    let mut mensaje = format!("Movido a la papelera: {}", destino.display());
                    if era_physmat {
                        mensaje.push_str(
                            ". Los colliders que referencien este asset conservarán \
                             una referencia rota y usarán el material de reserva",
                        );
                    }
                    self.set_info(mensaje);
                } else {
                    self.set_error(format!(
                        "Movido a la papelera: {}. El elemento principal se movió, pero algún archivo generado quedó sin mover: {}",
                        destino.display(),
                        fallos.join("; ")
                    ));
                }
            }
            Err(e) => self.set_error(e),
        }
    }

    fn duplicar(&mut self, ruta: PathBuf) {
        if rutas_iguales(&ruta, &self.assets_root) {
            self.set_error("No se puede duplicar la raíz de assets/");
            return;
        }

        match duplicar_elemento(&ruta) {
            Ok(ruta_nueva) => {
                self.seleccion = Some(ruta_nueva.clone());
                self.refrescar();
                self.set_info(format!("Duplicado: {}", ruta_nueva.display()));
            }
            Err(e) => self.set_error(e),
        }
    }

    /// Borra (a la papelera) el asset seleccionado, si hay selección.
    /// Es la vía de la tecla Supr enrutada desde el docking.
    pub fn eliminar_seleccion(&mut self) {
        if let Some(ruta) = self.seleccion.clone() {
            self.eliminar(ruta);
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
            TipoCreacion::PhysicsMaterial => match destino {
                Some(dir) => crear_physics_material_en(&dir, &nombre),
                None => crear_physics_material(&self.assets_root, &nombre),
            },
            TipoCreacion::Prefab => match destino {
                Some(dir) => crear_prefab_en(&dir, &nombre),
                None => crear_prefab(&self.assets_root, &nombre),
            },
        };
        match resultado {
            Ok(ruta) => {
                self.seleccion = Some(ruta.clone());
                self.refrescar();
                // El Physics Material nace con el renombrado inline activo
                // (§6.1 del diseño de colliders); los demás tipos no cambian.
                // Tras refrescar, para que la recarga del listado no anule
                // el estado de renombrado.
                if tipo == TipoCreacion::PhysicsMaterial {
                    self.iniciar_renombrado(ruta.clone());
                }
                self.set_info(format!("Creado: {}", ruta.display()));
                true
            }
            Err(e) => {
                self.set_error(e);
                false
            }
        }
    }

    /// Nombre para una creación desde menú contextual: el nombre por
    /// defecto del tipo (la antigua fila de creación con el campo
    /// «Nombre:» se retiró a petición del usuario el 2026-11-18; el
    /// asset se renombra después desde el mismo menú contextual).
    fn nombre_para_creacion(&self, tipo: TipoCreacion) -> String {
        tipo.nombre_por_defecto().to_string()
    }

    // --- dibujo --------------------------------------------------------------

    pub fn render(&mut self, ui: &mut Ui) {
        self.comprobar_cambios_externos();
        self.recoger_importacion();
        if self.import_rx.is_some() {
            // Sin entrada del usuario egui no repinta: hay que sondear el
            // canal aunque la banda no llegue a dibujarse.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.rect_ultimo = ui.max_rect();
        // --- Barra superior ---------------------------------------------------
        let en_raiz = self.en_raiz();
        let mut pedir_refresco = false;
        let mut pedir_subir = false;
        let mut navegar_cabecera: Option<PathBuf> = None;
        ui.horizontal(|ui| {
            ui.heading("📁 Proyecto / Assets");
            ui.separator();
            ui.label("Ruta:");
            let migas_cabecera = migas(&self.assets_root, &self.current_dir);
            let total_migas = migas_cabecera.len();
            for (i, (etiqueta, ruta)) in migas_cabecera.into_iter().enumerate() {
                if ui.small_button(etiqueta).clicked() {
                    navegar_cabecera = Some(ruta);
                }
                if i + 1 < total_migas {
                    ui.label("›");
                }
            }
            if let Some(tipo) = self.vista_rapida {
                ui.label("›");
                ui.label(egui::RichText::new(tipo.etiqueta()).strong());
            }
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
        if let Some(dir) = navegar_cabecera {
            self.current_dir = dir;
            self.vista_rapida = None;
            self.seleccion = None;
            self.refrescar();
        }


        // --- Buscador ---------------------------------------------------------
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(
                egui::TextEdit::singleline(&mut self.busqueda)
                    .hint_text("Buscar en esta carpeta…")
                    .desired_width(220.0),
            );
            if !self.busqueda.is_empty() && ui.small_button("✖").clicked() {
                self.busqueda.clear();
            }
        });

        // --- Dos zonas: árbol de carpetas (izquierda) + lista (derecha) -------
        // Columnas con layout ordinario, SIN egui::Panel/CentralPanel: dentro
        // de una pestaña del dock, esos paneles acaban robando la primera
        // fila del contenido (la cabecera queda bajo la barra de pestañas).
        // Verificado en vivo el 2026-11-17.
        // La barra inferior (seleccionado + slider) reserva 28 px. La altura
        // se mide contra el fondo del área visible y el cursor actual:
        // `available_height()` aquí devuelve la altura total de la hoja del
        // dock sin descontar lo ya dibujado, y la barra quedaba fuera de la
        // vista en paneles cortos (comprobado en vivo).
        let altura_zonas = (ui.clip_rect().bottom() - ui.cursor().top() - 26.0).max(8.0);
        let mut navegar_arbol: Option<PathBuf> = None;
        let mut vista_click: Option<TipoRapido> = None;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(180.0, altura_zonas),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    // Favoritos y árbol comparten UNA ScrollArea: fuera de
                    // ella, las etiquetas de Favoritos se desbordaban de la
                    // asignación de altura y empujaban la barra inferior
                    // fuera de la hoja en paneles cortos (comprobado en vivo).
                    egui::ScrollArea::vertical()
                        .id_salt("arbol_carpetas_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("⭐ Favoritos").strong());
                            for tipo in TipoRapido::TODOS {
                                let activa = self.vista_rapida == Some(tipo);
                                if ui.selectable_label(activa, tipo.etiqueta()).clicked() {
                                    vista_click = Some(tipo);
                                }
                            }
                            ui.separator();
                            let raiz = self.assets_root.clone();
                            for dir in self.arbol_carpetas.clone() {
                                let profundidad = dir
                                    .strip_prefix(&raiz)
                                    .map(|r| r.components().count())
                                    .unwrap_or(0);
                                let nombre = if profundidad == 0 {
                                    raiz.file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| "assets".to_string())
                                } else {
                                    dir.file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default()
                                };
                                ui.horizontal(|ui| {
                                    ui.add_space(profundidad as f32 * 14.0);
                                    let activa = rutas_iguales(&dir, &self.current_dir);
                                    if ui
                                        .selectable_label(activa, format!("📁 {nombre}"))
                                        .clicked()
                                    {
                                        navegar_arbol = Some(dir.clone());
                                    }
                                });
                            }
                        });
                },
            );
            ui.separator();
            // La columna derecha también va en una asignación de altura
            // fija: dentro de un `vertical` sin cota, su ScrollArea reclamaba
            // más altura de la que quedaba y empujaba la barra inferior
            // fuera de la hoja (comprobado en vivo).
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), altura_zonas),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
        // --- Banda de progreso transitoria (importación / conversión) ---
        // Solo existe mientras hay trabajo en curso: sin trabajo no ocupa
        // nada. El Spinner pide repintado por sí mismo.
        {
            let debil = ui.visuals().weak_text_color();
            let texto_importando = if self.importando.is_empty() {
                None
            } else {
                Some(format!("Importando {}…", describir_lote(&self.importando)))
            };
            let convirtiendo = nombres_convirtiendo(
                &self.entradas,
                &self.conversiones_en_curso,
                &self.current_dir,
            );
            let texto_convirtiendo = if convirtiendo.is_empty() {
                None
            } else {
                Some(format!("Convirtiendo {}…", describir_lote(&convirtiendo)))
            };
            for texto in [texto_importando, texto_convirtiendo].into_iter().flatten() {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    ui.label(egui::RichText::new(texto).size(12.5).color(debil));
                });
            }
        }

        if self.vista_rapida.is_some() {
            self.render_vista_rapida(ui);
        } else if self.tamano_iconos > 0.01 {
            self.render_rejilla(ui);
        } else {
        // --- Lista de assets --------------------------------------------------
        // La ventana visible de la columna: el fondo clicable se limita a
        // ella. Con `ui.clip_rect()` el fondo se extendía a todo el ancho
        // de la hoja (el clip heredado no se estrecha a la columna) y,
        // registrado después que la columna izquierda, le robaba los clics
        // a favoritos y árbol (comprobado en vivo).
        let viewport_lista = ui.max_rect();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Fondo clicable de la zona visible. Se registra ANTES que los
                // elementos para que estos queden encima y conserven su propio
                // menú contextual; el clic derecho en hueco vacío cae aquí.
                let fondo = ui.interact(
                    viewport_lista,
                    ui.make_persistent_id("fondo_lista_assets"),
                    egui::Sense::click(),
                );
                fondo.context_menu(|ui| {
                    if let Some(tipo) = submenu_crear(ui) {
                        let nombre = self.nombre_para_creacion(tipo);
                        let destino = self.current_dir.clone();
                        self.ejecutar_creacion_con(tipo, nombre, Some(destino));
                    }
                    ui.separator();
                    if ui.button("📥 Importar asset…").clicked() {
                        ui.close();
                        if let Some(origen) = rfd::FileDialog::new()
                            .set_title("Importar asset")
                            .pick_file()
                        {
                            match importar_archivo(&origen, &self.current_dir.clone()) {
                                Ok(destino) => {
                                    self.seleccion = Some(destino.clone());
                                    self.refrescar();
                                    self.set_info(format!("Importado: {}", destino.display()));
                                }
                                Err(e) => self.set_error(e),
                            }
                        }
                    }
                    if ui.button("↻ Refrescar").clicked() {
                        self.refrescar();
                        ui.close();
                    }
                });
                if self.entradas.is_empty() {
                    ui.label(egui::RichText::new("(carpeta vacía)").italics().weak());
                }

                let mut entradas = self.entradas.clone();
                if !self.busqueda.is_empty() {
                    let q = self.busqueda.to_lowercase();
                    entradas.retain(|e| e.nombre.to_lowercase().contains(&q));
                }

                // Acciones diferidas: durante el dibujo sólo se leen campos.
                let mut seleccionar: Option<PathBuf> = None;
                let mut entrar: Option<PathBuf> = None;
                let mut abrir_escena: Option<PathBuf> = None;
                let mut instanciar_prefab: Option<PathBuf> = None;
                let mut info: Option<String> = None;
                let mut empezar_renombrado: Option<PathBuf> = None;
                let mut eliminar: Option<PathBuf> = None;
                let mut duplicar: Option<PathBuf> = None;
                let mut crear_contextual: Option<(TipoCreacion, PathBuf)> = None;
                let mut accion_mover: Option<(PathBuf, PathBuf)> = None;
                let mut abrir_explorador: Option<PathBuf> = None;
                let mut reconvertir: Option<PathBuf> = None;
                let mut importar = false;
                let mut refrescar_pedido = false;
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
                        if ui.button("📄 Duplicar").clicked() {
                            duplicar = Some(ruta.clone());
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

                        if ui.button("📂 Abrir en el explorador").clicked() {
                            abrir_explorador = Some(ruta.clone());
                            ui.close();
                        }
                        if ui.button("📋 Copiar ruta").clicked() {
                            ui.ctx().copy_text(ruta.to_string_lossy().into_owned());
                            info = Some("Ruta copiada al portapapeles".to_string());
                            ui.close();
                        }
                        let nombre_min = nombre.to_lowercase();
                        if (nombre_min.ends_with(".blend") || nombre_min.ends_with(".fbx"))
                            && ui.button("🔄 Reconvertir").clicked()
                        {
                            reconvertir = Some(ruta.clone());
                            ui.close();
                        }
                        if ui.button("📥 Importar asset…").clicked() {
                            importar = true;
                            ui.close();
                        }
                        if ui.button("↻ Refrescar").clicked() {
                            refrescar_pedido = true;
                            ui.close();
                        }
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
                if let Some(r) = duplicar {
                    self.duplicar(r);
                }
                if let Some(r) = eliminar {
                    self.eliminar(r);
                }
                if let Some((item, destino)) = accion_mover {
                    self.ejecutar_movimiento(item, destino);
                }
                if let Some(r) = abrir_explorador {
                    glass_launcher::archivos::abrir_en_explorador(&r);
                }
                if let Some(r) = reconvertir {
                    self.pending_conversion = Some(r.clone());
                    self.set_info(format!("Conversión solicitada: {}", r.display()));
                }
                if importar {
                    if let Some(origen) = rfd::FileDialog::new()
                        .set_title("Importar asset")
                        .pick_file()
                    {
                        match importar_archivo(&origen, &self.current_dir.clone()) {
                            Ok(destino) => {
                                self.seleccion = Some(destino.clone());
                                self.refrescar();
                                self.set_info(format!("Importado: {}", destino.display()));
                            }
                            Err(e) => self.set_error(e),
                        }
                    }
                }
                if refrescar_pedido {
                    self.refrescar();
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
            });
        });
        if let Some(dir) = navegar_arbol {
            self.current_dir = dir;
            self.vista_rapida = None;
            self.seleccion = None;
            self.refrescar();
        }
        if let Some(tipo) = vista_click {
            self.vista_rapida = Some(tipo);
            self.seleccion = None;
            self.refrescar();
        }

        // --- Barra inferior: seleccionado + slider de tamaño -----------------
        // Se pinta en un rect absoluto al fondo de la hoja, con fondo opaco
        // y DESPUÉS de las zonas: en hojas cortas el espacio restante es
        // menor que la altura mínima que egui concede a las columnas
        // (~64 px) y su contenido se desborda hacia abajo; la barra queda
        // por encima como una barra de estado (comprobado en vivo).
        let clip = ui.clip_rect();
        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(clip.left() + 6.0, clip.bottom() - 24.0),
            egui::vec2(clip.width() - 12.0, 24.0),
        );
        let pintor = ui.painter();
        pintor.rect_filled(bar_rect, 0.0, ui.visuals().panel_fill);
        pintor.line_segment(
            [bar_rect.left_top(), bar_rect.right_top()],
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );
        let mut bar_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar_rect.shrink2(egui::vec2(2.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        match self.seleccion.clone() {
            Some(ruta) => {
                let nombre = ruta
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                bar_ui.label(format!(
                    "{} {} — {}",
                    icono_para(&nombre, ruta.is_dir()),
                    nombre,
                    ruta_legible(&self.assets_root, &ruta)
                ));
            }
            None => {
                let texto = match self.vista_rapida {
                    Some(tipo) => format!(
                        "{} · {} elementos",
                        tipo.etiqueta(),
                        self.entradas_rapidas.len()
                    ),
                    None => self.ruta_actual_legible(),
                };
                bar_ui.label(texto);
            }
        }
        bar_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Slider::new(&mut self.tamano_iconos, 0.0..=1.0).show_value(false),
            );
            ui.label("Tamaño:");
        });
    }

    /// Obtiene (con caché) la textura de miniatura de una imagen; `None` si
    /// el archivo no es una imagen legible.
    fn miniatura(&mut self, ui: &egui::Ui, ruta: &Path) -> Option<egui::TextureHandle> {
        if !self.miniaturas.contains_key(ruta) {
            let textura = std::fs::read(ruta).ok().and_then(|bytes| {
                image::load_from_memory(&bytes).ok().map(|img| {
                    let img = img.thumbnail(128, 128);
                    let rgba = img.to_rgba8();
                    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
                    let color =
                        egui::ColorImage::from_rgba_unmultiplied([w, h], rgba.as_raw());
                    ui.ctx().load_texture(
                        format!("asset_thumb_{}", ruta.display()),
                        color,
                        egui::TextureOptions::LINEAR,
                    )
                })
            });
            self.miniaturas.insert(ruta.to_path_buf(), textura);
        }
        self.miniaturas.get(ruta).cloned().flatten()
    }

    /// Obtiene (con caché) la miniatura renderizada de un modelo 3D;
    /// `None` si no hay .glb disponible, no se pudo cargar o el
    /// render salió vacío (todo transparente). La caché se invalida
    /// cuando cambia el mtime del .glb fuente (un .blend cuyo .glb
    /// aparece tras la conversión, o un modelo sustituido, refrescan
    /// su miniatura). La generación está racionada por frame
    /// (`presupuesto_miniaturas_modelo`): sin presupuesto se devuelve
    /// lo cacheado aunque esté caducado, y se rehace en otro frame.
    fn miniatura_modelo(&mut self, ui: &egui::Ui, ruta: &Path) -> Option<egui::TextureHandle> {
        let fuente = ruta_glb_miniatura(ruta);
        let mtime = fuente
            .as_ref()
            .and_then(|f| std::fs::metadata(f).ok())
            .and_then(|m| m.modified().ok());
        if let Some((tex, m)) = self.miniaturas_modelo.get(ruta) {
            if *m == mtime {
                return tex.clone();
            }
        }
        if self.presupuesto_miniaturas_modelo == 0 {
            return self
                .miniaturas_modelo
                .get(ruta)
                .and_then(|(t, _)| t.clone());
        }
        self.presupuesto_miniaturas_modelo -= 1;
        let textura = fuente
            .and_then(|glb| std::fs::read(glb).ok())
            .and_then(|bytes| {
                glass_renderer::MeshLoader::load_model_from_glb_bytes(&bytes).ok()
            })
            .and_then(|modelo| {
                let rgba = crate::panels::miniatura_modelo::renderizar_miniatura_modelo(
                    &modelo, 192,
                );
                if rgba.iter().skip(3).step_by(4).all(|&a| a == 0) {
                    None
                } else {
                    let color = egui::ColorImage::from_rgba_unmultiplied([192, 192], &rgba);
                    Some(ui.ctx().load_texture(
                        format!("asset_model_thumb_{}", ruta.display()),
                        color,
                        egui::TextureOptions::LINEAR,
                    ))
                }
            });
        self.miniaturas_modelo
            .insert(ruta.to_path_buf(), (textura.clone(), mtime));
        textura
    }

    /// Obtiene (con caché) el color `albedo` de un `.material.json`; `None`
    /// si no se puede leer o parsear.
    fn color_material(&mut self, ruta: &Path) -> Option<egui::Color32> {
        if !self.colores_material.contains_key(ruta) {
            let color = std::fs::read_to_string(ruta).ok().and_then(|texto| {
                serde_json::from_str::<serde_json::Value>(&texto)
                    .ok()
                    .map(|valor| {
                        let albedo = valor
                            .get("albedo")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                let get = |i: usize| {
                                    arr.get(i).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32
                                };
                                [get(0), get(1), get(2), get(3)]
                            })
                            .unwrap_or([1.0, 1.0, 1.0, 1.0]);
                        egui::Color32::from_rgba_unmultiplied(
                            (albedo[0] * 255.0) as u8,
                            (albedo[1] * 255.0) as u8,
                            (albedo[2] * 255.0) as u8,
                            (albedo[3] * 255.0) as u8,
                        )
                    })
            });
            self.colores_material.insert(ruta.to_path_buf(), color);
        }
        self.colores_material.get(ruta).cloned().flatten()
    }

    /// Renderiza la vista rápida de Favoritos: resultados de búsqueda
    /// recursiva por tipo, con el mismo repertorio de interacciones (menor)
    /// que la lista normal.
    /// Publica la carga útil del arrastre en curso y dibuja su fantasma
    /// junto al cursor. Es el mismo bloque que la lista aplica en línea;
    /// la vista rápida y la rejilla lo comparten desde aquí.
    fn publicar_fantasma_arrastre(&mut self, ui: &mut Ui) {
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
                        egui::Color32::from_rgba_unmultiplied(sel.r(), sel.g(), sel.b(), 225),
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
    }

    fn render_vista_rapida(&mut self, ui: &mut Ui) {
        let filtro = self.busqueda.trim().to_lowercase();
        let entradas: Vec<PathBuf> = self
            .entradas_rapidas
            .iter()
            .filter(|ruta| {
                if filtro.is_empty() {
                    return true;
                }
                ruta.file_name()
                    .map(|n| n.to_string_lossy().to_lowercase().contains(&filtro))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        let mut a_seleccionar: Option<PathBuf> = None;
        let mut abrir_escena: Option<PathBuf> = None;
        let mut instanciar_prefab: Option<PathBuf> = None;
        let mut info: Option<String> = None;
        let mut empezar_renombrado: Option<PathBuf> = None;
        let mut a_eliminar: Option<PathBuf> = None;
        let mut a_duplicar: Option<PathBuf> = None;
        let mut abrir_explorador: Option<PathBuf> = None;
        let mut nuevo_arrastre: Option<PathBuf> = None;
        let mut confirmar: bool = false;
        let mut cancelar: bool = false;

        egui::ScrollArea::vertical()
            .id_salt("vista_rapida_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if entradas.is_empty() {
                    ui.label(egui::RichText::new("(sin resultados)").weak().italics());
                    return;
                }
                for ruta in &entradas {
                    let nombre = ruta
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let padre = ruta.parent().unwrap_or(ruta);

                    if self.rename_target.as_deref() == Some(ruta.as_path()) {
                        ui.horizontal(|ui| {
                            let resp = ui.text_edit_singleline(&mut self.rename_buffer);
                            resp.request_focus();
                            if resp.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                confirmar = true;
                            }
                            if ui.button("✔").clicked() {
                                confirmar = true;
                            }
                            if ui.button("✖").clicked() {
                                cancelar = true;
                            }
                        });
                        continue;
                    }

                    let seleccionada = self
                        .seleccion
                        .as_ref()
                        .map(|s| rutas_iguales(s, ruta))
                        .unwrap_or(false);
                    let icono = icono_para(&nombre, false);

                    ui.horizontal(|ui| {
                        let resp =
                            ui.selectable_label(seleccionada, format!("{icono} {nombre}"));
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    egui::RichText::new(ruta_legible(&self.assets_root, padre))
                                        .weak(),
                                );
                            },
                        );

                        if resp.clicked() {
                            a_seleccionar = Some(ruta.clone());
                        }
                        if resp.double_clicked() {
                            if es_escena(&nombre) {
                                abrir_escena = Some(ruta.clone());
                            } else if es_prefab(&nombre) {
                                instanciar_prefab = Some(ruta.clone());
                            } else {
                                info = Some(format!("Archivo seleccionado: {}", ruta.display()));
                            }
                        }

                        // Mismo patrón probado de la lista: origen de la
                        // pulsación dentro de la fila + arrastre decidido.
                        let origen_en_fila = ui
                            .input(|i| i.pointer.press_origin())
                            .is_some_and(|o| resp.rect.contains(o));
                        if es_instanciable(&nombre, false)
                            && origen_en_fila
                            && ui.input(|i| i.pointer.is_decidedly_dragging())
                        {
                            nuevo_arrastre = Some(ruta.clone());
                        }

                        resp.context_menu(|ui| {
                            ui.label(egui::RichText::new(&nombre).strong());
                            ui.separator();
                            if ui.button("✏ Renombrar").clicked() {
                                empezar_renombrado = Some(ruta.clone());
                                ui.close();
                            }
                            if ui.button("📄 Duplicar").clicked() {
                                a_duplicar = Some(ruta.clone());
                                ui.close();
                            }
                            if ui.button("📂 Abrir en el explorador").clicked() {
                                abrir_explorador = Some(ruta.clone());
                                ui.close();
                            }
                            if ui.button("📋 Copiar ruta").clicked() {
                                ui.ctx().copy_text(ruta.to_string_lossy().into_owned());
                                info = Some("Ruta copiada al portapapeles".to_string());
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("🗑 Eliminar").clicked() {
                                a_eliminar = Some(ruta.clone());
                                ui.close();
                            }
                        });
                    });
                }
            });

        if confirmar {
            self.confirmar_renombrado();
        }
        if cancelar {
            self.rename_target = None;
        }
        if let Some(ruta) = a_seleccionar {
            self.seleccion = Some(ruta);
        }
        if let Some(ruta) = abrir_escena {
            self.seleccion = Some(ruta.clone());
            self.set_info(format!("Carga de escena encolada: {}", ruta.display()));
            self.pending_scene_load = Some(ruta);
        }
        if let Some(ruta) = instanciar_prefab {
            self.seleccion = Some(ruta.clone());
            self.set_info(format!(
                "Instanciación de prefab encolada: {}",
                ruta.display()
            ));
            self.pending_prefab_instantiate = Some(ruta);
        }
        if let Some(ruta) = empezar_renombrado {
            self.iniciar_renombrado(ruta);
        }
        if let Some(ruta) = a_duplicar {
            self.duplicar(ruta);
        }
        if let Some(ruta) = a_eliminar {
            self.eliminar(ruta);
        }
        if let Some(ruta) = abrir_explorador {
            glass_launcher::archivos::abrir_en_explorador(&ruta);
        }
        if let Some(ruta) = nuevo_arrastre {
            self.arrastre_asset = Some(ruta);
        }
        if let Some(texto) = info {
            self.set_info(texto);
        }
        self.publicar_fantasma_arrastre(ui);
    }

    /// Renderiza la zona derecha como rejilla de miniaturas cuando el
    /// slider de tamaño de iconos está por encima del mínimo.
    fn render_rejilla(&mut self, ui: &mut Ui) {
        // Presupuesto por frame para generar miniaturas de modelos
        // nuevas (carga + rasterizado síncronos; ver miniatura_modelo).
        self.presupuesto_miniaturas_modelo = 2;
        let celda = 40.0 + self.tamano_iconos * 88.0;
        let entradas = self.entradas.clone();
        let filtro = self.busqueda.trim().to_lowercase();
        let entradas: Vec<Entrada> = entradas
            .into_iter()
            .filter(|e| filtro.is_empty() || e.nombre.to_lowercase().contains(&filtro))
            .collect();

        let mut navegar: Option<PathBuf> = None;
        let mut a_seleccionar: Option<PathBuf> = None;
        let mut abrir_escena: Option<PathBuf> = None;
        let mut instanciar_prefab: Option<PathBuf> = None;
        let mut info: Option<String> = None;
        let mut empezar_renombrado: Option<PathBuf> = None;
        let mut a_eliminar: Option<PathBuf> = None;
        let mut a_duplicar: Option<PathBuf> = None;
        let mut abrir_explorador: Option<PathBuf> = None;
        let mut nuevo_arrastre: Option<PathBuf> = None;
        let mut crear_contextual: Option<(TipoCreacion, PathBuf)> = None;
        let mut accion_mover: Option<(PathBuf, PathBuf)> = None;
        let mut reconvertir: Option<PathBuf> = None;
        let mut importar: bool = false;
        let mut refrescar_click: bool = false;
        let mut confirmar: bool = false;
        let mut cancelar: bool = false;

        // La ventana visible de la columna, para limitar el fondo como en
        // la lista (con el clip heredado robaba los clics de la izquierda).
        let viewport_rejilla = ui.max_rect();
        egui::ScrollArea::vertical()
            .id_salt("rejilla_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Fondo clicable como el de la lista: se registra ANTES que
                // las celdas para que conserven su propio menú contextual.
                let fondo = ui.interact(
                    viewport_rejilla,
                    ui.make_persistent_id("fondo_rejilla_assets"),
                    egui::Sense::click(),
                );
                fondo.context_menu(|ui| {
                    if let Some(tipo) = submenu_crear(ui) {
                        crear_contextual = Some((tipo, self.current_dir.clone()));
                    }
                    ui.separator();
                    if ui.button("📥 Importar asset…").clicked() {
                        importar = true;
                        ui.close();
                    }
                    if ui.button("↻ Refrescar").clicked() {
                        refrescar_click = true;
                        ui.close();
                    }
                });

                if entradas.is_empty() {
                    ui.label(egui::RichText::new("(carpeta vacía)").weak().italics());
                    return;
                }

                for entrada in &entradas {
                    let ruta = self.current_dir.join(&entrada.nombre);
                    if self.rename_target.as_deref() == Some(ruta.as_path()) {
                        ui.horizontal(|ui| {
                            let resp = ui.text_edit_singleline(&mut self.rename_buffer);
                            resp.request_focus();
                            if resp.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                confirmar = true;
                            }
                            if ui.button("✔").clicked() {
                                confirmar = true;
                            }
                            if ui.button("✖").clicked() {
                                cancelar = true;
                            }
                        });
                    }
                }

                ui.horizontal_wrapped(|ui| {
                    for entrada in &entradas {
                        let ruta = self.current_dir.join(&entrada.nombre);
                        if self.rename_target.as_deref() == Some(ruta.as_path()) {
                            continue;
                        }
                        let nombre = entrada.nombre.clone();
                        let es_dir = entrada.es_dir;
                        let seleccionada = self
                            .seleccion
                            .as_ref()
                            .map(|s| rutas_iguales(s, &ruta))
                            .unwrap_or(false);

                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(celda, celda + 24.0),
                            egui::Sense::click(),
                        );
                        if ui.is_rect_visible(rect) {
                            if seleccionada {
                                ui.painter().rect_filled(
                                    rect,
                                    4.0,
                                    ui.visuals().selection.bg_fill,
                                );
                            }
                            let zona =
                                egui::Rect::from_min_size(rect.min, egui::vec2(celda, celda))
                                    .shrink(4.0);
                            let es_imagen = nombre.to_lowercase().ends_with(".png")
                                || nombre.to_lowercase().ends_with(".jpg")
                                || nombre.to_lowercase().ends_with(".jpeg");
                            let es_material = nombre.ends_with(".material.json");
                            let es_modelo_3d = es_modelo(&nombre);
                            let text_color = ui.visuals().text_color();

                            if !es_dir && es_imagen {
                                if let Some(tex) = self.miniatura(ui, &ruta) {
                                    ui.painter().image(
                                        tex.id(),
                                        zona,
                                        egui::Rect::from_min_max(
                                            egui::pos2(0.0, 0.0),
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        egui::Color32::WHITE,
                                    );
                                } else {
                                    ui.painter().text(
                                        zona.center(),
                                        egui::Align2::CENTER_CENTER,
                                        icono_para(&nombre, es_dir),
                                        egui::FontId::proportional(celda * 0.45),
                                        text_color,
                                    );
                                }
                            } else if !es_dir && es_modelo_3d {
                                if let Some(tex) = self.miniatura_modelo(ui, &ruta) {
                                    ui.painter().image(
                                        tex.id(),
                                        zona,
                                        egui::Rect::from_min_max(
                                            egui::pos2(0.0, 0.0),
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        egui::Color32::WHITE,
                                    );
                                } else {
                                    ui.painter().text(
                                        zona.center(),
                                        egui::Align2::CENTER_CENTER,
                                        icono_para(&nombre, es_dir),
                                        egui::FontId::proportional(celda * 0.45),
                                        text_color,
                                    );
                                }
                            } else if !es_dir && es_material {
                                if let Some(color) = self.color_material(&ruta) {
                                    ui.painter().rect_filled(zona, 6.0, color);
                                    ui.painter().rect_stroke(
                                        zona,
                                        6.0,
                                        egui::Stroke::new(1.0, egui::Color32::GRAY),
                                        egui::StrokeKind::Outside,
                                    );
                                } else {
                                    ui.painter().text(
                                        zona.center(),
                                        egui::Align2::CENTER_CENTER,
                                        icono_para(&nombre, es_dir),
                                        egui::FontId::proportional(celda * 0.45),
                                        text_color,
                                    );
                                }
                            } else {
                                ui.painter().text(
                                    zona.center(),
                                    egui::Align2::CENTER_CENTER,
                                    icono_para(&nombre, es_dir),
                                    egui::FontId::proportional(celda * 0.45),
                                    text_color,
                                );
                            }

                            let max_chars = (celda / 6.5) as usize;
                            let nombre_corto = if nombre.chars().count() > max_chars
                                && max_chars > 1
                            {
                                let recorte: String =
                                    nombre.chars().take(max_chars - 1).collect();
                                format!("{recorte}…")
                            } else {
                                nombre.clone()
                            };
                            ui.painter().text(
                                rect.min + egui::vec2(celda / 2.0, celda + 4.0),
                                egui::Align2::CENTER_TOP,
                                nombre_corto,
                                egui::FontId::proportional(11.0),
                                text_color,
                            );
                        }

                        let pista = if es_dir {
                            format!("Carpeta: {}", ruta.display())
                        } else {
                            format!("{} — {} bytes", ruta.display(), entrada.bytes)
                        };
                        let resp = resp.on_hover_text(pista);

                        if resp.clicked() {
                            a_seleccionar = Some(ruta.clone());
                        }
                        if resp.double_clicked() {
                            if es_dir {
                                navegar = Some(ruta.clone());
                            } else if es_escena(&nombre) {
                                abrir_escena = Some(ruta.clone());
                            } else if es_prefab(&nombre) {
                                instanciar_prefab = Some(ruta.clone());
                            } else {
                                info = Some(format!("Archivo seleccionado: {}", ruta.display()));
                            }
                        }

                        let origen_en_celda = ui
                            .input(|i| i.pointer.press_origin())
                            .is_some_and(|o| resp.rect.contains(o));
                        if es_instanciable(&nombre, es_dir)
                            && origen_en_celda
                            && ui.input(|i| i.pointer.is_decidedly_dragging())
                        {
                            nuevo_arrastre = Some(ruta.clone());
                        }

                        resp.context_menu(|ui| {
                            ui.label(egui::RichText::new(&nombre).strong());
                            ui.separator();

                            if let Some(tipo) = submenu_crear(ui) {
                                let destino = if es_dir {
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
                            if ui.button("📄 Duplicar").clicked() {
                                a_duplicar = Some(ruta.clone());
                                ui.close();
                            }

                            ui.menu_button("📦 Mover a…", |ui| {
                                let destinos = listar_carpetas(&self.assets_root);
                                if destinos.is_empty() {
                                    ui.label(
                                        egui::RichText::new("(sin carpetas)")
                                            .italics()
                                            .weak(),
                                    );
                                }
                                for destino in destinos {
                                    if rutas_iguales(&destino, &self.current_dir) {
                                        continue;
                                    }
                                    if es_dir && es_ancestro_o_igual(&ruta, &destino) {
                                        continue;
                                    }
                                    let etiqueta =
                                        ruta_legible(&self.assets_root, &destino);
                                    if ui.button(etiqueta).clicked() {
                                        accion_mover = Some((ruta.clone(), destino.clone()));
                                        ui.close();
                                    }
                                }
                            });

                            if ui.button("📂 Abrir en el explorador").clicked() {
                                abrir_explorador = Some(ruta.clone());
                                ui.close();
                            }
                            if ui.button("📋 Copiar ruta").clicked() {
                                ui.ctx().copy_text(ruta.to_string_lossy().into_owned());
                                info = Some("Ruta copiada al portapapeles".to_string());
                                ui.close();
                            }
                            let nombre_min = nombre.to_lowercase();
                            if (nombre_min.ends_with(".blend")
                                || nombre_min.ends_with(".fbx"))
                                && ui.button("🔄 Reconvertir").clicked()
                            {
                                reconvertir = Some(ruta.clone());
                                ui.close();
                            }
                            if ui.button("📥 Importar asset…").clicked() {
                                importar = true;
                                ui.close();
                            }
                            if ui.button("↻ Refrescar").clicked() {
                                refrescar_click = true;
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("🗑 Eliminar").clicked() {
                                a_eliminar = Some(ruta.clone());
                                ui.close();
                            }
                        });
                    }
                });
            });

        if confirmar {
            self.confirmar_renombrado();
        }
        if cancelar {
            self.rename_target = None;
        }
        if let Some(dir) = navegar {
            self.entrar_en(dir);
        }
        if let Some(ruta) = a_seleccionar {
            self.seleccion = Some(ruta);
        }
        if let Some(ruta) = abrir_escena {
            self.seleccion = Some(ruta.clone());
            self.set_info(format!("Carga de escena encolada: {}", ruta.display()));
            self.pending_scene_load = Some(ruta);
        }
        if let Some(ruta) = instanciar_prefab {
            self.seleccion = Some(ruta.clone());
            self.set_info(format!(
                "Instanciación de prefab encolada: {}",
                ruta.display()
            ));
            self.pending_prefab_instantiate = Some(ruta);
        }
        if let Some(ruta) = empezar_renombrado {
            self.iniciar_renombrado(ruta);
        }
        if let Some(ruta) = a_duplicar {
            self.duplicar(ruta);
        }
        if let Some(ruta) = a_eliminar {
            self.eliminar(ruta);
        }
        if let Some(ruta) = abrir_explorador {
            glass_launcher::archivos::abrir_en_explorador(&ruta);
        }
        if let Some(ruta) = nuevo_arrastre {
            self.arrastre_asset = Some(ruta);
        }
        if let Some((tipo, destino)) = crear_contextual {
            let nombre = self.nombre_para_creacion(tipo);
            self.ejecutar_creacion_con(tipo, nombre, Some(destino));
        }
        if let Some((item, destino)) = accion_mover {
            self.ejecutar_movimiento(item, destino);
        }
        if let Some(ruta) = reconvertir {
            self.pending_conversion = Some(ruta.clone());
            self.set_info(format!("Conversión solicitada: {}", ruta.display()));
        }
        if importar {
            if let Some(origen) = rfd::FileDialog::new()
                .set_title("Importar asset")
                .pick_file()
            {
                match importar_archivo(&origen, &self.current_dir.clone()) {
                    Ok(destino) => {
                        self.seleccion = Some(destino.clone());
                        self.refrescar();
                        self.set_info(format!("Importado: {}", destino.display()));
                    }
                    Err(e) => self.set_error(e),
                }
            }
        }
        if refrescar_click {
            self.refrescar();
        }
        if let Some(texto) = info {
            self.set_info(texto);
        }
        self.publicar_fantasma_arrastre(ui);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[test]
    fn es_modelo_acepta_extensiones_de_modelo() {
        assert!(es_modelo("a.GLB"));
        assert!(es_modelo("a.blend"));
        assert!(es_modelo("a.fbx"));
        assert!(es_modelo("a.gltf"));
        assert!(!es_modelo("a.png"));
        assert!(!es_modelo("a.material.json"));
        assert!(!es_modelo("carpeta"));
    }

    #[test]
    fn ruta_glb_miniatura_resuelve_el_hermano() {
        let dir = std::env::temp_dir().join(format!("ge_thumb_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let blend = dir.join("m.blend");
        std::fs::write(&blend, b"blend").unwrap();
        assert_eq!(ruta_glb_miniatura(&blend), None);
        let glb = dir.join("m.glb");
        std::fs::write(&glb, b"glb").unwrap();
        assert_eq!(ruta_glb_miniatura(&glb), Some(glb.clone()));
        assert_eq!(ruta_glb_miniatura(&blend), Some(glb.clone()));
        let fbx = dir.join("o.fbx");
        std::fs::write(&fbx, b"fbx").unwrap();
        assert_eq!(ruta_glb_miniatura(&fbx), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
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

        let d1 = mover_a_papelera(&assets, &assets.join("dup.txt")).unwrap();
        assert!(!assets.join("dup.txt").exists());
        assert!(d1.exists());
        assert_eq!(std::fs::read_to_string(&d1).unwrap(), "hola");

        std::fs::write(assets.join("dup.txt"), "otra vez").unwrap();
        let d2 = mover_a_papelera(&assets, &assets.join("dup.txt")).unwrap();
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
    fn papelera_con_assets_en_mayusculas() {
        // Regresión: las plantillas crean la carpeta como «Assets» y
        // reconstruir la ruta como raíz + "assets" hacía fallar la
        // resolución canónica en Linux: ningún asset se podía borrar.
        let raiz = temp_dir_unico("papelera_mayusculas");
        let assets = raiz.join("Assets");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(assets.join("casa.txt"), "casa").unwrap();

        let destino = mover_a_papelera(&assets, &assets.join("casa.txt")).unwrap();
        assert!(!assets.join("casa.txt").exists());
        assert!(destino.exists());
        assert!(destino.starts_with(raiz.join(".project_trash")));

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

    #[test]
    fn crear_prefab_vacio_produce_un_archivo_cargable() {
        let raiz = std::env::temp_dir()
            .join(format!("glass_prefab_vacio_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let ruta = crear_prefab(&raiz, "Heroe").unwrap();
        assert!(ruta.ends_with("Heroe.prefab.json"));
        let prefab = glass_core::Prefab::load_from_file(&ruta).unwrap();
        assert_eq!(prefab.name, "Heroe");
        assert_eq!(prefab.root_entity.name, "Heroe");
        assert!(prefab.descendant_entities.is_empty());
        assert!(crear_prefab(&raiz, "Heroe").is_err(), "duplicado debe fallar");
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn migas_desde_la_raiz_hasta_la_carpeta() {
        let raiz = PathBuf::from("/p/assets");
        let actual = PathBuf::from("/p/assets/models/x");
        let resultado = migas(&raiz, &actual);
        assert_eq!(
            resultado,
            vec![
                ("assets".to_string(), PathBuf::from("/p/assets")),
                ("models".to_string(), PathBuf::from("/p/assets/models")),
                ("x".to_string(), PathBuf::from("/p/assets/models/x")),
            ]
        );

        let en_raiz = migas(&raiz, &raiz);
        assert_eq!(en_raiz, vec![("assets".to_string(), PathBuf::from("/p/assets"))]);
    }

    #[test]
    fn listar_directorio_oculta_los_meta_json() {
        let dir = std::env::temp_dir().join(format!("glass_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.glb"), b"x").unwrap();
        std::fs::write(dir.join("a.glb.meta.json"), b"{}").unwrap();
        std::fs::write(dir.join("b.txt"), b"y").unwrap();

        let entradas = listar_directorio(&dir).unwrap();
        let nombres: Vec<_> = entradas.iter().map(|e| e.nombre.clone()).collect();
        assert!(nombres.contains(&"a.glb".to_string()));
        assert!(nombres.contains(&"b.txt".to_string()));
        assert!(!nombres.contains(&"a.glb.meta.json".to_string()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vista_unica_oculta_el_glb_gemelo_de_un_fbx() {
        let dir = std::env::temp_dir().join(format!(
            "glass_test_{}_f0_gemelo_fbx",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("casa.fbx"), b"fbx").unwrap();
        std::fs::write(dir.join("casa.glb"), b"glb").unwrap();
        std::fs::write(
            dir.join("casa.glb.meta.json"),
            br#"{"source":"casa.fbx","source_sha256":"x"}"#,
        )
        .unwrap();

        let entradas = listar_directorio(&dir).unwrap();
        let nombres: Vec<_> = entradas.iter().map(|e| e.nombre.clone()).collect();
        assert!(nombres.contains(&"casa.fbx".to_string()));
        assert!(!nombres.contains(&"casa.glb".to_string()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vista_unica_oculta_el_glb_gemelo_de_un_blend() {
        let dir = std::env::temp_dir().join(format!(
            "glass_test_{}_f0_gemelo_blend",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("arbol.blend"), b"blend").unwrap();
        std::fs::write(dir.join("arbol.glb"), b"glb").unwrap();
        std::fs::write(
            dir.join("arbol.glb.meta.json"),
            br#"{"source":"arbol.blend"}"#,
        )
        .unwrap();

        let entradas = listar_directorio(&dir).unwrap();
        let nombres: Vec<_> = entradas.iter().map(|e| e.nombre.clone()).collect();
        assert!(nombres.contains(&"arbol.blend".to_string()));
        assert!(!nombres.contains(&"arbol.glb".to_string()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vista_unica_muestra_un_glb_sin_sidecar() {
        let dir = std::env::temp_dir().join(format!(
            "glass_test_{}_f0_glb_sin_sidecar",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("suelto.glb"), b"glb").unwrap();

        let entradas = listar_directorio(&dir).unwrap();
        let nombres: Vec<_> = entradas.iter().map(|e| e.nombre.clone()).collect();
        assert!(nombres.contains(&"suelto.glb".to_string()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vista_unica_muestra_el_glb_si_la_fuente_se_borro() {
        let dir = std::env::temp_dir().join(format!(
            "glass_test_{}_f0_glb_fuente_borrada",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("huerfano.glb"), b"glb").unwrap();
        std::fs::write(
            dir.join("huerfano.glb.meta.json"),
            br#"{"source":"huerfano.fbx"}"#,
        )
        .unwrap();

        let entradas = listar_directorio(&dir).unwrap();
        let nombres: Vec<_> = entradas.iter().map(|e| e.nombre.clone()).collect();
        assert!(nombres.contains(&"huerfano.glb".to_string()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vista_unica_nunca_oculta_un_edited() {
        let dir = std::env::temp_dir().join(format!(
            "glass_test_{}_f0_glb_edited",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("casa_edited.fbx"), b"fbx").unwrap();
        std::fs::write(dir.join("casa_edited.glb"), b"glb").unwrap();
        std::fs::write(
            dir.join("casa_edited.glb.meta.json"),
            br#"{"source":"casa_edited.fbx"}"#,
        )
        .unwrap();

        let entradas = listar_directorio(&dir).unwrap();
        let nombres: Vec<_> = entradas.iter().map(|e| e.nombre.clone()).collect();
        assert!(nombres.contains(&"casa_edited.fbx".to_string()));
        assert!(nombres.contains(&"casa_edited.glb".to_string()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vista_unica_muestra_el_glb_si_el_sidecar_es_de_otro() {
        let dir = std::env::temp_dir().join(format!(
            "glass_test_{}_f0_sidecar_de_otro",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.glb"), b"glb").unwrap();
        std::fs::write(dir.join("b.fbx"), b"fbx").unwrap();
        std::fs::write(
            dir.join("a.glb.meta.json"),
            br#"{"source":"b.fbx"}"#,
        )
        .unwrap();

        let entradas = listar_directorio(&dir).unwrap();
        let nombres: Vec<_> = entradas.iter().map(|e| e.nombre.clone()).collect();
        assert!(nombres.contains(&"a.glb".to_string()));
        assert!(nombres.contains(&"b.fbx".to_string()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vista_unica_muestra_el_glb_si_el_sidecar_esta_roto() {
        let dir = std::env::temp_dir().join(format!(
            "glass_test_{}_f0_sidecar_roto",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("roto.glb"), b"glb").unwrap();
        std::fs::write(dir.join("roto.fbx"), b"fbx").unwrap();
        std::fs::write(dir.join("roto.glb.meta.json"), b"no es json").unwrap();

        let entradas = listar_directorio(&dir).unwrap();
        let nombres: Vec<_> = entradas.iter().map(|e| e.nombre.clone()).collect();
        assert!(nombres.contains(&"roto.glb".to_string()));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vista_unica_el_predicado_directo() {
        let dir = std::env::temp_dir().join(format!(
            "glass_test_{}_f0_predicado_directo",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("casa.fbx"), b"fbx").unwrap();
        std::fs::write(dir.join("casa.glb"), b"glb").unwrap();
        std::fs::write(
            dir.join("casa.glb.meta.json"),
            br#"{"source":"casa.fbx"}"#,
        )
        .unwrap();

        assert!(es_glb_gemelo_oculto(&dir, "casa.glb"));
        assert!(!es_glb_gemelo_oculto(&dir, "casa.fbx"));
        assert!(!es_glb_gemelo_oculto(&dir, "casa"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn buscar_por_tipo_recursivo_y_sin_meta() {
        let raiz = dir_temporal_creacion("rapido");
        std::fs::write(raiz.join("a.material.json"), "{}").unwrap();

        let scenes = raiz.join("scenes");
        std::fs::create_dir_all(&scenes).unwrap();
        std::fs::write(scenes.join("b.scene.json"), "{}").unwrap();

        let deep = raiz.join("models").join("deep");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("c.glb"), b"glb").unwrap();
        std::fs::write(deep.join("c.glb.meta.json"), "{}").unwrap();

        assert_eq!(
            super::buscar_por_tipo(&raiz, super::TipoRapido::Escenas),
            vec![scenes.join("b.scene.json")]
        );
        assert_eq!(
            super::buscar_por_tipo(&raiz, super::TipoRapido::Materiales),
            vec![raiz.join("a.material.json")]
        );
        assert_eq!(
            super::buscar_por_tipo(&raiz, super::TipoRapido::Modelos),
            vec![deep.join("c.glb")]
        );
        assert!(super::buscar_por_tipo(&raiz, super::TipoRapido::Scripts).is_empty());

        std::fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn tipo_rapido_coincide() {
        use super::TipoRapido;
        assert!(TipoRapido::Escenas.coincide("nivel1.scene.json"));
        assert!(!TipoRapido::Escenas.coincide("textura.png"));
        assert!(TipoRapido::Materiales.coincide("rojo.material.json"));
        assert!(!TipoRapido::Materiales.coincide("rojo.json"));
        assert!(TipoRapido::Modelos.coincide("arbol.glb"));
        assert!(TipoRapido::Modelos.coincide("arbol.blend"));
        assert!(!TipoRapido::Modelos.coincide("arbol.scene.json"));
        assert!(TipoRapido::Prefabs.coincide("caja.prefab.json"));
        assert!(!TipoRapido::Prefabs.coincide("arbol.scene.json"));
        assert!(TipoRapido::Scripts.coincide("Jugador.cs"));
        assert!(!TipoRapido::Scripts.coincide("Jugador.rs"));
    }

fn proyecto_gemelos_test(nombre: &str) -> PathBuf {
    let raiz = std::env::temp_dir().join(format!(
        "glassenginer_gemelos_{nombre}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&raiz);
    std::fs::create_dir_all(raiz.join("assets")).unwrap();
    raiz
}

#[test]
fn gemelos_convertidos_devuelve_glb_y_sidecar() {
    let proyecto = proyecto_gemelos_test("completos");
    let assets = proyecto.join("assets");
    let fuente = assets.join("casa.blend");
    let glb = assets.join("casa.glb");
    let sidecar = assets.join("casa.glb.meta.json");

    std::fs::write(&fuente, b"").unwrap();
    std::fs::write(&glb, b"").unwrap();
    std::fs::write(&sidecar, b"").unwrap();

    assert_eq!(
        gemelos_convertidos(&fuente),
        vec![glb, sidecar]
    );

    std::fs::remove_dir_all(proyecto).unwrap();
}

#[test]
fn gemelos_convertidos_ignora_fuentes_no_convertibles() {
    let proyecto = proyecto_gemelos_test("no_convertible");
    let glb = proyecto.join("assets").join("casa.glb");
    std::fs::write(&glb, b"").unwrap();

    assert!(gemelos_convertidos(&glb).is_empty());

    std::fs::remove_dir_all(proyecto).unwrap();
}

#[test]
fn gemelos_convertidos_devuelve_vacio_si_no_hay_productos() {
    let proyecto = proyecto_gemelos_test("sin_productos");
    let fuente = proyecto.join("assets").join("sola.fbx");
    std::fs::write(&fuente, b"").unwrap();

    assert!(gemelos_convertidos(&fuente).is_empty());

    std::fs::remove_dir_all(proyecto).unwrap();
}

#[test]
fn gemelos_convertidos_no_incluye_sidecar_sin_glb() {
    let proyecto = proyecto_gemelos_test("sidecar_huerfano");
    let assets = proyecto.join("assets");
    let fuente = assets.join("huerfano.blend");
    let sidecar = assets.join("huerfano.glb.meta.json");

    std::fs::write(&fuente, b"").unwrap();
    std::fs::write(&sidecar, b"").unwrap();

    // El sidecar pertenece al `.glb`: sin ese archivo no se incluye.
    assert!(gemelos_convertidos(&fuente).is_empty());

    std::fs::remove_dir_all(proyecto).unwrap();
}

    #[test]
    fn firma_directorio_detecta_altas_bajas_y_cambios() {
        let dir = dir_temporal_creacion("firma_directorio");
        let archivo = dir.join("Prueba.scene.json");

        let firma_inicial = super::firma_directorio(&dir);

        std::fs::write(&archivo, "{}").expect("debe escribirse");
        let firma_alta = super::firma_directorio(&dir);
        assert_ne!(firma_alta, firma_inicial, "un archivo nuevo debe cambiar la firma");

        std::fs::write(&archivo, "{\"nombre\":\"otro tamaño\"}").expect("debe reescribirse");
        let firma_cambio = super::firma_directorio(&dir);
        assert_ne!(firma_cambio, firma_alta, "un cambio de tamaño debe cambiar la firma");

        std::fs::remove_file(&archivo).expect("debe borrarse");
        let firma_baja = super::firma_directorio(&dir);
        assert_ne!(firma_baja, firma_cambio, "borrar el archivo debe cambiar la firma");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn panel_refresca_solo_ante_un_cambio_externo() {
        let assets = dir_temporal_creacion("cambio_externo");
        let mut panel = super::ProjectPanel::new();
        panel.set_assets_root(assets.clone());
        assert!(!panel.entradas.iter().any(|e| e.nombre == "Externo.scene.json"));

        // Cambio externo: alguien escribe en el directorio visible.
        std::fs::write(assets.join("Externo.scene.json"), "{}").expect("debe escribirse");
        panel.ultima_comprobacion_externa =
            std::time::Instant::now() - std::time::Duration::from_secs(1);
        panel.comprobar_cambios_externos();
        assert!(
            panel.entradas.iter().any(|e| e.nombre == "Externo.scene.json"),
            "el archivo externo debe aparecer tras la comprobación"
        );

        // Sin cambios externos, una segunda comprobación no altera el listado.
        let antes: Vec<(String, bool, u64)> = panel
            .entradas
            .iter()
            .map(|e| (e.nombre.clone(), e.es_dir, e.bytes))
            .collect();
        let firma_antes = panel.firma_directorio_actual;
        panel.ultima_comprobacion_externa =
            std::time::Instant::now() - std::time::Duration::from_secs(1);
        panel.comprobar_cambios_externos();
        let despues: Vec<(String, bool, u64)> = panel
            .entradas
            .iter()
            .map(|e| (e.nombre.clone(), e.es_dir, e.bytes))
            .collect();
        assert_eq!(antes, despues);
        assert_eq!(panel.firma_directorio_actual, firma_antes);

        let _ = std::fs::remove_dir_all(&assets);
    }

    fn entrada(nombre: &str, es_dir: bool) -> Entrada {
        Entrada {
            nombre: nombre.to_string(),
            es_dir,
            bytes: 0,
        }
    }

    #[test]
    fn convirtiendo_coincide_con_ruta_verbatim() {
        let dir = PathBuf::from(r"C:\Proyecto\Assets\Models");
        let entradas = vec![entrada("casa.blend", false)];
        let en_curso = vec![PathBuf::from(r"\\?\C:\Proyecto\Assets\Models\casa.blend")];
        assert_eq!(
            nombres_convirtiendo(&entradas, &en_curso, &dir),
            vec!["casa.blend".to_string()]
        );
    }

    #[test]
    fn convirtiendo_ignora_mayusculas() {
        let dir = PathBuf::from("/proyecto/Assets/Models");
        let entradas = vec![entrada("Casa.FBX", false)];
        let en_curso = vec![PathBuf::from("/PROYECTO/assets/MODELS/casa.fbx")];
        assert_eq!(
            nombres_convirtiendo(&entradas, &en_curso, &dir),
            vec!["Casa.FBX".to_string()]
        );
    }

    #[test]
    fn convirtiendo_no_coincide_con_otro_nombre_ni_otra_carpeta() {
        let dir = PathBuf::from("/proyecto/Assets/Models");
        let entradas = vec![entrada("casa.blend", false)];
        let otro_nombre = vec![PathBuf::from("/proyecto/Assets/Models/arbol.blend")];
        assert!(nombres_convirtiendo(&entradas, &otro_nombre, &dir).is_empty());
        // Sufijo de nombre sin separador: «micasa.blend» no es «casa.blend».
        let prefijado = vec![PathBuf::from("/proyecto/Assets/Models/micasa.blend")];
        assert!(nombres_convirtiendo(&entradas, &prefijado, &dir).is_empty());
        let otra_carpeta = vec![PathBuf::from("/proyecto/Assets/Props/casa.blend")];
        assert!(nombres_convirtiendo(&entradas, &otra_carpeta, &dir).is_empty());
    }

    #[test]
    fn convirtiendo_ignora_carpetas_y_extensiones_no_modelo() {
        let dir = PathBuf::from("/proyecto/Assets/Models");
        let entradas = vec![
            entrada("casa.blend", true),
            entrada("casa.png", false),
        ];
        let en_curso = vec![
            PathBuf::from("/proyecto/Assets/Models/casa.blend"),
            PathBuf::from("/proyecto/Assets/Models/casa.png"),
        ];
        assert!(nombres_convirtiendo(&entradas, &en_curso, &dir).is_empty());
    }

    /// Directorio temporal único con «origen» y «Assets» dentro.
    fn dir_temporal_importacion(etiqueta: &str) -> (PathBuf, PathBuf, PathBuf) {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let raiz = std::env::temp_dir().join(format!(
            "glass_import_{etiqueta}_{}_{nanos}",
            std::process::id()
        ));
        let origen = raiz.join("origen");
        let assets = raiz.join("Assets");
        std::fs::create_dir_all(&origen).unwrap();
        std::fs::create_dir_all(&assets).unwrap();
        (raiz, origen, assets)
    }

    /// Sondea recoger_importacion() cada 10 ms hasta que no quede trabajo
    /// en curso ni en cola y se haya completado al menos un lote.
    fn esperar_importaciones(panel: &mut ProjectPanel) {
        let limite = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            panel.recoger_importacion();
            if panel.importacion_completada_en.is_some()
                && panel.import_rx.is_none()
                && panel.importaciones_encoladas.is_empty()
            {
                return;
            }
            assert!(
                std::time::Instant::now() < limite,
                "la importación no terminó a tiempo"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn importar_soltados_es_asincrono_y_selecciona() {
        let (raiz, origen, assets) = dir_temporal_importacion("async");
        let fuente = origen.join("modelo.txt");
        std::fs::write(&fuente, b"contenido").unwrap();

        let mut panel = ProjectPanel::new();
        panel.assets_root = assets.clone();
        panel.current_dir = assets.clone();
        panel.importar_soltados(std::slice::from_ref(&fuente));
        assert!(panel.importacion_completada_en.is_none());

        esperar_importaciones(&mut panel);

        let destino = assets.join("modelo.txt");
        assert!(destino.exists());
        let sel = panel.seleccion.clone().expect("debe haber selección");
        assert!(sel.exists());
        assert_eq!(sel.file_name(), destino.file_name());
        assert!(panel.importando.is_empty());

        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn importar_soltados_encola_lotes_seguidos() {
        let (raiz, origen, assets) = dir_temporal_importacion("cola");
        let a = origen.join("a.txt");
        let b = origen.join("b.txt");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();

        let mut panel = ProjectPanel::new();
        panel.assets_root = assets.clone();
        panel.current_dir = assets.clone();
        panel.importar_soltados(&[a]);
        panel.importar_soltados(&[b]);
        assert_eq!(panel.importaciones_encoladas.len(), 1);

        esperar_importaciones(&mut panel);

        assert!(assets.join("a.txt").exists());
        assert!(assets.join("b.txt").exists());

        let _ = std::fs::remove_dir_all(&raiz);
    }
    #[test]
    fn crear_physics_material_escribe_la_plantilla() {
        let root = temp_dir_unico("physics_material_plantilla");
        let destino = crear_physics_material(&root, "Hielo").unwrap();

        assert!(destino.ends_with(std::path::Path::new("materials").join("Hielo.physmat.json")));
        assert!(destino.exists());

        let json = std::fs::read_to_string(&destino).unwrap();
        let material: glass_core::PhysicsMaterial = serde_json::from_str(&json).unwrap();
        assert_eq!(material.dynamic_friction, 0.6);
        assert_eq!(material.static_friction, 0.6);
        assert_eq!(material.bounciness, 0.0);
    }

    #[test]
    fn crear_physics_material_rechaza_duplicado_y_sufijo() {
        let root = temp_dir_unico("physics_material_duplicado");
        let destino = crear_physics_material(&root, "Hielo.physmat.json").unwrap();

        assert_eq!(destino, root.join("materials").join("Hielo.physmat.json"));
        assert!(crear_physics_material(&root, "Hielo.physmat.json").is_err());
    }

    #[test]
    fn physmat_se_clasifica_con_su_icono_y_no_es_instanciable() {
        assert_eq!(
            icono_para("Hielo.physmat.json", false),
            "🧲",
            "un .physmat.json debe usar el icono de material físico"
        );
        assert_eq!(
            icono_para("HIELO.PHYSMAT.JSON", false),
            "🧲",
            "el sufijo .physmat.json debe reconocerse sin distinguir mayúsculas"
        );
        assert_ne!(
            icono_para("Hielo.physmat.json", false),
            "📄",
            "un physmat no debe caer en el icono genérico de archivo"
        );
        assert_eq!(
            icono_para("datos.json", false),
            "📄",
            "un .json desconocido sí debe usar el icono genérico"
        );
        assert!(
            !es_instanciable("Hielo.physmat.json", false),
            "un physmat no debe arrastrarse a la escena como entidad instanciable"
        );
    }

    #[test]
    fn physmat_se_lista_como_asset_normal() {
        let dir = temp_dir_unico("physmat_listado");
        std::fs::write(dir.join("Hielo.physmat.json"), "{}")
            .expect("debe poder escribirse el physmat de prueba");
        std::fs::write(dir.join("oculto.meta.json"), "{}")
            .expect("debe poder escribirse el sidecar de prueba");

        let entradas = listar_directorio(&dir).expect("listar_directorio debe funcionar");
        let physmat = entradas
            .iter()
            .find(|e| e.nombre == "Hielo.physmat.json")
            .expect("el physmat no debe ocultarse como si fuera un sidecar");

        assert!(!physmat.es_dir, "el physmat debe listarse como archivo");
        assert!(
            entradas.iter().all(|e| e.nombre != "oculto.meta.json"),
            "los .meta.json deben seguir ocultos en el listado"
        );
    }

    #[test]
    fn physmat_no_tiene_gemelo_convertido() {
        let ruta = std::path::Path::new("assets/Hielo.physmat.json");
        let gemelos = gemelos_convertidos(ruta);
        assert!(
            gemelos.is_empty(),
            "borrar o mover un physmat no debe arrastrar ningún .glb gemelo"
        );
    }

    #[test]
    fn duplicar_archivo_crea_copia_con_sufijo() {
        let dir = temp_dir_unico("duplicar_archivo");
        std::fs::create_dir_all(&dir).unwrap();
        let original = dir.join("Hielo.physmat.json");
        std::fs::write(&original, "{}").unwrap();

        let copia = duplicar_elemento(&original).unwrap();
        assert_eq!(copia, dir.join("Hielo (2).physmat.json"));
        assert_eq!(std::fs::read_to_string(&copia).unwrap(), "{}");
        assert_eq!(std::fs::read_to_string(&original).unwrap(), "{}");

        let copia2 = duplicar_elemento(&original).unwrap();
        assert_eq!(copia2, dir.join("Hielo (3).physmat.json"));
        assert_eq!(std::fs::read_to_string(&copia2).unwrap(), "{}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicar_carpeta_copia_el_contenido() {
        let dir = temp_dir_unico("duplicar_carpeta");
        std::fs::create_dir_all(&dir).unwrap();
        let materiales = dir.join("Materiales");
        std::fs::create_dir_all(materiales.join("Sub")).unwrap();
        std::fs::write(materiales.join("Hielo.physmat.json"), "{\"friccion\":0.1}").unwrap();
        std::fs::write(materiales.join("Sub").join("Roca.material.json"), "{\"color\":1}").unwrap();

        let copia = duplicar_elemento(&materiales).unwrap();
        assert_eq!(copia, dir.join("Materiales (2)"));
        assert!(copia.is_dir());
        assert_eq!(
            std::fs::read_to_string(copia.join("Hielo.physmat.json")).unwrap(),
            "{\"friccion\":0.1}"
        );
        assert_eq!(
            std::fs::read_to_string(copia.join("Sub").join("Roca.material.json")).unwrap(),
            "{\"color\":1}"
        );
        assert!(materiales.join("Hielo.physmat.json").is_file());
        assert!(materiales.join("Sub").join("Roca.material.json").is_file());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicar_ruta_inexistente_devuelve_error() {
        let dir = temp_dir_unico("duplicar_inexistente");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("NoExiste.physmat.json");

        let res = duplicar_elemento(&ruta);
        assert!(res.is_err());
        assert!(!dir.join("NoExiste (2).physmat.json").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicar_modelo_no_arrastra_su_glb_gemelo() {
        let dir = temp_dir_unico("duplicar_modelo_glb");
        std::fs::create_dir_all(&dir).unwrap();

        let blend = dir.join("Caja.blend");
        let glb = dir.join("Caja.glb");
        std::fs::write(&blend, b"blend").unwrap();
        std::fs::write(&glb, b"glb").unwrap();

        let copia = duplicar_elemento(&blend).expect("duplicar Caja.blend debe funcionar");

        assert_eq!(
            copia.file_name().and_then(|n| n.to_str()),
            Some("Caja (2).blend"),
            "la copia debe llamarse 'Caja (2).blend'"
        );
        assert!(copia.exists(), "la copia 'Caja (2).blend' debe existir");

        let gemelo = dir.join("Caja (2).glb");
        assert!(
            !gemelo.exists(),
            "no debe crearse 'Caja (2).glb': cada archivo se duplica por su cuenta"
        );
    }

    #[test]
    fn duplicar_respeta_el_contenido_binario() {
        let dir = temp_dir_unico("duplicar_binario");
        std::fs::create_dir_all(&dir).unwrap();

        let original = dir.join("tex.png");
        let bytes: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF, 0x10];
        std::fs::write(&original, &bytes).unwrap();

        let copia = duplicar_elemento(&original).expect("duplicar tex.png debe funcionar");

        let original_leido: Vec<u8> = std::fs::read(&original).unwrap();
        let copia_leida: Vec<u8> = std::fs::read(&copia).unwrap();
        assert_eq!(
            original_leido, copia_leida,
            "la copia debe tener exactamente los mismos bytes que el original"
        );
    }

    #[test]
    fn duplicar_escena_conserva_sufijo_compuesto() {
        let dir = temp_dir_unico("duplicar_escena");
        std::fs::create_dir_all(&dir).unwrap();

        let escena = dir.join("Nivel.scene.json");
        std::fs::write(&escena, b"{}").unwrap();

        let copia = duplicar_elemento(&escena).expect("duplicar Nivel.scene.json debe funcionar");

        assert_eq!(
            copia.file_name().and_then(|n| n.to_str()),
            Some("Nivel (2).scene.json"),
            "la copia debe conservar el sufijo compuesto: 'Nivel (2).scene.json'"
        );

        let mal = dir.join("Nivel.scene (2).json");
        assert!(
            !mal.exists(),
            "no debe generarse 'Nivel.scene (2).json'"
        );
    }

}
