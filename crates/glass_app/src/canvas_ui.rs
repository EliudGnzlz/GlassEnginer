//! Construcción del lote de UI del Canvas para el pase 2D del renderer
//! (sin egui), usado en el modo standalone.
//!
//! Recorre las entidades de la escena activa y traduce cada elemento de
//! UI a quads del `UiBatch`, con EXACTAMENTE la misma semántica visual
//! que la superposición de egui del editor
//! (`render_in_game_canvas_ui` en glass_editor):
//! - La posición la resuelve `glass_core::resolve_ui_rect` (misma
//!   fórmula que el editor).
//! - Un botón es un rectángulo con `target_graphic_color` y su etiqueta
//!   (el texto de su `UITextData`, o «Button») centrada y en blanco.
//! - Un texto suelto se dibuja desde la esquina superior izquierda de
//!   su rect, con su color y su tamaño (mínimo 10, como en el editor).
//! - El orden de dibujo es el orden de las entidades en la escena.
//!
//! Este módulo también resuelve la mitad «de escena» de los clics:
//! `hit_test_button` localiza el botón bajo un punto reutilizando
//! `resolve_ui_rect` (la misma resolución que pinta), y
//! `ButtonClickTracker` implementa la semántica de clic pulsación-dentro
//! → liberación-dentro. La captura de eventos de winit vive en
//! `mouse_input` y el cableado que activa el botón (log + llamada FFI a
//! C#) en `main.rs`; ver REGISTRO_BETA_INPUT_RATON.md.

use glass_core::SceneState;
use glass_renderer::{FontAtlas, UiAtlasKind, UiBatch, UiSegment};

/// Color de las etiquetas de botón, como en el editor (blanco).
const BUTTON_LABEL_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Tamaño de etiqueta cuando el botón no lleva `UITextData` propio
/// (el botón de egui del editor usa su tamaño por defecto, ~14).
const BUTTON_LABEL_SIZE: f32 = 14.0;

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

/// Devuelve el lote del último segmento si ya es del mismo atlas;
/// si no, crea un segmento nuevo y devuelve su lote.
/// (No se usa un closure para evitar problemas de préstamos.)
fn segment_mut(segs: &mut Vec<UiSegment>, kind: UiAtlasKind) -> &mut UiBatch {
    let mismo = matches!(segs.last(), Some(s) if s.atlas == kind);
    if !mismo {
        segs.push(UiSegment {
            atlas: kind,
            batch: UiBatch::default(),
        });
    }
    &mut segs.last_mut().expect("segmento recién creado").batch
}

