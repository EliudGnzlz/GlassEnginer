use egui::{Color32, RichText, Stroke, Ui, Vec2};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Launcher,
    Editor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTemplate {
    Platformer3D,
    PhysicsSandbox,
    EmptyScene,
}

impl ProjectTemplate {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Platformer3D => "3D Platformer (Rapier3D + CoreCLR)",
            Self::PhysicsSandbox => "3D Physics Playground (Rapier3D)",
            Self::EmptyScene => "Empty 3D Scene (Escena Vacía)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleScenePreset {
    PlatformerDemo,
    PhysicsPlayground,
    AudioLightingShowcase,
}

#[derive(Debug, Clone)]
pub enum LauncherAction {
    OpenProject(PathBuf),
    CreateProject {
        name: String,
        path: PathBuf,
        template: ProjectTemplate,
    },
    LoadSampleScene(SampleScenePreset),
    OpenDefaultScene,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherTab {
    Projects,
    Samples,
    Learn,
}

pub struct LauncherPanel {
    pub active_tab: LauncherTab,
    pub search_filter: String,
    pub show_new_modal: bool,
    pub new_name: String,
    pub new_path: String,
    pub new_template: ProjectTemplate,
    pub status_message: Option<String>,
}

impl Default for LauncherPanel {
    fn default() -> Self {
        Self {
            active_tab: LauncherTab::Samples, // Mostrar directamente ejemplos y escena actual
            search_filter: String::new(),
            show_new_modal: false,
            new_name: "MyGlassGame".into(),
            new_path: "/home/d3/Documentos/GlassProjects/MyGlassGame".into(),
            new_template: ProjectTemplate::Platformer3D,
            status_message: None,
        }
    }
}

impl LauncherPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn render(&mut self, ui: &mut Ui) -> Option<LauncherAction> {
        let mut action: Option<LauncherAction> = None;

