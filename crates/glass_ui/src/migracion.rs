//! Migración del formato de UI anterior a la fase (versión 0) al
//! actual (versión 1), en memoria, idempotente. La conversión corre
//! en el cargador (editor y runtime); la copia de seguridad en disco
//! solo la escribe el editor, una sola vez, con
//! `escribir_copia_pre_ui`.

use std::path::{Path, PathBuf};

use glass_core::scene::{EntityData, MODO_CAMERA, MODO_OVERLAY, MODO_WORLD};

#[derive(Debug, Clone, Default)]
pub struct InformeMigracion {
    pub aplicada: bool,
    pub avisos: Vec<String>,
    pub entidades_creadas: usize,
}

pub fn necesita_migracion(version: u32) -> bool {
    version < glass_core::scene::UI_SCHEMA_VERSION_ACTUAL
}

fn es_modo_render_valido(modo: &str) -> bool {
    matches!(modo, MODO_OVERLAY | MODO_CAMERA | MODO_WORLD)
}

pub fn migrar_entidades(
    entities: &mut Vec<EntityData>,
    siguiente_id: &mut u64,
) -> InformeMigracion {
    let mut informe = InformeMigracion::default();

    // 1. Corregir modos de render inválidos.
    for entidad in entities.iter_mut() {
        let nombre = entidad.name.clone();

        if let Some(canvas) = entidad.canvas.as_mut() {
            if !es_modo_render_valido(canvas.render_mode.as_str()) {
                let anterior = canvas.render_mode.clone();
                canvas.render_mode = MODO_OVERLAY.to_string();
                informe.aplicada = true;
                informe.avisos.push(format!(
                    "Entidad '{nombre}': modo de render '{anterior}' no válido; se corrige a ScreenSpaceOverlay"
                ));
            }
        }
    }

    // 1b. Validar los campos nuevos de Canvas (P2): la cámara debe
    // existir y tener componente cámara; la distancia debe ser finita
    // y positiva. Lo inválido vuelve a su default (None) con aviso.
    // La lista de cámaras se toma antes del bucle: la migración no
    // crea ni borra cámaras, así que no cambia durante la pasada.
    let ids_con_camara: std::collections::HashSet<u64> = entities
        .iter()
        .filter(|e| e.camera.is_some())
        .map(|e| e.id)
        .collect();
    for entidad in entities.iter_mut() {
        let nombre = entidad.name.clone();
        if let Some(canvas) = entidad.canvas.as_mut() {
            if let Some(id_camara) = canvas.camera {
                if !ids_con_camara.contains(&id_camara) {
                    canvas.camera = None;
                    informe.aplicada = true;
                    informe.avisos.push(format!(
                        "Entidad '{nombre}': cámara {id_camara} no existe o no tiene componente cámara; se usa la cámara de la vista"
                    ));
                }
            }
            if let Some(d) = canvas.plane_distance {
                if !d.is_finite() || d <= 0.0 {
                    canvas.plane_distance = None;
                    informe.aplicada = true;
                    informe.avisos.push(format!(
                        "Entidad '{nombre}': plane_distance {d} no válido; se usa la distancia derivada"
                    ));
                }
            }
        }
    }

    // 2. Separar imagen y texto que conviven en una misma entidad.
    // Primero recogemos los índices para no modificar el Vec mientras lo iteramos.
    let mut indices_a_separar = Vec::new();
    for (indice, entidad) in entities.iter().enumerate() {
        if entidad.ui_image.is_some() && entidad.ui_text.is_some() {
            indices_a_separar.push(indice);
        }
    }

    let mut nuevas_entidades = Vec::with_capacity(indices_a_separar.len());

    for indice in indices_a_separar {
        let id_nuevo = *siguiente_id;
        *siguiente_id += 1;

        let padre_id = entities[indice].id;
        let padre_nombre = entities[indice].name.clone();

        let mut hija = entities[indice].clone();
        hija.id = id_nuevo;
        hija.name = format!("{padre_nombre} (Texto)");
        hija.parent = Some(padre_id);
        hija.children.clear();
        hija.ui_image = None;
        hija.ui_button = None;
        hija.ui_drag = None;
        hija.canvas = None;
        hija.layout_group = None;

        {
            let padre = &mut entities[indice];
            padre.ui_text = None;
            if !padre.children.contains(&id_nuevo) {
                padre.children.push(id_nuevo);
            }
        }

        nuevas_entidades.push(hija);
        informe.entidades_creadas += 1;
        informe.aplicada = true;
        informe
            .avisos
            .push(format!("Entidad '{padre_nombre}': se separa el texto en una entidad hija"));
    }

    entities.extend(nuevas_entidades);

    informe
}

