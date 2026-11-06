//! Skinning: datos de piel/animación importados de glTF, evaluación de la
//! pose en CPU y buffer de matrices de articulación en GPU.
//!
//! Diseño (bloque de skinning, ver REGISTRO_SKINNING.md):
//! - El importador conserva el grafo de nodos del documento SOLO cuando hay
//!   una piel: sin piel, un modelo no paga ni un byte de esto.
//! - La pose se evalúa en CPU por frame (pocos personajes, decenas de
//!   articulaciones: el coste es despreciable) y las matrices resultantes
//!   se escriben en un buffer CpuToGpu por ENTIDAD, que el vertex shader
//!   `pbr_skinned.vert` lee como SSBO en set 1. La sincronización es la del
//!   motor: un solo fence compartido que `render_frame` espera antes de
//!   tocar nada, así que al escribir el buffer la GPU ya terminó el frame
//!   anterior. Si algún día hay más de un frame en vuelo, este esquema
//!   necesitará un buffer por frame en vuelo (deuda anotada, no oculta).
//! - La tubería sin piel no se toca: mallas con piel van por su propio
//!   pipeline y su propio formato de vértice.

use ash::vk;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator};
use gpu_allocator::MemoryLocation;
use std::sync::Arc;

/// Máximo de articulaciones por piel que el motor acepta. El importador
/// RECHAZA con error claro un modelo que lo exceda en lugar de desbordar
/// el SSBO en silencio. 256 huesos cubre de sobra personajes jugables y
/// NPC (los rigs típicos de juego usan 30-120).
pub const MAX_JOINTS: usize = 256;

/// Un nodo del grafo glTF con su transform local de reposo (bind pose).
/// Solo se conserva para modelos con piel: es el esqueleto sobre el que
/// los canales de animación escriben.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GltfNodeData {
    pub parent: Option<usize>,
    pub translation: [f32; 3],
    /// Cuaternión `[x, y, z, w]`.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl Default for GltfNodeData {
    fn default() -> Self {
        Self {
            parent: None,
            translation: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

/// La piel de un modelo: qué nodos son articulaciones, sus matrices
/// inversas de unión y el nodo que lleva la malla a la que la piel deforma.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinData {
    /// Nodos del documento, en el orden del glTF (los canales de animación
    /// y las articulaciones referencian estos índices).
    pub nodes: Vec<GltfNodeData>,
    /// Articulaciones: índices de nodo, en el orden de la piel del glTF.
    /// Los `JOINTS_0` de los vértices indexan ESTA lista (posición), no el
    /// índice de nodo directamente.
    pub joints: Vec<usize>,
    /// Matriz inversa de unión por articulación, mismo orden que `joints`.
    pub inverse_bind_matrices: Vec<[f32; 16]>,
    /// Nodo que referencia la malla con esta piel. Se conserva como dato
    /// del documento, pero la fórmula de piel NO lo usa: los exportadores
    /// (Blender) escriben las IBM como inversas de la global de cada
    /// articulación en reposo, sin la global del nodo-malla, y la
    /// deformación se produce en el espacio local de la malla.
    pub mesh_node: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    Linear,
    Step,
}

/// Qué propiedad de un nodo anima un canal.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimChannelValues {
    Translation(Vec<[f32; 3]>),
    Rotation(Vec<[f32; 4]>),
    Scale(Vec<[f32; 3]>),
}

/// Un canal de animación: la curva de una propiedad de un nodo.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimChannel {
    pub node: usize,
    pub interpolation: Interpolation,
    pub times: Vec<f32>,
    pub values: AnimChannelValues,
}

/// Un clip de animación importado de un glTF.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationClip {
    pub name: String,
    /// Segundos del clip (último tiempo de keyframe entre sus canales).
    pub duration: f32,
    pub channels: Vec<AnimChannel>,
}

/// Busca el tramo de keyframes que contiene `t` y devuelve
/// (índice izquierdo, factor de mezcla 0..1). Fuera de rango se clampa a
/// los extremos: el muestreo nunca sale de la curva.
fn keyframe_span(times: &[f32], t: f32) -> (usize, f32) {
    if times.len() < 2 || t <= times[0] {
        return (0, 0.0);
    }
    let last = times.len() - 1;
    if t >= times[last] {
        return (last - 1, 1.0);
    }
    // Búsqueda binaria del tramo [i, i+1) que contiene t.
    let mut lo = 0usize;
    let mut hi = last;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if times[mid] <= t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let span = times[lo + 1] - times[lo];
    let f = if span > 0.0 { (t - times[lo]) / span } else { 0.0 };
    (lo, f.clamp(0.0, 1.0))
}

