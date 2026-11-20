use crate::panels::{
    ConsolePanel, CoordSpace, GizmoMode, HierarchyPanel, InspectorPanel, MeshToolPanel, PivotMode,
    PlayState, ProjectPanel, ShaderGraphEditorPanel, ViewportPanel,
    CanvasPanel,};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use glass_core::SceneManager;
use glass_mcp::SceneState;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EditorTab {
    Hierarchy,
    SceneView,
    GameView,
    Inspector,
    Console,
    Project,
    ShaderGraph,
    MeshForge,
    CanvasUi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutPreset {
    Default,
    TwoByThree,
    FourSplit,
    Wide,
}

/// Espacio de trabajo del editor: el de siempre («Editor», con su dock)
/// o el de las herramientas («Herramientas»), donde la herramienta activa
/// manda a cuerpo completo en vez de vivir en una pestaña del dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EspacioTrabajo {
    Editor,
    Herramientas,
}

/// Qué selección se tocó por última vez (incluye la deselección):
/// es el «foco» que enruta la tecla Supr, como la ventana activa
/// de Unity decide si Supr borra un asset o un objeto.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToqueSupr {
    Ninguno,
    Entidad,
    Asset,
}

/// Destino de la tecla Supr según el último toque y qué selecciones
/// siguen vivas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuprDestino {
    Nada,
    Asset,
    Entidad,
}

/// Regla de enrutado de Supr: actúa sobre la selección tocada más
/// recientemente, y solo si esa selección sigue viva. Si lo último
/// tocado fue una deselección (clic en vacío), Supr no hace nada,
/// igual que en Unity no actúa sobre una ventana sin selección.
pub fn destino_supr(toque: ToqueSupr, hay_asset: bool, hay_entidad: bool) -> SuprDestino {
    match toque {
        ToqueSupr::Asset if hay_asset => SuprDestino::Asset,
        ToqueSupr::Entidad if hay_entidad => SuprDestino::Entidad,
        _ => SuprDestino::Nada,
    }
}

pub struct EditorDockingLayout {
    pub hierarchy: HierarchyPanel,
    pub viewport: ViewportPanel,
    pub inspector: InspectorPanel,
    pub console: ConsolePanel,
    /// Panel «Proyecto» con organización de assets estilo Unity
    /// (crear, renombrar, mover y mandar a papelera). Sustituye al
    /// navegador plano y de sólo lectura que vivía en ConsolePanel.
    pub project: ProjectPanel,
    pub shader_graph: ShaderGraphEditorPanel,
    pub mesh_tool: MeshToolPanel,
    /// Panel de edición de Canvas y UI.
    pub canvas_ui: CanvasPanel,
    pub dock_state: DockState<EditorTab>,
    pub current_preset: LayoutPreset,
    /// Espacio de trabajo activo: el editor de siempre o el de herramientas.
    pub espacio: EspacioTrabajo,
    /// Id (del registro `herramientas`) de la herramienta activa en el
    /// espacio «Herramientas». Si deja de estar disponible, render() la
    /// resuelve a la primera disponible.
    pub herramienta_activa: String,
    pub request_launcher: bool,
    /// Lo fija la app: hay un Hub localizable en este equipo (V3). Sin Hub, el botón de volver se oculta en vez de fallar.
    pub hub_disponible: bool,
    /// Espejo de `ruta_escena_actual` de la app (se sincroniza cada frame).
    /// El guardado la prefiere si la escena activa sigue siendo esa.
    pub ruta_escena_actual: Option<std::path::PathBuf>,
    /// Espejo de `nombre_escena_con_ruta` de la app: nombre activo cuando
    /// se fijó la ruta.
    pub nombre_escena_con_ruta: String,
    pub request_build_and_run: bool,
    pub request_build_settings: bool,
    /// File → «Abrir carpeta contenedora de la escena»: lo resuelve la app,
    /// que es quien conoce la ruta de la escena abierta.
    pub request_abrir_contenedora_escena: bool,
    /// File → «Copiar ruta de la escena»: lo resuelve la app (misma razón).
    pub request_copiar_ruta_escena: bool,
    pub show_project_settings_dialog: bool,
/// Copia editable de los ejes del Input Manager; None = aún no cargada en este diálogo.
pub input_axes_edit: Option<Vec<glass_scripting::input_axes::InputAxisDef>>,
    pub project_settings: glass_core::ProjectSettings,
    pub status_message: Option<(String, std::time::Instant)>,
    /// Enrutado de Supr: valor anterior de cada selección y cuál se
    /// tocó por última vez (ver `destino_supr`).
    pub supr_sel_entidad_prev: Option<u64>,
    pub supr_sel_asset_prev: Option<std::path::PathBuf>,
    pub supr_ultimo_toque: ToqueSupr,
    /// Diálogo "Save Scene As" (equivalente al de Unity).
    pub show_save_as_dialog: bool,
    pub save_as_name: String,
    /// Hash (undo::scene_hash) de la escena en el último guardado o carga:
    /// si el hash actual difiere, la escena tiene cambios sin guardar.
    hash_guardado: u64,
    /// Nombre de la escena activa nueva cuando el vigía detecta un cambio
    /// (carga o reemplazo); la app lo consume para invalidar su ruta.
    pub cambio_escena: Option<String>,
    /// Intervalo de autoguardado en minutos (0 = desactivado). Se carga
    /// del almacén del Launcher y se persiste al cambiarlo.
    pub autosave_minutos: u32,
    /// Ventana «Preferencias del editor».
    pub show_preferences_dialog: bool,
    /// Diálogo «cambios sin guardar» abierto (lo pide la app al cerrar).
    pub dialogo_guardar_cierre: bool,
    /// Decisión del usuario en ese diálogo; la consume la app.
    pub decision_cierre: Option<DecisionCierre>,
    /// Historial de deshacer/rehacer por instantáneas (módulo `undo`).
    pub undo_history: crate::undo::UndoHistory,
    /// Nombre de la escena activa visto en el último frame: si
    /// cambia, la app aplicó una carga y el historial se reinicia.
    undo_escena: String,
    /// True mientras Play/Pause está activo (historial en pausa).
    undo_suspendido: bool,
}

impl Default for EditorDockingLayout {
    fn default() -> Self {
        Self::new()
    }

}

impl EditorDockingLayout {
    /// El espacio que manda de verdad: «Herramientas» solo existe con al
    /// menos una herramienta disponible en el registro; sin ninguna, el
    /// espacio efectivo es siempre «Editor» (las herramientas viven en su
    /// espacio propio, ya no son pestañas del dock).
    pub fn espacio_efectivo(&self, porton_activo: bool) -> EspacioTrabajo {
        match self.espacio {
            EspacioTrabajo::Herramientas if !porton_activo => EspacioTrabajo::Editor,
            e => e,
        }
    }

    /// ¿Está esta pestaña presente en el dock ahora mismo?
    pub fn tab_visible(&self, tab: &EditorTab) -> bool {
        self.dock_state.find_tab(tab).is_some()
    }

    /// Muestra u oculta cualquier pestaña del dock. Idempotente.
    pub fn set_tab_visible(&mut self, tab: EditorTab, on: bool) {
        let actual = self.dock_state.find_tab(&tab);
        match (on, actual) {
            (true, None) => self.dock_state.push_to_focused_leaf(tab),
            (false, Some(path)) => {
                let _ = self.dock_state.remove_tab(path);
            }
            _ => {}
        }
    }

    pub fn new() -> Self {
        let mut slf = Self {
            hierarchy: HierarchyPanel::new(),
            viewport: ViewportPanel::default(),
            inspector: InspectorPanel::new(),
            console: ConsolePanel::default(),
            project: ProjectPanel::new(),
            shader_graph: ShaderGraphEditorPanel::new(),
            mesh_tool: MeshToolPanel::new(),
            canvas_ui: CanvasPanel::new(),
            dock_state: DockState::new(vec![EditorTab::SceneView, EditorTab::GameView]),
            current_preset: LayoutPreset::Default,
            espacio: EspacioTrabajo::Editor,
            herramienta_activa: "meshforge".to_string(),
            request_launcher: false,
            hub_disponible: false,
            ruta_escena_actual: None,
            nombre_escena_con_ruta: String::new(),
            request_abrir_contenedora_escena: false,
            request_copiar_ruta_escena: false,
            request_build_and_run: false,
            request_build_settings: false,
            show_project_settings_dialog: false,
            input_axes_edit: None,
            show_save_as_dialog: false,
            save_as_name: String::new(),
            hash_guardado: 0,
            cambio_escena: None,
            autosave_minutos: crate::updates::LauncherStore::load().autosave_minutes,
            show_preferences_dialog: false,
            dialogo_guardar_cierre: false,
            decision_cierre: None,
            undo_history: crate::undo::UndoHistory::new(),
            undo_escena: String::new(),
            undo_suspendido: false,
            project_settings: glass_core::ProjectSettings::load_or_default("project.json"),
            status_message: None,
            supr_sel_entidad_prev: None,
            supr_sel_asset_prev: None,
            supr_ultimo_toque: ToqueSupr::Ninguno,
        };
        slf.apply_preset(LayoutPreset::Default);
        slf
    }

    /// Trae la pestaña de Juego al frente (al entrar en Play, como en
    /// Unity: dar a Play lleva a la pantalla de juego). Si el layout
    /// actual no tiene pestaña de Juego, no hace nada.
    pub fn focus_game_tab(&mut self) {
        if let Some(path) = self.dock_state.find_tab(&EditorTab::GameView) {
            let _ = self.dock_state.set_active_tab(path);
        }
    }

