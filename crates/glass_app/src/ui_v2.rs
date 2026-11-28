use glass_core::scene::SceneState;
use glass_renderer::sdf_atlas::SdfAtlas;
use glass_renderer::ui::{UiAtlasKind, UiBatch, UiSegment, MODO_BORDE, MODO_COLOR, MODO_IMAGEN, MODO_SOMBRA};
use glass_ui::pintor::{MedidaImagen, ModoPintado, Segmento};


/// Fuentes disponibles para maquetar el Canvas: la incrustada
/// («default», la DejaVu) y las cargadas de assets/fonts por
/// nombre. La app mantiene los atlas de CPU (para medir) y el
/// renderer, los pases GPU con el mismo nombre.
pub struct UiFonts<'a> {
    pub default: &'a mut SdfAtlas,
    pub named: &'a mut std::collections::HashMap<String, SdfAtlas>,
}

impl<'a> UiFonts<'a> {
    /// Atlas y tipo de segmento para un nombre de fuente:
    /// «default», vacío o desconocido → la incrustada.
    /// Devuelve el atlas mutable: maquetar puede insertar glifos
    /// nuevos (V3, presupuesto por frame).
    pub fn resolve(&mut self, name: &str) -> (&mut SdfAtlas, UiAtlasKind) {
        if name.is_empty() || name == "default" {
            return (&mut *self.default, UiAtlasKind::Font);
        }
        match self.named.get_mut(name) {
            Some(atlas) => (atlas, UiAtlasKind::FontNamed(name.to_string())),
            None => (&mut *self.default, UiAtlasKind::Font),
        }
    }
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

/// Entrada del mapa de imágenes de la escena: uv en el atlas y tamaño
/// original en píxeles (necesario para `preserve_aspect`).
#[derive(Clone, Debug)]
pub struct UiImageEntry {
    pub uv: [f32; 4],
    pub pixel_size: (u32, u32),
}

/// Sprites de la escena indexados por nombre de sprite.
pub type UiSceneImages = std::collections::HashMap<String, UiImageEntry>;

/// Recoge los sprites distintos de las entidades activas (en orden de
/// primera aparición), los decodifica desde `assets_root` y los empaqueta
/// en un atlas. Devuelve el mapa de sprites y el atlas (si hubo alguno).
pub fn build_ui_scene_images(
    scene: &SceneState,
    assets_root: &std::path::Path,
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

/// Calcula el clip en coordenadas locales al quad, restando el origen del rect.
fn clip_local(
    seg_clip: &Option<([f32; 4], f32)>,
    rx: f32,
    ry: f32,
) -> Option<([f32; 4], f32)> {
    seg_clip.map(|(c, soft)| ([c[0] - rx, c[1] - ry, c[2], c[3]], soft))
}

/// Convierte UN segmento del pintor V2 en vértices del lote.
/// `uv_imagen`: sprite → uv [u0,v0,u1,v1] en el atlas de imágenes; si
/// devuelve None, el segmento de imagen se omite (sin vértices).
/// Devuelve true si emitió vértices.
pub fn push_segmento(
    lote: &mut UiBatch,
    atlas_fuente: &mut SdfAtlas,
    seg: &Segmento,
    uv_imagen: &dyn Fn(&str) -> Option<[f32; 4]>,
) -> bool {
    // Validar dimensiones del rect
    let rect = [seg.rect.x, seg.rect.y, seg.rect.ancho, seg.rect.alto];
    if rect[2] <= 0.0 || rect[3] <= 0.0 {
        return false;
    }

    // Clip en coordenadas locales al quad
    let cl = clip_local(&seg.clip, rect[0], rect[1]);

    match seg.modo {
        ModoPintado::Color => {
            // UV del texel sólido del atlas de fuentes
            let u = atlas_fuente.solid_uv[0];
            let v = atlas_fuente.solid_uv[1];
            let uv = [u, v, u, v];
            lote.push_quad_extendido(
                rect,
                uv,
                seg.color,
                MODO_COLOR,
                seg.radios,
                cl,
                seg.clip_radios,
                None,       // sin degradado
                0.0,        // sin desenfoque
            );
            true
        }
        ModoPintado::Degradado => {
            // UV del texel sólido (no importa realmente para degradado)
            let u = atlas_fuente.solid_uv[0];
            let v = atlas_fuente.solid_uv[1];
            let uv = [u, v, u, v];
            lote.push_quad_extendido(
                rect,
                uv,
                seg.color,
                MODO_COLOR,
                seg.radios,
                cl,
                seg.clip_radios,
                seg.degradado, // parámetros de degradado
                0.0,
            );
            true
        }
        ModoPintado::Sombra => {
            // UV del texel sólido
            let u = atlas_fuente.solid_uv[0];
            let v = atlas_fuente.solid_uv[1];
            let uv = [u, v, u, v];
            lote.push_quad_extendido(
                rect,
                uv,
                seg.color,
                MODO_SOMBRA,
                seg.radios,
                cl,
                seg.clip_radios,
                None,
                seg.desenfoque,
            );
            true
        }
        ModoPintado::Imagen => {
            // Obtener nombre del sprite; si no hay o está vacío → omitir
            let nombre = match &seg.sprite {
                Some(s) if !s.is_empty() => s.as_str(),
                _ => return false,
            };
            // Buscar UV en el atlas de imágenes
            let uv = match uv_imagen(nombre) {
                Some(uv) => uv,
                None => return false,
            };
            lote.push_quad_extendido(
                rect,
                uv,
                seg.color,       // tint
                MODO_IMAGEN,
                seg.radios,
                cl,
                seg.clip_radios,
                None,
                0.0,
            );
            true
        }
        ModoPintado::Texto => {
            // Obtener datos de texto; si no hay o el texto está vacío → omitir
            // El nombre de fuente viaja en la tupla, pero el atlas lo
            // resuelve el llamador y llega aquí como `atlas_fuente`.
            let (texto, font_size, align, _font, espaciado) = match &seg.texto {
                Some((t, fs, a, f, ls)) if !t.is_empty() => (t.as_str(), *fs, a.as_str(), f.as_str(), *ls),
                _ => return false,
            };
            // Medir ancho del texto para alineación (con letter-spacing).
            let medido = atlas_fuente.measure_con_espaciado(texto, font_size, espaciado);
            // Calcular origen X según alineación
            let ox = if medido <= 0.0 {
                rect[0]
            } else {
                match align {
                    "Center" => rect[0] + (rect[2] - medido) * 0.5,
                    "Right" => rect[0] + rect[2] - medido,
                    _ => rect[0], // "Left" u otro
                }
            };
            // Origen Y = parte superior del rect
            let origen = [ox, rect[1]];
            lote.push_text_extendido(
                atlas_fuente,
                origen,
                texto,
                font_size,
                seg.color,
                seg.radios,
                cl,
                seg.clip_radios,
                espaciado,
            );
            true
        }
        ModoPintado::Borde => {
            // UV del texel sólido; el ancho del borde viaja en desenfoque.
            let u = atlas_fuente.solid_uv[0];
            let v = atlas_fuente.solid_uv[1];
            let uv = [u, v, u, v];
            lote.push_quad_extendido(
                rect,
                uv,
                seg.color,
                MODO_BORDE,
                seg.radios,
                cl,
                seg.clip_radios,
                None,
                seg.desenfoque,
            );
            true
        }
    }
}

/// Adaptador de `MedidaImagen` sobre el mapa de sprites de la escena.
struct MedidaEscena<'a> {
    imagenes: &'a UiSceneImages,
}

impl MedidaImagen for MedidaEscena<'_> {
    fn tamano(&self, sprite: &str) -> Option<(f32, f32)> {
        self.imagenes
            .get(sprite)
            .map(|e| (e.pixel_size.0 as f32, e.pixel_size.1 as f32))
    }
}

