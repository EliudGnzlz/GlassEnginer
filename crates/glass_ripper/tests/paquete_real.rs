//! Verificación contra un `.unitypackage` REAL comprado.
//!
//! El paquete no entra nunca al repo ni a CI (licencia de la Asset
//! Store): la ruta llega por la variable de entorno
//! `GLASS_RIPPER_PAQUETE_REAL`. Sin la variable, el test pasa sin
//! hacer nada. Con ella, lee el paquete con el lector de U1 y vuelca
//! un resumen medido (entradas, rutas, metas, sub-objetos y
//! distribución por extensión) para inspección con `--nocapture`.

use std::collections::BTreeMap;
use std::path::Path;

use glass_ripper::meta::parsear_meta;
use glass_ripper::paquete::leer_paquete;

#[test]
fn paquete_real_se_lee_y_sus_metas_parsean() {
    let Ok(ruta) = std::env::var("GLASS_RIPPER_PAQUETE_REAL") else {
        eprintln!("GLASS_RIPPER_PAQUETE_REAL no definida: se omite");
        return;
    };
    let lectura = leer_paquete(Path::new(&ruta)).expect("el paquete real debe leerse");

    let mut con_ruta = 0usize;
    let mut sin_ruta = 0usize;
    let mut carpetas = 0usize;
    let mut con_asset = 0usize;
    let mut con_meta = 0usize;
    let mut metas_ok = 0usize;
    let mut metas_fallo: Vec<String> = Vec::new();
    let mut con_textura = 0usize;
    let mut con_modelo = 0usize;
    let mut subobjetos = 0usize;
    let mut por_extension: BTreeMap<String, usize> = BTreeMap::new();

    for entrada in &lectura.entradas {
        assert_eq!(entrada.guid.len(), 32, "guid de 32 hex: {}", entrada.guid);
        match &entrada.ruta {
            Some(r) => {
                con_ruta += 1;
                let ext = r
                    .rsplit('.')
                    .next()
                    .filter(|e| !e.contains('/'))
                    .unwrap_or("")
                    .to_lowercase();
                *por_extension.entry(ext).or_default() += 1;
            }
            None => sin_ruta += 1,
        }
        match &entrada.asset {
            Some(_) => con_asset += 1,
            None => carpetas += 1,
        }
        if entrada.es_carpeta {
            // ya contada arriba por la ausencia de asset
        }
        if let Some(meta_texto) = &entrada.meta_texto {
            con_meta += 1;
            match parsear_meta(&entrada.guid, meta_texto) {
                Ok(meta) => {
                    metas_ok += 1;
                    if meta.textura.is_some() {
                        con_textura += 1;
                    }
                    if meta.importer == "ModelImporter" {
                        con_modelo += 1;
                    }
                    subobjetos += meta.sub_objetos.len();
                }
                Err(e) => metas_fallo.push(format!("{}: {e}", entrada.guid)),
            }
        }
    }

    eprintln!("== PAQUETE REAL ==");
    eprintln!("entradas totales: {}", lectura.entradas.len());
    eprintln!("entradas sueltas (fuera de carpeta GUID): {}", lectura.sueltas.len());
    eprintln!("con ruta válida: {con_ruta} · sin ruta: {sin_ruta}");
    eprintln!("con asset: {con_asset} · carpetas (sin asset): {carpetas}");
    eprintln!("con .meta: {con_meta} · parseados OK: {metas_ok} · fallos: {}", metas_fallo.len());
    for fallo in &metas_fallo {
        eprintln!("  META FALLO: {fallo}");
    }
    eprintln!("metas con TextureImporter: {con_textura} · con ModelImporter: {con_modelo}");
    eprintln!("sub-objetos totales (tablas de los .meta): {subobjetos}");
    eprintln!("por extensión: {por_extension:?}");

    assert!(lectura.entradas.len() > 300, "el paquete real trae cientos de entradas por GUID");
    assert_eq!(sin_ruta, 0, "ninguna ruta del paquete real debe rechazarse");
    assert!(metas_fallo.is_empty(), "ningún .meta del paquete real debe fallar");
}

