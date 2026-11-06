//! Gestión de escenas: carga real, diferida y observable.
//!
//! ITERACIÓN 5 — el stub de `SceneManager.LoadScene`.
//!
//! Hasta la Iteración 4 `SceneManager.LoadScene` sólo imprimía un log en C#:
//! no existía ninguna ruta en el motor que cargara una escena desde disco en
//! tiempo de ejecución. La carga inicial ocurría una única vez al arrancar
//! (`scene.load_from_json(&default_scene)`), de modo que el flujo de juego real
//! —menú → nivel 1 → nivel 2, o morir y reiniciar— era imposible.
//!
//! Este módulo cierra ese hueco con tres decisiones de diseño:
//!
//! 1. **Carga diferida al fin de frame.** `request_load` sólo anota la petición;
//!    `apply_pending` la ejecuta cuando el frame ya terminó su simulación. Es la
//!    semántica de Unity (`LoadScene` no es inmediato) y, aquí, la única segura:
//!    un script de C# puede pedir la carga desde dentro de su propio `Update`,
//!    mientras el motor itera entidades y tiene el mutex de la escena tomado.
//!    Destruir el mundo en ese instante sería un préstamo mutable vivo.
//! 2. **Toda la sustitución en un único punto.** `apply_pending` reemplaza las
//!    entidades, reasigna la tabla de cuerpos físicos y devuelve la lista de
//!    entidades que traen script, para que el llamante (que sí conoce el
//!    `ScriptHost` y el `PhysicsWorld`) haga el relevo sin acoplarlos aquí.
//! 3. **Fallos explícitos y no destructivos.** Si la escena no existe o el JSON
//!    es inválido, la escena activa **no se toca** y el error sube al llamante.
//!    Una escena rota no puede dejar el motor en un mundo vacío.

use crate::scene::SceneState;
use crate::vfs::{AssetVfs, VfsError};
use std::path::PathBuf;

/// Motivo por el que una carga de escena no pudo completarse.
#[derive(Debug)]
pub enum SceneError {
    /// No hay raíz de assets: no se encontró `project.json` subiendo por los padres.
    NoProjectRoot,
    /// El nombre no resolvió a ningún archivo de escena existente.
    NotFound { requested: String, available: Vec<String> },
    /// Ruta que escapa de la raíz de assets.
    InvalidPath(VfsError),
    /// Error de E/S al leer el archivo.
    Io { path: PathBuf, source: std::io::Error },
    /// El archivo existe pero no es una escena válida.
    Parse { path: PathBuf, message: String },
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SceneError::NoProjectRoot => write!(
                f,
                "no se encontró 'project.json' subiendo por los directorios padres: \
                 no hay raíz de assets desde la que resolver escenas"
            ),
            SceneError::NotFound { requested, available } => {
                write!(f, "escena '{requested}' no encontrada")?;
                if available.is_empty() {
                    write!(f, " (no hay escenas en assets/scenes)")
                } else {
                    write!(f, " (disponibles: {})", available.join(", "))
                }
            }
            SceneError::InvalidPath(e) => write!(f, "ruta de escena inválida: {e}"),
            SceneError::Io { path, source } => {
                write!(f, "error de E/S leyendo '{}': {source}", path.display())
            }
            SceneError::Parse { path, message } => {
                write!(f, "escena inválida '{}': {message}", path.display())
            }
        }
    }
}

impl std::error::Error for SceneError {}

/// Resultado de aplicar una carga pendiente.
#[derive(Debug, Clone)]
pub struct SceneLoaded {
    /// Nombre corto de la escena (sin ruta ni extensión).
    pub name: String,
    /// Clave de asset de la escena cargada.
    pub asset_key: String,
    /// Entidades que traen componente `Script`, para que el llamante instancie
    /// sus clases en el `ScriptHost`.
    pub scripted: Vec<(u64, String)>,
    /// Ids de todas las entidades cargadas.
    pub entity_ids: Vec<u64>,
}

/// Gestor de escenas del motor.
pub struct SceneManager {
    vfs: AssetVfs,
    /// Escena activa: nombre corto, tal como lo reporta `GetActiveSceneName`.
    active_name: String,
    /// Clave de asset de la escena activa.
    active_key: String,
    /// Petición pendiente de aplicar al fin de frame.
    pending: Option<String>,
    /// Solicitudes cuyo `apply_pending` ya se consumió, para estadísticas y tests.
    applied_count: u64,
}