/// Devuelve el lote del último segmento si ya es del mismo atlas;
/// si no, crea un segmento nuevo y devuelve su lote.
fn segmento_mut(segs: &mut Vec<UiSegment>, kind: UiAtlasKind) -> &mut UiBatch {
    let mismo = matches!(segs.last(), Some(s) if s.atlas == kind);
    if !mismo {
        segs.push(UiSegment { batch: UiBatch::default(), atlas: kind });
    }
    &mut segs.last_mut().expect("segmento recién creado").batch
}

/// Construye los segmentos de dibujo V2 en orden de pintor, agrupados
/// por atlas. Sin documento (escena sin Canvas) devuelve un vec vacío.
pub fn build_canvas_batch_v2(
    scene: &SceneState,
    fonts: &mut UiFonts<'_>,
    images: &UiSceneImages,
    screen_w: f32,
    screen_h: f32,
) -> Vec<UiSegment> {
    use glass_ui::api::{colocar_documento, Viewport};
    use glass_ui::derivacion::derivar_documentos;
    use glass_ui::pintor::pintar_documento;

    let docs = derivar_documentos(scene);
    let Some(doc) = docs.into_iter().next() else {
        return Vec::new();
    };
    let colocado = colocar_documento(
        &doc,
        Viewport { ancho: screen_w, alto: screen_h },
    );
    let medida = MedidaEscena { imagenes: images };
    let segmentos = pintar_documento(&doc, &colocado, &medida);
    let uv_imagen = |s: &str| images.get(s).map(|e| e.uv);

    let mut segs: Vec<UiSegment> = Vec::new();
    for seg in &segmentos {
        let (atlas, kind) = match seg.modo {
            ModoPintado::Texto => {
                let font = seg.texto.as_ref().map(|t| t.3.as_str()).unwrap_or("default");
                fonts.resolve(font)
            }
            ModoPintado::Imagen => (&mut *fonts.default, UiAtlasKind::Image),
            _ => (&mut *fonts.default, UiAtlasKind::Font),
        };
        let lote = segmento_mut(&mut segs, kind);
        push_segmento(lote, atlas, seg, &uv_imagen);
    }
    segs
}

