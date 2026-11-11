//! Pase 2D del Canvas de juego (Screen Space Overlay) SIN egui.
//!
//! El editor pinta el Canvas con egui sobre la vista de juego, pero el
//! binario de juego puro (`--no-default-features`) no enlaza egui: sin
//! este módulo, el HUD de las escenas simplemente no existía en el
//! standalone, y la beta jugable exige que exista (ley 4e y criterio de
//! beta del análisis profundo). Este pase dibuja las entidades Canvas ya
//! resueltas a píxeles (ver `glass_core::resolve_ui_rect`) sobre el
//! framebuffer offscreen del juego, después del 3D y antes del blit al
//! swapchain.
//!
//! Diseño mínimo, en la línea anti-sobreingeniería del motor:
//! - Un solo pipeline y una sola llamada de dibujo por frame: todos los
//!   elementos son quads con (posición, UV, color) en un buffer de
//!   vértices CPU→GPU que se reescribe cada frame.
//! - TODO muestrea el atlas de fuente: los rectángulos sólidos apuntan a
//!   un texel opaco reservado y los glifos a su cobertura en el canal A.
//! - La fuente es DejaVu Sans, incrustada con `include_bytes!` (como los
//!   SPIR-V) y rasterizada al arrancar con `ab_glyph` — crate que ya
//!   estaba en el árbol de dependencias vía epaint/egui, no se añade
//!   ninguna librería nueva al proyecto, solo se declara el uso directo.
//! - Sin test de profundidad: el orden de dibujo es el orden de la lista
//!   de entidades de la escena (algoritmo del pintor), como en el editor.

use crate::offscreen::OffscreenTarget;
use crate::renderer::RendererError;
use ab_glyph::{point, Font, FontRef, PxScale};
use ash::vk;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator};
use gpu_allocator::MemoryLocation;
use std::collections::HashMap;
use std::sync::Arc;

const UI_VERT_SPV: &[u8] = include_bytes!("../../../assets/shaders/ui.vert.spv");
const UI_FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/ui.frag.spv");
const UI_IMG_FRAG_SPV: &[u8] = include_bytes!("../../../assets/shaders/ui_image.frag.spv");
const FONT_TTF: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans.ttf");

/// Tamaño al que se rasterizan los glifos en el atlas. El texto se escala
/// al dibujar (`font_size / ATLAS_RASTER_PX`), así que un atlas basta para
/// todos los tamaños de fuente de la UI.
const ATLAS_RASTER_PX: f32 = 64.0;
const ATLAS_W: u32 = 1024;
const ATLAS_H: u32 = 1024;
/// Bloque opaco reservado en la esquina del atlas para los sólidos.
const SOLID_BLOCK: u32 = 8;

/// Capacidad fija del buffer de vértices de UI (vértices). Un HUD típico
/// usa unos cientos de quads; 16.384 vértices son ~2.700 quads. Si un
/// frame pide más, se trunca con aviso en el log (ver `record`).
const MAX_UI_VERTICES: usize = 16_384;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct UiVertex {
    pub pos: [f32; 2],   // píxeles de pantalla, origen arriba-izquierda
    pub uv: [f32; 2],    // coordenadas en el atlas de fuente
    pub color: [f32; 4], // RGBA lineal, igual convención que el resto del motor
}

/// Métricas de un glifo en el atlas, en píxeles al tamaño de rasterizado.
#[derive(Clone, Copy, Debug)]
pub struct GlyphEntry {
    /// UV (u0, v0, u1, v1) de la caja del glifo en el atlas.
    pub uv: [f32; 4],
    /// Tamaño de la caja en píxeles (al tamaño de rasterizado).
    pub size: [f32; 2],
    /// `px_bounds.min` del glifo: desplazamiento de su caja respecto al
    /// origen de la pluma (inicio de la línea base), Y hacia abajo.
    pub bearing: [f32; 2],
    /// Avance horizontal de la pluma tras este glifo, en píxeles.
    pub advance: f32,
}

/// Atlas de glifos rasterizado en CPU (la subida a VRAM la hace
/// `UiRenderer`). Contiene ASCII imprimible y Latin-1 (acentos del
/// español incluidos: á, é, í, ó, ú, ñ, ¡, ¿…).
pub struct FontAtlas {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    glyphs: HashMap<char, GlyphEntry>,
    /// Ascenso de la fuente en píxeles (al tamaño de rasterizado).
    pub ascent: f32,
    /// Descenso en píxeles (negativo, al tamaño de rasterizado).
    pub descent: f32,
    /// Línea completa en píxeles (al tamaño de rasterizado).
    pub line_height: f32,
    /// UV del texel opaco reservado para rectángulos sólidos.
    pub solid_uv: [f32; 2],
}

