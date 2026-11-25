//! Test de integración contra el paquete real. Se activa solo si está
//! definida `GLASS_RIPPER_PAQUETE_REAL`; en caso contrario pasa sin hacer nada.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use glass_ripper::EstadoInforme;

/// Devuelve la ruta del paquete real si la variable de entorno está definida.
fn ruta_paquete_real() -> Option<PathBuf> {
    std::env::var_os("GLASS_RIPPER_PAQUETE_REAL").map(PathBuf::from)
}

/// Recorre `raiz` recursivamente y devuelve la ruta de todos los archivos.
fn listar_archivos(raiz: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut archivos = Vec::new();
    let mut pila = vec![raiz.to_path_buf()];
    while let Some(dir) = pila.pop() {
        for entrada in fs::read_dir(&dir)? {
            let entrada = entrada?;
            let ruta = entrada.path();
            if entrada.file_type()?.is_dir() {
                pila.push(ruta);
            } else {
                archivos.push(ruta);
            }
        }
    }
    Ok(archivos)
}

/// Cuenta archivos cuyo nombre de archivo termina en `sufijo`.
fn contar_sufijo(archivos: &[PathBuf], sufijo: &str) -> usize {
    archivos
        .iter()
        .filter(|p| p.to_string_lossy().ends_with(sufijo))
        .count()
}

/// Cuenta materiales (`.material.json`) ubicados en el directorio `materials/`.
fn contar_materiales(archivos: &[PathBuf]) -> usize {
    archivos
        .iter()
        .filter(|p| {
            let nombre_ok = p
                .file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(".material.json"));
            let carpeta_ok = p
                .parent()
                .and_then(|d| d.file_name())
                .is_some_and(|n| n == "materials");
            nombre_ok && carpeta_ok
        })
        .count()
}

/// Nombre legible del estado de una línea del informe.
fn nombre_estado(estado: &EstadoInforme) -> &'static str {
    match estado {
        EstadoInforme::Convertido => "Convertido",
        EstadoInforme::Copiado => "Copiado",
        EstadoInforme::ConvertidoConAvisos => "ConvertidoConAvisos",
        EstadoInforme::NoConvertido => "NoConvertido",
        EstadoInforme::Rechazado => "Rechazado",
        EstadoInforme::OmitidoPorColision => "OmitidoPorColision",
    }
}

#[test]
fn importacion_completa_contra_el_paquete_real() {
    let ruta = match ruta_paquete_real() {
        Some(r) => r,
        None => return,
    };

    let raiz = std::env::temp_dir().join("glass_ripper_u5_real");
    if raiz.exists() {
        let _ = fs::remove_dir_all(&raiz);
    }
    fs::create_dir_all(&raiz).expect("crear la raíz de assets temporal");

    let informe = glass_ripper::importador::importar_paquete(&ruta, &raiz, false, None)
        .expect("la importación debe completar sin error global");

    let lectura = glass_ripper::paquete::leer_paquete(&ruta).expect("releer el paquete");
    let faltantes = informe.faltantes(&lectura);
    assert!(
        faltantes.is_empty(),
        "entradas del tar sin línea en el informe: {faltantes:?}"
    );

    let archivos = listar_archivos(&raiz).expect("recorrer la raíz de assets");

    let prefabs = contar_sufijo(&archivos, ".prefab.json");
    let escenas = contar_sufijo(&archivos, ".scene.json");
    let materiales = contar_materiales(&archivos);
    let png = contar_sufijo(&archivos, ".png");
    let glb = contar_sufijo(&archivos, ".glb");

    assert_eq!(prefabs, 96, "prefabs .prefab.json");
    assert_eq!(escenas, 4, "escenas .scene.json");
    assert_eq!(materiales, 43, "materiales bajo materials/");
    assert_eq!(png, 73, "texturas .png");
    assert_eq!(glb, 93, "modelos .glb");

    let registro_ruta = raiz.join(".glass").join("guids.json");
    assert!(
        registro_ruta.exists(),
        "el registro .glass/guids.json debe existir"
    );
    let registro_texto = fs::read_to_string(&registro_ruta).expect("leer el registro");
    let registro: glass_ripper::registro::RegistroGuids =
        serde_json::from_str(&registro_texto).expect("deserializar el registro");
    assert!(
        registro.len() >= 331,
        "el registro solo tiene {} entradas",
        registro.len()
    );

    let mut escenas_rutas: Vec<PathBuf> = archivos
        .iter()
        .filter(|p| p.to_string_lossy().ends_with(".scene.json"))
        .cloned()
        .collect();
    escenas_rutas.sort_by_key(|p| fs::metadata(p).map_or(0, |m| m.len()));
    let escena_mayor = escenas_rutas.last().expect("debe haber al menos una escena");
    let escena_texto = fs::read_to_string(escena_mayor).expect("leer la escena mayor");
    let entidades: Vec<glass_core::EntityData> =
        serde_json::from_str(&escena_texto).expect("deserializar la escena mayor");
    assert!(
        entidades.len() > 100,
        "la escena mayor solo trae {} entidades",
        entidades.len()
    );

    let mut por_estado: HashMap<&'static str, usize> = HashMap::new();
    for linea in &informe.lineas {
        *por_estado.entry(nombre_estado(&linea.estado)).or_insert(0) += 1;
    }
    eprintln!("== resumen de importación ==");
    let mut claves: Vec<&'static str> = por_estado.keys().copied().collect();
    claves.sort();
    for clave in claves {
        eprintln!("  {clave}: {}", por_estado[clave]);
    }
    eprintln!(
        "  archivos: prefab.json={prefabs} scene.json={escenas} material.json={materiales} png={png} glb={glb}"
    );

    let informe2 = glass_ripper::importador::importar_paquete(&ruta, &raiz, false, None)
        .expect("la segunda importación sin sobrescribir debe completar");
    let colisiones = informe2
        .lineas
        .iter()
        .filter(|l| matches!(&l.estado, EstadoInforme::OmitidoPorColision))
        .count();
    assert!(
        colisiones > 300,
        "la segunda pasada solo registró {colisiones} colisiones"
    );
}