#[cfg(test)]
mod tests {
    use super::*;
    use glass_ui::api::RectColocado;
    use glass_ui::pintor::{ModoPintado, Segmento};
    use glass_renderer::sdf_atlas::SdfAtlas;
    use glass_renderer::ui::{UiBatch, MODO_COLOR, MODO_IMAGEN, MODO_SOMBRA, MODO_TEXTO};

    /// Construye el atlas de fuentes real con DejaVuSans.
    fn atlas() -> SdfAtlas {
        SdfAtlas::build(include_bytes!("../../../assets/fonts/DejaVuSans.ttf"))
            .expect("No se pudo construir SdfAtlas con DejaVuSans.ttf")
    }

    /// Función de búsqueda de UV de prueba.
    fn uv_prueba(nombre: &str) -> Option<[f32; 4]> {
        if nombre == "textures/cuadro.png" {
            Some([0.0, 0.0, 0.5, 0.5])
        } else {
            None
        }
    }

    /// Helper: crea un Segmento de tipo Color con valores por defecto.
    fn seg_color(rect: [f32; 4], color: [f32; 4]) -> Segmento {
        Segmento {
            entidad: 0,
            modo: ModoPintado::Color,
            rect: RectColocado { x: rect[0], y: rect[1], ancho: rect[2], alto: rect[3] },
            color,
            degradado: None,
            radios: [0.0; 4],
            clip: None,
            clip_radios: [0.0; 4],
            texto: None,
            sprite: None,
            capa_superior: false,
            orden: 0,
            desenfoque: 0.0,
        }
    }

