//! Editor visual del grafo de shaders (primera versión).
//!
//! Lienzo con los nodos del grafo como cajas que se arrastran, las aristas
//! como líneas, y un inspector lateral para editar las propiedades del nodo
//! seleccionado y cablear sus entradas con desplegables (el cableado por
//! arrastre de puertos queda para una versión posterior). Guardar escribe
//! el JSON del grafo en `assets/shader_graphs/` y Compilar valida el grafo
//! con el compilador de `glass_core::shader_graph` y muestra el GLSL.
//!
//! Para usar un grafo como material: generar su `.spv` con la herramienta
//! del proyecto (`graph_<nombre>.frag.spv` en `assets/shaders/`) y poner
//! `shader = "graph:<nombre>"` en el MeshData de la entidad.

use egui::{Color32, Pos2, RichText, Sense, Stroke, Ui, Vec2};
use glass_core::shader_graph::{
    presets, GraphEdge, GraphNode, InteractionComponent, NodeKind, PositionAxis, ShaderGraph,
};

const NODE_W: f32 = 150.0;
const NODE_H: f32 = 46.0;
const CANVAS_W: f32 = 1500.0;
const CANVAS_H: f32 = 950.0;

pub struct ShaderGraphEditorPanel {
    graphs: Vec<ShaderGraph>,
    current: usize,
    selected: Option<u32>,
    status: Option<String>,
    glsl_preview: String,
    loaded: bool,
}

impl Default for ShaderGraphEditorPanel {
    fn default() -> Self {
        Self::new()
    }
}

fn kind_label(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Color { .. } => "🎨 Color",
        NodeKind::Float { .. } => "🔢 Float",
        NodeKind::Time => "⏱ Tiempo",
        NodeKind::Uv => "🌐 UV",
        NodeKind::Normal => "📐 Normal",
        NodeKind::Add => "➕ Sumar",
        NodeKind::Multiply => "✖ Multiplicar",
        NodeKind::Mix => "🎚 Mezclar",
        NodeKind::Sine => "〰 Seno",
        NodeKind::Abs => "Abs",
        NodeKind::OneMinus => "1 − x",
        NodeKind::Smoothstep { .. } => "Smoothstep",
        NodeKind::RadialUv { .. } => "⭕ Radial UV",
        NodeKind::Fresnel { .. } => "✨ Fresnel",
        NodeKind::Output => "🖥 Salida",
        NodeKind::Position { .. } => "📍 Posición (vértices)",
        NodeKind::VertexOutput => "🚀 Salida de vértices",
        NodeKind::InteractionPoint { .. } => "🎯 Punto de interacción",
        NodeKind::ValueNoise { .. } => "🔊 Ruido de valor",
        NodeKind::GradientNoise { .. } => "🌊 Ruido gradiente",
        NodeKind::FbmNoise { .. } => "☁ Ruido FBM",
        NodeKind::Subtract => "➖ Restar",
        NodeKind::Divide => "➗ Dividir",
        NodeKind::ChannelR => "🔴 Canal R",
        NodeKind::ChannelG => "🟢 Canal G",
        NodeKind::ChannelB => "🔵 Canal B",
        NodeKind::ChannelA => "⚪ Canal A",
        NodeKind::Combine => "🧩 Combinar RGBA",
        NodeKind::Dot => "· Producto escalar",
        NodeKind::Length => "📏 Longitud",
        NodeKind::Distance => "↔ Distancia",
        NodeKind::Normalize => "🧭 Normalizar",
        NodeKind::Clamp { .. } => "✂ Limitar (Clamp)",
        NodeKind::Power { .. } => "⬆ Potencia",
        NodeKind::Step { .. } => "🪜 Escalón (Step)",
        NodeKind::Modulo => "% Módulo",
    }
}