/// Construye los segmentos de dibujo del canvas en orden de pintor.
/// Por cada entidad: primero su imagen (si la hay) y después botón/texto.
pub fn build_canvas_batch(
    scene: &SceneState,
    fonts: &UiFonts<'_>,
    images: &UiSceneImages,
    screen_w: f32,
    screen_h: f32,
) -> Vec<UiSegment> {
    let mut segs: Vec<UiSegment> = Vec::new();
    // Fase 5: rects resueltos con jerarquía, Canvas Scaler y layout.
    let rects = glass_core::resolve_scene_ui_rects(scene, screen_w, screen_h);

    for ent in scene
        .entities
        .iter()
        .filter(|e| e.active && e.rect_transform.is_some())
    {
        let rect = rects.get(&ent.id).copied().unwrap_or([0.0, 0.0, 0.0, 0.0]);

        // (1) imagen de la escena con su tinte (segmento Image)
        if let Some(img) = &ent.ui_image {
            if !img.sprite.is_empty() {
                if let Some(entry) = images.get(img.sprite.as_str()) {
                    let mut dst = rect;
                    if img.preserve_aspect {
                        let iw = entry.pixel_size.0 as f32;
                        let ih = entry.pixel_size.1 as f32;
                        if iw > 0.0 && ih > 0.0 {
                            // scale = min(rw/iw, rh/ih), centrado en el rect
                            let scale = (rect[2] / iw).min(rect[3] / ih);
                            let w = iw * scale;
                            let h = ih * scale;
                            dst = [
                                rect[0] + (rect[2] - w) * 0.5,
                                rect[1] + (rect[3] - h) * 0.5,
                                w,
                                h,
                            ];
                        }
                    }
                    let batch = segment_mut(&mut segs, UiAtlasKind::Image);
                    // quad texturizado con uv normalizadas y tinte
                    batch.push_image(dst, entry.uv, img.color);
                }
            }
        }

        // (2) botón o texto, en el segmento de SU fuente: la
        // etiqueta del botón usa la fuente de su UITextData si la
        // tiene; el texto suelto respeta fuente y alineación.
        if let Some(btn) = &ent.ui_button {
            {
                let (atlas_btn, kind_btn) = fonts.resolve(
                    ent.ui_text
                        .as_ref()
                        .map(|t| t.font.as_str())
                        .unwrap_or("default"),
                );
                let batch = segment_mut(&mut segs, kind_btn);
                batch.push_solid(atlas_btn, rect, btn.target_graphic_color);
                let label = ent
                    .ui_text
                    .as_ref()
                    .map(|t| t.text.as_str())
                    .unwrap_or("Button");
                let size = ent
                    .ui_text
                    .as_ref()
                    .map(|t| t.font_size)
                    .unwrap_or(BUTTON_LABEL_SIZE)
                    .max(10.0);
                batch.push_text_centered(atlas_btn, rect, label, size, BUTTON_LABEL_COLOR);
            }
        } else if let Some(txt) = &ent.ui_text {
            let (atlas_txt, kind_txt) = fonts.resolve(txt.font.as_str());
            let batch = segment_mut(&mut segs, kind_txt);
            batch.push_text_aligned(
                atlas_txt,
                rect,
                &txt.text,
                txt.font_size.max(10.0),
                txt.color,
                &txt.align,
            );
        }
    }

    // sin contenido: un único segmento Font vacío
    if segs.is_empty() {
        segs.push(UiSegment {
            atlas: UiAtlasKind::Font,
            batch: UiBatch::default(),
        });
    }

    segs
}
/// Un clic completado sobre un botón del Canvas: qué entidad es y qué
/// acción declara su `UIButtonData` (`on_click_action`, el «On Click
/// Event» del inspector).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasButtonClick {
    pub entity_id: u64,
    pub entity_name: String,
    pub action: String,
}

/// Devuelve el id de la entidad de botón bajo el punto `(x, y)` (píxeles
/// del espacio del Canvas para una pantalla de `screen_w` × `screen_h`),
/// o `None` si ningún botón lo contiene.
///
/// Criterios, espejo de lo que pinta `build_canvas_batch` y de lo que
/// hace la superposición egui del editor:
/// - Solo entidades activas con `UIButtonData` **interactuable** y
///   `RectTransformData`.
/// - El rectángulo es el de `resolve_ui_rect`: una sola fórmula para
///   pintar y para pulsar.
/// - El punto pertenece al rect si `rx <= x < rx + w` y
///   `ry <= y < ry + h` (borde izquierdo/superior dentro, derecho/
///   inferior fuera: dos botones pegados no comparten el borde).
/// - Si varios botones se solapan, gana el ÚLTIMO en el orden de la
///   escena: es el que se dibuja encima en el lote de UI.
pub fn hit_test_button(
    scene: &SceneState,
    x: f32,
    y: f32,
    screen_w: f32,
    screen_h: f32,
) -> Option<u64> {
    let rects = glass_core::resolve_scene_ui_rects(scene, screen_w, screen_h);
    for ent in scene.entities.iter().rev() {
        if !ent.active {
            continue;
        }
        let (Some(_rt), Some(btn)) = (&ent.rect_transform, &ent.ui_button) else {
            continue;
        };
        if !btn.interactable {
            continue;
        }
        let [rx, ry, w, h] = rects.get(&ent.id).copied().unwrap_or([0.0, 0.0, 0.0, 0.0]);
        if x >= rx && x < rx + w && y >= ry && y < ry + h {
            return Some(ent.id);
        }
    }
    None
}

/// Rastrea el gesto de clic sobre botones del Canvas con la semántica
/// habitual (la de egui `clicked()` y la de `Button` de Unity): hay clic
/// si la pulsación Y la liberación ocurren dentro del MISMO botón.
/// Pulsar dentro y soltar fuera —o al revés— no activa nada, lo que
/// permite «arrepentirse» arrastrando el ratón fuera a mitad de clic.
#[derive(Debug, Default)]
pub struct ButtonClickTracker {
    pressed_button: Option<u64>,
}

