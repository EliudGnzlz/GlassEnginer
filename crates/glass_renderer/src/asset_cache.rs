use crate::mesh::{GltfImageData, GltfMaterial, GpuMesh, GpuTexture, MeshLoader, PrimitiveRange};
use ash::vk;
use gpu_allocator::vulkan::Allocator;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// Contexto de subida de una textura: la cola y el pool de comandos
/// que ejecutan la copia y el layout del set donde se enlaza.
/// (Agrupado por el lint too_many_arguments de Clippy.)
pub struct TextureUpload {
    pub queue: vk::Queue,
    pub command_pool: vk::CommandPool,
    pub set_layout: vk::DescriptorSetLayout,
}

pub struct AssetCache {
/// Geometría CPU retenida de cada modelo importado (posiciones e
    /// índices, misma clave que `meshes`): la física la necesita para los
    /// colliders de malla de la Fase 1.5; sin esta copia, el LoadedModel
    /// se descarta tras la subida a GPU y la malla sería irrecuperable.
    pub geometries: HashMap<String, (Vec<[f32; 3]>, Vec<u32>)>,

    /// Cotas locales (min, max) de la geometría retenida, para el
    /// frustum culling de la Fase 2.
    pub bounds: HashMap<String, ([f32; 3], [f32; 3])>,
    pub meshes: HashMap<String, GpuMesh>,
    /// Mallas GPU de los modelos CON piel, bajo la misma clave que en
    /// `meshes`: misma geometría pero con el formato `SkinnedVertex`
    /// (articulaciones y pesos entrelazados) para el pipeline con piel.
    /// La entrada de `meshes` se mantiene como respaldo rígido.
    pub skinned_meshes: HashMap<String, GpuMesh>,
    /// La piel importada de cada modelo con piel (grafo de nodos,
    /// articulaciones y matrices inversas de unión), datos CPU.
    pub skins: HashMap<String, crate::skin::SkinData>,
    /// Los clips de animación importados de cada modelo con piel.
    pub animations: HashMap<String, Vec<crate::skin::AnimationClip>>,
    /// Materiales PBR importados de cada modelo, por la misma clave que
    /// `meshes`. Datos CPU: el render los lee para los push constants y la
    /// subida de texturas a GPU.
    pub materials: HashMap<String, Vec<GltfMaterial>>,
    /// Material principal (primera primitiva) de cada modelo, si lo tiene.
    pub primary_materials: HashMap<String, GltfMaterial>,
    /// Imágenes decodificadas (RGBA8) de cada modelo, para subir a GPU.
    pub images: HashMap<String, Vec<GltfImageData>>,
    /// Rangos de índices por primitiva de cada modelo, por la misma clave
    /// que `meshes`: el bucle de dibujo emite un lote por primitiva, cada
    /// uno con su material y su textura.
    pub primitive_ranges: HashMap<String, Vec<PrimitiveRange>>,
    /// Objetos del documento por modelo (Fase 6): instancias y LOD.
    pub objects: HashMap<String, Vec<crate::mesh::ModelObject>>,
    /// Texturas ya subidas a VRAM, por (clave de modelo, índice de imagen
    /// dentro del .glb): cada material puede usar una imagen distinta.
    pub textures: HashMap<(String, usize), GpuTexture>,
    /// Descriptor set (set 0 del pipeline PBR) por (modelo, imagen).
    pub texture_sets: HashMap<(String, usize), vk::DescriptorSet>,
    /// Set de la textura blanca 1×1 por defecto: lo que se enlaza cuando la
    /// entidad no tiene textura, para que el shader siempre tenga un set
    /// válido y el color plano se comporte como antes.
    pub default_set: Option<vk::DescriptorSet>,
    default_texture: Option<GpuTexture>,
    sampler: Option<vk::Sampler>,
    descriptor_pool: Option<vk::DescriptorPool>,
    /// Último instante (ms del reloj de la caché) en que cada modelo
    /// fue dibujado o insertado (para la expulsión de ociosos). Mutex
    /// interior: marcar uso debe ser posible con la caché en lectura
    /// compartida durante el bucle de dibujo.
    last_used: Mutex<HashMap<String, u64>>,
    /// Reloj de la caché en milisegundos REALES acumulados (la app lo
    /// avanza con el delta de tiempo de cada frame, no por frame): la
    /// ociosidad se mide contra tiempo de reloj, no contra frames —
    /// con el bucle sin capar, 600 frames podían ser 1-2 s y la
    /// expulsión devoraba modelos recién cargados (Bug I).
    now_ms: AtomicU64,
}

