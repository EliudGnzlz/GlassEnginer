use crate::{EntradaPaquete, LecturaPaquete, RipperError};
use flate2::read::GzDecoder;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn leer_paquete(ruta: &Path) -> Result<LecturaPaquete, RipperError> {
    let ruta_paquete = ruta.to_string_lossy().into_owned();
    let archivo = File::open(ruta).map_err(|fuente| RipperError::Io {
        ruta: ruta_paquete.clone(),
        fuente,
    })?;
    let decodificador = GzDecoder::new(archivo);
    let mut archivo_tar = tar::Archive::new(decodificador);
    let entradas_tar = archivo_tar.entries().map_err(|error| RipperError::Paquete {
        ruta: ruta_paquete.clone(),
        motivo: format!("no se pudieron leer las entradas tar/gzip: {error}"),
    })?;

    let mut agrupadas: BTreeMap<String, EntradaPaquete> = BTreeMap::new();
    let mut sueltas = Vec::new();

    for entrada in entradas_tar {
        let mut entrada = entrada.map_err(|error| RipperError::Paquete {
            ruta: ruta_paquete.clone(),
            motivo: format!("entrada tar/gzip no válida: {error}"),
        })?;
        let ruta_completa = entrada
            .path()
            .map_err(|error| RipperError::Paquete {
                ruta: ruta_paquete.clone(),
                motivo: format!("ruta de entrada tar no válida: {error}"),
            })?
            .to_string_lossy()
            .into_owned();

        if ruta_completa.ends_with('/') {
            continue;
        }

        let mut partes = ruta_completa.split('/');
        let carpeta = partes.next().unwrap_or_default();
        if !es_guid(carpeta) {
            sueltas.push(ruta_completa);
            continue;
        }

        let guid = carpeta.to_ascii_lowercase();
        let nombre_interno = partes.next().unwrap_or_default();
        let registro = agrupadas
            .entry(guid.clone())
            .or_insert_with(|| EntradaPaquete {
                guid,
                ruta: None,
                es_carpeta: true,
                asset: None,
                meta_texto: None,
                preview: None,
            });

        match nombre_interno {
            "pathname" => {
                let mut contenido = Vec::new();
                entrada.read_to_end(&mut contenido).map_err(|error| {
                    RipperError::Paquete {
                        ruta: ruta_paquete.clone(),
                        motivo: format!("no se pudo leer pathname: {error}"),
                    }
                })?;
                let texto = String::from_utf8_lossy(&contenido);
                let primera_linea = texto.lines().next().unwrap_or_default().trim();
                registro.ruta = if primera_linea.is_empty() {
                    None
                } else {
                    Some(primera_linea.to_owned())
                };
            }
            "asset" => {
                let mut contenido = Vec::new();
                entrada.read_to_end(&mut contenido).map_err(|error| {
                    RipperError::Paquete {
                        ruta: ruta_paquete.clone(),
                        motivo: format!("no se pudo leer asset: {error}"),
                    }
                })?;
                registro.asset = Some(contenido);
            }
            "asset.meta" => {
                let mut contenido = Vec::new();
                entrada.read_to_end(&mut contenido).map_err(|error| {
                    RipperError::Paquete {
                        ruta: ruta_paquete.clone(),
                        motivo: format!("no se pudo leer asset.meta: {error}"),
                    }
                })?;
                registro.meta_texto = Some(String::from_utf8_lossy(&contenido).into_owned());
            }
            "preview.png" => {
                let mut contenido = Vec::new();
                entrada.read_to_end(&mut contenido).map_err(|error| {
                    RipperError::Paquete {
                        ruta: ruta_paquete.clone(),
                        motivo: format!("no se pudo leer preview.png: {error}"),
                    }
                })?;
                registro.preview = Some(contenido);
            }
            _ => {
                // Otros archivos internos no forman parte de la lectura.
            }
        }
    }

    let entradas = agrupadas
        .into_values()
        .map(|mut entrada| {
            entrada.es_carpeta = entrada.asset.is_none();
            entrada
        })
        .collect();

    Ok(LecturaPaquete { entradas, sueltas })
}

