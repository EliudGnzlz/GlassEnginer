//! Sonda para un TERCER paquete real (un proyecto entero, no un
//! pack suelto): invariantes del lector, censo por extensión y la
//! comprobación del posible prefijo «00» en los pathname. Se activa
//! con `GLASS_RIPPER_PAQUETE_3`; sin la variable pasa sin hacer
//! nada. El paquete nunca entra al repo ni a CI.

use std::collections::HashMap;
use std::path::Path;

use glass_ripper::meta::parsear_meta;
use glass_ripper::paquete::leer_paquete;

#[test]
fn tercer_paquete_lector_y_censo() {
    let Ok(ruta) = std::env::var("GLASS_RIPPER_PAQUETE_3") else {
        eprintln!("GLASS_RIPPER_PAQUETE_3 no definida: se omite");
        return;
    };
    let lectura = leer_paquete(Path::new(&ruta)).expect("el paquete debe leerse");

    let mut sin_ruta = 0usize;
    let mut con_prefijo_00 = 0usize;
    let mut censo: HashMap<String, usize> = HashMap::new();
    for e in &lectura.entradas {
        match &e.ruta {
            Some(r) => {
                if r.starts_with("00") {
                    con_prefijo_00 += 1;
                    if con_prefijo_00 <= 3 {
                        eprintln!("ruta con prefijo 00: {r}");
                    }
                }
                if !e.es_carpeta {
                    let ext = r
                        .rsplit('.')
                        .next()
                        .filter(|_| r.contains('.'))
                        .unwrap_or("")
                        .to_lowercase();
                    *censo.entry(ext).or_default() += 1;
                }
            }
            None => sin_ruta += 1,
        }
    }
    let mut metas_fallo = 0usize;
    for e in &lectura.entradas {
        if let Some(texto) = &e.meta_texto {
            if let Err(err) = parsear_meta(&e.guid, texto) {
                metas_fallo += 1;
                if metas_fallo <= 3 {
                    eprintln!(
                        "meta con fallo: guid {} ruta {:?}: {err}",
                        e.guid, e.ruta
                    );
                }
            }
        }
    }
    let mut censo_vec: Vec<(&String, &usize)> = censo.iter().collect();
    censo_vec.sort_by(|a, b| b.1.cmp(a.1));
    eprintln!(
        "== TERCER PAQUETE == entradas {} · sueltas {} · sin ruta {sin_ruta} · con prefijo 00: {con_prefijo_00} · metas con fallo {metas_fallo}",
        lectura.entradas.len(),
        lectura.sueltas.len()
    );
    for (ext, n) in censo_vec.iter().take(14) {
        eprintln!("  .{ext}: {n}");
    }
    assert_eq!(sin_ruta, 0, "ninguna ruta rechazada");
    assert_eq!(metas_fallo, 0, "ningún meta debe fallar");
}
