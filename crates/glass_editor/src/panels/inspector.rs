use egui::{Color32, DragValue, RichText, Slider, Ui};
use glass_core::{
    AudioSourceData, CameraData, CanvasData, LightData, MeshData, PhysicsData,
    RectTransformData, SceneState, ScriptComponentData, UIButtonData, UITextData,
    UIImageData,
    UIDragData,
};
use std::sync::{Arc, Mutex};

/// Busca los modelos que una entidad puede usar: los nombres (sin
/// extensión) de los `.glb` bajo `assets/`, que es lo que la precarga del
/// motor sube al AssetCache con esa misma clave.
fn scan_model_names() -> Vec<String> {
    scan_model_names_in(std::path::Path::new("assets"))
}

/// Igual que `scan_model_names`, pero bajo la raíz dada (para tests).
fn scan_model_names_in(root: &std::path::Path) -> Vec<String> {
    fn walk(dir: &std::path::Path, depth: u32, out: &mut Vec<String>) {
        if depth > 8 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, depth + 1, out);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("glb"))
            {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_string());
                }
            }
        }
    }
    let mut names = Vec::new();
    walk(root, 0, &mut names);
    names.sort();
    names.dedup();
    names
}

#[derive(Default)]
pub struct InspectorPanel {
    show_add_component_menu: bool,
    /// Nombres de modelo disponibles (los .glb bajo `assets/`, sin
    /// extensión) para el selector de malla. Se cachean por entidad
    /// seleccionada y se refrescan al cambiar la selección o con el
    /// botón ↻ de la sección Mesh Renderer.
    model_names: Vec<String>,
    model_names_for: Option<u64>,
    model_names_dirty: bool,
    /// Último valor de `mesh.model` para el que ya se reescaneó la lista por
    /// no aparecer en ella; evita reescanear en cada frame cuando el modelo
    /// falta de verdad.
    rescan_for_model: Option<String>,
}

