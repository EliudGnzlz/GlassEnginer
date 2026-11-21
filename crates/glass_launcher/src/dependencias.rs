//! Dependencias externas del launcher: detección del runtime de .NET y
//! descarga verificada del instalador oficial de Microsoft (Bug D).
//!
//! La detección no lanza procesos externos: resuelve hostfxr con nethost
//! (sin cargar CoreCLR) y examina el disco.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha512};

/// Página oficial para instalar .NET 8 a mano.
pub const URL_DESCARGA_MANUAL: &str = "https://dotnet.microsoft.com/download/dotnet/8.0";

/// Respaldo fijado si no se puede consultar `releases.json`.
pub const INSTALADOR_DOTNET8_VERSION: &str = "8.0.11";
pub const INSTALADOR_DOTNET8_URL: &str =
    "https://builds.dotnet.microsoft.com/dotnet/8.0.11/dotnet-runtime-8.0.11-win-x64.exe";
/// Huella SHA-512 del instalador de respaldo. Se deja VACÍA a propósito: no se
/// conoce un valor verificado y no se debe inventar. Con la huella vacía el
/// respaldo devuelve Err en lugar de descargar sin verificar. Para activarlo,
/// copiar el `hash` del archivo `dotnet-runtime-win-x64.exe` de la 8.0.11 en
/// https://builds.dotnet.microsoft.com/dotnet/release-metadata/8.0/releases.json
pub const INSTALADOR_DOTNET8_SHA512: &str = "";

/// Hosts desde los que se acepta descargar el instalador.
const HOSTS_PERMITIDOS: &[&str] = &["builds.dotnet.microsoft.com", "download.visualstudio.microsoft.com"];

/// Nombre de archivo que identifica un runtime compartido completo.
const ARCHIVO_CORELIB: &str = "System.Private.CoreLib.dll";

/// Límite de tamaño al leer `releases.json` (el del canal 8.0 ocupa varios MB).
#[cfg(windows)]
const LIMITE_METADATOS: u64 = 64 * 1024 * 1024;

const MSG_HUELLA_NO_COINCIDE: &str =
    "La descarga no coincide con la huella publicada por Microsoft. No se ha abierto.";

#[derive(Clone, Debug, PartialEq)]
pub enum EstadoDotnet {
    Ok { version: String, raiz: PathBuf },
    SinDotnet,
    SinNet8 { encontradas: Vec<String>, raiz: PathBuf },
    ConflictoDotnetRoot { raiz_env: PathBuf, raiz_buena: PathBuf, version: String },
    Error(String),
}

#[derive(Clone, Debug)]
pub struct RequisitoDotnet {
    pub framework: String,
    pub mayor: u32,
}

#[derive(Clone, Debug)]
pub struct InstaladorDotnet {
    pub url: String,
    pub sha512: String,
    pub version: String,
}

// ---------------------------------------------------------------------------
// Requisito desde runtimeconfig.json
// ---------------------------------------------------------------------------

fn requisito_por_defecto() -> RequisitoDotnet {
    RequisitoDotnet { framework: "Microsoft.NETCore.App".to_string(), mayor: 8 }
}

fn ruta_runtimeconfig(dir_motor: &Path) -> PathBuf {
    dir_motor
        .join("managed")
        .join("GlassEngine.Core")
        .join("bin")
        .join("Debug")
        .join("net8.0")
        .join("GlassEngine.Core.runtimeconfig.json")
}

/// Lee el framework requerido del runtimeconfig de GlassEngine.Core.
/// Si el archivo no existe o no parsea, devuelve Microsoft.NETCore.App 8.
pub fn requisito_desde_runtimeconfig(dir_motor: &Path) -> RequisitoDotnet {
    let ruta = ruta_runtimeconfig(dir_motor);
    match fs::read_to_string(&ruta) {
        Ok(texto) => requisito_desde_json(&texto).unwrap_or_else(|| {
            log::warn!("runtimeconfig no válido en {}; se asume .NET 8", ruta.display());
            requisito_por_defecto()
        }),
        Err(e) => {
            log::info!("No se pudo leer {}: {e}; se asume .NET 8", ruta.display());
            requisito_por_defecto()
        }
    }
}

