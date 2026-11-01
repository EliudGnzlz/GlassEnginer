use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

#[derive(Debug, Clone)]
pub enum AssetReloadEvent {
    MeshReloaded(PathBuf),
    ShaderReloaded(PathBuf),
}

pub struct AssetWatcher {
    _watcher: RecommendedWatcher,
    pub receiver: Receiver<AssetReloadEvent>,
}

impl AssetWatcher {
    pub fn new(assets_dir: impl AsRef<Path>) -> Result<Self, String> {
        let (tx, rx) = channel();
        let path = assets_dir.as_ref().to_path_buf();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| match res {
                Ok(event) => {
                    Self::handle_event(event, &tx);
                }
                Err(e) => {
                    log::warn!("Error en vigía de assets: {:?}", e);
                }
            },
            Config::default(),
        )
        .map_err(|e| format!("Fallo al inicializar vigía de assets: {}", e))?;

        watcher
            .watch(&path, RecursiveMode::Recursive)
            .map_err(|e| format!("Fallo al observar directorio {:?}: {}", path, e))?;

        log::info!("AssetWatcher iniciado en {:?}", path);

        Ok(Self {
            _watcher: watcher,
            receiver: rx,
        })
    }

    fn handle_event(event: Event, tx: &Sender<AssetReloadEvent>) {
        if !matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
            return;
        }

        for path in event.paths {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                match ext {
                    "blend" => {
                        log::info!("Detectado cambio en archivo Blender: {:?}", path);
                        let glb_path = path.with_extension("glb");
                        let input_path = path.clone();
                        let tx_clone = tx.clone();

                        // Ejecutar conversión desatendida en un hilo separado para no bloquear
                        thread::spawn(move || {
                            let bridge_script = "tools/headless_blender_bridge.py";
                            if Path::new(bridge_script).exists() {
                                let output = Command::new("python3")
                                    .args([
                                        bridge_script,
                                        input_path.to_str().unwrap_or_default(),
                                        glb_path.to_str().unwrap_or_default(),
                                    ])
                                    .output();

                                match output {
                                    Ok(res) if res.status.success() => {
                                        log::info!(
                                            "Conversión headless exitosa: {:?} -> {:?}",
                                            input_path,
                                            glb_path
                                        );
                                        let _ = tx_clone
                                            .send(AssetReloadEvent::MeshReloaded(glb_path));
                                    }
                                    Ok(res) => {
                                        log::error!(
                                            "Fallo en conversión Blender:\n{}",
                                            String::from_utf8_lossy(&res.stderr)
                                        );
                                    }
                                    Err(e) => {
                                        log::error!("Fallo al invocar python bridge: {}", e);
                                    }
                                }
                            }
                        });
                    }
                    "glb" => {
                        log::info!("Detectado archivo glTF binario modificado: {:?}", path);
                        let _ = tx.send(AssetReloadEvent::MeshReloaded(path));
                    }
                    "vert" | "frag" => {
                        log::info!("Detectado shader fuente modificado: {:?}", path);
                        let _ = tx.send(AssetReloadEvent::ShaderReloaded(path));
                    }
                    _ => (),
                }
            }
        }
    }
}
