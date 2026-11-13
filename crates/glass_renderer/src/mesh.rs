use ash::vk;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator};
use gpu_allocator::MemoryLocation;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

/// Vértice de una malla CON piel: el `Vertex` de siempre más las 4
/// influencias de articulación (`JOINTS_0`/`WEIGHTS_0` de glTF). Solo lo
/// usan las mallas con piel, que van por su propio pipeline
/// (`pbr_skinned.vert`); el formato compartido no cambia, así que las
/// mallas sin piel siguen pesando 32 B/vértice y su camino es idéntico.
///
/// Offsets (los declara el pipeline y el test de contrato los vigila):
/// position 0, normal 12, uv 24, joints 32 (u16×4), weights 40 (f32×4).
/// Total: 56 bytes, sin relleno.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SkinnedVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Índices DENTRO de la lista de articulaciones de la piel (no índices
    /// de nodo), como manda glTF.
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

pub struct GpuMesh {
    pub vertex_buffer: vk::Buffer,
    pub vertex_allocation: Option<Allocation>,
    pub index_buffer: vk::Buffer,
    pub index_allocation: Option<Allocation>,
    pub index_count: u32,
    pub vertex_count: u32,
}

/// Una textura ya subida a VRAM, lista para muestrear desde el shader PBR.
pub struct GpuTexture {
    pub image: vk::Image,
    pub allocation: Option<Allocation>,
    pub image_view: vk::ImageView,
    pub width: u32,
    pub height: u32,
    /// Formato Vulkan de la imagen (Fase 2: RGBA8 o BC según la ruta).
    pub format: vk::Format,
    /// Niveles de la cadena de mip-maps subida.
    pub mip_levels: u32,
    /// Bytes que ocupa la cadena completa en VRAM (todos los niveles).
    pub vram_bytes: u64,
}

impl GpuTexture {
    pub fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        unsafe {
            device.destroy_image_view(self.image_view, None);
            if let Some(alloc) = self.allocation.take() {
                let _ = allocator.free(alloc);
            }
            device.destroy_image(self.image, None);
        }
    }
}

impl GpuMesh {
    pub fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        unsafe {
            if let Some(alloc) = self.vertex_allocation.take() {
                let _ = allocator.free(alloc);
                device.destroy_buffer(self.vertex_buffer, None);
            }
            if let Some(alloc) = self.index_allocation.take() {
                let _ = allocator.free(alloc);
                device.destroy_buffer(self.index_buffer, None);
            }
        }
    }
}

/// Material PBR de una primitiva glTF, tal como lo declara el archivo.
/// Es la mitad CPU de la importación: el render lo consume para rellenar
/// `material_params`/`object_color` en vez de valores fijos, y la textura
/// base (si la hay) se sube a GPU desde `LoadedModel::images`.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfMaterial {
    pub name: String,
    /// Factor RGBA que multiplica al albedo (baseColorFactor).
    pub base_color_factor: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    /// Índice dentro de `LoadedModel::images` de la textura base, si el
    /// material declara baseColorTexture.
    pub base_color_image: Option<usize>,
}

/// Imagen decodificada de un glTF, normalizada a RGBA8.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfImageData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Rango de índices que ocupa una primitiva dentro de `LoadedModel::indices`,
/// junto con el material que declara. Es lo que permite dibujar cada
/// primitiva con SU material en vez del principal de todo el modelo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimitiveRange {
    /// Primer índice de la primitiva dentro de `LoadedModel::indices`.
    pub first_index: u32,
    /// Cuántos índices ocupa la primitiva.
    pub index_count: u32,
    /// Índice en `LoadedModel::materials` de su material (`None` si no declara).
    pub material: Option<usize>,
}

/// Resultado completo de importar un .glb: geometría fusionada (como antes)
/// más los materiales por primitiva y las imágenes decodificadas.
#[derive(Debug, Clone)]
pub struct LoadedModel {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub materials: Vec<GltfMaterial>,
    pub images: Vec<GltfImageData>,
    /// Material de cada primitiva, en el orden en que se fusionó la
    /// geometría (`None` = la primitiva no declara material).
    pub primitive_materials: Vec<Option<usize>>,
    /// Rangos de índices por primitiva, en el orden en que se fusionó la geometría.
    pub primitive_ranges: Vec<PrimitiveRange>,
    /// Piel del modelo, si algún nodo-malla declara una. `None` = modelo
    /// rígido: todo el bloque de skinning se salta y el modelo se dibuja
    /// exactamente como antes de que existiera.
    pub skin: Option<crate::skin::SkinData>,
    /// Clips de animación del documento (vacío si no trae ninguno).
    pub animations: Vec<crate::skin::AnimationClip>,
    /// `JOINTS_0` por vértice, alineado con `vertices`. Solo se rellena si
    /// el modelo tiene piel; las primitivas sin esos atributos aportan
    /// ceros (el shader las deja pasar sin deformar).
    pub vertex_joints: Vec<[u16; 4]>,
    /// `WEIGHTS_0` por vértice, alineado con `vertices` y `vertex_joints`.
    pub vertex_weights: Vec<[f32; 4]>,
    /// Objetos del documento (Fase 6): un registro por nodo con
    /// malla (o por malla sin nodo), con su transformación global y
    /// los rangos que aportó a la fusión. Varios objetos con el
    /// mismo `mesh_name` = instancias enlazadas de la misma malla.
    pub objects: Vec<ModelObject>,
}

/// Un objeto del documento glTF (Fase 6): su nombre de nodo, la
/// malla que referencia, su transformación global y los índices de
/// `LoadedModel::primitive_ranges` que salieron de él.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelObject {
    pub name: String,
    pub mesh_name: String,
    pub transform: [f32; 16],
    pub ranges: Vec<usize>,
}

#[cfg(test)]
mod lod_tests {
    use super::{lod_level_for_distance, lod_suffix};

    #[test]
    fn sufijo_lod_se_parsea() {
        assert_eq!(lod_suffix("Esfera_LOD2"), Some(("Esfera".to_string(), 2)));
        assert_eq!(lod_suffix("Esfera_LOD0"), Some(("Esfera".to_string(), 0)));
        assert_eq!(lod_suffix("Cubo"), None);
        assert_eq!(lod_suffix("Cubo_LODx"), None);
        assert_eq!(lod_suffix("Cubo_LOD"), None);
    }

    #[test]
    fn sufijo_lod_con_duplicado_de_blender_y_minusculas() {
        // Auditoría Fase 6 (M-16): el ".001" de duplicado de Blender
        // y las minúsculas ya no pierden el LOD.
        assert_eq!(lod_suffix("Esfera_LOD2.001"), Some(("Esfera".to_string(), 2)));
        assert_eq!(lod_suffix("Esfera_lod1"), Some(("Esfera".to_string(), 1)));
        assert_eq!(lod_suffix("Esfera_LOD0.042"), Some(("Esfera".to_string(), 0)));
    }

    #[test]
    fn nivel_por_distancia_entre_disponibles() {
        let disp = [0, 1, 2];
        assert_eq!(lod_level_for_distance(&disp, 3.0), 0);
        assert_eq!(lod_level_for_distance(&disp, 30.0), 1);
        assert_eq!(lod_level_for_distance(&disp, 90.0), 2);
        // Sin el nivel deseado: el mayor disponible por debajo.
        assert_eq!(lod_level_for_distance(&[0, 2], 30.0), 0);
        assert_eq!(lod_level_for_distance(&[0, 2], 90.0), 2);
        // Solo un nivel alto disponible: se usa ese.
        assert_eq!(lod_level_for_distance(&[2], 3.0), 2);
        // Niveles por encima de 2 alcanzables (pasada de medios):
        // las bandas se duplican (90, 180, ...).
        let disp4 = [0, 1, 2, 3];
        assert_eq!(lod_level_for_distance(&disp4, 100.0), 3);
        assert_eq!(lod_level_for_distance(&disp4, 50.0), 2);
        assert_eq!(lod_level_for_distance(&disp4, 500.0), 3);
    }
}

/// Nivel LOD de un nombre por el sufijo `_LOD<n>` (Fase 6):
/// "Esfera_LOD2" → Some(("Esfera", 2)). Sin sufijo válido → None.
pub fn lod_suffix(name: &str) -> Option<(String, u32)> {
    // Quita el sufijo de duplicado de Blender (".NNN", 3 dígitos) al final.
    let mut base = name;
    if let Some(dot) = base.rfind('.') {
        let dup = &base[dot + 1..];
        if dup.len() == 3 && dup.chars().all(|c| c.is_ascii_digit()) {
            base = &base[..dot];
        }
    }
    // Búsqueda del marcador insensible a mayúsculas (byte-length se conserva).
    let lower = base.to_ascii_lowercase();
    let idx = lower.rfind("_lod")?;
    let num = &base[idx + 4..];
    if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((base[..idx].to_string(), num.parse().ok()?))
}

/// Distancias a las que cambia el nivel LOD deseado (Fase 6).
pub const LOD_NEAR: f32 = 20.0;
pub const LOD_FAR: f32 = 45.0;

/// Nivel LOD a dibujar entre los DISPONIBLES para una distancia:
/// el deseado es 0 (<LOD_NEAR), 1 (<LOD_FAR) o 2; se elige el
/// mayor disponible ≤ deseado, o el menor disponible si no hay
/// ninguno ≤. `available` no puede venir vacío.
pub fn lod_level_for_distance(available: &[u32], distance: f32) -> u32 {
    // Bandas: <LOD_NEAR → 0, <LOD_FAR → 1, y a partir de LOD_FAR
    // cada nivel duplica la distancia anterior (90, 180, 360, …).
    // Auditoría (menor): antes el nivel deseado se topaba en 2 y los
    // niveles 3+ de un modelo eran inalcanzables.
    let desired = if distance < LOD_NEAR {
        0
    } else if distance < LOD_FAR {
        1
    } else {
        let mut nivel = 2u32;
        let mut umbral = LOD_FAR * 2.0;
        while distance >= umbral && nivel < 16 {
            nivel += 1;
            umbral *= 2.0;
        }
        nivel
    };
    let mut best: Option<u32> = None;
    for &l in available {
        if l <= desired && best.map(|b| l > b).unwrap_or(true) {
            best = Some(l);
        }
    }
    best.unwrap_or_else(|| *available.iter().min().unwrap_or(&0))
}

impl LoadedModel {
    /// Material «principal» del modelo: el de su primera primitiva.
    pub fn primary_material(&self) -> Option<&GltfMaterial> {
        self.primitive_materials
            .first()
            .copied()
            .flatten()
            .and_then(|i| self.materials.get(i))
    }
}