    pub fn apply_preset(&mut self, preset: LayoutPreset) {
        self.current_preset = preset;
        match preset {
            LayoutPreset::Default => {
                let mut state = DockState::new(vec![EditorTab::SceneView, EditorTab::GameView]);
                let surface = state.main_surface_mut();

                surface.split_left(
                    NodeIndex::root(),
                    0.20,
                    vec![EditorTab::Hierarchy],
                );
                // La Herramienta 3D ya no es una pestaña: vive en el
                // espacio de trabajo «Herramientas».
                let pestanas_abajo: Vec<EditorTab> = vec![
                    EditorTab::Console,
                    EditorTab::Project,
                    EditorTab::ShaderGraph,
                ];
                surface.split_below(NodeIndex::root(), 0.72, pestanas_abajo);
                surface.split_right(
                    NodeIndex::root(),
                    0.74,
                    vec![EditorTab::Inspector],
                );
                self.dock_state = state;
            }
            LayoutPreset::TwoByThree => {
                let mut state = DockState::new(vec![EditorTab::SceneView]);
                let surface = state.main_surface_mut();
                surface.split_right(
                    NodeIndex::root(),
                    0.50,
                    vec![EditorTab::GameView],
                );
                surface.split_left(
                    NodeIndex::root(),
                    0.22,
                    vec![EditorTab::Hierarchy, EditorTab::Project],
                );
                surface.split_right(
                    NodeIndex::root(),
                    0.74,
                    vec![EditorTab::Inspector, EditorTab::Console],
                );
                self.dock_state = state;
            }
            LayoutPreset::FourSplit => {
                let mut state = DockState::new(vec![EditorTab::SceneView]);
                let surface = state.main_surface_mut();
                surface.split_right(NodeIndex::root(), 0.5, vec![EditorTab::GameView]);
                surface.split_below(NodeIndex::root(), 0.5, vec![EditorTab::Hierarchy, EditorTab::Inspector]);
                self.dock_state = state;
            }
            LayoutPreset::Wide => {
                let mut state = DockState::new(vec![EditorTab::SceneView, EditorTab::GameView]);
                let surface = state.main_surface_mut();
                surface.split_below(
                    NodeIndex::root(),
                    0.65,
                    vec![EditorTab::Hierarchy, EditorTab::Inspector, EditorTab::Console, EditorTab::Project],
                );
                self.dock_state = state;
            }
        }
    }

    /// Aplica un paso de deshacer (`redo = false`) o rehacer
    /// sobre la escena compartida y ajusta la selección única:
    /// se conserva si la entidad sigue existiendo tras el paso.
    /// Devuelve si se aplicó algún paso.
    fn apply_history(&mut self, scene_state: &Arc<Mutex<SceneState>>, redo: bool) -> bool {
        let Ok(mut sc) = scene_state.lock() else {
            return false;
        };
        let snap = if redo {
            self.undo_history.redo(&sc.entities, sc.next_entity_id)
        } else {
            self.undo_history.undo(&sc.entities, sc.next_entity_id)
        };
        let Some(snap) = snap else {
            return false;
        };
        sc.entities = snap.entities;
        sc.next_entity_id = snap.next_entity_id;
        if let Some(sel) = self.hierarchy.selected_entity {
            if !sc.entities.iter().any(|e| e.id == sel) {
                self.hierarchy.selected_entity = None;
            }
        }
        self.status_message = Some((
            if redo { "↪ Rehecho".into() } else { "↩ Deshecho".into() },
            std::time::Instant::now(),
        ));
        true
    }

    /// ¿La escena difiere del último guardado/carga? (Comparación por
    /// hash de contenido: deshacer hasta el estado guardado la limpia.)
    pub fn escena_sucia(&self, scene: &SceneState) -> bool {
        crate::undo::scene_hash(&scene.entities, scene.next_entity_id) != self.hash_guardado
    }

    /// Marca la escena como guardada con su contenido actual.
    pub fn marcar_guardada(&mut self, scene: &SceneState) {
        self.hash_guardado = crate::undo::scene_hash(&scene.entities, scene.next_entity_id);
    }
}

/// Decisión del usuario ante el diálogo de cambios sin guardar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionCierre {
    Guardar,
    NoGuardar,
    Cancelar,
}

impl EditorDockingLayout {
    /// Guarda la escena activa en la ruta que dicta el `SceneManager`.
    ///
    /// Antes había tres copias del literal `"assets/scenes/SampleScene.scene.json"`,
    /// así que guardar cualquier otra escena pisaba siempre la primera y lo que
    /// se escribía en disco no coincidía con el nombre que reportaba el puente.
    /// Ahora la ruta sale de `SceneManager::save_key()`, que conoce la clave de
    /// asset de la escena cargada; `Save Scene As` sólo la deriva de un nombre.
    /// Ruta directa de guardado: la registrada al cargar la escena desde
    /// archivo, solo si la escena activa sigue llamándose igual (si el
    /// usuario cambió de escena, la ruta registrada ya no describe a la
    /// activa y manda la clave VFS de siempre).
    fn ruta_guardado_directa(
        ruta: &Option<std::path::PathBuf>,
        nombre_con_ruta: &str,
        nombre_activo: &str,
    ) -> Option<std::path::PathBuf> {
        match ruta {
            Some(_) if !nombre_con_ruta.is_empty() && nombre_con_ruta == nombre_activo => ruta.clone(),
            _ => None,
        }
    }

    fn save_scene_to(
        &mut self,
        scene_state: &Arc<Mutex<SceneState>>,
        scene_manager: &Arc<Mutex<SceneManager>>,
        save_as: Option<&str>,
    ) {
        let ahora = || std::time::Instant::now();

        // Se bloquea primero el gestor y después la escena: es el mismo orden
        // que usa `apply_pending_scene_load` en el bucle principal, así que no
        // puede formarse un abrazo mortal entre ambos candados.
        let Ok(mut manager) = scene_manager.lock() else {
            self.status_message = Some(("SceneManager bloqueado por un hilo en pánico".into(), ahora()));
            return;
        };

        // Guardado simple de una escena cargada desde archivo: se escribe en
        // su ruta absoluta, igual que el autoguardado de la app.
        let ruta_directa = match save_as {
            None => Self::ruta_guardado_directa(
                &self.ruta_escena_actual,
                &self.nombre_escena_con_ruta,
                manager.active_scene_name(),
            ),
            Some(_) => None,
        };
        if let Some(ruta) = ruta_directa {
            let Ok(scene) = scene_state.lock() else {
                self.status_message = Some(("SceneState bloqueado; no se guardó nada".into(), ahora()));
                return;
            };
            let mensaje = match scene.save_to_json(&ruta.to_string_lossy()) {
                Ok(()) => {
                    self.hash_guardado = crate::undo::scene_hash(&scene.entities, scene.next_entity_id);
                    format!("Escena guardada en {}", ruta.display())
                }
                Err(e) => format!("No se pudo guardar {}: {e}", ruta.display()),
            };
            drop(scene);
            drop(manager);
            self.status_message = Some((mensaje, ahora()));
            return;
        }

        let key = match save_as {
            Some(name) => glass_core::scenes::scene_key_for_name(name),
            None => manager.save_key(),
        };
        let path = match manager.vfs().resolve(&key) {
            Ok(path) => path,
            Err(e) => {
                self.status_message = Some((format!("Ruta de escena inválida: {e}"), ahora()));
                return;
            }
        };
        let Ok(scene) = scene_state.lock() else {
            self.status_message = Some(("SceneState bloqueado; no se guardó nada".into(), ahora()));
            return;
        };

        let mensaje = match scene.save_to_json(&path.to_string_lossy()) {
            Ok(()) => {
                self.hash_guardado = crate::undo::scene_hash(&scene.entities, scene.next_entity_id);
                if save_as.is_some() {
                    // La escena pasa a "ser" ese archivo: el siguiente Ctrl+S
                    // reescribe esta clave en lugar de volver a preguntar.
                    let nombre = glass_core::scenes::short_scene_name(&key);
                    manager.set_active_name(&nombre);
                }
                format!("Escena guardada en {key}")
            }
            Err(e) => format!("No se pudo guardar {key}: {e}"),
        };
        drop(scene);
        drop(manager);
        self.status_message = Some((mensaje, ahora()));
    }

