use crate::RipperError;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Error as IoError, ErrorKind},
    path::Path,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntradaRegistro {
    pub guid: String,
    pub ruta: String,
    pub importer: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistroGuids {
    entradas: BTreeMap<String, EntradaRegistro>,
}

impl RegistroGuids {
    pub fn nuevo() -> Self {
        Self::default()
    }

    /// Registra un GUID. Devuelve true si era nuevo; false si ya
    /// existía y se sustituyó (señal de colisión).
    pub fn registrar(&mut self, guid: &str, ruta: &str, importer: &str) -> bool {
        let entrada = EntradaRegistro {
            guid: guid.to_string(),
            ruta: ruta.to_string(),
            importer: importer.to_string(),
        };

        self.entradas.insert(guid.to_string(), entrada).is_none()
    }

    pub fn resolver(&self, guid: &str) -> Option<&EntradaRegistro> {
        self.entradas.get(guid)
    }

    pub fn len(&self) -> usize {
        self.entradas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entradas.is_empty()
    }

    /// Guarda el registro como JSON bonito, creando los directorios padre.
    pub fn guardar(&self, ruta: &Path) -> Result<(), RipperError> {
        if let Some(padre) = ruta.parent() {
            if !padre.as_os_str().is_empty() {
                fs::create_dir_all(padre).map_err(|fuente| RipperError::Io {
                    ruta: padre.to_string_lossy().into_owned(),
                    fuente,
                })?;
            }
        }

        let json = serde_json::to_string_pretty(self).map_err(|error| RipperError::Io {
            ruta: ruta.to_string_lossy().into_owned(),
            fuente: IoError::new(ErrorKind::InvalidData, error.to_string()),
        })?;

        fs::write(ruta, json).map_err(|fuente| RipperError::Io {
            ruta: ruta.to_string_lossy().into_owned(),
            fuente,
        })?;

        Ok(())
    }

    /// Carga el registro. Un archivo ausente NO es error: devuelve
    /// el registro vacío. Un JSON inválido sí es error.
    pub fn cargar(ruta: &Path) -> Result<RegistroGuids, RipperError> {
        let texto = match fs::read_to_string(ruta) {
            Ok(texto) => texto,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(RegistroGuids::nuevo());
            }
            Err(fuente) => {
                return Err(RipperError::Io {
                    ruta: ruta.to_string_lossy().into_owned(),
                    fuente,
                });
            }
        };

        serde_json::from_str(&texto).map_err(|error| RipperError::Io {
            ruta: ruta.to_string_lossy().into_owned(),
            fuente: IoError::new(ErrorKind::InvalidData, error.to_string()),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivoIntegrado {
    RecursosPorDefecto,
    RecursosExtra,
    Desconocido,
}

/// GUIDs integrados de Unity: ningún .meta los declara.
/// - "0000000000000000e000000000000000" (recursos por defecto: Default-Material, primitivas) → RecursosPorDefecto
/// - "0000000000000000f000000000000000" (recursos extra: shaders integrados, p. ej. el Standard) → RecursosExtra
/// - cualquier otro GUID que empiece por 16 ceros → Desconocido
/// - el resto → None
pub fn integrado(guid: &str) -> Option<ActivoIntegrado> {
    if !guid.starts_with("0000000000000000") {
        return None;
    }

    match guid {
        "0000000000000000e000000000000000" => Some(ActivoIntegrado::RecursosPorDefecto),
        "0000000000000000f000000000000000" => Some(ActivoIntegrado::RecursosExtra),
        _ => Some(ActivoIntegrado::Desconocido),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static CONTADOR: AtomicU64 = AtomicU64::new(0);

    fn ruta_temporal(nombre: &str) -> std::path::PathBuf {
        let id = CONTADOR.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "glass_ripper_registro_{}_{}_{}.json",
            nombre,
            std::process::id(),
            id
        ))
    }

    #[test]
    fn registrar_nuevo_devuelve_true_y_sustituye() {
        let mut registro = RegistroGuids::nuevo();

        assert!(registro.registrar("guid-1", "Assets/Uno.prefab", "PrefabImporter"));
        assert!(registro.resolver("guid-1").is_some());
        if let Some(entrada) = registro.resolver("guid-1") {
            assert_eq!(entrada.ruta.as_str(), "Assets/Uno.prefab");
        }

        assert!(!registro.registrar("guid-1", "Assets/Dos.prefab", "PrefabImporter"));
        if let Some(entrada) = registro.resolver("guid-1") {
            assert_eq!(entrada.ruta.as_str(), "Assets/Dos.prefab");
        }
    }

    #[test]
    fn guardar_y_cargar_reproducen_el_registro() {
        let ruta = ruta_temporal("roundtrip");

        let mut original = RegistroGuids::nuevo();
        assert!(original.registrar("g1", "Assets/Uno.prefab", "PrefabImporter"));
        assert!(original.registrar("g2", "Assets/Dos.mat", "MaterialImporter"));

        assert!(original.guardar(&ruta).is_ok());

        let cargado = RegistroGuids::cargar(&ruta);
        assert!(cargado.is_ok());
        if let Ok(cargado) = cargado {
            assert_eq!(cargado.len(), 2);
            if let Some(entrada) = cargado.resolver("g1") {
                assert_eq!(entrada.ruta.as_str(), "Assets/Uno.prefab");
                assert_eq!(entrada.importer.as_str(), "PrefabImporter");
            }
            if let Some(entrada) = cargado.resolver("g2") {
                assert_eq!(entrada.ruta.as_str(), "Assets/Dos.mat");
                assert_eq!(entrada.importer.as_str(), "MaterialImporter");
            }
        }

        let _ = std::fs::remove_file(&ruta);
    }

    #[test]
    fn cargar_inexistente_devuelve_vacio_sin_error() {
        let ruta = ruta_temporal("inexistente");
        let _ = std::fs::remove_file(&ruta);

        let cargado = RegistroGuids::cargar(&ruta);
        assert!(cargado.is_ok());
        if let Ok(cargado) = cargado {
            assert!(cargado.is_empty());
        }
    }

    #[test]
    fn integrado_clasifica_guids() {
        assert_eq!(
            integrado("0000000000000000e000000000000000"),
            Some(ActivoIntegrado::RecursosPorDefecto)
        );
        assert_eq!(
            integrado("0000000000000000f000000000000000"),
            Some(ActivoIntegrado::RecursosExtra)
        );
        assert_eq!(
            integrado("0000000000000000aaaaaaaaaaaaaaaa"),
            Some(ActivoIntegrado::Desconocido)
        );
        assert_eq!(integrado("0123456789abcdef0123456789abcdef"), None);
    }
}