fn sample_vec3(times: &[f32], values: &[[f32; 3]], t: f32, interp: Interpolation) -> [f32; 3] {
    if values.is_empty() {
        return [0.0, 0.0, 0.0];
    }
    let (i, f) = keyframe_span(times, t);
    if interp == Interpolation::Step || f == 0.0 || i + 1 >= values.len() {
        return values[i.min(values.len() - 1)];
    }
    let a = values[i];
    let b = values[i + 1];
    [
        a[0] + f * (b[0] - a[0]),
        a[1] + f * (b[1] - a[1]),
        a[2] + f * (b[2] - a[2]),
    ]
}

fn sample_quat(times: &[f32], values: &[[f32; 4]], t: f32, interp: Interpolation) -> [f32; 4] {
    if values.is_empty() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let (i, f) = keyframe_span(times, t);
    if interp == Interpolation::Step || f == 0.0 || i + 1 >= values.len() {
        return glass_core::math::quat_normalize(values[i.min(values.len() - 1)]);
    }
    glass_core::math::quat_slerp(values[i], values[i + 1], f)
}

/// Pose local de todos los nodos de una piel en un instante: reposo
/// sobrescrito por los canales del clip. Es la estructura intermedia que
/// `evaluate_skin_matrices` compone en globales; se extrae para poder
/// mezclar dos poses ANTES de componer (crossfade).
struct LocalPose {
    translations: Vec<[f32; 3]>,
    rotations: Vec<[f32; 4]>,
    scales: Vec<[f32; 3]>,
}

/// Muestrea la pose local de `skin` para `clip` en `time`, con el mismo
/// envoltorio de bucle que usaba `evaluate_skin_matrices` (wrap por
/// `duration` vía `rem_euclid`). `clip = None` da la pose de reposo.
fn sample_local_pose(skin: &SkinData, clip: Option<&AnimationClip>, time: f32) -> LocalPose {
    let node_count = skin.nodes.len();

    let mut translations: Vec<[f32; 3]> = skin.nodes.iter().map(|n| n.translation).collect();
    let mut rotations: Vec<[f32; 4]> = skin.nodes.iter().map(|n| n.rotation).collect();
    let mut scales: Vec<[f32; 3]> = skin.nodes.iter().map(|n| n.scale).collect();

    if let Some(clip) = clip {
        let t = if clip.duration > 0.0 {
            time.rem_euclid(clip.duration)
        } else {
            0.0
        };
        for channel in &clip.channels {
            if channel.node >= node_count {
                continue;
            }
            match &channel.values {
                AnimChannelValues::Translation(values) => {
                    translations[channel.node] =
                        sample_vec3(&channel.times, values, t, channel.interpolation);
                }
                AnimChannelValues::Rotation(values) => {
                    rotations[channel.node] =
                        sample_quat(&channel.times, values, t, channel.interpolation);
                }
                AnimChannelValues::Scale(values) => {
                    scales[channel.node] =
                        sample_vec3(&channel.times, values, t, channel.interpolation);
                }
            }
        }
    }

    LocalPose {
        translations,
        rotations,
        scales,
    }
}

/// Calcula las globales de todos los nodos de `skin` a partir de una pose
/// local, con memoización (recursión acotada igual que antes: un ciclo de
/// padres devuelve identidad para romperlo).
fn compute_globals(skin: &SkinData, pose: &LocalPose) -> Vec<[f32; 16]> {
    fn global_of(
        idx: usize,
        skin: &SkinData,
        pose: &LocalPose,
        memo: &mut [Option<[f32; 16]>],
        depth: usize,
    ) -> [f32; 16] {
        if let Some(m) = memo[idx] {
            return m;
        }
        let node = &skin.nodes[idx];
        let local = glass_core::math::mat4_from_trs_quat(
            pose.translations[idx],
            pose.rotations[idx],
            pose.scales[idx],
        );
        let global = match node.parent {
            None => local,
            Some(parent) if parent < skin.nodes.len() && depth < skin.nodes.len() => {
                let parent_global = global_of(parent, skin, pose, memo, depth + 1);
                glass_core::math::mat4_mul(parent_global, local)
            }
            _ => local,
        };
        memo[idx] = Some(global);
        global
    }

    let node_count = skin.nodes.len();
    let mut memo: Vec<Option<[f32; 16]>> = vec![None; node_count];
    (0..node_count)
        .map(|idx| global_of(idx, skin, pose, &mut memo, 0))
        .collect()
}