    /// Menú de escenas del proyecto, con la activa marcada.
    ///
    /// Sólo **encola** la carga: el motor la aplica al final del frame en
    /// `GlassEngineApp::apply_pending_scene_load`, igual que un
    /// `UnityEngine.SceneManagement.SceneManager.LoadScene` desde C#.
    fn scene_switcher_menu(
        &mut self,
        ui: &mut egui::Ui,
        scene_manager: &Arc<Mutex<SceneManager>>,
    ) {
        let (disponibles, activa) = match scene_manager.lock() {
            Ok(m) => (m.available_scenes(), m.active_scene_name().to_string()),
            Err(_) => (Vec::new(), "?".to_string()),
        };

        if disponibles.is_empty() {
            ui.label("No hay escenas en assets/scenes/");
            return;
        }

        for nombre in disponibles {
            let etiqueta = if nombre == activa {
                format!("✔ {nombre}")
            } else {
                nombre.clone()
            };
            if ui.button(etiqueta).clicked() {
                let mensaje = match scene_manager.lock() {
                    Ok(mut m) => match m.request_load(&nombre) {
                        Ok(key) => format!("Carga encolada: {key} (se aplica al final del frame)"),
                        Err(e) => format!("No se pudo cargar '{nombre}': {e}"),
                    },
                    Err(_) => "SceneManager bloqueado por un hilo en pánico".to_string(),
                };
                self.status_message = Some((mensaje, std::time::Instant::now()));
                ui.close();
            }
        }
    }

    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        scene_texture_id: Option<egui::TextureId>,
        game_texture_id: Option<egui::TextureId>,
        scene_state: &Arc<Mutex<SceneState>>,
        scene_manager: &Arc<Mutex<SceneManager>>,
    ) {
        // El editor trabaja en tema oscuro (el Launcher, en claro): cada
        // modo fija sus visuales al empezar a dibujarse.
        ui.ctx().set_visuals(crate::unity_visuals());
        // Espaciado compacto estilo Unity (la Style se hereda de
        // los Visuals; solo se toca el espaciado entre widgets).
        {
            let s = ui.style_mut();
            s.spacing.item_spacing = egui::vec2(6.0, 3.0);
            s.spacing.button_padding = egui::vec2(8.0, 3.0);
            s.spacing.indent = 14.0;
        }

        // --- Importación por arrastre desde el SO ---
        // Al soltar archivos sobre la ventana del editor se copian
        // a la carpeta visible del panel Project (con nombre libre
        // si colisionan); el panel refresca y lo anota en su estado.
        let soltados: Vec<std::path::PathBuf> = ui.ctx().input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !soltados.is_empty() {
            self.project.importar_soltados(&soltados);
        }
        // Aviso mientras se sobrevuela arrastrando archivos.
        if ui.ctx().input(|i| !i.raw.hovered_files.is_empty()) {
            let rect = ui.ctx().content_rect();
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("aviso_soltar_importar"),
            ));
            painter.rect_filled(
                rect,
                0.0,
                egui::Color32::from_rgba_unmultiplied(10, 60, 110, 90),
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!(
                    "Suelta para importar en {}",
                    crate::panels::project::ruta_legible(
                        &self.project.assets_root,
                        &self.project.current_dir
                    )
                ),
                egui::FontId::proportional(22.0),
                egui::Color32::WHITE,
            );
        }

        let mut selected_id = self.hierarchy.selected_entity;
        let seleccion_inicial = selected_id;

        // --- 0. ATAJOS DE TECLADO GLOBALES ESTILO UNITY ---
        // Con la Herramienta 3D abierta, un esquema de malla elegido y el
        // ratón encima del panel, mandan los atajos de ese esquema (los lee
        // el propio panel); en el resto del editor, los de Unity.
        // El panel lee el hover del viewport del frame anterior (mismo
        // patrón que su propio panel_hovered): con sesión activa, los
        // atajos de malla también mandan sobre la vista de escena (R2).
        self.mesh_tool.viewport_hovered = self.viewport.scene_hovered;
        let malla_manda = self.mesh_tool.session.is_some()
            && self.mesh_tool.prefs.scheme.is_some()
            && (self.mesh_tool.panel_hovered || self.viewport.scene_hovered);
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::B)) {
            self.request_build_and_run = true;
        }
        if ui.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::B)) {
            self.request_build_settings = true;
        }
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::P)) {
            self.viewport.play_state = match self.viewport.play_state {
                PlayState::Edit | PlayState::Paused => PlayState::Playing,
                PlayState::Playing => {
                    PlayState::Edit
                }
            };
        }
        if !malla_manda && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            self.save_scene_to(scene_state, scene_manager, None);
        }
        // Deshacer / rehacer de la escena. Con un campo de texto
        // enfocado, las teclas son del campo (que tiene su propio
        // deshacer), no de la escena.
        let texto_enfocado = ui.ctx().memory(|m| m.focused().is_some());
        if !texto_enfocado && !malla_manda {
            let (deshacer, rehacer) = ui.input(|i| {
                (
                    i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                    i.modifiers.command
                        && (i.key_pressed(egui::Key::Y)
                            || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                )
            });
            if deshacer && self.apply_history(scene_state, false) {
                selected_id = self.hierarchy.selected_entity;
            }
            if rehacer && self.apply_history(scene_state, true) {
                selected_id = self.hierarchy.selected_entity;
            }
            // Enrutado de Supr entre las dos selecciones (entidad y
            // asset): los toques se detectan por cambio de valor,
            // aquí, tras deshacer/rehacer (que pueden reasignar
            // `selected_id`). Dentro de este portón, un cambio hecho
            // con texto enfocado o con la malla al mando se detecta
            // al reabrirse el portón, no al ocurrir.
            if selected_id != self.supr_sel_entidad_prev {
                self.supr_sel_entidad_prev = selected_id;
                self.supr_ultimo_toque = ToqueSupr::Entidad;
            }
            if self.project.seleccion != self.supr_sel_asset_prev {
                self.supr_sel_asset_prev = self.project.seleccion.clone();
                self.supr_ultimo_toque = ToqueSupr::Asset;
            }
            // Toque por clic: un clic primario dentro de un panel lo
            // convierte en el contexto de Supr AUNQUE la selección no
            // cambie de valor (reclicar lo ya seleccionado tras haber
            // tocado el otro panel). Los rects son los del frame
            // anterior, como el resto de lecturas de hover del marco.
            // Se aplica tras los cambios de valor para que el gesto
            // físico mande en el mismo frame.
            if ui.input(|i| i.pointer.button_clicked(egui::PointerButton::Primary)) {
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    if self.project.rect_ultimo.contains(pos) {
                        self.supr_ultimo_toque = ToqueSupr::Asset;
                    } else if self.hierarchy.rect_ultimo.contains(pos)
                        || self.viewport.rect_escena.contains(pos)
                    {
                        self.supr_ultimo_toque = ToqueSupr::Entidad;
                    }
                }
            }
            // Supr: en Unity actúa sobre la ventana con foco
            // (Proyecto → el asset; Jerarquía/Escena → el objeto).
            // Aquí manda la selección tocada más recientemente.
            if self.viewport.play_state == PlayState::Edit
                && ui.input(|i| i.key_pressed(egui::Key::Delete))
            {
                match destino_supr(
                    self.supr_ultimo_toque,
                    self.project.seleccion.is_some(),
                    selected_id.is_some(),
                ) {
                    SuprDestino::Asset => {
                        // El asset va a la papelera; la entidad queda intacta.
                        self.project.eliminar_seleccion();
                    }
                    SuprDestino::Entidad => {
                        if let Some(id) = selected_id {
                            let borrado = scene_state
                                .lock()
                                .map(|mut sc| sc.delete_entity(id))
                                .unwrap_or(false);
                            if borrado {
                                self.hierarchy.selected_entity = None;
                                selected_id = None;
                                self.status_message = Some((
                                    "🗑 Objeto eliminado".into(),
                                    std::time::Instant::now(),
                                ));
                            }
                        }
                    }
                    SuprDestino::Nada => {}
                }
            }
        }

        // --- ESPACIO DE TRABAJO ---
        // El portón del espacio es «hay alguna herramienta disponible
        // según el registro». Los portones de cada herramienta se leen en
        // vivo (de disco, como hace el menú Window): si se apagan todas
        // estando en «Herramientas», se vuelve solo al «Editor» en este
        // mismo frame.
        let disponibles = crate::herramientas::herramientas_disponibles();
        let porton_herramientas = !disponibles.is_empty();
        let espacio = self.espacio_efectivo(porton_herramientas);
        if espacio != self.espacio {
            self.espacio = espacio;
        }
        // Si la herramienta activa dejó de estar disponible, pasa a la
        // primera disponible.
        if let Some(h) = crate::herramientas::resolver_activa(&disponibles, &self.herramienta_activa) {
            if h.id != self.herramienta_activa {
                self.herramienta_activa = h.id.to_string();
            }
        }

        // El panel de MeshForge resuelve los modelos y hornea contra los
        // Assets del proyecto abierto: los mismos que muestra el panel
        // Proyecto.
        self.mesh_tool.assets_root = self.project.assets_root.clone();

        // --- 1. BARRA DE MENÚS PRINCIPAL ESTILO UNITY / UNREAL ---
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.menu_button("File", |ui| {
                if ui.button("📁 New Scene\tCtrl+N").clicked() {
                    if let Ok(mut m) = scene_manager.lock() {
                        m.set_active_name("Untitled");
                    }
                    if let Ok(mut sc) = scene_state.lock() {
                        sc.entities.clear();
                        sc.next_entity_id = 1;
                        sc.create_entity("Main Camera".into(), vec!["Transform".into(), "Camera".into()]);
                        sc.create_entity("Directional Light".into(), vec!["Transform".into(), "Light".into()]);
                        self.undo_history.reset(&sc.entities, sc.next_entity_id);
                    }
                    self.status_message = Some(("Nueva escena inicializada".into(), std::time::Instant::now()));
                    ui.close();
                }
                if ui.button("💾 Save Scene\tCtrl+S").clicked() {
                    self.save_scene_to(scene_state, scene_manager, None);
                    ui.close();
                }
                if ui.button("💾 Save Scene As...").clicked() {
                    self.save_as_name = match scene_manager.lock() {
                        Ok(m) => m.active_scene_name().to_string(),
                        Err(_) => "SampleScene".to_string(),
                    };
                    self.show_save_as_dialog = true;
                    ui.close();
                }
                ui.menu_button("📂 Open Scene", |ui| {
                    self.scene_switcher_menu(ui, scene_manager);
                });
                ui.separator();
                if ui.button("📂 Abrir carpeta contenedora de la escena").clicked() {
                    self.request_abrir_contenedora_escena = true;
                    ui.close();
                }
                if ui.button("📋 Copiar ruta de la escena").clicked() {
                    self.request_copiar_ruta_escena = true;
                    ui.close();
                }
                if ui.button("⚙ Build Settings...\tCtrl+Shift+B").clicked() {
                    self.request_build_settings = true;
                    ui.close();
                }
                if ui.button("🚀 Build And Run\tCtrl+B").clicked() {
                    self.request_build_and_run = true;
                    ui.close();
                }
                if ui.button("💾 Save Project").clicked() {
                    let _ = self.project_settings.save_to_file("project.json");
                    self.status_message = Some(("Configuración del proyecto guardada en project.json".into(), std::time::Instant::now()));
                    ui.close();
                }
                ui.separator();
                if self.hub_disponible && ui.button("🏠 Project Hub / Launcher").clicked() {
                    self.request_launcher = true;
                    ui.close();
                }
            });

            ui.menu_button("Edit", |ui| {
                if ui.button("⚙ Preferencias...").clicked() {
                    self.show_preferences_dialog = true;
                    ui.close();
                }
                ui.separator();
                if ui.add_enabled(self.undo_history.can_undo(), egui::Button::new("↩ Undo\tCtrl+Z")).clicked() {
                    if self.apply_history(scene_state, false) {
                        selected_id = self.hierarchy.selected_entity;
                    }
                    ui.close();
                }
                if ui.add_enabled(self.undo_history.can_redo(), egui::Button::new("↪ Redo\tCtrl+Y")).clicked() {
                    if self.apply_history(scene_state, true) {
                        selected_id = self.hierarchy.selected_entity;
                    }
                    ui.close();
                }
                ui.separator();
                let play_label = if self.viewport.play_state == PlayState::Playing { "⏹ Stop\tCtrl+P" } else { "▶ Play\tCtrl+P" };
                if ui.button(play_label).clicked() {
                    self.viewport.play_state = match self.viewport.play_state {
                        PlayState::Edit | PlayState::Paused => PlayState::Playing,
                        PlayState::Playing => {
                            PlayState::Edit
                        }
                    };
                    ui.close();
                }
                ui.separator();
                if ui.button("📋 Duplicate\tCtrl+D").clicked() {
                    if let Some(id) = self.hierarchy.selected_entity {
                        if let Ok(mut sc) = scene_state.lock() {
                            if let Some(orig) = sc.get_entity(id).cloned() {
                                let new_id = sc.create_entity(format!("{} (Clone)", orig.name), orig.components.clone());
                                if let Some(new_ent) = sc.get_entity_mut(new_id) {
                                    new_ent.transform = orig.transform.clone();
                                    new_ent.transform.position[0] += 1.0;
                                    new_ent.mesh = orig.mesh.clone();
                                    new_ent.physics = orig.physics.clone();
                                    new_ent.audio = orig.audio.clone();
                                    new_ent.camera = orig.camera.clone();
                                    new_ent.light = orig.light.clone();
                                    new_ent.canvas = orig.canvas.clone();
                                    new_ent.rect_transform = orig.rect_transform.clone();
                                    new_ent.ui_text = orig.ui_text.clone();
                                    new_ent.ui_button = orig.ui_button.clone();
                                }
                                self.hierarchy.selected_entity = Some(new_id);
                            }
                        }
                    }
                    ui.close();
                }
                if ui.button("🗑 Delete\tDel").clicked() {
                    if let Some(id) = self.hierarchy.selected_entity {
                        if let Ok(mut sc) = scene_state.lock() {
                            sc.delete_entity(id);
                        }
                        self.hierarchy.selected_entity = None;
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button("⚙ Project Settings...").clicked() {
                    self.show_project_settings_dialog = true;
                    ui.close();
                }
            });

            ui.menu_button("Assets", |ui| {
                if let Some(id) = self.hierarchy.selected_entity {
                    if ui.button("📦 Save Selection as Prefab").clicked() {
                        if let Ok(sc) = scene_state.lock() {
                            if let Ok(prefab) = glass_core::Prefab::from_entity(&sc, id) {
                                let clean_name = prefab.name.replace(" ", "_");
                                let path = format!("assets/prefabs/{}.prefab.json", clean_name);
                                let _ = prefab.save_to_file(&path);
                                self.status_message = Some((format!("Prefab guardado exitosamente en {}", path), std::time::Instant::now()));
                            }
                        }
                        ui.close();
                    }
                } else {
                    ui.label("📦 Save as Prefab (Seleccione objeto)");
                }
                ui.menu_button("🧩 Instantiate Prefab", |ui| {
                    let mut found_prefabs = Vec::new();
                    if let Ok(entries) = std::fs::read_dir("assets/prefabs") {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                                found_prefabs.push(path);
                            }
                        }
                    }
                    if found_prefabs.is_empty() {
                        ui.label("No hay prefabs en assets/prefabs/");
                    } else {
                        for p in found_prefabs {
                            let file_name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Prefab");
                            if ui.button(format!("🧩 {}", file_name)).clicked() {
                                if let Ok(prefab) = glass_core::Prefab::load_from_file(&p) {
                                    if let Ok(mut sc) = scene_state.lock() {
                                        let new_id = sc.instantiate_prefab(&prefab, None, Some([0.0, 1.0, 0.0]));
                                        self.hierarchy.selected_entity = Some(new_id);
                                    }
                                    self.status_message = Some((format!("Prefab {} instanciado", file_name), std::time::Instant::now()));
                                }
                                ui.close();
                            }
                        }
                    }
                });
                ui.separator();
                if ui.button("🔄 Recompile C# Scripts").clicked() {
                    let mut comando = std::process::Command::new("dotnet");
                    comando.args(["build", "managed/GlassEngine.Core/GlassEngine.Core.csproj"]);
                    glass_core::proceso::sin_consola(&mut comando);
                    let res = comando.status();
                    if let Ok(status) = res {
                        if status.success() {
                            self.status_message = Some(("✅ C# Scripts recompilados exitosamente (0 errores)".into(), std::time::Instant::now()));
                        } else {
                            self.status_message = Some(("❌ Fallo en compilación C#".into(), std::time::Instant::now()));
                        }
                    }
                    ui.close();
                }
                if ui.button("🔄 Refresh Assets").clicked() {
                    self.status_message = Some(("Assets actualizados".into(), std::time::Instant::now()));
                    ui.close();
                }
            });

            ui.menu_button("GameObject", |ui| {
                if ui.button("📦 Create Empty").clicked() {
                    if let Ok(mut sc) = scene_state.lock() {
                        let id = sc.create_entity("GameObject".into(), vec!["Transform".into()]);
                        self.hierarchy.selected_entity = Some(id);
                    }
                    ui.close();
                }
                ui.menu_button("⬡ 3D Object", |ui| {
                    if ui.button("■ Cube").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Cube".into(), vec!["Transform".into(), "MeshRenderer".into()]);
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                    if ui.button("● Sphere").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Sphere".into(), vec!["Transform".into(), "MeshRenderer".into()]);
                            // La primitiva integrada, como en el preset
                            // de la jerarquía (antes dibujaba el cubo).
                            if let Some(ent) = sc.get_entity_mut(id) {
                                if let Some(m) = &mut ent.mesh {
                                    m.model = "sphere".into();
                                }
                            }
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                    if ui.button("▬ Plane").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Plane".into(), vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()]);
                            // La primitiva integrada, como en el preset de
                            // la jerarquía (antes esto dibujaba el cubo).
                            if let Some(ent) = sc.get_entity_mut(id) {
                                if let Some(m) = &mut ent.mesh {
                                    m.model = "plane".into();
                                }
                            }
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                    if ui.button("▮ Cylinder").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Cylinder".into(), vec!["Transform".into(), "MeshRenderer".into()]);
                            // La primitiva integrada, como en el preset
                            // de la jerarquía (antes dibujaba el cubo).
                            if let Some(ent) = sc.get_entity_mut(id) {
                                if let Some(m) = &mut ent.mesh {
                                    m.model = "cylinder".into();
                                }
                            }
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                });
                ui.menu_button("☼ Light", |ui| {
                    if ui.button("☼ Directional Light").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Directional Light".into(), vec!["Transform".into(), "Light".into()]);
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                    if ui.button("✦ Point Light").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Point Light".into(), vec!["Transform".into(), "Light".into()]);
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                    if ui.button("▼ Spot Light").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Spot Light".into(), vec!["Transform".into(), "Light".into()]);
                            if let Some(ent) = sc.get_entity_mut(id) {
                                if let Some(ref mut l) = ent.light {
                                    l.light_type = "Spot".into();
                                }
                            }
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                });
                if ui.button("🎥 Camera").clicked() {
                    if let Ok(mut sc) = scene_state.lock() {
                        let id = sc.create_entity("Camera".into(), vec!["Transform".into(), "Camera".into()]);
                        self.hierarchy.selected_entity = Some(id);
                    }
                    ui.close();
                }
                ui.menu_button("🖼 UI", |ui| {
                    if ui.button("🖼 Canvas").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Canvas".into(), vec!["Transform".into(), "RectTransform".into(), "Canvas".into()]);
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                    if ui.button("🔤 Text (TMP)").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Text (TMP)".into(), vec!["Transform".into(), "RectTransform".into(), "Text".into()]);
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                    if ui.button("🔘 Button").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Button".into(), vec!["Transform".into(), "RectTransform".into(), "Button".into(), "Text".into()]);
                            if let Some(ent) = sc.get_entity_mut(id) {
                                if let Some(txt) = &mut ent.ui_text {
                                    txt.text = "Button".into();
                                }
                            }
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                });
                ui.menu_button("🔊 Audio", |ui| {
                    if ui.button("🔊 Audio Source").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Audio Source".into(), vec!["Transform".into(), "AudioSource".into()]);
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                });
            });

            ui.menu_button("Component", |ui| {
                if let Some(id) = self.hierarchy.selected_entity {
                    ui.menu_button("Physics", |ui| {
                        if ui.button("Rigidbody").clicked() {
                            if let Ok(mut sc) = scene_state.lock() {
                                if let Some(ent) = sc.get_entity_mut(id) {
                                    if !ent.components.iter().any(|c| c == "RigidBody") {
                                        ent.components.push("RigidBody".into());
                                        ent.physics = Some(glass_core::PhysicsData { has_rigidbody: true, ..glass_core::PhysicsData::default() });
                                    }
                                }
                            }
                            ui.close();
                        }
                    });
                    ui.menu_button("Audio", |ui| {
                        if ui.button("Audio Source").clicked() {
                            if let Ok(mut sc) = scene_state.lock() {
                                if let Some(ent) = sc.get_entity_mut(id) {
                                    if !ent.components.iter().any(|c| c == "AudioSource") {
                                        ent.components.push("AudioSource".into());
                                        ent.audio = Some(glass_core::AudioSourceData::default());
                                    }
                                }
                            }
                            ui.close();
                        }
                    });
                } else {
                    ui.label("Seleccione un GameObject primero");
                }
            });

            ui.menu_button("Window", |ui| {
                ui.menu_button("Layouts", |ui| {
                    if ui.button("Default").clicked() { self.apply_preset(LayoutPreset::Default); ui.close(); }
                    if ui.button("2 by 3").clicked() { self.apply_preset(LayoutPreset::TwoByThree); ui.close(); }
                    if ui.button("4 Split").clicked() { self.apply_preset(LayoutPreset::FourSplit); ui.close(); }
                    if ui.button("Wide").clicked() { self.apply_preset(LayoutPreset::Wide); ui.close(); }
                });
                ui.separator();
                for (tab, etiqueta) in [
                    (EditorTab::Hierarchy, "🌿 Jerarquía"),
                    (EditorTab::SceneView, "🎬 Escena (Scene)"),
                    (EditorTab::GameView, "🎮 Juego (Game)"),
                    (EditorTab::Inspector, "🔍 Inspector"),
                    (EditorTab::Console, "📟 Consola"),
                    (EditorTab::Project, "📁 Proyecto / Assets"),
                    (EditorTab::ShaderGraph, "🎨 Grafo de Shaders"),
                    (EditorTab::CanvasUi, "🖼 Canvas / UI"),
                ] {
                    let mut ver = self.tab_visible(&tab);
                    if ui.checkbox(&mut ver, etiqueta).changed() {
                        self.set_tab_visible(tab, ver);
                        ui.close();
                    }
                }
                // MeshForge es un panel del Editor (lateral, sobre la
                // escena), pero solo existe si su complemento está activo:
                // la misma puerta del registro que abre el espacio
                // Herramientas. Sin herramienta disponible, ni menú.
                if disponibles.iter().any(|h| h.id == "meshforge") {
                    let mut ver_meshforge = self.tab_visible(&EditorTab::MeshForge);
                    if ui.checkbox(&mut ver_meshforge, "🧊 MeshForge").changed() {
                        self.set_tab_visible(EditorTab::MeshForge, ver_meshforge);
                        ui.close();
                    }
                }
            });

            ui.menu_button("Help", |ui| {
                if ui.button("📖 GlassEngine Documentation").clicked() { ui.close(); }
                if ui.button("ℹ About GlassEngine Beta").clicked() { ui.close(); }
            });

            // --- CONMUTADOR DE ESPACIO DE TRABAJO ---
            ui.separator();
            ui.label("Espacio:");
            if ui
                .selectable_label(espacio == EspacioTrabajo::Editor, "🛠 Editor")
                .on_hover_text("El editor de escenas de siempre")
                .clicked()
            {
                self.espacio = EspacioTrabajo::Editor;
            }
            if porton_herramientas
                && ui
                    .selectable_label(espacio == EspacioTrabajo::Herramientas, "🧊 Herramientas")
                    .on_hover_text("El espacio propio de las herramientas")
                    .clicked()
            {
                self.espacio = EspacioTrabajo::Herramientas;
            }

            // Feedback de estado en la barra superior
            if let Some((msg, time)) = &self.status_message {
                if time.elapsed().as_secs() < 6 {
                    ui.separator();
                    ui.label(egui::RichText::new(msg).italics().color(egui::Color32::from_rgb(100, 220, 160)));
                }
            }
        });

        ui.separator();

        // --- BARRA SUPERIOR DE HERRAMIENTAS PRINCIPALES ESTILO UNITY ---
        // Solo en el espacio Editor: en «Herramientas» esos controles
        // (gizmo, pivote, reproducción) no tienen vista sobre la que actuar.
        if espacio == EspacioTrabajo::Editor {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            // 0. Botón de acceso al Hub / Lanzador de Proyectos
            if self.hub_disponible
                && ui
                    .button(egui::RichText::new("🏠 Hub").strong().color(egui::Color32::from_rgb(80, 210, 255)))
                    .on_hover_text("Regresar al Lanzador de Proyectos y Escenas de Ejemplo (Hub)")
                    .clicked()
            {
                self.request_launcher = true;
            }

            ui.separator();

            // 1. Herramientas de Edición en Iconos Cuadrados (Unity Tools: Q, W, E, R, T, Y)
            let btn_size = egui::vec2(28.0, 24.0);
            let mut tool_btn = |ui: &mut egui::Ui, mode: GizmoMode, icon: &str, tip: &str| {
                let selected = self.viewport.gizmo_mode == mode;
                let resp = ui.add_sized(
                    btn_size,
                    egui::Button::new(egui::RichText::new(icon).size(15.0)).selected(selected),
                );
                if resp.clicked() {
                    self.viewport.gizmo_mode = mode;
                }
                resp.on_hover_text(tip);
            };

            tool_btn(ui, GizmoMode::View, "✋", "Herramienta Mano [Q]\nNavegar y desplazar vista (Pan libre)");
            tool_btn(ui, GizmoMode::Translate, "✥", "Herramienta Mover [W]\nTrasladar objeto en ejes X, Y, Z");
            tool_btn(ui, GizmoMode::Rotate, "↻", "Herramienta Rotar [E]\nRotar objeto con anillos orbitales");
            tool_btn(ui, GizmoMode::Scale, "⤢", "Herramienta Escalar [R]\nEscalar geometría 3D");
            tool_btn(ui, GizmoMode::Rect, "⊡", "Herramienta Rect [T]\nEdición rectangular 2D / UI");
            tool_btn(ui, GizmoMode::Transform, "✦", "Herramienta Transform [Y]\nMover, rotar y escalar combinado");

            ui.separator();

            // 2. Modos de Pivote y Coordenadas estilo Unity
            let pivot_label = match self.viewport.pivot_mode {
                PivotMode::Pivot => "🌐 Pivot",
                PivotMode::Center => "🌐 Center",
            };
            if ui.button(pivot_label).on_hover_text("Alternar punto de pivote (Pivot / Center)").clicked() {
                self.viewport.pivot_mode = match self.viewport.pivot_mode {
                    PivotMode::Pivot => PivotMode::Center,
                    PivotMode::Center => PivotMode::Pivot,
                };
            }

            let space_label = match self.viewport.coord_space {
                CoordSpace::Global => "🧭 Global",
                CoordSpace::Local => "🧭 Local",
            };
            if ui.button(space_label).on_hover_text("Alternar espacio de coordenadas (Global / Local)").clicked() {
                self.viewport.coord_space = match self.viewport.coord_space {
                    CoordSpace::Global => CoordSpace::Local,
                    CoordSpace::Local => CoordSpace::Global,
                };
            }

            ui.separator();

            // 2b. Cámara de escena. «Centrar» se queda aquí; la rejilla
            // se controla SOLO desde el menú «Gizmos» de la vista de
            // escena (un solo sitio manda, orden del usuario 2026-11-18).
            if ui.button("⌖ Centrar").on_hover_text("Centrar cámara de escena").clicked() {
                self.viewport.editor_cam_center = [3.5, 1.5, 0.0];
                self.viewport.editor_cam_dist = 10.0;
                self.viewport.editor_cam_pitch = 0.25;
                self.viewport.editor_cam_yaw = 0.0;
            }

            ui.separator();

            // 3. Controles de Play / Pause / Stop en el centro
            let is_playing = self.viewport.play_state == PlayState::Playing;
            let is_paused = self.viewport.play_state == PlayState::Paused;
            let is_edit = self.viewport.play_state == PlayState::Edit;

            let play_text = if is_playing {
                egui::RichText::new("▶ JUGANDO").color(egui::Color32::from_rgb(20, 240, 100)).strong()
            } else {
                egui::RichText::new("▶ Jugar")
            };
            if ui.selectable_label(is_playing, play_text).on_hover_text("Iniciar simulación del juego").clicked() {
                self.viewport.play_state = PlayState::Playing;
            }

            let pause_text = if is_paused {
                egui::RichText::new("⏸ PAUSADO").color(egui::Color32::from_rgb(255, 200, 40)).strong()
            } else {
                egui::RichText::new("⏸ Pausar")
            };
            if ui.selectable_label(is_paused, pause_text).on_hover_text("Pausar / Reanudar la simulación").clicked() {
                if is_playing {
                    self.viewport.play_state = PlayState::Paused;
                } else if is_paused {
                    self.viewport.play_state = PlayState::Playing;
                }
            }

            let stop_text = if is_edit {
                egui::RichText::new("⏹ Detenido").color(egui::Color32::GRAY)
            } else {
                egui::RichText::new("⏹ Detener").color(egui::Color32::LIGHT_RED).strong()
            };
            if ui.button(stop_text).on_hover_text("Detener simulación y reiniciar jugador").clicked() {
                self.viewport.play_state = PlayState::Edit;
            }

            // 4. Selector de Diseños (Layouts) y Resolución a la derecha
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("📐 Diseño ▾", |ui| {
                    if ui.button("Default (Estándar Unity)").clicked() {
                        self.apply_preset(LayoutPreset::Default);
                        ui.close();
                    }
                    if ui.button("2 by 3 (Dos Columnas)").clicked() {
                        self.apply_preset(LayoutPreset::TwoByThree);
                        ui.close();
                    }
                    if ui.button("4 Split (Cuatro Cuadrantes)").clicked() {
                        self.apply_preset(LayoutPreset::FourSplit);
                        ui.close();
                    }
                    if ui.button("Wide (Panorámico)").clicked() {
                        self.apply_preset(LayoutPreset::Wide);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("🔄 Restablecer Diseño").clicked() {
                        self.apply_preset(LayoutPreset::Default);
                        ui.close();
                    }
                });

                ui.separator();
                ui.label(format!("📐 {:.0}x{:.0}", self.viewport.width, self.viewport.height));
            });
        });
        }

        ui.separator();

        // --- ÁREA CENTRAL DE DOCKING DINÁMICO ---
        // Abre Canvas / UI al crear un Canvas desde la jerarquía.
        if self.hierarchy.abrir_panel_canvas {
            self.hierarchy.abrir_panel_canvas = false;
            self.set_tab_visible(EditorTab::CanvasUi, true);
        }

        let mut tab_viewer = EditorTabViewer {
            hierarchy: &mut self.hierarchy,
            viewport: &mut self.viewport,
            inspector: &mut self.inspector,
            console: &mut self.console,
            project: &mut self.project,
            shader_graph: &mut self.shader_graph,
            mesh_tool: &mut self.mesh_tool,
            canvas_ui: &mut self.canvas_ui,

            scene_texture_id,
            game_texture_id,
            selected_entity_id: &mut selected_id,
            scene_state,
        };

        if espacio == EspacioTrabajo::Herramientas {
            // Espacio de herramientas: docking es el anfitrión (lista del
            // registro a la izquierda, página de la activa a la derecha) y
            // el dock del editor ni se dibuja (su estado queda intacto
            // para la vuelta). Las páginas ya no son cuerpos de trabajo:
            // MeshForge trabaja sobre la escena y su página es una tarjeta
            // de estado con el salto a su panel. Todo lo demás de este
            // render (atajos, pendientes, diálogos) corre igual en los
            // dos espacios.
            let resto = ui.available_rect_before_wrap();
            let ancho_lista = 208.0_f32.min(resto.width() * 0.30);
            let gap = 6.0_f32;
            let rect_lista =
                egui::Rect::from_min_size(resto.min, egui::vec2(ancho_lista, resto.height()));
            let rect_pagina = egui::Rect::from_min_max(
                egui::pos2(rect_lista.max.x + gap, resto.min.y),
                resto.max,
            );
            let separador = ui.visuals().widgets.noninteractive.bg_stroke;
            ui.painter().vline(
                rect_lista.max.x + gap * 0.5,
                resto.min.y..=resto.max.y,
                separador,
            );
            let layout_col = egui::Layout::top_down(egui::Align::Min);
            let mut ui_lista =
                ui.new_child(egui::UiBuilder::new().max_rect(rect_lista).layout(layout_col));
            ui_lista.weak("Herramientas");
            for herramienta in &disponibles {
                if ui_lista
                    .selectable_label(
                        herramienta.id == self.herramienta_activa.as_str(),
                        format!("{} {}", herramienta.icono, herramienta.nombre),
                    )
                    .clicked()
                {
                    self.herramienta_activa = herramienta.id.to_string();
                }
            }
            if disponibles.len() < 2 {
                ui_lista.weak("Más herramientas en camino");
            }
            let mut ui_pagina =
                ui.new_child(egui::UiBuilder::new().max_rect(rect_pagina).layout(layout_col));
            match self.herramienta_activa.as_str() {
                "meshforge" => self.mesh_tool.render_espacio(&mut ui_pagina, scene_state),
                _ => {
                    ui_pagina.weak("Esta herramienta aún no tiene página.");
                }
            }
            // «Trabajar en la escena» de la tarjeta: salta al Editor y
            // muestra el panel de MeshForge.
            if self.mesh_tool.peticion_trabajar_escena {
                self.mesh_tool.peticion_trabajar_escena = false;
                self.espacio = EspacioTrabajo::Editor;
                self.set_tab_visible(EditorTab::MeshForge, true);
            }
        } else {
            DockArea::new(&mut self.dock_state)
                .show_inside(ui, &mut tab_viewer);
        }

        // --- PENDIENTES DEL PANEL «PROYECTO» ---
        // El ProjectPanel no carga escenas ni instancia prefabs por su
        // cuenta: deja la ruta en un campo público y aquí se aplica, con
        // la misma vía que usan el menú de escenas y el menú
        // Assets → Instantiate Prefab.
        if let Some(ruta) = self.project.pending_scene_load.take() {
            let archivo = ruta
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let nombre = archivo
                .strip_suffix(".scene.json")
                .unwrap_or(archivo.as_str())
                .to_string();
            let mensaje = match scene_manager.lock() {
                Ok(mut m) => match m.request_load(&nombre) {
                    Ok(key) => format!("Carga encolada: {key} (se aplica al final del frame)"),
                    Err(e) => format!("No se pudo cargar '{}': {e}", ruta.display()),
                },
                Err(_) => "SceneManager bloqueado por un hilo en pánico".to_string(),
            };
            self.status_message = Some((mensaje, std::time::Instant::now()));
        }

        if let Some(ruta) = self.project.pending_prefab_instantiate.take() {
            let mensaje = match glass_core::Prefab::load_from_file(&ruta) {
                Ok(prefab) => match scene_state.lock() {
                    Ok(mut state) => {
                        let id = state.instantiate_prefab(&prefab, None, Some([0.0, 1.0, 0.0]));
                        self.hierarchy.selected_entity = Some(id);
                        format!("Prefab instanciado: {} (entidad {id})", ruta.display())
                    }
                    Err(_) => "SceneState bloqueado; no se instanció nada".to_string(),
                },
                Err(e) => format!("No se pudo cargar el prefab '{}': {e}", ruta.display()),
            };
            self.status_message = Some((mensaje, std::time::Instant::now()));
        }

        // --- PENDIENTE DE MeshForge: ASIGNAR EL MODELO HORNEADO ---
        // Al «Hornear y salir», el panel deja la entidad editada y el
        // modelo horneado; aquí se aplica sobre la escena. Si la
        // asignación adapta el collider, el mesh collider se reconstruye
        // desde la nueva geometría por la vía que el motor ya usa al
        // cargar el modelo de la entidad.
        if let Some(asignacion) = self.mesh_tool.pending_model_assign.take() {
            let mensaje = match scene_state.lock() {
                Ok(mut sc) => {
                    let mut tocadas = 0usize;
                    for e in sc.entities.iter_mut() {
                        if e.id == asignacion.entity_id {
                            if let Some(m) = e.mesh.as_mut() {
                                m.model = asignacion.new_model.clone();
                                tocadas += 1;
                            }
                        }
                    }
                    if asignacion.adapta_collider {
                        format!(
                            "Mesh collider adaptado al nuevo modelo en {tocadas} entidad(es)"
                        )
                    } else {
                        format!(
                            "Modelo actualizado a {} en {tocadas} entidad(es)",
                            asignacion.new_model
                        )
                    }
                }
                Err(_) => "SceneState bloqueado; no se asignó el modelo horneado".to_string(),
            };
            self.status_message = Some((mensaje, std::time::Instant::now()));
        }

        // Sincronizar la selección ÚNICA del editor: vale el
        // cambio hecho este frame, venga del panel Hierarchy
        // (que escribe en su propio campo) o de la vista 3D /
        // el Inspector (que escriben en `selected_id`). Antes
        // esto sobrescribía siempre con `selected_id` y la
        // selección hecha clicando en el Hierarchy no persistía.
        if self.hierarchy.selected_entity != selected_id {
            if self.hierarchy.selected_entity == seleccion_inicial {
                // La vista 3D / el Inspector cambiaron la
                // selección: el panel adopta el valor compartido.
                // (Si la cambió el panel, su campo ya vale lo
                // correcto y `selected_id` se reinicializa de él
                // al empezar el siguiente frame.)
                self.hierarchy.selected_entity = selected_id;
            }
            ui.ctx().request_repaint();
        }

        // --- VIGÍA DE DESHACER (módulo `undo`) ---
        // Si cambió el nombre de la escena activa, la app aplicó
        // una carga (menús o panel Project): historial nuevo con
        // la escena cargada como base.
        let nombre_escena = scene_manager
            .lock()
            .map(|m| m.active_scene_name().to_string())
            .unwrap_or_default();
        if nombre_escena != self.undo_escena {
            self.undo_escena = nombre_escena.clone();
            self.cambio_escena = Some(nombre_escena);
            if let Ok(sc) = scene_state.lock() {
                self.undo_history.reset(&sc.entities, sc.next_entity_id);
                self.hash_guardado = crate::undo::scene_hash(&sc.entities, sc.next_entity_id);
            }
        }
        if self.viewport.play_state != PlayState::Edit {
            // En Play el runtime muta la escena: el historial se
            // limpia al entrar y queda en pausa hasta volver.
            if !self.undo_suspendido {
                self.undo_history = crate::undo::UndoHistory::new();
                self.undo_suspendido = true;
            }
        } else {
            self.undo_suspendido = false;
            let editando = ui.ctx().memory(|m| m.focused().is_some())
                || ui.input(|i| i.pointer.any_down());
            if let Ok(sc) = scene_state.lock() {
                self.undo_history
                    .observe(&sc.entities, sc.next_entity_id, editando);
            }
        }

        // --- DIÁLOGOS MODALES ESTILO UNITY (Project Settings y Build Settings) ---
        if self.show_project_settings_dialog {
            let mut open = true;
            egui::Window::new("⚙ Project Settings")
                .open(&mut open)
                .resizable(false)
                .default_width(360.0)
                .show(ui.ctx(), |ui| {
                    ui.heading("Configuración del Proyecto");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Product Name:  ");
                        ui.text_edit_singleline(&mut self.project_settings.name);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Company Name:  ");
                        ui.text_edit_singleline(&mut self.project_settings.company_name);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Version:       ");
                        ui.text_edit_singleline(&mut self.project_settings.version);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Default Scene: ");
                        ui.text_edit_singleline(&mut self.project_settings.default_scene);
                    });
                    self.input_manager_section(ui);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("💾 Guardar").clicked() {
                            let _ = self.project_settings.save_to_file("project.json");
                            self.status_message = Some(("project.json actualizado".into(), std::time::Instant::now()));
                            self.show_project_settings_dialog = false;
                        }
                        if ui.button("Cerrar").clicked() {
                            self.show_project_settings_dialog = false;
                        }
                    });
                });
            if !open {
                self.show_project_settings_dialog = false;
            }
        }

        if self.show_save_as_dialog {
            let mut open = true;
            egui::Window::new("💾 Save Scene As")
                .open(&mut open)
                .resizable(false)
                .default_width(380.0)
                .show(ui.ctx(), |ui| {
                    ui.heading("Guardar escena como");
                    ui.add_space(6.0);
                    ui.label("Se escribe en assets/scenes/<nombre>.scene.json");
                    ui.horizontal(|ui| {
                        ui.label("Nombre:  ");
                        ui.text_edit_singleline(&mut self.save_as_name);
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("💾 Guardar").clicked() {
                            let nombre = self.save_as_name.clone();
                            self.save_scene_to(scene_state, scene_manager, Some(&nombre));
                            self.show_save_as_dialog = false;
                        }
                        if ui.button("Cancelar").clicked() {
                            self.show_save_as_dialog = false;
                        }
                    });
                });
            if !open {
                self.show_save_as_dialog = false;
            }
        }

        if self.show_preferences_dialog {
            let mut open = true;
            let mut cerrar = false;
            egui::Window::new("⚙ Preferencias del editor")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    let cambiado = ui
                        .horizontal(|ui| {
                            ui.label("Autoguardado cada");
                            ui.add(
                                egui::DragValue::new(&mut self.autosave_minutos)
                                    .range(0..=120)
                                    .speed(1.0)
                                    .suffix(" min"),
                            )
                            .changed()
                        })
                        .inner;
                    ui.label("0 = desactivado. Por defecto, 5. Guarda la escena abierta cuando tiene cambios.");
                    if cambiado {
                        // Cargar-modificar-guardar FRESCO: el almacén también
                        // guarda proyectos e instalaciones.
                        let mut store = crate::updates::LauncherStore::load();
                        store.autosave_minutes = self.autosave_minutos;
                        if let Err(e) = store.save() {
                            self.status_message = Some((
                                format!("No se pudieron guardar las preferencias: {e:?}"),
                                std::time::Instant::now(),
                            ));
                        }
                    }
                    ui.separator();
                    if ui.button("Cerrar").clicked() {
                        cerrar = true;
                    }
                });
            if !open || cerrar {
                self.show_preferences_dialog = false;
            }
        }

        if self.dialogo_guardar_cierre {
            egui::Window::new("⚠ Cambios sin guardar")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.label("La escena tiene cambios sin guardar. ¿Quieres guardarlos antes de salir del editor?");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("💾 Guardar").clicked() {
                            self.decision_cierre = Some(DecisionCierre::Guardar);
                            self.dialogo_guardar_cierre = false;
                        }
                        if ui.button("No guardar").clicked() {
                            self.decision_cierre = Some(DecisionCierre::NoGuardar);
                            self.dialogo_guardar_cierre = false;
                        }
                        if ui.button("Cancelar").clicked() {
                            self.decision_cierre = Some(DecisionCierre::Cancelar);
                            self.dialogo_guardar_cierre = false;
                        }
                    });
                });
        }

        if self.request_build_settings {
            let mut open = true;
            egui::Window::new("🚀 Build Settings")
                .open(&mut open)
                .resizable(false)
                .default_width(400.0)
                .show(ui.ctx(), |ui| {
                    ui.heading("Ajustes de Compilación Standalone");
                    ui.add_space(4.0);
                    ui.label("Plataforma Destino: Linux x86_64 (Vulkan Nativo)");
                    ui.label("Arquitectura:       64-bit Clr .NET 8");
                    ui.separator();
                    ui.label(egui::RichText::new("Escenas en el Build:").strong());
                    for sc_path in &self.project_settings.scenes_in_build {
                        ui.label(format!("  ☑ {}", sc_path));
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("🚀 Build & Run").strong().color(egui::Color32::from_rgb(80, 220, 120))).clicked() {
                            self.request_build_and_run = true;
                            self.request_build_settings = false;
                        }
                        if ui.button("Cerrar").clicked() {
                            self.request_build_settings = false;
                        }
                    });
                });
            if !open {
                self.request_build_settings = false;
            }
        }
    }

    /// Dibuja la sección del Input Manager dentro del diálogo de Project Settings.
        pub fn input_manager_section(&mut self, ui: &mut egui::Ui) {
            ui.separator();
            ui.heading("🎮 Input Manager");
            ui.label(
                egui::RichText::new(
                    "Ejes y botones de assets/input_axes.json. Guardar aplica los cambios al momento, también en Play.",
                )
                .small(),
            );

            // Carga perezosa de la copia editable desde disco (o valores por defecto)
            if self.input_axes_edit.is_none() {
                #[derive(serde::Deserialize)]
                struct InputAxesFile {
                    axes: Vec<glass_scripting::input_axes::InputAxisDef>,
                }
                let cargados = std::fs::read_to_string("assets/input_axes.json")
                    .ok()
                    .and_then(|txt| serde_json::from_str::<InputAxesFile>(&txt).ok())
                    .map(|f| f.axes)
                    .filter(|ejes| !ejes.is_empty());
                self.input_axes_edit =
                    Some(cargados.unwrap_or_else(glass_scripting::input_axes::default_input_axes));
            }

            // Índice a borrar tras dibujar la lista (evita préstamos dobles)
            let mut borrar: Option<usize> = None;

            {
                let ejes = self.input_axes_edit.as_mut().unwrap();
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        for (i, eje) in ejes.iter_mut().enumerate() {
                            egui::CollapsingHeader::new(format!("{} · {}", i, eje.name))
                                .id_salt(format!("eje_{i}"))
                                .show(ui, |ui| {
                                    // Nombre + borrar
                                    ui.horizontal(|ui| {
                                        ui.label("Name:");
                                        ui.text_edit_singleline(&mut eje.name);
                                        if ui.button("🗑").clicked() {
                                            borrar = Some(i);
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Positive:");
                                        ui.text_edit_singleline(&mut eje.positive);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Negative:");
                                        ui.text_edit_singleline(&mut eje.negative);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Alt Positive:");
                                        ui.text_edit_singleline(&mut eje.alt_positive);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Alt Negative:");
                                        ui.text_edit_singleline(&mut eje.alt_negative);
                                    });
                                    // Valores numéricos
                                    ui.horizontal(|ui| {
                                        ui.label("Gravity:");
                                        ui.add(
                                            egui::DragValue::new(&mut eje.gravity)
                                                .speed(0.05)
                                                .range(0.0..=10000.0),
                                        );
                                        ui.label("Sensitivity:");
                                        ui.add(
                                            egui::DragValue::new(&mut eje.sensitivity)
                                                .speed(0.05)
                                                .range(0.0..=10000.0),
                                        );
                                        ui.label("Dead:");
                                        ui.add(
                                            egui::DragValue::new(&mut eje.dead)
                                                .speed(0.001)
                                                .range(0.0..=1.0),
                                        );
                                    });
                                    // Flags
                                    ui.horizontal(|ui| {
                                        ui.checkbox(&mut eje.snap, "Snap");
                                        ui.checkbox(&mut eje.invert, "Invert");
                                    });
                                });
                        }
                    });
            }

            // Aplicar borrado pendiente tras el bucle
            if let Some(i) = borrar {
                if let Some(ejes) = self.input_axes_edit.as_mut() {
                    if i < ejes.len() {
                        ejes.remove(i);
                    }
                }
            }

            // Botonera inferior: añadir / guardar
            ui.horizontal(|ui| {
                if ui.button("➕ Añadir eje").clicked() {
                    if let Some(ejes) = self.input_axes_edit.as_mut() {
                        ejes.push(glass_scripting::input_axes::InputAxisDef::button(
                            "NuevoEje",
                            "space",
                            "",
                        ));
                    }
                }
                if ui.button("💾 Guardar ejes").clicked() {
                    #[derive(serde::Serialize)]
                    struct InputAxesOut<'a> {
                        axes: &'a [glass_scripting::input_axes::InputAxisDef],
                    }
                    let resultado: Result<(), String> = (|| {
                        let ejes = self.input_axes_edit.as_ref().ok_or("sin ejes")?;
                        std::fs::create_dir_all("assets").map_err(|e| e.to_string())?;
                        let out = InputAxesOut { axes: ejes.as_slice() };
                        let txt =
                            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
                        std::fs::write("assets/input_axes.json", txt).map_err(|e| e.to_string())?;
                        Ok(())
                    })();
                    self.status_message = Some((
                        match resultado {
                            Ok(_) => "Input Manager: ejes guardados".to_string(),
                            Err(_) => "Input Manager: ERROR guardando ejes".to_string(),
                        },
                        std::time::Instant::now(),
                    ));
                }
            });
        }
}

