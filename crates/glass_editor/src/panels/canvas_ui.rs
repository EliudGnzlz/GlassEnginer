//! Panel «Canvas / UI»: edita el Canvas activo y sus elementos de UI sin pasar por el Inspector.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use egui::{DragValue, Ui};
use glass_core::scene::SceneState;

use super::hierarchy::{EntityPreset, HierarchyPanel};

/// Acción pedida durante el dibujo; se ejecuta después, con un lock nuevo.
enum AccionCanvas {
    Canvas,
    Texto { padre: u64 },
    Boton { padre: u64 },
    Panel { padre: u64 },
    Imagen { padre: u64 },
    InputField { padre: u64 },
}

#[derive(Default)]
pub struct CanvasPanel {
    /// Canvas elegido explícitamente en el ComboBox (si sigue siendo válido, tiene prioridad).
    pub canvas_elegido: Option<u64>,
}

impl CanvasPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        scene_state: &Arc<Mutex<SceneState>>,
        seleccion: &mut Option<u64>,
    ) {
        let mut accion: Option<AccionCanvas> = None;

        {
            let Ok(mut scene) = scene_state.lock() else {
                return;
            };

            let canvases: Vec<(u64, String)> = scene
                .entities
                .iter()
                .filter(|e| e.canvas.is_some())
                .map(|e| (e.id, e.name.clone()))
                .collect();

            if canvases.is_empty() {
                ui.label(
                    "Esta escena no tiene ningún Canvas. Crea uno para empezar a construir la interfaz.",
                );
                if ui.button("➕ Crear Canvas").clicked() {
                    accion = Some(AccionCanvas::Canvas);
                }
            } else {
                // Descartar una elección que ya no es un canvas.
                if let Some(elegido) = self.canvas_elegido {
                    if !es_canvas(&scene, elegido) {
                        self.canvas_elegido = None;
                    }
                }

                let Some(activo) = canvas_activo(&scene, *seleccion, self.canvas_elegido) else {
                    return;
                };

                // Selector de canvas (solo si hay varios).
                if canvases.len() > 1 {
                    let nombre_activo = canvases
                        .iter()
                        .find(|(id, _)| *id == activo)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default();
                    let mut eleccion = activo;
                    ui.horizontal(|ui| {
                        ui.label("Canvas:");
                        egui::ComboBox::from_id_salt("canvas_panel_selector")
                            .selected_text(nombre_activo)
                            .show_ui(ui, |ui| {
                                for (id, nombre) in &canvases {
                                    ui.selectable_value(
                                        &mut eleccion,
                                        *id,
                                        format!("{} (ID: {})", nombre, id),
                                    );
                                }
                            });
                    });
                    if eleccion != activo {
                        self.canvas_elegido = Some(eleccion);
                    }
                }

                // Cabecera y ajustes del canvas.
                if let Some(entity) = scene.get_entity_mut(activo) {
                    let id = entity.id;
                    ui.heading(format!("🖼 {}", entity.name));
                    ui.label(format!("ID: {}", id));

                    if let Some(canvas) = entity.canvas.as_mut() {
                        ui.horizontal(|ui| {
                            ui.label("Modo de Render:");
                            egui::ComboBox::from_id_salt(format!("canvas_panel_render_mode_{}", id))
                                .selected_text(&canvas.render_mode)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut canvas.render_mode,
                                        "ScreenSpaceOverlay".into(),
                                        "Screen Space - Overlay",
                                    );
                                    ui.selectable_value(
                                        &mut canvas.render_mode,
                                        "ScreenSpaceCamera".into(),
                                        "Screen Space - Camera",
                                    );
                                    ui.selectable_value(
                                        &mut canvas.render_mode,
                                        "WorldSpace".into(),
                                        "World Space",
                                    );
                                });
                        });
                        // P1 (V7, Q11): el modo aún no tiene efecto; se pinta
                        // como Overlay hasta P2/P3. Aviso visible, sin bloquear.
                        if canvas.render_mode != "ScreenSpaceOverlay" {
                            ui.label(super::AVISO_MODO_UI_SIN_EFECTO);
                        }
                        ui.horizontal(|ui| {
                            ui.label("Resolución Ref:");
                            ui.add(DragValue::new(&mut canvas.reference_resolution[0]).prefix("W: "));
                            ui.add(DragValue::new(&mut canvas.reference_resolution[1]).prefix("H: "));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Match W/H:");
                            ui.add(egui::Slider::new(&mut canvas.match_width_or_height, 0.0..=1.0));
                        });
                    }
                }

                ui.separator();
                ui.heading("Elementos de la UI");

                // Árbol de descendientes con RectTransform o Canvas.
                let elementos: Vec<(u64, usize, String)> = elementos_ui(&scene, activo)
                    .into_iter()
                    .map(|(id, prof)| {
                        let nombre = scene
                            .get_entity(id)
                            .map(|e| e.name.clone())
                            .unwrap_or_default();
                        (id, prof, nombre)
                    })
                    .collect();

                if elementos.is_empty() {
                    ui.label("(sin elementos: añade el primero con los botones)");
                } else {
                    for (id, profundidad, nombre) in &elementos {
                        let texto = format!("{}{}", "  ".repeat(*profundidad), nombre);
                        if ui.selectable_label(*seleccion == Some(*id), texto).clicked() {
                            *seleccion = Some(*id);
                        }
                    }
                }

                // Padre de los nuevos elementos: la selección si está dentro del canvas activo.
                let padre = match *seleccion {
                    Some(sel) if es_igual_o_descendiente(&scene, sel, activo) => sel,
                    _ => activo,
                };

                ui.horizontal(|ui| {
                    if ui.button("➕ Texto").clicked() {
                        accion = Some(AccionCanvas::Texto { padre });
                    }
                    if ui.button("➕ Botón").clicked() {
                        accion = Some(AccionCanvas::Boton { padre });
                    }
                    if ui.button("➕ Panel").clicked() {
                        accion = Some(AccionCanvas::Panel { padre });
                    }
                    if ui.button("➕ Imagen").clicked() {
                        accion = Some(AccionCanvas::Imagen { padre });
                    }
                    if ui.button("➕ InputField").clicked() {
                        accion = Some(AccionCanvas::InputField { padre });
                    }
                });
            }
        }

        // Ejecutar la acción pedida con un lock breve.
        if let Some(accion) = accion {
            let Ok(mut scene) = scene_state.lock() else {
                return;
            };
            match accion {
                AccionCanvas::Canvas => {
                    let id = HierarchyPanel::instantiate_preset(&mut scene, EntityPreset::Canvas);
                    *seleccion = Some(id);
                    self.canvas_elegido = Some(id);
                }
                AccionCanvas::Texto { padre } => {
                    let id = HierarchyPanel::instantiate_preset(&mut scene, EntityPreset::UIText);
                    let _ = scene.reparent(id, Some(padre));
                    *seleccion = Some(id);
                }
                AccionCanvas::Boton { padre } => {
                    let id = HierarchyPanel::instantiate_preset(&mut scene, EntityPreset::UIButton);
                    let _ = scene.reparent(id, Some(padre));
                    *seleccion = Some(id);
                }
                AccionCanvas::Panel { padre } => {
                    let id = HierarchyPanel::instantiate_preset(&mut scene, EntityPreset::UIPanel);
                    let _ = scene.reparent(id, Some(padre));
                    *seleccion = Some(id);
                }
                AccionCanvas::Imagen { padre } => {
                    let id = HierarchyPanel::instantiate_preset(&mut scene, EntityPreset::UIImage);
                    let _ = scene.reparent(id, Some(padre));
                    *seleccion = Some(id);
                }
                AccionCanvas::InputField { padre } => {
                    let id = HierarchyPanel::instantiate_preset(&mut scene, EntityPreset::UIInputField);
                    let _ = scene.reparent(id, Some(padre));
                    *seleccion = Some(id);
                }
            }
        }
    }
}

