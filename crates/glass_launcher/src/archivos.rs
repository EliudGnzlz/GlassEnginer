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

/// ¿La carpeta no contiene ningún archivo en todo su árbol? Una carpeta
/// con solo subcarpetas vacías cuenta como vacía: es el esqueleto que
/// deja una creación que falló a mitad, y debe poder reintentarse.
fn carpeta_sin_archivos(dir: &Path) -> Result<bool, String> {
    let mut pila = vec![dir.to_path_buf()];
    while let Some(d) = pila.pop() {
        let entradas = std::fs::read_dir(&d).map_err(|e| e.to_string())?;
        for entrada in entradas {
            let entrada = entrada.map_err(|e| e.to_string())?;
            let tipo = entrada.file_type().map_err(|e| e.to_string())?;
            if tipo.is_dir() {
                pila.push(entrada.path());
            } else {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Valida los datos de creación de un proyecto y resuelve su carpeta destino.
///
/// Sigue la regla del Unity Hub: lo que debe ser único es la carpeta dentro de
/// la ubicación elegida, no el nombre. Dos proyectos pueden llamarse igual si
/// están en ubicaciones distintas, pero no se crea un proyecto sobre una carpeta
/// que ya existe con contenido (el Hub responde «Failed to create project: File
/// already exists»).
///
/// La comprobación se hace sobre la carpeta ya resuelta porque `ubicacion`
/// admite dos formas: la propia carpeta del proyecto (si su último componente
/// coincide con el nombre) o su carpeta padre. Si se comprobara solo
/// `ubicacion.join(nombre)`, la primera forma esquivaría la validación y se
/// sobrescribirían la escena y el `project.glass` de un proyecto existente.
///
/// Devuelve el nombre recortado y la carpeta resuelta. Un directorio sin
/// archivos (vacío, o solo el esqueleto de un intento fallido) se acepta y
/// el proyecto se crea dentro de él.
pub fn validar_creacion_proyecto(
    nombre: &str,
    ubicacion: &Path,
) -> Result<(String, PathBuf), String> {
    let nombre = nombre.trim();
    if nombre.is_empty() {
        return Err("El nombre no puede estar vacío.".to_string());
    }
    if nombre == "." || nombre == ".." {
        return Err("El nombre no es válido.".to_string());
    }
    if nombre.contains('/') || nombre.contains('\\') {
        return Err("El nombre no puede contener barras.".to_string());
    }
    if nombre.chars().any(|c| matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return Err("El nombre contiene caracteres no permitidos en una carpeta.".to_string());
    }
    if nombre.ends_with('.') {
        return Err("El nombre no puede terminar en punto.".to_string());
    }
    // Nombres de dispositivo reservados por Windows (también con extensión:
    // «CON.txt» es igual de inválido como carpeta allí).
    let base = nombre.split('.').next().unwrap_or("");
    if matches!(
        base.to_ascii_uppercase().as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5"
            | "COM6" | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4"
            | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9"
    ) {
        return Err("Ese nombre está reservado por el sistema.".to_string());
    }

    let carpeta = if ubicacion.file_name().map(|n| n == nombre).unwrap_or(false) {
        ubicacion.to_path_buf()
    } else {
        ubicacion.join(nombre)
    };

    if carpeta.join("project.glass").exists() {
        return Err(format!(
            "Ya existe un proyecto llamado «{nombre}» en esta ubicación."
        ));
    }

    match std::fs::metadata(&carpeta) {
        Ok(meta) => {
            if !meta.is_dir() {
                return Err(format!(
                    "Ya existe un archivo llamado «{nombre}» en esta ubicación y no es una carpeta."
                ));
            }
            if !carpeta_sin_archivos(&carpeta)? {
                return Err(format!(
                    "Ya existe una carpeta llamada «{nombre}» en esta ubicación y no está vacía."
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }

    Ok((nombre.to_string(), carpeta))
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

#[cfg(test)]
mod tests {
    use super::validar_creacion_proyecto;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CONTADOR: AtomicUsize = AtomicUsize::new(0);

    fn directorio_temporal(sufijo: &str) -> PathBuf {
        let numero = CONTADOR.fetch_add(1, Ordering::Relaxed);
        let ruta = std::env::temp_dir().join(format!(
            "glass_archivos_test_{}_{}_{}",
            std::process::id(),
            numero,
            sufijo
        ));
        let _ = fs::remove_dir_all(&ruta);
        ruta
    }

    #[test]
    fn rechaza_nombre_vacio_o_solo_espacios() {
        let raiz = directorio_temporal("nombre_vacio");

        assert!(validar_creacion_proyecto("", &raiz).is_err());
        assert!(validar_creacion_proyecto("   \t  ", &raiz).is_err());

        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn rechaza_nombre_con_separadores_de_ruta() {
        let raiz = directorio_temporal("separadores");

        assert!(validar_creacion_proyecto("carpeta/nombre", &raiz).is_err());
        assert!(validar_creacion_proyecto(r"carpeta\nombre", &raiz).is_err());

        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn acepta_ubicacion_inexistente_y_recorta_el_nombre() {
        let raiz = directorio_temporal("inexistente");
        let ubicacion = raiz.join("ubicacion");
        let esperada = ubicacion.join("MiJuego");

        let resultado = validar_creacion_proyecto("  MiJuego  ", &ubicacion);

        assert_eq!(
            resultado.unwrap(),
            ("MiJuego".to_string(), esperada)
        );

        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn no_duplica_nombre_si_ubicacion_ya_es_carpeta_del_proyecto() {
        let raiz = directorio_temporal("sin_duplicar");
        let ubicacion = raiz.join("MiJuego");

        let resultado = validar_creacion_proyecto("MiJuego", &ubicacion);

        assert_eq!(
            resultado.unwrap(),
            ("MiJuego".to_string(), ubicacion)
        );

        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn rechaza_carpeta_destino_con_project_glass() {
        let raiz = directorio_temporal("project_glass");
        let destino = raiz.join("MiJuego");
        fs::create_dir_all(&destino).unwrap();
        fs::write(destino.join("project.glass"), "").unwrap();

        assert!(validar_creacion_proyecto("MiJuego", &raiz).is_err());

        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn acepta_carpeta_destino_existente_y_vacia() {
        let raiz = directorio_temporal("vacia");
        let destino = raiz.join("MiJuego");
        fs::create_dir_all(&destino).unwrap();

        let resultado = validar_creacion_proyecto("MiJuego", &raiz);

        assert_eq!(
            resultado.unwrap(),
            ("MiJuego".to_string(), destino)
        );

        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn rechaza_carpeta_destino_con_archivo_ajeno() {
        let raiz = directorio_temporal("archivo_ajeno");
        let destino = raiz.join("MiJuego");
        fs::create_dir_all(&destino).unwrap();
        fs::write(destino.join("notas.txt"), "contenido").unwrap();

        assert!(validar_creacion_proyecto("MiJuego", &raiz).is_err());

        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn rechaza_ruta_destino_existente_como_archivo() {
        let raiz = directorio_temporal("destino_archivo");
        let destino = raiz.join("MiJuego");
        fs::create_dir_all(&raiz).unwrap();
        fs::write(&destino, "no soy una carpeta").unwrap();

        assert!(validar_creacion_proyecto("MiJuego", &raiz).is_err());

        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn mismo_nombre_en_dos_ubicaciones_se_valida_independientemente() {
        let raiz = directorio_temporal("dos_ubicaciones");
        let primera = raiz.join("primera");
        let segunda = raiz.join("segunda");
        let destino_primera = primera.join("MiJuego");
        let destino_segunda = segunda.join("MiJuego");

        fs::create_dir_all(&destino_primera).unwrap();
        fs::write(destino_primera.join("project.glass"), "").unwrap();

        assert!(validar_creacion_proyecto("MiJuego", &primera).is_err());
        assert_eq!(
            validar_creacion_proyecto("MiJuego", &segunda).unwrap(),
            ("MiJuego".to_string(), destino_segunda)
        );

        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn rechaza_puntos_y_reservados() {
        let raiz = directorio_temporal("reservados");

        assert!(validar_creacion_proyecto(".", &raiz).is_err());
        assert!(validar_creacion_proyecto("..", &raiz).is_err());
        assert!(validar_creacion_proyecto("Juego.", &raiz).is_err());
        assert!(validar_creacion_proyecto("Juego: Director's Cut", &raiz).is_err());
        assert!(validar_creacion_proyecto("¿Qué?", &raiz).is_err());
        assert!(validar_creacion_proyecto("CON", &raiz).is_err());
        assert!(validar_creacion_proyecto("nul", &raiz).is_err());
        assert!(validar_creacion_proyecto("COM1.txt", &raiz).is_err());
        // Un nombre corriente con punto intermedio sí vale.
        assert!(validar_creacion_proyecto("Mi.Juego", &raiz).is_ok());

        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn acepta_esqueleto_de_un_intento_fallido() {
        // Solo subcarpetas vacías (lo que deja una creación interrumpida):
        // cuenta como vacía y se puede reintentar con el mismo nombre.
        let raiz = directorio_temporal("esqueleto");
        let destino = raiz.join("MiJuego");
        fs::create_dir_all(destino.join("Assets/Scenes")).unwrap();
        fs::create_dir_all(destino.join("ProjectSettings")).unwrap();

        assert!(validar_creacion_proyecto("MiJuego", &raiz).is_ok());

        // Pero un archivo dentro de una subcarpeta ya es contenido ajeno.
        fs::write(destino.join("Assets/notas.txt"), "x").unwrap();
        assert!(validar_creacion_proyecto("MiJuego", &raiz).is_err());

        let _ = fs::remove_dir_all(raiz);
    }
}