impl ButtonClickTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra una pulsación en `(x, y)`: recuerda qué botón quedó bajo
    /// el ratón, si lo hubo.
    pub fn press(&mut self, scene: &SceneState, x: f32, y: f32, screen_w: f32, screen_h: f32) {
        self.pressed_button = hit_test_button(scene, x, y, screen_w, screen_h);
    }

    /// Registra una liberación en `(x, y)`. Si cae sobre el mismo botón
    /// que la pulsación, devuelve el clic completado; en cualquier otro
    /// caso devuelve `None`. En ambos casos el gesto se da por terminado.
    pub fn release(
        &mut self,
        scene: &SceneState,
        x: f32,
        y: f32,
        screen_w: f32,
        screen_h: f32,
    ) -> Option<CanvasButtonClick> {
        let pressed = self.pressed_button.take()?;
        let hit = hit_test_button(scene, x, y, screen_w, screen_h)?;
        if hit != pressed {
            return None;
        }
        let ent = scene.entities.iter().find(|e| e.id == hit)?;
        let btn = ent.ui_button.as_ref()?;
        Some(CanvasButtonClick {
            entity_id: ent.id,
            entity_name: ent.name.clone(),
            action: btn.on_click_action.clone(),
        })
    }

    /// Descarta el gesto en curso (p. ej. al cambiar de escena: el botón
    /// memorizado ya no existe).
    pub fn cancel(&mut self) {
        self.pressed_button = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atlas() -> FontAtlas {
        // El atlas de producción se construye desde la TTF incrustada en
        // glass_renderer; aquí se reconstruye con la misma fuente para
        // probar el constructor de lotes sin GPU.
        FontAtlas::build(include_bytes!("../../../assets/fonts/DejaVuSans.ttf"))
            .expect("atlas de prueba")
    }

    #[test]
    fn texto_del_hud_genera_quads() {
        let mut scene = SceneState::new();
        let id = scene.create_entity(
            "Score HUD Text".into(),
            vec!["Transform".into(), "RectTransform".into(), "Text".into()],
        );
        {
            let e = scene.get_entity_mut(id).unwrap();
            let rt = e.rect_transform.as_mut().unwrap();
            rt.anchored_position = [20.0, 20.0];
            rt.size_delta = [250.0, 40.0];
            rt.anchor_min = [0.0, 0.0];
            rt.anchor_max = [0.0, 0.0];
            rt.pivot = [0.0, 0.0];
            let txt = e.ui_text.as_mut().unwrap();
            txt.text = "Score: 0 / 100 pts".into();
            txt.font_size = 22.0;
        }
        let batch = build_canvas_batch(&scene, &UiFonts { default: &atlas(), named: &std::collections::HashMap::new() }, &UiSceneImages::new(), 1280.0, 720.0)
            .into_iter()
            .next()
            .unwrap()
            .batch;
        assert!(!batch.is_empty(), "el HUD debe producir vértices");
        // 18 caracteres, 4 espacios → 14 glifos visibles × 6 vértices.
        assert_eq!(batch.vertex_count(), 14 * 6);
    }

    #[test]
    fn boton_genera_fondo_y_etiqueta() {
        let mut scene = SceneState::new();
        let id = scene.create_entity(
            "Play Button".into(),
            vec!["Transform".into(), "RectTransform".into(), "Button".into(), "Text".into()],
        );
        {
            let e = scene.get_entity_mut(id).unwrap();
            e.ui_text.as_mut().unwrap().text = "Jugar".into();
        }
        let batch = build_canvas_batch(&scene, &UiFonts { default: &atlas(), named: &std::collections::HashMap::new() }, &UiSceneImages::new(), 1280.0, 720.0)
            .into_iter()
            .next()
            .unwrap()
            .batch;
        // Fondo (1 quad) + «Jugar» (5 glifos).
        assert_eq!(batch.vertex_count(), 6 + 5 * 6);
    }

    #[test]
    fn escena_sin_ui_no_genera_nada() {
        let mut scene = SceneState::new();
        scene.create_entity("Cube".into(), vec!["Transform".into(), "MeshRenderer".into()]);
        let batch = build_canvas_batch(&scene, &UiFonts { default: &atlas(), named: &std::collections::HashMap::new() }, &UiSceneImages::new(), 1280.0, 720.0)
            .into_iter()
            .next()
            .unwrap()
            .batch;
        assert!(batch.is_empty());
    }

    /// Integración: la escena de prueba del Canvas (con texto suelto,
    /// título anclado arriba y botón) genera un lote no vacío.
    #[test]
    fn escena_canvas_ui_test_genera_lote() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/scenes/CanvasUiTest.scene.json"
        );
        let mut scene = glass_core::SceneState::default();
        scene.load_from_json(path).expect("la escena de prueba carga");
        let font = glass_renderer::FontAtlas::build(include_bytes!(
            "../../../assets/fonts/DejaVuSans.ttf"
        ))
        .expect("atlas");
        let batch = build_canvas_batch(&scene, &UiFonts { default: &font, named: &std::collections::HashMap::new() }, &UiSceneImages::new(), 1280.0, 720.0)
            .into_iter()
            .next()
            .unwrap()
            .batch;
        assert!(batch.vertices().len() > 100);
    }

    #[test]
    fn entidad_inactiva_se_omite() {
        let mut scene = SceneState::new();
        let id = scene.create_entity(
            "Hidden Text".into(),
            vec!["Transform".into(), "RectTransform".into(), "Text".into()],
        );
        scene.get_entity_mut(id).unwrap().active = false;
        let batch = build_canvas_batch(&scene, &UiFonts { default: &atlas(), named: &std::collections::HashMap::new() }, &UiSceneImages::new(), 1280.0, 720.0)
            .into_iter()
            .next()
            .unwrap()
            .batch;
        assert!(batch.is_empty());
    }

    /// Crea un botón centrado en la pantalla de `w`×`h` píxeles con el
    /// tamaño dado; en 1280×720 y 220×56 su rect es [530, 332, 220, 56]
    /// y su centro (640, 360). Devuelve el id de la entidad.
    fn boton_en(scene: &mut SceneState, nombre: &str, accion: &str, size: [f32; 2]) -> u64 {
        let id = scene.create_entity(
            nombre.into(),
            vec![
                "Transform".into(),
                "RectTransform".into(),
                "Button".into(),
                "Text".into(),
            ],
        );
        let e = scene.get_entity_mut(id).unwrap();
        let rt = e.rect_transform.as_mut().unwrap();
        rt.anchored_position = [0.0, 0.0];
        rt.size_delta = size;
        rt.anchor_min = [0.5, 0.5];
        rt.anchor_max = [0.5, 0.5];
        rt.pivot = [0.5, 0.5];
        let btn = e.ui_button.as_mut().unwrap();
        btn.interactable = true;
        btn.on_click_action = accion.into();
        id
    }

    #[test]
    fn hit_test_acierta_el_centro_del_boton() {
        let mut scene = SceneState::new();
        let id = boton_en(&mut scene, "Play", "StartGame", [220.0, 56.0]);
        assert_eq!(hit_test_button(&scene, 640.0, 360.0, 1280.0, 720.0), Some(id));
    }

    #[test]
    fn hit_test_falla_fuera_del_boton() {
        let mut scene = SceneState::new();
        boton_en(&mut scene, "Play", "StartGame", [220.0, 56.0]);
        assert_eq!(hit_test_button(&scene, 10.0, 10.0, 1280.0, 720.0), None);
        assert_eq!(hit_test_button(&scene, 640.0, 100.0, 1280.0, 720.0), None);
        assert_eq!(hit_test_button(&scene, 100.0, 360.0, 1280.0, 720.0), None);
    }

    #[test]
    fn hit_test_bordes_izquierdo_y_superior_dentro_derecho_e_inferior_fuera() {
        let mut scene = SceneState::new();
        let id = boton_en(&mut scene, "Play", "StartGame", [220.0, 56.0]);
        // Rect: x ∈ [530, 750), y ∈ [332, 388).
        assert_eq!(hit_test_button(&scene, 530.0, 332.0, 1280.0, 720.0), Some(id));
        assert_eq!(hit_test_button(&scene, 749.9, 387.9, 1280.0, 720.0), Some(id));
        assert_eq!(hit_test_button(&scene, 750.0, 360.0, 1280.0, 720.0), None);
        assert_eq!(hit_test_button(&scene, 640.0, 388.0, 1280.0, 720.0), None);
        assert_eq!(hit_test_button(&scene, 529.9, 360.0, 1280.0, 720.0), None);
    }

    #[test]
    fn hit_test_omite_inactivos_y_no_interactuables() {
        let mut scene = SceneState::new();
        let inactivo = boton_en(&mut scene, "Off", "A", [220.0, 56.0]);
        scene.get_entity_mut(inactivo).unwrap().active = false;
        assert_eq!(hit_test_button(&scene, 640.0, 360.0, 1280.0, 720.0), None);

        let no_inter = boton_en(&mut scene, "Disabled", "B", [220.0, 56.0]);
        scene
            .get_entity_mut(no_inter)
            .unwrap()
            .ui_button
            .as_mut()
            .unwrap()
            .interactable = false;
        assert_eq!(hit_test_button(&scene, 640.0, 360.0, 1280.0, 720.0), None);
    }

    #[test]
    fn hit_test_con_solape_gana_el_ultimo_de_la_escena() {
        let mut scene = SceneState::new();
        boton_en(&mut scene, "Debajo", "A", [220.0, 56.0]);
        let encima = boton_en(&mut scene, "Encima", "B", [220.0, 56.0]);
        // Mismo rect: gana el último creado, que es el que se pinta encima.
        assert_eq!(hit_test_button(&scene, 640.0, 360.0, 1280.0, 720.0), Some(encima));
    }

    #[test]
    fn clic_completo_pulsar_y_soltar_dentro() {
        let mut scene = SceneState::new();
        let id = boton_en(&mut scene, "Play Button", "StartGame", [220.0, 56.0]);
        let mut tracker = ButtonClickTracker::new();
        tracker.press(&scene, 640.0, 360.0, 1280.0, 720.0);
        let clic = tracker.release(&scene, 645.0, 355.0, 1280.0, 720.0);
        assert_eq!(
            clic,
            Some(CanvasButtonClick {
                entity_id: id,
                entity_name: "Play Button".into(),
                action: "StartGame".into(),
            })
        );
        // El gesto terminó: una segunda liberación sin pulsación no repite.
        assert_eq!(tracker.release(&scene, 640.0, 360.0, 1280.0, 720.0), None);
    }

    #[test]
    fn clic_se_cancela_al_soltar_fuera() {
        let mut scene = SceneState::new();
        boton_en(&mut scene, "Play", "StartGame", [220.0, 56.0]);
        let mut tracker = ButtonClickTracker::new();
        tracker.press(&scene, 640.0, 360.0, 1280.0, 720.0);
        assert_eq!(tracker.release(&scene, 20.0, 20.0, 1280.0, 720.0), None);
    }

    #[test]
    fn sin_clic_si_la_pulsacion_fue_fuera() {
        let mut scene = SceneState::new();
        boton_en(&mut scene, "Play", "StartGame", [220.0, 56.0]);
        let mut tracker = ButtonClickTracker::new();
        tracker.press(&scene, 20.0, 20.0, 1280.0, 720.0);
        assert_eq!(tracker.release(&scene, 640.0, 360.0, 1280.0, 720.0), None);
    }

    #[test]
    fn clic_no_salta_de_un_boton_a_otro() {
        let mut scene = SceneState::new();
        // Dos botones solapados: press recuerda el de encima; sin mover el
        // punto, release cae en el mismo → clic válido. Para forzar el
        // cambio, se desactiva el de encima entre press y release.
        boton_en(&mut scene, "Debajo", "A", [220.0, 56.0]);
        let encima = boton_en(&mut scene, "Encima", "B", [220.0, 56.0]);
        let mut tracker = ButtonClickTracker::new();
        tracker.press(&scene, 640.0, 360.0, 1280.0, 720.0);
        scene.get_entity_mut(encima).unwrap().active = false;
        // Al soltar, el hit es «Debajo», distinto del pulsado: no hay clic.
        assert_eq!(tracker.release(&scene, 640.0, 360.0, 1280.0, 720.0), None);
    }

    #[test]
    fn escena_real_canvas_ui_test_el_boton_jugar_recibe_el_clic() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/scenes/CanvasUiTest.scene.json"
        );
        let mut scene = SceneState::default();
        scene.load_from_json(path).expect("la escena de prueba carga");
        // Botón «Start Button Test»: centrado, 220×56 → centro (640, 360).
        let hit = hit_test_button(&scene, 640.0, 360.0, 1280.0, 720.0)
            .expect("el centro de la pantalla cae en el botón JUGAR");
        let ent = scene.entities.iter().find(|e| e.id == hit).unwrap();
        assert_eq!(ent.name, "Start Button Test");
        let mut tracker = ButtonClickTracker::new();
        tracker.press(&scene, 640.0, 360.0, 1280.0, 720.0);
        let clic = tracker.release(&scene, 640.0, 360.0, 1280.0, 720.0).unwrap();
        assert_eq!(clic.action, "StartGame");
    }
}

