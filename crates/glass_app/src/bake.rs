//! El horneado vive en glass_mcp::bake (compartido con la
//! tool MCP glass_bake_lightmap); aquí queda el cargador de
//! runtime y la re-exportación para la ruta GLASS_BAKE.
pub use glass_mcp::bake::bake_scene_lightmap;

/// Lightmap cargado en runtime para una escena: el atlas y, por
/// entidad horneada (mismo orden que `entities`), sus grupos
/// (eje, signo) con sub-rect y cotas de mundo.
pub struct LightmapRuntime {
    pub atlas_size: u32,
    pub rgba: Vec<u8>,
    pub entities: Vec<u64>,
    pub groups: Vec<Vec<glass_core::lightmap::LmGroupRect>>,
}

/// Carga `assets/lightmaps/<escena>.lm.json` + `.lm.png` si existen
/// para la escena dada (ruta del .scene.json). None si no hay bake.
pub fn load_lightmap_runtime(scene_path: &str) -> Option<LightmapRuntime> {
    let stem = std::path::Path::new(scene_path)
        .file_stem()?
        .to_string_lossy()
        .replace(".scene", "");
    let json_path = format!("assets/lightmaps/{stem}.lm.json");
    let png_path = format!("assets/lightmaps/{stem}.lm.png");
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json_path).ok()?).ok()?;
    let img = image::open(&png_path).ok()?.to_rgba8();
    let atlas_size = meta["atlas_size"].as_u64()? as u32;
    if img.width() != atlas_size || img.height() != atlas_size {
        return None;
    }
    let entities: Vec<u64> = meta["entities"]
        .as_array()?
        .iter()
        .filter_map(|v| v.as_u64())
        .collect();
    let mut groups = Vec::with_capacity(entities.len());
    for ent_groups in meta["groups"].as_array()? {
        let mut gs = Vec::new();
        for g in ent_groups.as_array()? {
            gs.push(glass_core::lightmap::LmGroupRect {
                axis: g["axis"].as_u64()? as u8,
                negative: g["negative"].as_bool()?,
                x: g["x"].as_u64()? as u32,
                y: g["y"].as_u64()? as u32,
                w: g["w"].as_u64()? as u32,
                h: g["h"].as_u64()? as u32,
                min_u: g["min_u"].as_f64()? as f32,
                max_u: g["max_u"].as_f64()? as f32,
                min_v: g["min_v"].as_f64()? as f32,
                max_v: g["max_v"].as_f64()? as f32,
            });
        }
        groups.push(gs);
    }
    if groups.len() != entities.len() {
        return None;
    }
    Some(LightmapRuntime {
        atlas_size,
        rgba: img.into_raw(),
        entities,
        groups,
    })
}
