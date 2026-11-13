//! glass_editor: Unity-style docking editor layout powered by egui.

pub mod docking;
pub mod launcher;
pub mod panels;
pub mod undo;

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

/// Visuales del editor estilo «Unity Pro» (skin del bloque
/// «UI v2 — skin de editor»): grises oscuros de Unity, azul
/// de selección y esquinas casi rectas. Lo aplica el docking
/// en cada frame en lugar del `Visuals::dark()` genérico.
pub fn unity_visuals() -> egui::Visuals {
    use egui::epaint::{CornerRadius, Shadow};
    use egui::style::{Selection, WidgetVisuals};
    use egui::{Color32, Stroke};

    let mut v = egui::Visuals::dark();

    // Paleta general «Unity Pro»
    v.panel_fill = Color32::from_rgb(56, 56, 56);
    v.window_fill = Color32::from_rgb(45, 45, 45);
    v.extreme_bg_color = Color32::from_rgb(35, 35, 35);
    v.faint_bg_color = Color32::from_rgb(48, 48, 48);
    v.code_bg_color = Color32::from_rgb(40, 40, 40);
    v.override_text_color = Some(Color32::from_rgb(217, 217, 217));

    // Selección azul de Unity
    v.selection = Selection {
        bg_fill: Color32::from_rgb(44, 93, 147),
        stroke: Stroke::new(1.0, Color32::from_rgb(217, 217, 217)),
    };

    v.hyperlink_color = Color32::from_rgb(76, 158, 235);

    // Esquinas casi rectas
    v.window_corner_radius = CornerRadius::same(4);
    v.menu_corner_radius = CornerRadius::same(3);

    // Sombras de ventanas y popups
    v.window_shadow = Shadow {
        offset: [0, 2],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(96),
    };
    v.popup_shadow = Shadow {
        offset: [0, 2],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(96),
    };

    // Widgets
    v.widgets.noninteractive = WidgetVisuals {
        bg_fill: Color32::from_rgb(56, 56, 56),
        weak_bg_fill: Color32::from_rgb(56, 56, 56),
        bg_stroke: Stroke::new(1.0, Color32::from_rgb(31, 31, 31)),
        corner_radius: CornerRadius::same(2),
        fg_stroke: Stroke::new(1.0, Color32::from_rgb(204, 204, 204)),
        expansion: 0.0,
    };

    v.widgets.inactive = WidgetVisuals {
        bg_fill: Color32::from_rgb(70, 70, 70),
        weak_bg_fill: Color32::from_rgb(63, 63, 63),
        bg_stroke: Stroke::new(1.0, Color32::from_rgb(31, 31, 31)),
        corner_radius: CornerRadius::same(2),
        fg_stroke: Stroke::new(1.0, Color32::from_rgb(217, 217, 217)),
        expansion: 0.0,
    };

    v.widgets.hovered = WidgetVisuals {
        bg_fill: Color32::from_rgb(80, 80, 80),
        weak_bg_fill: Color32::from_rgb(74, 74, 74),
        bg_stroke: Stroke::new(1.0, Color32::from_rgb(20, 20, 20)),
        corner_radius: CornerRadius::same(2),
        fg_stroke: Stroke::new(1.0, Color32::from_rgb(235, 235, 235)),
        expansion: 0.5,
    };

    v.widgets.active = WidgetVisuals {
        bg_fill: Color32::from_rgb(44, 93, 147),
        weak_bg_fill: Color32::from_rgb(90, 90, 90),
        bg_stroke: Stroke::new(1.0, Color32::from_rgb(20, 20, 20)),
        corner_radius: CornerRadius::same(2),
        fg_stroke: Stroke::new(1.0, Color32::WHITE),
        expansion: 0.0,
    };

    v.widgets.open = WidgetVisuals {
        bg_fill: Color32::from_rgb(64, 64, 64),
        weak_bg_fill: Color32::from_rgb(58, 58, 58),
        bg_stroke: Stroke::new(1.0, Color32::from_rgb(31, 31, 31)),
        corner_radius: CornerRadius::same(2),
        fg_stroke: Stroke::new(1.0, Color32::from_rgb(217, 217, 217)),
        expansion: 0.0,
    };

    v
}