use std::collections::HashMap;
use std::path::Path;

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

#[cfg(test)]
mod tests_img3 {
    use super::*;
    use glass_core::{EntityData, RectTransformData};

    fn atlas() -> FontAtlas {
        FontAtlas::build(include_bytes!("../../../assets/fonts/DejaVuSans.ttf"))
            .expect("atlas de prueba")
    }

    /// Entidad base activa con rect_transform por defecto.
    fn entidad(nombre: &str) -> EntityData {
        let mut e = EntityData::new(900, nombre.to_string(), Vec::new());
        e.active = true;
        e.rect_transform = Some(RectTransformData::default());
        e
    }

    /// Mapa de imágenes construido a mano para los tests.
    fn imagenes(uv: [f32; 4], pixel_size: (u32, u32)) -> UiSceneImages {
        let mut m = UiSceneImages::new();
        m.insert(
            "sprite.png".to_string(),
            UiImageEntry { uv, pixel_size },
        );
        m
    }

    // (a) imagen conocida + botón después -> [Image, Font]
    #[test]
    fn imagen_y_luego_boton() {
        let mut scene = SceneState::default();

        let mut e_img = entidad("imagen");
        e_img.ui_image = Some(glass_core::UIImageData {
            sprite: "sprite.png".to_string(),
            ..Default::default()
        });
        scene.entities.push(e_img);

        let mut e_btn = entidad("boton");
        e_btn.ui_button = Some(glass_core::UIButtonData::default());
        e_btn.ui_text = Some(glass_core::UITextData {
            text: "OK".to_string(),
            font_size: 16.0,
            ..Default::default()
        });
        scene.entities.push(e_btn);

        let imgs = imagenes([0.0, 0.0, 0.5, 0.5], (200, 100));
        let font = atlas();
        let segs = build_canvas_batch(&scene, &UiFonts { default: &font, named: &std::collections::HashMap::new() }, &imgs, 800.0, 600.0);

        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].atlas, UiAtlasKind::Image);
        assert_eq!(segs[1].atlas, UiAtlasKind::Font);
        // un quad = 2 triángulos = 6 vértices
        assert_eq!(segs[0].batch.vertices().len(), 6);
    }

    // (b) preserve_aspect: 200x100 dentro de un rect 100x100 -> 100x50
    #[test]
    fn preserve_aspect_encaja_y_centra() {
        let mut scene = SceneState::default();

        let mut e = entidad("imagen");
        let rt = RectTransformData {
            size_delta: [100.0, 100.0],
            ..Default::default()
        };
        e.rect_transform = Some(rt);
        e.ui_image = Some(glass_core::UIImageData {
            sprite: "sprite.png".to_string(),
            preserve_aspect: true,
            ..Default::default()
        });
        scene.entities.push(e);

        let imgs = imagenes([0.0, 0.0, 0.5, 0.5], (200, 100));
        let font = atlas();
        let segs = build_canvas_batch(&scene, &UiFonts { default: &font, named: &std::collections::HashMap::new() }, &imgs, 800.0, 600.0);

        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].atlas, UiAtlasKind::Image);

        let v = segs[0].batch.vertices();
        assert!(v.len() >= 3);
        let ancho = v[2].pos[0] - v[0].pos[0];
        let alto = v[1].pos[1] - v[0].pos[1];
        assert!((ancho - 100.0).abs() < 0.01, "ancho = {}", ancho);
        assert!((alto - 50.0).abs() < 0.01, "alto = {}", alto);
    }

    // (c) escena sin UI -> un único segmento Font vacío
    #[test]
    fn sin_ui_devuelve_segmento_vacio() {
        let scene = SceneState::default();
        let imgs = UiSceneImages::new();
        let font = atlas();
        let segs = build_canvas_batch(&scene, &UiFonts { default: &font, named: &std::collections::HashMap::new() }, &imgs, 800.0, 600.0);

        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].atlas, UiAtlasKind::Font);
        assert!(segs[0].batch.vertices().is_empty());
    }
}

