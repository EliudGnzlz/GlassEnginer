use egui::{Color32, DragValue, RichText, Slider, Ui};
use glass_core::{AudioSourceData, MeshData, PhysicsData, SceneState, ScriptComponentData};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct InspectorPanel {
    show_add_component_menu: bool,
}

impl InspectorPanel {
    pub fn new() -> Self {
        Self {
            show_add_component_menu: false,
        }
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        selected_id: Option<u64>,
        scene_state: &Arc<Mutex<SceneState>>,
    ) {
        ui.horizontal(|ui| {
            ui.heading("🔍 Inspector");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(id) = selected_id {
                    ui.label(RichText::new(format!("#{id}")).color(Color32::GRAY));
                }
            });
        });
        ui.separator();

        let mut scene = match scene_state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let entity = match selected_id.and_then(|id| scene.entities.iter_mut().find(|e| e.id == id)) {
            Some(e) => e,
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Ningún GameObject seleccionado.\nSelecciona uno en la Escena 3D o en la Jerarquía.").color(Color32::GRAY));
                });
                return;
            }
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.push_id(entity.id, |ui| {
                // ==========================================
                // 1. CABECERA DEL GAMEOBJECT ESTILO UNITY
                // ==========================================
                ui.horizontal(|ui| {
                    ui.checkbox(&mut entity.active, "");
                    let icon = if entity.components.iter().any(|c| c == "Camera") {
                        "🎥"
                    } else if entity.components.iter().any(|c| c == "Light") {
                        "💡"
                    } else if entity.components.iter().any(|c| c == "MeshRenderer") {
                        "📐"
                    } else {
                        "📦"
                    };
                    ui.label(icon);
                    ui.add_sized([130.0, 20.0], egui::TextEdit::singleline(&mut entity.name));
                    ui.checkbox(&mut entity.is_static, "Static");
                });

                ui.horizontal(|ui| {
                    ui.label("Tag:");
                    egui::ComboBox::from_id_salt(format!("tag_combo_{}", entity.id))
                        .selected_text(&entity.tag)
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut entity.tag, "Untagged".into(), "Untagged");
                            ui.selectable_value(&mut entity.tag, "Player".into(), "Player");
                            ui.selectable_value(&mut entity.tag, "MainCamera".into(), "MainCamera");
                            ui.selectable_value(&mut entity.tag, "Platform".into(), "Platform");
                            ui.selectable_value(&mut entity.tag, "Floor".into(), "Floor");
                            ui.selectable_value(&mut entity.tag, "Obstacle".into(), "Obstacle");
                        });

                    ui.label("Layer:");
                    egui::ComboBox::from_id_salt(format!("layer_combo_{}", entity.id))
                        .selected_text(&entity.layer)
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut entity.layer, "Default".into(), "Default");
                            ui.selectable_value(&mut entity.layer, "TransparentFX".into(), "TransparentFX");
                            ui.selectable_value(&mut entity.layer, "Ignore Raycast".into(), "Ignore Raycast");
                            ui.selectable_value(&mut entity.layer, "Water".into(), "Water");
                            ui.selectable_value(&mut entity.layer, "UI".into(), "UI");
                        });
                });

                ui.separator();

                // ==========================================
                // 2. COMPONENTE TRANSFORM (ESTILO UNITY)
                // ==========================================
                let mut reset_transform = false;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("📐 Transform").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("⟲ Reset").clicked() {
                            reset_transform = true;
                        }
                    });
                });

                if reset_transform {
                    entity.transform.position = [0.0, 0.0, 0.0];
                    entity.transform.rotation = [0.0, 0.0, 0.0];
                    entity.transform.scale = [1.0, 1.0, 1.0];
                }

                egui::Grid::new(format!("transform_grid_{}", entity.id))
                    .num_columns(4)
                    .spacing([6.0, 4.0])
                    .show(ui, |ui| {
                        let red = Color32::from_rgb(235, 75, 75);
                        let green = Color32::from_rgb(75, 205, 100);
                        let blue = Color32::from_rgb(70, 140, 245);

                        // Posición
                        ui.label("Posición");
                        ui.horizontal(|ui| {
                            ui.colored_label(red, "X");
                            ui.add(DragValue::new(&mut entity.transform.position[0]).speed(0.05));
                            ui.colored_label(green, "Y");
                            ui.add(DragValue::new(&mut entity.transform.position[1]).speed(0.05));
                            ui.colored_label(blue, "Z");
                            ui.add(DragValue::new(&mut entity.transform.position[2]).speed(0.05));
                        });
                        ui.end_row();

                        // Rotación
                        ui.label("Rotación");
                        ui.horizontal(|ui| {
                            ui.colored_label(red, "X");
                            ui.add(DragValue::new(&mut entity.transform.rotation[0]).speed(0.5).suffix("°"));
                            ui.colored_label(green, "Y");
                            ui.add(DragValue::new(&mut entity.transform.rotation[1]).speed(0.5).suffix("°"));
                            ui.colored_label(blue, "Z");
                            ui.add(DragValue::new(&mut entity.transform.rotation[2]).speed(0.5).suffix("°"));
                        });
                        ui.end_row();

                        // Escala
                        ui.label("Escala");
                        ui.horizontal(|ui| {
                            ui.colored_label(red, "X");
                            ui.add(DragValue::new(&mut entity.transform.scale[0]).speed(0.02));
                            ui.colored_label(green, "Y");
                            ui.add(DragValue::new(&mut entity.transform.scale[1]).speed(0.02));
                            ui.colored_label(blue, "Z");
                            ui.add(DragValue::new(&mut entity.transform.scale[2]).speed(0.02));
                        });
                        ui.end_row();
                    });

                ui.separator();

                // ==========================================
                // 3. COMPONENTE MESH RENDERER & MATERIAL
                // ==========================================
                let mut remove_mesh = false;
                if let Some(ref mut mesh) = entity.mesh {
                    ui.collapsing("🎨 Mesh Renderer & Material", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Color Albedo:");
                            ui.color_edit_button_rgb(&mut mesh.color);
                        });
                        ui.checkbox(&mut mesh.wireframe, "Modo Wireframe");
                        ui.label(format!("Shader: {}", mesh.shader));
                        ui.label("Malla: cube.glb (Cargador glTF/SPIR-V)");

                        if ui.small_button("🗑 Quitar MeshRenderer").clicked() {
                            remove_mesh = true;
                        }
                    });
                    ui.separator();
                }
                if remove_mesh {
                    entity.mesh = None;
                    entity.components.retain(|c| c != "MeshRenderer" && c != "Material");
                }

                // ==========================================
                // 4. COMPONENTE RIGIDBODY & COLLIDER (RAPIER3D)
                // ==========================================
                let mut remove_physics = false;
                if let Some(ref mut phys) = entity.physics {
                    ui.collapsing("⚙ RigidBody & Collider (Rapier3D)", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Tipo de Cuerpo:");
                            egui::ComboBox::from_id_salt(format!("phys_type_{}", entity.id))
                                .selected_text(&phys.body_type)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut phys.body_type, "Dynamic".into(), "Dynamic (Física y gravedad)");
                                    ui.selectable_value(&mut phys.body_type, "Static".into(), "Static (Inmóvil)");
                                    ui.selectable_value(&mut phys.body_type, "Kinematic".into(), "Kinematic (Controlado)");
                                });
                        });

                        ui.horizontal(|ui| {
                            ui.label("Masa:");
                            ui.add(DragValue::new(&mut phys.mass).speed(0.1).range(0.01..=500.0).suffix(" kg"));
                        });

                        ui.add(Slider::new(&mut phys.friction, 0.0..=1.0).text("Fricción"));
                        ui.add(Slider::new(&mut phys.restitution, 0.0..=1.0).text("Elasticidad (Bounciness)"));

                        ui.label(RichText::new("Colisionador Cuboide (Half-Extents):").strong());
                        ui.horizontal(|ui| {
                            ui.label("X");
                            ui.add(DragValue::new(&mut phys.box_extents[0]).speed(0.05));
                            ui.label("Y");
                            ui.add(DragValue::new(&mut phys.box_extents[1]).speed(0.05));
                            ui.label("Z");
                            ui.add(DragValue::new(&mut phys.box_extents[2]).speed(0.05));
                        });

                        ui.label(RichText::new(format!(
                            "Velocidad Lineal: ({:.2}, {:.2}, {:.2}) m/s",
                            phys.linear_velocity[0], phys.linear_velocity[1], phys.linear_velocity[2]
                        )).color(Color32::from_rgb(160, 210, 255)));

                        if ui.small_button("🗑 Quitar RigidBody").clicked() {
                            remove_physics = true;
                        }
                    });
                    ui.separator();
                }
                if remove_physics {
                    entity.physics = None;
                    entity.components.retain(|c| c != "RigidBody" && c != "Collider" && c != "StaticCollider");
                }

                // ==========================================
                // 5. COMPONENTE SCRIPT C# (.NET CORECLR)
                // ==========================================
                let mut remove_script = false;
                if let Some(ref mut script_name) = entity.script {
                    ui.collapsing("📜 Script C# (.NET CoreCLR)", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Script:");
                            ui.text_edit_singleline(script_name);
                        });

                        if let Some(ref mut script_data) = entity.script_data {
                            ui.label(RichText::new("Variables Públicas (Inspector):").strong().color(Color32::from_rgb(255, 220, 100)));

                            ui.horizontal(|ui| {
                                ui.label("Move Speed:");
                                if ui.add(DragValue::new(&mut script_data.move_speed).speed(0.1).range(0.5..=25.0).suffix(" m/s")).changed() {
                                    glass_scripting::set_script_params(glass_scripting::ScriptParamsFfi {
                                        move_speed: script_data.move_speed,
                                        jump_velocity: script_data.jump_velocity,
                                        auto_demo: script_data.auto_demo as u8,
                                    });
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.label("Jump Velocity:");
                                if ui.add(DragValue::new(&mut script_data.jump_velocity).speed(0.1).range(1.0..=30.0).suffix(" m/s")).changed() {
                                    glass_scripting::set_script_params(glass_scripting::ScriptParamsFfi {
                                        move_speed: script_data.move_speed,
                                        jump_velocity: script_data.jump_velocity,
                                        auto_demo: script_data.auto_demo as u8,
                                    });
                                }
                            });

                            if ui.checkbox(&mut script_data.auto_demo, "AutoDemo (Simulación Guiada)").changed() {
                                glass_scripting::set_script_params(glass_scripting::ScriptParamsFfi {
                                    move_speed: script_data.move_speed,
                                    jump_velocity: script_data.jump_velocity,
                                    auto_demo: script_data.auto_demo as u8,
                                });
                            }
                        }

                        ui.label(RichText::new("● Runtime .NET 8.0 CoreCLR Activo").color(Color32::from_rgb(60, 220, 110)));

                        if ui.small_button("🗑 Quitar Script").clicked() {
                            remove_script = true;
                        }
                    });
                    ui.separator();
                }
                if remove_script {
                    entity.script = None;
                    entity.script_data = None;
                    entity.components.retain(|c| c != "Script");
                }

                // ==========================================
                // 6. COMPONENTE AUDIOSOURCE (KIRA 2D/3D)
                // ==========================================
                let mut remove_audio = false;
                if let Some(ref mut audio) = entity.audio {
                    ui.collapsing("🔊 AudioSource (Kira CPAL)", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Audio Clip:");
                            egui::ComboBox::from_id_salt(format!("audio_clip_{}", entity.id))
                                .selected_text(&audio.clip_name)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut audio.clip_name, "beep_440".into(), "beep_440 (Tono grave 440 Hz)");
                                    ui.selectable_value(&mut audio.clip_name, "beep_880".into(), "beep_880 (Tono agudo 880 Hz)");
                                });
                        });

                        ui.add(Slider::new(&mut audio.volume, 0.0..=1.0).text("Volumen"));
                        ui.add(Slider::new(&mut audio.pitch, 0.1..=3.0).text("Pitch"));

                        ui.horizontal(|ui| {
                            if ui.button("▶ Probar Audio (Preescucha)").clicked() {
                                if let Some(audio_sys) = glass_audio::get_global_audio_system() {
                                    audio_sys.play_2d(&audio.clip_name, audio.volume, audio.pitch, false);
                                }
                            }
                            if ui.small_button("🗑 Quitar AudioSource").clicked() {
                                remove_audio = true;
                            }
                        });
                    });
                    ui.separator();
                }
                if remove_audio {
                    entity.audio = None;
                    entity.components.retain(|c| c != "AudioSource");
                }

                // ==========================================
                // 7. BOTÓN "AÑADIR COMPONENTE" ESTILO UNITY
                // ==========================================
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("➕ Añadir Componente").clicked() {
                        self.show_add_component_menu = !self.show_add_component_menu;
                    }
                });

                if self.show_add_component_menu {
                    ui.group(|ui| {
                        ui.label(RichText::new("Selecciona Componente a Añadir:").strong());

                        if entity.mesh.is_none() && ui.button("🎨 MeshRenderer & Material").clicked() {
                            entity.mesh = Some(MeshData::default());
                            entity.components.push("MeshRenderer".into());
                            entity.components.push("Material".into());
                            self.show_add_component_menu = false;
                        }

                        if entity.physics.is_none() && ui.button("⚙ RigidBody & Collider").clicked() {
                            entity.physics = Some(PhysicsData::default());
                            entity.components.push("RigidBody".into());
                            entity.components.push("Collider".into());
                            self.show_add_component_menu = false;
                        }

                        if entity.script.is_none() && ui.button("📜 Script C# (.NET CoreCLR)").clicked() {
                            entity.script = Some("GlassEngine.Core.Scripts.PlayerController".into());
                            entity.script_data = Some(ScriptComponentData::default());
                            entity.components.push("Script".into());
                            self.show_add_component_menu = false;
                        }

                        if entity.audio.is_none() && ui.button("🔊 AudioSource").clicked() {
                            entity.audio = Some(AudioSourceData::default());
                            entity.components.push("AudioSource".into());
                            self.show_add_component_menu = false;
                        }
                    });
                }
            });
        });
    }
}
