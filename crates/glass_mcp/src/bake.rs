//! Horneado de lightmaps de una escena (movido desde
//! glass_app::bake para que la tool MCP `glass_bake_lightmap`
//! y el comando sin interfaz GLASS_BAKE compartan el código).
use glass_core::lightmap::{
    bake_lightmap, expand_and_layout, AtlasRect, BakeMesh, BakePoint, BakeSun,
};
use glass_core::mat4_from_trs_euler;
use glass_core::scene::SceneState;
use glass_physics::PhysicsWorld;
use glass_renderer::mesh::{
    generate_capsule, generate_cylinder, generate_plane, generate_sphere, Vertex,
};

pub struct SceneBakeOutput {
    pub size: u32,
    pub rgba: Vec<u8>,
    pub rects: Vec<AtlasRect>,
    pub groups: Vec<Vec<glass_core::lightmap::LmGroupRect>>,
    pub baked_entities: Vec<u64>,
    pub skipped_models: Vec<String>,
}

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

pub fn bake_scene_lightmap(
    scene: &SceneState,
    physics: &std::sync::Arc<std::sync::Mutex<PhysicsWorld>>,
    texel_density: f32,
) -> Result<SceneBakeOutput, String> {
    let main = scene
        .main_light()
        .ok_or_else(|| "la escena no tiene luz principal".to_string())?;
    let main_light = main
        .light
        .as_ref()
        .ok_or_else(|| "la escena no tiene luz principal".to_string())?;

    let rot = main.transform.rotation();
    let lm = mat4_from_trs_euler([0.0; 3], rot, [1.0; 3]);
    let sun = BakeSun {
        direction: normalize([lm[8], lm[9], lm[10]]),
        color: [
            main_light.color[0] * main_light.intensity,
            main_light.color[1] * main_light.intensity,
            main_light.color[2] * main_light.intensity,
        ],
    };

    let mut points = Vec::new();
    for ent in &scene.entities {
        if !ent.active {
            continue;
        }
        let Some(light) = ent.light.as_ref() else {
            continue;
        };
        if light.light_type.as_str() == "Point" {
            let m = scene.compute_world_matrix(ent.id);
            points.push(BakePoint {
                position: [m[12], m[13], m[14]],
                color: [
                    light.color[0] * light.intensity,
                    light.color[1] * light.intensity,
                    light.color[2] * light.intensity,
                ],
                range: light.range,
            });
        }
    }

    let mut meshes = Vec::new();
    let mut baked_entities = Vec::new();
    let mut skipped_models = Vec::new();

    for ent in &scene.entities {
        if !ent.active
            || !(ent.is_static || ent.components.iter().any(|c| c == "StaticCollider"))
        {
            continue;
        }
        let Some(mesh) = ent.mesh.as_ref() else {
            continue;
        };

        let key = if mesh.model.is_empty() {
            "cube"
        } else {
            mesh.model.as_str()
        };

        let (verts, indices) = match key {
            "cube" => cube_mesh(),
            "plane" => generate_plane(1),
            "sphere" => generate_sphere(),
            "capsule" => generate_capsule(),
            "cylinder" => generate_cylinder(),
            _ => {
                if !skipped_models.iter().any(|m: &String| m.as_str() == key) {
                    skipped_models.push(key.to_string());
                }
                continue;
            }
        };

        let m = scene.compute_world_matrix(ent.id);
        let nm = normal_matrix(&m);

        let mut positions = Vec::with_capacity(verts.len());
        let mut normals = Vec::with_capacity(verts.len());

        for v in &verts {
            positions.push(transform_point(&m, v.position));
            normals.push(normalize(mul3(&nm, v.normal)));
        }

        meshes.push(BakeMesh {
            entity: ent.id,
            positions,
            normals,
            indices,
        });
        baked_entities.push(ent.id);
    }

    let (layout, meshes) =
        expand_and_layout(&meshes, texel_density).map_err(|e| format!("{:?}", e))?;

    let visible = |pos: [f32; 3], dir: [f32; 3]| -> bool {
        match physics.lock() {
            Ok(mut world) => world.cast_ray(pos, dir, 200.0, 0).is_none(),
            Err(_) => true,
        }
    };

    let baked = bake_lightmap(
        &meshes,
        &layout,
        &sun,
        [0.12, 0.13, 0.16],
        &points,
        &visible,
    );

    let rects = layout.rects.clone();
    let groups = layout.groups.clone();

    Ok(SceneBakeOutput {
        size: baked.size,
        rgba: baked.rgba,
        rects,
        groups,
        baked_entities,
        skipped_models,
    })
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-8 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        [0.0, 1.0, 0.0]
    }
}

