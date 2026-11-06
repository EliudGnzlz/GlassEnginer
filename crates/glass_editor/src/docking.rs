use crate::panels::{
    ConsolePanel, CoordSpace, GizmoMode, HierarchyPanel, InspectorPanel, PivotMode, PlayState,
    ProjectPanel, ShaderGraphEditorPanel, ViewportPanel,
};
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutPreset {
    Default,
    TwoByThree,
    FourSplit,
    Wide,
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
    pub dock_state: DockState<EditorTab>,
    pub current_preset: LayoutPreset,
    pub request_launcher: bool,
    pub request_build_and_run: bool,
    pub request_build_settings: bool,
    pub show_project_settings_dialog: bool,
/// Copia editable de los ejes del Input Manager; None = aún no cargada en este diálogo.
pub input_axes_edit: Option<Vec<glass_scripting::input_axes::InputAxisDef>>,
    pub project_settings: glass_core::ProjectSettings,
    pub status_message: Option<(String, std::time::Instant)>,
    /// Diálogo "Save Scene As" (equivalente al de Unity).
    pub show_save_as_dialog: bool,
    pub save_as_name: String,
}

impl Default for EditorDockingLayout {
    fn default() -> Self {
        Self::new()
    }

}

impl EditorDockingLayout {
    pub fn new() -> Self {
        let mut slf = Self {
            hierarchy: HierarchyPanel::new(),
            viewport: ViewportPanel::default(),
            inspector: InspectorPanel::new(),
            console: ConsolePanel::default(),
            project: ProjectPanel::new(),
            shader_graph: ShaderGraphEditorPanel::new(),
            dock_state: DockState::new(vec![EditorTab::SceneView, EditorTab::GameView]),
            current_preset: LayoutPreset::Default,
            request_launcher: false,
            request_build_and_run: false,
            request_build_settings: false,
            show_project_settings_dialog: false,
            input_axes_edit: None,
            show_save_as_dialog: false,
            save_as_name: String::new(),
            project_settings: glass_core::ProjectSettings::load_or_default("project.json"),
            status_message: None,
        };
        slf.apply_preset(LayoutPreset::Default);
        slf
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
                surface.split_below(
                    NodeIndex::root(),
                    0.72,
                    vec![EditorTab::Console, EditorTab::Project, EditorTab::ShaderGraph],
                );
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

    /// Guarda la escena activa en la ruta que dicta el `SceneManager`.
    ///
    /// Antes había tres copias del literal `"assets/scenes/SampleScene.scene.json"`,
    /// así que guardar cualquier otra escena pisaba siempre la primera y lo que
    /// se escribía en disco no coincidía con el nombre que reportaba el puente.
    /// Ahora la ruta sale de `SceneManager::save_key()`, que conoce la clave de
    /// asset de la escena cargada; `Save Scene As` sólo la deriva de un nombre.
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

        let mut selected_id = self.hierarchy.selected_entity;

        // --- 0. ATAJOS DE TECLADO GLOBALES ESTILO UNITY ---
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
                    self.viewport.request_reset_to_spawn = true;
                    PlayState::Edit
                }
            };
        }
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            self.save_scene_to(scene_state, scene_manager, None);
        }

        // --- 1. BARRA DE MENÚS PRINCIPAL ESTILO UNITY / UNREAL ---
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.menu_button("File", |ui| {
                if ui.button("📁 New Scene\tCtrl+N").clicked() {
                    if let Ok(mut sc) = scene_state.lock() {
                        sc.entities.clear();
                        sc.next_entity_id = 1;
                        sc.create_entity("Main Camera".into(), vec!["Transform".into(), "Camera".into()]);
                        sc.create_entity("Directional Light".into(), vec!["Transform".into(), "Light".into()]);
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
                if ui.button("🏠 Project Hub / Launcher").clicked() {
                    self.request_launcher = true;
                    ui.close();
                }
            });

            // Selector de escena activa, como la barra de herramientas de Unity:
            // muestra el nombre actual y permite cambiar sin abrir el menú File.
            let activa_actual = match scene_manager.lock() {
                Ok(m) => m.active_scene_name().to_string(),
                Err(_) => "?".to_string(),
            };
            ui.menu_button(format!("🎬 {activa_actual}"), |ui| {
                self.scene_switcher_menu(ui, scene_manager);
            });

            ui.menu_button("Edit", |ui| {
                let play_label = if self.viewport.play_state == PlayState::Playing { "⏹ Stop\tCtrl+P" } else { "▶ Play\tCtrl+P" };
                if ui.button(play_label).clicked() {
                    self.viewport.play_state = match self.viewport.play_state {
                        PlayState::Edit | PlayState::Paused => PlayState::Playing,
                        PlayState::Playing => {
                            self.viewport.request_reset_to_spawn = true;
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
                    let res = std::process::Command::new("dotnet")
                        .args(["build", "managed/GlassEngine.Core/GlassEngine.Core.csproj"])
                        .status();
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
                            let id = sc.create_entity("Cube".into(), vec!["Transform".into(), "MeshRenderer".into(), "RigidBody".into(), "Collider".into()]);
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                    if ui.button("● Sphere").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Sphere".into(), vec!["Transform".into(), "MeshRenderer".into(), "RigidBody".into(), "Collider".into()]);
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
                            let id = sc.create_entity("Cylinder".into(), vec!["Transform".into(), "MeshRenderer".into(), "RigidBody".into(), "Collider".into()]);
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
                                        ent.physics = Some(glass_core::PhysicsData::default());
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
            });

            ui.menu_button("Help", |ui| {
                if ui.button("📖 GlassEngine Documentation").clicked() { ui.close(); }
                if ui.button("ℹ About GlassEngine Beta").clicked() { ui.close(); }
            });

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
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            // 0. Botón de acceso al Hub / Lanzador de Proyectos
            if ui.button(egui::RichText::new("🏠 Hub").strong().color(egui::Color32::from_rgb(80, 210, 255)))
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
                self.viewport.request_reset_to_spawn = true;
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

        ui.separator();

        // --- ÁREA CENTRAL DE DOCKING DINÁMICO ---
        let mut tab_viewer = EditorTabViewer {
            hierarchy: &mut self.hierarchy,
            viewport: &mut self.viewport,
            inspector: &mut self.inspector,
            console: &mut self.console,
            project: &mut self.project,
            shader_graph: &mut self.shader_graph,
            scene_texture_id,
            game_texture_id,
            selected_entity_id: &mut selected_id,
            scene_state,
        };

        DockArea::new(&mut self.dock_state)
            .show_inside(ui, &mut tab_viewer);

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

        // Sincronizar selección de entidad de vuelta a la jerarquía
        if selected_id != self.hierarchy.selected_entity {
            self.hierarchy.selected_entity = selected_id;
            ui.ctx().request_repaint();
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
                        let total = ejes.len();
                        for i in 0..total {
                            let eje = &mut ejes[i];
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
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            EditorTab::Hierarchy => {
                self.hierarchy.render(ui, self.scene_state);
            }
            EditorTab::SceneView => {
                self.viewport.render_scene_tab(ui, self.scene_texture_id, self.game_texture_id, self.selected_entity_id, self.scene_state);
            }
            EditorTab::GameView => {
                self.viewport.render_game_tab(ui, self.game_texture_id, self.scene_state);
            }
            EditorTab::Inspector => {
                self.inspector.render(ui, *self.selected_entity_id, self.scene_state);
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
        }
    }

    fn is_closeable(&self, _tab: &Self::Tab) -> bool {
        false
    }
}