impl InspectorPanel {
    pub fn new() -> Self {
        Self {
            show_add_component_menu: false,
            model_names: Vec::new(),
            model_names_for: None,
            model_names_dirty: true,
            rescan_for_model: None,
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

        // La lista de modelos del selector de malla se refresca al cambiar
        // la entidad seleccionada, cuando la sección pide un refresco, o
        // cuando el modelo actual no está en la lista cacheada: un horneado
        // de MeshForge o una conversión pueden crear el .glb sin cambiar la
        // selección, y dar el aviso "no está en assets/" con la lista vieja
        // sería falso. Si tras reescanear sigue sin aparecer, el aviso es
        // real y no se vuelve a reescanear para ese mismo valor.
        let modelo_actual = entity
            .mesh
            .as_ref()
            .map(|m| m.model.clone())
            .unwrap_or_default();
        let falta_en_cache = !modelo_actual.is_empty()
            && !self.model_names.contains(&modelo_actual)
            && self.rescan_for_model.as_deref() != Some(modelo_actual.as_str());
        if self.model_names_dirty || self.model_names_for != Some(entity.id) || falta_en_cache {
            self.model_names = scan_model_names();
            self.model_names_for = Some(entity.id);
            self.model_names_dirty = false;
            self.rescan_for_model = Some(modelo_actual);
        }

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
                        "☼"
                    } else if entity.components.iter().any(|c| c == "MeshRenderer") {
                        "⬡"
                    } else {
                        "▣"
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
                    entity.transform.set_rotation_euler([0.0, 0.0, 0.0]);
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

                        // Rotación. Se edita una copia del Euler y se reaplica
                        // por el setter para que el cuaternión autoritativo no
                        // quede desactualizado cuando el usuario mueve un campo.
                        ui.label("Rotación");
                        let mut rot_euler = entity.transform.rotation();
                        ui.horizontal(|ui| {
                            ui.colored_label(red, "X");
                            ui.add(DragValue::new(&mut rot_euler[0]).speed(0.5).suffix("°"));
                            ui.colored_label(green, "Y");
                            ui.add(DragValue::new(&mut rot_euler[1]).speed(0.5).suffix("°"));
                            ui.colored_label(blue, "Z");
                            ui.add(DragValue::new(&mut rot_euler[2]).speed(0.5).suffix("°"));
                        });
                        if rot_euler != entity.transform.rotation() {
                            entity.transform.set_rotation_euler(rot_euler);
                        }
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
                    ui.collapsing("◈ Mesh Renderer & Material", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Color Albedo:");
                            ui.color_edit_button_rgb(&mut mesh.color);
                        });
                        // Selector de modelo: la clave es el nombre del
                        // .glb en assets/ (vacío = cubo por defecto), la
                        // misma que usa el bucle de dibujo desde la P1.
                        ui.horizontal(|ui| {
                            ui.label("Modelo:");
                            let selected_label = if mesh.model.is_empty() {
                                "(Cubo por defecto)".to_string()
                            } else {
                                mesh.model.clone()
                            };
                            egui::ComboBox::from_id_salt(format!("model_combo_{}", entity.id))
                                .selected_text(selected_label)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut mesh.model, String::new(), "(Cubo por defecto)");
                                    for name in &self.model_names {
                                        ui.selectable_value(&mut mesh.model, name.clone(), name.as_str());
                                    }
                                    if !mesh.model.is_empty() && !self.model_names.contains(&mesh.model) {
                                        let missing = mesh.model.clone();
                                        ui.selectable_value(&mut mesh.model, missing.clone(), format!("{missing} (no está en assets/)"));
                                    }
                                });
                            if ui.small_button("↻").on_hover_text("Volver a buscar modelos en assets/").clicked() {
                                self.model_names_dirty = true;
                            }
                        });
                        if !mesh.model.is_empty() && !self.model_names.contains(&mesh.model) {
                            ui.label(
                                RichText::new("⚠ Ese modelo no está en assets/: al dibujar se usará el cubo.")
                                    .size(11.0)
                                    .color(Color32::from_rgb(200, 120, 0)),
                            );
                        }
                        ui.checkbox(&mut mesh.wireframe, "Modo Wireframe");
                        // Material (Materiales v1): nombre del activo
                        // en assets/materials/<nombre>.material.json.
                        // Con material, sus parámetros mandan sobre el
                        // color y los valores por defecto.
                        ui.horizontal(|ui| {
                            ui.label("Material:");
                            ui.text_edit_singleline(&mut mesh.material);
                        });
                        if !mesh.material.is_empty() {
                            let ruta = format!(
                                "assets/materials/{}.material.json",
                                mesh.material
                            );
                            if std::path::Path::new(&ruta).exists() {
                                ui.label("Activo encontrado en assets/materials/.");
                            } else if ui
                                .small_button("➕ Crear material con valores por defecto")
                                .clicked()
                            {
                                let _ = std::fs::create_dir_all("assets/materials");
                                // JSON del MaterialData por defecto,
                                // escrito a mano: glass_editor no
                                // depende de serde_json.
                                let texto = "{\n  \"albedo\": [1.0, 1.0, 1.0, 1.0],\n  \"texture\": \"\",\n  \"metallic\": 0.0,\n  \"roughness\": 0.45,\n  \"emissive\": [0.0, 0.0, 0.0],\n  \"emissive_intensity\": 0.0,\n  \"tiling\": [1.0, 1.0],\n  \"offset\": [0.0, 0.0],\n  \"transparent\": false\n}\n";
                                let _ = std::fs::write(&ruta, texto);
                            }
                        }
                        ui.label(format!("Shader: {}", mesh.shader));

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
                // 3.5. COMPONENTE CÁMARA (CAMERA ESTILO UNITY)
                // ==========================================
                let mut remove_camera = false;
                if let Some(ref mut cam) = entity.camera {
                    ui.collapsing("🎥 Cámara (Camera)", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Field of View:");
                            ui.add(Slider::new(&mut cam.fov, 10.0..=120.0).suffix("°"));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Clipping Planes:");
                        });
                        ui.horizontal(|ui| {
                            ui.label("  Near:");
                            ui.add(DragValue::new(&mut cam.near_clip).speed(0.01).range(0.01..=10.0));
                            ui.label("  Far:");
                            ui.add(DragValue::new(&mut cam.far_clip).speed(1.0).range(1.0..=10000.0));
                        });
                        ui.checkbox(&mut cam.is_main, "Cámara Principal (Main Camera)");
                        if ui.small_button("🗑 Quitar Cámara").clicked() {
                            remove_camera = true;
                        }
                    });
                    ui.separator();
                }
                if remove_camera {
                    entity.camera = None;
                    entity.components.retain(|c| c != "Camera");
                }

                // ==========================================
                // 3.6. COMPONENTE LUZ (LIGHT ESTILO UNITY)
                // ==========================================
                let mut remove_light = false;
                if let Some(ref mut light) = entity.light {
                    ui.collapsing("☼ Luz (Light)", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Tipo de Luz:");
                            egui::ComboBox::from_id_salt(format!("light_type_{}", entity.id))
                                .selected_text(&light.light_type)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut light.light_type, "Directional".into(), "☼ Directional (Sol / Luz infinita)");
                                    ui.selectable_value(&mut light.light_type, "Point".into(), "✦ Point (Luz omnidireccional)");
                                    ui.selectable_value(&mut light.light_type, "Spot".into(), "⎚ Spot (Foco cónico)");
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Color:");
                            ui.color_edit_button_rgb(&mut light.color);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Intensidad:");
                            ui.add(DragValue::new(&mut light.intensity).speed(0.05).range(0.0..=10.0));
                        });
                        if light.light_type != "Directional" {
                            ui.horizontal(|ui| {
                                ui.label("Rango:");
                                ui.add(DragValue::new(&mut light.range).speed(0.2).range(0.1..=100.0).suffix(" m"));
                            });
                        }
                        if ui.small_button("🗑 Quitar Luz").clicked() {
                            remove_light = true;
                        }
                    });
                    ui.separator();
                }
                if remove_light {
                    entity.light = None;
                    entity.components.retain(|c| c != "Light");
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
                    ui.collapsing("§ Script C# (.NET CoreCLR)", |ui| {
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
                    ui.collapsing("♫ AudioSource (Kira CPAL)", |ui| {
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
                // 6.5. COMPONENTES DE UI (CANVAS, RECTTRANSFORM, TEXT, BUTTON)
                // ==========================================
                let mut remove_canvas = false;
                if let Some(ref mut canvas) = entity.canvas {
                    ui.collapsing("🖼 Canvas (Sistema UI)", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Modo de Render:");
                            egui::ComboBox::from_id_salt(format!("canvas_render_mode_{}", entity.id))
                                .selected_text(&canvas.render_mode)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut canvas.render_mode, "ScreenSpaceOverlay".into(), "Screen Space - Overlay");
                                    ui.selectable_value(&mut canvas.render_mode, "ScreenSpaceCamera".into(), "Screen Space - Camera");
                                    ui.selectable_value(&mut canvas.render_mode, "WorldSpace".into(), "World Space");
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Resolución Ref:");
                            ui.add(DragValue::new(&mut canvas.reference_resolution[0]).prefix("W: "));
                            ui.add(DragValue::new(&mut canvas.reference_resolution[1]).prefix("H: "));
                        });
                        if ui.small_button("🗑 Quitar Canvas").clicked() {
                            remove_canvas = true;
                        }
                    });
                    ui.separator();
                }
                if remove_canvas {
                    entity.canvas = None;
                    entity.components.retain(|c| c != "Canvas");
                }

                let mut remove_rect = false;
                if let Some(ref mut rt) = entity.rect_transform {
                    ui.collapsing("⊡ RectTransform (Geometría UI 2D)", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Posición Anclada:");
                            ui.add(DragValue::new(&mut rt.anchored_position[0]).speed(1.0).prefix("X: "));
                            ui.add(DragValue::new(&mut rt.anchored_position[1]).speed(1.0).prefix("Y: "));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Tamaño Delta:");
                            ui.add(DragValue::new(&mut rt.size_delta[0]).speed(1.0).prefix("W: "));
                            ui.add(DragValue::new(&mut rt.size_delta[1]).speed(1.0).prefix("H: "));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Pivote:");
                            ui.add(DragValue::new(&mut rt.pivot[0]).speed(0.05).range(0.0..=1.0).prefix("X: "));
                            ui.add(DragValue::new(&mut rt.pivot[1]).speed(0.05).range(0.0..=1.0).prefix("Y: "));
                        });
                        if ui.small_button("🗑 Quitar RectTransform").clicked() {
                            remove_rect = true;
                        }
                    });
                    ui.separator();
                }
                if remove_rect {
                    entity.rect_transform = None;
                    entity.components.retain(|c| c != "RectTransform");
                }

                let mut remove_text = false;
                if let Some(ref mut txt) = entity.ui_text {
                    ui.collapsing("🔤 Text (TMP / UI Text)", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Texto:");
                            ui.text_edit_singleline(&mut txt.text);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Tamaño:");
                            ui.add(DragValue::new(&mut txt.font_size).speed(0.5).range(8.0..=120.0).suffix(" px"));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Color:");
                            ui.color_edit_button_rgba_unmultiplied(&mut txt.color);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Fuente:");
                            ui.text_edit_singleline(&mut txt.font);
                        });
                        ui.label("Nombre del .ttf en assets/fonts (sin extensión) o «default».");
                        ui.horizontal(|ui| {
                            ui.label("Alineación:");
                            for (valor, etiqueta) in
                                [("Left", "Izquierda"), ("Center", "Centro"), ("Right", "Derecha")]
                            {
                                if ui
                                    .selectable_label(txt.align == valor, etiqueta)
                                    .clicked()
                                {
                                    txt.align = valor.to_string();
                                }
                            }
                        });
                        if ui.small_button("🗑 Quitar Text").clicked() {
                            remove_text = true;
                        }
                    });
                    ui.separator();
                }
                if remove_text {
                    entity.ui_text = None;
                    entity.components.retain(|c| c != "Text");
                }

                let mut remove_btn = false;
                if let Some(ref mut btn) = entity.ui_button {
                    ui.collapsing("🔘 Button (Interactable)", |ui| {
                        ui.checkbox(&mut btn.interactable, "Interactable (Habilitado)");
                        ui.horizontal(|ui| {
                            ui.label("Color Botón:");
                            ui.color_edit_button_rgba_unmultiplied(&mut btn.target_graphic_color);
                        });
                        ui.horizontal(|ui| {
                            ui.label("On Click Event:");
                            ui.text_edit_singleline(&mut btn.on_click_action);
                        });
                        if ui.small_button("🗑 Quitar Button").clicked() {
                            remove_btn = true;
                        }
                    });
                    ui.separator();
                }
                if remove_btn {
                    entity.ui_button = None;
                    entity.components.retain(|c| c != "Button");
                }

                let mut remove_img = false;
                if let Some(ref mut img) = entity.ui_image {
                    ui.collapsing("🖼 Image", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Sprite:");
                            ui.text_edit_singleline(&mut img.sprite);
                        });
                        ui.label("Ruta relativa a assets/, p. ej. textures/icono.png");
                        ui.horizontal(|ui| {
                            ui.label("Tinte:");
                            ui.color_edit_button_rgba_unmultiplied(&mut img.color);
                        });
                        ui.checkbox(&mut img.preserve_aspect, "Preserve Aspect (encajar sin deformar)");
                        if ui.small_button("🗑 Quitar Image").clicked() {
                            remove_img = true;
                        }
                    });
                    ui.separator();
                }
                if remove_img {
                    entity.ui_image = None;
                    entity.components.retain(|c| c != "Image");
                }

                let mut remove_drag = false;
                if let Some(ref mut drag) = entity.ui_drag {
                    ui.collapsing("✋ Drag & Drop (UI)", |ui| {
                        ui.checkbox(&mut drag.draggable, "Draggable (se puede arrastrar)");
                        ui.checkbox(&mut drag.accepts_drop, "Accepts Drop (recibe sueltas)");
                        ui.horizontal(|ui| {
                            ui.label("On Begin Drag:");
                            ui.text_edit_singleline(&mut drag.on_begin_drag);
                        });
                        ui.horizontal(|ui| {
                            ui.label("On Drag End:");
                            ui.text_edit_singleline(&mut drag.on_drag_end);
                        });
                        ui.horizontal(|ui| {
                            ui.label("On Drop:");
                            ui.text_edit_singleline(&mut drag.on_drop);
                        });
                        ui.label("Métodos del script de esta entidad; DragDrop.DraggedEntityId / DropTargetEntityId dan la pareja del gesto.");
                        if ui.small_button("🗑 Quitar Drag & Drop").clicked() {
                            remove_drag = true;
                        }
                    });
                    ui.separator();
                }
                if remove_drag {
                    entity.ui_drag = None;
                    entity.components.retain(|c| c != "DragDrop");
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

                        if entity.audio.is_none() && ui.button("♫ AudioSource").clicked() {
                            entity.audio = Some(AudioSourceData::default());
                            entity.components.push("AudioSource".into());
                            self.show_add_component_menu = false;
                        }

                        if entity.camera.is_none() && ui.button("🎥 Cámara (Camera)").clicked() {
                            entity.camera = Some(CameraData::default());
                            entity.components.push("Camera".into());
                            self.show_add_component_menu = false;
                        }

                        if entity.light.is_none() && ui.button("☼ Luz (Light)").clicked() {
                            entity.light = Some(LightData::default());
                            entity.components.push("Light".into());
                            self.show_add_component_menu = false;
                        }

                        if entity.canvas.is_none() && ui.button("🖼 Canvas").clicked() {
                            entity.canvas = Some(CanvasData::default());
                            entity.components.push("Canvas".into());
                            self.show_add_component_menu = false;
                        }

                        if entity.rect_transform.is_none() && ui.button("⊡ RectTransform").clicked() {
                            entity.rect_transform = Some(RectTransformData::default());
                            entity.components.push("RectTransform".into());
                            self.show_add_component_menu = false;
                        }

                        if entity.ui_text.is_none() && ui.button("🔤 Text (TMP)").clicked() {
                            entity.ui_text = Some(UITextData::default());
                            entity.components.push("Text".into());
                            self.show_add_component_menu = false;
                        }

                        if entity.ui_button.is_none() && ui.button("🔘 Button").clicked() {
                            entity.ui_button = Some(UIButtonData::default());
                            entity.components.push("Button".into());
                            self.show_add_component_menu = false;
                        }

                        if entity.ui_image.is_none() && ui.button("🖼 Image").clicked() {
                            entity.ui_image = Some(UIImageData::default());
                            entity.components.push("Image".into());
                            self.show_add_component_menu = false;
                        }

                        if entity.ui_drag.is_none() && ui.button("✋ Drag & Drop").clicked() {
                            entity.ui_drag = Some(UIDragData::default());
                            entity.components.push("DragDrop".into());
                            self.show_add_component_menu = false;
                        }
                    });
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::scan_model_names_in;
    use std::path::{Path, PathBuf};

    /// Crea un directorio temporal único para un test. Si ya existe (resto de
    /// una ejecución anterior con el mismo pid), lo borra antes.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "glass_inspector_test_{}_{}",
            tag,
            std::process::id()
        ));
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        std::fs::create_dir_all(&dir).expect("crear directorio temporal");
        dir
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("crear carpeta padre");
        }
        std::fs::write(path, b"").expect("crear archivo");
    }

    #[test]
    fn recoge_stems_glb_recursivo_ordenado_y_sin_duplicados() {
        let root = temp_dir("recoge");
        touch(&root.join("nave.glb"));
        touch(&root.join("nave.GLB")); // duplicado por mayúsculas
        touch(&root.join("sub").join("torreta.glb"));
        touch(&root.join("sub").join("profundo").join("base.GLB"));
        touch(&root.join("notas.txt"));
        touch(&root.join("sprite.png"));

        let names = scan_model_names_in(&root);

        assert_eq!(
            names,
            vec![
                "base".to_string(),
                "nave".to_string(),
                "torreta".to_string()
            ]
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn raiz_inexistente_devuelve_lista_vacia() {
        let root = temp_dir("inexistente");
        let missing = root.join("no_existe");
        assert!(scan_model_names_in(&missing).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