/// U2 contra el paquete real: los conversores de assets sueltos se
/// ejecutan sobre los assets de verdad del paquete y se vuelca la
/// distribución medida (cuántos materiales tienen mapa de shader,
/// si las texturas PNG salen idénticas, si los FBX convierten).
#[test]
fn conversores_u2_contra_el_paquete_real() {
    let Ok(ruta) = std::env::var("GLASS_RIPPER_PAQUETE_REAL") else {
        return;
    };
    let lectura = leer_paquete(Path::new(&ruta)).expect("el paquete real debe leerse");

    // GUID → ruta, para el resolvedor de texturas de los materiales.
    let rutas: std::collections::HashMap<String, String> = lectura
        .entradas
        .iter()
        .filter_map(|e| e.ruta.clone().map(|r| (e.guid.clone(), r)))
        .collect();
    let resolver = |guid: &str| rutas.get(guid).cloned();

    let mut mats_ok = 0usize;
    let mut mats_sin_mapa = 0usize;
    let mut mats_otro_error: Vec<String> = Vec::new();
    let mut png_identicas = 0usize;
    let mut escalas_distintas: Vec<f32> = Vec::new();
    let dir = std::env::temp_dir().join("glass_ripper_u2_real");
    std::fs::create_dir_all(&dir).unwrap();
    let mut fbx_probados = 0usize;

    for entrada in &lectura.entradas {
        let Some(ruta_asset) = &entrada.ruta else { continue };
        let ext = ruta_asset.rsplit('.').next().unwrap_or("").to_lowercase();
        let meta = entrada
            .meta_texto
            .as_deref()
            .and_then(|t| parsear_meta(&entrada.guid, t).ok());
        match ext.as_str() {
            "mat" => {
                let texto = String::from_utf8_lossy(entrada.asset.as_deref().unwrap_or(&[]));
                match glass_ripper::materiales::convertir_material(ruta_asset, &texto, &resolver) {
                    Ok(_) => mats_ok += 1,
                    Err(glass_ripper::RipperError::Conversion { motivo, .. })
                        if motivo.starts_with("shader sin mapa") =>
                    {
                        mats_sin_mapa += 1;
                    }
                    Err(e) => mats_otro_error.push(format!("{ruta_asset}: {e}")),
                }
            }
            "png" => {
                let bytes = entrada.asset.as_deref().unwrap_or(&[]);
                let (salida, _) =
                    glass_ripper::texturas::convertir_textura(ruta_asset, "png", bytes, meta.as_ref())
                        .expect("una PNG real debe convertirse");
                assert_eq!(salida, bytes, "la PNG de origen se copia tal cual: {ruta_asset}");
                png_identicas += 1;
            }
            "fbx" => {
                if let Some(gs) = meta.as_ref().and_then(|m| m.global_scale) {
                    if (gs - 1.0).abs() > 1e-6 {
                        escalas_distintas.push(gs);
                    }
                }
                if fbx_probados < 3 {
                    let fuente = dir.join(format!("m{fbx_probados}.fbx"));
                    let destino = dir.join(format!("m{fbx_probados}.glb"));
                    std::fs::write(&fuente, entrada.asset.as_deref().unwrap_or(&[])).unwrap();
                    let (informe, avisos) = glass_ripper::modelos::convertir_modelo(
                        &fuente,
                        &destino,
                        meta.as_ref(),
                    )
                    .unwrap_or_else(|e| panic!("un FBX real debe convertir ({ruta_asset}): {e}"));
                    let glb = std::fs::read(&destino).unwrap();
                    assert_eq!(&glb[..4], b"glTF", "el GLB generado es válido");
                    eprintln!(
                        "FBX {ruta_asset}: {} mallas, {} avisos de conversión",
                        informe.conteos.meshes,
                        avisos.len()
                    );
                    fbx_probados += 1;
                }
            }
            _ => {}
        }
    }

    eprintln!("== U2 CONTRA EL PAQUETE REAL ==");
    eprintln!("materiales .mat: convertidos {mats_ok} · shader sin mapa {mats_sin_mapa} · otros errores {}", mats_otro_error.len());
    for e in &mats_otro_error {
        eprintln!("  MAT ERROR: {e}");
    }
    eprintln!("texturas PNG idénticas: {png_identicas}");
    eprintln!("FBX probados a fondo: {fbx_probados} · metas con globalScale != 1: {}", escalas_distintas.len());
    assert!(mats_otro_error.is_empty(), "ningún .mat real debe fallar por otra causa");
    assert_eq!(fbx_probados, 3);
}

