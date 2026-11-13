use egui::{Color32, RichText, Stroke, Ui, Vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

    /// Nombre corto para la tarjeta de plantilla, como en el Hub viejo.
    pub fn short_name(&self) -> &'static str {
        match self {
            Self::Platformer3D => "Plataformas 3D",
            Self::PhysicsSandbox => "Físicas 3D",
            Self::EmptyScene => "Escena vacía",
        }
    }

    pub fn short_desc(&self) -> &'static str {
        match self {
            Self::Platformer3D => "Personaje 3D con físicas y script C#.",
            Self::PhysicsSandbox => "Cubos rígidos: fricción y rebote.",
            Self::EmptyScene => "Escena 3D limpia: cámara y luz solar.",
        }
    }

    pub fn glyph(&self) -> &'static str {
        match self {
            Self::Platformer3D => "🧊",
            Self::PhysicsSandbox => "🎲",
            Self::EmptyScene => "⬜",
        }
    }

    pub fn tile_color(&self) -> Color32 {
        match self {
            Self::Platformer3D => Color32::from_rgb(33, 150, 243),
            Self::PhysicsSandbox => Color32::from_rgb(255, 140, 0),
            Self::EmptyScene => Color32::from_rgb(130, 140, 150),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleScenePreset {
    PlatformerDemo,
    PhysicsPlayground,
    AudioLightingShowcase,
    FullSampleGame,
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
    Community,
    Installs,
}

/// El Launcher, con el aspecto del Hub VIEJO de Unity (el de la captura de
/// 2019 que guarda el usuario): tema CLARO (fondo blanco/gris muy claro,
/// acentos azules), navegación a la izquierda (Proyectos, Ejemplos,
/// Aprender, Comunidad, Instalaciones) y contenido a la derecha.
/// Los proyectos se muestran en CUADRÍCULA de tarjetas, cada una con la
/// imagen de su escena y el nombre debajo, para ubicarlos de un vistazo:
/// si la carpeta del proyecto trae un `thumbnail.png` se usa esa imagen;
/// si no, se genera un mosaico de color con la inicial.
///
/// Crear un proyecto NO abre ninguna ventana emergente: es una vista más
/// del Launcher (plantillas a la izquierda, ajustes a la derecha, como el
/// diálogo clásico pero integrado en la página).
///
/// Regla del proyecto: el Launcher es SIEMPRE offline. Las dos únicas
/// secciones que usarán internet cuando exista el repositorio oficial son
/// Instalaciones (descargar el motor y sus actualizaciones desde
/// GitHub Releases, módulo `updates`) y Comunidad (compartir
/// creaciones, shaders, mapas y juegos; aún sin red, botón
/// deshabilitado). El resto del Launcher funciona sin conexión:
/// Instalaciones solo se conecta cuando el usuario lo pide.
pub struct LauncherPanel {
    pub active_tab: LauncherTab,
    pub search_filter: String,
    /// Vista integrada de creación de proyecto (sustituye al antiguo
    /// formulario emergente dentro de la pestaña Proyectos).
    pub creating_project: bool,
    pub new_name: String,
    pub new_path: String,
    pub new_template: ProjectTemplate,
    pub status_message: Option<String>,
    /// Miniaturas ya intentadas por proyecto: `Some` si el `thumbnail.png`
    /// se cargó, `None` si no existe o no se pudo leer (no se reintenta).
    thumb_cache: HashMap<String, Option<egui::TextureHandle>>,
    /// Registro de versiones descargadas y repo de actualizaciones
    /// (módulo `updates`); se carga al abrir el Launcher.
    pub update_store: crate::updates::LauncherStore,
    /// Texto editable del ajuste «owner/repo».
    pub update_repo_edit: String,
    /// Estado de la comprobación/descarga en curso.
    pub update_state: UpdateUiState,
    /// Canal con el hilo que comprueba o instala (uno a la vez).
    update_rx: Option<std::sync::mpsc::Receiver<UpdateMsg>>,
}

/// Estado visible de las actualizaciones en la pestaña Instalaciones.
pub enum UpdateUiState {
    Idle,
    Checking,
    UpToDate,
    Available(crate::updates::ReleaseInfo),
    Working(String),
    Error(String),
}

/// Mensajes del hilo de red al Launcher.
pub enum UpdateMsg {
    Checked(Result<crate::updates::UpdateCheck, String>),
    Installed(Result<crate::updates::Installation, String>),
}

fn default_projects_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join("GlassProjects")
}

impl Default for LauncherPanel {
    fn default() -> Self {
        let name = "MyGlassGame".to_string();
        let path = default_projects_dir().join(&name);
        let update_store = crate::updates::LauncherStore::load();
        let update_repo_edit = update_store.repo.clone();
        Self {
            // Como el Hub viejo: se abre en la lista de proyectos.
            active_tab: LauncherTab::Projects,
            search_filter: String::new(),
            creating_project: false,
            new_name: name,
            new_path: path.to_string_lossy().into_owned(),
            new_template: ProjectTemplate::Platformer3D,
            status_message: None,
            thumb_cache: HashMap::new(),
            update_store,
            update_repo_edit,
            update_state: UpdateUiState::Idle,
            update_rx: None,
        }
    }
}

// Paleta clara del Hub viejo: blancos y grises muy claros, texto oscuro y
// el azul de Unity para selección y botones principales.
const ACCENT: Color32 = Color32::from_rgb(33, 150, 243);
const ACCENT_DARK: Color32 = Color32::from_rgb(21, 101, 192);
const SIDEBAR_BG: Color32 = Color32::from_rgb(250, 250, 251);
const CONTENT_BG: Color32 = Color32::from_rgb(242, 243, 245);
const CARD_BG: Color32 = Color32::WHITE;
const CARD_STROKE: Color32 = Color32::from_rgb(211, 213, 217);
const SELECTED_BG: Color32 = Color32::from_rgb(224, 240, 254);
const TEXT_MAIN: Color32 = Color32::from_rgb(33, 36, 41);
const TEXT_DIM: Color32 = Color32::from_rgb(106, 111, 121);
const OK_GREEN: Color32 = Color32::from_rgb(43, 140, 62);

/// Visuales claros que el Launcher impone mientras está en pantalla (el
/// editor fija los oscuros por su cuenta al dibujarse).
fn launcher_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    v.panel_fill = CONTENT_BG;
    v.window_fill = CARD_BG;
    v.extreme_bg_color = CARD_BG;
    v.faint_bg_color = Color32::from_rgb(243, 244, 246);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT_DARK;
    v
}

