use glass_renderer::watcher::{convertir_fuente, conteos_desde_resultado_bridge, ViaConversion};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Directorio temporal propio de cada test. El `Drop` limpia el directorio
/// incluso si un assert hace panic durante el test.
struct TempDir {
    ruta: PathBuf,
}

impl TempDir {
    fn nuevo(etiqueta: &str) -> Self {
        static CONTADOR: AtomicU64 = AtomicU64::new(0);

        let secuencia = CONTADOR.fetch_add(1, Ordering::SeqCst);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let ruta = std::env::temp_dir().join(format!(
            "glass_f2_{}_{}_{}_{}",
            etiqueta,
            std::process::id(),
            secuencia,
            nanos
        ));

        fs::create_dir_all(&ruta).unwrap_or_else(|e| {
            panic!(
                "no se pudo crear el directorio temporal {}: {e}",
                ruta.display()
            )
        });

        Self { ruta }
    }

    fn ruta(&self) -> &Path {
        &self.ruta
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.ruta);
    }
}

#[test]
fn convierte_xbot_por_la_via_nativa() {
    let tmp = TempDir::nuevo("convierte_xbot_por_la_via_nativa");

    let origen = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/models/xbot_walking.fbx")
        .canonicalize()
        .unwrap_or_else(|e| panic!("no se pudo resolver la ruta del corpus xbot: {e}"));
    assert!(
        origen.is_file(),
        "no existe el FBX del corpus en {}",
        origen.display()
    );

    let copia = tmp.ruta().join("xbot_walking.fbx");
    fs::copy(&origen, &copia).unwrap_or_else(|e| {
        panic!(
            "no se pudo copiar {} a {}: {e}",
            origen.display(),
            copia.display()
        )
    });

    let destino = tmp.ruta().join("xbot.glb");
    let informe = convertir_fuente(&copia, &destino)
        .unwrap_or_else(|e| panic!("convertir_fuente falló para el corpus xbot: {e}"));

    assert_eq!(informe.via, ViaConversion::Nativa);
    assert_eq!(informe.conteos.meshes, 2);
    assert_eq!(informe.conteos.nodes, 68);
    assert_eq!(informe.conteos.materials, 2);
    assert_eq!(informe.conteos.animations, 1);
    assert_eq!(informe.conteos.skins, 1);

    assert!(
        destino.is_file(),
        "no se creó el GLB de destino en {}",
        destino.display()
    );
    let glb = fs::read(&destino)
        .unwrap_or_else(|e| panic!("no se pudo leer {}: {e}", destino.display()));
    assert!(glb.len() >= 4, "el GLB generado es demasiado corto");
    assert_eq!(&glb[..4], &b"glTF"[..]);

    // El sidecar se llama <destino>.meta.json, es decir, sustituyendo la
    // extensión .glb por glb.meta.json.
    let sidecar = destino.with_extension("glb.meta.json");
    assert!(
        sidecar.is_file(),
        "no se creó el sidecar en {}",
        sidecar.display()
    );
    let meta = fs::read_to_string(&sidecar)
        .unwrap_or_else(|e| panic!("no se pudo leer el sidecar {}: {e}", sidecar.display()));
    assert!(
        meta.contains("glass_fbx"),
        "el sidecar no identifica al convertidor glass_fbx: {meta}"
    );
}

#[test]
fn fbx_corrupto_da_error() {
    let tmp = TempDir::nuevo("fbx_corrupto_da_error");

    let fbx = tmp.ruta().join("corrupto.fbx");
    let mut basura = Vec::new();
    for _ in 0..16 {
        for byte in 0..=255u8 {
            basura.push(byte);
        }
    }
    fs::write(&fbx, &basura)
        .unwrap_or_else(|e| panic!("no se pudo escribir el FBX corrupto: {e}"));

    let destino = tmp.ruta().join("corrupto.glb");
    match convertir_fuente(&fbx, &destino) {
        Err(mensaje) => assert!(
            !mensaje.trim().is_empty(),
            "se esperaba un mensaje de error no vacío para un FBX corrupto"
        ),
        Ok(_) => panic!("se esperaba Err para un FBX corrupto, pero convertir_fuente devolvió Ok"),
    }
}

#[test]
fn parser_extrae_los_conteos_de_la_ultima_linea() {
    let stdout = concat!(
        "arrancando bridge\n",
        "ruido antes del resultado\n",
        "GLASS_BRIDGE_RESULT {\"ok\": true, \"counts\": {\"meshes\": 3, \"nodes\": 11, \"materials\": 4, \"animations\": 2, \"skins\": 1, \"bytes\": 98765}}\n",
        "ruido después del resultado\n",
    );

    let conteos = conteos_desde_resultado_bridge(stdout)
        .expect("debería extraer los conteos de una línea GLASS_BRIDGE_RESULT válida");
    assert_eq!(conteos.meshes, 3);
    assert_eq!(conteos.nodes, 11);
    assert_eq!(conteos.materials, 4);
    assert_eq!(conteos.animations, 2);
    assert_eq!(conteos.skins, 1);
    assert_eq!(conteos.bytes, 98765);

    let dos_resultados = concat!(
        "GLASS_BRIDGE_RESULT {\"ok\": true, \"counts\": {\"meshes\": 1, \"nodes\": 2, \"materials\": 3, \"animations\": 4, \"skins\": 5, \"bytes\": 6}}\n",
        "ruido intermedio\n",
        "GLASS_BRIDGE_RESULT {\"ok\": true, \"counts\": {\"meshes\": 7, \"nodes\": 8, \"materials\": 9, \"animations\": 10, \"skins\": 11, \"bytes\": 12}}\n",
    );

    let ultimos = conteos_desde_resultado_bridge(dos_resultados)
        .expect("debería extraer los conteos de la última línea GLASS_BRIDGE_RESULT");
    assert_eq!(ultimos.meshes, 7);
    assert_eq!(ultimos.nodes, 8);
    assert_eq!(ultimos.materials, 9);
    assert_eq!(ultimos.animations, 10);
    assert_eq!(ultimos.skins, 11);
    assert_eq!(ultimos.bytes, 12);
}

#[test]
fn parser_sin_resultado_valido_devuelve_none() {
    let sin_linea = "solo ruido\nmás ruido del bridge\n";
    assert!(
        conteos_desde_resultado_bridge(sin_linea).is_none(),
        "sin línea GLASS_BRIDGE_RESULT debe devolver None"
    );

    let resultado_fallido = r#"GLASS_BRIDGE_RESULT {"ok": false, "counts": {"meshes": 1, "nodes": 2, "materials": 3, "animations": 4, "skins": 5, "bytes": 6}}"#;
    assert!(
        conteos_desde_resultado_bridge(resultado_fallido).is_none(),
        "una línea con ok=false debe devolver None"
    );

    let json_roto = r#"GLASS_BRIDGE_RESULT {"ok": true, "counts": {"meshes": 1, "#;
    assert!(
        conteos_desde_resultado_bridge(json_roto).is_none(),
        "JSON roto tras el prefijo debe devolver None"
    );
}