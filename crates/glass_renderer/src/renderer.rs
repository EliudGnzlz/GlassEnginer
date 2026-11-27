use crate::context::VulkanContext;
use crate::mesh::GpuMesh;
use crate::offscreen::OffscreenTarget;
use crate::pipeline::{GraphicsPipeline, MeshPipelineLayouts};
use crate::swapchain::SwapchainContext;
use crate::ui::{FontAtlas, UiAtlasKind, UiRenderer, UiSegment};
use ash::vk;
use gpu_allocator::vulkan::{
    Allocation, AllocationCreateDesc, AllocationScheme, Allocator, AllocatorCreateDesc,
};
use gpu_allocator::MemoryLocation;
use std::sync::{Arc, Mutex};
use thiserror::Error;

const VERT_SPV: &[u8] = include_bytes!("../../../assets/shaders/triangle.vert.spv");
const FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/triangle.frag.spv");
const MESH_VERT_SPV: &[u8] = include_bytes!("../../../assets/shaders/mesh.vert.spv");
const MESH_FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/mesh.frag.spv");
const PBR_VERT_SPV: &[u8] = include_bytes!("../../../assets/shaders/pbr.vert.spv");
/// Fragmento de recorte alfa para la profundidad de grafos con la
/// propiedad especial `alpha_clip` (Fase 2S).
const DEPTH_CLIP_FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/depth_clip.frag.spv");
const OUTLINE_VERT_SPV: &[u8] = include_bytes!("../../../assets/shaders/outline.vert.spv");
const OUTLINE_FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/outline.frag.spv");
/// Vértice del contorno de selección con piel (casco invertido deformado).
const OUTLINE_SKINNED_VERT_SPV: &[u8] =
    include_bytes!("../../../assets/shaders/outline_skinned.vert.spv");
const PORTAL_VERT_SPV: &[u8] = include_bytes!("../../../assets/shaders/portal.vert.spv");
const PORTAL_FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/portal.frag.spv");
const PARTICLE_VERT_SPV: &[u8] = include_bytes!("../../../assets/shaders/particles.vert.spv");
const PARTICLE_FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/particles.frag.spv");
const EDITOR_LINE_VERT_SPV: &[u8] = include_bytes!("../../../assets/shaders/editor_line.vert.spv");
const EDITOR_LINE_FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/editor_line.frag.spv");
const PBR_FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/pbr.frag.spv");
const PBR_SKINNED_VERT_SPV: &[u8] =
    include_bytes!("../../../assets/shaders/pbr_skinned.vert.spv");
/// Gemelo del PBR con el ACES dentro: el camino SIN post-procesado
/// (GLASS_POST=0), bit a bit el comportamiento anterior al bloque de
/// gráficos. El `pbr.frag` normal escribe lineal HDR y el ACES vive en
/// la pasada final del post.
const PBR_LEGACY_FRAG_SPV: &[u8] =
    include_bytes!("../../../assets/shaders/pbr_legacy.frag.spv");
const PBR_INSTANCED_VERT_SPV: &[u8] = include_bytes!("../../../assets/shaders/pbr_instanced.vert.spv");
const PBR_INSTANCED_FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/pbr_instanced.frag.spv");
const PBR_INSTANCED_LEGACY_FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/pbr_instanced_legacy.frag.spv");

/// Listas de dibujo de un frame para `render_views`: las mallas,
/// pieles y portales de las vistas de escena y de juego, los
/// proyectores de sombra y las matrices de partículas. (Agrupadas
/// por el lint too_many_arguments de Clippy.)
pub struct FrameDraws<'a> {
    pub scene_meshes: &'a [MeshDrawItem<'a>],
    pub game_meshes: Option<&'a [MeshDrawItem<'a>]>,
    pub scene_skinned: &'a [SkinnedDrawItem<'a>],
    pub game_skinned: Option<&'a [SkinnedDrawItem<'a>]>,
    /// Contorno de selección con piel de la vista de Escena (vacío si
    /// no hay selección con piel o sin editor).
    pub scene_outline_skinned: &'a [SkinnedDrawItem<'a>],
    pub shadow_casters: Option<&'a ShadowCasters<'a>>,
    pub scene_portal_draws: &'a [PortalDrawItem<'a>],
    pub game_portal_draws: Option<&'a [PortalDrawItem<'a>]>,
    pub particle_vps: Option<([f32; 16], [f32; 16])>,
}

/// Proyector estático del pase de sombras: (malla, modelo, rangos).
pub type StaticCaster<'a> = (&'a GpuMesh, [f32; 16], Option<Vec<(u32, u32)>>);

/// Proyector de grafo del pase de sombras: (malla, modelo, nombre
/// del grafo, set de texturas, umbral alfa, rangos).
pub type GraphCaster<'a> = (
    &'a GpuMesh,
    [f32; 16],
    String,
    vk::DescriptorSet,
    Option<f32>,
    Option<Vec<(u32, u32)>>,
);

/// Proyectores del pase de sombras de un frame: mallas rígidas (malla +
/// matriz de modelo) y mallas con piel (malla + modelo + set de
/// articulaciones de la entidad). La MVP de luz la compone el renderer
/// con la VP fijada por `set_shadow_frame`.
pub struct ShadowCasters<'a> {
    /// (malla, modelo, rangos a dibujar): `Some(rangos)` limita el
    /// pase a esos (first_index, index_count) — el nivel LOD activo
    /// (auditoría C3); `None` dibuja la malla entera.
    pub statics: &'a [StaticCaster<'a>],
    pub skinned: &'a [(&'a GpuMesh, [f32; 16], vk::DescriptorSet)],
    /// Proyectores cuyo material es un grafo con pase Shadow (Fase 2S):
    /// (malla, matriz de modelo, nombre del grafo, set de texturas del
    /// grafo, umbral de recorte alfa si el grafo declara `alpha_clip`).
    pub graph_statics: &'a [GraphCaster<'a>],
}

/// Qué vista procesa `render_post` (la de escena o la de juego).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PostView {
    Scene,
    Game,
}

/// Lote de dibujo de una malla CON piel, para una primitiva: malla (con
/// vértices `SkinnedVertex`), push constants como en el lote normal, set 0
/// con la textura del material y set 1 con el SSBO de articulaciones DE
/// ESA ENTIDAD (cada instancia animada tiene su propia pose), y el rango
/// de índices de la primitiva.
pub type SkinnedDrawItem<'a> = (
    &'a GpuMesh,
    MeshPushConstants,
    vk::DescriptorSet,
    vk::DescriptorSet,
    (u32, u32),
    // Ranura de luces del objeto; 0 = global.
    u32,
);

#[derive(Error, Debug)]
pub enum RendererError {
    #[error("Vulkan driver error: {0}")]
    Driver(#[from] vk::Result),
    #[error("Swapchain error: {0}")]
    Swapchain(#[from] crate::swapchain::SwapchainError),
    #[error("Offscreen error: {0}")]
    Offscreen(#[from] crate::offscreen::OffscreenError),
    #[error("Pipeline error: {0}")]
    Pipeline(#[from] crate::pipeline::PipelineError),
    #[error("GPU allocator initialization error: {0}")]
    Allocator(#[from] gpu_allocator::AllocationError),
    #[error("Descriptor error: {0}")]
    Descriptor(String),
}

/// Constantes empujadas por malla al shader.
///
/// Tamaño total: 208 bytes. El mínimo garantizado por Vulkan es 128 bytes, pero
/// toda GPU de escritorio expone 256; `context.rs` comprueba
/// `maxPushConstantsSize` al crear el dispositivo y aborta con un mensaje claro
/// si no alcanza, en lugar de fallar de forma silenciosa.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshPushConstants {
    /// Proyección * Vista * Modelo. Usado sólo para la posición en clip space.
    pub mvp: [f32; 16],
    /// Matriz de modelo. Necesaria aparte de `mvp` porque el fragment shader
    /// requiere la posición en espacio de mundo para el vector de vista real, y
    /// el vertex shader necesita transformar la normal.
    pub model: [f32; 16],
    pub light_dir: [f32; 4],       // xyz = dirección hacia la luz, w = ambiente
    pub light_color: [f32; 4],     // xyz = color * intensidad, w = reservado (sin uso)
    pub material_params: [f32; 4], // x=metallic, y=roughness, z=ao, w=has_light (fuente única)
    pub object_color: [f32; 4],    // rgb = albedo, a = opacidad
    pub camera_pos: [f32; 4],      // xyz = posición de la cámara en mundo
    pub lm_params: [f32; 4],       // x = índice en el UBO del lightmap (-1 = sin bake), y = tamaño del atlas px
    pub emissive: [f32; 4],        // rgb = color emisivo, w = intensidad (Materiales v1; 0 = sin emisión)
    pub uv_transform: [f32; 4],    // xy = tiling, zw = offset de la UV de la textura base
}

/// Tamaño en bytes que el `pipeline_layout` debe declarar y que
/// `context.rs` valida contra `maxPushConstantsSize`.
pub const MESH_PUSH_CONSTANTS_SIZE: u32 = std::mem::size_of::<MeshPushConstants>() as u32;

/// Vértice de partícula ya expandido por la CPU (Fase 8P): la
/// posición es la esquina del billboard en espacio de mundo.
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct ParticleVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
}

/// Buffers dinámicos de partículas de UNA vista: se reescriben
/// cada frame por memoria mapeada y crecen por duplicación.
pub struct ParticleBuffers {
    pub vertex_buffer: vk::Buffer,
    pub vertex_allocation: Option<Allocation>,
    pub index_buffer: vk::Buffer,
    pub index_allocation: Option<Allocation>,
    pub index_count: u32,
    pub vertex_capacity: usize,
    pub index_capacity: usize,
}

impl Default for ParticleBuffers {
    fn default() -> Self {
        Self {
            vertex_buffer: vk::Buffer::null(),
            vertex_allocation: None,
            index_buffer: vk::Buffer::null(),
            index_allocation: None,
            index_count: 0,
            vertex_capacity: 0,
            index_capacity: 0,
        }
    }
}


/// Datos POR INSTANCIA del render instanciado (96 B): lo que en el camino
/// de un draw por ítem viaja en los push constants y cambia de una entidad
/// a otra. Se lee en pbr_instanced.vert desde el binding 1 (rate INSTANCE).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceData {
    /// Matriz de modelo (columnas, locations 3..6).
    pub model: [f32; 16],
    /// rgb = albedo, a = opacidad (location 7).
    pub object_color: [f32; 4],
    /// x = índice en el UBO del lightmap (-1 = sin bake), y = tamaño del atlas px (location 8).
    pub lm_params: [f32; 4],
}

// Contrato con el vertex input de `new_mesh_pipeline_instanced`: sin
// relleno y con los offsets que declaran los atributos.
const _: () = {
    assert!(std::mem::size_of::<InstanceData>() == 96);
    assert!(std::mem::offset_of!(InstanceData, model) == 0);
    assert!(std::mem::offset_of!(InstanceData, object_color) == 64);
    assert!(std::mem::offset_of!(InstanceData, lm_params) == 80);
};

/// Push constants del camino instanciado (160 B): solo los campos por
/// vista y material. Se empujan desde el offset 0 dentro del rango de
/// 256 B que declara el layout de siempre.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstancedPushConstants {
    /// Proyección * Vista (el modelo va por instancia).
    pub vp: [f32; 16],
    pub light_dir: [f32; 4],
    pub light_color: [f32; 4],
    pub material_params: [f32; 4],
    pub camera_pos: [f32; 4],
    pub emissive: [f32; 4],
    pub uv_transform: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<InstancedPushConstants>() == 160);

impl InstancedPushConstants {
    /// Construye los push constants del grupo a partir de la VP de la
    /// vista y de un ítem representativo (copia sus campos por vista y
    /// material).
    pub fn from_item(vp: [f32; 16], item: &MeshPushConstants) -> Self {
        Self {
            vp,
            light_dir: item.light_dir,
            light_color: item.light_color,
            material_params: item.material_params,
            camera_pos: item.camera_pos,
            emissive: item.emissive,
            uv_transform: item.uv_transform,
        }
    }
}

/// Buffer dinámico de instancias de UNA vista: se reescribe cada frame
/// por memoria mapeada y crece por duplicación, como el de partículas.
pub struct InstanceBuffers {
    pub buffer: vk::Buffer,
    pub allocation: Option<Allocation>,
    pub capacity: usize,
}

impl Default for InstanceBuffers {
    fn default() -> Self {
        Self {
            buffer: vk::Buffer::null(),
            allocation: None,
            capacity: 0,
        }
    }
}

/// Vértice de las líneas del editor (rejilla): posición en mundo
/// y color RGBA (el alfa trae el desvanecido por distancia).
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct EditorLineVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

/// Buffer dinámico de las líneas del editor: sin índices
/// (LINE_LIST), se reescribe cada frame por memoria mapeada
/// y crece por duplicación, como el de partículas.
pub struct EditorLineBuffers {
    pub vertex_buffer: vk::Buffer,
    pub vertex_allocation: Option<Allocation>,
    pub vertex_count: u32,
    pub vertex_capacity: usize,
}

impl Default for EditorLineBuffers {
    fn default() -> Self {
        Self {
            vertex_buffer: vk::Buffer::null(),
            vertex_allocation: None,
            vertex_count: 0,
            vertex_capacity: 0,
        }
    }
}

/// Push constants de la superficie de un portal (144 B):
/// el MVP de la vista que la dibuja, el MVP de la CÁMARA
/// VIRTUAL del portal enlazado (para el muestreo proyectivo
/// de su textura) y el color del anillo.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PortalPushConstants {
    pub mvp: [f32; 16],
    pub mvp_virtual: [f32; 16],
    pub rim: [f32; 4],
    /// Mitad local de la superficie en X e Y (el cubo glb
    /// abarca ±1, el plano integrado ±0,5): el anillo del
    /// borde se calcula desde la posición local normalizada
    /// por esto, NO desde el UV (el cubo trae UV de atlas y
    /// el anillo salía como una banda vertical).
    pub local_half: [f32; 2],
    pub _pad: [f32; 2],
}

/// Un dibujo de superficie de portal dentro de una vista:
/// la malla (el plano del portal), sus dos MVP y qué textura
/// de portal muestrea (0 = vista del portal A, 1 = la del B).
pub struct PortalDrawItem<'a> {
    pub mesh: &'a GpuMesh,
    pub push: PortalPushConstants,
    pub texture_index: usize,
    pub range: (u32, u32),
}

/// El lote de mallas de una vista (alias del tipo que ya
/// viajaba por las firmas de render).
pub type MeshDrawItem<'a> = (
    &'a GpuMesh,
    MeshPushConstants,
    vk::DescriptorSet,
    Option<String>,
    (u32, u32),
    Option<vk::DescriptorSet>,
    // Ranura de luces del objeto; 0 = global.
    u32,
);

const IDENTITY_MAT4: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
];

impl Default for MeshPushConstants {
    fn default() -> Self {
        Self {
            mvp: IDENTITY_MAT4,
            model: IDENTITY_MAT4,
            light_dir: [0.5, 1.0, 0.7, 1.0],
            light_color: [1.0, 1.0, 1.0, 0.2],
            material_params: [0.2, 0.5, 1.0, 1.0],
            object_color: [0.18, 0.55, 0.92, 1.0],
            camera_pos: [0.0, 0.0, 0.0, 1.0],
            lm_params: [-1.0, 0.0, 0.0, 0.0],
            emissive: [0.0, 0.0, 0.0, 0.0],
            uv_transform: [1.0, 1.0, 0.0, 0.0],
        }
    }
}

impl MeshPushConstants {
    pub fn new(mvp: [f32; 16]) -> Self {
        Self {
            mvp,
            ..Default::default()
        }
    }
}

impl MeshPushConstants {
    /// Extrae los campos por entidad para el buffer de instancias.
    pub fn instance_data(&self) -> InstanceData {
        InstanceData {
            model: self.model,
            object_color: self.object_color,
            lm_params: self.lm_params,
        }
    }
}

// El contrato con GLSL depende de que no haya relleno entre campos: si el
// compilador insertase padding, los offsets dejarían de coincidir y el shader
// leería basura sin ningún error de compilación.
const _: () = assert!(std::mem::size_of::<MeshPushConstants>() == 256);
const _: () = assert!(std::mem::align_of::<MeshPushConstants>() == 4);

pub const MAX_POINT_LIGHTS: usize = 8;
pub const POINT_LIGHTS_UBO_SIZE: u64 = 656;
/// Ranuras del buffer de luces: la 0 es el UBO global y las demás
/// son los UBO por objeto de la asignación de luces por objeto.
pub const POINT_LIGHT_SLOTS: u64 = 1024;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PointLightsUbo {
    pub pos_range: [[f32; 4]; MAX_POINT_LIGHTS],
    pub color_intensity: [[f32; 4]; MAX_POINT_LIGHTS],
    /// Sombra por luz (Fase 2): x = índice de cubo en el atlas o -1 sin
    /// sombra; y = plano lejano (el rango); z = bias en profundidad
    /// NDC; w = plano cercano de la proyección del cubo.
    pub shadow: [[f32; 4]; MAX_POINT_LIGHTS],
    /// Cono Spot (integración de iluminación): xyz = dirección de
    /// mundo normalizada, w = coseno del ángulo exterior. En Point
    /// queda a cero y no se usa.
    pub dir_cos: [[f32; 4]; MAX_POINT_LIGHTS],
    /// Cono Spot: x = coseno del ángulo interior, y = 1.0 si la luz
    /// es Spot (0.0 si es Point), zw sin uso.
    pub cone: [[f32; 4]; MAX_POINT_LIGHTS],
    pub count: u32,
    pub _pad: [u32; 3],
}

const _: () = assert!(std::mem::size_of::<PointLightsUbo>() == 656);

impl Default for PointLightsUbo {
    fn default() -> Self {
        Self {
            pos_range: [[0.0; 4]; MAX_POINT_LIGHTS],
            shadow: [[-1.0, 0.0, 0.0, 0.0]; MAX_POINT_LIGHTS],
            color_intensity: [[0.0; 4]; MAX_POINT_LIGHTS],
            dir_cos: [[0.0; 4]; MAX_POINT_LIGHTS],
            cone: [[0.0; 4]; MAX_POINT_LIGHTS],
            count: 0,
            _pad: [0; 3],
        }
    }
}

/// Una luz puntual con sombra para los pases de cubos (Fase 2): la app
/// publica hasta 4 por frame con set_point_shadow_lights, en el MISMO
/// orden en que asignó los índices de cubo del PointLightsUbo.
#[derive(Debug, Clone, Copy)]
pub struct PointShadowLight {
    pub pos: [f32; 3],
    /// Plano lejano de la proyección del cubo (el rango de la luz).
    pub far: f32,
}

/// UBO del lightmap (set 5 del PBR): por entidad horneada (máx. 64),
/// los 6 grupos (eje, signo) con su sub-rect en el atlas (rect, en
/// UV) y sus cotas 2D de mundo (bounds: mínimo y extensión). Ranura =
/// entidad*6 + eje*2 + signo. Grupo ausente: todo ceros.
pub const MAX_LM_ENTITIES: usize = 64;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LmDataUbo {
    pub rect: [[f32; 4]; MAX_LM_ENTITIES * 6],
    pub bounds: [[f32; 4]; MAX_LM_ENTITIES * 6],
}

const _: () = assert!(std::mem::size_of::<LmDataUbo>() == 12288);

impl Default for LmDataUbo {
    fn default() -> Self {
        Self {
            rect: [[0.0; 4]; MAX_LM_ENTITIES * 6],
            bounds: [[0.0; 4]; MAX_LM_ENTITIES * 6],
        }
    }
}

fn create_point_lights_set_layout(
    device: &ash::Device,
) -> Result<vk::DescriptorSetLayout, vk::Result> {
    let binding = [vk::DescriptorSetLayoutBinding::default()
        .binding(0)
        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
    let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&binding);
    unsafe { device.create_descriptor_set_layout(&info, None) }
}

/// Layout del set 4 del lightmap: el atlas como textura muestreable.
fn create_lm_tex_set_layout(
    device: &ash::Device,
) -> Result<vk::DescriptorSetLayout, vk::Result> {
    let binding = [vk::DescriptorSetLayoutBinding::default()
        .binding(0)
        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
    let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&binding);
    unsafe { device.create_descriptor_set_layout(&info, None) }
}

/// Layout del set 5 del lightmap: el UBO de grupos por entidad.
fn create_lm_data_set_layout(
    device: &ash::Device,
) -> Result<vk::DescriptorSetLayout, vk::Result> {
    let binding = [vk::DescriptorSetLayoutBinding::default()
        .binding(0)
        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
    let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&binding);
    unsafe { device.create_descriptor_set_layout(&info, None) }
}

/// UBO del set 6 (IBL): parámetros del entorno en un vec4 —
/// x: intensidad, y: mip máximo del cubemap, z: 1.0 si hay entorno
/// válido (si es 0, el shader conserva el ambiente hemisférico).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct IblParamsUbo {
    pub params: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<IblParamsUbo>() == 16);

impl Default for IblParamsUbo {
    fn default() -> Self {
        Self { params: [0.0; 4] }
    }
}

/// Layout del set 6 (IBL): cubemap de entorno + UBO de parámetros.
fn create_ibl_set_layout(device: &ash::Device) -> Result<vk::DescriptorSetLayout, vk::Result> {
    let bindings = [
        vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        vk::DescriptorSetLayoutBinding::default()
            .binding(1)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
    ];
    let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    unsafe { device.create_descriptor_set_layout(&info, None) }
}

/// Crea y sube un cubemap RGBA16F con mips precocinados en CPU.
/// `data` viene empaquetado nivel a nivel: por cada nivel, las 6 caras
/// (+X, -X, +Y, -Y, +Z, -Z) de (size>>nivel)² téxeles de 8 bytes.
pub fn create_ibl_cube(
    device: &ash::Device,
    allocator: &mut Allocator,
    queue: vk::Queue,
    command_pool: vk::CommandPool,
    size: u32,
    mip_levels: u32,
    data: &[u8],
) -> Result<IblCubeTexture, String> {
    unsafe {
        let (staging_buffer, staging_alloc) = crate::mesh::MeshLoader::create_buffer(
            device,
            allocator,
            data.len() as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
            "ibl_staging",
        )?;
        let mapped = staging_alloc
            .mapped_ptr()
            .ok_or_else(|| "Fallo al mapear staging IBL".to_string())?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), mapped.as_ptr() as *mut u8, data.len());

        let image_info = vk::ImageCreateInfo::default()
            .flags(vk::ImageCreateFlags::CUBE_COMPATIBLE)
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::R16G16B16A16_SFLOAT)
            .extent(vk::Extent3D {
                width: size,
                height: size,
                depth: 1,
            })
            .mip_levels(mip_levels)
            .array_layers(6)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let image = device
            .create_image(&image_info, None)
            .map_err(|e| format!("Fallo al crear imagen IBL: {}", e))?;

        let requirements = device.get_image_memory_requirements(image);
        let image_alloc = allocator
            .allocate(&AllocationCreateDesc {
                name: "ibl_cube",
                requirements,
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|e| format!("Fallo al asignar memoria IBL: {}", e))?;
        device
            .bind_image_memory(image, image_alloc.memory(), image_alloc.offset())
            .map_err(|e| format!("Fallo al vincular memoria IBL: {}", e))?;

        let cmd = device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
            .map_err(|e| format!("Fallo al asignar command buffer IBL: {}", e))?[0];

        device
            .begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .map_err(|e| format!("Error al iniciar command buffer IBL: {}", e))?;

        let full_range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .base_mip_level(0)
            .level_count(mip_levels)
            .base_array_layer(0)
            .layer_count(6);

        let to_transfer = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(full_range)
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[to_transfer],
        );

        let mut regions = Vec::with_capacity((mip_levels * 6) as usize);
        let mut offset = 0u64;
        for mip in 0..mip_levels {
            let lado = (size >> mip).max(1);
            for cara in 0..6 {
                regions.push(
                    vk::BufferImageCopy::default()
                        .buffer_offset(offset)
                        .buffer_row_length(0)
                        .buffer_image_height(0)
                        .image_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .mip_level(mip)
                                .base_array_layer(cara)
                                .layer_count(1),
                        )
                        .image_offset(vk::Offset3D::default().x(0).y(0).z(0))
                        .image_extent(vk::Extent3D {
                            width: lado,
                            height: lado,
                            depth: 1,
                        }),
                );
                offset += (lado as u64) * (lado as u64) * 8;
            }
        }
        device.cmd_copy_buffer_to_image(
            cmd,
            staging_buffer,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &regions,
        );

        let to_shader = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(full_range)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ);
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[to_shader],
        );

        device
            .end_command_buffer(cmd)
            .map_err(|e| format!("Error al finalizar command buffer IBL: {}", e))?;
        let fence = device
            .create_fence(&vk::FenceCreateInfo::default(), None)
            .map_err(|e| format!("Fallo al crear fence IBL: {}", e))?;
        let cmds = [cmd];
        device
            .queue_submit(
                queue,
                &[vk::SubmitInfo::default().command_buffers(&cmds)],
                fence,
            )
            .map_err(|e| format!("Error en queue_submit IBL: {}", e))?;
        device
            .wait_for_fences(&[fence], true, u64::MAX)
            .map_err(|e| format!("Error esperando fence IBL: {}", e))?;
        device.destroy_fence(fence, None);
        device.free_command_buffers(command_pool, &cmds);
        allocator
            .free(staging_alloc)
            .map_err(|e| format!("Fallo al liberar staging IBL: {}", e))?;
        device.destroy_buffer(staging_buffer, None);

        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::CUBE)
            .format(vk::Format::R16G16B16A16_SFLOAT)
            .subresource_range(full_range);
        let image_view = device
            .create_image_view(&view_info, None)
            .map_err(|e| format!("Fallo al crear vista IBL: {}", e))?;

        Ok(IblCubeTexture {
            image,
            allocation: Some(image_alloc),
            image_view,
        })
    }
}