/// Paleta de mosaicos generados para proyectos sin `thumbnail.png`: el
/// color sale del nombre, así cada proyecto mantiene siempre el suyo.
const TILE_PALETTE: [Color32; 8] = [
    Color32::from_rgb(33, 150, 243),
    Color32::from_rgb(0, 172, 160),
    Color32::from_rgb(255, 140, 0),
    Color32::from_rgb(140, 100, 220),
    Color32::from_rgb(230, 80, 120),
    Color32::from_rgb(90, 180, 90),
    Color32::from_rgb(0, 150, 200),
    Color32::from_rgb(255, 110, 90),
];

fn tile_color_for(name: &str) -> Color32 {
    let mut h: u32 = 0;
    for b in name.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    TILE_PALETTE[(h as usize) % TILE_PALETTE.len()]
}

/// Parte un texto en dos líneas de como mucho `max_chars` caracteres la
/// primera; el resto va a la segunda (para las tarjetas de plantilla).
fn wrap_two_lines(text: &str, max_chars: usize) -> (String, String) {
    let mut line1 = String::new();
    let mut line2 = String::new();
    for word in text.split_whitespace() {
        let fits_first = line2.is_empty()
            && (line1.is_empty()
                || line1.chars().count() + 1 + word.chars().count() <= max_chars);
        let target = if fits_first { &mut line1 } else { &mut line2 };
        if !target.is_empty() {
            target.push(' ');
        }
        target.push_str(word);
    }
    (line1, line2)
}

/// Recorta un texto a `max` caracteres con puntos suspensivos (para que
/// los nombres largos no se salgan de su tarjeta).
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn initial_of(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".to_string())
}

/// Dibuja el mosaico generado de un proyecto: fondo de su color, una franja
/// inferior algo más oscura y la inicial en blanco. Es la miniatura de
/// reserva cuando el proyecto no trae imagen propia.
fn paint_generated_tile(painter: &egui::Painter, rect: egui::Rect, name: &str) {
    let base = tile_color_for(name);
    painter.rect_filled(rect, 4.0, base);
    let band = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, rect.max.y - rect.height() * 0.30),
        rect.max,
    );
    painter.rect_filled(
        band,
        4.0,
        Color32::from_rgba_unmultiplied(0, 0, 0, 36),
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initial_of(name),
        egui::FontId::proportional(30.0),
        Color32::WHITE,
    );
}

impl LauncherPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn render(&mut self, ui: &mut Ui) -> Option<LauncherAction> {
        // El Launcher es claro como el Hub viejo; el editor, oscuro.
        ui.ctx().set_visuals(launcher_visuals());
        let mut action: Option<LauncherAction> = None;

