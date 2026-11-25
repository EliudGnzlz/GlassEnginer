//! Importación completa de NeoFPS (proyecto entero, 396 MB, tercer
//! paquete real) por el driver de U5. Se activa con
//! `GLASS_RIPPER_PAQUETE_3`; sin la variable pasa sin hacer nada.
//! El paquete nunca entra al repo ni a CI. Los conteos son los
//! medidos el 2026-11-25 tras el arreglo de `face_material` nulo en
//! glass_fbx: 94/94 FBX convierten sin abortar el proceso.

fn contar(raiz: &std::path::Path, ext: &str) -> usize {
    let mut n = 0;
    let mut pila = vec![raiz.to_path_buf()];
    while let Some(d) = pila.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                pila.push(p);
            } else if p.to_string_lossy().ends_with(ext) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn importar_neofps() {
    let Ok(ruta) = std::env::var("GLASS_RIPPER_PAQUETE_3") else {
        eprintln!("GLASS_RIPPER_PAQUETE_3 no definida: se omite");
        return;
    };
    let raiz = std::env::temp_dir().join("glass_ripper_neofps");
    let _ = std::fs::remove_dir_all(&raiz);
    let t0 = std::time::Instant::now();
    let informe = glass_ripper::importador::importar_paquete(
        std::path::Path::new(&ruta),
        &raiz,
        false,
        None,
    )
    .expect("la importación de NeoFPS debe completar sin abortar");
    eprintln!(
        "NEOFPS en {:?}: {}",
        t0.elapsed(),
        glass_ripper::entorno::resumen_importacion(&informe)
    );
    assert!(informe.faltantes(&glass_ripper::paquete::leer_paquete(std::path::Path::new(&ruta)).unwrap()).is_empty());
    assert_eq!(contar(&raiz, ".glb"), 94, "todos los FBX convierten");
    assert_eq!(contar(&raiz, ".scene.json"), 25, "todas las escenas");
    assert_eq!(contar(&raiz, ".png"), 373, "todas las texturas");
    assert_eq!(contar(&raiz, ".prefab.json"), 377);
    assert_eq!(contar(&raiz, ".material.json"), 46);
    let _ = std::fs::remove_dir_all(&raiz);
}
