//! Prefabs vivos (Fase A): tras cargar una escena, las instancias
//! vinculadas a un `.prefab.json` se sincronizan con su asset — la
//! intacta se re-expande si el asset cambió; la modificada y la de
//! vínculo roto se dejan como están, con aviso en el log/Consola.

use std::path::{Path, PathBuf};

use glass_core::prefab::Prefab;
use glass_core::SceneState;

/// Resuelve la ruta de un `prefab_source` contra la raíz del proyecto.
/// Si no existe tal cual, prueba intercambiando la capitalización del
/// primer componente (`assets` ↔ `Assets`): las fuentes escritas por
/// distintas vías no siempre coinciden en mayúsculas y en Linux el
/// sistema de archivos sí distingue.
fn resolver_prefab(raiz: &Path, fuente: &str) -> Option<Prefab> {
    let directa = raiz.join(fuente);
    if directa.is_file() {
        return Prefab::load_from_file(&directa).ok();
    }
    let mut partes = fuente.splitn(2, '/');
    let primero = partes.next().unwrap_or("");
    let resto = partes.next().unwrap_or("");
    let alterno = match primero {
        "assets" => "Assets",
        "Assets" => "assets",
        _ => primero,
    };
    if alterno != primero {
        let alt: PathBuf = raiz.join(alterno).join(resto);
        if alt.is_file() {
            return Prefab::load_from_file(&alt).ok();
        }
    }
    None
}

/// Raíz del proyecto deducida de la ruta de una escena: el ancestro
/// cuyo hijo es la carpeta `assets`/`Assets`. Para las vías de carga
/// que no pasan por la apertura de proyecto en el editor.
pub fn raiz_desde_ruta_escena(ruta_escena: &str) -> Option<PathBuf> {
    let ruta = Path::new(ruta_escena);
    let mut actual = ruta.parent();
    while let Some(dir) = actual {
        for nombre in ["Assets", "assets"] {
            if dir.join(nombre).is_dir() {
                return Some(dir.to_path_buf());
            }
        }
        actual = dir.parent();
    }
    None
}

/// Sincroniza las instancias vinculadas de la escena recién cargada.
/// Sin raíz de proyecto no se puede resolver ningún asset: no se toca
/// nada (las instancias quedan como entidades normales).
pub fn refrescar_prefabs_escena_cargada(scene: &mut SceneState, raiz: Option<&Path>) {
    let Some(raiz) = raiz else {
        return;
    };
    if !scene.entities.iter().any(|e| e.prefab_source.is_some()) {
        return;
    }
    let avisos = scene.refrescar_instancias_prefab(&|fuente: &str| resolver_prefab(raiz, fuente));
    for aviso in &avisos {
        log::warn!("[Prefabs] {aviso}");
    }
}
