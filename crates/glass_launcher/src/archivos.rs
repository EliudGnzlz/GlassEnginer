//! Ayudantes de archivos y carpetas de proyectos del lanzador:
//! nombres visibles, apertura en el explorador del sistema y
//! renombrado/borrado de proyectos en disco. Vivían en el panel del
//! Hub; se comparten entre el Hub (`glass_hub`) y el editor (que abre
//! carpetas desde el panel Proyecto).

use std::path::{Path, PathBuf};

pub fn default_projects_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join("GlassProjects")
}

/// Ruta canonicalizada, o la ruta tal cual si no se puede canonicalizar.
pub fn canon(p: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Nombre visible de un proyecto: el campo "name" de su `project.glass` si
/// es un string no vacío; si no, el nombre de la carpeta; y si tampoco hay,
/// "Proyecto".
pub fn nombre_proyecto(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("project.glass"))
        .ok()
        .and_then(|texto| serde_json::from_str::<serde_json::Value>(&texto).ok())
        .and_then(|meta| meta.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .filter(|n| !n.trim().is_empty())
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Proyecto".to_string())
}

/// Abre la carpeta PADRE de `dir` en el explorador de archivos del
/// sistema (si no hay padre, abre la propia). El fallo al lanzar el
/// explorador se anota y no rompe nada.
pub fn abrir_carpeta_contenedora(dir: &Path) {
    let destino = dir.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| dir.to_path_buf());
    #[cfg(target_os = "windows")]
    let programa = "explorer";
    #[cfg(target_os = "macos")]
    let programa = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let programa = "xdg-open";
    if let Err(e) = std::process::Command::new(programa).arg(&destino).spawn() {
        log::warn!("No se pudo abrir la carpeta contenedora de {}: {e}", destino.display());
    }
}

/// Abre `path` en el explorador de archivos del sistema: si es una
/// carpeta, la carpeta misma; si es un archivo, su carpeta contenedora.
/// El fallo al lanzar el explorador se anota y no rompe nada.
pub fn abrir_en_explorador(path: &Path) {
    let destino = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| path.to_path_buf())
    };
    #[cfg(target_os = "windows")]
    let programa = "explorer";
    #[cfg(target_os = "macos")]
    let programa = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let programa = "xdg-open";
    if let Err(e) = std::process::Command::new(programa).arg(&destino).spawn() {
        log::warn!("No se pudo abrir {} en el explorador: {e}", destino.display());
    }
}

/// Renombra un proyecto: mueve su carpeta a una hermana con el nuevo
/// nombre, actualiza el campo "name" de su project.glass (mejor
/// esfuerzo: si el meta no se puede reescribir se anota y se sigue) y
/// refresca el registro. Devuelve la ruta nueva.
pub fn renombrar_proyecto(
    store: &mut crate::updates::LauncherStore,
    dir: &Path,
    nuevo_nombre: &str,
) -> Result<PathBuf, String> {
    let nombre = nuevo_nombre.trim();
    if nombre.is_empty() {
        return Err("El nombre no puede estar vacío.".to_string());
    }
    if nombre.contains('/') || nombre.contains('\\') {
        return Err("El nombre no puede contener barras.".to_string());
    }
    let padre = dir
        .parent()
        .ok_or_else(|| "El proyecto no tiene carpeta contenedora.".to_string())?;
    let destino = padre.join(nombre);
    if destino.exists() {
        return Err(format!("Ya existe una carpeta llamada «{nombre}» en la misma ubicación."));
    }
    std::fs::rename(dir, &destino)
        .map_err(|e| format!("No se pudo renombrar la carpeta: {e}"))?;
    // El "name" del project.glass, a la par del nombre visible.
    let meta_path = destino.join("project.glass");
    if let Ok(texto) = std::fs::read_to_string(&meta_path) {
        if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&texto) {
            if let Some(obj) = meta.as_object_mut() {
                obj.insert("name".to_string(), serde_json::Value::String(nombre.to_string()));
                if let Ok(nuevo_texto) = serde_json::to_string_pretty(&meta) {
                    let _ = std::fs::write(&meta_path, nuevo_texto);
                }
            }
        }
    }
    store.desregistrar_proyecto(dir);
    store.register_project(nombre, &destino);
    Ok(destino)
}

/// Elimina un proyecto del disco (la carpeta entera) y lo desregistra.
/// Pensado para llamarse solo tras la confirmación del usuario.
pub fn eliminar_proyecto_del_disco(
    store: &mut crate::updates::LauncherStore,
    dir: &Path,
) -> Result<(), String> {
    std::fs::remove_dir_all(dir)
        .map_err(|e| format!("No se pudo eliminar la carpeta del proyecto: {e}"))?;
    store.desregistrar_proyecto(dir);
    Ok(())
}

