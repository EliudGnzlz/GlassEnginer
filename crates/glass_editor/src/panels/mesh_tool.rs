//! Panel «Herramienta 3D»: mini modelador sobre `MeshSession` con preview
//! wireframe dibujado con el painter de egui (sin Vulkan).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use glass_mesh_edit::{
    project_edges, project_point, read_glb, BooleanOp, MeshCommand, MeshEditError, MeshSession,
    MirrorAxis, OrbitCamera,
};

const VIEWPORT_HEIGHT: f32 = 320.0;
const FOV_DEG: f32 = 60.0;
const QUANT: f32 = 1e-4;

pub struct MeshToolPanel {
    pub session: Option<MeshSession>,
    /// Ruta del .glb a abrir, relativa al proyecto (p. ej. "assets/cube.glb").
    pub path_input: String,
    /// Último resultado o error, en texto plano.
    pub status: String,
    pub camera: OrbitCamera,
    pub show_vertices: bool,
    pub dims: [f32; 3],
    pub array_count: u32,
    pub array_offset: [f32; 3],
    pub move_delta: [f32; 3],
    pub mirror_axis: MirrorAxis,
    pub weld_tolerance: f32,
    pub boolean_path: String,
    pub boolean_op: BooleanOp,
    pub selection_mode: SelectionMode,
    pub selected_verts: std::collections::HashSet<u32>,
    pub selected_faces: std::collections::HashSet<u32>,
    pub snap_enabled: bool,
    pub snap_step: f32,
    gizmo_axis: Option<usize>,
    gizmo_start: [f32; 2],
    gizmo_delta: [f32; 3],
    gizmo_commit_pending: bool,
    pending_pick: Option<(f32, f32, f32, f32, bool)>,
    selection_overlay: Option<(egui::Rect, Vec<egui::Shape>)>,
}