impl FontAtlas {
    pub fn build(font_bytes: &[u8]) -> Result<Self, String> {
        let font = FontRef::try_from_slice(font_bytes)
            .map_err(|e| format!("TTF de UI inválida: {e}"))?;
        let upem = font
            .units_per_em()
            .ok_or_else(|| "La TTF de UI no declara units_per_em".to_string())?;
        let k = ATLAS_RASTER_PX / upem;
        let ascent = font.ascent_unscaled() * k;
        let descent = font.descent_unscaled() * k;
        let line_height = (font.ascent_unscaled() - font.descent_unscaled()
            + font.line_gap_unscaled())
            * k;

        let mut pixels = vec![0u8; (ATLAS_W * ATLAS_H * 4) as usize];
        // Bloque opaco para los sólidos (RGBA blanco, alfa 255).
        for y in 0..SOLID_BLOCK {
            for x in 0..SOLID_BLOCK {
                let i = ((y * ATLAS_W + x) * 4) as usize;
                pixels[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }

        let mut glyphs = HashMap::new();
        // Estantería simple: filas de izquierda a derecha bajo el bloque
        // reservado, con 1 px de margen entre glifos.
        let mut shelf_x = 0u32;
        let mut shelf_y = SOLID_BLOCK + 2;
        let mut shelf_h = 0u32;

        let mut chars: Vec<char> = (32u8..=126).map(char::from).collect();
        chars.extend((160u8..=255).map(char::from));

        for c in chars {
            let id = font.glyph_id(c);
            let advance = font.h_advance_unscaled(id) * k;
            let glyph = id.with_scale_and_position(PxScale::from(ATLAS_RASTER_PX), point(0.0, 0.0));
            let outlined = match font.outline_glyph(glyph) {
                Some(o) => o,
                None => {
                    // Glifo sin contorno (espacio y similares): solo avanza.
                    glyphs.insert(
                        c,
                        GlyphEntry {
                            uv: [0.0, 0.0, 0.0, 0.0],
                            size: [0.0, 0.0],
                            bearing: [0.0, 0.0],
                            advance,
                        },
                    );
                    continue;
                }
            };
            let bounds = outlined.px_bounds();
            let gw = (bounds.max.x - bounds.min.x).ceil().max(0.0) as u32;
            let gh = (bounds.max.y - bounds.min.y).ceil().max(0.0) as u32;
            if gw == 0 || gh == 0 {
                glyphs.insert(
                    c,
                    GlyphEntry {
                        uv: [0.0, 0.0, 0.0, 0.0],
                        size: [0.0, 0.0],
                        bearing: [bounds.min.x, bounds.min.y],
                        advance,
                    },
                );
                continue;
            }
            if shelf_x + gw + 1 > ATLAS_W {
                shelf_x = 0;
                shelf_y += shelf_h + 1;
                shelf_h = 0;
            }
            if shelf_y + gh > ATLAS_H {
                log::warn!("Atlas de fuente de UI lleno: el glifo '{c}' queda fuera");
                continue;
            }
            let ox = shelf_x;
            let oy = shelf_y;
            outlined.draw(|dx, dy, v| {
                let px = ox + dx;
                let py = oy + dy;
                if px < ATLAS_W && py < ATLAS_H {
                    let i = ((py * ATLAS_W + px) * 4) as usize;
                    pixels[i] = 255;
                    pixels[i + 1] = 255;
                    pixels[i + 2] = 255;
                    pixels[i + 3] = (v.clamp(0.0, 1.0) * 255.0) as u8;
                }
            });
            glyphs.insert(
                c,
                GlyphEntry {
                    uv: [
                        ox as f32 / ATLAS_W as f32,
                        oy as f32 / ATLAS_H as f32,
                        (ox + gw) as f32 / ATLAS_W as f32,
                        (oy + gh) as f32 / ATLAS_H as f32,
                    ],
                    size: [gw as f32, gh as f32],
                    bearing: [bounds.min.x, bounds.min.y],
                    advance,
                },
            );
            shelf_x += gw + 1;
            shelf_h = shelf_h.max(gh);
        }

        Ok(Self {
            pixels,
            width: ATLAS_W,
            height: ATLAS_H,
            glyphs,
            ascent,
            descent,
            line_height,
            solid_uv: [
                (SOLID_BLOCK / 2) as f32 / ATLAS_W as f32,
                (SOLID_BLOCK / 2) as f32 / ATLAS_H as f32,
            ],
        })
    }

    /// El glifo de un carácter, con `?` como sustituto universal.
    pub fn glyph(&self, c: char) -> Option<&GlyphEntry> {
        self.glyphs
            .get(&c)
            .or_else(|| self.glyphs.get(&'?'))
    }

    /// Ancho en píxeles de un texto a un tamaño de fuente dado (la línea
    /// más ancha si hay saltos de línea).
    pub fn measure(&self, text: &str, font_size: f32) -> f32 {
        let k = font_size / ATLAS_RASTER_PX;
        let mut widest = 0.0f32;
        let mut line = 0.0f32;
        for c in text.chars() {
            if c == '\n' {
                widest = widest.max(line);
                line = 0.0;
                continue;
            }
            if let Some(g) = self.glyph(c) {
                line += g.advance * k;
            }
        }
        widest.max(line)
    }

    /// Alto de línea en píxeles a un tamaño de fuente dado.
    pub fn line_height_at(&self, font_size: f32) -> f32 {
        self.line_height * (font_size / ATLAS_RASTER_PX)
    }
}

/// Lote de vértices de UI de un frame: quads en orden de pintor (lo
/// primero de la lista se dibuja primero, debajo de lo siguiente).
#[derive(Default)]
pub struct UiBatch {
    vertices: Vec<UiVertex>,
}

impl UiBatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn vertices(&self) -> &[UiVertex] {
        &self.vertices
    }

    /// Quad genérico: `rect` = [x, y, ancho, alto] en píxeles de
    /// pantalla; `uv` = (u0, v0, u1, v1) en el atlas.
    pub fn push_quad(&mut self, rect: [f32; 4], uv: [f32; 4], color: [f32; 4]) {
        let [x, y, w, h] = rect;
        let [u0, v0, u1, v1] = uv;
        let tl = UiVertex { pos: [x, y], uv: [u0, v0], color };
        let tr = UiVertex { pos: [x + w, y], uv: [u1, v0], color };
        let bl = UiVertex { pos: [x, y + h], uv: [u0, v1], color };
        let br = UiVertex { pos: [x + w, y + h], uv: [u1, v1], color };
        self.vertices
            .extend_from_slice(&[tl, bl, tr, tr, bl, br]);
    }

    /// Rectángulo de color plano (paneles y fondos de botón).
    pub fn push_solid(&mut self, atlas: &FontAtlas, rect: [f32; 4], color: [f32; 4]) {
        let [u, v] = atlas.solid_uv;
        self.push_quad(rect, [u, v, u, v], color);
    }

    /// Texto con origen en la esquina superior izquierda del área de
    /// texto, como lo pinta el editor (alineación LEFT_TOP). Admite
    /// saltos de línea con `\n`.
    pub fn push_text(
        &mut self,
        atlas: &FontAtlas,
        origin_top_left: [f32; 2],
        text: &str,
        font_size: f32,
        color: [f32; 4],
    ) {
        let k = font_size / ATLAS_RASTER_PX;
        let mut pen_x = origin_top_left[0];
        let mut baseline_y = origin_top_left[1] + atlas.ascent * k;
        for c in text.chars() {
            if c == '\n' {
                pen_x = origin_top_left[0];
                baseline_y += atlas.line_height * k;
                continue;
            }
            let Some(g) = atlas.glyph(c) else { continue };
            if g.size[0] > 0.0 && g.size[1] > 0.0 {
                let quad = [
                    pen_x + g.bearing[0] * k,
                    baseline_y + g.bearing[1] * k,
                    g.size[0] * k,
                    g.size[1] * k,
                ];
                self.push_quad(quad, g.uv, color);
            }
            pen_x += g.advance * k;
        }
    }

    /// Texto centrado en un rectángulo (etiquetas de botón, como las
    /// dibuja egui en el editor).
    pub fn push_text_centered(
        &mut self,
        atlas: &FontAtlas,
        rect: [f32; 4],
        text: &str,
        font_size: f32,
        color: [f32; 4],
    ) {
        let w = atlas.measure(text, font_size);
        let origin = [
            rect[0] + (rect[2] - w) * 0.5,
            rect[1] + (rect[3] - atlas.line_height_at(font_size)) * 0.5,
        ];
        self.push_text(atlas, origin, text, font_size, color);
    }
}

/// Lado GPU del pase 2D: pipeline, atlas en VRAM y buffer de vértices.
pub struct UiRenderer {
    device: Arc<ash::Device>,
    font: FontAtlas,
    /// Render pass gemelo del offscreen pero con `load_op: LOAD`: el 3D
    /// ya escribió el color y la UI se compone encima sin borrarlo.
    render_pass: vk::RenderPass,
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    descriptor_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set: vk::DescriptorSet,
    atlas_image: vk::Image,
    atlas_allocation: Option<Allocation>,
    atlas_view: vk::ImageView,
    sampler: vk::Sampler,
    vertex_buffer: vk::Buffer,
    vertex_allocation: Option<Allocation>,
}

impl UiRenderer {
    pub fn new(
        device: Arc<ash::Device>,
        allocator: &mut Allocator,
        queue: vk::Queue,
        command_pool: vk::CommandPool,
    ) -> Result<Self, RendererError> {
        let font = FontAtlas::build(FONT_TTF).map_err(RendererError::Descriptor)?;
        Self::create_with_font(device, allocator, queue, command_pool, font, UI_FRAG_SPV)
    }

