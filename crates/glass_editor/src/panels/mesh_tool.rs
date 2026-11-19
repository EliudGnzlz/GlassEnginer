//! Panel «MeshForge»: edición de malla sobre `MeshSession` nacida de la
//! entidad seleccionada en la escena (modelo ProBuilder). La vista 3D propia
//! se retira: picking, gizmo y grab vuelven en R2 contra la vista de escena.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use glass_core::scene::{EntityData, MeshColliderMode, SceneState};
use glass_mesh_edit::{
    read_glb, BooleanOp, MeshCommand, MeshEditError, MeshSession, MirrorAxis,
};

use crate::panels::mesh_birth::{self, Editabilidad};
use crate::panels::mesh_keymap::{action_for, help_text, MeshKeyAction};
use crate::tool_prefs::{ShortcutScheme, ToolPrefs};

/// Modo de selección sub-objeto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    Verts,
    Faces,
}

/// Entidad seleccionada en la escena, tal como la ve el panel (la construye docking).
#[derive(Debug, Clone, PartialEq)]
pub struct EntidadSeleccionada {
    pub id: u64,
    pub nombre: String,
    /// None = la entidad no tiene componente de malla.
    pub modelo: Option<String>,
    pub tiene_mesh_collider: bool,
}

/// Asignación de modelo pendiente de aplicar sobre la escena: la deja
/// «Hornear y salir» y la aplica la capa de docking (la entidad pasa al
/// modelo horneado; si adapta_collider, el motor reconstruye el mesh
/// collider desde la nueva geometría por la vía de carga de modelos).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelAssign {
    pub entity_id: u64,
    pub new_model: String,
    pub adapta_collider: bool,
}

pub struct MeshToolPanel {
    pub session: Option<MeshSession>,
    /// Último resultado o error, en texto plano.
    pub status: String,
    pub dims: [f32; 3],
    pub array_count: u32,
    pub array_offset: [f32; 3],
    pub move_delta: [f32; 3],
    pub mirror_axis: MirrorAxis,
    pub weld_tolerance: f32,
    pub boolean_path: String,
    pub boolean_op: BooleanOp,
    /// Preferencias de la herramienta (esquema de atajos elegido).
    pub prefs: ToolPrefs,
    /// Archivo donde se persisten las preferencias.
    pub prefs_file: std::path::PathBuf,
    /// El puntero está sobre el panel (según el rect del frame anterior).
    pub panel_hovered: bool,
    last_rect: Option<egui::Rect>,
    /// Pregunta del Mesh Collider abierta, a la espera de respuesta.
    collider_prompt_open: bool,
    /// Checkbox «No volver a preguntar» de la pregunta en curso.
    collider_prompt_no_ask: bool,
    /// Raíz de Assets del proyecto abierto (la fija docking cada frame).
    pub assets_root: std::path::PathBuf,
    /// Entidad de la que nació la sesión en curso.
    pub target_entity: Option<u64>,
    /// Nombre visible de esa entidad (para la cabecera).
    pub target_entity_name: String,
    /// Modelo base de la sesión (stem normalizado).
    pub base_model: String,
    /// «Trabajar en la escena» pedido desde la tarjeta del espacio.
    pub peticion_trabajar_escena: bool,
    /// Asignación de modelo que aplica docking al hornear y salir.
    pub pending_model_assign: Option<ModelAssign>,
    /// Modo de selección sub-objeto.
    pub selection_mode: SelectionMode,
    /// Vértices seleccionados.
    pub selected_verts: std::collections::HashSet<u32>,
    /// Caras seleccionadas.
    pub selected_faces: std::collections::HashSet<u32>,
    /// Snap activo para los desplazamientos del gizmo.
    pub snap_enabled: bool,
    /// Paso de snap.
    pub snap_step: f32,
    /// Estado de arrastre/gizmo contra la vista de escena.
    pub view: crate::panels::mesh_view::MeshViewState,
    /// El puntero está sobre el viewport (lo fija docking cada frame).
    pub viewport_hovered: bool,
}

impl Default for MeshToolPanel {
    fn default() -> Self {
        let prefs_file = crate::tool_prefs::prefs_path();
        Self {
            session: None,
            prefs: ToolPrefs::load_from(&prefs_file),
            prefs_file,
            panel_hovered: false,
            last_rect: None,
            collider_prompt_open: false,
            collider_prompt_no_ask: false,
            assets_root: PathBuf::from("assets"),
            target_entity: None,
            target_entity_name: String::new(),
            base_model: String::new(),
            peticion_trabajar_escena: false,
            pending_model_assign: None,
            selection_mode: SelectionMode::Verts,
            selected_verts: std::collections::HashSet::new(),
            selected_faces: std::collections::HashSet::new(),
            snap_enabled: true,
            snap_step: 0.25,
            view: crate::panels::mesh_view::MeshViewState::default(),
            viewport_hovered: false,
            status: "Sin sesión: selecciona un objeto en la escena.".to_owned(),
            dims: [2.0, 2.0, 2.0],
            array_count: 2,
            array_offset: [2.0, 0.0, 0.0],
            move_delta: [0.0, 0.0, 0.0],
            mirror_axis: MirrorAxis::X,
            weld_tolerance: 1e-4,
            boolean_path: String::new(),
            boolean_op: BooleanOp::Union,
        }
    }
}

/// Acción pedida desde la UI; se ejecuta tras soltar los préstamos de `render_panel`.
enum Action {
    Undo,
    Redo,
    SetDims,
    Cmd(MeshCommand),
    Boolean,
}

/// Normaliza el nombre de modelo: "" equivale a la primitiva "cube".
fn normalize_model(model: &str) -> &str {
    if model.is_empty() {
        "cube"
    } else {
        model
    }
}

/// Entidades cuyo modelo es `model` y que tienen un Mesh Collider puesto
/// por el desarrollador. Los colliders primitivos o compuestos NO cuentan:
/// con ellos no se pregunta nada al salir.
pub fn entities_with_mesh_collider(entities: &[EntityData], model: &str) -> Vec<u64> {
    let target = normalize_model(model);
    entities
        .iter()
        .filter(|e| e.mesh.as_ref().map(|m| normalize_model(&m.model)) == Some(target))
        .filter(|e| {
            e.physics
                .as_ref()
                .is_some_and(|p| p.mesh_collider != MeshColliderMode::None)
        })
        .map(|e| e.id)
        .collect()
}

/// ¿Hay que preguntar por el collider al cerrar? Solo con Mesh Collider,
/// cambios en vigor y sin el «no volver a preguntar».
pub fn should_prompt_collider(has_mesh_collider: bool, history_len: usize, no_ask: bool) -> bool {
    has_mesh_collider && history_len > 0 && !no_ask
}

fn selection_indices_for(
    mode: SelectionMode,
    verts: &std::collections::HashSet<u32>,
    faces: &std::collections::HashSet<u32>,
    mesh_faces: Option<&[[u32; 3]]>,
) -> Vec<u32> {
    let mut out: Vec<u32> = match mode {
        SelectionMode::Verts => verts.iter().copied().collect(),
        SelectionMode::Faces => match mesh_faces {
            Some(mesh_faces) => faces
                .iter()
                .filter_map(|&f| mesh_faces.get(f as usize))
                .flat_map(|f| f.iter().copied())
                .collect(),
            None => Vec::new(),
        },
    };
    out.sort_unstable();
    out.dedup();
    out
}