fn es_canvas(scene: &SceneState, id: u64) -> bool {
    scene
        .get_entity(id)
        .is_some_and(|e| e.canvas.is_some())
}

/// `true` si `id` es `ancestro` o desciende de él (subiendo por `parent`).
fn es_igual_o_descendiente(scene: &SceneState, id: u64, ancestro: u64) -> bool {
    let mut actual = Some(id);
    // Límite de pasos para no colgarse ante un ciclo en los datos.
    let mut pasos = scene.entities.len() + 1;
    while let Some(cur) = actual {
        if cur == ancestro {
            return true;
        }
        if pasos == 0 {
            return false;
        }
        pasos -= 1;
        actual = scene.get_entity(cur).and_then(|e| e.parent);
    }
    false
}

/// Canvas activo: elegido válido > seleccionada si es canvas > ancestro canvas > primer canvas.
pub fn canvas_activo(
    scene: &SceneState,
    seleccion: Option<u64>,
    elegido: Option<u64>,
) -> Option<u64> {
    if let Some(e) = elegido {
        if es_canvas(scene, e) {
            return Some(e);
        }
    }

    if let Some(sel) = seleccion {
        let mut actual = scene.get_entity(sel);
        let mut pasos = scene.entities.len() + 1;
        while let Some(ent) = actual {
            if ent.canvas.is_some() {
                return Some(ent.id);
            }
            if pasos == 0 {
                break;
            }
            pasos -= 1;
            actual = ent.parent.and_then(|p| scene.get_entity(p));
        }
    }

    scene
        .entities
        .iter()
        .find(|e| e.canvas.is_some())
        .map(|e| e.id)
}

