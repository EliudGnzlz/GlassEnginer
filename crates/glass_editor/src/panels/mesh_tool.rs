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
}

impl Default for MeshToolPanel {
    fn default() -> Self {
        Self {
            session: None,
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
        self.session = None;
        self.status = "Sesión cerrada.".to_owned();
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

                let size = egui::vec2(ui.available_width().max(1.0), VIEWPORT_HEIGHT);
                let (response, painter) = ui.allocate_painter(size, egui::Sense::drag());
                let rect = response.rect;
                painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(24, 24, 28));

                if response.dragged_by(egui::PointerButton::Primary) {
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

                let baked = session.bake();
                let to_pos = |p: [f32; 2]| egui::pos2(rect.min.x + p[0], rect.min.y + p[1]);
                let edge_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(220));
                for (a, b) in project_edges(&baked, &self.camera, rect.width(), rect.height(), FOV_DEG)
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
        ui.label(self.status.as_str());
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
        assert!(panel.status.starts_with("Abierto cube:"), "{}", panel.status);

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
        let dir = std::env::temp_dir().join(format!("glass_mesh_tool_close_{}", std::process::id()));
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