fn toggle_or_set(
    active: &mut std::collections::HashSet<u32>,
    other: &mut std::collections::HashSet<u32>,
    id: u32,
    additive: bool,
) {
    if additive {
        if !active.remove(&id) {
            active.insert(id);
        }
    } else {
        active.clear();
        other.clear();
        active.insert(id);
    }
}

fn selection_status(mode: SelectionMode, verts: usize, faces: usize) -> String {
    match mode {
        SelectionMode::Verts if verts > 0 => format!("Seleccionado: {verts} vértices."),
        SelectionMode::Faces if faces > 0 => format!("Seleccionado: {faces} caras."),
        _ => "Sin selección.".to_string(),
    }
}

/// Delta final del gizmo (con snap si está activo). None si queda a cero.
fn gizmo_commit_delta(delta: [f32; 3], snap_enabled: bool, step: f32) -> Option<[f32; 3]> {
    let delta = if snap_enabled {
        glass_mesh_edit::preview::snap_delta(delta, step)
    } else {
        delta
    };
    if delta.iter().all(|v| v.abs() <= 1e-6) {
        None
    } else {
        Some(delta)
    }
}

/// `<dir>/<stem>_edited.<ext>` y `<dir>/<stem>_edited.mesh.json`; sin ruta de
/// origen, `assets/<nombre>_edited.glb` y `assets/<nombre>_edited.mesh.json`.
fn bake_paths(session: &MeshSession) -> (PathBuf, PathBuf) {
    match &session.source_path {
        Some(src) => {
            let stem = src
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| session.name.clone());
            let ext = src
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_else(|| "glb".to_owned());
            let dir = src.parent().map(Path::to_path_buf).unwrap_or_default();
            (
                dir.join(format!("{stem}_edited.{ext}")),
                dir.join(format!("{stem}_edited.mesh.json")),
            )
        }
        None => {
            let dir = PathBuf::from("assets");
            (
                dir.join(format!("{}_edited.glb", session.name)),
                dir.join(format!("{}_edited.mesh.json", session.name)),
            )
        }
    }
}

fn drag_vec3(ui: &mut egui::Ui, v: &mut [f32; 3], speed: f64) {
    for (label, value) in ["X", "Y", "Z"].iter().zip(v.iter_mut()) {
        ui.label(*label);
        ui.add(egui::DragValue::new(value).speed(speed));
    }
}

