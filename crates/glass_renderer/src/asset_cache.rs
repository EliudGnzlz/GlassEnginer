use crate::mesh::GpuMesh;
use gpu_allocator::vulkan::Allocator;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct AssetCache {
    pub meshes: HashMap<String, GpuMesh>,
}

impl Default for AssetCache {
    fn default() -> Self {
        Self {
            meshes: HashMap::new(),
        }
    }
}

impl AssetCache {
    pub fn new() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self::default()))
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
    }

    pub fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        for (name, mut mesh) in self.meshes.drain() {
            log::debug!("Liberando malla '{}' de VRAM", name);
            mesh.destroy(device, allocator);
        }
    }
}
