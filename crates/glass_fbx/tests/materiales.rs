//! Regresión del test físico de F3 (2026-11-19): el factor difuso/de
//! color base es un ESCALAR en ufbx (su vec4 se lee (f, 0, 0, 0)) y el
//! lector lo aplicaba por componentes, dejando cada material en
//! (r·f, 0, 0, 0): el edificio Synty salía invisible (alfa 0) y el X Bot
//! rojo oscuro. Estos tests fijan el color base compuesto correcto.

use std::path::{Path, PathBuf};

fn convertir(corpus: &str, nombre: &str) -> serde_json::Value {
    let origen = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(corpus)
        .canonicalize()
        .unwrap_or_else(|e| panic!("corpus {corpus} no resoluble: {e}"));
    let dir = std::env::temp_dir().join(format!("glass_fbx_mat_{}_{}", std::process::id(), nombre));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let destino: PathBuf = dir.join(format!("{nombre}.glb"));
    glass_fbx::convertir_fbx(&origen, &destino)
        .unwrap_or_else(|e| panic!("convertir_fbx({corpus}) falló: {e}"));
    let datos = std::fs::read(&destino).unwrap();
    assert_eq!(&datos[..4], b"glTF");
    let n = u32::from_le_bytes(datos[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&datos[20..20 + n]).unwrap()
}

fn factores(gltf: &serde_json::Value) -> Vec<(String, [f32; 4])> {
    gltf["materials"]
        .as_array()
        .expect("el GLB trae materiales")
        .iter()
        .map(|m| {
            let f = m["pbrMetallicRoughness"]["baseColorFactor"]
                .as_array()
                .expect("cada material trae baseColorFactor");
            (
                m["name"].as_str().unwrap_or("").to_string(),
                [
                    f[0].as_f64().unwrap() as f32,
                    f[1].as_f64().unwrap() as f32,
                    f[2].as_f64().unwrap() as f32,
                    f[3].as_f64().unwrap() as f32,
                ],
            )
        })
        .collect()
}

fn assert_factor(real: [f32; 4], esperado: [f32; 4], quien: &str) {
    for i in 0..4 {
        assert!(
            (real[i] - esperado[i]).abs() < 0.01,
            "{quien}: componente {i} = {} y se esperaba {} (factor completo {real:?})",
            real[i],
            esperado[i]
        );
    }
}

#[test]
fn edificio_synty_color_base_gris_con_alfa_uno() {
    // Difuso (1,1,1) con textura × factor escalar 0,8 → (0,8, 0,8, 0,8, 1).
    let gltf = convertir(
        "../../assets/polygon-prototype/FBX/SM_Buildings_Block_1x1_01.fbx",
        "edificio",
    );
    let mats = factores(&gltf);
    let (_, f) = mats
        .iter()
        .find(|(n, _)| n == "Texture_01")
        .expect("material Texture_01 presente");
    assert_factor(*f, [0.8, 0.8, 0.8, 1.0], "Texture_01");
}

#[test]
fn xbot_color_base_conserva_los_tres_canales() {
    // Beta_HighLimbsGeoSG3: difuso (0,837, 0,302, 0,264) × 0,8.
    // Beta_Joints_MAT1: difuso (0,333, 0,125, 0,101) × 0,8.
    let gltf = convertir("../../assets/models/xbot_walking.fbx", "xbot");
    let mats = factores(&gltf);
    for (nombre, esperado) in [
        ("Beta_HighLimbsGeoSG3", [0.6696, 0.2418, 0.2110, 1.0]),
        ("Beta_Joints_MAT1", [0.2667, 0.0996, 0.0808, 1.0]),
    ] {
        let (_, f) = mats
            .iter()
            .find(|(n, _)| n == nombre)
            .unwrap_or_else(|| panic!("material {nombre} presente"));
        assert_factor(*f, esperado, nombre);
    }
}
