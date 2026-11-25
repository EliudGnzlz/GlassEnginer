use crate::RipperError;
use std::fs;
use std::io;
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultadoEscritura {
    Escrito,
    OmitidoPorColision,
}

pub fn escribir_asset(
    raiz: &Path,
    ruta_rel: &Path,
    bytes: &[u8],
    forzar: bool,
) -> Result<ResultadoEscritura, RipperError> {
    if ruta_rel.is_absolute()
        || ruta_rel
            .components()
            .any(|componente| matches!(componente, Component::ParentDir))
    {
        return Err(RipperError::Io {
            ruta: ruta_rel.to_string_lossy().into_owned(),
            fuente: io::Error::new(io::ErrorKind::InvalidInput, "ruta de salida no válida"),
        });
    }

    let destino = raiz.join(ruta_rel);
    let ruta_destino = destino.to_string_lossy().into_owned();

    if let Some(padre) = destino.parent() {
        fs::create_dir_all(padre).map_err(|fuente| RipperError::Io {
            ruta: ruta_destino.clone(),
            fuente,
        })?;
    }

    if destino.exists() && !forzar {
        return Ok(ResultadoEscritura::OmitidoPorColision);
    }

    fs::write(&destino, bytes).map_err(|fuente| RipperError::Io {
        ruta: ruta_destino,
        fuente,
    })?;
    Ok(ResultadoEscritura::Escrito)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::path::PathBuf;
    use std::process;

    struct DirectorioTemporal {
        ruta: PathBuf,
    }

    impl DirectorioTemporal {
        fn nuevo() -> io::Result<Self> {
            let ruta = std::env::temp_dir().join(format!(
                "glass_ripper_escritor_tests_{}",
                process::id()
            ));
            if ruta.exists() {
                fs::remove_dir_all(&ruta)?;
            }
            fs::create_dir_all(&ruta)?;
            Ok(Self { ruta })
        }
    }

    impl Drop for DirectorioTemporal {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.ruta);
        }
    }

    #[test]
    fn escribe_respeta_colisiones_y_permite_forzar() -> Result<(), Box<dyn Error>> {
        let temporal = DirectorioTemporal::nuevo()?;
        let relativa = Path::new("Modelos/arbol.bin");
        let destino = temporal.ruta.join(relativa);

        assert_eq!(
            escribir_asset(&temporal.ruta, relativa, b"original", false)?,
            ResultadoEscritura::Escrito
        );
        assert_eq!(fs::read(&destino)?, b"original");

        assert_eq!(
            escribir_asset(&temporal.ruta, relativa, b"descartado", false)?,
            ResultadoEscritura::OmitidoPorColision
        );
        assert_eq!(fs::read(&destino)?, b"original");

        assert_eq!(
            escribir_asset(&temporal.ruta, relativa, b"nuevo", true)?,
            ResultadoEscritura::Escrito
        );
        assert_eq!(fs::read(&destino)?, b"nuevo");

        assert!(escribir_asset(
            &temporal.ruta,
            Path::new("Modelos/../fuera.bin"),
            b"datos",
            false
        )
        .is_err());

        Ok(())
    }
}