/// Entradas cableables de cada nodo (espejo del compilador de glass_core).
fn kind_inputs(kind: &NodeKind) -> &'static [&'static str] {
    match kind {
        NodeKind::Add | NodeKind::Multiply => &["a", "b"],
        NodeKind::Mix => &["a", "b", "t"],
        NodeKind::Sine | NodeKind::Abs | NodeKind::OneMinus | NodeKind::Smoothstep { .. } => &["x"],
        NodeKind::Output => &["color"],
        NodeKind::ValueNoise { .. } | NodeKind::GradientNoise { .. } | NodeKind::FbmNoise { .. } => &["uv"],
        NodeKind::Subtract | NodeKind::Divide | NodeKind::Dot | NodeKind::Distance => &["a", "b"],
        NodeKind::ChannelR | NodeKind::ChannelG | NodeKind::ChannelB | NodeKind::ChannelA => &["x"],
        NodeKind::Combine => &["r", "g", "b", "a"],
        NodeKind::Length | NodeKind::Normalize | NodeKind::Clamp { .. } | NodeKind::Power { .. } | NodeKind::Step { .. } => &["x"],
        NodeKind::Modulo => &["x", "y"],
        NodeKind::VertexOutput => &["offset"],
        _ => &[],
    }
}

impl ShaderGraphEditorPanel {
    pub fn new() -> Self {
        Self {
            graphs: Vec::new(),
            current: 0,
            selected: None,
            status: None,
            glsl_preview: String::new(),
            loaded: false,
        }
    }

