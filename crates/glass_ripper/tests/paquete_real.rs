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