    /// Variante para el atlas de IMÁGENES del Canvas (UI v2): la
    /// textura que se sube es el atlas de imágenes dado, no el de
    /// la fuente. Se construye un FontAtlas real solo para ocupar
    /// el campo `font` (con sus píxeles sustituidos por el atlas):
    /// esta instancia nunca maqueta texto ni se leen sus métricas;
    /// `record` solo muestrea la textura.
    pub fn new_for_texture(
        device: Arc<ash::Device>,
        allocator: &mut Allocator,
        queue: vk::Queue,
        command_pool: vk::CommandPool,
        rgba: Vec<u8>,
        size: u32,
    ) -> Result<Self, RendererError> {
        let mut font = FontAtlas::build(FONT_TTF).map_err(RendererError::Descriptor)?;
        font.pixels = rgba;
        font.width = size;
        font.height = size;
        Self::create_with_font(device, allocator, queue, command_pool, font, UI_IMG_FRAG_SPV)
    }

    fn create_with_font(
        device: Arc<ash::Device>,
        allocator: &mut Allocator,
        queue: vk::Queue,
        command_pool: vk::CommandPool,
        font: FontAtlas,
        frag_spv: &[u8],
    ) -> Result<Self, RendererError> {
        unsafe {
            // --- 1. Atlas a VRAM (mismo patrón de subida en un envío que
            // las texturas glTF de mesh.rs, sin mip-maps) ---
            let byte_size = font.pixels.len() as u64;
            let (stage_buf, stage_alloc) = create_ui_buffer(
                &device,
                allocator,
                byte_size,
                vk::BufferUsageFlags::TRANSFER_SRC,
                MemoryLocation::CpuToGpu,
                "ui_font_staging",
            )?;
            if let Some(ptr) = stage_alloc.mapped_ptr() {
                std::ptr::copy_nonoverlapping(
                    font.pixels.as_ptr(),
                    ptr.as_ptr() as *mut u8,
                    font.pixels.len(),
                );
            }

            let image_info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::R8G8B8A8_UNORM)
                .extent(vk::Extent3D {
                    width: font.width,
                    height: font.height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);
            let atlas_image = device.create_image(&image_info, None)?;
            let mem_reqs = device.get_image_memory_requirements(atlas_image);
            let atlas_allocation = allocator.allocate(&AllocationCreateDesc {
                name: "ui_font_atlas",
                requirements: mem_reqs,
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })?;
            device.bind_image_memory(
                atlas_image,
                atlas_allocation.memory(),
                atlas_allocation.offset(),
            )?;

            record_one_time_submit(&device, queue, command_pool, |cmd| {
                let subresource = vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .level_count(1)
                    .layer_count(1);
                let to_dst = [vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::empty())
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .image(atlas_image)
                    .subresource_range(subresource)];
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &to_dst,
                );
                let region = [vk::BufferImageCopy::default()
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .image_extent(vk::Extent3D {
                        width: font.width,
                        height: font.height,
                        depth: 1,
                    })];
                device.cmd_copy_buffer_to_image(
                    cmd,
                    stage_buf,
                    atlas_image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &region,
                );
                let to_read = [vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .image(atlas_image)
                    .subresource_range(subresource)];
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &to_read,
                );
            })?;