fn requisito_desde_json(texto: &str) -> Option<RequisitoDotnet> {
    let json: serde_json::Value = serde_json::from_str(texto).ok()?;
    let framework = json.pointer("/runtimeOptions/framework")?;
    let nombre = framework.get("name")?.as_str()?.trim();
    let version = framework.get("version")?.as_str()?;
    // El nombre se usa para construir una ruta: solo caracteres seguros.
    let nombre_valido = !nombre.is_empty()
        && !nombre.starts_with('.')
        && nombre.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
    if !nombre_valido {
        return None;
    }
    let mayor = mayor_de_version(version)?;
    Some(RequisitoDotnet { framework: nombre.to_string(), mayor })
}

fn mayor_de_version(version: &str) -> Option<u32> {
    version.trim().split('.').next()?.parse().ok()
}

// ---------------------------------------------------------------------------
// Detección de .NET
// ---------------------------------------------------------------------------

/// Obtiene la raíz de .NET a partir de `<raiz>/host/fxr/<ver>/hostfxr.dll|libhostfxr.so`.
/// Devuelve None si la ruta no tiene esa estructura.
pub fn raiz_desde_ruta_hostfxr(ruta: &Path) -> Option<PathBuf> {
    let dir_version = ruta.parent()?;
    let dir_fxr = dir_version.parent()?;
    let dir_host = dir_fxr.parent()?;
    let raiz = dir_host.parent()?;

    let nombre_es = |p: &Path, esperado: &str| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(esperado))
    };
    if !nombre_es(dir_fxr, "fxr") || !nombre_es(dir_host, "host") {
        return None;
    }
    if raiz.as_os_str().is_empty() {
        return None;
    }
    Some(raiz.to_path_buf())
}

/// Versiones semver estables (MAJOR.MINOR.PATCH, sin sufijos) instaladas en
/// `dir_shared` que contienen System.Private.CoreLib.dll, de mayor a menor.
pub fn versiones_instaladas(dir_shared: &Path) -> Vec<String> {
    let Ok(entradas) = fs::read_dir(dir_shared) else {
        return Vec::new();
    };
    let mut versiones: Vec<((u64, u64, u64), String)> = entradas
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let nombre = e.file_name().into_string().ok()?;
            let clave = parsear_semver_estable(&nombre)?;
            e.path().join(ARCHIVO_CORELIB).is_file().then_some((clave, nombre))
        })
        .collect();
    versiones.sort_by_key(|a| std::cmp::Reverse(a.0));
    versiones.into_iter().map(|(_, nombre)| nombre).collect()
}

fn parsear_semver_estable(texto: &str) -> Option<(u64, u64, u64)> {
    let partes: Vec<&str> = texto.split('.').collect();
    if partes.len() != 3 {
        return None;
    }
    let num = |s: &str| -> Option<u64> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    Some((num(partes[0])?, num(partes[1])?, num(partes[2])?))
}

fn versiones_de_raiz(raiz: &Path, framework: &str) -> Vec<String> {
    versiones_instaladas(&raiz.join("shared").join(framework))
}

/// Primera versión (la más alta, la lista viene ordenada) del mayor pedido.
fn mejor_del_mayor(versiones: &[String], mayor: u32) -> Option<String> {
    versiones.iter().find(|v| mayor_de_version(v) == Some(mayor)).cloned()
}

