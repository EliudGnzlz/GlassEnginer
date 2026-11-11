//! Etapa 1 del bake de iluminación: generación de UV2 y empaquetado en atlas.

/// Malla de una entidad estática ya en espacio de mundo, lista para hornear.
pub struct BakeMesh {
    pub entity: u64,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// Rectángulo de una malla dentro del atlas, en téxeles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AtlasRect {
    pub entity: u64,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Resultado: tamaño del atlas, un rect por malla y las UV2 por vértice
/// (mismo orden que la entrada: un Vec por malla, un [u,v] por vértice,
/// en espacio de atlas 0..1).
/// Un grupo (eje dominante, signo) de una malla dentro del atlas:
/// su sub-rect en píxeles de atlas y sus cotas 2D de mundo (las de
/// la proyección: X→(z,y), Y→(x,z), Z→(x,y)).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LmGroupRect {
    pub axis: u8,
    pub negative: bool,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub min_u: f32,
    pub max_u: f32,
    pub min_v: f32,
    pub max_v: f32,
}

pub struct LightmapLayout {
    pub atlas_size: u32,
    pub rects: Vec<AtlasRect>,
    pub uv2: Vec<Vec<[f32; 2]>>,
    pub groups: Vec<Vec<LmGroupRect>>,
}

#[derive(Debug)]
pub enum BakeError {
    /// Alguna malla tiene datos incoherentes (índices fuera de rango o
    /// longitudes distintas entre posiciones y normales).
    InvalidMesh(u64),
    /// El atlas máximo no basta para todas las mallas.
    AtlasOverflow,
}

/// Genera las UV2 y el atlas.
/// - `texel_density`: téxeles por metro (p. ej. 8.0).
/// - Proyección planar por vértice según el eje dominante de su normal
///   (la de mayor componente absoluta): X→usa (z,y), Y→usa (x,z), Z→usa (x,y).
/// - El rect de cada malla mide (extensión_u * densidad) x (extensión_v *
///   densidad) téxeles, redondeado hacia arriba y con mínimo 4 por lado
///   y máximo 1024 por lado.
/// - Empaquetado por estanterías (shelf): ordena los rects por altura
///   descendente; el atlas empieza en 1024 y se duplica (2048, 4096)
///   mientras no quepan; si ni en 4096 caben, AtlasOverflow.
/// - 1 téxel de margen entre rects para que no sangren los vecinos.
/// - Las UV2 de cada vértice caen dentro de su rect (con medio téxel de
///   margen interior) y en 0..1.
// Bloque 1: reemplazo de layout_lightmap

#[derive(Clone, Copy)]
struct LmGroup {
    min_u: f32,
    max_u: f32,
    min_v: f32,
    max_v: f32,
    w: u32,
    h: u32,
    offset_x: u32,
}

struct LmPlan {
    expanded: BakeMesh,
    tri_axes: Vec<usize>,
    groups: [Option<LmGroup>; 6],
    rect_w: u32,
    rect_h: u32,
}

fn lm_sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn lm_cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn lm_len_sq(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

fn lm_normalize(v: [f32; 3]) -> [f32; 3] {
    let inv = 1.0 / lm_len_sq(v).sqrt();
    [v[0] * inv, v[1] * inv, v[2] * inv]
}

fn lm_dominant_axis(n: [f32; 3]) -> usize {
    let ax = n[0].abs();
    let ay = n[1].abs();
    let az = n[2].abs();
    if ax >= ay && ax >= az {
        0
    } else if ay >= az {
        1
    } else {
        2
    }
}

fn lm_project(p: [f32; 3], axis: usize) -> (f32, f32) {
    match axis {
        0 => (p[2], p[1]), // X -> (z, y)
        1 => (p[0], p[2]), // Y -> (x, z)
        _ => (p[0], p[1]), // Z -> (x, y)
    }
}

fn lm_extent_px(extent: f32, density: f32) -> u32 {
    let px = (extent * density).ceil();
    if !px.is_finite() {
        return 4;
    }
    px.max(4.0).min(1024.0) as u32
}

fn lm_pack_rects(rects: Vec<AtlasRect>) -> Result<(u32, Vec<AtlasRect>), BakeError> {
    const SIZES: [u32; 3] = [1024, 2048, 4096];
    const MARGIN: u32 = 1;

    let n = rects.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| rects[b].h.cmp(&rects[a].h).then(a.cmp(&b)));

    for &size in &SIZES {
        let mut placements = vec![(0u32, 0u32); n];
        let mut ok = true;
        let mut cursor_x = 0u32;
        let mut shelf_y = 0u32;
        let mut shelf_h = 0u32;

        for &i in &order {
            let w = rects[i].w;
            let h = rects[i].h;

            if w + MARGIN > size || h + MARGIN > size {
                ok = false;
                break;
            }

            if cursor_x + w + MARGIN > size {
                shelf_y += shelf_h + MARGIN;
                cursor_x = 0;
                shelf_h = 0;
            }

            if shelf_y + h + MARGIN > size {
                ok = false;
                break;
            }

            placements[i] = (cursor_x, shelf_y);
            cursor_x += w + MARGIN;
            if h > shelf_h {
                shelf_h = h;
            }
        }

        if ok {
            let mut out = Vec::with_capacity(n);
            for (i, r) in rects.iter().enumerate() {
                out.push(AtlasRect {
                    entity: r.entity,
                    x: placements[i].0,
                    y: placements[i].1,
                    w: r.w,
                    h: r.h,
                });
            }
            return Ok((size, out));
        }
    }

    Err(BakeError::AtlasOverflow)
}

