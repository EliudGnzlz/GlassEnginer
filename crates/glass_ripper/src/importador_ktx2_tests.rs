//! Tests de extremo a extremo de la Parte A: un .unitypackage sintético
//! importado deja cada `.ktx2` junto a su PNG.

use std::fs::{self, File};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use flate2::write::GzEncoder;
use flate2::Compression;

use crate::compresor_texturas::bytes_nivel_bc;
use crate::importador::importar_paquete;

const GUID_ALBEDO: &str = "0123456789abcdef0123456789abcdef";
const GUID_NORMAL: &str = "fedcba9876543210fedcba9876543210";

fn png_color(lado: u32, color: [u8; 4]) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(lado, lado, image::Rgba(color));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("codificar PNG de prueba");
    bytes
}

fn anadir_entrada(builder: &mut tar::Builder<GzEncoder<File>>, ruta: &str, datos: &[u8]) {
    let mut cabecera = tar::Header::new_gnu();
    cabecera.set_size(datos.len() as u64);
    cabecera.set_mode(0o644);
    cabecera.set_cksum();
    builder.append_data(&mut cabecera, ruta, datos).expect("entrada tar");
}

/// Escribe un .unitypackage con una entrada (pathname, asset, asset.meta) por textura.
fn construir_paquete(ruta: &Path, texturas: &[(&str, &str, &[u8])]) {
    let archivo = File::create(ruta).expect("crear paquete");
    let mut builder = tar::Builder::new(GzEncoder::new(archivo, Compression::default()));
    for (guid, ruta_unity, bytes) in texturas {
        anadir_entrada(&mut builder, &format!("{guid}/pathname"), format!("{ruta_unity}\n").as_bytes());
        anadir_entrada(&mut builder, &format!("{guid}/asset"), bytes);
        let meta = format!("fileFormatVersion: 2\nguid: {guid}\n");
        anadir_entrada(&mut builder, &format!("{guid}/asset.meta"), meta.as_bytes());
    }
    builder.into_inner().expect("cerrar tar").finish().expect("cerrar gzip");
}

/// Busca un archivo por nombre en `dir` y sus subdirectorios.
fn buscar(dir: &Path, nombre: &str) -> Option<PathBuf> {
    for entrada in fs::read_dir(dir).ok()?.flatten() {
        let ruta = entrada.path();
        if ruta.is_dir() {
            if let Some(encontrado) = buscar(&ruta, nombre) {
                return Some(encontrado);
            }
        } else if ruta.file_name().and_then(|n| n.to_str()) == Some(nombre) {
            return Some(ruta);
        }
    }
    None
}

fn directorio_prueba(nombre: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("glass_ripper_test_{nombre}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("crear directorio de prueba");
    dir
}

fn comprobar_ktx2_bc7_srgb_16(ruta: &Path) {
    let bytes = fs::read(ruta).expect("leer ktx2");
    let reader = ktx2::Reader::new(&bytes).expect("KTX2 válido");
    assert_eq!(reader.header().format, Some(ktx2::Format::BC7_SRGB_BLOCK));
    let nivel0 = reader.levels().next().expect("al menos un nivel");
    assert_eq!(nivel0.data.len(), bytes_nivel_bc(16, 16));
}

fn comprobar_png_16(ruta: &Path) {
    let bytes = fs::read(ruta).expect("leer png");
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .expect("PNG válido")
        .to_rgba8();
    assert_eq!(img.dimensions(), (16, 16));
}

#[test]
fn importe_sintetico_escribe_ktx2_junto_al_png() {
    let base = directorio_prueba("ktx2_junto_png");
    let paquete = base.join("prueba.unitypackage");
    let salida = base.join("salida");
    fs::create_dir_all(&salida).unwrap();
    let albedo = png_color(16, [255, 0, 0, 255]);
    let normal = png_color(16, [128, 128, 255, 255]);
    construir_paquete(
        &paquete,
        &[
            (GUID_ALBEDO, "Assets/Texturas/albedo.png", &albedo),
            (GUID_NORMAL, "Assets/Texturas/normal.png", &normal),
        ],
    );

    let _resultado = importar_paquete(&paquete, &salida, true, None);

    for stem in ["albedo", "normal"] {
        let png = buscar(&salida, &format!("{stem}.png")).unwrap_or_else(|| panic!("falta {stem}.png"));
        comprobar_png_16(&png);
        let ktx2 = png.with_extension("ktx2");
        assert!(ktx2.exists(), "falta {stem}.ktx2 junto al PNG");
        comprobar_ktx2_bc7_srgb_16(&ktx2);
    }
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn importe_sintetico_ktx2_roto_no_tumba_el_png() {
    let base = directorio_prueba("ktx2_roto");
    let paquete = base.join("prueba.unitypackage");
    let salida = base.join("salida");
    fs::create_dir_all(&salida).unwrap();
    let albedo = png_color(16, [255, 0, 0, 255]);
    construir_paquete(
        &paquete,
        &[
            (GUID_ALBEDO, "Assets/Texturas/albedo.png", &albedo),
            (GUID_NORMAL, "Assets/Texturas/normal.png", b"no es imagen"),
        ],
    );

    let _resultado = importar_paquete(&paquete, &salida, true, None);

    // NoConvertido: convertir_textura falla y no se escribe nada de esa textura.
    assert!(buscar(&salida, "normal.png").is_none());
    assert!(buscar(&salida, "normal.ktx2").is_none());
    let png = buscar(&salida, "albedo.png").expect("falta albedo.png");
    comprobar_png_16(&png);
    comprobar_ktx2_bc7_srgb_16(&png.with_extension("ktx2"));
    let _ = fs::remove_dir_all(&base);
}
