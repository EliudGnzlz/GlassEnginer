use egui::Ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Console,
    Project,
}

pub struct ConsolePanel {
    pub active_tab: BottomTab,
    pub logs: Vec<(String, &'static str)>,
    pub filter_info: bool,
    pub filter_warn: bool,
    pub filter_err: bool,
    // Explorador real de assets (Iteración 4): el panel ya no inventa
    // entradas, sino que lee el sistema de archivos.
    pub assets_path: std::path::PathBuf,
    pub current_dir: std::path::PathBuf,
    pub asset_entries: Vec<(String, bool, u64)>, // (nombre, es_carpeta, bytes)
}

impl Default for ConsolePanel {
    fn default() -> Self {
        // Preferir `assets/` relativo al directorio de trabajo; si no existe,
        // caer a `.` para no romper el editor en otros entornos.
        let assets_path = if std::path::Path::new("assets").is_dir() {
            std::path::PathBuf::from("assets")
        } else {
            std::path::PathBuf::from(".")
        };
        let mut panel = Self {
            active_tab: BottomTab::Console,
            logs: vec![
                ("[Vulkan] Inicializado dispositivo y colas gráficas con éxito.".into(), "INFO"),
                ("[Renderer] Offscreen Framebuffer 1280x720 asignado en VRAM.".into(), "INFO"),
                ("[Editor] Panel Docking estilo Unity renderizado en egui.".into(), "INFO"),
                ("[Hot-Reload] Vigilante de recursos activo en assets/.".into(), "INFO"),
            ],
            filter_info: true,
            filter_warn: true,
            filter_err: true,
            assets_path: assets_path.clone(),
            current_dir: assets_path,
            asset_entries: Vec::new(),
        };
        panel.refresh_assets();
        panel
    }
}

impl ConsolePanel {
    pub fn render_console_only(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("📋 Consola");
            ui.separator();
            ui.checkbox(&mut self.filter_info, "ℹ Info");
            ui.checkbox(&mut self.filter_warn, "⚠ Advertencias");
            ui.checkbox(&mut self.filter_err, "⛔ Errores");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🗑 Limpiar").clicked() {
                    self.logs.clear();
                }
            });
        });

        ui.separator();

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for (msg, level) in &self.logs {
                    let (badge, color) = match *level {
                        "ERROR" => ("⛔ [ERROR]", egui::Color32::from_rgb(255, 100, 100)),
                        "WARN" => ("⚠ [WARN]", egui::Color32::from_rgb(255, 200, 80)),
                        _ => ("ℹ [INFO]", egui::Color32::from_rgb(150, 200, 255)),
                    };

                    let show = match *level {
                        "ERROR" => self.filter_err,
                        "WARN" => self.filter_warn,
                        _ => self.filter_info,
                    };

                    if show {
                        ui.horizontal(|ui| {
                            ui.colored_label(color, badge);
                            ui.label(msg);
                        });
                    }
                }
            });
    }

    /// Explorador real de `assets/`.
    ///
    /// ITERACIÓN 4: antes esta pestaña mostraba cinco entradas **inventadas**
    /// (`triangle.vert`, `soundtrack.ogg`, ...) que no existían en disco, así
    /// que el panel mentía al usuario. Ahora se lee el sistema de archivos de
    /// verdad y los menús contextuales operan sobre rutas reales.
    pub fn render_project_only(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("📁 Proyecto / Assets");
            ui.separator();
            ui.label(format!("Ruta: {}/", self.assets_path.display()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🔄 Refrescar").clicked() {
                    self.refresh_assets();
                }
                if ui.button("⬆ Subir").clicked() {
                    if let Some(parent) = self.current_dir.parent() {
                        self.current_dir = parent.to_path_buf();
                        self.refresh_assets();
                    }
                }
            });
        });

        ui.separator();

        if self.asset_entries.is_empty() {
            ui.label(egui::RichText::new("(carpeta vacía)").italics().weak());
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Entradas del directorio actual: primero carpetas, luego archivos.
            let entries = self.asset_entries.clone();
            for (name, is_dir, size) in &entries {
                let label = if *is_dir {
                    format!("📁 {name}")
                } else {
                    format!("{} {name}", Self::icon_for(name))
                };
                let resp = ui.selectable_label(false, label);

                // Doble clic en carpeta entra; en archivo, registra en consola.
                if resp.double_clicked() {
                    let full = self.current_dir.join(name);
                    if *is_dir {
                        self.current_dir = full;
                        self.refresh_assets();
                    } else {
                        self.logs.push((
                            format!("[Proyecto] Abierto: {}", full.display()),
                            "INFO",
                        ));
                    }
                }

                // Menú contextual estilo Unity sobre el elemento
                resp.context_menu(|ui| {
                    ui.label(egui::RichText::new(name).strong());
                    ui.separator();

                    if *is_dir
                        && ui.button("📂 Abrir carpeta").clicked() {
                            self.current_dir = self.current_dir.join(name);
                            self.refresh_assets();
                            ui.close();
                        }

                    if ui.button("📋 Copiar ruta al portapapeles").clicked() {
                        let full = self.current_dir.join(name);
                        ui.ctx().copy_text(full.display().to_string());
                        self.logs.push((
                            format!("[Proyecto] Ruta copiada: {}", full.display()),
                            "INFO",
                        ));
                        ui.close();
                    }

                    if ui.button("ℹ Mostrar en consola").clicked() {
                        let full = self.current_dir.join(name);
                        self.logs.push((
                            format!("[Proyecto] {} ({} bytes)", full.display(), size),
                            "INFO",
                        ));
                        ui.close();
                    }

                    ui.separator();
                    if ui.button("🗑 Eliminar del disco").clicked() {
                        let full = self.current_dir.join(name);
                        // Operación destructiva: sólo se ejecuta si el archivo
                        // está dentro de assets/ y existe.
                        let dentro_de_assets = full.starts_with(&self.assets_path);
                        if !dentro_de_assets {
                            self.logs.push((
                                format!("[Proyecto] RECHAZADO: {} está fuera de assets/", full.display()),
                                "WARN",
                            ));
                        } else if *is_dir {
                            match std::fs::remove_dir_all(&full) {
                                Ok(_) => self.logs.push((
                                    format!("[Proyecto] Carpeta eliminada: {}", full.display()),
                                    "WARN",
                                )),
                                Err(e) => self.logs.push((
                                    format!("[Proyecto] Error al eliminar {}: {e}", full.display()),
                                    "ERROR",
                                )),
                            }
                        } else {
                            match std::fs::remove_file(&full) {
                                Ok(_) => self.logs.push((
                                    format!("[Proyecto] Archivo eliminado: {}", full.display()),
                                    "WARN",
                                )),
                                Err(e) => self.logs.push((
                                    format!("[Proyecto] Error al eliminar {}: {e}", full.display()),
                                    "ERROR",
                                )),
                            }
                        }
                        self.refresh_assets();
                        ui.close();
                    }
                });
            }

            // Menú contextual del fondo vacío: crear carpeta / refrescar.
            let remaining = ui.available_size();
            if remaining.y > 10.0 {
                let (_r, bg) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), remaining.y),
                    egui::Sense::click(),
                );
                bg.context_menu(|ui| {
                    ui.label("📁 Proyecto");
                    ui.separator();
                    if ui.button("🔄 Refrescar").clicked() {
                        self.refresh_assets();
                        ui.close();
                    }
                    if ui.button("➕ Nueva carpeta").clicked() {
                        let nueva = self.current_dir.join("NuevaCarpeta");
                        match std::fs::create_dir_all(&nueva) {
                            Ok(_) => self.logs.push((
                                format!("[Proyecto] Carpeta creada: {}", nueva.display()),
                                "INFO",
                            )),
                            Err(e) => self.logs.push((
                                format!("[Proyecto] Error al crear carpeta: {e}"),
                                "ERROR",
                            )),
                        }
                        self.refresh_assets();
                        ui.close();
                    }
                });
            }
        });
    }

    /// Icono por extensión para el explorador de assets.
    fn icon_for(name: &str) -> &'static str {
        let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "json" | "scene" => "🧾",
            "png" | "jpg" | "jpeg" => "🖼",
            "glb" | "gltf" | "blend" => "🧊",
            "spv" | "vert" | "frag" | "glsl" => "📄",
            "cs" => "📜",
            "ogg" | "wav" | "mp3" => "🔊",
            "md" => "📘",
            _ => "📄",
        }
    }

    /// Relee el directorio actual del explorador.
    fn refresh_assets(&mut self) {
        self.asset_entries.clear();
        let Ok(rd) = std::fs::read_dir(&self.current_dir) else {
            return;
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            self.asset_entries.push((name, is_dir, size));
        }
        // Carpetas primero, luego alfabético.
        self.asset_entries
            .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    }

    pub fn render(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.active_tab, BottomTab::Console, "📋 Consola");
            ui.selectable_value(&mut self.active_tab, BottomTab::Project, "📁 Proyecto / Assets");
        });

        ui.separator();

        match self.active_tab {
            BottomTab::Console => self.render_console_only(ui),
            BottomTab::Project => self.render_project_only(ui),
        }
    }
}