struct EditorTabViewer<'a> {
    hierarchy: &'a mut HierarchyPanel,
    viewport: &'a mut ViewportPanel,
    inspector: &'a mut InspectorPanel,
    console: &'a mut ConsolePanel,
    project: &'a mut ProjectPanel,
    shader_graph: &'a mut ShaderGraphEditorPanel,
    mesh_tool: &'a mut MeshToolPanel,
    canvas_ui: &'a mut CanvasPanel,

    scene_texture_id: Option<egui::TextureId>,
    game_texture_id: Option<egui::TextureId>,
    selected_entity_id: &'a mut Option<u64>,
    scene_state: &'a Arc<Mutex<SceneState>>,
}

impl<'a> TabViewer for EditorTabViewer<'a> {
    type Tab = EditorTab;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            EditorTab::Hierarchy => "🌿 Jerarquía".into(),
            EditorTab::SceneView => "🎬 Escena (Scene)".into(),
            EditorTab::GameView => "🎮 Juego (Game)".into(),
            EditorTab::Inspector => "🔍 Inspector".into(),
            EditorTab::Console => "📟 Consola".into(),
            EditorTab::Project => "📁 Proyecto / Assets".into(),
            EditorTab::ShaderGraph => "🎨 Grafo de Shaders".into(),
            EditorTab::MeshForge => "🧊 MeshForge".into(),
            EditorTab::CanvasUi => "🖼 Canvas / UI".into(),

        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            EditorTab::Hierarchy => {
                self.hierarchy.render(ui, self.scene_state);
            }
            EditorTab::SceneView => {
                // Con sesión de malla activa, el viewport recibe el panel
                // y la entidad en edición: dibuja el overlay y enruta la
                // entrada a la malla (R2). Sin sesión, None (flujo normal).
                let objetivo = self.mesh_tool.target_entity;
                let mesh = match (self.mesh_tool.session.is_some(), objetivo) {
                    (true, Some(id)) => Some((&mut *self.mesh_tool, id)),
                    _ => None,
                };
                self.viewport.render_scene_tab(ui, self.scene_texture_id, self.game_texture_id, self.selected_entity_id, self.scene_state, mesh);
            }
            EditorTab::GameView => {
                self.viewport.render_game_tab(ui, self.game_texture_id, self.scene_state);
            }
            EditorTab::Inspector => {
                let mesh_bounds = &self.viewport.mesh_bounds;
                self.inspector.render(ui, *self.selected_entity_id, self.scene_state, mesh_bounds);
            }
            EditorTab::Console => {
                self.console.render_console_only(ui);
            }
            EditorTab::Project => {
                // El panel gestiona su propio estado de navegación; sólo deja
                // en `pending_*` lo que el editor debe aplicar fuera de él.
                self.project.render(ui);
            }
            EditorTab::ShaderGraph => {
                self.shader_graph.render(ui);
            }
            EditorTab::MeshForge => {
                // El panel edita la malla de la entidad seleccionada: se
                // le pasa su foto (nombre, modelo, mesh collider) leída
                // de la escena con un bloqueo mínimo.
                let seleccion = (*self.selected_entity_id).and_then(|id| {
                    self.scene_state.lock().ok().and_then(|sc| {
                        sc.entities.iter().find(|e| e.id == id).map(|e| {
                            crate::panels::mesh_tool::EntidadSeleccionada {
                                id: e.id,
                                nombre: e.name.clone(),
                                modelo: e.mesh.as_ref().map(|m| m.model.clone()),
                                tiene_mesh_collider: e.physics.as_ref().is_some_and(|p| {
                                    p.mesh_collider
                                        != glass_core::scene::MeshColliderMode::None
                                }),
                            }
                        })
                    })
                });
                self.mesh_tool
                    .render_panel(ui, self.scene_state, seleccion.as_ref());
            }
            EditorTab::CanvasUi => {
                self.canvas_ui
                    .render(ui, self.scene_state, self.selected_entity_id);
            }
        }
    }

    fn is_closeable(&self, _tab: &Self::Tab) -> bool {
        // Todos los paneles se cierran por su X (estilo Unity); el
        // menú Window es el camino de vuelta (set_tab_visible).
        true
    }

}