/// Un cubemap de entorno vivo (IBL): imagen cúbica R16F con su cadena
/// de mips precocinada en CPU y su vista TYPE_CUBE.
pub struct IblCubeTexture {
    pub image: vk::Image,
    pub allocation: Option<Allocation>,
    pub image_view: vk::ImageView,
}

impl IblCubeTexture {
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

/// Una entrada viva del `graph_props_pool`: el set asignado, el buffer
/// CpuToGpu que respalda su binding 1 y el puntero mapeado para que
/// `update_graph_props_set` pueda volcar datos sin reabrir el mapeo.
pub struct GraphPropsEntry {
    pub set: vk::DescriptorSet,
    pub buffer: vk::Buffer,
    #[allow(dead_code)]
    pub allocation: Option<Allocation>,
    pub mapped: *mut u8,
    pub size: u64,
}

pub struct GlassRenderer {
    pub device: Arc<ash::Device>,
    pub allocator: Arc<Mutex<Allocator>>,
    pub swapchain: SwapchainContext,
    pub swapchain_render_pass: vk::RenderPass,
    pub swapchain_framebuffers: Vec<vk::Framebuffer>,
    pub offscreen: OffscreenTarget,
    pub game_offscreen: OffscreenTarget,
    /// Objetivos LDR de presentación (bloque de gráficos): con el post
    /// activo, los objetivos de arriba son HDR (lineal, R16G16B16A16_
    /// SFLOAT) y la pasada final escribe aquí; la UI 2D, el blit del
    /// standalone y egui consumen ESTOS, con el formato y el layout de
    /// siempre. Sin post, el 3D se dibuja directamente aquí con los
    /// pipelines `*_legacy` y la cadena no se ejecuta.
    pub final_offscreen: OffscreenTarget,
    pub final_game_offscreen: OffscreenTarget,
    /// Objetivos HDR de las vistas de portal (bloque Portales):
    /// la escena renderizada desde las cámaras virtuales de
    /// cada portal de la pareja. Las superficies de portal los
    /// muestrean con `portal_sets` (set 4, layout del lightmap).
    pub portal_targets: [OffscreenTarget; 2],
    /// Sets que muestrean `portal_targets` (uno por objetivo).
    pub portal_sets: [vk::DescriptorSet; 2],
    /// Pipelines de la superficie de portal (HDR y legacy).
    pub portal_pipeline: GraphicsPipeline,
    pub portal_pipeline_legacy: GraphicsPipeline,
    pub pipeline: GraphicsPipeline,
    pub mesh_pipeline: GraphicsPipeline,
    pub pbr_pipeline: GraphicsPipeline,
    /// Gemelos instanciados del PBR opaco (HDR y legacy): mismos layouts
    /// que `pbr_pipeline`, con el binding 1 por instancia.
    pub pbr_pipeline_instanced: GraphicsPipeline,
    pub pbr_pipeline_instanced_legacy: GraphicsPipeline,
    /// Pipeline de mallas con piel (bloque de skinning): ver
    /// `pipeline::GraphicsPipeline::new_skinned_pipeline`.
    pub skinned_pipeline: GraphicsPipeline,
    /// Gemelos de los pipelines de arriba contra el render pass LDR y,
    /// en el PBR/piel, con el fragmento `pbr_legacy.frag` (ACES dentro):
    /// el camino sin post, bit a bit el de antes del bloque.
    pub triangle_pipeline_legacy: GraphicsPipeline,
    /// Pipeline de partículas (Fase 8P) y su gemelo legacy.
    pub particle_pipeline: GraphicsPipeline,
    pub particle_pipeline_legacy: GraphicsPipeline,
    /// Geometría de partículas del frame, por vista.
    pub particle_scene: ParticleBuffers,
    pub particle_game: ParticleBuffers,
    /// Buffers de instancias por vista (escena / juego).
    pub instance_scene: InstanceBuffers,
    pub instance_game: InstanceBuffers,
    /// Pipeline de líneas del editor (rejilla) y su gemelo legacy,
    /// con el buffer de vértices del frame y su view-proj.
    pub editor_line_pipeline: GraphicsPipeline,
    pub editor_line_pipeline_legacy: GraphicsPipeline,
    pub editor_line_buffers: EditorLineBuffers,
    pub editor_grid_vp: [f32; 16],
    /// Contorno de selección de la vista de Escena (casco invertido de
    /// la malla, sin escritura de profundidad), sin y con piel, cada
    /// uno con su gemelo legacy (render pass LDR).
    pub selection_outline_pipeline: GraphicsPipeline,
    pub selection_outline_pipeline_legacy: GraphicsPipeline,
    pub selection_outline_skinned_pipeline: GraphicsPipeline,
    pub selection_outline_skinned_pipeline_legacy: GraphicsPipeline,
    /// Draws de partículas grabados en el último render_views.
    pub particle_draws: std::cell::Cell<u32>,
    pub mesh_pipeline_legacy: GraphicsPipeline,
    pub pbr_pipeline_legacy: GraphicsPipeline,
    pub skinned_pipeline_legacy: GraphicsPipeline,
    /// Recursos del mapa de sombras del sol (módulo `shadow`).
    pub shadow: crate::shadow::ShadowResources,
    /// Cadena de post-procesado (módulo `post`).
    pub post: crate::post::PostChain,
    /// ¿Post activo? Se lee de `GLASS_POST` al crear el renderer
    /// (defecto: activo). Sin post, `render_post` no hace nada y
    /// `render_views` dibuja en los objetivos LDR con los pipelines
    /// legacy.
    pub post_enabled: bool,
    /// ¿La presentación (blit al swapchain y overlay del Canvas)
    /// usa la vista de JUEGO en vez de la de escena? Solo lo activa
    /// el binario con editor en modo standalone: allí la app es un
    /// juego, no el editor, y presentar la vista de escena dejaba
    /// fuera el FOV de la cámara de juego (la mira no se veía). En
    /// el binario puro no hace falta: su «vista de escena» ya es la
    /// de juego (vp_scene == vp_game).
    pub present_game: bool,
    /// Parámetros del post (umbral/intensidad del bloom, viñeta,
    /// exposición), con overrides de entorno al crear el renderer.
    pub post_settings: crate::post::PostSettings,
    /// ¿Sombras activas? De `GLASS_SHADOWS` (defecto: activas). Solo
    /// apaga el efecto: el pase se salta y el UBO se publica inactivo.
    pub shadows_enabled: bool,
    /// VP de la luz del frame en curso (lado CPU; la vuelca
    /// `render_views` al UBO del set 2 tras el fence).
    shadow_vp: [f32; 16],
    /// Parámetros del UBO de sombras del frame (lado CPU).
    shadow_params: [f32; 4],
    /// Niebla del frame (Fase 2): viaja en el UBO de frame junto a la
    /// sombra; la publica la app con set_fog antes de render_views.
    fog_color_dist: [f32; 4],
    fog_far_mode: [f32; 4],
    /// Ambiente de escena del frame (G1): colores lineales del
    /// gradient; los publica la app con set_ambient. Arrancan en los
    /// valores heredados para que sin app el render sea el de siempre.
    ambient_sky: [f32; 4],
    ambient_equator: [f32; 4],
    ambient_ground: [f32; 4],
    /// Atlas de cubos de las sombras puntuales (Fase 2).
    point_shadows: crate::shadow::PointShadowResources,
    /// Luces con sombra del frame, en orden de ranura del UBO.
    point_shadow_lights: Vec<PointShadowLight>,
    /// Pool de sets del set 1 (SSBO de articulaciones) de los
    /// `JointBuffer` por entidad. Los sets se liberan con
    /// FREE_DESCRIPTOR_SET cuando el buffer de una entidad se destruye.
    pub joints_pool: vk::DescriptorPool,
    /// Pipelines de materiales de grafo, por nombre (`graph:<nombre>` en el
    /// `MeshData.shader` de la entidad). Comparten vertex shader y layout
    /// con el PBR; solo cambia el fragmento generado por el grafo.
    pub custom_pipelines: std::collections::HashMap<String, GraphicsPipeline>,
    /// Gemelos de los pipelines de grafo contra el render pass LDR
    /// (mismos SPV; los grafos no mapean tono en ninguna de las dos
    /// rutas). Solo se usan con el post desactivado.
    pub custom_pipelines_legacy: std::collections::HashMap<String, GraphicsPipeline>,
    /// Canal global de los grafos (bloques «punto de interacción» y
    /// «huella con forma»): layout del set 1 (`GraphGlobals`, dos vec4
    /// UNIFORM_BUFFER leídos en vértices y fragmento) que comparten
    /// TODOS los pipelines de grafo y solo ellos — el PBR, las mallas y
    /// la piel conservan sus layouts de siempre. El Renderer posee el
    /// layout, el pool, el buffer y el set; el valor CPU vive en
    /// `graph_globals` y se vuelca al buffer al principio de
    /// `render_views`, tras la espera del fence (el mismo patrón de
    /// sincronización que los `JointBuffer`: un frame en vuelo).
    pub graph_globals_layout: vk::DescriptorSetLayout,
    /// Pipelines de profundidad por grafo con pase Shadow (Fase 2S):
    /// la etapa de vértices del grafo contra el render pass de sombras
    /// del sol. `graph_depth_clip_pipelines` es la variante con el
    /// fragmento de recorte alfa (grafos con propiedad `alpha_clip`).
    pub graph_depth_pipelines: std::collections::HashMap<String, (vk::Pipeline, vk::PipelineLayout)>,
    pub graph_depth_clip_pipelines: std::collections::HashMap<String, (vk::Pipeline, vk::PipelineLayout)>,
    pub graph_outline_pipelines: std::collections::HashMap<String, GraphicsPipeline>,
    pub graph_outline_pipelines_legacy: std::collections::HashMap<String, GraphicsPipeline>,
    /// Contadores de enlaces del frame (Fase 2S, S6): se reinician al
    /// abrir `render_views` y los acumula el bucle de mallas de cada
    /// pase de vista.
    pub frame_pipeline_binds: std::cell::Cell<u32>,
    pub frame_descriptor_binds: std::cell::Cell<u32>,
    /// Draws indexados REALES del bucle de mallas en las vistas de
    /// escena y juego (render instanciado): un ítem suelto cuenta 1 y
    /// un grupo instanciado cuenta 1, así que este contador baja de N
    /// a ~grupos cuando la agrupación actúa. Lo resetea render_views.
    pub frame_draw_calls: std::cell::Cell<u32>,
    /// Pool del que sale el único set global (no se libera nunca).
    pub graph_globals_pool: vk::DescriptorPool,
    /// El set global ya enlazado al buffer; se enlaza en el índice 1 en
    /// cada dibujo con pipeline de grafo.
    pub graph_globals_set: vk::DescriptorSet,
    /// Buffer de los vec4 globales (32 B, CpuToGpu, mapeado
    /// persistentemente).
    pub graph_globals_buffer: vk::Buffer,
    #[allow(dead_code)]
    graph_globals_allocation: Option<Allocation>,
    graph_globals_mapped: *mut u8,
    /// Buffer de respaldo del binding 1 (propiedades de grafo) para las
    /// mallas que dibujan con el set global en vez de un set de
    /// instancia propio: 32 vec4 a cero, el tope de propiedades que
    /// `GraphPropsEntry` puede declarar. Sin este buffer el binding 1
    /// del set global quedaría sin escribir y la validación de Vulkan
    /// fallaría en cuanto el layout exige dos bindings.
    pub graph_props_dummy_buffer: vk::Buffer,
    #[allow(dead_code)]
    graph_props_dummy_allocation: Option<Allocation>,
    #[allow(dead_code)]
    graph_props_dummy_mapped: *mut u8,
    /// Pool de sets de propiedades por instancia (CL-2S2a): cada malla
    /// que publica sus propias propiedades de grafo (distintas del
    /// canal global) se lleva un set de aquí en vez de compartir
    /// `graph_globals_set`. 64 sets, 128 descriptores de UBO (2 por
    /// set, el máximo de bindings del layout).
    pub graph_props_pool: vk::DescriptorPool,
    /// Entradas vivas del pool anterior, indexadas por el
    /// `vk::DescriptorSet` que `create_graph_props_set` devolvió.
    pub graph_props_sets: Vec<GraphPropsEntry>,
    /// Valor actual del canal global, en el lado CPU: el punto de
    /// interacción (posición de mundo + radio en el w) seguido de la
    /// forma de su huella (semiejes X/Z de la caja, rotación Y en
    /// radianes y discriminador 0 = esfera / 1 = caja). Inactivo =
    /// `glass_core::shader_graph::INTERACTION_GLOBALS_INACTIVE`.
    graph_globals: [f32; 8],
    /// Set 3 de los pipelines PBR y de piel: el UBO de luces puntuales
    /// del frame (hasta 8 Point/Spot; la Spot emite con su cono).
    /// Mismo patrón de buffer mapeado CpuToGpu y de volcado
    /// por frame que el canal de grafos. El renderer posee layout, pool,
    /// buffer y set; el valor CPU vive en `point_lights` y la app lo
    /// publica con `set_point_lights` antes de `render_views`.
    pub point_lights_layout: vk::DescriptorSetLayout,
    /// Pool del que sale el único set de luces (no se libera nunca).
    pub point_lights_pool: vk::DescriptorPool,
    /// El set de luces ya enlazado al buffer; se enlaza en el índice 3
    /// en cada dibujo PBR o con piel.
    pub point_lights_set: vk::DescriptorSet,
    /// Buffer del UBO de luces (272 B, CpuToGpu, mapeado).
    pub point_lights_buffer: vk::Buffer,
    #[allow(dead_code)]
    point_lights_allocation: Option<Allocation>,
    point_lights_mapped: *mut u8,
    /// Valor actual del UBO de luces en el lado CPU.
    point_lights: PointLightsUbo,
    /// Paso entre ranuras del buffer de luces (656 alineado al
    /// `min_uniform_buffer_offset_alignment` del dispositivo).
    point_lights_stride: u64,
    /// Ranuras por objeto pendientes de volcar en `upload_frame_ubos`.
    object_lights_pending: Vec<(u32, PointLightsUbo)>,
    /// Render instanciado: contenido conocido de cada ranura de
    /// luces (hash FNV-1a de su UBO y copia para verificar) y su
    /// ranura canónica — la primera con el mismo contenido. Dos
    /// objetos con listas de luces idénticas comparten canónica y
    /// pueden agruparse aunque su ranura propia difiera.
    slot_hash: Vec<u64>,
    slot_ubo: Vec<Option<PointLightsUbo>>,
    slot_canon: Vec<u32>,
    slot_canon_dirty: bool,
    /// Sets 4 y 5 (lightmap horneado): atlas + UBO de grupos. Mismo
    /// patrón de propiedad que el set 3. Sin bake publicado, el set 4
    /// enlaza una textura 1×1 negra y el UBO está a ceros: el PBR se
    /// comporta como siempre (lm_params = -1 en cada dibujo).
    pub lm_tex_layout: vk::DescriptorSetLayout,
    pub lm_data_layout: vk::DescriptorSetLayout,
    /// Pool de los sets del lightmap (defecto, datos y atlas vigente).
    pub lm_pool: vk::DescriptorPool,
    /// Set 4 vigente: el del atlas publicado o el de la textura 1×1.
    pub lm_tex_set: vk::DescriptorSet,
    /// Set 4 de la textura 1×1 negra (al que vuelve `clear_lightmap`).
    lm_default_tex_set: vk::DescriptorSet,
    /// Set 4 reutilizable del atlas publicado: `set_lightmap` solo
    /// reescribe su enlace (el pool no admite liberar sets).
    lm_runtime_tex_set: vk::DescriptorSet,
    /// Set 5 ya enlazado al buffer de grupos.
    pub lm_data_set: vk::DescriptorSet,
    pub lm_data_buffer: vk::Buffer,
    #[allow(dead_code)]
    lm_data_allocation: Option<Allocation>,
    lm_data_mapped: *mut u8,
    /// Muestreador propio del atlas (lineal, clamp).
    lm_sampler: vk::Sampler,
    /// La textura 1×1 negra por defecto: nadie la lee por el campo, pero
    /// conservarla viva es su función (el set por defecto la muestrea).
    #[allow(dead_code)]
    lm_default_texture: Option<crate::mesh::GpuTexture>,
    /// El atlas publicado por `set_lightmap`, si lo hay.
    lm_atlas_texture: Option<crate::mesh::GpuTexture>,
    /// Entidad → índice en el UBO de grupos.
    lm_index: std::collections::HashMap<u64, u32>,
    /// Tamaño del atlas vigente (1 si no hay bake).
    lm_atlas_size: f32,
    /// Set 6 (IBL de skybox): layout, pool y set único del renderer,
    /// UBO de parámetros mapeado y cubemap vigente. Sin entorno
    /// publicado, el cubemap es el 1×1 negro y `ibl_params.z = 0`:
    /// el shader conserva el ambiente hemisférico de siempre.
    pub ibl_set_layout: vk::DescriptorSetLayout,
    ibl_pool: vk::DescriptorPool,
    ibl_set: vk::DescriptorSet,
    ibl_params_buffer: vk::Buffer,
    #[allow(dead_code)]
    ibl_params_allocation: Option<Allocation>,
    ibl_params_mapped: *mut u8,
    ibl_sampler: vk::Sampler,
    ibl_cube_texture: Option<IblCubeTexture>,
    /// Pase 2D del Canvas de juego (sin egui) para el standalone: ver
    /// `render_ui_overlay` y el módulo `ui`.
    pub ui: UiRenderer,
    /// Pase 2D del atlas de imágenes del Canvas (UI v2); None si
    /// la escena activa no usa elementos Imagen.
    pub ui_images: Option<UiRenderer>,
    /// Pases 2D de fuentes con nombre (UI v2): una UiRenderer por
    /// fuente de assets/fonts usada por la escena; se crean bajo
    /// demanda con `ensure_named_font`.
    pub ui_named_fonts: std::collections::HashMap<String, UiRenderer>,
    pub command_pool: vk::CommandPool,
    pub command_buffer: vk::CommandBuffer,
    pub fence: vk::Fence,
    pub image_available_semaphore: vk::Semaphore,
    pub render_finished_semaphores: Vec<vk::Semaphore>,
    /// Descriptor set de respaldo para los caminos que dibujan sin pasar por
    /// la AssetCache (capturas offscreen): el set de la textura blanca por
    /// defecto. La app lo fija al crear esa textura; si queda nulo, los
    /// dibujos de ese camino no enlazan set (camino de deuda ya documentado
    /// en `render_offscreen_scene`).
    pub fallback_set: vk::DescriptorSet,
    /// Pool de queries de timestamp para medir la duración del pase principal
    /// (`render_views`). VK_NULL_HANDLE si el dispositivo o la familia gráfica
    /// no admiten timestamps: la telemetría queda desactivada sin tocar el render.
    /// En esta fase SOLO se mide `render_views`; portal/post/ui quedan fuera.
    timestamp_pool: vk::QueryPool,
    /// Periodo de los timestamps en ns/tick (`limits.timestamp_period`).
    timestamp_period: f32,
    /// Última duración medida del pase principal, en microsegundos.
    gpu_render_us: Option<u64>,
    /// Hay una query escrita en un frame en vuelo pendiente de leer.
    timestamp_pending: bool,
}

fn create_swapchain_render_pass(
    device: &ash::Device,
    format: vk::Format,
) -> Result<vk::RenderPass, vk::Result> {
    let attachments = [vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)];

    let color_attachment_ref = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_attachment_ref)];

    let dependencies = [vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)];

    let create_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);

    unsafe { device.create_render_pass(&create_info, None) }
}

fn create_swapchain_framebuffers(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    views: &[vk::ImageView],
    extent: vk::Extent2D,
) -> Result<Vec<vk::Framebuffer>, vk::Result> {
    let mut framebuffers = Vec::with_capacity(views.len());
    for &view in views {
        let attachments = [view];
        let create_info = vk::FramebufferCreateInfo::default()
            .render_pass(render_pass)
            .attachments(&attachments)
            .width(extent.width)
            .height(extent.height)
            .layers(1);
        let fb = unsafe { device.create_framebuffer(&create_info, None)? };
        framebuffers.push(fb);
    }
    Ok(framebuffers)
}

/// Grupo de dibujo instanciado: varios ítems con la misma clave
/// (malla, material, rango, luces y campos por vista/material de los
/// push constants) que se dibujan con UNA `cmd_draw_indexed` usando el
/// buffer de instancias de la vista.
#[derive(Debug, Clone, Copy)]
pub struct InstancedGroup {
    /// Índice del ítem representativo en la lista de la vista: de él
    /// salen la malla, los sets y los campos por vista/material.
    pub representative: usize,
    /// Rango de índices de la malla (compartido por el grupo).
    pub range: (u32, u32),
    /// Primera instancia del grupo en el buffer de la vista.
    pub first_instance: u32,
    /// Número de instancias del grupo.
    pub count: u32,
}

/// Plan de instanciado de una vista para un frame: los grupos, qué
/// ítems quedan agrupados (esos NO se dibujan por el camino por ítem)
/// y el contenido del buffer de instancias, en orden de grupo.
pub struct ViewInstancing {
    /// VP de la vista recuperada de los propios ítems (ver
    /// `plan_instancing`).
    pub vp: [f32; 16],
    pub groups: Vec<InstancedGroup>,
    pub grouped: Vec<bool>,
    pub instances: Vec<InstanceData>,
}

/// Clave de agrupación: identidad de los buffers de la malla, sets,
/// rango de índices, ranura de luces y los bits exactos de los campos
/// por vista/material de los push constants (los que el camino
/// instanciado sigue empujando una vez por grupo). Los campos por
/// entidad (model, object_color, lm_params) NO están en la clave: son
/// los que viajan por instancia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct InstancedKey {
    vertex_buffer: u64,
    index_buffer: u64,
    tex_set: u64,
    props_set: u64,
    light_slot: u32,
    range: (u32, u32),
    uniformes: [u32; 24],
}

impl InstancedKey {
    fn from_item(item: &MeshDrawItem, canon: &[u32]) -> Self {
        use ash::vk::Handle;
        let p = &item.1;
        let mut uniformes = [0u32; 24];
        let campos: [&[f32; 4]; 6] = [
            &p.light_dir,
            &p.light_color,
            &p.material_params,
            &p.camera_pos,
            &p.emissive,
            &p.uv_transform,
        ];
        for (i, c) in campos.iter().enumerate() {
            for (j, v) in c.iter().enumerate() {
                uniformes[i * 4 + j] = v.to_bits();
            }
        }
        Self {
            vertex_buffer: item.0.vertex_buffer.as_raw(),
            index_buffer: item.0.index_buffer.as_raw(),
            tex_set: item.2.as_raw(),
            props_set: item.5.map_or(0, |s| s.as_raw()),
            // La ranura entra canonizada: objetos con la misma
            // lista de luces comparten grupo aunque su ranura
            // propia difiera (el grupo enlaza la del representante,
            // de contenido idéntico).
            light_slot: canon.get(item.6 as usize).copied().unwrap_or(item.6),
            range: item.4,
            uniformes,
        }
    }
}