fn transform_point(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

fn mul3(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn normal_matrix(m: &[f32; 16]) -> [[f32; 3]; 3] {
    let a = m[0];
    let b = m[4];
    let c = m[8];
    let d = m[1];
    let e = m[5];
    let f = m[9];
    let g = m[2];
    let h = m[6];
    let i = m[10];

    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);

    if det.abs() < 1e-12 {
        return [[a, b, c], [d, e, f], [g, h, i]];
    }

    let inv_det = 1.0 / det;
    [
        [
            (e * i - f * h) * inv_det,
            (f * g - d * i) * inv_det,
            (d * h - e * g) * inv_det,
        ],
        [
            (c * h - b * i) * inv_det,
            (a * i - c * g) * inv_det,
            (b * g - a * h) * inv_det,
        ],
        [
            (b * f - c * e) * inv_det,
            (c * d - a * f) * inv_det,
            (a * e - b * d) * inv_det,
        ],
    ]
}

fn cube_mesh() -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);

    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [1.0, 0.0, 0.0],
            [
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [0.5, 0.5, -0.5],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-0.5, -0.5, 0.5],
                [-0.5, -0.5, -0.5],
                [-0.5, 0.5, -0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, -0.5],
            ],
        ),
        (
            [0.0, 0.0, 1.0],
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, -0.5],
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
            ],
        ),
    ];

    let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

    for (normal, corners) in faces {
        let base = vertices.len() as u32;
        for i in 0..4 {
            vertices.push(Vertex {
                position: corners[i],
                normal,
                uv: uvs[i],
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    (vertices, indices)
}

/// Escribe el atlas PNG (codificado en sRGB, como espera el
/// runtime) y el JSON de metadatos en
/// `<dir>/<nombre>.lm.{png,json}`. Devuelve las dos rutas.
pub fn write_lightmap_files(
    dir: &str,
    nombre: &str,
    out: &SceneBakeOutput,
) -> Result<(String, String), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let png = format!("{dir}/{nombre}.lm.png");
    let json_path = format!("{dir}/{nombre}.lm.json");
    let mut srgb = out.rgba.clone();
    for px in srgb.chunks_exact_mut(4) {
        for ch in px.iter_mut().take(3) {
            let l = *ch as f32 / 255.0;
            let s = if l <= 0.003_130_8 {
                l * 12.92
            } else {
                1.055 * l.powf(1.0 / 2.4) - 0.055
            };
            *ch = (s * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    let img = image::RgbaImage::from_raw(out.size, out.size, srgb)
        .ok_or_else(|| "atlas de tamaño inválido".to_string())?;
    img.save(&png).map_err(|e| e.to_string())?;
    let rects: Vec<_> = out
        .rects
        .iter()
        .map(|r| {
            serde_json::json!({
                "entity": r.entity, "x": r.x, "y": r.y, "w": r.w, "h": r.h,
            })
        })
        .collect();
    let groups: Vec<_> = out
        .groups
        .iter()
        .map(|gs| {
            gs.iter()
                .map(|g| {
                    serde_json::json!({
                        "axis": g.axis, "negative": g.negative,
                        "x": g.x, "y": g.y, "w": g.w, "h": g.h,
                        "min_u": g.min_u, "max_u": g.max_u,
                        "min_v": g.min_v, "max_v": g.max_v,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let meta = serde_json::json!({
        "atlas_size": out.size,
        "entities": out.baked_entities,
        "rects": rects,
        "groups": groups,
        "skipped_models": out.skipped_models,
    });
    std::fs::write(&json_path, serde_json::to_string_pretty(&meta).unwrap_or_default())
        .map_err(|e| e.to_string())?;
    Ok((png, json_path))
}