fn misma_ruta(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

/// Raíz indicada por DOTNET_ROOT_X64 o, si no, DOTNET_ROOT.
fn raiz_entorno() -> Option<PathBuf> {
    ["DOTNET_ROOT_X64", "DOTNET_ROOT"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Raíces de instalación global por defecto, en orden de preferencia.
fn raices_globales() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let program_files = std::env::var_os("ProgramFiles")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        let base = program_files.join("dotnet");
        let mut raices = Vec::new();
        // En Windows ARM64 el runtime x64 vive en dotnet\x64; solo aplica a un proceso x64.
        let x64 = base.join("x64");
        if cfg!(target_arch = "x86_64") && x64.is_dir() {
            raices.push(x64);
        }
        raices.push(base);
        raices
    }
    #[cfg(not(windows))]
    {
        ["/usr/share/dotnet", "/usr/lib/dotnet"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.is_dir())
            .into_iter()
            .collect()
    }
}

/// Comprueba si el runtime requerido está disponible para el motor.
pub fn comprobar_dotnet(req: &RequisitoDotnet) -> EstadoDotnet {
    // `get_hostfxr_path` devuelve un OsString: pasa a PathBuf sin pérdida.
    let efectiva = match netcorehost::nethost::get_hostfxr_path() {
        Ok(ruta) => {
            let ruta = PathBuf::from(ruta);
            match raiz_desde_ruta_hostfxr(&ruta) {
                Some(raiz) => Some(raiz),
                None => {
                    return EstadoDotnet::Error(format!(
                        "Ruta de hostfxr inesperada: {}",
                        ruta.display()
                    ))
                }
            }
        }
        Err(e) => {
            log::info!("nethost no encontró hostfxr: {e:?}");
            None
        }
    };
    evaluar_estado(req, efectiva, raiz_entorno(), &raices_globales())
}

fn evaluar_estado(
    req: &RequisitoDotnet,
    efectiva: Option<PathBuf>,
    raiz_env: Option<PathBuf>,
    globales: &[PathBuf],
) -> EstadoDotnet {
    let efectiva = efectiva.map(|raiz| {
        let versiones = versiones_de_raiz(&raiz, &req.framework);
        (raiz, versiones)
    });
    let efectiva_tiene_mayor = efectiva
        .as_ref()
        .is_some_and(|(_, v)| mejor_del_mayor(v, req.mayor).is_some());

    // Aproximación aceptada: variable definida, ni ella ni la raíz efectiva
    // tienen el mayor y la instalación global sí → conflicto.
    if !efectiva_tiene_mayor {
        if let Some(env) = &raiz_env {
            let env_tiene_mayor =
                mejor_del_mayor(&versiones_de_raiz(env, &req.framework), req.mayor).is_some();
            if !env_tiene_mayor {
                for global in globales.iter().filter(|g| !misma_ruta(g, env)) {
                    let versiones = versiones_de_raiz(global, &req.framework);
                    if let Some(version) = mejor_del_mayor(&versiones, req.mayor) {
                        return EstadoDotnet::ConflictoDotnetRoot {
                            raiz_env: env.clone(),
                            raiz_buena: global.clone(),
                            version,
                        };
                    }
                }
            }
        }
    }

    match efectiva {
        None => EstadoDotnet::SinDotnet,
        Some((raiz, versiones)) => match mejor_del_mayor(&versiones, req.mayor) {
            Some(version) => EstadoDotnet::Ok { version, raiz },
            None => EstadoDotnet::SinNet8 { encontradas: versiones, raiz },
        },
    }
}

// ---------------------------------------------------------------------------
// Instalación en Linux
// ---------------------------------------------------------------------------

/// Devuelve (distribución, comando) para instalar el runtime con el gestor de
/// paquetes, o None si no hay comando conocido (se muestra el enlace manual).
pub fn comando_instalacion_linux() -> Option<(&'static str, String)> {
    #[cfg(target_os = "linux")]
    {
        let contenido = fs::read_to_string("/etc/os-release").ok()?;
        comando_desde_os_release(&contenido)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// El nombre de paquete usa el mayor 8 fijo: la firma pública no recibe el
/// requisito y el requisito actual es .NET 8. Si el motor migra de versión,
/// hay que revisar estos comandos.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn comando_desde_os_release(contenido: &str) -> Option<(&'static str, String)> {
    let mut id = String::new();
    let mut id_like: Vec<String> = Vec::new();
    for linea in contenido.lines() {
        let Some((clave, valor)) = linea.trim().split_once('=') else {
            continue;
        };
        let valor = valor.trim().trim_matches('"').trim_matches('\'').to_ascii_lowercase();
        match clave.trim() {
            "ID" => id = valor,
            "ID_LIKE" => id_like = valor.split_whitespace().map(str::to_string).collect(),
            _ => {}
        }
    }
    let parecida = |n: &str| id_like.iter().any(|l| l == n);

    // Debian puro no trae dotnet-runtime-8.0 en sus repositorios oficiales: va al enlace.
    if id == "debian" {
        return None;
    }
    if id == "ubuntu" || parecida("ubuntu") || parecida("debian") {
        return Some(("Ubuntu", "sudo apt install dotnet-runtime-8.0".to_string()));
    }
    if id == "fedora" || parecida("fedora") {
        return Some(("Fedora", "sudo dnf install dotnet-runtime-8.0".to_string()));
    }
    if id == "arch" || parecida("arch") {
        return Some(("Arch", "sudo pacman -S dotnet-runtime-8.0".to_string()));
    }
    None
}

// ---------------------------------------------------------------------------
// Instalador de Windows
// ---------------------------------------------------------------------------

fn agente(timeout_total: Duration) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(timeout_total))
        .build();
    ureq::Agent::new_with_config(config)
}