/// Expande cada malla por caras y genera el layout del atlas.
/// Por cada triángulo: normal de cara = normalize(cross(p1-p0, p2-p0))
/// (si degenera, la media de las normales de vértice); eje dominante
/// de esa normal; proyección 2D (X→(z,y), Y→(x,z), Z→(x,y)). Cada
/// triángulo aporta 3 vértices NUEVOS (posición original, normal de
/// CARA) e índices secuenciales: la malla expandida no comparte
/// vértices entre caras, así ninguna cara pisa a otra en el atlas.
/// Las caras se agrupan por eje (≤3 grupos por malla); cada grupo
/// tiene su sub-rect dentro del rect de la malla, dimensionado por su
/// extensión 2D × texel_density (mín. 4, máx. 1024 por lado), con los
/// grupos colocados en fila (orden: altura descendente). Los rects de
/// malla se empaquetan en el atlas por estanterías como antes
/// (1024→2048→4096, 1 téxel de margen; si no cabe, AtlasOverflow).
/// UV2 dentro del sub-rect de su grupo, con medio téxel de margen.
/// Devuelve (layout, mallas expandidas en el mismo orden de entrada);
/// layout.uv2[i] corresponde a la malla expandida i.
pub fn expand_and_layout(
    meshes: &[BakeMesh],
    texel_density: f32,
) -> Result<(LightmapLayout, Vec<BakeMesh>), BakeError> {
    let density = if texel_density.is_finite() && texel_density > 0.0 {
        texel_density
    } else {
        1.0
    };

    let mut plans = Vec::with_capacity(meshes.len());

    for m in meshes {
        if m.positions.len() != m.normals.len() {
            return Err(BakeError::InvalidMesh(m.entity));
        }

        for &idx in &m.indices {
            if idx as usize >= m.positions.len() {
                return Err(BakeError::InvalidMesh(m.entity));
            }
        }

        let mut positions = Vec::with_capacity(m.indices.len());
        let mut normals = Vec::with_capacity(m.indices.len());
        let mut indices = Vec::with_capacity(m.indices.len());
        let mut tri_axes = Vec::with_capacity(m.indices.len() / 3);
        let mut bounds: [Option<(f32, f32, f32, f32)>; 6] = [None; 6];

        for tri in m.indices.chunks_exact(3) {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;

            let p0 = m.positions[i0];
            let p1 = m.positions[i1];
            let p2 = m.positions[i2];

            let n0 = m.normals[i0];
            let n1 = m.normals[i1];
            let n2 = m.normals[i2];

            let cr = lm_cross(lm_sub(p1, p0), lm_sub(p2, p0));
            let face = if lm_len_sq(cr) > 1e-20 {
                lm_normalize(cr)
            } else {
                let avg = [
                    n0[0] + n1[0] + n2[0],
                    n0[1] + n1[1] + n2[1],
                    n0[2] + n1[2] + n2[2],
                ];
                if lm_len_sq(avg) > 1e-20 {
                    lm_normalize(avg)
                } else {
                    [0.0, 1.0, 0.0]
                }
            };

            let axis = lm_dominant_axis(face);
            // Grupo = (eje, signo): las caras +Y y -Y proyectan sobre
            // el mismo plano 2D y se pisarían en el atlas si
            // compartieran sub-rect (suelo y techo de un cubo).
            let key = axis * 2 + (face[axis] < 0.0) as usize;

            let base = positions.len() as u32;
            positions.push(p0);
            positions.push(p1);
            positions.push(p2);
            // La normal de sombreador es la del vértice original (la
            // de cara solo elige el eje de proyección): el winding de
            // la malla no debe poder apagar una cara bien orientada.
            normals.push(n0);
            normals.push(n1);
            normals.push(n2);
            indices.push(base);
            indices.push(base + 1);
            indices.push(base + 2);
            tri_axes.push(key);

            for p in [p0, p1, p2] {
                let (u, v) = lm_project(p, axis);
                let slot = &mut bounds[key];
                match slot {
                    Some((min_u, max_u, min_v, max_v)) => {
                        if u < *min_u {
                            *min_u = u;
                        }
                        if u > *max_u {
                            *max_u = u;
                        }
                        if v < *min_v {
                            *min_v = v;
                        }
                        if v > *max_v {
                            *max_v = v;
                        }
                    }
                    None => *slot = Some((u, u, v, v)),
                }
            }
        }

        let mut groups: [Option<LmGroup>; 6] = [None, None, None, None, None, None];
        for axis in 0..6 {
            if let Some((min_u, max_u, min_v, max_v)) = bounds[axis] {
                let w = lm_extent_px(max_u - min_u, density);
                let h = lm_extent_px(max_v - min_v, density);
                groups[axis] = Some(LmGroup {
                    min_u,
                    max_u,
                    min_v,
                    max_v,
                    w,
                    h,
                    offset_x: 0,
                });
            }
        }

        let mut axes: Vec<usize> = (0..6).filter(|&a| groups[a].is_some()).collect();
        axes.sort_by(|&a, &b| {
            let ha = groups[a].as_ref().unwrap().h;
            let hb = groups[b].as_ref().unwrap().h;
            hb.cmp(&ha).then(a.cmp(&b))
        });

        let mut row_x = 0u32;
        let mut row_h = 0u32;
        for axis in axes {
            let g = groups[axis].as_mut().unwrap();
            g.offset_x = row_x;
            row_x += g.w;
            if g.h > row_h {
                row_h = g.h;
            }
        }

        let rect_w = if row_x == 0 { 1 } else { row_x };
        let rect_h = if row_h == 0 { 1 } else { row_h };

        plans.push(LmPlan {
            expanded: BakeMesh {
                entity: m.entity,
                positions,
                normals,
                indices,
            },
            tri_axes,
            groups,
            rect_w,
            rect_h,
        });
    }

    let input_rects: Vec<AtlasRect> = plans
        .iter()
        .map(|p| AtlasRect {
            entity: p.expanded.entity,
            x: 0,
            y: 0,
            w: p.rect_w,
            h: p.rect_h,
        })
        .collect();

    let (atlas_size, rects) = lm_pack_rects(input_rects)?;
    let inv_atlas = 1.0 / atlas_size as f32;

    let mut uv2 = Vec::with_capacity(plans.len());

    for (plan, rect) in plans.iter().zip(rects.iter()) {
        let mut uvs = vec![[0.0f32; 2]; plan.expanded.positions.len()];

        for (tri_idx, &key) in plan.tri_axes.iter().enumerate() {
            let axis = key / 2;
            let g = match plan.groups[key] {
                Some(g) => g,
                None => continue,
            };

            let base = tri_idx * 3;

            for k in 0..3 {
                let p = plan.expanded.positions[base + k];
                let (u, v) = lm_project(p, axis);

                let local_u = if g.max_u > g.min_u {
                    0.5 + ((u - g.min_u) / (g.max_u - g.min_u)) * (g.w as f32 - 1.0)
                } else {
                    g.w as f32 * 0.5
                };

                let local_v = if g.max_v > g.min_v {
                    0.5 + ((v - g.min_v) / (g.max_v - g.min_v)) * (g.h as f32 - 1.0)
                } else {
                    g.h as f32 * 0.5
                };

                uvs[base + k] = [
                    (rect.x as f32 + g.offset_x as f32 + local_u) * inv_atlas,
                    (rect.y as f32 + local_v) * inv_atlas,
                ];
            }
        }

        uv2.push(uvs);
    }

    let groups: Vec<Vec<LmGroupRect>> = plans
        .iter()
        .zip(rects.iter())
        .map(|(plan, rect)| {
            plan.groups
                .iter()
                .enumerate()
                .filter_map(|(key, group)| {
                    group.as_ref().map(|g| LmGroupRect {
                        axis: (key / 2) as u8,
                        negative: key % 2 == 1,
                        x: rect.x + g.offset_x,
                        y: rect.y,
                        w: g.w,
                        h: g.h,
                        min_u: g.min_u,
                        max_u: g.max_u,
                        min_v: g.min_v,
                        max_v: g.max_v,
                    })
                })
                .collect()
        })
        .collect();


    let expanded_meshes = plans.into_iter().map(|p| p.expanded).collect();

    Ok((
        LightmapLayout {
            atlas_size,
            rects,
            uv2,
            groups,
        },
        expanded_meshes,
    ))
}