            let _ = allocator.free(stage_alloc);
            device.destroy_buffer(stage_buf, None);

            let atlas_view = device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(atlas_image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .subresource_range(
                        vk::ImageSubresourceRange::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .level_count(1)
                            .layer_count(1),
                    ),
                None,
            )?;

            let sampler = device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .mipmap_mode(vk::SamplerMipmapMode::NEAREST),
                None,
            )?;

            // --- 2. Descriptor set del atlas ---
            let binding = [vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
            let descriptor_set_layout = device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&binding),
                None,
            )?;

            let pool_size = [vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)];
            let descriptor_pool = device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(1)
                    .pool_sizes(&pool_size),
                None,
            )?;
            let set_layouts = [descriptor_set_layout];
            let descriptor_set = device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(descriptor_pool)
                    .set_layouts(&set_layouts),
            )?[0];

            let image_info_desc = [vk::DescriptorImageInfo::default()
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .image_view(atlas_view)
                .sampler(sampler)];
            let write = [vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&image_info_desc)];
            device.update_descriptor_sets(&write, &[]);

            // --- 3. Render pass con LOAD sobre el framebuffer offscreen.
            // Debe ser compatible con el framebuffer de OffscreenTarget
            // (mismos formatos): color R8G8B8A8_UNORM + profundidad
            // D32_SFLOAT (presente pero sin uso: la UI no testea depth).
            let attachments = [
                vk::AttachmentDescription::default()
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::LOAD)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .final_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
                vk::AttachmentDescription::default()
                    .format(vk::Format::D32_SFLOAT)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                    .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
            ];
            let color_ref = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            let depth_ref = vk::AttachmentReference::default()
                .attachment(1)
                .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
            let subpasses = [vk::SubpassDescription::default()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(&color_ref)
                .depth_stencil_attachment(&depth_ref)];
            let dependencies = [
                vk::SubpassDependency::default()
                    .src_subpass(vk::SUBPASS_EXTERNAL)
                    .dst_subpass(0)
                    .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                    .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                    .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                    .dst_access_mask(
                        vk::AccessFlags::COLOR_ATTACHMENT_READ
                            | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                    ),
                vk::SubpassDependency::default()
                    .src_subpass(0)
                    .dst_subpass(vk::SUBPASS_EXTERNAL)
                    .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                    .dst_stage_mask(vk::PipelineStageFlags::FRAGMENT_SHADER)
                    .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ),
            ];
            let render_pass = device.create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(&attachments)
                    .subpasses(&subpasses)
                    .dependencies(&dependencies),
                None,
            )?;

            // --- 4. Pipeline 2D: sin depth, con mezcla alfa ---
            let vert_module = create_shader_module(&device, UI_VERT_SPV)?;
            let frag_module = create_shader_module(&device, frag_spv)?;
            let entry_point = c"main";
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(vert_module)
                    .name(entry_point),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(frag_module)
                    .name(entry_point),
            ];

            let binding_desc = [vk::VertexInputBindingDescription::default()
                .binding(0)
                .stride(std::mem::size_of::<UiVertex>() as u32)
                .input_rate(vk::VertexInputRate::VERTEX)];
            let attr_desc = [
                vk::VertexInputAttributeDescription::default()
                    .location(0)
                    .binding(0)
                    .format(vk::Format::R32G32_SFLOAT)
                    .offset(0),
                vk::VertexInputAttributeDescription::default()
                    .location(1)
                    .binding(0)
                    .format(vk::Format::R32G32_SFLOAT)
                    .offset(8),
                vk::VertexInputAttributeDescription::default()
                    .location(2)
                    .binding(0)
                    .format(vk::Format::R32G32B32A32_SFLOAT)
                    .offset(16),
            ];
            let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
                .vertex_binding_descriptions(&binding_desc)
                .vertex_attribute_descriptions(&attr_desc);
            let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
                .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
                .primitive_restart_enable(false);
            let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
            let dynamic_state =
                vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
            let viewport_state = vk::PipelineViewportStateCreateInfo::default()
                .viewport_count(1)
                .scissor_count(1);
            let rasterizer = vk::PipelineRasterizationStateCreateInfo::default()
                .depth_clamp_enable(false)
                .rasterizer_discard_enable(false)
                .polygon_mode(vk::PolygonMode::FILL)
                .line_width(1.0)
                .cull_mode(vk::CullModeFlags::NONE)
                .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
                .depth_bias_enable(false);
            let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
                .sample_shading_enable(false)
                .rasterization_samples(vk::SampleCountFlags::TYPE_1);
            let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
                .depth_test_enable(false)
                .depth_write_enable(false)
                .stencil_test_enable(false);
            let blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(vk::ColorComponentFlags::RGBA)
                .blend_enable(true)
                .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .alpha_blend_op(vk::BlendOp::ADD)];
            let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
                .logic_op_enable(false)
                .attachments(&blend_attachment);

            let push_range = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX)
                .offset(0)
                .size(16)];
            let layouts = [descriptor_set_layout];
            let pipeline_layout = device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&layouts)
                    .push_constant_ranges(&push_range),
                None,
            )?;

            let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
                .stages(&stages)
                .vertex_input_state(&vertex_input)
                .input_assembly_state(&input_assembly)
                .viewport_state(&viewport_state)
                .rasterization_state(&rasterizer)
                .multisample_state(&multisampling)
                .depth_stencil_state(&depth_stencil)
                .color_blend_state(&color_blending)
                .dynamic_state(&dynamic_state)
                .layout(pipeline_layout)
                .render_pass(render_pass)
                .subpass(0)];
            let pipelines = device
                .create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_info, None)
                .map_err(|(_, e)| RendererError::Driver(e))?;
            let pipeline = pipelines[0];
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_module, None);

            // --- 5. Buffer de vértices persistente CPU→GPU ---
            let (vertex_buffer, vertex_allocation) = create_ui_buffer(
                &device,
                allocator,
                (MAX_UI_VERTICES * std::mem::size_of::<UiVertex>()) as u64,
                vk::BufferUsageFlags::VERTEX_BUFFER,
                MemoryLocation::CpuToGpu,
                "ui_vertex_buffer",
            )?;

            log::info!(
                "UiRenderer inicializado: atlas de fuente {}x{} ({} glifos), pipeline 2D listo",
                font.width,
                font.height,
                font.glyphs.len()
            );

            Ok(Self {
                device,
                font,
                render_pass,
                pipeline,
                pipeline_layout,
                descriptor_set_layout,
                descriptor_pool,
                descriptor_set,
                atlas_image,
                atlas_allocation: Some(atlas_allocation),
                atlas_view,
                sampler,
                vertex_buffer,
                vertex_allocation: Some(vertex_allocation),
            })
        }
    }

    /// Las métricas de la fuente, para construir lotes en la app.
    pub fn font(&self) -> &FontAtlas {
        &self.font
    }

    /// Graba el lote de UI sobre el framebuffer del target dentro del
    /// command buffer dado (ya comenzado). El llamador gestiona el
    /// fence/envío, siguiendo el patrón serializado del renderer.
    ///
    /// Escribe el buffer de vértices directamente desde CPU: es seguro
    /// porque el llamador espera el fence del frame anterior antes de
    /// grabar (ver `GlassRenderer::render_ui_overlay`).
    pub fn record(&mut self, cmd: vk::CommandBuffer, target: &OffscreenTarget, batch: &UiBatch) {
        let count = batch.vertex_count().min(MAX_UI_VERTICES);
        if count < batch.vertex_count() {
            log::warn!(
                "Lote de UI truncado: {} vértices pedidos, capacidad {}",
                batch.vertex_count(),
                MAX_UI_VERTICES
            );
        }
        if count == 0 {
            return;
        }

        unsafe {
            if let Some(alloc) = &self.vertex_allocation {
                if let Some(ptr) = alloc.mapped_ptr() {
                    std::ptr::copy_nonoverlapping(
                        batch.vertices().as_ptr(),
                        ptr.as_ptr() as *mut UiVertex,
                        count,
                    );
                }
            }

            let begin_info = vk::RenderPassBeginInfo::default()
                .render_pass(self.render_pass)
                .framebuffer(target.framebuffer)
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: target.extent,
                });
            self.device
                .cmd_begin_render_pass(cmd, &begin_info, vk::SubpassContents::INLINE);

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

            self.device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline,
            );
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[self.descriptor_set],
                &[],
            );
            let push: [f32; 4] = [
                target.extent.width as f32,
                target.extent.height as f32,
                0.0,
                0.0,
            ];
            self.device.cmd_push_constants(
                cmd,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                bytemuck::cast_slice(&push),
            );
            self.device
                .cmd_bind_vertex_buffers(cmd, 0, &[self.vertex_buffer], &[0]);
            self.device.cmd_draw(cmd, count as u32, 1, 0, 0);

            self.device.cmd_end_render_pass(cmd);
        }
    }

    pub fn destroy(&mut self, allocator: &mut Allocator) {
        unsafe {
            self.device.destroy_pipeline(self.pipeline, None);
            self.device.destroy_pipeline_layout(self.pipeline_layout, None);
            self.device.destroy_render_pass(self.render_pass, None);
            self.device.destroy_descriptor_pool(self.descriptor_pool, None);
            self.device
                .destroy_descriptor_set_layout(self.descriptor_set_layout, None);
            self.device.destroy_sampler(self.sampler, None);
            self.device.destroy_image_view(self.atlas_view, None);
            self.device.destroy_image(self.atlas_image, None);
            self.device.destroy_buffer(self.vertex_buffer, None);
        }
        if let Some(alloc) = self.atlas_allocation.take() {
            allocator.free(alloc).unwrap_or(());
        }
        if let Some(alloc) = self.vertex_allocation.take() {
            allocator.free(alloc).unwrap_or(());
        }
    }
}

