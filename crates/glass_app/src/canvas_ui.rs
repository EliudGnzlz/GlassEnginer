//! Utilidades de UI del Canvas que aún viven aquí: fuentes, mapa de
//! imágenes de la escena (`build_ui_scene_images`), el tipo de clic
//! completado (`CanvasButtonClick`) y la migración de escenas viejas.
//! El lote de dibujo, el hit-test, el rastreador de clic y el arrastre
//! ya son V2 (`glass_ui::api`, `glass_ui::pintor` y
//! `crate::ui_v2::build_canvas_batch_v2`); este módulo se borra en P4d.

use glass_core::SceneState;
use glass_renderer::{FontAtlas, UiAtlasKind};
use std::collections::HashMap;
use std::path::Path;


/// Fuentes disponibles para maquetar el Canvas: la incrustada
/// («default», la DejaVu) y las cargadas de assets/fonts por
/// nombre. La app mantiene los atlas de CPU (para medir) y el
/// renderer, los pases GPU con el mismo nombre.
pub struct UiFonts<'a> {
    pub default: &'a FontAtlas,
    pub named: &'a std::collections::HashMap<String, FontAtlas>,
}

impl<'a> UiFonts<'a> {
    /// Atlas y tipo de segmento para un nombre de fuente:
    /// «default», vacío o desconocido → la incrustada.
    pub fn resolve(&self, name: &str) -> (&'a FontAtlas, UiAtlasKind) {
        if name.is_empty() || name == "default" {
            return (self.default, UiAtlasKind::Font);
        }
        match self.named.get(name) {
            Some(atlas) => (atlas, UiAtlasKind::FontNamed(name.to_string())),
            None => (self.default, UiAtlasKind::Font),
        }
    }
}

/// Construye los segmentos de dibujo del canvas en orden de pintor.
/// Un clic completado sobre un botón del Canvas: qué entidad es y qué
/// acción declara su `UIButtonData` (`on_click_action`, el «On Click
/// Event» del inspector).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasButtonClick {
    pub entity_id: u64,
    pub entity_name: String,
    pub action: String,
}


/// Entrada del mapa de imágenes de la escena: uv en el atlas y tamaño
/// original en píxeles (necesario para `preserve_aspect`).
#[derive(Clone, Debug)]
pub struct UiImageEntry {
    pub uv: [f32; 4],
    pub pixel_size: (u32, u32),
}

/// Sprites de la escena indexados por nombre de sprite.
pub type UiSceneImages = HashMap<String, UiImageEntry>;

/// Recoge los sprites distintos de las entidades activas (en orden de
/// primera aparición), los decodifica desde `assets_root` y los empaqueta
/// en un atlas. Devuelve el mapa de sprites y el atlas (si hubo alguno).
pub fn build_ui_scene_images(
    scene: &SceneState,
    assets_root: &Path,
) -> (UiSceneImages, Option<glass_renderer::UiImageAtlas>) {
    let mut mapa: UiSceneImages = UiSceneImages::new();

    // 1) nombres distintos, orden determinista = orden de aparición
    let mut nombres: Vec<String> = Vec::new();
    for ent in scene.entities.iter().filter(|e| e.active) {
        if let Some(img) = &ent.ui_image {
            if !img.sprite.is_empty() && !nombres.iter().any(|n| n == &img.sprite) {
                nombres.push(img.sprite.clone());
            }
        }
    }
    if nombres.is_empty() {
        return (mapa, None);
    }

    // 2) decodificación (los fallos se registran y se saltan)
    let mut decodificadas: Vec<(String, image::RgbaImage)> = Vec::new();
    for nombre in nombres {
        let ruta = assets_root.join(&nombre);
        match image::ImageReader::open(&ruta) {
            Ok(reader) => match reader.decode() {
                Ok(dinamica) => decodificadas.push((nombre, dinamica.to_rgba8())),
                Err(e) => log::warn!("no se pudo decodificar {:?}: {}", ruta, e),
            },
            Err(e) => log::warn!("no se pudo abrir {:?}: {}", ruta, e),
        }
    }
    if decodificadas.is_empty() {
        return (mapa, None);
    }

    // 3) empaquetado en el mismo orden de primera aparición
    let refs: Vec<(u32, u32, &[u8])> = decodificadas
        .iter()
        .map(|(_, img)| (img.width(), img.height(), img.as_raw().as_slice()))
        .collect();
    match glass_renderer::pack_ui_image_atlas(&refs, 2048) {
        Some(atlas) => {
            for ((nombre, img), uv) in decodificadas.iter().zip(atlas.uvs.iter()) {
                mapa.insert(
                    nombre.clone(),
                    UiImageEntry {
                        uv: *uv,
                        pixel_size: (img.width(), img.height()),
                    },
                );
            }
            (mapa, Some(atlas))
        }
        None => (mapa, None),
    }
}
/// Fase UI/Canvas (V1): tras cargar una escena cuyo esquema de UI es
/// anterior al actual, la migra EN MEMORIA con `glass_ui` y sube su
/// versión. Solo el editor escribe además la copia de seguridad
/// `<escena>.pre-ui.bak` (una sola vez, antes de tocar nada, y solo
/// si se conoce la ruta): el runtime migra sin escribir en disco y el
/// archivo original solo cambia cuando el usuario guarda la escena.
/// Sin migración pendiente, no hace nada.
pub fn migrar_ui_escena_cargada(scene: &mut SceneState, ruta: Option<&str>) {
    if !glass_ui::migracion::necesita_migracion(scene.ui_schema_version) {
        return;
    }
    let _ = ruta; // en el build de juego la ruta no se usa (no hay copia)
    #[cfg(feature = "editor")]
    if let Some(ruta) = ruta {
        match glass_ui::migracion::escribir_copia_pre_ui(std::path::Path::new(ruta)) {
            Ok(true) => {
                log::info!("Migración UI: copia previa de '{ruta}' guardada (.pre-ui.bak)")
            }
            Ok(false) => {}
            Err(e) => {
                log::warn!("Migración UI: sin copia previa de '{ruta}': {e}")
            }
        }
    }
    let mut siguiente_id = scene.next_entity_id;
    let informe = glass_ui::migracion::migrar_entidades(&mut scene.entities, &mut siguiente_id);
    if siguiente_id > scene.next_entity_id {
        scene.next_entity_id = siguiente_id;
    }
    scene.ui_schema_version = glass_core::scene::UI_SCHEMA_VERSION_ACTUAL;
    log::info!(
        "Migración UI aplicada ({} entidades creadas, {} avisos)",
        informe.entidades_creadas,
        informe.avisos.len()
    );
    for aviso in &informe.avisos {
        log::warn!("Migración UI: {aviso}");
    }
}