impl Default for MeshToolPanel {
    fn default() -> Self {
        Self {
            session: None,
            selection_mode: SelectionMode::Verts,
            selected_verts: std::collections::HashSet::new(),
            selected_faces: std::collections::HashSet::new(),
            snap_enabled: true,
            snap_step: 0.25,
            gizmo_axis: None,
            gizmo_start: [0.0; 2],
            gizmo_delta: [0.0; 3],
            gizmo_commit_pending: false,
            pending_pick: None,
            selection_overlay: None,
            path_input: String::new(),
            status: "Sin sesión: abre un .glb de assets/.".to_owned(),
            camera: OrbitCamera::new([0.0, 0.0, 0.0]),
            show_vertices: true,
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

/// Acción pedida desde la UI; se ejecuta tras soltar los préstamos de `render`.
enum Action {
    Open,
    Undo,
    Redo,
    Close,
    Bake,
    SetDims,
    Cmd(MeshCommand),
    Boolean,
}

fn quantize(p: [f32; 3]) -> [i64; 3] {
    [
        (p[0] / QUANT).round() as i64,
        (p[1] / QUANT).round() as i64,
        (p[2] / QUANT).round() as i64,
    ]
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

    pub fn open(&mut self, path: &str) -> Result<(), MeshEditError> {
        self.clear_selection();
        match MeshSession::enter_from_glb(Path::new(path)) {
            Ok(mut session) => {
                let (stats, _) = session.stats();
                let center = [
                    (stats.bbox_min[0] + stats.bbox_max[0]) * 0.5,
                    (stats.bbox_min[1] + stats.bbox_max[1]) * 0.5,
                    (stats.bbox_min[2] + stats.bbox_max[2]) * 0.5,
                ];
                let extent = (0..3)
                    .map(|i| stats.bbox_max[i] - stats.bbox_min[i])
                    .fold(0.0_f32, f32::max);
                let mut camera = OrbitCamera::new(center);
                camera.dist = (2.5 * extent.max(1.0)).clamp(1.0, 50.0);
                self.camera = camera;
                self.status = format!(
                    "Abierto {}: {} vértices, {} caras.",
                    session.name, stats.verts, stats.faces
                );
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
        self.clear_selection();
        self.session = None;
        self.status = "Sesión cerrada.".to_owned();
    }

    pub fn apply_cmd(&mut self, cmd: MeshCommand) {
        if matches!(cmd, MeshCommand::MoveVerts { .. }) {
            self.apply_cmd_inner(cmd);
            return;
        }
        let before = self
            .session
            .as_ref()
            .map(|s| (s.mesh.verts.clone(), s.mesh.faces.clone()));
        self.apply_cmd_inner(cmd);
        let changed = match (before, self.session.as_ref()) {
            (Some((verts, faces)), Some(s)) => verts != s.mesh.verts || faces != s.mesh.faces,
            _ => false,
        };
        if changed {
            self.clear_selection();
        }
    }

    fn apply_cmd_inner(&mut self, cmd: MeshCommand) {
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
            Action::Open => {
                let path = self.path_input.trim().to_owned();
                let _ = self.open(&path);
            }
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Close => self.close(),
            Action::Bake => {
                let _ = self.bake();
            }
            Action::SetDims => self.apply_dimensions(),
            Action::Cmd(cmd) => self.apply_cmd(cmd),
            Action::Boolean => self.apply_boolean(),
        }
    }

    pub fn render(&mut self, ui: &mut egui::Ui) {
        let mut action: Option<Action> = None;

        match self.session.as_mut() {
            None => {
                ui.horizontal(|ui| {
                    ui.label("Ruta .glb:");
                    ui.text_edit_singleline(&mut self.path_input);
                    if ui.button("Abrir .glb").clicked() {
                        action = Some(Action::Open);
                    }
                });
            }
            Some(session) => {
                let (stats, report) = session.stats();

                ui.horizontal(|ui| {
                    ui.strong(session.name.as_str());
                    if ui.button("Deshacer").clicked() {
                        action = Some(Action::Undo);
                    }
                    if ui.button("Rehacer").clicked() {
                        action = Some(Action::Redo);
                    }
                    if ui.button("Cerrar").clicked() {
                        action = Some(Action::Close);
                    }
                    if ui.button("Hornear (.glb + receta)").clicked() {
                        action = Some(Action::Bake);
                    }
                });

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
                    session.recipe.commands.len(),
                ));

                ui.separator();
                ui.label("Dimensiones");
                ui.horizontal(|ui| {
                    drag_vec3(ui, &mut self.dims, 0.05);
                    if ui.button("Aplicar dimensiones").clicked() {
                        action = Some(Action::SetDims);
                    }
                });

                ui.separator();
                ui.label("Operadores");
                if ui.button("Subdividir").clicked() {
                    action = Some(Action::Cmd(MeshCommand::Subdivide { levels: 1 }));
                }
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.mirror_axis, MirrorAxis::X, "X");
                    ui.selectable_value(&mut self.mirror_axis, MirrorAxis::Y, "Y");
                    ui.selectable_value(&mut self.mirror_axis, MirrorAxis::Z, "Z");
                    if ui.button("Espejo").clicked() {
                        action = Some(Action::Cmd(MeshCommand::Mirror {
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
                        action = Some(Action::Cmd(MeshCommand::Weld {
                            tolerance: self.weld_tolerance.max(0.0),
                        }));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Copias");
                    ui.add(egui::DragValue::new(&mut self.array_count).speed(0.1));
                    ui.label("Offset");
                    drag_vec3(ui, &mut self.array_offset, 0.05);
                    if ui.button("Array").clicked() {
                        action = Some(Action::Cmd(MeshCommand::Array {
                            count: self.array_count,
                            offset: self.array_offset,
                        }));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Delta");
                    drag_vec3(ui, &mut self.move_delta, 0.05);
                    if ui.button("Mover todos").clicked() {
                        let indices = (0..session.mesh.verts.len())
                            .filter_map(|i| u32::try_from(i).ok())
                            .collect();
                        action = Some(Action::Cmd(MeshCommand::MoveVerts {
                            indices,
                            delta: self.move_delta,
                        }));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Otro .glb:");
                    ui.text_edit_singleline(&mut self.boolean_path);
                });
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.boolean_op, BooleanOp::Union, "Unión");
                    ui.selectable_value(&mut self.boolean_op, BooleanOp::Difference, "Resta");
                    ui.selectable_value(
                        &mut self.boolean_op,
                        BooleanOp::Intersection,
                        "Intersección",
                    );
                    if ui.button("Booleana").clicked() {
                        action = Some(Action::Boolean);
                    }
                });

                ui.separator();
                ui.checkbox(&mut self.show_vertices, "Vértices");
                let selection_text = match self.selection_mode {
                    SelectionMode::Verts => format!("{} vértices", self.selected_verts.len()),
                    SelectionMode::Faces => format!("{} caras", self.selected_faces.len()),
                };
                let mut clear_clicked = false;
                let selection_mode = &mut self.selection_mode;
                ui.horizontal(|ui| {
                    ui.label("Selección");
                    ui.selectable_value(&mut *selection_mode, SelectionMode::Verts, "Vértices");
                    ui.selectable_value(&mut *selection_mode, SelectionMode::Faces, "Caras");
                    ui.label(selection_text);
                    clear_clicked = ui.button("Limpiar").clicked();
                });
                if clear_clicked {
                    self.selected_verts.clear();
                    self.selected_faces.clear();
                    self.gizmo_axis = None;
                    self.gizmo_start = [0.0; 2];
                    self.gizmo_delta = [0.0; 3];
                }
                let snap_enabled = &mut self.snap_enabled;
                let snap_step = &mut self.snap_step;
                ui.horizontal(|ui| {
                    ui.checkbox(&mut *snap_enabled, "Snap");
                    ui.label("Paso");
                    ui.add(
                        egui::DragValue::new(&mut *snap_step)
                            .speed(0.05)
                            .max_decimals(2),
                    );
                });
                self.snap_step = self.snap_step.clamp(0.01, 2.0);

                let size = egui::vec2(ui.available_width().max(1.0), VIEWPORT_HEIGHT);
                let (response, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
                let rect = response.rect;
                painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(24, 24, 28));

                let vp_w = rect.width();
                let vp_h = rect.height();
                let sel_indices = selection_indices_for(
                    self.selection_mode,
                    &self.selected_verts,
                    &self.selected_faces,
                    Some(session.mesh.faces.as_slice()),
                );
                let sel_set: std::collections::HashSet<u32> = sel_indices.iter().copied().collect();
                let gizmo_centroid =
                    glass_mesh_edit::preview::selection_centroid(&session.mesh.verts, &sel_set);
                let gizmo_len = 0.6 * bbox_extent(&session.mesh.verts).max(0.5);
                let mut gizmo_busy = false;
                if let Some(axis) = self.gizmo_axis {
                    gizmo_busy = true;
                    if response.dragged_by(egui::PointerButton::Primary) {
                        if let (Some(centroid), Some(pos)) =
                            (gizmo_centroid, response.interact_pointer_pos())
                        {
                            let drag = [
                                pos.x - rect.min.x - self.gizmo_start[0],
                                pos.y - rect.min.y - self.gizmo_start[1],
                            ];
                            let amount =
                                gizmo_world_amount(&self.camera, centroid, axis, drag, vp_w, vp_h);
                            let mut delta = [0.0; 3];
                            if let Some(slot) = delta.get_mut(axis) {
                                *slot = amount;
                            }
                            self.gizmo_delta = if self.snap_enabled {
                                glass_mesh_edit::preview::snap_delta(delta, self.snap_step)
                            } else {
                                delta
                            };
                        }
                    } else if response.drag_stopped_by(egui::PointerButton::Primary) {
                        self.gizmo_commit_pending = true;
                    }
                } else if let Some(centroid) = gizmo_centroid {
                    if response.drag_started_by(egui::PointerButton::Primary) {
                        let origin = ui
                            .input(|i| i.pointer.press_origin())
                            .or_else(|| response.interact_pointer_pos());
                        if let Some(origin) = origin {
                            let start = [origin.x - rect.min.x, origin.y - rect.min.y];
                            if let Some(axis) =
                                hit_gizmo_axis(&self.camera, centroid, gizmo_len, start, vp_w, vp_h)
                            {
                                self.gizmo_axis = Some(axis);
                                self.gizmo_start = start;
                                self.gizmo_delta = [0.0; 3];
                                gizmo_busy = true;
                            }
                        }
                    }
                }
                if self.gizmo_axis.is_none() && response.clicked() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let additive = ui.input(|i| i.modifiers.shift);
                        self.pending_pick =
                            Some((pos.x - rect.min.x, pos.y - rect.min.y, vp_w, vp_h, additive));
                    }
                }
                if !gizmo_busy && response.dragged_by(egui::PointerButton::Primary) {
                    let d = response.drag_delta();
                    self.camera.yaw += d.x * 0.01;
                    self.camera.pitch = (self.camera.pitch + d.y * 0.01).clamp(-1.4, 1.4);
                }
                if response.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        self.camera.dist =
                            (self.camera.dist * (1.0 - scroll * 0.002)).clamp(1.0, 50.0);
                    }
                }

                let drag_offset = self.gizmo_axis.is_some().then_some(self.gizmo_delta);
                let mut baked = session.bake();
                if let Some(offset) = drag_offset {
                    displace_positions(
                        &mut baked.positions,
                        &session.mesh.verts,
                        &sel_indices,
                        offset,
                    );
                }
                let overlay = selection_overlay(&SelectionOverlay {
                    camera: &self.camera,
                    rect,
                    verts: &session.mesh.verts,
                    faces: &session.mesh.faces,
                    selected_faces: &self.selected_faces,
                    sel_indices: &sel_indices,
                    sel_set: &sel_set,
                    offset: drag_offset,
                    centroid: gizmo_centroid,
                    len: gizmo_len,
                    active_axis: self.gizmo_axis,
                });
                self.selection_overlay = Some((rect, overlay));
                let to_pos = |p: [f32; 2]| egui::pos2(rect.min.x + p[0], rect.min.y + p[1]);
                let edge_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(220));
                for (a, b) in
                    project_edges(&baked, &self.camera, rect.width(), rect.height(), FOV_DEG)
                {
                    painter.line_segment([to_pos(a), to_pos(b)], edge_stroke);
                }
                if self.show_vertices {
                    let vert_color = egui::Color32::from_rgb(255, 180, 60);
                    let mut seen = HashSet::new();
                    for &p in &baked.positions {
                        if !seen.insert(quantize(p)) {
                            continue;
                        }
                        if let Some(s) =
                            project_point(p, &self.camera, rect.width(), rect.height(), FOV_DEG)
                        {
                            painter.circle_filled(to_pos(s), 2.5, vert_color);
                        }
                    }
                }
            }
        }

        if let Some(action) = action {
            self.run(action);
        }

        ui.separator();
        if let Some((clip, shapes)) = self.selection_overlay.take() {
            if self.session.is_some() {
                ui.painter().with_clip_rect(clip).extend(shapes);
            }
        }
        if let Some((px, py, w, h, additive)) = self.pending_pick.take() {
            self.pick_at(px, py, w, h, additive);
        }
        if self.gizmo_commit_pending {
            self.gizmo_commit_pending = false;
            self.commit_gizmo_drag();
        }
        ui.label(self.status.as_str());
    }
}

