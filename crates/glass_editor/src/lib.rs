//! glass_editor: Unity-style docking editor layout powered by egui.

pub mod docking;
pub mod launcher;
pub mod panels;

pub use docking::EditorDockingLayout;
pub use launcher::{AppMode, LauncherAction, LauncherPanel, ProjectTemplate, SampleScenePreset};
pub use panels::{GameResolution, PlayState, ViewportTab};

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

    ctx.set_fonts(fonts);
}