// Bloque 2: tests de layout reescritos

#[cfg(test)]
mod tests {
    use super::*;

    fn quad_mesh(entity: u64) -> BakeMesh {
        BakeMesh {
            entity,
            positions: vec![
                [0.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [2.0, 0.0, 1.0],
                [0.0, 0.0, 1.0],
            ],
            normals: vec![[0.0, 1.0, 0.0]; 4],
            indices: vec![0, 2, 1, 0, 3, 2],
        }
    }

    fn cube_mesh(entity: u64) -> BakeMesh {
        BakeMesh {
            entity,
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 1.0],
                [0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
                [1.0, 1.0, 1.0],
                [0.0, 1.0, 1.0],
            ],
            normals: vec![[0.0, 1.0, 0.0]; 8],
            indices: vec![
                0, 1, 2, 0, 2, 3, // bottom -Y
                4, 6, 5, 4, 7, 6, // top +Y
                3, 2, 6, 3, 6, 7, // front +Z
                0, 5, 1, 0, 4, 5, // back -Z
                0, 3, 7, 0, 7, 4, // left -X
                1, 6, 2, 1, 5, 6, // right +X
            ],
        }
    }

    #[test]
    fn floor_quad_expands_and_gets_rect() {
        let (layout, expanded) = expand_and_layout(&[quad_mesh(7)], 8.0).unwrap();

        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0].positions.len(), 6);
        assert_eq!(expanded[0].indices.len(), 6);

        assert_eq!(layout.rects.len(), 1);
        let r = &layout.rects[0];
        assert_eq!(r.entity, 7);
        assert_eq!((r.w, r.h), (16, 8));

        assert_eq!(layout.uv2.len(), 1);
        assert_eq!(layout.uv2[0].len(), 6);

        let atlas = layout.atlas_size as f32;
        for &[u, v] in &layout.uv2[0] {
            assert!(u >= 0.0 && u <= 1.0);
            assert!(v >= 0.0 && v <= 1.0);

            let x = u * atlas;
            let y = v * atlas;
            assert!(x >= r.x as f32 && x <= (r.x + r.w) as f32);
            assert!(y >= r.y as f32 && y <= (r.y + r.h) as f32);
        }
    }

    #[test]
    fn cube_groups_do_not_overlap() {
        let (layout, expanded) = expand_and_layout(&[cube_mesh(1)], 8.0).unwrap();

        assert_eq!(expanded[0].positions.len(), 36);
        assert_eq!(expanded[0].indices.len(), 36);
        assert_eq!(layout.uv2[0].len(), 36);

        let mut min_u = [f32::INFINITY; 3];
        let mut max_u = [f32::NEG_INFINITY; 3];
        let mut count = [0usize; 3];

        // El grupo de cada vértice lo marca la normal de su CARA
        // (recalculada de las posiciones expandidas), no la normal de
        // sombreador: la expansión proyecta por caras.
        for tri in expanded[0].indices.chunks_exact(3) {
            let p0 = expanded[0].positions[tri[0] as usize];
            let p1 = expanded[0].positions[tri[1] as usize];
            let p2 = expanded[0].positions[tri[2] as usize];
            let face = lm_normalize(lm_cross(lm_sub(p1, p0), lm_sub(p2, p0)));
            let axis = lm_dominant_axis(face);
            // Grupo = (eje, signo): las caras +Y y -Y proyectan sobre
            // el mismo plano 2D y se pisarían en el atlas si
            // compartieran sub-rect (suelo y techo de un cubo).
            for &vi in tri {
                let u = layout.uv2[0][vi as usize][0];
                min_u[axis] = min_u[axis].min(u);
                max_u[axis] = max_u[axis].max(u);
                count[axis] += 1;
            }
        }

        let present: Vec<usize> = (0..3).filter(|&a| count[a] > 0).collect();
        assert_eq!(present.len(), 3);

        let mut disjoint = false;
        for i in 0..present.len() {
            for j in (i + 1)..present.len() {
                let a = present[i];
                let b = present[j];
                if max_u[a] < min_u[b] || max_u[b] < min_u[a] {
                    disjoint = true;
                }
            }
        }

        assert!(disjoint, "expected at least two group UV ranges to be disjoint");
    }

    #[test]
    fn two_meshes_have_non_overlapping_rects() {
        let (layout, _) = expand_and_layout(&[quad_mesh(1), quad_mesh(2)], 8.0).unwrap();

        assert_eq!(layout.rects.len(), 2);
        let a = &layout.rects[0];
        let b = &layout.rects[1];

        let overlap = a.x < b.x + b.w
            && b.x < a.x + a.w
            && a.y < b.y + b.h
            && b.y < a.y + a.h;

        assert!(!overlap);
    }

    #[test]
    fn out_of_range_index_is_invalid() {
        let mesh = BakeMesh {
            entity: 42,
            positions: vec![[0.0, 0.0, 0.0]; 3],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            indices: vec![0, 1, 3],
        };

        assert!(matches!(
            expand_and_layout(&[mesh], 8.0),
            Err(BakeError::InvalidMesh(42))
        ));
    }

    #[test]
    fn cube_groups_are_six_and_disjoint() {
        let (layout, _e) = expand_and_layout(&[cube_mesh(7)], 8.0).unwrap();
        let rect = layout.rects[0];
        let groups = &layout.groups[0];

        assert_eq!(groups.len(), 6);

        for g in groups {
            assert!(g.x >= rect.x && g.y >= rect.y);
            assert!(g.x + g.w <= rect.x + rect.w);
            assert!(g.y + g.h <= rect.y + rect.h);
        }

        for i in 0..groups.len() {
            for j in i + 1..groups.len() {
                let a = &groups[i];
                let b = &groups[j];
                let overlap = a.x < b.x + b.w
                    && b.x < a.x + a.w
                    && a.y < b.y + b.h
                    && b.y < a.y + a.h;
                assert!(!overlap, "grupos {i} y {j} se solapan");
            }
        }
    }

    #[test]
    fn layout_is_deterministic() {
        let mesh = cube_mesh(9);

        let (layout_a, meshes_a) =
            expand_and_layout(std::slice::from_ref(&mesh), 8.0).unwrap();
        let (layout_b, meshes_b) =
            expand_and_layout(std::slice::from_ref(&mesh), 8.0).unwrap();

        assert_eq!(layout_a.atlas_size, layout_b.atlas_size);
        assert_eq!(layout_a.rects.len(), layout_b.rects.len());

        for (a, b) in layout_a.rects.iter().zip(layout_b.rects.iter()) {
            assert_eq!((a.entity, a.x, a.y, a.w, a.h), (b.entity, b.x, b.y, b.w, b.h));
        }

        assert_eq!(layout_a.uv2, layout_b.uv2);
        assert_eq!(meshes_a.len(), meshes_b.len());

        for (a, b) in meshes_a.iter().zip(meshes_b.iter()) {
            assert_eq!(a.entity, b.entity);
            assert_eq!(a.positions, b.positions);
            assert_eq!(a.normals, b.normals);
            assert_eq!(a.indices, b.indices);
        }
    }
}