#[cfg(test)]
mod tests_puerta_herramienta_3d {
    #[test]
    fn paneles_se_muestran_y_se_ocultan_desde_window() {
        let mut layout = super::EditorDockingLayout::new();
        // La jerarquía nace en el dock; se oculta y se vuelve a mostrar.
        assert!(layout.tab_visible(&super::EditorTab::Hierarchy));
        layout.set_tab_visible(super::EditorTab::Hierarchy, false);
        assert!(!layout.tab_visible(&super::EditorTab::Hierarchy));
        layout.set_tab_visible(super::EditorTab::Hierarchy, true);
        assert!(layout.tab_visible(&super::EditorTab::Hierarchy));
        // Idempotente: mostrar dos veces no duplica la pestaña.
        layout.set_tab_visible(super::EditorTab::Hierarchy, true);
        layout.set_tab_visible(super::EditorTab::Hierarchy, false);
        assert!(!layout.tab_visible(&super::EditorTab::Hierarchy));
    }

    #[test]
    fn panel_meshforge_se_muestra_y_se_oculta_desde_window() {
        let mut layout = super::EditorDockingLayout::new();

        // Nace oculto: solo entra en el dock cuando Window lo pide.
        assert!(layout.dock_state.find_tab(&super::EditorTab::MeshForge).is_none());

        layout.set_tab_visible(super::EditorTab::MeshForge, true);
        assert!(layout.dock_state.find_tab(&super::EditorTab::MeshForge).is_some());

        // Mostrarlo otra vez no añade una segunda pestaña.
        layout.set_tab_visible(super::EditorTab::MeshForge, true);
        let apariciones = layout
            .dock_state
            .iter_all_tabs()
            .filter(|(_, tab)| **tab == super::EditorTab::MeshForge)
            .count();
        assert_eq!(apariciones, 1);

        layout.set_tab_visible(super::EditorTab::MeshForge, false);
        assert!(layout.dock_state.find_tab(&super::EditorTab::MeshForge).is_none());
    }

