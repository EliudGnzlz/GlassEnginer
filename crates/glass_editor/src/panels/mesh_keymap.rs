//! Mapa de atajos de la edición de malla (esquemas Blender/Unity) y utilidades
//! geométricas puras para el modo «agarrar». Sin UI: testeable directamente.

use crate::tool_prefs::ShortcutScheme;

/// Acción de edición de malla disparada por teclado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshKeyAction {
    Undo,
    Redo,
    DeleteSelection,
    Mirror,
    Bake,
    ModeVerts,
    ModeFaces,
    Grab,
}

/// Traduce tecla + modificadores a una acción según el esquema.
pub fn action_for(
    scheme: ShortcutScheme,
    key: egui::Key,
    modifiers: egui::Modifiers,
) -> Option<MeshKeyAction> {
    use egui::Key;

    let ctrl = modifiers.ctrl || modifiers.command;
    let sin_mods = !ctrl && !modifiers.alt;

    // Común a ambos esquemas.
    if ctrl && key == Key::S {
        return Some(MeshKeyAction::Bake);
    }

    match scheme {
        ShortcutScheme::Blender => match key {
            Key::Z if ctrl && !modifiers.shift => Some(MeshKeyAction::Undo),
            Key::Z if ctrl && modifiers.shift => Some(MeshKeyAction::Redo),
            Key::G if sin_mods => Some(MeshKeyAction::Grab),
            Key::X if sin_mods => Some(MeshKeyAction::DeleteSelection),
            Key::M if ctrl => Some(MeshKeyAction::Mirror),
            Key::Num1 if sin_mods => Some(MeshKeyAction::ModeVerts),
            Key::Num2 if sin_mods => Some(MeshKeyAction::ModeFaces),
            _ => None,
        },
        ShortcutScheme::Unity => match key {
            Key::Z if ctrl && !modifiers.shift => Some(MeshKeyAction::Undo),
            Key::Y if ctrl => Some(MeshKeyAction::Redo),
            Key::W if sin_mods => Some(MeshKeyAction::Grab),
            Key::Delete if sin_mods => Some(MeshKeyAction::DeleteSelection),
            _ => None,
        },
    }
}

/// Línea de ayuda con las teclas del esquema.
pub fn help_text(scheme: ShortcutScheme) -> &'static str {
    match scheme {
        ShortcutScheme::Blender => {
            "Blender: G mover · X eliminar · 1/2 vértices/caras · Ctrl+M espejo · Ctrl+Z deshacer · Ctrl+Mayús+Z rehacer · Ctrl+S hornear"
        }
        ShortcutScheme::Unity => {
            "Unity: W mover · Supr eliminar · Ctrl+Z deshacer · Ctrl+Y rehacer · Ctrl+S hornear"
        }
    }
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Intersección rayo-plano. `None` si el rayo es paralelo al plano o el plano queda detrás.
pub fn plane_point(
    origin: [f32; 3],
    dir: [f32; 3],
    point_on_plane: [f32; 3],
    normal: [f32; 3],
) -> Option<[f32; 3]> {
    let denom = dot3(dir, normal);
    if denom.abs() < 1e-8 {
        return None;
    }
    let hacia_plano = [
        point_on_plane[0] - origin[0],
        point_on_plane[1] - origin[1],
        point_on_plane[2] - origin[2],
    ];
    let t = dot3(hacia_plano, normal) / denom;
    if t < 0.0 {
        return None;
    }
    Some([
        origin[0] + dir[0] * t,
        origin[1] + dir[1] * t,
        origin[2] + dir[2] * t,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers};

    const NADA: Modifiers = Modifiers::NONE;
    const B: ShortcutScheme = ShortcutScheme::Blender;
    const U: ShortcutScheme = ShortcutScheme::Unity;

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        }
    }
    fn cmd() -> Modifiers {
        Modifiers {
            command: true,
            ..Modifiers::NONE
        }
    }
    fn ctrl_shift() -> Modifiers {
        Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::NONE
        }
    }
    fn alt() -> Modifiers {
        Modifiers {
            alt: true,
            ..Modifiers::NONE
        }
    }

    fn cerca(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5)
    }

    #[test]
    fn comun_ctrl_s_hornea() {
        for esquema in [B, U] {
            assert_eq!(
                action_for(esquema, Key::S, ctrl()),
                Some(MeshKeyAction::Bake)
            );
            assert_eq!(
                action_for(esquema, Key::S, cmd()),
                Some(MeshKeyAction::Bake)
            );
            assert_eq!(action_for(esquema, Key::S, NADA), None);
        }
    }

    #[test]
    fn blender_positivos() {
        assert_eq!(action_for(B, Key::Z, ctrl()), Some(MeshKeyAction::Undo));
        assert_eq!(action_for(B, Key::Z, cmd()), Some(MeshKeyAction::Undo));
        assert_eq!(
            action_for(B, Key::Z, ctrl_shift()),
            Some(MeshKeyAction::Redo)
        );
        assert_eq!(action_for(B, Key::G, NADA), Some(MeshKeyAction::Grab));
        assert_eq!(
            action_for(B, Key::X, NADA),
            Some(MeshKeyAction::DeleteSelection)
        );
        assert_eq!(action_for(B, Key::M, ctrl()), Some(MeshKeyAction::Mirror));
        assert_eq!(
            action_for(B, Key::Num1, NADA),
            Some(MeshKeyAction::ModeVerts)
        );
        assert_eq!(
            action_for(B, Key::Num2, NADA),
            Some(MeshKeyAction::ModeFaces)
        );
    }

    #[test]
    fn unity_positivos() {
        assert_eq!(action_for(U, Key::Z, ctrl()), Some(MeshKeyAction::Undo));
        assert_eq!(action_for(U, Key::Z, cmd()), Some(MeshKeyAction::Undo));
        assert_eq!(action_for(U, Key::Y, ctrl()), Some(MeshKeyAction::Redo));
        assert_eq!(action_for(U, Key::W, NADA), Some(MeshKeyAction::Grab));
        assert_eq!(
            action_for(U, Key::Delete, NADA),
            Some(MeshKeyAction::DeleteSelection)
        );
    }

    #[test]
    fn negativos() {
        assert_eq!(action_for(B, Key::Z, NADA), None);
        assert_eq!(action_for(U, Key::Z, NADA), None);
        assert_eq!(action_for(B, Key::X, ctrl()), None);
        assert_eq!(action_for(B, Key::G, ctrl()), None);
        assert_eq!(action_for(B, Key::G, alt()), None);
        assert_eq!(action_for(U, Key::G, NADA), None);
        assert_eq!(action_for(U, Key::X, NADA), None);
        assert_eq!(action_for(U, Key::Num1, NADA), None);
        assert_eq!(action_for(B, Key::Delete, NADA), None);
        assert_eq!(action_for(U, Key::Z, ctrl_shift()), None);
    }

    #[test]
    fn plano_de_frente() {
        let p = plane_point(
            [1.0, 2.0, 5.0],
            [0.0, 0.0, -1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        );
        assert!(p.is_some_and(|p| cerca(p, [1.0, 2.0, 0.0])));
    }

    #[test]
    fn rayo_paralelo_none() {
        let p = plane_point(
            [0.0, 0.0, 5.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        );
        assert_eq!(p, None);
    }

    #[test]
    fn plano_detras_none() {
        let p = plane_point(
            [0.0, 0.0, 5.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        );
        assert_eq!(p, None);
    }
}