fn es_guid(carpeta: &str) -> bool {
    carpeta.len() == 32 && carpeta.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn validar_ruta(ruta: &str) -> Result<PathBuf, String> {
    let ruta = ruta.trim();
    if ruta.is_empty() {
        return Err("la ruta está vacía".to_owned());
    }
    if ruta.starts_with('/')
        || ruta.starts_with('\\')
        || ruta.chars().nth(1) == Some(':')
    {
        return Err("la ruta absoluta no está permitida".to_owned());
    }

    let componentes_crudos: Vec<&str> = ruta
        .split(['/', '\\'])
        .collect();
    if componentes_crudos.contains(&"..") {
        return Err("la ruta contiene un componente '..'".to_owned());
    }

    let componentes: Vec<&str> = componentes_crudos
        .into_iter()
        .filter(|componente| !componente.is_empty() && *componente != ".")
        .collect();

    let Some(primero) = componentes.first() else {
        return Err("la ruta no contiene componentes válidos".to_owned());
    };
    if *primero != "Assets" && *primero != "Packages" {
        return Err("la ruta debe comenzar por 'Assets' o 'Packages'".to_owned());
    }
    if componentes.len() < 2 {
        return Err("la ruta debe contener al menos dos componentes".to_owned());
    }

    let mut resultado = PathBuf::new();
    for componente in componentes {
        resultado.push(componente);
    }
    Ok(resultado)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::error::Error;
    use std::fs;
    use std::io;
    use std::process;

    struct DirectorioTemporal {
        ruta: PathBuf,
    }

    impl DirectorioTemporal {
        fn nuevo() -> io::Result<Self> {
            let ruta = std::env::temp_dir().join(format!(
                "glass_ripper_paquete_tests_{}",
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

    fn agregar_archivo(
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
    fn valida_rutas_permitidas_y_rechaza_peligrosas() {
        assert_eq!(
            validar_ruta("Assets/Modelos/arbol.fbx").ok(),
            Some(PathBuf::from("Assets/Modelos/arbol.fbx"))
        );
        assert!(validar_ruta("Assets/../fuera.txt").is_err());
        assert!(validar_ruta("/etc/passwd").is_err());
        assert!(validar_ruta("Assets\\..\\fuera.txt").is_err());
        assert!(validar_ruta("Modelos/x.png").is_err());
        assert!(validar_ruta("").is_err());
        assert!(validar_ruta("Assets").is_err());
    }

    #[test]
    fn lee_paquete_de_ida_y_vuelta() -> Result<(), Box<dyn Error>> {
        let temporal = DirectorioTemporal::nuevo()?;
        let ruta_paquete = temporal.ruta.join("prueba.unitypackage");
        let archivo = File::create(&ruta_paquete)?;
        let codificador = GzEncoder::new(archivo, Compression::default());
        let mut constructor = tar::Builder::new(codificador);

        agregar_archivo(
            &mut constructor,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/pathname",
            b"Assets/Modelos/arbol.fbx\n",
        )?;
        agregar_archivo(
            &mut constructor,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/asset",
            b"\x00\x01\x02asset",
        )?;
        agregar_archivo(
            &mut constructor,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/asset.meta",
            b"fileFormatVersion: 2\nguid: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
        )?;
        agregar_archivo(
            &mut constructor,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/pathname",
            b"Assets/Modelos\n",
        )?;
        agregar_archivo(&mut constructor, "LEEME.txt", b"texto")?;

        let codificador = constructor.into_inner()?;
        codificador.finish()?;

        let lectura = leer_paquete(&ruta_paquete)?;
        assert_eq!(lectura.entradas.len(), 2);
        assert_eq!(lectura.sueltas, vec!["LEEME.txt".to_owned()]);

        let primera = &lectura.entradas[0];
        assert_eq!(primera.guid, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(primera.ruta.as_deref(), Some("Assets/Modelos/arbol.fbx"));
        assert_eq!(primera.asset.as_deref(), Some(b"\x00\x01\x02asset".as_slice()));
        assert!(primera.meta_texto.is_some());
        assert!(!primera.es_carpeta);

        let segunda = &lectura.entradas[1];
        assert_eq!(segunda.guid, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert_eq!(segunda.ruta.as_deref(), Some("Assets/Modelos"));
        assert!(segunda.asset.is_none());
        assert!(segunda.es_carpeta);

        Ok(())
    }
}