    /// Helper: crea un Segmento de tipo Texto.
    fn seg_texto(
        rect: [f32; 4], color: [f32; 4], texto: &str, font_size: f32, align: &str,
    ) -> Segmento {
        Segmento {
            entidad: 0,
            modo: ModoPintado::Texto,
            rect: RectColocado { x: rect[0], y: rect[1], ancho: rect[2], alto: rect[3] },
            color,
            degradado: None,
            radios: [0.0; 4],
            clip: None,
            clip_radios: [0.0; 4],
            texto: Some((texto.to_string(), font_size, align.to_string(), "default".to_string(), 0.0)),
            sprite: None,
            capa_superior: false,
            orden: 0,
            desenfoque: 0.0,
        }
    }

    #[test]
    fn segmento_color_emite_seis_vertices_con_modo() {
        let mut at = atlas();
        let mut lote = UiBatch::new();
        let seg = seg_color([10.0, 20.0, 100.0, 40.0], [0.5, 0.0, 0.0, 0.5]);
        let ok = push_segmento(&mut lote, &mut at, &seg, &uv_prueba);
        assert!(ok);
        assert_eq!(lote.vertex_count(), 6);
        let verts = lote.vertices();
        for v in verts {
            assert_eq!(v.pack1[1], MODO_COLOR, "modo debe ser MODO_COLOR");
            assert_eq!(v.color, [0.5, 0.0, 0.0, 0.5]);
        }
        // Primera esquina (sup-izq)
        assert_eq!(verts[0].pos[0], 10.0);
        assert_eq!(verts[0].pos[1], 20.0);
        assert_eq!(verts[0].local[0], 0.0);
        assert_eq!(verts[0].local[1], 0.0);
        assert_eq!(verts[0].tamano[0], 100.0);
        assert_eq!(verts[0].tamano[1], 40.0);
    }

    #[test]
    fn segmento_degradado_tres_paradas_emite_doce() {
        let mut at = atlas();
        let mut lote = UiBatch::new();
        // Primera parada: t0=0, t1=0.5, ángulo 90
        let mut s1 = seg_color([10.0, 20.0, 100.0, 40.0], [1.0, 1.0, 1.0, 1.0]);
        s1.modo = ModoPintado::Degradado;
        s1.degradado = Some((90.0, 0.0, 0.5, [1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]));
        assert!(push_segmento(&mut lote, &mut at, &s1, &uv_prueba));
        // Segunda parada: t0=0.5, t1=1.0, ángulo 90
        let mut s2 = seg_color([10.0, 20.0, 100.0, 40.0], [1.0, 1.0, 1.0, 1.0]);
        s2.modo = ModoPintado::Degradado;
        s2.degradado = Some((90.0, 0.5, 1.0, [0.0, 1.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]));
        assert!(push_segmento(&mut lote, &mut at, &s2, &uv_prueba));
        assert_eq!(lote.vertex_count(), 12);
        let verts = lote.vertices();
        // Primeros 6: ángulo=90, t0=0, t1=0.5
        for v in &verts[..6] {
            assert_eq!(v.pack1[2], 90.0, "ángulo debe ser 90");
            assert_eq!(v.pack1[3], 0.0, "t0 debe ser 0.0");
            assert_eq!(v.pack2[0], 0.5, "t1 debe ser 0.5");
        }
        // Últimos 6: ángulo=90, t0=0.5, t1=1.0
        for v in &verts[6..12] {
            assert_eq!(v.pack1[2], 90.0, "ángulo debe ser 90");
            assert_eq!(v.pack1[3], 0.5, "t0 debe ser 0.5");
            assert_eq!(v.pack2[0], 1.0, "t1 debe ser 1.0");
        }
    }