    #[test]
    fn canvas_ui_se_muestra_y_se_oculta_desde_window() {
        let mut layout = super::EditorDockingLayout::new();
        let tab = super::EditorTab::CanvasUi;

        assert!(!layout.tab_visible(&tab));

        layout.set_tab_visible(tab, true);
        assert!(layout.tab_visible(&super::EditorTab::CanvasUi));

        layout.set_tab_visible(super::EditorTab::CanvasUi, false);
        assert!(!layout.tab_visible(&super::EditorTab::CanvasUi));
    }

    #[test]
    fn espacio_herramientas_exige_porton() {
        let mut layout = super::EditorDockingLayout::new();
        assert_eq!(layout.espacio, super::EspacioTrabajo::Editor);
        layout.espacio = super::EspacioTrabajo::Herramientas;
        // Sin portón, el espacio efectivo cae al Editor; con portón, manda.
        assert_eq!(layout.espacio_efectivo(false), super::EspacioTrabajo::Editor);
        assert_eq!(layout.espacio_efectivo(true), super::EspacioTrabajo::Herramientas);
    }

    #[test]
    fn cambiar_de_espacio_conserva_el_dock_del_editor() {
        let mut layout = super::EditorDockingLayout::new();
        assert!(layout.dock_state.find_tab(&super::EditorTab::Hierarchy).is_some());
        layout.espacio = super::EspacioTrabajo::Herramientas;
        layout.espacio = super::EspacioTrabajo::Editor;
        // El dock del editor queda como estaba: ni se reaplicó el preset
        // ni se perdió ninguna pestaña por el camino.
        assert!(layout.dock_state.find_tab(&super::EditorTab::Hierarchy).is_some());
        assert!(layout.dock_state.find_tab(&super::EditorTab::Project).is_some());
    }
}

