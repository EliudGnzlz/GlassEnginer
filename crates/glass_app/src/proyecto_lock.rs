//! Lock exclusivo de «proyecto abierto en el editor» (estilo Unity):
//! mientras una instancia del editor tiene un proyecto abierto, ninguna
//! otra puede abrirlo (dos editores pisándose escenas y registro es el
//! fallo que esto evita). Solo lo usa el editor; el standalone y el MCP
//! no lo toman.
//!
//! El lock es un lock de archivo del SO sobre `.glass_editor.lock` en la
//! raíz del proyecto: se libera solo al cerrar el handle o al morir el
//! proceso, así que un cierre sucio no deja el proyecto bloqueado. El
//! archivo NO se borra al soltar: borrarlo abriría una carrera entre
//! instancias (otra podría crear un archivo nuevo y bloquearlo mientras
//! la primera aún cree tener el suyo).

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// Archivo de lock en la raíz del proyecto. No se borra nunca al soltar.
pub const NOMBRE_LOCK_PROYECTO: &str = ".glass_editor.lock";

pub fn ruta_lock_proyecto(raiz: &Path) -> PathBuf {
    raiz.join(NOMBRE_LOCK_PROYECTO)
}

/// Ruta canónica usada como identidad del proyecto: `./p`, `p/` y la
/// ruta absoluta del mismo directorio se reconocen como el mismo.
pub fn raiz_canonica(dir: &Path) -> Result<PathBuf, String> {
    dir.canonicalize().map_err(|e| {
        format!(
            "No se pudo resolver la ruta del proyecto {}: {e}",
            dir.display()
        )
    })
}

/// Guardián: mientras viva, el proyecto está bloqueado para otras
/// instancias. Al soltarlo se cierra el handle y el SO libera el lock
/// (también si el proceso muere).
#[derive(Debug)]
pub struct ProyectoLock {
    raiz: PathBuf,
    _archivo: File,
}

impl ProyectoLock {
    /// Raíz canónica del proyecto bloqueado.
    pub fn raiz(&self) -> &Path {
        &self.raiz
    }
}

/// Toma el lock exclusivo del proyecto sin esperar.
/// Falla si otra instancia (u otro handle de esta misma) ya lo tiene.
pub fn tomar_lock_proyecto(dir: &Path) -> Result<ProyectoLock, String> {
    let raiz = raiz_canonica(dir)?;
    if !raiz.is_dir() {
        return Err(format!(
            "La ruta del proyecto no es una carpeta: {}",
            dir.display()
        ));
    }

    let ruta = ruta_lock_proyecto(&raiz);
    let archivo = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&ruta)
        .map_err(|e| {
            format!(
                "No se pudo crear el archivo de lock del proyecto {}: {e}",
                ruta.display()
            )
        })?;

    match archivo.try_lock() {
        Ok(()) => Ok(ProyectoLock {
            raiz,
            _archivo: archivo,
        }),
        Err(TryLockError::WouldBlock) => Err(format!(
            "El proyecto ya está abierto en otra instancia de GlassEnginer: {}. \
             Ciérralo en esa instancia antes de abrirlo aquí.",
            dir.display()
        )),
        Err(TryLockError::Error(e)) => Err(format!(
            "No se pudo bloquear el proyecto {} ({}): {e}",
            dir.display(),
            ruta.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Carpeta temporal única que se borra al soltar.
    struct DirTemp(PathBuf);

    impl DirTemp {
        fn nuevo() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "glass_lock_test_{}_{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            DirTemp(dir)
        }
    }

    impl Drop for DirTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn segundo_intento_sobre_proyecto_bloqueado_falla() {
        let d = DirTemp::nuevo();
        let _l1 = tomar_lock_proyecto(&d.0).expect("primer lock");
        let err = tomar_lock_proyecto(&d.0).expect_err("el segundo debe fallar");
        assert!(err.contains("otra instancia"), "mensaje inesperado: {err}");
    }

    #[test]
    fn liberar_permite_retomar() {
        let d = DirTemp::nuevo();
        let l1 = tomar_lock_proyecto(&d.0).unwrap();
        drop(l1);
        tomar_lock_proyecto(&d.0).expect("tras soltar se debe poder retomar");
    }

    #[test]
    fn rutas_distintas_no_se_bloquean_entre_si() {
        let a = DirTemp::nuevo();
        let b = DirTemp::nuevo();
        let _la = tomar_lock_proyecto(&a.0).unwrap();
        let _lb = tomar_lock_proyecto(&b.0).expect("otro proyecto debe poder bloquearse");
    }

    #[test]
    fn alias_de_la_misma_ruta_cuenta_como_el_mismo_proyecto() {
        let d = DirTemp::nuevo();
        let l1 = tomar_lock_proyecto(&d.0).unwrap();
        let alias = d.0.join(".");
        assert_eq!(l1.raiz(), raiz_canonica(&alias).unwrap().as_path());
        assert!(tomar_lock_proyecto(&alias).is_err());
    }

    #[test]
    fn traspaso_toma_el_nuevo_antes_de_soltar_el_anterior() {
        let a = DirTemp::nuevo();
        let b = DirTemp::nuevo();
        let mut vigente = Some(tomar_lock_proyecto(&a.0).unwrap());
        assert!(vigente.is_some(), "A bloqueado por el lock vigente");
        assert!(tomar_lock_proyecto(&a.0).is_err(), "A sigue bloqueado");
        let nuevo = tomar_lock_proyecto(&b.0).unwrap();
        // Asignación (como el campo del App): suelta A al instante.
        vigente = Some(nuevo);
        assert!(tomar_lock_proyecto(&b.0).is_err(), "B sigue bloqueado");
        tomar_lock_proyecto(&a.0).expect("A quedó libre");
        drop(vigente);
    }

    #[test]
    fn el_archivo_de_lock_no_se_borra_al_soltar() {
        let d = DirTemp::nuevo();
        drop(tomar_lock_proyecto(&d.0).unwrap());
        assert!(ruta_lock_proyecto(&d.0).exists());
    }

    #[test]
    fn carpeta_inexistente_da_error() {
        let d = DirTemp::nuevo();
        assert!(tomar_lock_proyecto(&d.0.join("no_existe")).is_err());
    }
}
