//! Ripper: importador de `.unitypackage` de Unity (≤2023) para
//! GlassEnginer. Lee el paquete (tar.gz con carpetas por GUID),
//! reconstruye el árbol por `pathname`, parsea los `.meta`, mantiene
//! el registro de GUIDs del proyecto y produce el informe de
//! importación. La conversión de assets vive en los sub-bloques
//! siguientes (U2–U5); este crate nace con el lector (U1).
//!
//! Diseño: `~/workspace/glass-engine/diseno_importacion_unity.md`.

pub mod compilacion;
pub mod compresor_texturas;
pub mod convertidor;
pub mod documentos;
pub mod ejes;
pub mod escena_fisica;
pub mod escenas;
pub mod entorno;
pub mod escritor;
pub mod fisico;
pub mod glb_ktx2;
pub mod importador;
pub mod informe;
pub mod instancias;
pub mod materiales;
pub mod meta;
pub mod modelos;
pub mod paquete;
pub mod registro;
pub mod scripts;
pub mod texturas;
pub mod texturas_ktx2;

#[cfg(test)]
mod importador_ktx2_tests;

use serde::{Deserialize, Serialize};

/// Errores del Ripper. Los mensajes van en español y con contexto:
/// acaban en la consola del editor tal cual.
#[derive(Debug, thiserror::Error)]
pub enum RipperError {
    #[error("No se pudo abrir el paquete '{ruta}': {motivo}")]
    Paquete { ruta: String, motivo: String },
    #[error("E/S en '{ruta}': {fuente}")]
    Io {
        ruta: String,
        #[source]
        fuente: std::io::Error,
    },
    #[error("El .meta del asset {guid} no se pudo parsear: {motivo}")]
    Meta { guid: String, motivo: String },
    #[error("No se pudo convertir '{ruta}': {motivo}")]
    Conversion { ruta: String, motivo: String },
}

/// Una carpeta GUID del `.unitypackage`, ya leída del tar.
#[derive(Debug, Clone)]
pub struct EntradaPaquete {
    pub guid: String,
    /// Primera línea de `pathname`; `None` si el archivo falta o
    /// está vacío.
    pub ruta: Option<String>,
    /// `true` si la entrada no trae archivo `asset` (carpeta de
    /// Unity).
    pub es_carpeta: bool,
    pub asset: Option<Vec<u8>>,
    pub meta_texto: Option<String>,
    pub preview: Option<Vec<u8>>,
}

/// Resultado de leer un `.unitypackage`: las entradas por GUID y
/// las rutas sueltas del tar que no tienen forma de carpeta GUID
/// (el informe las recoge como rechazadas; nada se pierde).
#[derive(Debug, Clone, Default)]
pub struct LecturaPaquete {
    pub entradas: Vec<EntradaPaquete>,
    pub sueltas: Vec<String>,
}

/// Sub-objeto de un asset compuesto (malla, clip…) según la tabla
/// de sub-objetos del `.meta`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubObjeto {
    pub class_id: i32,
    pub file_id: i64,
    pub nombre: String,
}

/// Ajustes del TextureImporter que U2 traduce a los del motor.
/// Los nombres son los de Unity ya decodificados (el `.meta` los
/// guarda como enteros).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TexturaInfo {
    /// "Default", "NormalMap", "Sprite", …
    pub tipo: String,
    pub srgb: bool,
    /// "Point", "Bilinear" o "Trilinear".
    pub filtro: String,
    /// "Repeat", "Clamp", "Mirror" o "MirrorOnce".
    pub wrap_u: String,
    pub wrap_v: String,
}

/// El `.meta` de un asset, parseado.
#[derive(Debug, Clone)]
pub struct MetaAsset {
    pub guid: String,
    /// Clave de nivel superior del importer en el `.meta`:
    /// "ModelImporter", "TextureImporter", "AudioImporter",
    /// "ScriptImporter", "FolderAsset", …
    pub importer: String,
    pub sub_objetos: Vec<SubObjeto>,
    /// Solo ModelImporter: `globalScale` y `useFileScale`.
    pub global_scale: Option<f32>,
    pub use_file_scale: Option<bool>,
    /// Solo TextureImporter.
    pub textura: Option<TexturaInfo>,
}

/// Estado de una entrada en el informe de importación.
/// Invariante del bloque: toda entrada del tar (por GUID o suelta)
/// aparece en el informe con un estado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EstadoInforme {
    Convertido,
    Copiado,
    ConvertidoConAvisos,
    NoConvertido,
    Rechazado,
    OmitidoPorColision,
}

/// Una línea del informe de importación.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineaInforme {
    pub guid: String,
    pub ruta: Option<String>,
    pub estado: EstadoInforme,
    pub detalle: String,
}

