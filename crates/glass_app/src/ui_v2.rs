// P4 conectará este conversor a la ruta de render (hoy solo lo usan los
// tests); hasta entonces, el dead_code es esperado, no un error.
#![allow(dead_code)]

use glass_renderer::ui::{FontAtlas, UiBatch, MODO_COLOR, MODO_IMAGEN, MODO_SOMBRA};
use glass_ui::pintor::{ModoPintado, Segmento};

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
    atlas_fuente: &FontAtlas,
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
                None,
                0.0,
            );
            true
        }
        ModoPintado::Texto => {
            // Obtener datos de texto; si no hay o el texto está vacío → omitir
            let (texto, font_size, align) = match &seg.texto {
                Some((t, fs, a)) if !t.is_empty() => (t.as_str(), *fs, a.as_str()),
                _ => return false,
            };
            // Medir ancho del texto para alineación
            let medido = atlas_fuente.measure(texto, font_size);
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
            );
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glass_ui::api::RectColocado;
    use glass_ui::pintor::{ModoPintado, Segmento};
    use glass_renderer::ui::{FontAtlas, UiBatch, MODO_COLOR, MODO_IMAGEN, MODO_SOMBRA, MODO_TEXTO};

    /// Construye el atlas de fuentes real con DejaVuSans.
    fn atlas() -> FontAtlas {
        FontAtlas::build(include_bytes!("../../../assets/fonts/DejaVuSans.ttf"))
            .expect("No se pudo construir FontAtlas con DejaVuSans.ttf")
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
            texto: Some((texto.to_string(), font_size, align.to_string())),
            sprite: None,
            capa_superior: false,
            orden: 0,
            desenfoque: 0.0,
        }
    }

    #[test]
    fn segmento_color_emite_seis_vertices_con_modo() {
        let at = atlas();
        let mut lote = UiBatch::new();
        let seg = seg_color([10.0, 20.0, 100.0, 40.0], [0.5, 0.0, 0.0, 0.5]);
        let ok = push_segmento(&mut lote, &at, &seg, &uv_prueba);
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
        let at = atlas();
        let mut lote = UiBatch::new();
        // Primera parada: t0=0, t1=0.5, ángulo 90
        let mut s1 = seg_color([10.0, 20.0, 100.0, 40.0], [1.0, 1.0, 1.0, 1.0]);
        s1.modo = ModoPintado::Degradado;
        s1.degradado = Some((90.0, 0.0, 0.5, [1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]));
        assert!(push_segmento(&mut lote, &at, &s1, &uv_prueba));
        // Segunda parada: t0=0.5, t1=1.0, ángulo 90
        let mut s2 = seg_color([10.0, 20.0, 100.0, 40.0], [1.0, 1.0, 1.0, 1.0]);
        s2.modo = ModoPintado::Degradado;
        s2.degradado = Some((90.0, 0.5, 1.0, [0.0, 1.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]));
        assert!(push_segmento(&mut lote, &at, &s2, &uv_prueba));
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
        let at = atlas();
        // Verificar que el atlas contiene los glifos A y B
        assert!(at.glyph('A').is_some(), "El atlas no contiene el glifo 'A'");
        assert!(at.glyph('B').is_some(), "El atlas no contiene el glifo 'B'");
        let mut lote = UiBatch::new();
        let seg = seg_texto([0.0, 0.0, 200.0, 40.0], [1.0, 1.0, 1.0, 1.0], "AB", 32.0, "Center");
        let ok = push_segmento(&mut lote, &at, &seg, &uv_prueba);
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
        let at = atlas();
        // Sprite inexistente → false, 0 vértices
        let mut lote = UiBatch::new();
        let mut seg = seg_color([0.0, 0.0, 64.0, 64.0], [1.0, 1.0, 1.0, 1.0]);
        seg.modo = ModoPintado::Imagen;
        seg.sprite = Some("textures/inexistente.png".to_string());
        assert!(!push_segmento(&mut lote, &at, &seg, &uv_prueba));
        assert_eq!(lote.vertex_count(), 0);
        // Sprite existente → true, 6 vértices
        let mut lote2 = UiBatch::new();
        seg.sprite = Some("textures/cuadro.png".to_string());
        assert!(push_segmento(&mut lote2, &at, &seg, &uv_prueba));
        assert_eq!(lote2.vertex_count(), 6);
        let verts = lote2.vertices();
        for v in verts {
            assert_eq!(v.pack1[1], MODO_IMAGEN, "modo debe ser MODO_IMAGEN");
        }
    }

    #[test]
    fn segmento_clip_se_convierte_a_local() {
        let at = atlas();
        let mut lote = UiBatch::new();
        let mut seg = seg_color([10.0, 20.0, 100.0, 40.0], [1.0, 0.0, 0.0, 1.0]);
        seg.clip = Some(([15.0, 25.0, 50.0, 20.0], 2.0));
        assert!(push_segmento(&mut lote, &at, &seg, &uv_prueba));
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
        let at = atlas();
        let mut lote = UiBatch::new();
        let mut seg = seg_color([0.0, 0.0, 80.0, 80.0], [0.0, 0.0, 0.0, 0.5]);
        seg.modo = ModoPintado::Sombra;
        seg.desenfoque = 8.0;
        assert!(push_segmento(&mut lote, &at, &seg, &uv_prueba));
        assert_eq!(lote.vertex_count(), 6);
        let verts = lote.vertices();
        for v in verts {
            assert_eq!(v.pack1[1], MODO_SOMBRA, "modo debe ser MODO_SOMBRA");
            assert_eq!(v.pack2[1], 8.0, "desenfoque debe ser 8.0");
        }
    }
}