#[cfg(test)]
mod tests_guardado {
    use super::*;

    #[test]
    fn ruta_directa_si_coincide_nombre() {
        let r = Some(std::path::PathBuf::from("/p/a.scene.json"));
        assert_eq!(
            EditorDockingLayout::ruta_guardado_directa(&r, "a", "a"),
            r
        );
    }

    #[test]
    fn sin_ruta_directa_si_cambia_nombre_o_vacio() {
        let r = Some(std::path::PathBuf::from("/p/a.scene.json"));
        assert_eq!(
            EditorDockingLayout::ruta_guardado_directa(&r, "a", "b"),
            None
        );
        assert_eq!(
            EditorDockingLayout::ruta_guardado_directa(&r, "", ""),
            None
        );
        assert_eq!(
            EditorDockingLayout::ruta_guardado_directa(&None, "a", "a"),
            None
        );
    }

    #[test]
    fn suciedad_por_contenido() {
        let mut layout = EditorDockingLayout::new();
        let mut escena = SceneState::new();

        layout.marcar_guardada(&escena);
        assert!(!layout.escena_sucia(&escena));

        escena.next_entity_id += 1;
        assert!(layout.escena_sucia(&escena));

        // Volver al contenido guardado la limpia sola.
        escena.next_entity_id -= 1;
        assert!(!layout.escena_sucia(&escena));

        escena.next_entity_id += 1;
        layout.marcar_guardada(&escena);
        assert!(!layout.escena_sucia(&escena));
    }
}