/// U3 contra el paquete real: convierte TODOS los prefabs en orden
/// topológico y las 4 escenas con un contexto construido desde el
/// propio paquete (rutas simuladas como las dejará el importador:
/// FBX→.glb, .mat→.material.json), y verifica que las escenas
/// resultantes serializan y deserializan como `Vec<EntityData>`
/// (el formato real de las escenas del motor).
#[test]
fn escenas_y_prefabs_u3_contra_el_paquete_real() {
    let Ok(ruta) = std::env::var("GLASS_RIPPER_PAQUETE_REAL") else {
        return;
    };
    let lectura = leer_paquete(Path::new(&ruta)).expect("el paquete real debe leerse");

    // Ruta importada simulada por guid.
    let mut rutas: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for e in &lectura.entradas {
        let Some(r) = &e.ruta else { continue };
        let ext = r.rsplit('.').next().unwrap_or("").to_lowercase();
        let importada = match ext.as_str() {
            "fbx" => format!("{}.glb", &r[..r.len() - 4]),
            "mat" => format!("{}.material.json", &r[..r.len() - 4]),
            _ => r.clone(),
        };
        rutas.insert(e.guid.clone(), importada);
    }
    let ctx = glass_ripper::escenas::ContextoEscena {
        rutas: &|guid: &str| rutas.get(guid).cloned(),
        clase_de_script: &|_| None,
        physmat_de_guid: &|_| None,
    };

    let texto_de = |e: &glass_ripper::EntradaPaquete| -> String {
        String::from_utf8_lossy(e.asset.as_deref().unwrap_or(&[])).into_owned()
    };

    // Prefabs en orden topológico por m_SourcePrefab.
    let prefabs_crudos: Vec<(String, String, String)> = lectura
        .entradas
        .iter()
        .filter(|e| e.ruta.as_deref().is_some_and(|r| r.to_lowercase().ends_with(".prefab")))
        .map(|e| (e.guid.clone(), e.ruta.clone().unwrap(), texto_de(e)))
        .collect();
    let deps: Vec<(String, Vec<String>)> = prefabs_crudos
        .iter()
        .map(|(g, _, t)| (g.clone(), glass_ripper::instancias::dependencias_prefab(t)))
        .collect();
    let orden = glass_ripper::instancias::ordenar_prefabs(&deps).expect("sin ciclos entre prefabs");
    let por_guid: std::collections::HashMap<&str, &(String, String, String)> =
        prefabs_crudos.iter().map(|p| (p.0.as_str(), p)).collect();

    let mut informe = glass_ripper::informe::InformeImportacion::default();
    let mut convertidos: std::collections::HashMap<String, glass_ripper::escenas::PrefabConvertido> =
        std::collections::HashMap::new();
    let mut prefabs_fallo: Vec<String> = Vec::new();
    for guid in &orden {
        let (_, ruta_p, texto) = por_guid[guid.as_str()];
        match glass_ripper::escenas::convertir_prefab(
            guid,
            ruta_p,
            texto,
            &ctx,
            &convertidos,
            &mut informe,
        ) {
            Ok(pc) => {
                convertidos.insert(guid.clone(), pc);
            }
            Err(e) => prefabs_fallo.push(format!("{ruta_p}: {e}")),
        }
    }

    // Escenas.
    let mut escenas_ok = 0usize;
    let mut total_entidades = 0usize;
    let mut con_malla = 0usize;
    let mut con_modelo = 0usize;
    let mut con_padre = 0usize;
    for e in &lectura.entradas {
        let Some(r) = &e.ruta else { continue };
        if !r.to_lowercase().ends_with(".unity") {
            continue;
        }
        let entidades = glass_ripper::escenas::convertir_escena(
            &e.guid,
            r,
            &texto_de(e),
            &ctx,
            &convertidos,
            &mut informe,
        )
        .unwrap_or_else(|err| panic!("la escena real debe convertir ({r}): {err}"));
        assert!(!entidades.is_empty(), "la escena {r} no debe salir vacía");
        // Ida y vuelta por el formato real de escena del motor.
        let json = serde_json::to_string(&entidades).expect("la escena serializa");
        let vuelta: Vec<glass_core::EntityData> =
            serde_json::from_str(&json).expect("la escena deserializa");
        assert_eq!(vuelta.len(), entidades.len(), "ida y vuelta sin pérdidas en {r}");
        escenas_ok += 1;
        total_entidades += entidades.len();
        con_malla += entidades.iter().filter(|x| x.mesh.is_some()).count();
        con_modelo += entidades
            .iter()
            .filter(|x| x.mesh.as_ref().is_some_and(|m| !m.model.is_empty()))
            .count();
        con_padre += entidades.iter().filter(|x| x.parent.is_some()).count();
        eprintln!("ESCENA {r}: {} entidades", entidades.len());
    }

    eprintln!("== U3 CONTRA EL PAQUETE REAL ==");
    eprintln!(
        "prefabs: {} de {} convertidos · fallos {}",
        convertidos.len(),
        prefabs_crudos.len(),
        prefabs_fallo.len()
    );
    for f in &prefabs_fallo {
        eprintln!("  PREFAB FALLO: {f}");
    }
    eprintln!("escenas convertidas: {escenas_ok}");
    eprintln!("entidades de escena: {total_entidades} · con malla {con_malla} · con modelo {con_modelo} · con padre {con_padre}");
    eprintln!("líneas de informe U3: {}", informe.lineas.len());
    let mut por_detalle: BTreeMap<String, usize> = BTreeMap::new();
    for linea in &informe.lineas {
        let clave: String = linea.detalle.chars().take(60).collect();
        *por_detalle.entry(clave).or_default() += 1;
    }
    for (detalle, veces) in &por_detalle {
        eprintln!("  INFORME ×{veces}: {detalle}");
    }
    assert!(prefabs_fallo.is_empty(), "ningún prefab real debe fallar");
    assert_eq!(escenas_ok, 4, "las 4 escenas del paquete convierten");
    assert!(total_entidades > 100, "las escenas reales traen cientos de entidades");
}
