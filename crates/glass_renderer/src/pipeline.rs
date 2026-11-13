use ash::vk;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PipelineError {
    #[error("Vulkan driver error: {0}")]
    Driver(#[from] vk::Result),
    #[error("Failed to load shader bytes")]
    ShaderLoadError,
}

pub struct GraphicsPipeline {
    pub device: Arc<ash::Device>,
    pub layout: vk::PipelineLayout,
    pub pipeline: vk::Pipeline,
    /// Layout del set 0 (sampler de la textura base), solo en los pipelines
    /// de malla (`new_mesh_pipeline`); el pipeline de pantalla completa no
    /// muestrea texturas por descriptor y lo deja en `None`.
    pub descriptor_set_layout: Option<vk::DescriptorSetLayout>,
    /// Layout del set 1 cuando el pipeline consume un set extra (hoy solo
    /// el pipeline con piel: el SSBO de matrices de articulación).
    /// `None` en todos los demás pipelines.
    pub joints_set_layout: Option<vk::DescriptorSetLayout>,
    /// Layout del set 1 en los pipelines de GRAFO: el UBO global
    /// `GraphGlobals` (punto de interacción). Es un set DISTINTO del de
    /// articulaciones aunque comparta índice: cada pipeline declara el
    /// suyo y el bucle de dibujo enlaza el que toca según el pipeline.
    /// `None` fuera de los pipelines de grafo.
    pub globals_set_layout: Option<vk::DescriptorSetLayout>,
    /// Layout del set 2 (UBO de frame + sampler de comparación del mapa
    /// de sombras) en los pipelines que reciben sombra: PBR y piel. El
    /// layout lo crea y posee `shadow::ShadowResources`; el pipeline
    /// solo lo referencia (no se destruye en el Drop, como el de
    /// globales de grafo). En los pipelines de grafo el set 2 es un UBO
    /// vacío de relleno (ver el módulo `shadow`).
    pub shadow_set_layout: Option<vk::DescriptorSetLayout>,
    /// Layout del set 3 (UBO de luces puntuales) en los pipelines que
    /// las aplican: PBR y piel. `None` en mallas Lambert, grafos y
    /// pantalla completa. El set lo posee el renderer y se enlaza en
    /// cada dibujo de esos pipelines.
    pub point_lights_set_layout: Option<vk::DescriptorSetLayout>,
    /// Sets 4 (textura del atlas) y 5 (UBO de grupos) del lightmap
    /// horneado: solo los pipelines cuyo fragmento es el PBR los
    /// declaran; el resto conserva su layout de siempre.
    pub lm_set_layouts: Option<(vk::DescriptorSetLayout, vk::DescriptorSetLayout)>,
    /// Set 6 (IBL de skybox): cubemap de entorno + UBO de parámetros.
    /// Solo los pipelines cuyo fragmento es el PBR lo declaran.
    pub ibl_set_layout: Option<vk::DescriptorSetLayout>,
}

impl GraphicsPipeline {
    pub fn new(
        device: Arc<ash::Device>,
        render_pass: vk::RenderPass,
        vert_spv: &[u8],
        frag_spv: &[u8],
    ) -> Result<Self, PipelineError> {
        let vert_module = Self::create_shader_module(&device, vert_spv)?;
        let frag_module = Self::create_shader_module(&device, frag_spv)?;

        let entry_point = c"main";

        let shader_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vert_module)
                .name(entry_point),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(frag_module)
                .name(entry_point),
        ];

        // 1. Vertex Input (usamos gl_VertexIndex, no requiere buffers de vértices)
        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default();

        // 2. Input Assembly
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
            .primitive_restart_enable(false);

        // 3. Dynamic Viewport & Scissor
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

        let viewport_state_info = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);

        // 4. Rasterizer
        let rasterizer = vk::PipelineRasterizationStateCreateInfo::default()
            .depth_clamp_enable(false)
            .rasterizer_discard_enable(false)
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::CLOCKWISE)
            .depth_bias_enable(false);

        // 5. Multisampling
        let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
            .sample_shading_enable(false)
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);

        // 6. Depth Stencil
        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL)
            .depth_bounds_test_enable(false)
            .stencil_test_enable(false);

        // 7. Color Blending
        let color_blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(false)];

        let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
            .logic_op_enable(false)
            .attachments(&color_blend_attachment);

        // 8. Pipeline Layout
        let layout_info = vk::PipelineLayoutCreateInfo::default();
        let layout = unsafe { device.create_pipeline_layout(&layout_info, None)? };

        // 9. Pipeline Creation
        let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state_info)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&color_blending)
            .dynamic_state(&dynamic_state_info)
            .layout(layout)
            .render_pass(render_pass)
            .subpass(0)];

        let pipelines = unsafe {
            device
                .create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_info, None)
                .map_err(|(_, err)| PipelineError::Driver(err))?
        };
        let pipeline = pipelines[0];

        // Limpiar módulos de shader una vez creado el pipeline
        unsafe {
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_module, None);
        }

        log::info!("GraphicsPipeline creado exitosamente con depth-testing.");

        Ok(Self {
            device,
            layout,
            pipeline,
            descriptor_set_layout: None,
            joints_set_layout: None,
            globals_set_layout: None,
            shadow_set_layout: None,
            point_lights_set_layout: None,
            lm_set_layouts: None,
            ibl_set_layout: None,
        })
    }

    /// Crea un pipeline gráfico con soporte de atributos de vértices y matriz MVP (PushConstants) para mallas 3D
    pub fn new_mesh_pipeline(
        device: Arc<ash::Device>,
        render_pass: vk::RenderPass,
        vert_spv: &[u8],
        frag_spv: &[u8],
        shadow_set_layout: Option<vk::DescriptorSetLayout>,
        point_lights_set_layout: Option<vk::DescriptorSetLayout>,
        lm_set_layouts: Option<(vk::DescriptorSetLayout, vk::DescriptorSetLayout)>,
        ibl_set_layout: Option<vk::DescriptorSetLayout>,
    ) -> Result<Self, PipelineError> {
        Self::new_mesh_pipeline_impl(device, render_pass, vert_spv, frag_spv, None, false, shadow_set_layout, point_lights_set_layout, lm_set_layouts, ibl_set_layout)
    }

    /// Igual que `new_mesh_pipeline` pero transparente (mezcla por
    /// alfa y sin escritura de profundidad): la variante del PBR
    /// para los materiales con `transparent: true` (Materiales v1).
    pub fn new_mesh_pipeline_transparent(
        device: Arc<ash::Device>,
        render_pass: vk::RenderPass,
        vert_spv: &[u8],
        frag_spv: &[u8],
        shadow_set_layout: Option<vk::DescriptorSetLayout>,
        point_lights_set_layout: Option<vk::DescriptorSetLayout>,
        lm_set_layouts: Option<(vk::DescriptorSetLayout, vk::DescriptorSetLayout)>,
        ibl_set_layout: Option<vk::DescriptorSetLayout>,
    ) -> Result<Self, PipelineError> {
        Self::new_mesh_pipeline_impl(device, render_pass, vert_spv, frag_spv, None, true, shadow_set_layout, point_lights_set_layout, lm_set_layouts, ibl_set_layout)
    }

    /// Pipeline de un material de GRAFO: idéntico al de mallas (mismo
    /// formato de vértice, mismos push constants, mismo set 0) más el
    /// set 1 con el UBO global de los grafos (`GraphGlobals`), cuyo
    /// layout crea y posee el Renderer y se comparte entre todos los
    /// pipelines de grafo. Los shaders generados lo declaran siempre;
    /// los que no usan el nodo de interacción simplemente no lo leen.
    ///
    /// Con `transparent` el pipeline cambia SOLO en dos estados: mezcla
    /// alfa clásica (SRC_ALPHA / ONE_MINUS_SRC_ALPHA) y SIN escritura
    /// de profundidad — el comportamiento de una calcomanía: se
    /// compone sobre lo ya dibujado y no ocluye a lo que venga detrás.
    /// Sin ese flag, el estado es el opaco de siempre, bit a bit.
    pub fn new_graph_pipeline(
        device: Arc<ash::Device>,
        render_pass: vk::RenderPass,
        vert_spv: &[u8],
        frag_spv: &[u8],
        globals_set_layout: vk::DescriptorSetLayout,
        transparent: bool,
        shadow_set_layout: Option<vk::DescriptorSetLayout>,
        point_lights_set_layout: Option<vk::DescriptorSetLayout>,
    ) -> Result<Self, PipelineError> {
        Self::new_mesh_pipeline_impl(
            device,
            render_pass,
            vert_spv,
            frag_spv,
            Some(globals_set_layout),
            transparent,
            shadow_set_layout,
            point_lights_set_layout,
            None,
            None,
        )
    }

    fn new_mesh_pipeline_impl(
        device: Arc<ash::Device>,
        render_pass: vk::RenderPass,
        vert_spv: &[u8],
        frag_spv: &[u8],
        globals_set_layout: Option<vk::DescriptorSetLayout>,
        transparent: bool,
        shadow_set_layout: Option<vk::DescriptorSetLayout>,
        point_lights_set_layout: Option<vk::DescriptorSetLayout>,
        lm_set_layouts: Option<(vk::DescriptorSetLayout, vk::DescriptorSetLayout)>,
        ibl_set_layout: Option<vk::DescriptorSetLayout>,
    ) -> Result<Self, PipelineError> {
        let vert_module = Self::create_shader_module(&device, vert_spv)?;
        let frag_module = Self::create_shader_module(&device, frag_spv)?;

        let entry_point = c"main";

        let shader_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vert_module)
                .name(entry_point),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(frag_module)
                .name(entry_point),
        ];

        let binding_descriptions = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(std::mem::size_of::<crate::mesh::Vertex>() as u32)
            .input_rate(vk::VertexInputRate::VERTEX)];

        let attribute_descriptions = [
            vk::VertexInputAttributeDescription::default()
                .location(0)
                .binding(0)
                .format(vk::Format::R32G32B32_SFLOAT)
                .offset(0),
            vk::VertexInputAttributeDescription::default()
                .location(1)
                .binding(0)
                .format(vk::Format::R32G32B32_SFLOAT)
                .offset(12),
            vk::VertexInputAttributeDescription::default()
                .location(2)
                .binding(0)
                .format(vk::Format::R32G32_SFLOAT)
                .offset(24),
        ];

        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&binding_descriptions)
            .vertex_attribute_descriptions(&attribute_descriptions);

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
            .primitive_restart_enable(false);

        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

        let viewport_state_info = vk::PipelineViewportStateCreateInfo::default()
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

        // Un pipeline transparente no escribe profundidad: sus
        // fragmentos semi-transparentes no deben ocluir a la geometría
        // que se dibuje después (si escribiera, las esquinas con alfa
        // 0 de un quad de espuma taparían lo de detrás con un cuadrado
        // invisible). El test de profundidad se queda: lo ya dibujado
        // (el agua, justo antes en el orden de la escena) sí lo ocluye.
        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(!transparent)
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL)
            .depth_bounds_test_enable(false)
            .stencil_test_enable(false);

        // Los factores de mezcla solo se fijan si hay mezcla: con
        // blend_enable(false) el estado opaco queda bit a bit el de
        // siempre.
        let mut blend_attachment = vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(transparent);
        if transparent {
            blend_attachment = blend_attachment
                .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .alpha_blend_op(vk::BlendOp::ADD);
        }
        let color_blend_attachment = [blend_attachment];

        let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
            .logic_op_enable(false)
            .attachments(&color_blend_attachment);

        let push_constant_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .offset(0)
            .size(crate::renderer::MESH_PUSH_CONSTANTS_SIZE)];


        // Set 0: la(s) textura(s) muestreadas por el fragment shader.
        // El PBR y las mallas clásicas solo declaran baseColor (binding 0),
        // un único COMBINED_IMAGE_SAMPLER como siempre. Los pipelines de
        // grafo (globals_set_layout.is_some()) reservan 4 bindings
        // (0..=3) para las hasta 4 texturas que un grafo de shader puede
        // declarar; el grafo que solo use la 0 simplemente no enlaza las
        // demás, pero el layout debe reflejar el máximo posible.
        let sampler_bindings: Vec<vk::DescriptorSetLayoutBinding> = if globals_set_layout.is_some() {
            (0..=3)
                .map(|binding| {
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(binding)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                })
                .collect()
        } else {
            vec![vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)]
        };
        let set_layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&sampler_bindings);
        let descriptor_set_layout =
            unsafe { device.create_descriptor_set_layout(&set_layout_info, None)? };
        // Set 0 siempre; el set 1 (globales de grafo) solo cuando el
        // llamante lo pide — el PBR y las mallas conservan su layout de
        // un solo set, byte a byte el comportamiento de siempre.
        let mut set_layouts = vec![descriptor_set_layout];
        if let Some(globals) = globals_set_layout {
            set_layouts.push(globals);
        }
        // Set 2 (sombras): en el PBR es el set real (UBO + sampler de
        // comparación); en los grafos, el UBO vacío de relleno. Los
        // pipelines sin este parámetro conservan su layout de siempre.
        if let Some(shadow) = shadow_set_layout {
            // Un pipeline de grafo ocupa el set 1 con sus globales; el
            // PBR no tiene set 1 pero su layout declara igualmente el
            // set 2 en la posición 2 — para eso el hueco del set 1 se
            // rellena con el MISMO layout de globales si el llamante lo
            // trae, o con un hueco vacío si no. El PBR nunca enlaza el
            // set 1: su shader no lo declara.
            if globals_set_layout.is_none() {
                // Hueco del set 1 para el PBR: se reutiliza el layout de
                // textura (nunca se enlaza ni se muestrea; solo ocupa
                // la posición para que el set 2 caiga en su índice).
                set_layouts.push(descriptor_set_layout);
            }
            set_layouts.push(shadow);
        }
        // Set 3 (luces puntuales): solo PBR. Si un llamante lo pidiera
        // sin set 2, el hueco se rellena como el del set 1 para que el
        // índice no se desplace.
        if let Some(pl) = point_lights_set_layout {
            if shadow_set_layout.is_none() {
                if globals_set_layout.is_none() {
                    set_layouts.push(descriptor_set_layout);
                }
                set_layouts.push(descriptor_set_layout);
            }
            set_layouts.push(pl);
        }
        // Sets 4 y 5 (lightmap): solo el PBR los pide; los huecos se
        // rellenan como en los sets anteriores para no desplazarlos.
        if let Some((lm_tex, lm_data)) = lm_set_layouts {
            while set_layouts.len() < 4 {
                set_layouts.push(descriptor_set_layout);
            }
            set_layouts.push(lm_tex);
            set_layouts.push(lm_data);
        }

        // Set 6 (IBL): solo el PBR lo pide; huecos rellenados igual.
        if let Some(ibl) = ibl_set_layout {
            while set_layouts.len() < 6 {
                set_layouts.push(descriptor_set_layout);
            }
            set_layouts.push(ibl);
        }

        let layout_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&set_layouts)
            .push_constant_ranges(&push_constant_ranges);
        let layout = unsafe { device.create_pipeline_layout(&layout_info, None)? };

        let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state_info)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&color_blending)
            .dynamic_state(&dynamic_state_info)
            .layout(layout)
            .render_pass(render_pass)
            .subpass(0)];

        let pipelines = unsafe {
            device
                .create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_info, None)
                .map_err(|(_, err)| PipelineError::Driver(err))?
        };

        unsafe {
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_module, None);
        }

        Ok(Self {
            device,
            layout,
            pipeline: pipelines[0],
            descriptor_set_layout: Some(descriptor_set_layout),
            joints_set_layout: None,
            globals_set_layout,
            shadow_set_layout,
            point_lights_set_layout,
            lm_set_layouts,
            ibl_set_layout,
        })
    }

    /// Pipeline de mallas CON piel: vértice `SkinnedVertex` (posición,
    /// normal, uv, articulaciones u16×4 en location 3, pesos f32×4 en
    /// location 4) y DOS sets — el 0 es la textura base con el MISMO
    /// contenido de layout que el del PBR normal (los sets de textura de
    /// la caché se enlazan tal cual) y el 1 es el SSBO de matrices de
    /// articulación, leído en la etapa de vértices. Mismos push constants
    /// y mismo fragment (pbr.frag) que el pipeline normal.
    pub fn new_skinned_pipeline(
        device: Arc<ash::Device>,
        render_pass: vk::RenderPass,
        vert_spv: &[u8],
        frag_spv: &[u8],
        shadow_set_layout: Option<vk::DescriptorSetLayout>,
        point_lights_set_layout: Option<vk::DescriptorSetLayout>,
        lm_set_layouts: Option<(vk::DescriptorSetLayout, vk::DescriptorSetLayout)>,
        ibl_set_layout: Option<vk::DescriptorSetLayout>,
    ) -> Result<Self, PipelineError> {
        let vert_module = Self::create_shader_module(&device, vert_spv)?;
        let frag_module = Self::create_shader_module(&device, frag_spv)?;

        let entry_point = c"main";

        let shader_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vert_module)
                .name(entry_point),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(frag_module)
                .name(entry_point),
        ];

        let binding_descriptions = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(std::mem::size_of::<crate::mesh::SkinnedVertex>() as u32)
            .input_rate(vk::VertexInputRate::VERTEX)];

        // Los offsets son el contrato de mesh::SkinnedVertex (#[repr(C)]):
        // position 0, normal 12, uv 24, joints 32 (u16×4), weights 40.
        let attribute_descriptions = [
            vk::VertexInputAttributeDescription::default()
                .location(0)
                .binding(0)
                .format(vk::Format::R32G32B32_SFLOAT)
                .offset(0),
            vk::VertexInputAttributeDescription::default()
                .location(1)
                .binding(0)
                .format(vk::Format::R32G32B32_SFLOAT)
                .offset(12),
            vk::VertexInputAttributeDescription::default()
                .location(2)
                .binding(0)
                .format(vk::Format::R32G32_SFLOAT)
                .offset(24),
            vk::VertexInputAttributeDescription::default()
                .location(3)
                .binding(0)
                .format(vk::Format::R16G16B16A16_UINT)
                .offset(32),
            vk::VertexInputAttributeDescription::default()
                .location(4)
                .binding(0)
                .format(vk::Format::R32G32B32A32_SFLOAT)
                .offset(40),
        ];

        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&binding_descriptions)
            .vertex_attribute_descriptions(&attribute_descriptions);

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
            .primitive_restart_enable(false);

        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

        let viewport_state_info = vk::PipelineViewportStateCreateInfo::default()
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
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL)
            .depth_bounds_test_enable(false)
            .stencil_test_enable(false);

        let color_blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(false)];

        let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
            .logic_op_enable(false)
            .attachments(&color_blend_attachment);

        let push_constant_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .offset(0)
            .size(crate::renderer::MESH_PUSH_CONSTANTS_SIZE)];

        // Set 0: textura base, mismo layout que en new_mesh_pipeline.
        let sampler_binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        let set_layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&sampler_binding);
        let descriptor_set_layout =
            unsafe { device.create_descriptor_set_layout(&set_layout_info, None)? };

        // Set 1: el SSBO de matrices de articulación (skin::JointBuffer).
        let joints_binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::VERTEX)];
        let joints_layout_info =
            vk::DescriptorSetLayoutCreateInfo::default().bindings(&joints_binding);
        let joints_set_layout =
            unsafe { device.create_descriptor_set_layout(&joints_layout_info, None)? };

        let mut set_layouts = vec![descriptor_set_layout, joints_set_layout];
        if let Some(shadow) = shadow_set_layout {
            set_layouts.push(shadow);
        }
        // Set 3 (luces puntuales): mismo UBO que el PBR; el fragmento es
        // compartido. Sin set 2 no puede haber set 3 (el llamante del
        // renderer siempre pasa ambos juntos).
        if let Some(pl) = point_lights_set_layout {
            set_layouts.push(pl);
        }
        // Sets 4 y 5 (lightmap): el fragmento es el mismo PBR.
        if let Some((lm_tex, lm_data)) = lm_set_layouts {
            while set_layouts.len() < 4 {
                set_layouts.push(descriptor_set_layout);
            }
            set_layouts.push(lm_tex);
            set_layouts.push(lm_data);
        }

        // Set 6 (IBL): solo el PBR lo pide; huecos rellenados igual.
        if let Some(ibl) = ibl_set_layout {
            while set_layouts.len() < 6 {
                set_layouts.push(descriptor_set_layout);
            }
            set_layouts.push(ibl);
        }

        let layout_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&set_layouts)
            .push_constant_ranges(&push_constant_ranges);
        let layout = unsafe { device.create_pipeline_layout(&layout_info, None)? };

        let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state_info)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&color_blending)
            .dynamic_state(&dynamic_state_info)
            .layout(layout)
            .render_pass(render_pass)
            .subpass(0)];

        let pipelines = unsafe {
            device
                .create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_info, None)
                .map_err(|(_, err)| PipelineError::Driver(err))?
        };

        unsafe {
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_module, None);
        }

        log::info!("Pipeline CON PIEL creada (SkinnedVertex + SSBO de articulaciones)");

        Ok(Self {
            device,
            layout,
            pipeline: pipelines[0],
            descriptor_set_layout: Some(descriptor_set_layout),
            joints_set_layout: Some(joints_set_layout),
            globals_set_layout: None,
            shadow_set_layout,
            point_lights_set_layout,
            lm_set_layouts,
            ibl_set_layout,
        })
    }


    /// Pipeline de SOLO PROFUNDIDAD para el pase del mapa de sombras:
    /// sin etapa de fragmento y sin attachments de color (el render pass
    /// de sombras no tiene ninguno). Reutiliza el vertex shader PBR — o
    /// el de piel si `skinned` — tal cual: su `gl_Position` sale de los
    /// mismos push constants de 208 B, con la MVP ya compuesta con la
    /// VP de la luz por quien graba el pase. El sesgo por hardware
    /// (constante 1,0 / pendiente 1,5) acompaña al sesgo manual del
    /// fragmento PBR. Sin culling: las caras traseras también escriben
    /// profundidad, como en el pase principal.
    ///
    /// Devuelve (pipeline, layout) CRUDOS: su dueño es
    /// `shadow::ShadowResources`, que los destruye — no caben en la
    /// forma de `GraphicsPipeline` sin inventar dueños compartidos.
    #[allow(clippy::too_many_arguments)]
    pub fn new_depth_pipeline(
        device: Arc<ash::Device>,
        render_pass: vk::RenderPass,
        vert_spv: &[u8],
        skinned: bool,
        filler_set_layout: Option<vk::DescriptorSetLayout>,
        joints_set_layout: Option<vk::DescriptorSetLayout>,
    ) -> Result<(vk::Pipeline, vk::PipelineLayout), PipelineError> {
        let vert_module = Self::create_shader_module(&device, vert_spv)?;
        let entry_point = c"main";
        let shader_stages = [vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vert_module)
            .name(entry_point)];

        let binding_descriptions = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(if skinned {
                std::mem::size_of::<crate::mesh::SkinnedVertex>()
            } else {
                std::mem::size_of::<crate::mesh::Vertex>()
            } as u32)
            .input_rate(vk::VertexInputRate::VERTEX)];
        let mut attribute_descriptions = vec![
            vk::VertexInputAttributeDescription::default().location(0).binding(0).format(vk::Format::R32G32B32_SFLOAT).offset(0),
            vk::VertexInputAttributeDescription::default().location(1).binding(0).format(vk::Format::R32G32B32_SFLOAT).offset(12),
            vk::VertexInputAttributeDescription::default().location(2).binding(0).format(vk::Format::R32G32_SFLOAT).offset(24),
        ];
        if skinned {
            attribute_descriptions.push(
                vk::VertexInputAttributeDescription::default().location(3).binding(0).format(vk::Format::R16G16B16A16_UINT).offset(32),
            );
            attribute_descriptions.push(
                vk::VertexInputAttributeDescription::default().location(4).binding(0).format(vk::Format::R32G32B32A32_SFLOAT).offset(40),
            );
        }
        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&binding_descriptions)
            .vertex_attribute_descriptions(&attribute_descriptions);

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
            .primitive_restart_enable(false);

        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let viewport_state_info = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);

        let rasterizer = vk::PipelineRasterizationStateCreateInfo::default()
            .depth_clamp_enable(false)
            .rasterizer_discard_enable(false)
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .depth_bias_enable(true)
            .depth_bias_constant_factor(1.0)
            .depth_bias_slope_factor(1.5);

        let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
            .sample_shading_enable(false)
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);

        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL)
            .depth_bounds_test_enable(false)
            .stencil_test_enable(false);

        // Sin attachments de color: el array vacío es el contrato con
        // el render pass de sombras (solo profundidad).
        let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
            .logic_op_enable(false)
            .attachments(&[]);

        let push_constant_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .offset(0)
            .size(crate::renderer::MESH_PUSH_CONSTANTS_SIZE)];

        let mut set_layouts: Vec<vk::DescriptorSetLayout> = Vec::new();
        if skinned {
            // El vértice de piel lee las articulaciones del set 1; el
            // set 0 no lo declara ningún shader de vértice aquí, pero
            // ocupa su posición en el layout (relleno del llamante).
            if let (Some(filler), Some(joints)) = (filler_set_layout, joints_set_layout) {
                set_layouts.push(filler);
                set_layouts.push(joints);
            }
        }
        let layout_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&set_layouts)
            .push_constant_ranges(&push_constant_ranges);
        let layout = unsafe { device.create_pipeline_layout(&layout_info, None)? };

        let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state_info)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&color_blending)
            .dynamic_state(&dynamic_state_info)
            .layout(layout)
            .render_pass(render_pass)
            .subpass(0)];

        let pipelines = unsafe {
            device
                .create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_info, None)
                .map_err(|(_, err)| PipelineError::Driver(err))?
        };
        unsafe { device.destroy_shader_module(vert_module, None) };
        Ok((pipelines[0], layout))
    }

    /// Pipeline de profundidad para los pases de sombra de un grafo
    /// (Fase 2S): la etapa de vértices es la del grafo (con sus
    /// GraphGlobals en el set 1) y el set 0 es el de texturas del grafo;
    /// sin fragmento (profundidad pura) o con el fragmento de recorte
    /// alfa de la propiedad `alpha_clip`.
    pub fn new_outline_pipeline(
        device: Arc<ash::Device>,
        render_pass: vk::RenderPass,
        vert_spv: &[u8],
        frag_spv: &[u8],
    ) -> Result<Self, PipelineError> {
        let vert_module = Self::create_shader_module(&device, vert_spv)?;
        let frag_module = Self::create_shader_module(&device, frag_spv)?;
        let entry_point = c"main";
        let shader_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vert_module)
                .name(entry_point),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(frag_module)
                .name(entry_point),
        ];

        let binding_descriptions = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(std::mem::size_of::<crate::mesh::Vertex>() as u32)
            .input_rate(vk::VertexInputRate::VERTEX)];
        let attribute_descriptions = [
            vk::VertexInputAttributeDescription::default().location(0).binding(0).format(vk::Format::R32G32B32_SFLOAT).offset(0),
            vk::VertexInputAttributeDescription::default().location(1).binding(0).format(vk::Format::R32G32B32_SFLOAT).offset(12),
            vk::VertexInputAttributeDescription::default().location(2).binding(0).format(vk::Format::R32G32_SFLOAT).offset(24),
        ];
        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&binding_descriptions)
            .vertex_attribute_descriptions(&attribute_descriptions);

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
            .primitive_restart_enable(false);

        let viewports = [vk::Viewport::default()];
        let scissors = [vk::Rect2D::default()];
        let viewport_state_info = vk::PipelineViewportStateCreateInfo::default()
            .viewports(&viewports)
            .scissors(&scissors);

        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

        let rasterizer = vk::PipelineRasterizationStateCreateInfo::default()
            .depth_clamp_enable(false)
            .rasterizer_discard_enable(false)
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::FRONT)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .depth_bias_enable(false);

        let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
            .sample_shading_enable(false)
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);

        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL)
            .depth_bounds_test_enable(false)
            .stencil_test_enable(false);

        let color_blend_attachments = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(false)
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
            .logic_op_enable(false)
            .attachments(&color_blend_attachments);

        let push_constant_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .offset(0)
            .size(crate::renderer::MESH_PUSH_CONSTANTS_SIZE)];

        let set_layouts: [vk::DescriptorSetLayout; 0] = [];
        let layout_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&set_layouts)
            .push_constant_ranges(&push_constant_ranges);
        let layout = unsafe { device.create_pipeline_layout(&layout_info, None)? };

        let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state_info)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&color_blending)
            .dynamic_state(&dynamic_state_info)
            .layout(layout)
            .render_pass(render_pass)
            .subpass(0)];

        let pipelines = unsafe {
            device
                .create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_info, None)
                .map_err(|(_, err)| PipelineError::Driver(err))?
        };

        unsafe {
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_module, None);
        }

        Ok(Self {
            device,
            layout,
            pipeline: pipelines[0],
            descriptor_set_layout: None,
            joints_set_layout: None,
            globals_set_layout: None,
            shadow_set_layout: None,
            point_lights_set_layout: None,
            lm_set_layouts: None,
            ibl_set_layout: None,
        })
    }



    /// Pipeline de partículas (Fase 8P): vértices ya expandidos
    /// por la CPU (posición + color + uv), mezcla alfa clásica,
    /// prueba de profundidad SIN escritura y sin culling (los
    /// billboards rotan). Sin descriptor sets: solo el bloque de
    /// push constants de siempre (usa el campo mvp).
    pub fn new_particle_pipeline(
        device: Arc<ash::Device>,
        render_pass: vk::RenderPass,
        vert_spv: &[u8],
        frag_spv: &[u8],
    ) -> Result<Self, PipelineError> {
        let vert_module = Self::create_shader_module(&device, vert_spv)?;
        let frag_module = Self::create_shader_module(&device, frag_spv)?;
        let entry_point = c"main";
        let shader_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vert_module)
                .name(entry_point),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(frag_module)
                .name(entry_point),
        ];

        let binding_descriptions = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(std::mem::size_of::<crate::renderer::ParticleVertex>() as u32)
            .input_rate(vk::VertexInputRate::VERTEX)];
        let attribute_descriptions = [
            vk::VertexInputAttributeDescription::default().location(0).binding(0).format(vk::Format::R32G32B32_SFLOAT).offset(0),
            vk::VertexInputAttributeDescription::default().location(1).binding(0).format(vk::Format::R32G32B32A32_SFLOAT).offset(12),
            vk::VertexInputAttributeDescription::default().location(2).binding(0).format(vk::Format::R32G32_SFLOAT).offset(28),
        ];
        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&binding_descriptions)
            .vertex_attribute_descriptions(&attribute_descriptions);

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
            .primitive_restart_enable(false);

        let viewports = [vk::Viewport::default()];
        let scissors = [vk::Rect2D::default()];
        let viewport_state_info = vk::PipelineViewportStateCreateInfo::default()
            .viewports(&viewports)
            .scissors(&scissors);

        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

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
            .depth_test_enable(true)
            .depth_write_enable(false)
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL)
            .depth_bounds_test_enable(false)
            .stencil_test_enable(false);

        let color_blend_attachments = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD)
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
            .logic_op_enable(false)
            .attachments(&color_blend_attachments);

        let push_constant_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .offset(0)
            .size(crate::renderer::MESH_PUSH_CONSTANTS_SIZE)];

        let set_layouts: [vk::DescriptorSetLayout; 0] = [];
        let layout_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&set_layouts)
            .push_constant_ranges(&push_constant_ranges);
        let layout = unsafe { device.create_pipeline_layout(&layout_info, None)? };

        let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state_info)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&color_blending)
            .dynamic_state(&dynamic_state_info)
            .layout(layout)
            .render_pass(render_pass)
            .subpass(0)];

        let pipelines = unsafe {
            device
                .create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_info, None)
                .map_err(|(_, err)| PipelineError::Driver(err))?
        };

        unsafe {
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_module, None);
        }

        Ok(Self {
            device,
            layout,
            pipeline: pipelines[0],
            descriptor_set_layout: None,
            joints_set_layout: None,
            globals_set_layout: None,
            shadow_set_layout: None,
            point_lights_set_layout: None,
            lm_set_layouts: None,
            ibl_set_layout: None,
        })
    }

    pub fn new_depth_pipeline_graph(
        device: Arc<ash::Device>,
        render_pass: vk::RenderPass,
        vert_spv: &[u8],
        frag_spv: Option<&[u8]>,
        set0_layout: vk::DescriptorSetLayout,
        globals_layout: vk::DescriptorSetLayout,
    ) -> Result<(vk::Pipeline, vk::PipelineLayout), PipelineError> {
        let vert_module = Self::create_shader_module(&device, vert_spv)?;
        let frag_module = match frag_spv {
            Some(spv) => Some(Self::create_shader_module(&device, spv)?),
            None => None,
        };
        let entry_point = c"main";
        let mut shader_stages = vec![vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vert_module)
            .name(entry_point)];
        if let Some(frag) = frag_module {
            shader_stages.push(
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(frag)
                    .name(entry_point),
            );
        }

        let binding_descriptions = [vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(std::mem::size_of::<crate::mesh::Vertex>() as u32)
            .input_rate(vk::VertexInputRate::VERTEX)];
        let attribute_descriptions = vec![
            vk::VertexInputAttributeDescription::default().location(0).binding(0).format(vk::Format::R32G32B32_SFLOAT).offset(0),
            vk::VertexInputAttributeDescription::default().location(1).binding(0).format(vk::Format::R32G32B32_SFLOAT).offset(12),
            vk::VertexInputAttributeDescription::default().location(2).binding(0).format(vk::Format::R32G32_SFLOAT).offset(24),
        ];
        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&binding_descriptions)
            .vertex_attribute_descriptions(&attribute_descriptions);

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
            .primitive_restart_enable(false);

        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let viewport_state_info = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);

        let rasterizer = vk::PipelineRasterizationStateCreateInfo::default()
            .depth_clamp_enable(false)
            .rasterizer_discard_enable(false)
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .depth_bias_enable(true)
            .depth_bias_constant_factor(1.0)
            .depth_bias_slope_factor(1.5);

        let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
            .sample_shading_enable(false)
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);

        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL)
            .depth_bounds_test_enable(false)
            .stencil_test_enable(false);

        // Sin attachments de color: el array vacío es el contrato con
        // el render pass de sombras (solo profundidad).
        let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
            .logic_op_enable(false)
            .attachments(&[]);

        let push_constant_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .offset(0)
            .size(crate::renderer::MESH_PUSH_CONSTANTS_SIZE)];

        let set_layouts = [set0_layout, globals_layout];
        let layout_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&set_layouts)
            .push_constant_ranges(&push_constant_ranges);
        let layout = unsafe { device.create_pipeline_layout(&layout_info, None)? };

        let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state_info)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&color_blending)
            .dynamic_state(&dynamic_state_info)
            .layout(layout)
            .render_pass(render_pass)
            .subpass(0)];

        let pipelines = unsafe {
            device
                .create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_info, None)
                .map_err(|(_, err)| PipelineError::Driver(err))?
        };
        unsafe { device.destroy_shader_module(vert_module, None) };
        if let Some(frag) = frag_module {
            unsafe { device.destroy_shader_module(frag, None) };
        }
        Ok((pipelines[0], layout))
    }


    /// Pipeline de pantalla completa para las pasadas del post
    /// (bright-pass, blur, final): sin entrada de vértices (el triángulo
    /// sale de `gl_VertexIndex`), sin test de profundidad, un set con
    /// dos texturas muestreadas y 32 B de push constants. El layout del
    /// set lo crea y posee `post::PostChain`.
    pub fn new_post_pipeline(
        device: Arc<ash::Device>,
        render_pass: vk::RenderPass,
        vert_spv: &[u8],
        frag_spv: &[u8],
        set_layout: vk::DescriptorSetLayout,
    ) -> Result<Self, PipelineError> {
        let vert_module = Self::create_shader_module(&device, vert_spv)?;
        let frag_module = Self::create_shader_module(&device, frag_spv)?;
        let entry_point = c"main";
        let shader_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vert_module)
                .name(entry_point),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(frag_module)
                .name(entry_point),
        ];
        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default();
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
            .primitive_restart_enable(false);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let viewport_state_info = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let rasterizer = vk::PipelineRasterizationStateCreateInfo::default()
            .depth_clamp_enable(false)
            .rasterizer_discard_enable(false)
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::CLOCKWISE)
            .depth_bias_enable(false);
        let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
            .sample_shading_enable(false)
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(false)
            .depth_write_enable(false)
            .depth_compare_op(vk::CompareOp::ALWAYS)
            .depth_bounds_test_enable(false)
            .stencil_test_enable(false);
        let color_blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(false)];
        let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
            .logic_op_enable(false)
            .attachments(&color_blend_attachment);
        let push_constant_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .offset(0)
            .size(32)];
        let set_layouts = [set_layout];
        let layout_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&set_layouts)
            .push_constant_ranges(&push_constant_ranges);
        let layout = unsafe { device.create_pipeline_layout(&layout_info, None)? };
        let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state_info)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&color_blending)
            .dynamic_state(&dynamic_state_info)
            .layout(layout)
            .render_pass(render_pass)
            .subpass(0)];
        let pipelines = unsafe {
            device
                .create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_info, None)
                .map_err(|(_, err)| PipelineError::Driver(err))?
        };
        unsafe {
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_module, None);
        }
        Ok(Self {
            device,
            layout,
            pipeline: pipelines[0],
            descriptor_set_layout: None,
            joints_set_layout: None,
            globals_set_layout: None,
            shadow_set_layout: None,
            point_lights_set_layout: None,
            lm_set_layouts: None,
            ibl_set_layout: None,
        })
    }

    pub(crate) fn create_shader_module(
        device: &ash::Device,
        spv_bytes: &[u8],
    ) -> Result<vk::ShaderModule, PipelineError> {
        if spv_bytes.len() % 4 != 0 {
            return Err(PipelineError::ShaderLoadError);
        }

        let words: Vec<u32> = spv_bytes
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        let create_info = vk::ShaderModuleCreateInfo::default().code(&words);
        let module = unsafe { device.create_shader_module(&create_info, None)? };
        Ok(module)
    }
}

impl Drop for GraphicsPipeline {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_pipeline(self.pipeline, None);
            self.device.destroy_pipeline_layout(self.layout, None);
            if let Some(set_layout) = self.descriptor_set_layout.take() {
                self.device.destroy_descriptor_set_layout(set_layout, None);
            }
            if let Some(joints_layout) = self.joints_set_layout.take() {
                self.device.destroy_descriptor_set_layout(joints_layout, None);
            }
        }
    }
}