#[cfg(test)]
mod tests_supr {
    use super::{destino_supr, SuprDestino, ToqueSupr};

    #[test]
    fn asset_tocado_y_vivo_va_al_asset() {
        assert_eq!(destino_supr(ToqueSupr::Asset, true, false), SuprDestino::Asset);
        // Aunque también haya entidad: manda lo último tocado.
        assert_eq!(destino_supr(ToqueSupr::Asset, true, true), SuprDestino::Asset);
    }

    #[test]
    fn entidad_tocada_y_viva_va_a_la_entidad() {
        assert_eq!(destino_supr(ToqueSupr::Entidad, false, true), SuprDestino::Entidad);
        assert_eq!(destino_supr(ToqueSupr::Entidad, true, true), SuprDestino::Entidad);
    }

    #[test]
    fn deseleccion_es_lo_ultimo_tocado_y_no_hay_nada() {
        // Clic en vacío del viewport tras tocar el asset: la entidad
        // quedó deseleccionada y Supr no cae sobre el asset.
        assert_eq!(destino_supr(ToqueSupr::Entidad, true, false), SuprDestino::Nada);
        // Clic en vacío del Proyecto tras tocar la entidad.
        assert_eq!(destino_supr(ToqueSupr::Asset, false, true), SuprDestino::Nada);
    }

    #[test]
    fn sin_toque_previo_no_hay_destino() {
        assert_eq!(destino_supr(ToqueSupr::Ninguno, true, true), SuprDestino::Nada);
        assert_eq!(destino_supr(ToqueSupr::Ninguno, false, false), SuprDestino::Nada);
    }
}