impl SceneManager {
    /// Crea un gestor anclado a una raíz de proyecto explícita.
    pub fn new(root: impl AsRef<std::path::Path>) -> Self {
        Self {
            vfs: AssetVfs::new(root),
            active_name: "Untitled".to_string(),
            active_key: String::new(),
            pending: None,
            applied_count: 0,
        }
    }

    /// Descubre la raíz subiendo por los padres y construye el gestor.
    pub fn locate() -> Result<Self, SceneError> {
        Ok(Self::new(
            AssetVfs::locate().ok_or(SceneError::NoProjectRoot)?.root(),
        ))
    }

    /// VFS subyacente, para que el editor explore assets con la misma raíz.
    pub fn vfs(&self) -> &AssetVfs {
        &self.vfs
    }

    /// Nombre de la escena activa.
    pub fn active_scene_name(&self) -> &str {
        &self.active_name
    }

    /// Clave de asset de la escena activa (vacía si nunca se cargó ninguna).
    pub fn active_scene_key(&self) -> &str {
        &self.active_key
    }

    /// Nombres de las escenas disponibles en `assets/scenes`.
    pub fn available_scenes(&self) -> Vec<String> {
        self.vfs.list_scenes()
    }

    /// Número de cargas aplicadas con éxito.
    pub fn applied_count(&self) -> u64 {
        self.applied_count
    }

    /// `true` si hay una carga esperando al fin de frame.
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Petición pendiente, si la hay.
    pub fn pending_request(&self) -> Option<&str> {
        self.pending.as_deref()
    }

    /// **Valida** la petición sin encolarla.
    ///
    /// Se separa de `request_load` para que tanto el editor como el puente FFI
    /// puedan dar un error inmediato y preciso (con la lista de escenas
    /// disponibles) en lugar de fallar silenciosamente al fin del frame.
    pub fn validate(&self, name: &str) -> Result<String, SceneError> {
        self.vfs
            .resolve_scene(name)
            .ok_or_else(|| SceneError::NotFound {
                requested: name.trim().to_string(),
                available: self.available_scenes(),
            })
    }

    /// Encola una carga para el fin del frame actual.
    ///
    /// Devuelve la clave de asset validada: si esto retorna `Ok`, la escena
    /// existe y es legible, así que el `apply_pending` posterior sólo puede
    /// fallar por un JSON corrupto o por un borrado entre ambos momentos.
    pub fn request_load(&mut self, name: &str) -> Result<String, SceneError> {
        let key = self.validate(name)?;
        self.pending = Some(key.clone());
        Ok(key)
    }

    /// Cancela la petición pendiente.
    pub fn cancel_pending(&mut self) {
        self.pending = None;
    }

    /// Aplica la petición pendiente, si la hay.
    ///
    /// Devuelve `Ok(None)` cuando no había nada que hacer, `Ok(Some(_))` cuando
    /// la escena se sustituyó realmente, y `Err(_)` cuando falló —en cuyo caso
    /// la escena anterior queda intacta.
    pub fn apply_pending(&mut self, scene: &mut SceneState) -> Result<Option<SceneLoaded>, SceneError> {
        let Some(key) = self.pending.take() else {
            return Ok(None);
        };

        let path = self
            .vfs
            .resolve(&key)
            .map_err(SceneError::InvalidPath)?;

        let json = std::fs::read_to_string(&path).map_err(|e| SceneError::Io {
            path: path.clone(),
            source: e,
        })?;

        let entities: Vec<crate::scene::EntityData> =
            serde_json::from_str(&json).map_err(|e| SceneError::Parse {
                path: path.clone(),
                message: e.to_string(),
            })?;

        // A partir de aquí nada puede fallar: la escena se sustituye de golpe.
        // El parseo se hizo sobre `entities` (no releyendo el archivo dentro de
        // `load_from_json`) para reportar un `SceneError::Parse` con la ruta y el
        // mensaje de serde, y para no leer el disco dos veces.
        scene.replace_entities(entities);

        // `script_data.class_name` es la fuente de verdad (lleva además los
        // parámetros del componente), pero una escena antigua o editada a mano
        // puede traer sólo el campo `script` de la primera versión del formato.
        // Se prefieren los datos estructurados y se cae al string.
        let scripted: Vec<(u64, String)> = scene
            .entities
            .iter()
            .filter_map(|e| {
                let class = e
                    .script_data
                    .as_ref()
                    .map(|s| s.class_name.clone())
                    .or_else(|| e.script.clone())?;
                Some((e.id, class))
            })
            .collect();

        self.active_name = short_scene_name(&key);
        self.active_key = key.clone();
        self.applied_count += 1;

        Ok(Some(SceneLoaded {
            name: self.active_name.clone(),
            asset_key: key,
            scripted,
            entity_ids: scene.entities.iter().map(|e| e.id).collect(),
        }))
    }