/// Multiplicación de matrices 4×4 en columna mayor (la convención de
/// los push constants y de GLSL): `r = a · b`.
fn mat4_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut r = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut s = 0.0f32;
            for k in 0..4 {
                s += a[k * 4 + row] * b[col * 4 + k];
            }
            r[col * 4 + row] = s;
        }
    }
    r
}

/// Inversa de una 4×4 en columna mayor por cofactores; `None` si el
/// determinante es ~0 (modelo degenerado: escala cero o similar). Se
/// llama una vez por vista y frame (recuperar la VP), así que prima
/// la claridad sobre la velocidad: elemento (fila, col) = m[col*4+fila]
/// y la inversa es la adjunta (cofactores traspuestos) entre el det.
fn mat4_inverse(m: &[f32; 16]) -> Option<[f32; 16]> {
    let e = |fila: usize, col: usize| m[col * 4 + fila];
    // Determinante 3×3 del menor (f, c): la matriz sin esa fila/columna.
    let menor = |f: usize, c: usize| -> f32 {
        let mut v = [0.0f32; 9];
        let mut k = 0;
        for col in 0..4 {
            if col == c {
                continue;
            }
            for fila in 0..4 {
                if fila == f {
                    continue;
                }
                v[k] = e(fila, col);
                k += 1;
            }
        }
        // v está en columna mayor: det de la 3×3 por la primera fila.
        v[0] * (v[4] * v[8] - v[5] * v[7]) - v[1] * (v[3] * v[8] - v[5] * v[6])
            + v[2] * (v[3] * v[7] - v[4] * v[6])
    };
    let cofactor = |f: usize, c: usize| -> f32 {
        let signo = if (f + c).is_multiple_of(2) { 1.0 } else { -1.0 };
        signo * menor(f, c)
    };
    let det: f32 = (0..4).map(|c| e(0, c) * cofactor(0, c)).sum();
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for fila in 0..4 {
            // Adjunta: el cofactor traspuesto.
            out[col * 4 + fila] = cofactor(col, fila) * inv_det;
        }
    }
    Some(out)
}

/// Construye el plan de instanciado de una vista. Elegibles: solo los
/// ítems del PBR estándar (`shader_name == None`); los de grafo,
/// transparentes y contornos quedan fuera y se dibujan por ítem. Un
/// grupo necesita 2+ ítems con la misma clave; los grupos de 1 se
/// deshacen (dibujarlos por ítem evita el cambio de pipeline).
///
/// La VP de la vista no viaja en `MeshDrawItem` (cada ítem trae su
/// mvp = VP · model ya compuesto por la app), así que se recupera del
/// primer ítem con modelo invertible: VP = mvp · model⁻¹. Sin un
/// ítem así no hay plan y la vista entera va por ítem.
pub fn plan_instancing(meshes: &[MeshDrawItem], canon: &[u32]) -> Option<ViewInstancing> {
    let mut vp: Option<[f32; 16]> = None;
    for item in meshes {
        if item.3.is_none() {
            if let Some(inv) = mat4_inverse(&item.1.model) {
                vp = Some(mat4_mul(&item.1.mvp, &inv));
                break;
            }
        }
    }
    let vp = vp?;

    use std::collections::HashMap;
    let mut indice: HashMap<InstancedKey, usize> = HashMap::new();
    let mut grupos: Vec<Vec<usize>> = Vec::new();
    for (i, item) in meshes.iter().enumerate() {
        if item.3.is_some() {
            continue;
        }
        let key = InstancedKey::from_item(item, canon);
        match indice.get(&key) {
            Some(&g) => grupos[g].push(i),
            None => {
                indice.insert(key, grupos.len());
                grupos.push(vec![i]);
            }
        }
    }

    let mut grouped = vec![false; meshes.len()];
    let mut instances: Vec<InstanceData> = Vec::new();
    let mut out: Vec<InstancedGroup> = Vec::new();
    for miembros in grupos {
        if miembros.len() < 2 {
            continue;
        }
        let first_instance = instances.len() as u32;
        for &i in &miembros {
            grouped[i] = true;
            instances.push(meshes[i].1.instance_data());
        }
        out.push(InstancedGroup {
            representative: miembros[0],
            range: meshes[miembros[0]].4,
            first_instance,
            count: miembros.len() as u32,
        });
    }
    if out.is_empty() {
        return None;
    }
    Some(ViewInstancing {
        vp,
        groups: out,
        grouped,
        instances,
    })
}


impl GlassRenderer {
    pub fn new(context: &VulkanContext, width: u32, height: u32) -> Result<Self, RendererError> {
        let device = Arc::new(unsafe {
            ash::Device::load(context.instance.fp_v1_0(), context.device.handle())
        });

        // 1. Inicializar GPU Allocator compartido
        let allocator = Arc::new(Mutex::new(Allocator::new(&AllocatorCreateDesc {
            instance: context.instance.clone(),
            device: context.device.clone(),
            physical_device: context.physical_device,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })?));

        // 2. Inicializar Swapchain
        let swapchain = SwapchainContext::new(
            &context.instance,
            device.clone(),
            crate::swapchain::SwapchainTarget {
                surface_loader: &context.surface_loader,
                surface: context.surface,
                physical_device: context.physical_device,
                width,
                height,
                old_swapchain: None,
            },
        )?;

        // 3. Crear RenderPass y Framebuffers para Swapchain
        let swapchain_render_pass = create_swapchain_render_pass(&device, swapchain.format)?;
        let swapchain_framebuffers = create_swapchain_framebuffers(
            &device,
            swapchain_render_pass,
            &swapchain.image_views,
            swapchain.extent,
        )?;

        // Flags del bloque de gráficos, leídos una sola vez del
        // entorno: GLASS_POST=0 apaga la cadena de post y
        // GLASS_SHADOWS=0 las sombras (ver el módulo `post`).
        let post_enabled = std::env::var("GLASS_POST").map(|v| v != "0").unwrap_or(true);
        let shadows_enabled = std::env::var("GLASS_SHADOWS").map(|v| v != "0").unwrap_or(true);
        let post_settings = crate::post::PostSettings::from_env();

        // Layouts del set 2 de sombras (el real, para PBR/piel, y el de
        // relleno, para grafos): se crean antes que los pipelines que
        // los referencian en sus layouts.
        let shadow_set_layout = crate::shadow::ShadowResources::create_set_layout(&device)?;
        let shadow_dummy_layout = crate::shadow::ShadowResources::create_dummy_layout(&device)?;

        // 4. Viewports Offscreen desacoplados (1280x720): los de
        // escena/juego en HDR lineal R16G16B16A16_SFLOAT (ahí se dibuja
        // el 3D con el post activo) y los de presentación en el UNORM
        // LDR de siempre (los consume la UI, el blit y egui; sin post,
        // el 3D se dibuja directamente en ellos).
        let offscreen = OffscreenTarget::with_format(
            device.clone(),
            &mut allocator.lock().unwrap(),
            1280,
            720,
            vk::Format::R16G16B16A16_SFLOAT,
        )?;
        let game_offscreen = OffscreenTarget::with_format(
            device.clone(),
            &mut allocator.lock().unwrap(),
            1280,
            720,
            vk::Format::R16G16B16A16_SFLOAT,
        )?;
        let final_offscreen = OffscreenTarget::new(
            device.clone(),
            &mut allocator.lock().unwrap(),
            1280,
            720,
        )?;
        let final_game_offscreen = OffscreenTarget::new(
            device.clone(),
            &mut allocator.lock().unwrap(),
            1280,
            720,
        )?;

        // 5. Inicializar Pipelines Gráficos (Triángulo por defecto + Mallas 3D + Cook-Torrance PBR)
        // Los de la ruta con post se crean contra el render pass HDR;
        // sus gemelos `*_legacy`, contra el LDR (y el PBR/piel legacy
        // con el fragmento que lleva el ACES dentro).
        let pipeline = GraphicsPipeline::new(device.clone(), offscreen.render_pass, VERT_SPV, FRAG_SPV)?;
        let triangle_pipeline_legacy = GraphicsPipeline::new(device.clone(), final_offscreen.render_pass, VERT_SPV, FRAG_SPV)?;
        let particle_pipeline = GraphicsPipeline::new_particle_pipeline(device.clone(), offscreen.render_pass, PARTICLE_VERT_SPV, PARTICLE_FRAG_SPV)?;
        let particle_pipeline_legacy = GraphicsPipeline::new_particle_pipeline(device.clone(), final_offscreen.render_pass, PARTICLE_VERT_SPV, PARTICLE_FRAG_SPV)?;
        let editor_line_pipeline = GraphicsPipeline::new_editor_line_pipeline(device.clone(), offscreen.render_pass, EDITOR_LINE_VERT_SPV, EDITOR_LINE_FRAG_SPV)?;
        let editor_line_pipeline_legacy = GraphicsPipeline::new_editor_line_pipeline(device.clone(), final_offscreen.render_pass, EDITOR_LINE_VERT_SPV, EDITOR_LINE_FRAG_SPV)?;
        // Contorno de selección de la vista de Escena: mismo par de
        // shaders que el contorno toon (y su variante con piel), sin
        // escritura de profundidad, para los dos render passes.
        let selection_outline_pipeline = GraphicsPipeline::new_selection_outline_pipeline(device.clone(), offscreen.render_pass, OUTLINE_VERT_SPV, OUTLINE_FRAG_SPV)?;
        let selection_outline_pipeline_legacy = GraphicsPipeline::new_selection_outline_pipeline(device.clone(), final_offscreen.render_pass, OUTLINE_VERT_SPV, OUTLINE_FRAG_SPV)?;
        let selection_outline_skinned_pipeline = GraphicsPipeline::new_selection_outline_skinned_pipeline(device.clone(), offscreen.render_pass, OUTLINE_SKINNED_VERT_SPV, OUTLINE_FRAG_SPV)?;
        let selection_outline_skinned_pipeline_legacy = GraphicsPipeline::new_selection_outline_skinned_pipeline(device.clone(), final_offscreen.render_pass, OUTLINE_SKINNED_VERT_SPV, OUTLINE_FRAG_SPV)?;
        // Layout del set 3 (luces puntuales): lo comparten los pipelines
        // PBR y de piel, en sus dos variantes de pase.
        let point_lights_layout = create_point_lights_set_layout(&device)?;
        let lm_tex_layout = create_lm_tex_set_layout(&device)?;
        let lm_data_layout = create_lm_data_set_layout(&device)?;
        let ibl_layout = create_ibl_set_layout(&device)?;
        let mesh_pipeline = GraphicsPipeline::new_mesh_pipeline(device.clone(), offscreen.render_pass, MESH_VERT_SPV, MESH_FRAG_SPV, MeshPipelineLayouts::default())?;
        let mesh_pipeline_legacy = GraphicsPipeline::new_mesh_pipeline(device.clone(), final_offscreen.render_pass, MESH_VERT_SPV, MESH_FRAG_SPV, MeshPipelineLayouts::default())?;
        let pbr_pipeline = GraphicsPipeline::new_mesh_pipeline(device.clone(), offscreen.render_pass, PBR_VERT_SPV, PBR_FRAG_SPV, MeshPipelineLayouts { shadow: Some(shadow_set_layout), point_lights: Some(point_lights_layout), lm: Some((lm_tex_layout, lm_data_layout)), ibl: Some(ibl_layout) })?;
        let pbr_pipeline_legacy = GraphicsPipeline::new_mesh_pipeline(device.clone(), final_offscreen.render_pass, PBR_VERT_SPV, PBR_LEGACY_FRAG_SPV, MeshPipelineLayouts { shadow: Some(shadow_set_layout), point_lights: Some(point_lights_layout), lm: Some((lm_tex_layout, lm_data_layout)), ibl: Some(ibl_layout) })?;
        // Gemelos instanciados del PBR opaco: mismos layouts que pbr_pipeline;
        // el vertex lee model/object_color/lm_params del binding 1.
        let pbr_pipeline_instanced = GraphicsPipeline::new_mesh_pipeline_instanced(device.clone(), offscreen.render_pass, PBR_INSTANCED_VERT_SPV, PBR_INSTANCED_FRAG_SPV, MeshPipelineLayouts { shadow: Some(shadow_set_layout), point_lights: Some(point_lights_layout), lm: Some((lm_tex_layout, lm_data_layout)), ibl: Some(ibl_layout) })?;
        let pbr_pipeline_instanced_legacy = GraphicsPipeline::new_mesh_pipeline_instanced(device.clone(), final_offscreen.render_pass, PBR_INSTANCED_VERT_SPV, PBR_INSTANCED_LEGACY_FRAG_SPV, MeshPipelineLayouts { shadow: Some(shadow_set_layout), point_lights: Some(point_lights_layout), lm: Some((lm_tex_layout, lm_data_layout)), ibl: Some(ibl_layout) })?;
        let skinned_pipeline = GraphicsPipeline::new_skinned_pipeline(device.clone(), offscreen.render_pass, PBR_SKINNED_VERT_SPV, PBR_FRAG_SPV, MeshPipelineLayouts { shadow: Some(shadow_set_layout), point_lights: Some(point_lights_layout), lm: Some((lm_tex_layout, lm_data_layout)), ibl: Some(ibl_layout) })?;
        let skinned_pipeline_legacy = GraphicsPipeline::new_skinned_pipeline(device.clone(), final_offscreen.render_pass, PBR_SKINNED_VERT_SPV, PBR_LEGACY_FRAG_SPV, MeshPipelineLayouts { shadow: Some(shadow_set_layout), point_lights: Some(point_lights_layout), lm: Some((lm_tex_layout, lm_data_layout)), ibl: Some(ibl_layout) })?;
        // Objetivos y pipelines de las vistas de portal (bloque
        // Portales): dos HDR de 1024² (las vistas desde cada
        // portal de la pareja), sus pipelines de superficie
        // (muestrean el objetivo contrario vía el set 4, con el
        // layout de textura del lightmap) y los sets que los
        // exponen.
        // OJO: cada with_format en su PROPIA sentencia — el
        // guardián temporal de allocator.lock() vive hasta el
        // fin de la sentencia; dentro de un literal de array
        // compartido, el segundo lock se autobloqueaba.
        let portal_target_a = OffscreenTarget::with_format(
            device.clone(),
            &mut allocator.lock().unwrap(),
            1024,
            1024,
            vk::Format::R16G16B16A16_SFLOAT,
        )?;
        let portal_target_b = OffscreenTarget::with_format(
            device.clone(),
            &mut allocator.lock().unwrap(),
            1024,
            1024,
            vk::Format::R16G16B16A16_SFLOAT,
        )?;
        let portal_targets = [portal_target_a, portal_target_b];
        let portal_pipeline = GraphicsPipeline::new_mesh_pipeline(
            device.clone(),
            offscreen.render_pass,
            PORTAL_VERT_SPV,
            PORTAL_FRAG_SPV,
            MeshPipelineLayouts { lm: Some((lm_tex_layout, lm_data_layout)), ..Default::default() },
        )?;
        let portal_pipeline_legacy = GraphicsPipeline::new_mesh_pipeline(
            device.clone(),
            final_offscreen.render_pass,
            PORTAL_VERT_SPV,
            PORTAL_FRAG_SPV,
            MeshPipelineLayouts { lm: Some((lm_tex_layout, lm_data_layout)), ..Default::default() },
        )?;
        let portal_sets = unsafe {
            let pool_sizes = [vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(2)];
            let pool_info = vk::DescriptorPoolCreateInfo::default()
                .pool_sizes(&pool_sizes)
                .max_sets(2);
            let pool = device.create_descriptor_pool(&pool_info, None)?;
            let layouts = [lm_tex_layout, lm_tex_layout];
            let alloc_info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(pool)
                .set_layouts(&layouts);
            let sets = device.allocate_descriptor_sets(&alloc_info)?;
            for (i, set) in sets.iter().enumerate() {
                let image_info = [vk::DescriptorImageInfo::default()
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .image_view(portal_targets[i].color_view)
                    .sampler(portal_targets[i].sampler)];
                let write = [vk::WriteDescriptorSet::default()
                    .dst_set(*set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(&image_info)];
                device.update_descriptor_sets(&write, &[]);
            }
            let mut it = sets.into_iter();
            [it.next().unwrap(), it.next().unwrap()]
        };
        // Variantes transparentes del PBR (Materiales v1): mismos
        // layouts que el PBR (el fragmento declara los sets 3/4/5,
        // así que NO pueden crearse como pipelines de grafo) con
        // mezcla por alfa. Se registran en los mapas de pipelines
        // con nombre reservado tras construir el renderer.
        let pbr_transparent = GraphicsPipeline::new_mesh_pipeline_transparent(device.clone(), offscreen.render_pass, PBR_VERT_SPV, PBR_FRAG_SPV, MeshPipelineLayouts { shadow: Some(shadow_set_layout), point_lights: Some(point_lights_layout), lm: Some((lm_tex_layout, lm_data_layout)), ibl: Some(ibl_layout) })?;
        let pbr_transparent_legacy = GraphicsPipeline::new_mesh_pipeline_transparent(device.clone(), final_offscreen.render_pass, PBR_VERT_SPV, PBR_LEGACY_FRAG_SPV, MeshPipelineLayouts { shadow: Some(shadow_set_layout), point_lights: Some(point_lights_layout), lm: Some((lm_tex_layout, lm_data_layout)), ibl: Some(ibl_layout) })?;

        // Recursos del mapa de sombras: imagen D32, UBO de frame, set
        // real (set 2 del PBR/piel) y de relleno (set 2 de grafos) y
        // los dos pipelines de profundidad — el de piel enlaza el set 1
        // de articulaciones, cuyo layout posee el pipeline con piel.
        let shadow = crate::shadow::ShadowResources::new(
            device.clone(),
            &mut allocator.lock().unwrap(),
            shadow_set_layout,
            shadow_dummy_layout,
            skinned_pipeline
                .joints_set_layout
                .ok_or_else(|| RendererError::Descriptor("pipeline con piel sin layout de articulaciones".into()))?,
            pbr_pipeline
                .descriptor_set_layout
                .ok_or_else(|| RendererError::Descriptor("pipeline PBR sin layout de textura".into()))?,
            PBR_VERT_SPV,
            PBR_SKINNED_VERT_SPV,
        )?;

// post-procesado.
        // Atlas de cubos de las sombras puntuales (Fase 2): se crea tras
        // los recursos del sol porque sus framebuffers usan su render
        // pass, y su vista se escribe en el binding 2 del set 2 real
        // antes del primer frame.
        let point_shadows = crate::shadow::PointShadowResources::new(
            &device,
            &mut allocator.lock().unwrap(),
            shadow.render_pass,
        )?;
        shadow.write_point_shadow_set(&device, point_shadows.sample_view, point_shadows.sampler);

        // Cadena de post-procesado: bright/blur a media resolución y la
        // pasada final contra el render pass LDR de presentación.
        let post = crate::post::PostChain::new(
            device.clone(),
            &mut allocator.lock().unwrap(),
            1280,
            720,
            final_offscreen.render_pass,
        )?;

        // Pool de sets de articulaciones: un STORAGE_BUFFER por entidad
        // con piel viva; 256 cubre de sobra los personajes/NPC del bloque.
        let joints_pool = unsafe {
            let pool_sizes = [vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(256)];
            let pool_info = vk::DescriptorPoolCreateInfo::default()
                .pool_sizes(&pool_sizes)
                .max_sets(256)
                .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET);
            device.create_descriptor_pool(&pool_info, None)?
        };

        // 5b. Canal global de los grafos: layout + pool + buffer + set del
        // UBO `GraphGlobals` (dos vec4). Se crea una sola vez y lo comparten
        // todos los pipelines de grafo; ningún otro pipeline lo declara.
        // Mismo patrón de buffer mapeado CpuToGpu que los JointBuffer.
        let (
            graph_globals_layout,
            graph_globals_pool,
            graph_globals_set,
            graph_globals_buffer,
            graph_globals_allocation,
            graph_globals_mapped,
            graph_props_dummy_buffer,
            graph_props_dummy_allocation,
            graph_props_dummy_mapped,
        ) = unsafe {
                let bindings = [
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(0)
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
                    // Binding 1: propiedades de grafo (CL-2S2a). Todos los
                    // pipelines de grafo lo declaran en el mismo layout que
                    // el canal de interacción, así que el set global y
                    // cualquier set de instancia de `graph_props_pool` son
                    // intercambiables desde el punto de vista del layout.
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(1)
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
                ];
                let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
                let layout = device.create_descriptor_set_layout(&layout_info, None)?;

                let pool_sizes = [vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(2)];
                let pool_info = vk::DescriptorPoolCreateInfo::default()
                    .pool_sizes(&pool_sizes)
                    .max_sets(1);
                let pool = device.create_descriptor_pool(&pool_info, None)?;

                let size = std::mem::size_of::<[f32; 8]>() as u64;
                let buffer_info = vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(vk::BufferUsageFlags::UNIFORM_BUFFER)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE);
                let buffer = device.create_buffer(&buffer_info, None)?;
                let mem_reqs = device.get_buffer_memory_requirements(buffer);
                let allocation = allocator
                    .lock()
                    .unwrap()
                    .allocate(&AllocationCreateDesc {
                        name: "graph_globals",
                        requirements: mem_reqs,
                        location: MemoryLocation::CpuToGpu,
                        linear: true,
                        allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                    })
                    .map_err(RendererError::Allocator)?;
                device.bind_buffer_memory(buffer, allocation.memory(), allocation.offset())?;

                // Buffer de respaldo del binding 1: 512 B (32 vec4), el
                // tope de propiedades que declara un `GraphPropsEntry`.
                let props_dummy_size = (std::mem::size_of::<[f32; 4]>() * 32) as u64;
                let props_dummy_buffer_info = vk::BufferCreateInfo::default()
                    .size(props_dummy_size)
                    .usage(vk::BufferUsageFlags::UNIFORM_BUFFER)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE);
                let props_dummy_buffer = device.create_buffer(&props_dummy_buffer_info, None)?;
                let props_dummy_mem_reqs = device.get_buffer_memory_requirements(props_dummy_buffer);
                let props_dummy_allocation = allocator
                    .lock()
                    .unwrap()
                    .allocate(&AllocationCreateDesc {
                        name: "graph_props_dummy",
                        requirements: props_dummy_mem_reqs,
                        location: MemoryLocation::CpuToGpu,
                        linear: true,
                        allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                    })
                    .map_err(RendererError::Allocator)?;
                device.bind_buffer_memory(
                    props_dummy_buffer,
                    props_dummy_allocation.memory(),
                    props_dummy_allocation.offset(),
                )?;
                let props_dummy_mapped = props_dummy_allocation
                    .mapped_ptr()
                    .map(|p| p.as_ptr() as *mut u8)
                    .ok_or(RendererError::Descriptor(
                        "el buffer de propiedades de grafo no quedó mapeado".into(),
                    ))?;
                std::ptr::write_bytes(props_dummy_mapped, 0, props_dummy_size as usize);