impl Default for AssetCache {
    fn default() -> Self {
        Self {
            geometries: HashMap::new(),
            bounds: HashMap::new(),
            meshes: HashMap::new(),
            skinned_meshes: HashMap::new(),
            skins: HashMap::new(),
            animations: HashMap::new(),
            materials: HashMap::new(),
            primary_materials: HashMap::new(),
            images: HashMap::new(),
            primitive_ranges: HashMap::new(),
            objects: HashMap::new(),
            textures: HashMap::new(),
            texture_sets: HashMap::new(),
            default_set: None,
            default_texture: None,
            sampler: None,
            descriptor_pool: None,
            last_used: Mutex::new(HashMap::new()),
            now_ms: AtomicU64::new(0),
        }
    }
}

impl AssetCache {
    pub fn new() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self::default()))
    }

/// Retiene la geometría CPU de un modelo importado (la llama la app
    /// justo tras cargarlo, con la misma clave/stem que insert_mesh).
    pub fn insert_geometry(&mut self, name: impl Into<String>, model: &crate::mesh::LoadedModel) {
        let positions: Vec<[f32; 3]> = model.vertices.iter().map(|v| v.position).collect();
        let name = name.into();
        if let Some(first) = positions.first() {
            let mut min = *first;
            let mut max = *first;
            for p in &positions[1..] {
                min[0] = min[0].min(p[0]);
                min[1] = min[1].min(p[1]);
                min[2] = min[2].min(p[2]);
                max[0] = max[0].max(p[0]);
                max[1] = max[1].max(p[1]);
                max[2] = max[2].max(p[2]);
            }
            self.bounds.insert(name.clone(), (min, max));
        }
        self.geometries.insert(name, (positions, model.indices.clone()));
        // (no tocar touch(): la geometría sigue el ciclo de vida del modelo;
        // si la entrada GPU se expulsa por inactividad, la geometría debe
        // expulsarse en el mismo sitio: ver evict_idle.)
    }

    /// Registra cotas locales (min, max) sin exigir la geometría CPU
    /// completa (primitivas integradas: generadas en memoria, nunca
    /// cargadas vía insert_geometry). No toca `geometries`.
    pub fn insert_bounds(&mut self, name: impl Into<String>, min: [f32; 3], max: [f32; 3]) {
        self.bounds.insert(name.into(), (min, max));
    }

    /// Geometría retenida de un modelo, si la hay.
    pub fn geometry(&self, name: &str) -> Option<&(Vec<[f32; 3]>, Vec<u32>)> {
        self.geometries.get(name)
    }

    /// Cotas locales (min, max) de la geometría retenida de un modelo.
    pub fn bounds(&self, name: &str) -> Option<([f32; 3], [f32; 3])> {
        self.bounds.get(name).copied()
    }

    /// Copia de todas las cotas locales registradas (clave → (min, max)).
    /// La consume el editor para picking y caja de selección: es un mapa
    /// pequeño (una entrada por malla distinta) y se refresca por frame.
    pub fn bounds_snapshot(&self) -> std::collections::HashMap<String, ([f32; 3], [f32; 3])> {
        self.bounds.clone()
    }

    pub fn insert_mesh(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        name: impl Into<String>,
        mesh: GpuMesh,
    ) {
        let key = name.into();
        // Si ya existía una malla con ese nombre, liberar los recursos VRAM anteriores limpiamente
        if let Some(mut old_mesh) = self.meshes.insert(key.clone(), mesh) {
            log::info!("Reemplazando malla '{}' en VRAM (Recarga en caliente)", key);
            old_mesh.destroy(device, allocator);
        } else {
            log::info!("Malla '{}' registrada en AssetCache", key);
        }
        self.touch(&key);
    }

    /// Registra la malla CON PIEL de un modelo (formato `SkinnedVertex`),
    /// con la misma semántica de reemplazo que `insert_mesh`: la anterior
    /// se destruye (recarga en caliente de un rig editado en Blender).
    pub fn insert_skinned_mesh(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        name: impl Into<String>,
        mesh: GpuMesh,
    ) {
        let key = name.into();
        if let Some(mut old_mesh) = self.skinned_meshes.insert(key.clone(), mesh) {
            log::info!("Reemplazando malla con piel '{}' en VRAM (Recarga en caliente)", key);
            old_mesh.destroy(device, allocator);
        }
        self.touch(&key);
    }

    /// Registra la piel y los clips de un modelo importado, bajo la misma
    /// clave que sus mallas.
    pub fn insert_skin_data(
        &mut self,
        name: impl Into<String>,
        skin: crate::skin::SkinData,
        animations: Vec<crate::skin::AnimationClip>,
    ) {
        let key = name.into();
        self.skins.insert(key.clone(), skin);
        self.animations.insert(key, animations);
    }

    /// Retira la piel de un modelo: destruye su malla con piel y olvida
    /// la piel y los clips. La usa la recarga en caliente cuando el
    /// modelo nuevo ya no trae piel: sin esto, los datos viejos
    /// sobrevivían en caché y la entidad se seguía dibujando por la
    /// vía con piel con la malla y las articulaciones del modelo
    /// anterior (en pantalla no se veía nada).
    pub fn remove_skin(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        name: &str,
    ) {
        if let Some(mut mesh) = self.skinned_meshes.remove(name) {
            mesh.destroy(device, allocator);
        }
        self.skins.remove(name);
        self.animations.remove(name);
    }

    /// Registra los datos CPU de un modelo importado (materiales e imágenes)
    /// bajo la misma clave que su malla GPU.
    pub fn insert_model_data(
        &mut self,
        name: impl Into<String>,
        materials: Vec<GltfMaterial>,
        primary_material: Option<GltfMaterial>,
        images: Vec<GltfImageData>,
        primitive_ranges: Vec<PrimitiveRange>,
        objects: Vec<crate::mesh::ModelObject>,
    ) {
        let key = name.into();
        if let Some(mat) = primary_material {
            self.primary_materials.insert(key.clone(), mat);
        } else {
            self.primary_materials.remove(&key);
        }
        self.materials.insert(key.clone(), materials);
        self.images.insert(key.clone(), images);
        self.primitive_ranges.insert(key.clone(), primitive_ranges);
        self.objects.insert(key.clone(), objects);
        self.touch(&key);
    }

    /// Crea (una sola vez) el sampler y el pool de descriptores compartidos.
    fn ensure_descriptor_infra(&mut self, device: &ash::Device) {
        if self.sampler.is_none() {
            let sampler_info = vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                // Mip-maps activos: las texturas suben con su cadena
                // completa (ver MeshLoader::upload_texture_to_gpu) y el
                // sampler compartido la recorre con filtrado trilineal.
                // El LOD máximo alto es seguro: cada vista limita los
                // niveles a los que su imagen realmente tiene.
                .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                .max_lod(16.0)
                // REPEAT y no CLAMP: el tiling de los materiales
                // (Materiales v1) multiplica la UV más allá de 1 y
                // con clamp estiraría el téxel del borde en vez de
                // repetir. Es además el modo por defecto de glTF
                // cuando el archivo no declara sampler.
                .address_mode_u(vk::SamplerAddressMode::REPEAT)
                .address_mode_v(vk::SamplerAddressMode::REPEAT)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
            self.sampler = unsafe { device.create_sampler(&sampler_info, None).ok() };
        }
        if self.descriptor_pool.is_none() {
            // 1024 sets: con 256, un mundo procedural con muchos modelos
            // texturizados agotaba el pool y las texturas caían en silencio
            // a la blanca por defecto (solo quedaba el log de error).
            let pool_sizes = [vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1024)];
            let pool_info = vk::DescriptorPoolCreateInfo::default()
                .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                .pool_sizes(&pool_sizes)
                .max_sets(1024);
            self.descriptor_pool = unsafe { device.create_descriptor_pool(&pool_info, None).ok() };
        }
    }

    /// Reserva un descriptor set nuevo para `layout` y lo apunta a la vista
    /// dada con el sampler compartido. Devuelve None si no hay pool.
    fn allocate_and_write_set(
        &mut self,
        device: &ash::Device,
        layout: vk::DescriptorSetLayout,
        image_view: vk::ImageView,
    ) -> Option<vk::DescriptorSet> {
        let pool = self.descriptor_pool?;
        let sampler = self.sampler?;
        let layouts = [layout];
        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&layouts);
        let set = unsafe { device.allocate_descriptor_sets(&alloc_info).ok()?.into_iter().next()? };
        let image_info = [vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(image_view)
            .sampler(sampler)];
        let write = [vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_info)];
        unsafe { device.update_descriptor_sets(&write, &[]) };
        Some(set)
    }

    /// Reapunta un set YA reservado a otra vista (recarga en caliente).
    fn rewrite_set(&self, device: &ash::Device, set: vk::DescriptorSet, image_view: vk::ImageView) {
        if let Some(sampler) = self.sampler {
            let image_info = [vk::DescriptorImageInfo::default()
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .image_view(image_view)
                .sampler(sampler)];
            let write = [vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&image_info)];
            unsafe { device.update_descriptor_sets(&write, &[]) };
        }
    }

    /// Crea la textura blanca 1×1 y su set por defecto (idempotente).
    pub fn ensure_default_texture(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        queue: vk::Queue,
        command_pool: vk::CommandPool,
        set_layout: vk::DescriptorSetLayout,
    ) {
        if self.default_set.is_some() {
            return;
        }
        self.ensure_descriptor_infra(device);
        let white = GltfImageData { width: 1, height: 1, rgba: vec![255, 255, 255, 255] };
        match MeshLoader::upload_texture_to_gpu(device, allocator, &white, queue, command_pool) {
            Ok(tex) => {
                let view = tex.image_view;
                self.default_texture = Some(tex);
                self.default_set = self.allocate_and_write_set(device, set_layout, view);
                if self.default_set.is_none() {
                    log::error!("AssetCache: sin descriptor para la textura por defecto");
                }
            }
            Err(e) => log::error!("AssetCache: fallo subiendo la textura por defecto: {}", e),
        }
    }

    /// Sube una imagen de un modelo (la que referencia un material como
    /// textura base) y la enlaza en su descriptor set, identificado por
    /// (modelo, índice de imagen). Si esa imagen ya tenía textura (recarga
    /// en caliente), libera la anterior y reapunta el MISMO set.
    pub fn insert_texture(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        upload: TextureUpload,
        name: impl Into<String>,
        image_index: usize,
        img: &GltfImageData,
    ) {
        let TextureUpload { queue, command_pool, set_layout } = upload;
        let key = name.into();
        let tex_key = (key.clone(), image_index);
        self.ensure_descriptor_infra(device);
        match MeshLoader::upload_texture_to_gpu(device, allocator, img, queue, command_pool) {
            Ok(tex) => {
                let view = tex.image_view;
                if let Some(mut old) = self.textures.insert(tex_key.clone(), tex) {
                    old.destroy(device, allocator);
                }
                if let Some(&set) = self.texture_sets.get(&tex_key) {
                    self.rewrite_set(device, set, view);
                } else if let Some(set) = self.allocate_and_write_set(device, set_layout, view) {
                    self.texture_sets.insert(tex_key, set);
                } else {
                    log::error!(
                        "AssetCache: sin descriptor para la textura {} de '{}' (pool agotado: {} sets vivos)",
                        image_index,
                        key,
                        self.texture_sets.len()
                    );
                }
                self.touch(&key);
                log::info!("Textura {} de '{}' registrada en AssetCache", image_index, key);
            }
            Err(e) => log::error!("AssetCache: fallo subiendo textura {} de '{}': {}", image_index, key, e),
        }
    }

    /// El set a enlazar para dibujar con la imagen `image` de un modelo:
    /// el suyo si esa imagen está subida; si no, el de la textura blanca
    /// por defecto (el shader siempre muestrea, así que hace falta un set
    /// válido en todos los casos).
    pub fn set_for_image(&self, model: Option<&str>, image: Option<usize>) -> Option<vk::DescriptorSet> {
        if let (Some(m), Some(i)) = (model, image) {
            if let Some(set) = self.texture_sets.get(&(m.to_string(), i)) {
                return Some(*set);
            }
        }
        self.default_set
    }

    /// La vista de imagen de la textura (modelo, índice); si no existe,
    /// la de la textura blanca por defecto (`default_texture`). Pensada
    /// para componer sets de grafo (`graph_texture_set`) sin depender de
    /// que cada combinación tenga ya su propio set de 1 binding.
    pub fn view_for_image(&self, model: Option<&str>, image: Option<usize>) -> Option<vk::ImageView> {
        if let (Some(m), Some(i)) = (model, image) {
            if let Some(tex) = self.textures.get(&(m.to_string(), i)) {
                return Some(tex.image_view);
            }
        }
        self.default_texture.as_ref().map(|tex| tex.image_view)
    }

    /// Aloja (del `descriptor_pool` propio) y escribe un descriptor set con
    /// 4 bindings COMBINED_IMAGE_SAMPLER (0..=3), uno por textura de grafo.
    /// Las entradas `None` de `views` se rellenan con la vista por defecto
    /// (`view_for_image(None, None)`) porque el shader siempre muestrea los
    /// 4 bindings aunque el grafo solo use alguno. `None` si falta el pool
    /// o el sampler compartido (infraestructura aún no inicializada).
    pub fn graph_texture_set(
        &mut self,
        device: &ash::Device,
        layout: vk::DescriptorSetLayout,
        views: [Option<vk::ImageView>; 4],
    ) -> Option<vk::DescriptorSet> {
        let pool = self.descriptor_pool?;
        let sampler = self.sampler?;
        let default_view = self.view_for_image(None, None);

        let layouts = [layout];
        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&layouts);
        let set = unsafe { device.allocate_descriptor_sets(&alloc_info) }
            .ok()?
            .into_iter()
            .next()?;

        let mut image_infos = [vk::DescriptorImageInfo::default(); 4];
        let mut present = [false; 4];
        for (binding, view_opt) in views.into_iter().enumerate() {
            let view = match view_opt.or(default_view) {
                Some(v) => v,
                None => continue,
            };
            image_infos[binding] = vk::DescriptorImageInfo::default()
                .sampler(sampler)
                .image_view(view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
            present[binding] = true;
        }
        let mut writes = Vec::with_capacity(4);
        for binding in 0..4 {
            if !present[binding] {
                continue;
            }
            writes.push(
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(binding as u32)
                    .dst_array_element(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(std::slice::from_ref(&image_infos[binding])),
            );
        }
        unsafe { device.update_descriptor_sets(&writes, &[]) };
        Some(set)
    }

    /// Marca un modelo como usado en el instante actual del reloj.
    pub fn touch(&self, name: &str) {
        let now = self.now_ms.load(Ordering::Relaxed);
        if let Ok(mut map) = self.last_used.lock() {
            map.insert(name.to_string(), now);
        }
    }

    /// Avanza el reloj de la caché en `delta_ms` milisegundos reales
    /// (la app lo llama una vez por frame con el delta medido) y
    /// devuelve el nuevo instante acumulado.
    pub fn advance_time_ms(&self, delta_ms: u64) -> u64 {
        self.now_ms.fetch_add(delta_ms, Ordering::Relaxed) + delta_ms
    }

    /// Decisión pura de expulsión: modelos con malla en VRAM que llevan
    /// más de `max_idle_ms` milisegundos reales sin dibujarse ni
    /// insertarse, que no estén en `protected`. Las primitivas
    /// integradas nunca se expulsan: el cubo
    /// es el respaldo de todas las entidades sin modelo, y el plano, la
    /// esfera, la cápsula y el cilindro son las otras mallas que el
    /// motor registra por código, no desde un archivo.
    pub fn select_evictions(&self, max_idle_ms: u64, protected: &HashSet<String>) -> Vec<String> {
        let now = self.now_ms.load(Ordering::Relaxed);
        let used = self.last_used.lock().map(|m| m.clone()).unwrap_or_default();
        // Claves de AMBOS mapas de mallas (rígidas y con piel), sin repetir.
        let mut keys: Vec<&String> = self.meshes.keys().collect();
        for k in self.skinned_meshes.keys() {
            if !self.meshes.contains_key(k) {
                keys.push(k);
            }
        }
        keys.into_iter()
            .filter(|k| {
                !matches!(
                    k.as_str(),
                    "cube" | "plane" | "sphere" | "capsule" | "cylinder"
                )
            })
            .filter(|k| !protected.contains(*k))
            .filter(|k| {
                let last = used.get(*k).copied().unwrap_or(0);
                now.saturating_sub(last) > max_idle_ms
            })
            .cloned()
            .collect()
    }

    /// Expulsa los modelos ociosos: libera malla y textura de VRAM, su
    /// descriptor set y sus datos CPU. Pensado para el enfoque procedural
    /// (crear y descartar mundos sin que la caché crezca sin límite).
    /// Devuelve las claves expulsadas.
    pub fn evict_idle(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        max_idle_ms: u64,
        protected: &HashSet<String>,
    ) -> Vec<String> {
        let keys = self.select_evictions(max_idle_ms, protected);
        for key in &keys {
            self.geometries.remove(key);
            self.bounds.remove(key);
            if let Some(mut mesh) = self.meshes.remove(key) {
                mesh.destroy(device, allocator);
            }
            if let Some(mut mesh) = self.skinned_meshes.remove(key) {
                mesh.destroy(device, allocator);
            }
            self.skins.remove(key);
            self.animations.remove(key);
            // Todas las texturas y sets del modelo (una entrada por imagen).
            let tex_keys: Vec<(String, usize)> = self
                .textures
                .keys()
                .filter(|(m, _)| m == key)
                .cloned()
                .collect();
            for tex_key in tex_keys {
                if let Some(mut tex) = self.textures.remove(&tex_key) {
                    tex.destroy(device, allocator);
                }
                if let Some(set) = self.texture_sets.remove(&tex_key) {
                    if let Some(pool) = self.descriptor_pool {
                        unsafe {
                            let _ = device.free_descriptor_sets(pool, &[set]);
                        }
                    }
                }
            }
            self.materials.remove(key);
            self.primary_materials.remove(key);
            self.images.remove(key);
            self.primitive_ranges.remove(key);
            self.objects.remove(key);
            if let Ok(mut map) = self.last_used.lock() {
                map.remove(key);
            }
            log::info!("AssetCache: modelo ocioso '{}' expulsado de VRAM", key);
        }
        keys
    }

    pub fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        for (name, mut mesh) in self.meshes.drain() {
            log::debug!("Liberando malla '{}' de VRAM", name);
            mesh.destroy(device, allocator);
        }
        for (name, mut mesh) in self.skinned_meshes.drain() {
            log::debug!("Liberando malla con piel '{}' de VRAM", name);
            mesh.destroy(device, allocator);
        }
        self.skins.clear();
        self.animations.clear();
        for ((name, image_index), mut tex) in self.textures.drain() {
            log::debug!("Liberando textura {} de '{}' de VRAM", image_index, name);
            tex.destroy(device, allocator);
        }
        if let Some(mut tex) = self.default_texture.take() {
            tex.destroy(device, allocator);
        }
        // El pool libera todos los descriptor sets de golpe.
        if let Some(pool) = self.descriptor_pool.take() {
            unsafe { device.destroy_descriptor_pool(pool, None) };
        }
        self.texture_sets.clear();
        self.default_set = None;
        if let Some(sampler) = self.sampler.take() {
            unsafe { device.destroy_sampler(sampler, None) };
        }
    }
}