    #[test]
    fn segmento_texto_emite_por_glifo_y_centra() {
        let mut at = atlas();
        // Verificar que el atlas contiene los glifos A y B
        assert!(at.glifo('A').size[0] > 0.0, "El atlas no contiene el glifo 'A'");
        assert!(at.glifo('B').size[0] > 0.0, "El atlas no contiene el glifo 'B'");
        let mut lote = UiBatch::new();
        let seg = seg_texto([0.0, 0.0, 200.0, 40.0], [1.0, 1.0, 1.0, 1.0], "AB", 32.0, "Center");
        let ok = push_segmento(&mut lote, &mut at, &seg, &uv_prueba);
        assert!(ok);
        // 2 glifos × 6 vértices = 12
        assert_eq!(lote.vertex_count(), 12, "Se esperan 12 vértices para 2 glifos");
        let verts = lote.vertices();
        for v in verts {
            assert_eq!(v.pack1[1], MODO_TEXTO, "modo debe ser MODO_TEXTO");
        }
        // El texto está centrado: el primer vértice no empieza en x=0
        assert!(verts[0].pos[0] > 0.0, "El texto centrado no debe empezar en x=0");
    }

    #[test]
    fn segmento_imagen_sin_uv_no_emite() {
        let mut at = atlas();
        // Sprite inexistente → false, 0 vértices
        let mut lote = UiBatch::new();
        let mut seg = seg_color([0.0, 0.0, 64.0, 64.0], [1.0, 1.0, 1.0, 1.0]);
        seg.modo = ModoPintado::Imagen;
        seg.sprite = Some("textures/inexistente.png".to_string());
        assert!(!push_segmento(&mut lote, &mut at, &seg, &uv_prueba));
        assert_eq!(lote.vertex_count(), 0);
        // Sprite existente → true, 6 vértices
        let mut lote2 = UiBatch::new();
        seg.sprite = Some("textures/cuadro.png".to_string());
        assert!(push_segmento(&mut lote2, &mut at, &seg, &uv_prueba));
        assert_eq!(lote2.vertex_count(), 6);
        let verts = lote2.vertices();
        for v in verts {
            assert_eq!(v.pack1[1], MODO_IMAGEN, "modo debe ser MODO_IMAGEN");
        }
    }

    #[test]
    fn segmento_clip_se_convierte_a_local() {
        let mut at = atlas();
        let mut lote = UiBatch::new();
        let mut seg = seg_color([10.0, 20.0, 100.0, 40.0], [1.0, 0.0, 0.0, 1.0]);
        seg.clip = Some(([15.0, 25.0, 50.0, 20.0], 2.0));
        assert!(push_segmento(&mut lote, &mut at, &seg, &uv_prueba));
        assert_eq!(lote.vertex_count(), 6);
        let verts = lote.vertices();
        for v in verts {
            // clip local = [15-10, 25-20, 50, 20] = [5, 5, 50, 20]
            assert_eq!(v.clip[0], 5.0, "clip_x local");
            assert_eq!(v.clip[1], 5.0, "clip_y local");
            assert_eq!(v.clip[2], 50.0, "clip_ancho");
            assert_eq!(v.clip[3], 20.0, "clip_alto");
            // softness en pack1[0]
            assert_eq!(v.pack1[0], 2.0, "softness del clip");
        }
    }

    #[test]
    fn segmento_sombra_lleva_desenfoque() {
        let mut at = atlas();
        let mut lote = UiBatch::new();
        let mut seg = seg_color([0.0, 0.0, 80.0, 80.0], [0.0, 0.0, 0.0, 0.5]);
        seg.modo = ModoPintado::Sombra;
        seg.desenfoque = 8.0;
        assert!(push_segmento(&mut lote, &mut at, &seg, &uv_prueba));
        assert_eq!(lote.vertex_count(), 6);
        let verts = lote.vertices();
        for v in verts {
            assert_eq!(v.pack1[1], MODO_SOMBRA, "modo debe ser MODO_SOMBRA");
            assert_eq!(v.pack2[1], 8.0, "desenfoque debe ser 8.0");
        }
    }

    // --- build_canvas_batch_v2 (V2): port de los tests del lote viejo ---