                let layouts = [layout];
                let set_alloc_info = vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&layouts);
                let set = device
                    .allocate_descriptor_sets(&set_alloc_info)?
                    .into_iter()
                    .next()
                    .ok_or(RendererError::Descriptor("pool de globales sin sets".into()))?;
                let buffer_desc = [vk::DescriptorBufferInfo::default()
                    .buffer(buffer)
                    .offset(0)
                    .range(size)];
                let props_dummy_desc = [vk::DescriptorBufferInfo::default()
                    .buffer(props_dummy_buffer)
                    .offset(0)
                    .range(props_dummy_size)];
                let write = [
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .buffer_info(&buffer_desc),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .buffer_info(&props_dummy_desc),
                ];
                device.update_descriptor_sets(&write, &[]);

                let mapped = allocation
                    .mapped_ptr()
                    .map(|p| p.as_ptr() as *mut u8)
                    .ok_or(RendererError::Descriptor(
                        "el buffer de globales de grafo no quedó mapeado".into(),
                    ))?;
                // Valor inicial: el canal inactivo, para que un grafo que
                // use el nodo antes de que nadie publique se vea normal.
                let inactive = glass_core::shader_graph::INTERACTION_GLOBALS_INACTIVE;
                std::ptr::copy_nonoverlapping(
                    inactive.as_ptr() as *const u8,
                    mapped,
                    std::mem::size_of::<[f32; 8]>(),
                );
                (
                    layout,
                    pool,
                    set,
                    buffer,
                    Some(allocation),
                    mapped,
                    props_dummy_buffer,
                    Some(props_dummy_allocation),
                    props_dummy_mapped,
                )
            };

        // Pool de sets de propiedades por instancia (CL-2S2a): comparte
        // layout con el canal global pero cada instancia que lo pida
        // obtiene su propio set y su propio buffer de binding 1.
        let graph_props_pool = unsafe {
            let pool_sizes = [vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(128)];
            let pool_info = vk::DescriptorPoolCreateInfo::default()
                .pool_sizes(&pool_sizes)
                .max_sets(64);
            device.create_descriptor_pool(&pool_info, None)?
        };
        let graph_props_sets: Vec<GraphPropsEntry> = Vec::new();

        // 5c. Luces puntuales: pool + buffer + set del UBO (set 3).
        // Mismo patrón que 5b; el contenido inicial es todo ceros
        // (count = 0: ninguna luz, resultado idéntico al de antes).
        let (point_lights_pool, point_lights_set, point_lights_buffer,
             point_lights_allocation, point_lights_mapped, point_lights_stride) = unsafe {
            let pool_sizes = [vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                .descriptor_count(1)];
            let pool_info = vk::DescriptorPoolCreateInfo::default()
                .pool_sizes(&pool_sizes)
                .max_sets(1);
            let pool = device.create_descriptor_pool(&pool_info, None)?;

            fn alinear_arriba(v: u64, a: u64) -> u64 {
                if a == 0 { v } else { v.div_ceil(a) * a }
            }
            let point_lights_stride = alinear_arriba(
                POINT_LIGHTS_UBO_SIZE,
                context.physical_device_properties.limits.min_uniform_buffer_offset_alignment,
            );
            let size = point_lights_stride * POINT_LIGHT_SLOTS;
            let buffer_info = vk::BufferCreateInfo::default()
                .size(size)
                .usage(vk::BufferUsageFlags::UNIFORM_BUFFER)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);
            let buffer = device.create_buffer(&buffer_info, None)?;
            let mem_reqs = device.get_buffer_memory_requirements(buffer);
            let allocation = allocator
                .lock()
                .unwrap()
                .allocate(&AllocationCreateDesc {
                    name: "point_lights",
                    requirements: mem_reqs,
                    location: MemoryLocation::CpuToGpu,
                    linear: true,
                    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                })
                .map_err(RendererError::Allocator)?;
            device.bind_buffer_memory(buffer, allocation.memory(), allocation.offset())?;

            let layouts = [point_lights_layout];
            let set_alloc_info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(pool)
                .set_layouts(&layouts);
            let set = device
                .allocate_descriptor_sets(&set_alloc_info)?
                .into_iter()
                .next()
                .ok_or(RendererError::Descriptor("pool de luces puntuales sin sets".into()))?;

            let buffer_desc = [vk::DescriptorBufferInfo::default()
                .buffer(buffer)
                .offset(0)
                .range(POINT_LIGHTS_UBO_SIZE)];
            let write = [vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                .buffer_info(&buffer_desc)];
            device.update_descriptor_sets(&write, &[]);

            let mapped = allocation
                .mapped_ptr()
                .map(|p| p.as_ptr() as *mut u8)
                .ok_or(RendererError::Descriptor(
                    "el buffer de luces puntuales no quedó mapeado".into(),
                ))?;
            std::ptr::write_bytes(mapped, 0, size as usize);

            (pool, set, buffer, Some(allocation), mapped, point_lights_stride)
        };

        // 6. Command Pool y Buffers
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(context.queue_families.graphics_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&pool_info, None)? };

        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let command_buffer = unsafe { device.allocate_command_buffers(&alloc_info)?[0] };


        // 6c. Lightmap (sets 4 y 5): pool, UBO de grupos a ceros,
        // muestreador propio y textura 1×1 negra por defecto con su
        // set. Sin bake publicado, los dibujos PBR enlazan esto y el
        // shader no muestrea nada (lm_params = -1): coste cero.
        let (lm_pool, lm_data_set, lm_data_buffer, lm_data_allocation,
             lm_data_mapped, lm_sampler, lm_default_tex_set,
             lm_runtime_tex_set, lm_default_texture) = unsafe {
            let pool_sizes = [
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(2),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(1),
            ];
            let pool_info = vk::DescriptorPoolCreateInfo::default()
                .pool_sizes(&pool_sizes)
                .max_sets(3);
            let pool = device.create_descriptor_pool(&pool_info, None)?;

            let size = std::mem::size_of::<LmDataUbo>() as u64;
            let buffer_info = vk::BufferCreateInfo::default()
                .size(size)
                .usage(vk::BufferUsageFlags::UNIFORM_BUFFER)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);
            let buffer = device.create_buffer(&buffer_info, None)?;
            let mem_reqs = device.get_buffer_memory_requirements(buffer);
            let allocation = allocator
                .lock()
                .unwrap()
                .allocate(&AllocationCreateDesc {
                    name: "lightmap_groups",
                    requirements: mem_reqs,
                    location: MemoryLocation::CpuToGpu,
                    linear: true,
                    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                })
                .map_err(RendererError::Allocator)?;
            device.bind_buffer_memory(buffer, allocation.memory(), allocation.offset())?;

            let layouts = [lm_data_layout];
            let set_alloc_info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(pool)
                .set_layouts(&layouts);
            let data_set = device
                .allocate_descriptor_sets(&set_alloc_info)?
                .into_iter()
                .next()
                .ok_or(RendererError::Descriptor(
                    "pool del lightmap sin set de datos".into(),
                ))?;
            let buffer_desc = [vk::DescriptorBufferInfo::default()
                .buffer(buffer)
                .offset(0)
                .range(size)];
            let write = [vk::WriteDescriptorSet::default()
                .dst_set(data_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&buffer_desc)];
            device.update_descriptor_sets(&write, &[]);

            let mapped = allocation
                .mapped_ptr()
                .map(|p| p.as_ptr() as *mut u8)
                .ok_or(RendererError::Descriptor(
                    "el UBO del lightmap no quedó mapeado".into(),
                ))?;
            std::ptr::write_bytes(mapped, 0, size as usize);

            let sampler_info = vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .mipmap_mode(vk::SamplerMipmapMode::LINEAR);
            let sampler = device.create_sampler(&sampler_info, None)?;

            let default_tex = crate::mesh::MeshLoader::upload_texture_to_gpu(
                &device,
                &mut allocator.lock().unwrap(),
                &crate::mesh::GltfImageData {
                    width: 1,
                    height: 1,
                    rgba: vec![0, 0, 0, 255],
                },
                context.graphics_queue,
                command_pool,
            )
            .map_err(RendererError::Descriptor)?;

            let tex_layouts = [lm_tex_layout, lm_tex_layout];
            let tex_alloc_info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(pool)
                .set_layouts(&tex_layouts);
            let mut tex_sets = device
                .allocate_descriptor_sets(&tex_alloc_info)?
                .into_iter();
            let default_set = tex_sets.next().ok_or(RendererError::Descriptor(
                "pool del lightmap sin set de textura".into(),
            ))?;
            let runtime_set = tex_sets.next().ok_or(RendererError::Descriptor(
                "pool del lightmap sin set runtime".into(),
            ))?;
            let image_info = [vk::DescriptorImageInfo::default()
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .image_view(default_tex.image_view)
                .sampler(sampler)];
            let write = [
                vk::WriteDescriptorSet::default()
                    .dst_set(default_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(&image_info),
                vk::WriteDescriptorSet::default()
                    .dst_set(runtime_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(&image_info),
            ];
            device.update_descriptor_sets(&write, &[]);

            (
                pool,
                data_set,
                buffer,
                Some(allocation),
                mapped,
                sampler,
                default_set,
                runtime_set,
                Some(default_tex),
            )
        };

        // 6e. IBL (set 6): pool propio, UBO de parámetros a ceros,
        // muestreador lineal con mips y cubemap 1×1 negro por defecto.
        let (ibl_pool, ibl_set, ibl_params_buffer, ibl_params_allocation,
             ibl_params_mapped, ibl_sampler, ibl_cube_texture) = unsafe {
            let pool_sizes = [
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(1),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(1),
            ];
            let ibl_pool = device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(1)
                        .pool_sizes(&pool_sizes),
                    None,
                )
                .map_err(|e| RendererError::Descriptor(format!("{e:?}")))?;

            let ibl_params_buffer = device
                .create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(16)
                        .usage(vk::BufferUsageFlags::UNIFORM_BUFFER)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE),
                    None,
                )
                .map_err(|e| RendererError::Descriptor(format!("{e:?}")))?;

            let ibl_params_requirements =
                device.get_buffer_memory_requirements(ibl_params_buffer);
            let ibl_params_allocation = allocator
                .lock()
                .unwrap()
                .allocate(&AllocationCreateDesc {
                    name: "ibl_params",
                    requirements: ibl_params_requirements,
                    location: MemoryLocation::CpuToGpu,
                    linear: true,
                    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                })
                .map_err(RendererError::Allocator)?;
            device.bind_buffer_memory(
                ibl_params_buffer,
                ibl_params_allocation.memory(),
                ibl_params_allocation.offset(),
            )?;
            let ibl_params_mapped = ibl_params_allocation
                .mapped_ptr()
                .map(|p| p.as_ptr() as *mut u8)
                .ok_or(RendererError::Descriptor(
                    "el buffer de parámetros IBL no quedó mapeado".into(),
                ))?;
            std::ptr::write_bytes(ibl_params_mapped, 0, 16);

            let ibl_sampler = device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::LINEAR)
                        .min_filter(vk::Filter::LINEAR)
                        .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .anisotropy_enable(false)
                        .max_lod(8.0),
                    None,
                )
                .map_err(|e| RendererError::Descriptor(format!("{e:?}")))?;

            let ibl_cube = create_ibl_cube(
                &device,
                &mut allocator.lock().unwrap(),
                context.graphics_queue,
                command_pool,
                1,
                1,
                &[0u8; 48],
            )
            .map_err(RendererError::Descriptor)?;

            let ibl_set = device
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(ibl_pool)
                        .set_layouts(&[ibl_layout]),
                )
                .map_err(|e| RendererError::Descriptor(format!("{e:?}")))?
                .into_iter()
                .next()
                .ok_or(RendererError::Descriptor("pool IBL sin sets".into()))?;

            let image_info = [vk::DescriptorImageInfo::default()
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .image_view(ibl_cube.image_view)
                .sampler(ibl_sampler)];
            let buffer_info = [vk::DescriptorBufferInfo::default()
                .buffer(ibl_params_buffer)
                .offset(0)
                .range(16)];
            let writes = [
                vk::WriteDescriptorSet::default()
                    .dst_set(ibl_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(&image_info),
                vk::WriteDescriptorSet::default()
                    .dst_set(ibl_set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(&buffer_info),
            ];
            device.update_descriptor_sets(&writes, &[]);

            (
                ibl_pool,
                ibl_set,
                ibl_params_buffer,
                Some(ibl_params_allocation),
                ibl_params_mapped,
                ibl_sampler,
                Some(ibl_cube),
            )
        };

        // 6b. Pase 2D del Canvas (UI de juego sin egui): necesita la cola
        // y el pool para la subida única del atlas de fuente.
        let ui = UiRenderer::new(
            device.clone(),
            &mut allocator.lock().unwrap(),
            context.graphics_queue,
            command_pool,
        )?;

        // 6d. Query pool de timestamps para la telemetría del pase principal.
        // Si la familia gráfica no admite timestamps (valid_bits = 0) o el
        // periodo es 0, se deja el handle nulo y la medición se desactiva.
        let queue_family_props = unsafe {
            context
                .instance
                .get_physical_device_queue_family_properties(context.physical_device)
        };
        let timestamp_valid_bits = queue_family_props
            .get(context.queue_families.graphics_family as usize)
            .map(|props| props.timestamp_valid_bits)
            .unwrap_or(0);
        let timestamp_period = context.physical_device_properties.limits.timestamp_period;
        let timestamp_pool = if timestamp_valid_bits > 0 && timestamp_period > 0.0 {
            let pool_info = vk::QueryPoolCreateInfo::default()
                .query_type(vk::QueryType::TIMESTAMP)
                .query_count(2);
            unsafe { device.create_query_pool(&pool_info, None)? }
        } else {
            log::warn!(
                "Timestamps GPU no admitidos (valid_bits={}, period={}); telemetría GPU desactivada",
                timestamp_valid_bits,
                timestamp_period
            );
            vk::QueryPool::null()
        };

        // 7. Sincronización
        let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
        let fence = unsafe { device.create_fence(&fence_info, None)? };

        let semaphore_info = vk::SemaphoreCreateInfo::default();
        let image_available_semaphore = unsafe { device.create_semaphore(&semaphore_info, None)? };
        let render_finished_semaphores = (0..swapchain.images.len())
            .map(|_| unsafe { device.create_semaphore(&semaphore_info, None) })
            .collect::<Result<Vec<_>, _>>()?;

        log::info!("GlassRenderer inicializado con éxito con soporte de Swapchain, PBR y Viewports Offscreen independientes (Scene & Game).");

        Ok(Self {
            device,
            allocator,
            swapchain,
            swapchain_render_pass,
            swapchain_framebuffers,
            offscreen,
            game_offscreen,
            final_offscreen,
            final_game_offscreen,
            portal_targets,
            portal_sets,
            portal_pipeline,
            portal_pipeline_legacy,
            pipeline,
            mesh_pipeline,
            pbr_pipeline,
            pbr_pipeline_instanced,
            skinned_pipeline,
            triangle_pipeline_legacy,
            particle_pipeline,
            particle_pipeline_legacy,
            particle_scene: ParticleBuffers::default(),
            particle_game: ParticleBuffers::default(),
            instance_scene: InstanceBuffers::default(),
            instance_game: InstanceBuffers::default(),
            editor_line_pipeline,
            editor_line_pipeline_legacy,
            selection_outline_pipeline,
            selection_outline_pipeline_legacy,
            selection_outline_skinned_pipeline,
            selection_outline_skinned_pipeline_legacy,
            editor_line_buffers: EditorLineBuffers::default(),
            editor_grid_vp: [0.0; 16],
            particle_draws: std::cell::Cell::new(0),
            mesh_pipeline_legacy,
            pbr_pipeline_legacy,
            pbr_pipeline_instanced_legacy,
            skinned_pipeline_legacy,
            shadow,
            point_shadows,
            point_shadow_lights: Vec::new(),
            post,
            post_enabled,
            present_game: false,
            post_settings,
            shadows_enabled,
            shadow_vp: IDENTITY_MAT4,
            shadow_params: [
                1.0 / crate::shadow::SHADOW_MAP_SIZE as f32,
                crate::shadow::SHADOW_DEPTH_BIAS,
                crate::shadow::SHADOW_NORMAL_BIAS,
                0.0,
            ],
            fog_color_dist: [0.0; 4],
            fog_far_mode: [0.0; 4],
            ambient_sky: [
                glass_core::scene::LEGACY_AMBIENT_SKY[0],
                glass_core::scene::LEGACY_AMBIENT_SKY[1],
                glass_core::scene::LEGACY_AMBIENT_SKY[2],
                0.0,
            ],
            ambient_equator: [
                glass_core::scene::LEGACY_AMBIENT_EQUATOR[0],
                glass_core::scene::LEGACY_AMBIENT_EQUATOR[1],
                glass_core::scene::LEGACY_AMBIENT_EQUATOR[2],
                0.0,
            ],
            ambient_ground: [
                glass_core::scene::LEGACY_AMBIENT_GROUND[0],
                glass_core::scene::LEGACY_AMBIENT_GROUND[1],
                glass_core::scene::LEGACY_AMBIENT_GROUND[2],
                0.0,
            ],
            joints_pool,
            custom_pipelines: std::collections::HashMap::from([(
                "material_transparent".to_string(),
                pbr_transparent,
            )]),
            custom_pipelines_legacy: std::collections::HashMap::from([(
                "material_transparent".to_string(),
                pbr_transparent_legacy,
            )]),
            graph_globals_layout,
            graph_globals_pool,
            graph_globals_set,
            graph_depth_pipelines: std::collections::HashMap::new(),
            graph_depth_clip_pipelines: std::collections::HashMap::new(),
            graph_outline_pipelines: std::collections::HashMap::new(),
            graph_outline_pipelines_legacy: std::collections::HashMap::new(),
            frame_pipeline_binds: std::cell::Cell::new(0),
            frame_descriptor_binds: std::cell::Cell::new(0),
            frame_draw_calls: std::cell::Cell::new(0),
            graph_globals_buffer,
            graph_globals_allocation,
            graph_globals_mapped,
            graph_props_dummy_buffer,
            graph_props_dummy_allocation,
            graph_props_dummy_mapped,
            graph_props_pool,
            graph_props_sets,
            graph_globals: glass_core::shader_graph::INTERACTION_GLOBALS_INACTIVE,
            point_lights_layout,
            point_lights_pool,
            point_lights_set,
            point_lights_buffer,
            point_lights_allocation,
            point_lights_mapped,
            point_lights: PointLightsUbo::default(),
            point_lights_stride,
            object_lights_pending: Vec::new(),
            slot_hash: vec![u64::MAX; POINT_LIGHT_SLOTS as usize],
            slot_ubo: vec![None; POINT_LIGHT_SLOTS as usize],
            slot_canon: (0..POINT_LIGHT_SLOTS as u32).collect(),
            slot_canon_dirty: false,
            lm_tex_layout,
            lm_data_layout,
            lm_pool,
            lm_tex_set: lm_default_tex_set,
            lm_default_tex_set,
            lm_runtime_tex_set,
            lm_data_set,
            lm_data_buffer,
            lm_data_allocation,
            lm_data_mapped,
            lm_sampler,
            lm_default_texture,
            lm_atlas_texture: None,
            lm_index: std::collections::HashMap::new(),
            lm_atlas_size: 1.0,
            ibl_set_layout: ibl_layout,
            ibl_pool,
            ibl_set,
            ibl_params_buffer,
            ibl_params_allocation,
            ibl_params_mapped,
            ibl_sampler,
            ibl_cube_texture,
            ui,
            ui_images: None,
            ui_named_fonts: std::collections::HashMap::new(),
            command_pool,
            command_buffer,
            fence,
            image_available_semaphore,
            render_finished_semaphores,
            fallback_set: vk::DescriptorSet::null(),
            timestamp_pool,
            timestamp_period,
            gpu_render_us: None,
            timestamp_pending: false,
        })
    }

    /// Duración del último pase principal (`render_views`) medida con
    /// timestamps GPU, en microsegundos. `None` si el dispositivo no admite
    /// timestamps o si aún no se ha leído ninguna medición.
    pub fn last_gpu_render_us(&self) -> Option<u64> {
        self.gpu_render_us
    }

    pub fn render_finished_for(&self, image_index: u32) -> vk::Semaphore {
        self.render_finished_semaphores[image_index as usize]
    }

    /// Crea el buffer de matrices de articulación de UNA entidad con piel
    /// (capacidad = nº de articulaciones de su piel), con su set del pool
    /// de articulaciones. La app guarda uno por entidad y lo reescribe
    /// cada frame con la pose evaluada.
    pub fn create_joint_buffer(
        &self,
        joint_count: usize,
    ) -> Result<crate::skin::JointBuffer, RendererError> {
        let layout = self
            .skinned_pipeline
            .joints_set_layout
            .ok_or_else(|| RendererError::Descriptor("pipeline con piel sin layout de set 1".into()))?;
        crate::skin::JointBuffer::new(
            &self.device,
            &mut self.allocator.lock().unwrap(),
            self.joints_pool,
            layout,
            joint_count,
        )
        .map_err(RendererError::Descriptor)
    }

    /// Dibuja los lotes con piel de una pasada. Se llama DESPUÉS de los
    /// lotes normales: enlaza el pipeline con piel una vez y, por lote,
    /// los sets 0 (textura) y 1 (articulaciones) en una sola llamada —
    /// dejar el set 0 de un dibujo anterior enlazado sería justo el fallo
    /// que el diseño quiere imposibilitar al ser explícito.
    unsafe fn draw_skinned(&self, cmd: vk::CommandBuffer, items: &[SkinnedDrawItem<'_>], legacy: bool) {
        let pipe = if legacy { &self.skinned_pipeline_legacy } else { &self.skinned_pipeline };
        self.device.cmd_bind_pipeline(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            pipe.pipeline,
        );
        for (mesh, push_consts, tex_set, joints_set, range, light_slot) in items {
            let sets = [*tex_set, *joints_set];
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipe.layout,
                0,
                &sets,
                &[],
            );
            // Set 2: el UBO de sombras (el fragmento es el mismo PBR).
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipe.layout,
                2,
                &[self.shadow.set],
                &[],
            );
            // Set 3: el UBO de luces puntuales (mismo fragmento PBR),
            // con el desplazamiento dinámico de la ranura del objeto.
            self.bind_point_lights_set(cmd, pipe, *light_slot);
            // Sets 4 y 5: el lightmap (mismo fragmento PBR).
            self.bind_lightmap_sets(cmd, pipe);
            self.bind_ibl_set(cmd, pipe);
            let push_bytes = bytemuck::bytes_of(push_consts);
            self.device.cmd_push_constants(
                cmd,
                pipe.layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                push_bytes,
            );
            self.device
                .cmd_bind_vertex_buffers(cmd, 0, &[mesh.vertex_buffer], &[0]);
            self.device
                .cmd_bind_index_buffer(cmd, mesh.index_buffer, 0, vk::IndexType::UINT32);
            self.device
                .cmd_draw_indexed(cmd, range.1, 1, range.0, 0, 0);
        }
    }

    /// Contorno de selección de las mallas con piel: como `draw_skinned`
    /// pero con la pipeline de selección con piel y enlazando SOLO los
    /// sets 0 (textura) y 1 (articulaciones). Sin sombras, luces,
    /// lightmap ni IBL: outline_skinned.vert solo declara el set 1.
    unsafe fn draw_skinned_outline(&self, cmd: vk::CommandBuffer, items: &[SkinnedDrawItem<'_>], legacy: bool) {
        let pipe = if legacy {
            &self.selection_outline_skinned_pipeline_legacy
        } else {
            &self.selection_outline_skinned_pipeline
        };
        self.device.cmd_bind_pipeline(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            pipe.pipeline,
        );
        for (mesh, push_consts, tex_set, joints_set, range, _light_slot) in items {
            let sets = [*tex_set, *joints_set];
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipe.layout,
                0,
                &sets,
                &[],
            );
            let push_bytes = bytemuck::bytes_of(push_consts);
            self.device.cmd_push_constants(
                cmd,
                pipe.layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                push_bytes,
            );
            self.device
                .cmd_bind_vertex_buffers(cmd, 0, &[mesh.vertex_buffer], &[0]);
            self.device
                .cmd_bind_index_buffer(cmd, mesh.index_buffer, 0, vk::IndexType::UINT32);
            self.device
                .cmd_draw_indexed(cmd, range.1, 1, range.0, 0, 0);
        }
    }

    /// Registra un pipeline de fragmento personalizado (material de grafo)
    /// bajo `name`. Mismo vertex shader y mismo layout que el PBR.
    pub fn register_fragment_pipeline(&mut self, name: &str, frag_spv: &[u8]) -> Result<(), RendererError> {
        self.register_graph_pipeline(name, None, frag_spv, false)
    }

    /// Fija el canal global que leerán los grafos desde el frame
    /// siguiente: los dos vec4 seguidos (punto de interacción + forma de
    /// su huella). El valor se vuelca al UBO global al principio de
    /// `render_views`, tras la espera del fence; llamarlo entre frames
    /// es siempre seguro. Para desactivar el efecto, publicar
    /// `glass_core::shader_graph::INTERACTION_GLOBALS_INACTIVE`.
    pub fn set_graph_globals(&mut self, globals: [f32; 8]) {
        self.graph_globals = globals;
    }

    /// El canal global vigente en el lado CPU.
    pub fn graph_globals(&self) -> [f32; 8] {
        self.graph_globals
    }

    /// Fija la VP de la luz y los parámetros de sombra del frame
    /// siguiente (`params.w = 1` activa el efecto; ver `shadow::
    /// ShadowUbo`). El valor se vuelca al UBO del set 2 al principio de
    /// `render_views`, tras la espera del fence, y el pase de
    /// profundidad se graba con los proyectores que acompañen a esa
    /// llamada. Sin luz en la escena, la app publica `params.w = 0`.
    /// Contadores de enlaces acumulados por el último `render_views`
    /// (Fase 2S, S6): (pipelines enlazados, descriptor sets enlazados)
    /// en los bucles de mallas de los pases de vista.
    pub fn frame_bind_counts(&self) -> (u32, u32) {
        (self.frame_pipeline_binds.get(), self.frame_descriptor_binds.get())
    }

    /// Draws reales del bucle de mallas del último frame (ver el campo).
    pub fn frame_draw_calls(&self) -> u32 {
        self.frame_draw_calls.get()
    }

    pub fn set_shadow_frame(&mut self, light_vp: [f32; 16], params: [f32; 4]) {
        self.shadow_vp = light_vp;
        self.shadow_params = params;
    }

    /// Enlaza el set 2 en un dibujo: el set real de sombras en el PBR
    /// (su shader lo declara) y el de RELLENO en los grafos (su shader
    /// no lo declara, pero su layout tiene la misma forma y el enlace
    /// mantiene válidos los sets 0/1 al alternar pipelines). El
    /// pipeline de mallas (Lambert) no declara set 2 y no recibe
    /// sombra en esta entrega.
    unsafe fn bind_shadow_set(&self, cmd: vk::CommandBuffer, pipe: &GraphicsPipeline) {
        if pipe.shadow_set_layout.is_some() {
            let set = if pipe.globals_set_layout.is_some() {
                self.shadow.dummy_set
            } else {
                self.shadow.set
            };
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipe.layout,
                2,
                &[set],
                &[],
            );
        }
    }

    /// Publica el UBO de luces puntuales del frame siguiente: la app lo
    /// recolecta de la escena (las 8 Point más cercanas a la cámara) y
    /// `render_views` lo vuelca al buffer tras la espera del fence.
    pub fn set_point_lights(&mut self, lights: PointLightsUbo) {
        self.point_lights = lights;
    }

    /// Publica el UBO de luces de UN objeto (su ranura se vuelca en
    /// `upload_frame_ubos`). La ranura 0 es el global: no usarla aquí.
    pub fn set_object_lights(&mut self, slot: u32, lights: PointLightsUbo) {
        if slot == 0 || slot as u64 >= POINT_LIGHT_SLOTS {
            return;
        }
        self.object_lights_pending.push((slot, lights));
    }

    /// Apunta el contenido de una ranura de luces recién volcada
    /// (render instanciado: la canónica agrupa por contenido).
    fn registrar_ranura_luz(&mut self, slot: u32, ubo: PointLightsUbo) {
        let i = slot as usize;
        if i >= self.slot_hash.len() {
            return;
        }
        let h = {
            let mut hash: u64 = 0xcbf29ce484222325;
            for b in bytemuck::bytes_of(&ubo) {
                hash ^= *b as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            hash
        };
        if self.slot_hash[i] != h {
            self.slot_hash[i] = h;
            self.slot_ubo[i] = Some(ubo);
            self.slot_canon_dirty = true;
        }
    }

    /// Mapa ranura → ranura canónica (la primera con el mismo
    /// contenido de luces; una ranura de contenido desconocido es su
    /// propia canónica). Se reconstruye solo cuando un volcado
    /// cambia algún contenido; la verificación por bytes descarta
    /// las colisiones del hash.
    fn canonical_slot_map(&mut self) -> &[u32] {
        if self.slot_canon_dirty {
            self.slot_canon_dirty = false;
            let mut por_hash: std::collections::HashMap<u64, Vec<u32>> =
                std::collections::HashMap::new();
            for slot in 0..POINT_LIGHT_SLOTS as u32 {
                let i = slot as usize;
                self.slot_canon[i] = slot;
                let Some(ubo) = self.slot_ubo[i] else {
                    continue;
                };
                let candidatas = por_hash.entry(self.slot_hash[i]).or_default();
                let mut canon = slot;
                for &c in candidatas.iter() {
                    let igual = self.slot_ubo[c as usize]
                        .map(|u| bytemuck::bytes_of(&u) == bytemuck::bytes_of(&ubo))
                        .unwrap_or(false);
                    if igual {
                        canon = c;
                        break;
                    }
                }
                if canon == slot {
                    candidatas.push(slot);
                }
                self.slot_canon[i] = canon;
            }
        }
        &self.slot_canon
    }

    /// Paso entre ranuras del buffer de luces, en bytes.
    pub fn point_lights_stride(&self) -> u64 {
        self.point_lights_stride
    }

    /// Publica la niebla del frame (Fase 2): color lineal, distancias de
    /// inicio y total en metros y si está activa. Viaja en el UBO de
    /// frame (set 2) junto a los parámetros de sombra.
    pub fn set_fog(&mut self, color: [f32; 3], near: f32, far: f32, enabled: bool) {
        self.fog_color_dist = [color[0], color[1], color[2], near];
        self.fog_far_mode = [far, if enabled { 1.0 } else { 0.0 }, 0.0, 0.0];
    }

    /// Publica el ambiente de escena del frame (G1): los tres colores
    /// lineales del gradient. Viaja en el UBO de frame (set 2) junto a
    /// la sombra y la niebla; la intensidad va en light_dir.w.
    pub fn set_ambient(&mut self, sky: [f32; 3], equator: [f32; 3], ground: [f32; 3]) {
        self.ambient_sky = [sky[0], sky[1], sky[2], 0.0];
        self.ambient_equator = [equator[0], equator[1], equator[2], 0.0];
        self.ambient_ground = [ground[0], ground[1], ground[2], 0.0];
    }


    /// Publica el post de escena del frame (G2): exposición,
    /// intensidad del bloom y viñeta resueltas por la app desde la
    /// escena activa (la app lo llama cada frame, como `set_ambient`).
    /// El umbral y la rodilla del bloom permanecen constantes del
    /// motor y no viajan por aquí.
    pub fn set_post(&mut self, settings: crate::post::PostSettings) {
        self.post_settings = settings;
    }

/// Publica las luces puntuales con sombra del frame (Fase 2), en el
    /// mismo orden de ranuras que el PointLightsUbo.
    pub fn set_point_shadow_lights(&mut self, lights: Vec<PointShadowLight>) {
        self.point_shadow_lights = lights;
    }

    /// Enlaza el set 3 en un dibujo cuyo pipeline lo declara (PBR; la
    /// piel lo enlaza dentro de `draw_skinned`). Los grafos y el
    /// Lambert no lo declaran y no lo reciben.
    unsafe fn bind_point_lights_set(&self, cmd: vk::CommandBuffer, pipe: &GraphicsPipeline, slot: u32) {
        if pipe.point_lights_set_layout.is_some() {
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipe.layout,
                3,
                &[self.point_lights_set],
                &[(slot as u64 * self.point_lights_stride) as u32],
            );
        }
    }


    /// Publica el lightmap horneado de la escena (bake, etapa 3b):
    /// sube el atlas, rellena el UBO de grupos y deja el set 4
    /// apuntando a él. `groups[i]` corresponde a `entities[i]`; la
    /// ranura de cada grupo es i*6 + eje*2 + signo. El atlas llega
    /// codificado en sRGB y la textura es SRGB: al muestrear se
    /// recupera el valor lineal horneado.
    pub fn set_lightmap(
        &mut self,
        queue: vk::Queue,
        rgba: &[u8],
        size: u32,
        entities: &[u64],
        groups: &[Vec<glass_core::lightmap::LmGroupRect>],
    ) -> Result<(), String> {
        let tex = crate::mesh::MeshLoader::upload_texture_to_gpu(
            &self.device,
            &mut self.allocator.lock().unwrap(),
            &crate::mesh::GltfImageData {
                width: size,
                height: size,
                rgba: rgba.to_vec(),
            },
            queue,
            self.command_pool,
        )?;
        let image_info = [vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(tex.image_view)
            .sampler(self.lm_sampler)];
        let write = [vk::WriteDescriptorSet::default()
            .dst_set(self.lm_runtime_tex_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_info)];
        unsafe { self.device.update_descriptor_sets(&write, &[]) };
        if let Some(mut prev) = self.lm_atlas_texture.take() {
            prev.destroy(&self.device, &mut self.allocator.lock().unwrap());
        }
        self.lm_atlas_texture = Some(tex);
        self.lm_tex_set = self.lm_runtime_tex_set;
        self.lm_atlas_size = size as f32;
        self.lm_index.clear();
        let mut data = LmDataUbo::default();
        let fsize = size as f32;
        for (i, gs) in groups.iter().enumerate().take(MAX_LM_ENTITIES) {
            if let Some(&e) = entities.get(i) {
                self.lm_index.insert(e, i as u32);
            }
            for g in gs {
                let slot = i * 6 + g.axis as usize * 2 + g.negative as usize;
                data.rect[slot] = [
                    g.x as f32 / fsize,
                    g.y as f32 / fsize,
                    g.w as f32 / fsize,
                    g.h as f32 / fsize,
                ];
                data.bounds[slot] =
                    [g.min_u, g.min_v, g.max_u - g.min_u, g.max_v - g.min_v];
            }
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytemuck::bytes_of(&data).as_ptr(),
                self.lm_data_mapped,
                std::mem::size_of::<LmDataUbo>(),
            );
        }
        Ok(())
    }

    /// Retira el lightmap publicado: el set 4 vuelve a la textura
    /// 1×1, el UBO se pone a ceros y el atlas se destruye.
    pub fn clear_lightmap(&mut self) {
        self.lm_tex_set = self.lm_default_tex_set;
        if let Some(mut prev) = self.lm_atlas_texture.take() {
            prev.destroy(&self.device, &mut self.allocator.lock().unwrap());
        }
        self.lm_index.clear();
        self.lm_atlas_size = 1.0;
        unsafe {
            std::ptr::write_bytes(
                self.lm_data_mapped,
                0,
                std::mem::size_of::<LmDataUbo>(),
            );
        }
    }

    /// Publica un cubemap de entorno nuevo (IBL): crea la imagen,
    /// reescribe el binding 0 del set 6 y destruye el cubo anterior.
    /// `rgba16f` va empaquetado nivel a nivel, 6 caras por nivel
    /// (ver `create_ibl_cube`).
    pub fn set_ibl_cubemap(
        &mut self,
        queue: vk::Queue,
        size: u32,
        mip_levels: u32,
        rgba16f: &[u8],
    ) -> Result<(), String> {
        let new_cube = {
            let mut allocator = self.allocator.lock().unwrap();
            create_ibl_cube(
                &self.device,
                &mut allocator,
                queue,
                self.command_pool,
                size,
                mip_levels,
                rgba16f,
            )?
        };

        let image_info = [vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(new_cube.image_view)
            .sampler(self.ibl_sampler)];
        let write = [vk::WriteDescriptorSet::default()
            .dst_set(self.ibl_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_info)];
        unsafe {
            self.device.update_descriptor_sets(&write, &[]);
        }

        if let Some(mut old_cube) = self.ibl_cube_texture.take() {
            old_cube.destroy(&self.device, &mut self.allocator.lock().unwrap());
        }

        self.ibl_cube_texture = Some(new_cube);

        Ok(())
    }

    /// Fija los parámetros del entorno (set 6, binding 1): intensidad,
    /// mip máximo del cubemap y si hay entorno válido. Sin entorno, el
    /// shader conserva el ambiente hemisférico de siempre.
    pub fn set_ibl_params(&mut self, intensity: f32, max_mip: f32, available: bool) {
        let ubo = IblParamsUbo {
            params: [intensity, max_mip, if available { 1.0 } else { 0.0 }, 0.0],
        };
        let bytes = bytemuck::bytes_of(&ubo);
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ibl_params_mapped, bytes.len());
        }
    }

    /// Enlaza el set 6 (IBL) si el pipeline lo declara.
    unsafe fn bind_ibl_set(&self, cmd: vk::CommandBuffer, pipe: &GraphicsPipeline) {
        if pipe.ibl_set_layout.is_some() {
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipe.layout,
                6,
                &[self.ibl_set],
                &[],
            );
        }
    }

    /// Push `lm_params` para una entidad: [índice, tamaño del
    /// atlas, 0, 0] si está horneada; [-1, 0, 0, 0] si no.
    pub fn lm_params_for(&self, entity: u64) -> [f32; 4] {
        match self.lm_index.get(&entity) {
            Some(&idx) => [idx as f32, self.lm_atlas_size, 0.0, 0.0],
            None => [-1.0, 0.0, 0.0, 0.0],
        }
    }

    /// Enlaza los sets 4 y 5 en un dibujo cuyo pipeline los declara
    /// (PBR y piel; los demás conservan su layout sin ellos).
    unsafe fn bind_lightmap_sets(&self, cmd: vk::CommandBuffer, pipe: &GraphicsPipeline) {
        if pipe.lm_set_layouts.is_some() {
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipe.layout,
                4,
                &[self.lm_tex_set],
                &[],
            );
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipe.layout,
                5,
                &[self.lm_data_set],
                &[],
            );
        }
    }

    /// Registra un material de grafo bajo `name`: su fragmento y, si el
    /// grafo declara etapa de vértices (`vert_spv`), también su vertex
    /// shader de desplazamiento. Sin vértice se usa el estándar PBR, así
    /// que los grafos sin sub-red de vértices se comportan como siempre.
    /// El layout es el del PBR MÁS el set 1 con el UBO global de los
    /// grafos (`GraphGlobals`): es lo único que distingue a estos
    /// pipelines de los de malla.
    ///
    /// Con `transparent` el pipeline mezcla el alfa del fragmento y no
    /// escribe profundidad (ver `GraphicsPipeline::new_graph_pipeline`);
    /// el grafo lo declara en su JSON y el cargador de materiales lo
    /// sabe por el marcador hermano del .spv.
    pub fn register_graph_pipeline(
        &mut self,
        name: &str,
        vert_spv: Option<&[u8]>,
        frag_spv: &[u8],
        transparent: bool,
    ) -> Result<(), RendererError> {
        let pipeline = GraphicsPipeline::new_graph_pipeline(
            self.device.clone(),
            self.offscreen.render_pass,
            vert_spv.unwrap_or(PBR_VERT_SPV),
            frag_spv,
            self.graph_globals_layout,
            transparent,
            MeshPipelineLayouts { shadow: Some(self.shadow.dummy_layout), point_lights: Some(self.point_lights_layout), ..Default::default() },
        )?;
        let graph_set0 = pipeline.descriptor_set_layout;
        self.custom_pipelines.insert(name.to_string(), pipeline);
        // Gemelo contra el render pass LDR para la ruta sin post
        // (mismos SPV: los grafos no mapean tono en ninguna ruta).
        let pipeline_legacy = GraphicsPipeline::new_graph_pipeline(
            self.device.clone(),
            self.final_offscreen.render_pass,
            vert_spv.unwrap_or(PBR_VERT_SPV),
            frag_spv,
            self.graph_globals_layout,
            transparent,
            MeshPipelineLayouts { shadow: Some(self.shadow.dummy_layout), point_lights: Some(self.point_lights_layout), ..Default::default() },
        )?;
        self.custom_pipelines_legacy.insert(name.to_string(), pipeline_legacy);
        // Pipelines de profundidad del grafo (Fase 2S): si el grafo
        // declara el pase Shadow, su sombra del sol se proyecta con su
        // etapa de vértices (desplazamiento incluido); un grafo sin
        // etapa propia usa el vértice PBR estándar, como en el pase
        // hacia adelante. Con la propiedad especial `alpha_clip`
        // (Float) se crea además la variante con recorte alfa.
        {
            let vert = vert_spv.unwrap_or(PBR_VERT_SPV);
            let pases = glass_core::shader_graph::graph_passes(name)
                .unwrap_or_else(|| vec![glass_core::shader_graph::PassKind::Forward]);
            if pases.contains(&glass_core::shader_graph::PassKind::Shadow) {
                if let Some(set0) = graph_set0 {
                    match GraphicsPipeline::new_depth_pipeline_graph(
                        self.device.clone(),
                        self.shadow.render_pass,
                        vert,
                        None,
                        set0,
                        self.graph_globals_layout,
                    ) {
                        Ok(p) => {
                            self.graph_depth_pipelines.insert(name.to_string(), p);
                        }
                        Err(e) => log::warn!(
                            "Renderer: fallo creando la profundidad de sombra del grafo '{}': {}",
                            name, e
                        ),
                    }
                    let con_clip = glass_core::shader_graph::graph_schema(name)
                        .map(|props| {
                            props.iter().any(|p| {
                                p.name == "alpha_clip"
                                    && matches!(p.kind, glass_core::shader_graph::PropertyKind::Float)
                            })
                        })
                        .unwrap_or(false);
                    if con_clip {
                        match GraphicsPipeline::new_depth_pipeline_graph(
                            self.device.clone(),
                            self.shadow.render_pass,
                            vert,
                            Some(DEPTH_CLIP_FRAG_SPV),
                            set0,
                            self.graph_globals_layout,
                        ) {
                            Ok(p) => {
                                self.graph_depth_clip_pipelines.insert(name.to_string(), p);
                            }
                            Err(e) => log::warn!(
                                "Renderer: fallo creando la profundidad con recorte del grafo '{}': {}",
                                name, e
                            ),
                        }
                    }
                }
            }
        }
        // Pipelines de contorno (Fase 2S, S5): si el grafo declara el
        // pase Outline, su gemelo de contorno en ambas rutas (HDR y
        // LDR). La app los pide con el nombre "outline:<grafo>".
        {
            let pases_o = glass_core::shader_graph::graph_passes(name)
                .unwrap_or_else(|| vec![glass_core::shader_graph::PassKind::Forward]);
            if pases_o.contains(&glass_core::shader_graph::PassKind::Outline) {
                match GraphicsPipeline::new_outline_pipeline(
                    self.device.clone(),
                    self.offscreen.render_pass,
                    OUTLINE_VERT_SPV,
                    OUTLINE_FRAG_SPV,
                ) {
                    Ok(p) => {
                        self.graph_outline_pipelines.insert(name.to_string(), p);
                    }
                    Err(e) => log::warn!(
                        "Renderer: fallo creando el contorno del grafo '{}': {}",
                        name, e
                    ),
                }
                match GraphicsPipeline::new_outline_pipeline(
                    self.device.clone(),
                    self.final_offscreen.render_pass,
                    OUTLINE_VERT_SPV,
                    OUTLINE_FRAG_SPV,
                ) {
                    Ok(p) => {
                        self.graph_outline_pipelines_legacy
                            .insert(name.to_string(), p);
                    }
                    Err(e) => log::warn!(
                        "Renderer: fallo creando el contorno LDR del grafo '{}': {}",
                        name, e
                    ),
                }
            }
        }
        log::info!(
            "Renderer: material de grafo '{}' registrado{}{}",
            name,
            if vert_spv.is_some() { " (con desplazamiento de vértices)" } else { "" },
            if transparent { " (transparente)" } else { "" }
        );
        Ok(())
    }

    /// Carga como materiales todos los `graph_<nombre>.frag.spv` de un
    /// directorio de shaders. Si junto a un fragmento existe su hermano
    /// `graph_<nombre>.vert.spv` (el grafo declara etapa de vértices), el
    /// pipeline se crea con ese vértice. Si existe el marcador
    /// `graph_<nombre>.transparent` (el grafo se declara transparente en
    /// su JSON), el pipeline se crea con mezcla alfa y sin escritura de
    /// profundidad. Un archivo que falle se registra y se sigue.
    /// Recarga en caliente de los materiales de grafo (auditoría C2):
    /// espera a que la GPU quede libre y vuelve a registrar todos los
    /// pipelines de grafo desde los .spv en disco, sustituyendo los
    /// anteriores. La invoca la app cuando el vigía detecta un
    /// `graph_*.spv` modificado.
    pub fn reload_graph_materials(&mut self, shaders_dir: &std::path::Path) {
        unsafe {
            let _ = self.device.device_wait_idle();
        }
        self.load_graph_materials(shaders_dir);
    }

    pub fn load_graph_materials(&mut self, shaders_dir: &std::path::Path) {        let entries = match std::fs::read_dir(shaders_dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if let Some(rest) = fname.strip_prefix("graph_") {
                if let Some(name) = rest.strip_suffix(".frag.spv") {
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            let vert_path = shaders_dir.join(format!("graph_{}.vert.spv", name));
                            // Solo «no existe» es el caso normal (grafo sin
                            // etapa de vértices); cualquier otro error de
                            // lectura se avisa, no se traga como ausencia.
                            let vert_bytes = match std::fs::read(&vert_path) {
                                Ok(b) => Some(b),
                                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                                Err(e) => {
                                    log::warn!(
                                        "Renderer: no se pudo leer el vértice del grafo '{}' ({:?}: {}); se usa el vértice estándar",
                                        name, vert_path, e
                                    );
                                    None
                                }
                            };
                            // El marcador de transparencia es un
                            // fichero (posiblemente vacío) junto al
                            // .spv, como el .vert.spv hermano: su mera
                            // presencia declara el pipeline transparente.
                            let transparent =
                                shaders_dir.join(format!("graph_{}.transparent", name)).exists();
                            if let Err(e) = self.register_graph_pipeline(
                                name,
                                vert_bytes.as_deref(),
                                &bytes,
                                transparent,
                            ) {
                                log::error!("Renderer: fallo registrando material de grafo '{}': {}", name, e);
                            }
                        }
                        Err(e) => log::error!("Renderer: no se pudo leer {:?}: {}", path, e),
                    }
                }
            }
        }
    }

    /// Enlaza el set 1 con el UBO global cuando el pipeline del lote es
    /// de grafo (su layout lo declara). Para el resto de pipelines no
    /// hace nada: ni el PBR ni el de piel conocen ese set. Se llama en
    /// cada dibujo, como el set 0 de textura: los layouts de grafo y PBR
    /// son handles distintos aunque el set 0 tenga el mismo contenido,
    /// así que ningún enlace se hereda entre pipelines.
    unsafe fn bind_graph_globals(
        &self,
        cmd: vk::CommandBuffer,
        pipe: &GraphicsPipeline,
        props_set: Option<vk::DescriptorSet>,
    ) {
        if pipe.globals_set_layout.is_some() {
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipe.layout,
                1,
                &[props_set.unwrap_or(self.graph_globals_set)],
                &[],
            );
        }
    }

    /// Crea un buffer y un set de propiedades de grafo para una
    /// instancia (CL-2S2a): el binding 0 comparte el buffer de
    /// `graph_globals` (el canal de interacción sigue siendo global),
    /// el binding 1 apunta a un buffer propio con `data`. `data` se
    /// copia tal cual; si es menor de 16 B el buffer se crea igual con
    /// 16 B (el mínimo de un UBO con sentido) y solo se copian los
    /// bytes de `data`. Devuelve `None` y loguea si falla la creación
    /// del buffer, la asignación de memoria o el mapeo.
    pub fn create_graph_props_set(&mut self, data: &[u8]) -> Option<vk::DescriptorSet> {
        let size = (data.len() as u64).max(16);
        unsafe {
            let buffer_info = vk::BufferCreateInfo::default()
                .size(size)
                .usage(vk::BufferUsageFlags::UNIFORM_BUFFER)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);
            let buffer = match self.device.create_buffer(&buffer_info, None) {
                Ok(b) => b,
                Err(e) => {
                    log::error!("graph_props: fallo al crear buffer: {e:?}");
                    return None;
                }
            };
            let mem_reqs = self.device.get_buffer_memory_requirements(buffer);
            let allocation = match self.allocator.lock().unwrap().allocate(&AllocationCreateDesc {
                name: "graph_props_instance",
                requirements: mem_reqs,
                location: MemoryLocation::CpuToGpu,
                linear: true,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            }) {
                Ok(a) => a,
                Err(e) => {
                    log::error!("graph_props: fallo al asignar memoria: {e:?}");
                    self.device.destroy_buffer(buffer, None);
                    return None;
                }
            };
            if let Err(e) = self
                .device
                .bind_buffer_memory(buffer, allocation.memory(), allocation.offset())
            {
                log::error!("graph_props: fallo al enlazar memoria: {e:?}");
                self.device.destroy_buffer(buffer, None);
                return None;
            }
            let mapped = match allocation.mapped_ptr() {
                Some(p) => p.as_ptr() as *mut u8,
                None => {
                    log::error!("graph_props: el buffer de instancia no quedó mapeado");
                    self.device.destroy_buffer(buffer, None);
                    return None;
                }
            };
            std::ptr::copy_nonoverlapping(data.as_ptr(), mapped, data.len());

            let layouts = [self.graph_globals_layout];
            let set_alloc_info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(self.graph_props_pool)
                .set_layouts(&layouts);
            let set = match self.device.allocate_descriptor_sets(&set_alloc_info) {
                Ok(mut sets) => match sets.pop() {
                    Some(s) => s,
                    None => {
                        log::error!("graph_props: pool de propiedades sin sets");
                        self.device.destroy_buffer(buffer, None);
                        return None;
                    }
                },
                Err(e) => {
                    log::error!("graph_props: fallo al asignar set: {e:?}");
                    self.device.destroy_buffer(buffer, None);
                    return None;
                }
            };
            let globals_desc = [vk::DescriptorBufferInfo::default()
                .buffer(self.graph_globals_buffer)
                .offset(0)
                .range(std::mem::size_of::<[f32; 8]>() as u64)];
            let props_desc = [vk::DescriptorBufferInfo::default()
                .buffer(buffer)
                .offset(0)
                .range(size)];
            let writes = [
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(&globals_desc),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(&props_desc),
            ];
            self.device.update_descriptor_sets(&writes, &[]);

            self.graph_props_sets.push(GraphPropsEntry {
                set,
                buffer,
                allocation: Some(allocation),
                mapped,
                size,
            });
            Some(set)
        }
    }

    /// Actualiza los datos de un set creado por `create_graph_props_set`.
    /// Copia hasta `size` bytes de `data` (los que sobren se descartan);
    /// no hace nada si `set` no corresponde a ninguna entrada viva.
    pub fn update_graph_props_set(&mut self, set: vk::DescriptorSet, data: &[u8]) {
        if let Some(entry) = self.graph_props_sets.iter().find(|e| e.set == set) {
            let len = data.len().min(entry.size as usize);
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), entry.mapped, len);
            }
        } else {
            log::warn!("graph_props: update sobre un set desconocido");
        }
    }

    /// El pipeline de un lote: el del grafo si lo pide y existe; si
    /// no, PBR. Con `legacy` (post desactivado) se usan los gemelos
    /// creados contra el render pass LDR.
    fn pipeline_for(&self, shader_name: &Option<String>, legacy: bool) -> &GraphicsPipeline {
        match shader_name {
            Some(name) => {
                if name == "__contorno_seleccion" {
                    return if legacy {
                        &self.selection_outline_pipeline_legacy
                    } else {
                        &self.selection_outline_pipeline
                    };
                }
                if let Some(grafo) = name.strip_prefix("outline:") {
                    let mapa = if legacy {
                        &self.graph_outline_pipelines_legacy
                    } else {
                        &self.graph_outline_pipelines
                    };
                    if let Some(p) = mapa.get(grafo) {
                        return p;
                    }
                }
                if legacy {
                    self.custom_pipelines_legacy.get(name).unwrap_or(&self.pbr_pipeline_legacy)
                } else {
                    self.custom_pipelines.get(name).unwrap_or(&self.pbr_pipeline)
                }
            }
            None => {
                if legacy { &self.pbr_pipeline_legacy } else { &self.pbr_pipeline }
            }
        }
    }

    /// Recrea el Swapchain y sus framebuffers cuando la ventana nativa cambia de tamaño.
    pub fn resize(
        &mut self,
        context: &VulkanContext,
        width: u32,
        height: u32,
    ) -> Result<(), RendererError> {
        if width == 0 || height == 0 || (width == self.swapchain.extent.width && height == self.swapchain.extent.height) {
            return Ok(());
        }
        unsafe {
            self.device.device_wait_idle()?;
            for fb in self.swapchain_framebuffers.drain(..) {
                self.device.destroy_framebuffer(fb, None);
            }
            for semaphore in self.render_finished_semaphores.drain(..) {
                self.device.destroy_semaphore(semaphore, None);
            }
        }
        let old_swapchain = self.swapchain.swapchain;
        self.swapchain = SwapchainContext::new(
            &context.instance,
            self.device.clone(),
            crate::swapchain::SwapchainTarget {
                surface_loader: &context.surface_loader,
                surface: context.surface,
                physical_device: context.physical_device,
                width,
                height,
                old_swapchain: Some(old_swapchain),
            },
        )?;
        let semaphore_info = vk::SemaphoreCreateInfo::default();
        self.render_finished_semaphores = (0..self.swapchain.images.len())
            .map(|_| unsafe { self.device.create_semaphore(&semaphore_info, None) })
            .collect::<Result<Vec<_>, _>>()?;
        self.swapchain_framebuffers = create_swapchain_framebuffers(
            &self.device,
            self.swapchain_render_pass,
            &self.swapchain.image_views,
            self.swapchain.extent,
        )?;
        log::debug!(
            "Swapchain redimensionado exitosamente a {}x{}",
            width,
            height
        );
        Ok(())
    }

    /// Crea un DescriptorSet para la textura offscreen compatible con egui_ash_renderer (Scene View).
    pub fn create_offscreen_descriptor_set(
        &self,
    ) -> Result<(vk::DescriptorSetLayout, vk::DescriptorPool, vk::DescriptorSet), RendererError> {
        let layout = egui_ash_renderer::vulkan::create_vulkan_descriptor_set_layout(&self.device)
            .map_err(|e| RendererError::Descriptor(e.to_string()))?;
        let pool = egui_ash_renderer::vulkan::create_vulkan_descriptor_pool(&self.device, 16)
            .map_err(|e| RendererError::Descriptor(e.to_string()))?;
        let set = egui_ash_renderer::vulkan::create_vulkan_descriptor_set(
            &self.device,
            layout,
            pool,
            self.final_offscreen.color_view,
            self.final_offscreen.sampler,
        )
        .map_err(|e| RendererError::Descriptor(e.to_string()))?;

        Ok((layout, pool, set))
    }

    /// Crea un DescriptorSet para la textura de juego compatible con egui_ash_renderer (Game View).
    pub fn create_game_offscreen_descriptor_set(
        &self,
    ) -> Result<(vk::DescriptorSetLayout, vk::DescriptorPool, vk::DescriptorSet), RendererError> {
        let layout = egui_ash_renderer::vulkan::create_vulkan_descriptor_set_layout(&self.device)
            .map_err(|e| RendererError::Descriptor(e.to_string()))?;
        let pool = egui_ash_renderer::vulkan::create_vulkan_descriptor_pool(&self.device, 16)
            .map_err(|e| RendererError::Descriptor(e.to_string()))?;
        let set = egui_ash_renderer::vulkan::create_vulkan_descriptor_set(
            &self.device,
            layout,
            pool,
            self.final_game_offscreen.color_view,
            self.final_game_offscreen.sampler,
        )
        .map_err(|e| RendererError::Descriptor(e.to_string()))?;

        Ok((layout, pool, set))
    }

    /// Renderiza ambas vistas (Scene View y Game View) de manera desacoplada e independiente en un solo comando de GPU.
    /// Vuelca los UBO del frame (globales de grafo, luces
    /// puntuales y sombras) a la memoria mapeada, con la GPU en
    /// reposo tras el fence. Devuelve los parámetros de sombra
    /// efectivos (con GLASS_SHADOWS=0 ya aplicados). Lo usan
    /// `render_views` y `render_portal_views`.
    fn upload_frame_ubos(&mut self) -> [f32; 4] {
        unsafe {
        // Canal global de los grafos: con la GPU ya en reposo tras el
        // fence, los dos vec4 del frame se vuelcan al UBO mapeado (el
        // mismo patrón de sincronización que los JointBuffer).
        std::ptr::copy_nonoverlapping(
            self.graph_globals.as_ptr() as *const u8,
            self.graph_globals_mapped,
            std::mem::size_of::<[f32; 8]>(),
        );

        // UBO de luces puntuales del frame (mismo patrón y momento).
        let pl_bytes = bytemuck::bytes_of(&self.point_lights);
        std::ptr::copy_nonoverlapping(
            pl_bytes.as_ptr(),
            self.point_lights_mapped,
            pl_bytes.len(),
        );
        self.registrar_ranura_luz(0, self.point_lights);

        // Ranuras por objeto pendientes de volcar.
        let pending = std::mem::take(&mut self.object_lights_pending);
        for (slot, ubo) in pending {
            let bytes = bytemuck::bytes_of(&ubo);
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.point_lights_mapped
                    .add((slot as u64 * self.point_lights_stride) as usize),
                bytes.len(),
            );
            self.registrar_ranura_luz(slot, ubo);
        }

        // UBO de sombras del frame (mismo patrón): la VP fijada por
        // la app y los parámetros, con el flag de actividad
        // combinado con el del renderer (GLASS_SHADOWS=0 lo apaga
        // aunque la app lo publique activo).
        let mut shadow_params = self.shadow_params;
        if !self.shadows_enabled {
            shadow_params[3] = 0.0;
        }
        self.shadow.write_ubo(&crate::shadow::ShadowUbo {
            light_vp: self.shadow_vp,
            params: shadow_params,
            fog_color_dist: self.fog_color_dist,
            fog_far_mode: self.fog_far_mode,
            ambient_sky: self.ambient_sky,
            ambient_equator: self.ambient_equator,
            ambient_ground: self.ambient_ground,
        });
            shadow_params
        }
    }


    /// Pase de profundidad del mapa de sombras (proyectores
    /// estáticos y con piel). Extraído de `render_views`.
    fn record_shadow_pass(
        &self,
        cmd: vk::CommandBuffer,
        casters: &ShadowCasters,
        shadow_params: [f32; 4],
    ) {
        unsafe {
        if shadow_params[3] > 0.5
            && (!casters.statics.is_empty()
                || !casters.skinned.is_empty()
                || !casters.graph_statics.is_empty())
        {
                self.shadow.begin_pass(&self.device, cmd);
                if !casters.statics.is_empty() {
                    self.device.cmd_bind_pipeline(
                        cmd,
                        vk::PipelineBindPoint::GRAPHICS,
                        self.shadow.depth_pipeline,
                    );
                    for (mesh, model, rangos) in casters.statics {
                        let pc = MeshPushConstants {
                            mvp: glass_core::mat4_mul(self.shadow_vp, *model),
                            model: *model,
                            ..Default::default()
                        };
                        self.device.cmd_push_constants(
                            cmd,
                            self.shadow.depth_layout,
                            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                            0,
                            bytemuck::bytes_of(&pc),
                        );
                        self.device
                            .cmd_bind_vertex_buffers(cmd, 0, &[mesh.vertex_buffer], &[0]);
                        self.device
                            .cmd_bind_index_buffer(cmd, mesh.index_buffer, 0, vk::IndexType::UINT32);
                        match rangos {
                            Some(rs) => {
                                for (first, count) in rs {
                                    self.device.cmd_draw_indexed(cmd, *count, 1, *first, 0, 0);
                                }
                            }
                            None => {
                                self.device.cmd_draw_indexed(cmd, mesh.index_count, 1, 0, 0, 0);
                            }
                        }
                    }
                }
                if !casters.skinned.is_empty() {
                    self.device.cmd_bind_pipeline(
                        cmd,
                        vk::PipelineBindPoint::GRAPHICS,
                        self.shadow.depth_skinned_pipeline,
                    );
                    for (mesh, model, joints_set) in casters.skinned {
                        self.device.cmd_bind_descriptor_sets(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            self.shadow.depth_skinned_layout,
                            1,
                            &[*joints_set],
                            &[],
                        );
                        let pc = MeshPushConstants {
                            mvp: glass_core::mat4_mul(self.shadow_vp, *model),
                            model: *model,
                            ..Default::default()
                        };
                        self.device.cmd_push_constants(
                            cmd,
                            self.shadow.depth_skinned_layout,
                            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                            0,
                            bytemuck::bytes_of(&pc),
                        );
                        self.device
                            .cmd_bind_vertex_buffers(cmd, 0, &[mesh.vertex_buffer], &[0]);
                        self.device
                            .cmd_bind_index_buffer(cmd, mesh.index_buffer, 0, vk::IndexType::UINT32);
                        self.device.cmd_draw_indexed(cmd, mesh.index_count, 1, 0, 0, 0);
                    }
                }
                if !casters.graph_statics.is_empty() {
                    // Proyectores de grafo (Fase 2S): profundidad con la
                    // etapa de vértices del grafo; la variante con
                    // recorte si la entidad trae umbral `alpha_clip` y
                    // el grafo tiene ese pipeline.
                    let mut bound_pipe: vk::Pipeline = vk::Pipeline::null();
                    for (mesh, model, graph, tex_set, alpha_clip, rangos) in casters.graph_statics {
                        let elegido = match alpha_clip {
                            Some(_) => self
                                .graph_depth_clip_pipelines
                                .get(graph)
                                .or_else(|| self.graph_depth_pipelines.get(graph)),
                            None => self.graph_depth_pipelines.get(graph),
                        };
                        let Some((pipe, layout)) = elegido else { continue };
                        if bound_pipe != *pipe {
                            self.device.cmd_bind_pipeline(
                                cmd,
                                vk::PipelineBindPoint::GRAPHICS,
                                *pipe,
                            );
                            bound_pipe = *pipe;
                        }
                        self.device.cmd_bind_descriptor_sets(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            *layout,
                            1,
                            &[self.graph_globals_set],
                            &[],
                        );
                        if alpha_clip.is_some() {
                            self.device.cmd_bind_descriptor_sets(
                                cmd,
                                vk::PipelineBindPoint::GRAPHICS,
                                *layout,
                                0,
                                &[*tex_set],
                                &[],
                            );
                        }
                        let pc = MeshPushConstants {
                            mvp: glass_core::mat4_mul(self.shadow_vp, *model),
                            model: *model,
                            material_params: [alpha_clip.unwrap_or(0.0), 0.0, 0.0, 0.0],
                            ..Default::default()
                        };
                        self.device.cmd_push_constants(
                            cmd,
                            *layout,
                            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                            0,
                            bytemuck::bytes_of(&pc),
                        );
                        self.device
                            .cmd_bind_vertex_buffers(cmd, 0, &[mesh.vertex_buffer], &[0]);
                        self.device
                            .cmd_bind_index_buffer(cmd, mesh.index_buffer, 0, vk::IndexType::UINT32);
                        // Pasada de medios (resto de C3): también los
                        // proyectores de grafo respetan el nivel LOD
                        // activo si la app pasa rangos.
                        match rangos {
                            Some(rs) => {
                                for (first, count) in rs {
                                    self.device.cmd_draw_indexed(cmd, *count, 1, *first, 0, 0);
                                }
                            }
                            None => {
                                self.device.cmd_draw_indexed(cmd, mesh.index_count, 1, 0, 0, 0);
                            }
                        }
                    }
                }
                self.device.cmd_end_render_pass(cmd);
            }
        }
    }

    /// Pases de profundidad de los cubos de las luces puntuales con
    /// sombra (Fase 2): por cada luz publicada (máx. 4) se renderizan
    /// las 6 caras de su cubo del atlas con los mismos pipelines de
    /// profundidad del sol y los proyectores del frame. Sin luces con
    /// sombra o sin proyectores no se graba nada.
    fn record_point_shadow_passes(&self, cmd: vk::CommandBuffer, casters: &ShadowCasters) {
        if self.point_shadow_lights.is_empty()
            || (casters.statics.is_empty() && casters.skinned.is_empty())
        {
            return;
        }
        unsafe {
            for (slot, luz) in self.point_shadow_lights.iter().enumerate() {
                let vps = crate::shadow::point_shadow_face_vps(luz.pos, 0.1, luz.far);
                for (cara, vp) in vps.iter().enumerate() {
                    self.point_shadows.begin_face_pass(&self.device, cmd, slot * 6 + cara);
                    if !casters.statics.is_empty() {
                        self.device.cmd_bind_pipeline(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            self.shadow.depth_pipeline,
                        );
                        for (mesh, model, rangos) in casters.statics {
                            let pc = MeshPushConstants {
                                mvp: glass_core::mat4_mul(*vp, *model),
                                model: *model,
                                ..Default::default()
                            };
                            self.device.cmd_push_constants(
                                cmd,
                                self.shadow.depth_layout,
                                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                                0,
                                bytemuck::bytes_of(&pc),
                            );
                            self.device
                                .cmd_bind_vertex_buffers(cmd, 0, &[mesh.vertex_buffer], &[0]);
                            self.device
                                .cmd_bind_index_buffer(cmd, mesh.index_buffer, 0, vk::IndexType::UINT32);
                            match rangos {
                                Some(rs) => {
                                    for (first, count) in rs {
                                        self.device.cmd_draw_indexed(cmd, *count, 1, *first, 0, 0);
                                    }
                                }
                                None => {
                                    self.device.cmd_draw_indexed(cmd, mesh.index_count, 1, 0, 0, 0);
                                }
                            }
                        }
                    }
                    if !casters.skinned.is_empty() {
                        self.device.cmd_bind_pipeline(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            self.shadow.depth_skinned_pipeline,
                        );
                        for (mesh, model, joints_set) in casters.skinned {
                            self.device.cmd_bind_descriptor_sets(
                                cmd,
                                vk::PipelineBindPoint::GRAPHICS,
                                self.shadow.depth_skinned_layout,
                                1,
                                &[*joints_set],
                                &[],
                            );
                            let pc = MeshPushConstants {
                                mvp: glass_core::mat4_mul(*vp, *model),
                                model: *model,
                                ..Default::default()
                            };
                            self.device.cmd_push_constants(
                                cmd,
                                self.shadow.depth_skinned_layout,
                                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                                0,
                                bytemuck::bytes_of(&pc),
                            );
                            self.device
                                .cmd_bind_vertex_buffers(cmd, 0, &[mesh.vertex_buffer], &[0]);
                            self.device
                                .cmd_bind_index_buffer(cmd, mesh.index_buffer, 0, vk::IndexType::UINT32);
                            self.device.cmd_draw_indexed(cmd, mesh.index_count, 1, 0, 0, 0);
                        }
                    }
                    self.device.cmd_end_render_pass(cmd);
                }
            }
        }
    }

    /// Sube la geometría de las líneas del editor (la rejilla de la
    /// vista de escena) y su view-proj. Espejo de `set_particle_frame`
    /// sin índices: buffer mapeado que crece por duplicación; la
    /// escritura es segura porque la app llama entre frames, con el
    /// fence del frame anterior ya esperado. Con `verts` vacío el
    /// contador queda a cero y el pase no dibuja nada.
    pub fn set_editor_grid_lines(&mut self, vp: [f32; 16], verts: &[EditorLineVertex]) {
        self.editor_grid_vp = vp;
        let device = self.device.clone();
        let allocator = self.allocator.clone();
        let buffers = &mut self.editor_line_buffers;
        // El contador solo se publica al final, tras la copia: si algo
        // falla a mitad, la rejilla no se dibuja este frame y el
        // crecimiento se reintenta en el siguiente.
        buffers.vertex_count = 0;
        if verts.is_empty() {
            return;
        }
        unsafe {
            if verts.len() > buffers.vertex_capacity {
                // Crecer: crear el buffer nuevo en variables locales y
                // confirmar el intercambio solo cuando está listo.
                let nueva_capacidad = verts.len().next_power_of_two().max(1024);
                let size = (nueva_capacidad * std::mem::size_of::<EditorLineVertex>()) as u64;
                let info = vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(vk::BufferUsageFlags::VERTEX_BUFFER)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE);
                let mut nuevo: Option<(vk::Buffer, Allocation)> = None;
                if let Ok(buffer) = device.create_buffer(&info, None) {
                    let mem_reqs = device.get_buffer_memory_requirements(buffer);
                    let allocation = allocator.lock().unwrap().allocate(&AllocationCreateDesc {
                        name: "editor_line_vertices",
                        requirements: mem_reqs,
                        location: MemoryLocation::CpuToGpu,
                        linear: true,
                        allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                    });
                    match allocation {
                        Ok(allocation)
                            if device
                                .bind_buffer_memory(
                                    buffer,
                                    allocation.memory(),
                                    allocation.offset(),
                                )
                                .is_ok() =>
                        {
                            nuevo = Some((buffer, allocation));
                        }
                        Ok(allocation) => {
                            let _ = allocator.lock().unwrap().free(allocation);
                            device.destroy_buffer(buffer, None);
                        }
                        Err(_) => {
                            device.destroy_buffer(buffer, None);
                        }
                    }
                }
                match nuevo {
                    Some((buffer, allocation)) => {
                        if buffers.vertex_buffer != vk::Buffer::null() {
                            device.destroy_buffer(buffers.vertex_buffer, None);
                        }
                        if let Some(a) = buffers.vertex_allocation.take() {
                            let _ = allocator.lock().unwrap().free(a);
                        }
                        buffers.vertex_buffer = buffer;
                        buffers.vertex_allocation = Some(allocation);
                        buffers.vertex_capacity = nueva_capacidad;
                    }
                    None => return,
                }
            }
            if let Some(alloc) = &buffers.vertex_allocation {
                if let Some(ptr) = alloc.mapped_ptr() {
                    std::ptr::copy_nonoverlapping(
                        verts.as_ptr() as *const u8,
                        ptr.as_ptr() as *mut u8,
                        std::mem::size_of_val(verts),
                    );
                    buffers.vertex_count = verts.len() as u32;
                }
            }
        }
    }

    /// Graba UNA vista de la escena en un objetivo (Scene, Game o
    /// una vista de portal): pase de color+profundidad con las
    /// mallas, las superficies de portal y la piel. Extraído de
    /// `render_views` (bloque Portales) para no triplicar el
    /// bucle de dibujo; el comportamiento de Scene/Game es el de
    /// siempre.
    #[allow(clippy::too_many_arguments)]
    fn record_view_pass(
        &self,
        cmd: vk::CommandBuffer,
        target: &OffscreenTarget,
        // `_clear` y `_fallback_triangle` quedan de la extracción del bloque
        // Portales: el pase usa hoy el clear fijo y dibuja el triángulo de
        // respaldo siempre que no haya mallas; se conservan en la firma para
        // no tocar a los llamadores en esta fase.
        _clear: [f32; 4],
        meshes: &[MeshDrawItem],
        skinned: &[SkinnedDrawItem],
        // Contorno de selección con piel (solo lo usa la vista de Escena).
        outline_skinned: &[SkinnedDrawItem],
        portal_draws: &[PortalDrawItem],
        _legacy: bool,
        _fallback_triangle: bool,
        particle_vp: Option<([f32; 16], bool)>,
        es_escena: bool,
        // Plan de instanciado de la vista (render instanciado): los
        // ítems agrupados se saltan en el bucle por ítem y se dibujan
        // tras él, una draw por grupo. None = todo por ítem.
        instancing: Option<&ViewInstancing>,
    ) {
        unsafe {
            // Con post: el 3D va al objetivo HDR y los pipelines son
            // los de siempre; sin post: directo al LDR con los
            // gemelos legacy (ACES dentro del PBR).
            let legacy = !self.post_enabled;
            let target = if legacy { &self.final_offscreen } else { target };
            let clear_values = [
                vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: [0.06, 0.09, 0.14, 1.0],
                    },
                },
                vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: 1.0,
                        stencil: 0,
                    },
                },
            ];

            let render_pass_begin_info = vk::RenderPassBeginInfo::default()
                .render_pass(target.render_pass)
                .framebuffer(target.framebuffer)
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: target.extent,
                })
                .clear_values(&clear_values);

            self.device.cmd_begin_render_pass(
                cmd,
                &render_pass_begin_info,
                vk::SubpassContents::INLINE,
            );

            let viewport = [vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: target.extent.width as f32,
                height: target.extent.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            }];
            self.device.cmd_set_viewport(cmd, 0, &viewport);

            let scissor = [vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: target.extent,
            }];
            self.device.cmd_set_scissor(cmd, 0, &scissor);

            if !meshes.is_empty() || !skinned.is_empty() || !portal_draws.is_empty() {
                // El pipeline se enlaza por lote: PBR por defecto o el
                // material de grafo que pida la entidad.
                let mut bound: *const GraphicsPipeline = std::ptr::null();
                for (idx, (mesh, push_consts, tex_set, shader_name, range, props_set, light_slot)) in
                    meshes.iter().enumerate()
                {
                    // Ítem agrupado: lo dibuja el bloque instanciado
                    // tras este bucle, con su grupo.
                    if let Some(plan) = instancing {
                        if plan.grouped[idx] {
                            continue;
                        }
                    }
                    let pipe = self.pipeline_for(shader_name, legacy);
                    if !std::ptr::eq(bound, pipe) {
                        self.device.cmd_bind_pipeline(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            pipe.pipeline,
                        );
                        bound = pipe;
                        self.frame_pipeline_binds
                            .set(self.frame_pipeline_binds.get() + 1);
                    }
                    // El set 0 solo se enlaza si el pipeline del lote lo
                    // declara: los pipelines de contorno (selección y
                    // toon) tienen layout SIN sets — su shader solo lee
                    // push constants — y enlazarles el set de texturas
                    // del modelo es UB que mata el driver (Bug I:
                    // 0xc0000005 al soltar un modelo y dibujar su
                    // contorno de selección). Es la misma guarda que
                    // ya usan los sets 2/3/4/5/6 en este bucle.
                    if *tex_set != vk::DescriptorSet::null() && pipe.descriptor_set_layout.is_some() {
                        self.device.cmd_bind_descriptor_sets(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            pipe.layout,
                            0,
                            &[*tex_set],
                            &[],
                        );
                    }
                    self.bind_graph_globals(cmd, pipe, *props_set);
                    self.bind_shadow_set(cmd, pipe);
                    self.bind_point_lights_set(cmd, pipe, *light_slot);
                    self.bind_lightmap_sets(cmd, pipe);
            self.bind_ibl_set(cmd, pipe);
                    {
                        let mut desc = 0u32;
                        if *tex_set != vk::DescriptorSet::null() && pipe.descriptor_set_layout.is_some() {
                            desc += 1;
                        }
                        if pipe.globals_set_layout.is_some() {
                            desc += 1;
                        }
                        if pipe.shadow_set_layout.is_some() {
                            desc += 1;
                        }
                        if pipe.point_lights_set_layout.is_some() {
                            desc += 1;
                        }
                        if pipe.lm_set_layouts.is_some() {
                            desc += 2;
                        }
                        self.frame_descriptor_binds
                            .set(self.frame_descriptor_binds.get() + desc);
                    }
                    let push_bytes = bytemuck::bytes_of(push_consts);
                    self.device.cmd_push_constants(
                        cmd,
                        pipe.layout,
                        vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                        0,
                        push_bytes,
                    );

                    self.device
                        .cmd_bind_vertex_buffers(cmd, 0, &[mesh.vertex_buffer], &[0]);
                    self.device
                        .cmd_bind_index_buffer(cmd, mesh.index_buffer, 0, vk::IndexType::UINT32);
                    // Rango de índices del lote: (0, total) para mallas
                    // enteras; un subrango por primitiva en modelos
                    // glTF con varios materiales.
                    self.device
                        .cmd_draw_indexed(cmd, range.1, 1, range.0, 0, 0);
                    self.frame_draw_calls
                        .set(self.frame_draw_calls.get() + 1);
                }
                // Render instanciado: un draw por grupo con el
                // pipeline gemelo. Los sets se enlazan como en el
                // camino por ítem, con los layouts del pipeline
                // instanciado (idénticos a los del PBR).
                if let Some(plan) = instancing {
                    let buffers = if es_escena {
                        &self.instance_scene
                    } else {
                        &self.instance_game
                    };
                    if buffers.buffer != vk::Buffer::null() {
                        let pipe = if legacy {
                            &self.pbr_pipeline_instanced_legacy
                        } else {
                            &self.pbr_pipeline_instanced
                        };
                        self.device.cmd_bind_pipeline(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            pipe.pipeline,
                        );
                        self.frame_pipeline_binds
                            .set(self.frame_pipeline_binds.get() + 1);
                        for group in &plan.groups {
                            let (mesh, push_consts, tex_set, _shader, _range, props_set, light_slot) =
                                &meshes[group.representative];
                            if *tex_set != vk::DescriptorSet::null()
                                && pipe.descriptor_set_layout.is_some()
                            {
                                self.device.cmd_bind_descriptor_sets(
                                    cmd,
                                    vk::PipelineBindPoint::GRAPHICS,
                                    pipe.layout,
                                    0,
                                    &[*tex_set],
                                    &[],
                                );
                            }
                            self.bind_graph_globals(cmd, pipe, *props_set);
                            self.bind_shadow_set(cmd, pipe);
                            self.bind_point_lights_set(cmd, pipe, *light_slot);
                            self.bind_lightmap_sets(cmd, pipe);
                            self.bind_ibl_set(cmd, pipe);
                            {
                                let mut desc = 0u32;
                                if *tex_set != vk::DescriptorSet::null()
                                    && pipe.descriptor_set_layout.is_some()
                                {
                                    desc += 1;
                                }
                                if pipe.globals_set_layout.is_some() {
                                    desc += 1;
                                }
                                if pipe.shadow_set_layout.is_some() {
                                    desc += 1;
                                }
                                if pipe.point_lights_set_layout.is_some() {
                                    desc += 1;
                                }
                                if pipe.lm_set_layouts.is_some() {
                                    desc += 2;
                                }
                                self.frame_descriptor_binds
                                    .set(self.frame_descriptor_binds.get() + desc);
                            }
                            let push = InstancedPushConstants::from_item(plan.vp, push_consts);
                            self.device.cmd_push_constants(
                                cmd,
                                pipe.layout,
                                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                                0,
                                bytemuck::bytes_of(&push),
                            );
                            self.device.cmd_bind_vertex_buffers(
                                cmd,
                                0,
                                &[mesh.vertex_buffer, buffers.buffer],
                                &[0, 0],
                            );
                            self.device.cmd_bind_index_buffer(
                                cmd,
                                mesh.index_buffer,
                                0,
                                vk::IndexType::UINT32,
                            );
                            self.device.cmd_draw_indexed(
                                cmd,
                                group.range.1,
                                group.count,
                                group.range.0,
                                0,
                                group.first_instance,
                            );
                            self.frame_draw_calls
                                .set(self.frame_draw_calls.get() + 1);
                        }
                    }
                }
                if !portal_draws.is_empty() {
                    let pipe = if legacy {
                        &self.portal_pipeline_legacy
                    } else {
                        &self.portal_pipeline
                    };
                    self.device.cmd_bind_pipeline(
                        cmd,
                        vk::PipelineBindPoint::GRAPHICS,
                        pipe.pipeline,
                    );
                    for d in portal_draws {
                        let set = self.portal_sets[d.texture_index.min(1)];
                        self.device.cmd_bind_descriptor_sets(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            pipe.layout,
                            4,
                            &[set],
                            &[],
                        );
                        self.device.cmd_push_constants(
                            cmd,
                            pipe.layout,
                            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                            0,
                            bytemuck::bytes_of(&d.push),
                        );
                        self.device
                            .cmd_bind_vertex_buffers(cmd, 0, &[d.mesh.vertex_buffer], &[0]);
                        self.device.cmd_bind_index_buffer(
                            cmd,
                            d.mesh.index_buffer,
                            0,
                            vk::IndexType::UINT32,
                        );
                        self.device
                            .cmd_draw_indexed(cmd, d.range.1, 1, d.range.0, 0, 0);
                    }
                }
                if !skinned.is_empty() {
                    self.draw_skinned(cmd, skinned, legacy);
                }
                // Contorno de selección con piel: tras las pieles, solo
                // en la vista de Escena.
                if es_escena && !outline_skinned.is_empty() {
                    self.draw_skinned_outline(cmd, outline_skinned, legacy);
                }
            }
            // Sin mallas no se dibuja nada: el triángulo de respaldo
            // (el «hello triangle» de los primeros hitos) aparecía como
            // un triángulo gigante en cualquier vista sin mallas
            // (escena vacía, solo cámaras/luces) — defecto reportado
            // por el probador y eliminado aquí de raíz.

            // Partículas (Fase 8P): al final del pase, tras las
            // mallas, con su pipeline transparente. La geometría
            // la subió la app con set_particle_frame y el VP es el
            // de ESTA vista.
            if let Some((vp, is_game)) = particle_vp {
                let buffers = if is_game {
                    &self.particle_game
                } else {
                    &self.particle_scene
                };
                if buffers.index_count > 0 && buffers.vertex_buffer != vk::Buffer::null() {
                    let pipe = if legacy {
                        &self.particle_pipeline_legacy
                    } else {
                        &self.particle_pipeline
                    };
                    let push = MeshPushConstants::new(vp);
                    self.device.cmd_bind_pipeline(
                        cmd,
                        vk::PipelineBindPoint::GRAPHICS,
                        pipe.pipeline,
                    );
                    self.device.cmd_push_constants(
                        cmd,
                        pipe.layout,
                        vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                        0,
                        bytemuck::bytes_of(&push),
                    );
                    self.device
                        .cmd_bind_vertex_buffers(cmd, 0, &[buffers.vertex_buffer], &[0]);
                    self.device.cmd_bind_index_buffer(
                        cmd,
                        buffers.index_buffer,
                        0,
                        vk::IndexType::UINT32,
                    );
                    self.device
                        .cmd_draw_indexed(cmd, buffers.index_count, 1, 0, 0, 0);
                    self.particle_draws.set(self.particle_draws.get() + 1);
                }
            }

            // Rejilla del editor: solo en la vista de escena, al final
            // del pase y con depth test (sin escribir profundidad):
            // los modelos la tapan, como en Unity. La geometría la
            // subió la app con set_editor_grid_lines.
            if es_escena {
                let lineas = &self.editor_line_buffers;
                if lineas.vertex_count > 0 && lineas.vertex_buffer != vk::Buffer::null() {
                    let pipe = if legacy {
                        &self.editor_line_pipeline_legacy
                    } else {
                        &self.editor_line_pipeline
                    };
                    let push = MeshPushConstants::new(self.editor_grid_vp);
                    self.device.cmd_bind_pipeline(
                        cmd,
                        vk::PipelineBindPoint::GRAPHICS,
                        pipe.pipeline,
                    );
                    self.device.cmd_push_constants(
                        cmd,
                        pipe.layout,
                        vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                        0,
                        bytemuck::bytes_of(&push),
                    );
                    self.device
                        .cmd_bind_vertex_buffers(cmd, 0, &[lineas.vertex_buffer], &[0]);
                    self.device.cmd_draw(cmd, lineas.vertex_count, 1, 0, 0);
                }
            }

            self.device.cmd_end_render_pass(cmd);
        }
    }


    /// Sube la geometría de partículas de una vista (Fase 8P):
    /// buffers mapeados que crecen por duplicación; la escritura
    /// es segura porque la app llama entre frames, con el fence
    /// del frame anterior ya esperado.
    pub fn set_particle_frame(
        &mut self,
        game: bool,
        verts: &[ParticleVertex],
        indices: &[u32],
    ) {
        let device = self.device.clone();
        let allocator = self.allocator.clone();
        let buffers = if game {
            &mut self.particle_game
        } else {
            &mut self.particle_scene
        };
        buffers.index_count = indices.len() as u32;
        if verts.is_empty() || indices.is_empty() {
            return;
        }
        unsafe {
            if verts.len() > buffers.vertex_capacity || indices.len() > buffers.index_capacity {
                // Crecer: destruir lo viejo y crear con el doble.
                if buffers.vertex_buffer != vk::Buffer::null() {
                    device.destroy_buffer(buffers.vertex_buffer, None);
                }
                if buffers.index_buffer != vk::Buffer::null() {
                    device.destroy_buffer(buffers.index_buffer, None);
                }
                let mut alloc = allocator.lock().unwrap();
                if let Some(a) = buffers.vertex_allocation.take() {
                    let _ = alloc.free(a);
                }
                if let Some(a) = buffers.index_allocation.take() {
                    let _ = alloc.free(a);
                }
                buffers.vertex_capacity = verts.len().next_power_of_two().max(1024);
                buffers.index_capacity = indices.len().next_power_of_two().max(1536);
                let mut mk = |size: u64,
                          usage: vk::BufferUsageFlags,
                          nombre: &'static str|
                 -> Option<(vk::Buffer, Allocation)> {
                    let info = vk::BufferCreateInfo::default()
                        .size(size)
                        .usage(usage)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE);
                    let buffer = device.create_buffer(&info, None).ok()?;
                    let mem_reqs = device.get_buffer_memory_requirements(buffer);
                    let allocation = alloc
                        .allocate(&AllocationCreateDesc {
                            name: nombre,
                            requirements: mem_reqs,
                            location: MemoryLocation::CpuToGpu,
                            linear: true,
                            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                        })
                        .ok()?;
                    device
                        .bind_buffer_memory(buffer, allocation.memory(), allocation.offset())
                        .ok()?;
                    Some((buffer, allocation))
                };
                if let Some((b, a)) = mk(
                    (buffers.vertex_capacity * std::mem::size_of::<ParticleVertex>()) as u64,
                    vk::BufferUsageFlags::VERTEX_BUFFER,
                    "particles_vertices",
                ) {
                    buffers.vertex_buffer = b;
                    buffers.vertex_allocation = Some(a);
                }
                if let Some((b, a)) = mk(
                    (buffers.index_capacity * std::mem::size_of::<u32>()) as u64,
                    vk::BufferUsageFlags::INDEX_BUFFER,
                    "particles_indices",
                ) {
                    buffers.index_buffer = b;
                    buffers.index_allocation = Some(a);
                }
            }
            if let Some(alloc) = &buffers.vertex_allocation {
                if let Some(ptr) = alloc.mapped_ptr() {
                    std::ptr::copy_nonoverlapping(
                        verts.as_ptr() as *const u8,
                        ptr.as_ptr() as *mut u8,
                        std::mem::size_of_val(verts),
                    );
                }
            }
            if let Some(alloc) = &buffers.index_allocation {
                if let Some(ptr) = alloc.mapped_ptr() {
                    std::ptr::copy_nonoverlapping(
                        indices.as_ptr() as *const u8,
                        ptr.as_ptr() as *mut u8,
                        std::mem::size_of_val(indices),
                    );
                }
            }
        }
    }

    /// Sube las instancias de una vista (`game` = vista de juego) al buffer
    /// mapeado. Si no caben, recrea el buffer con la siguiente potencia de
    /// dos (mínimo 1024 instancias), como `set_particle_frame`.
    pub fn set_instance_frame(&mut self, game: bool, instances: &[InstanceData]) {
        let device = self.device.clone();
        let allocator = self.allocator.clone();
        let buffers = if game {
            &mut self.instance_game
        } else {
            &mut self.instance_scene
        };
        if instances.is_empty() {
            return;
        }
        unsafe {
            if instances.len() > buffers.capacity {
                // Crecer: destruir lo viejo y crear con la nueva capacidad.
                if buffers.buffer != vk::Buffer::null() {
                    device.destroy_buffer(buffers.buffer, None);
                    buffers.buffer = vk::Buffer::null();
                }
                let mut alloc = allocator.lock().unwrap();
                if let Some(a) = buffers.allocation.take() {
                    let _ = alloc.free(a);
                }
                // Si la creación falla, el buffer queda nulo y la capacidad
                // a 0: nada que destruir dos veces y se reintenta el frame
                // siguiente.
                buffers.capacity = 0;
                let capacity = instances.len().next_power_of_two().max(1024);
                let info = vk::BufferCreateInfo::default()
                    .size((capacity * std::mem::size_of::<InstanceData>()) as u64)
                    .usage(vk::BufferUsageFlags::VERTEX_BUFFER)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE);
                let Ok(buffer) = device.create_buffer(&info, None) else {
                    return;
                };
                let mem_reqs = device.get_buffer_memory_requirements(buffer);
                let allocation = match alloc.allocate(&AllocationCreateDesc {
                    name: "instances",
                    requirements: mem_reqs,
                    location: MemoryLocation::CpuToGpu,
                    linear: true,
                    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                }) {
                    Ok(a) => a,
                    Err(_) => {
                        device.destroy_buffer(buffer, None);
                        return;
                    }
                };
                if device
                    .bind_buffer_memory(buffer, allocation.memory(), allocation.offset())
                    .is_err()
                {
                    device.destroy_buffer(buffer, None);
                    let _ = alloc.free(allocation);
                    return;
                }
                buffers.buffer = buffer;
                buffers.allocation = Some(allocation);
                buffers.capacity = capacity;
            }
            if let Some(alloc) = &buffers.allocation {
                if let Some(ptr) = alloc.mapped_ptr() {
                    std::ptr::copy_nonoverlapping(
                        instances.as_ptr() as *const u8,
                        ptr.as_ptr() as *mut u8,
                        std::mem::size_of_val(instances),
                    );
                }
            }
        }
    }

    /// Draws de partículas del último render_views.
    pub fn particle_draws_last_frame(&self) -> u32 {
        self.particle_draws.get()
    }

    pub fn render_views(
        &mut self,
        queue: vk::Queue,
        draws: FrameDraws<'_>,
    ) -> Result<(), RendererError> {
        let FrameDraws {
            scene_meshes,
            game_meshes,
            scene_skinned,
            game_skinned,
            scene_outline_skinned,
            shadow_casters,
            scene_portal_draws,
            game_portal_draws,
            particle_vps,
        } = draws;
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            self.device.reset_fences(&[self.fence])?;
            // Contadores de enlaces del frame (Fase 2S, S6).
            self.frame_pipeline_binds.set(0);
            self.particle_draws.set(0);
            self.frame_descriptor_binds.set(0);
            self.frame_draw_calls.set(0);

            // Lectura de la query del frame anterior: el fence ya garantizó
            // que ese trabajo terminó, así que WAIT no bloquea. La telemetría
            // nunca rompe el render: un error del driver descarta la medición.
            if self.timestamp_pending {
                self.timestamp_pending = false;
                if self.timestamp_pool != vk::QueryPool::null() {
                    let mut ticks = [0u64; 2];
                    let flags = vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT;
                    match self.device.get_query_pool_results(self.timestamp_pool, 0, &mut ticks, flags) {
                        Ok(()) => {
                            let delta_ticks = ticks[1].saturating_sub(ticks[0]);
                            self.gpu_render_us = Some(
                                (delta_ticks as f64 * self.timestamp_period as f64 / 1000.0).round() as u64,
                            );
                        }
                        Err(err) => {
                            log::warn!("Fallo leyendo la query de timestamps GPU: {:?}", err);
                        }
                    }
                }
            }

            let shadow_params = self.upload_frame_ubos();

            let cmd = self.command_buffer;
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device.begin_command_buffer(cmd, &begin_info)?;

            // Reset del pool y timestamp inicial (TOP_OF_PIPE) del pase.
            if self.timestamp_pool != vk::QueryPool::null() {
                self.device.cmd_reset_query_pool(cmd, self.timestamp_pool, 0, 2);
                self.device.cmd_write_timestamp(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, self.timestamp_pool, 0);
            }

            // --------------------------------------------------------------------
            // 0. Pase de profundidad del mapa de sombras (antes de las
            // vistas: el PBR lo muestrea en ambas). Sin proyectores o
            // con el efecto apagado no se graba.
            // --------------------------------------------------------------------
            if let Some(casters) = shadow_casters {
                self.record_shadow_pass(cmd, casters, shadow_params);
                self.record_point_shadow_passes(cmd, casters);
            }

            // --------------------------------------------------------------------
            // 1. Pass para Scene View (Cámara libre de Edición con Gizmos)
            // --------------------------------------------------------------------
            {
                let legacy = !self.post_enabled;
                // Render instanciado: plan de la vista y subida del
                // buffer de instancias antes de grabar el pase (la
                // subida es &mut: va antes de tomar el objetivo).
                let scene_instancing = {
                    let canon = self.canonical_slot_map();
                    plan_instancing(scene_meshes, canon)
                };
                if let Some(plan) = &scene_instancing {
                    self.set_instance_frame(false, &plan.instances);
                }
                let scene_target = if legacy { &self.final_offscreen } else { &self.offscreen };
                self.record_view_pass(
                    cmd,
                    scene_target,
                    [0.06, 0.09, 0.14, 1.0],
                    scene_meshes,
                    scene_skinned,
                    scene_outline_skinned,
                    scene_portal_draws,
                    legacy,
                    true,
                    particle_vps.map(|(vp, _)| (vp, false)),
                    true,
                    scene_instancing.as_ref(),
                );
            }

            // --------------------------------------------------------------------
            // 2. Pass para Game View (Cámara activa Main Camera del juego)
            // --------------------------------------------------------------------
            {
                let legacy = !self.post_enabled;
                let game_slice = game_meshes.unwrap_or(&[]);
                let game_instancing = {
                    let canon = self.canonical_slot_map();
                    plan_instancing(game_slice, canon)
                };
                if let Some(plan) = &game_instancing {
                    self.set_instance_frame(true, &plan.instances);
                }
                let game_target = if legacy { &self.final_game_offscreen } else { &self.game_offscreen };
                self.record_view_pass(
                    cmd,
                    game_target,
                    [0.02, 0.02, 0.03, 1.0],
                    game_slice,
                    game_skinned.unwrap_or(&[]),
                    &[],
                    game_portal_draws.unwrap_or(&[]),
                    legacy,
                    false,
                    particle_vps.map(|(_, vp)| (vp, true)),
                    false,
                    game_instancing.as_ref(),
                );
            }

            // Timestamp final del pase principal (BOTTOM_OF_PIPE).
            if self.timestamp_pool != vk::QueryPool::null() {
                self.device.cmd_write_timestamp(cmd, vk::PipelineStageFlags::BOTTOM_OF_PIPE, self.timestamp_pool, 1);
            }

            self.device.end_command_buffer(cmd)?;

            let command_buffers = [cmd];
            let submit_info = [vk::SubmitInfo::default().command_buffers(&command_buffers)];

            self.device.queue_submit(queue, &submit_info, self.fence)?;
            // El trabajo quedó en vuelo: su query se lee al inicio del
            // próximo render_views, tras esperar este mismo fence.
            if self.timestamp_pool != vk::QueryPool::null() {
                self.timestamp_pending = true;
            }
        }

        Ok(())
    }

    /// Renderiza las dos vistas de portal (bloque Portales):
    /// la escena desde las cámaras virtuales de cada portal de
    /// la pareja, a `portal_targets`. Sin post y SIN superficies
    /// de portal dentro (recursión 0 en v1: un portal no se ve a
    /// sí mismo ni al otro en su propia vista). Debe llamarse
    /// ANTES de `render_views`, que muestrea estas texturas; la
    /// disciplina de fence es la misma (serie por frame).
    pub fn render_portal_views(
        &mut self,
        queue: vk::Queue,
        view_a: (&[MeshDrawItem], &[SkinnedDrawItem]),
        view_b: (&[MeshDrawItem], &[SkinnedDrawItem]),
        shadow_casters: Option<&ShadowCasters<'_>>,
    ) -> Result<(), RendererError> {
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            self.device.reset_fences(&[self.fence])?;
            let shadow_params = self.upload_frame_ubos();

            let cmd = self.command_buffer;
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device.begin_command_buffer(cmd, &begin_info)?;

            if let Some(casters) = shadow_casters {
                self.record_shadow_pass(cmd, casters, shadow_params);
            }

            let clear = [0.06, 0.09, 0.14, 1.0];
            // Se clona la referencia al objetivo para no pelear
            // con el préstamo de &self de record_view_pass.
            let target_a = &self.portal_targets[0];
            self.record_view_pass(cmd, target_a, clear, view_a.0, view_a.1, &[], &[], false, false, None, false, None);
            let target_b = &self.portal_targets[1];
            self.record_view_pass(cmd, target_b, clear, view_b.0, view_b.1, &[], &[], false, false, None, false, None);

            self.device.end_command_buffer(cmd)?;
            let command_buffers = [cmd];
            let submit_info = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
            self.device.queue_submit(queue, &submit_info, self.fence)?;
        }
        Ok(())
    }

    /// Renderiza múltiples mallas 3D a la textura offscreen desacoplada (Scene View)
    /// LIMITACIÓN CONOCIDA (deuda declarada, no silenciosa):
    /// este camino sólo empaqueta el MVP. `model` queda en identidad,
    /// `camera_pos` en cero y `light_dir`/`light_color`/`material_params`
    /// en sus valores por defecto, así que el shader PBR recibe una cámara
    /// en el origen y sin luz. Sirve para el blit de standalone y para
    /// pruebas de humo del pipeline, pero NO para capturar el viewport con
    /// iluminación correcta. La ruta del editor usa `render_frame` con
    /// push constants completos.
    pub fn render_offscreen_scene(
        &mut self,
        queue: vk::Queue,
        meshes: &[(&GpuMesh, [f32; 16])],
    ) -> Result<(), RendererError> {
        let pc_list: Vec<MeshDrawItem> = meshes
            .iter()
            .map(|(m, mvp)| (*m, MeshPushConstants::new(*mvp), self.fallback_set, None, (0, m.index_count), None, 0))
            .collect();
        self.render_views(queue, FrameDraws {
            scene_meshes: &pc_list,
            game_meshes: None,
            scene_skinned: &[],
            game_skinned: None,
            scene_outline_skinned: &[],
            shadow_casters: None,
            scene_portal_draws: &[],
            game_portal_draws: None,
            particle_vps: None,
        })
    }

    /// Renderiza la escena 3D a la textura offscreen desacoplada (con soporte opcional de malla 3D indexada)
    pub fn render_offscreen_mesh(
        &mut self,
        queue: vk::Queue,
        mesh_data: Option<(&GpuMesh, [f32; 16])>,
    ) -> Result<(), RendererError> {
        match mesh_data {
            Some(data) => self.render_offscreen_scene(queue, &[data]),
            None => self.render_offscreen_scene(queue, &[]),
        }
    }

    pub fn render_offscreen(&mut self, queue: vk::Queue) -> Result<(), RendererError> {
        self.render_offscreen_mesh(queue, None)
    }

    /// Métricas de la fuente de la UI, para que la app construya los
    /// lotes del Canvas (`UiBatch`) con el mismo atlas que muestrea el
    /// pase 2D.
    pub fn ui_font(&self) -> &FontAtlas {
        self.ui.font()
    }

    /// Ejecuta la cadena de post-procesado sobre una vista (bright-pass
    /// + blur + pasada final con ACES y viñeta) y deja el resultado en
    ///   su objetivo LDR de presentación. Misma disciplina de
    ///   sincronización serializada que el resto del renderer (fence
    ///   compartido, un solo command buffer). Sin post activo
    ///   (GLASS_POST=0) no hace nada: en ese modo `render_views` ya
    ///   dibujó el 3D directamente en el objetivo LDR.
    pub fn render_post(
        &mut self,
        queue: vk::Queue,
        view: PostView,
    ) -> Result<(), RendererError> {
        if !self.post_enabled {
            return Ok(());
        }
        let settings = self.post_settings;
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            self.device.reset_fences(&[self.fence])?;

            let cmd = self.command_buffer;
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device.begin_command_buffer(cmd, &begin_info)?;

            match view {
                PostView::Scene => {
                    let (src, dst) = (&self.offscreen, &self.final_offscreen);
                    self.post.record(&self.device, cmd, src, dst, &settings)?;
                }
                PostView::Game => {
                    let (src, dst) = (&self.game_offscreen, &self.final_game_offscreen);
                    self.post.record(&self.device, cmd, src, dst, &settings)?;
                }
            }

            self.device.end_command_buffer(cmd)?;
            let command_buffers = [cmd];
            let submit_info = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
            self.device.queue_submit(queue, &submit_info, self.fence)?;
        }
        Ok(())
    }

    /// Dibuja el lote de UI del Canvas sobre la textura offscreen (la que
    /// presenta el blit del standalone), DESPUÉS del 3D y con la misma
    /// disciplina de sincronización serializada del resto del renderer:
    /// espera el fence compartido, graba en el command buffer único y
    /// envía señalándolo. Quien llame después (el blit) volverá a esperar
    /// ese mismo fence antes de leer la imagen.
    pub fn render_ui_overlay(
        &mut self,
        queue: vk::Queue,
        segments: &[UiSegment],
    ) -> Result<(), RendererError> {
        if segments.iter().all(|s| s.batch.is_empty()) {
            return Ok(());
        }
        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            self.device.reset_fences(&[self.fence])?;

            let cmd = self.command_buffer;
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device.begin_command_buffer(cmd, &begin_info)?;

            // Segmentos en orden de pintor; cada pase de render es
            // LOAD/STORE, así que encadenan sobre el mismo target.
            for seg in segments {
                if seg.batch.is_empty() {
                    continue;
                }
                let target = if self.present_game {
                    &self.final_game_offscreen
                } else {
                    &self.final_offscreen
                };
                match &seg.atlas {
                    UiAtlasKind::Font => self.ui.record(cmd, target, &seg.batch),
                    UiAtlasKind::Image => {
                        if let Some(ui_img) = &mut self.ui_images {
                            ui_img.record(cmd, target, &seg.batch);
                        }
                    }
                    UiAtlasKind::FontNamed(name) => {
                        if let Some(pass) = self.ui_named_fonts.get_mut(name) {
                            pass.record(cmd, target, &seg.batch);
                        }
                    }
                }
            }

            self.device.end_command_buffer(cmd)?;

            let command_buffers = [cmd];
            let submit_info = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
            self.device.queue_submit(queue, &submit_info, self.fence)?;
        }
        Ok(())
    }

    /// Sube el atlas de imágenes del Canvas (UI v2) y crea su pase
    /// 2D, sustituyendo el anterior si lo había. `rgba` es el
    /// atlas cuadrado empaquetado por `pack_ui_image_atlas`.
    pub fn set_ui_image_atlas(
        &mut self,
        queue: vk::Queue,
        rgba: Vec<u8>,
        size: u32,
    ) -> Result<(), RendererError> {
        if let Some(mut old) = self.ui_images.take() {
            old.destroy(&mut self.allocator.lock().unwrap());
        }
        let renderer = UiRenderer::new_for_texture(
            self.device.clone(),
            &mut self.allocator.lock().unwrap(),
            queue,
            self.command_pool,
            rgba,
            size,
        )?;
        self.ui_images = Some(renderer);
        Ok(())
    }

    /// Crea el pase 2D de una fuente con nombre (assets/fonts) si
    /// aún no existe; si ya existe, no hace nada. Los bytes son el
    /// .ttf completo.
    pub fn ensure_named_font(
        &mut self,
        queue: vk::Queue,
        name: &str,
        font_bytes: &[u8],
    ) -> Result<(), RendererError> {
        if self.ui_named_fonts.contains_key(name) {
            return Ok(());
        }
        let renderer = UiRenderer::new_with_font_bytes(
            self.device.clone(),
            &mut self.allocator.lock().unwrap(),
            queue,
            self.command_pool,
            font_bytes,
        )?;
        self.ui_named_fonts.insert(name.to_string(), renderer);
        Ok(())
    }

    /// ¿Ya existe el pase de esta fuente con nombre?
    pub fn has_named_font(&self, name: &str) -> bool {
        self.ui_named_fonts.contains_key(name)
    }

    /// Descarta el pase de imágenes del Canvas (la escena nueva no
    /// usa elementos Imagen).
    pub fn clear_ui_image_atlas(&mut self) {
        if let Some(mut old) = self.ui_images.take() {
            old.destroy(&mut self.allocator.lock().unwrap());
        }
    }

    /// Presenta la textura offscreen directamente sobre una imagen del Swapchain mediante hardware blit.
    /// Utilizado en modo Standalone (runtime de juego) para máximo rendimiento sin overhead de UI.
    pub fn blit_offscreen_to_swapchain(
        &mut self,
        queue: vk::Queue,
        image_index: u32,
    ) -> Result<(), RendererError> {
        let swapchain_image = self.swapchain.images[image_index as usize];
        let offscreen_image = if self.present_game {
            self.final_game_offscreen.color_image
        } else {
            self.final_offscreen.color_image
        };

        unsafe {
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            self.device.reset_fences(&[self.fence])?;

            let cmd = self.command_buffer;
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device.begin_command_buffer(cmd, &begin_info)?;

            // 1. Transición del buffer offscreen de SHADER_READ_ONLY_OPTIMAL a TRANSFER_SRC_OPTIMAL
            let barrier_offscreen_src = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(offscreen_image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .src_access_mask(vk::AccessFlags::SHADER_READ)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ);

            // 2. Transición de la imagen del Swapchain de UNDEFINED a TRANSFER_DST_OPTIMAL
            let barrier_swapchain_dst = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(swapchain_image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);

            self.device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier_offscreen_src, barrier_swapchain_dst],
            );

            // 3. Hardware Blit con escalado bilineal automático si el swapchain difiere de la resolución interna
            let blit_region = vk::ImageBlit::default()
                .src_subresource(vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .src_offsets([
                    vk::Offset3D { x: 0, y: 0, z: 0 },
                    vk::Offset3D {
                        x: self.final_offscreen.extent.width as i32,
                        y: self.final_offscreen.extent.height as i32,
                        z: 1,
                    },
                ])
                .dst_subresource(vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .dst_offsets([
                    vk::Offset3D { x: 0, y: 0, z: 0 },
                    vk::Offset3D {
                        x: self.swapchain.extent.width as i32,
                        y: self.swapchain.extent.height as i32,
                        z: 1,
                    },
                ]);

            self.device.cmd_blit_image(
                cmd,
                offscreen_image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                swapchain_image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[blit_region],
                vk::Filter::LINEAR,
            );

            // 4. Transición de la imagen del swapchain a PRESENT_SRC_KHR para la presentación
            let barrier_swapchain_present = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(swapchain_image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::MEMORY_READ);

            // 5. Transición del buffer offscreen de vuelta a SHADER_READ_ONLY_OPTIMAL
            let barrier_offscreen_restore = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(offscreen_image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                .dst_access_mask(vk::AccessFlags::SHADER_READ);

            self.device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier_swapchain_present, barrier_offscreen_restore],
            );

            self.device.end_command_buffer(cmd)?;

            let wait_semaphores = [self.image_available_semaphore];
            let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            let signal_semaphores = [self.render_finished_for(image_index)];
            let command_buffers = [cmd];

            let submit_info = [vk::SubmitInfo::default()
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&wait_stages)
                .command_buffers(&command_buffers)
                .signal_semaphores(&signal_semaphores)];

            self.device.queue_submit(queue, &submit_info, self.fence)?;
        }

        Ok(())
    }
}