/// Descendientes de `canvas_id` (id, profundidad desde 1) con RectTransform o Canvas,
/// en orden de recorrido en profundidad por `children`.
pub fn elementos_ui(scene: &SceneState, canvas_id: u64) -> Vec<(u64, usize)> {
    let mut out = Vec::new();
    let mut visitados = HashSet::new();
    visitados.insert(canvas_id);
    recorrer(scene, canvas_id, 1, &mut visitados, &mut out);
    out
}

fn recorrer(
    scene: &SceneState,
    id: u64,
    profundidad: usize,
    visitados: &mut HashSet<u64>,
    out: &mut Vec<(u64, usize)>,
) {
    let Some(ent) = scene.get_entity(id) else {
        return;
    };
    for &hijo in &ent.children {
        if !visitados.insert(hijo) {
            continue;
        }
        if let Some(h) = scene.get_entity(hijo) {
            if h.rect_transform.is_some() || h.canvas.is_some() {
                out.push((hijo, profundidad));
            }
            recorrer(scene, hijo, profundidad + 1, visitados, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glass_core::scene::{CanvasData, RectTransformData};

    fn datos_canvas() -> CanvasData {
        CanvasData {
            render_mode: "ScreenSpaceOverlay".into(),
            reference_resolution: [1920.0, 1080.0],
            match_width_or_height: 0.5,
        }
    }

    fn enlazar(scene: &mut SceneState, padre: u64, hijo: u64) {
        scene.get_entity_mut(hijo).unwrap().parent = Some(padre);
        scene.get_entity_mut(padre).unwrap().children.push(hijo);
    }

    /// Canvas -> Texto -> Botón, más una entidad suelta.
    fn escena_de_prueba() -> (SceneState, u64, u64, u64, u64) {
        let mut scene = SceneState::default();
        let canvas = scene.create_entity("Canvas".into(), Vec::new());
        let texto = scene.create_entity("Texto".into(), Vec::new());
        let boton = scene.create_entity("Botón".into(), Vec::new());
        let suelta = scene.create_entity("Suelta".into(), Vec::new());

        scene.get_entity_mut(canvas).unwrap().canvas = Some(datos_canvas());
        scene.get_entity_mut(texto).unwrap().rect_transform = Some(RectTransformData::default());
        scene.get_entity_mut(boton).unwrap().rect_transform = Some(RectTransformData::default());

        enlazar(&mut scene, canvas, texto);
        enlazar(&mut scene, texto, boton);

        (scene, canvas, texto, boton, suelta)
    }

    #[test]
    fn canvas_activo_desde_nieto() {
        let (scene, canvas, _, boton, _) = escena_de_prueba();
        assert_eq!(canvas_activo(&scene, Some(boton), None), Some(canvas));
    }

    #[test]
    fn canvas_activo_seleccion_es_canvas() {
        let (scene, canvas, _, _, _) = escena_de_prueba();
        assert_eq!(canvas_activo(&scene, Some(canvas), None), Some(canvas));
    }

    #[test]
    fn canvas_activo_cae_al_primero() {
        let (scene, canvas, _, _, suelta) = escena_de_prueba();
        assert_eq!(canvas_activo(&scene, Some(suelta), None), Some(canvas));
        assert_eq!(canvas_activo(&scene, None, None), Some(canvas));
    }

    #[test]
    fn canvas_activo_ignora_elegido_invalido() {
        let (scene, canvas, texto, _, _) = escena_de_prueba();
        assert_eq!(canvas_activo(&scene, None, Some(texto)), Some(canvas));
    }

    #[test]
    fn elementos_ui_con_profundidades() {
        let (scene, canvas, texto, boton, _) = escena_de_prueba();
        assert_eq!(elementos_ui(&scene, canvas), vec![(texto, 1), (boton, 2)]);
    }

    #[test]
    fn sin_canvas() {
        let mut scene = SceneState::default();
        let suelta = scene.create_entity("Suelta".into(), Vec::new());
        assert_eq!(canvas_activo(&scene, Some(suelta), None), None);
        assert!(elementos_ui(&scene, suelta).is_empty());
    }
}
