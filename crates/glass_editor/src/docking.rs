use crate::panels::{
    ConsolePanel, CoordSpace, GizmoMode, HierarchyPanel, InspectorPanel, PivotMode, PlayState,
    ViewportPanel,
};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
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
    pub dock_state: DockState<EditorTab>,
    pub current_preset: LayoutPreset,
    pub request_launcher: bool,
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
            dock_state: DockState::new(vec![EditorTab::SceneView, EditorTab::GameView]),
            current_preset: LayoutPreset::Default,
            request_launcher: false,
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
                    vec![EditorTab::Console, EditorTab::Project],
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

    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        scene_texture_id: Option<egui::TextureId>,
        game_texture_id: Option<egui::TextureId>,
        scene_state: &Arc<Mutex<SceneState>>,
    ) {
        let mut selected_id = self.hierarchy.selected_entity;

        // --- 0. BARRA DE MENÚS PRINCIPAL ESTILO UNITY / UNREAL ---
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.menu_button("File", |ui| {
                if ui.button("📁 New Scene\tCtrl+N").clicked() {
                    if let Ok(mut sc) = scene_state.lock() {
                        sc.entities.clear();
                        sc.next_entity_id = 1;
                    }
                    ui.close();
                }
                if ui.button("💾 Save Scene\tCtrl+S").clicked() {
                    if let Ok(sc) = scene_state.lock() {
                        let _ = sc.save_to_json("assets/scenes/scene.json");
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button("🏠 Project Hub / Launcher").clicked() {
                    self.request_launcher = true;
                    ui.close();
                }
            });

            ui.menu_button("Edit", |ui| {
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
            });

            ui.menu_button("Assets", |ui| {
                if ui.button("📁 New Folder").clicked() { ui.close(); }
                if ui.button("📜 C# Script").clicked() { ui.close(); }
                if ui.button("🎨 Material (PBR)").clicked() { ui.close(); }
                ui.separator();
                if ui.button("🔄 Refresh Assets").clicked() { ui.close(); }
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
                            self.hierarchy.selected_entity = Some(id);
                        }
                        ui.close();
                    }
                    if ui.button("▬ Plane").clicked() {
                        if let Ok(mut sc) = scene_state.lock() {
                            let id = sc.create_entity("Plane".into(), vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()]);
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
            scene_texture_id,
            game_texture_id,
            selected_entity_id: &mut selected_id,
            scene_state,
        };

        DockArea::new(&mut self.dock_state)
            .show_inside(ui, &mut tab_viewer);

        // Sincronizar selección de entidad de vuelta a la jerarquía
        if selected_id != self.hierarchy.selected_entity {
            self.hierarchy.selected_entity = selected_id;
            ui.ctx().request_repaint();
        }
    }
}

struct EditorTabViewer<'a> {
    hierarchy: &'a mut HierarchyPanel,
    viewport: &'a mut ViewportPanel,
    inspector: &'a mut InspectorPanel,
    console: &'a mut ConsolePanel,
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
                self.console.render_project_only(ui);
            }
        }
    }

    fn is_closeable(&self, _tab: &Self::Tab) -> bool {
        false
    }
}
