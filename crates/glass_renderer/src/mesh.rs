use ash::vk;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator};
use gpu_allocator::MemoryLocation;
use std::sync::Arc;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

pub struct GpuMesh {
    pub vertex_buffer: vk::Buffer,
    pub vertex_allocation: Option<Allocation>,
    pub index_buffer: vk::Buffer,
    pub index_allocation: Option<Allocation>,
    pub index_count: u32,
    pub vertex_count: u32,
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

pub struct MeshLoader;

impl MeshLoader {
    /// Carga vértices e índices desde un buffer binario glTF (.glb)
    pub fn load_from_glb_bytes(bytes: &[u8]) -> Result<(Vec<Vertex>, Vec<u32>), String> {
        let (document, buffers, _images) =
            gltf::import_slice(bytes).map_err(|e| format!("Error importando glTF: {}", e))?;

        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for mesh in document.meshes() {
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

                let positions: Vec<[f32; 3]> = reader
                    .read_positions()
                    .map(|iter| iter.collect())
                    .unwrap_or_default();

                let normals: Vec<[f32; 3]> = reader
                    .read_normals()
                    .map(|iter| iter.collect())
                    .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);

                let uvs: Vec<[f32; 2]> = reader
                    .read_tex_coords(0)
                    .map(|iter| iter.into_f32().collect())
                    .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);

                let base_index = vertices.len() as u32;

                for i in 0..positions.len() {
                    vertices.push(Vertex {
                        position: positions[i],
                        normal: normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
                        uv: uvs.get(i).copied().unwrap_or([0.0, 0.0]),
                    });
                }

                if let Some(read_indices) = reader.read_indices() {
                    for idx in read_indices.into_u32() {
                        indices.push(base_index + idx);
                    }
                } else {
                    for idx in 0..positions.len() as u32 {
                        indices.push(base_index + idx);
                    }
                }
            }
        }

        if vertices.is_empty() {
            return Err("El archivo glTF no contiene primitivas de malla válidas".into());
        }

        Ok((vertices, indices))
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
        let vertex_size = (vertices.len() * std::mem::size_of::<Vertex>()) as u64;
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
                    vertices.as_ptr() as *const u8,
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
                vertex_count: vertices.len() as u32,
            })
        }
    }

    unsafe fn create_buffer(
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