fn es_sha512_hex(texto: &str) -> bool {
    texto.len() == 128 && texto.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Exige https, puerto por defecto, sin credenciales y host de Microsoft permitido.
fn validar_url_descarga(url: &str) -> Result<(), String> {
    let uri: ureq::http::Uri =
        url.parse().map_err(|_| format!("URL de descarga no válida: {url}"))?;
    if !uri.scheme_str().is_some_and(|s| s.eq_ignore_ascii_case("https")) {
        return Err(format!("La URL de descarga no usa https: {url}"));
    }
    if uri.authority().is_some_and(|a| a.as_str().contains('@')) {
        return Err(format!("La URL de descarga contiene credenciales: {url}"));
    }
    if !matches!(uri.port_u16(), None | Some(443)) {
        return Err(format!("La URL de descarga usa un puerto no permitido: {url}"));
    }
    let host = uri.host().unwrap_or_default().to_ascii_lowercase();
    if !HOSTS_PERMITIDOS.contains(&host.as_str()) {
        return Err(format!("El host de descarga no está permitido: {host}"));
    }
    Ok(())
}

/// Elige el instalador win-x64 de la última versión del runtime a partir del
/// `releases.json` del canal `<mayor>.0`.
pub fn elegir_instalador(json: &serde_json::Value, mayor: u32) -> Result<InstaladorDotnet, String> {
    if let Some(canal) = json.get("channel-version").and_then(|v| v.as_str()) {
        if mayor_de_version(canal) != Some(mayor) {
            return Err(format!("Los metadatos son del canal {canal}, no de .NET {mayor}"));
        }
    }
    let ultima = json
        .get("latest-runtime")
        .and_then(|v| v.as_str())
        .ok_or("Los metadatos de .NET no indican latest-runtime")?;
    if mayor_de_version(ultima) != Some(mayor) {
        return Err(format!("latest-runtime ({ultima}) no es de .NET {mayor}"));
    }
    let release = json
        .get("releases")
        .and_then(|v| v.as_array())
        .ok_or("Los metadatos de .NET no contienen releases")?
        .iter()
        .find(|r| r.pointer("/runtime/version").and_then(|v| v.as_str()) == Some(ultima))
        .ok_or_else(|| format!("No se encontró la versión {ultima} en los metadatos"))?;
    let archivo = release
        .pointer("/runtime/files")
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("La versión {ultima} no lista archivos"))?
        .iter()
        .find(|f| {
            f.get("rid").and_then(|v| v.as_str()) == Some("win-x64")
                && f.get("name").and_then(|v| v.as_str()) == Some("dotnet-runtime-win-x64.exe")
        })
        .ok_or_else(|| format!("No hay instalador win-x64 para la versión {ultima}"))?;
    let url = archivo
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("El instalador no tiene URL")?;
    let hash = archivo
        .get("hash")
        .and_then(|v| v.as_str())
        .ok_or("El instalador no tiene huella")?
        .trim()
        .to_ascii_lowercase();
    if !es_sha512_hex(&hash) {
        return Err("La huella publicada no es un SHA-512 válido".to_string());
    }
    validar_url_descarga(url)?;
    Ok(InstaladorDotnet { url: url.to_string(), sha512: hash, version: ultima.to_string() })
}