// ---------------------------------------------------------------------------
// Drag & Drop del Canvas: sesión de arrastre y hit-tests
// ---------------------------------------------------------------------------

pub struct DragSession {
    pub entity_id: u64,
    /// Nombre de la entidad arrastrada: viaja con la sesión para registro
    /// y depuración; la lógica de arrastre no lo necesita.
    #[allow(dead_code)]
    pub name: String,
    pub begin_action: String,
    pub end_action: String,
    grab_offset: [f32; 2],      // puntero - esquina del rect al pulsar
    start_anchored: [f32; 2],   // anchored_position al pulsar
    start_rect_origin: [f32; 2] // esquina del rect al pulsar
}

pub fn drag_press(
    scene: &SceneState,
    x: f32,
    y: f32,
    screen_w: f32,
    screen_h: f32,
) -> Option<DragSession> {
    let rects = glass_core::resolve_scene_ui_rects(scene, screen_w, screen_h);
    // El último dibujado es el de encima: iteramos en orden inverso.
    for e in scene.entities.iter().rev() {
        if !e.active {
            continue;
        }

        let Some(rt) = e.rect_transform.as_ref() else {
            continue;
        };
        let Some(ui_drag) = e.ui_drag.as_ref() else {
            continue;
        };
        if !ui_drag.draggable {
            continue;
        }

        let [rx, ry, rw, rh] = rects.get(&e.id).copied().unwrap_or([0.0, 0.0, 0.0, 0.0]);
        if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
            return Some(DragSession {
                entity_id: e.id,
                name: e.name.clone(),
                begin_action: ui_drag.on_begin_drag.clone(),
                end_action: ui_drag.on_drag_end.clone(),
                grab_offset: [x - rx, y - ry],
                start_anchored: rt.anchored_position,
                start_rect_origin: [rx, ry],
            });
        }
    }

    None
}