    /// Carga los grafos de `assets/shader_graphs/` (o los prefabricados si
    /// aún no hay ninguno guardado) y les da una posición inicial en rejilla
    /// a los que vengan sin colocar.
    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let dir = std::path::Path::new("assets/shader_graphs");
        let mut loaded: Vec<ShaderGraph> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for path in paths {
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        if let Ok(g) = ShaderGraph::from_json(&text) {
                            loaded.push(g);
                        }
                    }
                }
            }
        }
        if loaded.is_empty() {
            loaded = presets::all();
        }
        for g in &mut loaded {
            if g.nodes.iter().all(|n| n.position == [0.0, 0.0]) {
                for (i, n) in g.nodes.iter_mut().enumerate() {
                    n.position = [24.0 + (i % 4) as f32 * 196.0, 24.0 + (i / 4) as f32 * 104.0];
                }
            }
        }
        self.graphs = loaded;
    }

    fn graph_mut(&mut self) -> Option<&mut ShaderGraph> {
        self.graphs.get_mut(self.current)
    }

    pub fn render(&mut self, ui: &mut Ui) {
        self.ensure_loaded();
        if self.graphs.is_empty() {
            ui.label("No hay grafos de shaders disponibles.");
            return;
        }

        // ---------- Barra de herramientas ----------
        ui.horizontal(|ui| {
            let names: Vec<String> = self.graphs.iter().map(|g| g.name.clone()).collect();
            egui::ComboBox::from_id_salt("sg_graph_combo")
                .selected_text(&names[self.current])
                .show_ui(ui, |ui| {
                    for (i, name) in names.iter().enumerate() {
                        if ui.selectable_value(&mut self.current, i, name).clicked() {
                            self.selected = None;
                            self.glsl_preview.clear();
                        }
                    }
                });
            if ui.button("↻ Recargar").clicked() {
                self.loaded = false;
                self.selected = None;
                self.ensure_loaded();
            }
            if ui.button("💾 Guardar").clicked() {
                self.save_current();
            }
            if ui.button("▶ Compilar").clicked() {
                self.compile_current();
            }
            ui.menu_button("＋ Añadir nodo", |ui| {
                let has_output = self
                    .graphs
                    .get(self.current)
                    .map(|g| g.nodes.iter().any(|n| matches!(n.kind, NodeKind::Output)))
                    .unwrap_or(false);
                let has_vertex_output = self
                    .graphs
                    .get(self.current)
                    .map(|g| g.nodes.iter().any(|n| matches!(n.kind, NodeKind::VertexOutput)))
                    .unwrap_or(false);
                let items: Vec<(&str, NodeKind)> = vec![
                    ("🎨 Color", NodeKind::Color { rgba: [1.0, 1.0, 1.0, 1.0] }),
                    ("🔢 Float", NodeKind::Float { value: 1.0 }),
                    ("⏱ Tiempo", NodeKind::Time),
                    ("🌐 UV", NodeKind::Uv),
                    ("📐 Normal", NodeKind::Normal),
                    ("➕ Sumar", NodeKind::Add),
                    ("✖ Multiplicar", NodeKind::Multiply),
                    ("🎚 Mezclar", NodeKind::Mix),
                    ("〰 Seno", NodeKind::Sine),
                    ("Abs", NodeKind::Abs),
                    ("1 − x", NodeKind::OneMinus),
                    ("Smoothstep", NodeKind::Smoothstep { edge0: 0.0, edge1: 1.0 }),
                    ("⭕ Radial UV", NodeKind::RadialUv { edge0: 0.7, edge1: 1.0 }),
                    ("✨ Fresnel", NodeKind::Fresnel { power: 2.0 }),
                    ("🖥 Salida", NodeKind::Output),
                    ("📍 Posición (vértices)", NodeKind::Position { axis: PositionAxis::All }),
                    ("🚀 Salida de vértices", NodeKind::VertexOutput),
                    ("🎯 Punto de interacción", NodeKind::InteractionPoint { component: InteractionComponent::All }),
                    ("🔊 Ruido de valor", NodeKind::ValueNoise { scale: 8.0 }),
                    ("🌊 Ruido gradiente", NodeKind::GradientNoise { scale: 5.0 }),
                    ("☁ Ruido FBM", NodeKind::FbmNoise { scale: 4.0, octaves: 4, lacunarity: 2.0, gain: 0.5 }),
                    ("➖ Restar", NodeKind::Subtract),
                    ("➗ Dividir", NodeKind::Divide),
                    ("🔴 Canal R", NodeKind::ChannelR),
                    ("🟢 Canal G", NodeKind::ChannelG),
                    ("🔵 Canal B", NodeKind::ChannelB),
                    ("⚪ Canal A", NodeKind::ChannelA),
                    ("🧩 Combinar RGBA", NodeKind::Combine),
                    ("· Producto escalar", NodeKind::Dot),
                    ("📏 Longitud", NodeKind::Length),
                    ("↔ Distancia", NodeKind::Distance),
                    ("🧭 Normalizar", NodeKind::Normalize),
                    ("✂ Limitar (Clamp)", NodeKind::Clamp { min: 0.0, max: 1.0 }),
                    ("⬆ Potencia", NodeKind::Power { exponent: 2.0 }),
                    ("🪜 Escalón (Step)", NodeKind::Step { edge: 0.5 }),
                    ("% Módulo", NodeKind::Modulo),
                ];
                for (label, kind) in items {
                    if matches!(kind, NodeKind::Output) && has_output {
                        continue;
                    }
                    if matches!(kind, NodeKind::VertexOutput) && has_vertex_output {
                        continue;
                    }
                    if ui.button(label).clicked() {
                        self.add_node(kind);
                        ui.close();
                    }
                }
            });
        });
        if let Some(status) = &self.status {
            ui.label(RichText::new(status.clone()).size(12.0).color(Color32::from_rgb(140, 220, 160)));
        }
        ui.separator();

        // ---------- Lienzo + inspector ----------
        ui.horizontal_top(|ui| {
            self.render_canvas(ui);
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(250.0);
                self.render_inspector(ui);
            });
        });

        // ---------- GLSL generado ----------
        if !self.glsl_preview.is_empty() {
            ui.separator();
            ui.label(RichText::new("GLSL generado").strong());
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.glsl_preview.as_str()).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
            });
        }
    }

    fn render_canvas(&mut self, ui: &mut Ui) {
        egui::ScrollArea::both().max_width(ui.available_width() - 270.0).show(ui, |ui| {
            let (_canvas_id, rect) = ui.allocate_space(Vec2::new(CANVAS_W, CANVAS_H));
            let origin = rect.min;
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, Color32::from_rgb(24, 25, 30));

            let graph = match self.graphs.get(self.current) {
                Some(g) => g.clone(),
                None => return,
            };
            let pos_of = |id: u32| -> Option<Pos2> {
                graph.nodes.iter().find(|n| n.id == id).map(|n| origin + Vec2::new(n.position[0], n.position[1]))
            };

            // Aristas: del borde derecho del origen al izquierdo del destino.
            for e in &graph.edges {
                if let (Some(a), Some(b)) = (pos_of(e.from_node), pos_of(e.to_node)) {
                    let start = a + Vec2::new(NODE_W, NODE_H * 0.5);
                    let end = b + Vec2::new(0.0, NODE_H * 0.5);
                    painter.line_segment([start, end], Stroke::new(2.0, Color32::from_rgb(0, 160, 255)));
                }
            }

            // Nodos: cajas arrastrables.
            let mut clicked: Option<u32> = None;
            let mut drags: Vec<(u32, Vec2)> = Vec::new();
            for node in &graph.nodes {
                let node_pos = origin + Vec2::new(node.position[0], node.position[1]);
                let node_rect = egui::Rect::from_min_size(node_pos, Vec2::new(NODE_W, NODE_H));
                let resp = ui.interact(node_rect, egui::Id::new(("sg_node", self.current, node.id)), Sense::click_and_drag());
                if resp.clicked() {
                    clicked = Some(node.id);
                }
                if resp.dragged() {
                    drags.push((node.id, resp.drag_delta()));
                }
                let selected = self.selected == Some(node.id);
                let is_output = matches!(node.kind, NodeKind::Output);
                let fill = if selected {
                    Color32::from_rgb(52, 58, 72)
                } else if is_output {
                    Color32::from_rgb(46, 40, 30)
                } else {
                    Color32::from_rgb(36, 39, 47)
                };
                painter.rect_filled(node_rect, 5.0, fill);
                painter.rect_stroke(node_rect, 5.0, Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { Color32::from_rgb(0, 160, 255) } else { Color32::from_rgb(70, 75, 88) }), egui::StrokeKind::Inside);
                painter.text(node_pos + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, kind_label(&node.kind), egui::FontId::proportional(13.0), Color32::WHITE);
                painter.text(node_pos + Vec2::new(8.0, 26.0), egui::Align2::LEFT_TOP, format!("#{}", node.id), egui::FontId::proportional(10.0), Color32::from_rgb(150, 155, 165));
            }

            if let Some(id) = clicked {
                self.selected = Some(id);
            }
            if !drags.is_empty() {
                if let Some(g) = self.graph_mut() {
                    for (id, delta) in drags {
                        if let Some(n) = g.nodes.iter_mut().find(|n| n.id == id) {
                            n.position[0] = (n.position[0] + delta.x).max(0.0);
                            n.position[1] = (n.position[1] + delta.y).max(0.0);
                        }
                    }
                }
            }
        });
    }

    fn render_inspector(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Nodo").size(15.0).strong().color(Color32::WHITE));
        ui.add_space(6.0);
        let Some(sel_id) = self.selected else {
            ui.label(RichText::new("Haz clic en un nodo del lienzo para editarlo. Arrástralo para moverlo.").size(12.0).color(Color32::from_rgb(170, 175, 185)));
            return;
        };
        let graph = match self.graphs.get(self.current) {
            Some(g) => g.clone(),
            None => return,
        };
        let Some(node_snapshot) = graph.nodes.iter().find(|n| n.id == sel_id).cloned() else {
            ui.label("El nodo seleccionado ya no existe.");
            return;
        };

        ui.label(RichText::new(format!("{}  #{}", kind_label(&node_snapshot.kind), sel_id)).strong());
        ui.add_space(6.0);

        // ----- Propiedades del nodo -----
        let mut prop_edit: Option<NodeKind> = None;
        match &node_snapshot.kind {
            NodeKind::Color { rgba } => {
                let mut c = Color32::from_rgba_unmultiplied(
                    (rgba[0] * 255.0) as u8,
                    (rgba[1] * 255.0) as u8,
                    (rgba[2] * 255.0) as u8,
                    (rgba[3] * 255.0) as u8,
                );
                if ui.color_edit_button_srgba(&mut c).changed() {
                    let f = c.to_normalized_gamma_f32();
                    prop_edit = Some(NodeKind::Color { rgba: f });
                }
            }
            NodeKind::Float { value } => {
                let mut v = *value;
                if ui.add(egui::DragValue::new(&mut v).speed(0.01).prefix("valor: ")).changed() {
                    prop_edit = Some(NodeKind::Float { value: v });
                }
            }
            NodeKind::Smoothstep { edge0, edge1 } => {
                let (mut e0, mut e1) = (*edge0, *edge1);
                let mut changed = false;
                changed |= ui.add(egui::DragValue::new(&mut e0).speed(0.01).prefix("borde 0: ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut e1).speed(0.01).prefix("borde 1: ")).changed();
                if changed {
                    prop_edit = Some(NodeKind::Smoothstep { edge0: e0, edge1: e1 });
                }
            }
            NodeKind::RadialUv { edge0, edge1 } => {
                // Los bordes de la caída radial del alfa: distancia a
                // la UV centrada (0 = centro, 1 = círculo inscrito).
                let (mut e0, mut e1) = (*edge0, *edge1);
                let mut changed = false;
                changed |= ui.add(egui::DragValue::new(&mut e0).speed(0.01).prefix("borde 0: ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut e1).speed(0.01).prefix("borde 1: ")).changed();
                if changed {
                    prop_edit = Some(NodeKind::RadialUv { edge0: e0, edge1: e1 });
                }
            }
            NodeKind::Fresnel { power } => {
                let mut p = *power;
                if ui.add(egui::DragValue::new(&mut p).speed(0.1).range(0.0..=8.0).prefix("potencia: ")).changed() {
                    prop_edit = Some(NodeKind::Fresnel { power: p });
                }
            }
            NodeKind::ValueNoise { scale } => {
                let mut s = *scale;
                if ui.add(egui::DragValue::new(&mut s).speed(0.1).range(0.1..=64.0).prefix("escala: ")).changed() {
                    prop_edit = Some(NodeKind::ValueNoise { scale: s });
                }
            }
            NodeKind::GradientNoise { scale } => {
                let mut s = *scale;
                if ui.add(egui::DragValue::new(&mut s).speed(0.1).range(0.1..=64.0).prefix("escala: ")).changed() {
                    prop_edit = Some(NodeKind::GradientNoise { scale: s });
                }
            }
            NodeKind::FbmNoise { scale, octaves, lacunarity, gain } => {
                let (mut s, mut o, mut l, mut g) = (*scale, *octaves, *lacunarity, *gain);
                let mut changed = false;
                changed |= ui.add(egui::DragValue::new(&mut s).speed(0.1).range(0.1..=64.0).prefix("escala: ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut o).speed(0.1).range(1..=6).prefix("octavas: ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut l).speed(0.01).range(1.0..=4.0).prefix("lagunaridad: ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut g).speed(0.01).range(0.0..=1.0).prefix("ganancia: ")).changed();
                if changed {
                    prop_edit = Some(NodeKind::FbmNoise { scale: s, octaves: o, lacunarity: l, gain: g });
                }
            }
            NodeKind::Clamp { min, max } => {
                let (mut lo, mut hi) = (*min, *max);
                let mut changed = false;
                changed |= ui.add(egui::DragValue::new(&mut lo).speed(0.01).prefix("mín: ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut hi).speed(0.01).prefix("máx: ")).changed();
                if changed {
                    prop_edit = Some(NodeKind::Clamp { min: lo, max: hi });
                }
            }
            NodeKind::Power { exponent } => {
                let mut e = *exponent;
                if ui.add(egui::DragValue::new(&mut e).speed(0.05).prefix("exponente: ")).changed() {
                    prop_edit = Some(NodeKind::Power { exponent: e });
                }
            }
            NodeKind::Step { edge } => {
                let mut e = *edge;
                if ui.add(egui::DragValue::new(&mut e).speed(0.01).prefix("borde: ")).changed() {
                    prop_edit = Some(NodeKind::Step { edge: e });
                }
            }
            NodeKind::Position { axis } => {
                // El eje que expone el nodo: un componente (float) o la
                // posición completa (vec3). Solo etapa de vértices.
                let mut a = *axis;
                egui::ComboBox::from_id_salt(("sg_axis", sel_id))
                    .selected_text(match a {
                        PositionAxis::X => "eje X (float)",
                        PositionAxis::Y => "eje Y (float)",
                        PositionAxis::Z => "eje Z (float)",
                        PositionAxis::All => "XYZ (vec3)",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut a, PositionAxis::X, "eje X (float)");
                        ui.selectable_value(&mut a, PositionAxis::Y, "eje Y (float)");
                        ui.selectable_value(&mut a, PositionAxis::Z, "eje Z (float)");
                        ui.selectable_value(&mut a, PositionAxis::All, "XYZ (vec3)");
                    });
                if a != *axis {
                    prop_edit = Some(NodeKind::Position { axis: a });
                }
            }
            NodeKind::InteractionPoint { component } => {
                // Qué entrega el nodo: el vec4 crudo, un componente o las
                // distancias con signo al punto (menos el radio). Vale en
                // vértices y en fragmento.
                let mut c = *component;
                egui::ComboBox::from_id_salt(("sg_inter", sel_id))
                    .selected_text(match c {
                        InteractionComponent::All => "XYZR (vec4)",
                        InteractionComponent::X => "X (float)",
                        InteractionComponent::Y => "Y (float)",
                        InteractionComponent::Z => "Z (float)",
                        InteractionComponent::Radius => "radio (float)",
                        InteractionComponent::Distance => "distancia 3D − radio",
                        InteractionComponent::DistanceXZ => "distancia XZ − radio",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut c, InteractionComponent::All, "XYZR (vec4)");
                        ui.selectable_value(&mut c, InteractionComponent::X, "X (float)");
                        ui.selectable_value(&mut c, InteractionComponent::Y, "Y (float)");
                        ui.selectable_value(&mut c, InteractionComponent::Z, "Z (float)");
                        ui.selectable_value(&mut c, InteractionComponent::Radius, "radio (float)");
                        ui.selectable_value(&mut c, InteractionComponent::Distance, "distancia 3D − radio");
                        ui.selectable_value(&mut c, InteractionComponent::DistanceXZ, "distancia XZ − radio");
                    });
                if c != *component {
                    prop_edit = Some(NodeKind::InteractionPoint { component: c });
                }
            }
            _ => {
                ui.label(RichText::new("Sin propiedades editables.").size(12.0).color(Color32::from_rgb(170, 175, 185)));
            }
        }
        if let Some(kind) = prop_edit {
            if let Some(g) = self.graph_mut() {
                if let Some(n) = g.nodes.iter_mut().find(|n| n.id == sel_id) {
                    n.kind = kind;
                }
            }
        }

        // ----- Cableado de entradas -----
        let inputs = kind_inputs(&node_snapshot.kind);
        if !inputs.is_empty() {
            ui.add_space(10.0);
            ui.label(RichText::new("Entradas").strong());
            // (entrada, nuevo origen o None para desconectar)
            let mut rewires: Vec<(String, Option<u32>)> = Vec::new();
            for input in inputs {
                let current_src = graph
                    .edges
                    .iter()
                    .find(|e| e.to_node == sel_id && e.to_input == *input)
                    .map(|e| e.from_node);
                let src_label = |id: Option<u32>| -> String {
                    match id {
                        Some(src) => graph
                            .nodes
                            .iter()
                            .find(|n| n.id == src)
                            .map(|n| format!("{} #{}", kind_label(&n.kind), src))
                            .unwrap_or_else(|| format!("#{src}")),
                        None => "— sin conectar —".to_string(),
                    }
                };
                ui.horizontal(|ui| {
                    ui.label(format!("{input}:"));
                    egui::ComboBox::from_id_salt(("sg_wire", sel_id, *input))
                        .selected_text(src_label(current_src))
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(current_src.is_none(), "— sin conectar —").clicked() {
                                rewires.push((input.to_string(), None));
                            }
                            for other in &graph.nodes {
                                if other.id == sel_id {
                                    continue;
                                }
                                let label = format!("{} #{}", kind_label(&other.kind), other.id);
                                if ui.selectable_label(current_src == Some(other.id), label).clicked() {
                                    rewires.push((input.to_string(), Some(other.id)));
                                }
                            }
                        });
                });
            }
            if !rewires.is_empty() {
                if let Some(g) = self.graph_mut() {
                    for (input, src) in rewires {
                        g.edges.retain(|e| !(e.to_node == sel_id && e.to_input == input));
                        if let Some(from_node) = src {
                            g.edges.push(GraphEdge { from_node, to_node: sel_id, to_input: input });
                        }
                    }
                }
            }
        }

        // ----- Eliminar -----
        ui.add_space(12.0);
        if !matches!(node_snapshot.kind, NodeKind::Output) && ui.button("🗑 Eliminar nodo").clicked() {
            if let Some(g) = self.graph_mut() {
                g.nodes.retain(|n| n.id != sel_id);
                g.edges.retain(|e| e.from_node != sel_id && e.to_node != sel_id);
            }
            self.selected = None;
        }
    }

    fn add_node(&mut self, kind: NodeKind) {
        if let Some(g) = self.graph_mut() {
            let next_id = g.nodes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
            let offset = (g.nodes.len() % 6) as f32 * 26.0;
            g.nodes.push(GraphNode { id: next_id, kind, position: [60.0 + offset, 60.0 + offset] });
            self.selected = Some(next_id);
        }
    }

    fn save_current(&mut self) {
        let Some(g) = self.graphs.get(self.current) else { return };
        let dir = std::path::Path::new("assets/shader_graphs");
        let path = dir.join(format!("{}.graph.json", g.name));
        let result = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, g.to_json()));
        self.status = Some(match result {
            Ok(_) => format!("💾 Grafo guardado en {}", path.display()),
            Err(e) => format!("❌ No se pudo guardar: {e}"),
        });
    }

    fn compile_current(&mut self) {
        let Some(g) = self.graphs.get(self.current) else { return };
        // Se validan las dos etapas: un error en la sub-red de vértices
        // (p. ej. un Fresnel, no soportado ahí en v1) debe verse aquí y
        // no al generar el .spv.
        let resultado = g.compile_fragment().and_then(|frag| {
            g.compile_vertex().map(|vert| (frag, vert))
        });
        match resultado {
            Ok((glsl, vert)) => {
                let nota_vert = if vert.is_some() {
                    " Incluye etapa de vértices (desplazamiento): su graph_<nombre>.vert.spv debe existir junto al .frag.spv."
                } else {
                    ""
                };
                self.status = Some(format!("✓ El grafo compila. Para usarlo como material, su .spv debe existir en assets/shaders/ y la entidad debe llevar shader = \"graph:<nombre>\".{nota_vert}"));
                self.glsl_preview = glsl;
            }
            Err(e) => {
                self.status = Some(format!("❌ Error de compilación: {e}"));
                self.glsl_preview.clear();
            }
        }
    }
}