/// Crea un buffer con el asignador del motor (espejo del patrón de
/// `MeshLoader::create_buffer` en mesh.rs, que no es público).
fn create_ui_buffer(
    device: &ash::Device,
    allocator: &mut Allocator,
    size: u64,
    usage: vk::BufferUsageFlags,
    location: MemoryLocation,
    name: &'static str,
) -> Result<(vk::Buffer, Allocation), RendererError> {
    unsafe {
        let buffer_info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = device.create_buffer(&buffer_info, None)?;
        let mem_reqs = device.get_buffer_memory_requirements(buffer);
        let allocation = allocator.allocate(&AllocationCreateDesc {
            name,
            requirements: mem_reqs,
            location,
            linear: location == MemoryLocation::CpuToGpu,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })?;
        device.bind_buffer_memory(buffer, allocation.memory(), allocation.offset())?;
        Ok((buffer, allocation))
    }
}

fn create_shader_module(device: &ash::Device, spv_bytes: &[u8]) -> Result<vk::ShaderModule, RendererError> {
    if spv_bytes.len() % 4 != 0 {
        return Err(RendererError::Descriptor("SPIR-V de UI con tamaño no múltiplo de 4".into()));
    }
    let words: Vec<u32> = spv_bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let info = vk::ShaderModuleCreateInfo::default().code(&words);
    Ok(unsafe { device.create_shader_module(&info, None)? })
}

