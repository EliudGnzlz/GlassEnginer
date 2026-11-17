//! Registro de la sesión a archivo y hook de pánico.
//!
//! En Windows el binario es GUI-subsystem: stderr no va a ninguna parte, así
//! que sin este módulo cualquier cierre es mudo. Todo lo que pasa por `log`
//! se escribe en `glass.log` (además de stderr), y los pánicos se anotan
//! en el mismo archivo antes de delegar en el hook anterior.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Tamaño a partir del cual el registro se rota a `glass.log.old`.
const TAMANO_MAX_LOG: u64 = 5 * 1024 * 1024;

/// Carpeta de datos del motor.
///
/// Duplica a propósito la regla de `updates::data_root`: glass_editor es
/// opcional en glass_app (feature `editor`) y este módulo no puede usarlo.
/// Si la regla cambia allí, hay que cambiarla aquí también.
/// 1. `GLASS_LAUNCHER_HOME` si está definida y no vacía.
/// 2. `HOME` o `USERPROFILE` + `GlassEnginer`.
/// 3. `.` si no hay nada de lo anterior.
fn data_root() -> PathBuf {
    if let Some(raiz) = std::env::var_os("GLASS_LAUNCHER_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(raiz);
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join("GlassEnginer"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Ruta del archivo de registro de la sesión.
pub fn ruta_log() -> PathBuf {
    data_root().join("glass.log")
}

/// Si el registro supera `TAMANO_MAX_LOG`, lo renombra a `glass.log.old`
/// (sustituyendo el anterior). Nunca falla ni entra en pánico.
fn rotar_si_hace_falta(ruta: &Path) {
    let Ok(meta) = fs::metadata(ruta) else {
        return;
    };
    if meta.len() <= TAMANO_MAX_LOG {
        return;
    }
    let antiguo = ruta.with_file_name("glass.log.old");
    let _ = fs::remove_file(&antiguo);
    let _ = fs::rename(ruta, &antiguo);
}

/// Destino del logger: escribe en el archivo y en stderr.
///
/// Vacía el archivo en cada escritura: si el proceso muere dentro del
/// driver, lo último registrado tiene que estar ya en disco. Los errores se
/// ignoran; el logger nunca debe propagar fallos.
struct EscritorRegistro {
    archivo: Arc<Mutex<File>>,
}

impl EscritorRegistro {
    /// Ejecuta `f` con el archivo bloqueado, aunque el Mutex esté envenenado.
    fn con_archivo(&self, f: impl FnOnce(&mut File)) {
        let mut archivo = match self.archivo.lock() {
            Ok(guarda) => guarda,
            Err(envenenado) => envenenado.into_inner(),
        };
        f(&mut archivo);
    }
}

impl Write for EscritorRegistro {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.con_archivo(|archivo| {
            let _ = archivo.write_all(buf);
            let _ = archivo.flush();
        });
        let _ = io::stderr().write_all(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.con_archivo(|archivo| {
            let _ = archivo.flush();
        });
        let _ = io::stderr().flush();
        Ok(())
    }
}

/// Compone la línea de registro de un pánico.
pub fn formatear_panico(mensaje: &str, ubicacion: Option<(String, u32)>) -> String {
    match ubicacion {
        Some((archivo, linea)) => format!("PANIC: {mensaje} (en {archivo}:{linea})"),
        None => format!("PANIC: {mensaje}"),
    }
}

/// Abre el registro en modo append, creándolo si no existe.
fn abrir_append(ruta: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(ruta)
}

/// Instala el logger a archivo y el hook de pánico. Devuelve la ruta del log.
///
/// Nunca entra en pánico: si el archivo no se puede abrir, el logger queda
/// como antes (solo stderr); si ya hay un logger instalado, no lo toca.
pub fn iniciar() -> PathBuf {
    let ruta = ruta_log();
    if let Some(carpeta) = ruta.parent() {
        let _ = fs::create_dir_all(carpeta);
    }
    rotar_si_hace_falta(&ruta);

    let entorno = env_logger::Env::default().default_filter_or("info");
    match abrir_append(&ruta) {
        Ok(archivo) => {
            let escritor = EscritorRegistro {
                archivo: Arc::new(Mutex::new(archivo)),
            };
            let _ = env_logger::Builder::from_env(entorno)
                .target(env_logger::Target::Pipe(Box::new(escritor)))
                .try_init();
        }
        Err(e) => {
            let _ = env_logger::Builder::from_env(entorno).try_init();
            log::warn!("No se pudo abrir el registro {}: {e}", ruta.display());
        }
    }

    instalar_hook_panico(ruta.clone());
    ruta
}

/// Añade al hook de pánico existente la escritura de la línea en el registro.
fn instalar_hook_panico(ruta: PathBuf) {
    let anterior = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let mensaje = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "pánico sin mensaje".to_owned());
        let ubicacion = info.location().map(|l| (l.file().to_owned(), l.line()));
        let linea = formatear_panico(&mensaje, ubicacion);

        // Se reabre el archivo en vez de usar el Mutex del logger: el pánico
        // pudo producirse con ese Mutex tomado.
        if let Ok(mut archivo) = abrir_append(&ruta) {
            let _ = writeln!(archivo, "{linea}");
            let _ = archivo.flush();
        }
        eprintln!("{linea}");

        anterior(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatear_panico_con_y_sin_ubicacion() {
        assert_eq!(formatear_panico("boom", None), "PANIC: boom");
        assert_eq!(
            formatear_panico("boom", Some(("src/main.rs".to_owned(), 42))),
            "PANIC: boom (en src/main.rs:42)"
        );
    }
}