impl DragSession {
    /// Nueva anchored_position para que el rect siga al puntero conservando el agarre.
    pub fn follow(&self, x: f32, y: f32) -> [f32; 2] {
        let origen_deseado = [x - self.grab_offset[0], y - self.grab_offset[1]];
        [
            self.start_anchored[0] + (origen_deseado[0] - self.start_rect_origin[0]),
            self.start_anchored[1] + (origen_deseado[1] - self.start_rect_origin[1]),
        ]
    }

    /// Posición original para restaurar si la suelta no cae en un destino válido.
    pub fn start_anchored(&self) -> [f32; 2] {
        self.start_anchored
    }
}

pub struct DropHit {
    pub entity_id: u64,
    /// Nombre de la casilla destino: viaja con el acierto para registro
    /// y depuración; la lógica de suelta no lo necesita.
    #[allow(dead_code)]
    pub name: String,
    pub anchored: [f32; 2],
    pub drop_action: String,
}

pub fn drag_drop_target(
    scene: &SceneState,
    dragged_id: u64,
    x: f32,
    y: f32,
    screen_w: f32,
    screen_h: f32,
) -> Option<DropHit> {
    let rects = glass_core::resolve_scene_ui_rects(scene, screen_w, screen_h);
    for e in scene.entities.iter().rev() {
        if !e.active || e.id == dragged_id {
            continue;
        }

        let Some(rt) = e.rect_transform.as_ref() else {
            continue;
        };
        let Some(ui_drag) = e.ui_drag.as_ref() else {
            continue;
        };
        if !ui_drag.accepts_drop {
            continue;
        }

        let [rx, ry, rw, rh] = rects.get(&e.id).copied().unwrap_or([0.0, 0.0, 0.0, 0.0]);
        if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
            return Some(DropHit {
                entity_id: e.id,
                name: e.name.clone(),
                anchored: rt.anchored_position,
                drop_action: ui_drag.on_drop.clone(),
            });
        }
    }

    None
}