        // Fondo y estilo general de Launcher estilo Unity Hub
        egui::Frame::new()
            .fill(Color32::from_rgb(20, 22, 28))
            .show(ui, |ui| {
                ui.add_space(20.0);

                // --- HEADER DEL LAUNCHER ---
                ui.horizontal(|ui| {
                    ui.add_space(32.0);
                    ui.label(RichText::new("🧊 GlassEngine Hub").size(26.0).strong().color(Color32::WHITE));
                    ui.add_space(10.0);
                    ui.label(RichText::new("v0.1.0 • Vulkan 1.3 • CoreCLR 8.0").size(12.0).color(Color32::from_rgb(0, 190, 255)));
                });

                ui.add_space(12.0);

                // --- BARRA DE NAVEGACIÓN DE PESTAÑAS ---
                ui.horizontal(|ui| {
                    ui.add_space(32.0);
                    ui.spacing_mut().item_spacing.x = 12.0;

                    let tab_btn = |ui: &mut Ui, tab: LauncherTab, icon: &str, label: &str, cur: LauncherTab| -> bool {
                        let selected = cur == tab;
                        let text = format!("{icon} {label}");
                        let rt = if selected {
                            RichText::new(text).size(15.0).strong().color(Color32::from_rgb(80, 210, 255))
                        } else {
                            RichText::new(text).size(15.0).color(Color32::LIGHT_GRAY)
                        };
                        ui.selectable_label(selected, rt).clicked()
                    };

                    if tab_btn(ui, LauncherTab::Samples, "✨", "Escenas de Ejemplo", self.active_tab) {
                        self.active_tab = LauncherTab::Samples;
                    }
                    if tab_btn(ui, LauncherTab::Projects, "📁", "Proyectos", self.active_tab) {
                        self.active_tab = LauncherTab::Projects;
                    }
                    if tab_btn(ui, LauncherTab::Learn, "📚", "Guía y Atajos", self.active_tab) {
                        self.active_tab = LauncherTab::Learn;
                    }
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(16.0);

                // --- CONTENIDO DE LA PESTAÑA SELECCIONADA ---
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(32.0);
                        ui.vertical(|ui| {
                            match self.active_tab {
                                LauncherTab::Samples => {
                                    self.render_samples_tab(ui, &mut action);
                                }
                                LauncherTab::Projects => {
                                    self.render_projects_tab(ui, &mut action);
                                }
                                LauncherTab::Learn => {
                                    self.render_learn_tab(ui);
                                }
                            }
                        });
                        ui.add_space(32.0);
                    });
                });
            });

        action
    }

    fn render_samples_tab(&mut self, ui: &mut Ui, action: &mut Option<LauncherAction>) {
        ui.label(RichText::new("Escenas y Plantillas de Ejemplo").size(18.0).strong().color(Color32::WHITE));
        ui.label(RichText::new("Selecciona una escena preconfigurada para abrir inmediatamente en el Editor de GlassEngine.").size(12.0).color(Color32::GRAY));
        ui.add_space(16.0);

        // Tarjeta 1: Nuestra Escena Actual de Prueba (Platformer 3D)
        Self::render_card(
            ui,
            "★ 3D Platformer & Physics Demo (Escena de Prueba)",
            "Escena completa con plataformas 3D escalables, cuerpo rígido dinámico Rapier3D, script C# PlayerController (.NET CoreCLR), audio interactivo Kira y shader PBR con luz solar.",
            "▶ Abrir Escena de Prueba",
            Color32::from_rgb(0, 160, 255),
            || {
                *action = Some(LauncherAction::LoadSampleScene(SampleScenePreset::PlatformerDemo));
            },
        );

        ui.add_space(14.0);

        // Tarjeta 2: Physics Playground (Rapier3D)
        Self::render_card(
            ui,
            "🎲 Physics Playground (Rapier3D)",
            "Simulación de múltiples cubos rígidos apilados, cajas dinámicas con colisiones Cuboid vs Cuboid, parámetros de fricción y coeficiente de restitución (rebote).",
            "▶ Cargar Física 3D",
            Color32::from_rgb(255, 140, 0),
            || {
                *action = Some(LauncherAction::LoadSampleScene(SampleScenePreset::PhysicsPlayground));
            },
        );

        ui.add_space(14.0);

        // Tarjeta 3: Audio & Dynamic Lighting Showcase
        Self::render_card(
            ui,
            "💡 Audio & Dynamic Lighting Showcase",
            "Demostración de fuentes de luz direccional y puntual con Gizmos 3D de alcance y orientación solar, acompañada de clips de audio y transformadas espaciales.",
            "▶ Cargar Iluminación & Audio",
            Color32::from_rgb(220, 200, 40),
            || {
                *action = Some(LauncherAction::LoadSampleScene(SampleScenePreset::AudioLightingShowcase));
            },
        );
    }

    fn render_projects_tab(&mut self, ui: &mut Ui, action: &mut Option<LauncherAction>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Proyectos Recientes").size(18.0).strong().color(Color32::WHITE));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(RichText::new("➕ Nuevo Proyecto").size(14.0).strong()).clicked() {
                    self.show_new_modal = !self.show_new_modal;
                }
                if ui.button(RichText::new("📂 Abrir Existente...").size(14.0)).clicked() {
                    *action = Some(LauncherAction::OpenDefaultScene);
                }
            });
        });

        ui.add_space(12.0);

        // Modal / Formulario para crear un nuevo proyecto
        if self.show_new_modal {
            egui::Frame::canvas(ui.style())
                .fill(Color32::from_rgb(28, 32, 42))
                .stroke(Stroke::new(1.0, Color32::from_rgb(0, 160, 255)))
                .corner_radius(6.0)
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Crear Nuevo Proyecto en GlassEngine").size(15.0).strong().color(Color32::WHITE));
                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                ui.label("Nombre del Proyecto:");
                                ui.text_edit_singleline(&mut self.new_name);
                            });

                            ui.horizontal(|ui| {
                                ui.label("Ruta en Disco:");
                                ui.text_edit_singleline(&mut self.new_path);
                            });

                            ui.horizontal(|ui| {
                                ui.label("Plantilla Inicial:");
                                egui::ComboBox::from_id_salt("template_combo")
                                    .selected_text(self.new_template.display_name())
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.new_template, ProjectTemplate::Platformer3D, ProjectTemplate::Platformer3D.display_name());
                                        ui.selectable_value(&mut self.new_template, ProjectTemplate::PhysicsSandbox, ProjectTemplate::PhysicsSandbox.display_name());
                                        ui.selectable_value(&mut self.new_template, ProjectTemplate::EmptyScene, ProjectTemplate::EmptyScene.display_name());
                                    });
                            });

                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.button(RichText::new("✓ Crear y Abrir en Editor").strong().color(Color32::from_rgb(80, 240, 120))).clicked() {
                                    *action = Some(LauncherAction::CreateProject {
                                        name: self.new_name.clone(),
                                        path: PathBuf::from(&self.new_path),
                                        template: self.new_template,
                                    });
                                    self.show_new_modal = false;
                                }
                                if ui.button("Cancelar").clicked() {
                                    self.show_new_modal = false;
                                }
                            });
                        });
                        ui.add_space(12.0);
                    });
                    ui.add_space(8.0);
                });
            ui.add_space(14.0);
        }

        // Proyecto predeterminado del motor
        Self::render_card(
            ui,
            "📁 GlassEngine Workspace Demo",
            "/home/d3/Documentos/GlassEnginer (Proyecto Activo • Vulkan Offscreen Multi-View)",
            "▶ Abrir en Editor",
            Color32::from_rgb(0, 200, 160),
            || {
                *action = Some(LauncherAction::OpenDefaultScene);
            },
        );
    }

    fn render_learn_tab(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Guía de Inicio Rápido & Atajos de GlassEngine").size(18.0).strong().color(Color32::WHITE));
        ui.add_space(12.0);

        egui::Frame::canvas(ui.style())
            .fill(Color32::from_rgb(26, 28, 36))
            .corner_radius(6.0)
            .show(ui, |ui| {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("🎮 Atajos en Ventana de Escena (Scene View)").size(15.0).strong().color(Color32::from_rgb(0, 190, 255)));
                        ui.label("• [ Q ] Herramienta Mano: Desplazamiento libre (Pan) con clic y arrastre.");
                        ui.label("• [ W ] Herramienta Mover: Gizmo 3D de traslación en ejes X, Y, Z.");
                        ui.label("• [ E ] Herramienta Rotar: Anillos orbitales 3D para orientar el GameObject.");
                        ui.label("• [ R ] Herramienta Escalar: Cubos ancla en extremos para redimensionar mallas.");
                        ui.label("• [ Clic Izquierdo ]: Selección precisa de GameObjects, Cámaras y Luces mediante Raycasting.");
                        ui.label("• [ Clic Derecho ]: Órbita 3D de la cámara de edición.");
                        ui.label("• [ Rueda del Ratón ]: Zoom fluido hacia el punto de enfoque.");
                        ui.label("• [ Delete ]: Eliminar el GameObject seleccionado.");

                        ui.add_space(10.0);
                        ui.label(RichText::new("🌿 Jerarquía estilo Unity (Hierarchy)").size(15.0).strong().color(Color32::from_rgb(0, 190, 255)));
                        ui.label("• Arrastrar al centro de un objeto: Anida el objeto dentro de ese padre (Parenting).");
                        ui.label("• Arrastrar al borde superior / inferior: Reordena con línea azul de inserción.");
                        ui.label("• Arrastrar al fondo vacío: Desemparenta el objeto a la raíz.");
                        ui.label("• Clic derecho en cualquier fila: Menú contextual completo de creación, duplicación y emparentado.");

                        ui.add_space(10.0);
                        ui.label(RichText::new("📜 Scripting C# (.NET CoreCLR)").size(15.0).strong().color(Color32::from_rgb(0, 190, 255)));
                        ui.label("• Clases derivadas de GlassBehaviour con métodos Awake(), Update(), OnCollisionEnter().");
                        ui.label("• Variables públicas editables en vivo desde el Inspector.");
                    });
                    ui.add_space(16.0);
                });
                ui.add_space(12.0);
            });
    }

    fn render_card<F: FnOnce()>(
        ui: &mut Ui,
        title: &str,
        desc: &str,
        btn_label: &str,
        accent_color: Color32,
        on_click: F,
    ) {
        let _full_width = ui.available_width().max(300.0);
        egui::Frame::canvas(ui.style())
            .fill(Color32::from_rgb(26, 29, 38))
            .stroke(Stroke::new(1.0, Color32::from_rgb(45, 50, 65)))
            .corner_radius(6.0)
            .show(ui, |ui| {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    // Barra de color acento lateral izquierda
                    let (bar_rect, _) = ui.allocate_exact_size(Vec2::new(4.0, 56.0), egui::Sense::hover());
                    ui.painter().rect_filled(bar_rect, 2.0, accent_color);
                    ui.add_space(10.0);

                    ui.vertical(|ui| {
                        ui.label(RichText::new(title).size(15.0).strong().color(Color32::WHITE));
                        ui.add_space(4.0);
                        ui.label(RichText::new(desc).size(12.0).color(Color32::from_rgb(180, 185, 195)));
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(14.0);
                        if ui.add_sized(Vec2::new(160.0, 32.0), egui::Button::new(RichText::new(btn_label).size(13.0).strong().color(Color32::WHITE))).clicked() {
                            on_click();
                        }
                    });
                });
                ui.add_space(12.0);
            });
    }
}