/// Envío de un solo uso con espera (inicialización de recursos GPU;
/// mismo patrón que `upload_texture_to_gpu` en mesh.rs).
unsafe fn record_one_time_submit(
    device: &ash::Device,
    queue: vk::Queue,
    command_pool: vk::CommandPool,
    record: impl FnOnce(vk::CommandBuffer),
) -> Result<(), RendererError> {
    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    let cmd = device.allocate_command_buffers(&alloc_info)?[0];
    let begin_info = vk::CommandBufferBeginInfo::default()
        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    device.begin_command_buffer(cmd, &begin_info)?;
    record(cmd);
    device.end_command_buffer(cmd)?;
    let fence = device.create_fence(&vk::FenceCreateInfo::default(), None)?;
    let command_buffers = [cmd];
    let submit_info = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
    device.queue_submit(queue, &submit_info, fence)?;
    device.wait_for_fences(&[fence], true, u64::MAX)?;
    device.destroy_fence(fence, None);
    device.free_command_buffers(command_pool, &[cmd]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_contiene_ascii_y_acentos() {
        let atlas = FontAtlas::build(FONT_TTF).expect("la fuente incrustada debe rasterizar");
        assert!(atlas.glyph('A').is_some());
        assert!(atlas.glyph('á').is_some(), "Latin-1: á debe estar en el atlas");
        assert!(atlas.glyph('ñ').is_some());
        assert!(atlas.glyph('¿').is_some());
        assert!(atlas.ascent > 0.0);
        assert!(atlas.line_height > atlas.ascent);
        // El espacio avanza pero no dibuja quad.
        let space = atlas.glyph(' ').expect("espacio");
        assert!(space.advance > 0.0);
        assert_eq!(space.size, [0.0, 0.0]);
    }

    #[test]
    fn medir_y_lote_de_texto() {
        let atlas = FontAtlas::build(FONT_TTF).expect("atlas");
        let w = atlas.measure("Score: 0", 22.0);
        assert!(w > 40.0 && w < 200.0, "ancho medido razonable: {w}");
        let mut batch = UiBatch::new();
        batch.push_text(&atlas, [20.0, 660.0], "Score: 0", 22.0, [1.0, 0.9, 0.2, 1.0]);
        // "Score: 0" tiene 7 caracteres con glifo visible (el espacio no
        // emite quad): 7 quads × 6 vértices.
        assert_eq!(batch.vertex_count(), 7 * 6);
        // El primer vértice (S) debe caer dentro del área del texto.
        let v = batch.vertices()[0];
        assert!(v.pos[0] >= 20.0 && v.pos[0] < 60.0, "x del primer glifo: {}", v.pos[0]);
        assert!(v.pos[1] >= 660.0 && v.pos[1] < 700.0, "y del primer glifo: {}", v.pos[1]);
    }

    #[test]
    fn lote_solid_y_centrado() {
        let atlas = FontAtlas::build(FONT_TTF).expect("atlas");
        let mut batch = UiBatch::new();
        batch.push_solid(&atlas, [10.0, 10.0, 100.0, 40.0], [0.25, 0.55, 0.85, 1.0]);
        assert_eq!(batch.vertex_count(), 6);
        let v = batch.vertices()[0];
        assert_eq!(v.pos, [10.0, 10.0]);
        assert_eq!(v.uv, atlas.solid_uv);
        batch.push_text_centered(&atlas, [10.0, 10.0, 100.0, 40.0], "OK", 14.0, [1.0; 4]);
        assert_eq!(batch.vertex_count(), 6 + 2 * 6);
    }
}

/// Atlas cuadrado de imágenes de UI empaquetadas por estanterías.
pub struct UiImageAtlas {
    /// Lado del atlas cuadrado en píxeles.
    pub size: u32,
    /// Píxeles RGBA8 del atlas completo (size*size*4).
    pub rgba: Vec<u8>,
    /// UV [u0, v0, u1, v1] (0..1) de cada imagen, en el mismo
    /// orden que la entrada.
    pub uvs: Vec<[f32; 4]>,
}

/// Intenta empaquetar las imágenes en un atlas de lado `size`.
/// Devuelve las posiciones en píxeles y las UVs normalizadas.
fn try_pack_ui_images(
    images: &[(u32, u32, &[u8])],
    size: u32,
) -> Option<(Vec<(u32, u32)>, Vec<[f32; 4]>)> {
    const BORDE: u64 = 2;
    const HUECO: u64 = 2;

    let lado = size as u64;
    let mut x = BORDE;
    let mut y = BORDE;
    let mut alto_estante = 0u64;
    let mut posiciones = Vec::with_capacity(images.len());
    let mut uvs = Vec::with_capacity(images.len());

    for &(w, h, _) in images {
        let ancho = w as u64;
        let alto = h as u64;

        // Debe caber al menos en una fila/columna con sus bordes.
        if ancho + BORDE * 2 > lado || alto + BORDE * 2 > lado {
            return None;
        }

        // Si no cabe en el estante actual, se abre uno nuevo.
        if x + ancho + BORDE > lado {
            y += alto_estante + HUECO;
            x = BORDE;
            alto_estante = 0;
        }

        // Comprobación final de que cabe en el atlas.
        if x + ancho + BORDE > lado || y + alto + BORDE > lado {
            return None;
        }

        posiciones.push((x as u32, y as u32));

        let s = size as f32;
        uvs.push([
            x as f32 / s,
            y as f32 / s,
            (x + ancho) as f32 / s,
            (y + alto) as f32 / s,
        ]);

        x += ancho + HUECO;
        if alto > alto_estante {
            alto_estante = alto;
        }
    }

    Some((posiciones, uvs))
}

/// Empaqueta imágenes RGBA8 en un atlas cuadrado por estanterías.
pub fn pack_ui_image_atlas(
    images: &[(u32, u32, &[u8])],
    max_size: u32,
) -> Option<UiImageAtlas> {
    if images.is_empty() {
        return None;
    }

    // Validación de dimensiones y longitudes de píxeles.
    for &(w, h, px) in images {
        if w == 0 || h == 0 {
            return None;
        }

        let esperado = (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4))?;

        if px.len() != esperado {
            return None;
        }
    }

    // Se prueban tamaños potencia de dos desde 256 hasta max_size.
    let mut size = 256u32;
    while size <= max_size {
        if let Some((posiciones, uvs)) = try_pack_ui_images(images, size) {
            let lado = size as usize;
            let total = lado.checked_mul(lado).and_then(|n| n.checked_mul(4))?;
            let mut rgba = vec![0u8; total];

            for (img, &(px_x, px_y)) in images.iter().zip(posiciones.iter()) {
                let (w, h, px) = *img;
                let ancho = w as usize;
                let alto = h as usize;
                let fila_bytes = ancho * 4;

                for fila in 0..alto {
                    let origen = fila * fila_bytes;
                    let destino =
                        (((px_y as usize + fila) * lado) + px_x as usize) * 4;

                    rgba[destino..destino + fila_bytes]
                        .copy_from_slice(&px[origen..origen + fila_bytes]);
                }
            }

            return Some(UiImageAtlas { size, rgba, uvs });
        }

        size = match size.checked_mul(2) {
            Some(s) => s,
            None => break,
        };
    }

    None
}

