//! glass_launcher: infraestructura compartida del lanzador —
//! instalaciones y actualizaciones (`updates`), registro de
//! complementos (`plugins`), ayudantes de archivos de proyecto
//! (`archivos`) y las fuentes incrustadas de la interfaz. La usan el
//! Hub (`glass_hub`, su dueño) y el editor (sellos de versión,
//! portón de complementos y datos del lanzador).

pub mod archivos;
pub mod tipos;
pub mod plugins;
pub mod dependencias;
pub mod updates;

use egui::{FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

pub fn setup_editor_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    // 1. Intentar cargar DejaVu Sans (cobertura Unicode universal en Linux)
    let dejavu_paths = [
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    for p in dejavu_paths {
        if let Ok(bytes) = std::fs::read(p) {
            fonts.font_data.insert(
                "dejavu_sans".to_owned(),
                Arc::new(FontData::from_owned(bytes)),
            );
            fonts.families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "dejavu_sans".to_owned());
            break;
        }
    }

    // 2. Intentar cargar Noto Sans Symbols y Symbols2 si están presentes
    let symbol_paths = [
        "/usr/share/fonts/noto/NotoSansSymbols-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
    ];
    for (i, sp) in symbol_paths.iter().enumerate() {
        if let Ok(bytes) = std::fs::read(sp) {
            let key = format!("noto_symbols_{}", i);
            fonts.font_data.insert(
                key.clone(),
                Arc::new(FontData::from_owned(bytes)),
            );
            fonts.families
                .entry(FontFamily::Proportional)
                .or_default()
                .push(key.clone());
            fonts.families
                .entry(FontFamily::Monospace)
                .or_default()
                .push(key);
        }
    }

    // 3. Intentar cargar Meslo Nerd Font si existe (cobertura completa de iconos)
    let nerd_paths = [
        "/usr/share/fonts/TTF/MesloLGLDZNerdFont-Regular.ttf",
        "/usr/share/fonts/TTF/MesloLGLNerdFont-Regular.ttf",
    ];
    for np in nerd_paths {
        if let Ok(bytes) = std::fs::read(np) {
            fonts.font_data.insert(
                "nerd_icons".to_owned(),
                Arc::new(FontData::from_owned(bytes)),
            );
            fonts.families
                .entry(FontFamily::Proportional)
                .or_default()
                .push("nerd_icons".to_owned());
            fonts.families
                .entry(FontFamily::Monospace)
                .or_default()
                .push("nerd_icons".to_owned());
            break;
        }
    }

    // 4. Fuentes INCRUSTADAS en el binario: la interfaz usa símbolos y
    // emoji (📁 🎬 ⬆ ♻ ▶…) que las fuentes del sistema no siempre
    // traen — en Windows salían como cuadrados vacíos. Noto Emoji
    // (monocroma, rasterizable por egui) y Noto Sans Symbols 2 viajan
    // con el editor y cierran la cadena de respaldo en cualquier
    // plataforma. Licencia OFL 1.1 en assets/fonts/OFL.txt.
    fonts.font_data.insert(
        "noto_emoji_incrustada".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/NotoEmoji.ttf"
        ))),
    );
    fonts.font_data.insert(
        "noto_symbols2_incrustada".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansSymbols2-Regular.ttf"
        ))),
    );
    for familia in [FontFamily::Proportional, FontFamily::Monospace] {
        let lista = fonts.families.entry(familia).or_default();
        lista.push("noto_symbols2_incrustada".to_owned());
        lista.push("noto_emoji_incrustada".to_owned());
    }

    ctx.set_fonts(fonts);
}
