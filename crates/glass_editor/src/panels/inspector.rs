use egui::{Color32, DragValue, RichText, Slider, Ui};
use glass_core::{
    AudioSourceData, CameraData, CanvasData, CharacterData, LightData, MeshData, PhysicsData,
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

/// Convierte `size` entre familias de forma (copia a nivel de módulo de
/// la que usa la sección legacy del Collider, para la materialización
/// del inferido al añadir un collider nuevo en P3):
/// · Box (half-extents) → Sphere: radio = x; → Capsule/Cylinder: [half_height = y, radio = x].
/// · Sphere (radio r) → Box: [r, r, r]; → Capsule/Cylinder: [r, r].
/// · Capsule ↔ Cylinder comparten disposición [half_height, radio]: se conserva.
fn tamano_convertido_modulo(desde: &str, hacia: &str, s: [f32; 3]) -> [f32; 3] {
    let familia = |t: &str| match t {
        "Box" => 0,
        "Sphere" => 1,
        _ => 2,
    };
    match (familia(desde), familia(hacia)) {
        (0, 1) => [s[0], 0.0, 0.0],
        (0, 2) => [s[1], s[0], 0.0],
        (1, 0) => [s[0], s[0], s[0]],
        (1, 2) => [s[0], s[0], 0.0],
        (2, 0) => [s[1], s[0], s[1]],
        (2, 1) => [s[1], 0.0, 0.0],
        _ => s,
    }
}

/// Forma que la física infiere del nombre de la malla integrada (la
/// misma regla exacta de la sección Collider: "sphere"/"capsule"/
/// "cylinder", cualquier otro nombre es Box).
fn forma_inferida_collider(modelo: &str) -> &'static str {
    match modelo {
        "sphere" => "Sphere",
        "capsule" => "Capsule",
        "cylinder" => "Cylinder",
        _ => "Box",
    }
}

/// Qué collider pide una entrada del menú «Añadir Componente» (P3).
#[derive(Clone, Copy)]
enum NuevoCollider {
    Primitiva(&'static str),
    Mesh,
}

/// Añade un collider a la entidad desde el menú «Añadir Componente»
/// (P3: cada collider es su propio componente, repetible). Crea la
/// PhysicsData si no hay; si la entidad vivía del collider inferido
/// legacy (lista vacía, sin mesh, con Collider), lo materializa primero
/// como spec para no perderlo — también al añadir el Mesh, porque fijar
/// el modo sacaría a la entidad del estado legacy.
fn anadir_collider_componente(
    entity: &mut glass_core::EntityData,
    modelo: &str,
    nuevo: NuevoCollider,
) {
    let era_collider = entity
        .physics
        .as_ref()
        .is_some_and(|p| p.has_collider);
    if entity.physics.is_none() {
        entity.physics = Some(PhysicsData {
            body_type: "Static".into(),
            has_rigidbody: false,
            has_collider: true,
            ..PhysicsData::default()
        });
    }
    let phys = entity.physics.as_mut().expect("physics recién asegurado");
    if era_collider
        && phys.colliders.is_empty()
        && phys.mesh_collider == glass_core::MeshColliderMode::None
    {
        let forma = forma_inferida_collider(modelo);
        let spec = glass_core::ColliderSpec {
            shape: forma.to_string(),
            size: tamano_convertido_modulo("Box", forma, phys.box_extents),
            offset: phys.box_offset,
            is_trigger: phys.is_trigger,
        };
        phys.colliders.push(spec);
    }
    phys.has_collider = true;
    match nuevo {
        NuevoCollider::Primitiva(forma) => {
            phys.anadir_collider(glass_core::spec_por_defecto(forma));
        }
        NuevoCollider::Mesh => {
            phys.mesh_collider = glass_core::MeshColliderMode::Convex;
        }
    }
    if !entity.components.iter().any(|c| c == "Collider") {
        entity.components.push("Collider".into());
    }
}

/// Las cinco entradas de collider del menú «Añadir Componente» (P3).
/// Devuelve true si se añadió algo (el llamador cierra el menú).
fn botones_collider_menu(
    ui: &mut Ui,
    entity: &mut glass_core::EntityData,
    modelo: &str,
    q: &str,
) -> bool {
    let coincide = |nombre: &str| q.is_empty() || nombre.to_lowercase().contains(q);
    let hay_mesh = entity
        .physics
        .as_ref()
        .is_some_and(|p| p.mesh_collider != glass_core::MeshColliderMode::None);
    for (nombre, etiqueta, forma) in [
        ("Box Collider", "🧊 Box Collider", "Box"),
        ("Sphere Collider", "🧊 Sphere Collider", "Sphere"),
        ("Capsule Collider", "🧊 Capsule Collider", "Capsule"),
        ("Cylinder Collider", "🧊 Cylinder Collider", "Cylinder"),
    ] {
        if coincide(nombre) && ui.button(etiqueta).clicked() {
            anadir_collider_componente(entity, modelo, NuevoCollider::Primitiva(forma));
            return true;
        }
    }
    if !hay_mesh && coincide("Mesh Collider") && ui.button("🧊 Mesh Collider").clicked() {
        anadir_collider_componente(entity, modelo, NuevoCollider::Mesh);
        return true;
    }
    false
}

/// Acción estructural pedida desde una sección de collider (P3): se
/// recoge mientras se pinta y se aplica después, para no mutar la lista
/// durante la iteración.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AccionCollider {
    Duplicar(usize),
    Quitar(usize),
    QuitarMesh,
}