pub fn ruta_copia_pre_ui(ruta_escena: &Path) -> PathBuf {
    let mut s = ruta_escena.as_os_str().to_owned();
    s.push(".pre-ui.bak");
    PathBuf::from(s)
}

pub fn escribir_copia_pre_ui(ruta_escena: &Path) -> std::io::Result<bool> {
    let ruta_copia = ruta_copia_pre_ui(ruta_escena);

    if ruta_copia.exists() {
        return Ok(false);
    }

    std::fs::copy(ruta_escena, &ruta_copia)?;
    Ok(true)
}

/// Migra en memoria las entidades de un prefab (raíz +
/// descendientes) con las mismas reglas que una escena. El archivo
/// del prefab no se toca ni lleva versión de UI: la pasada es
/// idempotente, así que un prefab ya limpio sale intacto. Los ids
/// nuevos salen por encima del mayor id del prefab, y las entidades
/// creadas quedan como descendientes.
pub fn migrar_prefab(prefab: &mut glass_core::Prefab) -> InformeMigracion {
    let mut entidades: Vec<EntityData> =
        Vec::with_capacity(1 + prefab.descendant_entities.len());
    entidades.push(prefab.root_entity.clone());
    entidades.extend(prefab.descendant_entities.iter().cloned());
    let mut siguiente_id = entidades.iter().map(|e| e.id).max().unwrap_or(0) + 1;
    let informe = migrar_entidades(&mut entidades, &mut siguiente_id);
    let mut iter = entidades.into_iter();
    if let Some(raiz) = iter.next() {
        prefab.root_entity = raiz;
    }
    prefab.descendant_entities = iter.collect();
    informe
}