/// Convierte los píxeles que devuelve la crate `gltf` a RGBA8.
fn image_to_rgba(data: &gltf::image::Data) -> Vec<u8> {
    use gltf::image::Format;
    let count = (data.width * data.height) as usize;
    let mut out = Vec::with_capacity(count * 4);
    match data.format {
        Format::R8G8B8A8 => out.extend_from_slice(&data.pixels),
        Format::R8G8B8 => {
            for px in data.pixels.chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        Format::R8G8 => {
            for px in data.pixels.chunks_exact(2) {
                out.extend_from_slice(&[px[0], px[1], 0, 255]);
            }
        }
        Format::R8 => {
            for &v in &data.pixels {
                out.extend_from_slice(&[v, v, v, 255]);
            }
        }
        Format::R16G16B16A16 => {
            for px in data.pixels.chunks_exact(8) {
                for c in 0..4 {
                    let v = u16::from_le_bytes([px[c * 2], px[c * 2 + 1]]);
                    out.push((v / 257) as u8);
                }
            }
        }
        Format::R16G16B16 => {
            for px in data.pixels.chunks_exact(6) {
                for c in 0..3 {
                    let v = u16::from_le_bytes([px[c * 2], px[c * 2 + 1]]);
                    out.push((v / 257) as u8);
                }
                out.push(255);
            }
        }
        Format::R16G16 => {
            for px in data.pixels.chunks_exact(4) {
                for c in 0..2 {
                    let v = u16::from_le_bytes([px[c * 2], px[c * 2 + 1]]);
                    out.push((v / 257) as u8);
                }
                out.extend_from_slice(&[0, 255]);
            }
        }
        Format::R16 => {
            for px in data.pixels.chunks_exact(2) {
                let v = u16::from_le_bytes([px[0], px[1]]);
                let b = (v / 257) as u8;
                out.extend_from_slice(&[b, b, b, 255]);
            }
        }
        Format::R32G32B32A32FLOAT => {
            for px in data.pixels.chunks_exact(16) {
                for c in 0..4 {
                    let v = f32::from_le_bytes([px[c * 4], px[c * 4 + 1], px[c * 4 + 2], px[c * 4 + 3]]);
                    out.push((v.clamp(0.0, 1.0) * 255.0) as u8);
                }
            }
        }
        Format::R32G32B32FLOAT => {
            for px in data.pixels.chunks_exact(12) {
                for c in 0..3 {
                    let v = f32::from_le_bytes([px[c * 4], px[c * 4 + 1], px[c * 4 + 2], px[c * 4 + 3]]);
                    out.push((v.clamp(0.0, 1.0) * 255.0) as u8);
                }
                out.push(255);
            }
        }
    }
    out
}

/// Extrae la piel de un documento glTF: el grafo de nodos (padres y TRS de
/// reposo), las articulaciones y las matrices inversas de unión del primer
/// nodo que lleve malla Y piel. Sin ese nodo, el modelo es rígido y se
/// devuelve `None` (el resto del motor ni se entera de que existe esto).
///
/// Límite declarado: UNA piel por modelo (si el archivo declara más, se
/// usa la primera y se avisa). Una piel por encima de
/// `skin::MAX_JOINTS` articulaciones es un ERROR de importación claro,
/// no una corrupción silenciosa.
fn parse_skin(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
) -> Result<Option<crate::skin::SkinData>, String> {
    use crate::skin::{GltfNodeData, SkinData, MAX_JOINTS};

    let node_list: Vec<gltf::Node> = document.nodes().collect();
    // Padres desde los hijos: el documento no garantiza que un padre tenga
    // un índice menor que sus hijos.
    let mut parents: Vec<Option<usize>> = vec![None; node_list.len()];
    for node in &node_list {
        for child in node.children() {
            parents[child.index()] = Some(node.index());
        }
    }

    let mut found: Option<(usize, gltf::Skin)> = None;
    let mut skin_nodes = 0usize;
    for node in &node_list {
        if node.skin().is_some() {
            skin_nodes += 1;
            if node.mesh().is_some() && found.is_none() {
                found = Some((node.index(), node.skin().unwrap()));
            }
        }
    }
    if skin_nodes > 1 {
        log::warn!(
            "glTF con {} nodos con piel: el motor usa solo la primera piel de un modelo (límite declarado)",
            skin_nodes
        );
    }
    let Some((mesh_node, skin)) = found else {
        return Ok(None);
    };

    let joints: Vec<usize> = skin.joints().map(|j| j.index()).collect();
    if joints.len() > MAX_JOINTS {
        return Err(format!(
            "La piel tiene {} articulaciones y el máximo del motor es {} (skin::MAX_JOINTS)",
            joints.len(),
            MAX_JOINTS
        ));
    }
    if joints.is_empty() {
        return Err("La piel del glTF no declara articulaciones".into());
    }

    let reader = skin.reader(|buffer| Some(&buffers[buffer.index()]));
    // Las matrices de la crate vienen como [[f32;4];4] por columnas:
    // concatenar las columnas da el [f32;16] column-major de glass_core.
    let inverse_bind_matrices: Vec<[f32; 16]> = reader
        .read_inverse_bind_matrices()
        .map(|iter| {
            iter.map(|m| {
                let mut flat = [0.0f32; 16];
                for c in 0..4 {
                    flat[c * 4..c * 4 + 4].copy_from_slice(&m[c]);
                }
                flat
            })
            .collect()
        })
        .unwrap_or_default();
    if inverse_bind_matrices.len() != joints.len() {
        return Err(format!(
            "La piel declara {} articulaciones pero {} matrices inversas de unión",
            joints.len(),
            inverse_bind_matrices.len()
        ));
    }

    let nodes: Vec<GltfNodeData> = node_list
        .iter()
        .map(|n| {
            let (translation, rotation, scale) = n.transform().decomposed();
            GltfNodeData {
                parent: parents[n.index()],
                translation,
                rotation,
                scale,
            }
        })
        .collect();

    Ok(Some(SkinData {
        nodes,
        joints,
        inverse_bind_matrices,
        mesh_node,
    }))
}

/// Recoge los valores de salida de un canal quedándose, si el sampler es
/// CUBICSPLINE, solo con el valor central de cada keyframe (el formato
/// trae tangente-entrada, valor, tangente-salida por keyframe y las
/// tangentes se ignoran: ver `parse_animations`).
fn collect_channel_values<T>(iter: impl Iterator<Item = T>, cubic_spline: bool) -> Vec<T> {
    if cubic_spline {
        iter.enumerate()
            .filter(|(i, _)| i % 3 == 1)
            .map(|(_, v)| v)
            .collect()
    } else {
        iter.collect()
    }
}

/// Extrae los clips de animación del documento. Los canales de pesos de
/// morph targets se saltan (fuera de alcance del bloque de skinning).
/// CUBICSPLINE se degrada a LINEAR con el valor central de cada keyframe:
/// simplificación deliberada — la curva pierde suavidad, no corrección.
fn parse_animations(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
) -> Vec<crate::skin::AnimationClip> {
    use crate::skin::{AnimChannel, AnimChannelValues, AnimationClip, Interpolation as SkinInterp};
    use gltf::animation::util::ReadOutputs;

    let mut clips = Vec::new();
    for (anim_idx, anim) in document.animations().enumerate() {
        let mut channels = Vec::new();
        let mut duration = 0.0f32;
        for channel in anim.channels() {
            let node = channel.target().node().index();
            let interpolation_raw = channel.sampler().interpolation();
            let cubic = interpolation_raw == gltf::animation::Interpolation::CubicSpline;
            let reader = channel.reader(|buffer| Some(&buffers[buffer.index()]));
            let Some(times) = reader.read_inputs().map(|i| i.collect::<Vec<f32>>()) else {
                continue;
            };
            if let Some(last) = times.last() {
                duration = duration.max(*last);
            }
            let Some(outputs) = reader.read_outputs() else {
                continue;
            };
            let values = match outputs {
                ReadOutputs::Translations(it) => {
                    AnimChannelValues::Translation(collect_channel_values(it, cubic))
                }
                ReadOutputs::Rotations(it) => {
                    AnimChannelValues::Rotation(collect_channel_values(it.into_f32(), cubic))
                }
                ReadOutputs::Scales(it) => AnimChannelValues::Scale(collect_channel_values(it, cubic)),
                ReadOutputs::MorphTargetWeights(_) => continue,
            };
            let interpolation = match interpolation_raw {
                gltf::animation::Interpolation::Step => SkinInterp::Step,
                _ => SkinInterp::Linear,
            };
            channels.push(AnimChannel {
                node,
                interpolation,
                times,
                values,
            });
        }
        let name = anim
            .name()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Clip {}", anim_idx));
        clips.push(AnimationClip {
            name,
            duration,
            channels,
        });
    }
    clips
}

pub struct MeshLoader;

/// Segmentos por lado de la primitiva Plano integrada (64×64 celdas =
/// 4.225 vértices): densidad de sobra para que un shader de grafo con
/// etapa de vértices ondule la superficie sin facetas visibles a las
/// amplitudes de trabajo (centímetros) — y coste despreciable en GPU.
pub const PLANE_SEGMENTS: u32 = 64;

/// Genera la geometría de la primitiva Plano: una rejilla de
/// `segments`×`segments` celdas sobre el plano XZ, mirando a +Y, que
/// abarca 1×1 en espacio de objeto (x, z ∈ [−0,5, 0,5]) — la misma
/// convención de tamaño que el cubo por defecto a escala 1 — con UV
/// 0..1 (u a lo largo de +X, v a lo largo de +Z) y normales +Y.
///
/// Hasta este generador NO existía plano en el motor: el preset «Plano»
/// del editor era el cubo aplastado, sin vértices interiores que un
/// shader pudiera desplazar. Los triángulos se enrollan (a, c, b) /
/// (b, c, d) para que su normal geométrica apunte a +Y, como las
/// normales declaradas (el pipeline no descarta caras, pero la
/// convención queda correcta).
pub fn generate_plane(segments: u32) -> (Vec<Vertex>, Vec<u32>) {
    let seg = segments.max(1);
    let side = seg + 1;
    let mut vertices = Vec::with_capacity((side * side) as usize);
    for j in 0..side {
        let v = j as f32 / seg as f32;
        for i in 0..side {
            let u = i as f32 / seg as f32;
            vertices.push(Vertex {
                position: [u - 0.5, 0.0, v - 0.5],
                normal: [0.0, 1.0, 0.0],
                uv: [u, v],
            });
        }
    }
    let mut indices = Vec::with_capacity((seg * seg * 6) as usize);
    for j in 0..seg {
        for i in 0..seg {
            let a = j * side + i;
            let b = a + 1;
            let c = a + side;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    (vertices, indices)
}

/// Columnas (segmentos longitudinales) de la esfera integrada.
pub const SPHERE_WIDTH_SEGMENTS: u32 = 32;
/// Filas (segmentos de latitud) de la esfera integrada.
pub const SPHERE_HEIGHT_SEGMENTS: u32 = 16;
/// Columnas de la cápsula integrada.
pub const CAPSULE_RADIAL_SEGMENTS: u32 = 32;
/// Anillos por casquete de la cápsula integrada (del polo al ecuador).
pub const CAPSULE_CAP_RINGS: u32 = 8;
/// Columnas del cilindro integrado.
pub const CYLINDER_RADIAL_SEGMENTS: u32 = 32;

/// Genera la geometría de la primitiva Esfera: esfera UV de radio
/// `glass_core::primitives::SPHERE_RADIUS` (0,5: diámetro 1, la
/// convención de Unity) centrada en el origen, con normales = la
/// propia dirección radial y UV esférica estándar (u da la vuelta
/// completa, v va de 1 en el polo superior a 0 en el inferior).
///
/// Hasta este generador la «esfera» del editor era la malla de cubo:
/// no existía ninguna esfera en el motor. La columna de la costura
/// (θ = 2π) duplica los vértices de θ = 0 para que la UV no se
/// estire en el cierre, como en cualquier esfera UV.
pub fn generate_sphere() -> (Vec<Vertex>, Vec<u32>) {
    use glass_core::primitives::SPHERE_RADIUS;
    let r = SPHERE_RADIUS;
    let w = SPHERE_WIDTH_SEGMENTS;
    let h = SPHERE_HEIGHT_SEGMENTS;
    let mut vertices = Vec::with_capacity(((w + 1) * (h + 1)) as usize);
    for j in 0..=h {
        // φ: 0 en el polo superior → π en el inferior. En los polos el
        // seno se fuerza a 0 exacto: sin(π) en f32 no es 0 (≈ −8,7e−8)
        // y dejaría los vértices del polo repartidos en un micro-círculo
        // con triángulos astilla; con el snap, el polo es un punto.
        let phi = std::f32::consts::PI * j as f32 / h as f32;
        let (sin_phi, cos_phi) = if j == 0 || j == h {
            (0.0, phi.cos())
        } else {
            phi.sin_cos()
        };
        for i in 0..=w {
            let theta = 2.0 * std::f32::consts::PI * i as f32 / w as f32;
            let (sin_theta, cos_theta) = theta.sin_cos();
            let dir = [sin_phi * cos_theta, cos_phi, sin_phi * sin_theta];
            vertices.push(Vertex {
                position: [dir[0] * r, dir[1] * r, dir[2] * r],
                normal: dir,
                uv: [i as f32 / w as f32, 1.0 - j as f32 / h as f32],
            });
        }
    }
    // Enrollado (a, b, c) / (b, d, c): el que deja la normal
    // geométrica hacia fuera (verificado por el test de orientación
    // y por cálculo numérico independiente).
    let mut indices = Vec::with_capacity((w * h * 6) as usize);
    let side = w + 1;
    for j in 0..h {
        for i in 0..w {
            let a = j * side + i;
            let b = a + 1;
            let c = a + side;
            let d = c + 1;
            indices.extend_from_slice(&[a, b, c, b, d, c]);
        }
    }
    (vertices, indices)
}

/// Genera la geometría de la primitiva Cápsula: superficie de
/// revolución sobre +Y con el radio y la altura de
/// `glass_core::primitives` (radio 0,5; tramo recto de 1 entre
/// y = −0,5 e y = +0,5; casquetes semiesféricos; altura total 2, la
/// cápsula de Unity). Se construye por anillos de abajo a arriba:
/// casquete inferior (del polo al ecuador), banda cilíndrica y
/// casquete superior. Las normales salen de la propia revolución
/// (radial en la banda, esféricas en los casquetes) y son continuas
/// en el ecuador, donde ambas fórmulas coinciden.
pub fn generate_capsule() -> (Vec<Vertex>, Vec<u32>) {
    use glass_core::primitives::{CAPSULE_CYLINDER_HALF_HEIGHT, CAPSULE_RADIUS};
    let r = CAPSULE_RADIUS;
    let ch = CAPSULE_CYLINDER_HALF_HEIGHT;
    let radial = CAPSULE_RADIAL_SEGMENTS;
    let cap = CAPSULE_CAP_RINGS;
    // Perfil: (centro_y_del_casquete, ángulo) por anillo. El ángulo va
    // de −π/2 (polo inferior) a +π/2 (polo superior); el anillo del
    // ecuador inferior (ángulo 0, centro −ch) y el del superior
    // (ángulo 0, centro +ch) son las dos orillas de la banda.
    let mut profile: Vec<(f32, f32)> = Vec::with_capacity((2 * (cap + 1)) as usize);
    for k in 0..=cap {
        profile.push((-ch, -std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_2 * k as f32 / cap as f32));
    }
    for k in 0..=cap {
        profile.push((ch, std::f32::consts::FRAC_PI_2 * k as f32 / cap as f32));
    }
    let rings = profile.len() as u32;
    let mut vertices = Vec::with_capacity((rings * (radial + 1)) as usize);
    for (ring_idx, &(center_y, ang)) in profile.iter().enumerate() {
        let (sin_a, cos_a) = ang.sin_cos();
        let y = center_y + r * sin_a;
        // En los polos cos(±π/2) en f32 no es 0 exacto: sin snap, el
        // anillo del polo sería un micro-círculo con triángulos astilla.
        let ring_r = if cos_a.abs() < 1e-6 { 0.0 } else { r * cos_a };
        for i in 0..=radial {
            let theta = 2.0 * std::f32::consts::PI * i as f32 / radial as f32;
            let (sin_t, cos_t) = theta.sin_cos();
            vertices.push(Vertex {
                position: [cos_t * ring_r, y, sin_t * ring_r],
                // En el casquete la normal es la dirección desde el
                // centro de la esfera del casquete; en la banda
                // (sin_a = 0) sale radial pura. Una sola fórmula.
                normal: [cos_t * cos_a, sin_a, sin_t * cos_a],
                uv: [i as f32 / radial as f32, ring_idx as f32 / (rings - 1) as f32],
            });
        }
    }
    // Enrollado (a, c, b) / (b, c, d): con los anillos de abajo a
    // arriba es el que deja la normal geométrica hacia fuera
    // (verificado por el test de orientación).
    let mut indices = Vec::with_capacity(((rings - 1) * radial * 6) as usize);
    let side = radial + 1;
    for j in 0..rings - 1 {
        for i in 0..radial {
            let a = j * side + i;
            let b = a + 1;
            let c = a + side;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    (vertices, indices)
}

/// Genera la geometría de la primitiva Cilindro: radio
/// `glass_core::primitives::CYLINDER_RADIUS` (0,5) y altura total 2
/// (y ∈ [−1, 1], el cilindro de Unity), con tapas. Banda y tapas
/// llevan vértices propios (las normales de la tapa son ±Y y las de
/// la banda radiales: compartir vértice mezclaría ambas). La UV de
/// la banda envuelve el lateral; la de las tapas es el círculo en
/// planta dentro del cuadrado 0..1.
pub fn generate_cylinder() -> (Vec<Vertex>, Vec<u32>) {
    use glass_core::primitives::{CYLINDER_HALF_HEIGHT, CYLINDER_RADIUS};
    let r = CYLINDER_RADIUS;
    let hh = CYLINDER_HALF_HEIGHT;
    let radial = CYLINDER_RADIAL_SEGMENTS;
    let cols = radial + 1;
    let mut vertices: Vec<Vertex> = Vec::with_capacity((4 * cols + 2) as usize);
    // Banda: anillo inferior y superior (de abajo a arriba, como la
    // cápsula: mismo enrollado hacia fuera).
    for &(y, v) in &[(-hh, 0.0f32), (hh, 1.0f32)] {
        for i in 0..=radial {
            let theta = 2.0 * std::f32::consts::PI * i as f32 / radial as f32;
            let (sin_t, cos_t) = theta.sin_cos();
            vertices.push(Vertex {
                position: [cos_t * r, y, sin_t * r],
                normal: [cos_t, 0.0, sin_t],
                uv: [i as f32 / radial as f32, v],
            });
        }
    }
    let mut indices: Vec<u32> = Vec::with_capacity((radial * 12) as usize);
    for i in 0..radial {
        let a = i;
        let b = a + 1;
        let c = a + cols;
        let d = c + 1;
        indices.extend_from_slice(&[a, c, b, b, c, d]);
    }
    // Tapas: centro + anillo propio por tapa. La superior enrolla
    // (centro, i+1, i) y la inferior (centro, i, i+1): cada abanico
    // queda mirando hacia su lado (±Y), verificado por el test.
    for &(y, ny) in &[(hh, 1.0f32), (-hh, -1.0f32)] {
        let center = vertices.len() as u32;
        vertices.push(Vertex {
            position: [0.0, y, 0.0],
            normal: [0.0, ny, 0.0],
            uv: [0.5, 0.5],
        });
        let ring_start = vertices.len() as u32;
        for i in 0..=radial {
            let theta = 2.0 * std::f32::consts::PI * i as f32 / radial as f32;
            let (sin_t, cos_t) = theta.sin_cos();
            vertices.push(Vertex {
                position: [cos_t * r, y, sin_t * r],
                normal: [0.0, ny, 0.0],
                uv: [0.5 + cos_t * 0.5, 0.5 + sin_t * 0.5],
            });
        }
        for i in 0..radial {
            let p = ring_start + i;
            let q = p + 1;
            if ny > 0.0 {
                indices.extend_from_slice(&[center, q, p]);
            } else {
                indices.extend_from_slice(&[center, p, q]);
            }
        }
    }
    (vertices, indices)
}

/// Número de niveles de la cadena de mip-maps de una textura:
/// floor(log2(max(ancho, alto))) + 1, hasta llegar a 1×1.
pub fn mip_level_count(width: u32, height: u32) -> u32 {
    let max_side = width.max(height).max(1);
    32 - max_side.leading_zeros()
}

/// Sin BC la textura seguirá subiendo en RGBA8 SRGB.
static TEXTURE_BC_ENABLED: AtomicBool = AtomicBool::new(false);

/// Lo fija el contexto al crear el dispositivo según el feature
/// textureCompressionBC de la GPU; sin BC, todo sigue en RGBA8.
pub fn set_texture_bc_enabled(enabled: bool) {
    TEXTURE_BC_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn texture_bc_enabled() -> bool {
    TEXTURE_BC_ENABLED.load(Ordering::Relaxed)
}

/// ¿Algún píxel con alfa < 250? Decide BC3 frente a BC1. El umbral
/// (antes < 255) evita pagar BC3 por texturas «casi opacas» cuyo
/// único píxel no-255 es ruido de 1 nivel (auditoría, menor).
pub fn rgba_has_alpha(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).any(|p| p[3] < 250)
}

fn srgb_byte_to_linear(v: u8) -> f32 {
    let s = v as f32 / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb_byte(l: f32) -> u8 {
    let s = if l <= 0.0031308 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Downsample 2x2 (media de caja) de un nivel RGBA; las dimensiones de
/// salida son ceil(w/2) y ceil(h/2) con el borde repetido (clamp).
pub fn downsample_rgba(rgba: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    debug_assert_eq!(rgba.len(), (width * height * 4) as usize);
    // Auditoría Fase 2 (M-11): las dimensiones de cada nivel deben
    // seguir la regla de Vulkan, max(1, floor(w/2)) — con ceil, en
    // texturas NPOT los extents de copia superaban el mip real.
    let dw = (width / 2).max(1);
    let dh = (height / 2).max(1);
    let mut out = vec![0u8; (dw * dh * 4) as usize];

    for y in 0..dh {
        for x in 0..dw {
            // Índices del bloque 2x2 con clamp en el borde derecho/inferior.
            let x0 = (x * 2).min(width - 1);
            let x1 = (x * 2 + 1).min(width - 1);
            let y0 = (y * 2).min(height - 1);
            let y1 = (y * 2 + 1).min(height - 1);

            // Auditoría (menor): el color se promedia en espacio
            // LINEAL (los bytes son sRGB; promediarlos tal cual
            // oscurece los mips). El alfa, dato lineal, va en media
            // aritmética como siempre.
            let mut acc_lin = [0.0f32; 3];
            let mut acc_a = 0u32;
            for (px, py) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
                let i = ((py * width + px) * 4) as usize;
                for c in 0..3 {
                    acc_lin[c] += srgb_byte_to_linear(rgba[i + c]);
                }
                acc_a += rgba[i + 3] as u32;
            }

            let o = ((y * dw + x) * 4) as usize;
            for c in 0..3 {
                out[o + c] = linear_to_srgb_byte(acc_lin[c] / 4.0);
            }
            out[o + 3] = ((acc_a + 2) / 4) as u8;
        }
    }

    (out, dw, dh)
}

/// Bytes de un nivel en el formato dado: para BC, bloques de 4x4
/// (BC1: 8 bytes/bloque; BC3: 16).
pub fn texture_level_bytes(width: u32, height: u32, format: vk::Format) -> u64 {
    match format {
        vk::Format::BC1_RGB_SRGB_BLOCK | vk::Format::BC1_RGBA_SRGB_BLOCK => {
            let bw = ((width + 3) / 4) as u64;
            let bh = ((height + 3) / 4) as u64;
            bw * bh * 8
        }
        vk::Format::BC3_SRGB_BLOCK => {
            let bw = ((width + 3) / 4) as u64;
            let bh = ((height + 3) / 4) as u64;
            bw * bh * 16
        }
        // Respaldo RGBA8 (ruta clásica).
        _ => width as u64 * height as u64 * 4,
    }
}

/// Prepara la subida BC de una imagen: genera la cadena de mips en CPU
/// (downsample sucesivo desde el nivel 0), comprime cada nivel y
/// devuelve (formato, bytes concatenados, [(offset, ancho, alto) por
/// nivel]). Formato: BC3_SRGB si hay alfa, BC1_SRGB si no.
pub fn prepare_bc_upload(img: &GltfImageData) -> (vk::Format, Vec<u8>, Vec<(u64, u32, u32)>) {
    let levels = mip_level_count(img.width, img.height);
    let format = if rgba_has_alpha(&img.rgba) {
        vk::Format::BC3_SRGB_BLOCK
    } else {
        vk::Format::BC1_RGBA_SRGB_BLOCK
    };
    let bc_format = match format {
        vk::Format::BC3_SRGB_BLOCK => texpresso::Format::Bc3,
        _ => texpresso::Format::Bc1,
    };

    let mut data: Vec<u8> = Vec::new();
    let mut levels_info: Vec<(u64, u32, u32)> = Vec::with_capacity(levels as usize);

    // Nivel 0 tal cual; el resto por downsample sucesivo.
    let mut current = img.rgba.clone();
    let (mut w, mut h) = (img.width, img.height);

    for _ in 0..levels {
        let size = bc_format.compressed_size(w as usize, h as usize);
        let offset = data.len() as u64;
        data.resize(offset as usize + size, 0);
        bc_format.compress(
            &current,
            w as usize,
            h as usize,
            texpresso::Params::default(),
            &mut data[offset as usize..offset as usize + size],
        );
        levels_info.push((offset, w, h));

        if levels_info.len() as u32 == levels {
            break;
        }
        let (next, nw, nh) = downsample_rgba(&current, w, h);
        current = next;
        w = nw;
        h = nh;
    }

    (format, data, levels_info)
}

/// Transformaciones globales de todos los nodos del documento
/// (Fase 6): recorre las escenas y compone padre × local. Clave =
/// índice de nodo.
fn node_globals(document: &gltf::Document) -> std::collections::HashMap<usize, [f32; 16]> {
    use std::collections::HashSet;
    fn local_flat(node: &gltf::Node) -> [f32; 16] {
        let m = node.transform().matrix();
        [
            m[0][0], m[0][1], m[0][2], m[0][3], //
            m[1][0], m[1][1], m[1][2], m[1][3], //
            m[2][0], m[2][1], m[2][2], m[2][3], //
            m[3][0], m[3][1], m[3][2], m[3][3],
        ]
    }
    fn walk(
        node: gltf::Node,
        parent: [f32; 16],
        path: &mut HashSet<usize>,
        out: &mut std::collections::HashMap<usize, [f32; 16]>,
    ) {
        if path.contains(&node.index()) {
            log::warn!(
                "node_globals: ciclo detectado en el nodo {}; se trata como hoja",
                node.index()
            );
            return;
        }
        path.insert(node.index());
        let global = glass_core::math::mat4_mul(parent, local_flat(&node));
        out.insert(node.index(), global);
        for child in node.children() {
            walk(child, global, path, out);
        }
        path.remove(&node.index());
    }
    let mut out = std::collections::HashMap::new();
    let mut path = HashSet::new();
    if let Some(scene) = document.default_scene() {
        for node in scene.nodes() {
            walk(node, glass_core::math::mat4_identity(), &mut path, &mut out);
        }
    } else {
        for scene in document.scenes() {
            for node in scene.nodes() {
                walk(node, glass_core::math::mat4_identity(), &mut path, &mut out);
            }
        }
    }
    out
}

/// Fusiona UNA primitiva en los vectores del modelo (el cuerpo que
/// antes vivía dentro del doble bucle de `load_model_from_glb_bytes`).
/// Con `transform` (modelos sin piel, Fase 6), las posiciones se
/// transforman por la global del nodo y las normales por su
/// inversa-transpuesta normalizada; sin él, la primitiva entra tal
/// cual (el camino histórico, que es el de los modelos con piel).
#[allow(clippy::too_many_arguments)]
fn merge_primitive(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
    transform: Option<&[f32; 16]>,
    has_skin: bool,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    primitive_materials: &mut Vec<Option<usize>>,
    primitive_ranges: &mut Vec<PrimitiveRange>,
    vertex_joints: &mut Vec<[u16; 4]>,
    vertex_weights: &mut Vec<[f32; 4]>,
) {
    let material = primitive.material().index();
    primitive_materials.push(material);
    let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

    let positions: Vec<[f32; 3]> = reader
        .read_positions()
        .map(|iter| iter.collect())
        .unwrap_or_default();
    let positions: Vec<[f32; 3]> = match transform {
        Some(m) => positions
            .iter()
            .map(|p| glass_core::math::mat4_transform_point(*m, *p))
            .collect(),
        None => positions,
    };

    let normals: Vec<[f32; 3]> = reader
        .read_normals()
        .map(|iter| iter.collect())
        .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
    let normals: Vec<[f32; 3]> = match transform {
        Some(m) => {
            // Normales por la inversa-transpuesta de la global:
            // n'[r] = Σ_c inv[r*4+c] · n[c], y normalizar.
            let inv = glass_core::math::mat4_inverse_affine(*m);
            normals
                .iter()
                .map(|n| match inv {
                    Some(iv) => {
                        let v = [
                            iv[0] * n[0] + iv[1] * n[1] + iv[2] * n[2],
                            iv[4] * n[0] + iv[5] * n[1] + iv[6] * n[2],
                            iv[8] * n[0] + iv[9] * n[1] + iv[10] * n[2],
                        ];
                        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                        if len > 1e-8 {
                            [v[0] / len, v[1] / len, v[2] / len]
                        } else {
                            *n
                        }
                    }
                    None => *n,
                })
                .collect()
        }
        None => normals,
    };

    let uvs: Vec<[f32; 2]> = reader
        .read_tex_coords(0)
        .map(|iter| iter.into_f32().collect())
        .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);

    let prim_joints: Vec<[u16; 4]> = if has_skin {
        reader
            .read_joints(0)
            .map(|j| j.into_u16().collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let prim_weights: Vec<[f32; 4]> = if has_skin {
        reader
            .read_weights(0)
            .map(|w| w.into_f32().collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let base_index = vertices.len() as u32;

    for i in 0..positions.len() {
        vertices.push(Vertex {
            position: positions[i],
            normal: normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
            uv: uvs.get(i).copied().unwrap_or([0.0, 0.0]),
        });
        if has_skin {
            vertex_joints.push(prim_joints.get(i).copied().unwrap_or([0, 0, 0, 0]));
            vertex_weights.push(prim_weights.get(i).copied().unwrap_or([0.0; 4]));
        }
    }

    let mirrored = match transform {
        Some(m) => {
            let c0 = [m[0], m[1], m[2]];
            let c1 = [m[4], m[5], m[6]];
            let c2 = [m[8], m[9], m[10]];
            let cx = [
                c1[1] * c2[2] - c1[2] * c2[1],
                c1[2] * c2[0] - c1[0] * c2[2],
                c1[0] * c2[1] - c1[1] * c2[0],
            ];
            let det = c0[0] * cx[0] + c0[1] * cx[1] + c0[2] * cx[2];
            det < 0.0
        }
        None => false,
    };

    let first_index = indices.len() as u32;
    if let Some(read_indices) = reader.read_indices() {
        let mut tri = [0u32; 3];
        let mut n = 0;
        for idx in read_indices.into_u32() {
            tri[n] = base_index + idx;
            n += 1;
            if n == 3 {
                indices.push(tri[0]);
                if mirrored {
                    indices.push(tri[2]);
                    indices.push(tri[1]);
                } else {
                    indices.push(tri[1]);
                    indices.push(tri[2]);
                }
                n = 0;
            }
        }
        for &i in &tri[..n] {
            indices.push(i);
        }
    } else {
        let count = positions.len() as u32;
        if mirrored {
            let mut i = 0;
            while i + 2 < count {
                indices.push(base_index + i);
                indices.push(base_index + i + 2);
                indices.push(base_index + i + 1);
                i += 3;
            }
            while i < count {
                indices.push(base_index + i);
                i += 1;
            }
        } else {
            for idx in 0..count {
                indices.push(base_index + idx);
            }
        }
    }
    let index_count = indices.len() as u32 - first_index;
    primitive_ranges.push(PrimitiveRange { first_index, index_count, material });
}

impl MeshLoader {
    /// Carga vértices e índices desde un buffer binario glTF (.glb).
    /// Compatibilidad: equivale a `load_model_from_glb_bytes` descartando
    /// materiales e imágenes.
    pub fn load_from_glb_bytes(bytes: &[u8]) -> Result<(Vec<Vertex>, Vec<u32>), String> {
        let model = Self::load_model_from_glb_bytes(bytes)?;
        Ok((model.vertices, model.indices))
    }

    /// Importa un .glb completo: geometría fusionada, materiales PBR por
    /// primitiva e imágenes decodificadas a RGBA8. Antes el importador
    /// descartaba materiales e imágenes (`_images`) y todo se dibujaba con
    /// color plano y metallic/roughness fijos.
    pub fn load_model_from_glb_bytes(bytes: &[u8]) -> Result<LoadedModel, String> {
        let (document, buffers, images) =
            gltf::import_slice(bytes).map_err(|e| format!("Error importando glTF: {}", e))?;

        let materials: Vec<GltfMaterial> = document
            .materials()
            .map(|m| {
                let pbr = m.pbr_metallic_roughness();
                GltfMaterial {
                    name: m.name().unwrap_or("").to_string(),
                    base_color_factor: pbr.base_color_factor(),
                    metallic: pbr.metallic_factor(),
                    roughness: pbr.roughness_factor(),
                    base_color_image: pbr
                        .base_color_texture()
                        .map(|info| info.texture().source().index()),
                }
            })
            .collect();

        let images: Vec<GltfImageData> = images
            .iter()
            .map(|img| GltfImageData {
                width: img.width,
                height: img.height,
                rgba: image_to_rgba(img),
            })
            .collect();

        // Piel (si la hay) ANTES de la geometría: el bucle de primitivas
        // necesita saber si debe leer JOINTS_0/WEIGHTS_0. Un error aquí
        // (p. ej. demasiadas articulaciones) invalida la importación
        // entera con un mensaje claro.
        let skin = parse_skin(&document, &buffers)?;
        let has_skin = skin.is_some();
        // Los clips solo tienen sentido con el grafo de nodos de la piel.
        let animations = if has_skin {
            parse_animations(&document, &buffers)
        } else {
            Vec::new()
        };

        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut primitive_materials: Vec<Option<usize>> = Vec::new();
        let mut primitive_ranges: Vec<PrimitiveRange> = Vec::new();
        let mut vertex_joints: Vec<[u16; 4]> = Vec::new();
        let mut vertex_weights: Vec<[f32; 4]> = Vec::new();
        let mut objects: Vec<ModelObject> = Vec::new();

        if has_skin {
            // Con piel: las mallas usadas por nodos CON piel se
            // fusionan como siempre (sin transformar; la piel compone
            // los globales en el shader). Las mallas que ningún nodo
            // con piel usa (un prop estático en el mismo archivo) se
            // fusionan POR NODO con su transformación global y pesos
            // cero — el shader con piel tiene fallback para peso cero
            // y las dibuja tal cual (pasada de medios: antes perdían
            // su transformación de nodo y aparecían en el origen).
            let globals = node_globals(&document);
            let mut con_piel: std::collections::HashSet<usize> = Default::default();
            for node in document.nodes() {
                if node.skin().is_some() {
                    if let Some(m) = node.mesh() {
                        con_piel.insert(m.index());
                    }
                }
            }
            let global_de = |node: &gltf::Node| -> [f32; 16] {
                globals.get(&node.index()).copied().unwrap_or_else(|| {
                    // Sin global (huérfano u otra escena): usa la local.
                    let m = node.transform().matrix();
                    [
                        m[0][0], m[0][1], m[0][2], m[0][3], //
                        m[1][0], m[1][1], m[1][2], m[1][3], //
                        m[2][0], m[2][1], m[2][2], m[2][3], //
                        m[3][0], m[3][1], m[3][2], m[3][3],
                    ]
                })
            };
            let mut fusionadas: std::collections::HashMap<usize, Vec<usize>> = Default::default();
            let mut estaticas: std::collections::HashSet<usize> = Default::default();
            for node in document.nodes() {
                let Some(mesh) = node.mesh() else { continue };
                let mi = mesh.index();
                let g = global_de(&node);
                if con_piel.contains(&mi) {
                    let ranges = fusionadas
                        .entry(mi)
                        .or_insert_with(|| {
                            let primera = primitive_ranges.len();
                            for primitive in mesh.primitives() {
                                merge_primitive(
                                    &primitive,
                                    &buffers,
                                    None,
                                    has_skin,
                                    &mut vertices,
                                    &mut indices,
                                    &mut primitive_materials,
                                    &mut primitive_ranges,
                                    &mut vertex_joints,
                                    &mut vertex_weights,
                                );
                            }
                            (primera..primitive_ranges.len()).collect()
                        })
                        .clone();
                    objects.push(ModelObject {
                        name: node.name().unwrap_or("").to_string(),
                        mesh_name: mesh.name().unwrap_or("").to_string(),
                        transform: g,
                        ranges,
                    });
                } else {
                    estaticas.insert(mi);
                    let primera = primitive_ranges.len();
                    for primitive in mesh.primitives() {
                        merge_primitive(
                            &primitive,
                            &buffers,
                            Some(&g),
                            has_skin,
                            &mut vertices,
                            &mut indices,
                            &mut primitive_materials,
                            &mut primitive_ranges,
                            &mut vertex_joints,
                            &mut vertex_weights,
                        );
                    }
                    objects.push(ModelObject {
                        name: node.name().unwrap_or("").to_string(),
                        mesh_name: mesh.name().unwrap_or("").to_string(),
                        transform: g,
                        ranges: (primera..primitive_ranges.len()).collect(),
                    });
                }
            }
            // Mallas no referenciadas por ningún nodo: fusión
            // histórica sin transformar, sin objeto.
            for mesh in document.meshes() {
                if fusionadas.contains_key(&mesh.index()) || estaticas.contains(&mesh.index()) {
                    continue;
                }
                for primitive in mesh.primitives() {
                    merge_primitive(
                        &primitive,
                        &buffers,
                        None,
                        has_skin,
                        &mut vertices,
                        &mut indices,
                        &mut primitive_materials,
                        &mut primitive_ranges,
                        &mut vertex_joints,
                        &mut vertex_weights,
                    );
                }
            }
        } else {
            // Sin piel (Fase 6): la fusión recorre los NODOS y aplica
            // su transformación global; una malla referenciada por
            // varios nodos se fusiona una vez por nodo (instancias
            // enlazadas horneadas pero registradas en `objects`).
            let globals = node_globals(&document);
            let mut meshes_vistas: std::collections::HashSet<usize> =
                std::collections::HashSet::new();
            for node in document.nodes() {
                if let Some(mesh) = node.mesh() {
                    meshes_vistas.insert(mesh.index());
                    let g = globals
                        .get(&node.index())
                        .copied()
                        .unwrap_or_else(glass_core::math::mat4_identity);
                    let primera = primitive_ranges.len();
                    for primitive in mesh.primitives() {
                        merge_primitive(
                            &primitive,
                            &buffers,
                            Some(&g),
                            has_skin,
                            &mut vertices,
                            &mut indices,
                            &mut primitive_materials,
                            &mut primitive_ranges,
                            &mut vertex_joints,
                            &mut vertex_weights,
                        );
                    }
                    objects.push(ModelObject {
                        name: node.name().unwrap_or("").to_string(),
                        mesh_name: mesh.name().unwrap_or("").to_string(),
                        transform: g,
                        ranges: (primera..primitive_ranges.len()).collect(),
                    });
                }
            }
            // Mallas que ningún nodo referencia: sin transformar,
            // al final (compatibilidad con archivos laxos).
            for mesh in document.meshes() {
                if meshes_vistas.contains(&mesh.index()) {
                    continue;
                }
                let primera = primitive_ranges.len();
                for primitive in mesh.primitives() {
                    merge_primitive(
                        &primitive,
                        &buffers,
                        None,
                        has_skin,
                        &mut vertices,
                        &mut indices,
                        &mut primitive_materials,
                        &mut primitive_ranges,
                        &mut vertex_joints,
                        &mut vertex_weights,
                    );
                }
                objects.push(ModelObject {
                    name: mesh.name().unwrap_or("").to_string(),
                    mesh_name: mesh.name().unwrap_or("").to_string(),
                    transform: glass_core::math::mat4_identity(),
                    ranges: (primera..primitive_ranges.len()).collect(),
                });
            }
        }

        if vertices.is_empty() {
            return Err("El archivo glTF no contiene primitivas de malla válidas".into());
        }

        Ok(LoadedModel {
            vertices,
            indices,
            materials,
            images,
            primitive_materials,
            primitive_ranges,
            skin,
            animations,
            vertex_joints,
            vertex_weights,
            objects,
        })
    }

    /// Sube los datos de vértices e índices a la GPU mediante Staging Buffers
    pub fn upload_to_gpu(
        device: &Arc<ash::Device>,
        allocator: &mut Allocator,
        vertices: &[Vertex],
        indices: &[u32],
        queue: vk::Queue,
        command_pool: vk::CommandPool,
    ) -> Result<GpuMesh, String> {
        // Vertex es #[repr(C)] de f32/u16 planos: su memoria ES el buffer.
        let vertex_bytes = unsafe {
            std::slice::from_raw_parts(
                vertices.as_ptr() as *const u8,
                std::mem::size_of_val(vertices),
            )
        };
        Self::upload_vertex_bytes(
            device,
            allocator,
            vertex_bytes,
            vertices.len() as u32,
            indices,
            queue,
            command_pool,
        )
    }

    /// Como `upload_to_gpu` pero entrelazando las influencias de piel del
    /// modelo (`SkinnedVertex`): es el buffer que consume el pipeline con
    /// piel. Exige que el modelo tenga piel (si no, no hay nada que
    /// entrelazar: el llamante usa la subida normal).
    pub fn upload_skinned_to_gpu(
        device: &Arc<ash::Device>,
        allocator: &mut Allocator,
        model: &LoadedModel,
        queue: vk::Queue,
        command_pool: vk::CommandPool,
    ) -> Result<GpuMesh, String> {
        if model.skin.is_none() {
            return Err("upload_skinned_to_gpu llamado con un modelo sin piel".into());
        }
        let skinned: Vec<SkinnedVertex> = model
            .vertices
            .iter()
            .enumerate()
            .map(|(i, v)| SkinnedVertex {
                position: v.position,
                normal: v.normal,
                uv: v.uv,
                joints: model.vertex_joints.get(i).copied().unwrap_or([0, 0, 0, 0]),
                weights: model.vertex_weights.get(i).copied().unwrap_or([0.0; 4]),
            })
            .collect();
        let vertex_bytes = unsafe {
            std::slice::from_raw_parts(
                skinned.as_ptr() as *const u8,
                std::mem::size_of_val(&skinned[..]),
            )
        };
        Self::upload_vertex_bytes(
            device,
            allocator,
            vertex_bytes,
            skinned.len() as u32,
            &model.indices,
            queue,
            command_pool,
        )
    }

    /// Cuerpo compartido de las subidas de mallas: staging CpuToGpu, copia
    /// a buffers GpuOnly y liberación del staging. Solo cambia el formato
    /// de vértice (bytes crudos + conteo), que el pipeline interpreta.
    fn upload_vertex_bytes(
        device: &Arc<ash::Device>,
        allocator: &mut Allocator,
        vertex_bytes: &[u8],
        vertex_count: u32,
        indices: &[u32],
        queue: vk::Queue,
        command_pool: vk::CommandPool,
    ) -> Result<GpuMesh, String> {
        let vertex_size = vertex_bytes.len() as u64;
        let index_size = (indices.len() * std::mem::size_of::<u32>()) as u64;

        log::info!("upload_to_gpu: Creando staging buffers...");
        unsafe {
            // 1. Crear Staging Buffers en CPU-to-GPU
            let (v_stage_buf, v_stage_alloc) = Self::create_buffer(
                device,
                allocator,
                vertex_size,
                vk::BufferUsageFlags::TRANSFER_SRC,
                MemoryLocation::CpuToGpu,
                "vertex_staging",
            )?;
            log::info!("upload_to_gpu: V_stage creado");

            let (i_stage_buf, i_stage_alloc) = Self::create_buffer(
                device,
                allocator,
                index_size,
                vk::BufferUsageFlags::TRANSFER_SRC,
                MemoryLocation::CpuToGpu,
                "index_staging",
            )?;
            log::info!("upload_to_gpu: I_stage creado");

            // 2. Copiar memoria CPU a Staging
            if let Some(ptr) = v_stage_alloc.mapped_ptr() {
                std::ptr::copy_nonoverlapping(
                    vertex_bytes.as_ptr(),
                    ptr.as_ptr() as *mut u8,
                    vertex_size as usize,
                );
            }
            if let Some(ptr) = i_stage_alloc.mapped_ptr() {
                std::ptr::copy_nonoverlapping(
                    indices.as_ptr() as *const u8,
                    ptr.as_ptr() as *mut u8,
                    index_size as usize,
                );
            }
            log::info!("upload_to_gpu: Datos copiados a staging");

            // 3. Crear GPU-Only Buffers de destino
            let (vertex_buffer, v_alloc) = Self::create_buffer(
                device,
                allocator,
                vertex_size,
                vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
                MemoryLocation::GpuOnly,
                "mesh_vertex_buffer",
            )?;

            let (index_buffer, i_alloc) = Self::create_buffer(
                device,
                allocator,
                index_size,
                vk::BufferUsageFlags::INDEX_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
                MemoryLocation::GpuOnly,
                "mesh_index_buffer",
            )?;
            log::info!("upload_to_gpu: Destinos creados");

            // 4. Copiar de Staging a GPU mediante Command Buffer
            Self::copy_buffer(
                device,
                command_pool,
                queue,
                v_stage_buf,
                vertex_buffer,
                vertex_size,
            )?;
            log::info!("upload_to_gpu: Copia v_stage -> vertex_buffer completada");

            Self::copy_buffer(
                device,
                command_pool,
                queue,
                i_stage_buf,
                index_buffer,
                index_size,
            )?;
            log::info!("upload_to_gpu: Copia i_stage -> index_buffer completada");

            // 5. Liberar Staging Buffers
            log::info!("upload_to_gpu: Liberando staging allocations...");
            let _ = allocator.free(v_stage_alloc);
            device.destroy_buffer(v_stage_buf, None);
            let _ = allocator.free(i_stage_alloc);
            device.destroy_buffer(i_stage_buf, None);
            log::info!("upload_to_gpu: Staging buffers destruidos con éxito");

            Ok(GpuMesh {
                vertex_buffer,
                vertex_allocation: Some(v_alloc),
                index_buffer,
                index_allocation: Some(i_alloc),
                index_count: indices.len() as u32,
                vertex_count,
            })
        }
    }

    /// Sube una imagen RGBA8 a VRAM como textura muestreable
    /// (R8G8B8A8_SRGB, layout SHADER_READ_ONLY_OPTIMAL) en un solo envío,
    /// generando la cadena completa de mip-maps por blit en la GPU.
    pub fn upload_texture_to_gpu(
        device: &ash::Device,
        allocator: &mut Allocator,
        img: &GltfImageData,
        queue: vk::Queue,
        command_pool: vk::CommandPool,
    ) -> Result<GpuTexture, String> {
        let size = img.rgba.len() as u64;
        if size == 0 || img.width == 0 || img.height == 0 {
            return Err("Textura vacía".into());
        }
        // Fase 2: si la GPU soporta BC, se comprime en CPU y se sube en BC.
        if texture_bc_enabled() && img.width >= 4 && img.height >= 4 {
            return Self::upload_texture_to_gpu_bc(device, allocator, img, queue, command_pool);
        }
        let mip_levels = mip_level_count(img.width, img.height);
        unsafe {
            // 1. Staging con los píxeles.
            let (stage_buf, stage_alloc) = Self::create_buffer(
                device,
                allocator,
                size,
                vk::BufferUsageFlags::TRANSFER_SRC,
                MemoryLocation::CpuToGpu,
                "texture_staging_buffer",
            )?;
            if let Some(ptr) = stage_alloc.mapped_ptr() {
                std::ptr::copy_nonoverlapping(img.rgba.as_ptr(), ptr.as_ptr() as *mut u8, img.rgba.len());
            }

            // 2. Imagen GPU.
            let image_info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::R8G8B8A8_SRGB)
                .extent(vk::Extent3D { width: img.width, height: img.height, depth: 1 })
                .mip_levels(mip_levels)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::SAMPLED)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);
            let image = device
                .create_image(&image_info, None)
                .map_err(|e| format!("Fallo al crear imagen de textura: {}", e))?;
            let mem_reqs = device.get_image_memory_requirements(image);
            let allocation = allocator
                .allocate(&AllocationCreateDesc {
                    name: "texture_image",
                    requirements: mem_reqs,
                    location: MemoryLocation::GpuOnly,
                    linear: false,
                    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                })
                .map_err(|e| format!("Fallo de asignación de memoria de textura: {}", e))?;
            device
                .bind_image_memory(image, allocation.memory(), allocation.offset())
                .map_err(|e| format!("Fallo al enlazar memoria de textura: {}", e))?;

            // 3. Transiciones y copia en un command buffer de un solo uso.
            let alloc_info = vk::CommandBufferAllocateInfo::default()
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1);
            let cmd = device
                .allocate_command_buffers(&alloc_info)
                .map_err(|e| format!("Fallo al asignar command buffer de textura: {}", e))?[0];
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            device
                .begin_command_buffer(cmd, &begin_info)
                .map_err(|e| format!("Error en begin_command_buffer de textura: {}", e))?;

            // Rango con TODOS los niveles (transición inicial y vista) y
            // constructor de rangos por nivel (cadena de mip-maps).
            let subresource = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(mip_levels)
                .layer_count(1);
            let level_range = |base: u32| {
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(base)
                    .level_count(1)
                    .layer_count(1)
            };
            let barrier_to_dst = [vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .image(image)
                .subresource_range(subresource)];
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barrier_to_dst,
            );

            let region = [vk::BufferImageCopy::default()
                .image_subresource(vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1))
                .image_extent(vk::Extent3D { width: img.width, height: img.height, depth: 1 })];
            device.cmd_copy_buffer_to_image(
                cmd,
                stage_buf,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &region,
            );

            // Cadena de mip-maps: cada nivel se genera por blit desde el
            // anterior y el anterior queda ya en lectura para shaders. Al
            // final, el último nivel (que sigue en TRANSFER_DST) también
            // pasa a lectura. Con un solo nivel, el bucle no corre y solo
            // se aplica la transición final.
            let mut mip_w = img.width as i32;
            let mut mip_h = img.height as i32;
            for level in 1..mip_levels {
                let to_src = [vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .image(image)
                    .subresource_range(level_range(level - 1))];
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &to_src,
                );

                let next_w = (mip_w / 2).max(1);
                let next_h = (mip_h / 2).max(1);
                let blit = [vk::ImageBlit::default()
                    .src_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .mip_level(level - 1)
                            .layer_count(1),
                    )
                    .src_offsets([
                        vk::Offset3D { x: 0, y: 0, z: 0 },
                        vk::Offset3D { x: mip_w, y: mip_h, z: 1 },
                    ])
                    .dst_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .mip_level(level)
                            .layer_count(1),
                    )
                    .dst_offsets([
                        vk::Offset3D { x: 0, y: 0, z: 0 },
                        vk::Offset3D { x: next_w, y: next_h, z: 1 },
                    ])];
                device.cmd_blit_image(
                    cmd,
                    image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &blit,
                    vk::Filter::LINEAR,
                );

                let prev_to_read = [vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .image(image)
                    .subresource_range(level_range(level - 1))];
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &prev_to_read,
                );

                mip_w = next_w;
                mip_h = next_h;
            }

            let barrier_to_read = [vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .image(image)
                .subresource_range(level_range(mip_levels - 1))];
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barrier_to_read,
            );

            device
                .end_command_buffer(cmd)
                .map_err(|e| format!("Error en end_command_buffer de textura: {}", e))?;
            let fence = device
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .map_err(|e| format!("Error creando fence de textura: {}", e))?;
            let command_buffers = [cmd];
            let submit_info = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
            device
                .queue_submit(queue, &submit_info, fence)
                .map_err(|e| format!("Error en queue_submit de textura: {}", e))?;
            device
                .wait_for_fences(&[fence], true, u64::MAX)
                .map_err(|e| format!("Error esperando fence de textura: {}", e))?;
            device.destroy_fence(fence, None);
            device.free_command_buffers(command_pool, &[cmd]);

            // 4. Liberar staging y crear la vista.
            let _ = allocator.free(stage_alloc);
            device.destroy_buffer(stage_buf, None);

            let view_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(vk::Format::R8G8B8A8_SRGB)
                .subresource_range(subresource);
            let image_view = device
                .create_image_view(&view_info, None)
                .map_err(|e| format!("Fallo al crear vista de textura: {}", e))?;

            // VRAM en RGBA8: suma por nivel de (w>>l).max(1) * (h>>l).max(1) * 4.
            let mut vram_bytes = 0u64;
            for level in 0..mip_levels {
                let w = (img.width >> level).max(1) as u64;
                let h = (img.height >> level).max(1) as u64;
                vram_bytes += w * h * 4;
            }
            Ok(GpuTexture {
                image,
                allocation: Some(allocation),
                image_view,
                width: img.width,
                height: img.height,
                format: vk::Format::R8G8B8A8_SRGB,
                mip_levels,
                vram_bytes,
            })
        }
    }

    /// Ruta BC de subida (Fase 2): imagen en formato BC con la cadena de
    /// mips generada y comprimida en CPU; una copia por nivel desde un
    /// único buffer staging. Mismo patrón de comandos de un solo uso y
    /// de barreras que upload_texture_to_gpu (que queda como respaldo
    /// para dispositivos sin textureCompressionBC).
    pub fn upload_texture_to_gpu_bc(
        device: &ash::Device,
        allocator: &mut Allocator,
        img: &GltfImageData,
        queue: vk::Queue,
        command_pool: vk::CommandPool,
    ) -> Result<GpuTexture, String> {
        let (format, data, levels) = prepare_bc_upload(img);
        let mip_levels = levels.len() as u32;
        let total_bytes = data.len() as u64;
        unsafe {
            // 1. Staging con la cadena comprimida completa.
            let (stage_buf, stage_alloc) = Self::create_buffer(
                device,
                allocator,
                total_bytes,
                vk::BufferUsageFlags::TRANSFER_SRC,
                MemoryLocation::CpuToGpu,
                "texture_bc_staging_buffer",
            )?;
            if let Some(ptr) = stage_alloc.mapped_ptr() {
                std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.as_ptr() as *mut u8, data.len());
            }

            // 2. Imagen GPU en el formato BC elegido.
            let image_info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(format)
                .extent(vk::Extent3D { width: img.width, height: img.height, depth: 1 })
                .mip_levels(mip_levels)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);
            let image = device
                .create_image(&image_info, None)
                .map_err(|e| format!("Fallo al crear imagen de textura BC: {}", e))?;
            let mem_reqs = device.get_image_memory_requirements(image);
            let allocation = allocator
                .allocate(&AllocationCreateDesc {
                    name: "texture_bc_image",
                    requirements: mem_reqs,
                    location: MemoryLocation::GpuOnly,
                    linear: false,
                    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                })
                .map_err(|e| format!("Fallo de asignación de memoria de textura BC: {}", e))?;
            device
                .bind_image_memory(image, allocation.memory(), allocation.offset())
                .map_err(|e| format!("Fallo al enlazar memoria de textura BC: {}", e))?;

            // 3. Transiciones y copias por nivel en un solo uso.
            let alloc_info = vk::CommandBufferAllocateInfo::default()
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1);
            let cmd = device
                .allocate_command_buffers(&alloc_info)
                .map_err(|e| format!("Fallo al asignar command buffer de textura BC: {}", e))?[0];
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            device
                .begin_command_buffer(cmd, &begin_info)
                .map_err(|e| format!("Error en begin_command_buffer de textura BC: {}", e))?;

            let subresource = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(mip_levels)
                .layer_count(1);
            let barrier_to_dst = [vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .image(image)
                .subresource_range(subresource)];
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barrier_to_dst,
            );

            let regions: Vec<vk::BufferImageCopy> = levels
                .iter()
                .enumerate()
                .map(|(level, (offset, w, h))| {
                    vk::BufferImageCopy::default()
                        .buffer_offset(*offset)
                        .image_subresource(vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .mip_level(level as u32)
                            .layer_count(1))
                        .image_extent(vk::Extent3D { width: *w, height: *h, depth: 1 })
                })
                .collect();
            device.cmd_copy_buffer_to_image(
                cmd,
                stage_buf,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &regions,
            );

            let barrier_to_read = [vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .image(image)
                .subresource_range(subresource)];
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barrier_to_read,
            );

            device
                .end_command_buffer(cmd)
                .map_err(|e| format!("Error en end_command_buffer de textura BC: {}", e))?;
            let fence = device
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .map_err(|e| format!("Error creando fence de textura BC: {}", e))?;
            let command_buffers = [cmd];
            let submit_info = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
            device
                .queue_submit(queue, &submit_info, fence)
                .map_err(|e| format!("Error en queue_submit de textura BC: {}", e))?;
            device
                .wait_for_fences(&[fence], true, u64::MAX)
                .map_err(|e| format!("Error esperando fence de textura BC: {}", e))?;
            device.destroy_fence(fence, None);
            device.free_command_buffers(command_pool, &[cmd]);

            // 4. Liberar staging y crear la vista.
            let _ = allocator.free(stage_alloc);
            device.destroy_buffer(stage_buf, None);

            let view_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(format)
                .subresource_range(subresource);
            let image_view = device
                .create_image_view(&view_info, None)
                .map_err(|e| format!("Fallo al crear vista de textura BC: {}", e))?;

            Ok(GpuTexture {
                image,
                allocation: Some(allocation),
                image_view,
                width: img.width,
                height: img.height,
                format,
                mip_levels,
                vram_bytes: total_bytes,
            })
        }
    }


    pub unsafe fn create_buffer(
        device: &ash::Device,
        allocator: &mut Allocator,
        size: u64,
        usage: vk::BufferUsageFlags,
        location: MemoryLocation,
        name: &'static str,
    ) -> Result<(vk::Buffer, Allocation), String> {
        let buffer_info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let buffer = device
            .create_buffer(&buffer_info, None)
            .map_err(|e| format!("Fallo al crear buffer: {}", e))?;

        let mem_reqs = device.get_buffer_memory_requirements(buffer);

        let allocation = allocator
            .allocate(&AllocationCreateDesc {
                name,
                requirements: mem_reqs,
                location,
                linear: location == MemoryLocation::CpuToGpu,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|e| format!("Fallo de asignación de memoria VRAM: {}", e))?;

        device
            .bind_buffer_memory(buffer, allocation.memory(), allocation.offset())
            .map_err(|e| format!("Fallo al enlazar memoria de buffer: {}", e))?;

        Ok((buffer, allocation))
    }

    unsafe fn copy_buffer(
        device: &ash::Device,
        pool: vk::CommandPool,
        queue: vk::Queue,
        src: vk::Buffer,
        dst: vk::Buffer,
        size: u64,
    ) -> Result<(), String> {
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);

        let cmd = device
            .allocate_command_buffers(&alloc_info)
            .map_err(|e| format!("Fallo al asignar command buffer: {}", e))?[0];

        let begin_info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        device
            .begin_command_buffer(cmd, &begin_info)
            .map_err(|e| format!("Error en begin_command_buffer: {}", e))?;

        let copy_region = [vk::BufferCopy::default().size(size)];
        device.cmd_copy_buffer(cmd, src, dst, &copy_region);

        device
            .end_command_buffer(cmd)
            .map_err(|e| format!("Error en end_command_buffer: {}", e))?;

        let fence_info = vk::FenceCreateInfo::default();
        let fence = device
            .create_fence(&fence_info, None)
            .map_err(|e| format!("Error creando fence de copia: {}", e))?;

        let command_buffers = [cmd];
        let submit_info = [vk::SubmitInfo::default().command_buffers(&command_buffers)];

        device
            .queue_submit(queue, &submit_info, fence)
            .map_err(|e| format!("Error en queue_submit de copia: {}", e))?;

        device
            .wait_for_fences(&[fence], true, u64::MAX)
            .map_err(|e| format!("Error esperando fence de copia: {}", e))?;

        device.destroy_fence(fence, None);
        device.free_command_buffers(pool, &[cmd]);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

/// 4x4 con rejilla conocida: R = 4*(x + 4y); salida 2x2 con medias exactas.
    #[test]
    fn downsample_4x4_medias_exactas() {
        let mut rgba = vec![0u8; 4 * 4 * 4];
        for y in 0..4u32 {
            for x in 0..4u32 {
                let i = ((y * 4 + x) * 4) as usize;
                rgba[i] = (4 * (x + 4 * y)) as u8;
                rgba[i + 3] = 255;
            }
        }

        let (out, w, h) = downsample_rgba(&rgba, 4, 4);
        assert_eq!((w, h), (2, 2));
        // Medias en espacio LINEAL (auditoría, menor): (0,0)=11,
        // (1,0)=19, (0,1)=43, (1,1)=51 — la media aritmética en
        // sRGB daría 10/18/42/50, sistemáticamente más oscura.
        let esperado = vec![
            11, 0, 0, 255, //
            19, 0, 0, 255, //
            43, 0, 0, 255, //
            51, 0, 0, 255, //
        ];
        assert_eq!(out, esperado);
    }

    /// 3x3 -> 2x2 con clamp del borde en la última columna/fila.
    #[test]
    fn downsample_3x3_clamp() {
        let mut rgba = vec![0u8; 3 * 3 * 4];
        for y in 0..3u32 {
            for x in 0..3u32 {
                let i = ((y * 3 + x) * 4) as usize;
                rgba[i] = (10 * (x + 3 * y)) as u8;
                rgba[i + 3] = 255;
            }
        }

        let (out, w, h) = downsample_rgba(&rgba, 3, 3);
        // Auditoría Fase 2 (M-11): regla de Vulkan, max(1, floor(3/2))
        // = 1. El píxel resultante promedia el bloque 2x2 superior
        // izquierdo (0, 10, 30, 40) en espacio lineal -> 24.
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![24, 0, 0, 255]);
    }

    #[test]
    fn level_bytes_bc_y_rgba() {
        let bc1 = vk::Format::BC1_RGB_SRGB_BLOCK;
        let bc3 = vk::Format::BC3_SRGB_BLOCK;
        let rgba = vk::Format::R8G8B8A8_SRGB;

        assert_eq!(texture_level_bytes(256, 256, bc1), 32768);
        assert_eq!(texture_level_bytes(256, 256, bc3), 65536);
        assert_eq!(texture_level_bytes(256, 256, rgba), 262144);
        assert_eq!(texture_level_bytes(4, 4, bc1), 8);
        // 5x5 -> ceil(5/4)=2 bloques por eje = 4 bloques * 8 bytes.
        assert_eq!(texture_level_bytes(5, 5, bc1), 32);
    }

    #[test]
    fn prepare_bc_upload_opaca_64x64() {
        let (w, h) = (64u32, 64u32);
        let mut rgba = vec![128u8; (w * h * 4) as usize];
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255; // alfa opaco de verdad (128 en los 4 canales incluía el alfa)
        }
        let img = GltfImageData {
            rgba, // opaca
            width: w,
            height: h,
        };

        let (format, data, levels) = prepare_bc_upload(&img);
        assert_eq!(format, vk::Format::BC1_RGBA_SRGB_BLOCK);
        assert_eq!(levels.len() as u32, mip_level_count(w, h)); // 7
        assert_eq!(levels.len(), 7);
        assert_eq!(levels[0], (0, 64, 64));

        let total: u64 = levels
            .iter()
            .map(|&(_, lw, lh)| texture_level_bytes(lw, lh, format))
            .sum();
        assert_eq!(data.len() as u64, total);

        // Offsets contiguos y en orden.
        let mut esperado = 0u64;
        for &(offset, _, _) in &levels {
            assert_eq!(offset, esperado);
            esperado += texture_level_bytes(
                levels.iter().find(|l| l.0 == offset).unwrap().1,
                levels.iter().find(|l| l.0 == offset).unwrap().2,
                format,
            );
        }
    }

    #[test]
    fn prepare_bc_upload_con_alfa_usa_bc3() {
        let (w, h) = (64u32, 64u32);
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        rgba[3] = 128; // un solo píxel semitransparente
        let img = GltfImageData { rgba, width: w, height: h };

        let (format, _, levels) = prepare_bc_upload(&img);
        assert_eq!(format, vk::Format::BC3_SRGB_BLOCK);
        assert_eq!(levels.len(), 7);
    }

    #[test]
    fn prepare_bc_upload_npot_sigue_la_regla_de_vulkan() {
        // Auditoría Fase 2 (M-11): 18x10 -> 9x5 -> 4x2 -> 2x1 -> 1x1
        // (floor), nunca por encima del mip que Vulkan calcula.
        let (w, h) = (18u32, 10u32);
        let mut rgba = vec![90u8; (w * h * 4) as usize];
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
        let img = GltfImageData { rgba, width: w, height: h };
        let (_, _, levels) = prepare_bc_upload(&img);
        let dims: Vec<(u32, u32)> = levels.iter().map(|&(_, lw, lh)| (lw, lh)).collect();
        assert_eq!(dims, vec![(18, 10), (9, 5), (4, 2), (2, 1), (1, 1)]);
    }

    #[test]
    fn rgba_has_alpha_detecta_transparencia() {
        let opaca = vec![10u8, 20, 30, 255, 40, 50, 60, 255];
        assert!(!rgba_has_alpha(&opaca));

        // Umbral de la pasada de medios: 254 ya cuenta como opaco
        // (no fuerza BC3); la transparencia real sí se detecta.
        let mut casi = opaca.clone();
        casi[7] = 254;
        assert!(!rgba_has_alpha(&casi));
        let mut con_alfa = opaca.clone();
        con_alfa[7] = 200;
        assert!(rgba_has_alpha(&con_alfa));
    }

    #[test]
    fn plano_conteos_y_esquinas() {
        let (vertices, indices) = generate_plane(PLANE_SEGMENTS);
        // 64×64 celdas: 65×65 vértices y 6 índices por celda.
        assert_eq!(vertices.len(), 65 * 65);
        assert_eq!(indices.len(), 64 * 64 * 6);
        assert!(indices.iter().all(|&i| (i as usize) < vertices.len()));
        // Esquinas exactas del 1×1 en XZ, con su UV en el extremo.
        let primera = vertices[0];
        assert_eq!(primera.position, [-0.5, 0.0, -0.5]);
        assert_eq!(primera.uv, [0.0, 0.0]);
        let ultima = vertices[vertices.len() - 1];
        assert_eq!(ultima.position, [0.5, 0.0, 0.5]);
        assert_eq!(ultima.uv, [1.0, 1.0]);
    }

    #[test]
    fn plano_normales_arriba_y_triangulos_hacia_arriba() {
        let (vertices, indices) = generate_plane(4);
        for v in &vertices {
            assert_eq!(v.normal, [0.0, 1.0, 0.0]);
        }
        // La normal GEOMÉTRICA de cada triángulo (producto cruz de sus
        // aristas) también apunta a +Y: el enrollado declarado es el real.
        for tri in indices.chunks_exact(3) {
            let p = |i: u32| vertices[i as usize].position;
            let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let ny = e1[2] * e2[0] - e1[0] * e2[2];
            assert!(ny > 0.0, "triángulo {:?} con normal geométrica no +Y", tri);
        }
    }

    #[test]
    fn plano_uv_cubre_0_a_1_y_segmentos_minimos() {
        let (vertices, indices) = generate_plane(0); // se clampa a 1 segmento
        assert_eq!(vertices.len(), 4);
        assert_eq!(indices.len(), 6);
        let us: Vec<f32> = vertices.iter().map(|v| v.uv[0]).collect();
        assert!(us.contains(&0.0) && us.contains(&1.0));
    }

    /// Normal geométrica (sin normalizar) de un triángulo por índices.
    fn normal_geometrica(vertices: &[Vertex], tri: &[u32]) -> [f32; 3] {
        let p = |i: u32| vertices[i as usize].position;
        let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ]
    }

    fn longitud(v: [f32; 3]) -> f32 {
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    }

    #[test]
    fn esfera_conteos_extremos_y_normales() {
        let (vertices, indices) = generate_sphere();
        assert_eq!(vertices.len(), (SPHERE_WIDTH_SEGMENTS + 1) as usize * (SPHERE_HEIGHT_SEGMENTS + 1) as usize);
        assert_eq!(indices.len(), (SPHERE_WIDTH_SEGMENTS * SPHERE_HEIGHT_SEGMENTS * 6) as usize);
        assert!(indices.iter().all(|&i| (i as usize) < vertices.len()));
        let r = glass_core::primitives::SPHERE_RADIUS;
        for v in &vertices {
            // Todo vértice vive SOBRE la esfera y su normal es la radial.
            assert!((longitud(v.position) - r).abs() < 1e-5, "vértice fuera de la esfera: {:?}", v.position);
            assert!((longitud(v.normal) - 1.0).abs() < 1e-5);
            let esperada = [v.position[0] / r, v.position[1] / r, v.position[2] / r];
            for c in 0..3 {
                assert!((v.normal[c] - esperada[c]).abs() < 1e-5);
            }
        }
        let ys: Vec<f32> = vertices.iter().map(|v| v.position[1]).collect();
        assert!((ys.iter().cloned().fold(f32::MIN, f32::max) - r).abs() < 1e-6);
        assert!((ys.iter().cloned().fold(f32::MAX, f32::min) + r).abs() < 1e-6);
    }

    #[test]
    fn esfera_triangulos_hacia_fuera() {
        let (vertices, indices) = generate_sphere();
        for tri in indices.chunks_exact(3) {
            let n = normal_geometrica(&vertices, tri);
            if longitud(n) < 1e-9 {
                continue; // triángulo degenerado del polo
            }
            let centroide = [
                (vertices[tri[0] as usize].position[0] + vertices[tri[1] as usize].position[0] + vertices[tri[2] as usize].position[0]) / 3.0,
                (vertices[tri[0] as usize].position[1] + vertices[tri[1] as usize].position[1] + vertices[tri[2] as usize].position[1]) / 3.0,
                (vertices[tri[0] as usize].position[2] + vertices[tri[1] as usize].position[2] + vertices[tri[2] as usize].position[2]) / 3.0,
            ];
            // En una esfera centrada en el origen, «hacia fuera» es la
            // dirección del propio centroide.
            let dot = n[0] * centroide[0] + n[1] * centroide[1] + n[2] * centroide[2];
            assert!(dot > 0.0, "triángulo {:?} enrollado hacia dentro", tri);
        }
    }

    #[test]
    fn capsula_conteos_extremos_y_normales() {
        let (vertices, indices) = generate_capsule();
        let rings = 2 * (CAPSULE_CAP_RINGS + 1) as usize;
        assert_eq!(vertices.len(), rings * (CAPSULE_RADIAL_SEGMENTS + 1) as usize);
        assert_eq!(indices.len(), (rings - 1) * CAPSULE_RADIAL_SEGMENTS as usize * 6);
        assert!(indices.iter().all(|&i| (i as usize) < vertices.len()));
        let r = glass_core::primitives::CAPSULE_RADIUS;
        let mitad_total = glass_core::primitives::CAPSULE_CYLINDER_HALF_HEIGHT + r;
        let mut max_radio_xz = 0.0f32;
        for v in &vertices {
            assert!((longitud(v.normal) - 1.0).abs() < 1e-4, "normal no unitaria: {:?}", v.normal);
            assert!(v.position[1].abs() <= mitad_total + 1e-6, "vértice fuera del alto total: {:?}", v.position);
            max_radio_xz = max_radio_xz.max((v.position[0] * v.position[0] + v.position[2] * v.position[2]).sqrt());
        }
        assert!((max_radio_xz - r).abs() < 1e-6);
        let ys: Vec<f32> = vertices.iter().map(|v| v.position[1]).collect();
        assert!((ys.iter().cloned().fold(f32::MIN, f32::max) - mitad_total).abs() < 1e-6);
        assert!((ys.iter().cloned().fold(f32::MAX, f32::min) + mitad_total).abs() < 1e-6);
    }

    #[test]
    fn capsula_triangulos_hacia_fuera() {
        let (vertices, indices) = generate_capsule();
        let ch = glass_core::primitives::CAPSULE_CYLINDER_HALF_HEIGHT;
        for tri in indices.chunks_exact(3) {
            let n = normal_geometrica(&vertices, tri);
            if longitud(n) < 1e-9 {
                continue; // degenerado del polo
            }
            let c = [
                (vertices[tri[0] as usize].position[0] + vertices[tri[1] as usize].position[0] + vertices[tri[2] as usize].position[0]) / 3.0,
                (vertices[tri[0] as usize].position[1] + vertices[tri[1] as usize].position[1] + vertices[tri[2] as usize].position[1]) / 3.0,
                (vertices[tri[0] as usize].position[2] + vertices[tri[1] as usize].position[2] + vertices[tri[2] as usize].position[2]) / 3.0,
            ];
            // «Fuera» en una cápsula = alejándose del segmento de la
            // espina [(0,−ch,0), (0,+ch,0)]: se resta el punto más
            // cercano del segmento al centroide.
            let y_clamped = c[1].clamp(-ch, ch);
            let fuera = [c[0], c[1] - y_clamped, c[2]];
            let dot = n[0] * fuera[0] + n[1] * fuera[1] + n[2] * fuera[2];
            assert!(dot > 0.0, "triángulo {:?} enrollado hacia dentro", tri);
        }
    }

    #[test]
    fn cilindro_conteos_extremos_normales_y_tapas() {
        let (vertices, indices) = generate_cylinder();
        let cols = (CYLINDER_RADIAL_SEGMENTS + 1) as usize;
        assert_eq!(vertices.len(), 4 * cols + 2);
        assert_eq!(indices.len(), (CYLINDER_RADIAL_SEGMENTS * 12) as usize);
        assert!(indices.iter().all(|&i| (i as usize) < vertices.len()));
        let r = glass_core::primitives::CYLINDER_RADIUS;
        let hh = glass_core::primitives::CYLINDER_HALF_HEIGHT;
        for v in &vertices {
            assert!((longitud(v.normal) - 1.0).abs() < 1e-5);
            assert!(v.position[1].abs() <= hh + 1e-6);
            let radio_xz = (v.position[0] * v.position[0] + v.position[2] * v.position[2]).sqrt();
            assert!(radio_xz <= r + 1e-6, "vértice fuera del radio: {:?}", v.position);
        }
        // Las tapas existen: hay vértices con normal ±Y y posición en ±hh.
        assert!(vertices.iter().any(|v| v.normal == [0.0, 1.0, 0.0] && (v.position[1] - hh).abs() < 1e-6));
        assert!(vertices.iter().any(|v| v.normal == [0.0, -1.0, 0.0] && (v.position[1] + hh).abs() < 1e-6));
    }

    #[test]
    fn cilindro_triangulos_hacia_fuera() {
        let (vertices, indices) = generate_cylinder();
        for tri in indices.chunks_exact(3) {
            let n = normal_geometrica(&vertices, tri);
            if longitud(n) < 1e-9 {
                continue;
            }
            let c = [
                (vertices[tri[0] as usize].position[0] + vertices[tri[1] as usize].position[0] + vertices[tri[2] as usize].position[0]) / 3.0,
                (vertices[tri[0] as usize].position[1] + vertices[tri[1] as usize].position[1] + vertices[tri[2] as usize].position[1]) / 3.0,
                (vertices[tri[0] as usize].position[2] + vertices[tri[1] as usize].position[2] + vertices[tri[2] as usize].position[2]) / 3.0,
            ];
            // En la banda manda la dirección radial XZ; en las tapas,
            // el signo de Y del centroide. El test usa la dirección
            // desde el eje (o desde el plano medio, en las tapas).
            let dot = if n[1].abs() > 0.99 * longitud(n) {
                n[1] * c[1] // tapa: la normal geométrica debe apuntar al lado del centroide
            } else {
                n[0] * c[0] + n[2] * c[2] // banda: alejándose del eje
            };
            assert!(dot > 0.0, "triángulo {:?} enrollado hacia dentro", tri);
        }
    }

    #[test]
    fn mip_levels_cadena_completa() {
        assert_eq!(mip_level_count(1, 1), 1);
        assert_eq!(mip_level_count(2, 2), 2);
        assert_eq!(mip_level_count(256, 256), 9);
        assert_eq!(mip_level_count(300, 200), 9);
        assert_eq!(mip_level_count(1024, 16), 11);
        assert_eq!(mip_level_count(0, 0), 1);
    }

    /// Construye un .glb mínimo en memoria: un triángulo con material PBR y,
    /// opcionalmente, una textura PNG embebida de 2x2 píxeles rojos.
    fn build_test_glb(with_texture: bool) -> Vec<u8> {
        // BIN: posiciones (36 B) @0, índices u16 (6 B) @36, PNG @44.
        let positions: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let indices: [u16; 3] = [0, 1, 2];
        let mut bin: Vec<u8> = Vec::new();
        for v in positions {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for i in indices {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        while bin.len() % 4 != 0 {
            bin.push(0);
        }

        let png = if with_texture {
            let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
            let mut cur = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(img)
                .write_to(&mut cur, image::ImageFormat::Png)
                .expect("codificar PNG de prueba");
            cur.into_inner()
        } else {
            Vec::new()
        };
        let png_offset = bin.len();
        bin.extend_from_slice(&png);
        while bin.len() % 4 != 0 {
            bin.push(0);
        }

        let buffer_views = if with_texture {
            format!(
                r#"[{{"buffer": 0, "byteOffset": 0, "byteLength": 36}}, {{"buffer": 0, "byteOffset": 36, "byteLength": 6}}, {{"buffer": 0, "byteOffset": {}, "byteLength": {}}}]"#,
                png_offset,
                png.len()
            )
        } else {
            r#"[{"buffer": 0, "byteOffset": 0, "byteLength": 36}, {"buffer": 0, "byteOffset": 36, "byteLength": 6}]"#.to_string()
        };
        let images_json = if with_texture {
            r#", "images": [{"bufferView": 2, "mimeType": "image/png"}], "textures": [{"source": 0}]"#
        } else {
            ""
        };
        let tex_ref = if with_texture {
            r#", "baseColorTexture": {"index": 0}"#
        } else {
            ""
        };

        let json = format!(
            r#"{{"asset": {{"version": "2.0"}}, "buffers": [{{"byteLength": {}}}], "bufferViews": {}, "accessors": [{{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]}}, {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}}], "materials": [{{"name": "RojoMetal", "pbrMetallicRoughness": {{"baseColorFactor": [1.0, 0.0, 0.0, 1.0], "metallicFactor": 0.8, "roughnessFactor": 0.25{}}}}}] , "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0}}, "indices": 1, "material": 0}}]}}]{}}}"#,
            bin.len(),
            buffer_views,
            tex_ref,
            images_json,
        );

        let mut glb: Vec<u8> = Vec::new();
        let json_bytes = json.as_bytes();
        let json_padded = (json_bytes.len() + 3) / 4 * 4;
        let total = 12 + 8 + json_padded + 8 + bin.len();
        glb.extend_from_slice(&0x4654_6C67u32.to_le_bytes()); // 'glTF'
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total as u32).to_le_bytes());
        glb.extend_from_slice(&(json_padded as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes()); // 'JSON'
        glb.extend_from_slice(json_bytes);
        glb.resize(12 + 8 + json_padded, b' ');
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E_4942u32.to_le_bytes()); // 'BIN\0'
        glb.extend_from_slice(&bin);
        glb
    }

    #[test]
    fn glb_importa_material_pbr() {
        let glb = build_test_glb(false);
        let model = MeshLoader::load_model_from_glb_bytes(&glb).expect("importar glb de prueba");
        assert_eq!(model.vertices.len(), 3);
        assert_eq!(model.primitive_materials, vec![Some(0)]);
        let mat = model.primary_material().expect("material principal");
        assert_eq!(mat.name, "RojoMetal");
        assert_eq!(mat.base_color_factor, [1.0, 0.0, 0.0, 1.0]);
        assert!((mat.metallic - 0.8).abs() < 1e-6);
        assert!((mat.roughness - 0.25).abs() < 1e-6);
        assert_eq!(mat.base_color_image, None);
    }

    #[test]
    fn glb_importa_textura_base() {
        let glb = build_test_glb(true);
        let model = MeshLoader::load_model_from_glb_bytes(&glb).expect("importar glb con textura");
        assert_eq!(model.images.len(), 1);
        let img = &model.images[0];
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.rgba.len(), 2 * 2 * 4);
        assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]);
        let mat = model.primary_material().expect("material principal");
        assert_eq!(mat.base_color_image, Some(0));
    }

    /// .glb en memoria con UNA malla de DOS primitivas (un triángulo cada
    /// una) y dos materiales distintos. BIN: posiciones A (36 B) @0,
    /// índices A u16 (6 B) @36, 2 B de relleno, posiciones B (36 B) @44,
    /// índices B u16 (6 B) @80; total 86 B.
    fn build_test_glb_two_primitives() -> Vec<u8> {
        let mut bin: Vec<u8> = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for i in [0u16, 1, 2] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        bin.extend_from_slice(&[0, 0]);
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for i in [0u16, 1, 2] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        assert_eq!(bin.len(), 86);

        let json = format!(
            r#"{{"asset": {{"version": "2.0"}}, "buffers": [{{"byteLength": {}}}], "bufferViews": [{{"buffer": 0, "byteOffset": 0, "byteLength": 36}}, {{"buffer": 0, "byteOffset": 36, "byteLength": 6}}, {{"buffer": 0, "byteOffset": 44, "byteLength": 36}}, {{"buffer": 0, "byteOffset": 80, "byteLength": 6}}], "accessors": [{{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]}}, {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}}, {{"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]}}, {{"bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR"}}], "materials": [{{"name": "Rojo", "pbrMetallicRoughness": {{"baseColorFactor": [1.0, 0.0, 0.0, 1.0]}}}}, {{"name": "Verde", "pbrMetallicRoughness": {{"baseColorFactor": [0.0, 1.0, 0.0, 1.0]}}}}], "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0}}, "indices": 1, "material": 0}}, {{"attributes": {{"POSITION": 2}}, "indices": 3, "material": 1}}]}}]}}"#,
            bin.len(),
        );

        let mut glb: Vec<u8> = Vec::new();
        let json_bytes = json.as_bytes();
        let json_padded = (json_bytes.len() + 3) / 4 * 4;
        let total = 12 + 8 + json_padded + 8 + bin.len();
        glb.extend_from_slice(&0x4654_6C67u32.to_le_bytes());
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total as u32).to_le_bytes());
        glb.extend_from_slice(&(json_padded as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
        glb.extend_from_slice(json_bytes);
        glb.resize(12 + 8 + json_padded, b' ');
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        glb.extend_from_slice(&bin);
        glb
    }

    #[test]
    fn glb_dos_primitivas_dos_materiales() {
        let glb = build_test_glb_two_primitives();
        let model = MeshLoader::load_model_from_glb_bytes(&glb).expect("importar glb de dos primitivas");
        assert_eq!(model.vertices.len(), 6);
        assert_eq!(model.indices, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(
            model.primitive_ranges,
            vec![
                PrimitiveRange { first_index: 0, index_count: 3, material: Some(0) },
                PrimitiveRange { first_index: 3, index_count: 3, material: Some(1) },
            ]
        );
        let primary = model.primary_material().expect("material principal");
        assert_eq!(primary.name, "Rojo");
        assert_eq!(model.materials[1].name, "Verde");
    }

    /// Regresión del bloque de skinning: un modelo SIN piel importa
    /// exactamente como antes de que existiera el bloque — misma
    /// geometría, sin datos de piel ni clips fantasma.
    #[test]
    fn glb_sin_piel_no_trae_datos_de_skinning() {
        let glb = build_test_glb(false);
        let model = MeshLoader::load_model_from_glb_bytes(&glb).expect("importar glb sin piel");
        assert!(model.skin.is_none());
        assert!(model.animations.is_empty());
        assert!(model.vertex_joints.is_empty());
        assert!(model.vertex_weights.is_empty());
        assert_eq!(model.vertices.len(), 3);
        assert_eq!(model.vertices[1].position, [1.0, 0.0, 0.0]);
    }

    /// El contrato de memoria de `SkinnedVertex` es el que declara el
    /// pipeline con piel (offsets de atributos) y el que entrelaza la
    /// subida `upload_skinned_to_gpu`. Si alguien reordena un campo, este
    /// test falla aquí en vez de deformar la malla en silencio.
    #[test]
    fn skinned_vertex_respeta_el_contrato_de_offsets() {
        assert_eq!(std::mem::size_of::<Vertex>(), 32);
        assert_eq!(std::mem::size_of::<SkinnedVertex>(), 56);
        assert_eq!(std::mem::offset_of!(SkinnedVertex, position), 0);
        assert_eq!(std::mem::offset_of!(SkinnedVertex, normal), 12);
        assert_eq!(std::mem::offset_of!(SkinnedVertex, uv), 24);
        assert_eq!(std::mem::offset_of!(SkinnedVertex, joints), 32);
        assert_eq!(std::mem::offset_of!(SkinnedVertex, weights), 40);
    }

    /// Construye un .glb mínimo CON piel en memoria: un triángulo, dos
    /// articulaciones encadenadas (la 2ª cuelga de la 1ª a +1 en Y), IBMs
    /// identidad y un clip «Saludar» que rota la articulación hija.
    fn build_test_glb_skinned() -> Vec<u8> {
        let mut bin: Vec<u8> = Vec::new();
        let f32s = |vals: &[f32], bin: &mut Vec<u8>| {
            for v in vals {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        };
        let u16s = |vals: &[u16], bin: &mut Vec<u8>| {
            for v in vals {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        };
        // @0 posiciones (36 B)
        f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &mut bin);
        // @36 JOINTS_0: v0→art0, v1→art1, v2→art0 (24 B)
        u16s(&[0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0], &mut bin);
        // @60 WEIGHTS_0 (48 B)
        f32s(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.0, 0.0], &mut bin);
        // @108 índices (6 B) + relleno a múltiplo de 4
        u16s(&[0, 1, 2], &mut bin);
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        // IBMs: dos matrices identidad (128 B)
        let ibm_offset = bin.len();
        let identidad: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        f32s(&identidad, &mut bin);
        f32s(&identidad, &mut bin);
        // Tiempos del clip (8 B)
        let times_offset = bin.len();
        f32s(&[0.0, 1.0], &mut bin);
        // Rotaciones del clip: identidad y 90° en Z (32 B)
        let rot_offset = bin.len();
        let s = std::f32::consts::FRAC_1_SQRT_2;
        f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, s, s], &mut bin);

        let json = format!(
            r#"{{"asset": {{"version": "2.0"}}, "scene": 0, "scenes": [{{"nodes": [0, 1]}}], "nodes": [{{"mesh": 0, "skin": 0}}, {{"children": [2]}}, {{"translation": [0.0, 1.0, 0.0]}}], "skins": [{{"joints": [1, 2], "inverseBindMatrices": 4}}], "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0, "JOINTS_0": 1, "WEIGHTS_0": 2}}, "indices": 3}}]}}], "animations": [{{"name": "Saludar", "channels": [{{"sampler": 0, "target": {{"node": 2, "path": "rotation"}}}}], "samplers": [{{"input": 5, "output": 6, "interpolation": "LINEAR"}}]}}], "buffers": [{{"byteLength": {}}}], "bufferViews": [{{"buffer": 0, "byteOffset": 0, "byteLength": 36}}, {{"buffer": 0, "byteOffset": 36, "byteLength": 24}}, {{"buffer": 0, "byteOffset": 60, "byteLength": 48}}, {{"buffer": 0, "byteOffset": 108, "byteLength": 6}}, {{"buffer": 0, "byteOffset": {}, "byteLength": 128}}, {{"buffer": 0, "byteOffset": {}, "byteLength": 8}}, {{"buffer": 0, "byteOffset": {}, "byteLength": 32}}], "accessors": [{{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]}}, {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "VEC4"}}, {{"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC4"}}, {{"bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR"}}, {{"bufferView": 4, "componentType": 5126, "count": 2, "type": "MAT4"}}, {{"bufferView": 5, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0.0], "max": [1.0]}}, {{"bufferView": 6, "componentType": 5126, "count": 2, "type": "VEC4"}}]}}"#,
            bin.len(),
            ibm_offset,
            times_offset,
            rot_offset
        );

        let mut glb: Vec<u8> = Vec::new();
        let json_bytes = json.as_bytes();
        let json_padded = (json_bytes.len() + 3) / 4 * 4;
        let total = 12 + 8 + json_padded + 8 + bin.len();
        glb.extend_from_slice(&0x4654_6C67u32.to_le_bytes());
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total as u32).to_le_bytes());
        glb.extend_from_slice(&(json_padded as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
        glb.extend_from_slice(json_bytes);
        glb.resize(12 + 8 + json_padded, b' ');
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        glb.extend_from_slice(&bin);
        glb
    }

    /// GLB mixto: el triángulo con piel del builder clásico + una
    /// segunda malla SIN piel en un nodo trasladado [5,0,0].
    fn build_test_glb_mixto() -> Vec<u8> {
        let mut bin: Vec<u8> = Vec::new();
        let f32s = |vals: &[f32], bin: &mut Vec<u8>| {
            for v in vals {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        };
        let u16s = |vals: &[u16], bin: &mut Vec<u8>| {
            for v in vals {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        };
        // @0 posiciones con piel (36 B)
        f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &mut bin);
        // @36 JOINTS_0 (24 B)
        u16s(&[0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0], &mut bin);
        // @60 WEIGHTS_0 (48 B)
        f32s(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.0, 0.0], &mut bin);
        // @108 índices (6 B) + relleno
        u16s(&[0, 1, 2], &mut bin);
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        // IBMs (128 B)
        let ibm_offset = bin.len();
        let identidad: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        f32s(&identidad, &mut bin);
        f32s(&identidad, &mut bin);
        // Tiempos (8 B) y rotaciones (32 B) del clip
        let times_offset = bin.len();
        f32s(&[0.0, 1.0], &mut bin);
        let rot_offset = bin.len();
        let s = std::f32::consts::FRAC_1_SQRT_2;
        f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, s, s], &mut bin);
        // Posiciones de la malla estática (36 B): el mismo triángulo.
        let pos2_offset = bin.len();
        f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &mut bin);

        let json = format!(
            r#"{{"asset": {{"version": "2.0"}}, "scene": 0, "scenes": [{{"nodes": [0, 1, 3]}}], "nodes": [{{"mesh": 0, "skin": 0}}, {{"children": [2]}}, {{"translation": [0.0, 1.0, 0.0]}}, {{"mesh": 1, "translation": [5.0, 0.0, 0.0]}}], "skins": [{{"joints": [1, 2], "inverseBindMatrices": 4}}], "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0, "JOINTS_0": 1, "WEIGHTS_0": 2}}, "indices": 3}}]}}, {{"primitives": [{{"attributes": {{"POSITION": 7}}, "indices": 3}}]}}], "animations": [{{"name": "Saludar", "channels": [{{"sampler": 0, "target": {{"node": 2, "path": "rotation"}}}}], "samplers": [{{"input": 5, "output": 6, "interpolation": "LINEAR"}}]}}], "buffers": [{{"byteLength": {}}}], "bufferViews": [{{"buffer": 0, "byteOffset": 0, "byteLength": 36}}, {{"buffer": 0, "byteOffset": 36, "byteLength": 24}}, {{"buffer": 0, "byteOffset": 60, "byteLength": 48}}, {{"buffer": 0, "byteOffset": 108, "byteLength": 6}}, {{"buffer": 0, "byteOffset": {}, "byteLength": 128}}, {{"buffer": 0, "byteOffset": {}, "byteLength": 8}}, {{"buffer": 0, "byteOffset": {}, "byteLength": 32}}, {{"buffer": 0, "byteOffset": {}, "byteLength": 36}}], "accessors": [{{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]}}, {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "VEC4"}}, {{"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC4"}}, {{"bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR"}}, {{"bufferView": 4, "componentType": 5126, "count": 2, "type": "MAT4"}}, {{"bufferView": 5, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0.0], "max": [1.0]}}, {{"bufferView": 6, "componentType": 5126, "count": 2, "type": "VEC4"}}, {{"bufferView": 7, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]}}]}}"#,
            bin.len(),
            ibm_offset,
            times_offset,
            rot_offset,
            pos2_offset
        );

        let mut glb: Vec<u8> = Vec::new();
        let json_bytes = json.as_bytes();
        let json_padded = (json_bytes.len() + 3) / 4 * 4;
        let total = 12 + 8 + json_padded + 8 + bin.len();
        glb.extend_from_slice(&0x4654_6C67u32.to_le_bytes());
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total as u32).to_le_bytes());
        glb.extend_from_slice(&(json_padded as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
        glb.extend_from_slice(json_bytes);
        glb.resize(12 + 8 + json_padded, b' ');
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        glb.extend_from_slice(&bin);
        glb
    }

    #[test]
    fn glb_mixto_la_malla_sin_piel_conserva_su_nodo() {
        // Pasada de medios (menor): en un GLB con piel, la malla sin
        // piel se fusiona con la transformación de su nodo (x+5) y
        // pesos cero (el shader la deja pasar sin deformar); la parte
        // con piel queda intacta en el origen.
        let glb = build_test_glb_mixto();
        let model = MeshLoader::load_model_from_glb_bytes(&glb).expect("importar glb mixto");
        assert!(model.skin.is_some());
        assert_eq!(model.vertices.len(), 6);
        assert_eq!(model.vertices[0].position, [0.0, 0.0, 0.0]);
        assert_eq!(model.vertices[3].position, [5.0, 0.0, 0.0]);
        assert_eq!(model.vertices[4].position, [6.0, 0.0, 0.0]);
        assert_eq!(model.vertices[5].position, [5.0, 1.0, 0.0]);
        assert_eq!(model.vertex_weights[3], [0.0; 4]);
        assert!(model.vertex_weights[0][0] > 0.0);
    }

    #[test]
    fn glb_con_piel_importa_articulaciones_atributos_y_clip() {
        let glb = build_test_glb_skinned();
        let model = MeshLoader::load_model_from_glb_bytes(&glb).expect("importar glb con piel");

        let skin = model.skin.as_ref().expect("el modelo debe traer piel");
        assert_eq!(skin.mesh_node, 0);
        assert_eq!(skin.joints, vec![1, 2]);
        assert_eq!(skin.nodes.len(), 3);
        assert_eq!(skin.nodes[2].parent, Some(1));
        assert_eq!(skin.nodes[1].parent, None);
        assert_eq!(skin.nodes[2].translation, [0.0, 1.0, 0.0]);
        assert_eq!(skin.inverse_bind_matrices.len(), 2);
        let identidad: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        assert_eq!(skin.inverse_bind_matrices[0], identidad);
        assert_eq!(skin.inverse_bind_matrices[1], identidad);

        assert_eq!(
            model.vertex_joints,
            vec![[0, 0, 0, 0], [1, 0, 0, 0], [0, 1, 0, 0]]
        );
        assert_eq!(model.vertex_weights.len(), 3);
        assert_eq!(model.vertex_weights[2], [0.5, 0.5, 0.0, 0.0]);

        assert_eq!(model.animations.len(), 1);
        let clip = &model.animations[0];
        assert_eq!(clip.name, "Saludar");
        assert!((clip.duration - 1.0).abs() < 1e-6);
        assert_eq!(clip.channels.len(), 1);
        assert_eq!(clip.channels[0].node, 2);
        match &clip.channels[0].values {
            crate::skin::AnimChannelValues::Rotation(quats) => {
                assert_eq!(quats.len(), 2);
                assert_eq!(quats[0], [0.0, 0.0, 0.0, 1.0]);
            }
            otro => panic!("se esperaba un canal de rotación, llegó {:?}", otro),
        }
    }

    /// Una piel por encima de MAX_JOINTS es un error de importación con
    /// mensaje, no una deformación silenciosa a mitad: el .glb es válido
    /// (257 articulaciones reales con sus 257 IBMs), y quien rechaza es
    /// NUESTRO validador.
    #[test]
    fn glb_piel_enorme_se_rechaza_con_error_claro() {
        let n = crate::skin::MAX_JOINTS + 1;
        let mut bin: Vec<u8> = Vec::new();
        // @0 posiciones (36 B), @36 índices (6 B) + relleno a 44.
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for i in [0u16, 1, 2] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let ibm_offset = bin.len();
        let identidad: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        for _ in 0..n {
            for v in identidad {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        let joints_json = vec!["1"; n].join(", ");
        let json = format!(
            r#"{{"asset": {{"version": "2.0"}}, "scene": 0, "scenes": [{{"nodes": [0, 1]}}], "nodes": [{{"mesh": 0, "skin": 0}}, {{}}], "skins": [{{"joints": [{}], "inverseBindMatrices": 2}}], "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0}}, "indices": 1}}]}}], "buffers": [{{"byteLength": {}}}], "bufferViews": [{{"buffer": 0, "byteOffset": 0, "byteLength": 36}}, {{"buffer": 0, "byteOffset": 36, "byteLength": 6}}, {{"buffer": 0, "byteOffset": {}, "byteLength": {}}}], "accessors": [{{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]}}, {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}}, {{"bufferView": 2, "componentType": 5126, "count": {}, "type": "MAT4"}}]}}"#,
            joints_json,
            bin.len(),
            ibm_offset,
            n * 64,
            n
        );
        let mut glb: Vec<u8> = Vec::new();
        let json_bytes = json.as_bytes();
        let json_padded = (json_bytes.len() + 3) / 4 * 4;
        let total = 12 + 8 + json_padded + 8 + bin.len();
        glb.extend_from_slice(&0x4654_6C67u32.to_le_bytes());
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total as u32).to_le_bytes());
        glb.extend_from_slice(&(json_padded as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
        glb.extend_from_slice(json_bytes);
        glb.resize(12 + 8 + json_padded, b' ');
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        glb.extend_from_slice(&bin);

        let err = MeshLoader::load_model_from_glb_bytes(&glb)
            .err()
            .expect("una piel gigante no debe importar");
        assert!(
            err.contains("articulaciones"),
            "el error debe explicar el límite, llegó: {err}"
        );
    }
}