/// Compone las matrices de piel finales a partir de las globales de nodo
/// ya calculadas: piel[i] = global(articulación[i]) * IBM[i].
fn compose_skin_matrices(skin: &SkinData, globals: &[[f32; 16]]) -> Vec<[f32; 16]> {
    let node_count = skin.nodes.len();
    skin.joints
        .iter()
        .enumerate()
        .map(|(i, &joint_node)| {
            let joint_global = if joint_node < node_count {
                globals[joint_node]
            } else {
                glass_core::math::mat4_identity()
            };
            let ibm = skin
                .inverse_bind_matrices
                .get(i)
                .copied()
                .unwrap_or_else(glass_core::math::mat4_identity);
            glass_core::math::mat4_mul(joint_global, ibm)
        })
        .collect()
}

/// Evalúa las matrices de piel de un modelo en un instante.
///
/// `clip = None` (o tiempo cualquiera sin clip) produce la pose de reposo
/// (bind pose). Con clip, el tiempo da la vuelta al llegar a la duración
/// (bucle). La fórmula por articulación es la de los exportadores glTF
/// (Blender) y los motores habituales:
///
/// ```text
/// piel[i] = global(articulación[i]) * IBM[i]
/// ```
///
/// Las IBM que escribe Blender son la inversa de la global de la
/// articulación en reposo, sin ningún factor del nodo-malla: la
/// deformación queda en el espacio local de la malla y en reposo es la
/// identidad. La primera versión de esta función multiplicaba además por
/// la inversa de la global del nodo-malla; eso solo cuadraba cuando ese
/// nodo era la raíz identidad (el activo de prueba generado por código)
/// y rompía cualquier exportación estándar cuyo nodo-malla cuelgue de
/// una armadura transformada: en Mixamo la armadura lleva escala 0,01 y
/// el personaje salía multiplicado por 100 (defecto cazado por la
/// prueba de Mixamo, 2026-11-05; test
/// `bind_pose_con_armadura_transformada_da_identidad`).
/// Las globales se calculan con memoización: el orden de los nodos en el
/// archivo no garantiza padres antes que hijos.
pub fn evaluate_skin_matrices(
    skin: &SkinData,
    clip: Option<&AnimationClip>,
    time: f32,
) -> Vec<[f32; 16]> {
    let pose = sample_local_pose(skin, clip, time);
    let globals = compute_globals(skin, &pose);
    compose_skin_matrices(skin, &globals)
}
///