/// Fase UI/Canvas (V1, entrada de la app): tras cargar una escena cuyo
/// esquema de UI es anterior al actual, la migra EN MEMORIA y sube su
/// versión. Solo el editor escribe además la copia de seguridad
/// `<escena>.pre-ui.bak` (una sola vez, antes de tocar nada, y solo si
/// se conoce la ruta): el runtime migra sin escribir en disco y el
/// archivo original solo cambia cuando el usuario guarda la escena.
/// Sin migración pendiente, no hace nada. `escribir_copia` la decide
/// quien llama (la app pasa `cfg!(feature = "editor")`): este crate
/// no tiene feature de editor y no puede decidirlo solo.
pub fn migrar_escena_cargada(
    scene: &mut glass_core::scene::SceneState,
    ruta: Option<&str>,
    escribir_copia: bool,
) {
    if !necesita_migracion(scene.ui_schema_version) {
        return;
    }
    if escribir_copia {
        if let Some(ruta) = ruta {
            match escribir_copia_pre_ui(Path::new(ruta)) {
                Ok(true) => {
                    log::info!("Migración UI: copia previa de '{ruta}' guardada (.pre-ui.bak)")
                }
                Ok(false) => {}
                Err(e) => {
                    log::warn!("Migración UI: sin copia previa de '{ruta}': {e}")
                }
            }
        }
    }
    let mut siguiente_id = scene.next_entity_id;
    let informe = migrar_entidades(&mut scene.entities, &mut siguiente_id);
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

#[cfg(test)]
mod tests {
    use super::*;

    use glass_core::scene::CanvasData;
    use std::fs;

    fn entidad_con_canvas(id: u64, nombre: &str, modo: &str) -> EntityData {
        let mut entidad = EntityData::new(id, nombre.to_string(), Vec::new());
        entidad.canvas = Some(CanvasData {
            render_mode: modo.to_string(),
            reference_resolution: [1920.0, 1080.0],
            match_width_or_height: 0.5,
            camera: None,
            plane_distance: None,
            sorting_order: 0,
        });
        entidad
    }

    fn entidad_con_camara(id: u64, nombre: &str) -> EntityData {
        let mut entidad = EntityData::new(id, nombre.to_string(), Vec::new());
        entidad.camera = Some(Default::default());
        entidad
    }

    fn panel_con_imagen_y_texto() -> EntityData {
        let mut panel = EntityData::new(7, "Panel".to_string(), Vec::new());

        panel.rect_transform = Some(Default::default());
        if let Some(rect) = panel.rect_transform.as_mut() {
            rect.anchored_position = [12.0, -8.0];
            rect.size_delta = [320.0, 180.0];
        }

        panel.ui_image = Some(Default::default());
        panel.ui_text = Some(Default::default());
        panel
    }

    #[test]
    fn render_mode_invalido_se_corrige() {
        let mut entities = vec![entidad_con_canvas(1, "HUD", "Inventado")];
        let mut siguiente_id = 2;

        let informe = migrar_entidades(&mut entities, &mut siguiente_id);

        assert!(informe.aplicada);
        assert_eq!(
            entities[0].canvas.as_ref().unwrap().render_mode,
            "ScreenSpaceOverlay"
        );
        assert!(informe.avisos.iter().any(|aviso| aviso.contains("Inventado")));
    }

    #[test]
    fn render_mode_valido_intacto() {
        let modos = ["ScreenSpaceOverlay", "ScreenSpaceCamera", "WorldSpace"];
        let mut entities: Vec<_> = modos
            .iter()
            .enumerate()
            .map(|(indice, modo)| entidad_con_canvas(indice as u64 + 1, "Canvas", modo))
            .collect();
        let mut siguiente_id = 10;

        let informe = migrar_entidades(&mut entities, &mut siguiente_id);

        assert!(!informe.aplicada);
        assert!(informe.avisos.is_empty());

        for (indice, modo) in modos.iter().enumerate() {
            assert_eq!(
                entities[indice].canvas.as_ref().unwrap().render_mode,
                *modo
            );
        }
    }

    #[test]
    fn camara_canvas_valida_intacta() {
        let mut canvas = entidad_con_canvas(1, "HUD", "ScreenSpaceCamera");
        canvas.canvas.as_mut().unwrap().camera = Some(2);
        let mut entities = vec![canvas, entidad_con_camara(2, "Cam")];
        let mut siguiente_id = 3;

        let informe = migrar_entidades(&mut entities, &mut siguiente_id);

        assert!(!informe.aplicada);
        assert_eq!(entities[0].canvas.as_ref().unwrap().camera, Some(2));
    }

    #[test]
    fn camara_canvas_desconocida_a_none() {
        let mut canvas = entidad_con_canvas(1, "HUD", "ScreenSpaceCamera");
        canvas.canvas.as_mut().unwrap().camera = Some(99);
        let mut entities = vec![canvas, entidad_con_camara(2, "Cam")];
        let mut siguiente_id = 3;

        let informe = migrar_entidades(&mut entities, &mut siguiente_id);

        assert!(informe.aplicada);
        assert_eq!(entities[0].canvas.as_ref().unwrap().camera, None);
        assert!(informe.avisos.iter().any(|a| a.contains("99")));
    }

    #[test]
    fn camara_canvas_sin_componente_a_none() {
        let mut canvas = entidad_con_canvas(1, "HUD", "ScreenSpaceCamera");
        canvas.canvas.as_mut().unwrap().camera = Some(2);
        let mut entities = vec![canvas, EntityData::new(2, "Luz".into(), Vec::new())];
        let mut siguiente_id = 3;

        let informe = migrar_entidades(&mut entities, &mut siguiente_id);

        assert!(informe.aplicada);
        assert_eq!(entities[0].canvas.as_ref().unwrap().camera, None);
    }

    #[test]
    fn plane_distance_invalida_a_none() {
        for distancia in [0.0, -3.0, f32::NAN, f32::INFINITY] {
            let mut canvas = entidad_con_canvas(1, "HUD", "ScreenSpaceCamera");
            canvas.canvas.as_mut().unwrap().plane_distance = Some(distancia);
            let mut entities = vec![canvas];
            let mut siguiente_id = 2;

            let informe = migrar_entidades(&mut entities, &mut siguiente_id);

            assert!(informe.aplicada, "distancia {distancia} debe corregirse");
            assert_eq!(
                entities[0].canvas.as_ref().unwrap().plane_distance,
                None
            );
        }
    }

    #[test]
    fn plane_distance_valida_intacta() {
        let mut canvas = entidad_con_canvas(1, "HUD", "ScreenSpaceCamera");
        canvas.canvas.as_mut().unwrap().plane_distance = Some(2.5);
        let mut entities = vec![canvas];
        let mut siguiente_id = 2;

        let informe = migrar_entidades(&mut entities, &mut siguiente_id);

        assert!(!informe.aplicada);
        assert_eq!(
            entities[0].canvas.as_ref().unwrap().plane_distance,
            Some(2.5)
        );
    }

    #[test]
    fn imagen_y_texto_se_separan() {
        let mut entities = vec![panel_con_imagen_y_texto()];
        let mut siguiente_id = 100;

        let informe = migrar_entidades(&mut entities, &mut siguiente_id);

        assert!(informe.aplicada);
        assert_eq!(informe.entidades_creadas, 1);
        assert_eq!(siguiente_id, 101);
        assert_eq!(entities.len(), 2);

        let padre = &entities[0];
        assert!(padre.ui_text.is_none());
        assert!(padre.ui_image.is_some());
        assert_eq!(padre.children, vec![100]);

        let hija = &entities[1];
        assert_eq!(hija.id, 100);
        assert_eq!(hija.name, "Panel (Texto)");
        assert_eq!(hija.parent, Some(7));
        assert!(hija.ui_text.is_some());
        assert!(hija.ui_image.is_none());
        assert!(hija.ui_button.is_none());
        assert!(hija.canvas.is_none());

        let rect_padre = padre.rect_transform.as_ref().unwrap();
        let rect_hija = hija.rect_transform.as_ref().unwrap();
        assert_eq!(rect_hija.anchored_position, rect_padre.anchored_position);
        assert_eq!(rect_hija.size_delta, rect_padre.size_delta);
        assert_eq!(rect_hija.anchored_position, [12.0, -8.0]);
        assert_eq!(rect_hija.size_delta, [320.0, 180.0]);
    }

    #[test]
    fn migracion_idempotente() {
        let mut entities = vec![panel_con_imagen_y_texto()];
        let mut siguiente_id = 100;

        let primera = migrar_entidades(&mut entities, &mut siguiente_id);
        assert!(primera.aplicada);

        let total_entidades = entities.len();

        let segunda = migrar_entidades(&mut entities, &mut siguiente_id);
        assert!(!segunda.aplicada);
        assert_eq!(segunda.entidades_creadas, 0);
        assert!(segunda.avisos.is_empty());
        assert_eq!(entities.len(), total_entidades);
        assert_eq!(siguiente_id, 101);
    }

    #[test]
    fn copia_pre_ui_una_sola_vez() {
        let dir = std::env::temp_dir().join(format!(
            "glass_ui_migracion_copia_pre_ui_{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();

        let ruta = dir.join("X.scene.json");
        let contenido = r#"{"escena":"falsa","capas":[],"nota":"prueba"}"#;
        fs::write(&ruta, contenido).unwrap();

        let primera = escribir_copia_pre_ui(&ruta).unwrap();
        assert!(primera, "la primera copia pre-UI debe crearse");

        let ruta_copia = ruta_copia_pre_ui(&ruta);
        assert!(ruta_copia.exists(), "la copia pre-UI debe existir");
        let contenido_copia = fs::read_to_string(&ruta_copia).unwrap();
        assert_eq!(contenido_copia, contenido, "la copia debe ser idéntica al original");

        let segunda = escribir_copia_pre_ui(&ruta).unwrap();
        assert!(!segunda, "la segunda llamada no debe reescribir la copia pre-UI");

        let _ = fs::remove_file(&ruta_copia);
        let _ = fs::remove_file(&ruta);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn ruta_copia_no_es_escena() {
        let ruta = std::path::Path::new("assets/scenes/X.scene.json");
        let copia = ruta_copia_pre_ui(ruta);
        let nombre = copia.file_name().unwrap().to_string_lossy().into_owned();

        assert!(
            nombre.ends_with("X.scene.json.pre-ui.bak"),
            "la copia debe terminar en X.scene.json.pre-ui.bak, obtenido: {nombre}"
        );
        assert!(
            !nombre.ends_with(".scene.json"),
            "la copia no debe terminar en .scene.json, obtenido: {nombre}"
        );
    }
}