    fn canvas_raiz() -> glass_core::EntityData {
        use glass_core::{CanvasData, EntityData, RectTransformData};
        let mut canvas = EntityData::new(1, "Canvas".to_string(), Vec::new());
        canvas.children = vec![2, 3];
        canvas.active = true;
        canvas.canvas = Some(CanvasData {
            render_mode: "ScreenSpaceOverlay".to_string(),
            reference_resolution: [800.0, 600.0],
            match_width_or_height: 0.5,
        });
        canvas.rect_transform = Some(RectTransformData {
            anchored_position: [0.0, 0.0],
            size_delta: [0.0, 0.0],
            anchor_min: [0.0, 0.0],
            anchor_max: [1.0, 1.0],
            pivot: [0.5, 0.5],
        });
        canvas
    }

    fn imagenes(uv: [f32; 4], pixel_size: (u32, u32)) -> UiSceneImages {
        let mut m = UiSceneImages::new();
        m.insert(
            "sprite.png".to_string(),
            UiImageEntry { uv, pixel_size },
        );
        m
    }

    #[test]
    fn lote_v2_imagen_y_luego_boton() {
        use glass_core::{EntityData, RectTransformData};
        let mut scene = SceneState::default();
        let mut e_img = EntityData::new(2, "imagen".to_string(), Vec::new());
        e_img.active = true;
        e_img.parent = Some(1);
        e_img.rect_transform = Some(RectTransformData {
            anchored_position: [0.0, 0.0],
            size_delta: [200.0, 100.0],
            anchor_min: [0.5, 0.5],
            anchor_max: [0.5, 0.5],
            pivot: [0.5, 0.5],
        });
        e_img.ui_image = Some(glass_core::UIImageData {
            sprite: "sprite.png".to_string(),
            ..Default::default()
        });
        let mut e_btn = EntityData::new(3, "boton".to_string(), Vec::new());
        e_btn.active = true;
        e_btn.parent = Some(1);
        e_btn.rect_transform = Some(RectTransformData {
            anchored_position: [0.0, 0.0],
            size_delta: [220.0, 56.0],
            anchor_min: [0.5, 0.5],
            anchor_max: [0.5, 0.5],
            pivot: [0.5, 0.5],
        });
        e_btn.ui_button = Some(glass_core::UIButtonData::default());
        e_btn.ui_text = Some(glass_core::UITextData {
            text: "OK".to_string(),
            font_size: 16.0,
            ..Default::default()
        });
        scene.replace_entities(vec![canvas_raiz(), e_img, e_btn]);

        let imgs = imagenes([0.0, 0.0, 0.5, 0.5], (200, 100));
        let mut font = atlas();
        let mut fonts = UiFonts { default: &mut font, named: &mut std::collections::HashMap::new() };
        let segs = build_canvas_batch_v2(&scene, &mut fonts, &imgs, 800.0, 600.0);
        // La cascada V2 pinta el fondo del componente antes que su
        // contenido: fondo blanco de la imagen, la imagen, fondo azul
        // del botón + su texto (mismo atlas Font, un solo segmento).
        assert_eq!(segs.len(), 3, "fondo imagen + imagen + fondo botón y texto");
        assert_eq!(segs[0].atlas, UiAtlasKind::Font);
        assert_eq!(segs[1].atlas, UiAtlasKind::Image);
        assert_eq!(segs[2].atlas, UiAtlasKind::Font);
        assert_eq!(segs[0].batch.vertex_count(), 6);
        assert_eq!(segs[1].batch.vertex_count(), 6);
        assert_eq!(segs[2].batch.vertex_count(), 18);
    }


