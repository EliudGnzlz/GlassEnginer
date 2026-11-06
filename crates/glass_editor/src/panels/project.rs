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

    let dir = assets_root.join("shader_graphs");
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

/// Crea `assets/scenes/<Nombre>.scene.json` con la plantilla mínima.
pub fn crear_escena(assets_root: &Path, nombre: &str) -> Result<PathBuf, String> {
    let base = nombre_sin_sufijo(nombre.trim(), &[".scene.json", ".json"]);
    validar_nombre(&base)?;

    let dir = assets_root.join("scenes");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;

    let destino = dir.join(format!("{base}.scene.json"));
    if destino.exists() {
        return Err(format!("Ya existe {}", destino.display()));
    }
    std::fs::write(&destino, PLANTILLA_ESCENA)
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
        };
        panel.refrescar();
        panel
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
        let raiz_proyecto = self.raiz_proyecto();
        let resultado = match tipo {
            TipoCreacion::Carpeta => crear_carpeta(&self.current_dir, &nombre),
            TipoCreacion::Script => crear_script_cs(&raiz_proyecto, &nombre),
            TipoCreacion::Grafo => crear_grafo(&self.assets_root, &nombre),
            TipoCreacion::Escena => crear_escena(&self.assets_root, &nombre),
        };
        match resultado {
            Ok(ruta) => {
                self.nuevo_nombre.clear();
                self.seleccion = Some(ruta.clone());
                self.refrescar();
                self.set_info(format!("Creado: {}", ruta.display()));
            }
            Err(e) => self.set_error(e),
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
                let mut accion_mover: Option<(PathBuf, PathBuf)> = None;
                let mut confirmar_renombrado = false;
                let mut cancelar_renombrado = false;

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
                    let resp = ui
                        .selectable_label(seleccionada, format!("{icono} {nombre}"))
                        .on_hover_text(if *es_dir {
                            format!("Carpeta: {}", ruta.display())
                        } else {
                            format!("{} — {} bytes", ruta.display(), bytes)
                        });

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
            });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
}