#[cfg(test)]
mod tests_drag {
    use super::*;
    use glass_core::{EntityData, RectTransformData};

    fn rect_centrado(anchored_position: [f32; 2], size_delta: [f32; 2]) -> RectTransformData {
        RectTransformData {
            anchored_position,
            size_delta,
            anchor_min: [0.5, 0.5],
            anchor_max: [0.5, 0.5],
            pivot: [0.5, 0.5],
        }
    }

    fn ui_drag(
        draggable: bool,
        accepts_drop: bool,
        on_begin_drag: &str,
        on_drag_end: &str,
        on_drop: &str,
    ) -> glass_core::UIDragData {
        glass_core::UIDragData {
            draggable,
            accepts_drop,
            on_begin_drag: on_begin_drag.into(),
            on_drag_end: on_drag_end.into(),
            on_drop: on_drop.into(),
        }
    }

    #[test]
    fn entity_new_deja_active_true_y_ui_none() {
        let e = EntityData::new(7, "nodo".into(), vec![]);
        assert!(e.active);
        assert!(e.ui_drag.is_none());
        assert!(e.rect_transform.is_none());
    }

    #[test]
    fn press_devuelve_el_draggable_y_fuera_da_none() {
        let mut scene = SceneState::default();

        let mut e = EntityData::new(1, "arrastrable".into(), vec![]);
        e.active = true;
        e.rect_transform = Some(rect_centrado([0.0, 0.0], [100.0, 100.0]));
        e.ui_drag = Some(ui_drag(true, false, "begin", "end", ""));
        scene.entities.push(e);

        let sesion = drag_press(&scene, 400.0, 300.0, 800.0, 600.0)
            .expect("debe pulsar el draggable");
        assert_eq!(sesion.entity_id, 1);
        assert_eq!(sesion.name, "arrastrable");
        assert_eq!(sesion.begin_action, "begin");
        assert_eq!(sesion.end_action, "end");
        assert_eq!(sesion.grab_offset, [50.0, 50.0]);
        assert_eq!(sesion.start_anchored, [0.0, 0.0]);
        assert_eq!(sesion.start_rect_origin, [350.0, 250.0]);

        assert!(drag_press(&scene, 10.0, 10.0, 800.0, 600.0).is_none());
    }