impl Drop for GlassRenderer {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().unwrap_or(());
            for fb in self.swapchain_framebuffers.drain(..) {
                self.device.destroy_framebuffer(fb, None);
            }
            self.device
                .destroy_render_pass(self.swapchain_render_pass, None);
            self.device.destroy_semaphore(self.image_available_semaphore, None);
            for semaphore in self.render_finished_semaphores.drain(..) {
                self.device.destroy_semaphore(semaphore, None);
            }
            self.device.destroy_fence(self.fence, None);
            if self.timestamp_pool != vk::QueryPool::null() {
                self.device.destroy_query_pool(self.timestamp_pool, None);
            }
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_descriptor_pool(self.joints_pool, None);
            // Buffer de líneas del editor (rejilla).
            if self.editor_line_buffers.vertex_buffer != vk::Buffer::null() {
                self.device
                    .destroy_buffer(self.editor_line_buffers.vertex_buffer, None);
                self.editor_line_buffers.vertex_buffer = vk::Buffer::null();
            }
            if let Some(a) = self.editor_line_buffers.vertex_allocation.take() {
                let _ = self.allocator.lock().unwrap().free(a);
            }
            for buffers in [&mut self.particle_scene, &mut self.particle_game] {
                if buffers.vertex_buffer != vk::Buffer::null() {
                    self.device.destroy_buffer(buffers.vertex_buffer, None);
                }
                if buffers.index_buffer != vk::Buffer::null() {
                    self.device.destroy_buffer(buffers.index_buffer, None);
                }
                let mut alloc = self.allocator.lock().unwrap();
                if let Some(a) = buffers.vertex_allocation.take() {
                    let _ = alloc.free(a);
                }
                if let Some(a) = buffers.index_allocation.take() {
                    let _ = alloc.free(a);
                }
            }
        // Buffers de instancias (mismo patrón que los de partículas).
        {
            let mut alloc = self.allocator.lock().unwrap();
            for buffers in [&mut self.instance_scene, &mut self.instance_game] {
                if buffers.buffer != vk::Buffer::null() {
                    self.device.destroy_buffer(buffers.buffer, None);
                    buffers.buffer = vk::Buffer::null();
                }
                if let Some(a) = buffers.allocation.take() {
                    let _ = alloc.free(a);
                }
                buffers.capacity = 0;
            }
        }
            self.device.destroy_buffer(self.graph_globals_buffer, None);
            self.device.destroy_buffer(self.graph_props_dummy_buffer, None);
            self.device
                .destroy_descriptor_pool(self.graph_globals_pool, None);
            for entry in self.graph_props_sets.drain(..) {
                self.device.destroy_buffer(entry.buffer, None);
            }
            self.device
                .destroy_descriptor_pool(self.graph_props_pool, None);
            self.device
                .destroy_descriptor_set_layout(self.graph_globals_layout, None);
            // Set 6 (IBL): pool y UBO de parámetros creados en el init;
            // se destruyen aquí como el resto de pools del renderer
            // (sin esto, además, los campos quedaban «nunca leídos» y
            // el CI con -D warnings no compilaba glass_renderer).
            self.device.destroy_descriptor_pool(self.ibl_pool, None);
            self.device.destroy_buffer(self.ibl_params_buffer, None);
        }
        self.ui.destroy(&mut self.allocator.lock().unwrap());
        if let Some(ui_img) = &mut self.ui_images {
            ui_img.destroy(&mut self.allocator.lock().unwrap());
        }
        for pass in self.ui_named_fonts.values_mut() {
            pass.destroy(&mut self.allocator.lock().unwrap());
        }
        self.post
            .destroy(&self.device, &mut self.allocator.lock().unwrap());
        self.shadow.destroy_with_device(
            &self.device,
            &mut self.allocator.lock().unwrap(),
        );
        self.point_shadows.destroy_with_device(
            &self.device,
            &mut self.allocator.lock().unwrap(),
        );
        self.offscreen
            .destroy(&mut self.allocator.lock().unwrap());
        self.game_offscreen
            .destroy(&mut self.allocator.lock().unwrap());
        self.final_offscreen
            .destroy(&mut self.allocator.lock().unwrap());
        self.final_game_offscreen
            .destroy(&mut self.allocator.lock().unwrap());
    }
}