/// Tipo de atlas usado por un segmento de dibujo.
/// Tipo de textura que usa un lote de UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAtlasKind {
    /// Fuente incrustada por defecto.
    Font,
    /// Imagen cargada como atlas.
    Image,
    /// Fuente con nombre ubicada en assets/fonts.
    FontNamed(String),
}

/// Segmento de dibujo que conserva el orden de pintor y el atlas usado.
pub struct UiSegment {
    pub batch: UiBatch,
    pub atlas: UiAtlasKind,
}

impl UiBatch {
    /// Quad de imagen: `rect` en píxeles de pantalla y `uv` la
    /// sub-región del atlas de imágenes; `color` es el tinte.
    pub fn push_image(&mut self, rect: [f32; 4], uv: [f32; 4], color: [f32; 4]) {
        self.push_quad(rect, uv, color);
    }
}

#[cfg(test)]
mod tests_img2 {
    use super::*;

    fn solid(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
        let n = (w as usize) * (h as usize);
        let mut v = Vec::with_capacity(n * 4);
        for _ in 0..n {
            v.extend_from_slice(&color);
        }
        v
    }

    fn pixel(atlas: &UiImageAtlas, x: u32, y: u32) -> [u8; 4] {
        let i = ((y as usize * atlas.size as usize) + x as usize) * 4;
        [atlas.rgba[i], atlas.rgba[i + 1], atlas.rgba[i + 2], atlas.rgba[i + 3]]
    }

    fn centro(atlas: &UiImageAtlas, uv: [f32; 4]) -> [u8; 4] {
        let s = atlas.size as f32;
        let x = ((uv[0] + uv[2]) * 0.5 * s) as u32;
        let y = ((uv[1] + uv[3]) * 0.5 * s) as u32;
        pixel(atlas, x, y)
    }