#[cfg(windows)]
fn descargar_metadatos(mayor: u32) -> Result<serde_json::Value, String> {
    let url = format!(
        "https://builds.dotnet.microsoft.com/dotnet/release-metadata/{mayor}.0/releases.json"
    );
    let mut respuesta = agente(Duration::from_secs(60))
        .get(&url)
        .call()
        .map_err(|e| format!("Error de red al consultar {url}: {e}"))?;
    let texto = respuesta
        .body_mut()
        .with_config()
        .limit(LIMITE_METADATOS)
        .read_to_string()
        .map_err(|e| format!("Error al leer {url}: {e}"))?;
    serde_json::from_str(&texto).map_err(|e| format!("Metadatos de .NET no válidos: {e}"))
}

#[cfg(windows)]
fn instalador_respaldo(mayor: u32, error_original: &str) -> Result<InstaladorDotnet, String> {
    if mayor != 8 {
        return Err(format!(
            "No se pudo obtener el instalador de .NET {mayor} ({error_original}). \
             Descárgalo manualmente desde https://dotnet.microsoft.com/download"
        ));
    }
    if INSTALADOR_DOTNET8_SHA512.is_empty() {
        return Err(format!(
            "No se pudo obtener la información de descarga de Microsoft ({error_original}) \
             y no hay una huella de respaldo verificada. Descarga .NET manualmente desde {URL_DESCARGA_MANUAL}"
        ));
    }
    Ok(InstaladorDotnet {
        url: INSTALADOR_DOTNET8_URL.to_string(),
        sha512: INSTALADOR_DOTNET8_SHA512.to_ascii_lowercase(),
        version: INSTALADOR_DOTNET8_VERSION.to_string(),
    })
}