/// Evalúa las matrices de piel mezclando DOS clips (crossfade) antes de
/// componer globales: la pose local de cada nodo se interpola (lerp en
/// traslación/escala, slerp en rotación) y el resultado se compone con la
/// misma fórmula que `evaluate_skin_matrices` (piel[i] = global * IBM).
///
/// `w` se clampa a [0,1]: `w=0` reproduce exactamente `clip_a` solo,
/// `w=1` reproduce exactamente `clip_b` solo. Cada clip usa su propio
/// tiempo (`time_a`, `time_b`) con el mismo wrap por duración que
/// `sample_local_pose`/`evaluate_skin_matrices` ya aplican. Si una piel o
/// clip tiene menos nodos animados que el otro, el que falta aporta la
/// pose de reposo de ese nodo (igual que `sample_local_pose` hace con
/// nodos sin canal).
pub fn evaluate_skin_matrices_blended(
    skin: &SkinData,
    clip_a: Option<&AnimationClip>,
    time_a: f32,
    clip_b: Option<&AnimationClip>,
    time_b: f32,
    w: f32,
) -> Vec<[f32; 16]> {
    let w = w.clamp(0.0, 1.0);

    let pose_a = sample_local_pose(skin, clip_a, time_a);
    let pose_b = sample_local_pose(skin, clip_b, time_b);

    // Ambas poses tienen un elemento por nodo de `skin` (sample_local_pose
    // siempre produce `skin.nodes.len()` entradas, con reposo donde no hay
    // canal), así que no hace falta igualar longitudes aquí: el "máximo de
    // nodos, reposo donde falte" ya lo resuelve `sample_local_pose` al
    // partir del reposo de `skin.nodes`.
    let node_count = skin.nodes.len();
    let mut translations = Vec::with_capacity(node_count);
    let mut rotations = Vec::with_capacity(node_count);
    let mut scales = Vec::with_capacity(node_count);

    for i in 0..node_count {
        translations.push(lerp_vec3(pose_a.translations[i], pose_b.translations[i], w));
        rotations.push(slerp_shortest_normalized(
            pose_a.rotations[i],
            pose_b.rotations[i],
            w,
        ));
        scales.push(lerp_vec3(pose_a.scales[i], pose_b.scales[i], w));
    }

    let pose = LocalPose {
        translations,
        rotations,
        scales,
    };
    let globals = compute_globals(skin, &pose);
    compose_skin_matrices(skin, &globals)
}

/// Interpolación lineal de un vec3.
fn lerp_vec3(a: [f32; 3], b: [f32; 3], w: f32) -> [f32; 3] {
    [
        a[0] + w * (b[0] - a[0]),
        a[1] + w * (b[1] - a[1]),
        a[2] + w * (b[2] - a[2]),
    ]
}

/// Slerp entre dos cuaterniones eligiendo el camino corto (si el
/// producto punto es negativo, se niega `b` antes de interpolar) y
/// normalizando el resultado. `w=0` da `a` normalizado, `w=1` da `b`
/// normalizado (hasta el signo, que no afecta a la rotación representada).
fn slerp_shortest_normalized(a: [f32; 4], b: [f32; 4], w: f32) -> [f32; 4] {
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let b = if dot < 0.0 {
        [-b[0], -b[1], -b[2], -b[3]]
    } else {
        b
    };
    glass_core::math::quat_normalize(glass_core::math::quat_slerp(a, b, w))
}

/// Buffer GPU (SSBO) con las matrices de articulación de UNA entidad con
/// piel, más su descriptor set (set 1 del pipeline con piel). Vive mientras
/// la entidad lo use; la app lo guarda por id de entidad y lo destruye al
/// cambiar la escena o al cerrar.
pub struct JointBuffer {
    pub buffer: vk::Buffer,
    allocation: Option<Allocation>,
    pub descriptor_set: vk::DescriptorSet,
    /// Capacidad en matrices (nº de articulaciones para el que se creó).
    pub joint_count: usize,
    mapped: *mut u8,
}

// La escritura ocurre solo desde el hilo de render, con la GPU en reposo
// tras la espera del fence del frame (ver la nota de sincronización del
// módulo). El puntero mapeado no se comparte.
unsafe impl Send for JointBuffer {}

impl JointBuffer {
    pub fn new(
        device: &Arc<ash::Device>,
        allocator: &mut Allocator,
        pool: vk::DescriptorPool,
        set_layout: vk::DescriptorSetLayout,
        joint_count: usize,
    ) -> Result<Self, String> {
        let joint_count = joint_count.max(1);
        let size = (joint_count * 64) as u64;
        unsafe {
            let buffer_info = vk::BufferCreateInfo::default()
                .size(size)
                .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);
            let buffer = device
                .create_buffer(&buffer_info, None)
                .map_err(|e| format!("Fallo al crear buffer de articulaciones: {}", e))?;
            let mem_reqs = device.get_buffer_memory_requirements(buffer);
            let allocation = allocator
                .allocate(&AllocationCreateDesc {
                    name: "joint_matrices",
                    requirements: mem_reqs,
                    location: MemoryLocation::CpuToGpu,
                    linear: true,
                    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                })
                .map_err(|e| format!("Fallo de asignación del buffer de articulaciones: {}", e))?;
            device
                .bind_buffer_memory(buffer, allocation.memory(), allocation.offset())
                .map_err(|e| format!("Fallo al enlazar el buffer de articulaciones: {}", e))?;

            let layouts = [set_layout];
            let alloc_info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(pool)
                .set_layouts(&layouts);
            let descriptor_set = device
                .allocate_descriptor_sets(&alloc_info)
                .map_err(|e| format!("Fallo al reservar el set de articulaciones: {}", e))?
                .into_iter()
                .next()
                .ok_or("Pool de articulaciones sin sets")?;
            let buffer_info_desc = [vk::DescriptorBufferInfo::default()
                .buffer(buffer)
                .offset(0)
                .range(size)];
            let write = [vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffer_info_desc)];
            device.update_descriptor_sets(&write, &[]);