        // Paneles reales a pantalla completa: la barra lateral y el
        // contenido ocupan toda la ventana, sin zonas sin pintar.
        egui::Panel::left("glass_launcher_sidebar")
            .resizable(false)
            .exact_size(204.0)
            .frame(egui::Frame::new().fill(SIDEBAR_BG))
            .show(ui, |ui| {
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(RichText::new("🧊 GlassEnginer").size(19.0).strong().color(TEXT_MAIN));
                });
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(RichText::new("HUB").size(11.0).strong().color(ACCENT));
                });
                ui.add_space(20.0);

                self.menu_item(ui, LauncherTab::Projects, "📁", "Proyectos");
                self.menu_item(ui, LauncherTab::Samples, "✨", "Ejemplos");
                self.menu_item(ui, LauncherTab::Learn, "📚", "Aprender");
                ui.add_space(10.0);
                self.menu_item(ui, LauncherTab::Community, "🌐", "Comunidad");
                self.menu_item(ui, LauncherTab::Installs, "⬇", "Instalaciones");

                // Pie anclado al fondo de la ventana.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        // El Hub trabaja en local: sin cuentas ni nube.
                        ui.label(RichText::new("● Modo local (sin conexión)").size(11.0).color(OK_GREEN));
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.label(RichText::new(format!("v{} • Vulkan 1.3", crate::updates::ENGINE_VERSION)).size(11.0).color(TEXT_DIM));
                    });
                    ui.add_space(8.0);
                    ui.separator();
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(CONTENT_BG))
            .show(ui, |ui| {
                ui.add_space(16.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.vertical(|ui| {
                            if self.creating_project {
                                self.render_create_view(ui, &mut action);
                            } else {
                                match self.active_tab {
                                    LauncherTab::Projects => self.render_projects_tab(ui, &mut action),
                                    LauncherTab::Samples => self.render_samples_tab(ui, &mut action),
                                    LauncherTab::Learn => self.render_learn_tab(ui),
                                    LauncherTab::Community => self.render_community_tab(ui),
                                    LauncherTab::Installs => self.render_installs_tab(ui),
                                }
                            }
                        });
                        ui.add_space(24.0);
                    });
                });
                ui.add_space(16.0);
            });

        action
    }

    fn menu_item(&mut self, ui: &mut Ui, tab: LauncherTab, icon: &str, label: &str) {
        let selected = self.active_tab == tab && !self.creating_project;
        let text = RichText::new(format!("{icon}  {label}"))
            .size(14.0)
            .color(if selected { ACCENT_DARK } else { TEXT_MAIN });
        let text = if selected { text.strong() } else { text };
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            let fill = if selected { SELECTED_BG } else { Color32::TRANSPARENT };
            let mut clicked = false;
            egui::Frame::new().fill(fill).corner_radius(4.0).show(ui, |ui| {
                ui.set_min_width(176.0);
                if ui.selectable_label(selected, text).clicked() {
                    clicked = true;
                }
            });
            if clicked {
                // Navegar por la barra lateral abandona la vista de creación.
                self.creating_project = false;
                self.active_tab = tab;
            }
        });
        ui.add_space(2.0);
    }

    fn tab_title(ui: &mut Ui, title: &str, subtitle: &str) {
        ui.label(RichText::new(title).size(20.0).strong().color(TEXT_MAIN));
        ui.label(RichText::new(subtitle).size(12.0).color(TEXT_DIM));
        ui.add_space(14.0);
    }

    /// Botón principal azul, como el NEW/CREATE del Hub viejo.
    fn primary_button(ui: &mut Ui, label: &str, size: Vec2) -> egui::Response {
        ui.add_sized(
            size,
            egui::Button::new(RichText::new(label).size(13.0).strong().color(Color32::WHITE)).fill(ACCENT),
        )
    }

    /// Pinta la miniatura de un proyecto en `rect`: su `thumbnail.png` si
    /// la carpeta lo trae; si no, el mosaico generado con su inicial.
    fn paint_thumbnail(&mut self, ui: &Ui, rect: egui::Rect, name: &str, dir: &Path) {
        let key = dir.to_string_lossy().into_owned();
        if !self.thumb_cache.contains_key(&key) {
            let tex = std::fs::read(dir.join("thumbnail.png"))
                .ok()
                .and_then(|bytes| image::load_from_memory(&bytes).ok())
                .map(|img| {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [w as usize, h as usize],
                        &rgba,
                    );
                    ui.ctx().load_texture(
                        format!("project_thumb_{key}"),
                        color,
                        egui::TextureOptions::LINEAR,
                    )
                });
            self.thumb_cache.insert(key.clone(), tex);
        }
        match self.thumb_cache.get(&key).and_then(|t| t.as_ref()) {
            Some(tex) => {
                ui.painter().image(
                    tex.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => paint_generated_tile(ui.painter(), rect, name),
        }
        // Borde del mosaico, para que la miniatura se lea como tarjeta.
        ui.painter().rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, CARD_STROKE),
            egui::StrokeKind::Inside,
        );
    }

    /// Tarjeta de proyecto para la cuadrícula: imagen de la escena arriba
    /// y el nombre debajo, para ubicar cada proyecto de un vistazo. Un clic
    /// en la tarjeta lo abre en el editor.
    fn project_card(
        &mut self,
        ui: &mut Ui,
        name: &str,
        desc: &str,
        dir: &Path,
        action: &mut Option<LauncherAction>,
    ) {
        let size = Vec2::new(232.0, 178.0);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let painter = ui.painter();
        painter.rect_filled(rect, 6.0, CARD_BG);
        let stroke = if response.hovered() {
            Stroke::new(1.5, ACCENT)
        } else {
            Stroke::new(1.0, CARD_STROKE)
        };
        painter.rect_stroke(rect, 6.0, stroke, egui::StrokeKind::Inside);

        // Imagen de la escena, ocupando la parte alta de la tarjeta.
        let img_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 8.0, rect.min.y + 8.0),
            egui::pos2(rect.max.x - 8.0, rect.min.y + 124.0),
        );
        self.paint_thumbnail(ui, img_rect, name, dir);

        // Nombre y detalle bajo la imagen.
        painter.text(
            egui::pos2(rect.min.x + 10.0, rect.min.y + 141.0),
            egui::Align2::LEFT_CENTER,
            truncate_chars(name, 30),
            egui::FontId::proportional(14.0),
            TEXT_MAIN,
        );
        painter.text(
            egui::pos2(rect.min.x + 10.0, rect.min.y + 160.0),
            egui::Align2::LEFT_CENTER,
            truncate_chars(desc, 38),
            egui::FontId::proportional(10.5),
            TEXT_DIM,
        );

        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            *action = Some(LauncherAction::OpenDefaultScene);
        }
    }

    /// Fila de lista estilo Hub viejo: nombre, descripción/ruta y un botón.
    fn render_row<F: FnOnce()>(ui: &mut Ui, title: &str, desc: &str, btn_label: &str, accent: Color32, on_click: F) {
        egui::Frame::new()
            .fill(CARD_BG)
            .stroke(Stroke::new(1.0, CARD_STROKE))
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    let (bar_rect, _) = ui.allocate_exact_size(Vec2::new(4.0, 40.0), egui::Sense::hover());
                    ui.painter().rect_filled(bar_rect, 2.0, accent);
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(title).size(14.0).strong().color(TEXT_MAIN));
                        ui.label(RichText::new(desc).size(11.5).color(TEXT_DIM));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.0);
                        if ui.add_sized(Vec2::new(130.0, 28.0), egui::Button::new(RichText::new(btn_label).size(12.5).strong())).clicked() {
                            on_click();
                        }
                    });
                });
                ui.add_space(8.0);
            });
    }

    fn render_samples_tab(&mut self, ui: &mut Ui, action: &mut Option<LauncherAction>) {
        Self::tab_title(ui, "Ejemplos", "Escenas de ejemplo incluidas con el motor. Se abren al momento en el editor, sin descargar nada.");
        Self::render_row(ui, "★ 3D Platformer & Physics Demo (Escena de Prueba)",
            "Plataformas 3D, cuerpo rígido Rapier3D, script C# PlayerController (CoreCLR), audio Kira y shader PBR con luz solar.",
            "▶ Abrir", ACCENT,
            || *action = Some(LauncherAction::LoadSampleScene(SampleScenePreset::PlatformerDemo)));
        ui.add_space(8.0);
        Self::render_row(ui, "🎲 Physics Playground (Rapier3D)",
            "Cubos rígidos apilados, colisiones Cuboid, fricción y restitución (rebote) ajustables.",
            "▶ Abrir", Color32::from_rgb(255, 140, 0),
            || *action = Some(LauncherAction::LoadSampleScene(SampleScenePreset::PhysicsPlayground)));
        ui.add_space(8.0);
        Self::render_row(ui, "💡 Audio & Dynamic Lighting Showcase",
            "Luz direccional y puntual con gizmos de alcance y orientación, clips de audio y transformadas espaciales.",
            "▶ Abrir", Color32::from_rgb(220, 180, 30),
            || *action = Some(LauncherAction::LoadSampleScene(SampleScenePreset::AudioLightingShowcase)));
        ui.add_space(8.0);
        Self::render_row(ui, "🏆 Complete 3D Game Showcase (Juego Completo)",
            "Juego completo: jugador físico, cámara orbital, plataformas, gemas recolectables con audio 3D, HUD Canvas (Score & Timer) y meta.",
            "▶ Abrir", Color32::from_rgb(90, 180, 60),
            || *action = Some(LauncherAction::LoadSampleScene(SampleScenePreset::FullSampleGame)));
    }

    fn render_projects_tab(&mut self, ui: &mut Ui, action: &mut Option<LauncherAction>) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Proyectos").size(20.0).strong().color(TEXT_MAIN));
                ui.label(RichText::new("Tus proyectos de GlassEnginer en este equipo.").size(12.0).color(TEXT_DIM));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Como en el Hub viejo: NEW en azul, ADD/Abrir en claro.
                if Self::primary_button(ui, "➕ Nuevo", Vec2::new(110.0, 30.0)).clicked() {
                    self.creating_project = true;
                }
                if ui.add_sized(Vec2::new(110.0, 30.0), egui::Button::new(RichText::new("📂 Abrir").size(13.0))).clicked() {
                    *action = Some(LauncherAction::OpenDefaultScene);
                }
            });
        });
        ui.add_space(10.0);

        // Buscador (como en el Hub viejo: filtra la lista al escribir).
        ui.horizontal(|ui| {
            ui.label(RichText::new("🔎").size(13.0).color(TEXT_DIM));
            ui.add_sized(Vec2::new(280.0, 24.0), egui::TextEdit::singleline(&mut self.search_filter).hint_text("Buscar proyectos…"));
        });
        ui.add_space(12.0);

        // Cuadrícula de proyectos: cada uno en su cuadro, con la imagen de
        // su escena y el nombre debajo (hoy: el proyecto de trabajo del
        // motor; su carpeta es el directorio desde el que se lanza el Hub).
        let filter = self.search_filter.to_lowercase();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let projects: [(&str, &str, PathBuf); 1] = [(
            "GlassEnginer",
            "Proyecto del motor • En este equipo",
            cwd,
        )];
        let mut shown = 0;
        ui.spacing_mut().item_spacing = egui::vec2(14.0, 14.0);
        ui.horizontal_wrapped(|ui| {
            for (name, desc, dir) in &projects {
                if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                    continue;
                }
                shown += 1;
                self.project_card(ui, name, desc, dir, action);
            }
        });
        if shown == 0 {
            ui.label(RichText::new("Ningún proyecto coincide con la búsqueda.").size(12.0).color(TEXT_DIM));
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new("Consejo: deja un archivo thumbnail.png en la carpeta de un proyecto y el Hub lo usará como miniatura.")
                .size(11.5)
                .color(TEXT_DIM),
        );
    }

    /// Vista integrada de creación: el diálogo clásico del Hub viejo
    /// (plantillas a la izquierda, ajustes a la derecha) convertido en una
    /// página del Launcher, SIN ventana emergente.
    fn render_create_view(&mut self, ui: &mut Ui, action: &mut Option<LauncherAction>) {
        ui.label(RichText::new("Crear un proyecto nuevo").size(20.0).strong().color(TEXT_MAIN));
        ui.label(
            RichText::new(format!("GlassEnginer v{} • Todo se crea en tu equipo, sin conexión.", crate::updates::ENGINE_VERSION))
                .size(12.0)
                .color(TEXT_DIM),
        );
        ui.add_space(16.0);

        ui.horizontal_top(|ui| {
            // ----- Plantillas (izquierda) -----
            ui.vertical(|ui| {
                ui.label(RichText::new("Plantillas").size(14.0).strong().color(TEXT_MAIN));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    for template in [
                        ProjectTemplate::Platformer3D,
                        ProjectTemplate::PhysicsSandbox,
                        ProjectTemplate::EmptyScene,
                    ] {
                        self.template_card(ui, template);
                        ui.add_space(10.0);
                    }
                });
                ui.add_space(10.0);
                ui.label(
                    RichText::new(self.new_template.display_name()).size(12.0).color(TEXT_DIM),
                );
            });

            ui.add_space(28.0);

            // ----- Ajustes (derecha) -----
            ui.vertical(|ui| {
                ui.label(RichText::new("Ajustes").size(14.0).strong().color(TEXT_MAIN));
                ui.add_space(8.0);
                ui.label(RichText::new("Nombre del proyecto *").size(12.0).color(ACCENT_DARK));
                ui.add_sized(Vec2::new(300.0, 26.0), egui::TextEdit::singleline(&mut self.new_name));
                ui.add_space(10.0);
                ui.label(RichText::new("Ubicación *").size(12.0).color(ACCENT_DARK));
                ui.add_sized(Vec2::new(300.0, 26.0), egui::TextEdit::singleline(&mut self.new_path));
                ui.add_space(6.0);
                ui.label(
                    RichText::new("La carpeta del proyecto se crea en esa ubicación.")
                        .size(11.5)
                        .color(TEXT_DIM),
                );
                ui.add_space(16.0);
                ui.label(RichText::new("Así se verá en tu lista de proyectos:").size(12.0).color(TEXT_DIM));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(104.0, 66.0), egui::Sense::hover());
                    let preview_name = if self.new_name.trim().is_empty() {
                        "Sin nombre"
                    } else {
                        self.new_name.trim()
                    };
                    paint_generated_tile(ui.painter(), rect, preview_name);
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.add_space(10.0);
                        ui.label(RichText::new(preview_name).size(14.0).strong().color(TEXT_MAIN));
                        ui.label(RichText::new(self.new_template.short_name()).size(11.5).color(TEXT_DIM));
                    });
                });
            });
        });

        ui.add_space(22.0);
        ui.separator();
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let name_ok = !self.new_name.trim().is_empty();
            let create = ui.add_enabled_ui(name_ok, |ui| {
                Self::primary_button(ui, "Crear proyecto", Vec2::new(150.0, 32.0))
            });
            if create.inner.clicked() {
                *action = Some(LauncherAction::CreateProject {
                    name: self.new_name.clone(),
                    path: PathBuf::from(&self.new_path),
                    template: self.new_template,
                });
                self.creating_project = false;
            }
            if ui.add_sized(Vec2::new(110.0, 32.0), egui::Button::new(RichText::new("Cancelar").size(13.0))).clicked() {
                self.creating_project = false;
            }
        });
    }

    /// Tarjeta de plantilla seleccionable: mosaico con su glifo, nombre y
    /// una línea de descripción, como las del Hub viejo.
    fn template_card(&mut self, ui: &mut Ui, template: ProjectTemplate) {
        let size = Vec2::new(168.0, 138.0);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let selected = self.new_template == template;
        let painter = ui.painter();
        let fill = if selected { SELECTED_BG } else { CARD_BG };
        painter.rect_filled(rect, 6.0, fill);
        let stroke = if selected {
            Stroke::new(2.0, ACCENT)
        } else if response.hovered() {
            Stroke::new(1.0, ACCENT_DARK)
        } else {
            Stroke::new(1.0, CARD_STROKE)
        };
        painter.rect_stroke(rect, 6.0, stroke, egui::StrokeKind::Inside);

        // Mosaico superior con el glifo de la plantilla.
        let tile = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 10.0, rect.min.y + 10.0),
            egui::pos2(rect.max.x - 10.0, rect.min.y + 74.0),
        );
        painter.rect_filled(tile, 4.0, template.tile_color());
        painter.text(
            tile.center(),
            egui::Align2::CENTER_CENTER,
            template.glyph(),
            egui::FontId::proportional(30.0),
            Color32::WHITE,
        );
        painter.text(
            egui::pos2(rect.center().x, rect.min.y + 92.0),
            egui::Align2::CENTER_CENTER,
            template.short_name(),
            egui::FontId::proportional(13.5),
            TEXT_MAIN,
        );
        // Descripción corta en dos líneas como máximo, centrada.
        let (line1, line2) = wrap_two_lines(template.short_desc(), 25);
        painter.text(
            egui::pos2(rect.center().x, rect.min.y + 110.0),
            egui::Align2::CENTER_CENTER,
            line1,
            egui::FontId::proportional(10.0),
            TEXT_DIM,
        );
        if !line2.is_empty() {
            painter.text(
                egui::pos2(rect.center().x, rect.min.y + 123.0),
                egui::Align2::CENTER_CENTER,
                line2,
                egui::FontId::proportional(10.0),
                TEXT_DIM,
            );
        }

        if response.clicked() {
            self.new_template = template;
        }
    }

    fn render_learn_tab(&mut self, ui: &mut Ui) {
        Self::tab_title(ui, "Aprender", "Guía de inicio rápido y atajos del editor. Todo este contenido viene con el motor: no hace falta conexión.");
        egui::Frame::new()
            .fill(CARD_BG)
            .stroke(Stroke::new(1.0, CARD_STROKE))
            .corner_radius(6.0)
            .show(ui, |ui| {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("🎮 Atajos en la vista de escena").size(15.0).strong().color(ACCENT_DARK));
                        ui.label("• [ Q ] Herramienta Mano: desplazamiento libre (Pan) con clic y arrastre.");
                        ui.label("• [ W ] Herramienta Mover: gizmo 3D de traslación en ejes X, Y, Z.");
                        ui.label("• [ E ] Herramienta Rotar: anillos orbitales para orientar el objeto.");
                        ui.label("• [ R ] Herramienta Escalar: anclas en los extremos para redimensionar.");
                        ui.label("• [ Clic izquierdo ]: selección de objetos, cámaras y luces por raycasting.");
                        ui.label("• [ Clic derecho ]: órbita 3D de la cámara de edición.");
                        ui.label("• [ Rueda del ratón ]: zoom hacia el punto de enfoque.");
                        ui.label("• [ Delete ]: eliminar el objeto seleccionado.");
                        ui.add_space(10.0);
                        ui.label(RichText::new("🌿 Jerarquía estilo Unity").size(15.0).strong().color(ACCENT_DARK));
                        ui.label("• Arrastrar al centro de un objeto: anidarlo dentro de ese padre.");
                        ui.label("• Arrastrar al borde superior/inferior: reordenar con línea de inserción.");
                        ui.label("• Arrastrar al fondo vacío: desemparentar a la raíz.");
                        ui.label("• Clic derecho en una fila: menú contextual de creación y duplicado.");
                        ui.add_space(10.0);
                        ui.label(RichText::new("📜 Scripting C# (.NET CoreCLR)").size(15.0).strong().color(ACCENT_DARK));
                        ui.label("• Clases derivadas de GlassBehaviour con Awake(), Update(), OnCollisionEnter().");
                        ui.label("• Variables públicas editables en vivo desde el Inspector.");
                        ui.label("• Y con el MCP nativo, una IA puede trabajar el proyecto directamente.");
                    });
                    ui.add_space(16.0);
                });
                ui.add_space(12.0);
            });
    }

    fn render_community_tab(&mut self, ui: &mut Ui) {
        Self::tab_title(ui, "Comunidad", "El rincón para compartir lo que la gente crea con GlassEnginer.");
        egui::Frame::new()
            .fill(CARD_BG)
            .stroke(Stroke::new(1.0, CARD_STROKE))
            .corner_radius(6.0)
            .show(ui, |ui| {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("🌐 Todavía no hay comunidad en línea").size(15.0).strong().color(TEXT_MAIN));
                        ui.add_space(6.0);
                        ui.label(RichText::new("Aquí podrás subir y descargar creaciones de otros usuarios: juegos, mapas, shaders del grafo (como los prefabricados de agua o láser) y proyectos de ejemplo.").size(12.5).color(TEXT_DIM));
                        ui.add_space(6.0);
                        ui.label(RichText::new("El Launcher trabaja sin conexión; la única parte que usa internet es Instalaciones, y solo cuando tú pides buscar actualizaciones.").size(12.5).color(TEXT_DIM));
                        ui.add_space(12.0);
                        // Sin repositorio en línea, el botón queda deshabilitado:
                        // nada de prometer una red que aún no existe.
                        ui.add_enabled(false, egui::Button::new(RichText::new("Explorar la comunidad (próximamente)").size(13.0)));
                    });
                    ui.add_space(16.0);
                });
                ui.add_space(16.0);
            });
    }

    fn render_installs_tab(&mut self, ui: &mut Ui) {
        Self::tab_title(ui, "Instalaciones", "Versiones del motor en este equipo y actualizaciones desde GitHub Releases. Es la única parte del Launcher que usa internet, y solo cuando tú lo pides.");
        Self::render_row(ui, &format!("🧊 GlassEnginer v{}", crate::updates::ENGINE_VERSION),
            "Instalada • Esta copia del motor (Vulkan 1.3, Rapier3D, Kira, CoreCLR 8)",
            "✓ En uso", OK_GREEN,
            || {});
        let instalaciones = self.update_store.installations.clone();
        for inst in &instalaciones {
            Self::render_row(ui, &format!("🧊 GlassEnginer v{}", inst.version),
                &format!("Descargada • {}", inst.path),
                "✓ Instalada", OK_GREEN,
                || {});
        }
        ui.add_space(12.0);

        // Ajuste del repositorio de actualizaciones (owner/repo).
        ui.horizontal(|ui| {
            ui.label(RichText::new("Repositorio (owner/repo):").size(12.5).color(TEXT_MAIN));
            ui.add(egui::TextEdit::singleline(&mut self.update_repo_edit).desired_width(230.0));
            if ui.button("Guardar").clicked() {
                let repo = self.update_repo_edit.trim().trim_matches('/').to_string();
                if repo.is_empty() {
                    self.status_message = Some("El repositorio no puede estar vacío.".to_string());
                } else {
                    self.update_store.repo = repo.clone();
                    self.update_repo_edit = repo;
                    match self.update_store.save() {
                        Ok(()) => self.status_message = Some("Repositorio de actualizaciones guardado.".to_string()),
                        Err(e) => self.status_message = Some(format!("No se pudo guardar: {e}")),
                    }
                }
            }
        });
        ui.label(RichText::new("Sin cuentas ni tokens: solo se pueden leer repositorios públicos.").size(11.5).color(TEXT_DIM));
        ui.add_space(10.0);

        // Recoger el resultado del hilo de red, si ya llegó.
        if let Some(rx) = &self.update_rx {
            match rx.try_recv() {
                Ok(UpdateMsg::Checked(Ok(crate::updates::UpdateCheck::UpToDate))) => {
                    self.update_state = UpdateUiState::UpToDate;
                    self.update_rx = None;
                }
                Ok(UpdateMsg::Checked(Ok(crate::updates::UpdateCheck::Available(rel)))) => {
                    self.update_state = UpdateUiState::Available(rel);
                    self.update_rx = None;
                }
                Ok(UpdateMsg::Checked(Err(e))) => {
                    self.update_state = UpdateUiState::Error(e);
                    self.update_rx = None;
                }
                Ok(UpdateMsg::Installed(Ok(inst))) => {
                    self.update_store = crate::updates::LauncherStore::load();
                    self.update_state = UpdateUiState::Idle;
                    self.update_rx = None;
                    self.status_message = Some(format!("✅ GlassEnginer v{} descargado e instalado.", inst.version));
                }
                Ok(UpdateMsg::Installed(Err(e))) => {
                    self.update_state = UpdateUiState::Error(e);
                    self.update_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.ctx().request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.update_rx = None;
                    if matches!(self.update_state, UpdateUiState::Checking | UpdateUiState::Working(_)) {
                        self.update_state = UpdateUiState::Error("El hilo de red terminó sin responder.".to_string());
                    }
                }
            }
        }

        // Instantáneas propias del estado para no pelear con el
        // préstamo mientras se dibujan los botones.
        let ocupado = self.update_rx.is_some();
        let (es_checking, fase_working, disponible, error_txt, al_dia) = match &self.update_state {
            UpdateUiState::Checking => (true, None, None, None, false),
            UpdateUiState::Working(f) => (false, Some(f.clone()), None, None, false),
            UpdateUiState::Available(rel) => (false, None, Some(rel.clone()), None, false),
            UpdateUiState::Error(e) => (false, None, None, Some(e.clone()), false),
            UpdateUiState::UpToDate => (false, None, None, None, true),
            UpdateUiState::Idle => (false, None, None, None, false),
        };

        if es_checking {
            ui.add_enabled(false, egui::Button::new(RichText::new("⏳ Comprobando…").size(13.0)));
        } else if let Some(fase) = &fase_working {
            ui.add_enabled(false, egui::Button::new(RichText::new(format!("⏳ {fase}")).size(13.0)));
        } else if ui.add_enabled(!ocupado, egui::Button::new(RichText::new("⬇ Buscar actualizaciones").size(13.0))).clicked() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.update_rx = Some(rx);
            self.update_state = UpdateUiState::Checking;
            let repo = self.update_store.repo.clone();
            std::thread::spawn(move || {
                let r = crate::updates::check_update(&crate::updates::api_base(), &repo);
                let _ = tx.send(UpdateMsg::Checked(r));
            });
        }

        if al_dia {
            ui.add_space(6.0);
            ui.label(RichText::new(format!("✓ Estás en la última versión publicada (v{}).", crate::updates::ENGINE_VERSION)).size(12.5).color(OK_GREEN));
        }
        if let Some(rel) = &disponible {
            let tag = rel.tag_name.clone();
            ui.add_space(6.0);
            ui.label(RichText::new(format!("⬆ Hay una versión nueva: {tag} (esta copia es v{}).", crate::updates::ENGINE_VERSION)).size(13.0).strong().color(TEXT_MAIN));
            if crate::updates::is_installed(&self.update_store, &tag) {
                ui.label(RichText::new("Ya está descargada en este equipo (mira la lista de arriba).").size(12.5).color(TEXT_DIM));
            } else if !ocupado && ui.button(RichText::new(format!("⬇ Descargar e instalar {tag}")).size(13.0)).clicked() {
                let (tx, rx) = std::sync::mpsc::channel();
                self.update_rx = Some(rx);
                self.update_state = UpdateUiState::Working(format!("Descargando {tag}, verificando su SHA-256 e instalando…"));
                let rel2 = rel.clone();
                std::thread::spawn(move || {
                    let mut store = crate::updates::LauncherStore::load();
                    let r = crate::updates::install_release(&rel2, &mut store);
                    let _ = tx.send(UpdateMsg::Installed(r));
                });
            }
        }
        if let Some(e) = &error_txt {
            ui.add_space(6.0);
            ui.label(RichText::new(format!("⚠ {e}")).size(12.5).color(Color32::from_rgb(176, 42, 42)));
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Cada versión se instala en su propia carpeta y queda registrada: las anteriores no se borran y los proyectos no se tocan. Una descarga solo se instala si su SHA-256 coincide con el publicado.").size(12.0).color(TEXT_DIM));
    }
}