#[derive(Default)]
pub struct InspectorPanel {
    show_add_component_menu: bool,
    busqueda_componente: String,
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
            busqueda_componente: String::new(),
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
        model_bounds: &std::collections::HashMap<String, ([f32; 3], [f32; 3])>,
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
                // 4. COMPONENTES RIGIDBODY Y COLLIDER (RAPIER3D)
                // Separados como en Unity: un Collider sin Rigidbody es estático.
                // ==========================================
                let mut remove_rigidbody = false;
                let mut remove_collider = false;
                // Datos de la entidad que la sección Collider necesita sin
                // volver a prestar `entity` (dentro va prestada mutable
                // vía `phys`): nombre del modelo, si tiene malla y sus
                // cotas locales para «Ajustar a la malla».
                let modelo_entidad: String =
                    entity.mesh.as_ref().map(|m| m.model.clone()).unwrap_or_default();
                let cotas_entidad = super::gizmo_3d::entity_local_bounds(entity, model_bounds);
                if let Some(ref mut phys) = entity.physics {
                    if phys.has_rigidbody {
                        ui.collapsing("⚙ Rigidbody", |ui| {
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

                            // Lee/pone un bit de `phys.constraints` con una casilla.
                            macro_rules! casilla_bit {
                                ($ui:expr, $label:expr, $bit:expr) => {{
                                    let mut v = (phys.constraints & $bit) != 0;
                                    if $ui.checkbox(&mut v, $label).changed() {
                                        if v {
                                            phys.constraints |= $bit;
                                        } else {
                                            phys.constraints &= !$bit;
                                        }
                                    }
                                }};
                            }

                            ui.horizontal(|ui| {
                                ui.label("Masa:");
                                ui.add(DragValue::new(&mut phys.mass).speed(0.1).range(0.01..=500.0).suffix(" kg"));
                            });

                            ui.horizontal(|ui| {
                                ui.label("Drag:");
                                ui.add(DragValue::new(&mut phys.linear_damping).speed(0.05).range(0.0..=100.0));
                            });

                            ui.horizontal(|ui| {
                                ui.label("Angular Drag:");
                                ui.add(DragValue::new(&mut phys.angular_damping).speed(0.05).range(0.0..=100.0));
                            });

                            ui.horizontal(|ui| {
                                ui.checkbox(&mut phys.use_gravity, "Use Gravity");
                                ui.checkbox(&mut phys.interpolate, "Interpolate");
                            });

                            ui.horizontal(|ui| {
                                ui.label("Is Kinematic");
                                if phys.body_type == "Static" {
                                    let mut apagada = false;
                                    ui.add_enabled(false, |ui: &mut Ui| ui.checkbox(&mut apagada, ""));
                                } else {
                                    let mut kinematic = phys.body_type == "Kinematic";
                                    if ui.checkbox(&mut kinematic, "").changed() {
                                        phys.body_type = if kinematic { "Kinematic" } else { "Dynamic" }.into();
                                    }
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.label("Collision Detection:");
                                egui::ComboBox::from_id_salt(format!("phys_ccd_{}", selected_id.unwrap_or(0)))
                                    .selected_text(if phys.ccd { "Continua" } else { "Discreta" })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut phys.ccd, false, "Discreta");
                                        ui.selectable_value(&mut phys.ccd, true, "Continua");
                                    });
                            });

                            ui.label(RichText::new("Constraints").strong());
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Freeze Position");
                                    casilla_bit!(ui, "X", 1);
                                    casilla_bit!(ui, "Y", 2);
                                    casilla_bit!(ui, "Z", 4);
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Freeze Rotation");
                                    casilla_bit!(ui, "X", 8);
                                    casilla_bit!(ui, "Y", 16);
                                    casilla_bit!(ui, "Z", 32);
                                });
                            });

                            ui.label(RichText::new(format!(
                                "Velocidad Lineal: ({:.2}, {:.2}, {:.2}) m/s",
                                phys.linear_velocity[0], phys.linear_velocity[1], phys.linear_velocity[2]
                            )).color(Color32::from_rgb(160, 210, 255)));

                            if ui.small_button("🗑 Quitar Rigidbody").clicked() {
                                remove_rigidbody = true;
                            }
                        });
                    }

                    if phys.has_collider
                        && phys.colliders.is_empty()
                        && phys.mesh_collider == glass_core::MeshColliderMode::None
                    {
                        ui.collapsing("🧊 Collider", |ui| {
                            use glass_core::scene::{ColliderSpec, MeshColliderMode};

                            // Convierte `size` entre familias de forma con un criterio simple:
                            // · Box (half-extents) → Sphere: radio = x; → Capsule/Cylinder: [half_height = y, radio = x].
                            // · Sphere (radio r) → Box: [r, r, r]; → Capsule/Cylinder: [r, r].
                            // · Capsule/Cylinder ([hh, r]) → Box: [r, hh, r]; → Sphere: radio = r.
                            // Capsule ↔ Cylinder comparten disposición [half_height, radio]: se conserva.
                            fn tamano_convertido(desde: &str, hacia: &str, s: [f32; 3]) -> [f32; 3] {
                                let familia = |t: &str| match t {
                                    "Box" => 0,
                                    "Sphere" => 1,
                                    _ => 2,
                                };
                                match (familia(desde), familia(hacia)) {
                                    (0, 1) => [s[0], 0.0, 0.0],
                                    (0, 2) => [s[1], s[0], 0.0],
                                    (1, 0) => [s[0], s[0], s[0]],
                                    (1, 2) => [s[0], s[0], 0.0],
                                    (2, 0) => [s[1], s[0], s[1]],
                                    (2, 1) => [s[1], 0.0, 0.0],
                                    _ => s,
                                }
                            }

                            // En Unity fricción y elasticidad son del material del collider.
                            ui.add(Slider::new(&mut phys.friction, 0.0..=1.0).text("Fricción"));
                            ui.add(Slider::new(&mut phys.restitution, 0.0..=1.0).text("Elasticidad (Bounciness)"));

                            let modelo: String = modelo_entidad.clone();
                            let inferido: &str = match modelo.as_str() {
                                "sphere" => "Sphere",
                                "capsule" => "Capsule",
                                "cylinder" => "Cylinder",
                                _ => "Box",
                            };
                            let tipo_actual: String = match phys.mesh_collider {
                                MeshColliderMode::Convex => "Mesh (Convex)".to_string(),
                                MeshColliderMode::TriMesh => "Mesh (TriMesh)".to_string(),
                                MeshColliderMode::None => match phys.colliders.first() {
                                    Some(c) => c.shape.clone(),
                                    None => inferido.to_string(),
                                },
                            };

                            let mut tipo_nuevo = tipo_actual.clone();
                            ui.horizontal(|ui| {
                                ui.label("Tipo");
                                egui::ComboBox::from_id_salt(format!("collider_tipo_{}", selected_id.unwrap_or(0)))
                                    .selected_text(tipo_actual.clone())
                                    .show_ui(ui, |ui| {
                                        for t in ["Box", "Sphere", "Capsule", "Cylinder", "Mesh (Convex)", "Mesh (TriMesh)"] {
                                            ui.selectable_value(&mut tipo_nuevo, t.to_string(), t);
                                        }
                                    });
                            });

                            if tipo_nuevo != tipo_actual {
                                match tipo_nuevo.as_str() {
                                    "Mesh (Convex)" => phys.mesh_collider = MeshColliderMode::Convex,
                                    "Mesh (TriMesh)" => phys.mesh_collider = MeshColliderMode::TriMesh,
                                    prim => {
                                        phys.mesh_collider = MeshColliderMode::None;
                                        if phys.colliders.is_empty() {
                                            // Box → Box con lista vacía: se sigue editando box_extents/box_offset.
                                            if !(prim == "Box" && tipo_actual == "Box") {
                                                let size = tamano_convertido("Box", prim, phys.box_extents);
                                                let is_trigger = phys.is_trigger;
                                                phys.colliders.push(ColliderSpec {
                                                    shape: prim.to_string(),
                                                    size,
                                                    offset: phys.box_offset,
                                                    is_trigger,
                                                });
                                            }
                                        } else {
                                            let c = &mut phys.colliders[0];
                                            if c.shape != prim {
                                                c.size = tamano_convertido(&c.shape, prim, c.size);
                                                c.shape = prim.to_string();
                                            }
                                        }
                                    }
                                }
                            }
                            let tipo = tipo_nuevo;

                            if let Some(c) = phys.colliders.first_mut() {
                                ui.checkbox(&mut c.is_trigger, "Is Trigger");
                            } else {
                                ui.checkbox(&mut phys.is_trigger, "Is Trigger");
                            }

                            if tipo.starts_with("Mesh") {
                                if modelo.is_empty() {
                                    ui.label(RichText::new("(sin malla: caerá a los primitivos)").weak());
                                } else {
                                    ui.label(format!("Malla: {modelo}"));
                                }
                                if matches!(phys.mesh_collider, MeshColliderMode::TriMesh)
                                    && phys.has_rigidbody
                                    && phys.body_type != "Static"
                                {
                                    ui.label(
                                        RichText::new("En cuerpos dinámicos TriMesh se degrada a Convex al simular.")
                                            .color(Color32::from_rgb(255, 190, 80)),
                                    );
                                }
                            } else {
                                // Copias locales del offset/size efectivos. `cambiado` es la bandera de
                                // materialización perezosa: solo al editar se escribe (y, con lista vacía
                                // y tipo no-Box, se crea el spec equivalente).
                                let (mut center, mut size) = match phys.colliders.first() {
                                    Some(c) => (c.offset, c.size),
                                    None => (phys.box_offset, tamano_convertido("Box", &tipo, phys.box_extents)),
                                };
                                let mut cambiado = false;

                                ui.horizontal(|ui| {
                                    ui.label("Center");
                                    for (eje, v) in ["X", "Y", "Z"].iter().zip(center.iter_mut()) {
                                        ui.label(*eje);
                                        cambiado |= ui.add(DragValue::new(v).speed(0.05)).changed();
                                    }
                                });

                                match tipo.as_str() {
                                    "Box" => {
                                        ui.horizontal(|ui| {
                                            ui.label("Size");
                                            for (eje, h) in ["X", "Y", "Z"].iter().zip(size.iter_mut()) {
                                                ui.label(*eje);
                                                let mut completo = *h * 2.0;
                                                if ui
                                                    .add(DragValue::new(&mut completo).speed(0.05).range(0.01..=f32::INFINITY))
                                                    .changed()
                                                {
                                                    *h = completo.max(0.01) * 0.5;
                                                    cambiado = true;
                                                }
                                            }
                                        });
                                    }
                                    "Sphere" => {
                                        ui.horizontal(|ui| {
                                            ui.label("Radius");
                                            cambiado |= ui
                                                .add(DragValue::new(&mut size[0]).speed(0.05).range(0.01..=f32::INFINITY))
                                                .changed();
                                        });
                                    }
                                    "Capsule" => {
                                        ui.horizontal(|ui| {
                                            ui.label("Radius");
                                            cambiado |= ui
                                                .add(DragValue::new(&mut size[1]).speed(0.05).range(0.01..=f32::INFINITY))
                                                .changed();
                                        });
                                        let mut altura = 2.0 * (size[0] + size[1]);
                                        ui.horizontal(|ui| {
                                            ui.label("Height");
                                            if ui
                                                .add(DragValue::new(&mut altura).speed(0.05).range(0.0..=f32::INFINITY))
                                                .changed()
                                            {
                                                size[0] = (altura * 0.5 - size[1]).max(0.0);
                                                cambiado = true;
                                            }
                                        });
                                        ui.label(RichText::new("Dirección: Y").weak());
                                    }
                                    "Cylinder" => {
                                        ui.horizontal(|ui| {
                                            ui.label("Radius");
                                            cambiado |= ui
                                                .add(DragValue::new(&mut size[1]).speed(0.05).range(0.01..=f32::INFINITY))
                                                .changed();
                                        });
                                        let mut altura = 2.0 * size[0];
                                        ui.horizontal(|ui| {
                                            ui.label("Height");
                                            if ui
                                                .add(DragValue::new(&mut altura).speed(0.05).range(0.0..=f32::INFINITY))
                                                .changed()
                                            {
                                                size[0] = altura.max(0.0) * 0.5;
                                                cambiado = true;
                                            }
                                        });
                                    }
                                    _ => {}
                                }

                                if tipo == "Box" || tipo == "Sphere" {
                                    let pulsado = ui
                                        .add_enabled_ui(!modelo.is_empty(), |ui| ui.button("🎯 Ajustar a la malla").clicked())
                                        .inner;
                                    if pulsado {
                                        let (lo, hi) = cotas_entidad;
                                        for i in 0..3 {
                                            center[i] = (lo[i] + hi[i]) * 0.5;
                                        }
                                        if tipo == "Box" {
                                            for i in 0..3 {
                                                size[i] = ((hi[i] - lo[i]) * 0.5).max(0.005);
                                            }
                                        } else {
                                            let mayor = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]);
                                            size[0] = (mayor * 0.5).max(0.01);
                                        }
                                        cambiado = true;
                                    }
                                }

                                if cambiado {
                                    if let Some(c) = phys.colliders.first_mut() {
                                        c.offset = center;
                                        c.size = size;
                                    } else if tipo == "Box" {
                                        phys.box_offset = center;
                                        phys.box_extents = size;
                                    } else {
                                        let is_trigger = phys.is_trigger;
                                        phys.colliders.push(ColliderSpec {
                                            shape: tipo.clone(),
                                            size,
                                            offset: center,
                                            is_trigger,
                                        });
                                    }
                                }
                            }

                            if ui.small_button("🗑 Quitar Collider").clicked() {
                                remove_collider = true;
                            }
                        });
                    } else if phys.has_collider {
                        // P3: cada collider es su propio componente, como
                        // en Unity. Fricción y elasticidad son del cuerpo
                        // (compartidos) y van una sola vez, arriba.
                        ui.collapsing("🧊 Colliders", |ui| {
                            ui.add(Slider::new(&mut phys.friction, 0.0..=1.0).text("Fricción"));
                            ui.add(Slider::new(&mut phys.restitution, 0.0..=1.0).text("Elasticidad (Bounciness)"));
                            ui.label(
                                RichText::new("Fricción y elasticidad son del cuerpo: las comparten sus colliders.")
                                    .weak(),
                            );
                        });

                        let mut acciones: Vec<AccionCollider> = Vec::new();
                        for i in 0..phys.colliders.len() {
                            let titulo = format!("{} Collider", phys.colliders[i].shape);
                            egui::CollapsingHeader::new(titulo)
                                .id_salt(("collider_spec", selected_id.unwrap_or(0), i))
                                .default_open(true)
                                .show(ui, |ui| {
                                    let spec = &mut phys.colliders[i];
                                    ui.checkbox(&mut spec.is_trigger, "Is Trigger");

                                    let mut center = spec.offset;
                                    let mut size = spec.size;
                                    let mut cambiado = false;

                                    ui.horizontal(|ui| {
                                        ui.label("Center");
                                        for (eje, v) in ["X", "Y", "Z"].iter().zip(center.iter_mut()) {
                                            ui.label(*eje);
                                            cambiado |= ui.add(DragValue::new(v).speed(0.05)).changed();
                                        }
                                    });

                                    match spec.shape.as_str() {
                                        "Sphere" => {
                                            ui.horizontal(|ui| {
                                                ui.label("Radius");
                                                cambiado |= ui
                                                    .add(DragValue::new(&mut size[0]).speed(0.05).range(0.01..=f32::INFINITY))
                                                    .changed();
                                            });
                                        }
                                        "Capsule" => {
                                            ui.horizontal(|ui| {
                                                ui.label("Radius");
                                                cambiado |= ui
                                                    .add(DragValue::new(&mut size[1]).speed(0.05).range(0.01..=f32::INFINITY))
                                                    .changed();
                                            });
                                            let mut altura = 2.0 * (size[0] + size[1]);
                                            ui.horizontal(|ui| {
                                                ui.label("Height");
                                                if ui
                                                    .add(DragValue::new(&mut altura).speed(0.05).range(0.0..=f32::INFINITY))
                                                    .changed()
                                                {
                                                    size[0] = (altura * 0.5 - size[1]).max(0.0);
                                                    cambiado = true;
                                                }
                                            });
                                            ui.label(RichText::new("Dirección: Y").weak());
                                        }
                                        "Cylinder" => {
                                            ui.horizontal(|ui| {
                                                ui.label("Radius");
                                                cambiado |= ui
                                                    .add(DragValue::new(&mut size[1]).speed(0.05).range(0.01..=f32::INFINITY))
                                                    .changed();
                                            });
                                            let mut altura = 2.0 * size[0];
                                            ui.horizontal(|ui| {
                                                ui.label("Height");
                                                if ui
                                                    .add(DragValue::new(&mut altura).speed(0.05).range(0.0..=f32::INFINITY))
                                                    .changed()
                                                {
                                                    size[0] = altura.max(0.0) * 0.5;
                                                    cambiado = true;
                                                }
                                            });
                                        }
                                        _ => {
                                            // Box (y cualquier forma desconocida).
                                            ui.horizontal(|ui| {
                                                ui.label("Size");
                                                for (eje, h) in ["X", "Y", "Z"].iter().zip(size.iter_mut()) {
                                                    ui.label(*eje);
                                                    let mut completo = *h * 2.0;
                                                    if ui
                                                        .add(DragValue::new(&mut completo).speed(0.05).range(0.01..=f32::INFINITY))
                                                        .changed()
                                                    {
                                                        *h = completo.max(0.01) * 0.5;
                                                        cambiado = true;
                                                    }
                                                }
                                            });
                                        }
                                    }

                                    if spec.shape == "Box" || spec.shape == "Sphere" {
                                        let pulsado = ui
                                            .add_enabled_ui(!modelo_entidad.is_empty(), |ui| {
                                                ui.button("🎯 Ajustar a la malla").clicked()
                                            })
                                            .inner;
                                        if pulsado {
                                            let (lo, hi) = cotas_entidad;
                                            for k in 0..3 {
                                                center[k] = (lo[k] + hi[k]) * 0.5;
                                            }
                                            if spec.shape == "Box" {
                                                for k in 0..3 {
                                                    size[k] = ((hi[k] - lo[k]) * 0.5).max(0.005);
                                                }
                                            } else {
                                                let mayor = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]);
                                                size[0] = (mayor * 0.5).max(0.01);
                                            }
                                            cambiado = true;
                                        }
                                    }

                                    if cambiado {
                                        spec.offset = center;
                                        spec.size = size;
                                    }

                                    ui.horizontal(|ui| {
                                        if ui.small_button("⧉ Duplicar").clicked() {
                                            acciones.push(AccionCollider::Duplicar(i));
                                        }
                                        if ui.small_button("🗑 Quitar").clicked() {
                                            acciones.push(AccionCollider::Quitar(i));
                                        }
                                    });
                                });
                        }

                        if phys.mesh_collider != glass_core::MeshColliderMode::None {
                            egui::CollapsingHeader::new("Mesh Collider")
                                .id_salt(("mesh_collider", selected_id.unwrap_or(0)))
                                .default_open(true)
                                .show(ui, |ui| {
                                    if modelo_entidad.is_empty() {
                                        ui.label(RichText::new("(sin malla: caerá a los primitivos)").weak());
                                    } else {
                                        ui.label(format!("Malla: {modelo_entidad}"));
                                    }
                                    ui.label(
                                        RichText::new("Se adapta a la malla real del objeto.")
                                            .weak(),
                                    );
                                    let mut convex =
                                        phys.mesh_collider == glass_core::MeshColliderMode::Convex;
                                    if ui.checkbox(&mut convex, "Convex").changed() {
                                        phys.mesh_collider = if convex {
                                            glass_core::MeshColliderMode::Convex
                                        } else {
                                            glass_core::MeshColliderMode::TriMesh
                                        };
                                    }
                                    if phys.mesh_collider == glass_core::MeshColliderMode::TriMesh
                                        && phys.has_rigidbody
                                        && phys.body_type != "Static"
                                    {
                                        ui.label(
                                            RichText::new("En cuerpos dinámicos TriMesh se degrada a Convex al simular.")
                                                .color(Color32::from_rgb(255, 190, 80)),
                                        );
                                    }
                                    if ui.small_button("🗑 Quitar").clicked() {
                                        acciones.push(AccionCollider::QuitarMesh);
                                    }
                                });
                        }

                        if !acciones.is_empty() {
                            // Aplicar tras pintar, reconstruyendo la lista
                            // para que varias acciones en el mismo frame
                            // no desplacen índices.
                            let originales = std::mem::take(&mut phys.colliders);
                            let mut nuevos = Vec::with_capacity(originales.len() + 1);
                            for (i, spec) in originales.into_iter().enumerate() {
                                let quitar = acciones.contains(&AccionCollider::Quitar(i));
                                let duplicar = acciones.contains(&AccionCollider::Duplicar(i));
                                match (quitar, duplicar) {
                                    (true, false) => {}
                                    (false, true) => {
                                        let copia = spec.clone();
                                        nuevos.push(spec);
                                        nuevos.push(copia);
                                    }
                                    _ => nuevos.push(spec),
                                }
                            }
                            phys.colliders = nuevos;
                            if acciones.contains(&AccionCollider::QuitarMesh) {
                                phys.mesh_collider = glass_core::MeshColliderMode::None;
                            }
                            // Sin specs y sin mesh no queda ningún
                            // collider: apagar el componente para no
                            // resucitar el inferido legacy.
                            if phys.colliders.is_empty()
                                && phys.mesh_collider == glass_core::MeshColliderMode::None
                            {
                                phys.has_collider = false;
                                remove_collider = true;
                            }
                        }
                    }
                    ui.separator();
                }
                if remove_rigidbody || remove_collider {
                    if let Some(ref mut phys) = entity.physics {
                        if remove_rigidbody {
                            phys.has_rigidbody = false;
                        }
                        if remove_collider {
                            phys.has_collider = false;
                        }
                    }
                    if remove_rigidbody {
                        entity.components.retain(|c| c != "RigidBody");
                    }
                    if remove_collider {
                        entity.components.retain(|c| c != "Collider");
                    }
                    let sin_componentes = entity
                        .physics
                        .as_ref()
                        .is_some_and(|p| !p.has_rigidbody && !p.has_collider);
                    if sin_componentes {
                        entity.physics = None;
                        entity.components.retain(|c| c != "StaticCollider");
                    }
                }

                // ==========================================
                // 4b. COMPONENTE CHARACTER CONTROLLER
                // ==========================================
                let mut remove_character = false;
                if let Some(ref mut cc) = entity.character {
                    ui.collapsing("🚶 Character Controller", |ui| {
                        ui.checkbox(&mut cc.enabled, "Enabled");
                        ui.horizontal(|ui| {
                            ui.label("Slope Limit");
                            ui.add(
                                egui::DragValue::new(&mut cc.slope_limit_deg)
                                    .speed(0.5)
                                    .range(0.0..=90.0)
                                    .suffix("°"),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Step Offset");
                            ui.add(
                                egui::DragValue::new(&mut cc.step_offset)
                                    .speed(0.01)
                                    .range(0.0..=2.0),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Skin Width");
                            ui.add(
                                egui::DragValue::new(&mut cc.skin_width)
                                    .speed(0.001)
                                    .range(0.001..=0.5),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Min Move Distance");
                            ui.add(
                                egui::DragValue::new(&mut cc.min_move_distance)
                                    .speed(0.0005)
                                    .range(0.0..=0.1),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Center");
                            ui.add(egui::DragValue::new(&mut cc.center[0]).speed(0.01).prefix("X "));
                            ui.add(egui::DragValue::new(&mut cc.center[1]).speed(0.01).prefix("Y "));
                            ui.add(egui::DragValue::new(&mut cc.center[2]).speed(0.01).prefix("Z "));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Radius");
                            let resp = ui.add(
                                egui::DragValue::new(&mut cc.radius)
                                    .speed(0.01)
                                    .range(0.01..=2.0),
                            );
                            // La cápsula exige height ≥ 2·radio + 0.02
                            // (half_height = height/2 − radius ≥ 0.01, el
                            // mínimo que el runtime garantiza al crearla).
                            if resp.changed() && cc.height < cc.radius * 2.0 + 0.02 {
                                cc.height = cc.radius * 2.0 + 0.02;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Height");
                            // Mínimo conjunto con el radio: la cápsula no
                            // puede bajar de 2·radio + 0.02 (ver Radius).
                            let min_h = cc.radius * 2.0 + 0.02;
                            ui.add(
                                egui::DragValue::new(&mut cc.height)
                                    .speed(0.01)
                                    .range(min_h..=10.0),
                            );
                        });
                        ui.checkbox(&mut cc.detect_collisions, "Detect Collisions");
                        if ui.button("🗑 Quitar Character Controller").clicked() {
                            remove_character = true;
                        }
                    });
                }
                if remove_character {
                    entity.character = None;
                    entity.components.retain(|c| c != "CharacterController");
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
                    if !self.show_add_component_menu && !self.busqueda_componente.is_empty() {
                        // Cierre centralizado: cualquier camino que cierre
                        // el menú (toggle o elegir un componente) deja la
                        // búsqueda limpia para la próxima apertura.
                        self.busqueda_componente.clear();
                    }
                    if ui.button("➕ Añadir Componente").clicked() {
                        self.show_add_component_menu = !self.show_add_component_menu;
                        if !self.show_add_component_menu {
                            self.busqueda_componente.clear();
                        }
                    }
                });

                if self.show_add_component_menu {
                    ui.group(|ui| {
                        ui.label(RichText::new("Selecciona Componente a Añadir:").strong());
                        ui.add(
                            egui::TextEdit::singleline(&mut self.busqueda_componente)
                                .hint_text("Buscar componente…"),
                        );

                        let q = self.busqueda_componente.trim().to_lowercase();
                        let coincide = |nombre: &str| q.is_empty() || nombre.to_lowercase().contains(&q);

                        let mut hay_resultados = false;
                        hay_resultados |= coincide("MeshRenderer");
                        hay_resultados |= coincide("Rigidbody");
                        hay_resultados |= coincide("Box Collider");
                        hay_resultados |= coincide("Sphere Collider");
                        hay_resultados |= coincide("Capsule Collider");
                        hay_resultados |= coincide("Cylinder Collider");
                        hay_resultados |= coincide("Mesh Collider");
                        hay_resultados |= coincide("Script");
                        hay_resultados |= coincide("AudioSource");
                        hay_resultados |= coincide("CharacterController");
                        hay_resultados |= coincide("Camera");
                        hay_resultados |= coincide("Light");
                        hay_resultados |= coincide("Canvas");
                        hay_resultados |= coincide("RectTransform");
                        hay_resultados |= coincide("Text");
                        hay_resultados |= coincide("Button");
                        hay_resultados |= coincide("Image");
                        hay_resultados |= coincide("DragDrop");

                        if !hay_resultados && !q.is_empty() {
                            ui.label(RichText::new("Sin componentes que coincidan.").weak());
                        }

                        if coincide("MeshRenderer") && entity.mesh.is_none() && ui.button("🎨 MeshRenderer & Material").clicked() {
                            entity.mesh = Some(MeshData::default());
                            entity.components.push("MeshRenderer".into());
                            entity.components.push("Material".into());
                            self.show_add_component_menu = false;
                        }

                        if entity.physics.is_none() {
                            if coincide("Rigidbody") && ui.button("⚙ Rigidbody").clicked() {
                                entity.physics = Some(PhysicsData {
                                    has_rigidbody: true,
                                    has_collider: false,
                                    ..PhysicsData::default()
                                });
                                entity.components.push("RigidBody".into());
                                self.show_add_component_menu = false;
                            }
                            // P3: cada collider es su componente; estas
                            // entradas crean la PhysicsData al vuelo.
                            if botones_collider_menu(ui, entity, &modelo_entidad, &q) {
                                self.show_add_component_menu = false;
                            }
                        } else if let Some(ref mut phys) = entity.physics {
                            if coincide("Rigidbody") && !phys.has_rigidbody && ui.button("⚙ Añadir Rigidbody").clicked() {
                                phys.has_rigidbody = true;
                                // Un cuerpo que era «Static» SOLO por no
                                // tener Rigidbody (el Collider solo nace
                                // estático) pasa a dinámico al añadirlo,
                                // como en Unity: si no, el Rigidbody nuevo
                                // quedaría fijo y no caería. Un tipo
                                // deliberado (Kinematic) no se toca.
                                if phys.body_type == "Static" {
                                    phys.body_type = "Dynamic".into();
                                }
                                entity.components.push("RigidBody".into());
                                self.show_add_component_menu = false;
                            }
                        }

                        // P3: con física ya presente, los colliders se
                        // añaden uno a uno y son repetibles (fuera del
                        // préstamo de `phys`, el helper toma la entidad).
                        if entity.physics.is_some()
                            && botones_collider_menu(ui, entity, &modelo_entidad, &q)
                        {
                            self.show_add_component_menu = false;
                        }

                        if coincide("Script") && entity.script.is_none() && ui.button("📜 Script C# (.NET CoreCLR)").clicked() {
                            entity.script = Some("GlassEngine.Core.Scripts.PlayerController".into());
                            entity.script_data = Some(ScriptComponentData::default());
                            entity.components.push("Script".into());
                            self.show_add_component_menu = false;
                        }

                        if coincide("AudioSource") && entity.audio.is_none() && ui.button("♫ AudioSource").clicked() {
                            entity.audio = Some(AudioSourceData::default());
                            entity.components.push("AudioSource".into());
                            self.show_add_component_menu = false;
                        }

                        if coincide("CharacterController") && entity.character.is_none() && ui.button("🚶 Character Controller").clicked() {
                            entity.character = Some(CharacterData::default());
                            entity.components.push("CharacterController".into());
                            self.show_add_component_menu = false;
                        }

                        if coincide("Camera") && entity.camera.is_none() && ui.button("🎥 Cámara (Camera)").clicked() {
                            entity.camera = Some(CameraData::default());
                            entity.components.push("Camera".into());
                            self.show_add_component_menu = false;
                        }

                        if coincide("Light") && entity.light.is_none() && ui.button("☼ Luz (Light)").clicked() {
                            entity.light = Some(LightData::default());
                            entity.components.push("Light".into());
                            self.show_add_component_menu = false;
                        }

                        if coincide("Canvas") && entity.canvas.is_none() && ui.button("🖼 Canvas").clicked() {
                            entity.canvas = Some(CanvasData::default());
                            entity.components.push("Canvas".into());
                            self.show_add_component_menu = false;
                        }

                        if coincide("RectTransform") && entity.rect_transform.is_none() && ui.button("⊡ RectTransform").clicked() {
                            entity.rect_transform = Some(RectTransformData::default());
                            entity.components.push("RectTransform".into());
                            self.show_add_component_menu = false;
                        }

                        if coincide("Text") && entity.ui_text.is_none() && ui.button("🔤 Text (TMP)").clicked() {
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