/// Obtiene URL y huella del instalador oficial win-x64 (solo Windows).
pub fn instalador_dotnet_windows(req: &RequisitoDotnet) -> Result<InstaladorDotnet, String> {
    #[cfg(windows)]
    {
        match descargar_metadatos(req.mayor).and_then(|json| elegir_instalador(&json, req.mayor)) {
            Ok(instalador) => Ok(instalador),
            Err(e) => {
                log::warn!("Fallo al obtener metadatos de .NET: {e}; se usa el respaldo");
                instalador_respaldo(req.mayor, &e)
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = req;
        Err("La descarga automática del instalador de .NET solo está disponible en Windows.".to_string())
    }
}

// ---------------------------------------------------------------------------
// Descarga verificada
// ---------------------------------------------------------------------------

/// Copia por bloques calculando SHA-512 sobre la marcha. Devuelve la huella en hex minúsculas.
fn copiar_con_hash<R: Read, W: Write>(mut origen: R, destino: &mut W) -> Result<String, String> {
    let mut hasher = Sha512::new();
    let mut bloque = vec![0u8; 64 * 1024];
    loop {
        let leidos = match origen.read(&mut bloque) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("Error de red durante la descarga: {e}")),
        };
        hasher.update(&bloque[..leidos]);
        destino
            .write_all(&bloque[..leidos])
            .map_err(|e| format!("Error al escribir el instalador: {e}"))?;
    }
    destino.flush().map_err(|e| format!("Error al escribir el instalador: {e}"))?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Descarga el instalador a `destino` y verifica su SHA-512. Si la huella no
/// coincide o la descarga falla a medias, borra el archivo.
pub fn descargar_instalador_dotnet(inst: &InstaladorDotnet, destino: &Path) -> Result<(), String> {
    validar_url_descarga(&inst.url)?;
    let esperado = inst.sha512.trim().to_ascii_lowercase();
    if !es_sha512_hex(&esperado) {
        return Err("La huella SHA-512 del instalador no es válida; no se descarga sin verificar.".to_string());
    }
    if let Some(padre) = destino.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(padre)
            .map_err(|e| format!("No se pudo crear {}: {e}", padre.display()))?;
    }

    // Redirecciones: ureq las sigue por defecto. La huella protege el contenido final.
    let respuesta = agente(Duration::from_secs(600))
        .get(&inst.url)
        .call()
        .map_err(|e| format!("Error de red al descargar el instalador de .NET: {e}"))?;

    let mut archivo = fs::File::create(destino)
        .map_err(|e| format!("No se pudo crear {}: {e}", destino.display()))?;
    let resultado = copiar_con_hash(respuesta.into_body().into_reader(), &mut archivo)
        .and_then(|hash| {
            archivo
                .sync_all()
                .map_err(|e| format!("Error al escribir el instalador: {e}"))?;
            Ok(hash)
        });
    drop(archivo);

    match resultado {
        Ok(calculado) if calculado == esperado => Ok(()),
        Ok(_) => {
            let _ = fs::remove_file(destino);
            Err(MSG_HUELLA_NO_COINCIDE.to_string())
        }
        Err(e) => {
            let _ = fs::remove_file(destino);
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Directorio temporal propio del test; se borra al salir.
    struct DirTemporal(PathBuf);

    impl DirTemporal {
        fn nuevo(nombre: &str) -> Self {
            static CONTADOR: AtomicU32 = AtomicU32::new(0);
            let n = CONTADOR.fetch_add(1, Ordering::Relaxed);
            let ruta = std::env::temp_dir()
                .join(format!("glass_launcher_dep_{nombre}_{}_{n}", std::process::id()));
            let _ = fs::remove_dir_all(&ruta);
            fs::create_dir_all(&ruta).unwrap();
            DirTemporal(ruta)
        }
    }

    impl Drop for DirTemporal {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn crear_version(dir_shared: &Path, version: &str, con_corelib: bool) {
        let dir = dir_shared.join(version);
        fs::create_dir_all(&dir).unwrap();
        if con_corelib {
            fs::write(dir.join(ARCHIVO_CORELIB), b"").unwrap();
        }
    }

    fn req8() -> RequisitoDotnet {
        requisito_por_defecto()
    }

    #[test]
    fn raiz_desde_hostfxr_windows() {
        let ruta: PathBuf = ["C:\\", "Program Files", "dotnet", "host", "fxr", "8.0.11", "hostfxr.dll"]
            .iter()
            .collect();
        let esperada: PathBuf = ["C:\\", "Program Files", "dotnet"].iter().collect();
        assert_eq!(raiz_desde_ruta_hostfxr(&ruta), Some(esperada));
    }

    #[test]
    fn raiz_desde_hostfxr_linux() {
        let ruta: PathBuf = ["/", "usr", "share", "dotnet", "host", "fxr", "8.0.11", "libhostfxr.so"]
            .iter()
            .collect();
        let esperada: PathBuf = ["/", "usr", "share", "dotnet"].iter().collect();
        assert_eq!(raiz_desde_ruta_hostfxr(&ruta), Some(esperada));
    }

    #[test]
    fn raiz_desde_hostfxr_estructura_inesperada() {
        let ruta: PathBuf = ["/", "opt", "app", "hostfxr.dll"].iter().collect();
        assert_eq!(raiz_desde_ruta_hostfxr(&ruta), None);
        let ruta: PathBuf = ["/", "x", "otro", "fxr", "8.0.11", "libhostfxr.so"].iter().collect();
        assert_eq!(raiz_desde_ruta_hostfxr(&ruta), None);
    }

    #[test]
    fn versiones_instaladas_filtra_y_ordena() {
        let tmp = DirTemporal::nuevo("versiones");
        let shared = &tmp.0;
        crear_version(shared, "8.0.11", true);
        crear_version(shared, "8.0.0-rc.1", true);
        crear_version(shared, "9.0.1", true);
        crear_version(shared, "8.0.5", false);
        crear_version(shared, "8.0.2", true);
        crear_version(shared, "no-es-version", true);

        assert_eq!(versiones_instaladas(shared), vec!["9.0.1", "8.0.11", "8.0.2"]);
    }

    #[test]
    fn versiones_instaladas_directorio_inexistente() {
        let tmp = DirTemporal::nuevo("inexistente");
        assert!(versiones_instaladas(&tmp.0.join("no_existe")).is_empty());
    }

    #[test]
    fn requisito_desde_runtimeconfig_lee_archivo() {
        let tmp = DirTemporal::nuevo("runtimeconfig");
        let ruta = ruta_runtimeconfig(&tmp.0);
        fs::create_dir_all(ruta.parent().unwrap()).unwrap();
        fs::write(
            &ruta,
            r#"{"runtimeOptions":{"tfm":"net9.0","framework":{"name":"Microsoft.NETCore.App","version":"9.0.0"}}}"#,
        )
        .unwrap();

        let req = requisito_desde_runtimeconfig(&tmp.0);
        assert_eq!(req.framework, "Microsoft.NETCore.App");
        assert_eq!(req.mayor, 9);
    }

    #[test]
    fn requisito_desde_runtimeconfig_fallback() {
        let tmp = DirTemporal::nuevo("sin_runtimeconfig");
        let req = requisito_desde_runtimeconfig(&tmp.0);
        assert_eq!(req.framework, "Microsoft.NETCore.App");
        assert_eq!(req.mayor, 8);

        let ruta = ruta_runtimeconfig(&tmp.0);
        fs::create_dir_all(ruta.parent().unwrap()).unwrap();
        fs::write(&ruta, "esto no es json").unwrap();
        let req = requisito_desde_runtimeconfig(&tmp.0);
        assert_eq!(req.mayor, 8);
    }

    #[test]
    fn evaluar_detecta_conflicto_dotnet_root() {
        let env = DirTemporal::nuevo("env");
        let global = DirTemporal::nuevo("global");
        crear_version(&env.0.join("shared").join("Microsoft.NETCore.App"), "6.0.36", true);
        crear_version(&global.0.join("shared").join("Microsoft.NETCore.App"), "8.0.11", true);

        let estado = evaluar_estado(
            &req8(),
            Some(env.0.clone()),
            Some(env.0.clone()),
            std::slice::from_ref(&global.0),
        );
        assert_eq!(
            estado,
            EstadoDotnet::ConflictoDotnetRoot {
                raiz_env: env.0.clone(),
                raiz_buena: global.0.clone(),
                version: "8.0.11".to_string(),
            }
        );

        let estado = evaluar_estado(&req8(), Some(env.0.clone()), None, std::slice::from_ref(&global.0));
        assert_eq!(
            estado,
            EstadoDotnet::SinNet8 { encontradas: vec!["6.0.36".to_string()], raiz: env.0.clone() }
        );
    }

    #[test]
    fn evaluar_ok_y_sin_dotnet() {
        let raiz = DirTemporal::nuevo("ok");
        let shared = raiz.0.join("shared").join("Microsoft.NETCore.App");
        crear_version(&shared, "8.0.2", true);
        crear_version(&shared, "8.0.11", true);
        assert_eq!(
            evaluar_estado(&req8(), Some(raiz.0.clone()), None, &[]),
            EstadoDotnet::Ok { version: "8.0.11".to_string(), raiz: raiz.0.clone() }
        );
        assert_eq!(evaluar_estado(&req8(), None, None, &[]), EstadoDotnet::SinDotnet);
    }

    #[test]
    fn os_release_distribuciones() {
        let ubuntu = "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n";
        assert_eq!(comando_desde_os_release(ubuntu).unwrap().0, "Ubuntu");
        let mint = "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n";
        assert_eq!(comando_desde_os_release(mint).unwrap().0, "Ubuntu");
        assert_eq!(comando_desde_os_release("ID=debian\n"), None);
        assert_eq!(
            comando_desde_os_release("ID=fedora\n").unwrap().1,
            "sudo dnf install dotnet-runtime-8.0"
        );
        assert_eq!(comando_desde_os_release("ID=manjaro\nID_LIKE=arch\n").unwrap().0, "Arch");
        assert_eq!(comando_desde_os_release("ID=opensuse-tumbleweed\n"), None);
    }

    fn fixture(url_buena: &str) -> serde_json::Value {
        let hash = "AB".repeat(64);
        json!({
            "channel-version": "8.0",
            "latest-runtime": "8.0.11",
            "releases": [
                {
                    "runtime": {
                        "version": "8.0.10",
                        "files": [{
                            "name": "dotnet-runtime-win-x64.exe",
                            "rid": "win-x64",
                            "url": "https://builds.dotnet.microsoft.com/dotnet/8.0.10/dotnet-runtime-8.0.10-win-x64.exe",
                            "hash": "cd".repeat(64)
                        }]
                    }
                },
                {
                    "runtime": {
                        "version": "8.0.11",
                        "files": [
                            {
                                "name": "dotnet-runtime-win-arm64.exe",
                                "rid": "win-arm64",
                                "url": "https://builds.dotnet.microsoft.com/dotnet/8.0.11/dotnet-runtime-8.0.11-win-arm64.exe",
                                "hash": "ef".repeat(64)
                            },
                            {
                                "name": "dotnet-runtime-win-x64.zip",
                                "rid": "win-x64",
                                "url": "https://builds.dotnet.microsoft.com/dotnet/8.0.11/dotnet-runtime-8.0.11-win-x64.zip",
                                "hash": "01".repeat(64)
                            },
                            {
                                "name": "dotnet-runtime-win-x64.exe",
                                "rid": "win-x64",
                                "url": url_buena,
                                "hash": hash
                            }
                        ]
                    }
                }
            ]
        })
    }

    #[test]
    fn elegir_instalador_exacto() {
        let url = "https://builds.dotnet.microsoft.com/dotnet/8.0.11/dotnet-runtime-8.0.11-win-x64.exe";
        let inst = elegir_instalador(&fixture(url), 8).unwrap();
        assert_eq!(inst.url, url);
        assert_eq!(inst.version, "8.0.11");
        assert_eq!(inst.sha512, "ab".repeat(64));
    }

    #[test]
    fn elegir_instalador_rechaza_host_y_esquema() {
        let malo = fixture("https://ejemplo.com/dotnet-runtime-8.0.11-win-x64.exe");
        assert!(elegir_instalador(&malo, 8).is_err());
        let http = fixture("http://builds.dotnet.microsoft.com/dotnet/8.0.11/dotnet-runtime-8.0.11-win-x64.exe");
        assert!(elegir_instalador(&http, 8).is_err());
        let url = "https://builds.dotnet.microsoft.com/dotnet/8.0.11/dotnet-runtime-8.0.11-win-x64.exe";
        assert!(elegir_instalador(&fixture(url), 9).is_err());
    }

    #[test]
    fn copiar_con_hash_sha512_conocido() {
        let mut salida = Vec::new();
        let hash = copiar_con_hash(&b"abc"[..], &mut salida).unwrap();
        assert_eq!(salida, b"abc");
        assert_eq!(
            hash,
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }
}