#[cfg(test)]
mod push_constant_contract_tests {
    use super::*;
    use std::mem::offset_of;

    /// Los offsets que los cuatro shaders GLSL documentan en sus comentarios.
    /// Si alguien reordena o añade un campo en Rust sin tocar el GLSL, este test
    /// falla aquí en vez de producir geometría deformada silenciosa en pantalla.
    const GLSL_OFFSETS: [(&str, usize, usize); 8] = [
        ("mvp", 0, 64),
        ("model", 64, 64),
        ("light_dir", 128, 16),
        ("light_color", 144, 16),
        ("material_params", 160, 16),
        ("object_color", 176, 16),
        ("camera_pos", 192, 16),
        ("lm_params", 208, 16),
    ];

    #[test]
    fn offsets_coinciden_con_glsl() {
        let reales: [(&str, usize, usize); 8] = [
            (
                "mvp",
                offset_of!(MeshPushConstants, mvp),
                std::mem::size_of::<[f32; 16]>(),
            ),
            (
                "model",
                offset_of!(MeshPushConstants, model),
                std::mem::size_of::<[f32; 16]>(),
            ),
            (
                "light_dir",
                offset_of!(MeshPushConstants, light_dir),
                std::mem::size_of::<[f32; 4]>(),
            ),
            (
                "light_color",
                offset_of!(MeshPushConstants, light_color),
                std::mem::size_of::<[f32; 4]>(),
            ),
            (
                "material_params",
                offset_of!(MeshPushConstants, material_params),
                std::mem::size_of::<[f32; 4]>(),
            ),
            (
                "object_color",
                offset_of!(MeshPushConstants, object_color),
                std::mem::size_of::<[f32; 4]>(),
            ),
            (
                "camera_pos",
                offset_of!(MeshPushConstants, camera_pos),
                std::mem::size_of::<[f32; 4]>(),
            ),
            (
                "lm_params",
                offset_of!(MeshPushConstants, lm_params),
                std::mem::size_of::<[f32; 4]>(),
            ),
        ];
        assert_eq!(reales, GLSL_OFFSETS, "El layout de MeshPushConstants ya no coincide con el bloque push_constant de los shaders .vert/.frag");
    }