static GLOBAL_ASSET_CACHE: std::sync::OnceLock<
    std::sync::Arc<std::sync::RwLock<AssetCache>>,
> = std::sync::OnceLock::new();

/// Registra la caché de assets global (la llama la app al arrancar,
/// junto al mundo físico global): la leen las tools MCP de inspección.
pub fn register_global_asset_cache(
    cache: std::sync::Arc<std::sync::RwLock<AssetCache>>,
) {
    let _ = GLOBAL_ASSET_CACHE.set(cache);
}

/// La caché global registrada, si la app ya arrancó.
pub fn global_asset_cache() -> Option<std::sync::Arc<std::sync::RwLock<AssetCache>>> {
    GLOBAL_ASSET_CACHE.get().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_mesh() -> GpuMesh {
        GpuMesh {
            vertex_buffer: vk::Buffer::null(),
            vertex_allocation: None,
            index_buffer: vk::Buffer::null(),
            index_allocation: None,
            index_count: 0,
            vertex_count: 0,
        }
    }

    #[test]
    fn expulsion_respeta_recientes_protegidos_y_al_cubo() {
        let mut cache = AssetCache::default();
        cache.meshes.insert("cube".into(), fake_mesh());
        // Las demás primitivas integradas tampoco se expulsan nunca,
        // aunque lleven 16 s sin usarse y no estén en
        // `protected`.
        cache.meshes.insert("plane".into(), fake_mesh());
        cache.meshes.insert("sphere".into(), fake_mesh());
        cache.meshes.insert("capsule".into(), fake_mesh());
        cache.meshes.insert("cylinder".into(), fake_mesh());
        cache.meshes.insert("viejo".into(), fake_mesh());
        cache.meshes.insert("reciente".into(), fake_mesh());
        cache.meshes.insert("protegido".into(), fake_mesh());
        // Instante 0: se "usa" todo menos "viejo" y "protegido".
        cache.touch("cube");
        cache.touch("reciente");
        cache.touch("protegido");
        // El tiempo pasa: 16 s reales sin usar nada (1.000 avances de
        // 16 ms, el delta típico de un frame a 60 fps).
        for _ in 0..1000 {
            cache.advance_time_ms(16);
        }
        // "reciente" se vuelve a dibujar en el presente.
        cache.touch("reciente");

        let protected: HashSet<String> = ["protegido".to_string()].into_iter().collect();
        let evictions = cache.select_evictions(10_000, &protected);
        assert_eq!(evictions, vec!["viejo".to_string()]);
    }

    #[test]
    fn sin_ociosidad_no_se_expulsa_nada() {
        let mut cache = AssetCache::default();
        cache.meshes.insert("mundo_1".into(), fake_mesh());
        cache.touch("mundo_1");
        for _ in 0..10 {
            cache.advance_time_ms(16);
        }
        let evictions = cache.select_evictions(10_000, &HashSet::new());
        assert!(evictions.is_empty());
    }
}