            let mapped = allocation
                .mapped_ptr()
                .map(|p| p.as_ptr() as *mut u8)
                .ok_or("El buffer de articulaciones no quedó mapeado")?;

            Ok(Self {
                buffer,
                allocation: Some(allocation),
                descriptor_set,
                joint_count,
                mapped,
            })
        }
    }

    /// Escribe las matrices del frame. Si llegan más que la capacidad, se
    /// recortan (el importador ya rechazó pieles por encima de MAX_JOINTS;
    /// esto es solo la red de seguridad).
    pub fn write(&mut self, matrices: &[[f32; 16]]) {
        let count = matrices.len().min(self.joint_count);
        if count == 0 {
            return;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                matrices.as_ptr() as *const u8,
                self.mapped,
                count * 64,
            );
        }
    }

    pub fn destroy(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        pool: vk::DescriptorPool,
    ) {
        unsafe {
            let _ = device.free_descriptor_sets(pool, &[self.descriptor_set]);
            if let Some(alloc) = self.allocation.take() {
                let _ = allocator.free(alloc);
            }
            device.destroy_buffer(self.buffer, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Esqueleto mínimo: raíz (nodo 0) en el origen, articulación (nodo 1)
    /// hija a +1 en X, y el nodo-malla (2) en el origen sin padre. La
    /// global de la articulación en reposo es T(1,0,0) y su IBM, T(-1,0,0):
    /// en bind pose la matriz de piel debe ser la identidad.
    fn skin_simple() -> SkinData {
        let mut joint = GltfNodeData::default();
        joint.parent = Some(0);
        joint.translation = [1.0, 0.0, 0.0];
        SkinData {
            nodes: vec![GltfNodeData::default(), joint, GltfNodeData::default()],
            joints: vec![1],
            inverse_bind_matrices: vec![glass_core::math::mat4_from_trs_quat(
                [-1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
                [1.0, 1.0, 1.0],
            )],
            mesh_node: 2,
        }
    }

    fn clip_rotacion_z(grados_final: f32, duracion: f32) -> AnimationClip {
        let completo = grados_final.to_radians();
        let q = |a: f32| [0.0, 0.0, (a / 2.0).sin(), (a / 2.0).cos()];
        AnimationClip {
            name: "Giro".into(),
            duration: duracion,
            channels: vec![AnimChannel {
                node: 1,
                interpolation: Interpolation::Linear,
                times: vec![0.0, duracion],
                values: AnimChannelValues::Rotation(vec![q(0.0), q(completo)]),
            }],
        }
    }

    #[test]
    fn bind_pose_da_identidad() {
        let skin = skin_simple();
        let mats = evaluate_skin_matrices(&skin, None, 0.0);
        assert_eq!(mats.len(), 1);
        let identidad = glass_core::math::mat4_identity();
        for i in 0..16 {
            assert!((mats[0][i] - identidad[i]).abs() < 1e-5, "elemento {i}");
        }
    }

    #[test]
    fn muestreo_en_el_medio_del_clip_rota_45() {
        // Clip: la articulación rota 0 -> 90 grados en Z durante 1 s.
        // En t=0.5 la rotación debe ser 45 grados: el punto (1,0,0) (que en
        // reposo está a +1 de la articulación tras la IBM... el vértice de
        // prueba es el origen de la articulación: su posición de reposo en
        // espacio de malla es (1,0,0)) debe ir a (cos45, sin45, 0) tras la
        // matriz de piel, porque la articulación está en (1,0,0) y rota
        // sobre sí misma... la traslación se conserva y el punto hijo a
        // distancia 1 en X local, (2,0,0) en la malla, va a
        // (1 + cos45, sin45, 0).
        let skin = skin_simple();
        let clip = clip_rotacion_z(90.0, 1.0);
        let mats = evaluate_skin_matrices(&skin, Some(&clip), 0.5);
        let p = glass_core::math::mat4_transform_point(mats[0], [2.0, 0.0, 0.0]);
        let c45 = std::f32::consts::FRAC_1_SQRT_2;
        assert!((p[0] - (1.0 + c45)).abs() < 1e-4, "x: {}", p[0]);
        assert!((p[1] - c45).abs() < 1e-4, "y: {}", p[1]);
        assert!(p[2].abs() < 1e-4, "z: {}", p[2]);
    }

    #[test]
    fn el_bucle_envuelve_el_tiempo() {
        let skin = skin_simple();
        let clip = clip_rotacion_z(90.0, 1.0);
        let a = evaluate_skin_matrices(&skin, Some(&clip), 0.25);
        let b = evaluate_skin_matrices(&skin, Some(&clip), 1.25);
        let c = evaluate_skin_matrices(&skin, Some(&clip), 7.25);
        for i in 0..16 {
            assert!((a[0][i] - b[0][i]).abs() < 1e-5);
            assert!((a[0][i] - c[0][i]).abs() < 1e-5);
        }
    }

    #[test]
    fn interpolacion_step_salta_sin_mezclar() {
        let skin = skin_simple();
        let mut clip = clip_rotacion_z(90.0, 1.0);
        clip.channels[0].interpolation = Interpolation::Step;
        // En t=0.75 con STEP sigue valiendo el keyframe izquierdo (0 grados):
        // la matriz de piel es la de reposo (identidad para el punto (2,0,0)).
        let mats = evaluate_skin_matrices(&skin, Some(&clip), 0.75);
        let p = glass_core::math::mat4_transform_point(mats[0], [2.0, 0.0, 0.0]);
        assert!((p[0] - 2.0).abs() < 1e-4 && p[1].abs() < 1e-4, "{p:?}");
        // Y en t=1.0 envuelve a t=0 (bucle), no al último keyframe.
        let mats = evaluate_skin_matrices(&skin, Some(&clip), 0.999);
        let p = glass_core::math::mat4_transform_point(mats[0], [2.0, 0.0, 0.0]);
        assert!((p[0] - 2.0).abs() < 1e-4 && p[1].abs() < 1e-4, "{p:?}");
    }

    #[test]
    fn bind_pose_con_armadura_transformada_da_identidad() {
        // Estructura típica de Mixamo/Blender: la malla y el esqueleto
        // cuelgan de un nodo armadura con escala 0,01 y rotación, y las
        // IBM son las inversas de las globales de articulación en reposo
        // (sin la global del nodo-malla). La pose de reposo debe ser la
        // identidad; con la fórmula anterior (que multiplicaba por la
        // inversa de la global del nodo-malla) el personaje salía
        // deformado por esa transformada (×100 en el caso de Mixamo).
        let qx90 = [0.70710678f32, 0.0, 0.0, 0.70710678];
        let mut root = GltfNodeData::default();
        root.rotation = qx90;
        root.scale = [0.01, 0.01, 0.01];
        let mut joint = GltfNodeData::default();
        joint.parent = Some(0);
        joint.translation = [100.0, 0.0, 0.0];
        let mut mesh = GltfNodeData::default();
        mesh.parent = Some(0);
        let root_m =
            glass_core::math::mat4_from_trs_quat([0.0, 0.0, 0.0], qx90, [0.01, 0.01, 0.01]);
        let joint_global = glass_core::math::mat4_mul(
            root_m,
            glass_core::math::mat4_from_trs_quat(
                [100.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
                [1.0, 1.0, 1.0],
            ),
        );
        let ibm = glass_core::math::mat4_inverse_affine(joint_global).expect("ibm invertible");
        let skin = SkinData {
            nodes: vec![root, joint, mesh],
            joints: vec![1],
            inverse_bind_matrices: vec![ibm],
            mesh_node: 2,
        };
        let mats = evaluate_skin_matrices(&skin, None, 0.0);
        let identidad = glass_core::math::mat4_identity();
        for i in 0..16 {
            assert!((mats[0][i] - identidad[i]).abs() < 1e-4, "elemento {i}: {}", mats[0][i]);
        }
    }

    #[test]
    fn canal_de_traslacion_se_interpola() {
        let skin = skin_simple();
        let clip = AnimationClip {
            name: "Sube".into(),
            duration: 2.0,
            channels: vec![AnimChannel {
                node: 1,
                interpolation: Interpolation::Linear,
                times: vec![0.0, 2.0],
                values: AnimChannelValues::Translation(vec![[1.0, 0.0, 0.0], [1.0, 2.0, 0.0]]),
            }],
        };
        // t=1: la articulación está en (1,1,0); el punto (2,0,0) sube a (2,1,0).
        let mats = evaluate_skin_matrices(&skin, Some(&clip), 1.0);
        let p = glass_core::math::mat4_transform_point(mats[0], [2.0, 0.0, 0.0]);
        assert!((p[0] - 2.0).abs() < 1e-4 && (p[1] - 1.0).abs() < 1e-4, "{p:?}");
    }

// Insertar dentro de `mod tests`, junto a los tests existentes.

#[test]
fn blended_w0_reproduce_clip_a_solo() {
    let skin = skin_simple();
    let clip_a = clip_rotacion_z(90.0, 1.0);
    let clip_b = clip_rotacion_z(30.0, 1.0);
    let solo = evaluate_skin_matrices(&skin, Some(&clip_a), 0.5);
    let mezclado =
        evaluate_skin_matrices_blended(&skin, Some(&clip_a), 0.5, Some(&clip_b), 0.5, 0.0);
    for i in 0..16 {
        assert!((solo[0][i] - mezclado[0][i]).abs() < 1e-5, "elemento {i}");
    }
}

#[test]
fn blended_w1_reproduce_clip_b_solo() {
    let skin = skin_simple();
    let clip_a = clip_rotacion_z(90.0, 1.0);
    let clip_b = clip_rotacion_z(30.0, 1.0);
    let solo = evaluate_skin_matrices(&skin, Some(&clip_b), 0.5);
    let mezclado =
        evaluate_skin_matrices_blended(&skin, Some(&clip_a), 0.5, Some(&clip_b), 0.5, 1.0);
    for i in 0..16 {
        assert!((solo[0][i] - mezclado[0][i]).abs() < 1e-5, "elemento {i}");
    }
}

#[test]
fn blended_w_medio_da_la_rotacion_intermedia() {
    // El helper `clip_rotacion_z` anima de 0 al ángulo final a lo largo
    // de la duración (no es una pose constante): clip a llega a 0 grados
    // (0 en todo instante) y clip b, de 180 grados en 2 s, está en 90
    // grados a t=1. Con w=0,5 la articulación debe quedar a 45 grados:
    // el punto (2,0,0) va a (1+cos45, sin45, 0), igual que el test de
    // muestreo simple a mitad de clip.
    let skin = skin_simple();
    let clip_a = clip_rotacion_z(0.0, 1.0);
    let clip_b = clip_rotacion_z(180.0, 2.0);
    let mats = evaluate_skin_matrices_blended(&skin, Some(&clip_a), 1.0, Some(&clip_b), 1.0, 0.5);
    let p = glass_core::math::mat4_transform_point(mats[0], [2.0, 0.0, 0.0]);
    let c45 = std::f32::consts::FRAC_1_SQRT_2;
    assert!((p[0] - (1.0 + c45)).abs() < 1e-4, "x: {}", p[0]);
    assert!((p[1] - c45).abs() < 1e-4, "y: {}", p[1]);
    assert!(p[2].abs() < 1e-4, "z: {}", p[2]);
}

#[test]
fn blended_sin_clips_da_identidad() {
    // Regresión: sin canales en ningún lado, la pose mezclada sigue
    // siendo la de reposo (identidad para esta skin sintética).
    let skin = skin_simple();
    let mats = evaluate_skin_matrices_blended(&skin, None, 0.0, None, 0.0, 0.5);
    let identidad = glass_core::math::mat4_identity();
    for i in 0..16 {
        assert!((mats[0][i] - identidad[i]).abs() < 1e-5, "elemento {i}");
    }
}

}