    #[test]
    fn tamano_y_alineacion_son_los_esperados() {
        assert_eq!(MESH_PUSH_CONSTANTS_SIZE, 256);
        assert_eq!(std::mem::align_of::<MeshPushConstants>(), 4);
        // Todos los offsets deben caer en múltiplo de 16 para respetar std140.
        for (nombre, off, tam) in GLSL_OFFSETS {
            assert_eq!(off % 16, 0, "campo '{nombre}' no está alineado a 16 bytes");
            assert!(off + tam <= 256, "campo '{nombre}' desborda los 256 bytes");
        }
    }

    #[test]
    fn default_es_identidad_y_new_solo_cambia_la_mvp() {
        let d = MeshPushConstants::default();
        assert_eq!(d.mvp, IDENTITY_MAT4);
        assert_eq!(d.model, IDENTITY_MAT4, "el modelo por defecto debe ser identidad, no basura");

        let m = [7.0_f32; 16];
        let n = MeshPushConstants::new(m);
        assert_eq!(n.mvp, m);
        assert_eq!(n.model, IDENTITY_MAT4);
        assert_eq!(n.camera_pos, d.camera_pos);
    }

    #[test]
    fn bytemuck_serializa_exactamente_256_bytes() {
        let pc = MeshPushConstants::default();
        let bytes = bytemuck::bytes_of(&pc);
        assert_eq!(bytes.len(), 256);
        assert_eq!(bytes.len(), MESH_PUSH_CONSTANTS_SIZE as usize);
    }
}