    #[test]
    fn empaqueta_dos_imagenes_sin_solapar() {
        let roja = solid(4, 4, [255, 0, 0, 255]);
        let verde = solid(2, 6, [0, 255, 0, 255]);
        let imgs: Vec<(u32, u32, &[u8])> = vec![(4, 4, &roja), (2, 6, &verde)];
        let atlas = pack_ui_image_atlas(&imgs, 2048).expect("debe caber");
        assert_eq!(atlas.uvs.len(), 2);
        for uv in &atlas.uvs {
            assert!(uv[0] >= 0.0 && uv[2] <= 1.0 && uv[1] >= 0.0 && uv[3] <= 1.0);
            assert!(uv[2] > uv[0] && uv[3] > uv[1]);
        }
        // Sin solape: los rects en píxeles no se cruzan.
        let r = |uv: [f32; 4]| {
            let s = atlas.size as f32;
            [uv[0] * s, uv[1] * s, uv[2] * s, uv[3] * s]
        };
        let (a, b) = (r(atlas.uvs[0]), r(atlas.uvs[1]));
        assert!(a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1]);
        assert_eq!(centro(&atlas, atlas.uvs[0]), [255, 0, 0, 255]);
        assert_eq!(centro(&atlas, atlas.uvs[1]), [0, 255, 0, 255]);
    }

    #[test]
    fn rechaza_oversize_vacio_y_longitud_mala() {
        let grande = solid(300, 300, [9, 9, 9, 255]);
        let imgs: Vec<(u32, u32, &[u8])> = vec![(300, 300, &grande)];
        assert!(pack_ui_image_atlas(&imgs, 256).is_none());
        let vacio: Vec<(u32, u32, &[u8])> = Vec::new();
        assert!(pack_ui_image_atlas(&vacio, 2048).is_none());
        let corta = vec![0u8; 10];
        let malas: Vec<(u32, u32, &[u8])> = vec![(4, 4, &corta)];
        assert!(pack_ui_image_atlas(&malas, 2048).is_none());
    }

    #[test]
    fn push_image_emite_seis_vertices_con_sus_uv() {
        let mut b = UiBatch::new();
        b.push_image([10.0, 20.0, 30.0, 40.0], [0.25, 0.5, 0.75, 1.0], [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(b.vertex_count(), 6);
        let v = b.vertices();
        assert_eq!(v[0].uv, [0.25, 0.5]);
        assert_eq!(v[2].uv, [0.75, 0.5]);
    }
}

impl UiBatch {
    /// Dibuja el texto línea a línea alineado dentro del rect [x, y, ancho, alto].
    pub fn push_text_aligned(
        &mut self,
        atlas: &FontAtlas,
        rect: [f32; 4],
        text: &str,
        font_size: f32,
        color: [f32; 4],
        align: &str,
    ) {
        let x = rect[0];
        let ancho = rect[2];
        let mut y = rect[1];
        let alto_linea = atlas.line_height_at(font_size);

        for linea in text.split('\n') {
            // Medimos la línea para calcular su origen horizontal.
            let ancho_linea = atlas.measure(linea, font_size);
            let x_linea = match align {
                "Center" => x + (ancho - ancho_linea) * 0.5,
                "Right" => x + ancho - ancho_linea,
                _ => x,
            };
            self.push_text(atlas, [x_linea, y], linea, font_size, color);
            // Bajamos a la siguiente línea.
            y += alto_linea;
        }
    }
}

impl UiRenderer {
    /// Construye un `UiRenderer` a partir de los bytes de una fuente.
    pub fn new_with_font_bytes(
        device: Arc<ash::Device>,
        allocator: &mut Allocator,
        queue: vk::Queue,
        command_pool: vk::CommandPool,
        font_bytes: &[u8],
    ) -> Result<Self, RendererError> {
        // Construimos el atlas de fuente mapeando el error de construcción.
        let font = FontAtlas::build(font_bytes).map_err(RendererError::Descriptor)?;
        // Delegamos en la creación común con el fragment shader de fuente.
        Self::create_with_font(device, allocator, queue, command_pool, font, UI_FRAG_SPV)
    }
}

#[cfg(test)]
mod tests_dyn1 {
    use super::*;

    #[test]
    fn alineaciones_producen_x_creciente_y_centrado_correcto() {
        // Atlas con la fuente incrustada.
        let atlas = FontAtlas::build(FONT_TTF).unwrap();
        let font_size = 16.0;
        let rect = [0.0f32, 0.0, 400.0, 100.0];
        let texto = "Hola";
        let color = [1.0, 1.0, 1.0, 1.0];

        // Lotes separados para cada alineación.
        let mut lote_izq = UiBatch::default();
        lote_izq.push_text_aligned(&atlas, rect, texto, font_size, color, "Left");

        let mut lote_centro = UiBatch::default();
        lote_centro.push_text_aligned(&atlas, rect, texto, font_size, color, "Center");

        let mut lote_der = UiBatch::default();
        lote_der.push_text_aligned(&atlas, rect, texto, font_size, color, "Right");

        // Primer vértice de cada lote (incluye el bearing del glifo).
        let x_izq = lote_izq.vertices[0].pos[0];
        let x_centro = lote_centro.vertices[0].pos[0];
        let x_der = lote_der.vertices[0].pos[0];

        assert!(
            x_izq < x_centro,
            "izquierda {} debe ser menor que centro {}",
            x_izq,
            x_centro
        );
        assert!(
            x_centro < x_der,
            "centro {} debe ser menor que derecha {}",
            x_centro,
            x_der
        );

        // Verificamos el centrado con tolerancia amplia por el bearing.
        let ancho_texto = atlas.measure(texto, font_size);
        let centro_rect = rect[0] + rect[2] * 0.5;
        let desvio = (x_centro + ancho_texto * 0.5) - centro_rect;
        assert!(
            desvio.abs() < 6.0,
            "desvío del centrado fuera de tolerancia: {}",
            desvio
        );
    }

    #[test]
    fn font_named_compara_por_nombre() {
        let a = UiAtlasKind::FontNamed("pixel".to_string());
        let b = UiAtlasKind::FontNamed("pixel".to_string());
        // Mismo nombre => iguales.
        assert_eq!(a, b);
        // Distinto de la variante sin nombre.
        assert_ne!(a, UiAtlasKind::Font);
    }
}