/// ¿El motor reproduce este formato de audio tal cual? Verificado
/// contra el código el 2026-11-25: `glass_audio` compila kira solo
/// con la feature `wav` y su gestor carga bytes WAV, así que en v1
/// solo `.wav` se copia directo; mp3/ogg/flac van al informe.
pub fn audio_es_copia_directa(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("wav")
}

#[cfg(test)]
mod tests_extremo_a_extremo {
    //! Composición de U1: paquete sintético → lectura → parseo de
    //! `.meta` → registro de GUIDs → informe con el invariante
    //! cumplido (toda entrada del tar tiene su línea).

    use std::fs::File;
    use std::io;
    use std::path::PathBuf;

    use flate2::write::GzEncoder;
    use flate2::Compression;

    use crate::informe::InformeImportacion;
    use crate::meta::parsear_meta;
    use crate::paquete::leer_paquete;
    use crate::registro::RegistroGuids;
    use crate::EstadoInforme;

    fn agregar(
        constructor: &mut tar::Builder<GzEncoder<File>>,
        ruta: &str,
        contenido: &[u8],
    ) -> io::Result<()> {
        let mut cabecera = tar::Header::new_gnu();
        cabecera.set_size(contenido.len() as u64);
        cabecera.set_mode(0o644);
        cabecera.set_cksum();
        constructor.append_data(&mut cabecera, ruta, contenido)
    }

    #[test]
    fn lectura_meta_registro_e_informe_componen() -> io::Result<()> {
        let dir = std::env::temp_dir().join(format!(
            "glass_ripper_e2e_{}",
            std::process::id()
        ));
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        let ruta_paquete: PathBuf = dir.join("sintetico.unitypackage");

        let guid_modelo = "3f2a9c1e5b7d4e8fa1c2b3d4e5f6a718";
        let meta_modelo = format!(
            "fileFormatVersion: 2\nguid: {guid_modelo}\nModelImporter:\n  serializedVersion: 20200\n  internalIDToNameTable:\n  - first:\n      43: 4300000\n    second: Tree3\n  meshes:\n    globalScale: 1\n    useFileScale: 1\n"
        );
        {
            let archivo = File::create(&ruta_paquete)?;
            let codificador = GzEncoder::new(archivo, Compression::default());
            let mut constructor = tar::Builder::new(codificador);
            agregar(
                &mut constructor,
                &format!("{guid_modelo}/pathname"),
                b"Assets/Modelos/arbol.fbx\n",
            )?;
            agregar(
                &mut constructor,
                &format!("{guid_modelo}/asset"),
                b"fbx-falso",
            )?;
            agregar(
                &mut constructor,
                &format!("{guid_modelo}/asset.meta"),
                meta_modelo.as_bytes(),
            )?;
            agregar(
                &mut constructor,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/pathname",
                b"Assets/Modelos\n",
            )?;
            agregar(&mut constructor, "LEEME.txt", b"suelto")?;
            let codificador = constructor.into_inner()?;
            codificador.finish()?;
        }

        let lectura = leer_paquete(&ruta_paquete).expect("el paquete sintético se lee");
        assert_eq!(lectura.entradas.len(), 2);
        assert_eq!(lectura.sueltas.len(), 1);

        let mut registro = RegistroGuids::nuevo();
        let mut informe = InformeImportacion::nuevo();
        for entrada in &lectura.entradas {
            let importer = match (&entrada.meta_texto, entrada.ruta.as_deref()) {
                (Some(texto), _) => {
                    let meta = parsear_meta(&entrada.guid, texto)
                        .expect("el .meta sintético parsea");
                    if entrada.guid == guid_modelo {
                        assert_eq!(meta.sub_objetos.len(), 1);
                        assert_eq!(meta.sub_objetos[0].nombre, "Tree3");
                        assert_eq!(meta.sub_objetos[0].class_id, 43);
                    }
                    meta.importer
                }
                _ => "SinMeta".to_string(),
            };
            if let Some(ruta) = &entrada.ruta {
                registro.registrar(&entrada.guid, ruta, &importer);
            }
            informe.anadir(
                &entrada.guid,
                entrada.ruta.as_deref(),
                EstadoInforme::Copiado,
                "lectura U1",
            );
        }
        for suelta in &lectura.sueltas {
            informe.anadir("", Some(suelta), EstadoInforme::Rechazado, "suelta");
        }

        assert!(informe.faltantes(&lectura).is_empty());
        let resuelta = registro
            .resolver(guid_modelo)
            .expect("el GUID del modelo se registró");
        assert_eq!(resuelta.ruta, "Assets/Modelos/arbol.fbx");
        assert_eq!(resuelta.importer, "ModelImporter");

        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