/// Luz direccional para el horneado: dirección HACIA la luz
/// (normalizada) y color ya multiplicado por la intensidad.
pub struct BakeSun {
    pub direction: [f32; 3],
    pub color: [f32; 3],
}

/// Luz puntual para el horneado, con la misma atenuación que el PBR
/// del motor: (1-(d/r)^4)^2 / (d^2+1), a cero fuera del rango.
pub struct BakePoint {
    pub position: [f32; 3],
    pub color: [f32; 3],
    pub range: f32,
}

/// Lightmap horneado: atlas RGBA8 (valores lineales recortados a
/// 0..1, sin gamma: la conversión la decide el runtime al muestrear).
pub struct BakedLightmap {
    pub size: u32,
    pub rgba: Vec<u8>,
}

fn lm_dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn lm_norm(v: [f32; 3]) -> [f32; 3] {
    let d = lm_dot(v, v).sqrt();
    if d > 1e-20 {
        [v[0] / d, v[1] / d, v[2] / d]
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// Hornea la iluminación estática en el atlas.
/// - Rasteriza cada triángulo en espacio de atlas usando las UV2,
///   interpolando posición y normal de mundo (baricéntricas).
/// - Por téxel cubierto: color = ambiente + sol * max(dot(n, dir), 0)
///   si `sun_visible(pos, dir)` lo permite + suma de puntuales con su
///   atenuación (sin oclusión en las puntuales en esta etapa).
/// - El origen del rayo de visibilidad se levanta 0.02 * normal para
///   evitar auto-oclusión.
/// - Al final dilata el resultado: 3 pasadas que rellenan téxeles
///   vacíos con la media de sus vecinos rellenos (evita costuras).
/// - Los téxeles nunca cubiertos quedan a 0.
pub fn bake_lightmap(
    meshes: &[BakeMesh],
    layout: &LightmapLayout,
    sun: &BakeSun,
    ambient: [f32; 3],
    points: &[BakePoint],
    sun_visible: &dyn Fn([f32; 3], [f32; 3]) -> bool,
) -> BakedLightmap {
    let size = layout.atlas_size as usize;
    if size == 0 {
        return BakedLightmap {
            size: layout.atlas_size,
            rgba: Vec::new(),
        };
    }

    let size_f = size as f32;
    let mut rgba = vec![0u8; size * size * 4];
    let mut filled = vec![false; size * size];

    let sun_dir = lm_norm(sun.direction);
    for (mesh_index, mesh) in meshes.iter().enumerate() {
        let vcount = mesh.positions.len();
        if vcount == 0 {
            continue;
        }

        let Some(uvs) = layout.uv2.get(mesh_index) else {
            continue;
        };
        let uvs: &[[f32; 2]] = uvs;

        for tri in mesh.indices.chunks_exact(3) {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;

            if i0 >= vcount || i1 >= vcount || i2 >= vcount {
                continue;
            }

            let p0 = mesh.positions[i0];
            let p1 = mesh.positions[i1];
            let p2 = mesh.positions[i2];

            let n0 = if i0 < mesh.normals.len() {
                lm_norm(mesh.normals[i0])
            } else {
                [0.0, 1.0, 0.0]
            };
            let n1 = if i1 < mesh.normals.len() {
                lm_norm(mesh.normals[i1])
            } else {
                [0.0, 1.0, 0.0]
            };
            let n2 = if i2 < mesh.normals.len() {
                lm_norm(mesh.normals[i2])
            } else {
                [0.0, 1.0, 0.0]
            };

            let a = [uvs[i0][0] * size_f, uvs[i0][1] * size_f];
            let b = [uvs[i1][0] * size_f, uvs[i1][1] * size_f];
            let c = [uvs[i2][0] * size_f, uvs[i2][1] * size_f];

            let min_x = a[0].min(b[0]).min(c[0]);
            let max_x = a[0].max(b[0]).max(c[0]);
            let min_y = a[1].min(b[1]).min(c[1]);
            let max_y = a[1].max(b[1]).max(c[1]);

            let x0 = min_x.floor().max(0.0) as i32;
            let x1 = max_x.ceil().min((size - 1) as f32) as i32;
            let y0 = min_y.floor().max(0.0) as i32;
            let y1 = max_y.ceil().min((size - 1) as f32) as i32;

            if x0 > x1 || y0 > y1 {
                continue;
            }

            let v0x = b[0] - a[0];
            let v0y = b[1] - a[1];
            let v1x = c[0] - a[0];
            let v1y = c[1] - a[1];
            let den = v0x * v1y - v0y * v1x;

            if den.abs() < 1e-12 {
                continue;
            }

            for py in y0..=y1 {
                for px in x0..=x1 {
                    let idx = py as usize * size + px as usize;
                    if filled[idx] {
                        continue;
                    }

                    let fx = px as f32 + 0.5;
                    let fy = py as f32 + 0.5;

                    let v2x = fx - a[0];
                    let v2y = fy - a[1];

                    let w1 = (v2x * v1y - v2y * v1x) / den;
                    let w2 = (v0x * v2y - v0y * v2x) / den;
                    let w0 = 1.0 - w1 - w2;

                    const EPS: f32 = 1e-6;
                    if w0 < -EPS || w1 < -EPS || w2 < -EPS {
                        continue;
                    }

                    let pos = [
                        p0[0] * w0 + p1[0] * w1 + p2[0] * w2,
                        p0[1] * w0 + p1[1] * w1 + p2[1] * w2,
                        p0[2] * w0 + p1[2] * w1 + p2[2] * w2,
                    ];

                    let mut n = [
                        n0[0] * w0 + n1[0] * w1 + n2[0] * w2,
                        n0[1] * w0 + n1[1] * w1 + n2[1] * w2,
                        n0[2] * w0 + n1[2] * w1 + n2[2] * w2,
                    ];
                    n = lm_norm(n);
                    if lm_dot(n, n) < 1e-12 {
                        n = n0;
                    }

                    let mut col = ambient;

                    let ndl_sun = lm_dot(n, sun_dir).max(0.0);
                    if ndl_sun > 0.0 {
                        let origin = [
                            pos[0] + n[0] * 0.02,
                            pos[1] + n[1] * 0.02,
                            pos[2] + n[2] * 0.02,
                        ];
                        if sun_visible(origin, sun_dir) {
                            col[0] += sun.color[0] * ndl_sun;
                            col[1] += sun.color[1] * ndl_sun;
                            col[2] += sun.color[2] * ndl_sun;
                        }
                    }

                    for pl in points {
                        if pl.range <= 0.0 {
                            continue;
                        }

                        let lx = pl.position[0] - pos[0];
                        let ly = pl.position[1] - pos[1];
                        let lz = pl.position[2] - pos[2];
                        let d2 = lx * lx + ly * ly + lz * lz;
                        let r2 = pl.range * pl.range;

                        if d2 >= r2 {
                            continue;
                        }

                        let (atten, ndl) = if d2 <= 1e-12 {
                            (1.0, 1.0)
                        } else {
                            let ratio = d2 / r2;
                            let t = 1.0 - ratio * ratio;
                            let atten = (t * t) / (d2 + 1.0);
                            let inv_d = 1.0 / d2.sqrt();
                            let ndl = (n[0] * lx + n[1] * ly + n[2] * lz) * inv_d;
                            (atten, ndl.max(0.0))
                        };

                        if ndl > 0.0 {
                            let k = atten * ndl;
                            col[0] += pl.color[0] * k;
                            col[1] += pl.color[1] * k;
                            col[2] += pl.color[2] * k;
                        }
                    }

                    let r = (col[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    let g = (col[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    let b = (col[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;

                    rgba[idx * 4] = r;
                    rgba[idx * 4 + 1] = g;
                    rgba[idx * 4 + 2] = b;
                    rgba[idx * 4 + 3] = 255;
                    filled[idx] = true;
                }
            }
        }
    }

    for _ in 0..3 {
        let mut updates: Vec<(usize, [u8; 3])> = Vec::new();

        for y in 0..size {
            for x in 0..size {
                let idx = y * size + x;
                if filled[idx] {
                    continue;
                }

                let mut sum = [0u32; 3];
                let mut count = 0u32;

                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }

                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;

                        if nx < 0 || ny < 0 || nx >= size as i32 || ny >= size as i32 {
                            continue;
                        }

                        let nidx = ny as usize * size + nx as usize;
                        if filled[nidx] {
                            sum[0] += rgba[nidx * 4] as u32;
                            sum[1] += rgba[nidx * 4 + 1] as u32;
                            sum[2] += rgba[nidx * 4 + 2] as u32;
                            count += 1;
                        }
                    }
                }

                if count > 0 {
                    updates.push((
                        idx,
                        [
                            (sum[0] / count) as u8,
                            (sum[1] / count) as u8,
                            (sum[2] / count) as u8,
                        ],
                    ));
                }
            }
        }

        if updates.is_empty() {
            break;
        }

        for (idx, rgb) in updates {
            rgba[idx * 4] = rgb[0];
            rgba[idx * 4 + 1] = rgb[1];
            rgba[idx * 4 + 2] = rgb[2];
            rgba[idx * 4 + 3] = 255;
            filled[idx] = true;
        }
    }

    BakedLightmap {
        size: layout.atlas_size,
        rgba,
    }
}

#[cfg(test)]
mod bake_tests {
    use super::*;

    fn floor_quad(normal: [f32; 3]) -> BakeMesh {
        BakeMesh {
            entity: Default::default(),
            positions: vec![
                [-1.0, 0.0, -1.0],
                [1.0, 0.0, -1.0],
                [1.0, 0.0, 1.0],
                [-1.0, 0.0, 1.0],
            ],
            normals: vec![normal; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }

    fn filled_reds(baked: &BakedLightmap) -> Vec<u8> {
        let mut out = Vec::new();
        let texels = (baked.size * baked.size) as usize;
        for i in 0..texels {
            if baked.rgba[i * 4 + 3] != 0 {
                out.push(baked.rgba[i * 4]);
            }
        }
        out
    }

    fn avg_red(baked: &BakedLightmap) -> f32 {
        let reds = filled_reds(baked);
        if reds.is_empty() {
            return 0.0;
        }
        let sum: u32 = reds.iter().map(|&r| r as u32).sum();
        sum as f32 / (reds.len() as f32 * 255.0)
    }

    #[test]
    fn sunlit_floor_is_bright() {
        let meshes = vec![floor_quad([0.0, 1.0, 0.0])];
        let (layout, meshes) = expand_and_layout(&meshes, 8.0).unwrap();
        let sun = BakeSun {
            direction: [0.0, 1.0, 0.0],
            color: [1.0, 1.0, 1.0],
        };

        let baked = bake_lightmap(
            &meshes,
            &layout,
            &sun,
            [0.1, 0.1, 0.1],
            &[],
            &|_, _| true,
        );

        let reds = filled_reds(&baked);
        assert!(!reds.is_empty());
        for r in reds {
            assert!(r as f32 / 255.0 > 0.9);
        }
    }

    #[test]
    fn occluded_sun_leaves_ambient() {
        let meshes = vec![floor_quad([0.0, 1.0, 0.0])];
        let (layout, meshes) = expand_and_layout(&meshes, 8.0).unwrap();
        let sun = BakeSun {
            direction: [0.0, 1.0, 0.0],
            color: [1.0, 1.0, 1.0],
        };

        let baked = bake_lightmap(
            &meshes,
            &layout,
            &sun,
            [0.1, 0.1, 0.1],
            &[],
            &|_, _| false,
        );

        let reds = filled_reds(&baked);
        assert!(!reds.is_empty());
        for r in reds {
            let f = r as f32 / 255.0;
            assert!(f >= 0.05 && f <= 0.2);
        }
    }

    #[test]
    fn backfacing_floor_gets_only_ambient() {
        let meshes = vec![floor_quad([0.0, -1.0, 0.0])];
        let (layout, meshes) = expand_and_layout(&meshes, 8.0).unwrap();
        let sun = BakeSun {
            direction: [0.0, 1.0, 0.0],
            color: [1.0, 1.0, 1.0],
        };

        let baked = bake_lightmap(
            &meshes,
            &layout,
            &sun,
            [0.1, 0.1, 0.1],
            &[],
            &|_, _| true,
        );

        let reds = filled_reds(&baked);
        assert!(!reds.is_empty());
        for r in reds {
            let f = r as f32 / 255.0;
            assert!(f >= 0.05 && f <= 0.2);
        }
    }

    #[test]
    fn nearby_point_light_adds_light() {
        let meshes = vec![floor_quad([0.0, 1.0, 0.0])];
        let (layout, meshes) = expand_and_layout(&meshes, 8.0).unwrap();
        let sun = BakeSun {
            direction: [0.0, 1.0, 0.0],
            color: [0.0, 0.0, 0.0],
        };
        let ambient = [0.1, 0.1, 0.1];

        let without = bake_lightmap(&meshes, &layout, &sun, ambient, &[], &|_, _| true);

        let points = [BakePoint {
            position: [0.0, 0.5, 0.0],
            color: [1.0, 1.0, 1.0],
            range: 5.0,
        }];

        let with = bake_lightmap(&meshes, &layout, &sun, ambient, &points, &|_, _| true);

        assert!(avg_red(&with) > avg_red(&without) + 0.1);
    }
}