    #[test]
    fn lote_v2_fuente_desconocida_cae_al_atlas_por_defecto() {
        use glass_core::{EntityData, RectTransformData};
        let mut scene = SceneState::default();
        let mut canvas = canvas_raiz();
        canvas.children = vec![2];
        let mut e_txt = EntityData::new(2, "texto".to_string(), Vec::new());
        e_txt.active = true;
        e_txt.parent = Some(1);
        e_txt.rect_transform = Some(RectTransformData {
            anchored_position: [0.0, 0.0],
            size_delta: [220.0, 56.0],
            anchor_min: [0.5, 0.5],
            anchor_max: [0.5, 0.5],
            pivot: [0.5, 0.5],
        });
        e_txt.ui_text = Some(glass_core::UITextData {
            text: "A\u{20ac}\u{e000}".to_string(),
            font_size: 16.0,
            font: "fuente_que_no_existe".to_string(),
            ..Default::default()
        });
        scene.replace_entities(vec![canvas, e_txt]);

        let imgs = UiSceneImages::new();
        let mut font = atlas();
        let mut fonts = UiFonts { default: &mut font, named: &mut std::collections::HashMap::new() };
        let segs = build_canvas_batch_v2(&scene, &mut fonts, &imgs, 800.0, 600.0);
        // Cadena de reserva: fuente de proyecto ausente -> atlas por
        // defecto; '€' se inserta dinámicamente; U+E000 no tiene
        // contorno y se sustituye por '?'. El lote es un solo segmento
        // de texto (3 glifos x 6 vértices) en el atlas Font.
        assert_eq!(segs.len(), 1, "solo el segmento de texto");
        assert_eq!(segs[0].atlas, UiAtlasKind::Font);
        assert_eq!(segs[0].batch.vertex_count(), 18);
    }

    #[test]
    fn lote_v2_sin_canvas_devuelve_vacio() {
        let scene = SceneState::default();
        let imgs = UiSceneImages::new();
        let mut font = atlas();
        let mut fonts = UiFonts { default: &mut font, named: &mut std::collections::HashMap::new() };
        let segs = build_canvas_batch_v2(&scene, &mut fonts, &imgs, 800.0, 600.0);
        assert!(segs.is_empty());
    }
}

#[cfg(test)]
mod tests_texto_v2 {
    use super::*;
    use glass_core::scene::parsear_escena;

    #[test]
    fn texto_v2_uvs_son_glifo_no_solido() {
        let json = std::fs::read_to_string("../../assets/scenes/CanvasUiTest.scene.json")
            .expect("leer escena");
        let (_v, entities) = parsear_escena(&json).expect("parsear");
        let mut scene = SceneState::default();
        scene.replace_entities(entities);
        let docs = glass_ui::derivacion::derivar_documentos(&scene);
        let doc = &docs[0];
        let colocado = glass_ui::api::colocar_documento(
            doc,
            glass_ui::api::Viewport { ancho: 1280.0, alto: 720.0 },
        );
        let mut atlas = SdfAtlas::build(include_bytes!("../../../assets/fonts/DejaVuSans.ttf"))
            .expect("atlas");
        let segs = glass_ui::pintor::pintar_documento(doc, &colocado, &glass_ui::pintor::MedidaImagenNula);
        let mut lote = UiBatch::new();
        for seg in &segs {
            if seg.modo == ModoPintado::Texto {
                push_segmento(&mut lote, &mut atlas, seg, &|_| None);
            }
        }
        assert!(!lote.vertices().is_empty(), "sin vértices de texto");
        let sol = atlas.solid_uv;
        let mut n_solido = 0usize;
        for v in lote.vertices() {
            // UV del texel sólido = (sol[0], sol[1]) en las 4 esquinas.
            if (v.uv[0] - sol[0]).abs() < 1e-6 && (v.uv[1] - sol[1]).abs() < 1e-6 {
                n_solido += 1;
            }
        }
        eprintln!(
            "vértices texto: {}, con UV sólido: {}, primer UV: {:?}",
            lote.vertices().len(),
            n_solido,
            lote.vertices().first().map(|v| v.uv)
        );
        assert_eq!(n_solido, 0, "vértices de texto con UV del texel sólido");
    }
}

#[cfg(test)]
mod tests_drag_escena {
    use super::*;
    use glass_core::scene::parsear_escena;

    /// Escena de arrastre como fixture dentro del repo (la misma
    /// escena física de P4c). Se incrusta en compilación con
    /// `include_str!`: sin rutas absolutas de la VM ni dependencia
    /// del directorio de trabajo — el CI de GitHub la tiene porque
    /// viaja en el repo (el test anterior leía una ruta absoluta de
    /// /home/hatch y era la causa de los exit 101 de P4c/P4d/f32639f).
    const ESCENA_ARRASTRE: &str =
        include_str!("../tests/fixtures/EscenaArrastre.scene.json");