/// Modo de selección sub-objeto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    Verts,
    Faces,
}

const SEL_QUANT: f32 = 1e-4;
const VERT_PICK_PX: f32 = 14.0;
const GIZMO_PICK_PX: f32 = 12.0;
const GIZMO_AXES: [([f32; 3], egui::Color32); 3] = [
    ([1.0, 0.0, 0.0], egui::Color32::from_rgb(220, 60, 60)),
    ([0.0, 1.0, 0.0], egui::Color32::from_rgb(60, 200, 60)),
    ([0.0, 0.0, 1.0], egui::Color32::from_rgb(60, 120, 230)),
];

impl MeshToolPanel {
    /// Vacía la selección y cancela el arrastre del gizmo.
    pub fn clear_selection(&mut self) {
        self.selected_verts.clear();
        self.selected_faces.clear();
        self.gizmo_axis = None;
        self.gizmo_start = [0.0; 2];
        self.gizmo_delta = [0.0; 3];
        self.gizmo_commit_pending = false;
    }

    /// Índices de vértices lógicos de la selección, ordenados y sin duplicados.
    pub fn selection_vert_indices(&self) -> Vec<u32> {
        selection_indices_for(
            self.selection_mode,
            &self.selected_verts,
            &self.selected_faces,
            self.session.as_ref().map(|s| s.mesh.faces.as_slice()),
        )
    }