#[cfg(test)]
mod point_lights_ubo_tests {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn point_lights_ubo_respeta_std140() {
        assert_eq!(offset_of!(PointLightsUbo, pos_range), 0);
        assert_eq!(offset_of!(PointLightsUbo, color_intensity), 128);
        assert_eq!(offset_of!(PointLightsUbo, shadow), 256);
        assert_eq!(offset_of!(PointLightsUbo, dir_cos), 384);
        assert_eq!(offset_of!(PointLightsUbo, cone), 512);
        assert_eq!(offset_of!(PointLightsUbo, count), 640);
        assert_eq!(offset_of!(PointLightsUbo, _pad), 644);
        assert_eq!(std::mem::size_of::<PointLightsUbo>(), 656);
        assert_eq!(POINT_LIGHTS_UBO_SIZE, 656);
    }

    #[test]
    fn point_lights_default_no_ilumina() {
        let ubo = PointLightsUbo::default();
        assert_eq!(ubo.count, 0);
        assert_eq!(ubo.pos_range, [[0.0; 4]; MAX_POINT_LIGHTS]);
        assert_eq!(ubo.color_intensity, [[0.0; 4]; MAX_POINT_LIGHTS]);
    }

    #[test]
    fn point_lights_bytemuck_serializa_656_bytes() {
        let ubo = PointLightsUbo::default();
        assert_eq!(bytemuck::bytes_of(&ubo).len(), 656);
        assert_eq!(bytemuck::bytes_of(&ubo).len(), POINT_LIGHTS_UBO_SIZE as usize);
    }
}

#[cfg(test)]
mod instancing_tests {
    use super::*;
    use ash::vk::Handle;

    fn identidad() -> Vec<u32> {
        (0..POINT_LIGHT_SLOTS as u32).collect()
    }

    fn traslacion(x: f32, y: f32, z: f32) -> [f32; 16] {
        let mut m = IDENTITY_MAT4;
        m[12] = x;
        m[13] = y;
        m[14] = z;
        m
    }

    fn item<'a>(
        mesh: &'a GpuMesh,
        model: [f32; 16],
        vp: &[f32; 16],
    ) -> MeshDrawItem<'a> {
        let mut push = MeshPushConstants {
            model,
            ..Default::default()
        };
        push.mvp = mat4_mul(vp, &model);
        (
            mesh,
            push,
            vk::DescriptorSet::null(),
            None,
            (0, 36),
            None,
            0,
        )
    }

    fn malla_fake(raw: u64) -> GpuMesh {
        GpuMesh {
            vertex_buffer: vk::Buffer::from_raw(raw),
            vertex_allocation: None,
            index_buffer: vk::Buffer::from_raw(raw + 1),
            index_allocation: None,
            index_count: 36,
            vertex_count: 24,
        }
    }

    #[test]
    fn mat4_inversa_desace_la_multiplicacion() {
        // TRS: escala (2, 3, 4) + traslación.
        let mut m = traslacion(5.0, -2.0, 7.0);
        m[0] = 2.0;
        m[5] = 3.0;
        m[10] = 4.0;
        let inv = mat4_inverse(&m).expect("invertible");
        let prod = mat4_mul(&m, &inv);
        for i in 0..16 {
            assert!(
                (prod[i] - IDENTITY_MAT4[i]).abs() < 1e-5,
                "prod[{i}] = {}",
                prod[i]
            );
        }
    }

    #[test]
    fn mat4_inversa_degenerada_es_none() {
        let mut m = IDENTITY_MAT4;
        m[0] = 0.0; // escala x a cero
        assert!(mat4_inverse(&m).is_none());
    }

    #[test]
    fn plan_agrupa_repetidos_y_recupera_la_vp() {
        let mesh = malla_fake(0x1000);
        let vp = traslacion(0.0, 0.0, -10.0);
        let items = vec![
            item(&mesh, traslacion(0.0, 0.0, 0.0), &vp),
            item(&mesh, traslacion(1.0, 0.0, 0.0), &vp),
            item(&mesh, traslacion(2.0, 0.0, 0.0), &vp),
        ];
        let plan = plan_instancing(&items, &identidad()).expect("hay grupo");
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].count, 3);
        assert_eq!(plan.groups[0].first_instance, 0);
        assert_eq!(plan.instances.len(), 3);
        assert!(plan.grouped.iter().all(|g| *g));
        for i in 0..16 {
            assert!((plan.vp[i] - vp[i]).abs() < 1e-5, "vp[{i}]");
        }
        // Las instancias conservan el modelo de cada ítem.
        assert_eq!(plan.instances[1].model[12], 1.0);
        assert_eq!(plan.instances[2].model[12], 2.0);
    }

    #[test]
    fn plan_no_agrupa_distinto_material_ni_shader_ni_solitarios() {
        let mesh = malla_fake(0x2000);
        let vp = traslacion(0.0, 0.0, -10.0);
        let mut distinto_material = item(&mesh, traslacion(9.0, 0.0, 0.0), &vp);
        distinto_material.1.material_params = [0.9, 0.1, 1.0, 1.0];
        let mut con_shader = item(&mesh, traslacion(8.0, 0.0, 0.0), &vp);
        con_shader.3 = Some("material_transparent".to_string());
        let items = vec![
            item(&mesh, traslacion(0.0, 0.0, 0.0), &vp),
            item(&mesh, traslacion(1.0, 0.0, 0.0), &vp),
            distinto_material,
            con_shader,
        ];
        let plan = plan_instancing(&items, &identidad()).expect("el par idéntico agrupa");
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].count, 2);
        assert!(plan.grouped[0] && plan.grouped[1]);
        assert!(!plan.grouped[2] && !plan.grouped[3]);
    }

    #[test]
    fn plan_sin_grupo_devuelve_none() {
        let mesh = malla_fake(0x3000);
        let otra = malla_fake(0x4000);
        let vp = traslacion(0.0, 0.0, -10.0);
        let items = vec![
            item(&mesh, traslacion(0.0, 0.0, 0.0), &vp),
            item(&otra, traslacion(1.0, 0.0, 0.0), &vp),
        ];
        assert!(plan_instancing(&items, &identidad()).is_none());
        assert!(plan_instancing(&[], &identidad()).is_none());
    }

    #[test]
    fn plan_agrupa_por_ranura_canonizada() {
        // Dos objetos con ranuras PROPIAS distintas (3 y 7) pero la
        // misma lista de luces (canónica 3): agrupan. Con la
        // identidad no agrupan.
        let mesh = malla_fake(0x3000);
        let vp = traslacion(0.0, 0.0, -10.0);
        let mut items = vec![
            item(&mesh, traslacion(0.0, 0.0, 0.0), &vp),
            item(&mesh, traslacion(1.0, 0.0, 0.0), &vp),
        ];
        items[0].6 = 3;
        items[1].6 = 7;
        assert!(plan_instancing(&items, &identidad()).is_none());
        let mut canon = identidad();
        canon[7] = 3;
        let plan = plan_instancing(&items, &canon).expect("la canónica agrupa");
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].count, 2);
    }

    #[test]
    fn instance_data_extrae_los_campos_por_entidad() {
        let push = MeshPushConstants {
            model: traslacion(3.0, 4.0, 5.0),
            object_color: [0.1, 0.2, 0.3, 0.4],
            lm_params: [7.0, 2048.0, 0.0, 0.0],
            ..Default::default()
        };
        let d = push.instance_data();
        assert_eq!(d.model[12], 3.0);
        assert_eq!(d.object_color, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(d.lm_params, [7.0, 2048.0, 0.0, 0.0]);
        let pc = InstancedPushConstants::from_item(IDENTITY_MAT4, &push);
        assert_eq!(pc.vp, IDENTITY_MAT4);
        assert_eq!(pc.material_params, push.material_params);
        assert_eq!(std::mem::size_of::<InstanceData>(), 96);
        assert_eq!(std::mem::size_of::<InstancedPushConstants>(), 160);
    }
}