    fn cerca(a: [f32; 4], b: [f32; 4]) {
        for i in 0..4 {
            assert!((a[i] - b[i]).abs() < 0.5, "rect {:?} != esperado {:?}", a, b);
        }
    }

    #[test]
    fn drag_escena_rects_paso_a_paso() {
        let (_v, entities) = parsear_escena(ESCENA_ARRASTRE).expect("parsear");
        let mut scene = SceneState::default();
        scene.replace_entities(entities);
        let vp = glass_ui::api::Viewport { ancho: 1280.0, alto: 720.0 };
        let colocar = |scene: &SceneState| {
            let docs = glass_ui::derivacion::derivar_documentos(scene);
            let doc = docs.into_iter().next().expect("doc");
            let col = glass_ui::api::colocar_documento(&doc, vp);
            let panel = col.nodos.iter().zip(doc.nodos.iter()).find(|(_, n)| n.entidad == 20).unwrap();
            let rect = panel.0.rect;
            (doc, col, rect)
        };
        let (doc, col, r0) = colocar(&scene);
        eprintln!("antes: panel rect={:?}", r0);
        // 120×120 con factor 2/3 (referencia 1920×1080 a 1280×720):
        // centro (506.67, 360), rect [466.67, 320, 80, 80].
        cerca([r0.x, r0.y, r0.ancho, r0.alto], [466.67, 320.0, 80.0, 80.0]);
        let ses = glass_ui::api::arrastre_pulsar(&doc, &col, 507.0, 360.0).expect("pulsar");
        drop(doc);
        drop(col);
        eprintln!("sesión: entidad={} anclado_inicial={:?} agarre={:?} origen={:?}", ses.entidad, ses.anclado_inicial, ses.agarre, ses.origen_inicial);
        // follow hasta el centro de la casilla, como el arrastre físico
        let nueva = ses.follow(773.0, 360.0);
        eprintln!("follow(773,360)={:?}", nueva);
        assert!((nueva[0] - 66.0).abs() < 0.5 && (nueva[1] - 0.0).abs() < 0.5,
            "follow(773,360) = {:?}, esperado [66, 0]", nueva);
        if let Some(ent) = scene.entities.iter_mut().find(|e| e.id == 20) {
            ent.rect_transform.as_mut().unwrap().anchored_position = nueva;
        }
        let (doc2, col2, r1) = colocar(&scene);
        eprintln!("tras follow: panel rect={:?}", r1);
        cerca([r1.x, r1.y, r1.ancho, r1.alto], [644.0, 320.0, 80.0, 80.0]);
        let destino = glass_ui::api::arrastre_soltar(&doc2, &col2, 20, 773.0, 360.0).expect("destino");
        eprintln!("destino: entidad={} anclado={:?} acción={}", destino.entidad, destino.anclado, destino.accion_soltar);
        assert_eq!(destino.entidad, 21, "el destino debe ser la casilla");
        assert_eq!(destino.accion_soltar, "SoltarEnCasilla");
        assert!((destino.anclado[0] - 200.0).abs() < 0.5 && (destino.anclado[1] - 0.0).abs() < 0.5,
            "anclado destino = {:?}, esperado [200, 0]", destino.anclado);
        if let Some(ent) = scene.entities.iter_mut().find(|e| e.id == 20) {
            ent.rect_transform.as_mut().unwrap().anchored_position = destino.anclado;
        }
        let (_doc3, _col3, r2) = colocar(&scene);
        eprintln!("tras soltar: panel rect={:?}", r2);
        cerca([r2.x, r2.y, r2.ancho, r2.alto], [733.33, 320.0, 80.0, 80.0]);
        assert!(r2.ancho < 100.0, "el panel no debe estirarse: {:?}", r2);
    }
}