    #[test]
    fn follow_conserva_el_agarre() {
        let mut scene = SceneState::default();

        let mut e = EntityData::new(1, "arrastrable".into(), vec![]);
        e.active = true;
        e.rect_transform = Some(rect_centrado([0.0, 0.0], [100.0, 100.0]));
        e.ui_drag = Some(ui_drag(true, false, "begin", "end", ""));
        scene.entities.push(e);

        let sesion = drag_press(&scene, 400.0, 300.0, 800.0, 600.0).unwrap();
        let nueva = sesion.follow(500.0, 380.0);
        assert_eq!(nueva, [100.0, 80.0]);
        assert_eq!(sesion.start_anchored(), [0.0, 0.0]);
    }

    #[test]
    fn drop_target_excluye_al_arrastrado_y_da_none_sin_casilla() {
        let mut scene = SceneState::default();

        let mut destino = EntityData::new(10, "casilla".into(), vec![]);
        destino.active = true;
        destino.rect_transform = Some(rect_centrado([10.0, 10.0], [100.0, 100.0]));
        destino.ui_drag = Some(ui_drag(false, true, "", "", "soltar"));
        scene.entities.push(destino);

        let mut arrastrado = EntityData::new(1, "arrastrado".into(), vec![]);
        arrastrado.active = true;
        arrastrado.rect_transform = Some(rect_centrado([0.0, 0.0], [100.0, 100.0]));
        arrastrado.ui_drag = Some(ui_drag(true, false, "begin", "end", ""));
        scene.entities.push(arrastrado);

        let hit = drag_drop_target(&scene, 1, 400.0, 300.0, 800.0, 600.0)
            .expect("debe encontrar la casilla");
        assert_eq!(hit.entity_id, 10);
        assert_eq!(hit.name, "casilla");
        assert_eq!(hit.anchored, [10.0, 10.0]);
        assert_eq!(hit.drop_action, "soltar");

        scene.entities.retain(|e| e.id != 10);
        assert!(drag_drop_target(&scene, 1, 400.0, 300.0, 800.0, 600.0).is_none());
    }

    #[test]
    fn un_no_draggable_encima_no_bloquea_al_draggable_de_debajo() {
        let mut scene = SceneState::default();

        let mut debajo = EntityData::new(1, "debajo".into(), vec![]);
        debajo.active = true;
        debajo.rect_transform = Some(rect_centrado([0.0, 0.0], [100.0, 100.0]));
        debajo.ui_drag = Some(ui_drag(true, false, "begin", "end", ""));
        scene.entities.push(debajo);

        let mut encima = EntityData::new(2, "encima".into(), vec![]);
        encima.active = true;
        encima.rect_transform = Some(rect_centrado([0.0, 0.0], [100.0, 100.0]));
        encima.ui_drag = Some(ui_drag(false, false, "", "", ""));
        scene.entities.push(encima);

        let sesion = drag_press(&scene, 400.0, 300.0, 800.0, 600.0)
            .expect("debe atravesar el no draggable y devolver el de debajo");
        assert_eq!(sesion.entity_id, 1);
        assert_eq!(sesion.name, "debajo");
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