impl MeshToolPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Limpia la selección sub-objeto y el estado de arrastre del gizmo.
    pub fn clear_selection(&mut self) {
        self.selected_verts.clear();
        self.selected_faces.clear();
        self.view.grab = None;
        self.view.grab_requested = false;
        self.view.gizmo_axis = None;
        self.view.drag_delta_world = [0.0; 3];
    }

    /// Índices de vértice que abarca la selección actual (caras expandidas).
    pub fn selection_vert_indices(&self) -> Vec<u32> {
        selection_indices_for(
            self.selection_mode,
            &self.selected_verts,
            &self.selected_faces,
            self.session.as_ref().map(|s| s.mesh.faces.as_slice()),
        )
    }

    /// Centroide de los vértices seleccionados en espacio local.
    pub fn centroide_seleccion(&self) -> Option<[f32; 3]> {
        let session = self.session.as_ref()?;
        let indices: std::collections::HashSet<u32> =
            self.selection_vert_indices().into_iter().collect();
        glass_mesh_edit::preview::selection_centroid(&session.mesh.verts, &indices)
    }

    /// Aplica el snap configurado a un desplazamiento local.
    pub fn snap_local(&self, d: [f32; 3]) -> [f32; 3] {
        if self.snap_enabled {
            glass_mesh_edit::preview::snap_delta(d, self.snap_step)
        } else {
            d
        }
    }

    /// Picking sub-objeto contra el rayo local que llega del viewport.
    pub fn pick_viewport(
        &mut self,
        origin_local: [f32; 3],
        dir_local: [f32; 3],
        point_rel: [f32; 2],
        project: &dyn Fn([f32; 3]) -> Option<([f32; 2], f32)>,
        additive: bool,
    ) {
        let (face_pick, vert_pick, hubo_impacto) = {
            let Some(session) = self.session.as_ref() else {
                return;
            };
            // En modo Vértices el picking por pantalla manda primero: un
            // vértice visible bajo el cursor se selecciona aunque el rayo
            // no atraviese ninguna cara (p. ej. un vértice de silueta).
            let directo = if self.selection_mode == SelectionMode::Verts {
                glass_mesh_edit::preview::pick_vertex_with(
                    &session.mesh.verts,
                    point_rel,
                    14.0,
                    project,
                )
            } else {
                None
            };
            match directo {
                Some(vertice) => (None, Some(vertice), true),
                None => match glass_mesh_edit::preview::pick_face(
                &session.mesh.verts,
                &session.mesh.faces,
                origin_local,
                dir_local,
            ) {
                Some((face, _t)) if self.selection_mode == SelectionMode::Faces => {
                    (Some(face), None, true)
                }
                Some((face, t)) => {
                    let vertice = glass_mesh_edit::preview::pick_vertex_with(
                        &session.mesh.verts,
                        point_rel,
                        14.0,
                        project,
                    )
                    .or_else(|| {
                        let impacto = [
                            origin_local[0] + dir_local[0] * t,
                            origin_local[1] + dir_local[1] * t,
                            origin_local[2] + dir_local[2] * t,
                        ];
                        let mut mejor: Option<(u32, f32)> = None;
                        let indices_cara = session
                            .mesh
                            .faces
                            .get(face as usize)
                            .copied()
                            .unwrap_or([0; 3]);
                        for indice in indices_cara {
                            let Some(p) = session.mesh.verts.get(indice as usize) else {
                                continue;
                            };
                            let dx = p[0] - impacto[0];
                            let dy = p[1] - impacto[1];
                            let dz = p[2] - impacto[2];
                            let d2 = dx * dx + dy * dy + dz * dz;
                            let mejor_candidato = match mejor {
                                None => true,
                                Some((_, mejor_d2)) => d2 < mejor_d2,
                            };
                            if mejor_candidato {
                                mejor = Some((indice, d2));
                            }
                        }
                        mejor.map(|(indice, _)| indice)
                    });
                    (None, vertice, true)
                }
                None => (None, None, false),
            },
            }
        };

        match (face_pick, vert_pick) {
            (Some(face), _) => {
                toggle_or_set(
                    &mut self.selected_faces,
                    &mut self.selected_verts,
                    face,
                    additive,
                );
            }
            (_, Some(vertice)) => {
                toggle_or_set(
                    &mut self.selected_verts,
                    &mut self.selected_faces,
                    vertice,
                    additive,
                );
            }
            _ => {
                if !additive && !hubo_impacto {
                    self.selected_verts.clear();
                    self.selected_faces.clear();
                }
            }
        }

        self.status = selection_status(
            self.selection_mode,
            self.selected_verts.len(),
            self.selected_faces.len(),
        );
    }

    /// Confirma el arrastre del gizmo con el delta local ya calculado.
    pub fn commit_drag_local(&mut self, raw_local: [f32; 3]) {
        let Some(delta) = gizmo_commit_delta(raw_local, self.snap_enabled, self.snap_step) else {
            self.status = "Sin desplazamiento.".to_string();
            return;
        };
        let indices = self.selection_vert_indices();
        if indices.is_empty() {
            return;
        }
        self.apply_cmd(MeshCommand::MoveVerts { indices, delta });
    }

    /// Entidad de escena cuya malla se está editando, si hay sesión.
    pub fn entidad_en_edicion(&self) -> Option<u64> {
        self.session.is_some().then_some(self.target_entity).flatten()
    }

    /// Abre una sesión desde un .glb por ruta (API pública; ya no está en la UI).
    /// La sesión no nace de ninguna entidad: al hornear y salir no se asigna nada.
    pub fn open(&mut self, path: &str) -> Result<(), MeshEditError> {
        match MeshSession::enter_from_glb(Path::new(path)) {
            Ok(mut session) => {
                let (stats, _) = session.stats();
                self.status = format!(
                    "Abierto {}: {} vértices, {} caras.",
                    session.name, stats.verts, stats.faces
                );
                self.target_entity = None;
                self.target_entity_name.clear();
                self.base_model = session.name.clone();
                self.collider_prompt_open = false;
                self.session = Some(session);
                Ok(())
            }
            Err(e) => {
                self.status = format!("No se pudo abrir {path}: {e}");
                Err(e)
            }
        }
    }

    pub fn close(&mut self) {
        self.session = None;
        self.target_entity = None;
        self.target_entity_name.clear();
        self.base_model.clear();
        self.collider_prompt_open = false;
        self.status = "Sesión cerrada.".to_owned();
        self.clear_selection();
    }

    /// Editabilidad de `modelo` según las raíces de modelos del proyecto.
    fn editabilidad_de(&self, modelo: &str) -> Editabilidad {
        let raices = mesh_birth::raices_modelos(&self.assets_root);
        let raices: Vec<&Path> = raices.iter().map(PathBuf::as_path).collect();
        mesh_birth::editabilidad(modelo, &raices)
    }

    /// Abre la sesión de edición desde la entidad seleccionada en la escena.
    pub fn editar_entidad(&mut self, sel: &EntidadSeleccionada) {
        if self.session.is_some() {
            self.status =
                "Ya hay una sesión de edición abierta: hornea o descarta antes de editar otra entidad."
                    .to_owned();
            return;
        }
        let Some(modelo) = sel.modelo.as_deref() else {
            self.status = format!(
                "La entidad «{}» no tiene malla: no se puede editar.",
                sel.nombre
            );
            return;
        };
        let raices = mesh_birth::raices_modelos(&self.assets_root);
        let raices: Vec<&Path> = raices.iter().map(PathBuf::as_path).collect();
        if let Editabilidad::NoEditable(motivo) = mesh_birth::editabilidad(modelo, &raices) {
            self.status = motivo;
            return;
        }
        match mesh_birth::abrir_sesion(modelo, &raices) {
            Ok(mut session) => {
                let (stats, _) = session.stats();
                self.status = format!(
                    "Editando «{}» ({modelo}): {} vértices, {} caras.",
                    sel.nombre, stats.verts, stats.faces
                );
                self.target_entity = Some(sel.id);
                self.target_entity_name = sel.nombre.clone();
                self.base_model = mesh_birth::normalize_model(modelo).to_owned();
                self.collider_prompt_open = false;
                self.session = Some(session);
                self.clear_selection();
            }
            Err(e) => self.status = e,
        }
    }

    /// ¿La entidad destino tiene ahora mismo un Mesh Collider en la escena?
    /// Si ya no existe, cuenta como sin collider.
    fn target_tiene_mesh_collider(&self, scene_state: &Arc<Mutex<SceneState>>) -> bool {
        let Some(id) = self.target_entity else {
            return false;
        };
        let scene = scene_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        scene
            .entities
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| e.physics.as_ref())
            .is_some_and(|p| p.mesh_collider != MeshColliderMode::None)
    }

    /// Cierra la sesión sin escribir archivos ni tocar la escena.
    fn descartar(&mut self) {
        self.close();
        self.status = "Sesión descartada: la escena no se ha tocado.".to_owned();
    }

    /// «Hornear y salir»: con Mesh Collider en la entidad destino pregunta
    /// antes (o respeta el «no volver a preguntar»); sin él hornea y asigna.
    fn hornear_y_salir(&mut self, scene_state: &Arc<Mutex<SceneState>>) {
        let Some(history_len) = self.session.as_ref().map(|s| s.history.len()) else {
            return;
        };
        if history_len == 0 {
            self.close();
            self.status = "Sin cambios: nada que hornear.".to_owned();
            return;
        }
        if self.target_tiene_mesh_collider(scene_state) {
            if should_prompt_collider(true, history_len, self.prefs.collider_no_ask) {
                self.collider_prompt_open = true;
                self.collider_prompt_no_ask = false;
                self.status = "El modelo tenía un Mesh Collider.".to_owned();
            } else {
                self.close();
                self.status = "Sesión cerrada sin adaptar el collider (preferencia guardada: no volver a preguntar).".to_owned();
            }
            return;
        }
        if let Ok(Some(nombre)) = self.hornear_y_asignar(false) {
            self.status = format!("Horneado y asignado a «{nombre}».");
        }
    }

    /// Hornea al directorio del proyecto, cierra y deja la asignación para
    /// docking. Devuelve el nombre de la entidad destino (None si la sesión
    /// no nació de una entidad). Si el horneado falla, la sesión sigue abierta
    /// y el error queda en el status.
    fn hornear_y_asignar(&mut self, adapta_collider: bool) -> Result<Option<String>, MeshEditError> {
        self.bake_escena()?;
        let target = self
            .target_entity
            .map(|id| (id, self.target_entity_name.clone()));
        let new_model = format!("{}_edited", self.base_model);
        let horneado = std::mem::take(&mut self.status);
        self.close();
        match target {
            Some((entity_id, nombre)) => {
                self.pending_model_assign = Some(ModelAssign {
                    entity_id,
                    new_model,
                    adapta_collider,
                });
                Ok(Some(nombre))
            }
            None => {
                self.status = horneado;
                Ok(None)
            }
        }
    }

    /// Aplica la respuesta a la pregunta del Mesh Collider.
    fn responder_collider(&mut self, adaptar: bool) {
        if !self.collider_prompt_open {
            return;
        }
        self.collider_prompt_open = false;
        let mut prefs_error = None;
        if self.collider_prompt_no_ask {
            self.prefs.collider_no_ask = true;
            if let Err(e) = self.prefs.save_to(&self.prefs_file) {
                prefs_error = Some(e);
            }
        }
        if adaptar {
            if self.hornear_y_asignar(true).is_err() {
                // La pregunta se descarta y la sesión sigue abierta;
                // bake_escena() ya dejó el error en el status.
                return;
            }
            self.status =
                "Sesión cerrada: el mesh collider se adaptará al nuevo modelo.".to_string();
        } else {
            self.close();
        }
        if let Some(e) = prefs_error {
            self.status
                .push_str(&format!(" (No se pudo guardar la preferencia: {e})"));
        }
    }

    pub fn apply_cmd(&mut self, cmd: MeshCommand) {
        let Some(session) = self.session.as_mut() else {
            self.status = "Sin sesión.".to_owned();
            return;
        };
        let name = cmd.name();
        self.status = match session.apply(cmd) {
            Ok(()) => format!("{name} aplicado."),
            Err(e) => format!("Error en {name}: {e}"),
        };
    }

    pub fn undo(&mut self) {
        let Some(session) = self.session.as_mut() else {
            self.status = "Sin sesión: nada que deshacer.".to_owned();
            return;
        };
        self.status = match session.undo() {
            Some(cmd) => format!("Deshecho: {}.", cmd.name()),
            None => "Nada que deshacer.".to_owned(),
        };
    }

    pub fn redo(&mut self) {
        let Some(session) = self.session.as_mut() else {
            self.status = "Sin sesión: nada que rehacer.".to_owned();
            return;
        };
        self.status = match session.redo() {
            Some(cmd) => format!("Rehecho: {}.", cmd.name()),
            None => "Nada que rehacer.".to_owned(),
        };
    }

    pub fn bake(&mut self) -> Result<(), MeshEditError> {
        let Some(session) = self.session.as_ref() else {
            self.status = "Sin sesión: nada que hornear.".to_owned();
            return Err(MeshEditError::Gltf("sin sesión".into()));
        };
        let (glb, recipe) = bake_paths(session);
        match session.save(&glb, &recipe) {
            Ok(()) => {
                self.status = format!("Horneado: {} + {}.", glb.display(), recipe.display());
                Ok(())
            }
            Err(e) => {
                self.status = format!("Error al hornear: {e}");
                Err(e)
            }
        }
    }

    /// Horneado de la sesión de escena al directorio de modelos del proyecto:
    /// `<dir_horneado>/<base_model>_edited.glb` + `.mesh.json`.
    fn bake_escena(&mut self) -> Result<(PathBuf, PathBuf), MeshEditError> {
        let Some(session) = self.session.as_ref() else {
            self.status = "Sin sesión: nada que hornear.".to_owned();
            return Err(MeshEditError::Gltf("sin sesión".into()));
        };
        let dir = mesh_birth::dir_horneado(&self.assets_root);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.status = format!("No se pudo crear {}: {e}", dir.display());
            return Err(MeshEditError::Gltf(format!(
                "no se pudo crear {}: {e}",
                dir.display()
            )));
        }
        let glb = dir.join(format!("{}_edited.glb", self.base_model));
        let recipe = dir.join(format!("{}_edited.mesh.json", self.base_model));
        match session.save(&glb, &recipe) {
            Ok(()) => {
                self.status = format!("Horneado: {} + {}.", glb.display(), recipe.display());
                Ok((glb, recipe))
            }
            Err(e) => {
                self.status = format!("Error al hornear: {e}");
                Err(e)
            }
        }
    }

    fn apply_dimensions(&mut self) {
        let dims = self.dims;
        let Some(session) = self.session.as_mut() else {
            self.status = "Sin sesión.".to_owned();
            return;
        };
        self.status = match session.set_dimensions(dims) {
            Ok(()) => format!(
                "Dimensiones aplicadas: {:.3} × {:.3} × {:.3}.",
                dims[0], dims[1], dims[2]
            ),
            Err(e) => format!("Error al aplicar dimensiones: {e}"),
        };
    }

    fn apply_boolean(&mut self) {
        if self.session.is_none() {
            self.status = "Sin sesión.".to_owned();
            return;
        }
        let path = self.boolean_path.trim().to_owned();
        if path.is_empty() {
            self.status = "Indica la ruta del segundo .glb.".to_owned();
            return;
        }
        match read_glb(Path::new(&path)) {
            Ok(other) => {
                let op = self.boolean_op;
                self.apply_cmd(MeshCommand::Boolean { op, other });
            }
            Err(e) => self.status = format!("No se pudo leer {path}: {e}"),
        }
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::SetDims => self.apply_dimensions(),
            Action::Cmd(cmd) => self.apply_cmd(cmd),
            Action::Boolean => self.apply_boolean(),
        }
    }

    /// Fija el esquema de atajos de malla y lo persiste.
    pub fn set_scheme(&mut self, scheme: ShortcutScheme) {
        self.prefs.scheme = Some(scheme);
        self.status = match self.prefs.save_to(&self.prefs_file) {
            Ok(()) => match scheme {
                ShortcutScheme::Blender => "Atajos de edición de malla: Blender.".to_string(),
                ShortcutScheme::Unity => "Atajos de edición de malla: Unity.".to_string(),
            },
            Err(e) => format!("No se pudo guardar la preferencia de atajos: {e}"),
        };
    }

    /// Ejecuta una acción de teclado de edición de malla.
    pub fn run_key_action(&mut self, action: MeshKeyAction) {
        match action {
            MeshKeyAction::Undo => self.undo(),
            MeshKeyAction::Redo => self.redo(),
            MeshKeyAction::Bake => {
                let _ = self.bake();
            }
            MeshKeyAction::Mirror => {
                self.apply_cmd(MeshCommand::Mirror {
                    axis: self.mirror_axis,
                });
            }
            MeshKeyAction::DeleteSelection => {
                let indices = self.selection_vert_indices();
                if indices.is_empty() {
                    self.status = "Sin selección que eliminar.".to_string();
                } else {
                    self.apply_cmd(MeshCommand::DeleteVerts { indices });
                    self.selected_verts.clear();
                    self.selected_faces.clear();
                }
            }
            MeshKeyAction::ModeVerts => self.selection_mode = SelectionMode::Verts,
            MeshKeyAction::ModeFaces => self.selection_mode = SelectionMode::Faces,
            MeshKeyAction::Grab => {
                let hay_seleccion = !self.selection_vert_indices().is_empty();
                if self.session.is_some() && hay_seleccion {
                    self.view.grab_requested = true;
                    self.status = "Moviendo la selección: mueve el ratón y haz clic en la vista de escena para confirmar · Esc para cancelar.".to_string();
                } else {
                    self.status = "Selecciona vértices o caras antes de mover.".to_string();
                }
            }
        }
    }

    /// Fila de sesión: deshacer/rehacer y las dos salidas.
    fn zona_sesion(
        &self,
        ui: &mut egui::Ui,
        action: &mut Option<Action>,
        hornear_salir: &mut bool,
        descartar: &mut bool,
    ) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Deshacer").clicked() {
                *action = Some(Action::Undo);
            }
            if ui.button("Rehacer").clicked() {
                *action = Some(Action::Redo);
            }
            if ui
                .add_enabled(!self.collider_prompt_open, egui::Button::new("Hornear y salir"))
                .clicked()
            {
                *hornear_salir = true;
            }
            if ui.button("Descartar").clicked() {
                *descartar = true;
            }
        });
    }

    /// Zona de operadores: Dimensiones y Operadores.
    fn zona_operadores(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>, num_verts: usize) {
        ui.separator();
        ui.label("Dimensiones");
        ui.horizontal_wrapped(|ui| {
            drag_vec3(ui, &mut self.dims, 0.05);
            if ui.button("Aplicar dimensiones").clicked() {
                *action = Some(Action::SetDims);
            }
        });

        ui.separator();
        ui.label("Operadores");
        if ui.button("Subdividir").clicked() {
            *action = Some(Action::Cmd(MeshCommand::Subdivide { levels: 1 }));
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mirror_axis, MirrorAxis::X, "X");
            ui.selectable_value(&mut self.mirror_axis, MirrorAxis::Y, "Y");
            ui.selectable_value(&mut self.mirror_axis, MirrorAxis::Z, "Z");
            if ui.button("Espejo").clicked() {
                *action = Some(Action::Cmd(MeshCommand::Mirror {
                    axis: self.mirror_axis,
                }));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Tolerancia");
            ui.add(
                egui::DragValue::new(&mut self.weld_tolerance)
                    .speed(1e-5)
                    .max_decimals(6),
            );
            if ui.button("Soldar").clicked() {
                *action = Some(Action::Cmd(MeshCommand::Weld {
                    tolerance: self.weld_tolerance.max(0.0),
                }));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Copias");
            ui.add(egui::DragValue::new(&mut self.array_count).speed(0.1));
            ui.label("Offset");
            drag_vec3(ui, &mut self.array_offset, 0.05);
            if ui.button("Array").clicked() {
                *action = Some(Action::Cmd(MeshCommand::Array {
                    count: self.array_count,
                    offset: self.array_offset,
                }));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Delta");
            drag_vec3(ui, &mut self.move_delta, 0.05);
            if ui.button("Mover todos").clicked() {
                let indices = (0..num_verts)
                    .filter_map(|i| u32::try_from(i).ok())
                    .collect();
                *action = Some(Action::Cmd(MeshCommand::MoveVerts {
                    indices,
                    delta: self.move_delta,
                }));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Otro .glb:");
            ui.text_edit_singleline(&mut self.boolean_path);
        });
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.boolean_op, BooleanOp::Union, "Unión");
            ui.selectable_value(&mut self.boolean_op, BooleanOp::Difference, "Resta");
            ui.selectable_value(
                &mut self.boolean_op,
                BooleanOp::Intersection,
                "Intersección",
            );
            if ui.button("Booleana").clicked() {
                *action = Some(Action::Boolean);
            }
        });
    }

    /// Fila de elección/cambio del esquema de atajos (visible haya o no sesión).
    fn render_scheme_prefs(&mut self, ui: &mut egui::Ui) {
        let mut elegido: Option<ShortcutScheme> = None;
        match self.prefs.scheme {
            None => {
                ui.label(egui::RichText::new("Primera vez en MeshForge").strong());
                ui.label("¿Qué atajos quieres usar mientras editas la malla? En el resto del editor mandan los de Unity, como siempre.");
                ui.horizontal(|ui| {
                    if ui.button("Esquema Blender").clicked() {
                        elegido = Some(ShortcutScheme::Blender);
                    }
                    if ui.button("Esquema Unity").clicked() {
                        elegido = Some(ShortcutScheme::Unity);
                    }
                });
            }
            Some(actual) => {
                let mut scheme_local = actual;
                ui.horizontal(|ui| {
                    ui.label("Atajos de malla:");
                    ui.selectable_value(&mut scheme_local, ShortcutScheme::Blender, "Blender");
                    ui.selectable_value(&mut scheme_local, ShortcutScheme::Unity, "Unity");
                });
                ui.label(egui::RichText::new(help_text(scheme_local)).weak());
                if scheme_local != actual {
                    elegido = Some(scheme_local);
                }
            }
        }
        if let Some(esquema) = elegido {
            self.set_scheme(esquema);
        }
    }

    /// Lee como mucho una acción de teclado por frame. Sin el ratón sobre el
    /// panel, ninguna tecla es de la malla (son del resto del editor).
    fn read_mesh_key(&mut self, ui: &egui::Ui) -> Option<MeshKeyAction> {
        let scheme = self.prefs.scheme?;
        if self.session.is_none() || ui.ctx().egui_wants_keyboard_input() {
            return None;
        }
        if self.view.grab.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.view.grab = None;
            self.view.grab_requested = false;
            self.status = "Movimiento cancelado.".to_string();
            return None;
        }
        if !self.panel_hovered && !self.viewport_hovered {
            return None;
        }
        const CANDIDATAS: [egui::Key; 10] = [
            egui::Key::Z,
            egui::Key::Y,
            egui::Key::S,
            egui::Key::M,
            egui::Key::G,
            egui::Key::X,
            egui::Key::W,
            egui::Key::Delete,
            egui::Key::Num1,
            egui::Key::Num2,
        ];
        ui.input(|i| {
            CANDIDATAS
                .iter()
                .copied()
                .filter(|k| i.key_pressed(*k))
                .find_map(|k| action_for(scheme, k, i.modifiers))
        })
    }

    /// Pestaña lateral «🧊 MeshForge» del editor: sin sesión ofrece editar la
    /// entidad seleccionada; con sesión, la cabecera, la salida y los operadores.
    pub fn render_panel(
        &mut self,
        ui: &mut egui::Ui,
        scene_state: &Arc<Mutex<SceneState>>,
        seleccion: Option<&EntidadSeleccionada>,
    ) {
        // Hover determinista: rect del panel en el frame anterior.
        self.panel_hovered = ui
            .input(|i| i.pointer.hover_pos())
            .zip(self.last_rect)
            .is_some_and(|(p, r)| r.contains(p));
        self.render_scheme_prefs(ui);
        let key_action = self.read_mesh_key(ui);
        let mut action: Option<Action> = None;
        let mut hornear_salir = false;
        let mut descartar = false;
        let mut collider_answer: Option<bool> = None;
        let mut preguntar_de_nuevo = false;
        let mut editar = false;

        // Foto de cabecera: stats() exige &mut, ámbito cerrado para soltar el préstamo.
        let cabecera = self.session.as_mut().map(|s| {
            let (stats, report) = s.stats();
            (stats, report, s.recipe.commands.len(), s.mesh.verts.len())
        });

        match &cabecera {
            None => {
                ui.heading("🧊 MeshForge");
                match seleccion {
                    None => {
                        ui.label("Selecciona un objeto en la escena para editar su malla.");
                    }
                    Some(sel) => {
                        ui.strong(sel.nombre.as_str());
                        match sel.modelo.as_deref() {
                            None => {
                                ui.weak(format!(
                                    "La entidad «{}» no tiene malla: no se puede editar.",
                                    sel.nombre
                                ));
                            }
                            Some(modelo) => {
                                ui.label(format!("Modelo: {modelo}"));
                                match self.editabilidad_de(modelo) {
                                    Editabilidad::Editable => {
                                        if ui.button("Editar malla").clicked() {
                                            editar = true;
                                        }
                                    }
                                    Editabilidad::NoEditable(motivo) => {
                                        ui.weak(motivo);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Some((stats, report, receta_len, num_verts)) => {
                ui.label(format!(
                    "Vértices: {} · Caras: {} · Aristas: {} · bbox [{:.3}, {:.3}, {:.3}] – [{:.3}, {:.3}, {:.3}] · Área: {:.3} · Manifold: {} · Receta: {} comandos",
                    stats.verts,
                    stats.faces,
                    stats.edges,
                    stats.bbox_min[0],
                    stats.bbox_min[1],
                    stats.bbox_min[2],
                    stats.bbox_max[0],
                    stats.bbox_max[1],
                    stats.bbox_max[2],
                    stats.surface_area,
                    if report.is_closed_manifold { "sí" } else { "no" },
                    receta_len,
                ));
                ui.label(format!(
                    "Editando: {} (modelo {})",
                    self.target_entity_name, self.base_model
                ));

                // Solo se consulta la escena cuando la preferencia lo hace relevante.
                if self.prefs.collider_no_ask && self.target_tiene_mesh_collider(scene_state) {
                    ui.horizontal_wrapped(|ui| {
                        ui.weak("Mesh Collider: al salir no se adaptará (no volver a preguntar).");
                        if ui.button("Preguntar de nuevo").clicked() {
                            preguntar_de_nuevo = true;
                        }
                    });
                }

                self.zona_sesion(ui, &mut action, &mut hornear_salir, &mut descartar);

                let mut limpiar_seleccion = false;
                ui.horizontal(|ui| {
                    ui.label("Selección");
                    ui.selectable_value(&mut self.selection_mode, SelectionMode::Verts, "Vértices");
                    ui.selectable_value(&mut self.selection_mode, SelectionMode::Faces, "Caras");
                    let conteo = match self.selection_mode {
                        SelectionMode::Verts => {
                            format!("{} vértices", self.selected_verts.len())
                        }
                        SelectionMode::Faces => format!("{} caras", self.selected_faces.len()),
                    };
                    ui.label(conteo);
                    if ui.button("Limpiar").clicked() {
                        limpiar_seleccion = true;
                    }
                });
                if limpiar_seleccion {
                    self.clear_selection();
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.snap_enabled, "Snap");
                    ui.label("Paso");
                    ui.add(
                        egui::DragValue::new(&mut self.snap_step)
                            .speed(0.05)
                            .max_decimals(2),
                    );
                });
                self.snap_step = self.snap_step.clamp(0.01, 2.0);

                // Pregunta del Mesh Collider pendiente de respuesta.
                if self.collider_prompt_open {
                    ui.group(|ui| {
                        ui.label(
                            egui::RichText::new(
                                "¿Quieres adaptar el mesh collider al nuevo modelo?",
                            )
                            .strong(),
                        );
                        ui.checkbox(&mut self.collider_prompt_no_ask, "No volver a preguntar");
                        ui.horizontal(|ui| {
                            if ui.button("Sí, adaptar").clicked() {
                                collider_answer = Some(true);
                            }
                            if ui.button("No").clicked() {
                                collider_answer = Some(false);
                            }
                        });
                    });
                }

                // Hueco reservado abajo para el status.
                let alto = (ui.available_height() - 60.0).max(80.0);
                egui::ScrollArea::vertical()
                    .id_salt("mesh_ops")
                    .max_height(alto)
                    .show(ui, |ui| {
                        self.zona_operadores(ui, &mut action, *num_verts);
                    });
            }
        }

        if editar {
            if let Some(sel) = seleccion {
                self.editar_entidad(sel);
            }
        }
        if let Some(action) = action {
            self.run(action);
        }
        if hornear_salir {
            self.hornear_y_salir(scene_state);
        }
        if descartar {
            self.descartar();
        }
        if let Some(adaptar) = collider_answer {
            self.responder_collider(adaptar);
        }
        if preguntar_de_nuevo {
            self.prefs.collider_no_ask = false;
            self.status = match self.prefs.save_to(&self.prefs_file) {
                Ok(()) => "Se volverá a preguntar por el Mesh Collider al salir.".to_string(),
                Err(e) => format!("No se pudo guardar la preferencia: {e}"),
            };
        }
        if let Some(accion) = key_action {
            self.run_key_action(accion);
        }

        ui.separator();
        ui.add(egui::Label::new(self.status.as_str()).wrap());
        self.last_rect = Some(ui.max_rect());
    }

    /// Tarjeta de MeshForge en el espacio Herramientas (docking pinta la lista).
    pub fn render_espacio(&mut self, ui: &mut egui::Ui, _scene_state: &Arc<Mutex<SceneState>>) {
        let Some(info) = crate::herramientas::herramientas_disponibles()
            .into_iter()
            .find(|h| h.id == "meshforge")
        else {
            return;
        };
        let estado = if self.session.is_some() {
            format!(
                "Editando «{}» ({}) — la edición sigue en el panel del Editor.",
                self.target_entity_name, self.base_model
            )
        } else {
            "Sin sesión de edición.".to_string()
        };
        let mut trabajar = false;
        ui.group(|ui| {
            ui.strong(format!("{} {}", info.icono, info.nombre));
            ui.weak(info.descripcion);
            ui.separator();
            ui.label(estado);
            if ui.button("Trabajar en la escena").clicked() {
                trabajar = true;
            }
            ui.weak("Selecciona un objeto en la vista de escena y pulsa «Editar malla» en el panel.");
        });
        if trabajar {
            self.peticion_trabajar_escena = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glass_mesh_edit::{write_glb, MeshData, Recipe};

    #[test]
    fn panel_abre_cubo_y_hornea() {
        let dir = std::env::temp_dir().join(format!("glass_mesh_tool_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("cube.glb");
        write_glb(&MeshData::cube(2.0), &src).unwrap();

        let mut panel = MeshToolPanel::new();
        panel.open(src.to_str().unwrap()).unwrap();
        assert!(
            panel.status.starts_with("Abierto cube:"),
            "{}",
            panel.status
        );

        panel.apply_cmd(MeshCommand::Subdivide { levels: 1 });
        assert!(panel.status.ends_with(" aplicado."), "{}", panel.status);
        panel.undo();
        assert!(panel.status.starts_with("Deshecho: "), "{}", panel.status);
        panel.redo();
        assert!(panel.status.starts_with("Rehecho: "), "{}", panel.status);

        panel.bake().unwrap();
        let glb = dir.join("cube_edited.glb");
        let recipe_path = dir.join("cube_edited.mesh.json");
        assert!(glb.exists());
        assert!(recipe_path.exists());

        let json = std::fs::read_to_string(&recipe_path).unwrap();
        let recipe = Recipe::from_json(&json).unwrap();
        assert_eq!(recipe.commands.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn panel_cerrar_cierra_la_sesion() {
        let dir =
            std::env::temp_dir().join(format!("glass_mesh_tool_close_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("cube.glb");
        write_glb(&MeshData::cube(2.0), &src).unwrap();

        let mut panel = MeshToolPanel::new();
        panel.open(src.to_str().unwrap()).unwrap();
        assert!(panel.session.is_some());

        panel.close();
        assert!(panel.session.is_none());
        assert_eq!(panel.status, "Sesión cerrada.");
        // Tras cerrar, aplicar y hornear fallan sin pánico y sin sesión.
        panel.apply_cmd(MeshCommand::Subdivide { levels: 1 });
        assert_eq!(panel.status, "Sin sesión.");
        assert!(panel.bake().is_err());
        // Y se puede volver a abrir la misma ruta.
        panel.open(src.to_str().unwrap()).unwrap();
        assert!(panel.session.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn panel_sin_sesion_no_panica() {
        let mut panel = MeshToolPanel::new();
        panel.apply_cmd(MeshCommand::Subdivide { levels: 1 });
        assert_eq!(panel.status, "Sin sesión.");
        panel.undo();
        assert!(panel.status.starts_with("Sin sesión"), "{}", panel.status);
        panel.redo();
        assert!(panel.status.starts_with("Sin sesión"), "{}", panel.status);
        assert!(panel.bake().is_err());
        assert!(panel.status.starts_with("Sin sesión"), "{}", panel.status);
    }

    fn ruta_prefs_temporal(sufijo: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "glass_mesh_tool_prefs_{}_{sufijo}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("tool_prefs.json")
    }

    fn panel_con_prefs(ruta: &std::path::Path) -> MeshToolPanel {
        MeshToolPanel {
            prefs_file: ruta.to_path_buf(),
            prefs: ToolPrefs::load_from(ruta),
            ..Default::default()
        }
    }

    #[test]
    fn panel_sin_prefs_pide_esquema() {
        let ruta = ruta_prefs_temporal("sin_prefs");
        let panel = panel_con_prefs(&ruta);
        assert_eq!(panel.prefs.scheme, None);
    }

    #[test]
    fn panel_guardar_esquema_persiste() {
        let ruta = ruta_prefs_temporal("guardar_esquema");
        let mut panel = panel_con_prefs(&ruta);
        panel.set_scheme(ShortcutScheme::Blender);
        assert_eq!(
            ToolPrefs::load_from(&ruta).scheme,
            Some(ShortcutScheme::Blender)
        );
        panel.set_scheme(ShortcutScheme::Unity);
        assert_eq!(
            ToolPrefs::load_from(&ruta).scheme,
            Some(ShortcutScheme::Unity)
        );
        if let Some(dir) = ruta.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn acciones_de_seleccion_sin_sesion() {
        let mut panel = MeshToolPanel::default();
        panel.run_key_action(MeshKeyAction::ModeFaces);
        assert_eq!(panel.selection_mode, SelectionMode::Faces);
        panel.run_key_action(MeshKeyAction::ModeVerts);
        assert_eq!(panel.selection_mode, SelectionMode::Verts);

        panel.status.clear();
        panel.run_key_action(MeshKeyAction::Grab);
        assert_eq!(
            panel.status,
            "Selecciona vértices o caras antes de mover."
        );

        panel.status.clear();
        panel.run_key_action(MeshKeyAction::DeleteSelection);
        assert_eq!(panel.status, "Sin selección que eliminar.");
        assert!(panel.session.is_none());
    }

    #[test]
    fn pick_viewport_selecciona_cara_y_vertice() {
        let (mut panel, _scene, dir) = panel_editando("pick_viewport", None);

        panel.selection_mode = SelectionMode::Faces;
        let sin_proyeccion = |_: [f32; 3]| -> Option<([f32; 2], f32)> { None };
        panel.pick_viewport(
            [0.0, 0.0, 5.0],
            [0.0, 0.0, -1.0],
            [0.0, 0.0],
            &sin_proyeccion,
            false,
        );
        assert_eq!(panel.selected_faces.len(), 1);
        assert!(panel.selected_verts.is_empty());
        assert_eq!(panel.status, "Seleccionado: 1 caras.");

        panel.clear_selection();
        panel.selection_mode = SelectionMode::Verts;
        let objetivo = panel.session.as_ref().unwrap().mesh.verts[0];
        let proyeccion = move |p: [f32; 3]| {
            let profundidad = if p == objetivo { -1.0 } else { 0.0 };
            Some(([p[0] * 100.0, p[1] * 100.0], profundidad))
        };
        let punto = [objetivo[0] * 100.0, objetivo[1] * 100.0];
        panel.pick_viewport(
            [0.0, 0.0, 5.0],
            [0.0, 0.0, -1.0],
            punto,
            &proyeccion,
            false,
        );
        assert!(panel.selected_verts.contains(&0));
        assert_eq!(panel.selected_verts.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pick_viewport_vertice_de_silueta_sin_impacto_de_cara() {
        // Regresión R2: clic exactamente sobre un vértice cuyo rayo no
        // atraviesa ninguna cara (silueta). El picking por pantalla debe
        // seleccionarlo igual; antes se limpiaba la selección.
        let (mut panel, _scene, dir) = panel_editando("pick_silueta", None);
        panel.selection_mode = SelectionMode::Verts;
        let objetivo = panel.session.as_ref().unwrap().mesh.verts[0];
        let proyeccion = move |p: [f32; 3]| Some(([p[0] * 100.0, p[1] * 100.0], 0.0));
        let punto = [objetivo[0] * 100.0, objetivo[1] * 100.0];
        panel.pick_viewport(
            [0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0],
            punto,
            &proyeccion,
            false,
        );
        assert!(panel.selected_verts.contains(&0));
        assert_eq!(panel.selected_verts.len(), 1);
        assert_eq!(panel.status, "Seleccionado: 1 vértices.");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn commit_drag_local_aplica_snap_y_ignora_delta_nulo() {
        let (mut panel, _scene, dir) = panel_editando("commit_drag", None);
        panel.selection_mode = SelectionMode::Verts;
        panel.selected_verts.insert(0);
        panel.snap_enabled = true;
        panel.snap_step = 0.25;

        let antes = panel.session.as_ref().unwrap().mesh.verts[0];
        let receta_antes = panel.session.as_ref().unwrap().recipe.commands.len();
        panel.commit_drag_local([0.26, 0.0, 0.0]);
        let despues = panel.session.as_ref().unwrap().mesh.verts[0];
        assert!((despues[0] - antes[0] - 0.25).abs() < 1e-6);
        let receta_despues = panel.session.as_ref().unwrap().recipe.commands.len();
        assert_eq!(receta_despues, receta_antes + 1);

        panel.commit_drag_local([0.1, 0.0, 0.0]);
        assert_eq!(
            panel.session.as_ref().unwrap().recipe.commands.len(),
            receta_despues
        );
        assert_eq!(panel.status, "Sin desplazamiento.");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Nacimiento desde la entidad y salida (regla del collider de B-E2) ---

    fn entidad_escena(
        id: u64,
        model: &str,
        physics: Option<glass_core::scene::PhysicsData>,
    ) -> EntityData {
        let mut e = EntityData::new(id, format!("E{id}"), vec!["Transform".to_string()]);
        e.mesh = Some(glass_core::scene::MeshData {
            model: model.to_string(),
            ..Default::default()
        });
        e.physics = physics;
        e
    }

    fn fisica(mode: MeshColliderMode) -> Option<glass_core::scene::PhysicsData> {
        Some(glass_core::scene::PhysicsData {
            mesh_collider: mode,
            ..Default::default()
        })
    }

    fn escena_con(entidad: EntityData) -> Arc<Mutex<SceneState>> {
        let mut scene = SceneState::default();
        scene.entities.push(entidad);
        Arc::new(Mutex::new(scene))
    }

    fn seleccion(id: u64, modelo: Option<&str>, collider: bool) -> EntidadSeleccionada {
        EntidadSeleccionada {
            id,
            nombre: format!("E{id}"),
            modelo: modelo.map(str::to_string),
            tiene_mesh_collider: collider,
        }
    }

    /// Panel con preferencias y assets_root temporales (sin tocar los del usuario).
    fn panel_temporal(sufijo: &str) -> (MeshToolPanel, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("glass_mesh_tool_r1_{sufijo}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        let mut panel = MeshToolPanel::new();
        panel.prefs_file = dir.join("tool_prefs.json");
        panel.prefs = ToolPrefs::default();
        panel.assets_root = dir.join("assets");
        (panel, dir)
    }

    /// Sesión nacida de la entidad 99 (primitiva "cube") con la física dada.
    fn panel_editando(
        sufijo: &str,
        physics: Option<glass_core::scene::PhysicsData>,
    ) -> (MeshToolPanel, Arc<Mutex<SceneState>>, std::path::PathBuf) {
        let (mut panel, dir) = panel_temporal(sufijo);
        // El test es autocontenido: el cubo vive en el assets_root
        // temporal, sin depender del cwd desde el que corra cargo test.
        write_glb(&MeshData::cube(2.0), &panel.assets_root.join("cube.glb"))
            .expect("escribir cube.glb");
        let con_collider = physics
            .as_ref()
            .is_some_and(|p| p.mesh_collider != MeshColliderMode::None);
        let scene = escena_con(entidad_escena(99, "cube", physics));
        panel.editar_entidad(&seleccion(99, Some("cube"), con_collider));
        assert!(panel.session.is_some(), "status: {}", panel.status);
        (panel, scene, dir)
    }

    fn horneado(panel: &MeshToolPanel) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = mesh_birth::dir_horneado(&panel.assets_root);
        (
            dir.join("cube_edited.glb"),
            dir.join("cube_edited.mesh.json"),
        )
    }

    #[test]
    fn collider_solo_mesh_collider() {
        let primitivos = glass_core::scene::PhysicsData {
            box_extents: [1.0, 1.0, 1.0],
            mesh_collider: MeshColliderMode::None,
            ..Default::default()
        };
        let entidades = vec![
            entidad_escena(1, "cube", fisica(MeshColliderMode::TriMesh)),
            entidad_escena(2, "cube", fisica(MeshColliderMode::Convex)),
            entidad_escena(3, "cube", Some(primitivos)),
            entidad_escena(4, "cube", None),
            entidad_escena(5, "rock", fisica(MeshColliderMode::TriMesh)),
            entidad_escena(6, "", fisica(MeshColliderMode::TriMesh)),
        ];
        assert_eq!(entities_with_mesh_collider(&entidades, "cube"), vec![1, 2, 6]);
        assert_eq!(entities_with_mesh_collider(&entidades, ""), vec![1, 2, 6]);
    }

    #[test]
    fn prompt_solo_con_todo() {
        assert!(should_prompt_collider(true, 1, false));
        assert!(!should_prompt_collider(true, 0, false));
        assert!(!should_prompt_collider(true, 1, true));
        assert!(!should_prompt_collider(false, 1, false));
        assert!(!should_prompt_collider(false, 0, true));
    }

    #[test]
    fn editar_entidad_sin_malla_no_abre() {
        let (mut panel, dir) = panel_temporal("sin_malla");
        panel.editar_entidad(&seleccion(7, None, false));
        assert!(panel.session.is_none());
        assert_eq!(
            panel.status,
            "La entidad «E7» no tiene malla: no se puede editar."
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editar_entidad_modelo_no_resoluble_no_abre() {
        let (mut panel, dir) = panel_temporal("no_resoluble");
        panel.editar_entidad(&seleccion(8, Some("no_existe_r1_xyz"), false));
        assert!(panel.session.is_none());
        assert!(panel.target_entity.is_none());
        assert!(!panel.status.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editar_entidad_primitiva_abre_con_destino() {
        let (mut panel, dir) = panel_temporal("primitiva");
        panel.editar_entidad(&seleccion(5, Some("sphere"), false));
        assert!(panel.session.is_some(), "status: {}", panel.status);
        assert_eq!(panel.target_entity, Some(5));
        assert_eq!(panel.target_entity_name, "E5");
        assert_eq!(panel.base_model, "sphere");
        assert!(
            panel.status.starts_with("Editando «E5» (sphere):"),
            "{}",
            panel.status
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn salir_con_mesh_collider_pregunta() {
        let (mut panel, scene, dir) = panel_editando("pregunta", fisica(MeshColliderMode::TriMesh));
        panel.apply_cmd(MeshCommand::Subdivide { levels: 1 });
        panel.hornear_y_salir(&scene);
        assert!(panel.session.is_some(), "no debía cerrar todavía");
        assert!(panel.collider_prompt_open);
        assert!(panel.pending_model_assign.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn responder_no_cierra_sin_tocar_escena() {
        let (mut panel, scene, dir) =
            panel_editando("responde_no", fisica(MeshColliderMode::TriMesh));
        panel.apply_cmd(MeshCommand::Subdivide { levels: 1 });
        panel.hornear_y_salir(&scene);
        panel.responder_collider(false);
        assert!(panel.session.is_none());
        assert!(panel.pending_model_assign.is_none());
        assert!(!horneado(&panel).0.exists());
        let sc = scene.lock().unwrap();
        let e = sc.entities.iter().find(|e| e.id == 99).unwrap();
        assert_eq!(e.mesh.as_ref().unwrap().model, "cube");
        drop(sc);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn responder_si_hornea_y_deja_asignacion() {
        let (mut panel, scene, dir) =
            panel_editando("responde_si", fisica(MeshColliderMode::TriMesh));
        panel.apply_cmd(MeshCommand::Subdivide { levels: 1 });
        panel.hornear_y_salir(&scene);
        panel.responder_collider(true);
        assert!(panel.session.is_none());
        let (glb, receta) = horneado(&panel);
        assert!(glb.exists());
        assert!(receta.exists());
        assert_eq!(
            panel.pending_model_assign,
            Some(ModelAssign {
                entity_id: 99,
                new_model: "cube_edited".to_string(),
                adapta_collider: true,
            })
        );
        assert_eq!(
            panel.status,
            "Sesión cerrada: el mesh collider se adaptará al nuevo modelo."
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collider_primitivo_no_pregunta() {
        let primitivos = glass_core::scene::PhysicsData {
            box_extents: [1.0, 1.0, 1.0],
            mesh_collider: MeshColliderMode::None,
            ..Default::default()
        };
        for (sufijo, physics) in [("primitivo", Some(primitivos)), ("sin_fisica", None)] {
            let (mut panel, scene, dir) = panel_editando(sufijo, physics);
            panel.apply_cmd(MeshCommand::Subdivide { levels: 1 });
            panel.hornear_y_salir(&scene);
            assert!(panel.session.is_none(), "{sufijo}: {}", panel.status);
            assert!(!panel.collider_prompt_open);
            assert_eq!(
                panel.pending_model_assign.as_ref().map(|a| a.adapta_collider),
                Some(false)
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn no_ask_cierra_sin_asignar() {
        let (mut panel, scene, dir) = panel_editando("no_ask", fisica(MeshColliderMode::TriMesh));
        panel.prefs.collider_no_ask = true;
        panel.apply_cmd(MeshCommand::Subdivide { levels: 1 });
        panel.hornear_y_salir(&scene);
        assert!(panel.session.is_none());
        assert!(!panel.collider_prompt_open);
        assert!(panel.pending_model_assign.is_none());
        assert!(!horneado(&panel).0.exists());
        assert_eq!(
            panel.status,
            "Sesión cerrada sin adaptar el collider (preferencia guardada: no volver a preguntar)."
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_volver_a_preguntar_se_persiste() {
        let (mut panel, scene, dir) =
            panel_editando("no_preguntar", fisica(MeshColliderMode::TriMesh));
        panel.apply_cmd(MeshCommand::Subdivide { levels: 1 });
        panel.hornear_y_salir(&scene);
        assert!(panel.collider_prompt_open);
        panel.collider_prompt_no_ask = true;
        panel.responder_collider(false);
        assert!(ToolPrefs::load_from(&panel.prefs_file).collider_no_ask);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn salir_sin_cambios_no_hornea() {
        let (mut panel, scene, dir) =
            panel_editando("sin_cambios", fisica(MeshColliderMode::TriMesh));
        panel.hornear_y_salir(&scene);
        assert!(panel.session.is_none());
        assert!(!panel.collider_prompt_open);
        assert!(panel.pending_model_assign.is_none());
        assert_eq!(panel.status, "Sin cambios: nada que hornear.");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn salir_sin_collider_hornea_y_asigna() {
        let (mut panel, scene, dir) = panel_editando("sin_collider", None);
        panel.apply_cmd(MeshCommand::Subdivide { levels: 1 });
        panel.hornear_y_salir(&scene);
        assert!(panel.session.is_none());
        assert!(panel.target_entity.is_none());
        let (glb, receta) = horneado(&panel);
        assert!(glb.exists());
        assert!(receta.exists());
        assert_eq!(
            panel.pending_model_assign,
            Some(ModelAssign {
                entity_id: 99,
                new_model: "cube_edited".to_string(),
                adapta_collider: false,
            })
        );
        assert_eq!(panel.status, "Horneado y asignado a «E99».");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn descartar_no_escribe_ni_asigna() {
        let (mut panel, _scene, dir) = panel_editando("descartar", None);
        panel.apply_cmd(MeshCommand::Subdivide { levels: 1 });
        panel.descartar();
        assert!(panel.session.is_none());
        assert!(panel.target_entity.is_none());
        assert!(panel.base_model.is_empty());
        assert!(panel.pending_model_assign.is_none());
        assert!(!horneado(&panel).0.exists());
        assert_eq!(panel.status, "Sesión descartada: la escena no se ha tocado.");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