    /// Clave de asset donde debe guardarse la escena activa.
    ///
    /// Si la escena activa vino de un archivo, se reescribe ese mismo archivo.
    /// Si se armó en código (`setup_*_scene`) o con `File > New Scene`, todavía
    /// no tiene archivo: se propone `<SCENES_DIR>/<nombre activo>.scene.json`.
    /// Antes el editor tenía `"assets/scenes/SampleScene.scene.json"` escrito a
    /// mano en cinco sitios, así que guardar una escena distinta pisaba la
    /// primera y el nombre que reportaba el puente no correspondía al archivo.
    pub fn save_key(&self) -> String {
        if !self.active_key.is_empty() {
            return self.active_key.clone();
        }
        scene_key_for_name(&self.active_name)
    }

    /// Ruta absoluta donde guardar la escena activa.
    pub fn save_path(&self) -> Result<std::path::PathBuf, SceneError> {
        self.vfs
            .resolve(&self.save_key())
            .map_err(SceneError::InvalidPath)
    }

    /// Marca la escena activa sin cargar nada de disco.
    ///
    /// Lo usan los constructores de escena en código (`setup_*_scene`), que
    /// arman el mundo a mano y no pasan por un archivo.
    pub fn set_active_name(&mut self, name: &str) {
        self.active_name = name.to_string();
        self.active_key.clear();
    }
}

/// `<SCENES_DIR>/<nombre saneado>.scene.json`.
///
/// El nombre se sanea para que no pueda escapar de `assets/scenes` con `/` o
/// `..`, y cae a `SampleScene` cuando está vacío o es el marcador `Untitled`.
pub fn scene_key_for_name(name: &str) -> String {
    let name = name.trim();
    let name = if name.is_empty() || name == "Untitled" {
        "SampleScene"
    } else {
        name
    };
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim();
    let safe = if safe.is_empty() { "SampleScene" } else { safe };
    format!("{}/{}.scene.json", crate::vfs::SCENES_DIR, safe)
}

