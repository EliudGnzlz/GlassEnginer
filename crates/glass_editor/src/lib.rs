//! glass_editor: Unity-style docking editor layout powered by egui.

pub mod docking;
pub mod herramientas;
pub mod panels;
pub use glass_launcher::plugins;
pub use glass_launcher::updates;
pub mod tool_prefs;
pub mod gizmo_prefs;
pub mod undo;

pub use docking::{DecisionCierre, EditorDockingLayout};
pub use panels::{GameResolution, PlayState, ViewportTab};


pub use glass_launcher::setup_editor_fonts;

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
