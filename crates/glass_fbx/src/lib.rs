//! Convertidor FBX nativo del motor: lee un `.fbx` con ufbx y escribe
//! el `.glb` (más su sidecar `.meta.json`) que el resto del motor ya
//! consume, sin necesitar Blender. El `.blend` NO pasa por aquí: sigue
//! por el bridge de Blender (`glass_renderer::watcher`).
//!
//! Punto de entrada: [`convertir_fbx`]. La lectura vive en `lector`
//! (ufbx → [`modelo::EscenaIr`]) y la escritura en `escritor`
//! (IR → GLB binario); el IR es el contrato entre ambos.

pub mod escritor;
pub mod lector;
pub mod modelo;

use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Errores del convertidor. Los mensajes van en español y con contexto:
/// acaban en la consola del editor tal cual.
#[derive(Debug, thiserror::Error)]
pub enum FbxError {
    #[error("No se pudo leer el FBX '{ruta}': {motivo}")]
    Lectura { ruta: String, motivo: String },
    #[error("No se pudo escribir '{ruta}': {motivo}")]
    Escritura { ruta: String, motivo: String },
    #[error("E/S al convertir '{ruta}': {fuente}")]
    Io {
        ruta: String,
        #[source]
        fuente: std::io::Error,
    },
}

/// Conteos del modelo convertido, con el mismo significado que los del
/// sidecar del bridge de Blender (para poder comparar ambas vías).
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Conteos {
    pub meshes: usize,
    pub nodes: usize,
    pub materials: usize,
    pub animations: usize,
    pub skins: usize,
    pub bytes: usize,
}

/// Resultado de una conversión: los conteos del sidecar y los avisos
/// que el lector anotó por el camino (nada se esconde).
#[derive(Debug)]
pub struct InformeConversion {
    pub conteos: Conteos,
    pub avisos: Vec<String>,
}

/// Sidecar `<glb>.meta.json`: la misma forma que escribe el bridge de
/// Blender en lo que el resto del motor lee (`source` para la vista
/// única del panel Proyecto, `source_sha256` para frescura), más la
/// marca del convertidor que lo produjo.
#[derive(Serialize)]
struct Sidecar<'a> {
    source: &'a str,
    source_sha256: String,
    converter: &'a str,
    counts: &'a Conteos,
}

/// Convierte `fuente` (un `.fbx`) en `destino_glb` y escribe su
/// sidecar al lado. No toca la fuente. Si algo falla, no deja un GLB a
/// medias: primero se genera todo en memoria y solo entonces se escribe.
pub fn convertir_fbx(fuente: &Path, destino_glb: &Path) -> Result<InformeConversion, FbxError> {
    let escena = lector::leer_escena(fuente)?;
    let bytes_glb = escritor::escribir_glb(&escena);

    let conteos = Conteos {
        meshes: escena.mallas.len(),
        nodes: escena.nodos.len(),
        materials: escena.materiales.len(),
        animations: escena.animaciones.len(),
        skins: if escena.piel.is_some() { 1 } else { 0 },
        bytes: bytes_glb.len(),
    };

    std::fs::write(destino_glb, &bytes_glb).map_err(|e| FbxError::Io {
        ruta: destino_glb.display().to_string(),
        fuente: e,
    })?;

    // El hash se calcula de la fuente ya convertida: si cambió durante
    // la conversión, el sidecar describe otra cosa y el vigía lo
    // detectará como desactualizado en su siguiente pasada.
    let datos_fuente = std::fs::read(fuente).map_err(|e| FbxError::Io {
        ruta: fuente.display().to_string(),
        fuente: e,
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&datos_fuente);
    let sha = format!("{:x}", hasher.finalize());

    let nombre_fuente = fuente
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let sidecar = Sidecar {
        source: &nombre_fuente,
        source_sha256: sha,
        converter: "glass_fbx (ufbx)",
        counts: &conteos,
    };
    let ruta_sidecar = destino_glb.with_extension("glb.meta.json");
    let texto = serde_json::to_string_pretty(&sidecar).map_err(|e| FbxError::Escritura {
        ruta: ruta_sidecar.display().to_string(),
        motivo: e.to_string(),
    })?;
    std::fs::write(&ruta_sidecar, texto).map_err(|e| FbxError::Io {
        ruta: ruta_sidecar.display().to_string(),
        fuente: e,
    })?;

    Ok(InformeConversion {
        conteos,
        avisos: escena.avisos,
    })
}