    /// Selección por clic en (px, py) del viewport de tamaño width x height.
    pub fn pick_at(&mut self, px: f32, py: f32, width: f32, height: f32, additive: bool) {
        use glass_mesh_edit::preview::{pick_face, pick_vertex, screen_ray};

        let Some(session) = self.session.as_ref() else {
            return;
        };
        let (origin, dir) = screen_ray(&self.camera, px, py, width, height, FOV_DEG);
        match pick_face(&session.mesh.verts, &session.mesh.faces, origin, dir) {
            Some((face, t)) => match self.selection_mode {
                SelectionMode::Faces => toggle_or_set(
                    &mut self.selected_faces,
                    &mut self.selected_verts,
                    face,
                    additive,
                ),
                SelectionMode::Verts => {
                    let hit_point = add3(origin, scale3(dir, t));
                    let picked = pick_vertex(
                        &session.mesh.verts,
                        &self.camera,
                        [px, py],
                        [width, height],
                        FOV_DEG,
                        VERT_PICK_PX,
                    )
                    .or_else(|| {
                        session.mesh.faces.get(face as usize).and_then(|f| {
                            f.iter()
                                .copied()
                                .filter_map(|v| {
                                    session
                                        .mesh
                                        .verts
                                        .get(v as usize)
                                        .map(|p| (v, len3(sub3(*p, hit_point))))
                                })
                                .min_by(|a, b| a.1.total_cmp(&b.1))
                                .map(|(v, _)| v)
                        })
                    });
                    if let Some(v) = picked {
                        toggle_or_set(
                            &mut self.selected_verts,
                            &mut self.selected_faces,
                            v,
                            additive,
                        );
                    }
                }
            },
            None => {
                if !additive {
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

    /// Aplica el arrastre del gizmo como un único MoveVerts y limpia el estado de arrastre.
    pub fn commit_gizmo_drag(&mut self) {
        let axis = self.gizmo_axis.take();
        let raw = self.gizmo_delta;
        self.gizmo_delta = [0.0; 3];
        self.gizmo_start = [0.0; 2];
        self.gizmo_commit_pending = false;
        if axis.is_none() || self.session.is_none() {
            return;
        }
        let Some(delta) = gizmo_commit_delta(raw, self.snap_enabled, self.snap_step) else {
            return;
        };
        let indices = self.selection_vert_indices();
        if indices.is_empty() {
            return;
        }
        self.apply_cmd(MeshCommand::MoveVerts { indices, delta });
    }
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn len3(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
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

/// Mayor extensión por eje del bbox de `verts`; 0 si está vacío.
fn bbox_extent(verts: &[[f32; 3]]) -> f32 {
    let mut iter = verts.iter();
    let Some(first) = iter.next() else {
        return 0.0;
    };
    let mut min = *first;
    let mut max = *first;
    for p in iter {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    (max[0] - min[0]).max(max[1] - min[1]).max(max[2] - min[2])
}

fn point_segment_dist(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if len2 <= f32::EPSILON {
        0.0
    } else {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / len2).clamp(0.0, 1.0)
    };
    let c = [a[0] + ab[0] * t - p[0], a[1] + ab[1] * t - p[1]];
    (c[0] * c[0] + c[1] * c[1]).sqrt()
}

/// Eje del gizmo cuya flecha pasa a menos de GIZMO_PICK_PX de `point` (el más cercano).
fn hit_gizmo_axis(
    camera: &OrbitCamera,
    centroid: [f32; 3],
    len: f32,
    point: [f32; 2],
    width: f32,
    height: f32,
) -> Option<usize> {
    let project =
        |p: [f32; 3]| glass_mesh_edit::preview::project_point(p, camera, width, height, FOV_DEG);
    let base = project(centroid)?;
    let mut best: Option<(usize, f32)> = None;
    for (axis, (dir, _)) in GIZMO_AXES.iter().enumerate() {
        let Some(tip) = project(add3(centroid, scale3(*dir, len))) else {
            continue;
        };
        let d = point_segment_dist(point, base, tip);
        if d >= GIZMO_PICK_PX {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, best_d)) => d < best_d,
        };
        if better {
            best = Some((axis, d));
        }
    }
    best.map(|(axis, _)| axis)
}

/// Desplazamiento en mundo a lo largo de `axis` para un arrastre de pantalla `drag`.
fn gizmo_world_amount(
    camera: &OrbitCamera,
    centroid: [f32; 3],
    axis: usize,
    drag: [f32; 2],
    width: f32,
    height: f32,
) -> f32 {
    let Some((dir, _)) = GIZMO_AXES.get(axis) else {
        return 0.0;
    };
    let project =
        |p: [f32; 3]| glass_mesh_edit::preview::project_point(p, camera, width, height, FOV_DEG);
    let (Some(a), Some(b)) = (project(centroid), project(add3(centroid, *dir))) else {
        return 0.0;
    };
    let screen = [b[0] - a[0], b[1] - a[1]];
    let screen_len = (screen[0] * screen[0] + screen[1] * screen[1]).sqrt();
    if screen_len <= 1e-6 {
        return 0.0;
    }
    let along = (drag[0] * screen[0] + drag[1] * screen[1]) / screen_len;
    let dist = len3(sub3(centroid, camera.eye()));
    let focal = (height * 0.5) / (FOV_DEG.to_radians() * 0.5).tan();
    if !focal.is_finite() || focal <= 0.0 {
        return 0.0;
    }
    along * dist / focal
}

fn sel_quantize(p: [f32; 3]) -> [i64; 3] {
    [
        (p[0] / SEL_QUANT).round() as i64,
        (p[1] / SEL_QUANT).round() as i64,
        (p[2] / SEL_QUANT).round() as i64,
    ]
}

/// Suma `offset` a las esquinas horneadas que coinciden (cuantizadas) con vértices seleccionados.
fn displace_positions(
    positions: &mut [[f32; 3]],
    verts: &[[f32; 3]],
    indices: &[u32],
    offset: [f32; 3],
) {
    let keys: std::collections::HashSet<[i64; 3]> = indices
        .iter()
        .filter_map(|&i| verts.get(i as usize))
        .map(|p| sel_quantize(*p))
        .collect();
    if keys.is_empty() {
        return;
    }
    for p in positions.iter_mut() {
        if keys.contains(&sel_quantize(*p)) {
            *p = add3(*p, offset);
        }
    }
}

struct SelectionOverlay<'a> {
    camera: &'a OrbitCamera,
    rect: egui::Rect,
    verts: &'a [[f32; 3]],
    faces: &'a [[u32; 3]],
    selected_faces: &'a std::collections::HashSet<u32>,
    sel_indices: &'a [u32],
    sel_set: &'a std::collections::HashSet<u32>,
    offset: Option<[f32; 3]>,
    centroid: Option<[f32; 3]>,
    len: f32,
    active_axis: Option<usize>,
}

/// Formas de highlight (caras, vértices seleccionados) y gizmo, en coordenadas absolutas.
fn selection_overlay(ov: &SelectionOverlay<'_>) -> Vec<egui::Shape> {
    let width = ov.rect.width();
    let height = ov.rect.height();
    let origin = ov.rect.min;
    let offset = ov.offset.unwrap_or([0.0; 3]);
    let world = |i: u32| -> Option<[f32; 3]> {
        let p = *ov.verts.get(i as usize)?;
        if ov.sel_set.contains(&i) {
            Some(add3(p, offset))
        } else {
            Some(p)
        }
    };
    let screen = |p: [f32; 3]| {
        glass_mesh_edit::preview::project_point(p, ov.camera, width, height, FOV_DEG)
            .map(|s| egui::pos2(origin.x + s[0], origin.y + s[1]))
    };
    let highlight = egui::Color32::from_rgb(255, 180, 60);
    let fill = egui::Color32::from_rgba_unmultiplied(255, 180, 60, 60);
    let selected_color = egui::Color32::from_rgb(255, 140, 0);
    let mut shapes = Vec::new();

    let mut faces: Vec<u32> = ov.selected_faces.iter().copied().collect();
    faces.sort_unstable();
    for face_index in faces {
        let Some(face) = ov.faces.get(face_index as usize) else {
            continue;
        };
        let points: Option<Vec<egui::Pos2>> =
            face.iter().map(|&v| world(v).and_then(screen)).collect();
        if let Some(points) = points {
            shapes.push(egui::Shape::convex_polygon(
                points,
                fill,
                egui::Stroke::new(2.0, highlight),
            ));
        }
    }

    for &v in ov.sel_indices {
        if let Some(p) = world(v).and_then(screen) {
            shapes.push(egui::Shape::circle_filled(p, 5.0, selected_color));
        }
    }

    if let Some(centroid) = ov.centroid {
        let base = add3(centroid, offset);
        for (axis, (dir, color)) in GIZMO_AXES.iter().enumerate() {
            let tip = add3(base, scale3(*dir, ov.len));
            if let (Some(a), Some(b)) = (screen(base), screen(tip)) {
                let (stroke, radius) = if ov.active_axis == Some(axis) {
                    (3.0, 6.0)
                } else {
                    (2.0, 4.0)
                };
                shapes.push(egui::Shape::line_segment(
                    [a, b],
                    egui::Stroke::new(stroke, *color),
                ));
                shapes.push(egui::Shape::circle_filled(b, radius, *color));
            }
        }
    }
    shapes
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use glass_mesh_edit::{write_glb, MeshData};
    use std::collections::HashSet;

    #[test]
    fn indices_de_caras_unen_sus_vertices() {
        let mesh_faces = [[0, 1, 2], [2, 3, 0], [4, 5, 6]];
        let faces: HashSet<u32> = [0, 1, 99].into_iter().collect();
        let verts: HashSet<u32> = [7, 3].into_iter().collect();
        let out = selection_indices_for(SelectionMode::Faces, &verts, &faces, Some(&mesh_faces));
        assert_eq!(out, vec![0, 1, 2, 3]);
        let out = selection_indices_for(SelectionMode::Faces, &verts, &faces, None);
        assert!(out.is_empty());
        let out = selection_indices_for(SelectionMode::Verts, &verts, &faces, None);
        assert_eq!(out, vec![3, 7]);
    }

    #[test]
    fn commit_delta_aplica_snap() {
        let d = gizmo_commit_delta([0.26, 0.0, 0.0], true, 0.25).expect("delta");
        assert!((d[0] - 0.25).abs() < 1e-6);
        assert!(gizmo_commit_delta([0.1, 0.0, 0.0], true, 0.25).is_none());
        assert!(gizmo_commit_delta([0.0; 3], false, 0.25).is_none());
    }

    #[test]
    fn commit_delta_sin_snap_es_exacto() {
        let d = gizmo_commit_delta([0.26, 0.0, 0.0], false, 0.25).unwrap();
        assert!((d[0] - 0.26).abs() < 1e-6, "d[0] = {}", d[0]);
        assert!(d[1].abs() < 1e-6);
        assert!(d[2].abs() < 1e-6);
    }

    #[test]
    fn panel_seleccion_cara_y_vertice() {
        let dir =
            std::env::temp_dir().join(format!("glass_mesh_tool_bd_sel_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("cube.glb");
        write_glb(&MeshData::cube(2.0), &src).unwrap();
        let mut panel = MeshToolPanel::new();
        panel.open(src.to_str().unwrap()).unwrap();

        panel.selection_mode = SelectionMode::Faces;
        panel.pick_at(400.0, 160.0, 800.0, 320.0, false);
        assert!(
            !panel.selected_faces.is_empty(),
            "no se seleccionó ninguna cara"
        );
        assert!(
            panel.status.starts_with("Seleccionado:"),
            "status: {}",
            panel.status
        );

        panel.clear_selection();
        panel.selection_mode = SelectionMode::Verts;
        panel.pick_at(400.0, 160.0, 800.0, 320.0, false);
        assert!(
            !panel.selected_verts.is_empty(),
            "no se seleccionó ningún vértice"
        );
        assert!(
            panel.status.starts_with("Seleccionado:"),
            "status: {}",
            panel.status
        );

        panel.pick_at(5.0, 5.0, 800.0, 320.0, false);
        assert!(panel.selected_verts.is_empty());
        assert!(panel.selected_faces.is_empty());
        assert_eq!(panel.status, "Sin selección.");

        panel.selection_mode = SelectionMode::Faces;
        panel.pick_at(400.0, 160.0, 800.0, 320.0, false);
        let antes = panel.selected_faces.len();
        assert!(antes > 0, "el primer pick no seleccionó nada");
        panel.pick_at(400.0, 160.0, 800.0, 320.0, true);
        assert!(
            panel.selected_faces.is_empty(),
            "el toggle con Shift no quitó la cara"
        );
        panel.pick_at(400.0, 160.0, 800.0, 320.0, true);
        assert_eq!(
            panel.selected_faces.len(),
            antes,
            "el toggle con Shift no volvió a añadir la cara"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn panel_gizmo_mueve_seleccion_con_snap() {
        let dir =
            std::env::temp_dir().join(format!("glass_mesh_tool_bd_gizmo_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("cube.glb");
        write_glb(&MeshData::cube(2.0), &src).unwrap();
        let mut panel = MeshToolPanel::new();
        panel.open(src.to_str().unwrap()).unwrap();

        let orig = panel.session.as_ref().unwrap().mesh.verts[0];
        panel.selected_verts.insert(0);
        panel.gizmo_delta = [0.26, 0.0, 0.0];
        panel.gizmo_axis = Some(0);
        panel.commit_gizmo_drag();

        let movido = panel.session.as_ref().unwrap().mesh.verts[0];
        assert!(
            (movido[0] - (orig[0] + 0.25)).abs() < 1e-5,
            "x = {}",
            movido[0]
        );
        assert!((movido[1] - orig[1]).abs() < 1e-5);
        assert!((movido[2] - orig[2]).abs() < 1e-5);
        assert_eq!(panel.session.as_ref().unwrap().recipe.commands.len(), 1);

        panel.undo();
        let restaurado = panel.session.as_ref().unwrap().mesh.verts[0];
        assert!((restaurado[0] - orig[0]).abs() < 1e-5);
        assert!((restaurado[1] - orig[1]).abs() < 1e-5);
        assert!((restaurado[2] - orig[2]).abs() < 1e-5);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn panel_seleccion_se_limpia_al_subdividir() {
        let dir =
            std::env::temp_dir().join(format!("glass_mesh_tool_bd_sub_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("cube.glb");
        write_glb(&MeshData::cube(2.0), &src).unwrap();
        let mut panel = MeshToolPanel::new();
        panel.open(src.to_str().unwrap()).unwrap();

        panel.selected_verts.insert(0);
        panel.apply_cmd(MeshCommand::Subdivide { levels: 1 });
        assert!(
            panel.selected_verts.is_empty(),
            "la selección sobrevivió a Subdivide"
        );
        assert!(panel.selected_faces.is_empty());

        panel.selected_verts.insert(0);
        panel.apply_cmd(MeshCommand::MoveVerts {
            indices: vec![0],
            delta: [0.1, 0.0, 0.0],
        });
        assert!(
            !panel.selected_verts.is_empty(),
            "MoveVerts limpió la selección"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn panel_snap_redondea() {
        let dir =
            std::env::temp_dir().join(format!("glass_mesh_tool_bd_snap_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("cube.glb");
        write_glb(&MeshData::cube(2.0), &src).unwrap();
        let mut panel = MeshToolPanel::new();
        panel.open(src.to_str().unwrap()).unwrap();

        let orig = panel.session.as_ref().unwrap().mesh.verts[0];
        panel.selected_verts.insert(0);

        panel.snap_enabled = true;
        panel.snap_step = 0.25;
        panel.gizmo_delta = [0.26, 0.0, 0.0];
        panel.gizmo_axis = Some(0);
        panel.commit_gizmo_drag();
        let con_snap = panel.session.as_ref().unwrap().mesh.verts[0];
        assert!(
            (con_snap[0] - (orig[0] + 0.25)).abs() < 1e-5,
            "x con snap = {}",
            con_snap[0]
        );

        panel.undo();
        let restaurado = panel.session.as_ref().unwrap().mesh.verts[0];
        assert!((restaurado[0] - orig[0]).abs() < 1e-5);

        panel.selected_verts.insert(0);
        panel.snap_enabled = false;
        panel.gizmo_delta = [0.26, 0.0, 0.0];
        panel.gizmo_axis = Some(0);
        panel.commit_gizmo_drag();
        let sin_snap = panel.session.as_ref().unwrap().mesh.verts[0];
        assert!(
            (sin_snap[0] - (orig[0] + 0.26)).abs() < 1e-5,
            "x sin snap = {}",
            sin_snap[0]
        );

        let _ = std::fs::remove_dir_all(&dir);
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
}
