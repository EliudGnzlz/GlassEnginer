//! Conversión sRGB <-> lineal (IEC 61966-2-1) y helper de picker de
//! color para valores que el motor almacena en espacio lineal (G1/F5).
//!
//! El picker de egui trabaja en sRGB (espacio de pantalla); el shader
//! consume los colores guardados como lineales, igual que Unity
//! convierte los colores del Inspector antes de la GPU. Sin esta
//! conversión en el borde, un color escogido a mano rendía más oscuro
//! y saturado que la muestra elegida.

/// Convierte un canal sRGB a lineal.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Convierte un canal lineal a sRGB.
pub fn linear_to_srgb(l: f32) -> f32 {
    if l <= 0.0031308 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// Aplica una edición del picker (que trabaja en sRGB) sobre el valor
/// guardado (lineal). Solo escribe si `changed` es true: abrir el
/// picker sin tocar no debe modificar el dato ni perder valores
/// fuera de [0, 1].
pub fn apply_srgb_edit(lin: &mut [f32; 3], srgb_new: [f32; 3], changed: bool) {
    if !changed {
        return;
    }
    for i in 0..3 {
        lin[i] = srgb_to_linear(srgb_new[i]);
    }
}

/// Sustituto de `ui.color_edit_button_rgb` para colores que el motor
/// guarda en LINEAL (mesh.color, light.color, ambiente): muestra la
/// conversión a sRGB y escribe de vuelta convertido a lineal.
pub fn color_edit_linear_rgb(ui: &mut egui::Ui, lin: &mut [f32; 3]) -> egui::Response {
    let mut srgb = [
        linear_to_srgb(lin[0]).clamp(0.0, 1.0),
        linear_to_srgb(lin[1]).clamp(0.0, 1.0),
        linear_to_srgb(lin[2]).clamp(0.0, 1.0),
    ];
    let resp = ui.color_edit_button_rgb(&mut srgb);
    apply_srgb_edit(lin, srgb, resp.changed());
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_known_values() {
        assert!((srgb_to_linear(0.5) - 0.21404).abs() < 1e-4);
        assert!((srgb_to_linear(0.0) - 0.0).abs() < 1e-6);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        assert!((linear_to_srgb(0.21404) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn srgb_threshold_continuity() {
        let eps = 1e-6;
        let a = srgb_to_linear(0.04045 - eps);
        let b = srgb_to_linear(0.04045 + eps);
        assert!((a - b).abs() < 1e-3);
        let c = linear_to_srgb(0.0031308 - eps);
        let d = linear_to_srgb(0.0031308 + eps);
        assert!((c - d).abs() < 1e-3);
    }

    #[test]
    fn srgb_roundtrip() {
        for i in 0..=1000 {
            let x = i as f32 / 1000.0;
            let round = linear_to_srgb(srgb_to_linear(x));
            assert!((round - x).abs() < 1e-5, "x={x} round={round}");
        }
    }

    #[test]
    fn apply_srgb_edit_no_change_keeps_bits() {
        let mut lin = [1.5f32, -0.3, 2.0];
        let before = lin;
        apply_srgb_edit(&mut lin, [0.1, 0.2, 0.3], false);
        assert_eq!(lin, before);
    }

    #[test]
    fn apply_srgb_edit_converts() {
        let mut lin = [0.0f32, 0.0, 0.0];
        apply_srgb_edit(&mut lin, [0.5, 0.5, 0.5], true);
        for v in lin {
            assert!((v - 0.21404).abs() < 1e-4);
        }
    }
}