/// `"assets/scenes/Nivel1.scene.json"` → `"Nivel1"`.
pub fn short_scene_name(asset_key: &str) -> String {
    let file = asset_key.rsplit('/').next().unwrap_or(asset_key);
    let stem = file.strip_suffix(".scene.json").unwrap_or(file);
    stem.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_project(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("glass_scenes_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(base.join("assets/scenes")).unwrap();
        // `ProjectSettings` exige todos los campos (sin `#[serde(default)]`),
        // así que el proyecto de prueba se escribe completo.
        std::fs::write(
            base.join("project.json"),
            r#"{"name":"T","version":"0.1.0","company_name":"T","default_scene":"assets/scenes/Nivel1.scene.json",
                "scenes_in_build":["assets/scenes/Nivel1.scene.json"],"window_width":1280,"window_height":720,
                "fullscreen":false,"target_fps":60,"vsync":true}"#,
        )
        .unwrap();
        base
    }

    /// Escena vacía real. `SceneState::new()` siembra una escena de demo con
    /// cámara, luz y suelo, que aquí sólo ensuciaría los recuentos.
    fn empty_scene() -> SceneState {
        let mut scene = SceneState::new();
        scene.entities.clear();
        scene.next_entity_id = 1;
        scene
    }

    /// Escribe una escena en disco **serializándola con el propio motor**, en
    /// lugar de con un JSON literal. Así el test no se rompe cuando
    /// `EntityData` gana un campo: sigue ejercitando el mismo camino que el
    /// editor y valida de paso el round-trip de serialización.
    fn write_scene(base: &std::path::Path, name: &str, build: impl FnOnce(&mut SceneState)) {
        let mut scene = empty_scene();
        build(&mut scene);
        scene
            .save_to_json(&base.join(format!("assets/scenes/{name}.scene.json")).to_string_lossy())
            .unwrap();
    }

    #[test]
    fn nombre_corto_quita_ruta_y_doble_extension() {
        assert_eq!(short_scene_name("assets/scenes/Nivel1.scene.json"), "Nivel1");
        assert_eq!(short_scene_name("Nivel1.scene.json"), "Nivel1");
        assert_eq!(short_scene_name("Suelto"), "Suelto");
    }

    #[test]
    fn carga_real_sustituye_la_escena_y_reporta_scripts() {
        let base = tmp_project("load");
        write_scene(&base, "Nivel1", |s| {
            let jugador = s.create_entity(
                "Jugador".into(),
                vec!["Transform".into(), "Script".into()],
            );
            s.modify_transform(jugador, Some([1.0, 2.0, 3.0]), None, None);
            s.create_entity("Piso".into(), vec!["Transform".into()]);
        });

        let mut mgr = SceneManager::new(&base);
        let mut scene = empty_scene();
        scene.create_entity("Basura".into(), vec!["Transform".into()]);

        let key = mgr.request_load("Nivel1").expect("debe validar");
        assert_eq!(key, "assets/scenes/Nivel1.scene.json");
        assert!(mgr.has_pending());

        let loaded = mgr
            .apply_pending(&mut scene)
            .expect("debe aplicar")
            .expect("debe haber carga");

        // La escena anterior fue reemplazada por completo, no fusionada.
        assert_eq!(scene.entities.len(), 2);
        assert!(scene.entities.iter().all(|e| e.name != "Basura"));
        // El nombre corto se reporta sin ruta ni extensión, y los ids cargados
        // son los del archivo.
        assert_eq!(loaded.name, "Nivel1");
        assert_eq!(loaded.entity_ids.len(), 2);
        // `next_entity_id` quedó por encima del mayor id del archivo, así que
        // una entidad creada después no colisiona con las cargadas.
        assert!(scene.next_entity_id > loaded.entity_ids.iter().copied().max().unwrap());
        // Solo la entidad con componente Script llega como script a instanciar.
        assert_eq!(loaded.scripted.len(), 1);
        assert_eq!(loaded.scripted[0].0, jugador_id(&scene));
        assert_eq!(
            loaded.scripted[0].1,
            "GlassEngine.Core.Scripts.PlayerController"
        );
        // Y queda registrada como activa.
        assert_eq!(mgr.active_scene_name(), "Nivel1");
        assert_eq!(mgr.applied_count(), 1);
        assert!(!mgr.has_pending(), "la petición se consume al aplicarse");

        std::fs::remove_dir_all(&base).ok();
    }

    /// Id de la entidad llamada "Jugador" en la escena ya cargada.
    fn jugador_id(scene: &SceneState) -> u64 {
        scene
            .entities
            .iter()
            .find(|e| e.name == "Jugador")
            .expect("la escena cargada debe tener un Jugador")
            .id
    }

    #[test]
    fn escena_inexistente_no_toca_la_escena_activa() {
        let base = tmp_project("missing");
        let mut mgr = SceneManager::new(&base);
        let mut scene = empty_scene();
        scene.create_entity("Intacta".into(), vec!["Transform".into()]);

        let err = mgr.request_load("NoExiste").expect_err("debe fallar");
        match err {
            SceneError::NotFound { requested, .. } => assert_eq!(requested, "NoExiste"),
            otro => panic!("se esperaba NotFound, llegó {otro:?}"),
        }
        // Un fallo de validación no deja nada encolado ni destruye el mundo.
        assert!(!mgr.has_pending());
        assert_eq!(scene.entities.len(), 1);
        assert_eq!(scene.entities[0].name, "Intacta");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn json_corrupto_deja_la_escena_anterior_intacta() {
        let base = tmp_project("corrupt");
        std::fs::write(
            base.join("assets/scenes/Rota.scene.json"),
            "{ esto no es una lista de entidades }",
        )
        .unwrap();
        let mut mgr = SceneManager::new(&base);
        let mut scene = empty_scene();
        scene.create_entity("Intacta".into(), vec!["Transform".into()]);

        // La validación sólo comprueba existencia, así que la petición se encola.
        mgr.request_load("Rota").expect("existe, debe encolar");
        let err = mgr.apply_pending(&mut scene).expect_err("debe fallar al parsear");
        assert!(matches!(err, SceneError::Parse { .. }), "llegó {err:?}");

        // Ésta es la garantía del módulo: la escena anterior sobrevive.
        assert_eq!(scene.entities.len(), 1);
        assert_eq!(scene.entities[0].name, "Intacta");
        assert_eq!(mgr.applied_count(), 0);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn nombre_vacio_usa_la_escena_por_defecto_del_proyecto() {
        let base = tmp_project("default");
        write_scene(&base, "Nivel1", |_| {});
        let mgr = SceneManager::new(&base);

        assert_eq!(
            mgr.validate("").expect("debe resolver el default").as_str(),
            "assets/scenes/Nivel1.scene.json"
        );
        assert_eq!(
            mgr.validate("<default>").expect("alias del default").as_str(),
            "assets/scenes/Nivel1.scene.json"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn peticion_pendiente_se_puede_cancelar() {
        let base = tmp_project("cancel");
        write_scene(&base, "Nivel1", |_| {});
        let mut mgr = SceneManager::new(&base);

        mgr.request_load("Nivel1").unwrap();
        assert!(mgr.has_pending());
        mgr.cancel_pending();
        assert!(!mgr.has_pending());

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn sin_peticion_pendiente_no_hay_nada_que_aplicar() {
        let base = tmp_project("idle");
        let mut mgr = SceneManager::new(&base);
        let mut scene = empty_scene();
        scene.create_entity("Intacta".into(), vec!["Transform".into()]);

        assert!(mgr.apply_pending(&mut scene).unwrap().is_none());
        assert_eq!(scene.entities.len(), 1);
        assert_eq!(mgr.applied_count(), 0);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn clave_de_guardado_sigue_la_escena_activa_y_no_un_literal() {
        let base = tmp_project("savekey");
        write_scene(&base, "Nivel1", |s| {
            s.create_entity("Uno".into(), vec!["Transform".into()]);
        });
        write_scene(&base, "Nivel2", |s| {
            s.create_entity("Dos".into(), vec!["Transform".into()]);
        });

        let mut mgr = SceneManager::new(&base);

        // Sin archivo asociado todavía: se propone uno derivado del nombre.
        mgr.set_active_name("Nivel2");
        assert_eq!(mgr.save_key(), "assets/scenes/Nivel2.scene.json");

        // Tras cargar, se reescribe exactamente el archivo de origen.
        mgr.request_load("Nivel1").unwrap();
        let mut scene = empty_scene();
        mgr.apply_pending(&mut scene).unwrap().unwrap();
        assert_eq!(mgr.save_key(), "assets/scenes/Nivel1.scene.json");

        // Y la ruta resuelta existe y es escribible por el motor.
        let path = mgr.save_path().unwrap();
        assert!(path.ends_with("assets/scenes/Nivel1.scene.json"));
        scene.save_to_json(&path.to_string_lossy()).unwrap();
        assert!(path.exists());

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn clave_de_guardado_sanea_nombres_peligrosos() {
        // Ni el marcador `Untitled` ni una ruta con `..` pueden producir una
        // clave que escape de `assets/scenes`.
        assert_eq!(scene_key_for_name("Untitled"), "assets/scenes/SampleScene.scene.json");
        assert_eq!(scene_key_for_name("   "), "assets/scenes/SampleScene.scene.json");
        assert_eq!(
            scene_key_for_name("../../etc/passwd"),
            // Los puntos también se sustituyen: no queda ni un `..` en la clave.
            "assets/scenes/______etc_passwd.scene.json"
        );
        // Guiones y espacios son legítimos y se conservan.
        assert_eq!(
            scene_key_for_name("Nivel 2 - Suelo"),
            "assets/scenes/Nivel 2 - Suelo.scene.json"
        );
    }

    #[test]
    fn ciclos_de_carga_sucesivos_son_independientes() {
        let base = tmp_project("cycle");
        write_scene(&base, "Nivel1", |s| {
            s.create_entity("Uno".into(), vec!["Transform".into()]);
        });
        write_scene(&base, "Nivel2", |s| {
            s.create_entity("Dos".into(), vec!["Transform".into()]);
            s.create_entity("Tres".into(), vec!["Transform".into()]);
        });
        let mut mgr = SceneManager::new(&base);
        let mut scene = empty_scene();

        for (nombre, esperado) in [("Nivel1", 1usize), ("Nivel2", 2), ("Nivel1", 1)] {
            mgr.request_load(nombre).unwrap();
            let loaded = mgr.apply_pending(&mut scene).unwrap().unwrap();
            assert_eq!(loaded.name, nombre);
            assert_eq!(scene.entities.len(), esperado);
            assert_eq!(mgr.active_scene_name(), nombre);
        }
        assert_eq!(mgr.applied_count(), 3);

        std::fs::remove_dir_all(&base).ok();
    }
}
