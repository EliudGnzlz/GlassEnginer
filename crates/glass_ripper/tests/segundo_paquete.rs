//! Sonda genérica para un SEGUNDO paquete real (u otro cualquiera):
//! verifica los invariantes del lector y de los conversores sin
//! suponer contenido concreto, y vuelca las mediciones con
//! `--nocapture`. Se activa con `GLASS_RIPPER_PAQUETE_2` apuntando
//! a la ruta del `.unitypackage`; sin la variable pasa sin hacer
//! nada. El paquete nunca entra al repo ni a CI.

use std::collections::HashMap;
use std::path::Path;

use glass_ripper::meta::parsear_meta;
use glass_ripper::paquete::leer_paquete;

#[test]
fn segundo_paquete_invariantes_y_conversores() {
    let Ok(ruta) = std::env::var("GLASS_RIPPER_PAQUETE_2") else {
        eprintln!("GLASS_RIPPER_PAQUETE_2 no definida: se omite");
        return;
    };
    let lectura = leer_paquete(Path::new(&ruta)).expect("el paquete debe leerse");

    let mut sin_ruta = 0usize;
    let mut metas_fallo = 0usize;
    let rutas: HashMap<String, String> = lectura
        .entradas
        .iter()
        .filter_map(|e| {
            match &e.ruta {
                Some(r) => Some((e.guid.clone(), r.clone())),
                None => {
                    sin_ruta += 1;
                    None
                }
            }
        })
        .collect();
    for e in &lectura.entradas {
        if let Some(texto) = &e.meta_texto {
            if parsear_meta(&e.guid, texto).is_err() {
                metas_fallo += 1;
            }
        }
    }
    eprintln!(
        "== SEGUNDO PAQUETE == entradas {} · sin ruta {sin_ruta} · metas con fallo {metas_fallo}",
        lectura.entradas.len()
    );
    assert_eq!(sin_ruta, 0, "ninguna ruta rechazada");
    assert_eq!(metas_fallo, 0, "ningún meta debe fallar");

    // Conversores sueltos y prefab, con un contexto de rutas espejo.
    let dir = std::env::temp_dir().join("glass_ripper_sonda2");
    std::fs::create_dir_all(&dir).unwrap();
    let plan: HashMap<String, String> = rutas
        .iter()
        .map(|(g, r)| {
            let espejo = r
                .strip_prefix("Assets/")
                .or_else(|| r.strip_prefix("Packages/"))
                .unwrap_or(r);
            let ext = espejo.rsplit('.').next().unwrap_or("").to_lowercase();
            let destino = match ext.as_str() {
                "fbx" => format!("{}.glb", &espejo[..espejo.len() - 4]),
                "mat" => {
                    let stem = espejo
                        .rsplit('/')
                        .next()
                        .unwrap_or(espejo)
                        .trim_end_matches(".mat");
                    format!("materials/{stem}.material.json")
                }
                "prefab" => format!("{}.json", espejo),
                _ => espejo.to_string(),
            };
            (g.clone(), destino)
        })
        .collect();
    let ctx = glass_ripper::escenas::ContextoEscena {
        rutas: &|guid: &str| plan.get(guid).cloned(),
        clase_de_script: &|_| None,
        physmat_de_guid: &|_| None,
    };
    let resolver_mat = |guid: &str| plan.get(guid).cloned();

    let mut informe = glass_ripper::informe::InformeImportacion::default();
    let vacios: HashMap<String, glass_ripper::escenas::PrefabConvertido> = HashMap::new();
    for e in &lectura.entradas {
        let Some(r) = &e.ruta else { continue };
        let ext = r.rsplit('.').next().unwrap_or("").to_lowercase();
        let bytes = e.asset.as_deref().unwrap_or(&[]);
        let meta = e
            .meta_texto
            .as_deref()
            .and_then(|t| parsear_meta(&e.guid, t).ok());
        match ext.as_str() {
            "fbx" => {
                let fuente = dir.join("sonda.fbx");
                let destino = dir.join("sonda.glb");
                std::fs::write(&fuente, bytes).unwrap();
                let (inf, avisos) =
                    glass_ripper::modelos::convertir_modelo(&fuente, &destino, meta.as_ref())
                        .unwrap_or_else(|err| panic!("el FBX debe convertir ({r}): {err}"));
                let glb = std::fs::read(&destino).unwrap();
                assert_eq!(&glb[..4], b"glTF");
                // El JSON del GLB dice si viajaron animaciones y piel.
                let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
                let gltf: serde_json::Value =
                    serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
                eprintln!(
                    "FBX {r}: mallas {} · avisos {} · GLB: nodos {} · animaciones {} · skins {}",
                    inf.conteos.meshes,
                    avisos.len(),
                    gltf["nodes"].as_array().map_or(0, Vec::len),
                    gltf["animations"].as_array().map_or(0, Vec::len),
                    gltf["skins"].as_array().map_or(0, Vec::len),
                );
            }
            "mat" => {
                let texto = String::from_utf8_lossy(bytes);
                match glass_ripper::materiales::convertir_material(r, &texto, &resolver_mat) {
                    Ok((m, avisos)) => eprintln!(
                        "MAT {r}: albedo {:?} · textura '{}' · avisos {}",
                        m.albedo, m.texture, avisos.len()
                    ),
                    Err(err) => eprintln!("MAT {r}: no convertido: {err}"),
                }
            }
            "prefab" => {
                let texto = String::from_utf8_lossy(bytes);
                let pc = glass_ripper::escenas::convertir_prefab(
                    &e.guid,
                    r,
                    &texto,
                    &ctx,
                    &vacios,
                    &mut informe,
                )
                .unwrap_or_else(|err| panic!("el prefab debe convertir ({r}): {err}"));
                let total = 1 + pc.prefab.descendant_entities.len();
                eprintln!(
                    "PREFAB {r}: {total} entidades · raíz '{}'",
                    pc.prefab.root_entity.name
                );
            }
            _ => {}
        }
    }
    for linea in &informe.lineas {
        eprintln!("  INFORME: {}", linea.detalle);
    }
}
