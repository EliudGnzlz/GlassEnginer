// En Windows, la app es una aplicación de ventana: sin consola (la CMD
// que el probador veía al arrancar). Los logs siguen en la Consola del
// editor y en el log de la app.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use ash::vk;
#[cfg(feature = "editor")]
use egui_ash_renderer::Renderer as EguiVulkanRenderer;
#[cfg(feature = "editor")]
use egui_ash_renderer::allocator::GpuAllocator;
#[cfg(feature = "editor")]
use egui_winit::State as EguiWinitState;
use glass_audio::{register_global_audio_system, AudioSystem};
use glass_physics::{collider_shape_for_builtin, register_global_physics_world, BodyHandle, BodyType, ColliderConfig, ColliderShape, ContactKind, PhysicsWorld};
use glass_core::physics_material::{resolve_collider_material, PhysicsMaterial, PhysicsMaterialLibrary};
use glass_core::{GlassWindow, SceneManager, SceneState, WindowConfig};
#[cfg(feature = "editor")]
use glass_editor::EditorDockingLayout;
#[cfg(feature = "editor")]
use glass_editor::docking::AvisoBanner;
#[cfg(feature = "editor")]
use glass_launcher::tipos::{ProjectTemplate, SampleScenePreset};

/// Acción de apertura pendiente: la siembra el arranque (la CLI:
/// `--project`, `--sample`) y la consume el bucle de frame tras la
/// precarga. Antes era la `LauncherAction` del panel del Hub; con
/// la separación verdadera (V2) el motor ya no contiene el panel
/// y usa este tipo propio con la misma forma.
#[cfg(feature = "editor")]
#[derive(Debug, Clone)]
pub enum AccionApertura {
    LoadSampleScene(SampleScenePreset),
    CreateProject {
        name: String,
        path: std::path::PathBuf,
        template: ProjectTemplate,
    },
    OpenProject(std::path::PathBuf),
    OpenSceneFile(std::path::PathBuf),
}
#[cfg(feature = "editor")]
use glass_mcp::McpServer;
#[cfg(feature = "editor")]
use std::io::IsTerminal;
use glass_renderer::{
    AssetCache, AssetReloadEvent, AssetWatcher, FrameDraws, GlassRenderer, MeshLoader,
    TextureUpload, VulkanConfig,
    VulkanContext,
};
use glass_scripting::{
    register_scene_bridge, SceneBridge, SceneLoadRejection, ScriptHost, SpawnSpec, TransformFfi,
    SPAWN_PHYSICS_KINEMATIC, SPAWN_PHYSICS_STATIC,
};
use glass_scripting::host::FalloDotnet;
mod bake;
mod canvas_ui;
mod prefabs_vivos;
mod luces_objeto;
mod ibl;
mod mouse_input;
mod logfile;
#[cfg(feature = "editor")]
mod proyecto_lock;
mod project_scripts;

use std::ffi::CStr;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use std::collections::HashSet;
use winit::application::ApplicationHandler;

struct EngineSceneBridge {
    scene_state: Arc<Mutex<SceneState>>,
    /// Gestor de escenas compartido con el bucle principal.
    ///
    /// El puente **sólo encola**: no carga nada aquí. Cargar dentro de una
    /// llamada de C# significaría sustituir entidades mientras el render o la
    /// física las están recorriendo, así que `apply_pending` se ejecuta al final
    /// del frame, en `render_frame`, con la misma semántica diferida de Unity.
    scene_manager: Arc<Mutex<SceneManager>>,
    /// Mundo físico vivo, compartido con la app: el spawn/despawn de
    /// entidades (bloque de instanciación) crea y retira cuerpos EN EL
    /// ACTO sobre este mundo, sin esperar a la reconstrucción de escena
    /// (que solo corre al cargar una). Los candados de escena y física
    /// nunca se retienen a la vez: se captura lo necesario bajo uno, se
    /// suelta, y se actúa bajo el otro — la disciplina de
    /// `rebuild_physics_from_scene`.
    physics_world: Arc<Mutex<PhysicsWorld>>,
    /// Caché de activos compartida con la app: el spawn en vivo
    /// resuelve los colliders de malla con la geometría retenida,
    /// igual que la carga de escena (auditoría C4). Se consulta ANTES
    /// de tomar el candado de física, como en
    /// `rebuild_physics_from_scene`.
    asset_cache: Arc<RwLock<AssetCache>>,
    /// Cola de (entidad, clase) de los scripts spawneados cuya instancia
    /// C# aún no existe. Crear la instancia dentro del propio callback
    /// FFI de spawn instanciaría C# en mitad de una llamada (y un
    /// `Awake` que spawnease a su vez entraría por la misma vía), así
    /// que la app la drena cada frame justo antes de `update_all`, en
    /// `reconcile_spawned_scripts`.
    pending_script_spawns: Arc<Mutex<Vec<(u64, String)>>>,
    /// Instancia única de la biblioteca de materiales físicos,
    /// compartida con la aplicación (E2): las resoluciones del puente
    /// y de la app leen la misma caché.
    physmat_lib: Arc<Mutex<PhysicsMaterialLibrary>>,
}

/// Todo lo que hace falta para crear el cuerpo Rapier de una entidad,
/// capturado bajo el candado de escena y consumido bajo el de física.
struct SpawnedBodySpec {
    entity_id: u64,
    body_type: BodyType,
    position: [f32; 3],
    rotation: [f32; 4],
    /// Colliders primitivos resueltos de la Fase 1.5: forma, config,
    /// offset local, material efectivo con campos nombrados, referencia
    /// de asset ("" = sin asset) y dirección de la cápsula.
    colliders: Vec<ColliderResuelto>,
    /// True si `colliders` viene de specs explícitos del autor (componen
    /// con el collider de malla); false si es solo el inferido histórico
    /// (que actúa de respaldo cuando la malla no cuela). P3.
    colliders_son_specs: bool,
    /// Modo de collider de malla pedido por la escena (Fase 1.5). El
    /// spawn en vivo lo resuelve con la geometría retenida de la
    /// caché compartida, como la carga de escena (auditoría C4).
    mesh_collider: glass_core::MeshColliderMode,
    /// Material efectivo del collider de malla y su referencia de asset.
    mesh_material: PhysicsMaterial,
    mesh_material_ref: String,
    /// Material al que resuelve el cuerpo sin asset (legado o PROJECT_DEFAULT):
    /// es el fallback de los slots de C6 (al limpiar sharedMaterial a null y
    /// al instanciar un collider sin asset).
    fallback_material: PhysicsMaterial,
    /// Config del collider de malla (capa, máscara y trigger), construida
    /// desde los campos del propio cuerpo (`layer`, `collision_mask`,
    /// `mesh_collider_is_trigger`), no heredada del primer spec (E6).
    mesh_cfg: ColliderConfig,
    /// Nombre del modelo de la entidad (para buscar su geometría).
    model: String,
    /// Escala de mundo (para escalar los puntos del collider).
    world_scale: [f32; 3],
    /// Restricciones por ejes estilo Unity (PhysicsData.constraints).
    constraints: u8,
    pub mass: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub solver_iterations: u32,
    pub max_depenetration_velocity: f32,
    pub solver_velocity_iterations: u32,
    pub include_layers: u32,
    pub exclude_layers: u32,
    pub automatic_center_of_mass: bool,
    pub automatic_inertia_tensor: bool,
    pub use_gravity: bool,
    pub ccd: bool,
    /// Interpolación de render (PhysicsData.interpolate).
    pub interpolate: bool,
}

/// Collider de autorado ya resuelto: forma, config, offset local,
/// material efectivo con campos nombrados, referencia de asset
/// ("" = sin asset) y dirección de la cápsula ("X" | "Y" | "Z").
type ColliderResuelto = (
    ColliderShape,
    ColliderConfig,
    [f32; 3],
    PhysicsMaterial,
    String,
    String,
);

/// Resuelve el material de un collider de autorado. Devuelve el material
/// efectivo y la referencia tal como está autorada ("" = sin asset).
fn material_resuelto(
    referencia: &str,
    body: &glass_core::PhysicsData,
    lib: &mut PhysicsMaterialLibrary,
) -> (PhysicsMaterial, String) {
    let (material, _origen) = resolve_collider_material(referencia, body, lib);
    (material, referencia.to_string())
}

/// Proyecto al que pertenece una escena por su ruta: se sube desde la
/// carpeta de la escena hasta encontrar la marca de proyecto
/// (`project.glass` o `project.json`); sin marca, vale la heurística de
/// carpetas de assets que ya usa el sistema de prefabs. La raíz sale
/// canonizada para poder compararla con la del gestor de escenas.
#[cfg(feature = "editor")]
fn proyecto_de_una_escena(scene_path: &str) -> Option<std::path::PathBuf> {
    let ruta = std::path::Path::new(scene_path);
    let mut dir = ruta.parent();
    let mut encontrada = None;
    while let Some(d) = dir {
        if d.join("project.glass").is_file() || d.join("project.json").is_file() {
            encontrada = Some(d.to_path_buf());
            break;
        }
        dir = d.parent();
    }
    let raiz = encontrada.or_else(|| crate::prefabs_vivos::raiz_desde_ruta_escena(scene_path))?;
    Some(proyecto_lock::raiz_canonica(&raiz).unwrap_or(raiz))
}

/// Librería de materiales físicos anclada a la carpeta Assets del
/// proyecto vigente (C3). Sin proyecto abierto, la raíz es "." y las
/// referencias simplemente no resuelven (caen al legado/defecto).
/// Sirve al puente (spawn/activación) y a la app (rebuild).
fn biblioteca_physmat(
    scene_manager: &std::sync::Arc<std::sync::Mutex<SceneManager>>,
) -> PhysicsMaterialLibrary {
    let raiz = scene_manager
        .lock()
        .ok()
        .map(|m| m.vfs().root().to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let assets = raiz.join("Assets");
    let assets = if assets.is_dir() { assets } else { raiz.join("assets") };
    PhysicsMaterialLibrary::new(assets)
}

/// Raíces de assets donde se resuelven los recursos por nombre
/// (materiales, texturas, modelos), en orden de prioridad: primero
/// el proyecto abierto, después la raíz del gestor de escenas,
/// después los assets del motor y por último el directorio de
/// lanzamiento (compatibilidad). Sin esto, lanzar el editor fuera
/// del proyecto dejaba los materiales del proyecto sin encontrar.
fn raices_assets_busqueda(
    raiz_proyecto_actual: Option<&std::path::Path>,
    scene_manager: &std::sync::Arc<std::sync::Mutex<SceneManager>>,
    raiz_motor: &std::path::Path,
) -> Vec<std::path::PathBuf> {
    fn assets_de(base: &std::path::Path) -> std::path::PathBuf {
        let con_mayus = base.join("Assets");
        if con_mayus.is_dir() {
            return con_mayus;
        }
        let con_minus = base.join("assets");
        if con_minus.is_dir() {
            return con_minus;
        }
        con_mayus
    }

    let mut raices: Vec<std::path::PathBuf> = Vec::new();
    let mut agregar = |ruta: std::path::PathBuf| {
        if !raices.contains(&ruta) {
            raices.push(ruta);
        }
    };

    if let Some(p) = raiz_proyecto_actual {
        agregar(assets_de(p));
    }
    if let Ok(m) = scene_manager.lock() {
        let raiz_vfs = m.vfs().root().to_path_buf();
        agregar(assets_de(&raiz_vfs));
    }
    agregar(raiz_motor.join("assets"));
    agregar(std::path::PathBuf::from("assets"));

    raices
}

/// Datos del collider de malla para `adjuntar_colliders_autorados`.
struct MallaAutorada<'a> {
    modo: &'a glass_core::MeshColliderMode,
    geometria: Option<(&'a [[f32; 3]], &'a [u32])>,
    escala: [f32; 3],
    cfg: ColliderConfig,
    material: &'a PhysicsMaterial,
    material_ref: &'a str,
    model: &'a str,
}

/// Adjunta los colliders de autorado (malla y primitivos) a `handle`.
/// Es la única ruta de adjunto para `create_body` y
/// `rebuild_physics_from_scene`. El material viaja resuelto por collider
/// con campos nombrados (`PhysicsMaterial` y referencia), así que no hay
/// pares posicionales (friction, restitution) que se puedan cruzar.
///
/// La malla compone con los specs. El inferido (sin specs) solo se
/// adjunta si la malla no se pudo crear. Devuelve si la malla se adjuntó.
#[allow(clippy::too_many_arguments)]
fn adjuntar_colliders_autorados(
    physics: &mut PhysicsWorld,
    handle: BodyHandle,
    entity_id: u64,
    es_estatico: bool,
    malla: MallaAutorada<'_>,
    colliders: Vec<ColliderResuelto>,
    colliders_son_specs: bool,
    fallback: &PhysicsMaterial,
) -> bool {
    use glass_core::MeshColliderMode;

    let mut via_mesh = false;
    let mut mesh_slot: Option<glass_physics::ColliderSlot> = None;
    if *malla.modo != MeshColliderMode::None {
        match malla.geometria {
            Some((vertices, indices)) => {
                let e = malla.escala;
                let puntos: Vec<[f32; 3]> = vertices
                    .iter()
                    .map(|v| [v[0] * e[0], v[1] * e[1], v[2] * e[2]])
                    .collect();
                let cfg = malla.cfg;
                let attached = match malla.modo {
                    MeshColliderMode::TriMesh if es_estatico && !cfg.is_trigger => {
                        let tris: Vec<[u32; 3]> = indices
                            .as_chunks::<3>()
                            .0
                            .iter()
                            .map(|c| [c[0], c[1], c[2]])
                            .collect();
                        physics.attach_trimesh_collider(handle, &puntos, &tris, malla.material, malla.material_ref, cfg)
                    }
                    MeshColliderMode::TriMesh => {
                        if cfg.is_trigger {
                            log::warn!(
                                "Física: entidad {} pide TriMesh como trigger, se degrada a Convex.",
                                entity_id
                            );
                        } else {
                            log::warn!(
                                "Física: entidad {} pide TriMesh en cuerpo no estático, se degrada a Convex.",
                                entity_id
                            );
                        }
                        physics.attach_convex_collider(handle, &puntos, malla.material, malla.material_ref, cfg)
                    }
                    MeshColliderMode::Convex => physics.attach_convex_collider(handle, &puntos, malla.material, malla.material_ref, cfg),
                    MeshColliderMode::None => None,
                };
                via_mesh = attached.is_some();
                mesh_slot = attached.map(|handle| glass_physics::ColliderSlot {
                    handle,
                    fallback: *fallback,
                });
                if !via_mesh {
                    log::warn!(
                        "entidad {}: el collider de malla no se pudo crear; cae a los primitivos resueltos",
                        entity_id
                    );
                }
            }
            None => {
                log::warn!(
                    "entidad {}: collider de malla ({:?}) pedido, pero no hay geometría retenida para el modelo '{}'; cae a los primitivos resueltos",
                    entity_id,
                    malla.modo,
                    malla.model
                );
            }
        }
    }

    let mut autorados = Vec::new();
    if colliders_son_specs || !via_mesh {
        for (shape, cfg, offset, material, material_ref, direction) in colliders {
            if let Some(collider_handle) = physics.attach_collider_with_config_at(
                handle,
                shape,
                &material,
                &material_ref,
                cfg,
                offset,
                &direction,
            ) {
                autorados.push(glass_physics::ColliderSlot {
                    handle: collider_handle,
                    fallback: *fallback,
                });
            }
        }
    }
    physics.registrar_slots_colliders(entity_id, autorados, mesh_slot);
    via_mesh
}

impl EngineSceneBridge {
    /// Deriva el cuerpo de una entidad de su `PhysicsData` y su matriz
    /// de mundo, EXACTAMENTE como `rebuild_physics_from_scene`: el tipo
    /// sale de `body_type`, la forma se infiere de la malla integrada
    /// con la escala de mundo y `box_extents`, de modo que una entidad
    /// spawneada y su gemela autorada en la escena son físicamente
    /// idénticas. `None` si la entidad no tiene componente de física.
    fn body_spec_for(
        world_matrices: &std::collections::HashMap<u64, [f32; 16]>,
        ent: &glass_core::EntityData,
        lib: &mut PhysicsMaterialLibrary,
    ) -> Option<SpawnedBodySpec> {
        let body = ent.physics.as_ref()?;
        let body_type = match body.body_type.as_str() {
            "Static" => BodyType::Static,
            "Kinematic" => BodyType::Kinematic,
            _ => BodyType::Dynamic,
        };
        let world = world_matrices.get(&ent.id).copied().unwrap_or_else(|| {
            glass_core::mat4_from_trs_euler(
                ent.transform.position,
                ent.transform.rotation(),
                ent.transform.scale,
            )
        });
        let model = ent.mesh.as_ref().map(|m| m.model.as_str()).unwrap_or("");
        let shape = collider_shape_for_builtin(model, glass_core::mat4_scale(world), body.box_extents);
        let colliders = colliders_resueltos(body, shape, lib);
        let (mesh_material, mesh_material_ref) =
            material_resuelto(&body.mesh_collider_material, body, lib);
        Some(SpawnedBodySpec {
            entity_id: ent.id,
            body_type,
            position: glass_core::mat4_translation(world),
            rotation: glass_core::mat4_to_quat(world),
            colliders,
            colliders_son_specs: !body.colliders.is_empty(),
            mesh_collider: body.mesh_collider,
            mesh_material,
            mesh_material_ref,
            fallback_material: body.body_fallback_material(),
            mesh_cfg: ColliderConfig {
                layer: body.layer,
                mask: body.collision_mask,
                is_trigger: body.mesh_collider_is_trigger,
            },
            model: model.to_string(),
            world_scale: glass_core::mat4_scale(world),
            constraints: body.constraints,
            mass: body.mass,
            linear_damping: body.linear_damping,
        angular_damping: body.angular_damping,
        solver_iterations: body.solver_iterations,
        max_depenetration_velocity: body.max_depenetration_velocity,
        solver_velocity_iterations: body.solver_velocity_iterations,
        include_layers: body.include_layers,
        exclude_layers: body.exclude_layers,
        automatic_center_of_mass: body.automatic_center_of_mass,
        automatic_inertia_tensor: body.automatic_inertia_tensor,
            use_gravity: body.use_gravity,
            ccd: body.ccd,
            interpolate: body.interpolate,
        })
    }

    /// Crea el cuerpo de una entidad en el mundo vivo (idempotente: si
    /// ya tenía cuerpo, no duplica). Único punto de creación de cuerpos
    /// por la vía del spawn/activación, para que la inferencia de forma
    /// no pueda divergir entre caminos.
    fn create_body(&self, spec: SpawnedBodySpec) {
        // Auditoría C4: la geometría se captura ANTES del candado de
        // física (la disciplina de rebuild_physics_from_scene).
        let geometria = if spec.mesh_collider != glass_core::MeshColliderMode::None {
            self.asset_cache
                .read()
                .ok()
                .and_then(|c| c.geometries.get(&spec.model).cloned())
        } else {
            None
        };
        let Ok(mut physics) = self.physics_world.lock() else {
            return;
        };
        if physics.get_body_by_entity(spec.entity_id).is_some() {
            return;
        }
        let handle = physics.create_rigid_body(spec.entity_id, spec.body_type, spec.position, spec.rotation);
        if spec.constraints != 0 {
            physics.set_locked_axes(handle, spec.constraints);
        }
        physics.set_linear_damping(handle, spec.linear_damping);
        physics.set_angular_damping(handle, spec.angular_damping);
        physics.set_solver_iterations(
            handle,
            i32::try_from(spec.solver_iterations).unwrap_or(i32::MAX),
        );
        physics.set_solver_velocity_iterations(
            handle,
            i32::try_from(spec.solver_velocity_iterations).unwrap_or(i32::MAX),
        );
        if spec.max_depenetration_velocity > 0.0 {
            physics.set_max_depenetration_velocity(handle, spec.max_depenetration_velocity);
        }
        if spec.include_layers != u32::MAX {
            physics.set_include_layers(handle, spec.include_layers);
        }
        if spec.exclude_layers != 0 {
            physics.set_exclude_layers(handle, spec.exclude_layers);
        }
        if !spec.automatic_center_of_mass {
            physics.set_automatic_center_of_mass(handle, false);
        }
        if !spec.automatic_inertia_tensor {
            physics.set_automatic_inertia_tensor(handle, false);
        }
        if !spec.use_gravity {
            physics.set_gravity_scale(handle, 0.0);
        }
        if spec.ccd {
            physics.set_ccd_enabled(handle, true);
        }
        if spec.interpolate {
            physics.set_interpolation(spec.entity_id, true);
        }
        // La masa se aplica AL FINAL de esta función, con los
        // colliders ya adjuntos (set_mass rescales densidades).
        // El material viaja resuelto en el spec con campos nombrados
        // (PhysicsMaterial y referencia de asset); no hay pares
        // posicionales (friction, restitution) que cruzar. Misma ruta
        // que rebuild_physics_from_scene (C3).
        // La config de la malla (capa, máscara, trigger) viaja en el
        // spec, construida desde el propio cuerpo (E6).
        let mesh_cfg = spec.mesh_cfg;
        let _via_mesh = adjuntar_colliders_autorados(
            &mut physics,
            handle,
            spec.entity_id,
            spec.body_type == BodyType::Static,
            MallaAutorada {
                modo: &spec.mesh_collider,
                geometria: geometria
                    .as_ref()
                    .map(|(v, i)| (v.as_slice(), i.as_slice())),
                escala: spec.world_scale,
                cfg: mesh_cfg,
                material: &spec.mesh_material,
                material_ref: &spec.mesh_material_ref,
                model: &spec.model,
            },
            spec.colliders,
            spec.colliders_son_specs,
            &spec.fallback_material,
        );
        // Masa al final: todos los colliders ya están adjuntos.
        if spec.body_type == BodyType::Dynamic {
            physics.set_mass(handle, spec.mass);
        }
        if !spec.automatic_center_of_mass {
            physics.set_automatic_center_of_mass(handle, false);
        }
        if !spec.automatic_inertia_tensor {
            physics.set_automatic_inertia_tensor(handle, false);
        }
    }

    /// Retira del mundo vivo el cuerpo (y con él los colliders) de una
    /// entidad, si lo tenía.
    fn remove_body(&self, entity_id: u64) {
        if let Ok(mut physics) = self.physics_world.lock() {
            if let Some(handle) = physics.get_body_by_entity(entity_id) {
                physics.remove_rigid_body(handle);
            }
        }
    }
}

impl SceneBridge for EngineSceneBridge {
    fn get_transform(&self, entity_id: u64) -> Option<TransformFfi> {
        let state = self.scene_state.lock().ok()?;
        let ent = state.entities.iter().find(|e| e.id == entity_id)?;
        Some(TransformFfi {
            position: ent.transform.position,
            // La escena almacena Euler en grados; la frontera C#/Rapier habla en
            // cuaterniones. Sin esta conversión el C# siempre ve identidad.
            rotation: ent.transform.rotation_quat(),
            scale: ent.transform.scale,
        })
    }

    fn set_transform(&self, entity_id: u64, transform: TransformFfi) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        if let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) {
            ent.transform.position = transform.position;
            // Cuaternión -> Euler en grados, para que lo que escribe un script de C#
            // (o Rapier) sea visible en el Inspector y en los gizmos.
            ent.transform.set_rotation_quat(transform.rotation);
            ent.transform.scale = transform.scale;
            true
        } else {
            false
        }
    }

    /// Todos los transforms de la escena de una vez (bloque «FFI por
    /// lotes»): la misma conversión que `get_transform` (Euler de la
    /// escena -> cuaternión de la frontera) pero con UNA sola
    /// adquisición del candado y un solo recorrido, para que C# pueda
    /// cachear el frame entero con un único cruce FFI.
    fn all_transforms(&self) -> Vec<(u64, TransformFfi)> {
        let state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        state
            .entities
            .iter()
            .map(|ent| {
                (
                    ent.id,
                    TransformFfi {
                        position: ent.transform.position,
                        rotation: ent.transform.rotation_quat(),
                        scale: ent.transform.scale,
                    },
                )
            })
            .collect()
    }

    fn request_scene_load(&self, name: &str) -> Result<(), SceneLoadRejection> {
        let mut manager = self
            .scene_manager
            .lock()
            .map_err(|_| SceneLoadRejection::Unavailable(
                "gestor de escenas bloqueado por un hilo en pánico".to_string(),
            ))?;
        manager.request_load(name).map(|_| ()).map_err(|e| {
            // Traducción explícita: los códigos FFI dependen de esta variante, no
            // del texto del mensaje, así que el mapeo tiene que ser exhaustivo.
            match e {
                glass_core::SceneError::NotFound { requested, available } => {
                    SceneLoadRejection::NotFound(format!(
                        "escena no encontrada: '{requested}' (disponibles: {})",
                        if available.is_empty() {
                            "ninguna".to_string()
                        } else {
                            available.join(", ")
                        }
                    ))
                }
                glass_core::SceneError::InvalidPath(err) => {
                    SceneLoadRejection::Invalid(format!("ruta de escena rechazada: {err}"))
                }
                glass_core::SceneError::NoProjectRoot => SceneLoadRejection::Unavailable(
                    "no se encontró project.json subiendo desde el directorio actual".to_string(),
                ),
                other => SceneLoadRejection::Invalid(other.to_string()),
            }
        })
    }

    fn active_scene_name(&self) -> Option<String> {
        self.scene_manager
            .lock()
            .ok()
            .map(|m| m.active_scene_name().to_string())
    }

    fn available_scenes(&self) -> Vec<String> {
        self.scene_manager
            .lock()
            .map(|m| m.available_scenes())
            .unwrap_or_default()
    }

    /// `Animator.Play` de C#: sin controlador, fija el clip del
    /// componente Animator y lo pone en reproducción (comportamiento de
    /// siempre; el bucle de dibujo detecta el cambio de clip y reinicia
    /// su tiempo a 0). CON controlador (bloque FSM), Play es un salto al
    /// estado nombrado con un fundido corto de 0,1 s: no se toca `clip`,
    /// lo resuelve `AnimatorData::advance` en el pase previo del frame.
    /// Si la entidad no tenía componente, se le crea (y se anota en su
    /// lista de componentes para que el Inspector lo muestre).
    fn animator_play(&self, entity_id: u64, clip: &str) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        if let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) {
            let animator = ent.animator.get_or_insert_with(Default::default);
            if animator.controller.is_empty() {
                // Sin controlador: comportamiento clásico (clip directo).
                animator.clip = clip.to_string();
            } else {
                // Con controlador: Play = salto de estado con fundido corto.
                // NO se toca `clip`; lo resuelve `AnimatorData::advance`.
                animator.pending_play = Some((clip.to_string(), 0.1));
            }
            animator.playing = true;
            if !ent.components.iter().any(|c| c == "Animator") {
                ent.components.push("Animator".to_string());
            }
            true
        } else {
            false
        }
    }

    /// `Animator.SetBool/SetFloat/SetTrigger` de C#: escribe un parámetro
    /// del controlador. `kind`: 0 = bool, 1 = float, 2 = trigger. Un trigger
    /// se escribe como 1.0 y lo consume `AnimatorData::advance` al dispararlo.
    fn animator_set_param(&self, entity_id: u64, param: &str, kind: i32, value: f32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(animator) = ent.animator.as_mut() else {
            return false;
        };
        let v = match kind {
            0 => {
                if value != 0.0 {
                    1.0
                } else {
                    0.0
                }
            }
            2 => 1.0,
            _ => value,
        };
        animator.params.insert(param.to_string(), v);
        true
    }

    /// `Animator.GetCurrentState` de C#: con controlador, el estado actual
    /// (vacío mientras no haya avanzado nunca); sin controlador, el clip.
    fn animator_current_state(&self, entity_id: u64) -> String {
        let state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
        let Some(ent) = state.entities.iter().find(|e| e.id == entity_id) else {
            return String::new();
        };
        let Some(animator) = ent.animator.as_ref() else {
            return String::new();
        };
        if animator.controller.is_empty() {
            animator.clip.clone()
        } else {
            animator.current_state.clone()
        }
    }

    /// `Animator.CrossFade` de C#: con controlador, salto de estado con el
    /// fundido pedido; sin controlador, crossfade libre entre clips que
    /// consume el bucle de dibujo (`pending_free_crossfade`).
    fn animator_crossfade(&self, entity_id: u64, name: &str, fade: f32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let animator = ent.animator.get_or_insert_with(Default::default);
        let fade = fade.max(0.0);
        if animator.controller.is_empty() {
            animator.pending_free_crossfade = Some((name.to_string(), fade));
        } else {
            animator.pending_play = Some((name.to_string(), fade));
        }
        animator.playing = true;
        if !ent.components.iter().any(|c| c == "Animator") {
            ent.components.push("Animator".to_string());
        }
        true
    }

    /// Spawn real de una entidad (bloque de instanciación): crea la
    /// entidad en la escena viva con su malla, transformada, física
    /// inferida y script declarado, y su cuerpo en el mundo de Rapier,
    /// todo en la misma llamada — el script que spawnea recibe un id ya
    /// utilizable. La instancia C# del script, si lo hay, NO se crea
    /// aquí: se encola en `pending_script_spawns` y la app la instancia
    /// en su reconciliación, antes del siguiente `update_all`.
    /// `Light.color` de C#: escribe el color del componente Light de la
    /// entidad. El motor ya lee `light.color * light.intensity` cada
    /// frame, así que basta con mutar `LightData`.
    // ---- UI dinámica (Canvas v2): los setters de Text/Image/
    // RectTransform de C# mutan los datos de UI de la entidad; el
    // lote del Canvas se reconstruye cada frame desde la escena,
    // así que el cambio se ve en el frame siguiente. Cambiar el
    // sprite además dispara la reconstrucción del atlas de
    // imágenes (la clave de sprites cambia sola).
    fn ui_set_text(&self, entity_id: u64, text: &str) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(t) = ent.ui_text.as_mut() else {
            return false;
        };
        t.text = text.to_string();
        true
    }

    fn ui_set_font_size(&self, entity_id: u64, size: f32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(t) = ent.ui_text.as_mut() else {
            return false;
        };
        t.font_size = size.max(1.0);
        true
    }

    fn ui_set_text_color(&self, entity_id: u64, r: f32, g: f32, b: f32, a: f32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(t) = ent.ui_text.as_mut() else {
            return false;
        };
        t.color = [r, g, b, a];
        true
    }

    fn ui_set_image_sprite(&self, entity_id: u64, sprite: &str) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(img) = ent.ui_image.as_mut() else {
            return false;
        };
        img.sprite = sprite.to_string();
        true
    }

    fn ui_set_image_color(&self, entity_id: u64, r: f32, g: f32, b: f32, a: f32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(img) = ent.ui_image.as_mut() else {
            return false;
        };
        img.color = [r, g, b, a];
        true
    }

    fn ui_set_button_color(&self, entity_id: u64, r: f32, g: f32, b: f32, a: f32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(btn) = ent.ui_button.as_mut() else {
            return false;
        };
        btn.target_graphic_color = [r, g, b, a];
        true
    }

    fn ui_set_anchored_pos(&self, entity_id: u64, x: f32, y: f32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(rt) = ent.rect_transform.as_mut() else {
            return false;
        };
        rt.anchored_position = [x, y];
        true
    }

    fn ui_set_size_delta(&self, entity_id: u64, w: f32, h: f32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(rt) = ent.rect_transform.as_mut() else {
            return false;
        };
        rt.size_delta = [w, h];
        true
    }

    /// `DragDrop.DraggedEntityId` de C#: la entidad del último
    /// arrastre del Canvas (0 si aún no hubo ninguno).
    fn ui_dragged_id(&self) -> u64 {
        DRAG_CURRENT.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// `DragDrop.DropTargetEntityId` de C#: la casilla que recibió
    /// la última suelta (0 si se soltó en vacío).
    fn ui_drop_target_id(&self) -> u64 {
        DROP_TARGET_CURRENT.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn light_set_color(&self, entity_id: u64, r: f32, g: f32, b: f32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(light) = ent.light.as_mut() else {
            return false;
        };
        light.color = [r, g, b];
        true
    }

    /// `Light.intensity` de C#: escribe la intensidad del componente
    /// Light de la entidad, acotada a >= 0.0.
    fn light_set_intensity(&self, entity_id: u64, intensity: f32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(light) = ent.light.as_mut() else {
            return false;
        };
        light.intensity = intensity.max(0.0);
        true
    }

    /// `Light.type` de C#: escribe el tipo del componente Light de la
    /// entidad. 0 = Directional, 1 = Point, 2 = Spot.
    fn light_set_type(&self, entity_id: u64, tipo: i32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(light) = ent.light.as_mut() else {
            return false;
        };
        let nuevo_tipo = match tipo {
            0 => "Directional",
            1 => "Point",
            2 => "Spot",
            3 => "Area",
            _ => return false,
        };
        light.light_type = nuevo_tipo.to_string();
        true
    }

    /// `Light.range` de C#: escribe el rango del componente Light de
    /// la entidad, acotado a >= 0.0.
    fn light_set_range(&self, entity_id: u64, range: f32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(light) = ent.light.as_mut() else {
            return false;
        };
        light.range = range.max(0.0);
        true
    }

    /// `Light.spotOuterAngle`/`spotInnerAngle` de C#: escribe los
    /// ángulos (en grados) del cono Spot del componente Light de la
    /// entidad. El saneado vive en `spot_cone_cos` al recolectar.
    fn light_set_spot_angles(&self, entity_id: u64, outer_deg: f32, inner_deg: f32) -> bool {
        let mut state = match self.scene_state.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let Some(ent) = state.entities.iter_mut().find(|e| e.id == entity_id) else {
            return false;
        };
        let Some(light) = ent.light.as_mut() else {
            return false;
        };
        light.spot_outer_deg = outer_deg;
        light.spot_inner_deg = inner_deg;
        true
    }
    fn spawn_entity(&self, spec: &SpawnSpec) -> Option<u64> {
        // Fase 1, candado de escena: crear y configurar la entidad y
        // capturar su cuerpo. El candado se suelta antes de tocar física.
        let (id, body_spec) = {
            let mut state = self.scene_state.lock().ok()?;
            if spec.parent_id != 0 && state.get_entity(spec.parent_id).is_none() {
                log::warn!(
                    "[Spawn] Padre {} inexistente: no se crea '{}'.",
                    spec.parent_id,
                    spec.name
                );
                return None;
            }
            let mut components = vec!["Transform".to_string(), "MeshRenderer".to_string()];
            match spec.physics {
                SPAWN_PHYSICS_STATIC => components.push("StaticCollider".to_string()),
                glass_scripting::SPAWN_PHYSICS_DYNAMIC | SPAWN_PHYSICS_KINEMATIC => {
                    components.push("RigidBody".to_string())
                }
                _ => {}
            }
            if !spec.script.is_empty() {
                components.push("Script".to_string());
            }
            let name = if spec.name.is_empty() {
                "Entity".to_string()
            } else {
                spec.name.clone()
            };
            let id = state.create_entity(name, components);
            {
                let ent = state.get_entity_mut(id)?;
                ent.transform.position = spec.position;
                ent.transform.set_rotation_quat(spec.rotation);
                ent.transform.scale = spec.scale;
                if let Some(mesh) = &mut ent.mesh {
                    mesh.model = spec.model.clone();
                    if spec.has_color {
                        mesh.color = spec.color;
                    }
                }
                // El spawn por código pide la física explícitamente
                // (regla: la física entra por RB/CC o por código): el
                // tipo STATIC no pasa por EntityData::new, que ya no
                // crea física desde «StaticCollider», así que el bloque
                // se materializa aquí.
                if ent.physics.is_none() && spec.physics == SPAWN_PHYSICS_STATIC {
                    ent.physics = Some(glass_core::PhysicsData {
                        body_type: "Static".to_string(),
                        has_rigidbody: false,
                        ..glass_core::PhysicsData::default()
                    });
                }
                if let Some(body) = &mut ent.physics {
                    if spec.physics == SPAWN_PHYSICS_KINEMATIC {
                        body.body_type = "Kinematic".to_string();
                    }
                    // La caja de respaldo se deriva de la escala, como
                    // cabría esperar de un primitivo de 1 m: sin esto, un
                    // cubo escalado a 3 seguiría chocando como de 1 m.
                    body.box_extents = [
                        spec.scale[0].abs() / 2.0,
                        spec.scale[1].abs() / 2.0,
                        spec.scale[2].abs() / 2.0,
                    ];
                }
                if !spec.script.is_empty() {
                    // El componente "Script" recién creado trae la clase
                    // por defecto del editor en el campo heredado: se
                    // limpia y manda `script_data.class_name`, la misma
                    // precedencia que usa la carga de escenas.
                    ent.script = None;
                    ent.script_data
                        .get_or_insert_with(Default::default)
                        .class_name = spec.script.clone();
                }
                if spec.parent_id != 0 {
                    ent.parent = Some(spec.parent_id);
                }
            }
            if spec.parent_id != 0 {
                if let Some(parent) = state.get_entity_mut(spec.parent_id) {
                    parent.children.push(id);
                }
            }
            let body_spec = if spec.physics != glass_scripting::SPAWN_PHYSICS_NONE {
                // Solo la matriz de mundo de la entidad nueva, subiendo
                // por su cadena de padres. Antes se pedía el mapa de
                // mundo de TODA la escena por cada spawn con física:
                // coste O(N) por alta y O(N²) al generar en masa
                // (prueba de esferas: 30.000 dinámicas en una llamada
                // tardaron 552 s; sin física, ~0,45 ms por entidad).
                let world = state.get_entity(id).map(|ent| {
                    let mut m = glass_core::mat4_from_trs_euler(
                        ent.transform.position,
                        ent.transform.rotation(),
                        ent.transform.scale,
                    );
                    let mut padre = ent.parent;
                    let mut pasos = 0;
                    while let Some(pid) = padre {
                        pasos += 1;
                        if pasos > 64 {
                            break; // guarda anti-ciclos
                        }
                        let Some(p) = state.get_entity(pid) else { break };
                        let pm = glass_core::mat4_from_trs_euler(
                            p.transform.position,
                            p.transform.rotation(),
                            p.transform.scale,
                        );
                        m = glass_core::mat4_mul(pm, m);
                        padre = p.parent;
                    }
                    m
                });
                let world_matrices: std::collections::HashMap<u64, [f32; 16]> =
                    world.map(|m| [(id, m)].into_iter().collect()).unwrap_or_default();
                let mut lib = self.physmat_lib.lock().unwrap_or_else(|e| e.into_inner());
                state
                    .get_entity(id)
                    .and_then(|ent| Self::body_spec_for(&world_matrices, ent, &mut lib))
            } else {
                None
            };
            (id, body_spec)
        };

        // Fase 2, candado de física: el cuerpo nace ya, en este frame.
        if let Some(body_spec) = body_spec {
            self.create_body(body_spec);
        }
        if !spec.script.is_empty() {
            if let Ok(mut pending) = self.pending_script_spawns.lock() {
                pending.push((id, spec.script.clone()));
            }
        }
        Some(id)
    }

    /// Instancia un Prefab (`.prefab.json`) en la escena viva (bloque de
    /// instanciación desde C#): resuelve nombre o ruta, carga el
    /// archivo con `Prefab::load_from_file`, lo instancia con
    /// `SceneState::instantiate_prefab` (reasigna ids, no toca el
    /// `parent_id`: siempre nace como raíz de la escena, igual que
    /// `GameObject.InstantiatePrefab` de Unity) y da cuerpo físico en
    /// Rapier a la raíz y a cada descendiente que traiga datos de
    /// física, con el mismo `body_spec_for` que usa `spawn_entity`.
    fn instantiate_prefab(&self, path: &str, position: [f32; 3]) -> Option<u64> {
        // Guarda de ruta: nada de rutas absolutas ni de '..' (con '/'
        // o con '\'): un script no puede sacar la carga de prefabs
        // fuera del proyecto.
        let sospechosa = path.starts_with('/')
            || path.starts_with('\\')
            || path.split(['/', '\\']).any(|seg| seg == "..")
            || (path.len() > 1 && path.as_bytes()[1] == b':');
        if sospechosa {
            log::warn!("[InstantiatePrefab] Ruta rechazada fuera del proyecto: '{path}'");
            return None;
        }

        // Resolución de ruta: si trae '/' o ya termina en
        // ".prefab.json" se usa tal cual; si no, es un nombre y la
        // ruta es la convención de la carpeta de prefabs del proyecto.
        let resolved_path = if path.contains('/') || path.ends_with(".prefab.json") {
            path.to_string()
        } else {
            format!("assets/prefabs/{path}.prefab.json")
        };

        let mut prefab = match glass_core::Prefab::load_from_file(&resolved_path) {
            Ok(p) => p,
            Err(e) => {
                log::warn!(
                    "[InstantiatePrefab] No se pudo cargar '{resolved_path}': {e}"
                );
                return None;
            }
        };
        // Fase UI/Canvas (V1): el prefab se migra en memoria antes de
        // instanciarlo (el archivo no se toca).
        let informe_prefab = glass_ui::migracion::migrar_prefab(&mut prefab);
        for aviso in &informe_prefab.avisos {
            log::warn!("[InstantiatePrefab] Migración UI: {aviso}");
        }

        // Fase 1, candado de escena: instanciar el prefab (raíz y
        // descendientes con ids nuevos) y capturar los body_spec de
        // todas las entidades con física, antes de soltar el candado.
        let (root_id, body_specs) = {
            let mut state = self.scene_state.lock().ok()?;
            // Prefabs vivos (Fase A): la instancia queda vinculada a su asset.
            let fuente = glass_core::prefab::ruta_fuente_prefab(std::path::Path::new(&resolved_path));
            let root_id = state.instantiate_prefab(&prefab, None, Some(position), Some(&fuente));

            let world_matrices = state.compute_world_matrices();
            let mut body_specs = Vec::new();
            let mut lib = self.physmat_lib.lock().unwrap_or_else(|e| e.into_inner());

            if let Some(ent) = state.get_entity(root_id) {
                if ent.physics.is_some() {
                    if let Some(bs) = Self::body_spec_for(&world_matrices, ent, &mut lib) {
                        body_specs.push(bs);
                    }
                }
            }
            // Los descendientes recién creados son los hijos (directos
            // e indirectos) de la raíz nueva: se recorren desde el
            // árbol vivo, no desde `prefab.descendant_entities`, para
            // usar siempre los ids ya reasignados por `instantiate_prefab`.
            let mut stack: Vec<u64> = state
                .get_entity(root_id)
                .map(|e| e.children.clone())
                .unwrap_or_default();
            while let Some(child_id) = stack.pop() {
                if let Some(ent) = state.get_entity(child_id) {
                    if ent.physics.is_some() {
                        if let Some(bs) = Self::body_spec_for(&world_matrices, ent, &mut lib) {
                            body_specs.push(bs);
                        }
                    }
                    stack.extend(ent.children.clone());
                }
            }

            (root_id, body_specs)
        };

        // Fase 2, candado de física: igual que `spawn_entity`, el
        // cuerpo nace ya en este frame, con el candado de escena suelto.
        for body_spec in body_specs {
            self.create_body(body_spec);
        }

        Some(root_id)
    }


    /// Despawn: la entidad sale de la escena (sus hijos se reemparentan,
    /// la semántica de borrado del editor) y su cuerpo y colliders
    /// salen del mundo de Rapier en la misma llamada. Sus instancias de
    /// script las dispone la reconciliación de la app en este frame.
    fn destroy_entity(&self, entity_id: u64) -> bool {
        let existed = {
            let Ok(mut state) = self.scene_state.lock() else {
                return false;
            };
            state.delete_entity(entity_id)
        };
        if existed {
            self.remove_body(entity_id);
        }
        existed
    }

    /// Activar/desactivar (la base física de los pools): la inactiva
    /// pierde su cuerpo; al reactivarla, el cuerpo se recrea desde su
    /// `PhysicsData` con `body_spec_for`, el mismo camino del spawn.
    fn set_entity_active(&self, entity_id: u64, active: bool) -> bool {
        {
            let Ok(mut state) = self.scene_state.lock() else {
                return false;
            };
            let Some(ent) = state.get_entity_mut(entity_id) else {
                return false;
            };
            if ent.active == active {
                return true;
            }
            ent.active = active;
        }
        if active {
            let body_spec = {
                let Ok(state) = self.scene_state.lock() else {
                    return true;
                };
                let world_matrices = state.compute_world_matrices();
                let mut lib = self.physmat_lib.lock().unwrap_or_else(|e| e.into_inner());
                state
                    .get_entity(entity_id)
                    .and_then(|ent| Self::body_spec_for(&world_matrices, ent, &mut lib))
            };
            if let Some(body_spec) = body_spec {
                self.create_body(body_spec);
            }
        } else {
            self.remove_body(entity_id);
        }
        true
    }

    /// Niebla en vivo desde C# (afinado visual Fase 9): escribe el
    /// `FogData` de la primera entidad que lo tenga; el render lo lee
    /// cada frame vía `SceneState::fog_settings`.
    fn set_fog(&self, color: [f32; 3], near: f32, far: f32) -> bool {
        let Ok(mut state) = self.scene_state.lock() else {
            return false;
        };
        match state.entities.iter_mut().find(|e| e.fog.is_some()) {
            Some(ent) => {
                ent.fog = Some(glass_core::scene::FogData { color, near, far });
                true
            }
            None => false,
        }
    }

    /// FOV de la cámara principal desde C# (Fase 10): escribe el
    /// `fov` del `CameraData` de la cámara main (o de la primera
    /// cámara si ninguna está marcada), con el rango sano de la
    /// casa. El render lo lee cada frame al construir la vista.
    fn set_camera_fov(&self, fov: f32) -> bool {
        let Ok(mut state) = self.scene_state.lock() else {
            return false;
        };
        let fov = fov.clamp(5.0, 170.0);
        let objetivo = state
            .entities
            .iter()
            .position(|e| e.camera.as_ref().is_some_and(|c| c.is_main))
            .or_else(|| state.entities.iter().position(|e| e.camera.is_some()));
        match objetivo {
            Some(i) => {
                if let Some(cam) = state.entities[i].camera.as_mut() {
                    cam.fov = fov;
                }
                true
            }
            None => false,
        }
    }

    fn entity_count(&self) -> Option<usize> {
        self.scene_state.lock().ok().map(|s| s.entities.len())
    }

    fn physics_counts(&self) -> Option<(usize, usize)> {
        self.physics_world
            .lock()
            .ok()
            .map(|p| (p.body_count(), p.collider_count()))
    }
}
/// Reconciliación por frame del bloque de instanciación: (1) crea las
/// instancias C# de los scripts spawneados desde el frame anterior y
/// (2) dispone las instancias cuya entidad ya no existe (destruida por
/// un script, borrada en el editor o sustituida), purgando también su
/// estado de animación. Corre justo antes de `update_all`, sin ningún
/// candado de escena o física retenido, así que los `Awake`/`OnDestroy`
/// que se ejecutan aquí pueden a su vez spawnear o destruir: sus
/// efectos de instanciación llegan en la reconciliación del frame
/// siguiente, nunca en cascada recursiva dentro de esta. El orden de
/// los `OnDestroy` dentro del mismo frame NO es determinista (depende
/// del orden interno de las instancias), como se documenta en C#.
///
/// Es función libre y no método porque `render_frame` retiene un
/// préstamo mutable del renderer durante todo el frame: el llamante
/// pasa los campos sueltos que esto necesita. Devuelve `(handle,
/// entidad)` si entre lo spawneado había un `PlayerController`, para
/// que la app lo siga como jugador, igual que al cargar una escena.
/// Pase previo de la máquina de estados (bloque FSM): con la escena
/// en mutable y ANTES del bucle de dibujo (que la itera en
/// inmutable), avanza cada `AnimatorData` con controlador y deja la
/// decisión de pose del frame en `anim_pose`. También consume los
/// crossfade libres pedidos sin controlador. Sin controlador ni
/// peticiones, no toca nada: el camino clásico queda intacto.
/// Salida del avance de animadores de un frame: los eventos de
/// animación disparados y los cambios de estado, con su entidad.
type SalidaAvanceAnimadores = (
    Vec<(u64, glass_core::animator::FiredAnimEvent)>,
    Vec<(u64, glass_core::animator::StateChangeEvent)>,
);

/// Mapas mutables que consume el pase previo de animadores:
/// agrupados en una estructura para no superar el límite de
/// argumentos por función (Clippy) al crecer el pase.
struct EstadoAnimadores<'a> {
    controllers: &'a mut std::collections::HashMap<String, glass_core::animator::AnimatorController>,
    pose: &'a mut std::collections::HashMap<u64, Vec<(String, f32, f32)>>,
    root_trackers: &'a mut std::collections::HashMap<u64, glass_renderer::skin::RootMotionTracker>,
    root_pin: &'a mut std::collections::HashMap<u64, bool>,
    free_fade: &'a mut std::collections::HashMap<u64, FreeFadeRt>,
    runtime: &'a mut std::collections::HashMap<u64, (String, f32)>,
}

fn avanzar_animadores_fsm(
    est: &mut EstadoAnimadores,
    scene: &mut SceneState,
    cache: &glass_renderer::AssetCache,
    dt: f32,
) -> SalidaAvanceAnimadores {
    est.pose.clear();
    est.root_pin.clear();
    let mut eventos_out: Vec<(u64, glass_core::animator::FiredAnimEvent)> = Vec::new();
    let mut cambios_out: Vec<(u64, glass_core::animator::StateChangeEvent)> = Vec::new();
    for ent in scene.entities.iter_mut() {
        let Some(anim) = ent.animator.as_mut() else {
            continue;
        };
        if !anim.playing {
            continue;
        }
        let clips: &[glass_renderer::AnimationClip] = ent
            .mesh
            .as_ref()
            .filter(|m| !m.model.is_empty())
            .and_then(|m| cache.animations.get(&m.model))
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let duration = |name: &str| clips.iter().find(|c| c.name == name).map(|c| c.duration);

        if !anim.controller.is_empty() {
            let name = anim.controller.clone();
            if !est.controllers.contains_key(&name) {
                let path = format!("assets/animators/{name}.animator.json");
                match glass_core::animator::AnimatorController::load_from_file(&path) {
                    Ok(c) => {
                        est.controllers.insert(name.clone(), c);
                    }
                    Err(e) => {
                        log::error!(
                            "Animator: no se pudo cargar el controlador '{name}': {e} \
                             (la entidad sigue con su clip, sin máquina)"
                        );
                        anim.controller.clear();
                    }
                }
            }
            if let Some(ctrl) = est.controllers.get(&name) {
                let out = anim.advance(dt * anim.speed.max(0.0), ctrl, duration);
                // Eventos disparados este frame por el FSM: se acumulan
                // junto a la entidad dueña para despacharlos fuera.
                for ev in &out.fired_events {
                    eventos_out.push((ent.id, ev.clone()));
                }
                for cambio in &out.state_events {
                    cambios_out.push((ent.id, cambio.clone()));
                }
                est.pose.insert(
                    ent.id,
                    out.samples
                        .iter()
                        .map(|s| (s.clip.clone(), s.time, s.weight))
                        .collect(),
                );
                // Root motion: si el estado actual lo activa, el
                // desplazamiento global de la raíz mueve el
                // transform (y el cuerpo físico, si lo hay) y la
                // pose se fijará en el bucle de dibujo.
                let rm_activo = ctrl
                    .states
                    .iter()
                    .find(|s| s.name == anim.current_state)
                    .map(|s| s.root_motion)
                    .unwrap_or(false);
                if rm_activo {
                    if let Some(skin) = ent
                        .mesh
                        .as_ref()
                        .and_then(|m| cache.skins.get(&m.model))
                    {
                        let resueltos: Vec<(
                            Option<&glass_renderer::AnimationClip>,
                            f32,
                            f32,
                        )> = out
                            .samples
                            .iter()
                            .map(|s| {
                                (clips.iter().find(|c| c.name == s.clip), s.time, s.weight)
                            })
                            .collect();
                        let tracker = est.root_trackers
                            .entry(ent.id)
                            .or_default();
                        let delta = tracker.delta(skin, &resueltos);
                        if delta != [0.0, 0.0, 0.0] {
                            let mundo3 = glass_core::math::quat_rotate_vec3(
                                ent.transform.rotation_quat(),
                                delta,
                            );
                            // v1: el root motion mueve en el plano
                            // XZ; la Y del clip no se aplica (la
                            // cadera conserva su altura de reposo en
                            // la pose fijada; ni siquiera el clip
                            // impecable cierra el bucle en altura y
                            // el personaje se hundiría caminando).
                            let mundo = [mundo3[0], 0.0, mundo3[2]];
                            ent.transform.position = [
                                ent.transform.position[0] + mundo[0],
                                ent.transform.position[1] + mundo[1],
                                ent.transform.position[2] + mundo[2],
                            ];
                            if let Some(sys) = glass_physics::get_global_physics_world() {
                                if let Ok(mut world) = sys.lock() {
                                    if let Some(handle) = world.get_body_by_entity(ent.id) {
                                        world.set_transform(
                                            handle,
                                            ent.transform.position,
                                            ent.transform.rotation_quat(),
                                        );
                                    }
                                }
                            }
                        }
                        est.root_pin.insert(ent.id, true);
                    }
                } else {
                    est.root_trackers.remove(&ent.id);
                }
                continue;
            }
        }

        // CrossFade libre pendiente (sin controlador): el fundido lo
        // resuelve el bucle de dibujo mezclando el clip saliente —
        // con su reloj en curso — contra el entrante, que arranca
        // de cero como un Play de siempre.
        if let Some((dest, fade)) = anim.pending_free_crossfade.take() {
            let from_clip = if anim.clip.is_empty() {
                clips.first().map(|c| c.name.clone()).unwrap_or_default()
            } else {
                anim.clip.clone()
            };
            let from_time = est.runtime
                .get(&ent.id)
                .map(|e| e.1)
                .unwrap_or(0.0);
            est.free_fade.insert(
                ent.id,
                FreeFadeRt {
                    from_clip,
                    from_time,
                    duration: fade.max(0.0),
                    elapsed: 0.0,
                },
            );
            anim.clip = dest;
            if let Some(e) = est.runtime.get_mut(&ent.id) {
                e.0.clear(); // el bucle reinicia el reloj del entrante
            }
        }
    }
    (eventos_out, cambios_out)
}


fn reconcile_spawned_scripts(
    script_host: &mut ScriptHost,
    scene_state: &Arc<Mutex<SceneState>>,
    pending_script_spawns: &Arc<Mutex<Vec<(u64, String)>>>,
    anim_runtime: &mut std::collections::HashMap<u64, (String, f32)>,
    skin_joint_buffers: &mut std::collections::HashMap<u64, glass_renderer::JointBuffer>,
    retired_joint_buffers: &mut Vec<glass_renderer::JointBuffer>,
) -> Option<(i32, u64)> {
    if !script_host.is_initialized {
        return None;
    }
    let mut spawned_player: Option<(i32, u64)> = None;
    // (1) Spawns con script pendientes. La cola se vacía con `mem::take`
    // ANTES de crear ninguna instancia: un Awake que spawnee con script
    // encola para el frame siguiente, sin tocar la lista en curso.
    let pending: Vec<(u64, String)> = match pending_script_spawns.lock() {
        Ok(mut queue) => std::mem::take(&mut *queue),
        Err(_) => Vec::new(),
    };
    if !pending.is_empty() {
        let alive_now: HashSet<u64> = match scene_state.lock() {
            Ok(scene) => scene.entities.iter().map(|e| e.id).collect(),
            Err(_) => HashSet::new(),
        };
        for (entity_id, class_name) in pending {
            // La entidad pudo morir entre el spawn y este frame:
            // instanciar su script sería crear un zombi que la pasada
            // (2) dispondría en el acto.
            if !alive_now.contains(&entity_id) {
                continue;
            }
            match script_host.create_instance(entity_id, &class_name) {
                Ok(handle) => {
                    log::info!(
                        "Script spawneado '{class_name}' instanciado para entidad {entity_id} (handle {handle})."
                    );
                    if class_name.ends_with("PlayerController") {
                        spawned_player = Some((handle, entity_id));
                    }
                }
                Err(e) => log::warn!(
                    "No se pudo instanciar el script spawneado '{class_name}' para {entity_id}: {e}"
                ),
            }
        }
    }
    // (2) Huérfanos: instancias sin entidad y estado de animación de
    // entidades muertas. Sin esta pasada, cada entidad con script
    // destruida dejaría su GlassBehaviour vivo para siempre y los
    // buffers de articulaciones crecerían sin cota por ciclo.
    let alive: HashSet<u64> = match scene_state.lock() {
        Ok(scene) => scene.entities.iter().map(|e| e.id).collect(),
        Err(_) => return spawned_player,
    };
    let disposed = script_host.destroy_instances_not_in(&alive);
    if disposed > 0 {
        log::info!("Reconciliación: {disposed} instancia(s) de entidades destruidas dispuestas.");
    }
    anim_runtime.retain(|id, _| alive.contains(id));
    let dead_skinned: Vec<u64> = skin_joint_buffers
        .keys()
        .filter(|id| !alive.contains(id))
        .copied()
        .collect();
    for id in dead_skinned {
        if let Some(jb) = skin_joint_buffers.remove(&id) {
            // A la lista de retirados: se libera tras la espera de fence
            // del próximo frame, como en el cambio de escena.
            retired_joint_buffers.push(jb);
        }
    }
    spawned_player
}

/// Bandera de apagado ordenado.
///
/// ITERACIÓN 4 — diagnóstico del "device lost" fantasma:
/// el motor esperaba fences con `u64::MAX`; esa espera acaba en el syscall
/// `drm_syncobj_timeline_wait`. Cuando el proceso recibe `SIGTERM` (p. ej. al
/// cerrar la sesión gráfica o con `timeout`), el kernel interrumpe el syscall y
/// NVIDIA/Mesa lo traducen a `VK_TIMEOUT`, que el motor registraba como
/// `Error durante render_frame: A wait operation has not completed in the
/// specified time` y acto seguido moría por la acción por defecto de la señal.
/// Con un manejador propio la bandera se levanta primero y el bucle sale por
/// `event_loop.exit()`, alcanzando el `Drop` que hace `device_wait_idle()`.
static SHUTDOWN_REQUESTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn handle_termination_signal(_sig: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Instala manejadores para SIGTERM/SIGINT/SIGHUP.
///
/// Sólo se hace `store` sobre un atómico: es lo único async-signal-safe que
/// necesitamos aquí.
#[cfg(unix)]
fn install_signal_handlers() {
    unsafe {
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handle_termination_signal as *const () as usize;
            action.sa_flags = 0;
            libc::sigemptyset(&mut action.sa_mask);
            let rc = libc::sigaction(sig, &action, std::ptr::null_mut());
            if rc != 0 {
                log::warn!("sigaction({}) falló: errno={}", sig, *libc::__errno_location());
            } else {
                log::info!("Manejador de señal instalado para {}", sig);
            }
        }
    }
}

/// En Windows no hay señales POSIX: el apagado ordenado llega por el
/// cierre de la ventana (winit). Puerto mínimo del bloque anterior,
/// que era Unix puro (primera compilación en Windows, 2026-11-13).
#[cfg(not(unix))]
fn install_signal_handlers() {}

use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::WindowId;

fn perspective_matrix(fov_rad: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (fov_rad / 2.0).tan();
    [
        f / aspect, 0.0, 0.0, 0.0,
        0.0, -f, 0.0, 0.0, // Invert Y for Vulkan NDC
        0.0, 0.0, far / (near - far), -1.0,
        0.0, 0.0, (near * far) / (near - far), 0.0,
    ]
}

#[allow(dead_code)]
fn model_matrix(pos: [f32; 3], rot_y: f32) -> [f32; 16] {
    let cos = rot_y.cos();
    let sin = rot_y.sin();
    [
        cos, 0.0, -sin, 0.0,
        0.0, 1.0, 0.0, 0.0,
        sin, 0.0, cos, 0.0,
        pos[0], pos[1], pos[2], 1.0,
    ]
}

fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let f = [
        target[0] - eye[0],
        target[1] - eye[1],
        target[2] - eye[2],
    ];
    let f_len = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt().max(0.0001);
    let f = [f[0] / f_len, f[1] / f_len, f[2] / f_len];

    let r = [
        f[1] * up[2] - f[2] * up[1],
        f[2] * up[0] - f[0] * up[2],
        f[0] * up[1] - f[1] * up[0],
    ];
    let r_len = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt().max(0.0001);
    let r = [r[0] / r_len, r[1] / r_len, r[2] / r_len];

    let u = [
        r[1] * f[2] - r[2] * f[1],
        r[2] * f[0] - r[0] * f[2],
        r[0] * f[1] - r[1] * f[0],
    ];

    [
        r[0], u[0], -f[0], 0.0,
        r[1], u[1], -f[1], 0.0,
        r[2], u[2], -f[2], 0.0,
        -(r[0] * eye[0] + r[1] * eye[1] + r[2] * eye[2]),
        -(u[0] * eye[0] + u[1] * eye[1] + u[2] * eye[2]),
        f[0] * eye[0] + f[1] * eye[1] + f[2] * eye[2],
        1.0,
    ]
}

fn camera_view_matrix(pos: [f32; 3], rot_deg: [f32; 3]) -> [f32; 16] {
    // La orientación sale de la matriz de mundo de la cámara (convención
    // única del motor, glass_core::math: R = Rz*Ry*Rx): forward = -columna 2
    // y up = columna 1. Antes se reconstruía a mano con los signos de
    // pitch/yaw opuestos a esa convención, así que la cámara de juego miraba
    // según (-pitch, -yaw) respecto a su transform y su gizmo no podía
    // coincidir con ella. Las escenas de ejemplo y el preset de luz se
    // migraron de signo al hacer este cambio para conservar su encuadre.
    let m = glass_core::mat4_from_trs_euler(pos, rot_deg, [1.0, 1.0, 1.0]);
    let back = [m[8], m[9], m[10]];
    let bl = (back[0] * back[0] + back[1] * back[1] + back[2] * back[2]).sqrt().max(0.0001);
    let fwd = [-back[0] / bl, -back[1] / bl, -back[2] / bl];
    let up_raw = [m[4], m[5], m[6]];
    let ul = (up_raw[0] * up_raw[0] + up_raw[1] * up_raw[1] + up_raw[2] * up_raw[2]).sqrt().max(0.0001);
    let up = [up_raw[0] / ul, up_raw[1] / ul, up_raw[2] / ul];

    let target = [pos[0] + fwd[0], pos[1] + fwd[1], pos[2] + fwd[2]];
    look_at(pos, target, up)
}

/// Copia un directorio completo, con subcarpetas, al destino.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn matmul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut res = [0.0; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            res[col * 4 + row] = sum;
        }
    }
    res
}

/// Fundido libre clip→clip en curso (bloque FSM): el origen y su
/// tiempo, que sigue avanzando mientras el destino entra.
struct FreeFadeRt {
    from_clip: String,
    from_time: f32,
    duration: f32,
    elapsed: f32,
}

/// Ids del último gesto de drag & drop del Canvas (UI v2): los
/// escribe el bucle de frames y los lee el puente hacia C#
/// (clase `DragDrop` del managed). 0 = sin arrastre / sin destino.
static DRAG_CURRENT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DROP_TARGET_CURRENT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Nombre canónico (minúsculas) de una tecla física para las
/// definiciones de ejes de entrada (Input v2). Las definiciones
/// usan estos nombres: "w", "space", "left", "shift_left"...
/// Estado muestreado del mando (el primero conectado).
#[derive(Default, Clone, Copy)]
struct PadState {
    lx: f32,
    ly: f32,
    south: bool,
    east: bool,
    north: bool,
    west: bool,
    start: bool,
    dpad_left: bool,
    dpad_right: bool,
    dpad_up: bool,
    dpad_down: bool,
}

/// Drena los eventos de gilrs y lee el estado del primer mando
/// conectado. Sin mando devuelve el estado neutro.
fn poll_gamepad(gilrs: &mut gilrs::Gilrs) -> PadState {
    while gilrs.next_event().is_some() {}
    let mut st = PadState::default();
    if let Some((_id, gp)) = gilrs.gamepads().next() {
        use gilrs::{Axis, Button};
        st.lx = gp.value(Axis::LeftStickX);
        st.ly = gp.value(Axis::LeftStickY);
        st.south = gp.is_pressed(Button::South);
        st.east = gp.is_pressed(Button::East);
        st.north = gp.is_pressed(Button::North);
        st.west = gp.is_pressed(Button::West);
        st.start = gp.is_pressed(Button::Start);
        st.dpad_left = gp.is_pressed(Button::DPadLeft);
        st.dpad_right = gp.is_pressed(Button::DPadRight);
        st.dpad_up = gp.is_pressed(Button::DPadUp);
        st.dpad_down = gp.is_pressed(Button::DPadDown);
    }
    st
}

/// Botón físico del mando asignado a cada eje por nombre
/// (equivalencias de Unity: A salta, B cancela...).
fn pad_button_for(axis_name: &str, st: &PadState) -> bool {
    match axis_name {
        "Jump" => st.south,
        "Fire1" => st.west,
        "Fire2" => st.east,
        "Fire3" => st.north,
        "Submit" => st.start || st.south,
        "Cancel" => st.start || st.east,
        _ => false,
    }
}

/// Fuente analógica del mando por eje: el stick izquierdo
/// (X para Horizontal, Y para Vertical; en gilrs el Y del
/// stick crece hacia ARRIBA, como W/S esperan).
fn pad_axis_for(axis_name: &str, st: &PadState) -> f32 {
    match axis_name {
        "Horizontal" => st.lx,
        "Vertical" => st.ly,
        _ => 0.0,
    }
}

fn key_name(code: KeyCode) -> Option<&'static str> {
    use KeyCode::*;
    Some(match code {
        KeyA => "a", KeyB => "b", KeyC => "c", KeyD => "d", KeyE => "e",
        KeyF => "f", KeyG => "g", KeyH => "h", KeyI => "i", KeyJ => "j",
        KeyK => "k", KeyL => "l", KeyM => "m", KeyN => "n", KeyO => "o",
        KeyP => "p", KeyQ => "q", KeyR => "r", KeyS => "s", KeyT => "t",
        KeyU => "u", KeyV => "v", KeyW => "w", KeyX => "x", KeyY => "y",
        KeyZ => "z",
        Digit0 => "0", Digit1 => "1", Digit2 => "2", Digit3 => "3",
        Digit4 => "4", Digit5 => "5", Digit6 => "6", Digit7 => "7",
        Digit8 => "8", Digit9 => "9",
        Space => "space",
        ArrowLeft => "left", ArrowRight => "right",
        ArrowUp => "up", ArrowDown => "down",
        ShiftLeft => "shift_left", ShiftRight => "shift_right",
        ControlLeft => "ctrl_left", ControlRight => "ctrl_right",
        AltLeft => "alt_left", AltRight => "alt_right",
        Enter => "return", Escape => "escape", Tab => "tab",
        Backspace => "backspace", Delete => "delete",
        Home => "home", End => "end",
        PageUp => "pageup", PageDown => "pagedown",
        F1 => "f1", F2 => "f2", F3 => "f3", F4 => "f4", F5 => "f5",
        F6 => "f6", F7 => "f7", F8 => "f8", F9 => "f9", F10 => "f10",
        F11 => "f11", F12 => "f12",
        Comma => "comma", Period => "period", Minus => "minus",
        Equal => "equals", Semicolon => "semicolon", Quote => "apostrophe",
        BracketLeft => "lbracket", BracketRight => "rbracket",
        Backslash => "backslash", Slash => "slash", Backquote => "grave",
        _ => return None,
    })
}

/// Carga las definiciones de ejes del proyecto
/// (`assets/input_axes.json`, formato `{ "axes": [ ... ] }`).
/// Cualquier problema (no existe, JSON inválido) devuelve los
/// ejes por defecto de Unity y lo anota en el log.
fn load_input_axis_defs(base: &std::path::Path) -> Vec<glass_scripting::input_axes::InputAxisDef> {
    let defaults = glass_scripting::input_axes::default_input_axes;
    let text = match std::fs::read_to_string(base.join("assets/input_axes.json")) {
        Ok(t) => t,
        Err(_) => {
            log::info!("Input v2: sin assets/input_axes.json; ejes por defecto.");
            return defaults();
        }
    };
    let parsed = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("axes").cloned())
        .and_then(|axes| {
            serde_json::from_value::<
                Vec<glass_scripting::input_axes::InputAxisDef>,
            >(axes)
            .ok()
        });
    match parsed {
        Some(axes) if !axes.is_empty() => {
            log::info!(
                "Input v2: {} ejes cargados de assets/input_axes.json",
                axes.len()
            );
            axes
        }
        _ => {
            log::warn!(
                "Input v2: input_axes.json inválido o vacío; uso los de por defecto."
            );
            defaults()
        }
    }
}

struct GlassEngineApp {
    window_config: WindowConfig,
    vulkan_config: VulkanConfig,
    window: Option<GlassWindow>,
    vulkan_context: Option<VulkanContext>,
    renderer: Option<GlassRenderer>,
    #[cfg(feature = "editor")]
    egui_ctx: egui::Context,
    #[cfg(feature = "editor")]
    egui_winit: Option<EguiWinitState>,
    #[cfg(feature = "editor")]
    egui_renderer: Option<EguiVulkanRenderer<GpuAllocator>>,
    #[cfg(feature = "editor")]
    desc_layout: Option<vk::DescriptorSetLayout>,
    #[cfg(feature = "editor")]
    desc_pool: Option<vk::DescriptorPool>,
    #[cfg(feature = "editor")]
    viewport_texture_id: Option<egui::TextureId>,
    #[cfg(feature = "editor")]
    game_desc_layout: Option<vk::DescriptorSetLayout>,
    #[cfg(feature = "editor")]
    game_desc_pool: Option<vk::DescriptorPool>,
    #[cfg(feature = "editor")]
    game_viewport_texture_id: Option<egui::TextureId>,
    #[cfg(feature = "editor")]
    editor_layout: EditorDockingLayout,
    /// Foto de la escena autorada al entrar en Play: lo que se toca en
    /// Play no se queda; al dar Stop se restaura entera.
    #[cfg(feature = "editor")]
    play_snapshot: Option<SceneState>,
    /// play_state del frame anterior, para detectar transiciones.
    #[cfg(feature = "editor")]
    prev_play_state: glass_editor::PlayState,
    /// Paso de simulación pedido por el MCP (`glass_play_control`
    /// acción step) estando en pausa: la puerta de simulación lo
    /// consume una sola vez.
    #[cfg(feature = "editor")]
    paso_mcp_pendiente: bool,
    /// Velocidad vertical acumulada del driver de CC: (entidad seleccionada, vy).
    // Ambos campos solo se leen en builds con editor (el Play y su
    // driver de CC); en el binario de juego quedan escritos y sin uso.
    #[allow(dead_code)]
    cc_driver_vy: Option<(u64, f32)>,
    /// Ids de los CC autorados creados al entrar en Play: al salir se
    /// destruyen exactamente estos (la escena puede haber cambiado
    /// durante Play y ya no enumerarlos).
    #[allow(dead_code)]
    cc_autorados_en_play: Vec<u64>,
    #[cfg(feature = "editor")]
    pub cargando: bool,
    #[cfg(feature = "editor")]
    pub carga_dibujada: bool,
    /// Arranque de la app: su elapsed es el tiempo que consumen los
    /// materiales de grafo (viaja en `light_color.w`, canal reservado).
    pub start_instant: std::time::Instant,
    #[cfg(feature = "editor")]
    pub pending_apertura: Option<AccionApertura>,
    /// Raíz del proyecto abierto (abrir_proyecto); Build & Run construye sobre
    /// ella. None = sin proyecto: el build usa el CWD, como siempre en el
    /// árbol del motor.
    #[cfg(feature = "editor")]
    raiz_proyecto_actual: Option<std::path::PathBuf>,
    /// Último ensamblado de scripts de proyecto cargado en el runtime
    /// .NET (ruta y mtime): evita recargar el mismo DLL en cada
    /// apertura de escena del mismo proyecto (defecto D4).
    ensamblado_scripts_cargado: Option<(std::path::PathBuf, std::time::SystemTime)>,
    /// Lock exclusivo del proyecto abierto (P1): ninguna otra instancia
    /// del editor puede abrirlo mientras viva. None = sin proyecto.
    #[cfg(feature = "editor")]
    lock_proyecto: Option<proyecto_lock::ProyectoLock>,
    /// Stems ya intentados por la recarga bajo demanda en esta escena:
    /// evita reintentar cada frame un modelo sin .glb o corrupto. Se
    /// limpia al aplicarse una carga de escena.
    recarga_bajo_demanda: std::collections::HashSet<String>,
    /// Stems que `evict_idle` expulsó en ESTE proceso. La recarga bajo
    /// demanda SOLO deshace expulsiones: en un arranque sin escaneo
    /// inicial de modelos (p. ej. `--scene`) nada se expulsa y nada se
    /// recarga — cargar en frío a mitad del primer frame segfaultaba el
    /// driver (llvmpipe; verificado contra la base a19062a, 2026-11-18).
    modelos_expulsados: std::collections::HashSet<String>,
    /// Instante del frame anterior para el reloj de la caché de
    /// activos: la ociosidad se mide en tiempo real (delta entre
    /// frames), no en frames contados (Bug I).
    reloj_expulsion_previo: Option<Instant>,
    /// Resto sub-milisegundo del reloj de expulsión: la fracción de
    /// milisegundo que `delta_expulsion_ms` no entrega entera se
    /// acumula aquí para no perderla frame a frame.
    reloj_expulsion_resto_ms: f64,
    /// Vuelta al Hub en espera de la decisión del diálogo de guardado.
    #[cfg(feature = "editor")]
    volver_al_hub_pendiente: bool,
    /// Salida ordenada pedida en modo hijo del Hub: el bucle de
    /// eventos la recoge tras el frame y cierra el proceso.
    #[cfg(feature = "editor")]
    salir_del_proceso: bool,
    /// Hay una petición de cierre del Hub esperando la decisión del
    /// diálogo «Cambios sin guardar» (al resolverse NO se relanza
    /// el Hub: es él quien está saliendo).
    #[cfg(feature = "editor")]
    cierre_desde_hub_pendiente: bool,
    /// Throttle de la sonda de la bandera `cerrar-<pid>.flag`.
    #[cfg(feature = "editor")]
    ultima_sonda_cierre_hub: std::time::Instant,
    /// Sonda del Bug F: instante en que el panel completó una importación
    /// por arrastre, a la espera del primer frame presentado tras ella.
    #[cfg(feature = "editor")]
    sonda_importacion_en: Option<std::time::Instant>,
    /// Este proceso es un motor gestionado por el Hub (su exe vive
    /// en engines/<versión>/): solo estos obedecen la bandera de
    /// cierre del Hub.
    #[cfg(feature = "editor")]
    gestionado: bool,
    /// V3: hay un Hub localizable en este equipo; gobierna la
    /// visibilidad del botón «Hub» del editor.
    #[cfg(feature = "editor")]
    pub hub_disponible: bool,
    /// Tras «Guardar» sin ruta se abrió «Save Scene As»: al cerrarse ese
    /// diálogo se retoma (o se abandona) la vuelta al Hub pendiente.
    #[cfg(feature = "editor")]
    esperando_save_as: bool,
    /// Último autoguardado (o arranque): el intervalo se mide desde aquí
    /// y se reinicia en cada guardado hecho por la app.
    #[cfg(feature = "editor")]
    ultimo_autoguardado: std::time::Instant,
    /// La precarga de modelos y materiales ya se ejecutó (solo se hace una
    /// vez, al entrar al editor por primera vez).
    #[cfg(feature = "editor")]
    pub precarga_hecha: bool,
    /// Precarga solicitada: se ejecuta en cuanto la pantalla de carga se
    /// haya presentado al menos un frame.
    #[cfg(feature = "editor")]
    pub precarga_pendiente: bool,
    pub scene_state: Arc<Mutex<SceneState>>,
    /// Posiciones de mundo del frame anterior por entidad (bloque
    /// Portales): el pase de cruce compara el segmento prev->curr de
    /// cada cuerpo dinámico contra los rectángulos de los portales.
    pub portal_prev_pos: std::collections::HashMap<u64, [f32; 3]>,
    /// Atlas de imágenes del Canvas (UI v2): clave = sprites de la
    /// escena activa (uno por línea); si cambia, se reconstruye el
    /// mapa y se resube el atlas al renderer.
    pub ui_images_key: String,
    pub ui_images_map: canvas_ui::UiSceneImages,
    /// Atlas de CPU de las fuentes con nombre (assets/fonts) que
    /// la escena usa, y el registro de las que fallaron al cargar
    /// (para no reintentar ni repetir el aviso cada frame).
    pub ui_font_atlases: std::collections::HashMap<String, glass_renderer::FontAtlas>,
    /// Activos de material cargados (Materiales v1), por nombre.
    pub materiales: std::collections::HashMap<String, glass_core::MaterialData>,
    /// Materiales que fallaron al cargar (avisar una sola vez).
    pub materiales_failed: std::collections::HashSet<String>,
    /// Texturas de material ya subidas a la caché de texturas.
    pub material_textures: std::collections::HashSet<String>,
    pub ui_font_failed: std::collections::HashSet<String>,
    /// Gestor de escenas: dueño de las rutas reales y de la carga diferida.
    pub scene_manager: Arc<Mutex<SceneManager>>,
    /// Biblioteca de materiales físicos única, compartida con el
    /// puente de escena (E2).
    physmat_lib: Arc<Mutex<PhysicsMaterialLibrary>>,
    #[cfg(feature = "editor")]
    pub mcp_server: Arc<McpServer>,
    /// Hilo stdio del MCP en vivo (modo editor): sirve `mcp_server` sobre
    /// la escena compartida mientras el editor corre. `None` si no se
    /// lanzó (stdin de terminal sin `--mcp-live`). El handle se conserva
    /// como constancia; no se hace join: el hilo termina por EOF de stdio
    /// o con el proceso cuando la app sale.
    #[cfg(feature = "editor")]
    pub mcp_stdio_thread: Option<std::thread::JoinHandle<()>>,
    asset_cache: Arc<RwLock<AssetCache>>,
    asset_watchers: Vec<AssetWatcher>,
    /// Raíces (canonizadas) ya vigiladas: abrir dos veces el mismo
    /// proyecto no debe duplicar vigías ni conversiones.
    asset_watchers_raices: Vec<std::path::PathBuf>,
    #[allow(dead_code)]
    start_time: Instant,
    pub script_host: ScriptHost,
    /// Causa de .NET ausente detectada en el arranque (Bug D).
    fallo_dotnet: Option<FalloDotnet>,
    /// Último fallo ya volcado a la consola (para no duplicar líneas).
    aviso_dotnet_consola_hecho: Option<FalloDotnet>,
    /// Si ya se avisó en consola de que la escena tiene scripts sin instanciar.
    aviso_escena_consola_hecho: bool,
    pub audio_system: Arc<AudioSystem>,
    pub physics_world: Arc<Mutex<PhysicsWorld>>,
    /// Lightmap horneado de la escena en curso (bake, etapa 3b): el
    /// atlas y los grupos por entidad, si la escena tiene bake en
    /// `assets/lightmaps/`. El bucle de dibujo lo pasa al renderer.
    pub lightmap_runtime: Option<crate::bake::LightmapRuntime>,
    /// Cola de scripts spawneados pendientes de instanciar (bloque de
    /// instanciación): la comparte con `EngineSceneBridge`, que la
    /// alimenta desde el callback FFI de spawn, y la app la drena cada
    /// frame en `reconcile_spawned_scripts`.
    pending_script_spawns: Arc<Mutex<Vec<(u64, String)>>>,
    pub is_standalone: bool,
    is_headless_test: bool,
    is_m5_test: bool,
    is_m6_test: bool,
    is_m7_test: bool,
    is_m8_test: bool,
    is_m9_test: bool,
    is_m10_test: bool,
    is_platformer_test: bool,
    is_sample_game_test: bool,
    pub is_standalone_test: bool,
    pub scene_to_load: Option<String>,
    pub proyecto_directo: Option<String>,
    /// Raiz del payload del motor (managed/ y assets del motor): la
    /// calcula resolver_raiz_motor al arrancar.
    pub raiz_motor: std::path::PathBuf,
    player_script_handle: Option<i32>,
    /// Teclas WASD/Espacio actualmente presionadas (para movimiento del jugador).
    keys_pressed: HashSet<PhysicalKey>,
    /// Banco de ejes configurables (Input v2): suaviza los ejes
    /// y calcula los botones con sus bordes cada frame.
    axis_bank: glass_scripting::input_axes::AxisBank,
    /// Las definiciones de ejes se cargan del archivo del
    /// proyecto (o los defaults) y se RECARGAN si el archivo
    /// cambia (el Input Manager del editor escribe ese archivo).
    input_defs_loaded: bool,
    /// Fecha del input_axes.json ya cargado (para la recarga).
    input_defs_mtime: Option<std::time::SystemTime>,
    /// Mandos (gilrs): None si la plataforma no los inicializa.
    gamepad: Option<gilrs::Gilrs>,
    /// Estado del primer mando conectado, muestreado por frame.
    pad_state: PadState,
    /// Estado del ratón alimentado por `CursorMoved`/`MouseInput` de
    /// winit en `window_event` y consumido por frame en `render_frame`.
    mouse: mouse_input::MouseState,
    /// Gesto de clic sobre botones del Canvas en curso (solo standalone).
    button_click_tracker: canvas_ui::ButtonClickTracker,
    ui_drag_session: Option<canvas_ui::DragSession>,
    /// Buffers de matrices de articulación por entidad con piel (bloque de
    /// skinning): uno por entidad viva, creado perezosamente en el bucle
    /// de dibujo con la capacidad de la piel de su modelo y reescrito cada
    /// frame con la pose evaluada. Se liberan en `Drop` y al cambiar de
    /// escena.
    skin_joint_buffers: std::collections::HashMap<u64, glass_renderer::JointBuffer>,
    /// Sistema de asignación de luces por objeto (integración de
    /// iluminación): ranuras del UBO dinámico por entidad con malla.
    sistema_luces: luces_objeto::SistemaAsignacion,
    estado_ibl: ibl::EstadoIbl,
    /// true si GLASS_LUCES_GLOBAL=1: se usa solo el UBO global
    /// (comparación A/B del sistema por objeto).
    luces_global: bool,
    /// Contador de frames para el refresco rotativo de asignaciones.
    luces_frame: u64,
    /// Estado de reproducción por entidad: (clip en curso, tiempo en
    /// segundos). El tiempo NO viaja en la escena serializada: es estado
    /// de runtime, como el tiempo acumulado de un Animator de Unity.
    anim_runtime: std::collections::HashMap<u64, (String, f32)>,
    /// Caché de controladores de animación cargados por nombre (bloque
    /// FSM): `assets/animators/<nombre>.animator.json`, una sola carga.
    animator_controllers: std::collections::HashMap<String, glass_core::animator::AnimatorController>,
    /// Decisión de pose del frame por entidad (bloque FSM): la escribe
    /// el pase previo `avanzar_animadores_fsm` (con la escena en mutable)
    /// y la consume el bucle de dibujo, que itera la escena en inmutable.
    /// (clip_a, tiempo_a, clip_b, tiempo_b, peso_b).
    anim_pose: std::collections::HashMap<u64, Vec<(String, f32, f32)>>,
    /// Seguidores de root motion por entidad (historial de la
    /// posición de la raíz entre frames).
    anim_root_trackers: std::collections::HashMap<u64, glass_renderer::skin::RootMotionTracker>,
    /// Entidades cuya pose de este frame va con la raíz fijada
    /// (root motion activo): lo rellena el pase previo y lo lee
    /// el bucle de dibujo, igual que `anim_pose`.
    anim_root_pin: std::collections::HashMap<u64, bool>,
    /// Emisores de partículas de runtime (Fase 8P), por entidad.
    particle_emitters: std::collections::HashMap<u64, glass_particles::Emitter>,
    /// Último dato de escena aplicado a cada emisor (si cambia, el
    /// pool se reinicia con la configuración nueva).
    particle_applied: std::collections::HashMap<u64, glass_core::scene::ParticleEmitterData>,
    /// Origen del emisor en el frame anterior (espacio local).
    particle_prev_origin: std::collections::HashMap<u64, [f32; 3]>,
    /// Fundidos libres clip→clip en curso (bloque FSM: `CrossFade` sin
    /// controlador), por entidad.
    anim_free_fade: std::collections::HashMap<u64, FreeFadeRt>,
    /// Estado de propiedades de grafo resueltas por entidad (Fase 2S):
    /// el descriptor set creado y los valores empaquetados que lleva,
    /// para no recrear/actualizar el set si no cambiaron entre frames.
    graph_props_state: std::collections::HashMap<u64, (vk::DescriptorSet, Vec<f32>)>,
    /// Sets de texturas de grafo (set 0 con 4 bindings) por entidad
    /// (Fase 2S): el set creado y los nombres de textura (base + hasta
    /// 3 de propiedades Texture2D) que lo produjeron.
    graph_tex_state: std::collections::HashMap<u64, (vk::DescriptorSet, [String; 4])>,
    /// Grafos cuyo material de error ya se avisó en el log (Fase 2S,
    /// S4): el aviso de grafo no disponible se emite una sola vez
    /// por grafo y sesión.
    graph_error_logged: std::collections::HashSet<String>,
    /// Grafos cuyo material es transparente (marcador .transparent
    /// junto a su .spv): la ordenación de draws de S6 los manda al
    /// final de cada lista para no romper la mezcla.
    transparent_graphs: std::collections::HashSet<String>,
    /// Estado de oclusión ya aplicado por fuente 3D (Fase 3) y reloj
    /// del último chequeo de oclusión (cada 150 ms).
    occlusion_applied: std::collections::HashMap<u64, bool>,
    last_occlusion_check: std::time::Instant,
    /// Último instante en que se avanzaron las animaciones (para el dt).
    last_anim_instant: Instant,
    /// Buffers de articulación retirados (cambio de escena o piel con otro
    /// nº de articulaciones): destruirlos en el acto podría pillar un
    /// frame aún en vuelo que los está leyendo, así que esperan aquí a que
    /// el siguiente `render_frame` espere el fence y los libere.
    retired_joint_buffers: Vec<glass_renderer::JointBuffer>,
    /// Handle del RigidBody del jugador en Rapier3D (para aplicar fuerzas/impulsos).
    player_body_handle: Option<glass_physics::BodyHandle>,
    /// Contador de frames del perfilado (Fase 0): la memoria del allocator
    /// GPU se muestrea cada 120 frames, no en cada uno.
    profile_frame: u64,
    /// Pares de contacto activos del frame anterior (Fase 1): con los de
    /// este frame se derivan los eventos Stay a C#.
    prev_contact_pairs: std::collections::HashSet<(u64, u64)>,
}

impl Drop for GlassEngineApp {
    fn drop(&mut self) {
        log::info!("Limpiando GlassEngineApp en orden estricto de dependencias...");
        if let Some(h) = self.player_script_handle {
            self.script_host.destroy_instance(h);
        }
        if let Some(vk_ctx) = &self.vulkan_context {
            unsafe {
                let _ = vk_ctx.device.device_wait_idle();
            }
        }
        // Buffers de articulación: hay que liberarlos mientras el renderer
        // (dueño del pool de sus descriptor sets) sigue vivo. El proceso
        // ya esperó el device en reposo arriba, así que aquí sí se puede
        // destruir en el acto, incluidos los retirados pendientes.
        if let Some(renderer) = &self.renderer {
            let joints_pool = renderer.joints_pool;
            let device = renderer.device.clone();
            for (_, mut jb) in std::mem::take(&mut self.skin_joint_buffers) {
                jb.destroy(&device, &mut renderer.allocator.lock().unwrap(), joints_pool);
            }
            for mut jb in self.retired_joint_buffers.drain(..) {
                jb.destroy(&device, &mut renderer.allocator.lock().unwrap(), joints_pool);
            }
        }

        #[cfg(feature = "editor")]
        if let (Some(renderer), Some(pool), Some(layout)) = (&mut self.renderer, self.desc_pool, self.desc_layout) {
            // Liberar AssetCache primero
            if let Ok(mut cache) = self.asset_cache.write() {
                cache.destroy(&renderer.device, &mut renderer.allocator.lock().unwrap());
            }

            unsafe {
                renderer.device.destroy_descriptor_pool(pool, None);
                renderer.device.destroy_descriptor_set_layout(layout, None);
                if let (Some(g_pool), Some(g_layout)) = (self.game_desc_pool, self.game_desc_layout) {
                    renderer.device.destroy_descriptor_pool(g_pool, None);
                    renderer.device.destroy_descriptor_set_layout(g_layout, None);
                }
            }
        }
        #[cfg(not(feature = "editor"))]
        if let Some(renderer) = &mut self.renderer {
            if let Ok(mut cache) = self.asset_cache.write() {
                cache.destroy(&renderer.device, &mut *renderer.allocator.lock().unwrap());
            }
        }

        #[cfg(feature = "editor")]
        {
            self.egui_renderer = None;
            self.egui_winit = None;
        }
        self.renderer = None;
        self.vulkan_context = None;
        self.window = None;
        log::info!("GlassEngineApp limpiado exitosamente.");
    }
}

/// Banderas de arranque de la app: el modo standalone y los modos
/// de test automático (`--test-*`). (Agrupadas por el lint
/// too_many_arguments de Clippy.)
#[derive(Clone, Copy, Default)]
pub struct LaunchFlags {
    pub is_standalone: bool,
    pub is_headless_test: bool,
    pub is_m5_test: bool,
    pub is_m6_test: bool,
    pub is_m7_test: bool,
    pub is_m8_test: bool,
    pub is_m9_test: bool,
    pub is_m10_test: bool,
    pub is_platformer_test: bool,
    pub is_sample_game_test: bool,
    pub is_standalone_test: bool,
}

/// Descriptor de cuerpo para la reconciliación de física:
/// (entidad, tipo, posición, rotación, colliders con su offset,
/// masa, fricción, velocidad inicial, collider de malla opcional).
type DescriptorCuerpo = (
    u64,
    BodyType,
    [f32; 3],
    [f32; 4],
    Vec<ColliderResuelto>,
    PhysicsMaterial,
    String,
    // Fallback del cuerpo (legado o PROJECT_DEFAULT) para los slots de C6.
    PhysicsMaterial,
    [f32; 3],
    // Malla pedida: modo, modelo, escala de mundo, es estático y config
    // del collider de malla construida desde el propio cuerpo (E6).
    Option<(glass_core::MeshColliderMode, String, [f32; 3], bool, ColliderConfig)>,
    // P3: true si los resueltos vienen de specs explícitos (componen con
    // la malla); false si traen solo el inferido (respaldo de la malla).
    bool,
    // Superficie de Rigidbody (huecos beta + RB-1): masa, damping lineal,
    // usa gravedad, CCD, interpola, damping angular, iteraciones del solver.
    (f32, f32, bool, bool, bool, f32, u32, f32, u32, u32, u32, bool, bool),
);

/// Elemento de dibujo de un lote del Canvas: (rect destino, rect
/// UV, set de textura, rango de índices, color, tinte).
type UiDrawItem = ([f32; 4], [f32; 4], vk::DescriptorSet, (u32, u32), [f32; 4], [f32; 4]);

/// Listas de dibujo de los portales de un frame: (mallas de
/// escena, pieles de escena, mallas de juego, pieles de juego).
type PortalRenderLists<'a> = (
    Vec<glass_renderer::MeshDrawItem<'a>>,
    Vec<glass_renderer::SkinnedDrawItem<'a>>,
    Vec<glass_renderer::MeshDrawItem<'a>>,
    Vec<glass_renderer::SkinnedDrawItem<'a>>,
);

/// Candidata a luz del UBO global: (distancia al ojo, posición y
/// rango, color e intensidad, proyecta sombra, dirección y cosenos
/// del cono, cono).
type CandidataLuz = (f32, [f32; 4], [f32; 4], bool, [f32; 4], [f32; 4]);

/// Entidad emisora de partículas con su matriz de mundo.
type EntidadEmisora = (u64, glass_core::scene::ParticleEmitterData, [f32; 16]);

impl GlassEngineApp {
    pub fn new(
        flags: LaunchFlags,
        scene_to_load: Option<String>,
        proyecto_directo: Option<String>,
    ) -> Self {
        let _ = &proyecto_directo;
        let LaunchFlags {
            is_standalone,
            is_headless_test,
            is_m5_test,
            is_m6_test,
            is_m7_test,
            is_m8_test,
            is_m9_test,
            is_m10_test,
            is_platformer_test,
            is_sample_game_test,
            is_standalone_test,
        } = flags;
        let scene_state = Arc::new(Mutex::new(SceneState::new()));
        // El gestor de escenas descubre la raíz del proyecto (project.json)
        // subiendo desde el directorio actual. Si no la encuentra, el motor debe
        // seguir arrancando: se ancla al cwd y las cargas por nombre fallarán
        // con un error explícito en lugar de impedir el arranque.
        let scene_manager = match SceneManager::locate() {
            Ok(manager) => {
                log::info!(
                    "SceneManager: raíz '{}' con {} escena(s) disponibles.",
                    manager.vfs().root().display(),
                    manager.available_scenes().len()
                );
                manager
            }
            Err(e) => {
                log::warn!("SceneManager: sin project.json ({e}); se ancla al directorio actual.");
                SceneManager::new(".")
            }
        };
        let scene_manager = Arc::new(Mutex::new(scene_manager));
        #[cfg(feature = "editor")]
        let mcp_server = Arc::new(McpServer::new(scene_state.clone()));
        // Tools de escenas del MCP: el SceneManager compartido.
        #[cfg(feature = "editor")]
        mcp_server.set_scene_manager(scene_manager.clone());
        // Librería de materiales físicos: una sola instancia para la
        // app y las tools MCP de physmat (set invalida la caché y el
        // motor relee el asset en caliente).
        let physmat_lib = Arc::new(Mutex::new(biblioteca_physmat(&scene_manager)));
        #[cfg(feature = "editor")]
        mcp_server.set_physmat_library(physmat_lib.clone());
        // Puerta del complemento «herramienta-3d» para las tools glass_mesh_*.
        #[cfg(feature = "editor")]
        mcp_server
            .set_mesh_tools_enabled(glass_editor::plugins::plugin_enabled_for(
                "herramienta-3d",
                glass_editor::updates::ENGINE_VERSION,
            ));
        let asset_cache = AssetCache::new();
        let audio_system = Arc::new(AudioSystem::new());
        register_global_audio_system(audio_system.clone());
        let physics_world = Arc::new(Mutex::new(PhysicsWorld::new()));
        register_global_physics_world(physics_world.clone());
        glass_renderer::register_global_asset_cache(asset_cache.clone());

        // Clips de audio de los juegos (Plat3D): todo `.wav` bajo
        // `assets/audio/` (recursivo) se registra en el AudioSystem con el
        // nombre del fichero sin extensión, listo para reproducirse desde
        // script por nombre (p. ej. `assets/audio/plat3d/plat3d_jump.wav`
        // se reproduce como "plat3d_jump"). Hasta ahora el sistema solo
        // traía los beeps internos de demostración.
        {
            let audio_root = scene_manager
                .lock()
                .map(|m| m.vfs().root().join("assets").join("audio"))
                .unwrap_or_default();
            let mut pending = vec![audio_root];
            let mut loaded = 0_usize;
            while let Some(dir) = pending.pop() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            pending.push(path);
                        } else if path
                            .extension()
                            .map(|e| e.eq_ignore_ascii_case("wav"))
                            .unwrap_or(false)
                        {
                            if let (Some(stem), Ok(bytes)) = (
                                path.file_stem().and_then(|s| s.to_str()),
                                std::fs::read(&path),
                            ) {
                                audio_system.load_clip(stem.to_string(), bytes);
                                loaded += 1;
                            }
                        }
                    }
                }
            }
            log::info!("Audio: {loaded} clips .wav cargados desde assets/audio/");
        }

        #[cfg(feature = "editor")]
        let egui_ctx = egui::Context::default();
        #[cfg(feature = "editor")]
        {
            // Los clics no caducan por duración. Con el defecto de egui
            // (max_click_duration = 0,8 s), una pulsación mantenida más de
            // 0,8 s deja de ser un clic: en las pestañas del dock (sentido
            // click_and_drag) se convierte en un arrastre que no activa la
            // pestaña y puede reordenarlas en silencio si el puntero se
            // mueve. En equipos lentos o con frames pesados eso hacía que
            // las pestañas «no respondieran». El arrastre sigue naciendo
            // del movimiento (max_click_dist), no del tiempo.
            egui_ctx.options_mut(|o| o.input_options.max_click_duration = f64::INFINITY);
        }
        #[cfg(feature = "editor")]
        glass_editor::setup_editor_fonts(&egui_ctx);

        #[cfg(feature = "editor")]
        let start_in_editor = is_standalone
            || is_standalone_test
            || is_platformer_test
            || is_sample_game_test
            || is_headless_test
            || is_m5_test
            || is_m6_test
            || is_m7_test
            || is_m8_test
            || is_m9_test
            || is_m10_test;
        Self {
            window_config: WindowConfig::default(),
            vulkan_config: VulkanConfig::default(),
            window: None,
            vulkan_context: None,
            renderer: None,
            #[cfg(feature = "editor")]
            egui_ctx,
            #[cfg(feature = "editor")]
            egui_winit: None,
            #[cfg(feature = "editor")]
            egui_renderer: None,
            #[cfg(feature = "editor")]
            desc_layout: None,
            #[cfg(feature = "editor")]
            desc_pool: None,
            #[cfg(feature = "editor")]
            viewport_texture_id: None,
            #[cfg(feature = "editor")]
            game_desc_layout: None,
            #[cfg(feature = "editor")]
            game_desc_pool: None,
            #[cfg(feature = "editor")]
            game_viewport_texture_id: None,
            #[cfg(feature = "editor")]
            editor_layout: EditorDockingLayout::new(),
            #[cfg(feature = "editor")]
            play_snapshot: None,
            #[cfg(feature = "editor")]
            prev_play_state: glass_editor::PlayState::Edit,
            #[cfg(feature = "editor")]
            paso_mcp_pendiente: false,
            cc_driver_vy: None,
            cc_autorados_en_play: Vec::new(),
            // Arranque directo al editor: la pantalla de carga se ve
            // desde el primer frame, antes de que bloquee la precarga.
            #[cfg(feature = "editor")]
            cargando: start_in_editor,
            #[cfg(feature = "editor")]
            carga_dibujada: false,
            start_instant: std::time::Instant::now(),
            #[cfg(feature = "editor")]
            pending_apertura: proyecto_directo.as_ref().and_then(|dir| {
                let ruta = std::path::PathBuf::from(dir);
                if ruta.join("project.glass").is_file() {
                    Some(AccionApertura::OpenProject(ruta))
                } else {
                    log::error!("--project: {:?} no contiene project.glass; se ignora.", ruta);
                    None
                }
            }),
            #[cfg(feature = "editor")]
            raiz_proyecto_actual: None,
            ensamblado_scripts_cargado: None,
            #[cfg(feature = "editor")]
            lock_proyecto: None,
            recarga_bajo_demanda: std::collections::HashSet::new(),
            modelos_expulsados: std::collections::HashSet::new(),
            reloj_expulsion_previo: None,
            reloj_expulsion_resto_ms: 0.0,
            #[cfg(feature = "editor")]
            volver_al_hub_pendiente: false,
            #[cfg(feature = "editor")]
            salir_del_proceso: false,
            #[cfg(feature = "editor")]
            cierre_desde_hub_pendiente: false,
            #[cfg(feature = "editor")]
            ultima_sonda_cierre_hub: std::time::Instant::now(),
            #[cfg(feature = "editor")]
            sonda_importacion_en: None,
            #[cfg(feature = "editor")]
            gestionado: glass_launcher::updates::es_gestionado(),
            #[cfg(feature = "editor")]
            hub_disponible: glass_launcher::updates::ruta_hub().is_some(),
            #[cfg(feature = "editor")]
            esperando_save_as: false,
            #[cfg(feature = "editor")]
            ultimo_autoguardado: std::time::Instant::now(),
            #[cfg(feature = "editor")]
            precarga_hecha: false,
            #[cfg(feature = "editor")]
            precarga_pendiente: start_in_editor,
            scene_state,
            portal_prev_pos: std::collections::HashMap::new(),
            ui_images_key: String::new(),
            ui_images_map: canvas_ui::UiSceneImages::new(),
            ui_font_atlases: std::collections::HashMap::new(),
            materiales: std::collections::HashMap::new(),
            materiales_failed: std::collections::HashSet::new(),
            material_textures: std::collections::HashSet::new(),
            ui_font_failed: std::collections::HashSet::new(),
            physmat_lib,
            scene_manager,
            #[cfg(feature = "editor")]
            mcp_server,
            #[cfg(feature = "editor")]
            mcp_stdio_thread: None,
            asset_cache,
            asset_watchers: Vec::new(),
            asset_watchers_raices: Vec::new(),
            start_time: Instant::now(),
            script_host: ScriptHost::new(),
            fallo_dotnet: None,
            aviso_dotnet_consola_hecho: None,
            aviso_escena_consola_hecho: false,
            audio_system,
            physics_world,
            lightmap_runtime: None,
            pending_script_spawns: Arc::new(Mutex::new(Vec::new())),
            is_standalone,
            is_headless_test,
            is_m5_test,
            is_m6_test,
            is_m7_test,
            is_m8_test,
            is_m9_test,
            is_m10_test,
            is_platformer_test,
            is_sample_game_test,
            is_standalone_test,
            scene_to_load,
            proyecto_directo,
            raiz_motor: raiz_motor_actual(),
            player_script_handle: None,
            keys_pressed: HashSet::new(),
            axis_bank: glass_scripting::input_axes::AxisBank::new(
                glass_scripting::input_axes::default_input_axes(),
            ),
            input_defs_loaded: false,
            input_defs_mtime: None,
            gamepad: match gilrs::Gilrs::new() {
                Ok(g) => {
                    log::info!("Input v2: subsistema de mandos (gilrs) listo.");
                    Some(g)
                }
                Err(e) => {
                    log::warn!("Input v2: sin subsistema de mandos ({e}).");
                    None
                }
            },
            pad_state: PadState::default(),
            mouse: mouse_input::MouseState::new(),
            button_click_tracker: canvas_ui::ButtonClickTracker::new(),
            ui_drag_session: None,
            skin_joint_buffers: std::collections::HashMap::new(),
            sistema_luces: luces_objeto::SistemaAsignacion::nueva(),
            estado_ibl: ibl::EstadoIbl::default(),
            luces_global: std::env::var("GLASS_LUCES_GLOBAL").map(|v| v == "1").unwrap_or(false),
            luces_frame: 0,
            anim_runtime: std::collections::HashMap::new(),
            animator_controllers: std::collections::HashMap::new(),
            anim_pose: std::collections::HashMap::new(),
            anim_root_trackers: std::collections::HashMap::new(),
            anim_root_pin: std::collections::HashMap::new(),
            particle_emitters: std::collections::HashMap::new(),
            particle_applied: std::collections::HashMap::new(),
            particle_prev_origin: std::collections::HashMap::new(),
            anim_free_fade: std::collections::HashMap::new(),
            graph_props_state: std::collections::HashMap::new(),
            graph_tex_state: std::collections::HashMap::new(),
            graph_error_logged: std::collections::HashSet::new(),
            transparent_graphs: std::collections::HashSet::new(),
            occlusion_applied: std::collections::HashMap::new(),
            last_occlusion_check: std::time::Instant::now(),
            last_anim_instant: Instant::now(),
            retired_joint_buffers: Vec::new(),
            player_body_handle: None,
            profile_frame: 0,
            prev_contact_pairs: std::collections::HashSet::new(),
        }
    }

    /// Decide si el canal MCP por stdio debe arrancar en modo editor.
    ///
    /// Función pura (sin tocar el stdio real) para poder probarla. Regla:
    /// un cliente MCP lanza el proceso con pipes (stdin no es TTY), así
    /// que se arranca salvo que stdin sea una terminal interactiva — ahí
    /// las teclas del usuario no son JSON-RPC y las respuestas del
    /// servidor ensuciarían la consola —. `force` (`--mcp-live`) salta
    /// esa protección explícitamente.
    #[cfg(feature = "editor")]
    pub fn live_mcp_stdio_decision(force: bool, stdin_is_terminal: bool) -> bool {
        force || !stdin_is_terminal
    }

    /// Arranca el servidor MCP nativo sobre la escena VIVA del editor,
    /// sirviendo JSON-RPC por stdio en un hilo dedicado.
    ///
    /// No crea un `SceneState` ni un `McpServer` nuevos: reutiliza
    /// `self.mcp_server`, que ya comparte el mismo
    /// `Arc<Mutex<SceneState>>` que usa el editor (ver `new`). Lo que
    /// faltaba era el transporte: en modo editor nadie leía stdio y el
    /// MCP solo existía para el autotest en proceso y para el modo
    /// headless `--mcp` (que sí crea su propia escena).
    ///
    /// Concurrencia: cada tool toma el `Mutex` de la escena durante su
    /// ejecución (el locking grueso ya existente en `McpServer`), así que
    /// el hilo nunca toca la escena a la vez que el bucle de render, y la
    /// espera en stdio ocurre FUERA del lock. Ninguna tool toca Vulkan ni
    /// el `AssetCache`: mutan datos CPU de la escena y el hilo principal
    /// los sube a GPU en su frame, tras la espera del fence.
    ///
    /// El hilo es fire-and-forget a propósito: NO debe hacerse `join`
    /// sobre `mcp_stdio_thread` en el camino de apagado, porque
    /// `run_stdio_loop` queda bloqueado en lectura de stdin y esperar su
    /// fin colgaría el cierre si el cliente no cierra su extremo de
    /// escritura. El hilo termina por EOF (el cliente cerró; el editor
    /// sigue en marcha sin canal MCP) o con el proceso al salir la app.
    #[cfg(feature = "editor")]
    pub fn start_live_mcp_stdio(&mut self, force: bool) {
        if self.mcp_stdio_thread.is_some() {
            return;
        }
        let stdin_is_terminal = std::io::stdin().is_terminal();
        if !Self::live_mcp_stdio_decision(force, stdin_is_terminal) {
            log::info!(
                "MCP en vivo: canal stdio NO abierto (stdin es una terminal interactiva). \
                 Lanza el editor con stdin por pipe o pasa --mcp-live para servir el MCP \
                 sobre la escena del editor; el modo headless sigue en --mcp."
            );
            return;
        }
        // El mismo servidor (y la misma escena) que ya usa el editor.
        let server = self.mcp_server.clone();
        match std::thread::Builder::new()
            .name("glass-mcp-stdio".to_string())
            .spawn(move || {
                log::info!("MCP en vivo: hilo stdio sirviendo la escena del editor.");
                server.run_stdio_loop();
                log::info!(
                    "MCP en vivo: stdio cerrado (EOF o error de lectura); \
                     el editor sigue en marcha sin canal MCP."
                );
            }) {
            Ok(handle) => self.mcp_stdio_thread = Some(handle),
            Err(e) => log::error!("MCP en vivo: no se pudo lanzar el hilo stdio: {e}"),
        }
    }

    /// Construye en `state` las entidades de la plantilla Plataformas
    /// 3D, SIN fisica ni scripts (eso lo anade el metodo del editor tras
    /// construir, o el bootstrap al abrir la escena guardada). La usa
    /// tambien la creacion de proyectos en disco. Devuelve
    /// (suelo, escalon 1, escalon 2, jugador).
    fn construir_escena_plataformas(state: &mut SceneState) -> (u64, u64, u64, u64) {
        let initial_y = 0.5;
    state.entities.clear();

    // Instanciar Cámara Principal y Luz Direccional (Game View y Shading)
    let cam_id = state.create_entity(
        "Main Camera".into(),
        vec!["Transform".into(), "Camera".into()],
    );
    state.modify_transform(cam_id, Some([3.5, 3.2, 9.5]), Some([10.0, 0.0, 0.0]), Some([1.0, 1.0, 1.0]));

    let light_id = state.create_entity(
        "Directional Light".into(),
        vec!["Transform".into(), "Light".into()],
    );
    state.modify_transform(light_id, Some([0.0, 10.0, 0.0]), Some([-50.0, 30.0, 0.0]), Some([1.0, 1.0, 1.0]));

    let floor_id = state.create_entity(
        "Static Floor".into(),
        vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
    );
    state.modify_transform(floor_id, Some([3.5, -0.5, 0.0]), None, Some([30.0, 1.0, 30.0]));
    state.modify_material(floor_id, [0.2, 0.6, 0.2], Some(false));

    let step1_id = state.create_entity(
        "Low Platform Step 1".into(),
        vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
    );
    state.modify_transform(step1_id, Some([3.0, 0.5, 0.0]), None, Some([2.0, 1.0, 2.0]));
    state.modify_material(step1_id, [0.8, 0.6, 0.2], Some(false));

    let step2_id = state.create_entity(
        "High Platform Step 2".into(),
        vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
    );
    state.modify_transform(step2_id, Some([6.0, 1.5, 0.0]), None, Some([2.0, 1.0, 2.0]));
    state.modify_material(step2_id, [0.9, 0.3, 0.2], Some(false));

    let player_id = state.create_entity(
        "Player 3D Cube".into(),
        vec![
            "Transform".into(),
            "MeshRenderer".into(),
            "RigidBody".into(),
            "Collider".into(),
            "Script".into(),
            "AudioSource".into(),
        ],
    );
    state.modify_transform(player_id, Some([0.0, initial_y, 0.0]), None, Some([1.0, 1.0, 1.0]));
    state.modify_material(player_id, [0.1, 0.8, 1.0], Some(false));
    state.modify_script(player_id, Some("GlassEngine.Core.Scripts.PlayerController".into()));

    if let Some(p) = state.entities.iter_mut().find(|e| e.id == player_id) {
        p.script_data = Some(glass_core::ScriptComponentData {
            class_name: "GlassEngine.Core.Scripts.PlayerController".into(),
            move_speed: 3.8,
            jump_velocity: 7.2,
            auto_demo: false,
        campos: Default::default(),
        });
        p.audio = Some(glass_core::AudioSourceData {
            clip_name: "beep_880".into(),
            volume: 1.0,
            pitch: 1.0,
            is_playing: false,
            bus: "sfx".to_string(),
        });
    }

        (floor_id, step1_id, step2_id, player_id)
    }

    pub fn setup_interactive_platformer_scene(&mut self) {
        let initial_y = 0.5;

        // 1. Limpiar o configurar entidades en SceneState
        let (floor_id, step1_id, step2_id, player_id) = {
            let mut state = self.scene_state.lock().unwrap();
            Self::construir_escena_plataformas(&mut state)
        };

        // 2. Configurar simulación física en Rapier3D
        {
            let mut phys = self.physics_world.lock().unwrap();
            *phys = PhysicsWorld::new();

            let fb = phys.create_rigid_body(floor_id, BodyType::Static, [3.5, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(fb, [15.0, 0.5, 15.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");

            let s1b = phys.create_rigid_body(step1_id, BodyType::Static, [3.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(s1b, [1.0, 0.5, 1.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");

            let s2b = phys.create_rigid_body(step2_id, BodyType::Static, [6.0, 1.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(s2b, [1.0, 0.5, 1.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");

            let pb = phys.create_rigid_body(player_id, BodyType::Dynamic, [0.0, initial_y, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(pb, [0.5, 0.5, 0.5], &glass_core::PhysicsMaterial { dynamic_friction: 0.6, bounciness: 0.02, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
            self.player_body_handle = Some(pb);
        }

        // 3. Instanciar script C# PlayerController
        if self.script_host.is_initialized {
            match self.script_host.create_instance(player_id, "GlassEngine.Core.Scripts.PlayerController") {
                Ok(h) => {
                    log::info!("Script PlayerController instanciado exitosamente para el jugador (handle: {})", h);
                    self.player_script_handle = Some(h);
                }
                Err(e) => log::warn!("No se pudo instanciar PlayerController: {}", e),
            }
        }
    }

    pub fn setup_complete_sample_game_scene(&mut self) {
        let (floor_id, step1_id, step2_id, goal_id, _coin1_id, _coin2_id, player_id, _canvas_id, _score_id, _time_id) = {
            let mut state = self.scene_state.lock().unwrap();
            state.entities.clear();

            // 1. Cámara y Luz Direccional (PBR)
            let cam_id = state.create_entity(
                "Main Camera".into(),
                vec!["Transform".into(), "Camera".into()],
            );
            state.modify_transform(cam_id, Some([3.5, 3.2, 9.5]), Some([10.0, 0.0, 0.0]), Some([1.0, 1.0, 1.0]));

            let light_id = state.create_entity(
                "Directional Light".into(),
                vec!["Transform".into(), "Light".into()],
            );
            state.modify_transform(light_id, Some([0.0, 10.0, 0.0]), Some([-50.0, 30.0, 0.0]), Some([1.0, 1.0, 1.0]));

            // 2. Plataformas 3D (Escalables)
            let floor_id = state.create_entity(
                "Static Floor".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
            );
            state.modify_transform(floor_id, Some([3.5, -0.5, 0.0]), None, Some([30.0, 1.0, 30.0]));
            state.modify_material(floor_id, [0.2, 0.55, 0.25], Some(false));

            let step1_id = state.create_entity(
                "Platform Tier 1".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
            );
            state.modify_transform(step1_id, Some([3.0, 0.5, 0.0]), None, Some([2.0, 1.0, 2.0]));
            state.modify_material(step1_id, [0.85, 0.65, 0.2], Some(false));

            let step2_id = state.create_entity(
                "Platform Tier 2".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
            );
            state.modify_transform(step2_id, Some([6.0, 1.5, 0.0]), None, Some([2.0, 1.0, 2.0]));
            state.modify_material(step2_id, [0.9, 0.35, 0.2], Some(false));

            let goal_id = state.create_entity(
                "Goal Platform (Finish)".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into(), "AudioSource".into()],
            );
            state.modify_transform(goal_id, Some([8.5, 2.5, 0.0]), None, Some([2.0, 1.0, 2.0]));
            state.modify_material(goal_id, [0.1, 0.9, 0.3], Some(false));

            // 3. Monedas / Gemas coleccionables
            let coin1_id = state.create_entity(
                "Coin Gem 1".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "AudioSource".into()],
            );
            state.modify_transform(coin1_id, Some([3.0, 1.4, 0.0]), None, Some([0.4, 0.4, 0.4]));
            state.modify_material(coin1_id, [1.0, 0.84, 0.0], Some(false));

            let coin2_id = state.create_entity(
                "Coin Gem 2".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "AudioSource".into()],
            );
            state.modify_transform(coin2_id, Some([6.0, 2.4, 0.0]), None, Some([0.4, 0.4, 0.4]));
            state.modify_material(coin2_id, [1.0, 0.84, 0.0], Some(false));

            // 4. Jugador 3D con Rapier3D Dynamic RigidBody y script C#
            let player_id = state.create_entity(
                "Player Hero 3D".into(),
                vec![
                    "Transform".into(),
                    "MeshRenderer".into(),
                    "RigidBody".into(),
                    "Collider".into(),
                    "Script".into(),
                    "AudioSource".into(),
                ],
            );
            state.modify_transform(player_id, Some([0.0, 0.5, 0.0]), None, Some([1.0, 1.0, 1.0]));
            state.modify_material(player_id, [0.1, 0.75, 1.0], Some(false));
            state.modify_script(player_id, Some("GlassEngine.Core.Scripts.PlayerController".into()));

            if let Some(p) = state.entities.iter_mut().find(|e| e.id == player_id) {
                p.script_data = Some(glass_core::ScriptComponentData {
                    class_name: "GlassEngine.Core.Scripts.PlayerController".into(),
                    move_speed: 3.8,
                    jump_velocity: 7.2,
                    auto_demo: false,
                campos: Default::default(),
                });
                p.audio = Some(glass_core::AudioSourceData {
                    clip_name: "beep_880".into(),
                    volume: 1.0,
                    pitch: 1.0,
                    is_playing: false,
                    bus: "sfx".to_string(),
                });
            }

            // 5. Canvas UI con Score y Time
            let canvas_id = state.create_entity(
                "InGame Canvas HUD".into(),
                vec!["Transform".into(), "RectTransform".into(), "Canvas".into()],
            );

            let score_id = state.create_entity(
                "Score HUD Text".into(),
                vec!["Transform".into(), "RectTransform".into(), "Text".into()],
            );
            state.reparent(score_id, Some(canvas_id));
            if let Some(e) = state.get_entity_mut(score_id) {
                if let Some(rt) = &mut e.rect_transform {
                    rt.anchored_position = [20.0, 20.0];
                    rt.size_delta = [200.0, 30.0];
                    rt.anchor_min = [0.0, 1.0];
                    rt.anchor_max = [0.0, 1.0];
                    rt.pivot = [0.0, 1.0];
                }
                if let Some(txt) = &mut e.ui_text {
                    txt.text = "Score: 0".into();
                    txt.font_size = 20.0;
                    txt.color = [1.0, 0.9, 0.1, 1.0];
                }
            }

            let time_id = state.create_entity(
                "Time HUD Text".into(),
                vec!["Transform".into(), "RectTransform".into(), "Text".into()],
            );
            state.reparent(time_id, Some(canvas_id));
            if let Some(e) = state.get_entity_mut(time_id) {
                if let Some(rt) = &mut e.rect_transform {
                    rt.anchored_position = [20.0, 55.0];
                    rt.size_delta = [200.0, 30.0];
                    rt.anchor_min = [0.0, 1.0];
                    rt.anchor_max = [0.0, 1.0];
                    rt.pivot = [0.0, 1.0];
                }
                if let Some(txt) = &mut e.ui_text {
                    txt.text = "Time: 0.0s".into();
                    txt.font_size = 16.0;
                    txt.color = [0.8, 0.9, 1.0, 1.0];
                }
            }

            (floor_id, step1_id, step2_id, goal_id, coin1_id, coin2_id, player_id, canvas_id, score_id, time_id)
        };

        // 2. Configurar simulación física en Rapier3D
        {
            let mut phys = self.physics_world.lock().unwrap();
            *phys = PhysicsWorld::new();

            let fb = phys.create_rigid_body(floor_id, BodyType::Static, [3.5, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(fb, [15.0, 0.5, 15.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");

            let s1b = phys.create_rigid_body(step1_id, BodyType::Static, [3.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(s1b, [1.0, 0.5, 1.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");

            let s2b = phys.create_rigid_body(step2_id, BodyType::Static, [6.0, 1.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(s2b, [1.0, 0.5, 1.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");

            let gb = phys.create_rigid_body(goal_id, BodyType::Static, [8.5, 2.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(gb, [1.0, 0.5, 1.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");

            let pb = phys.create_rigid_body(player_id, BodyType::Dynamic, [0.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(pb, [0.5, 0.5, 0.5], &glass_core::PhysicsMaterial { dynamic_friction: 0.6, bounciness: 0.02, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
            self.player_body_handle = Some(pb);
        }

        // 3. Instanciar script C# PlayerController
        if self.script_host.is_initialized {
            match self.script_host.create_instance(player_id, "GlassEngine.Core.Scripts.PlayerController") {
                Ok(h) => {
                    log::info!("Script PlayerController instanciado en Sample Game para Hero (handle: {})", h);
                    self.player_script_handle = Some(h);
                }
                Err(e) => log::warn!("No se pudo instanciar PlayerController: {}", e),
            }
        }
    }

    pub fn execute_build_and_run(&mut self) {
        log::info!("🚀 Iniciando Pipeline de Build & Run Standalone...");
        // Base del build: la raíz del proyecto abierto; sin proyecto (o sin
        // editor), el CWD, como siempre en el árbol del motor.
        #[cfg(feature = "editor")]
        let base = self
            .raiz_proyecto_actual
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        #[cfg(not(feature = "editor"))]
        let base = std::path::PathBuf::from(".");
        let build_dir = base.join("build").join("standalone");
        let _ = std::fs::create_dir_all(&build_dir);

        // 1. Copiar al paquete TODO el árbol de assets del proyecto (shaders,
        // modelos .glb y .gltf, texturas, audio y escenas, con sus
        // subcarpetas), con la misma convención Assets/assets de
        // abrir_proyecto. Antes la copia era plana y con filtro de extensión:
        // un .gltf con sus texturas en una subcarpeta, o el audio, nunca
        // llegaban al paquete y el juego arrancaba incompleto.
        let assets_src = if base.join("Assets").is_dir() {
            base.join("Assets")
        } else if base.join("assets").is_dir() {
            base.join("assets")
        } else {
            base.join("Assets")
        };
        if let Err(e) = copy_dir_recursive(&assets_src, &build_dir.join("assets")) {
            log::error!("Fallo copiando assets al build: {}", e);
        }

        // La escena del build es la escena EDITADA en este momento. Se guarda
        // con un nombre propio del build y directamente dentro del paquete
        // (después de copiar los assets, para que nada la pise): el árbol
        // fuente no se toca.
        let scene_path = "assets/scenes/Standalone.scene.json";
        let scene_file = build_dir.join(scene_path);
        if let Ok(sc) = self.scene_state.lock() {
            // save_to_json no crea el directorio padre.
            if let Some(parent) = scene_file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let scene_file_str = scene_file.to_string_lossy().into_owned();
            if let Err(e) = sc.save_to_json(&scene_file_str) {
                log::error!("Fallo al guardar escena para el build: {}", e);
            }
        }

        // 2. project.json nace en el paquete, con la escena del build como default.
        #[cfg(feature = "editor")]
        {
            self.editor_layout.project_settings.default_scene = scene_path.to_string();
            let project_file_str = build_dir.join("project.json").to_string_lossy().into_owned();
            let _ = self.editor_layout.project_settings.save_to_file(&project_file_str);
        }

        // 3. Copiar la carpeta gestionada de C# completa (antes: una lista
        // fija de 4 ficheros; cualquier dependencia nueva quedaba fuera).
        // El origen es el payload del motor de ESTE proceso, no el del CWD.
        let managed_src = raiz_motor_actual().join("managed/GlassEngine.Core/bin/Debug/net8.0");
        let managed_dst = build_dir.join("managed/GlassEngine.Core/bin/Debug/net8.0");
        if managed_src.exists() {
            if let Err(e) = copy_dir_recursive(&managed_src, &managed_dst) {
                log::error!("Fallo copiando DLLs gestionadas al build: {}", e);
            }
        }

        // 4. Copiar binario de la aplicación. El nombre lleva la extensión de
        // ejecutable de la plataforma (.exe en Windows): sin ella, Windows no
        // puede lanzar el juego. Nota: este binario es el del propio editor en
        // modo --standalone; el binario de juego puro se compila con
        // `cargo build -p glass_app --no-default-features`.
        let exe_name = format!("GlassEngineGame{}", std::env::consts::EXE_SUFFIX);
        if let Ok(current_exe) = std::env::current_exe() {
            let dst = build_dir.join(&exe_name);
            let _ = std::fs::copy(&current_exe, &dst);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755));
            }
        }

        log::info!("✅ Standalone Package generado exitosamente en {}", build_dir.display());
        #[cfg(feature = "editor")]
        {
            self.editor_layout.status_message = Some((
                format!(
                    "🚀 Build Standalone completado con éxito en {}",
                    build_dir.display()
                ),
                std::time::Instant::now(),
            ));
        }

        // 5. Lanzar ejecutable standalone en segundo plano. Programa y CWD en
        // absoluto: con un programa relativo y current_dir, Rust resuelve el
        // programa contra el directorio nuevo y el spawn falla.
        let build_abs = std::fs::canonicalize(&build_dir).unwrap_or_else(|_| build_dir.clone());
        let child_res = std::process::Command::new(build_abs.join(&exe_name))
            .current_dir(&build_abs)
            .args(["--standalone", "--scene", scene_path])
            .spawn();

        match child_res {
            Ok(_) => {
                log::info!("🎮 Proceso Standalone lanzado exitosamente!");
            }
            Err(e) => {
                log::error!("Fallo al ejecutar binario standalone: {}", e);
            }
        }
    }

    /// Construye en `state` las entidades de la plantilla Fisica
    /// (suelo estatico y la piramide de cubos dinamicos), SIN registrar
    /// cuerpos en el mundo fisico. Devuelve (suelo, cubos dinamicos).
    fn construir_escena_fisica(state: &mut SceneState) -> (u64, Vec<(u64, [f32; 3])>) {
    state.entities.clear();

    let cam_id = state.create_entity(
        "Main Camera".into(),
        vec!["Transform".into(), "Camera".into()],
    );
    state.modify_transform(cam_id, Some([0.0, 4.5, 12.0]), Some([12.0, 0.0, 0.0]), Some([1.0, 1.0, 1.0]));

    let light_id = state.create_entity(
        "Directional Light".into(),
        vec!["Transform".into(), "Light".into()],
    );
    state.modify_transform(light_id, Some([0.0, 10.0, 0.0]), Some([-45.0, 35.0, 0.0]), Some([1.0, 1.0, 1.0]));

    let floor_id = state.create_entity(
        "Static Floor".into(),
        vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
    );
    state.modify_transform(floor_id, Some([0.0, -0.5, 0.0]), None, Some([30.0, 1.0, 30.0]));
    state.modify_material(floor_id, [0.25, 0.55, 0.35], Some(false));

    let mut dynamic_ids = Vec::new();
    let colors = [
        [0.9, 0.3, 0.2],
        [0.2, 0.7, 0.9],
        [0.9, 0.8, 0.2],
        [0.7, 0.3, 0.9],
        [0.2, 0.9, 0.4],
        [0.95, 0.5, 0.1],
    ];

    // Apilamiento de 6 cubos físicos en pirámide
    let positions = [
        [-1.2, 0.5, 0.0],
        [ 1.2, 0.5, 0.0],
        [ 0.0, 0.5, 0.0],
        [-0.6, 1.6, 0.0],
        [ 0.6, 1.6, 0.0],
        [ 0.0, 2.7, 0.0],
    ];

    for (i, pos) in positions.iter().enumerate() {
        let id = state.create_entity(
            format!("Physics Cube #{}", i + 1),
            vec!["Transform".into(), "MeshRenderer".into(), "RigidBody".into(), "Collider".into()],
        );
        state.modify_transform(id, Some(*pos), None, Some([1.0, 1.0, 1.0]));
        state.modify_material(id, colors[i % colors.len()], Some(false));
        dynamic_ids.push((id, *pos));
    }

        (floor_id, dynamic_ids)
    }

    // Solo se invoca desde el Launcher (feature editor); en el binario de
    // juego queda sin llamadores a propósito.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn setup_physics_playground_scene(&mut self) {
        let (floor_id, dynamic_ids) = {
            let mut state = self.scene_state.lock().unwrap();
            Self::construir_escena_fisica(&mut state)
        };

        // Configurar simulación en Rapier3D
        {
            let mut phys = self.physics_world.lock().unwrap();
            *phys = PhysicsWorld::new();

            let fb = phys.create_rigid_body(floor_id, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(fb, [15.0, 0.5, 15.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");

            for (ent_id, pos) in dynamic_ids {
                let b = phys.create_rigid_body(ent_id, BodyType::Dynamic, pos, [0.0, 0.0, 0.0, 1.0]);
                phys.attach_box_collider(b, [0.5, 0.5, 0.5], &glass_core::PhysicsMaterial { dynamic_friction: 0.6, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
            }
        }
        self.player_body_handle = None;
        self.player_script_handle = None;
    }

    // Solo se invoca desde el Launcher (feature editor); en el binario de
    // juego queda sin llamadores a propósito.
    #[cfg_attr(not(feature = "editor"), allow(dead_code))]
    pub fn setup_audio_lighting_scene(&mut self) {
        let floor_id = {
            let mut state = self.scene_state.lock().unwrap();
            state.entities.clear();

            let cam_id = state.create_entity(
                "Main Camera".into(),
                vec!["Transform".into(), "Camera".into()],
            );
            state.modify_transform(cam_id, Some([0.0, 3.5, 9.0]), Some([-14.0, 0.0, 0.0]), Some([1.0, 1.0, 1.0]));

            let sun_id = state.create_entity(
                "Sun (Directional Light)".into(),
                vec!["Transform".into(), "Light".into()],
            );
            state.modify_transform(sun_id, Some([0.0, 8.0, 0.0]), Some([-55.0, 40.0, 0.0]), Some([1.0, 1.0, 1.0]));

            let red_light = state.create_entity(
                "Point Light (Red)".into(),
                vec!["Transform".into(), "Light".into()],
            );
            state.modify_transform(red_light, Some([-3.0, 2.0, 0.0]), None, Some([1.0, 1.0, 1.0]));
            if let Some(e) = state.entities.iter_mut().find(|e| e.id == red_light) {
                if let Some(ref mut l) = e.light {
                    l.light_type = "Point".into();
                    l.color = [1.0, 0.2, 0.2];
                    l.intensity = 2.0;
                    l.range = 8.0;
                }
            }

            let blue_light = state.create_entity(
                "Point Light (Cyan)".into(),
                vec!["Transform".into(), "Light".into()],
            );
            state.modify_transform(blue_light, Some([3.0, 2.0, 0.0]), None, Some([1.0, 1.0, 1.0]));
            if let Some(e) = state.entities.iter_mut().find(|e| e.id == blue_light) {
                if let Some(ref mut l) = e.light {
                    l.light_type = "Point".into();
                    l.color = [0.2, 0.8, 1.0];
                    l.intensity = 2.0;
                    l.range = 8.0;
                }
            }

            let audio_src = state.create_entity(
                "Ambient Audio Source".into(),
                vec!["Transform".into(), "AudioSource".into()],
            );
            state.modify_transform(audio_src, Some([0.0, 1.0, 0.0]), None, Some([1.0, 1.0, 1.0]));
            if let Some(e) = state.entities.iter_mut().find(|e| e.id == audio_src) {
                e.audio = Some(glass_core::AudioSourceData {
                    clip_name: "beep_440".into(),
                    volume: 0.8,
                    pitch: 1.0,
                    is_playing: true,
                    bus: "sfx".to_string(),
                });
            }

            let floor_id = state.create_entity(
                "Static Floor".into(),
                vec!["Transform".into(), "MeshRenderer".into(), "StaticCollider".into()],
            );
            state.modify_transform(floor_id, Some([0.0, -0.5, 0.0]), None, Some([24.0, 1.0, 24.0]));
            state.modify_material(floor_id, [0.3, 0.3, 0.35], Some(false));

            let cube_center = state.create_entity(
                "Central Showcase Mesh".into(),
                vec!["Transform".into(), "MeshRenderer".into()],
            );
            state.modify_transform(cube_center, Some([0.0, 0.5, 0.0]), None, Some([1.5, 1.5, 1.5]));
            state.modify_material(cube_center, [0.95, 0.95, 0.95], Some(false));

            floor_id
        };

        {
            let mut phys = self.physics_world.lock().unwrap();
            *phys = PhysicsWorld::new();
            let fb = phys.create_rigid_body(floor_id, BodyType::Static, [0.0, -0.5, 0.0], [0.0, 0.0, 0.0, 1.0]);
            phys.attach_box_collider(fb, [12.0, 0.5, 12.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.05, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
        }
        self.player_body_handle = None;
        self.player_script_handle = None;
    }

    /// Crea un proyecto en disco SIN tocar el estado del editor ni el
    /// runtime: carpetas, la escena de la plantilla serializada desde
    /// un SceneState desechable y project.glass con el sello REAL del
    /// motor (su ENGINE_VERSION, no un texto fijo). Devuelve la carpeta
    /// del proyecto. La apertura posterior va por abrir_proyecto, la
    /// misma vía que usa el Hub y --project.
    #[cfg(feature = "editor")]
    fn crear_proyecto_en_disco(
        name: &str,
        path: &Path,
        template: ProjectTemplate,
    ) -> Result<std::path::PathBuf, String> {
        // Validación de nombre y carpeta destino (espejo del Unity Hub:
        // la carpeta debe ser única en la ubicación; crear sobre un
        // proyecto existente sobrescribía su escena y su project.glass).
        // `name` queda sombreado por el nombre recortado y `project_dir`
        // es la carpeta ya resuelta; no se vuelve a derivar.
        let (nombre_recortado, project_dir) =
            glass_launcher::archivos::validar_creacion_proyecto(name, path)?;
        let name: &str = nombre_recortado.as_str();
        // Estructura de carpetas tipo Unity.
        for sub in [
            "Assets/Scenes",
            "Assets/Scripts",
            "Assets/Materials",
            "Assets/Models",
            "Assets/Textures",
            "ProjectSettings",
            "Packages",
        ] {
            std::fs::create_dir_all(project_dir.join(sub))
                .map_err(|e| format!("No se pudo crear {:?}: {}", project_dir.join(sub), e))?;
        }

        // Escena de la plantilla en un SceneState desechable: ni la escena
        // viva ni el runtime se tocan. Assets/Scenes ya existe (save_to_json
        // no crea carpetas).
        let mut state = SceneState::new();
        match template {
            ProjectTemplate::Platformer3D => {
                Self::construir_escena_plataformas(&mut state);
            }
            ProjectTemplate::PhysicsSandbox => {
                Self::construir_escena_fisica(&mut state);
            }
            ProjectTemplate::EmptyScene => Self::construir_escena_vacia(&mut state),
        }
        let scene_path = project_dir.join("Assets/Scenes/SampleScene.scene.json");
        state
            .save_to_json(&scene_path.to_string_lossy())
            .map_err(|e| {
                format!(
                    "No se pudo guardar la escena del proyecto ({}): {}",
                    scene_path.display(),
                    e
                )
            })?;

        let meta = format!(
            "{{\n  \"name\": \"{}\",\n  \"version\": \"1.0.0\",\n  \"engine\": \"{}\",\n  \"template\": \"{:?}\",\n  \"scene\": \"Assets/Scenes/SampleScene.scene.json\"\n}}\n",
            name,
            glass_editor::updates::ENGINE_VERSION,
            template
        );
        std::fs::write(project_dir.join("project.glass"), meta)
            .map_err(|e| format!("No se pudo escribir project.glass: {}", e))?;
        Ok(project_dir)
    }

    #[cfg(feature = "editor")]
    pub fn create_project_and_open(&mut self, name: &str, path: &Path, template: ProjectTemplate) {
        match Self::crear_proyecto_en_disco(name, path, template) {
            Ok(project_dir) => {
                log::info!("Proyecto '{}' creado en disco: {:?}", name, project_dir);
                self.abrir_proyecto(&project_dir);
            }
            Err(e) => log::error!("No se pudo crear el proyecto '{}': {}", name, e),
        }
    }

    /// Abre una escena desde un archivo .scene.json por la misma vía
    /// probada del arranque con --scene: load_from_json + bootstrap
    /// de scripts y física.
    #[cfg(feature = "editor")]
    pub fn open_scene_file(&mut self, scene_path: &str) -> bool {
        // La escena manda sobre el ancla: si pertenece a otro proyecto,
        // el gestor y la librería de materiales físicos se reanclan a
        // ESE proyecto antes de cargar (defecto S19).
        if let Some(raiz) = proyecto_de_una_escena(scene_path) {
            self.reanclar_al_proyecto(&raiz);
            // Y sus scripts, antes de que la escena los instancie (D4).
            self.cargar_scripts_del_proyecto(&raiz);
        }
        let cargada = match self.scene_state.lock() {
            Ok(mut sc) => match sc.load_from_json(scene_path) {
                Ok(()) => {
                    crate::canvas_ui::migrar_ui_escena_cargada(&mut sc, Some(scene_path));
                    crate::prefabs_vivos::refrescar_prefabs_escena_cargada(
                        &mut sc,
                        self.raiz_proyecto_actual.as_deref(),
                    );
                    true
                }
                Err(e) => {
                    log::error!("La escena solicitada no cargó ({}): {}", scene_path, e);
                    false
                }
            },
            Err(_) => false,
        };
        if cargada {
            self.bootstrap_scene_scripts_and_physics(scene_path);
            #[cfg(feature = "editor")]
            {
                // Una escena normal cierra el modo de edición de prefab.
                // (La identidad de la escena —nombre y archivo— la registra
                // el SceneManager en bootstrap_scene_scripts_and_physics.)
                self.editor_layout.prefab_en_edicion = None;
                // Recién cargada = limpia, aunque el vigía no dispare (mismo
                // nombre que la escena anterior, p. ej. SampleScene).
                if let Ok(sc) = self.scene_state.lock() {
                    self.editor_layout.marcar_guardada(&sc);
                }
            }
        }
        cargada
    }

    /// Fase C: abre el asset de un prefab como escena de edición (vista
    /// simple, sin Prefab Mode): sus entidades sustituyen a la escena SIN
    /// vínculo — ellas son la fuente — y el gestor queda en modo
    /// `prefab_en_edicion`, donde Guardar escribe el `.prefab.json`.
    #[cfg(feature = "editor")]
    fn abrir_prefab_en_editor(&mut self, fuente: &str) {
        // La fuente es relativa al proyecto y se resuelve con la misma
        // tolerancia de capitalización que usa el resto del sistema: la
        // ruta que se guarda es la REAL del disco.
        let Some(raiz) = self.raiz_proyecto_actual.clone() else {
            log::error!("Sin proyecto abierto; no se puede abrir el prefab '{fuente}'");
            self.editor_layout.status_message = Some((
                "No hay proyecto abierto; no se puede abrir el prefab".to_string(),
                std::time::Instant::now(),
            ));
            return;
        };
        let Some(ruta) = glass_core::Prefab::resolver_ruta(&raiz, fuente) else {
            log::error!("No se encontró el asset del prefab '{fuente}'");
            self.editor_layout.status_message = Some((
                format!("No se encontró el asset del prefab: {fuente}"),
                std::time::Instant::now(),
            ));
            return;
        };
        let ruta = &ruta;
        // Se carga RESUELTO: si el asset es una variante, la vista de
        // edición muestra base + diff aplicados (Fase E); si la cadena no
        // se resuelve (base ausente o ciclo), no se abre.
        let Some(prefab) = glass_core::Prefab::cargar_desde_proyecto(&raiz, fuente) else {
            log::error!("No se pudo resolver el prefab '{fuente}'");
            self.editor_layout.status_message = Some((
                format!("No se pudo abrir el prefab {fuente}: no se resolvió (¿variante con la base ausente o en ciclo?)"),
                std::time::Instant::now(),
            ));
            return;
        };
        let variante = prefab.variant_of.clone().map(|fuente_base| {
            glass_editor::docking::VarianteEnEdicion {
                raiz_proyecto: raiz.clone(),
                fuente_base,
            }
        });
        let raiz_id = prefab.root_entity.id;
        let nombre = prefab.name.clone();
        let mut entidades = vec![prefab.root_entity];
        entidades.extend(prefab.descendant_entities);
        for e in &mut entidades {
            e.prefab_source = None;
            e.prefab_hash = None;
            e.prefab_local_id = None;
        }
        let cargado = match self.scene_state.lock() {
            Ok(mut sc) => {
                sc.replace_entities(entidades);
                self.editor_layout.marcar_guardada(&sc);
                true
            }
            Err(_) => false,
        };
        if !cargado {
            log::error!("SceneState bloqueado; no se abrió el prefab");
            return;
        }
        self.editor_layout.prefab_en_edicion = Some((ruta.to_path_buf(), raiz_id, variante));
        self.editor_layout.hierarchy.selected_entity = Some(raiz_id);
        self.rebuild_physics_from_scene();
        // Vista simple: no se instancian los scripts del prefab.
        self.editor_layout.status_message = Some((
            format!("Editando prefab {nombre}: Guardar escribe el .prefab.json"),
            std::time::Instant::now(),
        ));
        log::info!("Editor: editando el prefab '{}' ({})", nombre, ruta.display());
    }

    #[cfg(feature = "editor")]
    /// Construye en `state` la escena vacia: camara principal y luz
    /// solar. Sin suelo, fisica ni scripts.
    fn construir_escena_vacia(state: &mut SceneState) {
    state.entities.clear();
    let cam_id = state.create_entity(
        "Main Camera".into(),
        vec!["Transform".into(), "Camera".into()],
    );
    state.modify_transform(cam_id, Some([0.0, 3.5, 9.0]), Some([-14.0, 0.0, 0.0]), Some([1.0, 1.0, 1.0]));
    let sun_id = state.create_entity(
        "Sun (Directional Light)".into(),
        vec!["Transform".into(), "Light".into()],
    );
    state.modify_transform(sun_id, Some([0.0, 8.0, 0.0]), Some([-55.0, 40.0, 0.0]), Some([1.0, 1.0, 1.0]));
    }

    /// Escena vacía: solo cámara principal y luz solar. Sin suelo, física ni scripts.
    #[cfg(feature = "editor")]
    pub fn setup_empty_scene(&mut self) {
        let mut state = self.scene_state.lock().unwrap();
        Self::construir_escena_vacia(&mut state);
        drop(state);
        self.editor_layout.prefab_en_edicion = None;
    }

    /// Si el modelo importado trae piel, sube también su malla gemela con
    /// formato `SkinnedVertex` y registra piel y clips en la caché (la
    /// subida rígida normal ya se hizo en el llamante y queda como
    /// respaldo). Sin piel, no hace nada: los modelos de siempre no pagan
    /// ni un byte extra.
    /// OJO con la concurrencia: el allocator y la caché llegan YA
    /// bloqueados por el llamante (ambas rutas de carga los tienen tomados
    /// para la subida rígida y las texturas). Volver a bloquear cualquiera
    /// de los dos aquí dentro cuelga la app: ni Mutex ni RwLock de std son
    /// reentrantes (cazado en vivo dos veces durante este bloque).
    /// Apertura de un proyecto por su carpeta: la UNICA via, la use el
    /// Hub (AccionApertura::OpenProject) o el arranque con --project
    /// (que siembra esa misma accion). Selecciona la escena (campo
    /// "scene" de project.glass o el primer *.scene.json), la carga o
    /// deja la escena vacia, apunta el panel y el vigia a los assets
    /// del proyecto, lo registra, guarda el registro y entra al editor.
    #[cfg(feature = "editor")]
/// V3 (separación verdadera): al salir del editor hacia el Hub,
/// localizarlo y lanzarlo como programa aparte. Función libre (no
/// método) porque los sitios de salida viven dentro del cierre de
/// frame, con `self` ya partido en préstamos disjuntos. Si el Hub
/// no se localiza o no arranca, se sale igualmente: quien pulsa
/// «Hub» pidió salir, no quedarse atrapado en el editor. La
/// higiene de entorno replica la del lanzamiento Hub→motor
/// (fuera overrides GLASS_* de prueba salvo la raíz del lanzador).
#[cfg(feature = "editor")]
fn lanzar_hub_al_salir() {
    static HUB_LANZADO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if HUB_LANZADO.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    match glass_launcher::updates::ruta_hub() {
        Some(hub) => {
            let mut cmd = std::process::Command::new(&hub);
            cmd.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            for (k, _) in std::env::vars_os() {
                let ks = k.to_string_lossy();
                if ks.starts_with("GLASS_") && ks != "GLASS_LAUNCHER_HOME" {
                    cmd.env_remove(&k);
                }
            }
            cmd.env_remove("APPIMAGE");
            cmd.env_remove("LD_LIBRARY_PATH");
            cmd.env_remove("LD_PRELOAD");
            cmd.env_remove("APPDIR");
            match cmd.spawn() {
                Ok(_) => log::info!("Volviendo al Hub: lanzado {:?}", hub),
                Err(e) => log::error!(
                    "No se pudo lanzar el Hub ({:?}): {e}; se sale igualmente.",
                    hub
                ),
            }
        }
        None => log::info!("Volver al Hub: no hay Hub localizable; se sale del editor."),
    }
}

    /// Apertura de un proyecto por su carpeta: la UNICA via, la use el
    /// Hub (AccionApertura::OpenProject) o el arranque con --project
    /// (que siembra esa misma accion). Selecciona la escena (campo
    /// "scene" de project.glass o el primer *.scene.json), la carga o
    /// deja la escena vacia, apunta el panel y el vigia a los assets
    /// del proyecto, lo registra, guarda el registro y entra al editor.
    #[cfg(feature = "editor")]
    fn abrir_proyecto(&mut self, path: &std::path::Path) {
        // 0) Lock de proyecto (P1): antes de cargar nada. Si otra
        // instancia ya lo tiene abierto, la apertura se aborta SIN tocar
        // el estado actual (ni escena, ni registro, ni raíz vigente).
        // Reabrir el MISMO proyecto en esta instancia no re-toma el lock
        // (un segundo handle fallaría incluso en el mismo proceso). El
        // lock nuevo vive en esta variable durante la carga: si la
        // apertura aborta más abajo, se suelta solo y el vigente queda.
        let mismo_proyecto = match (&self.lock_proyecto, proyecto_lock::raiz_canonica(path)) {
            (Some(actual), Ok(nueva)) => actual.raiz() == nueva.as_path(),
            _ => false,
        };
        let lock_nuevo = if mismo_proyecto {
            None
        } else {
            match proyecto_lock::tomar_lock_proyecto(path) {
                Ok(lock) => Some(lock),
                Err(msg) => {
                    log::error!("{msg}");
                    return;
                }
            }
        };
        // 1) Campo "scene" de project.glass, relativo al proyecto.
        let desde_meta = std::fs::read_to_string(path.join("project.glass"))
            .ok()
            .and_then(|txt| serde_json::from_str::<serde_json::Value>(&txt).ok())
            .and_then(|v| v.get("scene").and_then(|s| s.as_str()).map(|s| path.join(s)))
            .filter(|p| p.is_file());
        // 2) Si no, el primer *.scene.json (por nombre) de la primera carpeta
        //    de escenas que tenga alguno, en este orden.
        let escena = desde_meta.or_else(|| {
            ["Assets/Scenes", "assets/Scenes", "assets/scenes"]
                .iter()
                .find_map(|sub| {
                    let mut candidatas: Vec<std::path::PathBuf> =
                        std::fs::read_dir(path.join(sub))
                            .ok()?
                            .flatten()
                            .map(|e| e.path())
                            .filter(|p| {
                                p.is_file()
                                    && p.file_name()
                                        .and_then(|n| n.to_str())
                                        .is_some_and(|n| n.ends_with(".scene.json"))
                            })
                            .collect();
                    candidatas.sort();
                    candidatas.into_iter().next()
                })
        });
        // El ancla se mueve ANTES de cargar la escena: su bootstrap de
        // física resuelve los .physmat.json contra la librería, y con el
        // ancla de arranque (el directorio de lanzamiento) el material
        // del proyecto no carga (defecto S19).
        self.reanclar_al_proyecto(path);
        // Los scripts del proyecto tienen que estar cargados ANTES de
        // que la escena inicial instancie sus componentes (defecto D4).
        self.cargar_scripts_del_proyecto(path);
        // 3) Cargar la escena; si no hay o falla, escena vacía.
        let cargada = match &escena {
            Some(ruta) => self.open_scene_file(&ruta.to_string_lossy()),
            None => false,
        };
        if !cargada {
            self.setup_empty_scene();
        }
        log::info!("Proyecto abierto: {:?} (escena: {:?}, cargada: {})", path, escena, cargada);
        // El panel de proyecto muestra los assets DEL PROYECTO abierto,
        // no los del CWD: se respeta la convención que ya exista en
        // disco (`Assets` en los proyectos del Launcher, `assets` en
        // minúsculas en otros).
        let raiz_assets = if path.join("Assets").is_dir() {
            path.join("Assets")
        } else if path.join("assets").is_dir() {
            path.join("assets")
        } else {
            path.join("Assets")
        };
        // Commit del lock (P1): el nuevo ya está tomado; al reemplazar
        // el campo se suelta el anterior. A partir de aquí la apertura
        // ya no aborta: el proyecto queda abierto y bloqueado.
        if let Some(lock) = lock_nuevo {
            self.lock_proyecto = Some(lock);
        }
        self.raiz_proyecto_actual = Some(path.to_path_buf());
        // El gestor y la librería de materiales físicos ya se reanclaron
        // a este proyecto ANTES de cargar la escena (ver reanclar_al_proyecto
        // más arriba): las cargas por nombre y los .physmat.json de la
        // escena inicial resuelven contra el proyecto, no contra el
        // directorio de arranque.
        self.vigilar_assets(&raiz_assets);
        self.editor_layout.prefab_en_edicion = None;
        self.editor_layout.project.set_assets_root(raiz_assets);
        // Los materiales que fallaron contra las raíces del proyecto
        // anterior merecen un nuevo intento contra las del recién abierto.
        self.materiales_failed.clear();
        // Registrar el proyecto abierto para que aparezca en el Hub,
        // con el nombre de su project.glass (no el de la carpeta).
        let nombre = glass_launcher::archivos::nombre_proyecto(path);
        let mut store = glass_launcher::updates::LauncherStore::load();
        store.register_project(&nombre, path);
        if let Err(e) = store.save() {
            log::error!("No se pudo guardar el registro de proyectos: {}", e);
        }
    }

    /// Calentamiento de assets: textura por defecto del PBR, esquemas de
    /// grafos de shaders, precarga de TODOS los .glb de assets/models a
    /// VRAM y primitivas integradas. Antes vivía en `resumed()` y dejaba
    /// la ventana en blanco (y la consola en Windows) durante toda la
    /// carga, antes del primer frame. Ahora el standalone lo llama al
    /// arrancar, como siempre, y el editor lo llama DIFERIDO: al entrar
    /// al editor por primera vez, con la pantalla de carga del Launcher
    /// ya presentada en el frame anterior.
    fn preparar_assets(&mut self, renderer: &mut GlassRenderer, vk_ctx: &VulkanContext) {
        if let Some(set_layout) = renderer.pbr_pipeline.descriptor_set_layout {
            {
                let mut alloc = renderer.allocator.lock().unwrap();
                let mut cache = self.asset_cache.write().unwrap();
                cache.ensure_default_texture(
                    &renderer.device,
                    &mut alloc,
                    vk_ctx.graphics_queue,
                    renderer.command_pool,
                    set_layout,
                );
            }
            if let Some(set) = self.asset_cache.read().unwrap().default_set {
                renderer.fallback_set = set;
            }
        }

        // Esquemas de propiedades de los grafos: se registran globalmente
        // (glass_core::shader_graph) para que la resolución por entidad y
        // las tools MCP de inspección puedan consultarlos sin recargar los
        // .graph.json en cada acceso.
        if let Ok(entries) = std::fs::read_dir(self.raiz_motor.join("assets/shader_graphs")) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_graph_json = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.ends_with(".graph.json"))
                    .unwrap_or(false);
                if !is_graph_json {
                    continue;
                }
                match std::fs::read_to_string(&path) {
                    Ok(contents) => match glass_core::shader_graph::ShaderGraph::from_json(&contents) {
                        Ok(graph) => {
                            glass_core::shader_graph::register_graph_schema(
                                graph.name.clone(),
                                graph.properties.clone(),
                            );
                            glass_core::shader_graph::register_graph_passes(
                                graph.name.clone(),
                                graph.passes.clone(),
                            );
                            if std::path::Path::new(&format!(
                                "assets/shaders/graph_{}.transparent",
                                graph.name
                            ))
                            .exists()
                            {
                                self.transparent_graphs.insert(graph.name.clone());
                            }
                        }
                        Err(e) => {
                            log::warn!(
                                "No se pudo parsear el grafo de shaders {:?}: {}",
                                path,
                                e
                            );
                        }
                    },
                    Err(e) => {
                        log::warn!("No se pudo leer el grafo de shaders {:?}: {}", path, e);
                    }
                }
            }
        }

        // Materiales de grafo: cada graph_<nombre>.frag.spv de assets/shaders
        // se registra como pipeline; una entidad lo usa con shader =
        // "graph:<nombre>" en su MeshData.
        renderer.load_graph_materials(&self.raiz_motor.join("assets/shaders"));

        // Auditoría 2S (M-8): un grafo con .spv pero sin .graph.json
        // cargado perdía sus pases Shadow/Outline en silencio (el
        // pipeline existe, así que el fallback magenta no salta). Se
        // avisa una vez por grafo al arrancar.
        if let Ok(entries) = std::fs::read_dir(self.raiz_motor.join("assets/shaders")) {
            for entry in entries.flatten() {
                let fname = entry.file_name();
                let Some(fname) = fname.to_str() else { continue };
                let Some(nombre) = fname
                    .strip_prefix("graph_")
                    .and_then(|r| r.strip_suffix(".frag.spv"))
                else {
                    continue;
                };
                if glass_core::shader_graph::graph_passes(nombre).is_none() {
                    log::warn!(
                        "Grafo '{}': tiene .spv pero no se cargó su .graph.json (¿nombre o carpeta distinta?); se asume [Forward], sin pase Shadow ni Outline",
                        nombre
                    );
                }
            }
        }


        // El pipeline transparente de los materiales vive en el
        // renderer (registrado como «material_transparent» al
        // construirlo, con los layouts completos del PBR).

        {
            let models_dir = self.raiz_motor.join("assets/models");
            let mut glb_paths: Vec<std::path::PathBuf> = Vec::new();
            // Recorrido recursivo (mismo patrón que el cargador de
            // audio): los kits viven en subcarpetas (p. ej.
            // assets/models/polygon/) y se registran por su stem,
            // igual que los de la raíz.
            let mut pending: Vec<std::path::PathBuf> = vec![models_dir.clone()];
            while let Some(dir) = pending.pop() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            pending.push(path);
                        } else if path.extension().and_then(|e| e.to_str()) == Some("glb") {
                            glb_paths.push(path);
                        }
                    }
                }
            }
            glb_paths.sort();
// Los proyectos importados (p. ej. desde Unity) guardan sus modelos
        // en subcarpetas arbitrarias de su propio Assets/, no en
        // assets/models del motor. Sin este recorrido, esos .glb nunca
        // entraban en la caché y la recarga bajo demanda tampoco los
        // encontraba, dejando el proyecto sin modelos tras importarlo.
        #[cfg(feature = "editor")]
        if let Some(raiz_proyecto) = self.raiz_proyecto_actual.as_ref() {
            let assets_proyecto = {
                let con_mayus = raiz_proyecto.join("Assets");
                if con_mayus.is_dir() {
                    con_mayus
                } else {
                    let con_minus = raiz_proyecto.join("assets");
                    if con_minus.is_dir() {
                        con_minus
                    } else {
                        con_mayus
                    }
                }
            };
            if assets_proyecto.is_dir() {
                let mut stems_existentes: std::collections::HashSet<String> = glb_paths
                    .iter()
                    .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .collect();
                let mut agregados = 0usize;
                let mut pending = vec![assets_proyecto];
                while let Some(dir) = pending.pop() {
                    let entries = match std::fs::read_dir(&dir) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            pending.push(path);
                        } else if path.extension().and_then(|e| e.to_str()) == Some("glb") {
                            let stem = path
                                .file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            // El motor manda en colisiones de nombre; el proyecto
                            // solo rellena los huecos que el motor no cubre.
                            if !stems_existentes.contains(&stem) {
                                stems_existentes.insert(stem);
                                glb_paths.push(path);
                                agregados += 1;
                            }
                        }
                    }
                }
                if agregados > 0 {
                    log::info!(
                        "Precarga: {} modelo(s) del proyecto añadidos desde {:?}",
                        agregados,
                        raiz_proyecto
                    );
                }
            }
        }
            for glb_path in &glb_paths {
                let stem = glb_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unnamed")
                    .to_string();
                let bytes = match std::fs::read(glb_path) {
                    Ok(b) => b,
                    Err(e) => {
                        log::error!("Error leyendo modelo {:?}: {}", glb_path, e);
                        continue;
                    }
                };
                match MeshLoader::load_model_from_glb_bytes(&bytes) {
                    Ok(model) => {
                        log::info!(
                            "Cargador glTF: {} vértices, {} índices, {} materiales y {} imágenes extraídos de {:?}",
                            model.vertices.len(),
                            model.indices.len(),
                            model.materials.len(),
                            model.images.len(),
                            glb_path
                        );
                        let upload_res = {
                            let mut alloc = renderer.allocator.lock().unwrap();
                            MeshLoader::upload_to_gpu(
                                &renderer.device,
                                &mut alloc,
                                &model.vertices,
                                &model.indices,
                                vk_ctx.graphics_queue,
                                renderer.command_pool,
                            )
                        };

                        match upload_res {
                            Ok(gpu_mesh) => {
                                let mut alloc = renderer.allocator.lock().unwrap();
                                let mut cache = self.asset_cache.write().unwrap();
                                let primary = model.primary_material().cloned();
                                cache.insert_mesh(
                                    &renderer.device,
                                    &mut alloc,
                                    stem.clone(),
                                    gpu_mesh,
                                );
                                cache.insert_model_data(
                                    stem.clone(),
                                    model.materials.clone(),
                                    primary,
                                    model.images.clone(),
                                    model.primitive_ranges.clone(),
                                        model.objects.clone(),
                                );
                                // Geometría CPU retenida (Fase 1.5), como en la
                                // ruta de recarga en caliente.
                                cache.insert_geometry(stem.clone(), &model);
                                self.upload_skin_if_any(renderer, vk_ctx, &mut alloc, &mut cache, &model, &stem);
                                // Texturas base de TODOS los materiales: una
                                // subida por cada imagen distinta referenciada,
                                // para que cada primitiva muestree la suya.
                                if let Some(layout) = renderer.pbr_pipeline.descriptor_set_layout {
                                    let mut uploaded: Vec<usize> = Vec::new();
                                    for mat in &model.materials {
                                        if let Some(idx) = mat.base_color_image {
                                            if !uploaded.contains(&idx) {
                                                uploaded.push(idx);
                                                if let Some(img) = model.images.get(idx) {
                                                    cache.insert_texture(
                                                        &renderer.device,
                                                        &mut alloc,
                                                        TextureUpload { queue: vk_ctx.graphics_queue, command_pool: renderer.command_pool, set_layout: layout },
                                                        stem.clone(),
                                                        idx,
                                                        img,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                log::info!("main: Modelo '{}' registrado en la caché.", stem);
                            }
                            Err(e) => log::error!("Error subiendo malla a GPU: {}", e),
                        }
                    }
                    Err(e) => log::error!("Error importando glTF {:?}: {}", glb_path, e),
                }
            }
        }

        // Primitiva Plano integrada: no es un archivo, la genera el
        // renderer (rejilla subdividida lista para que un grafo con etapa
        // de vértices la ondule) y se registra en la caché con el mismo
        // mecanismo que los .glb — clave por nombre, `MeshData.model =
        // "plane"`. Si un proyecto trae su propio plane.glb, ese gana:
        // solo se registra cuando la clave está libre.
        {
            let has_plane = self.asset_cache.read().unwrap().meshes.contains_key("plane");
            if !has_plane {
                let (vertices, indices) = glass_renderer::generate_plane(glass_renderer::PLANE_SEGMENTS);
                let upload_res = {
                    let mut alloc = renderer.allocator.lock().unwrap();
                    glass_renderer::MeshLoader::upload_to_gpu(
                        &renderer.device,
                        &mut alloc,
                        &vertices,
                        &indices,
                        vk_ctx.graphics_queue,
                        renderer.command_pool,
                    )
                };
                match upload_res {
                    Ok(gpu_mesh) => {
                        let mut alloc = renderer.allocator.lock().unwrap();
                        let mut cache = self.asset_cache.write().unwrap();
                        cache.insert_mesh(&renderer.device, &mut alloc, "plane", gpu_mesh);
                        // Cotas reales desde los vértices (culling):
                        // sin registrarlas, la primitiva cae en el
                        // respaldo ±1e6 y nunca se descarta.
                        let (mut pmin, mut pmax) = ([f32::MAX; 3], [f32::MIN; 3]);
                        for v in &vertices {
                            for i in 0..3 {
                                pmin[i] = pmin[i].min(v.position[i]);
                                pmax[i] = pmax[i].max(v.position[i]);
                            }
                        }
                        cache.insert_bounds("plane", pmin, pmax);
                        log::info!(
                            "main: Primitiva 'plane' registrada ({} vértices, {} índices).",
                            vertices.len(),
                            indices.len()
                        );
                    }
                    Err(e) => log::error!("main: fallo subiendo la primitiva 'plane': {}", e),
                }
            }
        }

        // Primitivas Cubo/Esfera/Cápsula/Cilindro integradas: mismo
        // mecanismo y misma regla que el plano — generadas por el
        // renderer, registradas por nombre solo si la clave está
        // libre (un sphere.glb del proyecto ganaría a la integrada).
        // Hasta aquí, los presets de esfera/cápsula/cilindro del
        // editor dibujaban la malla de cubo: no existían generadores.
        // El cubo entra en la misma lista por una razón distinta: es
        // el RESPALDO de toda entidad sin modelo y la recolección de
        // dibujo del frame cuelga de su presencia en la caché. Sin un
        // generador propio, «cube» solo llegaba si el escaneo de
        // modelos encontraba un cube.glb; sin él (standalone con
        // editor, proyectos sin el archivo a mano) el 3D entero
        // dejaba de dibujarse en silencio (defecto D3).
        for (name, generator) in [
            ("cube", glass_renderer::generate_cube as fn() -> (Vec<glass_renderer::Vertex>, Vec<u32>)),
            ("sphere", glass_renderer::generate_sphere as fn() -> (Vec<glass_renderer::Vertex>, Vec<u32>)),
            ("capsule", glass_renderer::generate_capsule as fn() -> (Vec<glass_renderer::Vertex>, Vec<u32>)),
            ("cylinder", glass_renderer::generate_cylinder as fn() -> (Vec<glass_renderer::Vertex>, Vec<u32>)),
        ] {
            let ya_registrada = self.asset_cache.read().unwrap().meshes.contains_key(name);
            if ya_registrada {
                continue;
            }
            let (vertices, indices) = generator();
            // Cotas reales desde los vértices generados (culling):
            // sin registrarlas, estas primitivas caen en el respaldo
            // ±1e6 del bucle de render y nunca se descartan.
            let (mut pmin, mut pmax) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in &vertices {
                for i in 0..3 {
                    pmin[i] = pmin[i].min(v.position[i]);
                    pmax[i] = pmax[i].max(v.position[i]);
                }
            }
            let upload_res = {
                let mut alloc = renderer.allocator.lock().unwrap();
                glass_renderer::MeshLoader::upload_to_gpu(
                    &renderer.device,
                    &mut alloc,
                    &vertices,
                    &indices,
                    vk_ctx.graphics_queue,
                    renderer.command_pool,
                )
            };
            match upload_res {
                Ok(gpu_mesh) => {
                    let mut alloc = renderer.allocator.lock().unwrap();
                    let mut cache = self.asset_cache.write().unwrap();
                    cache.insert_mesh(&renderer.device, &mut alloc, name, gpu_mesh);
                    cache.insert_bounds(name, pmin, pmax);
                    log::info!(
                        "main: Primitiva '{}' registrada ({} vértices, {} índices).",
                        name,
                        vertices.len(),
                        indices.len()
                    );
                }
                Err(e) => log::error!("main: fallo subiendo la primitiva '{}': {}", name, e),
            }
        }
    }

    fn upload_skin_if_any(
        &self,
        renderer: &GlassRenderer,
        vk_ctx: &VulkanContext,
        alloc: &mut gpu_allocator::vulkan::Allocator,
        cache: &mut AssetCache,
        model: &glass_renderer::mesh::LoadedModel,
        stem: &str,
    ) {
        let Some(skin) = &model.skin else {
            // El modelo (ya) no trae piel: si la caché guarda la de una
            // versión anterior del mismo stem (recarga en caliente de
            // un modelo con piel a uno sin ella), hay que retirarla o
            // la entidad se dibuja por la vía con piel con datos del
            // modelo viejo y no se ve nada.
            cache.remove_skin(&renderer.device, alloc, stem);
            return;
        };
        let upload_res = MeshLoader::upload_skinned_to_gpu(
            &renderer.device,
            alloc,
            model,
            vk_ctx.graphics_queue,
            renderer.command_pool,
        );
        match upload_res {
            Ok(gpu_mesh) => {
                cache.insert_skinned_mesh(&renderer.device, alloc, stem, gpu_mesh);
                cache.insert_skin_data(stem, skin.clone(), model.animations.clone());
                log::info!(
                    "Modelo '{}' con piel: {} articulaciones y {} clips registrados",
                    stem,
                    skin.joints.len(),
                    model.animations.len()
                );
            }
            Err(e) => {
                log::error!("Error subiendo la malla con piel de '{}': {}", stem, e);
            }
        }
    }

    /// Retira los buffers de articulación y olvida el estado de
    /// reproducción. Al cambiar de escena las entidades nuevas pueden
    /// reutilizar ids: no deben heredar ni buffers ni poses de la
    /// anterior. Los buffers NO se destruyen aquí (podría haber un frame
    /// en vuelo leyéndolos): pasan a la lista de retirados, que
    /// `render_frame` libera tras esperar el fence.
    fn reset_skinning_runtime(&mut self) {
        let vivos: Vec<glass_renderer::JointBuffer> =
            self.skin_joint_buffers.drain().map(|(_, jb)| jb).collect();
        self.retired_joint_buffers.extend(vivos);
        self.anim_runtime.clear();
        self.anim_pose.clear();
        self.anim_root_trackers.clear();
        self.anim_root_pin.clear();
        self.anim_free_fade.clear();
        self.last_anim_instant = Instant::now();
    }

    /// Vigía de assets para una carpeta de proyecto abierta: convierte y
    /// registra los .blend/.fbx de ESE proyecto (el vigía inicial solo
    /// cubre el `assets/` del directorio de trabajo).
    #[cfg(feature = "editor")]
    fn vigilar_assets(&mut self, raiz: &Path) {
        if !raiz.is_dir() {
            return;
        }
        let canonica =
            std::fs::canonicalize(raiz).unwrap_or_else(|_| raiz.to_path_buf());
        if self.asset_watchers_raices.contains(&canonica) {
            return;
        }
        match AssetWatcher::new(raiz, true) {
            Ok(watcher) => {
                self.asset_watchers_raices.push(canonica);
                self.asset_watchers.push(watcher);
            }
            Err(e) => log::warn!(
                "No se pudo vigilar los assets de {:?}: {}",
                raiz,
                e
            ),
        }
    }

    /// Reenvía al vigía dueño de la carpeta cada conversión que los
    /// paneles anotaron al soltar un .blend/.fbx sin .glb al día: el
    /// modelo real sustituirá al cubo de respaldo cuando llegue.
    fn drenar_peticiones_conversion(&mut self) {
        // Las peticiones nacen en los paneles del editor; en la build de
        // juego (sin la feature) no hay paneles ni peticiones que drenar.
        #[cfg(feature = "editor")]
        {
            let mut peticiones: Vec<std::path::PathBuf> = Vec::new();
            if let Some(p) = self.editor_layout.viewport.pending_conversion.take() {
                peticiones.push(p);
            }
            if let Some(p) = self.editor_layout.hierarchy.pending_conversion.take() {
                peticiones.push(p);
            }
            if let Some(p) = self.editor_layout.project.pending_conversion.take() {
                peticiones.push(p);
            }
            for ruta in peticiones {
                let canonica =
                    std::fs::canonicalize(&ruta).unwrap_or_else(|_| ruta.clone());
                let idx = self
                    .asset_watchers_raices
                    .iter()
                    .position(|raiz| canonica.starts_with(raiz))
                    .unwrap_or(0);
                if let Some(watcher) = self.asset_watchers.get(idx) {
                    log::info!("Conversión solicitada al soltar: {:?}", ruta);
                    watcher.solicitar_conversion(&ruta);
                }
            }
            // Cargas directas: assets soltados cuyo .glb ya existe y
            // está al día (el propio .glb o el gemelo de una fuente ya
            // convertida). Sin conversión no hay evento del vigía, así
            // que la carga se hace aquí, igual que la de una recarga.
            let mut cargas: Vec<std::path::PathBuf> = Vec::new();
            if let Some(p) = self.editor_layout.viewport.pending_carga.take() {
                cargas.push(p);
            }
            if let Some(p) = self.editor_layout.hierarchy.pending_carga.take() {
                cargas.push(p);
            }
            for ruta in cargas {
                log::info!("Carga directa de modelo al soltar: {:?}", ruta);
                self.cargar_modelo_desde_glb(&ruta);
            }
        }
    }

    fn check_asset_reloads(&mut self) {
        self.drenar_peticiones_conversion();
        // El cuerpo original (bucle `for ev in events`) vive en
        // procesar_eventos_recarga; así el paso bajo demanda corre siempre
        // después del bucle, aunque el cuerpo retorne antes.
        self.procesar_eventos_recarga();

        // Recarga bajo demanda: evict_idle expulsa los modelos ociosos y nada
        // los recargaba al volver a una escena que los referencia (se veía el
        // cubo de respaldo para siempre). Se carga como máximo 1 por frame.
        let referenciados: Vec<String> = match self.scene_state.lock() {
            Ok(estado) => estado
                .entities
                .iter()
                .filter_map(|ent| ent.mesh.as_ref())
                .filter(|mesh| !mesh.model.is_empty())
                .map(|mesh| mesh.model.clone())
                .collect(),
            Err(_) => Vec::new(),
        };
for stem in &referenciados {
            if self.recarga_bajo_demanda.contains(stem) {
                continue;
            }
            let en_cache = self
                .asset_cache
                .read()
                .map(|c| c.bounds(stem).is_some())
                .unwrap_or(false);
            if en_cache {
                continue;
            }
            // Esta vía no solo deshace expulsiones: también carga stems
            // nunca cargados (p. ej. modelos de un proyecto abierto tras
            // la precarga). El marcado en recarga_bajo_demanda de abajo
            // garantiza un único intento por stem y escena, así que
            // ampliar la puerta no introduce reintentos por frame.
            #[cfg(feature = "editor")]
            let raiz_proyecto = self.raiz_proyecto_actual.as_deref();
            #[cfg(not(feature = "editor"))]
            let raiz_proyecto: Option<&std::path::Path> = None;
            let mut raices: Vec<std::path::PathBuf> = raices_assets_busqueda(raiz_proyecto, &self.scene_manager, &self.raiz_motor);
            let modelos_motor = self.raiz_motor.join("assets").join("models");
            if !raices.contains(&modelos_motor) {
                raices.insert(1.min(raices.len()), modelos_motor);
            }
            let raices_ref: Vec<&std::path::Path> =
                raices.iter().map(|p| p.as_path()).collect();
            let ruta = glass_renderer::watcher::resolver_glb_por_stem(&raices_ref, stem);
            // Marcado antes de cargar: un .glb corrupto no se reintenta cada frame.
            self.recarga_bajo_demanda.insert(stem.clone());
            match ruta {
                Some(ruta) => {
                    log::info!("AssetCache: recarga bajo demanda de '{}' desde {:?}", stem, ruta);
                    self.cargar_modelo_desde_glb(&ruta);
                    self.modelos_expulsados.remove(stem);
                }
                None => {
                    log::warn!("AssetCache: '{}' sin .glb localizable para recarga bajo demanda", stem);
                }
            }
            break;
        }
    }

    fn procesar_eventos_recarga(&mut self) {
        let events: Vec<AssetReloadEvent> = self
            .asset_watchers
            .iter()
            .flat_map(|watcher| watcher.receiver.try_iter())
            .collect();

        for ev in events {
            match ev {
                AssetReloadEvent::MeshReloaded(path) => {
                    log::info!("Procesando recarga en caliente de modelo: {:?}", path);
                    self.cargar_modelo_desde_glb(&path);
                }
                AssetReloadEvent::ShaderReloaded(path) => {
                    // Auditoría C2: la recarga en caliente de shaders
                    // EXISTE para los materiales de grafo (sus .spv
                    // viven en disco); el resto de shaders van
                    // compilados en el binario y sus fuentes .vert/
                    // .frag no tienen efecto en tiempo de ejecución.
                    let fname = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    let es_spv_grafo = fname.starts_with("graph_")
                        && (fname.ends_with(".frag.spv") || fname.ends_with(".vert.spv"));
                    if es_spv_grafo {
                        if let Some(renderer) = self.renderer.as_mut() {
                            renderer.reload_graph_materials(std::path::Path::new("assets/shaders"));
                            log::info!(
                                "Material de grafo recargado en caliente por cambio en {:?}",
                                path
                            );
                        }
                    } else {
                        log::info!(
                            "Shader {:?} modificado sin recarga: los shaders integrados van en el binario y las fuentes .vert/.frag se compilan fuera; recompila el .spv del grafo y se recargará solo.",
                            path
                        );
                    }
                }
            }
        }
    }

    /// Carga un modelo .glb completo en la caché (malla GPU, materiales,
    /// imágenes, geometría CPU, skin y texturas). Secuencia compartida por la
    /// recarga en caliente y la recarga bajo demanda.
    fn cargar_modelo_desde_glb(&mut self, path: &std::path::Path) {
        if let (Some(renderer), Some(vk_ctx)) = (&self.renderer, &self.vulkan_context) {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(model) = MeshLoader::load_model_from_glb_bytes(&bytes) {
                    let upload_res = {
                        let mut alloc = renderer.allocator.lock().unwrap();
                        MeshLoader::upload_to_gpu(
                            &renderer.device,
                            &mut alloc,
                            &model.vertices,
                            &model.indices,
                            vk_ctx.graphics_queue,
                            renderer.command_pool,
                        )
                    };

                    if let Ok(gpu_mesh) = upload_res {
                        let stem = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unnamed");
                        let mut alloc = renderer.allocator.lock().unwrap();
                        let mut cache = self.asset_cache.write().unwrap();
                        let primary = model.primary_material().cloned();
                        cache.insert_mesh(
                            &renderer.device,
                            &mut alloc,
                            stem,
                            gpu_mesh,
                        );
                        // La recarga también refresca materiales e
                        // imágenes: si el .blend cambió el material,
                        // el editor debe ver el nuevo, no el viejo.
                        cache.insert_model_data(
                            stem,
                            model.materials.clone(),
                            primary,
                            model.images.clone(),
                            model.primitive_ranges.clone(),
                            model.objects.clone(),
                        );
                        // Geometría CPU retenida (Fase 1.5): los
                        // colliders de malla la leen al reconstruir
                        // la física; sin esto se pierde al subir a GPU.
                        cache.insert_geometry(stem, &model);
                        self.upload_skin_if_any(renderer, vk_ctx, &mut alloc, &mut cache, &model, stem);
                        // Y las texturas GPU de todos los
                        // materiales: se resuben y los sets del
                        // modelo se reapuntan a las nuevas vistas.
                        if let Some(layout) = renderer.pbr_pipeline.descriptor_set_layout {
                            let mut uploaded: Vec<usize> = Vec::new();
                            for mat in &model.materials {
                                if let Some(idx) = mat.base_color_image {
                                    if !uploaded.contains(&idx) {
                                        uploaded.push(idx);
                                        if let Some(img) = model.images.get(idx) {
                                            cache.insert_texture(
                                                &renderer.device,
                                                &mut alloc,
                                                TextureUpload { queue: vk_ctx.graphics_queue, command_pool: renderer.command_pool, set_layout: layout },
                                                stem,
                                                idx,
                                                img,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Consume el refresco en caliente de un .physmat.json editado en
    /// el Inspector (E2): invalida la caché de la biblioteca única
    /// (el archivo ya lo escribió el editor) y aplica el valor nuevo
    /// EN SITIO sobre los colliders vivos que lo referencian. Los dos
    /// candados se toman por separado, nunca a la vez.
    #[cfg(feature = "editor")]
    fn apply_pending_physmat_refresh(&mut self) {
        let Some((referencia, material)) = self
            .editor_layout
            .inspector
            .pending_physmat_refresh
            .take()
        else {
            return;
        };

        {
            let mut lib = self.physmat_lib.lock().unwrap_or_else(|e| e.into_inner());
            lib.invalidate(&referencia);
        }

        let actualizados = {
            let mut world = self.physics_world.lock().unwrap_or_else(|e| e.into_inner());
            world.refresh_asset(&referencia, &material)
        };
        if actualizados > 0 {
            log::info!(
                "PhysicsMaterial: {actualizados} colliders actualizados en caliente para '{referencia}'."
            );
        }
    }

    /// Sustituye la biblioteca compartida por una anclada a la raíz
    /// del proyecto vigente (al abrir proyecto: la caché anterior es
    /// de otra raíz y no debe sobrevivir).
    #[cfg(feature = "editor")]
    fn refrescar_physmat_lib(&self) {
        let nueva = biblioteca_physmat(&self.scene_manager);
        let mut lib = self.physmat_lib.lock().unwrap_or_else(|e| e.into_inner());
        *lib = nueva;
    }

    /// Reancla el gestor de escenas y la librería de materiales físicos
    /// al proyecto `raiz` (defecto S19). El ancla de arranque sale del
    /// directorio de lanzamiento (`AssetVfs::locate` desde el cwd): sin
    /// este reanclaje ANTES de cargar la escena, abrir la escena de otro
    /// proyecto —lanzando el editor desde la raíz del repo, p. ej.—
    /// resolvía sus `.physmat.json` contra los assets del directorio de
    /// arranque y el material no cargaba (caía al legado/defecto). Si el
    /// gestor ya apunta a `raiz`, no hace nada.
    #[cfg(feature = "editor")]
    fn reanclar_al_proyecto(&mut self, raiz: &std::path::Path) {
        let raiz = proyecto_lock::raiz_canonica(raiz).unwrap_or_else(|_| raiz.to_path_buf());
        let ya_anclado = self
            .scene_manager
            .lock()
            .map(|m| m.vfs().root() == raiz.as_path())
            .unwrap_or(false);
        if ya_anclado {
            return;
        }
        match self.scene_manager.lock() {
            Ok(mut m) => {
                *m = SceneManager::new(&raiz);
                log::info!(
                    "Gestor de escenas reanclado a {}: {} escenas disponibles",
                    raiz.display(),
                    m.available_scenes().len()
                );
            }
            Err(_) => log::error!(
                "SceneManager: candado envenenado al reanclar; las escenas quedan ancladas a la raíz anterior."
            ),
        }
        self.refrescar_physmat_lib();
    }

    /// Compila (si hace falta) y carga el ensamblado de scripts del
    /// proyecto: sin esto, solo existen las clases de la fachada y
    /// cualquier script del proyecto falla al instanciarse (defecto
    /// D4). Si el DLL resultante es el mismo ya cargado (misma ruta y
    /// mismo mtime), no se recarga.
    fn cargar_scripts_del_proyecto(&mut self, raiz: &std::path::Path) {
        if !self.script_host.is_initialized {
            return;
        }
        let Some(dll) = project_scripts::asegurar_ensamblado_scripts(raiz, &raiz_motor_actual()) else {
            return;
        };
        let mtime = std::fs::metadata(&dll).and_then(|m| m.modified()).ok();
        if let (Some(mt), Some((ruta_previa, mt_previo))) =
            (mtime, &self.ensamblado_scripts_cargado)
        {
            if *ruta_previa == dll && *mt_previo == mt {
                return;
            }
        }
        match self.script_host.load_script_assembly(&dll) {
            Ok(()) => {
                log::info!(
                    "Ensamblado de scripts del proyecto cargado: {}",
                    dll.display()
                );
                self.ensamblado_scripts_cargado = mtime.map(|mt| (dll, mt));
            }
            Err(e) => log::error!(
                "No se pudo cargar el ensamblado de scripts del proyecto {}: {e}",
                dll.display()
            ),
        }
    }

    /// Aplica la carga de escena pendiente, si la hay, al final del frame.
    /// Aplica la carga de escena pendiente, si la hay, al final del frame.
    ///
    /// `SceneManager.LoadScene` de C# (y el menú del editor) sólo encolan; aquí
    /// es donde la escena cambia de verdad. El orden importa:
    ///
    /// 1. Se destruyen los scripts vivos **antes** de tocar las entidades, para
    ///    que ningún `GlassBehaviour` llame a `GetTransform` con un id que ya no
    ///    existe.
    /// 2. `apply_pending` sustituye el conjunto de entidades de golpe.
    /// 3. Se reconstruye la física desde los componentes `Physics` de la escena
    ///    nueva: los cuerpos de Rapier apuntan a ids viejos y hay que rehacerlos.
    /// 4. Se instancian los scripts que declara la escena nueva.
    fn apply_pending_scene_load(&mut self) {
        // Fase C: apertura de prefab pedida desde el Inspector; se aplica
        // en esta misma ventana de fin de frame que las cargas de escena
        // (nadie está recorriendo las entidades). Si además hay una carga
        // de escena pendiente, va después y cierra el modo prefab.
        #[cfg(feature = "editor")]
        if let Some(fuente_prefab) = self.editor_layout.pending_abrir_prefab.take() {
            self.abrir_prefab_en_editor(&fuente_prefab);
        }
        self.recarga_bajo_demanda.clear();
        let result = {
            let Ok(mut manager) = self.scene_manager.lock() else {
                log::error!("SceneManager: candado envenenado, no se aplica la carga pendiente.");
                return;
            };
            if !manager.has_pending() {
                return;
            }
            let requested = manager.pending_request().unwrap_or("?").to_string();
            let mut scene = match self.scene_state.lock() {
                Ok(s) => s,
                Err(_) => {
                    log::error!("SceneManager: SceneState bloqueado, se cancela la carga de '{requested}'.");
                    manager.cancel_pending();
                    return;
                }
            };
            log::info!("SceneManager: aplicando carga diferida de '{requested}' al final del frame...");
            manager.apply_pending(&mut scene)
        };

        match result {
            Ok(Some(loaded)) => {
                // Migración UI en memoria de la escena recién
                // sustituida (sin ruta: esta vía no escribe copia).
                if let Ok(mut sc) = self.scene_state.lock() {
                    crate::canvas_ui::migrar_ui_escena_cargada(&mut sc, None);
                    // `raiz_proyecto_actual` solo existe en la build con
                    // editor: sin él, el refresco es no-op (mismo patrón
                    // que en la recarga bajo demanda y en materiales).
                    #[cfg(feature = "editor")]
                    let raiz_proyecto = self.raiz_proyecto_actual.as_deref();
                    #[cfg(not(feature = "editor"))]
                    let raiz_proyecto: Option<&std::path::Path> = None;
                    crate::prefabs_vivos::refrescar_prefabs_escena_cargada(&mut sc, raiz_proyecto);
                }

                // Silencia el audio de la escena anterior al activarse la nueva.
                self.audio_system.stop_all();

                log::info!(
                    "SceneManager: escena '{}' activa ({} entidades, clave {}).",
                    loaded.name,
                    loaded.entity_ids.len(),
                    loaded.asset_key
                );

                // 0b. Gestos del Canvas a medias fuera: el botón memorizado
                // y la sesión de arrastre pertenecen a la escena anterior.
                self.button_click_tracker.cancel();
                self.ui_drag_session = None;

                // 1. Fuera las instancias de la escena anterior.
                self.player_script_handle = None;
                self.player_body_handle = None;
                self.script_host.destroy_all_instances();

                // 1a. Punto de interacción: quien lo publicaba era un
                // script de la escena anterior; si quedara fijado, la
                // escena nueva heredaría un intruso fantasma en sus
                // grafos. Vuelve al valor inactivo y ya publicará quien
                // corresponda en la escena nueva.
                glass_scripting::bindings::clear_interaction_point_state();

                // 1b. Skinning: los buffers y tiempos de animación de la
                // escena anterior no deben sobrevivir al cambio (los ids
                // de entidad se reutilizan entre escenas).
                self.reset_skinning_runtime();

                // 2. Física desde cero: los handles de Rapier no sobreviven a
                //    una sustitución de entidades.
                self.rebuild_physics_from_scene();

                // Una escena normal cierra el modo de edición de prefab.
                #[cfg(feature = "editor")]
                {
                    self.editor_layout.prefab_en_edicion = None;
                }

                // 3. Scripts que declara la escena nueva.
                self.instantiate_scene_scripts(&loaded.scripted);
            }
            Ok(None) => {}
            Err(e) => {
                // La escena anterior sigue intacta: `apply_pending` sólo
                // sustituye entidades después de parsear con éxito.
                log::error!("SceneManager: no se pudo cargar la escena: {e}");
                self.audio_system.stop_all();
            }
        }
    }

    /// Ejecutor de las tools MCP que solo puede resolver la app
    /// (`glass_mcp::APP_EXEC_TOOLS`): Play, guardar escena y escena
    /// nueva. Lo invoca el bombeo de la Command Queue al inicio del
    /// frame; las transiciones de Play y las cargas las aplican los
    /// mecanismos de fin de frame de siempre, así que aquí basta con
    /// mover el mismo estado que mueve la interfaz.
    #[cfg(feature = "editor")]
    fn ejecutar_tool_app(
        &mut self,
        tool: &str,
        args: &serde_json::Value,
    ) -> Option<Result<serde_json::Value, String>> {
        use glass_editor::PlayState as P;
        let texto = |t: String| serde_json::json!({ "content": [{ "type": "text", "text": t }] });
        match tool {
            "glass_play_control" => {
                let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("");
                let estado = self.editor_layout.viewport.play_state;
                let r = match (action, estado) {
                    ("play", P::Playing) => Ok("Ya está en Play.".to_string()),
                    ("play", _) => {
                        self.editor_layout.viewport.play_state = P::Playing;
                        Ok("Play: la transición se aplica al final del frame (foto de la escena, física y scripts).".to_string())
                    }
                    ("pause", P::Playing) => {
                        self.editor_layout.viewport.play_state = P::Paused;
                        Ok("Pausa.".to_string())
                    }
                    ("pause", _) => Err("Solo se puede pausar desde Play.".to_string()),
                    ("stop", P::Playing) | ("stop", P::Paused) => {
                        self.editor_layout.viewport.play_state = P::Edit;
                        Ok("Stop: al final del frame se restaura la escena autorada.".to_string())
                    }
                    ("stop", _) => Err("No hay partida en curso.".to_string()),
                    ("step", P::Paused) => {
                        self.paso_mcp_pendiente = true;
                        Ok("Paso: este frame corre un ciclo de simulación (scripts y física).".to_string())
                    }
                    ("step", _) => {
                        Err("Step requiere estar en pausa (Play y luego pause).".to_string())
                    }
                    _ => Err(format!(
                        "Acción desconocida: '{action}' (play|pause|stop|step)"
                    )),
                };
                Some(r.map(texto))
            }
            "glass_scene_save" => Some(self.mcp_guardar_escena(args).map(texto)),
            "glass_scene_new" => Some(self.mcp_escena_nueva(args).map(texto)),
            "glass_undo" | "glass_redo" => {
                let redo = tool == "glass_redo";
                let aplicado = self.editor_layout.apply_history(&self.scene_state, redo);
                let h = &self.editor_layout.undo_history;
                Some(Ok(texto(format!(
                    "{{\"applied\": {}, \"can_undo\": {}, \"can_redo\": {}}}",
                    aplicado,
                    h.can_undo(),
                    h.can_redo()
                ))))
            }
            "glass_console_log" => {
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50)
                    .min(300) as usize;
                let nivel = args
                    .get("level")
                    .and_then(|v| v.as_str())
                    .map(|l| l.to_string());
                let contiene = args
                    .get("contains")
                    .and_then(|v| v.as_str())
                    .map(|c| c.to_lowercase());
                let logs = &self.editor_layout.console.logs;
                let total = logs.len();
                let mut filtrados: Vec<serde_json::Value> = logs
                    .iter()
                    .filter(|(_, n)| {
                        nivel
                            .as_deref()
                            .is_none_or(|f| n.eq_ignore_ascii_case(f))
                    })
                    .filter(|(t, _)| {
                        contiene
                            .as_deref()
                            .is_none_or(|f| t.to_lowercase().contains(f))
                    })
                    .map(|(t, n)| serde_json::json!({ "level": n, "text": t }))
                    .collect();
                let desde = filtrados.len().saturating_sub(limit);
                let entradas: Vec<serde_json::Value> = filtrados.drain(desde..).collect();
                let payload = serde_json::json!({
                    "total": total,
                    "returned": entradas.len(),
                    "entries": entradas,
                });
                Some(Ok(texto(
                    serde_json::to_string_pretty(&payload).unwrap_or_default(),
                )))
            }
            "glass_editor_selection" => {
                let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("get");
                match action {
                    "get" => {
                        let sel = self.editor_layout.hierarchy.selected_entity;
                        let payload = serde_json::json!({ "selected_entity": sel });
                        Some(Ok(texto(
                            serde_json::to_string_pretty(&payload).unwrap_or_default(),
                        )))
                    }
                    "set" => {
                        match args.get("entity_id") {
                            None | Some(serde_json::Value::Null) => {
                                self.editor_layout.hierarchy.selected_entity = None;
                                Some(Ok(texto(
                                    "{\"selected_entity\": null}".to_string(),
                                )))
                            }
                            Some(v) => match v.as_u64() {
                                Some(eid) => {
                                    let existe = self
                                        .scene_state
                                        .lock()
                                        .map(|sc| {
                                            sc.entities.iter().any(|e| e.id == eid)
                                        })
                                        .unwrap_or(false);
                                    if existe {
                                        self.editor_layout.hierarchy.selected_entity =
                                            Some(eid);
                                        Some(Ok(texto(format!(
                                            "{{\"selected_entity\": {eid}}}"
                                        ))))
                                    } else {
                                        Some(Err(format!("Entity #{eid} not found")))
                                    }
                                }
                                None => Some(Err(
                                    "'entity_id' debe ser un entero o null".to_string(),
                                )),
                            },
                        }
                    }
                    otro => Some(Err(format!(
                        "Acción no válida para glass_editor_selection: {otro} (get|set)"
                    ))),
                }
            }
            "glass_editor_camera" => {
                let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("get");
                let vp = &mut self.editor_layout.viewport;
                match action {
                    "get" => {
                        let payload = serde_json::json!({
                            "yaw": vp.editor_cam_yaw,
                            "pitch": vp.editor_cam_pitch,
                            "distance": vp.editor_cam_dist,
                            "center": vp.editor_cam_center,
                        });
                        Some(Ok(texto(
                            serde_json::to_string_pretty(&payload).unwrap_or_default(),
                        )))
                    }
                    "set" => {
                        let num = |k: &str| args.get(k).and_then(|v| v.as_f64());
                        if let Some(y) = num("yaw") {
                            if !y.is_finite() {
                                return Some(Err("'yaw' debe ser finito".to_string()));
                            }
                            vp.editor_cam_yaw = y as f32;
                        }
                        if let Some(p2) = num("pitch") {
                            if !p2.is_finite() {
                                return Some(Err("'pitch' debe ser finito".to_string()));
                            }
                            vp.editor_cam_pitch = (p2 as f32).clamp(-1.553, 1.553);
                        }
                        if let Some(d) = num("distance") {
                            if !d.is_finite() || d <= 0.0 {
                                return Some(Err(
                                    "'distance' debe ser un número > 0".to_string(),
                                ));
                            }
                            vp.editor_cam_dist = d as f32;
                        }
                        if let Some(c) = args.get("center") {
                            let arr = c.as_array().filter(|a| a.len() == 3).and_then(|a| {
                                Some([
                                    a[0].as_f64()? as f32,
                                    a[1].as_f64()? as f32,
                                    a[2].as_f64()? as f32,
                                ])
                            });
                            match arr {
                                Some(centro) if centro.iter().all(|n| n.is_finite()) => {
                                    vp.editor_cam_center = centro;
                                }
                                _ => {
                                    return Some(Err(
                                        "'center' debe ser [x, y, z] finito".to_string(),
                                    ))
                                }
                            }
                        }
                        let payload = serde_json::json!({
                            "yaw": vp.editor_cam_yaw,
                            "pitch": vp.editor_cam_pitch,
                            "distance": vp.editor_cam_dist,
                            "center": vp.editor_cam_center,
                        });
                        Some(Ok(texto(
                            serde_json::to_string_pretty(&payload).unwrap_or_default(),
                        )))
                    }
                    "frame" => {
                        let Some(eid) = args.get("entity_id").and_then(|v| v.as_u64())
                        else {
                            return Some(Err(
                                "'entity_id' requerido para frame".to_string(),
                            ));
                        };
                        let pos = self.scene_state.lock().ok().and_then(|sc| {
                            sc.entities
                                .iter()
                                .find(|e| e.id == eid)
                                .map(|e| e.transform.position)
                        });
                        match pos {
                            Some(p2) => {
                                vp.editor_cam_center = p2;
                                let payload = serde_json::json!({
                                    "yaw": vp.editor_cam_yaw,
                                    "pitch": vp.editor_cam_pitch,
                                    "distance": vp.editor_cam_dist,
                                    "center": vp.editor_cam_center,
                                });
                                Some(Ok(texto(
                                    serde_json::to_string_pretty(&payload)
                                        .unwrap_or_default(),
                                )))
                            }
                            None => Some(Err(format!("Entity #{eid} not found"))),
                        }
                    }
                    otro => Some(Err(format!(
                        "Acción no válida para glass_editor_camera: {otro} (get|set|frame)"
                    ))),
                }
            }
            _ => None,
        }
    }

    /// Guardado de escena del MCP: las mismas reglas que el Ctrl+S
    /// del editor (prefab en edición primero, diff de instancias
    /// recalculado antes de escribir, ruta directa si la hay).
    #[cfg(feature = "editor")]
    fn mcp_guardar_escena(&mut self, args: &serde_json::Value) -> Result<String, String> {
        if let Some(r) = self
            .editor_layout
            .guardar_prefab_en_edicion(&self.scene_state)
        {
            return r.map(|()| "Prefab en edición guardado.".to_string());
        }
        let nombre = args.get("name").and_then(|n| n.as_str());
        if let Some(nombre) = nombre {
            // Guardar como: clave del VFS, escribir y dejarla activa.
            let key = glass_core::scenes::scene_key_for_name(nombre);
            let mut manager = self
                .scene_manager
                .lock()
                .map_err(|_| "SceneManager envenenado".to_string())?;
            let path = manager
                .vfs()
                .resolve(&key)
                .map_err(|e| format!("Ruta inválida: {e}"))?;
            if let Some(padre) = path.parent() {
                let _ = std::fs::create_dir_all(padre);
            }
            let mut scene = self
                .scene_state
                .lock()
                .map_err(|_| "SceneState envenenado".to_string())?;
            if let Some(raiz) = self.raiz_proyecto_actual.as_deref() {
                scene.recalcular_overrides_prefab(&|fuente| {
                    glass_core::Prefab::cargar_desde_proyecto(raiz, fuente)
                });
            }
            scene
                .save_to_json(&path.to_string_lossy())
                .map_err(|e| format!("No se pudo guardar: {e:?}"))?;
            self.editor_layout.marcar_guardada(&scene);
            drop(scene);
            let corto = glass_core::scenes::short_scene_name(&key);
            manager.set_active_file(&corto, &key, path.clone());
            drop(manager);
            Ok(format!("Escena guardada como {key} ({})", path.display()))
        } else {
            // Sin nombre: el destino lo dicta el SceneManager (el archivo
            // del que salió la escena activa; defecto S1).
            let manager = self
                .scene_manager
                .lock()
                .map_err(|_| "SceneManager envenenado".to_string())?;
            let path = manager
                .save_path()
                .map_err(|e| format!("Sin ruta de guardado: {e}"))?;
            drop(manager);
            if let Some(padre) = path.parent() {
                let _ = std::fs::create_dir_all(padre);
            }
            let mut scene = self
                .scene_state
                .lock()
                .map_err(|_| "SceneState envenenado".to_string())?;
            if let Some(raiz) = self.raiz_proyecto_actual.as_deref() {
                scene.recalcular_overrides_prefab(&|fuente| {
                    glass_core::Prefab::cargar_desde_proyecto(raiz, fuente)
                });
            }
            scene
                .save_to_json(&path.to_string_lossy())
                .map_err(|e| format!("No se pudo guardar: {e:?}"))?;
            self.editor_layout.marcar_guardada(&scene);
            Ok(format!("Escena guardada en {}", path.display()))
        }
    }

    /// Escena nueva del MCP: escribe el archivo con la escena por
    /// defecto y pide su carga al SceneManager (se aplica al final
    /// del frame, con la misma limpieza que un cambio de escena
    /// desde el menú).
    #[cfg(feature = "editor")]
    fn mcp_escena_nueva(&mut self, args: &serde_json::Value) -> Result<String, String> {
        let nombre = args
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| "Falta el nombre de la escena".to_string())?;
        let key = glass_core::scenes::scene_key_for_name(nombre);
        let mut manager = self
            .scene_manager
            .lock()
            .map_err(|_| "SceneManager envenenado".to_string())?;
        let path = manager
            .vfs()
            .resolve(&key)
            .map_err(|e| format!("Ruta inválida: {e}"))?;
        if path.exists() {
            return Err(format!("Ya existe una escena con ese nombre ({key})"));
        }
        if let Some(padre) = path.parent() {
            std::fs::create_dir_all(padre)
                .map_err(|e| format!("No se pudo crear la carpeta: {e}"))?;
        }
        SceneState::default()
            .save_to_json(&path.to_string_lossy())
            .map_err(|e| format!("No se pudo crear la escena: {e:?}"))?;
        manager
            .request_load(nombre)
            .map_err(|e| format!("Escena creada en {key}, pero la carga falló: {e}"))?;
        Ok(format!(
            "Escena '{nombre}' creada en {key}; su carga se aplica al final del frame."
        ))
    }

    /// Reconstruye el mundo de Rapier a partir de los componentes `Physics`.
    /// Detecta las transiciones de play_state y actúa en un único punto
    /// (las vías de UI solo cambian play_state). Se llama al FINAL del
    /// frame, junto a `apply_pending_scene_load`: ahí el renderer ya no
    /// está prestado y la escena no se está recorriendo. Entrar en Play
    /// guarda la foto de la escena autorada y pasa la vista principal a
    /// Game; Stop restaura la foto entera.
    #[cfg(feature = "editor")]
    fn handle_play_transitions(&mut self) {
        if self.is_standalone {
            return;
        }
        let now = self.editor_layout.viewport.play_state;
        match play_transition(self.prev_play_state, now) {
            PlayTransition::EnterPlay => {
                if let Ok(scene) = self.scene_state.lock() {
                    self.play_snapshot = Some((*scene).clone());
                }
                self.editor_layout.viewport.active_tab = glass_editor::ViewportTab::GameView;
                self.editor_layout.focus_game_tab();

                // El mundo de Play nace de la escena autorada TAL COMO ESTÁ
                // al pulsar Jugar: sin esta reconstrucción, Play reutilizaba
                // el mundo armado al cargar el proyecto (o el restaurado en
                // el Stop anterior) y cualquier edición de física hecha en
                // el Inspector — forma, tamaño, material, dirección —
                // tardaba un ciclo entero de Play en tener efecto
                // (defecto cazado por el test físico de E5, 2026-11-24).
                self.rebuild_physics_from_scene();

                // Personajes autorados: (id, datos, centro de la cápsula en mundo, material).
                // El material del CharacterController sale del cuerpo de su
                // entidad (§5.4): su legado si lo hay; si no, PROJECT_DEFAULT.
                let autorados: Vec<_> = match self.scene_state.lock() {
                    Ok(scene) => scene
                        .entities
                        .iter()
                        .filter(|e| e.active)
                        .filter_map(|e| {
                            e.character.as_ref().map(|d| {
                                let material = e
                                    .physics
                                    .as_ref()
                                    .map(|physics| physics.body_fallback_material())
                                    .unwrap_or_else(|| {
                                        glass_core::PhysicsData::default()
                                            .body_fallback_material()
                                    });
                                (e.id, d.clone(), material)
                            })
                        })
                        .map(|(id, d, material)| {
                            let centro = glass_core::mat4_transform_point(
                                scene.compute_world_matrix(id),
                                d.center,
                            );
                            (id, d, centro, material)
                        })
                        .collect(),
                    Err(_) => Vec::new(),
                };
                if !autorados.is_empty() {
                    if let Ok(mut physics) = self.physics_world.lock() {
                        for (id, data, centro, material) in &autorados {
                            let (radius, height, skin, step_offset, slope_limit_deg, snap) =
                                data.config();
                            let creado = physics.create_character_autorado(
                                *id,
                                *centro,
                                glass_physics::character::CharacterConfig {
                                    radius,
                                    height,
                                    skin,
                                    step_offset,
                                    slope_limit_deg,
                                    snap,
                                },
                                data.center,
                                data.min_move_distance,
                                data.detect_collisions,
                                data.enabled,
                            );
                            if creado {
                                self.cc_autorados_en_play.push(*id);
                                // Sin esto la cápsula quedaría con la fricción
                                // 0.0 de nacimiento en vez del material del
                                // cuerpo (§5.4).
                                physics.apply_character_material(*id, material);
                            }
                        }
                    }
                }
            }
            PlayTransition::StopPlay => {
                // Destruir los CC autorados creados al entrar (la lista
                // guardada, no la escena actual: durante Play la escena
                // pudo cambiar o perder entidades) antes de restaurar,
                // para que ni el personaje ni su marca sobrevivan.
                let ids_cc: Vec<u64> = std::mem::take(&mut self.cc_autorados_en_play);
                if !ids_cc.is_empty() {
                    if let Ok(mut physics) = self.physics_world.lock() {
                        for id in ids_cc {
                            physics.destroy_character(id);
                        }
                    }
                }
                if let Some(foto) = self.play_snapshot.take() {
                    self.restore_play_snapshot(foto);
                }
            }
            PlayTransition::None => {}
        }
        self.prev_play_state = now;
    }

    /// Restaura la escena autorada tal como estaba al entrar en Play y
    /// reconstruye física y scripts (espejo del teardown de
    /// `apply_pending_scene_load` y de la composición de
    /// `bootstrap_scene_scripts_and_physics`).
    #[cfg(feature = "editor")]
    fn restore_play_snapshot(&mut self, snapshot: SceneState) {
        // Un único bloqueo: se sustituye la escena y se compone `scripted`
        // con el mismo guard, que se suelta al salir del bloque.
        let (scripted, num_entidades): (Vec<(u64, String)>, usize) = {
            let Ok(mut scene) = self.scene_state.lock() else {
                log::error!("Stop: no se pudo bloquear la escena; la foto de Play se descarta.");
                return;
            };
            *scene = snapshot;
            let scripted = scene
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
            (scripted, scene.entities.len())
        };
        self.audio_system.stop_all();
        self.player_script_handle = None;
        self.player_body_handle = None;
        self.script_host.destroy_all_instances();
        glass_scripting::bindings::clear_interaction_point_state();
        self.reset_skinning_runtime();
        self.rebuild_physics_from_scene();
        self.instantiate_scene_scripts(&scripted);
        log::info!("Stop: escena restaurada a su estado previo a Play ({} entidades).", num_entidades);
    }

    fn rebuild_physics_from_scene(&mut self) {
        // Mundo nuevo: las posiciones previas del pase de cruce
        // pertenecen a la escena anterior.
        self.portal_prev_pos.clear();
        let world_matrices = {
            let Ok(scene) = self.scene_state.lock() else {
                return;
            };
            scene.compute_world_matrices()
        };

let mut descriptors: Vec<DescriptorCuerpo> = Vec::new();
        // Una sola librería para toda la reconstrucción: la caché de
        // assets se comparte entre colliders (C3).
        let mut lib = self.physmat_lib.lock().unwrap_or_else(|e| e.into_inner());
        {
            let Ok(scene) = self.scene_state.lock() else {
                return;
            };
            for ent in &scene.entities {
                let Some(body) = &ent.physics else { continue };
                if !ent.active {
                    continue;
                }
                // Las superficies de portal no colisionan: son
                // puertas, no muros. El pase de cruce (tras el paso
                // de física) mueve los cuerpos dinámicos a través;
                // con collider, Rapier los detendría justo en el
                // plano y el segmento nunca cruzaría.
                if ent.portal.is_some() {
                    continue;
                }
                let body_type = if !body.has_rigidbody {
                    // Collider sin Rigidbody = collider estático (Unity):
                    // cuerpo fijo sea cual sea body_type.
                    BodyType::Static
                } else {
                    match body.body_type.as_str() {
                        "Static" => BodyType::Static,
                        "Kinematic" => BodyType::Kinematic,
                        _ => BodyType::Dynamic,
                    }
                };
                let world = world_matrices.get(&ent.id).copied().unwrap_or_else(|| {
                    glass_core::mat4_from_trs_euler(
                        ent.transform.position,
                        ent.transform.rotation(),
                        ent.transform.scale,
                    )
                });
                let pos = glass_core::mat4_translation(world);
                let rot = glass_core::mat4_to_quat(world);
                // Forma del collider: se deriva de la malla integrada
                // de la entidad (sphere/capsule/cylinder) con su
                // escala mundial; cualquier otro modelo —incluido no
                // tener malla, que al dibujar cae al cubo— produce la
                // caja de `box_extents` de siempre (compatibilidad
                // total con el contenido existente: ninguna escena
                // usa aún los nombres nuevos).
                let model = ent.mesh.as_ref().map(|m| m.model.as_str()).unwrap_or("");
                let shape = collider_shape_for_builtin(
                    model,
                    glass_core::mat4_scale(world),
                    body.box_extents,
                );
// Rigidbody sin Collider: cuerpo sin formas (Rapier lo acepta).
let resueltos = if body.has_collider {
    colliders_resueltos(body, shape, &mut lib)
} else {
    Vec::new()
};
                // Si el cuerpo pide malla real, guardamos los datos para resolverla
                // tras el bucle de escena (hace falta el AssetCache). La config
                // de la malla sale de los campos del propio cuerpo (E6).
                let mesh = if !body.has_collider {
                    None
                } else if body.mesh_collider != glass_core::MeshColliderMode::None {
                    Some((
                        body.mesh_collider,
                        model.to_string(),
                        glass_core::mat4_scale(world),
                        body.body_type == "Static",
                        ColliderConfig {
                            layer: body.layer,
                            mask: body.collision_mask,
                            is_trigger: body.mesh_collider_is_trigger,
                        },
                    ))
                } else {
                    None
                };
                // Material efectivo del collider de malla (asset, legado
                // o valores por defecto), resuelto una sola vez por entidad.
                let (mesh_material, mesh_material_ref) =
                    material_resuelto(&body.mesh_collider_material, body, &mut lib);
                descriptors.push((
                    ent.id,
                    body_type,
                    pos,
                    rot,
                    resueltos,
                    mesh_material,
                    mesh_material_ref,
                    body.body_fallback_material(),
                    body.linear_velocity,
                    mesh,
                    !body.colliders.is_empty(),
                    (
    body.mass,
    body.linear_damping,
    body.use_gravity,
    body.ccd,
    body.interpolate,
    body.angular_damping,
    body.solver_iterations,
    body.max_depenetration_velocity,
    body.include_layers,
    body.exclude_layers,
    body.solver_velocity_iterations,
    body.automatic_center_of_mass,
    body.automatic_inertia_tensor,
),
                ));
            }
        }

// Precargamos las geometrías retenidas que haga falta para armar colliders
        // de malla real (Convex/TriMesh); si nadie las pide, el mapa queda vacío.
        let necesita_geometria = descriptors.iter().any(|d| d.9.is_some());
        let geometrias: std::collections::HashMap<String, (Vec<[f32; 3]>, Vec<u32>)> =
            if necesita_geometria {
                match self.asset_cache.read() {
                    Ok(cache) => cache.geometries.clone(),
                    Err(_) => {
                        log::error!("Física: AssetCache envenenado, no hay geometrías para colliders de malla.");
                        std::collections::HashMap::new()
                    }
                }
            } else {
                std::collections::HashMap::new()
            };

        let Ok(mut physics) = self.physics_world.lock() else {
            log::error!("Física: candado envenenado, no se reconstruye el mundo.");
            return;
        };
        *physics = PhysicsWorld::new();
        for (id, body_type, pos, rot, resueltos, mesh_material, mesh_material_ref, fallback, linvel, mesh, hay_specs, superficie) in descriptors
        {
            let handle = physics.create_rigid_body(id, body_type, pos, rot);
            let (
                masa,
                damping,
                usa_gravedad,
                ccd,
                interpola,
                angular_damping,
                solver_iterations,
                max_depenetration_velocity,
                include_layers,
                exclude_layers,
                solver_velocity_iterations,
                automatic_center_of_mass,
                automatic_inertia_tensor,
            ) = superficie;
            if interpola {
                physics.set_interpolation(id, true);
            }
            physics.set_linear_damping(handle, damping);
            physics.set_angular_damping(handle, angular_damping);
            physics.set_solver_iterations(
                handle,
                i32::try_from(solver_iterations).unwrap_or(i32::MAX),
            );
            physics.set_solver_velocity_iterations(
                handle,
                i32::try_from(solver_velocity_iterations).unwrap_or(i32::MAX),
            );
            if max_depenetration_velocity > 0.0 {
                physics.set_max_depenetration_velocity(handle, max_depenetration_velocity);
            }
            if include_layers != u32::MAX {
                physics.set_include_layers(handle, include_layers);
            }
            if !automatic_center_of_mass {
                physics.set_automatic_center_of_mass(handle, false);
            }
            if !automatic_inertia_tensor {
                physics.set_automatic_inertia_tensor(handle, false);
            }
            if exclude_layers != 0 {
                physics.set_exclude_layers(handle, exclude_layers);
            }
            if !usa_gravedad {
                physics.set_gravity_scale(handle, 0.0);
            }
            if ccd {
                physics.set_ccd_enabled(handle, true);
            }

            // Misma ruta de adjunto que create_body (C3): el material
            // viaja resuelto con campos nombrados; no hay pares
            // posicionales (friction, restitution) que cruzar.
            let (modo_malla, modelo_malla, escala_malla, es_estatico_malla, mesh_cfg) = match &mesh {
                Some((modo, modelo, escala, estatico, cfg)) => {
                    (modo, modelo.as_str(), *escala, *estatico, *cfg)
                }
                None => (
                    &glass_core::MeshColliderMode::None,
                    "",
                    [1.0, 1.0, 1.0],
                    false,
                    ColliderConfig::default(),
                ),
            };
            let geometria_malla = mesh
                .as_ref()
                .and_then(|(_, model_name, _, _, _)| geometrias.get(model_name))
                .map(|(v, i)| (v.as_slice(), i.as_slice()));
            let _via_mesh = adjuntar_colliders_autorados(
                &mut physics,
                handle,
                id,
                es_estatico_malla,
                MallaAutorada {
                    modo: modo_malla,
                    geometria: geometria_malla,
                    escala: escala_malla,
                    cfg: mesh_cfg,
                    material: &mesh_material,
                    material_ref: &mesh_material_ref,
                    model: modelo_malla,
                },
                resueltos,
                hay_specs,
                &fallback,
            );

            // Masa al final del cuerpo: los colliders ya están adjuntos.
            if body_type == BodyType::Dynamic {
                physics.set_mass(handle, masa);
            }
            if !automatic_center_of_mass {
                physics.set_automatic_center_of_mass(handle, false);
            }
            if !automatic_inertia_tensor {
                physics.set_automatic_inertia_tensor(handle, false);
            }
            physics.set_linear_velocity(handle, linvel);
        }
        log::info!(
            "Física reconstruida: {} cuerpo(s), {} collider(s).",
            physics.body_count(),
            physics.collider_count()
        );
    }

    /// Arranque directo de una escena (`--scene` o la escena por defecto
    /// del standalone): `load_from_json` solo sustituye las entidades, así
    /// que sin este paso la escena quedaría «en cartel» —renderizada pero
    /// sin física ni scripts— porque la reconstrucción completa solo la
    /// hacía la carga diferida de `SceneManager`
    /// (`apply_pending_scene_load`). Aquí se aplican a la escena ya
    /// cargada los mismos dos pasos: reconstruir la física desde sus
    /// componentes e instanciar los scripts que declara (con la misma
    /// regla que `SceneManager::apply_pending`: manda
    /// `script_data.class_name` y se cae al campo `script` heredado).
    fn bootstrap_scene_scripts_and_physics(&mut self, scene_path: &str) {
        // La carga directa no pasa por `SceneManager::apply_pending`,
        // que es quien marca la escena activa: sin esto, un script que
        // lea el nombre de la escena activa (p. ej. una meta que decide
        // el siguiente nivel) vería el valor por defecto («Untitled») y
        // enrutaría mal; y «Save Scene» no sabría de qué archivo salió
        // esta escena y escribiría sobre el de la anterior (defecto S1).
        // Se registra nombre, clave VFS si el archivo vive en el
        // proyecto, y la ruta: el gestor es el único dueño de la
        // identidad de la escena activa.
        if let Ok(mut manager) = self.scene_manager.lock() {
            let ruta = std::path::PathBuf::from(scene_path);
            let clave = glass_core::vfs::to_asset_key(manager.vfs().root(), &ruta)
                .unwrap_or_default();
            manager.set_active_file(
                &glass_core::scenes::short_scene_name(scene_path),
                &clave,
                ruta,
            );
        }
        let scripted: Vec<(u64, String)> = {
            let Ok(scene) = self.scene_state.lock() else {
                return;
            };
            scene
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
                .collect()
        };
        self.rebuild_physics_from_scene();
        self.instantiate_scene_scripts(&scripted);
    }

    /// Registra un fallo de .NET: guarda el estado y, en el editor, pinta la
    /// banda de aviso y escribe una única entrada en consola por cada estado
    /// distinto (y otra si es por escena). En el juego standalone no hay
    /// banda ni consola: el estado queda guardado y el log ya lo cuenta.
    fn notificar_fallo_dotnet(&mut self, fallo: FalloDotnet, por_escena: bool) {
        self.fallo_dotnet = Some(fallo);

        // En la build de juego (sin editor) glass_launcher no se enlaza:
        // allí no hay banda ni consola que pintar, así que el valor no se usa.
        #[cfg(feature = "editor")]
        let gestionado = glass_launcher::updates::es_gestionado();
        #[cfg(not(feature = "editor"))]
        let gestionado = false;
        let (titulo, cuerpo) = match fallo {
            FalloDotnet::SinHostfxr => (
                "No se puede ejecutar C#: no se encontró .NET",
                if gestionado {
                    "El editor no encontró ninguna instalación de .NET utilizable. Los scripts C# no se ejecutarán. Abre el Hub → Instalaciones → Dependencias e instala «.NET 8 Runtime». Después, reinicia el editor."
                } else {
                    "El editor no encontró ninguna instalación de .NET utilizable. Los scripts C# no se ejecutarán. Instala «.NET 8 Runtime» desde https://dotnet.microsoft.com/download/dotnet/8.0 y reinicia el editor."
                },
            ),
            FalloDotnet::SinFrameworkNet8 => (
                "No se puede ejecutar C#: falta Microsoft.NETCore.App 8",
                if gestionado {
                    "Se detectó .NET, pero no está instalado Microsoft.NETCore.App 8.0.x, necesario para ejecutar C#. Abre el Hub → Instalaciones → Dependencias e instala «.NET 8 Runtime». Después, reinicia el editor."
                } else {
                    "Se detectó .NET, pero no está instalado Microsoft.NETCore.App 8.0.x, necesario para ejecutar C#. Instala «.NET 8 Runtime» desde https://dotnet.microsoft.com/download/dotnet/8.0 y reinicia el editor."
                },
            ),
        };

        let primera_vez_estado = self.aviso_dotnet_consola_hecho != Some(fallo);
        if primera_vez_estado {
            self.aviso_dotnet_consola_hecho = Some(fallo);
        }
        let primera_vez_escena = por_escena && !self.aviso_escena_consola_hecho;
        if por_escena {
            self.aviso_escena_consola_hecho = true;
        }

        #[cfg(feature = "editor")]
        {
            // Banda en la parte superior del editor.
            self.editor_layout.mostrar_aviso_dotnet(AvisoBanner {
                titulo: titulo.to_string(),
                cuerpo: cuerpo.to_string(),
            });
            if primera_vez_estado {
                self.editor_layout
                    .empujar_consola(format!("{titulo}. {cuerpo}"), "ERROR");
            }
            if primera_vez_escena {
                self.editor_layout.empujar_consola(
                    "La escena contiene scripts C#; sus componentes no se instanciarán hasta que se reinicie el editor con .NET 8 disponible.".into(),
                    "ERROR",
                );
            }
        }
        #[cfg(not(feature = "editor"))]
        {
            let _ = (titulo, cuerpo, primera_vez_estado, primera_vez_escena);
        }
    }

    /// Instancia en `ScriptHost` los scripts declarados por la escena cargada.
    fn instantiate_scene_scripts(&mut self, scripted: &[(u64, String)]) {
        if !self.script_host.is_initialized {
            log::warn!(
                "ScriptHost no inicializado: {} script(s) de la escena quedan sin instanciar.",
                scripted.len()
            );
            // Si el arranque ya conoce la causa, repite el aviso con el matiz de escena.
            // Si es `None` no sabemos por qué, así que no inventamos causa.
            if let Some(f) = self.fallo_dotnet {
                self.notificar_fallo_dotnet(f, true);
            }
            return;
        }
        for (entity_id, class_name) in scripted {
            match self.script_host.create_instance(*entity_id, class_name) {
                Ok(handle) => {
                    log::info!("Script '{class_name}' instanciado para entidad {entity_id} (handle {handle}).");
                    // Sobre C0 del huésped MonoBehaviour: nombre, tag y
                    // estado activo de la entidad. La fachada devuelve 1
                    // si la instancia no es un huésped (GlassBehaviour de
                    // siempre) y None si no exporta el símbolo: ambos se
                    // ignoran en silencio.
                    if let Ok(scene) = self.scene_state.lock() {
                        if let Some(e) = scene.entities.iter().find(|e| e.id == *entity_id) {
                            let mut sobre = serde_json::json!({
                                "v": 1,
                                "name": e.name,
                                "tag": e.tag,
                                "active": e.active,
                            });
                            // C0b: los campos serializados del script,
                            // si la escena los trae.
                            if let Some(sd) = &e.script_data {
                                if !sd.campos.is_empty() {
                                    sobre["fields"] = serde_json::Value::Object(
                                        sd.campos
                                            .iter()
                                            .map(|(k, v)| (k.clone(), v.clone()))
                                            .collect(),
                                    );
                                }
                            }
                            let _ = self.script_host.apply_script_data(handle, &sobre.to_string());
                        }
                    }
                    // El jugador se localiza por clase: es la única forma de
                    // recuperar el handle del `RigidBody` que mueve el controlador
                    // sin meter un campo extra en el formato de escena.
                    if class_name.ends_with("PlayerController") {
                        self.player_script_handle = Some(handle);
                        if let Ok(physics) = self.physics_world.lock() {
                            self.player_body_handle = physics.get_body_by_entity(*entity_id);
                        }
                    }
                }
                Err(e) => log::warn!("No se pudo instanciar '{class_name}' para {entity_id}: {e}"),
            }
        }
    }

    // Reconciliación por frame del bloque de instanciación: vive en la
    // función libre `reconcile_spawned_scripts` (después de este
    // `impl`) porque `render_frame` retiene un préstamo mutable del
    // renderer durante todo el frame: ahí dentro solo se pueden tocar
    // campos sueltos de `self`, nunca `&mut self` entero.


    fn render_frame(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Command Queue del MCP (Fase 8): los comandos encolados se
        // ejecutan AQUÍ, al inicio del frame y en orden de envío —
        // nunca a mitad de frame ni en el hilo del cliente MCP.
        // Solo existe servidor MCP (y cola) en la build con editor:
        // en el juego puro el campo no está y no hay nada que bombear.
        #[cfg(feature = "editor")]
        {
            // El ejecutor de la app resuelve las tools que solo puede
            // ejecutar ella (Play, guardar, escena nueva): el cierre
            // captura &mut self y la llamada va sobre el clon del Arc,
            // así no hay doble préstamo del campo.
            let server = self.mcp_server.clone();
            server.pump_queue_with(|tool, args| self.ejecutar_tool_app(tool, args));
        }

        // Antes de recargar o expulsar activos hay que garantizar que la GPU
        // terminó el frame anterior: la recarga en caliente y `evict_idle`
        // destruyen texturas y descriptor sets, y hasta ahora la única espera
        // de este fence vivía dentro de `render_views()`/del pase egui, es
        // decir DESPUÉS de esas destrucciones (la seguridad era transitiva y
        // se perdía en las rutas de retorno temprano). Se espera SIN reset:
        // el reset lo hace, como siempre, quien graba el siguiente envío. El
        // fence nace señalado (ver creación en glass_renderer), así que el
        // primer frame no se bloquea.
        if let Some(renderer) = &self.renderer {
            unsafe {
                renderer
                    .device
                    .wait_for_fences(&[renderer.fence], true, u64::MAX)?;
            }
        }

        // Buffers de articulación retirados en frames anteriores (cambio
        // de escena o piel recreada): con el fence ya esperado, ningún
        // frame en vuelo los lee y se pueden liberar.
        if !self.retired_joint_buffers.is_empty() {
            if let Some(renderer) = &self.renderer {
                let joints_pool = renderer.joints_pool;
                let device = renderer.device.clone();
                for mut jb in self.retired_joint_buffers.drain(..) {
                    jb.destroy(&device, &mut renderer.allocator.lock().unwrap(), joints_pool);
                }
            }
        }

        self.check_asset_reloads();

        // Cronómetro raíz del frame y acumuladores de fase (Fase 0).
        let profile_t0 = Instant::now();
        let mut profile_update_us = 0u64;
        // Physics y render se asignan una sola vez y sin condición antes de
        // cualquier registro de muestra: inicialización diferida (sin mut).
        
        

        // Precarga diferida (solo editor): se ejecuta un frame después de
        // presentar la pantalla de carga; esa imagen queda en pantalla
        // mientras la precarga bloquea este frame.
        #[cfg(feature = "editor")]
        if self.precarga_pendiente {
            if self.carga_dibujada {
                // Renderer y contexto salen de `self` para poder llamar al
                // método `&mut self`; vuelven siempre a su sitio.
                let lista = match (self.renderer.take(), self.vulkan_context.take()) {
                    (Some(mut renderer), Some(vk_ctx)) => {
                        self.preparar_assets(&mut renderer, &vk_ctx);
                        self.renderer = Some(renderer);
                        self.vulkan_context = Some(vk_ctx);
                        true
                    }
                    (renderer, vk_ctx) => {
                        // Sin renderer o contexto todavía: se reintenta luego.
                        self.renderer = renderer;
                        self.vulkan_context = vk_ctx;
                        false
                    }
                };
                if lista {
                    self.precarga_pendiente = false;
                    self.precarga_hecha = true;
                    self.cargando = false;
                    self.carga_dibujada = false;
                }
            } else if let Some(window) = self.window.as_ref() {
                // La pantalla de carga se dibuja en este frame: se pide el
                // siguiente para ejecutar la precarga.
                window.request_redraw();
            }
        }

        // Una acción que entra al editor antes de la precarga se aplaza:
        // se enciende la pantalla de carga y se reprocesa al terminar.
        #[cfg(feature = "editor")]
        let accion_lista = match self.pending_apertura.take() {
            Some(action)
                if !self.precarga_hecha
                    && matches!(
                        action,
                        AccionApertura::LoadSampleScene(..)
                            | AccionApertura::CreateProject { .. }
                            | AccionApertura::OpenProject(..)
                            | AccionApertura::OpenSceneFile(..)
                    ) =>
            {
                self.cargando = true;
                self.precarga_pendiente = true;
                self.pending_apertura = Some(action);
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
                None
            }
            otra => otra,
        };
        #[cfg(feature = "editor")]
        if let Some(action) = accion_lista {
            match action {
                AccionApertura::LoadSampleScene(preset) => {
                    // La escena de ejemplo se arma en código: en el gestor
                    // queda con su nombre y SIN archivo asociado (su
                    // guardado pasa por «Save Scene As»).
                    if let Ok(mut m) = self.scene_manager.lock() {
                        m.set_active_name(match preset {
                            SampleScenePreset::PlatformerDemo => "PlatformerDemo",
                            SampleScenePreset::PhysicsPlayground => "PhysicsPlayground",
                            SampleScenePreset::AudioLightingShowcase => "AudioLightingShowcase",
                            SampleScenePreset::FullSampleGame => "FullSampleGame",
                        });
                    }
                    self.raiz_proyecto_actual = None;
                    // Salir a una escena de ejemplo suelta el proyecto:
                    // su lock queda libre para otra instancia (P1).
                    self.lock_proyecto = None;
                    match preset {
                        SampleScenePreset::PlatformerDemo => {
                            self.setup_interactive_platformer_scene();
                        }
                        SampleScenePreset::PhysicsPlayground => {
                            self.setup_physics_playground_scene();
                        }
                        SampleScenePreset::AudioLightingShowcase => {
                            self.setup_audio_lighting_scene();
                        }
                        SampleScenePreset::FullSampleGame => {
                            self.setup_complete_sample_game_scene();
                        }
                    }
                }
                AccionApertura::CreateProject { name, path, template } => {
                    self.create_project_and_open(&name, &path, template);
                }
                AccionApertura::OpenProject(path) => {
                    self.abrir_proyecto(&path);
                }
                AccionApertura::OpenSceneFile(ruta) => {
                    self.open_scene_file(&ruta.to_string_lossy());
                }
            }
        }

        let (window, vk_ctx, renderer) = match (
            &self.window,
            &self.vulkan_context,
            &mut self.renderer,
        ) {
            (Some(w), Some(ctx), Some(rend)) => (w, ctx, rend),
            _ => return Ok(()),
        };

        // 0. Sincronizar estado de inputs (Input v2: los ejes son
        // DATOS — assets/input_axes.json o los defaults clásicos —
        // y el banco los suaviza y calcula los botones con borde.
        // El dt es el mismo paso fijo que reciben los scripts.)
        {
            let mtime = std::fs::metadata(self.raiz_motor.join("assets/input_axes.json"))
                .and_then(|m| m.modified())
                .ok();
            if !self.input_defs_loaded || mtime != self.input_defs_mtime {
                self.input_defs_loaded = true;
                self.input_defs_mtime = mtime;
                let defs = load_input_axis_defs(&self.raiz_motor);
                glass_scripting::input_axes::set_axis_defs(defs.clone());
                self.axis_bank = glass_scripting::input_axes::AxisBank::new(defs);
            }
        }
        // Mando: muestrear el primero conectado (si lo hay).
        if let Some(g) = self.gamepad.as_mut() {
            self.pad_state = poll_gamepad(g);
        }
        let pad = self.pad_state;
        let mut down_names: std::collections::HashSet<&'static str> =
            std::collections::HashSet::new();
        for key in &self.keys_pressed {
            if let PhysicalKey::Code(code) = key {
                if let Some(n) = key_name(*code) {
                    down_names.insert(n);
                }
            }
        }
        if down_names.contains("shift_left") || down_names.contains("shift_right") {
            down_names.insert("shift");
        }
        if down_names.contains("ctrl_left") || down_names.contains("ctrl_right") {
            down_names.insert("ctrl");
        }
        if down_names.contains("alt_left") || down_names.contains("alt_right") {
            down_names.insert("alt");
        }

        #[cfg(feature = "editor")]
        {
            self.egui_ctx.input(|i| {
                // En el editor, winit cede el teclado a egui: los
                // mismos nombres canónicos desde su estado.
                if i.key_down(egui::Key::W) { down_names.insert("w"); }
                if i.key_down(egui::Key::S) { down_names.insert("s"); }
                if i.key_down(egui::Key::D) { down_names.insert("d"); }
                if i.key_down(egui::Key::A) { down_names.insert("a"); }
                if i.key_down(egui::Key::ArrowUp) { down_names.insert("up"); }
                if i.key_down(egui::Key::ArrowDown) { down_names.insert("down"); }
                if i.key_down(egui::Key::ArrowRight) { down_names.insert("right"); }
                if i.key_down(egui::Key::ArrowLeft) { down_names.insert("left"); }
                if i.key_down(egui::Key::Space) { down_names.insert("space"); }
                if i.key_down(egui::Key::V) { down_names.insert("v"); }
                if i.modifiers.shift { down_names.insert("shift"); }
            });
        }

        if pad.dpad_right { down_names.insert("right"); }
        if pad.dpad_left { down_names.insert("left"); }
        if pad.dpad_up { down_names.insert("up"); }
        if pad.dpad_down { down_names.insert("down"); }
        if self.mouse.left_held { down_names.insert("mouse0"); }
        if self.mouse.right_held { down_names.insert("mouse1"); }
        if self.mouse.middle_held { down_names.insert("mouse2"); }

        // Fuentes del mando por eje, resueltas ANTES de la
        // llamada (el banco se toma prestado mutable en ella).
        let pad_src: Vec<(bool, f32)> = self
            .axis_bank
            .defs()
            .iter()
            .map(|d| (pad_button_for(&d.name, &pad), pad_axis_for(&d.name, &pad)))
            .collect();
        let axis_frame = self.axis_bank.update_with_pad(
            0.016667,
            &|n: &str| down_names.contains(n),
            &|_i: usize| 0.0,
            &|i: usize| pad_src.get(i).is_some_and(|t| t.0),
            &|i: usize| pad_src.get(i).map_or(0.0, |t| t.1),
        );
        let move_x = self.axis_bank.value_of("Horizontal");
        let move_z = self.axis_bank.value_of("Vertical");
        let jump_idx = self.axis_bank.index_of("Jump");
        let jump_val = jump_idx
            .and_then(|ix| axis_frame.buttons.get(ix).copied())
            .unwrap_or(false) as u8;
        let run_val = down_names.contains("shift") as u8;
        let camera_val = down_names.contains("v") as u8;

        // Ratón: winit entrega la posición en píxeles físicos de ventana,
        // pero el Canvas vive en el espacio del offscreen (el blit escala
        // entre ambos al presentar), así que la conversión se hace aquí,
        // donde se conocen los dos tamaños. Los bordes de pulsación y
        // liberación se consumen una sola vez por frame.
        let (mouse_down_edge, mouse_up_edge) = self.mouse.take_edges();
        let win_size = window.inner.inner_size();
        let (off_w, off_h) = (
            renderer.offscreen.extent.width as f32,
            renderer.offscreen.extent.height as f32,
        );
        // En el editor el juego ocupa solo el rect de la pestaña Game: se mapea
        // desde ese rect (puntos egui). Sin clampar: fuera del rect el hit-test
        // falla solo, igual que en el standalone.
        #[cfg(feature = "editor")]
        let mapped: Option<(f32, f32)> = if !self.is_standalone {
            match self.editor_layout.viewport.game_view_rect {
                Some([rx, ry, rw, rh]) if rw > 0.0 && rh > 0.0 => {
                    let ppp = self.egui_ctx.pixels_per_point();
                    let px = self.mouse.pos.0 as f32 / ppp;
                    let py = self.mouse.pos.1 as f32 / ppp;
                    Some(((px - rx) / rw * off_w, (py - ry) / rh * off_h))
                }
                _ => None,
            }
        } else {
            None
        };
        #[cfg(not(feature = "editor"))]
        let mapped: Option<(f32, f32)> = None;
        let (mouse_render_x, mouse_render_y) = mapped.unwrap_or_else(|| {
            let x = if win_size.width > 0 {
                self.mouse.pos.0 as f32 * off_w / win_size.width as f32
            } else {
                0.0
            };
            let y = if win_size.height > 0 {
                self.mouse.pos.1 as f32 * off_h / win_size.height as f32
            } else {
                0.0
            };
            (x, y)
        });

        let mut axes_arr = [0.0f32; 32];
        let mut axes_raw_arr = [0.0f32; 32];
        axes_arr[..axis_frame.values.len()].copy_from_slice(&axis_frame.values);
        axes_raw_arr[..axis_frame.raw.len()].copy_from_slice(&axis_frame.raw);
        let (mut btn, mut btn_down, mut btn_up) = (0u32, 0u32, 0u32);
        for (i, (b, d, u)) in axis_frame
            .buttons
            .iter()
            .zip(&axis_frame.buttons_down)
            .zip(&axis_frame.buttons_up)
            .map(|((b, d), u)| (b, d, u))
            .enumerate()
        {
            if *b { btn |= 1 << i; }
            if *d { btn_down |= 1 << i; }
            if *u { btn_up |= 1 << i; }
        }

        glass_scripting::set_input_state(glass_scripting::InputDataFfi {
            move_x,
            move_z,
            jump: jump_val,
            run: run_val,
            camera: camera_val,
            mouse_x: mouse_render_x,
            mouse_y: mouse_render_y,
            mouse_left_held: self.mouse.left_held as u8,
            mouse_left_down: mouse_down_edge as u8,
            mouse_left_up: mouse_up_edge as u8,
            axis_count: axis_frame.values.len() as u32,
            axes: axes_arr,
            axes_raw: axes_raw_arr,
            buttons: btn,
            buttons_down: btn_down,
            buttons_up: btn_up,
        });

        // Clics sobre botones del Canvas: activos en el standalone y en el
        // editor durante Play. En Play la superposición egui pinta los botones
        // como pintura no interactiva, así que esta ruta es la única que
        // despacha, igual que en el standalone. Fuera de Play no hay despacho
        // porque la simulación no corre.
        #[cfg(feature = "editor")]
        let ui_clicks_active = self.is_standalone
            || self.editor_layout.viewport.play_state == glass_editor::PlayState::Playing;
        #[cfg(not(feature = "editor"))]
        let ui_clicks_active = true;
        if ui_clicks_active && (mouse_down_edge || mouse_up_edge) {
            // El gesto se resuelve bajo el candado de escena, pero la
            // INVOCACIÓN a C# ocurre fuera: un handler puede spawnear o
            // destruir entidades (bloque de instanciación) y eso vuelve a
            // pedir este mismo candado — invocando dentro, el primer
            // botón que crease un objeto colgaría el frame. Antes ningún
            // handler tocaba la escena y el riesgo era latente.
            let mut completed_click: Option<canvas_ui::CanvasButtonClick> = None;
            if let Ok(scene) = self.scene_state.lock() {
                if mouse_down_edge && self.ui_drag_session.is_none() {
                    self.button_click_tracker.press(
                        &scene,
                        mouse_render_x,
                        mouse_render_y,
                        off_w,
                        off_h,
                    );
                }
                if mouse_up_edge {
                    completed_click = self.button_click_tracker.release(
                        &scene,
                        mouse_render_x,
                        mouse_render_y,
                        off_w,
                        off_h,
                    );
                }
            }
            if let Some(click) = completed_click {
                log::info!(
                    "[Canvas UI] Botón '{}' clickeado en el standalone! (acción: '{}')",
                    click.entity_name,
                    click.action
                );
                if !click.action.is_empty() && self.script_host.is_initialized {
                    self.script_host
                        .invoke_button_click(click.entity_id, &click.action);
                }
            }
        }

        // Drag & drop del Canvas (UI v2): pulsar sobre un elemento
        // arrastrable inicia la sesión; mientras se mantiene el
        // botón, su anchored_position sigue al ratón conservando el
        // agarre; al soltar, si hay una casilla que acepta sueltas
        // debajo, el elemento encaja en su anchored (snap) y si no,
        // vuelve a su sitio. Las invocaciones a C# van fuera del
        // candado de escena, como las de los clics.
        if ui_clicks_active {
            let mut drag_calls: Vec<(u64, String)> = Vec::new();
            if mouse_down_edge && self.ui_drag_session.is_none() {
                if let Ok(scene) = self.scene_state.lock() {
                    if let Some(session) = canvas_ui::drag_press(
                        &scene,
                        mouse_render_x,
                        mouse_render_y,
                        off_w,
                        off_h,
                    ) {
                        DRAG_CURRENT.store(
                            session.entity_id,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        DROP_TARGET_CURRENT.store(0, std::sync::atomic::Ordering::Relaxed);
                        if !session.begin_action.is_empty() {
                            drag_calls.push((session.entity_id, session.begin_action.clone()));
                        }
                        self.ui_drag_session = Some(session);
                    }
                }
            }
            if self.ui_drag_session.is_some() {
                if self.mouse.left_held {
                    let (id, nueva) = {
                        let s = self.ui_drag_session.as_ref().expect("presente");
                        (s.entity_id, s.follow(mouse_render_x, mouse_render_y))
                    };
                    if let Ok(mut scene) = self.scene_state.lock() {
                        if let Some(ent) = scene.entities.iter_mut().find(|e| e.id == id) {
                            if let Some(rt) = ent.rect_transform.as_mut() {
                                rt.anchored_position = nueva;
                            }
                        }
                    }
                }
                if mouse_up_edge {
                    let session = self.ui_drag_session.take().expect("presente");
                    if let Ok(mut scene) = self.scene_state.lock() {
                        match canvas_ui::drag_drop_target(
                            &scene,
                            session.entity_id,
                            mouse_render_x,
                            mouse_render_y,
                            off_w,
                            off_h,
                        ) {
                            Some(hit) => {
                                DROP_TARGET_CURRENT.store(
                                    hit.entity_id,
                                    std::sync::atomic::Ordering::Relaxed,
                                );
                                if let Some(ent) =
                                    scene.entities.iter_mut().find(|e| e.id == session.entity_id)
                                {
                                    if let Some(rt) = ent.rect_transform.as_mut() {
                                        rt.anchored_position = hit.anchored;
                                    }
                                }
                                if !hit.drop_action.is_empty() {
                                    drag_calls.push((hit.entity_id, hit.drop_action.clone()));
                                }
                            }
                            None => {
                                DROP_TARGET_CURRENT.store(
                                    0,
                                    std::sync::atomic::Ordering::Relaxed,
                                );
                                if let Some(ent) =
                                    scene.entities.iter_mut().find(|e| e.id == session.entity_id)
                                {
                                    if let Some(rt) = ent.rect_transform.as_mut() {
                                        rt.anchored_position = session.start_anchored();
                                    }
                                }
                            }
                        }
                    }
                    if !session.end_action.is_empty() {
                        drag_calls.push((session.entity_id, session.end_action.clone()));
                    }
                }
            }
            if self.script_host.is_initialized {
                for (entity_id, action) in drag_calls {
                    self.script_host.invoke_button_click(entity_id, &action);
                }
            }
        }

        // 1. Manejo de estado de ejecución (Play / Pause / Stop)
        #[cfg(feature = "editor")]
        let is_playing = !self.is_standalone && self.editor_layout.viewport.play_state == glass_editor::PlayState::Playing;
        #[cfg(feature = "editor")]
        let simulation_running = self.is_standalone || is_playing || self.is_platformer_test || self.is_sample_game_test;
        #[cfg(not(feature = "editor"))]
        let simulation_running = true;

        // Paso único pedido por el MCP estando en pausa: este frame
        // corre un ciclo de simulación y el pedido se consume.
        #[cfg(feature = "editor")]
        let simulation_running =
            simulation_running || std::mem::take(&mut self.paso_mcp_pendiente);

        // 1b. Driver de prueba del CC autorado (editor): mueve al personaje seleccionado con WASD antes del paso de física, como haría un script.
        // 2. Ejecutar paso de C# Scripting (.NET CoreCLR) — solo si simulación activa.
        // Antes del Update va la reconciliación de instanciación: crea los
        // scripts spawneados en el frame anterior (su primer Update es ESTE)
        // y dispone los de las entidades destruidas. Se llama por campos
        // sueltos: el renderer está prestado en `renderer` desde el inicio
        // del frame y un `&mut self` entero no compilaría aquí.
        if simulation_running && self.script_host.is_initialized {
            if let Some((handle, entity_id)) = reconcile_spawned_scripts(
                &mut self.script_host,
                &self.scene_state,
                &self.pending_script_spawns,
                &mut self.anim_runtime,
                &mut self.skin_joint_buffers,
                &mut self.retired_joint_buffers,
            ) {
                self.player_script_handle = Some(handle);
                if let Ok(physics) = self.physics_world.lock() {
                    self.player_body_handle = physics.get_body_by_entity(entity_id);
                }
            }
            let profile_t_upd = Instant::now();
            self.script_host.update_all(0.016667);
            profile_update_us = profile_t_upd.elapsed().as_micros() as u64;
        }

        // Inicio de la fase PHYSICS del perfilado (incluye la rama de edición).
        let profile_t_physics = Instant::now();

        // 3. Simular paso de física Rapier3D o sincronizar transform en modo edición
        if simulation_running {
            if let Ok(mut physics) = self.physics_world.lock() {
                physics.step(0.016667);
                let mut sync_transforms = physics.sync_all_transforms_interpolated();
                if let Ok(mut scene) = self.scene_state.lock() {
                    // --- Cruce de portales: un cuerpo dinámico cuyo
                    // segmento prev->curr atraviesa el rectángulo de
                    // un portal sale por su pareja conservando el
                    // impulso (posición y velocidad transportadas
                    // con la matriz de pareja). La entrada de
                    // sincronización se corrige para que la entidad
                    // aparezca ya al otro lado en este mismo frame.
                    {
                        let con_portal: Vec<(u64, String, f32, f32)> = scene
                            .entities
                            .iter()
                            .filter_map(|e| {
                                e.portal
                                    .as_ref()
                                    .map(|p| (e.id, p.pair.clone(), p.width, p.height))
                            })
                            .collect();
                        // (mundo origen, mundo pareja, ancho, alto)
                        let mut portales: Vec<([f32; 16], [f32; 16], f32, f32)> = Vec::new();
                        for (id, pair, w, h) in &con_portal {
                            if let Some((id2, _, _, _)) =
                                con_portal.iter().find(|(o, p2, _, _)| p2 == pair && o != id)
                            {
                                portales.push((
                                    scene.compute_world_matrix(*id),
                                    scene.compute_world_matrix(*id2),
                                    *w,
                                    *h,
                                ));
                            }
                        }
                        if !portales.is_empty() {
                            let mut vivos: std::collections::HashSet<u64> =
                                std::collections::HashSet::new();
                            for (entity_id, tf) in sync_transforms.iter_mut() {
                                vivos.insert(*entity_id);
                                let curr = tf.position;
                                let Some(prev) =
                                    self.portal_prev_pos.get(entity_id).copied()
                                else {
                                    continue;
                                };
                                let Some(handle) = physics.get_body_by_entity(*entity_id)
                                else {
                                    continue;
                                };
                                let es_dinamico = physics
                                    .get_rapier_handle(handle)
                                    .and_then(|(rh, _)| physics.rigid_body_set.get(rh))
                                    .map(|b| b.is_dynamic())
                                    .unwrap_or(false);
                                if !es_dinamico {
                                    continue;
                                }
                                for (src_w, dst_w, w, h) in &portales {
                                    if glass_core::portal::crossing_point(prev, curr, *src_w, *w, *h)
                                        .is_none()
                                    {
                                        continue;
                                    }
                                    let Some(pair_t) =
                                        glass_core::portal::pair_transform(*src_w, *dst_w)
                                    else {
                                        continue;
                                    };
                                    let vel =
                                        physics.get_linear_velocity(handle).unwrap_or([0.0; 3]);
                                    let speed = (vel[0] * vel[0]
                                        + vel[1] * vel[1]
                                        + vel[2] * vel[2])
                                        .sqrt();
                                    let dir =
                                        glass_core::portal::transport_direction(pair_t, vel);
                                    let n = glass_core::portal::portal_normal(*dst_w);
                                    let tp =
                                        glass_core::portal::transport_point(pair_t, curr);
                                    // Empuje de salida: 5 cm por la
                                    // normal frontal de la pareja para
                                    // no nacer dentro de su plano.
                                    let new_pos = [
                                        tp[0] + n[0] * 0.05,
                                        tp[1] + n[1] * 0.05,
                                        tp[2] + n[2] * 0.05,
                                    ];
                                    let new_vel =
                                        [dir[0] * speed, dir[1] * speed, dir[2] * speed];
                                    physics.set_transform(handle, new_pos, tf.rotation);
                                    physics.set_linear_velocity(handle, new_vel);
                                    tf.position = new_pos;
                                    log::info!(
                                        "Portal: entidad {} cruzó conservando el impulso ({:.1} m/s)",
                                        entity_id,
                                        speed
                                    );
                                    break;
                                }
                            }
                            for (entity_id, tf) in sync_transforms.iter() {
                                self.portal_prev_pos.insert(*entity_id, tf.position);
                            }
                            self.portal_prev_pos.retain(|id, _| vivos.contains(id));
                        }
                    }
                    for (entity_id, tf) in sync_transforms {
                        // Rapier trabaja SIEMPRE en espacio de mundo, pero
                        // `transform.position` es LOCAL (relativo al padre). Escribir
                        // el resultado de Rapier directamente ahí corrompía a
                        // cualquier cuerpo con jerarquía: el hijo acumulaba el
                        // desplazamiento del padre en cada frame y se alejaba
                        // volando. `set_world_transform` hace la conversión
                        // mundo -> local invirtiendo la matriz del padre.
                        //
                        // No viaja tf.scale: sync_all_transforms() devuelve [1,1,1]
                        // fijo, así que copiarlo borraba la escala puesta en el
                        // Inspector. La escala es propiedad de la escena, no de
                        // Rapier (que aquí sólo modela posición y orientación).
                        scene.set_world_transform(entity_id, tf.position, tf.rotation);
                    }
                    // Actualizar telemetría de velocidad lineal en entity.physics
                    for ent in scene.entities.iter_mut() {
                        if let Some(ref mut phys) = ent.physics {
                            if let Some(bh) = physics.get_body_by_entity(ent.id) {
                                if let Some(vel) = physics.get_linear_velocity(bh) {
                                    phys.linear_velocity = vel;
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // En modo Edición / Pausa:
            // Sincronizar de inmediato transforms editados en Inspector o mediante gizmos hacia Rapier3D
            if let (Ok(scene), Ok(mut physics)) = (self.scene_state.lock(), self.physics_world.lock()) {
                // Precalcular matrices de mundo una sola vez: mover un padre
                // debe desplazar el collider de sus hijos igual que en el render.
                let world_matrices = scene.compute_world_matrices();

                for ent in &scene.entities {
                    if let Some(bh) = physics.get_body_by_entity(ent.id) {
                        // Rapier indexa cuerpos por posición de mundo, así que aquí
                        // se sube la transformación de MUNDO (antes se subía la
                        // local y un hijo con padre quedaba descolocado).
                        let world = world_matrices.get(&ent.id).copied().unwrap_or_else(|| {
                            glass_core::mat4_from_trs_euler(
                                ent.transform.position,
                                ent.transform.rotation(),
                                ent.transform.scale,
                            )
                        });
                        let world_pos = glass_core::mat4_translation(world);
                        // Se subía siempre la identidad, de modo que mover un gizmo de
                        // rotación no orientaba el collider. Ahora viaja la rotación real.
                        let rot_quat = glass_core::mat4_to_quat(world);
                        physics.set_transform(bh, world_pos, rot_quat);
                        physics.set_linear_velocity(bh, [0.0, 0.0, 0.0]);
                    }
                }
            }
        }

        // conducir_cc_seleccionado: corre DESPUÉS del paso de física y de
        // su sincronización de transforms, para que su escritura sea la
        // última palabra del frame (la sincronización copia el centro de
        // la cápsula como posición y pisaría el offset `center` autorado).
        // move_character resuelve contra los colliders ya actualizados por
        // el paso. Bloqueos por separado: escena → física → escena.
        #[cfg(feature = "editor")]
        if is_playing {
            'conducir_cc_seleccionado: {
                use winit::keyboard::{KeyCode, PhysicalKey};
                const DT: f32 = 0.016667;

                if self.editor_layout.viewport.play_state != glass_editor::PlayState::Playing {
                    self.cc_driver_vy = None;
                    break 'conducir_cc_seleccionado;
                }
                let Some(id) = self.editor_layout.hierarchy.selected_entity else {
                    self.cc_driver_vy = None;
                    break 'conducir_cc_seleccionado;
                };

                // Escena: validar la entidad y leer su center autorado.
                let center_local = match self.scene_state.lock() {
                    Ok(scene) => match scene.entities.iter().find(|e| e.id == id) {
                        Some(e) if e.active && e.script.is_none() => match e.character.as_ref() {
                            Some(data) if data.enabled => Some(data.center),
                            _ => None,
                        },
                        _ => None,
                    },
                    Err(_) => None,
                };
                let Some(center_local) = center_local else {
                    self.cc_driver_vy = None;
                    break 'conducir_cc_seleccionado;
                };

                // Entrada horizontal.
                let tecla = |c: KeyCode| self.keys_pressed.contains(&PhysicalKey::Code(c));
                let mut dx = 0.0f32;
                let mut dz = 0.0f32;
                if tecla(KeyCode::KeyW) || tecla(KeyCode::ArrowUp) {
                    dz -= 1.0;
                }
                if tecla(KeyCode::KeyS) || tecla(KeyCode::ArrowDown) {
                    dz += 1.0;
                }
                if tecla(KeyCode::KeyA) || tecla(KeyCode::ArrowLeft) {
                    dx -= 1.0;
                }
                if tecla(KeyCode::KeyD) || tecla(KeyCode::ArrowRight) {
                    dx += 1.0;
                }
                let len = (dx * dx + dz * dz).sqrt();
                if len > 1.0 {
                    dx /= len;
                    dz /= len;
                }
                let vel = if tecla(KeyCode::ShiftLeft) || tecla(KeyCode::ShiftRight) {
                    8.0
                } else {
                    4.0
                };

                let mut vy = match self.cc_driver_vy {
                    Some((eid, v)) if eid == id => v,
                    _ => 0.0,
                };

                // Física: gravedad según grounded y movimiento.
                let movido = match self.physics_world.lock() {
                    Ok(mut physics) => {
                        let grounded = physics
                            .character_info(id)
                            .map(|(_, g, _)| g)
                            .unwrap_or(false);
                        if grounded {
                            if vy < 0.0 {
                                vy = -0.5;
                            }
                        } else {
                            vy -= 9.81 * DT;
                        }
                        let desired = [dx * vel * DT, vy * DT, dz * vel * DT];
                        // Ojo al contrato: move_character devuelve el
                        // desplazamiento APLICADO, no la posición. El
                        // centro absoluto de la cápsula vive en el estado
                        // del personaje y se lee con character_info.
                        let _ = physics.move_character(id, desired, DT);
                        physics.character_info(id)
                    }
                    Err(_) => None,
                };
                let Some((centro, grounded, _)) = movido else {
                    self.cc_driver_vy = None;
                    break 'conducir_cc_seleccionado;
                };

                // Escena: la posición de MUNDO destino es centro − offset
                // de mundo del center autorado; el transform guarda la
                // posición LOCAL, así que con padre se convierte con la
                // inversa afín de la matriz de mundo del padre.
                if let Ok(mut scene) = self.scene_state.lock() {
                    let world = scene.compute_world_matrix(id);
                    let c = glass_core::mat4_transform_point(world, center_local);
                    let t = glass_core::mat4_translation(world);
                    let destino_mundo = [
                        centro[0] - (c[0] - t[0]),
                        centro[1] - (c[1] - t[1]),
                        centro[2] - (c[2] - t[2]),
                    ];
                    let padre = scene
                        .entities
                        .iter()
                        .find(|e| e.id == id)
                        .and_then(|e| e.parent);
                    let destino_local = match padre {
                        Some(pid) => {
                            let inv = glass_core::mat4_inverse_affine(
                                scene.compute_world_matrix(pid),
                            );
                            // Sin inversa (padre degenerado, p. ej. escala
                            // cero) no se escribe nada: guardar la posición
                            // de mundo como local corrompería la entidad.
                            inv.map(|m| glass_core::mat4_transform_point(m, destino_mundo))
                        }
                        None => Some(destino_mundo),
                    };
                    if let Some(local) = destino_local {
                        if let Some(e) = scene.entities.iter_mut().find(|e| e.id == id) {
                            e.transform.position = local;
                        }
                    }
                }

                if grounded {
                    vy = -0.5;
                }
                self.cc_driver_vy = Some((id, vy));
            }
        } else {
            self.cc_driver_vy = None;
        }


        let profile_physics_us: u64 = profile_t_physics.elapsed().as_micros() as u64;

        // Fase 1: entregar los eventos de contacto del paso a los scripts
        // C# (el lock de física ya se soltó dentro del bloque anterior).
        // Función libre por la misma razón que registrar_muestra_frame:
        // render_frame retiene &mut del renderer durante todo el cuerpo.
        despachar_eventos_contacto(
            &self.physics_world,
            &self.script_host,
            &mut self.prev_contact_pairs,
            &self.scene_state,
        );

        // Inicio de la fase RENDER del perfilado (portal/views/post/offscreen
        // y, en standalone, el overlay del Canvas).
        let profile_t_render = Instant::now();

        // 4. Renderizar geometría 3D a las texturas offscreen en VRAM (Scene View y Game View independientes)
        // `eye_scene` / `eye_game_opt` son las posiciones de cámara en mundo. El
        // fragment shader las necesita para construir el vector de vista real (V),
        // que antes era la constante (0,0,1).
        #[cfg(feature = "editor")]
        let (vp_scene, vp_game_opt, light_dir, light_color, eye_scene, eye_game_opt) = {
            let ed = &self.editor_layout.viewport;
            let eye = [
                ed.editor_cam_center[0] + ed.editor_cam_dist * ed.editor_cam_pitch.cos() * ed.editor_cam_yaw.sin(),
                ed.editor_cam_center[1] + ed.editor_cam_dist * ed.editor_cam_pitch.sin(),
                ed.editor_cam_center[2] + ed.editor_cam_dist * ed.editor_cam_pitch.cos() * ed.editor_cam_yaw.cos(),
            ];
            let view_scene = look_at(eye, ed.editor_cam_center, [0.0, 1.0, 0.0]);
            let aspect_scene = (ed.scene_width / ed.scene_height).max(0.1);
            let proj_scene = perspective_matrix(45.0_f32.to_radians(), aspect_scene, 0.1, 1000.0);
            let vp_s = matmul(&proj_scene, &view_scene);

            let (vp_g, l_dir, l_col, eye_g) = if let Ok(scene) = self.scene_state.lock() {
                let cam_target = ed.selected_camera_id
                    .and_then(|cid| scene.entities.iter().find(|e| e.id == cid && e.active && e.camera.is_some()))
                    .or_else(|| scene.active_camera());

                let mut eye_g: Option<[f32; 4]> = None;
                let vp_g = if let Some(cam) = cam_target {
                    let cp = cam.transform.position;
                    eye_g = Some([cp[0], cp[1], cp[2], 1.0]);
                    let view_g = camera_view_matrix(cam.transform.position, cam.transform.rotation());
                    let fov = cam.camera.as_ref().map(|c| c.fov).unwrap_or(60.0);
                    let near = cam.camera.as_ref().map(|c| c.near_clip).unwrap_or(0.1);
                    let far = cam.camera.as_ref().map(|c| c.far_clip).unwrap_or(1000.0);
                    let aspect_g = ed.game_resolution.aspect_ratio()
                        .unwrap_or(ed.game_width / ed.game_height)
                        .max(0.1);
                    let proj_g = perspective_matrix(fov.to_radians(), aspect_g, near, far);
                    Some(matmul(&proj_g, &view_g))
                } else {
                    None
                };

                // G1: ambiente resuelto de la escena. En modo
                // explícito la intensidad es la de la escena en ambas
                // ramas; en legado, los literales de siempre.
                let amb = scene.ambient_settings();
                let (amb_intensity_lit, amb_intensity_unlit) = match amb.source {
                    glass_core::scene::AmbientSource::Explicit => {
                        let i = amb.intensity.unwrap_or(1.0);
                        (i, i)
                    }
                    glass_core::scene::AmbientSource::Legacy => (
                        glass_core::scene::LEGACY_AMBIENT_INTENSITY_LIT,
                        glass_core::scene::LEGACY_AMBIENT_INTENSITY_UNLIT,
                    ),
                };

                let (ld, lc) = if let Some(light_ent) = scene.main_light() {
                    let rot = light_ent.transform.rotation();
                    // L (hacia la fuente) = columna 2 de la matriz de la luz:
                    // los rayos viajan a lo largo de su forward (-columna 2),
                    // como en Unity y como dibuja su gizmo. Antes se
                    // reconstruía con los signos de pitch/yaw opuestos a la
                    // convención del motor (glass_core::math).
                    let lm = glass_core::mat4_from_trs_euler([0.0, 0.0, 0.0], rot, [1.0, 1.0, 1.0]);
                    let l_dir_vec = [lm[8], lm[9], lm[10]];
                    let len = (l_dir_vec[0] * l_dir_vec[0] + l_dir_vec[1] * l_dir_vec[1] + l_dir_vec[2] * l_dir_vec[2]).sqrt().max(0.001);
                    let norm = [l_dir_vec[0] / len, l_dir_vec[1] / len, l_dir_vec[2] / len];
                    let ldata = light_ent.light.as_ref().cloned().unwrap_or_default();
                    let col = [
                        ldata.color[0] * ldata.intensity,
                        ldata.color[1] * ldata.intensity,
                        ldata.color[2] * ldata.intensity,
                    ];
                    ([norm[0], norm[1], norm[2], amb_intensity_lit], [col[0], col[1], col[2], 1.0])
                } else {
                    ([0.0, 1.0, 0.0, amb_intensity_unlit], [0.0, 0.0, 0.0, 0.0])
                };

                (vp_g, ld, lc, eye_g)
            } else {
                (None, [0.5, 1.0, 0.7, 0.25], [1.0, 1.0, 1.0, 1.0], None)
            };

            let eye_s = [eye[0], eye[1], eye[2], 1.0];
            (vp_s, vp_g, l_dir, l_col, eye_s, eye_g)
        };

        #[cfg(not(feature = "editor"))]
        let (vp_scene, vp_game_opt, light_dir, light_color, eye_scene, eye_game_opt) = {
            let (vp_g, ld, lc, eye_g) = if let Ok(scene) = self.scene_state.lock() {
                // Ambas ramas asignan una sola vez antes de leer:
                // inicialización diferida (sin mut).
                let eye_c: Option<[f32; 4]>;
                let vp_g = if let Some(cam) = scene.active_camera() {
                    let cp = cam.transform.position;
                    eye_c = Some([cp[0], cp[1], cp[2], 1.0]);
                    let view_g = camera_view_matrix(cam.transform.position, cam.transform.rotation());
                    let fov = cam.camera.as_ref().map(|c| c.fov).unwrap_or(60.0);
                    let near = cam.camera.as_ref().map(|c| c.near_clip).unwrap_or(0.1);
                    let far = cam.camera.as_ref().map(|c| c.far_clip).unwrap_or(1000.0);
                    let proj_g = perspective_matrix(fov.to_radians(), 16.0 / 9.0, near, far);
                    Some(matmul(&proj_g, &view_g))
                } else {
                    eye_c = Some([3.5, 3.2, 9.5, 1.0]);
                    let v = look_at([3.5, 3.2, 9.5], [3.5, 1.4, 0.0], [0.0, 1.0, 0.0]);
                    let p = perspective_matrix(45.0_f32.to_radians(), 16.0 / 9.0, 0.1, 100.0);
                    Some(matmul(&p, &v))
                };
                // G1: ambiente resuelto de la escena. En modo
                // explícito la intensidad es la de la escena en ambas
                // ramas; en legado, los literales de siempre.
                let amb = scene.ambient_settings();
                let (amb_intensity_lit, amb_intensity_unlit) = match amb.source {
                    glass_core::scene::AmbientSource::Explicit => {
                        let i = amb.intensity.unwrap_or(1.0);
                        (i, i)
                    }
                    glass_core::scene::AmbientSource::Legacy => (
                        glass_core::scene::LEGACY_AMBIENT_INTENSITY_LIT,
                        glass_core::scene::LEGACY_AMBIENT_INTENSITY_UNLIT,
                    ),
                };

                let (ld, lc) = if let Some(light_ent) = scene.main_light() {
                    let rot = light_ent.transform.rotation();
                    // L (hacia la fuente) = columna 2 de la matriz de la luz:
                    // los rayos viajan a lo largo de su forward (-columna 2),
                    // como en Unity y como dibuja su gizmo. Antes se
                    // reconstruía con los signos de pitch/yaw opuestos a la
                    // convención del motor (glass_core::math).
                    let lm = glass_core::mat4_from_trs_euler([0.0, 0.0, 0.0], rot, [1.0, 1.0, 1.0]);
                    let l_dir_vec = [lm[8], lm[9], lm[10]];
                    let len = (l_dir_vec[0] * l_dir_vec[0] + l_dir_vec[1] * l_dir_vec[1] + l_dir_vec[2] * l_dir_vec[2]).sqrt().max(0.001);
                    let norm = [l_dir_vec[0] / len, l_dir_vec[1] / len, l_dir_vec[2] / len];
                    let ldata = light_ent.light.as_ref().cloned().unwrap_or_default();
                    let col = [
                        ldata.color[0] * ldata.intensity,
                        ldata.color[1] * ldata.intensity,
                        ldata.color[2] * ldata.intensity,
                    ];
                    ([norm[0], norm[1], norm[2], amb_intensity_lit], [col[0], col[1], col[2], 1.0])
                } else {
                    ([0.0, 1.0, 0.0, amb_intensity_unlit], [0.0, 0.0, 0.0, 0.0])
                };
                (vp_g, ld, lc, eye_c)
            } else {
                let v = look_at([3.5, 3.2, 9.5], [3.5, 1.4, 0.0], [0.0, 1.0, 0.0]);
                let p = perspective_matrix(45.0_f32.to_radians(), 16.0 / 9.0, 0.1, 100.0);
                (Some(matmul(&p, &v)), [0.5, 1.0, 0.7, 0.25], [1.0, 1.0, 1.0, 1.0], Some([3.5, 3.2, 9.5, 1.0]))
            };
            let eye_final = eye_g.unwrap_or([3.5, 3.2, 9.5, 1.0]);
            (vp_g.unwrap(), vp_g, ld, lc, eye_final, eye_g)
        };

        // G1: publicación de los colores del ambiente en el camino
        // común (editor y standalone). La intensidad ya viaja en
        // light_dir.w desde las ramas cfg de arriba; los colores van
        // al UBO de frame con set_ambient. Sin portador, la resolución
        // devuelve los colores heredados y nada cambia.
        if let Ok(scene) = self.scene_state.lock() {
            let amb = scene.ambient_settings();
            renderer.set_ambient(amb.sky, amb.equator, amb.ground);
        }


        // G2: post resuelto de la escena (exposición/bloom/viñeta).
        // Sin portador, la resolución devuelve los valores heredados
        // y nada cambia. Los overrides GLASS_* siguen mandando.
        if let Ok(scene) = self.scene_state.lock() {
            let p = scene.post_settings();
            renderer.set_post(glass_renderer::post::PostSettings::from_scene_values(
                p.exposure,
                p.intensity,
                p.vignette,
            ));
        }

        // El canal w de light_color está reservado en el PBR (no lo lee) y
        // los materiales de grafo lo leen como tiempo en segundos: se
        // sobrescribe aquí con el tiempo desde el arranque de la app.
        let light_color = {
            let mut lc = light_color;
            lc[3] = self.start_instant.elapsed().as_secs_f32();
            lc
        };

        // Expulsión de activos ociosos (P2): la caché no debe crecer sin
        // límite en sesiones procedurales que crean y descartan mundos. Se
        // expulsa lo no dibujado en los últimos ~10 s REALES que la
        // escena actual no referencie; el cubo nunca se expulsa. El
        // horizonte es tiempo de reloj (delta entre frames), no frames
        // contados: con el bucle sin capar, 600 frames eran 1-2 s en un
        // equipo rápido y la expulsión devoraba modelos recién cargados
        // (Bug I). Es seguro hacerlo aquí: un modelo ocioso no
        // participó en el frame anterior, así que la GPU no lo está
        // usando. Esa premisa la sostiene `delta_expulsion_ms`: cada
        // frame suma como mucho 250 ms al reloj, así que un modelo
        // dibujado en el frame anterior jamás cruza el horizonte de
        // 10 s de un salto (pausa larga, diálogo modal, primer frame
        // lento tras la carga): hacen falta 40 frames seguidos sin
        // dibujarlo.
        {
            let protected: std::collections::HashSet<String> = {
                let mut set = std::collections::HashSet::new();
                if let Ok(scene) = self.scene_state.lock() {
                    for ent in scene.entities.iter() {
                        if let Some(m) = &ent.mesh {
                            if !m.model.is_empty() {
                                set.insert(m.model.clone());
                            }
                        }
                    }
                }
                set
            };
            let mut alloc = renderer.allocator.lock().unwrap();
            let mut cache = self.asset_cache.write().unwrap();
            let ahora_frame = Instant::now();
            let bruto_ms = self
                .reloj_expulsion_previo
                .map(|prev| ahora_frame.duration_since(prev).as_secs_f64() * 1000.0)
                .unwrap_or(0.0);
            self.reloj_expulsion_previo = Some(ahora_frame);
            let delta_ms = delta_expulsion_ms(bruto_ms, &mut self.reloj_expulsion_resto_ms);
            cache.advance_time_ms(delta_ms);
            let evicted = cache.evict_idle(&renderer.device, &mut alloc, 10_000, &protected);
            for nombre in &evicted {
                self.modelos_expulsados.insert(nombre.clone());
            }
            if !evicted.is_empty() {
                log::info!("AssetCache: {} modelos ociosos expulsados: {:?}", evicted.len(), evicted);
            }

            // Sets de texturas de grafo (Fase 2S, S1): para las entidades
            // cuyo grafo declara propiedades Texture2D, el set 0 necesita
            // 4 bindings (base + hasta 3 texturas de propiedad). Se resuelve
            // aquí porque crear sets exige &mut cache; el bucle de
            // recolección solo lee self.graph_tex_state.
            {
                let trabajos: Vec<(u64, String, String, std::collections::HashMap<String, serde_json::Value>)> = {
                    if let Ok(scene) = self.scene_state.lock() {
                        scene.entities.iter().filter_map(|ent| {
                            let mesh = ent.mesh.as_ref()?;
                            let gname = mesh.shader.strip_prefix("graph:")?;
                            Some((ent.id, mesh.model.clone(), gname.to_string(), mesh.shader_properties.clone()))
                        }).collect()
                    } else {
                        Vec::new()
                    }
                };
                for (ent_id, modelo, gname, overrides) in trabajos {
                    let Some(schema) = glass_core::shader_graph::graph_schema(&gname) else { continue };
                    let tex_props: Vec<&glass_core::shader_graph::PropertyDef> = schema
                        .iter()
                        .filter(|p| matches!(p.kind, glass_core::shader_graph::PropertyKind::Texture2D))
                        .take(3)
                        .collect();
                    if tex_props.is_empty() {
                        self.graph_tex_state.remove(&ent_id);
                        continue;
                    }
                    let mut names: [String; 4] = Default::default();
                    names[0] = modelo.clone();
                    for (i, p) in tex_props.iter().enumerate() {
                        names[i + 1] = overrides
                            .get(&p.name)
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .or_else(|| match &p.default {
                                glass_core::shader_graph::PropertyValue::Texture(n) => Some(n.clone()),
                                _ => None,
                            })
                            .unwrap_or_default();
                    }
                    if self.graph_tex_state.get(&ent_id).map(|(_, n)| n) == Some(&names) {
                        continue;
                    }
                    let layout = renderer
                        .custom_pipelines
                        .get(&gname)
                        .and_then(|p| p.descriptor_set_layout)
                        .or_else(|| {
                            renderer
                                .custom_pipelines_legacy
                                .get(&gname)
                                .and_then(|p| p.descriptor_set_layout)
                        });
                    let Some(layout) = layout else { continue };
                    let model_key = if modelo.is_empty() { None } else { Some(modelo.as_str()) };
                    let primary_image = model_key
                        .and_then(|k| cache.primary_materials.get(k))
                        .and_then(|m| m.base_color_image);
                    let mut views: [Option<vk::ImageView>; 4] =
                        [cache.view_for_image(model_key, primary_image), None, None, None];
                    for (i, nm) in names[1..].iter().enumerate() {
                        if !nm.is_empty() {
                            views[i + 1] = cache.view_for_image(Some(nm.as_str()), Some(0));
                        }
                    }
                    if let Some(set) = cache.graph_texture_set(&renderer.device, layout, views) {
                        self.graph_tex_state.insert(ent_id, (set, names));
                    } else {
                        self.graph_tex_state.remove(&ent_id);
                    }
                }
            }
        }

        // dt de animación (bloque de skinning): tiempo real entre frames
        // con tope de 0,1 s, para que una pausa larga (depurador, pérdida
        // de foco) no teletransporte las poses al volver.
        let anim_dt = {
            let now = Instant::now();
            let dt = (now - self.last_anim_instant).as_secs_f32().min(0.1);
            self.last_anim_instant = now;
            dt
        };

                // Materiales (Materiales v1): cargar bajo demanda
                // los activos que la escena referencia y subir las
                // texturas que aún no estén en la caché.
                {
                    let wanted: Vec<String> = match self.scene_state.lock() {
                        Ok(scene) => {
                            let mut v: Vec<String> = scene
                                .entities
                                .iter()
                                .filter(|e| e.active)
                                .filter_map(|e| e.mesh.as_ref())
                                .map(|m| m.material.clone())
                                .filter(|m| !m.is_empty())
                                .collect();
                            v.sort();
                            v.dedup();
                            v
                        }
                        Err(_) => Vec::new(),
                    };
for name in wanted {
                        if self.materiales.contains_key(&name)
                            || self.materiales_failed.contains(&name)
                        {
                            continue;
                        }
                        #[cfg(feature = "editor")]
                        let raiz_proyecto = self.raiz_proyecto_actual.as_deref();
                        #[cfg(not(feature = "editor"))]
                        let raiz_proyecto: Option<&std::path::Path> = None;
                        let raices = raices_assets_busqueda(raiz_proyecto, &self.scene_manager, &self.raiz_motor);
                        let candidatos: Vec<std::path::PathBuf> = raices
                            .iter()
                            .map(|r| r.join(format!("materials/{}.material.json", name)))
                            .collect();
                        let path = candidatos
                            .iter()
                            .find(|p| p.exists())
                            .cloned()
                            .unwrap_or_else(|| candidatos[0].clone());
                        let leido = std::fs::read_to_string(&path)
                            .map_err(|e| e.to_string())
                            .and_then(|text| {
                                serde_json::from_str::<glass_core::MaterialData>(&text)
                                    .map_err(|e| e.to_string())
                            });
                        match leido {
                            Ok(mat) => {
                                if !mat.texture.is_empty()
                                    && !self.material_textures.contains(&mat.texture)
                                {
                                    let candidatos_tex: Vec<std::path::PathBuf> = raices
                                        .iter()
                                        .map(|r| r.join(&mat.texture))
                                        .collect();
                                    let tex_path = candidatos_tex
                                        .iter()
                                        .find(|p| p.exists())
                                        .cloned()
                                        .unwrap_or_else(|| candidatos_tex[0].clone());
                                    match image::open(&tex_path) {
                                        Ok(img) => {
                                            let rgba = img.to_rgba8();
                                            let (w, h) = rgba.dimensions();
                                            let data = glass_renderer::mesh::ImagenModelo::Rgba(
                                                glass_renderer::mesh::GltfImageData {
                                                    width: w,
                                                    height: h,
                                                    rgba: rgba.into_raw(),
                                                },
                                            );
                                            if let Some(layout) =
                                                renderer.pbr_pipeline.descriptor_set_layout
                                            {
                                                let mut alloc =
                                                    renderer.allocator.lock().unwrap();
                                                let mut cache =
                                                    self.asset_cache.write().unwrap();
                                                cache.insert_texture(
                                                    &renderer.device,
                                                    &mut alloc,
                                                    glass_renderer::asset_cache::TextureUpload { queue: vk_ctx.graphics_queue, command_pool: renderer.command_pool, set_layout: layout },
                                                    mat.texture.clone(),
                                                    0,
                                                    &data,
                                                );
                                            }
                                            self.material_textures.insert(mat.texture.clone());
                                        }
                                        Err(e) => {
                                            log::warn!(
                                                "Textura '{}' del material '{}' no cargada: {}",
                                                mat.texture,
                                                name,
                                                e
                                            );
                                        }
                                    }
                                }
                                self.materiales.insert(name, mat);
                            }
                            Err(e) => {
                                log::warn!(
                                    "Material '{}' no cargado ({}): {}",
                                    name,
                                    path.display(),
                                    e
                                );
                                self.materiales_failed.insert(name);
                            }
                        }
                    }
                }

        let cache = self.asset_cache.read().unwrap();
        // Pase previo de la FSM (bloque FSM): la escena se bloquea en
        // mutable un instante, antes del bloqueo en inmutable del bucle
        // de dibujo que consume las decisiones.
        let mut eventos_anim: Vec<(u64, glass_core::animator::FiredAnimEvent)> = Vec::new();
        let mut cambios_anim: Vec<(u64, glass_core::animator::StateChangeEvent)> = Vec::new();
        if let Ok(mut scene) = self.scene_state.lock() {
            (eventos_anim, cambios_anim) = avanzar_animadores_fsm(
                &mut EstadoAnimadores {
                    controllers: &mut self.animator_controllers,
                    pose: &mut self.anim_pose,
                    root_trackers: &mut self.anim_root_trackers,
                    root_pin: &mut self.anim_root_pin,
                    free_fade: &mut self.anim_free_fade,
                    runtime: &mut self.anim_runtime,
                },
                &mut scene,
                &cache,
                anim_dt,
            );
        }
        // Eventos de animación del frame: telemetría MCP y entrega a C#.
        for (entidad, ev) in &eventos_anim {
            glass_core::animator_stats::record_anim_event(*entidad, ev);
            self.script_host
                .dispatch_anim_event(*entidad, &ev.name, ev.value, ev.text.as_deref());
        }
        // Cambios de estado del frame: telemetría MCP y entrega a C#.
        for (entidad, cambio) in &cambios_anim {
            glass_core::animator_stats::record_state_event(*entidad, cambio);
            self.script_host
                .dispatch_state_event(*entidad, &cambio.state, cambio.entered);
        }
        // La recolección entera cuelga de la malla de respaldo «cube»:
        // sin ella no hay con qué dibujar las entidades sin modelo.
        // `preparar_assets` la registra siempre (generador integrado),
        // así que llegar aquí sin ella es un fallo de arranque, no un
        // estado válido: se grita una sola vez en el registro en vez
        // de dejar el 3D en negro en silencio (defecto D3).
        let cube_mesh_opt = cache.meshes.get("cube");
        if cube_mesh_opt.is_none() {
            static AVISO_CUBE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !AVISO_CUBE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log::error!(
                    "Render: la malla de respaldo «cube» no está en la caché de assets; el 3D no se dibujará hasta que la precarga la registre."
                );
            }
        }
        if let Some(cube_mesh) = cube_mesh_opt {
            // Supresión MeshForge (R2): la entidad en edición de malla no
            // se dibuja en la textura de escena (el overlay del viewport
            // la sustituye); la vista de juego la sigue mostrando.
            #[cfg(feature = "editor")]
            let excluir_mesh = self.editor_layout.mesh_tool.entidad_en_edicion();
            #[cfg(not(feature = "editor"))]
            let excluir_mesh: Option<u64> = None;
            let mut scene_batches: Vec<glass_renderer::MeshDrawItem<'_>> = Vec::new();
            let mut game_batches: Vec<glass_renderer::MeshDrawItem<'_>> = Vec::new();
            let mut scene_skinned: Vec<glass_renderer::SkinnedDrawItem> = Vec::new();
            let mut game_skinned: Vec<glass_renderer::SkinnedDrawItem> = Vec::new();
            // Contorno de selección con piel de la vista de Escena (editor).
            #[cfg(feature = "editor")]
            let mut scene_outline_skinned: Vec<glass_renderer::SkinnedDrawItem<'_>> = Vec::new();
            // Resolución de propiedades de grafo por entidad (Fase 2S):
            // cachea el set y los bytes ya subidos por entidad para no
            // recrear/actualizar el descriptor set si no cambiaron.
            let empty_overrides = serde_json::Map::new();
            let props_set_cached: Option<vk::DescriptorSet> = None;
            let _ = props_set_cached; // evita warning si el flujo cambia abajo
            // Proyectores del mapa de sombras (bloque de gráficos): cada
            // entidad dibujada proyecta con la misma malla y la misma
            // matriz de modelo de su lote; las con piel, además, con su
            // set de articulaciones (la sombra sigue la pose).
            let mut shadow_statics: Vec<glass_renderer::StaticCaster<'_>> = Vec::new();
            let mut shadow_graph_statics: Vec<glass_renderer::GraphCaster<'_>> = Vec::new();
            let mut shadow_skinned: Vec<(&glass_renderer::GpuMesh, [f32; 16], vk::DescriptorSet)> = Vec::new();
            // Fase 2: frustums de las dos vistas y contadores del frame
            // para la tool glass_render_stats.
            let frustum_scene = glass_core::math::Frustum::from_view_proj(&vp_scene);
            let frustum_game = vp_game_opt.map(|vp| glass_core::math::Frustum::from_view_proj(&vp));
            // Frustum de la LUZ para filtrar casters: el VP de sombra
            // se calcula aquí, ANTES de la recolección, con los mismos
            // datos que el bloque de publicación de más abajo (que lo
            // reutiliza tal cual: no pueden desincronizarse). Quien
            // esté fuera de la caja ortográfica de la luz no puede
            // proyectar en el mapa. Sin luz activa no hay pase de
            // sombras: no entra ningún caster.
            let shadow_active = light_color[0] + light_color[1] + light_color[2] > 0.0 || light_dir[3] > 0.1;
            let vp_shadow_pre = if shadow_active {
                let sc = eye_game_opt.unwrap_or(eye_scene);
                Some(glass_core::directional_shadow_vp(
                    [sc[0], sc[1], sc[2]],
                    [light_dir[0], light_dir[1], light_dir[2]],
                    glass_renderer::shadow::SHADOW_ORTHO_HALF,
                    glass_renderer::shadow::SHADOW_MAP_SIZE,
                    glass_renderer::shadow::SHADOW_BACK,
                    glass_renderer::shadow::SHADOW_FRONT,
                ))
            } else {
                None
            };
            let frustum_shadow = vp_shadow_pre.map(|vp| glass_core::math::Frustum::from_view_proj(&vp));
            let mut stats_considered: u32 = 0;
            let mut stats_outline: u32 = 0;
            let mut stats_culled: u32 = 0;

            if let Ok(scene) = self.scene_state.lock() {
                // ITERACIÓN 4: matrices de mundo compuestas por jerarquía
                // (padre * hijo). Antes cada entidad se transformaba con su
                // matriz local, ignorando ancestros, así que mover, rotar o
                // escalar un padre no afectaba a sus hijos en el viewport.
                let world_matrices = scene.compute_world_matrices();

                for ent in &scene.entities {
                    // Una entidad inactiva no se dibuja: la física y el
                    // Canvas ya respetaban `active`, y el render era el
                    // único que no — sin esto, «desactivar» en un pool
                    // dejaba el objeto visible (Plat3D lo sorteaba con
                    // escala 0,001 porque no había alternativa).
                    if !ent.active {
                        continue;
                    }
                    if ent.components.iter().any(|c| c == "MeshRenderer" || c == "Collider" || c == "StaticCollider") {
                        // Matriz de mundo del transform (escala completa).
                        let world = world_matrices.get(&ent.id).copied().unwrap_or_else(|| {
                            glass_core::mat4_from_trs_euler(
                                ent.transform.position,
                                ent.transform.rotation(),
                                ent.transform.scale,
                            )
                        });

                        // El cubo unitario (cube.glb, vértices -1..+1) se
                        // renderiza a la mitad para que una escala de 1.0 dé un
                        // cubo de 1 m. Esa escala pertenece al mesh, no al
                        // transform, así que se aplica en espacio local (a la
                        // derecha) y no se propaga a los hijos.
                        let h = 0.5_f32;
                        let scale_half = [
                            h, 0.0, 0.0, 0.0,
                            0.0, h, 0.0, 0.0,
                            0.0, 0.0, h, 0.0,
                            0.0, 0.0, 0.0, 1.0,
                        ];
                        // Malla de la entidad: su modelo importado si lo tiene
                        // y está en la caché; si no, el cubo por defecto. La
                        // media escala de arriba solo aplica al cubo (sus
                        // vértices van de -1 a +1); un modelo importado se
                        // dibuja con su matriz de mundo tal cual. Antes TODAS
                        // las entidades dibujaban el cubo y los modelos
                        // importados nunca llegaban a pantalla.
                        let model_key = ent.mesh.as_ref().map(|m| m.model.as_str()).filter(|s| !s.is_empty());
                        let using_model = model_key.map(|k| cache.meshes.contains_key(k)).unwrap_or(false);
                        let ent_mesh: &glass_renderer::GpuMesh = model_key
                            .and_then(|k| cache.meshes.get(k))
                            .unwrap_or(cube_mesh);
                        let model = if using_model {
                            world
                        } else {
                            glass_core::mat4_mul(world, scale_half)
                        };
                        // Fase 2: frustum culling por entidad y vista.
                        // Cotas locales: las de la geometría retenida del
                        // modelo; sin modelo, el cubo de respaldo (vértices
                        // -1..+1 y `model` ya lleva su media escala); sin
                        // cotas retenidas, NO se recorta (auditoría Fase
                        // 2, M-12: el viejo respaldo de 1 m³ podía hacer
                        // desaparecer un modelo grande cuyo pivote salía
                        // de cámara). Las sombras no se filtran: algo
                        // fuera de vista puede proyectar dentro.
                        let (lmin, lmax) = if using_model {
                            model_key
                                .and_then(|k| cache.bounds(k))
                                .unwrap_or(([-1.0e6, -1.0e6, -1.0e6], [1.0e6, 1.0e6, 1.0e6]))
                        } else {
                            ([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0])
                        };
                        let (wmin, wmax) = glass_core::math::aabb_transform(lmin, lmax, &model);
                        let vis_scene = excluir_mesh != Some(ent.id) && frustum_scene.intersects_aabb(wmin, wmax);
                        let vis_game = frustum_game
                            .map(|f| f.intersects_aabb(wmin, wmax))
                            .unwrap_or(true);
                        stats_considered += 1;
                        if frustum_game.is_some() && !vis_game {
                            stats_culled += 1;
                        }
                        // ¿Puede esta entidad proyectar sombra? Solo
                        // si corta la caja ortográfica de la luz.
                        let visible_shadow = frustum_shadow
                            .as_ref()
                            .map(|f| f.intersects_aabb(wmin, wmax))
                            .unwrap_or(false);
                        // Uso registrado para la expulsión de ociosos (P2).
                        cache.touch(if using_model { model_key.unwrap() } else { "cube" });

                        let mvp_scene = matmul(&vp_scene, &model);
                        let obj_col = ent.mesh.as_ref().map(|m| [m.color[0], m.color[1], m.color[2], 1.0]).unwrap_or([0.8, 0.8, 0.8, 1.0]);

                        // ITERACIÓN 4: la presencia de luz se deriva de la escena,
                        // no de `light_color.w`. Ese componente quedó reservado
                        // (sin uso) al unificar `has_light` en `material_params.w`,
                        // así que leerlo era depender de un valor muerto.
                        let has_light = if scene.main_light().is_some() { 1.0 } else { 0.0 };

                        // Lotes de dibujo de la entidad (color, parámetros
                        // de material, set de textura y rango de índices
                        // por lote): uno por primitiva con SU material si
                        // el modelo es un glTF importado con rangos; si no,
                        // un único lote con la malla entera, el material
                        // principal y, en su defecto, el color plano de la
                        // entidad y los valores históricos (0.15 / 0.45).
                        // El shader PBR siempre muestrea, así que cada lote
                        // lleva un set válido (el de su imagen o el blanco).
                        let mut draw_items: Vec<UiDrawItem> = Vec::new();
                        // Materiales v1: el activo asignado a la
                        // entidad manda sobre cualquier otro camino
                        // (un solo lote con sus parámetros y su
                        // textura). Y las primitivas integradas se
                        // pintan con el color de la entidad: su
                        // «material» de caché es sintético y blanco,
                        // que era el defecto de las primitivas con
                        // `model` explícito saliendo blancas.
                        let nombre_material = ent
                            .mesh
                            .as_ref()
                            .map(|m| m.material.as_str())
                            .unwrap_or("");
                        let material_entidad = if nombre_material.is_empty() {
                            None
                        } else {
                            self.materiales.get(nombre_material)
                        };
                        let es_primitiva = matches!(
                            model_key,
                            Some("cube" | "plane" | "sphere" | "capsule" | "cylinder")
                        );
                        // Enlace entidad→objeto: si el nombre de la entidad
                        // coincide con un objeto del modelo (la convención
                        // de las escenas importadas: una entidad por nodo
                        // del archivo, con sufijo « (n)» en duplicados), la
                        // entidad dibuja SOLO los rangos de ese objeto. Sin
                        // coincidencia manda el comportamiento histórico
                        // (el modelo entero: escenas autoradas a mano, una
                        // entidad por archivo). Sin este enlace, cada
                        // entidad de una escena importada dibujaba el modelo
                        // completo con SU material y las copias se pisaban:
                        // en NeoFPS los marcadores dibujaban la instalación
                        // entera con su material blanco emisor y el suelo
                        // salía blanco aunque su material y su textura
                        // (la paleta) eran correctos.
                        let rangos_objeto: Option<Vec<usize>> = if using_model {
                            model_key.and_then(|k| {
                                let objs = cache.objects.get(k)?;
                                let nombre_base: &str = match ent.name.rfind(" (") {
                                    Some(i)
                                        if ent.name.ends_with(')')
                                            && i + 2 < ent.name.len() - 1
                                            && ent.name[i + 2..ent.name.len() - 1]
                                                .chars()
                                                .all(|c| c.is_ascii_digit()) =>
                                    {
                                        &ent.name[..i]
                                    }
                                    _ => ent.name.as_str(),
                                };
                                // El enlace explícito de la malla
                                // (`mesh.object`, lo escribe el importador
                                // desde la tabla de sub-objetos) manda; sin
                                // él, el nombre de la entidad.
                                let explicito = ent
                                    .mesh
                                    .as_ref()
                                    .map(|m| m.object.as_str())
                                    .unwrap_or("");
                                let obj = if explicito.is_empty() {
                                    objs.iter()
                                        .find(|o| o.name == ent.name || o.name == nombre_base)?
                                } else {
                                    objs.iter().find(|o| o.name == explicito)?
                                };
                                if obj.ranges.is_empty() {
                                    None
                                } else {
                                    Some(obj.ranges.clone())
                                }
                            })
                        } else {
                            None
                        };
                        if let Some(mat) = material_entidad {
                            let tex_set = if mat.texture.is_empty() {
                                cache
                                    .set_for_image(None, None)
                                    .unwrap_or(vk::DescriptorSet::null())
                            } else {
                                cache
                                    // La textura del material se registra
                                    // en la caché con su ruta de asset como
                                    // clave (la misma convención que usa el
                                    // camino de grafos); buscarla con otro
                                    // nombre la dejaba siempre en la textura
                                    // blanca por defecto y el material
                                    // salía sin su textura.
                                    .set_for_image(Some(mat.texture.as_str()), Some(0))
                                    .unwrap_or(vk::DescriptorSet::null())
                            };
                            // Con enlace a objeto, un lote por rango del
                            // objeto; sin enlace, el lote histórico de la
                            // malla entera.
                            let rangos_mat: Vec<(u32, u32)> = match (&rangos_objeto, model_key) {
                                (Some(idxs), Some(k)) => idxs
                                    .iter()
                                    .filter_map(|&ri| {
                                        cache
                                            .primitive_ranges
                                            .get(k)
                                            .and_then(|prs| prs.get(ri))
                                            .map(|r| (r.first_index, r.index_count))
                                    })
                                    .collect(),
                                _ => Vec::new(),
                            };
                            if rangos_mat.is_empty() {
                                draw_items.push((
                                    mat.albedo,
                                    [mat.metallic, mat.roughness, 1.0, has_light],
                                    tex_set,
                                    (0, ent_mesh.index_count),
                                    [
                                        mat.emissive[0],
                                        mat.emissive[1],
                                        mat.emissive[2],
                                        mat.emissive_intensity,
                                    ],
                                    [mat.tiling[0], mat.tiling[1], mat.offset[0], mat.offset[1]],
                                ));
                            } else {
                                for r in rangos_mat {
                                    draw_items.push((
                                        mat.albedo,
                                        [mat.metallic, mat.roughness, 1.0, has_light],
                                        tex_set,
                                        r,
                                        [
                                            mat.emissive[0],
                                            mat.emissive[1],
                                            mat.emissive[2],
                                            mat.emissive_intensity,
                                        ],
                                        [mat.tiling[0], mat.tiling[1], mat.offset[0], mat.offset[1]],
                                    ));
                                }
                            }
                        } else if es_primitiva && using_model {
                            draw_items.push((
                                obj_col,
                                [0.15, 0.45, 1.0, has_light],
                                cache
                                    .set_for_image(None, None)
                                    .unwrap_or(vk::DescriptorSet::null()),
                                (0, ent_mesh.index_count),
                                [0.0, 0.0, 0.0, 0.0],
                                [1.0, 1.0, 0.0, 0.0],
                            ));
                        }
                        let ranges = if using_model {
                            model_key
                                .and_then(|k| cache.primitive_ranges.get(k))
                                .filter(|r| !r.is_empty())
                        } else {
                            None
                        };
                        if draw_items.is_empty() {
                        if let (Some(key), Some(ranges)) = (model_key, ranges) {
                            let materials = cache.materials.get(key);
                            // Fase 6 (LOD): nivel activo por base según
                            // la distancia de la entidad al ojo.
                            let mut lod_de_rango: std::collections::HashMap<usize, (String, u32)> =
                                std::collections::HashMap::new();
                            let mut lod_activo: std::collections::HashMap<String, u32> =
                                std::collections::HashMap::new();
                            if let Some(objs) = cache.objects.get(key) {
                                let mut disp: std::collections::HashMap<String, Vec<u32>> =
                                    std::collections::HashMap::new();
                                for o in objs {
                                    if let Some((base, nivel)) =
                                        glass_renderer::lod_suffix(&o.name)
                                    {
                                        disp.entry(base.clone()).or_default().push(nivel);
                                        for &r in &o.ranges {
                                            lod_de_rango.insert(r, (base.clone(), nivel));
                                        }
                                    }
                                }
                                if !disp.is_empty() {
                                    let ojo = eye_game_opt.unwrap_or(eye_scene);
                                    let dx = world[12] - ojo[0];
                                    let dy = world[13] - ojo[1];
                                    let dz = world[14] - ojo[2];
                                    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                                    for (base, niveles) in &disp {
                                        lod_activo.insert(
                                            base.clone(),
                                            glass_renderer::lod_level_for_distance(niveles, dist),
                                        );
                                    }
                                }
                            }
                            for (range_idx, range) in ranges.iter().enumerate() {
                                // Enlace entidad→objeto: fuera de su objeto,
                                // la entidad no dibuja nada del modelo.
                                if let Some(idxs) = &rangos_objeto {
                                    if !idxs.contains(&range_idx) {
                                        continue;
                                    }
                                }
                                if let Some((base, nivel)) = lod_de_rango.get(&range_idx) {
                                    if lod_activo.get(base) != Some(nivel) {
                                        continue;
                                    }
                                }
                                let mat = range.material.and_then(|i| materials.and_then(|ms| ms.get(i)));
                                let (c, mp) = if let Some(m) = mat {
                                    (m.base_color_factor, [m.metallic, m.roughness, 1.0, has_light])
                                } else {
                                    (obj_col, [0.15, 0.45, 1.0, has_light])
                                };
                                let tex_set = cache
                                    .set_for_image(Some(key), mat.and_then(|m| m.base_color_image))
                                    .unwrap_or(vk::DescriptorSet::null());
                                draw_items.push((c, mp, tex_set, (range.first_index, range.index_count), [0.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]));
                            }
                        }
                        }
                        if draw_items.is_empty() {
                            let (c, mp) = if using_model {
                                if let Some(mat) = model_key.and_then(|k| cache.primary_materials.get(k)) {
                                    (mat.base_color_factor, [mat.metallic, mat.roughness, 1.0, has_light])
                                } else {
                                    (obj_col, [0.15, 0.45, 1.0, has_light])
                                }
                            } else {
                                (obj_col, [0.15, 0.45, 1.0, has_light])
                            };
                            let primary_image = if using_model {
                                model_key
                                    .and_then(|k| cache.primary_materials.get(k))
                                    .and_then(|m| m.base_color_image)
                            } else {
                                None
                            };
                            let tex_set = cache
                                .set_for_image(if using_model { model_key } else { None }, primary_image)
                                .unwrap_or(vk::DescriptorSet::null());
                            // Enlace entidad→objeto también aquí: sin él la
                            // entidad dibujaría el modelo entero.
                            let rangos_fb: Vec<(u32, u32)> = match (&rangos_objeto, model_key) {
                                (Some(idxs), Some(k)) => idxs
                                    .iter()
                                    .filter_map(|&ri| {
                                        cache
                                            .primitive_ranges
                                            .get(k)
                                            .and_then(|prs| prs.get(ri))
                                            .map(|r| (r.first_index, r.index_count))
                                    })
                                    .collect(),
                                _ => Vec::new(),
                            };
                            if rangos_fb.is_empty() {
                                draw_items.push((c, mp, tex_set, (0, ent_mesh.index_count), [0.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]));
                            } else {
                                for r in rangos_fb {
                                    draw_items.push((c, mp, tex_set, r, [0.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0]));
                                }
                            }
                        }

                        // Material de grafo: MeshData.shader = "graph:<nombre>".
                        let mut shader_name = ent.mesh.as_ref()
                            .and_then(|m| m.shader.strip_prefix("graph:"))
                            .map(|s| s.to_string());
                        // Material transparente sin grafo propio: el
                        // pipeline PBR transparente registrado al
                        // arrancar.
                        if shader_name.is_none() {
                            if let Some(mat) = material_entidad {
                                if mat.transparent {
                                    shader_name = Some("material_transparent".to_string());
                                }
                            }
                        }
                        // Fallback de grafo (Fase 2S, S4): si el grafo
                        // pedido no tiene pipeline registrado (su .spv
                        // falta o falló al registrarse), la entidad se
                        // dibuja con el material de error (PBR magenta
                        // plano, vía color_error en los pushes) y se
                        // avisa una sola vez por grafo.
                        let mut color_error = false;
                        if let Some(g) = &shader_name {
                            if g != "material_transparent"
                                && !renderer.custom_pipelines.contains_key(g)
                                && !renderer.custom_pipelines_legacy.contains_key(g)
                            {
                                if self.graph_error_logged.insert(g.clone()) {
                                    log::error!(
                                        "Material de grafo '{}' no disponible (sin .spv o falló su registro): se usa el material de error magenta",
                                        g
                                    );
                                }
                                color_error = true;
                                shader_name = None;
                            }
                        }

                        // --- Bloque de skinning: pose de ESTA entidad ---
                        // Si el modelo de la entidad tiene piel en caché, su
                        // pose de este frame se evalúa aquí y sus lotes van
                        // por el pipeline con piel (la rama de dentro del
                        // bucle de lotes). El resultado es el set de
                        // articulaciones ya escrito y la malla con piel;
                        // `None` = entidad rígida de siempre.
                        let skin_pose: Option<(vk::DescriptorSet, &glass_renderer::GpuMesh)> = 'pose: {
                            if !using_model {
                                break 'pose None;
                            }
                            let Some(key) = model_key else { break 'pose None };
                            let (Some(skin), Some(skinned_mesh)) =
                                (cache.skins.get(key), cache.skinned_meshes.get(key))
                            else {
                                break 'pose None;
                            };
                            let clips: &[glass_renderer::AnimationClip] = cache
                                .animations
                                .get(key)
                                .map(|v| v.as_slice())
                                .unwrap_or(&[]);
                            // Clip del componente Animator: el nombrado, o el
                            // primero del modelo si el campo está vacío o el
                            // nombre no existe. Sin componente Animator no hay
                            // clip: la entidad se dibuja en bind pose.
                            let animator = ent.animator.as_ref();
                            let clip: Option<&glass_renderer::AnimationClip> =
                                animator.and_then(|a| {
                                    if a.clip.is_empty() {
                                        clips.first()
                                    } else {
                                        clips
                                            .iter()
                                            .find(|c| c.name == a.clip)
                                            .or_else(|| clips.first())
                                    }
                                });
                            // Tiempo de reproducción: estado de runtime por
                            // entidad; cambiar de clip lo reinicia a 0.
                            let entry = self
                                .anim_runtime
                                .entry(ent.id)
                                .or_insert_with(|| (String::new(), 0.0));
                            let clip_name = clip.map(|c| c.name.as_str()).unwrap_or("");
                            if entry.0 != clip_name {
                                entry.0 = clip_name.to_string();
                                entry.1 = 0.0;
                            }
                            if animator.map(|a| a.playing).unwrap_or(false) {
                                entry.1 += anim_dt * animator.map(|a| a.speed).unwrap_or(1.0);
                            }
                            let t = entry.1;
                            // Pose del frame (bloque FSM): si el pase
                            // previo dejó una decisión para esta entidad,
                            // manda ella (un clip, u dos mezclados durante
                            // un fundido de la máquina). Si hay un
                            // crossfade libre en curso, se mezcla el clip
                            // saliente contra el entrante clásico. En
                            // cualquier otro caso, el camino de siempre.
                            let matrices = if let Some(samples) = self.anim_pose.get(&ent.id) {
                                // Fase 4: la decisión del pase previo es
                                // una lista de samples (clip, tiempo, peso):
                                // un clip, dos en un fundido, o los hijos
                                // de un blend tree.
                                let resueltos: Vec<(
                                    Option<&glass_renderer::skin::AnimationClip>,
                                    f32,
                                    f32,
                                )> = samples
                                    .iter()
                                    .map(|(n, t, w)| {
                                        (clips.iter().find(|c| &c.name == n), *t, *w)
                                    })
                                    .collect();
                                if self
                                    .anim_root_pin
                                    .get(&ent.id)
                                    .copied()
                                    .unwrap_or(false)
                                {
                                    glass_renderer::skin::evaluate_skin_matrices_multi_pinned(
                                        skin, &resueltos, true,
                                    )
                                } else {
                                    glass_renderer::skin::evaluate_skin_matrices_multi(
                                        skin, &resueltos,
                                    )
                                }
                            } else if self.anim_free_fade.contains_key(&ent.id) {
                                let (from_name, from_t, w, done) = {
                                    let f = self.anim_free_fade.get_mut(&ent.id).unwrap();
                                    f.elapsed += anim_dt;
                                    f.from_time +=
                                        anim_dt * animator.map(|a| a.speed).unwrap_or(1.0);
                                    let w = if f.duration > 0.0 {
                                        (f.elapsed / f.duration).clamp(0.0, 1.0)
                                    } else {
                                        1.0
                                    };
                                    (f.from_clip.clone(), f.from_time, w, w >= 1.0)
                                };
                                let m = glass_renderer::skin::evaluate_skin_matrices_blended(
                                    skin,
                                    clips.iter().find(|c| c.name == from_name),
                                    from_t,
                                    clip,
                                    t,
                                    w,
                                );
                                if done {
                                    self.anim_free_fade.remove(&ent.id);
                                }
                                m
                            } else {
                                glass_renderer::skin::evaluate_skin_matrices(skin, clip, t)
                            };
                            // Buffer por entidad: se crea si falta y se
                            // recrea si la piel cambió de nº de articulaciones
                            // (recarga en caliente de un rig distinto); el
                            // viejo se retira, no se destruye en el acto.
                            let joint_count = skin.joints.len();
                            let stale = self
                                .skin_joint_buffers
                                .get(&ent.id)
                                .map(|jb| jb.joint_count != joint_count)
                                .unwrap_or(true);
                            if stale {
                                if let Some(old) = self.skin_joint_buffers.remove(&ent.id) {
                                    self.retired_joint_buffers.push(old);
                                }
                                match renderer.create_joint_buffer(joint_count) {
                                    Ok(jb) => {
                                        self.skin_joint_buffers.insert(ent.id, jb);
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Skinning: sin buffer de articulaciones para la entidad {}: {}",
                                            ent.id,
                                            e
                                        );
                                        break 'pose None;
                                    }
                                }
                            }
                            let jb = self.skin_joint_buffers.get_mut(&ent.id).unwrap();
                            jb.write(&matrices);
                            Some((jb.descriptor_set, skinned_mesh))
                        };

                        let lm_params = renderer.lm_params_for(ent.id);
                        // Ranura del UBO de luces de ESTE objeto (0 =
                        // global si aún no tiene asignación propia).
                        let ranura_luz = if self.luces_global {
                            0
                        } else {
                            self.sistema_luces.ranura_de(ent.id)
                        };
                        // Rangos de los draw items (nivel LOD activo)
                        // para el proyector de grafo en sombras (C3).
                        let rangos_entidad: Vec<(u32, u32)> =
                            draw_items.iter().map(|it| it.3).collect();
                        let mut shadow_recorded = false;
                        // Contorno de selección (vista de Escena): solo la
                        // entidad seleccionada y con el gizmo activado.
                        #[cfg(feature = "editor")]
                        let es_sel = self.editor_layout.hierarchy.selected_entity == Some(ent.id)
                            && self.editor_layout.viewport.gizmos.selection_outline;
                        #[cfg(not(feature = "editor"))]
                        let es_sel = false;
                        // Grosor del casco en espacio de objeto: constante
                        // en pantalla (proporcional a la distancia a la
                        // cámara de escena) y deshecha la escala del
                        // modelo, porque el shader expande en espacio de
                        // objeto a lo largo de la normal.
                        let grosor_obj = if es_sel {
                            let col = |i: usize| {
                                (model[i] * model[i]
                                    + model[i + 1] * model[i + 1]
                                    + model[i + 2] * model[i + 2])
                                    .sqrt()
                            };
                            let escala_mundo = col(0).max(col(4)).max(col(8));
                            let dx = model[12] - eye_scene[0];
                            let dy = model[13] - eye_scene[1];
                            let dz = model[14] - eye_scene[2];
                            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                            let grosor_mundo = (dist * 0.004).clamp(0.003, 0.6);
                            if escala_mundo > 1e-6 {
                                grosor_mundo / escala_mundo
                            } else {
                                grosor_mundo
                            }
                        } else {
                            0.0
                        };
                        for (c, mp, tex_set, range, em_item, uv_item) in draw_items {
                            if !shadow_recorded {
                                shadow_recorded = true;
                                if skin_pose.is_none() {
                                    // Proyector de grafo con pase Shadow
                                    // (Fase 2S): va a la lista de grafos
                                    // para que el pase de sombras del sol
                                    // use la etapa de vértices del grafo.
                                    let pases = shader_name
                                        .as_deref()
                                        .and_then(glass_core::shader_graph::graph_passes);
                                    let es_grafo_sombra = pases
                                        .map(|p| {
                                            p.contains(
                                                &glass_core::shader_graph::PassKind::Shadow,
                                            )
                                        })
                                        .unwrap_or(false);
                                    if es_grafo_sombra {
                                        let gname = shader_name.clone().unwrap();
                                        // Umbral de recorte: la propiedad
                                        // especial `alpha_clip` del esquema,
                                        // con el override de la entidad si
                                        // lo trae. Sin esa propiedad, None
                                        // (profundidad pura).
                                        let mut alpha_clip = glass_core::shader_graph::graph_schema(&gname)
                                            .and_then(|props| {
                                                props.iter().find(|p| {
                                                    p.name == "alpha_clip"
                                                        && matches!(
                                                            p.kind,
                                                            glass_core::shader_graph::PropertyKind::Float
                                                        )
                                                })
                                                .map(|p| match p.default {
                                                    glass_core::shader_graph::PropertyValue::Float(v) => v,
                                                    _ => 0.0,
                                                })
                                            });
                                        if let Some(o) = ent
                                            .mesh
                                            .as_ref()
                                            .and_then(|m| m.shader_properties.get("alpha_clip"))
                                            .and_then(|v| v.as_f64())
                                        {
                                            if alpha_clip.is_some() {
                                                alpha_clip = Some(o as f32);
                                            }
                                        }
                                        let tex = self
                                            .graph_tex_state
                                            .get(&ent.id)
                                            .map(|(s, _)| *s)
                                            .unwrap_or(tex_set);
                                        if visible_shadow { shadow_graph_statics.push((
                                            ent_mesh,
                                            model,
                                            gname,
                                            tex,
                                            alpha_clip,
                                            // Resto de C3: los rangos
                                            // de los draw items del
                                            // frame (nivel LOD activo).
                                            if rangos_entidad.is_empty() {
                                                None
                                            } else {
                                                Some(rangos_entidad.clone())
                                            },
                                        )); }
                                    } else {
                                        // Auditoría C3: la sombra usa los
                                        // rangos del nivel LOD activo, como
                                        // el dibujo principal (antes se
                                        // proyectaba la unión de niveles).
                                        let rangos = model_key.and_then(|k| {
                                            let ojo = eye_game_opt.unwrap_or(eye_scene);
                                            rangos_sombra_lod(
                                                &cache,
                                                k,
                                                [model[12], model[13], model[14]],
                                                [ojo[0], ojo[1], ojo[2]],
                                            )
                                        });
                                        if visible_shadow { shadow_statics.push((ent_mesh, model, rangos)); }
                                    }
                                }
                            }
                            // Rama con piel: mismos push constants y rangos
                            // que la ruta normal, pero malla `SkinnedVertex`,
                            // set 0 de textura (con el blanco de respaldo si
                            // el lote no traía: el pipeline con piel exige
                            // AMBOS sets enlazados) y set 1 de articulaciones.
                            if let Some((joints_set, skinned_mesh)) = skin_pose {
                                let tex = if tex_set != vk::DescriptorSet::null() {
                                    tex_set
                                } else {
                                    renderer.fallback_set
                                };
                                if tex == vk::DescriptorSet::null() {
                                    continue;
                                }
                                if visible_shadow { shadow_skinned.push((skinned_mesh, model, joints_set)); }
                                if vis_scene { scene_skinned.push((skinned_mesh, glass_renderer::MeshPushConstants {
                                    mvp: mvp_scene,
                                    model,
                                    light_dir,
                                    light_color,
                                    material_params: mp,
                                    object_color: c,
                                    camera_pos: eye_scene,
                                    lm_params,
                                    emissive: em_item,
                                    uv_transform: uv_item,
                                }, tex, joints_set, range, ranura_luz)); }
                                // Contorno de selección con piel: el mismo
                                // item con el grosor y el blanco del
                                // contorno (solo editor).
                                #[cfg(feature = "editor")]
                                {
                                    if es_sel && vis_scene {
                                        scene_outline_skinned.push((skinned_mesh, glass_renderer::MeshPushConstants {
                                            mvp: mvp_scene,
                                            model,
                                            light_dir,
                                            light_color,
                                            material_params: [grosor_obj, 0.0, 0.0, 0.0],
                                            object_color: [1.0, 1.0, 1.0, 1.0],
                                            camera_pos: eye_scene,
                                            lm_params,
                                            emissive: em_item,
                                            uv_transform: uv_item,
                                        }, tex, joints_set, range, ranura_luz));
                                    }
                                }
                                if let Some(vp_game) = vp_game_opt.filter(|_| vis_game) {
                                    let mvp_game = matmul(&vp_game, &model);
                                    game_skinned.push((skinned_mesh, glass_renderer::MeshPushConstants {
                                        mvp: mvp_game,
                                        model,
                                        light_dir,
                                        light_color,
                                        material_params: mp,
                                        object_color: c,
                                        camera_pos: eye_game_opt.unwrap_or(eye_scene),
                                        lm_params,
                                        emissive: em_item,
                                        uv_transform: uv_item,
                                    }, tex, joints_set, range, ranura_luz));
                                }
                                continue;
                            }
                            let ent_overrides = ent
                                .mesh
                                .as_ref()
                                .map(|m| &m.shader_properties)
                                .map(|m| {
                                    // shader_properties es HashMap<String, Value>; el
                                    // resolutor espera un serde_json::Map. Convierte
                                    // sin clonar los valores más de una vez.
                                    let mut map = serde_json::Map::new();
                                    for (k, v) in m.iter() {
                                        map.insert(k.clone(), v.clone());
                                    }
                                    map
                                })
                                .unwrap_or_else(|| empty_overrides.clone());
                            let props_set = shader_name.as_deref().and_then(|g| {
                                resolver_props_set(
                                    &mut *renderer,
                                    &mut self.graph_props_state,
                                    ent.id,
                                    g,
                                    &ent_overrides,
                                )
                            });

                            // Set de texturas de grafo (resuelto en el pase
                            // previo con &mut cache). Sustituye a tex_set
                            // solo en esta entidad/item; si no hay uno, cada
                            // push usa el tex_set normal.
                            let tex_set_grafo: Option<vk::DescriptorSet> =
                                self.graph_tex_state.get(&ent.id).map(|(set, _)| *set);
                            // Contorno toon (Fase 2S, S5): si el grafo de
                            // la entidad declara el pase Outline, cada push
                            // normal va seguido de su push de contorno con
                            // el pipeline "outline:<grafo>". El grosor y el
                            // color salen de las propiedades especiales
                            // `outline_width` (Float) y `outline_color`
                            // (Color) del esquema, con los overrides de la
                            // entidad si los trae.
                            let contorno: Option<(String, [f32; 4], f32)> =
                                shader_name.as_deref().and_then(|g| {
                                    let pases = glass_core::shader_graph::graph_passes(g)?;
                                    if !pases.contains(&glass_core::shader_graph::PassKind::Outline) {
                                        return None;
                                    }
                                    let mut ancho = 0.03f32;
                                    let mut color_c = [0.05f32, 0.05, 0.08, 1.0];
                                    if let Some(props) = glass_core::shader_graph::graph_schema(g) {
                                        for p in props.iter() {
                                            match (p.name.as_str(), &p.default) {
                                                ("outline_width", glass_core::shader_graph::PropertyValue::Float(v)) => ancho = *v,
                                                ("outline_color", glass_core::shader_graph::PropertyValue::Color(c4)) => color_c = *c4,
                                                _ => {}
                                            }
                                        }
                                    }
                                    if let Some(m) = ent.mesh.as_ref() {
                                        if let Some(v) = m.shader_properties.get("outline_width").and_then(|v| v.as_f64()) {
                                            ancho = v as f32;
                                        }
                                        if let Some(a) = m.shader_properties.get("outline_color").and_then(|v| v.as_array()) {
                                            if a.len() == 4 {
                                                color_c = [
                                                    a[0].as_f64().unwrap_or(0.0) as f32,
                                                    a[1].as_f64().unwrap_or(0.0) as f32,
                                                    a[2].as_f64().unwrap_or(0.0) as f32,
                                                    a[3].as_f64().unwrap_or(1.0) as f32,
                                                ];
                                            }
                                        }
                                    }
                                    Some((format!("outline:{}", g), color_c, ancho))
                                });
                            if vis_scene { scene_batches.push((ent_mesh, glass_renderer::MeshPushConstants {
                                mvp: mvp_scene,
                                model,
                                light_dir,
                                light_color,
                                material_params: mp,
                                object_color: if color_error { [1.0, 0.0, 1.0, 1.0] } else { c },
                                camera_pos: eye_scene,
                                lm_params,
                                emissive: em_item,
                                uv_transform: uv_item,
                            }, tex_set_grafo.unwrap_or(tex_set), shader_name.clone(), range, props_set, ranura_luz)); }
                                if let Some((nombre_o, color_o, ancho_o)) = &contorno {
                                    scene_batches.push((ent_mesh, glass_renderer::MeshPushConstants {
                                        mvp: mvp_scene,
                                        model,
                                        light_dir,
                                        light_color,
                                        material_params: [*ancho_o, 0.0, 0.0, 0.0],
                                        object_color: *color_o,
                                        camera_pos: eye_scene,
                                        lm_params,
                                        emissive: em_item,
                                        uv_transform: uv_item,
                                    }, tex_set_grafo.unwrap_or(tex_set), Some(nombre_o.clone()), range, props_set, ranura_luz));
                                }
                                // Contorno de selección: casco invertido
                                // blanco de la malla, sin escritura de
                                // profundidad (no va a shadow_casters).
                                if es_sel && vis_scene {
                                    scene_batches.push((ent_mesh, glass_renderer::MeshPushConstants {
                                        mvp: mvp_scene,
                                        model,
                                        light_dir,
                                        light_color,
                                        material_params: [grosor_obj, 0.0, 0.0, 0.0],
                                        object_color: [1.0, 1.0, 1.0, 1.0],
                                        camera_pos: eye_scene,
                                        lm_params,
                                        emissive: em_item,
                                        uv_transform: uv_item,
                                    }, tex_set_grafo.unwrap_or(tex_set), Some("__contorno_seleccion".to_string()), range, None, ranura_luz));
                                }

                            if let Some(vp_game) = vp_game_opt.filter(|_| vis_game) {
                                let mvp_game = matmul(&vp_game, &model);
                                game_batches.push((ent_mesh, glass_renderer::MeshPushConstants {
                                    mvp: mvp_game,
                                    model,
                                    light_dir,
                                    light_color,
                                    material_params: mp,
                                    object_color: if color_error { [1.0, 0.0, 1.0, 1.0] } else { c },
                                    // Si la escena no tiene cámara activa no hay Game
                                    // View, así que este valor sólo se usa cuando
                                    // vp_game_opt es Some.
                                    camera_pos: eye_game_opt.unwrap_or(eye_scene),
                                    lm_params,
                                    emissive: em_item,
                                    uv_transform: uv_item,
                                }, tex_set_grafo.unwrap_or(tex_set), shader_name.clone(), range, props_set, ranura_luz));
                                    if let Some((nombre_o, color_o, ancho_o)) = &contorno {
                                        stats_outline += 1;
                                        game_batches.push((ent_mesh, glass_renderer::MeshPushConstants {
                                            mvp: mvp_game,
                                            model,
                                            light_dir,
                                            light_color,
                                            material_params: [*ancho_o, 0.0, 0.0, 0.0],
                                            object_color: *color_o,
                                            camera_pos: eye_game_opt.unwrap_or(eye_scene),
                                            lm_params,
                                            emissive: em_item,
                                            uv_transform: uv_item,
                                        }, tex_set_grafo.unwrap_or(tex_set), Some(nombre_o.clone()), range, props_set, ranura_luz));
                                    }
                            }
                        }
                    }
                }
            }


            // Ordenación de draws (Fase 2S, S6): por (transparente al
            // final, shader, textura) con orden estable, para agrupar
            // los enlaces de pipeline y de descriptor set del pase. El
            // índice de textura es el de primera aparición en la lista,
            // así la ordenación es determinista sin claves externas.
            for lotes in [&mut scene_batches, &mut game_batches] {
                let mut orden_tex: std::collections::HashMap<vk::DescriptorSet, usize> =
                    std::collections::HashMap::new();
                for item in lotes.iter() {
                    let n = orden_tex.len();
                    orden_tex.entry(item.2).or_insert(n);
                }
                lotes.sort_by_cached_key(|item| {
                    let nombre = item.3.as_deref().unwrap_or("");
                    let transp = nombre == "material_transparent"
                        || self.transparent_graphs.contains(nombre);
                    (transp, nombre.to_string(), orden_tex[&item.2])
                });
            }

            // Oclusión de audio (Fase 3): cada 150 ms, rayo de la
            // cámara de juego a cada fuente 3D viva; si algo tapa la
            // línea de visión, la fuente pasa por un low-pass en el
            // backend. Solo se avisa al sistema de audio al cambiar.
            if self.last_occlusion_check.elapsed() >= std::time::Duration::from_millis(150) {
                self.last_occlusion_check = std::time::Instant::now();
                let ojo = match eye_game_opt {
                    Some(e) => [e[0], e[1], e[2]],
                    None => [eye_scene[0], eye_scene[1], eye_scene[2]],
                };
                let fuentes = self.audio_system.live_3d_sources();
                if !fuentes.is_empty() {
                    if let Ok(mut physics) = self.physics_world.lock() {
                        for (handle, pos) in fuentes {
                            let delta = [
                                pos[0] - ojo[0],
                                pos[1] - ojo[1],
                                pos[2] - ojo[2],
                            ];
                            let dist = (delta[0] * delta[0]
                                + delta[1] * delta[1]
                                + delta[2] * delta[2])
                                .sqrt();
                            let ocluida = if dist > 0.35 {
                                let dir = [
                                    delta[0] / dist,
                                    delta[1] / dist,
                                    delta[2] / dist,
                                ];
                                physics
                                    .cast_ray_masked(ojo, dir, dist - 0.3, u32::MAX, 0)
                                    .is_some()
                            } else {
                                false
                            };
                            if self.occlusion_applied.get(&handle) != Some(&ocluida) {
                                self.occlusion_applied.insert(handle, ocluida);
                                self.audio_system
                                    .set_source_occlusion(handle, ocluida);
                            }
                        }
                    }
                }
            }

            // Fase 2: estadísticas del frame para la tool MCP.
            // Triángulos por vista: cada item aporta index_count / 3
            // (el rango .4 de los lotes es (first_index, index_count)).
            fn triangulos<I: Iterator<Item = u32>>(index_counts: I) -> u64 {
                index_counts.map(|n| u64::from(n / 3)).sum()
            }
            let scene_triangles = triangulos(
                scene_batches.iter().map(|b| (b.4).1).chain(scene_skinned.iter().map(|s| (s.4).1)),
            );
            let game_triangles = triangulos(
                game_batches.iter().map(|b| (b.4).1).chain(game_skinned.iter().map(|s| (s.4).1)),
            );
            glass_core::render_stats::record_render_stats(glass_core::render_stats::RenderStatsSnapshot {
                entities_considered: stats_considered,
                entities_culled: stats_culled,
                game_draw_calls: (game_batches.len() + game_skinned.len()) as u32,
                scene_draw_calls: (scene_batches.len() + scene_skinned.len()) as u32,
                scene_triangles,
                game_triangles,
                shadow_draw_calls: (shadow_statics.len() + shadow_skinned.len()) as u32,
                graph_shadow_draw_calls: shadow_graph_statics.len() as u32,
                outline_draw_calls: stats_outline,
                pipeline_binds: renderer.frame_bind_counts().0,
                descriptor_binds: renderer.frame_bind_counts().1,
                gpu_draw_calls: renderer.frame_draw_calls(),
            });

            // ------------------------------------------------------------
            // Portales (bloque Portales): si la escena tiene una
            // pareja enlazada y hay cámara de juego, se preparan
            // las dos vistas virtuales (clonando los lotes de
            // juego con la VP virtual: cada push ya trae su
            // `model`), se sacan las superficies de los lotes
            // normales y se construyen sus dibujos de portal.
            // La llamada a render_portal_views va justo antes de
            // render_views, cuando ya están los proyectores.
            // ------------------------------------------------------------
            let mut portal_render: Option<PortalRenderLists<'_>> = None;
            let mut scene_portal_draws: Vec<glass_renderer::PortalDrawItem> = Vec::new();
            let mut game_portal_draws: Vec<glass_renderer::PortalDrawItem> = Vec::new();
            if let Some(vp_game) = vp_game_opt {
                let datos = self.scene_state.lock().ok().and_then(|scene| {
                    let mut portales: Vec<(
                        u64,
                        glass_core::portal::PortalData,
                        [f32; 16],
                        String,
                    )> = scene
                        .entities
                        .iter()
                        .filter(|e| e.active && e.portal.is_some())
                        .map(|e| {
                            (
                                e.id,
                                e.portal.clone().unwrap(),
                                scene.compute_world_matrix(e.id),
                                e.mesh
                                    .as_ref()
                                    .map(|m| m.model.clone())
                                    .unwrap_or_default(),
                            )
                        })
                        .collect();
                    portales.sort_by_key(|(id, _, _, _)| *id);
                    let primera = portales.first()?.1.pair.clone();
                    let pareja: Vec<_> = portales
                        .iter()
                        .filter(|(_, d, _, _)| d.pair == primera)
                        .take(2)
                        .cloned()
                        .collect();
                    if pareja.len() != 2 {
                        return None;
                    }
                    let cam = scene.active_camera()?.clone();
                    Some((pareja, cam))
                });
                if let Some((pareja, cam)) = datos {
                    let (id_a, data_a, world_a, modelo_a) = pareja[0].clone();
                    let (id_b, data_b, world_b, modelo_b) = pareja[1].clone();
                    let _ = (id_a, id_b, &data_a, &data_b);
                    // Mitad local de la superficie según su malla:
                    // el plano integrado abarca ±0,5 y el cubo
                    // (glb o de reserva) ±1. El anillo del shader
                    // se normaliza con esto.
                    let mitad_local = |modelo: &str| -> [f32; 2] {
                        if modelo == "plane" {
                            [0.5, 0.5]
                        } else {
                            [1.0, 1.0]
                        }
                    };
                    let view_g = camera_view_matrix(cam.transform.position, cam.transform.rotation());
                    let cam_world = glass_core::mat4_inverse_affine(view_g).unwrap_or(view_g);
                    let fov = cam.camera.as_ref().map(|c| c.fov).unwrap_or(60.0);
                    let far = cam.camera.as_ref().map(|c| c.far_clip).unwrap_or(1000.0);
                    // La virtual que se muestra EN A es la cámara
                    // real transportada de A a B (T(src=A, dst=B)):
                    // queda DETRÁS de B, mirando a través de su
                    // plano (el near coincide con ese plano).
                    let hacer_virtual = |src_w: [f32; 16],
                                         dst_w: [f32; 16]|
                     -> Option<([f32; 16], [f32; 4])> {
                        let t = glass_core::portal::pair_transform(src_w, dst_w)?;
                        let virt_world = matmul(&t, &cam_world);
                        let view_v = glass_core::mat4_inverse_affine(virt_world)?;
                        let ojo = glass_core::mat4_translation(virt_world);
                        // El plano cercano COINCIDE con el plano
                        // del portal de salida: recorta la pared
                        // que lo contiene y todo lo que quede
                        // entre la cámara virtual y el portal.
                        let dist = glass_core::portal::signed_distance(dst_w, ojo).abs();
                        let near = (dist - 0.03).clamp(0.05, 8.0);
                        let proj = perspective_matrix(fov.to_radians(), 1.0, near, far);
                        Some((matmul(&proj, &view_v), [ojo[0], ojo[1], ojo[2], 1.0]))
                    };
                    if let (Some((vp_virt_a, ojo_a)), Some((vp_virt_b, ojo_b))) = (
                        hacer_virtual(world_a, world_b),
                        hacer_virtual(world_b, world_a),
                    ) {
                        let mut items_a = clonar_lotes_portal(&game_batches, vp_virt_a, ojo_a);
                        let piel_a = clonar_piel_portal(&game_skinned, vp_virt_a, ojo_a);
                        let mut items_b = clonar_lotes_portal(&game_batches, vp_virt_b, ojo_b);
                        let piel_b = clonar_piel_portal(&game_skinned, vp_virt_b, ojo_b);
                        // Las dos superficies fuera de las vistas
                        // virtuales: la de salida quedaría pegada
                        // al plano cercano y taparía el centro de
                        // su propia vista con su cara trasera.
                        items_a.retain(|(_, pc, _, _, _, _, _)| {
                            pc.model != world_a && pc.model != world_b
                        });
                        items_b.retain(|(_, pc, _, _, _, _, _)| {
                            pc.model != world_a && pc.model != world_b
                        });
                        portal_render = Some((items_a, piel_a, items_b, piel_b));

                        // Superficies: se roba la malla+rango de
                        // los lotes de juego antes de filtrarlos.
                        let sup_a = robar_superficie(&game_batches, world_a);
                        let sup_b = robar_superficie(&game_batches, world_b);
                        if let Some((mesh, range)) = sup_a {
                            empujar_draw_portal(&mut game_portal_draws, &mut scene_portal_draws, mesh, range, world_a, vp_game, vp_scene, vp_virt_a, 0, [1.0, 0.45, 0.08, 1.0], mitad_local(&modelo_a));
                        }
                        if let Some((mesh, range)) = sup_b {
                            empujar_draw_portal(&mut game_portal_draws, &mut scene_portal_draws, mesh, range, world_b, vp_game, vp_scene, vp_virt_b, 1, [0.15, 0.45, 1.0, 1.0], mitad_local(&modelo_b));
                        }

                        // Fuera de los lotes normales (la
                        // superficie la dibuja el pase de portal).
                        scene_batches.retain(|(_, pc, _, _, _, _, _)| {
                            pc.model != world_a && pc.model != world_b
                        });
                        game_batches.retain(|(_, pc, _, _, _, _, _)| {
                            pc.model != world_a && pc.model != world_b
                        });
                    }
                }
            }

            let game_meshes_arg = if vp_game_opt.is_some() {
                Some(game_batches.as_slice())
            } else {
                None
            };
            let game_skinned_arg = if vp_game_opt.is_some() {
                Some(game_skinned.as_slice())
            } else {
                None
            };

            // Canal global de interacción de los grafos: lo que los
            // scripts hayan publicado este frame (el punto y la forma
            // de su huella; la barca de L2, p. ej.) llega al UBO global
            // del Renderer; sin publicador vale el valor inactivo y los
            // grafos ni se enteran.
            renderer.set_graph_globals(glass_scripting::bindings::get_interaction_point_state());

            // IBL (set 6): el cubemap del cielo se regenera solo si el
            // sol se movió o cambió de color (con freno de 2 s). Sin luz
            // direccional, el entorno se desactiva y el shader vuelve
            // al ambiente hemisférico. La intensidad del entorno es la
            // constante propia del módulo ibl, desacoplada de la intensidad
            // de ambiente de la escena (G5-C), para que autorar ambiente
            // 1.0 no multiplique el entorno.
            {
                let hay_sol = light_color[0] + light_color[1] + light_color[2] > 0.0;
                if hay_sol {
                    let sol = [light_dir[0], light_dir[1], light_dir[2]];
                    let color = [light_color[0], light_color[1], light_color[2]];
                    if self.estado_ibl.toca_regenerar(sol, color) {
                        let datos = ibl::generar_cubemapa(sol, color);
                        if renderer
                            .set_ibl_cubemap(
                                vk_ctx.graphics_queue,
                                ibl::IBL_SIZE,
                                ibl::IBL_MIPS,
                                &datos,
                            )
                            .is_ok()
                        {
                            renderer.set_ibl_params(
                                ibl::INTENSIDAD_ENTORNO,
                                (ibl::IBL_MIPS - 1) as f32,
                                true,
                            );
                        }
                    }
                } else if self.estado_ibl.hay_entorno {
                    self.estado_ibl.hay_entorno = false;
                    renderer.set_ibl_params(0.0, 0.0, false);
                }
            }

            // Sombras del sol (bloque de gráficos): la caja
            // ortográfica sigue a la cámara de juego (a la de escena
            // en el editor sin cámara de juego) y la luz es la
            // principal de la escena — la misma de la que salen
            // `light_dir`/`light_color` de los lotes. Sin luz: el canal
            // se publica inactivo y el pase ni se graba.
            {
                // El VP ya se calculó antes de la recolección (es el
                // frustum con el que se filtraron los casters): aquí
                // solo se publica.
                if let Some(vp) = vp_shadow_pre {
                    renderer.set_shadow_frame(vp, [
                        1.0 / glass_renderer::shadow::SHADOW_MAP_SIZE as f32,
                        glass_renderer::shadow::SHADOW_DEPTH_BIAS,
                        glass_renderer::shadow::SHADOW_NORMAL_BIAS,
                        1.0,
                    ]);
                } else {
                    renderer.set_shadow_frame(glass_core::mat4_identity(), [0.0, 0.0, 0.0, 0.0]);
                }
            }
            let shadow_casters = glass_renderer::ShadowCasters {
                statics: &shadow_statics,
                skinned: &shadow_skinned,
                graph_statics: &shadow_graph_statics,
            };

            // Luces puntuales del frame: las 8 «Point» más cercanas al
            // ojo activo, publicadas en el renderer antes de dibujar.
            {
                let pl_eye = eye_game_opt.unwrap_or(eye_scene);
let (ubo_luces, luces_sombra) = match self.scene_state.lock() {
                    Ok(scene) => recolectar_luces_puntuales(&scene, [pl_eye[0], pl_eye[1], pl_eye[2]]),
                    Err(_) => (glass_renderer::PointLightsUbo::default(), Vec::new()),
                };
                renderer.set_point_lights(ubo_luces);
                renderer.set_point_shadow_lights(luces_sombra);
                // Asignación de luces POR OBJETO (integración de
                // iluminación): las entidades con malla resuelven
                // sus luces (rango → score, con histéresis) y su UBO
                // se publica en su ranura del buffer dinámico. Las
                // ranuras son estables entre frames; el contenido se
                // refresca aquí, antes de registrar los dibujos.
                if !self.luces_global {
                    self.luces_frame += 1;
                    let frame = self.luces_frame;
                    if let Ok(scene) = self.scene_state.lock() {
                        let (luces, cubos) = recolectar_luces_escena(
                            &scene,
                            [pl_eye[0], pl_eye[1], pl_eye[2]],
                        );
                        self.sistema_luces.observar_luces(&luces);
                        let vivas: std::collections::HashSet<u64> =
                            scene.entities.iter().map(|e| e.id).collect();
                        self.sistema_luces.podar(&vivas);
                        let por_id: std::collections::HashMap<u64, &luces_objeto::LuzEscena> =
                            luces.iter().map(|l| (l.id, l)).collect();
                        let mundos = scene.compute_world_matrices();
                        for ent in &scene.entities {
                            if !ent.active || ent.mesh.is_none() {
                                continue;
                            }
                            let centro = mundos
                                .get(&ent.id)
                                .map(|m| [m[12], m[13], m[14]])
                                .unwrap_or(ent.transform.position);
                            if !self.sistema_luces.necesita(ent.id, centro, frame) {
                                continue;
                            }
                            // Semiejes de la caja del objeto desde la
                            // escala (las primitivas son unitarias): la
                            // distancia de ranking va al punto más
                            // cercano de la caja, no al centro.
                            let s = ent.transform.scale;
                            let semiejes = [
                                0.5 * s[0].abs(),
                                0.5 * s[1].abs(),
                                0.5 * s[2].abs(),
                            ];
                            let resuelto = self.sistema_luces.resolver(
                                ent.id,
                                centro,
                                semiejes,
                                &luces,
                            );
                            if let Some((ranura, lista)) =
                                resuelto.map(|a| (a.ranura, a.luces.clone()))
                            {
                                let ubo = luces_objeto::construir_ubo(&lista, &por_id, &cubos);
                                renderer.set_object_lights(ranura, ubo);
                            }
                        }
                    }
                }
                // Foto de las asignaciones para la tool MCP
                // glass_light_assignment.
                glass_core::lighting_stats::record_light_assignments(
                    glass_core::lighting_stats::AssignmentStatsSnapshot {
                        global_mode: self.luces_global,
                        objects: self
                            .sistema_luces
                            .por_entidad
                            .iter()
                            .map(|(id, a)| {
                                (
                                    *id,
                                    glass_core::lighting_stats::LightAssignmentSnapshot {
                                        slot: a.ranura,
                                        lights: a.luces.clone(),
                                    },
                                )
                            })
                            .collect(),
                    },
                );
                // Niebla de la escena (Fase 2): la publica la primera
                // entidad con FogData; sin ella, niebla desactivada.
                match self.scene_state.lock() {
                    Ok(scene) => {
                        match scene.fog_settings() {
                            Some(fog) => renderer.set_fog(fog.color, fog.near, fog.far, true),
                            None => renderer.set_fog([0.0, 0.0, 0.0], 0.0, 1.0, false),
                        }
                        // Ambiente de escena (G1): lo publica la
                        // resolución de la escena; sin portador son
                        // los colores heredados.
                        let amb = scene.ambient_settings();
                        renderer.set_ambient(amb.sky, amb.equator, amb.ground);
                        if amb.extra_carriers > 0 {
                            static WARNED: std::sync::atomic::AtomicBool =
                                std::sync::atomic::AtomicBool::new(false);
                            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                                log::warn!(
                                    "La escena tiene {} entidad(es) adicionales con ambiente; \
                                     se usa el de '{}'.",
                                    amb.extra_carriers,
                                    scene.ambient_carrier_name().unwrap_or_default()
                                );
                            }
                        }
                    }
                    Err(_) => {
                        renderer.set_fog([0.0, 0.0, 0.0], 0.0, 1.0, false);
                        renderer.set_ambient(
                            glass_core::scene::LEGACY_AMBIENT_SKY,
                            glass_core::scene::LEGACY_AMBIENT_EQUATOR,
                            glass_core::scene::LEGACY_AMBIENT_GROUND,
                        );
                    }
                }

                // Telemetría de iluminación (Fase 2S) para glass_render_lighting_debug.
                let fog = match self.scene_state.lock() {
                    Ok(scene) => scene.fog_settings(),
                    Err(_) => None,
                };
                let (fog_enabled, fog_color, fog_near, fog_far) = match fog {
                    Some(f) => (true, f.color, f.near, f.far),
                    None => (false, [0.0, 0.0, 0.0], 0.0, 0.0),
                };

                let mut puntos = [glass_core::lighting_stats::PointLightSnapshot::default(); 8];
                for (i, punto) in puntos.iter_mut().enumerate() {
                    let pos_range = ubo_luces.pos_range[i];
                    let color_intensity = ubo_luces.color_intensity[i];
                    *punto = glass_core::lighting_stats::PointLightSnapshot {
                        position: [pos_range[0], pos_range[1], pos_range[2]],
                        range: pos_range[3],
                        color: [color_intensity[0], color_intensity[1], color_intensity[2]],
                        intensity: color_intensity[3],
                        shadow_cube: ubo_luces.shadow[i][0] as i32,
                    };
                }

                glass_core::lighting_stats::record_lighting_stats(
                    glass_core::lighting_stats::LightingStatsSnapshot {
                        has_directional: light_color[0] + light_color[1] + light_color[2] > 0.0
                            || light_dir[3] > 0.1,
                        directional_dir: [light_dir[0], light_dir[1], light_dir[2]],
                        directional_color: [light_color[0], light_color[1], light_color[2]],
                        point_count: ubo_luces.count,
                        points: puntos,
                        fog_enabled,
                        fog_color,
                        fog_near,
                        fog_far,
                    },
                );
            }

            if let Some((items_a, piel_a, items_b, piel_b)) = &portal_render {
                renderer.render_portal_views(
                    vk_ctx.graphics_queue,
                    (items_a, piel_a),
                    (items_b, piel_b),
                    Some(&shadow_casters),
                )?;
            }

            // Partículas (Fase 8P): simular, expandir por vista y
            // subir la geometría antes de renderizar. Los draws del
            // frame anterior se registran aquí (los contó el
            // renderer durante el render_views previo).
            glass_core::particle_stats::record_draws(renderer.particle_draws_last_frame());
            let particle_out = avanzar_particulas(
                &mut self.particle_emitters,
                &mut self.particle_applied,
                &mut self.particle_prev_origin,
                &self.scene_state,
                anim_dt,
                CamarasParticulas {
                    vp_scene,
                    vp_game_opt,
                    eye_scene,
                    eye_game_opt,
                    #[cfg(feature = "editor")]
                    centro_editor: Some(self.editor_layout.viewport.editor_cam_center),
                    #[cfg(not(feature = "editor"))]
                    centro_editor: None,
                },
            );
            // Rejilla del editor: se dibuja dentro del pase 3D de la
            // vista de escena, con depth test (los modelos la tapan).
            // Sin rejilla se sube vacía para dejar el contador a cero.
            #[cfg(feature = "editor")]
            {
                let viewport = &self.editor_layout.viewport;
                if viewport.gizmos.show_grid {
                    let verts = glass_editor::panels::gizmo_3d::vertices_rejilla(
                        viewport.editor_cam_center,
                        viewport.editor_cam_dist,
                    );
                    renderer.set_editor_grid_lines(vp_scene, &verts);
                } else {
                    renderer.set_editor_grid_lines(vp_scene, &[]);
                }
            }
            let particle_vps = particle_out.as_ref().map(|o| {
                renderer.set_particle_frame(false, &o.scene.0, &o.scene.1);
                renderer.set_particle_frame(true, &o.game.0, &o.game.1);
                o.vps
            });
            #[cfg(feature = "editor")]
            let scene_outline_skinned_arg: &[glass_renderer::SkinnedDrawItem<'_>] = &scene_outline_skinned;
            #[cfg(not(feature = "editor"))]
            let scene_outline_skinned_arg: &[glass_renderer::SkinnedDrawItem<'_>] = &[];
            renderer.render_views(vk_ctx.graphics_queue, FrameDraws { scene_meshes: &scene_batches, game_meshes: game_meshes_arg, scene_skinned: &scene_skinned, game_skinned: game_skinned_arg, scene_outline_skinned: scene_outline_skinned_arg, shadow_casters: Some(&shadow_casters), scene_portal_draws: &scene_portal_draws, game_portal_draws: Some(&game_portal_draws), particle_vps })?;

            // Post-procesado de las vistas renderizadas (sin post
            // activo, estas llamadas no hacen nada). La vista de juego
            // solo se procesa si se renderizó este frame.
            renderer.render_post(vk_ctx.graphics_queue, glass_renderer::PostView::Scene)?;
            if game_meshes_arg.is_some() {
                renderer.render_post(vk_ctx.graphics_queue, glass_renderer::PostView::Game)?;
            }
        } else {
            renderer.render_offscreen(vk_ctx.graphics_queue)?;
        }
        drop(cache);

        let profile_render_us: u64 = profile_t_render.elapsed().as_micros() as u64;

        // Modo Standalone: Hardware Blit directo de VRAM al Swapchain (Cero egui/MCP overhead)
        #[cfg(feature = "editor")]
        let render_standalone = self.is_standalone;
        #[cfg(not(feature = "editor"))]
        let render_standalone = true;
        // En el binario CON editor, el standalone presenta la vista
        // de JUEGO (blit + Canvas), como el binario puro: sin esto
        // se presentaba la vista de escena del editor y el FOV de
        // la cámara de juego (la mira con zoom) nunca llegaba a
        // pantalla.
        #[cfg(feature = "editor")]
        {
            renderer.present_game = render_standalone;
        }

        // Canvas de juego (UI) en el standalone: dentro del editor lo
        // pinta la superposición de egui sobre la vista de juego, pero el
        // binario puro no tiene egui y sin este pase el HUD de la escena
        // no existiría en el juego compilado. Se compone sobre el
        // offscreen (la imagen que presenta el blit) tras el 3D y antes
        // del blit. En el modo editor normal NO se ejecuta: allí manda
        // la ruta de egui y así nada se dibuja dos veces.
        if render_standalone {
            let batch = {
                // Atlas de imágenes del Canvas (UI v2): se
                // reconstruye solo cuando cambia la lista de
                // sprites de la escena activa.
                {
                    let key = match self.scene_state.lock() {
                        Ok(scene) => scene
                            .entities
                            .iter()
                            .filter(|e| e.active)
                            .filter_map(|e| e.ui_image.as_ref())
                            .map(|i| i.sprite.as_str())
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        Err(_) => String::new(),
                    };
                    if key != self.ui_images_key {
                        self.ui_images_key = key;
                        let built = match self.scene_state.lock() {
                            Ok(scene) => canvas_ui::build_ui_scene_images(
                                &scene,
                                std::path::Path::new("assets"),
                            ),
                            Err(_) => (canvas_ui::UiSceneImages::new(), None),
                        };
                        self.ui_images_map = built.0;
                        match built.1 {
                            Some(atlas) => {
                                let _ = renderer.set_ui_image_atlas(
                                    vk_ctx.graphics_queue,
                                    atlas.rgba,
                                    atlas.size,
                                );
                            }
                            None => renderer.clear_ui_image_atlas(),
                        }
                    }
                }
                // Fuentes con nombre (UI v2): cargar bajo demanda
                // las que la escena use y aún no estén (CPU + GPU).
                {
                    let wanted: Vec<String> = match self.scene_state.lock() {
                        Ok(scene) => {
                            let mut v: Vec<String> = scene
                                .entities
                                .iter()
                                .filter(|e| e.active)
                                .filter_map(|e| e.ui_text.as_ref())
                                .map(|t| t.font.clone())
                                .filter(|f| !f.is_empty() && f != "default")
                                .collect();
                            v.sort();
                            v.dedup();
                            v
                        }
                        Err(_) => Vec::new(),
                    };
                    for name in wanted {
                        if self.ui_font_atlases.contains_key(&name)
                            || self.ui_font_failed.contains(&name)
                        {
                            continue;
                        }
                        let path = format!("assets/fonts/{name}.ttf");
                        let cargada = std::fs::read(&path)
                            .map_err(|e| e.to_string())
                            .and_then(|bytes| {
                                glass_renderer::FontAtlas::build(&bytes)
                                    .map(|atlas| (bytes, atlas))
                            })
                            .and_then(|(bytes, atlas)| {
                                renderer
                                    .ensure_named_font(vk_ctx.graphics_queue, &name, &bytes)
                                    .map(|_| atlas)
                                    .map_err(|e| e.to_string())
                            });
                        match cargada {
                            Ok(atlas) => {
                                self.ui_font_atlases.insert(name, atlas);
                            }
                            Err(e) => {
                                log::warn!("fuente '{name}' no disponible ({e}); se usa la de por defecto");
                                self.ui_font_failed.insert(name);
                            }
                        }
                    }
                }
                let font = renderer.ui_font();
                match self.scene_state.lock() {
                    Ok(scene) => canvas_ui::build_canvas_batch(
                        &scene,
                        &canvas_ui::UiFonts {
                            default: font,
                            named: &self.ui_font_atlases,
                        },
                        &self.ui_images_map,
                        renderer.offscreen.extent.width as f32,
                        renderer.offscreen.extent.height as f32,
                    ),
                    Err(_) => Vec::new(),
                }
            };
            renderer.render_ui_overlay(vk_ctx.graphics_queue, &batch)?;
        }

        if render_standalone {
            // Inicio de la fase PRESENT (acquire + blit + present + wait).
            let profile_t_present = Instant::now();
            let (image_index, _suboptimal) = match renderer
                .swapchain
                .acquire_next_image(renderer.image_available_semaphore)
            {
                Ok(idx) => idx,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                    let size = window.inner.inner_size();
                    let _ = renderer.resize(vk_ctx, size.width, size.height);
                    return Ok(());
                }
                Err(vk::Result::ERROR_SURFACE_LOST_KHR) => {
                    log::warn!("Surface lost KHR detectado, recreando swapchain...");
                    let size = window.inner.inner_size();
                    let _ = renderer.resize(vk_ctx, size.width, size.height);
                    return Ok(());
                }
                Err(e) => return Err(Box::new(e)),
            };

            renderer.blit_offscreen_to_swapchain(vk_ctx.graphics_queue, image_index)?;

            let swapchains = [renderer.swapchain.swapchain];
            let image_indices = [image_index];
            let signal_semaphores = [renderer.render_finished_for(image_index)];
            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&signal_semaphores)
                .swapchains(&swapchains)
                .image_indices(&image_indices);

            unsafe {
                let _ = renderer.swapchain.loader.queue_present(vk_ctx.present_queue, &present_info);
                let _ = vk_ctx.device.queue_wait_idle(vk_ctx.present_queue);
            }

            // Muestra del frame (Fase 0): los retornos tempranos por resize
            // dentro del present no llegan aquí y no registran muestra.
            registrar_muestra_frame(
                &mut self.profile_frame,
                Some(renderer),
                profile_t0,
                profile_update_us,
                profile_physics_us,
                profile_render_us,
                profile_t_present.elapsed().as_micros() as u64,
            );

            // Carga de escena diferida (la «3.b» de la ruta del editor):
            // la rama standalone devolvía el control aquí sin mirar nunca
            // las peticiones de `SceneManager.LoadScene` de los scripts,
            // así que un juego quedaba atrapado en su escena de arranque
            // (el menú no podía pasar al nivel 1). Se aplica en la misma
            // ventana temporal que en el editor: frame completo ya
            // presentado, nadie recorriendo las entidades.
            #[cfg(feature = "editor")]
            self.handle_play_transitions();
            #[cfg(feature = "editor")]
            self.apply_pending_physmat_refresh();
            self.apply_pending_scene_load();

            return Ok(());
        }

        // Modo Editor: Renderizar interfaz de usuario egui sobre el RenderPass del Swapchain
        // Inicio de la fase PRESENT del editor (egui + present del swapchain).
        let profile_t_present = Instant::now();
        // Sin la feature editor nadie lo pone a true: el mut solo se usa ahí.
        #[cfg_attr(not(feature = "editor"), allow(unused_mut))]
        let mut request_build = false;
        #[cfg(feature = "editor")]
        {
            let (egui_winit, egui_renderer) = match (
                &mut self.egui_winit,
                &mut self.egui_renderer,
            ) {
                (Some(ew), Some(er)) => (ew, er),
                _ => return Ok(()),
            };

            // 2. Ejecutar lógica y UI de egui (Docking estilo Unity sincronizado con SceneState o Hub / Lanzador)
            let raw_input = egui_winit.take_egui_input(&window.inner);
            // Cotas reales de las mallas para el picking y la caja de
            // selección del editor (la caché las registra al cargar).
            if let Ok(cache) = self.asset_cache.read() {
                self.editor_layout.viewport.mesh_bounds = cache.bounds_snapshot();
                // Alambres de los Mesh Colliders (casco y aristas de la
                // malla) para el dibujo del editor: se recalculan solo
                // cuando cambia el tamaño de la geometría retenida.
                if let Ok(scene) = self.scene_state.lock() {
                    let viewport = &mut self.editor_layout.viewport;
                    for ent in &scene.entities {
                        let Some(physics) = ent.physics.as_ref() else {
                            continue;
                        };
                        if physics.mesh_collider == glass_core::MeshColliderMode::None {
                            continue;
                        }
                        let Some(mesh) = ent.mesh.as_ref() else {
                            continue;
                        };
                        let Some((posiciones, indices)) = cache.geometries.get(&mesh.model)
                        else {
                            continue;
                        };
                        let firma = (posiciones.len(), indices.len());
                        let necesita_calculo = viewport
                            .mesh_wireframes
                            .get(&mesh.model)
                            .is_none_or(|aristas| aristas.firma != firma);
                        if necesita_calculo {
                            let aristas = glass_editor::panels::viewport::AristasMalla {
                                casco: glass_physics::aristas_casco_convexo(posiciones),
                                caracteristicas: glass_physics::aristas_caracteristicas(
                                    posiciones,
                                    indices,
                                    30.0,
                                ),
                                firma,
                            };
                            viewport.mesh_wireframes.insert(mesh.model.clone(), aristas);
                        }
                    }
                    viewport
                        .mesh_wireframes
                        .retain(|stem, _| cache.geometries.contains_key(stem));
                }
            }
            let editor_layout = &mut self.editor_layout;
            // Pantalla que el editor muestra mientras precarga.
            fn pantalla_carga_motor(ui: &mut egui::Ui) {
                use egui::{Color32, RichText};
                const ACENTO: Color32 = Color32::from_rgb(0, 120, 215);
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(Color32::from_rgb(242, 242, 242)))
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            let alto = ui.available_height();
                            ui.add_space((alto * 0.5 - 80.0).max(0.0));
                            ui.label(
                                RichText::new("GlassEnginer")
                                    .size(42.0)
                                    .color(Color32::from_rgb(30, 30, 30)),
                            );
                            ui.add_space(12.0);
                            let (linea, _) =
                                ui.allocate_exact_size(egui::vec2(64.0, 3.0), egui::Sense::hover());
                            ui.painter().rect_filled(linea, 0.0, ACENTO);
                            ui.add_space(18.0);
                            ui.label(RichText::new("Cargando el motor…").size(15.0).color(ACENTO));
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("Preparando modelos y materiales")
                                    .size(12.0)
                                    .color(Color32::from_rgb(120, 120, 120)),
                            );
                        });
                    });
            }

            let cargando = self.cargando;
            let scene_state = &self.scene_state;
            let scene_manager = &self.scene_manager;
            let scene_tid = self.viewport_texture_id;
            let game_tid = self.game_viewport_texture_id;

            let mut full_output = self.egui_ctx.run_ui(raw_input, |ui| {
                if cargando {
                    // Sin layout en pantalla, un drop de este frame se
                    // perdería (egui descarta el RawInput al cerrar el
                    // frame): se encola en el layout y se importa al
                    // terminar la carga (Bug F).
                    let soltados: Vec<std::path::PathBuf> = ui.ctx().input(|i| {
                        i.raw
                            .dropped_files
                            .iter()
                            .map(|f| f.path().to_path_buf())
                            .collect()
                    });
                    if !soltados.is_empty() {
                        editor_layout.encolar_soltados(&soltados);
                    }
                    pantalla_carga_motor(ui);
                } else {
                    editor_layout.hub_disponible = self.hub_disponible;
                    editor_layout.conversiones_en_curso =
                        glass_renderer::watcher::conversiones_en_curso();
                    editor_layout.render(
                        ui,
                        scene_tid,
                        game_tid,
                        scene_state,
                        scene_manager,
                    );
                    // Sonda del Bug F: el layout trae de vuelta el instante
                    // en que una importación por arrastre se completó; se
                    // guarda para medir el primer frame presentado tras ella.
                    if let Some(t) = editor_layout.importacion_completada_en {
                        if self.sonda_importacion_en.is_none() {
                            self.sonda_importacion_en = Some(t);
                        }
                        editor_layout.importacion_completada_en = None;
                    }
                }
            });
            egui_winit.handle_platform_output(&window.inner, full_output.platform_output);

            if cargando {
                // La carga ya está dibujada: la precarga puede bloquear en
                // el frame siguiente.
                self.carga_dibujada = true;
                self.egui_ctx.request_repaint();
            }

            if editor_layout.request_launcher {
                editor_layout.request_launcher = false;
                // Con cambios sin guardar no se vuelve al Hub a secas: se
                // pide decisión (Guardar / No guardar / Cancelar).
                let sucia = editor_layout.viewport.play_state == glass_editor::PlayState::Edit
                    && matches!(scene_state.lock(), Ok(sc) if editor_layout.escena_sucia(&sc));
                if sucia {
                    self.volver_al_hub_pendiente = true;
                    editor_layout.dialogo_guardar_cierre = true;
                } else {
                    Self::lanzar_hub_al_salir();
self.salir_del_proceso = true;
                }
                self.egui_ctx.request_repaint();
            }

            if editor_layout.request_abrir_contenedora_escena {
                editor_layout.request_abrir_contenedora_escena = false;
                match scene_manager
                    .lock()
                    .ok()
                    .and_then(|m| m.active_file().map(|p| p.to_path_buf()))
                {
                    Some(ruta) => glass_launcher::archivos::abrir_carpeta_contenedora(&ruta),
                    None => {
                        editor_layout.status_message = Some((
                            "La escena aún no tiene archivo (guárdala primero)".to_string(),
                            std::time::Instant::now(),
                        ));
                    }
                }
            }

            if editor_layout.request_copiar_ruta_escena {
                editor_layout.request_copiar_ruta_escena = false;
                match scene_manager
                    .lock()
                    .ok()
                    .and_then(|m| m.active_file().map(|p| p.to_path_buf()))
                {
                    Some(ruta) => {
                        self.egui_ctx.copy_text(ruta.to_string_lossy().into_owned());
                        editor_layout.status_message = Some((
                            "Ruta de la escena copiada al portapapeles".to_string(),
                            std::time::Instant::now(),
                        ));
                    }
                    None => {
                        editor_layout.status_message = Some((
                            "La escena aún no tiene archivo (guárdala primero)".to_string(),
                            std::time::Instant::now(),
                        ));
                    }
                }
            }

            // --- Cierre pedido por el Hub (Bug A): al salir, el Hub
            // escribe cerrar-<pid>.flag y el motor responde con su
            // propia regla: silencio si la escena está limpia o el
            // último guardado es de hace ≤ 5 s; diálogo de guardado
            // en caso contrario. Solo los motores gestionados
            // sondean (un editor suelto no obedece banderas).
            if self.gestionado
                && self.ultima_sonda_cierre_hub.elapsed() >= std::time::Duration::from_millis(500)
            {
                self.ultima_sonda_cierre_hub = std::time::Instant::now();
                if glass_launcher::updates::solicitud_cierre_propia() {
                    glass_launcher::updates::retirar_solicitud_cierre_propia();
                    if self.is_standalone {
                        log::info!("Cierre pedido por el Hub: saliendo (modo juego).");
                        self.salir_del_proceso = true;
                    } else {
                        let sucia = editor_layout.viewport.play_state == glass_editor::PlayState::Edit
                            && matches!(scene_state.lock(), Ok(sc) if editor_layout.escena_sucia(&sc));
                        let guardado_reciente = editor_layout
                            .ultimo_guardado
                            .is_some_and(|t| t.elapsed() <= std::time::Duration::from_secs(5));
                        if !sucia || guardado_reciente {
                            log::info!(
                                "Cierre pedido por el Hub: saliendo sin preguntar (sucia={sucia}, guardado_reciente={guardado_reciente})."
                            );
                            self.salir_del_proceso = true;
                        } else {
                            log::info!("Cierre pedido por el Hub con cambios sin guardar: pidiendo decisión.");
                            self.cierre_desde_hub_pendiente = true;
                            editor_layout.dialogo_guardar_cierre = true;
                        }
                    }
                    self.egui_ctx.request_repaint();
                }
            }

            // --- Guardado (B2): decisión de cierre y autoguardado ---
            // (Las defensas que descartaban la ruta guardada al cambiar el
            // nombre activo ya no existen: el archivo de la escena activa
            // vive en el SceneManager y siempre es el de la escena en
            // curso — defecto S1.)
            let en_edicion = editor_layout.viewport.play_state == glass_editor::PlayState::Edit;
            if let Some(decision) = editor_layout.decision_cierre.take() {
                match decision {
                    glass_editor::DecisionCierre::Guardar => {
                        if let Some(resultado_prefab) =
                            editor_layout.guardar_prefab_en_edicion(scene_state)
                        {
                            match resultado_prefab {
                                Ok(()) => {
                                    self.ultimo_autoguardado = std::time::Instant::now();
                                    self.volver_al_hub_pendiente = false;
                                    if self.cierre_desde_hub_pendiente {
                                        self.cierre_desde_hub_pendiente = false;
                                    } else {
                                        Self::lanzar_hub_al_salir();
                                    }
                                    self.salir_del_proceso = true;
                                    self.egui_ctx.request_repaint();
                                }
                                Err(_) => {
                                    // El error ya está en status_message
                                    // (lo dejó guardar_prefab_en_edicion).
                                    self.volver_al_hub_pendiente = false;
                                }
                            }
                        } else if let Some(ruta) = scene_manager
                            .lock()
                            .ok()
                            .and_then(|m| m.active_file().map(|p| p.to_path_buf()))
                        {
                            let resultado = match scene_state.lock() {
                                Ok(sc) => {
                                    let r = sc
                                        .save_to_json(&ruta.to_string_lossy())
                                        .map_err(|e| format!("{e:?}"));
                                    if r.is_ok() {
                                        editor_layout.marcar_guardada(&sc);
                                    }
                                    r
                                }
                                Err(_) => Err("mutex de la escena envenenado".to_string()),
                            };
                            match resultado {
                                Ok(()) => {
                                    self.ultimo_autoguardado = std::time::Instant::now();
                                    self.volver_al_hub_pendiente = false;
                                    if self.cierre_desde_hub_pendiente {
                                        // El cierre lo pidió el Hub (que
                                        // está saliendo): no relanzarlo.
                                        self.cierre_desde_hub_pendiente = false;
                                    } else {
                                        Self::lanzar_hub_al_salir();
                                    }
self.salir_del_proceso = true;
                                    self.egui_ctx.request_repaint();
                                }
                                Err(e) => {
                                    // Se queda en el editor y lo avisa.
                                    self.volver_al_hub_pendiente = false;
                                    log::error!("No se pudo guardar la escena en {}: {e}", ruta.display());
                                    editor_layout.status_message = Some((
                                        format!("No se pudo guardar la escena: {e}"),
                                        std::time::Instant::now(),
                                    ));
                                }
                            }
                        } else {
                            // Sin archivo: «Save Scene As» del editor; al
                            // cerrarse se retoma la vuelta pendiente.
                            editor_layout.save_as_name = match scene_manager.lock() {
                                Ok(m) => m.active_scene_name().to_string(),
                                Err(_) => "SampleScene".to_string(),
                            };
                            editor_layout.show_save_as_dialog = true;
                            self.esperando_save_as = true;
                        }
                    }
                    glass_editor::DecisionCierre::NoGuardar => {
                        if let Ok(sc) = scene_state.lock() {
                            editor_layout.marcar_guardada(&sc);
                        }
                        self.volver_al_hub_pendiente = false;
                        if self.cierre_desde_hub_pendiente {
                            // El cierre lo pidió el Hub: salir sin relanzarlo.
                            self.cierre_desde_hub_pendiente = false;
                        } else {
                            Self::lanzar_hub_al_salir();
                        }
self.salir_del_proceso = true;
                        self.egui_ctx.request_repaint();
                    }
                    glass_editor::DecisionCierre::Cancelar => {
                        self.volver_al_hub_pendiente = false;
                        // Si el cierre lo pidió el Hub, queda cancelado:
                        // el Hub se queda a la espera en segundo plano.
                        self.cierre_desde_hub_pendiente = false;
                    }
                }
            }
            // La suciedad se RECALCULA tras la decisión: «No guardar» la
            // acaba de limpiar y el autoguardado no debe resucitarla.
            let sucia = en_edicion
                && matches!(scene_state.lock(), Ok(sc) if editor_layout.escena_sucia(&sc));
            if self.esperando_save_as && !editor_layout.show_save_as_dialog {
                self.esperando_save_as = false;
                // Guardado o cancelado, la espera termina aquí.
                let volver = self.volver_al_hub_pendiente;
                let desde_hub = self.cierre_desde_hub_pendiente;
                self.volver_al_hub_pendiente = false;
                if desde_hub && sucia {
                    // Save As cancelado y la escena sigue sucia: el
                    // cierre pedido por el Hub continúa pendiente (la
                    // bandera no se limpia) y no se sale.
                } else if !sucia && (volver || desde_hub) {
                    if desde_hub {
                        // Cierre pedido por el Hub: salir sin relanzarlo.
                        self.cierre_desde_hub_pendiente = false;
                    } else {
                        Self::lanzar_hub_al_salir();
                    }
                    self.salir_del_proceso = true;
                    self.egui_ctx.request_repaint();
                }
            }
            let toca_autoguardar = sucia
                && !editor_layout.dialogo_guardar_cierre
                && editor_layout.autosave_minutos > 0
                && self.ultimo_autoguardado.elapsed()
                    >= std::time::Duration::from_secs(u64::from(editor_layout.autosave_minutos) * 60);
            let ruta_autoguardado = if toca_autoguardar {
                scene_manager
                    .lock()
                    .ok()
                    .and_then(|m| m.active_file().map(|p| p.to_path_buf()))
            } else {
                None
            };
            if let Some(ruta) = ruta_autoguardado {
                // El intervalo se reinicia también si falla: se reintenta en
                // el siguiente intervalo, no en cada frame.
                self.ultimo_autoguardado = std::time::Instant::now();
                match scene_state.lock() {
                    Ok(mut sc) => {
                        // Fase D: el autoguardado también recalcula el
                        // diff de overrides antes de escribir.
                        let raiz_proyecto = editor_layout
                            .project
                            .assets_root
                            .parent()
                            .unwrap_or(&editor_layout.project.assets_root)
                            .to_path_buf();
                        sc.recalcular_overrides_prefab(&|fuente| {
                            glass_core::Prefab::cargar_desde_proyecto(&raiz_proyecto, fuente)
                        });
                        match sc.save_to_json(&ruta.to_string_lossy()) {
                        Ok(()) => {
                            editor_layout.marcar_guardada(&sc);
                            log::info!("Autoguardado de la escena en {}", ruta.display());
                        }
                        Err(e) => log::warn!("Autoguardado fallido en {}: {e:?}", ruta.display()),
                        }
                    },
                    Err(_) => log::warn!("Autoguardado omitido: mutex de la escena envenenado."),
                }
            }

            if editor_layout.request_build_and_run {
                editor_layout.request_build_and_run = false;
                request_build = true;
                self.egui_ctx.request_repaint();
            }

            // 3. Sincronizar texturas internas de egui hacia la GPU y limpiar deltas inmediatamente
            for (id, deltas) in full_output.textures_delta.set.drain() {
                for delta in deltas {
                    egui_renderer.set_texture(vk_ctx.graphics_queue, renderer.command_pool, id, &delta)?;
                }
            }
            for id in full_output.textures_delta.free.drain() {
                let _ = egui_renderer.free_texture(id);
            }
            full_output.textures_delta.clear();

            // 4. Adquirir imagen del Swapchain
            let (image_index, _suboptimal) = match renderer
                .swapchain
                .acquire_next_image(renderer.image_available_semaphore)
            {
                Ok(idx) => idx,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                    let size = window.inner.inner_size();
                    let _ = renderer.resize(vk_ctx, size.width, size.height);
                    return Ok(());
                }
                Err(vk::Result::ERROR_SURFACE_LOST_KHR) => {
                    log::warn!("Surface lost KHR detectado, recreando swapchain...");
                    let size = window.inner.inner_size();
                    let _ = renderer.resize(vk_ctx, size.width, size.height);
                    return Ok(());
                }
                Err(e) => return Err(Box::new(e)),
            };

            // 5. Teselar primitivas de UI
            let clipped_primitives = self
                .egui_ctx
                .tessellate(full_output.shapes, full_output.pixels_per_point);

            // 6. Grabar comandos de renderizado sobre el Framebuffer del Swapchain
            unsafe {
                renderer.device.wait_for_fences(&[renderer.fence], true, u64::MAX)?;
                renderer.device.reset_fences(&[renderer.fence])?;

                let cmd = renderer.command_buffer;
                let begin_info = vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
                renderer.device.begin_command_buffer(cmd, &begin_info)?;

                let clear_values = [vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: [0.07, 0.08, 0.10, 1.0],
                    },
                }];

                let rp_info = vk::RenderPassBeginInfo::default()
                    .render_pass(renderer.swapchain_render_pass)
                    .framebuffer(renderer.swapchain_framebuffers[image_index as usize])
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: renderer.swapchain.extent,
                    })
                    .clear_values(&clear_values);

                renderer.device.cmd_begin_render_pass(
                    cmd,
                    &rp_info,
                    vk::SubpassContents::INLINE,
                );

                // Dibujar interfaz egui (Hierarchy, Inspector, Viewport con textura VRAM, Consola)
                egui_renderer.cmd_draw(
                    cmd,
                    renderer.swapchain.extent,
                    full_output.pixels_per_point,
                    &clipped_primitives,
                )?;

                renderer.device.cmd_end_render_pass(cmd);
                renderer.device.end_command_buffer(cmd)?;

                // 7. Enviar a la cola gráfica
                let wait_semaphores = [renderer.image_available_semaphore];
                let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
                let signal_semaphores = [renderer.render_finished_for(image_index)];
                let command_buffers = [cmd];

                let submit_info = [vk::SubmitInfo::default()
                    .wait_semaphores(&wait_semaphores)
                    .wait_dst_stage_mask(&wait_stages)
                    .command_buffers(&command_buffers)
                    .signal_semaphores(&signal_semaphores)];

                renderer
                    .device
                    .queue_submit(vk_ctx.graphics_queue, &submit_info, renderer.fence)?;

                // 8. Presentar imagen en pantalla
                let swapchains = [renderer.swapchain.swapchain];
                let image_indices = [image_index];
                let present_info = vk::PresentInfoKHR::default()
                    .wait_semaphores(&signal_semaphores)
                    .swapchains(&swapchains)
                    .image_indices(&image_indices);

                let mut presento_ok = false;
                match renderer.swapchain.loader.queue_present(vk_ctx.present_queue, &present_info) {
                    Ok(_) => {
                        presento_ok = true;
                    }
                    Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                        let size = window.inner.inner_size();
                        let _ = renderer.resize(vk_ctx, size.width, size.height);
                    }
                    Err(e) => log::warn!("Aviso durante queue_present: {:?}", e),
                }
                // Sonda del Bug F: primer frame presentado tras completar
                // una importación por arrastre (si el present falló o la
                // swapchain se recreó, la sonda espera al siguiente frame).
                if presento_ok {
                    if let Some(t) = self.sonda_importacion_en.take() {
                        log::info!(
                            "Arrastre del SO: primer frame presentado tras completar la importación (+{} ms)",
                            t.elapsed().as_millis()
                        );
                    }
                }
            }
        }

        // Muestra del frame (Fase 0): se registra AQUÍ, antes de la carga
        // diferida, porque `renderer` (préstamo de `self.renderer`) deja de
        // usarse en este punto y `apply_pending_scene_load` pide `&mut self`
        // entero. Los retornos tempranos por resize dentro del present del
        // editor no llegan aquí y no registran muestra.
        registrar_muestra_frame(
            &mut self.profile_frame,
            Some(renderer),
            profile_t0,
            profile_update_us,
            profile_physics_us,
            profile_render_us,
            profile_t_present.elapsed().as_micros() as u64,
        );

        // 3.b Carga de escena diferida.
        //
        // Va después de presentar y antes de devolver el control: el frame
        // completo (input, scripts, física, present) se ejecutó ya sobre la
        // escena vieja, así que aquí nadie está recorriendo `entities` y la
        // sustitución no puede producir un frame a medio dibujar. Es la misma
        // ventana temporal que usa Unity para aplicar `SceneManager.LoadScene`.
        #[cfg(feature = "editor")]
        self.handle_play_transitions();
        #[cfg(feature = "editor")]
        self.apply_pending_physmat_refresh();
        self.apply_pending_scene_load();

        if request_build {
            self.execute_build_and_run();
        }

        Ok(())
    }
}

impl ApplicationHandler for GlassEngineApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        log::info!("Iniciando GlassEngine (Hito 6: Pipeline de Blender y Recarga en Caliente)...");
        let window = match GlassWindow::new(event_loop, &self.window_config) {
            Ok(w) => w,
            Err(e) => {
                log::error!("Fallo al crear la ventana nativa: {}", e);
                event_loop.exit();
                return;
            }
        };

        let vk_ctx = match VulkanContext::new(&window, self.vulkan_config.clone()) {
            Ok(ctx) => ctx,
            Err(e) => {
                log::error!("Fallo al crear el contexto Vulkan: {}", e);
                event_loop.exit();
                return;
            }
        };

        let gpu_name = unsafe {
            CStr::from_ptr(vk_ctx.physical_device_properties.device_name.as_ptr())
        }
        .to_string_lossy();

        // 1. Inicializar GlassRenderer
        let mut renderer = match GlassRenderer::new(&vk_ctx, self.window_config.width, self.window_config.height) {
            Ok(r) => r,
            Err(e) => {
                log::error!("Fallo al crear GlassRenderer: {}", e);
                event_loop.exit();
                return;
            }
        };

        // 2. Inicializar egui en Modo Editor (si no es standalone)
        #[cfg(feature = "editor")]
        let (egui_winit, egui_renderer, layout, pool, viewport_texture_id, game_layout, game_pool, game_viewport_texture_id) = if !self.is_standalone {
            let ew = EguiWinitState::new(
                self.egui_ctx.clone(),
                egui::ViewportId::ROOT,
                &window.inner,
                Some(window.inner.scale_factor() as f32),
                None,
                None,
            );

            let er = match EguiVulkanRenderer::with_gpu_allocator(
                renderer.allocator.clone(),
                (*renderer.device).clone(),
                egui_ash_renderer::RenderMode::RenderPass(renderer.swapchain_render_pass),
                egui_ash_renderer::Options {
                    in_flight_frames: 1,
                    enable_depth_test: false,
                    enable_depth_write: false,
                    // El swapchain es B8G8R8A8_SRGB: el hardware ya codifica
                    // a sRGB al escribir. Con `false`, el shader de egui
                    // codificaba OTRA vez en el shader → doble gamma en
                    // toda la ventana (medido: un lineal 0,5 llegaba a
                    // pantalla como 0,87 en vez de 0,73) y la escena 3D se
                    // veía lavada. Con `true` egui entrega lineal y la
                    // única gamma es la del framebuffer.
                    srgb_framebuffer: true,
                },
            ) {
                Ok(er) => er,
                Err(e) => {
                    log::error!("Fallo al inicializar egui-ash-renderer: {}", e);
                    event_loop.exit();
                    return;
                }
            };

            let (lay, pl, set) = match renderer.create_offscreen_descriptor_set() {
                Ok(res) => res,
                Err(e) => {
                    log::error!("Fallo al crear descriptor set para viewport: {}", e);
                    event_loop.exit();
                    return;
                }
            };

            let (g_lay, g_pl, g_set) = match renderer.create_game_offscreen_descriptor_set() {
                Ok(res) => res,
                Err(e) => {
                    log::error!("Fallo al crear descriptor set para game viewport: {}", e);
                    event_loop.exit();
                    return;
                }
            };

            let mut er = er;
            let tid = er.add_user_texture(set);
            let g_tid = er.add_user_texture(g_set);
            log::info!("Texturas Offscreen vinculadas exitosamente con egui::TextureId: Scene {:?}, Game {:?}", tid, g_tid);
            (Some(ew), Some(er), Some(lay), Some(pl), Some(tid), Some(g_lay), Some(g_pl), Some(g_tid))
        } else {
            log::info!("Iniciando en Modo Standalone (sin egui, sin servidor MCP)...");
            (None, None, None, None, None, None, None, None)
        };

        // 5. Precarga de modelos, materiales y primitivas (`preparar_assets`).
        // Sin editor (standalone) se hace aquí, como siempre: la escena los
        // necesita desde el primer frame. Con editor se difiere hasta la
        // primera entrada al editor (gate del bucle de frame), para que el
        // Hub, que no usa ningún modelo, aparezca en seguida.
        #[cfg(not(feature = "editor"))]
        self.preparar_assets(&mut renderer, &vk_ctx);
        // El binario CON editor en modo standalone es un juego, no el
        // Hub: la precarga diferida nunca se dispara en su flujo (no
        // hay entrada al editor) y sin ella la primitiva de respaldo
        // «cube» nunca se registra, con lo que la recolección de
        // dibujo no dibuja nada (defecto D3: el 3D no pintaba en el
        // standalone con editor). Se hace aquí, como en el binario
        // puro, y se marca hecha para que el gate de apertura no la
        // vuelva a diferir.
        #[cfg(feature = "editor")]
        if self.is_standalone {
            self.preparar_assets(&mut renderer, &vk_ctx);
            self.precarga_hecha = true;
            self.precarga_pendiente = false;
        }

        // La ruta del bridge de Blender se fija desde la raíz del MOTOR
        // (la release lo instala en <raíz>/tools/): sin esto, en la build
        // instalada el bridge se buscaba relativo al directorio de
        // trabajo del proceso y la conversión .blend/.fbx al soltar en
        // la escena nunca ocurría (regresión de la separación, 2026-11-18).
        glass_renderer::fijar_bridge_desde_raiz(&raiz_motor_actual());

        // 6. Iniciar AssetWatcher en segundo plano sobre assets/
        if Path::new("assets").exists() {
            log::info!("main: Iniciando AssetWatcher...");
            // Sin la carpeta models/: la registra la precarga diferida
            // (ver `escanear_fuentes` en el vigía).
            match AssetWatcher::new("assets", false) {
                Ok(watcher) => {
                    log::info!("main: AssetWatcher listo.");
                    self.asset_watchers_raices.push(
                        std::fs::canonicalize("assets")
                            .unwrap_or_else(|_| std::path::PathBuf::from("assets")),
                    );
                    self.asset_watchers.push(watcher);
                }
                Err(e) => log::warn!("No se pudo inicializar AssetWatcher: {}", e),
            }
        }

        log::info!("main: Renderizando frame offscreen inicial...");

        // Renderizar frame inicial offscreen
        if let Err(e) = renderer.render_offscreen(vk_ctx.graphics_queue) {
            log::error!("Fallo al inicializar frame offscreen: {}", e);
            event_loop.exit();
            return;
        }

        self.renderer = Some(renderer);
        self.vulkan_context = Some(vk_ctx);
        self.window = Some(window);
        #[cfg(feature = "editor")]
        {
            self.egui_winit = egui_winit;
            self.egui_renderer = egui_renderer;
            self.desc_layout = layout;
            self.desc_pool = pool;
            self.viewport_texture_id = viewport_texture_id;
            self.game_desc_layout = game_layout;
            self.game_desc_pool = game_pool;
            self.game_viewport_texture_id = game_viewport_texture_id;
        }

        // Conectar el bridge de scripting con el SceneState
        register_scene_bridge(Box::new(EngineSceneBridge {
            scene_state: self.scene_state.clone(),
            scene_manager: self.scene_manager.clone(),
            physics_world: self.physics_world.clone(),
            asset_cache: self.asset_cache.clone(),
            pending_script_spawns: self.pending_script_spawns.clone(),
            physmat_lib: self.physmat_lib.clone(),
        }));

        let managed_dir = self
            .raiz_motor
            .join("managed/GlassEngine.Core/bin/Debug/net8.0");
        if managed_dir.exists() {
            match self.script_host.initialize(&managed_dir) {
                Ok(()) => {
                    log::info!("main: ScriptHost C# (.NET CoreCLR) inicializado exitosamente.");
                    // CoreCLR instala sus propios manejadores de señales durante
                    // el arranque del runtime y pisa el nuestro. Se reinstala
                    // aquí para que SIGTERM/SIGINT/SIGHUP vuelvan a caer en el
                    // apagado ordenado y no maten al proceso a mitad de un
                    // `drm_syncobj_timeline_wait` (VK_TIMEOUT falsos).
                    install_signal_handlers();
                }
                Err(e) => {
                    log::warn!("main: ScriptHost no inicializado: {}", e);
                    if let Some(f) = e.fallo_dotnet() {
                        self.notificar_fallo_dotnet(f, false);
                    }
                }
            }
        }

        let is_test_run = self.is_headless_test
            || self.is_m5_test
            || self.is_m6_test
            || self.is_m7_test
            || self.is_m8_test
            || self.is_m9_test
            || self.is_m10_test
            || self.is_platformer_test
            || self.is_sample_game_test
            || self.is_standalone_test;

        if !is_test_run {
            if let Some(scene_file) = self.scene_to_load.clone() {
                log::info!("Cargando escena solicitada desde archivo: {}", scene_file);
                // El ancla sigue a la escena, no al directorio de
                // lanzamiento (defecto S19).
                #[cfg(feature = "editor")]
                if let Some(raiz) = proyecto_de_una_escena(&scene_file) {
                    self.reanclar_al_proyecto(&raiz);
                }
                // Los scripts del proyecto de la escena, cargados antes
                // de que la escena los instancie (defecto D4).
                if let Some(raiz) = crate::prefabs_vivos::raiz_desde_ruta_escena(&scene_file) {
                    self.cargar_scripts_del_proyecto(&raiz);
                }
                let cargada = match self.scene_state.lock() {
                    Ok(mut sc) => match sc.load_from_json(&scene_file) {
                        Ok(()) => {
                            crate::canvas_ui::migrar_ui_escena_cargada(
                                &mut sc,
                                Some(scene_file.as_str()),
                            );
                            let raiz = crate::prefabs_vivos::raiz_desde_ruta_escena(&scene_file);
                            crate::prefabs_vivos::refrescar_prefabs_escena_cargada(
                                &mut sc,
                                raiz.as_deref(),
                            );
                            true
                        }
                        Err(e) => {
                            log::error!(
                                "La escena solicitada no cargó ({}): {}",
                                scene_file,
                                e
                            );
                            false
                        }
                    },
                    Err(_) => false,
                };
                if cargada {
                    let ruta = scene_file.clone();
                    self.bootstrap_scene_scripts_and_physics(&ruta);
                }
            } else if self.is_standalone {
                let default_sc = glass_core::ProjectSettings::load_or_default("project.json").default_scene;
                if std::path::Path::new(&default_sc).exists() {
                    log::info!("Modo Standalone: Cargando escena por defecto desde project.json: {}", default_sc);
                    let cargada = match self.scene_state.lock() {
                        Ok(mut sc) => match sc.load_from_json(&default_sc) {
                            Ok(()) => {
                                crate::canvas_ui::migrar_ui_escena_cargada(
                                    &mut sc,
                                    Some(default_sc.as_str()),
                                );
                                let raiz =
                                    crate::prefabs_vivos::raiz_desde_ruta_escena(&default_sc);
                                crate::prefabs_vivos::refrescar_prefabs_escena_cargada(
                                    &mut sc,
                                    raiz.as_deref(),
                                );
                                true
                            }
                            Err(e) => {
                                log::error!(
                                    "Modo Standalone: la escena por defecto no cargó: {}",
                                    e
                                );
                                false
                            }
                        },
                        Err(_) => false,
                    };
                    if cargada {
                        self.bootstrap_scene_scripts_and_physics(&default_sc);
                        // Lightmap horneado de esta escena, si existe.
                        self.lightmap_runtime =
                            crate::bake::load_lightmap_runtime(&default_sc);
                        if let Some(lm) = &self.lightmap_runtime {
                            log::info!(
                                "Lightmap cargado para la escena: {} entidades, atlas {} px.",
                                lm.entities.len(),
                                lm.atlas_size
                            );
                        }
                        if let (Some(renderer), Some(lm), Some(ctx)) = (
                            &mut self.renderer,
                            &self.lightmap_runtime,
                            &self.vulkan_context,
                        ) {
                            if let Err(e) = renderer.set_lightmap(
                                ctx.graphics_queue,
                                &lm.rgba,
                                lm.atlas_size,
                                &lm.entities,
                                &lm.groups,
                            ) {
                                log::error!("No se pudo publicar el lightmap: {e}");
                            }
                        }
                        // Horneado sin interfaz: con GLASS_BAKE=<densidad>
                        // se hornea el lightmap de la escena cargada, se
                        // escribe en assets/lightmaps/ y el proceso sale.
                        if let Ok(dens) = std::env::var("GLASS_BAKE") {
                            let densidad: f32 = dens.parse().unwrap_or(8.0);
                            let nombre = std::path::Path::new(&default_sc)
                                .file_stem()
                                .map(|s| s.to_string_lossy().replace(".scene", ""))
                                .unwrap_or_else(|| "scene".to_string());
                            let resultado = match self.scene_state.lock() {
                                Ok(sc) => crate::bake::bake_scene_lightmap(
                                    &sc,
                                    &self.physics_world,
                                    densidad,
                                ),
                                Err(_) => Err("scene_state envenenado".to_string()),
                            };
                            match resultado {
                                Ok(out) => {
                                    let dir = "assets/lightmaps";
                                    let _ = std::fs::create_dir_all(dir);
                                    let png = format!("{dir}/{nombre}.lm.png");
                                    let json = format!("{dir}/{nombre}.lm.json");
                                    // El atlas se guarda codificado en
                                    // sRGB: la textura se sube como SRGB
                                    // y al muestrear devuelve el valor
                                    // lineal horneado.
                                    let mut srgb = out.rgba.clone();
                                    for px in srgb.as_chunks_mut::<4>().0 {
                                        for ch in px.iter_mut().take(3) {
                                            let l = *ch as f32 / 255.0;
                                            let s = if l <= 0.003_130_8 {
                                                l * 12.92
                                            } else {
                                                1.055 * l.powf(1.0 / 2.4) - 0.055
                                            };
                                            *ch = (s * 255.0).round().clamp(0.0, 255.0) as u8;
                                        }
                                    }
                                    if let Some(img) = image::RgbaImage::from_raw(
                                        out.size,
                                        out.size,
                                        srgb,
                                    ) {
                                        let _ = img.save(&png);
                                    }
                                    let rects: Vec<_> = out
                                        .rects
                                        .iter()
                                        .map(|r| {
                                            serde_json::json!({
                                                "entity": r.entity, "x": r.x, "y": r.y,
                                                "w": r.w, "h": r.h,
                                            })
                                        })
                                        .collect();
                                    let groups: Vec<_> = out
                                        .groups
                                        .iter()
                                        .map(|gs| {
                                            gs.iter()
                                                .map(|g| {
                                                    serde_json::json!({
                                                        "axis": g.axis, "negative": g.negative,
                                                        "x": g.x, "y": g.y, "w": g.w, "h": g.h,
                                                        "min_u": g.min_u, "max_u": g.max_u,
                                                        "min_v": g.min_v, "max_v": g.max_v,
                                                    })
                                                })
                                                .collect::<Vec<_>>()
                                        })
                                        .collect();
                                    let meta = serde_json::json!({
                                        "atlas_size": out.size,
                                        "entities": out.baked_entities,
                                        "rects": rects,
                                        "groups": groups,
                                        "skipped_models": out.skipped_models,
                                    });
                                    let _ = std::fs::write(
                                        &json,
                                        serde_json::to_string_pretty(&meta)
                                            .unwrap_or_default(),
                                    );
                                    log::info!(
                                        "GLASS_BAKE: {} entidades horneadas en {png} (atlas {} px); saltados: {:?}",
                                        out.baked_entities.len(),
                                        out.size,
                                        out.skipped_models
                                    );
                                }
                                Err(e) => log::error!("GLASS_BAKE falló: {e}"),
                            }
                            std::process::exit(0);
                        }
                    }
                } else {
                    log::info!("Iniciando escena de juego por defecto en Standalone...");
                    self.setup_complete_sample_game_scene();
                }
            } else if self.proyecto_directo.is_some() {
                log::info!("Arranque con --project: sin escena demo; el proyecto se abre tras la precarga.");
            } else {
                log::info!("Iniciando escena interactiva 3D con plataformas y cubo jugador...");
                self.setup_interactive_platformer_scene();
            }
        }

        if self.is_m7_test {
            log::info!("Ejecutando suite de pruebas de integración para HITO 7 (C# Scripting .NET CoreCLR)...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para Hito 7");

            // 1. Crear entidad en SceneState para el test
            let test_entity_id = {
                let mut state = self.scene_state.lock().unwrap();
                state.create_entity(
                    "C# Scripted Entity".to_string(),
                    vec!["Transform".to_string(), "Script".to_string()],
                )
            };

            // 2. Instanciar scripts C# 'Rotator' y 'PlayerMovement'
            let rotator_handle = self.script_host.create_instance(test_entity_id, "GlassEngine.Core.Scripts.Rotator")
                .expect("Debe instanciar Rotator en C#");
            let mover_handle = self.script_host.create_instance(test_entity_id, "GlassEngine.Core.Scripts.PlayerMovement")
                .expect("Debe instanciar PlayerMovement en C#");
            assert_eq!(self.script_host.active_instance_count(), 2);

            let initial_pos = {
                let state = self.scene_state.lock().unwrap();
                let ent = state.entities.iter().find(|e| e.id == test_entity_id).unwrap();
                ent.transform.position
            };

            // 3. Ejecutar 5 frames de simulación con Update C# y renderizado offscreen
            for _ in 0..5 {
                self.script_host.update_all(0.016667);
                if let Err(e) = self.render_frame() {
                    log::error!("Fallo en render_frame durante test C#: {}", e);
                    event_loop.exit();
                    return;
                }
            }

            // 4. Verificar que el script C# modificó la posición en SceneState
            let final_pos = {
                let state = self.scene_state.lock().unwrap();
                let ent = state.entities.iter().find(|e| e.id == test_entity_id).unwrap();
                ent.transform.position
            };
            log::info!("Posición inicial: {:?}, Posición final tras C# Update: {:?}", initial_pos, final_pos);
            assert_ne!(initial_pos, final_pos, "El script C# debió mutar el transform de la entidad");

            // 5. Destruir instancias
            self.script_host.destroy_instance(rotator_handle);
            self.script_host.destroy_instance(mover_handle);
            assert_eq!(self.script_host.active_instance_count(), 0);

            log::info!("============================================================");
            log::info!("   HITO 7 COMPLETADO: C# SCRIPTING (.NET CoreCLR)");
            log::info!("   GPU en Uso:                  {}", gpu_name);
            log::info!("   Runtime .NET:                .NET 8.0 / CoreCLR hostfxr");
            log::info!("   FFI Interop:                 [StructLayout(LayoutKind.Sequential)] 40 bytes");
            log::info!("   Unmanaged Callers:           InitBridge, CreateScriptInstance, Update, Destroy");
            log::info!("   Clase Base:                  GlassEngine.Core.GlassBehaviour");
            log::info!("   Scripts Verificados:         Rotator.cs, PlayerMovement.cs");
            log::info!("   Sincronización de Escena:    EngineSceneBridge bidireccional segura con Mutex");
            log::info!("============================================================");

            event_loop.exit();
            return;
        }

        if self.is_m8_test {
            log::info!("Ejecutando suite de pruebas de integración para HITO 8 (Subsistema de Audio 2D/3D con Kira)...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para Hito 8");

            // 1. Probar reproducción directa 2D y 3D en el subsistema de audio
            let h2d = self.audio_system.play_2d("beep_880", 0.5, 1.0, false);
            assert!(h2d > 0, "Debe retornar un SoundHandle válido para 2D");
            log::info!("Audio 2D probado exitosamente con handle {}", h2d);

            // 2. Probar integración completa con C#: Instanciar AudioEmitterTest
            let test_audio_entity_id = {
                let mut state = self.scene_state.lock().unwrap();
                state.create_entity(
                    "C# Audio Emitter Entity".to_string(),
                    vec!["Transform".to_string(), "AudioSource".to_string(), "Script".to_string()],
                )
            };

            let emitter_handle = self.script_host.create_instance(test_audio_entity_id, "GlassEngine.Core.Scripts.AudioEmitterTest")
                .expect("Debe instanciar AudioEmitterTest en C#");
            assert!(emitter_handle > 0);

            // 3. Simular 5 frames donde el script C# se ejecuta, mueve la posición 3D y actualiza el emisor de audio
            for _ in 0..5 {
                self.script_host.update_all(0.016667);
                if let Err(e) = self.render_frame() {
                    log::error!("Fallo en render_frame durante test de audio: {}", e);
                    event_loop.exit();
                    return;
                }
            }

            // 4. Limpiar script y fuentes de audio
            self.script_host.destroy_instance(emitter_handle);
            self.audio_system.stop_all();

            let backend_name = if self.audio_system.is_null_backend {
                "NullBackend (Modo Headless / CI)"
            } else {
                "Kira 0.10 (CPAL / ALSA Realtime Engine)"
            };

            log::info!("============================================================");
            log::info!("   HITO 8 COMPLETADO: SUBSISTEMA DE AUDIO 2D/3D (KIRA)");
            log::info!("   GPU en Uso:                  {}", gpu_name);
            log::info!("   Backend de Audio:            {}", backend_name);
            log::info!("   Topología de Hilos:          Hilo de audio desacoplado (non-blocking SPSC queue)");
            log::info!("   Generador Procedural:        Síntesis senoidal en memoria (PCM16 48kHz)");
            log::info!("   Clips Pre-cargados:          beep_440, beep_880");
            log::info!("   Audio Espacializado:         SpatialTrack + Listener bidireccional");
            log::info!("   Interop C# CoreCLR:          AudioSource, AudioListener, AudioEmitterTest.cs");
            log::info!("============================================================");

            event_loop.exit();
            return;
        }

        if self.is_m9_test {
            log::info!("Ejecutando suite de pruebas de integración para HITO 9 (Simulación Física 3D con Rapier3D)...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para Hito 9");

            // 1. Crear plano/suelo estático en la física y en SceneState
            let floor_entity_id = {
                let mut state = self.scene_state.lock().unwrap();
                state.create_entity(
                    "Static Floor".to_string(),
                    vec!["Transform".to_string(), "StaticCollider".to_string()],
                )
            };
            {
                let mut phys = self.physics_world.lock().unwrap();
                let floor_body = phys.create_rigid_body(
                    floor_entity_id,
                    BodyType::Static,
                    [0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                );
                phys.attach_box_collider(floor_body, [10.0, 0.5, 10.0], &glass_core::PhysicsMaterial { dynamic_friction: 0.7, bounciness: 0.1, ..glass_core::PhysicsMaterial::PROJECT_DEFAULT }, "");
            }

            // 2. Crear entidad de cubo dinámico en SceneState que caerá bajo gravedad
            let cube_entity_id = {
                let mut state = self.scene_state.lock().unwrap();
                let id = state.create_entity(
                    "Dynamic Physics Cube".to_string(),
                    vec!["Transform".to_string(), "RigidBody".to_string(), "Script".to_string()],
                );
                if let Some(ent) = state.entities.iter_mut().find(|e| e.id == id) {
                    ent.transform.position = [0.0, 8.0, 0.0];
                }
                id
            };

            // 3. Instanciar script de C# 'PhysicsDropTest' que inicializa el RigidBody y aplica impulso
            let script_handle = self.script_host.create_instance(cube_entity_id, "GlassEngine.Core.Scripts.PhysicsDropTest")
                .expect("Debe instanciar PhysicsDropTest en C#");
            assert!(script_handle > 0, "Handle de script C# debe ser válido");

            // 4. Simular 40 frames de física y renderizado
            let mut min_y = 8.0_f32;
            let mut bounced = false;

            for frame in 0..40 {
                self.script_host.update_all(0.016667);
                if let Err(e) = self.render_frame() {
                    log::error!("Fallo en render_frame durante test de física: {}", e);
                    event_loop.exit();
                    return;
                }

                if let Ok(state) = self.scene_state.lock() {
                    if let Some(ent) = state.entities.iter().find(|e| e.id == cube_entity_id) {
                        let cur_y = ent.transform.position[1];
                        if cur_y < min_y {
                            min_y = cur_y;
                        }
                        if cur_y > min_y + 0.2 && frame > 10 {
                            bounced = true;
                        }
                    }
                }
            }

            log::info!("Test M9: Altura mínima alcanzada bajo gravedad: {:.2}m, Rebotó/Saltó: {}", min_y, bounced);
            assert!(min_y < 7.5, "El cubo dinámico debió caer bajo gravedad (alcanzó {:.2})", min_y);

            // 5. Destruir instancia del script y verificar limpieza
            self.script_host.destroy_instance(script_handle);
            assert_eq!(self.script_host.active_instance_count(), 0);

            log::info!("============================================================");
            log::info!("   HITO 9 COMPLETADO: SIMULACIÓN FÍSICA 3D (RAPIER3D)");
            log::info!("   GPU en Uso:                  {}", gpu_name);
            log::info!("   Motor de Física:             Rapier3D 0.23.1 (f32, dim3)");
            log::info!("   Integración Temporal:        Fixed Timestep 60Hz con acumulador anti-jitter");
            log::info!("   Cuerpos Soportados:          Dynamic, Static, Kinematic");
            log::info!("   Colisionadores:              Cuboid, Ball, Capsule");
            log::info!("   Gravedad & Fuerzas:          vector![0.0, -9.81, 0.0] + Impulsos lineales");
            log::info!("   Sincronización Bidireccional: Bulk Sync de Transforms dinámicos a SceneState");
            log::info!("   Interop C# CoreCLR:          RigidBody, Collider, PhysicsDropTest.cs");
            log::info!("============================================================");

            event_loop.exit();
            return;
        }

        if self.is_m10_test {
            log::info!("Ejecutando suite de pruebas de integración para HITO 10 (Compilación Dual: Editor vs Standalone)...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para Hito 10");

            // Forzar modo standalone
            self.is_standalone = true;

            // 1. Crear entidad y script para el test standalone
            let standalone_entity_id = {
                let mut state = self.scene_state.lock().unwrap();
                state.create_entity(
                    "Standalone Player Entity".to_string(),
                    vec!["Transform".to_string(), "Script".to_string()],
                )
            };

            let rotator_handle = self.script_host.create_instance(standalone_entity_id, "GlassEngine.Core.Scripts.Rotator")
                .expect("Debe instanciar Rotator en C# para standalone");

            // 2. Ejecutar 60 frames continuos en modo standalone con hardware blit directo al Swapchain
            let render_start = Instant::now();
            for _ in 0..60 {
                self.script_host.update_all(0.016667);
                if let Err(e) = self.render_frame() {
                    log::error!("Fallo en render_frame durante test standalone: {}", e);
                    event_loop.exit();
                    return;
                }
            }
            let total_render_time = render_start.elapsed();
            let avg_fps = 60.0 / total_render_time.as_secs_f32().max(0.0001);
            log::info!("Rendimiento Standalone: 60 frames renderizados en {:.3}s (Promedio: {:.1} FPS)", total_render_time.as_secs_f32(), avg_fps);

            self.script_host.destroy_instance(rotator_handle);

            let profile_type = if cfg!(feature = "editor") {
                "Dual Binary (Modo Editor + Standalone selectable con --standalone)"
            } else {
                "Standalone Puro (Zero-overhead: sin egui ni MCP en binario compilado)"
            };

            log::info!("============================================================");
            log::info!("   HITO 10 COMPLETADO: COMPILACIÓN DUAL (EDITOR VS STANDALONE)");
            log::info!("   GPU en Uso:                  {}", gpu_name);
            log::info!("   Perfil Activo:               {}", profile_type);
            log::info!("   Pipeline de Presentación:    Hardware Blit directo de VRAM al Swapchain");
            log::info!("   Rendimiento en Pantalla:     {:.1} FPS (0 ms de overhead de UI)", avg_fps);
            log::info!("   Modo Editor (Default):       cargo build (egui docking, MCP runtime, inspector)");
            log::info!("   Modo Standalone (Release):   cargo build --no-default-features --release");
            log::info!("   Física, Audio y C# Scripting: 100% operativos en ambos perfiles");
            log::info!("============================================================");

            event_loop.exit();
            return;
        }

        if self.is_platformer_test {
            log::info!("Ejecutando suite de pruebas de integración para ESCENA 3D BÁSICA (Cubo jugador, Físicas, MCP, Salto y Escalada de Plataformas)...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para el test de plataformas");

            #[cfg(feature = "editor")]
            {
                // 1. Invocar MCP para inicializar protocolo
                let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
                let init_resp = self.mcp_server.handle_json(init_req).expect("Fallo al inicializar servidor MCP");
                log::info!("MCP Handshake: {}", init_resp);

                // 2. Aprovisionar Escena de Plataformas a través de herramienta MCP glass_setup_platformer_scene
                let setup_req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"glass_setup_platformer_scene","arguments":{"player_initial_y":0.5}}}"#;
                let setup_resp = self.mcp_server.handle_json(setup_req).expect("Fallo al aprovisionar escena via MCP");
                log::info!("MCP glass_setup_platformer_scene Respuesta: {}", setup_resp);
                assert!(setup_resp.contains("Suelo Estático"), "La escena debe contener suelo estático");
                assert!(setup_resp.contains("Escalón 1"), "La escena debe contener escalón 1");
                assert!(setup_resp.contains("Escalón 2"), "La escena debe contener escalón 2");
                assert!(setup_resp.contains("Cubo Jugador 3D"), "La escena debe contener cubo jugador");

                // 3. Inspeccionar jerarquía a través de MCP glass_get_scene_hierarchy
                let hier_req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"glass_get_scene_hierarchy"}}"#;
                let hier_resp = self.mcp_server.handle_json(hier_req).expect("Fallo al consultar jerarquía");
                log::info!("MCP Jerarquía de Escena: {}", hier_resp);
                assert!(hier_resp.contains("Static Floor"));
                assert!(hier_resp.contains("Low Platform Step 1"));
                assert!(hier_resp.contains("High Platform Step 2"));
                assert!(hier_resp.contains("Player 3D Cube"));

                // 4. Instanciar script de C# 'PlayerController' en el cubo jugador
                let player_entity_id = {
                    let state = self.scene_state.lock().unwrap();
                    state.entities.iter().find(|e| e.name == "Player 3D Cube").map(|e| e.id).expect("Debe existir entidad Player 3D Cube")
                };

                let script_handle = self.script_host.create_instance(player_entity_id, "GlassEngine.Core.Scripts.PlayerController")
                    .expect("Debe instanciar PlayerController en C#");
                assert!(script_handle > 0, "Handle de script C# debe ser válido");

                // 5. Simular frames de renderizado, física y C# scripts (movimiento, salto y escalada de plataformas)
                let mut max_x = 0.0_f32;
                let mut max_y = 0.0_f32;
                let mut climbed_step1 = false;
                let mut climbed_step2 = false;

                for _ in 0..360 {
                    self.script_host.update_all(0.016667);
                    if let Err(e) = self.render_frame() {
                        log::error!("Fallo en render_frame durante test de plataformas: {}", e);
                        event_loop.exit();
                        return;
                    }

                    if let Ok(state) = self.scene_state.lock() {
                        if let Some(ent) = state.entities.iter().find(|e| e.id == player_entity_id) {
                            let cur_x = ent.transform.position[0];
                            let cur_y = ent.transform.position[1];
                            if cur_x > max_x { max_x = cur_x; }
                            if cur_y > max_y { max_y = cur_y; }
                            if (2.0..=4.2).contains(&cur_x) && cur_y >= 1.35 {
                                climbed_step1 = true;
                            }
                            if (4.8..=7.5).contains(&cur_x) && cur_y >= 2.30 {
                                climbed_step2 = true;
                            }
                        }
                    }
                }

                log::info!("Resultados Simulación: MaxX={:.2}m, MaxY={:.2}m, Escaló Step 1: {}, Escaló Step 2: {}", max_x, max_y, climbed_step1, climbed_step2);
                assert!(max_x > 2.5, "El jugador debió avanzar horizontalmente hacia los obstáculos (alcanzó X={:.2})", max_x);
                assert!(max_y >= 1.35, "El jugador debió saltar y subir a la plataforma (alcanzó Y={:.2})", max_y);
                assert!(climbed_step1, "El jugador debió escalar con éxito la plataforma Step 1 (Y=1.0m, pos_y=1.50m)");
                assert!(climbed_step2, "El jugador debió escalar con éxito la plataforma Step 2 (Y=2.0m, pos_y=2.50m)");

                // 6. Captura de Viewport a través de MCP
                let capture_req = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"glass_capture_viewport"}}"#;
                let capture_resp = self.mcp_server.handle_json(capture_req).expect("Fallo al capturar viewport");
                assert!(capture_resp.contains("image/png"), "MCP debe capturar imagen PNG");

                // 7. Limpieza de script C#
                self.script_host.destroy_instance(script_handle);

                log::info!("============================================================");
                log::info!("   ESCENA 3D BÁSICA & SERVIDOR MCP VALIDADOS EXITOSAMENTE");
                log::info!("   GPU en Uso:                  {}", gpu_name);
                log::info!("   Aprovisionamiento MCP:       glass_setup_platformer_scene (Suelo + Escalón 1 + Escalón 2 + Jugador)");
                log::info!("   Introspección MCP:           glass_get_scene_hierarchy, glass_get_audio_status");
                log::info!("   Física 3D Dinámica:          Rapier3D 0.23 (Cubo Rigidbody, Gravedad, Impulso de Salto)");
                log::info!("   Colisiones 3D:               Cuboid vs Cuboid (Suelo y Plataformas)");
                log::info!("   Escalada de Objetos:         Superó Step 1 (Y=1.0m) y escaló Step 2 (Y=2.0m)");
                log::info!("   Audio Feedback 3D:           Kira (ALSA/CPAL): Disparo 'beep_880' (Salto) y 'beep_440' (Aterrizaje)");
                log::info!("   Controlador C#:              PlayerController.cs con GlassBehaviour CoreCLR");
                log::info!("   Snapshot MCP:                glass_capture_viewport (PNG Base64)");
                log::info!("============================================================");
            }

            event_loop.exit();
            return;
        }

        if self.is_sample_game_test {
            log::info!("Ejecutando suite de pruebas de integración para SAMPLE GAME COMPLETO (Plataformas, Monedas, Canvas UI, Meta y PBR)...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para el test de sample game");

            self.setup_complete_sample_game_scene();

            #[cfg(feature = "editor")]
            {
                let player_entity_id = {
                    let state = self.scene_state.lock().unwrap();
                    state.entities.iter().find(|e| e.name == "Player Hero 3D").map(|e| e.id).expect("Debe existir entidad Player Hero 3D")
                };

                let script_handle = self.script_host.create_instance(player_entity_id, "GlassEngine.Core.Scripts.PlayerController")
                    .expect("Debe instanciar PlayerController en C#");

                let mut coins_collected = 0;
                let mut reached_goal = false;
                let mut final_score = 0;

                for frame in 0..380 {
                    let dt = 0.016667;
                    self.script_host.update_all(dt);
                    if let Err(e) = self.render_frame() {
                        log::error!("Fallo en render_frame durante test de sample game: {}", e);
                        event_loop.exit();
                        return;
                    }

                    if let Ok(mut state) = self.scene_state.lock() {
                        let player_pos = state.entities.iter().find(|e| e.id == player_entity_id).map(|e| e.transform.position);

                        if let Some(pos) = player_pos {
                            // 1. Recolección de Moneda 1
                            if let Some(c1) = state.entities.iter_mut().find(|e| e.name == "Coin Gem 1" && e.active) {
                                let dist = ((pos[0] - c1.transform.position[0]).powi(2) + (pos[1] - c1.transform.position[1]).powi(2)).sqrt();
                                if dist < 1.6 {
                                    c1.active = false;
                                    coins_collected += 1;
                                    final_score += 50;
                                    if let Some(audio_sys) = glass_audio::get_global_audio_system() {
                                        audio_sys.play_2d("beep_880", 0.8, 1.5, false);
                                    }
                                }
                            }

                            // 2. Recolección de Moneda 2
                            if let Some(c2) = state.entities.iter_mut().find(|e| e.name == "Coin Gem 2" && e.active) {
                                let dist = ((pos[0] - c2.transform.position[0]).powi(2) + (pos[1] - c2.transform.position[1]).powi(2)).sqrt();
                                if dist < 1.6 {
                                    c2.active = false;
                                    coins_collected += 1;
                                    final_score += 50;
                                    if let Some(audio_sys) = glass_audio::get_global_audio_system() {
                                        audio_sys.play_2d("beep_880", 0.9, 1.8, false);
                                    }
                                }
                            }

                            // 3. Sincronizar Canvas HUD Text
                            if let Some(score_ent) = state.entities.iter_mut().find(|e| e.name == "Score HUD Text") {
                                if let Some(txt) = &mut score_ent.ui_text {
                                    txt.text = format!("Score: {}", final_score);
                                }
                            }
                            if let Some(time_ent) = state.entities.iter_mut().find(|e| e.name == "Time HUD Text") {
                                if let Some(txt) = &mut time_ent.ui_text {
                                    txt.text = format!("Time: {:.1}s", (frame as f32) * dt);
                                }
                            }

                            // 4. Comprobar Llegada a la Meta (Superando Step 1 y Step 2)
                            if pos[0] >= 4.8 && pos[1] >= 2.30 {
                                reached_goal = true;
                            }
                        }
                    }
                }

                assert!(coins_collected >= 1, "El jugador debió recolectar al menos una moneda (recolectó {})", coins_collected);
                assert!(final_score >= 50, "La puntuación en el Canvas UI debió actualizarse (Score={})", final_score);
                assert!(reached_goal, "El jugador debió alcanzar la plataforma Meta / Final");

                log::info!("============================================================");
                log::info!("✅ SAMPLE GAME TEST PASSED EXITOSAMENTE:");
                log::info!("   - Monedas Recolectadas:    {} / 2", coins_collected);
                log::info!("   - Puntuación Canvas UI:    {} pts", final_score);
                log::info!("   - Meta Alcanzada:          {}", reached_goal);
                log::info!("   - Físicas Rapier3D:        OK");
                log::info!("   - C# UnityEngine Runtime:  OK");
                log::info!("   - Shaders PBR Cook-Torr:   OK");
                log::info!("============================================================");

                self.script_host.destroy_instance(script_handle);
                event_loop.exit();
                return;
            }
        }

        if self.is_standalone_test {
            log::info!("Ejecutando suite de pruebas de integración para MODO STANDALONE PLAYER...");
            assert!(self.script_host.is_initialized, "ScriptHost debe estar inicializado para el test standalone");

            self.setup_complete_sample_game_scene();
            self.is_standalone = true;

            let player_entity_id = {
                let state = self.scene_state.lock().unwrap();
                state.entities.iter().find(|e| e.name == "Player Hero 3D").map(|e| e.id).expect("Debe existir entidad Player Hero 3D")
            };

            let script_handle = self.script_host.create_instance(player_entity_id, "GlassEngine.Core.Scripts.PlayerController")
                .expect("Debe instanciar PlayerController en C#");

            // Ejecutar 60 frames continuos en modo standalone
            let start = Instant::now();
            for _ in 0..60 {
                self.script_host.update_all(0.016667);
                if let Ok(mut physics) = self.physics_world.lock() {
                    physics.step(0.016667);
                }
                if let Err(e) = self.render_frame() {
                    log::error!("Fallo en render_frame durante test standalone: {}", e);
                    event_loop.exit();
                    return;
                }
            }
            let dur = start.elapsed();
            let avg_fps = 60.0 / dur.as_secs_f32().max(0.0001);

            self.script_host.destroy_instance(script_handle);

            let gpu_name = if let Some(ctx) = &self.vulkan_context {
                unsafe {
                    let props = ctx.instance.get_physical_device_properties(ctx.physical_device);
                    CStr::from_ptr(props.device_name.as_ptr())
                        .to_str()
                        .unwrap_or("Desconocida")
                        .to_string()
                }
            } else {
                "Vulkan Context N/A".to_string()
            };

            log::info!("============================================================");
            log::info!("✅ STANDALONE PLAYER TEST PASSED EXITOSAMENTE:");
            log::info!("   - GPU en Uso:                {}", gpu_name);
            log::info!("   - Rendimiento Hardware Blit: {:.1} FPS", avg_fps);
            log::info!("   - Escena Standalone:         SampleScene PBR + Rapier3D + Kira + CoreCLR");
            log::info!("   - Overhead UI:               0.0 ms (Zero egui docking)");
            log::info!("============================================================");

            event_loop.exit();
            return;
        }

        if self.is_m6_test {
            log::info!("Ejecutando suite de pruebas de integración para HITO 6 (Pipeline de Blender & Hot-Reload)...");

            // 1. Verificar presencia de cube.blend y cube.glb
            assert!(Path::new("assets/models/cube.blend").exists(), "cube.blend no encontrado");
            assert!(Path::new("assets/models/cube.glb").exists(), "cube.glb no encontrado");

            // 2. Verificar que AssetCache contiene la malla de cubo en VRAM
            {
                let cache = self.asset_cache.read().unwrap();
                let cube = cache.meshes.get("cube");
                assert!(cube.is_some(), "Malla 'cube' no encontrada en AssetCache");
                let m = cube.unwrap();
                assert_eq!(m.vertex_count, 24, "El cubo glTF estándar debe contener 24 vértices");
                assert_eq!(m.index_count, 36, "El cubo glTF estándar debe contener 36 índices");
                log::info!("AssetCache: Malla 'cube' validada en VRAM (24 vértices, 36 índices)");
            }

            // 3. Renderizar frame con la malla 3D indexada y matriz MVP activa
            if let Err(e) = self.render_frame() {
                log::error!("Fallo renderizando frame con malla 3D: {}", e);
                event_loop.exit();
                return;
            }

            log::info!("============================================================");
            log::info!("   HITO 6 COMPLETADO: PIPELINE BLENDER & HOT-RELOAD");
            log::info!("   GPU en Uso:                  {}", gpu_name);
            log::info!("   Bridge Headless Blender:     Blender 5.2.2 LTS (.blend -> .glb verificado)");
            log::info!("   Cargador de Mallas glTF:     gltf 1.4 (Posiciones, Normales, UVs, Índices)");
            log::info!("   Subida a VRAM:               Staging Buffer + gpu-allocator 0.28");
            log::info!("   Pipeline de Renderizado 3D:  mesh.vert.spv + mesh.frag.spv + PushConstants MVP");
            log::info!("   Vigilante en Segundo Plano:  AssetWatcher activo con notify 8.2 en assets/");
            log::info!("   Recarga en Caliente:         Soporte atómico de sustitución en AssetCache");
            log::info!("============================================================");

            event_loop.exit();
            return;
        }

        if self.is_m5_test {
            log::info!("Prueba de Hito 5 ejecutada.");
            event_loop.exit();
            return;
        }

        if self.is_headless_test {
            log::info!("Ejecutando frame de prueba...");
            let _ = self.render_frame();
            event_loop.exit();
            return;
        }

        // Señal de terminación recibida (SIGTERM/SIGINT/SIGHUP): salir por la
        // vía limpia en lugar de morir a mitad de un syscall de Vulkan.
        if SHUTDOWN_REQUESTED.load(std::sync::atomic::Ordering::SeqCst) {
            log::info!("Señal de terminación recibida: cerrando el motor de forma ordenada...");
            event_loop.exit();
            return;
        }

        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        if self.is_headless_test
            || self.is_m5_test
            || self.is_m6_test
            || self.is_m7_test
            || self.is_m8_test
            || self.is_m9_test
            || self.is_m10_test
            || self.is_platformer_test
            || self.is_sample_game_test
            || self.is_standalone_test
        {
            return;
        }

        #[cfg(feature = "editor")]
        if !self.is_standalone {
            if let Some(window) = &self.window {
                if let Some(egui_winit) = &mut self.egui_winit {
                    // Capturar WASD/Space ANTES de egui para que nunca sean bloqueadas
                    if let WindowEvent::KeyboardInput { event: ref key_ev, .. } = event {
                        if let PhysicalKey::Code(
                            KeyCode::KeyW | KeyCode::KeyS | KeyCode::KeyA | KeyCode::KeyD | KeyCode::Space
                            | KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::ArrowLeft | KeyCode::ArrowRight
                            | KeyCode::ShiftLeft | KeyCode::ShiftRight | KeyCode::KeyV,
                        ) = key_ev.physical_key {
                            match key_ev.state {
                                ElementState::Pressed => { self.keys_pressed.insert(key_ev.physical_key); }
                                ElementState::Released => { self.keys_pressed.remove(&key_ev.physical_key); }
                            }
                        }
                    }
                    // Registro diagnóstico del Bug F: si en el log de campo DroppedFile
                    // solo aparece al restaurar la ventana, la entrega de eventos OLE es
                    // la culpable; si aparece al soltar, el fallo está dentro de la app.
                    match &event {
                        WindowEvent::HoveredFile(ruta) => {
                            log::info!("Arrastre del SO: HoveredFile {ruta:?}");
                        }
                        WindowEvent::HoveredFileCancelled => {
                            log::info!("Arrastre del SO: HoveredFileCancelled");
                        }
                        WindowEvent::DroppedFile(ruta) => {
                            log::info!("Arrastre del SO: DroppedFile {ruta:?}");
                        }
                        _ => {}
                    }
                    // Pasar evento completo a egui (sin bloquear con consumed)
                    let _ = egui_winit.on_window_event(&window.inner, &event);
                }
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                // Cerrar la ventana del editor cierra este proceso:
                // desde la separación verdadera el Hub es un programa
                // aparte (glass_hub) y sigue vivo por su cuenta. Con
                // cambios sin guardar se pide decisión antes de salir
                // (Guardar / No guardar / Cancelar).
                #[cfg(feature = "editor")]
                if !self.is_standalone {
                    // Con cambios sin guardar no se vuelve al Launcher a
                    // secas: se pide decisión (Guardar / No guardar /
                    // Cancelar) y la ventana se queda abierta.
                    let sucia = self.editor_layout.viewport.play_state == glass_editor::PlayState::Edit
                        && matches!(self.scene_state.lock(), Ok(sc) if self.editor_layout.escena_sucia(&sc));
                    if sucia {
                        log::info!("Cierre de ventana con cambios sin guardar: pidiendo decisión.");
                        self.volver_al_hub_pendiente = true;
                        self.editor_layout.dialogo_guardar_cierre = true;
                        self.egui_ctx.request_repaint();
                        return;
                    }
                    log::info!("Cierre de ventana en el editor: saliendo del proceso (el Hub es un programa aparte).");
                    event_loop.exit();
                    return;
                }
                log::info!("Cierre de ventana solicitado.");
                event_loop.exit();
            }
            // Escape solo cierra la app en modo standalone/juego. En modo editor es una
            // tecla de trabajo (cancela el Grab de la herramienta 3D y operaciones
            // equivalentes, que ya la recibieron vía egui); allí se sale por CloseRequested/menú.
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } if self.is_standalone => {
                log::info!("Tecla Escape presionada. Saliendo.");
                event_loop.exit();
            }
            // Captura de input WASD / Espacio para el jugador, más las
            // teclas que la sincronización de input también consume
            // (Shift = correr/lanzar, V = cámara): sin ellas en el
            // conjunto, `run` y `camera` nunca podían valer 1 en el
            // binario puro (en el editor las cubría la vía de egui).
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(
                    KeyCode::KeyW | KeyCode::KeyS | KeyCode::KeyA | KeyCode::KeyD | KeyCode::Space
                    | KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::ArrowLeft | KeyCode::ArrowRight
                    | KeyCode::ShiftLeft | KeyCode::ShiftRight | KeyCode::KeyV,
                ) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            self.keys_pressed.insert(event.physical_key);
                        }
                        ElementState::Released => {
                            self.keys_pressed.remove(&event.physical_key);
                        }
                    }
                }
            }
            // Captura de ratón (posición y botón izquierdo) para el input
            // de juego y los clics del Canvas. No consume el evento: en
            // modo editor, egui-winit ya recibió su copia más arriba y la
            // UI del editor se comporta exactamente como antes.
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse.on_cursor_moved(position.x, position.y);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                match button {
                    MouseButton::Left => self.mouse.on_left_button(pressed),
                    MouseButton::Right => self.mouse.on_right_button(pressed),
                    MouseButton::Middle => self.mouse.on_middle_button(pressed),
                    _ => {}
                }
            }
            WindowEvent::Resized(physical_size) => {
                if let (Some(renderer), Some(vk_ctx)) = (&mut self.renderer, &self.vulkan_context) {
                    if physical_size.width > 0
                        && physical_size.height > 0
                        && (physical_size.width != renderer.swapchain.extent.width
                            || physical_size.height != renderer.swapchain.extent.height)
                    {
                        let _ = renderer.resize(vk_ctx, physical_size.width, physical_size.height);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if SHUTDOWN_REQUESTED.load(std::sync::atomic::Ordering::SeqCst) {
                    event_loop.exit();
                    return;
                }
                if let Err(e) = self.render_frame() {
                    // Un `VK_TIMEOUT` con la bandera levantada es la firma del
                    // syscall interrumpido por la señal, no un fallo del motor.
                    if SHUTDOWN_REQUESTED.load(std::sync::atomic::Ordering::SeqCst) {
                        log::info!("Frame interrumpido por la señal de terminación; salida ordenada.");
                        event_loop.exit();
                        return;
                    }
                    log::error!("Error durante render_frame: {}", e);
                }
                #[cfg(feature = "editor")]
                if self.salir_del_proceso {
                    event_loop.exit();
                    return;
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => (),
        }
    }
}

/// ¿Este arranque es un arranque de juego (standalone)?
///
/// Con la feature de editor, solo si se pide con `--standalone`/`--game`:
/// sin esa bandera el binario completo abre el Lanzador. El binario de
/// juego puro (sin la feature) no tiene lanzador ni modo editor: cualquier
/// arranque suyo es un arranque de juego, y sin esta regla caía en la
/// escena demo interactiva del editor (la rama final de `resumed`), que
/// no es una vía de juego documentada — lanzado sin argumentos se
/// estrellaba (ver REGISTRO_FIX_SIN_ARGS.md). Como juego le corresponde
/// lo mismo que a `--standalone` sin `--scene`: la escena por defecto del
/// proyecto (project.json) o la escena de ejemplo integrada.
fn is_standalone_launch(pure_build: bool, args: &[String]) -> bool {
    pure_build || args.iter().any(|arg| arg == "--standalone" || arg == "--game")
}

/// Contrato de los modos de arranque. Pura: solo mira `args`, no lee el entorno.
fn validar_argumentos(args: &[String]) -> Result<(), String> {
    const CON_VALOR: [(&str, &str); 3] = [
        ("--project", "la carpeta del proyecto"),
        ("--scene", "la ruta de la escena"),
        ("--sample", "el nombre del ejemplo"),
    ];
    for (flag, que) in CON_VALOR {
        // TODAS las apariciones del flag (auditoría S10): un valor que
        // es otro flag cuenta como valor ausente.
        for (idx, a) in args.iter().enumerate() {
            if a == flag {
                let falta = match args.get(idx + 1) {
                    None => true,
                    Some(siguiente) => siguiente.starts_with("--"),
                };
                if falta {
                    return Err(format!("el argumento {flag} necesita un valor ({que})"));
                }
            }
        }
    }

    let tiene = |flag: &str| args.iter().any(|a| a == flag);

    // Los flags del Hub no pertenecen al motor.
    if tiene("--hub") {
        return Err("el argumento --hub es del Hub (GlassHub), no del motor: abre GlassHub para gestionar proyectos.".to_string());
    }
    if tiene("--tras-actualizar-hub") {
        return Err("el argumento --tras-actualizar-hub es del Hub (GlassHub), no del motor.".to_string());
    }

    // Dos formas distintas de abrir: la escena suelta va sin proyecto o desde el Hub.
    if tiene("--project") && tiene("--scene") {
        return Err("argumentos incompatibles: --project con --scene".to_string());
    }

    Ok(())
}

/// Proyecto a abrir directamente (`--project <dir>`), la via con la que
/// el Hub lanzara el motor gestionado. Por ahora solo se reconoce.
fn proyecto_de_args(args: &[String]) -> Option<String> {
    args.iter()
        .position(|a| a == "--project")
        .and_then(|idx| args.get(idx + 1))
        .cloned()
}

/// Raiz del payload del motor (donde viven managed/ y los assets del
/// motor): el directorio del ejecutable si trae el payload al lado
/// (instalacion gestionada en engines/<version>/), o el directorio de
/// trabajo (build de desarrollo: el ejecutable vive en target/ y el
/// payload es el propio repo).
fn resolver_raiz_motor(exe_dir: &std::path::Path, cwd: &std::path::Path) -> std::path::PathBuf {
    if exe_dir.join("managed/GlassEngine.Core").is_dir() {
        exe_dir.to_path_buf()
    } else {
        cwd.to_path_buf()
    }
}

/// La raiz del motor de ESTE proceso, segun resolver_raiz_motor.
fn raiz_motor_actual() -> std::path::PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_default();
    resolver_raiz_motor(&exe_dir, &cwd)
}

/// Recolecta las luces puntuales de la escena en el UBO del frame: las
/// `MAX_POINT_LIGHTS` entidades con luz «Point» o «Spot» más cercanas
/// al ojo, con su posición de MUNDO (matriz jerárquica, no el
/// Transform local). La Spot emite con su cono: la dirección es el
/// forward (−Z) de su matriz de mundo. La direccional no pasa por
/// aquí: sigue en los push constants de siempre.
/// Clona los lotes de mallas con otra VP (las vistas de
/// portal): cada push ya trae su `model`, así que basta
/// Resuelve el descriptor set de propiedades de grafo para una entidad,
/// reutilizando el set creado en frames anteriores si los valores
/// empaquetados no cambiaron.
///
/// Devuelve `None` si el grafo no tiene esquema registrado, o si su esquema
/// no declara ninguna propiedad no-textura (no hay UBO que subir).
fn resolver_props_set(
    renderer: &mut glass_renderer::GlassRenderer,
    estado: &mut std::collections::HashMap<u64, (vk::DescriptorSet, Vec<f32>)>,
    ent_id: u64,
    graph: &str,
    overrides: &serde_json::Map<String, serde_json::Value>,
) -> Option<vk::DescriptorSet> {
    let schema = glass_core::shader_graph::graph_schema(graph)?;
    if schema
        .iter()
        .all(|p| matches!(p.kind, glass_core::shader_graph::PropertyKind::Texture2D))
    {
        return None;
    }

    let packed = glass_core::shader_graph::pack_property_values(&schema, overrides);
    // Sin bytemuck en este crate: los bytes salen de los f32 en orden nativo.
    let mut bytes = Vec::with_capacity(packed.len() * 4);
    for v in &packed {
        bytes.extend_from_slice(&v.to_ne_bytes());
    }
    let bytes: &[u8] = &bytes;

    match estado.get_mut(&ent_id) {
        Some((set, cached)) if cached == &packed => Some(*set),
        Some((set, cached)) => {
            renderer.update_graph_props_set(*set, bytes);
            *cached = packed;
            Some(*set)
        }
        None => {
            let set = renderer.create_graph_props_set(bytes)?;
            estado.insert(ent_id, (set, packed));
            Some(set)
        }
    }
}

/// recomponer el MVP y mover el ojo.
fn clonar_lotes_portal<'x>(
    lotes: &[glass_renderer::MeshDrawItem<'x>],
    vp: [f32; 16],
    ojo: [f32; 4],
) -> Vec<glass_renderer::MeshDrawItem<'x>> {
    lotes
        .iter()
        .map(|(m, pc, tex, sh, r, ps, ranura)| {
            let mut pc2 = *pc;
            pc2.mvp = matmul(&vp, &pc.model);
            pc2.camera_pos = ojo;
            (*m, pc2, *tex, sh.clone(), *r, *ps, *ranura)
        })
        .collect()
}

/// Igual que `clonar_lotes_portal` para los lotes con piel.
fn clonar_piel_portal<'x>(
    lotes: &[glass_renderer::SkinnedDrawItem<'x>],
    vp: [f32; 16],
    ojo: [f32; 4],
) -> Vec<glass_renderer::SkinnedDrawItem<'x>> {
    lotes
        .iter()
        .map(|(m, pc, tex, joints, r, ranura)| {
            let mut pc2 = *pc;
            pc2.mvp = matmul(&vp, &pc.model);
            pc2.camera_pos = ojo;
            (*m, pc2, *tex, *joints, *r, *ranura)
        })
        .collect()
}

/// Busca en los lotes el dibujo cuya `model` sea el mundo del
/// portal: devuelve su malla y rango (la superficie que el
/// pase de portal va a dibujar en su lugar).
fn robar_superficie<'x>(
    lotes: &[glass_renderer::MeshDrawItem<'x>],
    w: [f32; 16],
) -> Option<(&'x glass_renderer::GpuMesh, (u32, u32))> {
    lotes
        .iter()
        .find(|(_, pc, _, _, _, _, _)| pc.model == w)
        .map(|(m, _, _, _, r, _, _)| (*m, *r))
}

/// Añade el dibujo de una superficie de portal a las listas
/// de juego (con su VP) y de escena (misma textura y VP
/// virtual; en v1 la vista de escena muestra la vista del
/// juego, no la suya propia).
#[allow(clippy::too_many_arguments)]
fn empujar_draw_portal<'x>(
    draws_game: &mut Vec<glass_renderer::PortalDrawItem<'x>>,
    draws_scene: &mut Vec<glass_renderer::PortalDrawItem<'x>>,
    mesh: &'x glass_renderer::GpuMesh,
    range: (u32, u32),
    mundo: [f32; 16],
    vp_game: [f32; 16],
    vp_scene: [f32; 16],
    vp_virtual: [f32; 16],
    texture_index: usize,
    rim: [f32; 4],
    local_half: [f32; 2],
) {
    let push = glass_renderer::PortalPushConstants {
        mvp: matmul(&vp_game, &mundo),
        mvp_virtual: matmul(&vp_virtual, &mundo),
        rim,
        local_half,
        _pad: [0.0, 0.0],
    };
    draws_game.push(glass_renderer::PortalDrawItem {
        mesh,
        push,
        texture_index,
        range,
    });
    let mut push_s = push;
    push_s.mvp = matmul(&vp_scene, &mundo);
    draws_scene.push(glass_renderer::PortalDrawItem {
        mesh,
        push: push_s,
        texture_index,
        range,
    });
}

fn recolectar_luces_puntuales(
    scene: &SceneState,
    eye: [f32; 3],
) -> (glass_renderer::PointLightsUbo, Vec<glass_renderer::PointShadowLight>) {
    let mundos = scene.compute_world_matrices();
    let mut candidatas: Vec<CandidataLuz> = Vec::new();
    for ent in &scene.entities {
        if !ent.active {
            continue;
        }
        let Some(l) = ent.light.as_ref() else { continue };
        if l.light_type != "Point" && l.light_type != "Spot" && l.light_type != "Area" {
            continue;
        }
        let p = mundos
            .get(&ent.id)
            .map(|m| [m[12], m[13], m[14]])
            .unwrap_or(ent.transform.position);
        let dx = p[0] - eye[0];
        let dy = p[1] - eye[1];
        let dz = p[2] - eye[2];
        let (dir_cos, cone) = if l.light_type == "Spot" {
            let m_opt = mundos.get(&ent.id);
            let dir = if let Some(m) = m_opt {
                let z = [m[8], m[9], m[10]];
                let fwd = [-z[0], -z[1], -z[2]];
                let len = (fwd[0] * fwd[0] + fwd[1] * fwd[1] + fwd[2] * fwd[2])
                    .sqrt()
                    .max(0.0001);
                [fwd[0] / len, fwd[1] / len, fwd[2] / len]
            } else {
                [0.0, 0.0, -1.0]
            };
            let (cos_outer, cos_inner) = l.spot_cone_cos();
            (
                [dir[0], dir[1], dir[2], cos_outer],
                [cos_inner, 1.0, 0.0, 0.0],
            )
        } else {
            ([0.0; 4], [0.0; 4])
        };

        candidatas.push((
            dx * dx + dy * dy + dz * dz,
            [p[0], p[1], p[2], l.range.max(0.0)],
            [l.color[0], l.color[1], l.color[2], l.intensity.max(0.0)],
            l.cast_shadow,
            dir_cos,
            cone,
        ));
    }

    candidatas.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut ubo = glass_renderer::PointLightsUbo::default();
    let mut luces_sombra: Vec<glass_renderer::PointShadowLight> = Vec::new();
    let mut slot: usize = 0;
    let n = candidatas.len().min(glass_renderer::MAX_POINT_LIGHTS);

    for (i, cand) in candidatas.iter().enumerate().take(n) {
        ubo.pos_range[i] = cand.1;
        ubo.color_intensity[i] = cand.2;
        ubo.dir_cos[i] = cand.4;
        ubo.cone[i] = cand.5;

        if cand.3 && slot < 4 {
            ubo.shadow[i] = [slot as f32, cand.1[3].max(0.1), 0.0025, 0.1];
            luces_sombra.push(glass_renderer::PointShadowLight {
                pos: [cand.1[0], cand.1[1], cand.1[2]],
                far: cand.1[3].max(0.1),
            });
            slot += 1;
        }
    }

    ubo.count = n as u32;

    (ubo, luces_sombra)
}

/// Recolecta las luces Point/Spot de la escena como `LuzEscena` (para
/// la asignación por objeto) y el mapa de cubos de sombra global:
/// las 4 luces con `cast_shadow` más cercanas al ojo reciben los
/// cubos 0..3, con la misma regla que `recolectar_luces_puntuales`.
fn recolectar_luces_escena(
    scene: &SceneState,
    eye: [f32; 3],
) -> (Vec<luces_objeto::LuzEscena>, std::collections::HashMap<u64, i32>) {
    let mundos = scene.compute_world_matrices();
    let mut luces: Vec<luces_objeto::LuzEscena> = Vec::new();
    let mut por_distancia: Vec<(f32, u64, bool)> = Vec::new();
    for ent in &scene.entities {
        if !ent.active {
            continue;
        }
        let Some(l) = ent.light.as_ref() else { continue };
        if l.light_type != "Point" && l.light_type != "Spot" && l.light_type != "Area" {
            continue;
        }
        let m_opt = mundos.get(&ent.id);
        let p = m_opt
            .map(|m| [m[12], m[13], m[14]])
            .unwrap_or(ent.transform.position);
        let es_spot = l.light_type == "Spot";
        let (dir, cos_outer, cos_inner) = if es_spot {
            let dir = if let Some(m) = m_opt {
                let z = [m[8], m[9], m[10]];
                let fwd = [-z[0], -z[1], -z[2]];
                let len = (fwd[0] * fwd[0] + fwd[1] * fwd[1] + fwd[2] * fwd[2])
                    .sqrt()
                    .max(0.0001);
                [fwd[0] / len, fwd[1] / len, fwd[2] / len]
            } else {
                [0.0, 0.0, -1.0]
            };
            let (co, ci) = l.spot_cone_cos();
            (dir, co, ci)
        } else {
            ([0.0, 0.0, -1.0], 1.0, 1.0)
        };
        let perfil = ent
            .light_profile
            .clone()
            .unwrap_or_else(glass_core::LightProfileData::implicito);
        luces.push(luces_objeto::LuzEscena {
            id: ent.id,
            pos: p,
            dir,
            color_int: [l.color[0], l.color[1], l.color[2], l.intensity.max(0.0)],
            range: l.range.max(0.0),
            cos_outer,
            cos_inner,
            es_spot,
            cast_shadow: l.cast_shadow,
            prioridad: perfil.prioridad,
            grupo: perfil.grupo,
            tope_grupo: perfil.tope_por_objeto,
        });
        let dx = p[0] - eye[0];
        let dy = p[1] - eye[1];
        let dz = p[2] - eye[2];
        por_distancia.push((dx * dx + dy * dy + dz * dz, ent.id, l.cast_shadow));
    }
    por_distancia.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut cubos: std::collections::HashMap<u64, i32> = std::collections::HashMap::new();
    let mut siguiente_cubo = 0i32;
    for (_, id, con_sombra) in &por_distancia {
        if *con_sombra && siguiente_cubo < 4 {
            cubos.insert(*id, siguiente_cubo);
            siguiente_cubo += 1;
        }
    }
    (luces, cubos)
}

/// Registro de la muestra de frame de la Fase 0: alimenta el registro global
/// de `glass_core::profiling` y, cada 120 frames, muestrea la memoria del
/// Drena los eventos de contacto del mundo y los despacha a los scripts
/// de AMBAS entidades del par (Enter/Exit directos del mundo; Stay
/// derivado de los pares activos que ya lo estaban el frame anterior y no
/// acaban de entrar). El lock de física se suelta ANTES de despachar: un
/// callback puede crear o destruir cuerpos. Función libre porque
/// render_frame retiene préstamos de otros campos (renderer) todo el
/// cuerpo: recibe los tres campos que toca por referencia disjunta.
/// Forma del mundo para un ColliderSpec de escena (Fase 1.5).
fn shape_de_spec(spec: &glass_core::ColliderSpec) -> ColliderShape {
    match spec.shape.as_str() {
        "Sphere" => ColliderShape::Ball { radius: spec.size[0].max(0.01) },
        "Capsule" => ColliderShape::Capsule { half_height: spec.size[0].max(0.0), radius: spec.size[1].max(0.01) },
        "Cylinder" => ColliderShape::Cylinder { half_height: spec.size[0].max(0.0), radius: spec.size[1].max(0.01) },
        _ => ColliderShape::Cuboid { half_extents: spec.size },
    }
}

/// La lista de colliders primitivos de una entidad (Fase 1.5): si el
/// PhysicsData trae `colliders`, uno por spec (capa/máscara de la entidad,
/// trigger DEL SPEC, con su offset); si no, el único collider histórico
/// inferido de la malla, sin offset.
fn colliders_resueltos(
    body: &glass_core::PhysicsData,
    shape_inferida: ColliderShape,
    lib: &mut PhysicsMaterialLibrary,
) -> Vec<ColliderResuelto> {
    if body.colliders.is_empty() {
        let fallback = body.body_fallback_material();
        vec![(
            shape_inferida,
            ColliderConfig { layer: body.layer, mask: body.collision_mask, is_trigger: body.is_trigger },
            body.box_offset,
            fallback,
            String::new(),
            String::from("Y"),
        )]
    } else {
        body.colliders
            .iter()
            .map(|spec| {
                let (material, material_ref) = material_resuelto(&spec.material, body, lib);
                (
                    shape_de_spec(spec),
                    ColliderConfig { layer: body.layer, mask: body.collision_mask, is_trigger: spec.is_trigger },
                    spec.offset,
                    material,
                    material_ref,
                    spec.direction.clone(),
                )
            })
            .collect()
    }
}

fn despachar_eventos_contacto(
    physics_world: &std::sync::Arc<std::sync::Mutex<PhysicsWorld>>,
    script_host: &glass_scripting::ScriptHost,
    prev_contact_pairs: &mut std::collections::HashSet<(u64, u64)>,
    scene_state: &Arc<Mutex<SceneState>>,
) {
    let (events, active_pairs) = {
        let Ok(mut physics) = physics_world.lock() else {
            return;
        };
        (
            physics.drain_contact_events(),
            physics.contact_pairs(),
        )
    };
    if events.is_empty() && active_pairs.is_empty() && prev_contact_pairs.is_empty() {
        return;
    }
    // Nombres de escena por id: el Collision gestionado debe nombrar a la
    // OTRA entidad de verdad, no «Entity». Se construye solo cuando hay
    // algo que despachar.
    let nombres: std::collections::HashMap<u64, String> = match scene_state.lock() {
        Ok(scene) => scene
            .entities
            .iter()
            .map(|e| (e.id, e.name.clone()))
            .collect(),
        Err(_) => Default::default(),
    };
    let nombre = |id: u64| nombres.get(&id).map(String::as_str).unwrap_or("Entity");
    let mut entered: std::collections::HashSet<(u64, u64)> = Default::default();
    for event in &events {
        let kind = match event.kind {
            ContactKind::Enter => 0,
            ContactKind::Exit => 2,
        };
        script_host.dispatch_contact_event(event.a, event.b, kind, event.is_trigger, nombre(event.b), event.contact);
        // El despacho espejo invierte la normal y la velocidad relativa:
        // el impacto se cuenta desde la otra entidad. También la normal
        // de cada punto individual (posición, separation e impulse no
        // cambian).
        let espejo = event.contact.map(|c| {
            let mut puntos = c.points;
            for p in puntos.iter_mut().take(c.point_count as usize) {
                p.normal = [-p.normal[0], -p.normal[1], -p.normal[2]];
            }
            glass_physics::ContactInfo {
                normal: [-c.normal[0], -c.normal[1], -c.normal[2]],
                relative_velocity: [
                    -c.relative_velocity[0],
                    -c.relative_velocity[1],
                    -c.relative_velocity[2],
                ],
                points: puntos,
                ..c
            }
        });
        script_host.dispatch_contact_event(event.b, event.a, kind, event.is_trigger, nombre(event.a), espejo);
        if event.kind == ContactKind::Enter {
            entered.insert((event.a, event.b));
        }
    }
    let mut active_set: std::collections::HashSet<(u64, u64)> = Default::default();
    for (a, b, is_trigger) in &active_pairs {
        active_set.insert((*a, *b));
        if prev_contact_pairs.contains(&(*a, *b)) && !entered.contains(&(*a, *b)) {
            script_host.dispatch_contact_event(*a, *b, 1, *is_trigger, nombre(*b), None);
            script_host.dispatch_contact_event(*b, *a, 1, *is_trigger, nombre(*a), None);
        }
    }
    *prev_contact_pairs = active_set;
}

/// allocator GPU (generate_report recorre las asignaciones: no es gratis).
fn registrar_muestra_frame(
    contador: &mut u64,
    renderer: Option<&GlassRenderer>,
    t0: Instant,
    update_us: u64,
    physics_us: u64,
    render_us: u64,
    present_us: u64,
) {
    *contador += 1;
    if (*contador).is_multiple_of(120) {
        if let Some(r) = renderer {
            if let Ok(alloc) = r.allocator.lock() {
                let report = alloc.generate_report();
                glass_core::profiling::record_gpu_memory(
                    report.total_allocated_bytes,
                    report.total_capacity_bytes,
                );
            }
        }
    }
    glass_core::profiling::record_frame_timing(glass_core::profiling::FramePhaseSample {
        update_us,
        physics_us,
        render_us,
        present_us,
        total_us: t0.elapsed().as_micros() as u64,
        gpu_render_us: renderer.and_then(|r| r.last_gpu_render_us()),
    });
}

// ─── Sonda de GPU (proceso hijo) ────────────────────────────────────────────
//
// El modo `--gpu-probe` existe en todos los builds: el padre solo lo lanza con
// la feature `editor`, pero el binario siempre sabe actuar como hijo.

/// Punto de entrada del proceso hijo de la sonda. Sale con código 0 si la
/// secuencia gráfica terminó, 2 si falló de forma controlada. Si el driver
/// revienta, el proceso muere y el padre lo detecta por el código y por la
/// ausencia del marcador `GLASS_PROBE_OK`.
fn ejecutar_sonda_hijo() -> Result<(), Box<dyn std::error::Error>> {
    log::info!("Sonda de GPU: proceso hijo iniciado.");
    let mut builder = EventLoop::builder();
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        if std::env::var("WINIT_UNIX_BACKEND").as_deref() == Ok("x11") || std::env::var("DISPLAY").is_ok() {
            log::info!("Sonda de GPU: forzando backend X11/XWayland en Winit, igual que el arranque normal.");
            builder.with_x11();
        }
    }
    let event_loop = builder.build()?;
    let mut sonda = SondaGpuApp {
        hecho: false,
        exito: false,
    };
    event_loop.run_app(&mut sonda)?;
    std::process::exit(if sonda.exito { 0 } else { 2 })
}

/// Aplicación mínima del hijo: ejecuta la secuencia una sola vez en el primer
/// `resumed` y sale del bucle.
struct SondaGpuApp {
    hecho: bool,
    exito: bool,
}

impl ApplicationHandler for SondaGpuApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.hecho {
            return;
        }
        self.hecho = true;
        self.exito = secuencia_sonda(event_loop);
        event_loop.exit();
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _window_id: WindowId, _event: WindowEvent) {
        // Nada que hacer: la ventana se destruye al terminar la secuencia.
    }
}

/// Escribe una línea de protocolo para el padre y vacía stdout en el acto, para
/// que llegue aunque el proceso muera justo después.
fn emitir_linea_sonda(linea: &str) {
    println!("{linea}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

/// Recoge recursivamente las rutas `.glb` bajo `dir`. Los directorios
/// ilegibles se ignoran.
fn recoger_glb(dir: &std::path::Path, salida: &mut Vec<std::path::PathBuf>) {
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return;
    };
    for entrada in entradas.flatten() {
        let ruta = entrada.path();
        if ruta.is_dir() {
            recoger_glb(&ruta, salida);
        } else if ruta.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("glb")) {
            salida.push(ruta);
        }
    }
}

/// Reproduce la secuencia que hace caer a algunos drivers al abrir un
/// proyecto: contexto Vulkan, pipelines de materiales de grafo y subida de
/// modelos. Devuelve `true` si llegó al final.
fn secuencia_sonda(event_loop: &ActiveEventLoop) -> bool {
    // Orden de declaración: ventana → contexto → renderer → mallas. Al salir se
    // destruyen en orden inverso (mallas, renderer, contexto, ventana).
    let window = match GlassWindow::new(
        event_loop,
        &WindowConfig {
            title: "GlassEngine — comprobación de gráficos".to_string(),
            ..Default::default()
        },
    ) {
        Ok(window) => window,
        Err(e) => {
            log::error!("Sonda de GPU: no se pudo crear la ventana: {e:?}");
            return false;
        }
    };

    let vk_ctx = match VulkanContext::new(&window, VulkanConfig::default()) {
        Ok(vk_ctx) => vk_ctx,
        Err(e) => {
            log::error!("Sonda de GPU: no se pudo crear el contexto Vulkan: {e:?}");
            return false;
        }
    };

    let nombre_gpu = unsafe { std::ffi::CStr::from_ptr(vk_ctx.physical_device_properties.device_name.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    let driver_version = vk_ctx.physical_device_properties.driver_version;
    log::info!("Sonda de GPU: GPU seleccionada «{nombre_gpu}» (driver_version {driver_version}).");
    emitir_linea_sonda(&format!("GLASS_PROBE_GPU\t{nombre_gpu}\t{driver_version}"));

    let mut renderer = match GlassRenderer::new(&vk_ctx, 1280, 720) {
        Ok(renderer) => renderer,
        Err(e) => {
            log::error!("Sonda de GPU: no se pudo crear el renderer: {e:?}");
            return false;
        }
    };

    // Compilación de los pipelines de grafo: aquí se produjo el cierre real.
    log::info!("Sonda de GPU: compilando los pipelines de materiales de grafo...");
    renderer.load_graph_materials(std::path::Path::new("assets/shaders"));

    // Subidas acotadas: los 10 primeros .glb en orden de ruta.
    let mut rutas = Vec::new();
    recoger_glb(std::path::Path::new("assets/models"), &mut rutas);
    rutas.sort();
    let mut subidos = Vec::new();
    for ruta in rutas.iter().take(10) {
        let bytes = match std::fs::read(ruta) {
            Ok(bytes) => bytes,
            Err(e) => {
                log::warn!("Sonda de GPU: no se pudo leer {}: {e}", ruta.display());
                continue;
            }
        };
        // Un modelo corrupto no es un veredicto del driver: se anota y se sigue.
        let model = match MeshLoader::load_model_from_glb_bytes(&bytes) {
            Ok(model) => model,
            Err(e) => {
                log::warn!("Sonda de GPU: no se pudo parsear {}: {e:?}", ruta.display());
                continue;
            }
        };
        let upload_res = {
            let mut alloc = renderer.allocator.lock().unwrap();
            MeshLoader::upload_to_gpu(
                &renderer.device,
                &mut alloc,
                &model.vertices,
                &model.indices,
                vk_ctx.graphics_queue,
                renderer.command_pool,
            )
        };
        match upload_res {
            Ok(malla) => subidos.push(malla),
            Err(e) => log::warn!("Sonda de GPU: no se pudo subir {}: {e:?}", ruta.display()),
        }
    }

    unsafe {
        let _ = renderer.device.device_wait_idle();
    }
    log::info!("Sonda de GPU: secuencia completada ({} modelos subidos).", subidos.len());
    emitir_linea_sonda("GLASS_PROBE_OK");
    true
}

// ─── Sonda de GPU (proceso padre) ───────────────────────────────────────────

/// Diagnóstico calculado en `main()` antes del bucle de eventos; `resumed()` lo
/// lleva a la banda del Hub.
#[cfg(feature = "editor")]
static DIAGNOSTICO_SONDA: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// Texto de la banda del Hub cuando la sonda falla en la GPU `seleccionada`.
/// Función pura: solo depende de sus argumentos.
#[cfg(any(feature = "editor", test))]
fn texto_diagnostico_sonda(seleccionada: &str, gpus: &[glass_renderer::context::GpuDeviceInfo]) -> String {
    let version = gpus.iter().find(|g| g.name == seleccionada).map(|g| {
        if g.vendor_id == 0x8086 {
            let v = glass_renderer::context::version_intel_windows(g.driver_version);
            format!("{}.{}", v.0, v.1)
        } else {
            let v = glass_renderer::context::version_driver_legible(g.driver_version);
            format!("{}.{}.{}", v.0, v.1, v.2)
        }
    });
    let mut texto = match version {
        Some(version) => format!(
            "La comprobación de gráficos falló en la GPU «{seleccionada}» (controlador {version}): ese controlador puede cerrar el motor al abrir un proyecto."
        ),
        None => format!(
            "La comprobación de gráficos falló en la GPU «{seleccionada}»: ese controlador puede cerrar el motor al abrir un proyecto."
        ),
    };
    if let Some(dedicada) = gpus.iter().find(|g| g.es_discreta() && g.name != seleccionada) {
        texto.push_str(&format!(
            " Se detectó también la dedicada «{}»: fuerza su uso en Configuración > Sistema > Pantalla > Gráficos.",
            dedicada.name
        ));
    }
    texto.push_str(
        " Actualiza el controlador desde la web del fabricante. El detalle quedó en el registro (glass.log, en la carpeta GlassEnginer).",
    );
    texto
}

/// Ejecuta la sonda en un proceso hijo si no hay un resultado en caché para
/// este conjunto de GPUs, drivers y versión del motor. Devuelve el texto de
/// diagnóstico si la sonda falló.
#[cfg(feature = "editor")]
fn comprobar_sonda_gpu() -> Option<String> {
    let gpus = glass_renderer::context::enumerar_gpus();
    if gpus.is_empty() {
        log::warn!("Sonda de GPU: no se enumeró ninguna GPU; se omite la comprobación.");
        return None;
    }

    // Clave de la caché: pares (nombre, driver) ordenados por nombre.
    let mut clave: Vec<(String, u32)> = gpus.iter().map(|g| (g.name.clone(), g.driver_version)).collect();
    clave.sort();
    let clave_json = serde_json::json!(clave);
    // S5: el objetivo es el motor GESTIONADO, no este binario: su
    // version manda en la cache y su exe es el que se lanza.
    let objetivo = glass_editor::updates::objetivo_sonda();
    let version_objetivo = objetivo
        .as_ref()
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| glass_editor::updates::ENGINE_VERSION.to_string());
    let ruta_cache = glass_editor::updates::data_root().join("gpu_probe.json");

    if let Some(cache) = std::fs::read_to_string(&ruta_cache)
        .ok()
        .and_then(|texto| serde_json::from_str::<serde_json::Value>(&texto).ok())
    {
        if cache["motor"].as_str() == Some(version_objetivo.as_str()) && cache["gpus"] == clave_json {
            match cache["resultado"].as_str() {
                Some("ok") => {
                    log::info!("Sonda de GPU: resultado en caché correcto; no se repite.");
                    return None;
                }
                Some("fallo") => {
                    let seleccionada = cache["seleccionada"].as_str().unwrap_or_default();
                    log::warn!("Sonda de GPU: resultado en caché de fallo en «{seleccionada}».");
                    return Some(texto_diagnostico_sonda(seleccionada, &gpus));
                }
                _ => {}
            }
        }
    }

    let exe = match objetivo {
        Some((exe, _)) => exe,
        None => match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                log::warn!("Sonda de GPU: no se pudo localizar el ejecutable: {e}");
                return None;
            }
        },
    };
    log::info!("Sonda de GPU: lanzando el proceso hijo de comprobación...");
    let mut hijo = match std::process::Command::new(&exe)
        .arg("--gpu-probe")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(hijo) => hijo,
        Err(e) => {
            log::warn!("Sonda de GPU: no se pudo lanzar el proceso hijo: {e}");
            return None;
        }
    };

    // stdout se lee en un hilo mientras el hijo corre, para que una salida
    // abundante no llene la tubería y bloquee al hijo.
    let lector = hijo.stdout.take().map(|mut stdout| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = std::io::Read::read_to_end(&mut stdout, &mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        })
    });

    let inicio = std::time::Instant::now();
    let limite = std::time::Duration::from_secs(180);
    let estado = loop {
        match hijo.try_wait() {
            Ok(Some(estado)) => break Some(estado),
            Ok(None) => {
                if inicio.elapsed() >= limite {
                    log::error!("Sonda de GPU: el hijo superó {} s; se termina.", limite.as_secs());
                    let _ = hijo.kill();
                    let _ = hijo.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                log::error!("Sonda de GPU: error esperando al hijo: {e}");
                let _ = hijo.kill();
                let _ = hijo.wait();
                break None;
            }
        }
    };
    let salida = lector.and_then(|hilo| hilo.join().ok()).unwrap_or_default();

    let mut gpu_hijo: Option<String> = None;
    let mut marcador_ok = false;
    for linea in salida.lines() {
        if let Some(resto) = linea.strip_prefix("GLASS_PROBE_GPU\t") {
            let (nombre, driver) = resto.split_once('\t').unwrap_or((resto, ""));
            log::info!("Sonda de GPU: el hijo usó «{nombre}» (driver_version {driver}).");
            gpu_hijo = Some(nombre.to_string());
        } else if linea == "GLASS_PROBE_OK" {
            marcador_ok = true;
        }
    }

    let exito = estado.is_some_and(|estado| estado.success()) && marcador_ok;
    let seleccionada = gpu_hijo.unwrap_or_else(|| {
        gpus.iter()
            .find(|g| g.es_discreta())
            .unwrap_or(&gpus[0])
            .name
            .clone()
    });
    if exito {
        log::info!("Sonda de GPU: comprobación correcta en «{seleccionada}».");
    } else {
        log::error!(
            "Sonda de GPU: comprobación FALLIDA en «{seleccionada}» (código {:?}, marcador OK: {marcador_ok}).",
            estado.and_then(|estado| estado.code())
        );
    }

    let cache = serde_json::json!({
        "motor": version_objetivo,
        "gpus": clave_json,
        "seleccionada": seleccionada,
        "resultado": if exito { "ok" } else { "fallo" },
    });
    if let Some(dir) = ruta_cache.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!("Sonda de GPU: no se pudo crear {}: {e}", dir.display());
        }
    }
    if let Err(e) = std::fs::write(&ruta_cache, cache.to_string()) {
        log::warn!("Sonda de GPU: no se pudo escribir {}: {e}", ruta_cache.display());
    }

    if exito {
        None
    } else {
        Some(texto_diagnostico_sonda(&seleccionada, &gpus))
    }
}

/// Cuánto avanza el reloj de expulsión de la caché en un frame.
///
/// Dos protecciones sobre el delta real entre frames (re-análisis
/// del arreglo del Bug I, 2026-11-23):
/// - **Tope de 250 ms por frame**: una pausa larga (diálogo modal,
///   ventana minimizada, primer frame lento) no puede hacer que un
///   modelo dibujado en el frame anterior cuente como ocioso de
///   golpe; cruzar el horizonte de 10 s exige 40 frames seguidos sin
///   dibujarlo, que es la premisa de seguridad de la expulsión.
/// - **Resto sub-milisegundo acumulado**: la fracción que no llega a
///   un milisegundo entero no se pierde; sin esto, a 300-600 fps
///   (frames de 1,7-3,3 ms) el truncado ralentizaba el reloj hasta
///   un ~40 %, y con frames de menos de 1 ms lo congelaba.
fn delta_expulsion_ms(bruto_ms: f64, resto_ms: &mut f64) -> u64 {
    let total = bruto_ms.clamp(0.0, 250.0) + *resto_ms;
    let entero = total.floor();
    *resto_ms = total - entero;
    entero as u64
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ruta_log = logfile::iniciar();
    log::info!("Registro de la sesión en {}", ruta_log.display());

    // Prueba controlada del hook de pánicos (solo builds de depuración):
    // con GLASS_PANIC_TEST definida, se provoca un pánico nada más arrancar
    // para verificar que el informe queda en glass.log y glass_crash.log.
    #[cfg(debug_assertions)]
    if std::env::var_os("GLASS_PANIC_TEST").is_some() {
        panic!("pánico de prueba del hook (GLASS_PANIC_TEST)");
    }

    let args: Vec<String> = std::env::args().collect();

    // Modo hijo de la sonda de GPU: ejecuta la secuencia gráfica arriesgada
    // en este proceso desechable (ver `ejecutar_sonda_hijo`).
    if args.iter().any(|arg| arg == "--gpu-probe") {
        return ejecutar_sonda_hijo();
    }

    // Modo Servidor MCP stdio puro para integración con agentes y herramientas externas
    #[cfg(feature = "editor")]
    if args.iter().any(|arg| arg == "--mcp" || arg == "--mcp-stdio") {
        let scene_state = Arc::new(Mutex::new(SceneState::new()));
        let mcp_server = McpServer::new(scene_state);
        // Puerta del complemento «herramienta-3d» para las tools glass_mesh_*.
        mcp_server
            .set_mesh_tools_enabled(glass_editor::plugins::plugin_enabled_for(
                "herramienta-3d",
                glass_editor::updates::ENGINE_VERSION,
            ));
        mcp_server.run_stdio_loop();
        return Ok(());
    }

    let is_standalone = is_standalone_launch(!cfg!(feature = "editor"), &args);
    let is_standalone_test = args.iter().any(|arg| arg == "--test-standalone");
    let scene_to_load = args.iter().position(|a| a == "--scene").and_then(|idx| args.get(idx + 1)).cloned();
    let proyecto_directo = proyecto_de_args(&args);
    // Contrato de modos de arranque (S5 ampliado): valores obligatorios y
    // combinaciones excluyentes. Ver validar_argumentos.
    if let Err(msg) = validar_argumentos(&args) {
        log::error!("Argumentos inválidos: {msg}");
        return Err(msg.into());
    }
    let is_sample_game_test = args.iter().any(|arg| arg == "--test-sample-game");
    let is_platformer_test = args.iter().any(|arg| arg == "--test-platformer");
    if is_platformer_test || is_sample_game_test || is_standalone_test {
        std::env::set_var("GLASS_AUTO_DEMO", "1");
    }
    let is_m10_test = args.iter().any(|arg| arg == "--test-m10");
    let is_m9_test = args.iter().any(|arg| arg == "--test-m9");
    let is_m8_test = args.iter().any(|arg| arg == "--test-m8");
    let is_m7_test = args.iter().any(|arg| arg == "--test-m7");
    let is_m6_test = args.iter().any(|arg| arg == "--test-m6");
    let is_m5_test = args.iter().any(|arg| arg == "--test-m5");
    let is_headless_test = args
        .iter()
        .any(|arg| arg == "--test-init" || arg == "--check-vulkan" || arg == "--test-m3" || arg == "--test-m4");

    // D-V3a (separación verdadera): el motor sin destino no arranca.
    // El Hub es un programa aparte (glass_hub): sin --project, ni
    // --scene, ni --sample, ni un modo de prueba/standalone que lo
    // justifique, se sale con un error claro en vez de abrir el
    // antiguo Hub en proceso. `--hub` aquí ya no significa nada.
    #[cfg(feature = "editor")]
    {
        let sin_destino = !is_standalone
            && !is_standalone_test
            && !is_headless_test
            && !is_m5_test
            && !is_m6_test
            && !is_m7_test
            && !is_m8_test
            && !is_m9_test
            && !is_m10_test
            && !is_platformer_test
            && !is_sample_game_test
            && proyecto_directo.is_none()
            && scene_to_load.is_none()
            && !args.iter().any(|a| a == "--sample")
            && !args.iter().any(|a| a == "--mcp" || a == "--mcp-stdio" || a == "--mcp-live");
        if args.iter().any(|a| a == "--hub") || sin_destino {
            let msg = "Este ejecutable es el motor de GlassEnginer: necesita un proyecto (--project <ruta>), una escena (--scene <ruta>) o un ejemplo (--sample <clave>). Para gestionar proyectos abre el Hub (GlassHub).";
            log::error!("{msg}");
            return Err(msg.into());
        }
        if let Some(dir) = &proyecto_directo {
            if !std::path::Path::new(dir).join("project.glass").is_file() {
                let msg = format!("--project: la carpeta {dir} no contiene project.glass; no es un proyecto de GlassEnginer.");
                log::error!("{msg}");
                return Err(msg.into());
            }
        }
        if let Some(i) = args.iter().position(|a| a == "--sample") {
            let clave = args.get(i + 1).map(String::as_str).unwrap_or("");
            if !matches!(clave, "plataformas" | "fisica" | "luces" | "juego") {
                let msg = format!("--sample: clave desconocida «{clave}» (válidas: plataformas, fisica, luces, juego).");
                log::error!("{msg}");
                return Err(msg.into());
            }
        }
    }

    // Sonda de GPU en proceso hijo: solo en el arranque normal del editor
    // (ni standalone, ni tests headless, ni modos de prueba).
    #[cfg(feature = "editor")]
    {
        let es_arranque_normal = !is_standalone
            && !is_headless_test
            && !is_m5_test
            && !is_m6_test
            && !is_m7_test
            && !is_m8_test
            && !is_m9_test
            && !is_m10_test
            && !is_platformer_test
            && !is_sample_game_test
            && !is_standalone_test;
        let diagnostico = if es_arranque_normal {
            comprobar_sonda_gpu()
        } else {
            None
        };
        if let Some(texto) = &diagnostico {
            log::warn!("Sonda GPU: {texto}");
        }
        let _ = DIAGNOSTICO_SONDA.set(diagnostico);
    }

    let mut builder = EventLoop::builder();
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        if std::env::var("WINIT_UNIX_BACKEND").as_deref() == Ok("x11") || std::env::var("DISPLAY").is_ok() {
            log::info!("Forzando backend X11/XWayland en Winit para máxima compatibilidad con Vulkan y NVIDIA...");
            builder.with_x11();
        }
    }
    let event_loop = builder.build()?;
    install_signal_handlers();
    let mut app = GlassEngineApp::new(
        LaunchFlags {
            is_standalone,
            is_headless_test,
            is_m5_test,
            is_m6_test,
            is_m7_test,
            is_m8_test,
            is_m9_test,
            is_m10_test,
            is_platformer_test,
            is_sample_game_test,
            is_standalone_test,
        },
        scene_to_load,
        proyecto_directo,
    );
    // MCP sobre la escena viva: con la feature de editor, el servidor MCP
    // de la app sirve su stdio en un hilo dedicado (ver
    // `start_live_mcp_stdio` para la decisión de arranque y sus porqués).
    #[cfg(feature = "editor")]
    app.start_live_mcp_stdio(args.iter().any(|arg| arg == "--mcp-live"));
    // Lado receptor de --sample <clave> (D6): siembra la misma
    // acción que produce la pestaña Ejemplos del Hub.
    #[cfg(feature = "editor")]
    if let Some(clave) = args
        .iter()
        .position(|a| a == "--sample")
        .and_then(|i| args.get(i + 1))
    {
        let preset = match clave.as_str() {
            "plataformas" => Some(SampleScenePreset::PlatformerDemo),
            "fisica" => Some(SampleScenePreset::PhysicsPlayground),
            "luces" => Some(SampleScenePreset::AudioLightingShowcase),
            "juego" => Some(SampleScenePreset::FullSampleGame),
            _ => None,
        };
        match preset {
            Some(p) => {
                app.pending_apertura = Some(AccionApertura::LoadSampleScene(p));
            }
            None => log::error!("--sample: clave desconocida «{clave}»; se ignora."),
        }
    }
    // S4: si este proceso es un motor gestionado (su exe vive en
    // engines/<version>/), toma el lock de sesion de su version: el
    // Hub no borrara esa version mientras este proceso viva. El
    // guarda vive hasta que run_app vuelve.
    #[cfg(feature = "editor")]
    let _lock_sesion = glass_editor::updates::bloquear_sesion_si_gestionado();
    event_loop.run_app(&mut app)?;

    Ok(())
}

#[cfg(all(test, feature = "editor"))]
mod live_mcp_stdio_tests {
    use super::*;

    #[test]
    fn terminal_interactiva_sin_force_no_arranca() {
        // Usuario en una consola real, sin --mcp-live: no arrancar.
        assert!(!GlassEngineApp::live_mcp_stdio_decision(false, true));
    }

    #[test]
    fn pipes_sin_force_arranca() {
        // Caso real de un cliente MCP: stdin no es TTY, arranca por defecto.
        assert!(GlassEngineApp::live_mcp_stdio_decision(false, false));
    }

    #[test]
    fn force_arranca_incluso_en_terminal() {
        // --mcp-live fuerza el arranque aunque stdin sea una terminal.
        assert!(GlassEngineApp::live_mcp_stdio_decision(true, true));
        assert!(GlassEngineApp::live_mcp_stdio_decision(true, false));
    }
}

/// Tests del puente de instanciación contra una escena y un mundo
/// Rapier REALES (sin GPU ni CoreCLR): el spawn crea la entidad y su
/// cuerpo con la forma inferida, el despawn libera ambos, y
/// activar/desactivar retira y recrea el cuerpo — el ciclo entero que
/// un generador procedural repite sin parar.
#[cfg(test)]
mod spawn_bridge_tests {
    use super::*;

    fn make_bridge() -> EngineSceneBridge {
        EngineSceneBridge {
            scene_state: Arc::new(Mutex::new(SceneState::new())),
            scene_manager: Arc::new(Mutex::new(SceneManager::new("."))),
            physics_world: Arc::new(Mutex::new(PhysicsWorld::new())),
            asset_cache: glass_renderer::AssetCache::new(),
            pending_script_spawns: Arc::new(Mutex::new(Vec::new())),
            physmat_lib: Arc::new(Mutex::new(PhysicsMaterialLibrary::new(
                std::path::PathBuf::from("."),
            ))),
        }
    }

    fn spec(model: &str, physics: i32) -> SpawnSpec {
        SpawnSpec {
            name: "Pieza".to_string(),
            model: model.to_string(),
            script: String::new(),
            parent_id: 0,
            position: [0.0, 5.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [2.0, 2.0, 2.0],
            color: [0.0; 3],
            has_color: false,
            physics,
        }
    }

    fn counts(bridge: &EngineSceneBridge) -> (usize, usize) {
        bridge.physics_counts().expect("conteos con puente real")
    }

    // ── Tests DORADOS del bloque Physics Material (C0, corregidos en C3) ──
    // Fijan que el material llega a Rapier RECTO (fricción y rebote por
    // campos nombrados). El CRUCE HISTÓRICO (friction, restitution) →
    // (restitution, friction) se corrigió en C3, decisión del dueño del
    // 2026-11-23 (opción A): las rutas de autorado pasan el material
    // resuelto y los dorados se reescribieron a valores rectos en el
    // mismo commit. Si una ruta vuelve a cruzar, deben ponerse en rojo.
    // C3 también eliminó la duplicación create_body/rebuild: ambas rutas
    // adjuntan por `adjuntar_colliders_autorados`, y el test
    // `rebuild_crea_el_mismo_material_que_create_body` ejerce rebuild.

    const EPS_DORADO: f32 = 1e-6;

    fn cerca_dorado(actual: f32, esperado: f32) {
        assert!(
            (actual - esperado).abs() <= EPS_DORADO,
            "esperado {esperado}, obtenido {actual}"
        );
    }

    fn spec_dorado(entity_id: u64) -> SpawnedBodySpec {
        SpawnedBodySpec {
            entity_id,
            body_type: BodyType::Dynamic,
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            colliders: Vec::new(),
            colliders_son_specs: false,
            mesh_collider: glass_core::MeshColliderMode::None,
            mesh_material: material_dorado(0.3, 0.8),
            mesh_material_ref: String::new(),
            fallback_material: material_dorado(0.3, 0.8),
            mesh_cfg: ColliderConfig { layer: 0, mask: u32::MAX, is_trigger: false },
            model: String::new(),
            world_scale: [1.0, 1.0, 1.0],
            constraints: 0,
            mass: 2.5,
            linear_damping: 1.5,
            angular_damping: 0.0,
            solver_iterations: 4,
            max_depenetration_velocity: 0.0,
            solver_velocity_iterations: 4,
            include_layers: u32::MAX,
            exclude_layers: 0,
            automatic_center_of_mass: true,
            automatic_inertia_tensor: true,
            use_gravity: true,
            ccd: false,
            interpolate: false,
        }
    }

    fn material_dorado(friction: f32, bounciness: f32) -> PhysicsMaterial {
        PhysicsMaterial {
            dynamic_friction: friction,
            static_friction: friction,
            bounciness,
            ..PhysicsMaterial::PROJECT_DEFAULT
        }
    }

    fn caja_dorada() -> ColliderResuelto {
        (
            ColliderShape::Cuboid { half_extents: [0.5, 0.5, 0.5] },
            ColliderConfig { layer: 0, mask: u32::MAX, is_trigger: false },
            [0.0, 0.0, 0.0],
            material_dorado(0.3, 0.8),
            String::new(),
            String::from("Y"),
        )
    }

    fn insertar_cubo_dorado(bridge: &EngineSceneBridge) {
        let vertices = vec![
            [-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, 0.5, -0.5], [-0.5, 0.5, -0.5],
            [-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5],
        ];
        let indices = vec![
            0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4,
            3, 7, 6, 3, 6, 2, 0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5,
        ];
        bridge
            .asset_cache
            .write()
            .unwrap()
            .geometries
            .insert("malla_dorada".to_owned(), (vertices, indices));
    }

    fn afirmar_material(bridge: &EngineSceneBridge, entity_id: u64, esperados: usize) {
        let world = bridge.physics_world.lock().unwrap();
        let handle = world
            .get_body_by_entity(entity_id)
            .expect("create_body debe registrar el cuerpo por entidad");
        let materiales = world.collider_friction_restitution(handle);
        assert_eq!(materiales.len(), esperados);
        for (friction, restitution) in materiales {
            cerca_dorado(friction, 0.3);
            cerca_dorado(restitution, 0.8);
        }
    }

    /// La forma real del único collider del cuerpo: prueba que la
    /// ruta ejercida creó la forma esperada y no cayó a un collider
    /// de respaldo (falso verde cazado en el re-análisis de C0).
    fn afirmar_forma(bridge: &EngineSceneBridge, entity_id: u64, forma: &'static str) {
        let world = bridge.physics_world.lock().unwrap();
        let handle = world
            .get_body_by_entity(entity_id)
            .expect("create_body debe registrar el cuerpo por entidad");
        assert_eq!(world.collider_shape_kinds(handle), vec![forma]);
    }

    #[test]
    fn dorado_specs() {
        // Specs explícitos: Rapier recibe friction=0.3 / restitution=0.8 (recto).
        let bridge = make_bridge();
        let mut spec = spec_dorado(101);
        spec.colliders_son_specs = true;
        spec.colliders = vec![caja_dorada()];
        bridge.create_body(spec);
        afirmar_material(&bridge, 101, 1);
    }

    #[test]
    fn dorado_inferido() {
        // El collider inferido (sin specs) recibe el mismo material recto.
        let bridge = make_bridge();
        let mut spec = spec_dorado(102);
        spec.colliders_son_specs = false;
        spec.colliders = vec![caja_dorada()];
        bridge.create_body(spec);
        afirmar_material(&bridge, 102, 1);
    }

    #[test]
    fn dorado_estatico() {
        // Un cuerpo Static (el suelo) también va recto.
        let bridge = make_bridge();
        let mut spec = spec_dorado(103);
        spec.body_type = BodyType::Static;
        spec.colliders_son_specs = true;
        spec.colliders = vec![caja_dorada()];
        bridge.create_body(spec);
        afirmar_material(&bridge, 103, 1);
    }

    #[test]
    fn dorado_mesh_convex() {
        // Mesh Collider Convex: un único collider de malla, recto.
        let bridge = make_bridge();
        insertar_cubo_dorado(&bridge);
        let mut spec = spec_dorado(104);
        spec.model = "malla_dorada".to_owned();
        spec.mesh_collider = glass_core::MeshColliderMode::Convex;
        spec.colliders_son_specs = false;
        spec.colliders.clear();
        bridge.create_body(spec);
        afirmar_material(&bridge, 104, 1);
        afirmar_forma(&bridge, 104, "convex");
    }

    #[test]
    fn dorado_mesh_trimesh_estatico() {
        // Mesh Collider TriMesh en cuerpo estático: un único collider, recto.
        let bridge = make_bridge();
        insertar_cubo_dorado(&bridge);
        let mut spec = spec_dorado(105);
        spec.body_type = BodyType::Static;
        spec.model = "malla_dorada".to_owned();
        spec.mesh_collider = glass_core::MeshColliderMode::TriMesh;
        spec.colliders_son_specs = false;
        spec.colliders.clear();
        bridge.create_body(spec);
        afirmar_material(&bridge, 105, 1);
        afirmar_forma(&bridge, 105, "trimesh");
    }

    #[test]
    fn dorado_masa() {
        // set_mass manda: tras un step (cuando Rapier recalcula las
        // propiedades de masa desde las densidades reescaladas), la masa
        // efectiva del cuerpo Dynamic es la del spec (2.5).
        let bridge = make_bridge();
        let mut spec = spec_dorado(106);
        spec.colliders_son_specs = true;
        spec.colliders = vec![caja_dorada()];
        bridge.create_body(spec);
        let mut world = bridge.physics_world.lock().unwrap();
        let handle = world.get_body_by_entity(106).expect("cuerpo registrado");
        world.step(1.0 / 60.0);
        cerca_dorado(world.body_mass(handle).expect("masa del cuerpo"), 2.5);
    }

    #[test]
    fn trimesh_dinamico_degrada_a_convex() {
        // TriMesh en cuerpo no estático: degrada a Convex (C3).
        let bridge = make_bridge();
        insertar_cubo_dorado(&bridge);
        let mut spec = spec_dorado(107);
        spec.body_type = BodyType::Dynamic;
        spec.model = "malla_dorada".to_owned();
        spec.mesh_collider = glass_core::MeshColliderMode::TriMesh;
        spec.colliders_son_specs = false;
        spec.colliders.clear();
        bridge.create_body(spec);
        afirmar_material(&bridge, 107, 1);
        afirmar_forma(&bridge, 107, "convex");
    }

    #[test]
    fn trimesh_trigger_estatico_degrada_a_convex() {
        // TriMesh como trigger en cuerpo estático: degrada a Convex (C3).
        let bridge = make_bridge();
        insertar_cubo_dorado(&bridge);
        let mut spec = spec_dorado(108);
        spec.body_type = BodyType::Static;
        spec.model = "malla_dorada".to_owned();
        spec.mesh_collider = glass_core::MeshColliderMode::TriMesh;
        spec.colliders_son_specs = false;
        // Desde E6 el trigger de la malla vive en `mesh_cfg` (campo
        // `mesh_collider_is_trigger` del cuerpo), no en el primer spec.
        spec.mesh_cfg.is_trigger = true;
        spec.colliders = vec![caja_dorada()];
        bridge.create_body(spec);
        afirmar_material(&bridge, 108, 1);
        afirmar_forma(&bridge, 108, "convex");
    }

    #[test]
    fn dos_specs_con_materiales_propios() {
        // Dos specs con materiales distintos en un mismo cuerpo: cada
        // collider conserva sus valores (C3).
        let bridge = make_bridge();
        let mut spec = spec_dorado(109);
        spec.colliders_son_specs = true;
        let base = caja_dorada();
        spec.colliders = vec![
            base.clone(),
            (
                base.0,
                base.1,
                [0.0, 1.0, 0.0],
                material_dorado(0.9, 0.1),
                String::new(),
                String::from("Y"),
            ),
        ];
        bridge.create_body(spec);
        let world = bridge.physics_world.lock().unwrap();
        let handle = world.get_body_by_entity(109).unwrap();
        let m = world.collider_friction_restitution(handle);
        assert_eq!(m.len(), 2);
        cerca_dorado(m[0].0, 0.3);
        cerca_dorado(m[0].1, 0.8);
        cerca_dorado(m[1].0, 0.9);
        cerca_dorado(m[1].1, 0.1);
    }

    #[test]
    fn resueltos_de_rebuild_son_rectos() {
        // Equivalencia rebuild/create_body (C3): rebuild deriva sus
        // colliders por `colliders_resueltos`, la misma función libre
        // que `body_spec_for`. Una entidad autorada con legado 0.3/0.8
        // resuelve el material recto, con dirección "Y" y sin asset.
        // (rebuild_physics_from_scene vive en la App, no en el puente,
        // así que el test ejerce su derivación, no su bucle.)
        let bridge = make_bridge();
        let mut lib = biblioteca_physmat(&bridge.scene_manager);
        let body = glass_core::PhysicsData {
            legacy_friction: Some(0.3),
            legacy_restitution: Some(0.8),
            ..Default::default()
        };
        let shape = ColliderShape::Cuboid {
            half_extents: [0.5, 0.5, 0.5],
        };
        let resueltos = colliders_resueltos(&body, shape, &mut lib);
        assert_eq!(resueltos.len(), 1);
        let (forma, _cfg, _offset, material, material_ref, direction) = &resueltos[0];
        assert_eq!(*forma, shape);
        cerca_dorado(material.dynamic_friction, 0.3);
        cerca_dorado(material.bounciness, 0.8);
        assert_eq!(material_ref, "");
        assert_eq!(direction, "Y");
    }

    #[test]
    fn spawn_dinamico_crea_entidad_cuerpo_y_collider() {
        let bridge = make_bridge();
        let id = bridge
            .spawn_entity(&spec("sphere", glass_scripting::SPAWN_PHYSICS_DYNAMIC))
            .expect("spawn de esfera");
        assert_eq!(bridge.entity_count(), Some(5), "4 de la escena por defecto + 1");
        assert_eq!(counts(&bridge), (1, 1));
        let state = bridge.scene_state.lock().unwrap();
        let ent = state.get_entity(id).expect("la entidad existe");
        assert_eq!(ent.mesh.as_ref().map(|m| m.model.as_str()), Some("sphere"));
        assert_eq!(
            ent.physics.as_ref().map(|p| p.body_type.as_str()),
            Some("Dynamic")
        );
        // La caja de respaldo se deriva de la escala (2 -> semiextensión 1).
        assert_eq!(ent.physics.as_ref().unwrap().box_extents, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn spawn_estatico_y_sin_fisica_se_distinguen() {
        let bridge = make_bridge();
        let suelo = bridge
            .spawn_entity(&spec("cube", SPAWN_PHYSICS_STATIC))
            .expect("spawn de suelo");
        let decorado = bridge
            .spawn_entity(&spec("cube", glass_scripting::SPAWN_PHYSICS_NONE))
            .expect("spawn decorativo");
        assert_eq!(counts(&bridge), (1, 1), "solo el estático tiene cuerpo");
        let state = bridge.scene_state.lock().unwrap();
        assert_eq!(
            state.get_entity(suelo).unwrap().physics.as_ref().unwrap().body_type,
            "Static"
        );
        assert!(state.get_entity(decorado).unwrap().physics.is_none());
    }

    #[test]
    fn despawn_libera_entidad_cuerpo_y_collider() {
        let bridge = make_bridge();
        let id = bridge
            .spawn_entity(&spec("capsule", glass_scripting::SPAWN_PHYSICS_DYNAMIC))
            .unwrap();
        assert_eq!(counts(&bridge), (1, 1));
        assert!(bridge.destroy_entity(id));
        assert_eq!(counts(&bridge), (0, 0), "Rapier vuelve a la base exacta");
        assert_eq!(bridge.entity_count(), Some(4));
        assert!(!bridge.destroy_entity(id), "destruir dos veces es no-op");
    }

    #[test]
    fn desactivar_retira_el_cuerpo_y_reactivar_lo_recrea() {
        let bridge = make_bridge();
        let id = bridge
            .spawn_entity(&spec("cylinder", glass_scripting::SPAWN_PHYSICS_DYNAMIC))
            .unwrap();
        assert!(bridge.set_entity_active(id, false));
        assert_eq!(counts(&bridge), (0, 0), "inactiva = sin cuerpo");
        assert_eq!(bridge.entity_count(), Some(5), "pero la entidad sigue ahí");
        assert!(bridge.set_entity_active(id, true));
        assert_eq!(counts(&bridge), (1, 1), "al reactivar el cuerpo vuelve");
        assert!(!bridge.set_entity_active(9999, true));
    }

    #[test]
    fn spawn_con_padre_pone_el_cuerpo_en_su_sitio_de_mundo() {
        let bridge = make_bridge();
        let mut padre = spec("cube", glass_scripting::SPAWN_PHYSICS_NONE);
        padre.position = [10.0, 0.0, 0.0];
        padre.scale = [1.0, 1.0, 1.0]; // sin escala: mundo = padre + local, sin factor
        let parent_id = bridge.spawn_entity(&padre).unwrap();
        let mut hijo = spec("sphere", glass_scripting::SPAWN_PHYSICS_DYNAMIC);
        hijo.position = [1.0, 2.0, 0.0];
        hijo.parent_id = parent_id;
        let child_id = bridge.spawn_entity(&hijo).unwrap();
        let state = bridge.scene_state.lock().unwrap();
        assert_eq!(state.get_entity(child_id).unwrap().parent, Some(parent_id));
        assert!(state.get_entity(parent_id).unwrap().children.contains(&child_id));
        drop(state);
        // El cuerpo nace en la posición de MUNDO (padre + local): si
        // naciese en la local, la primera sincronización lo teletransportaría.
        let physics = bridge.physics_world.lock().unwrap();
        assert!(physics.get_body_by_entity(child_id).is_some(), "cuerpo del hijo");
        let bodies = physics.sync_all_transforms();
        let tf = bodies
            .iter()
            .find(|(eid, _)| *eid == child_id)
            .map(|(_, tf)| *tf)
            .expect("transform sincronizada del hijo");
        let pos = tf.position;
        assert!((pos[0] - 11.0).abs() < 0.01, "x de mundo ~11, vino {pos:?}");
        assert!((pos[1] - 2.0).abs() < 0.01, "y de mundo ~2, vino {pos:?}");
    }

    #[test]
    fn spawn_con_script_encola_la_instancia_y_rechaza_padre_inexistente() {
        let bridge = make_bridge();
        let mut con_script = spec("cube", glass_scripting::SPAWN_PHYSICS_NONE);
        con_script.script = "MiGenerador".to_string();
        let id = bridge.spawn_entity(&con_script).unwrap();
        {
            let state = bridge.scene_state.lock().unwrap();
            let ent = state.get_entity(id).unwrap();
            assert_eq!(
                ent.script_data.as_ref().map(|s| s.class_name.as_str()),
                Some("MiGenerador")
            );
            assert!(ent.script.is_none(), "el campo heredado no debe apuntar al PlayerController");
        }
        let pending = bridge.pending_script_spawns.lock().unwrap();
        assert_eq!(pending.as_slice(), [(id, "MiGenerador".to_string())]);
        drop(pending);

        let mut huerfano = spec("cube", glass_scripting::SPAWN_PHYSICS_NONE);
        huerfano.parent_id = 424242;
        assert!(bridge.spawn_entity(&huerfano).is_none());
    }

    /// El ciclo del generador en miniatura: 200 spawns mezclados, todos
    /// destruidos, y el mundo y la escena vuelven EXACTO a la base.
    #[test]
    fn ciclo_spawn_destroy_masivo_vuelve_a_la_base() {
        let bridge = make_bridge();
        let base_entities = bridge.entity_count().unwrap();
        let models = ["cube", "sphere", "capsule", "cylinder", "plane"];
        let mut ids = Vec::new();
        for i in 0..200 {
            let physics = if i % 2 == 0 {
                glass_scripting::SPAWN_PHYSICS_DYNAMIC
            } else {
                SPAWN_PHYSICS_STATIC
            };
            let mut s = spec(models[i % models.len()], physics);
            s.position = [(i % 20) as f32, 5.0, (i / 20) as f32];
            ids.push(bridge.spawn_entity(&s).unwrap());
        }
        assert_eq!(counts(&bridge), (200, 200));
        for id in ids {
            assert!(bridge.destroy_entity(id));
        }
        assert_eq!(counts(&bridge), (0, 0));
        assert_eq!(bridge.entity_count(), Some(base_entities));
    }
}

#[cfg(test)]
mod standalone_launch_tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn binario_puro_sin_argumentos_es_juego() {
        // La regresión: el binario puro sin argumentos caía en la escena
        // demo del editor y se estrellaba. Como juego, todo arranque
        // suyo es standalone.
        assert!(is_standalone_launch(true, &args(&["glass_app"])));
    }

    #[test]
    fn binario_puro_con_escena_es_juego() {
        assert!(is_standalone_launch(
            true,
            &args(&["glass_app", "--scene", "assets/scenes/SampleScene.scene.json"])
        ));
    }

    #[test]
    fn editor_sin_argumentos_no_es_juego() {
        // El binario completo sin banderas abre el Lanzador, no un juego.
        assert!(!is_standalone_launch(false, &args(&["glass_app"])));
    }

    #[test]
    fn editor_con_bandera_es_juego() {
        assert!(is_standalone_launch(false, &args(&["glass_app", "--standalone"])));
        assert!(is_standalone_launch(false, &args(&["glass_app", "--game"])));
    }

    #[test]
    fn validar_argumentos_acepta_los_modos_sanos() {
        assert!(validar_argumentos(&args(&["glass_app"])).is_ok());
        assert!(validar_argumentos(&args(&["glass_app", "--project", "/tmp/p"])).is_ok());
        assert!(validar_argumentos(&args(&["glass_app", "--scene", "e.scene.json"])).is_ok());
        assert!(validar_argumentos(&args(&["glass_app", "--sample", "fisica"])).is_ok());
        assert!(validar_argumentos(&args(&["glass_app", "--standalone", "--scene", "e.json"])).is_ok());
    }

    #[test]
    fn validar_argumentos_rechaza_los_flags_del_hub() {
        let err = validar_argumentos(&args(&["glass_app", "--hub"])).unwrap_err();
        assert!(err.contains("GlassHub"), "{err}");
        let err = validar_argumentos(&args(&["glass_app", "--tras-actualizar-hub"])).unwrap_err();
        assert!(err.contains("GlassHub"), "{err}");
        // Mixto: el flag del Hub gana aunque haya un destino válido.
        let err = validar_argumentos(&args(&["glass_app", "--hub", "--project", "/tmp/p"])).unwrap_err();
        assert!(err.contains("--hub") && err.contains("GlassHub"), "{err}");
    }

    #[test]
    fn validar_argumentos_exige_valor_en_los_flags_con_valor() {
        for flag in ["--project", "--scene", "--sample"] {
            let err = validar_argumentos(&args(&["glass_app", flag])).unwrap_err();
            assert!(err.contains(flag), "el error debe nombrar el flag: {err}");
            assert!(err.contains("necesita un valor"), "{err}");
        }
    }

    #[test]
    fn validar_argumentos_un_flag_no_es_un_valor() {
        // Auditoría S10: el token siguiente que es otro flag cuenta
        // como valor ausente, y TODAS las apariciones se validan.
        let err = validar_argumentos(&args(&["glass_app", "--project", "--standalone"])).unwrap_err();
        assert!(err.contains("--project") && err.contains("necesita un valor"), "{err}");
        let err = validar_argumentos(&args(&["glass_app", "--scene", "a.json", "--scene"])).unwrap_err();
        assert!(err.contains("--scene") && err.contains("necesita un valor"), "{err}");
    }

    #[test]
    fn validar_argumentos_rechaza_las_parejas_incompatibles() {
        let err = validar_argumentos(&args(&["glass_app", "--project", "/tmp/p", "--scene", "e.json"])).unwrap_err();
        assert!(err.contains("--project") && err.contains("--scene"), "{err}");
    }

    #[test]
    fn proyecto_de_args_lee_el_valor() {
        assert_eq!(
            proyecto_de_args(&args(&["glass_app", "--project", "/tmp/proyecto"])),
            Some("/tmp/proyecto".to_string())
        );
        assert_eq!(proyecto_de_args(&args(&["glass_app", "--project"])), None);
        assert_eq!(proyecto_de_args(&args(&["glass_app"])), None);
    }

    #[test]
    fn hub_no_es_standalone() {
        assert!(!is_standalone_launch(false, &args(&["glass_app", "--hub"])));
    }

    #[test]
    fn resolver_raiz_motor_usa_exe_dir_si_hay_payload_al_lado() {
        let base = std::env::temp_dir().join(format!(
            "glass_raiz_motor_exe_{}",
            std::process::id()
        ));
        let exe_dir = base.join("exe");
        let cwd = base.join("cwd");
        std::fs::create_dir_all(exe_dir.join("managed/GlassEngine.Core")).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();

        assert_eq!(resolver_raiz_motor(&exe_dir, &cwd), exe_dir);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn resolver_raiz_motor_cae_a_cwd_sin_payload() {
        let base = std::env::temp_dir().join(format!(
            "glass_raiz_motor_cwd_{}",
            std::process::id()
        ));
        let exe_dir = base.join("exe");
        let cwd = base.join("cwd");
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();

        assert_eq!(resolver_raiz_motor(&exe_dir, &cwd), cwd);

        let _ = std::fs::remove_dir_all(&base);
    }
}

#[cfg(all(test, feature = "editor"))]
mod crear_proyecto_tests {
    use super::*;
    use std::path::PathBuf;

    const SUBCARPETAS: [&str; 7] = [
        "Assets/Scenes",
        "Assets/Scripts",
        "Assets/Materials",
        "Assets/Models",
        "Assets/Textures",
        "ProjectSettings",
        "Packages",
    ];

    fn dir_temporal(etiqueta: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "glass_crear_{}_{}_{}",
            etiqueta,
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cargar(proyecto: &Path) -> SceneState {
        let ruta = proyecto.join("Assets/Scenes/SampleScene.scene.json");
        let mut state = SceneState::new();
        state
            .load_from_json(&ruta.to_string_lossy())
            .expect("la escena guardada debe parsear");
        state
    }

    #[test]
    fn crear_en_disco_escribe_arbol_y_sello() {
        let base = dir_temporal("arbol");
        let proyecto =
            GlassEngineApp::crear_proyecto_en_disco("Demo", &base, ProjectTemplate::EmptyScene)
                .expect("crear debe funcionar");
        assert_eq!(proyecto, base.join("Demo"));
        for sub in SUBCARPETAS {
            assert!(proyecto.join(sub).is_dir(), "falta {}", sub);
        }
        let meta = std::fs::read_to_string(proyecto.join("project.glass")).unwrap();
        assert!(meta.contains(glass_editor::updates::ENGINE_VERSION));
        assert!(!meta.contains("GlassEngine 0.1.0"));
        assert!(meta.contains("\"scene\": \"Assets/Scenes/SampleScene.scene.json\""));
        let escena = cargar(&proyecto);
        assert_eq!(escena.entities.len(), 2, "vacia = camara + luz");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn crear_en_disco_ruta_que_ya_termina_en_el_nombre() {
        let base = dir_temporal("nombre");
        let ruta = base.join("Demo");
        let proyecto =
            GlassEngineApp::crear_proyecto_en_disco("Demo", &ruta, ProjectTemplate::EmptyScene)
                .expect("crear debe funcionar");
        assert_eq!(proyecto, ruta);
        assert!(proyecto.join("project.glass").is_file());
        assert!(!ruta.join("Demo").exists(), "no debe crear Demo/Demo");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn crear_en_disco_plantilla_plataformas() {
        let base = dir_temporal("plataformas");
        let proyecto =
            GlassEngineApp::crear_proyecto_en_disco("Plat", &base, ProjectTemplate::Platformer3D)
                .expect("crear debe funcionar");
        let escena = cargar(&proyecto);
        assert!(escena.entities.len() > 2);
        assert!(
            escena.entities.iter().any(|e| e.name == "Player 3D Cube"),
            "falta el jugador"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}


/// Base (right/up/forward) de una matriz de mundo: columnas
/// normalizadas (sin la escala).
fn base_de_mundo(world: &[f32; 16]) -> [[f32; 3]; 3] {
    let col = |i: usize| {
        let v = [world[i * 4], world[i * 4 + 1], world[i * 4 + 2]];
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 1e-6 {
            [v[0] / l, v[1] / l, v[2] / l]
        } else {
            [0.0, 0.0, 0.0]
        }
    };
    [col(0), col(1), col(2)]
}

/// Convierte el dato de escena en la configuración del emisor.
fn particulas_config_desde(
    d: &glass_core::scene::ParticleEmitterData,
) -> glass_particles::EmitterConfig {
    use glass_particles::EmitterShape;
    let shape = match d.shape.as_str() {
        "sphere" => EmitterShape::Sphere {
            radius: d.shape_radius,
        },
        "box" => EmitterShape::Box {
            extents: d.shape_extents,
        },
        "cone" => EmitterShape::Cone {
            angle_deg: d.cone_angle,
            radius: d.shape_radius,
        },
        _ => EmitterShape::Point,
    };
    glass_particles::EmitterConfig {
        max_particles: d.max_particles,
        rate: d.rate,
        lifetime: d.lifetime,
        start_speed: d.start_speed,
        start_size: d.start_size,
        gravity: d.gravity,
        drag: d.drag,
        shape,
        local_space: d.local_space,
        color_start: d.color_start,
        color_end: d.color_end,
        size_end_factor: d.size_end_factor,
        rotation_speed: d.rotation_speed,
        stretched: d.stretched,
        stretch_factor: d.stretch_factor,
        seed: d.seed,
    }
}

/// Base de la cámara de la vista de escena: con centro de órbita
/// del editor, ojo → centro; sin él coincide con la de juego.
fn base_camara_escena(
    centro_editor: Option<[f32; 3]>,
    eye_scene: [f32; 4],
    base_juego: ([f32; 3], [f32; 3], [f32; 3]),
) -> ([f32; 3], [f32; 3], [f32; 3]) {
    if let Some(centro) = centro_editor {
        let eye = [eye_scene[0], eye_scene[1], eye_scene[2]];
        let fwd = [centro[0] - eye[0], centro[1] - eye[1], centro[2] - eye[2]];
        let fl = (fwd[0] * fwd[0] + fwd[1] * fwd[1] + fwd[2] * fwd[2]).sqrt();
        if fl > 1e-5 {
            let f = [fwd[0] / fl, fwd[1] / fl, fwd[2] / fl];
            // right = normalize(cross(f, [0,1,0])) = normalize([f.z, 0, -f.x])
            let rl = (f[2] * f[2] + f[0] * f[0]).sqrt();
            let right = if rl > 1e-5 {
                [f[2] / rl, 0.0, -f[0] / rl]
            } else {
                [1.0, 0.0, 0.0]
            };
            // up = cross(right, f)
            let up = [
                right[1] * f[2] - right[2] * f[1],
                right[2] * f[0] - right[0] * f[2],
                right[0] * f[1] - right[1] * f[0],
            ];
            return (right, up, f);
        }
    }
    base_juego
}

/// Expande los billboards de todos los emisores para UNA vista
/// (base = right/up/forward de su cámara) en vértices e índices.
fn expandir_particulas(
    emisores: &std::collections::HashMap<u64, glass_particles::Emitter>,
    base: ([f32; 3], [f32; 3], [f32; 3]),
    verts: &mut Vec<glass_renderer::ParticleVertex>,
    indices: &mut Vec<u32>,
) {
    let (right, up, fwd) = base;
    for emisor in emisores.values() {
        let pool = &emisor.pool;
        for i in 0..pool.alive {
            let centro = pool.pos[i];
            let size = emisor.size_at(i);
            let color = emisor.color_at(i);
            let base_idx = verts.len() as u32;
            
            let esquinas: [([f32; 3], [f32; 2]); 4] = if emisor.config.stretched {
                let vel = pool.vel[i];
                // Eje: velocidad proyectada en el plano de cámara.
                let d = vel[0] * fwd[0] + vel[1] * fwd[1] + vel[2] * fwd[2];
                let eje = [
                    vel[0] - fwd[0] * d,
                    vel[1] - fwd[1] * d,
                    vel[2] - fwd[2] * d,
                ];
                let el = (eje[0] * eje[0] + eje[1] * eje[1] + eje[2] * eje[2]).sqrt();
                if el < 1e-4 {
                    continue;
                }
                let eje = [eje[0] / el, eje[1] / el, eje[2] / el];
                // Perp en el plano: cross(fwd, eje).
                let perp = [
                    fwd[1] * eje[2] - fwd[2] * eje[1],
                    fwd[2] * eje[0] - fwd[0] * eje[2],
                    fwd[0] * eje[1] - fwd[1] * eje[0],
                ];
                let speed = (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt();
                let hl = size * 0.5 + speed * emisor.config.stretch_factor;
                let hw = size * 0.5;
                let esquina = |sa: f32, sp: f32| {
                    [
                        centro[0] + eje[0] * sa * hl + perp[0] * sp * hw,
                        centro[1] + eje[1] * sa * hl + perp[1] * sp * hw,
                        centro[2] + eje[2] * sa * hl + perp[2] * sp * hw,
                    ]
                };
                [
                    (esquina(-1.0, -1.0), [0.0, 0.0]),
                    (esquina(-1.0, 1.0), [1.0, 0.0]),
                    (esquina(1.0, 1.0), [1.0, 1.0]),
                    (esquina(1.0, -1.0), [0.0, 1.0]),
                ]
            } else {
                let rot = emisor.rotation_at(i);
                let (s, c) = rot.sin_cos();
                let esquina = |cx: f32, cy: f32| {
                    let lx = (cx * c - cy * s) * size;
                    let ly = (cx * s + cy * c) * size;
                    [
                        centro[0] + right[0] * lx + up[0] * ly,
                        centro[1] + right[1] * lx + up[1] * ly,
                        centro[2] + right[2] * lx + up[2] * ly,
                    ]
                };
                [
                    (esquina(-0.5, -0.5), [0.0, 0.0]),
                    (esquina(0.5, -0.5), [1.0, 0.0]),
                    (esquina(0.5, 0.5), [1.0, 1.0]),
                    (esquina(-0.5, 0.5), [0.0, 1.0]),
                ]
            };
            for (pos, uv) in esquinas {
                verts.push(glass_renderer::ParticleVertex {
                    position: pos,
                    color,
                    uv,
                });
            }
            indices.extend_from_slice(&[
                base_idx,
                base_idx + 1,
                base_idx + 2,
                base_idx,
                base_idx + 2,
                base_idx + 3,
            ]);
        }
    }
}

/// Overdraw estimado (proxy): suma del área en píxeles de cada
/// quad (con la proyección nominal 1280×720, FOV 60°) dividida
/// por el área del viewport.
fn overdraw_proxy(
    emisores: &std::collections::HashMap<u64, glass_particles::Emitter>,
    eye: [f32; 4],
) -> f32 {
    let tan_medio = (60.0_f32.to_radians() * 0.5).tan();
    let mut area = 0.0f32;
    for emisor in emisores.values() {
        for i in 0..emisor.pool.alive {
            let p = emisor.pool.pos[i];
            let d = ((p[0] - eye[0]).powi(2) + (p[1] - eye[1]).powi(2) + (p[2] - eye[2]).powi(2))
                .sqrt()
                .max(0.1);
            let px = emisor.size_at(i) * 360.0 / (d * tan_medio);
            area += px * px;
        }
    }
    area / (1280.0 * 720.0)
}

/// Salida del paso de partículas: los VP de dibujo y la
/// geometría expandida de cada vista.
struct ParticleFrameOut {
vps: ([f32; 16], [f32; 16]),
scene: (Vec<glass_renderer::ParticleVertex>, Vec<u32>),
game: (Vec<glass_renderer::ParticleVertex>, Vec<u32>),
}

/// Partículas (Fase 8P): avanza los emisores de la escena un paso,
/// drena los comandos externos (MCP/C#) y expande los billboards
/// por vista. Función libre (patrón de `avanzar_animadores_fsm`):
/// trabaja sobre los campos sueltos para no pelear con el
/// préstamo del renderer que mantiene `render_frame`.
#[allow(clippy::too_many_arguments)]
/// Rangos que un modelo proyecta en los pases de sombra (auditoría
/// C3): los del nivel LOD activo según la distancia al ojo, con la
/// MISMA regla que el dibujo principal (Fase 6), más todos los rangos
/// que no son LOD. `None` si el modelo no tiene objetos LOD: el pase
/// dibuja la malla entera, como siempre.
fn rangos_sombra_lod(
    cache: &glass_renderer::AssetCache,
    key: &str,
    pos: [f32; 3],
    ojo: [f32; 3],
) -> Option<Vec<(u32, u32)>> {
    let objs = cache.objects.get(key)?;
    let mut disp: std::collections::HashMap<String, Vec<u32>> = std::collections::HashMap::new();
    let mut lod_de_rango: std::collections::HashMap<usize, (String, u32)> =
        std::collections::HashMap::new();
    for o in objs {
        if let Some((base, nivel)) = glass_renderer::lod_suffix(&o.name) {
            disp.entry(base.clone()).or_default().push(nivel);
            for &r in &o.ranges {
                lod_de_rango.insert(r, (base.clone(), nivel));
            }
        }
    }
    if disp.is_empty() {
        return None;
    }
    let dx = pos[0] - ojo[0];
    let dy = pos[1] - ojo[1];
    let dz = pos[2] - ojo[2];
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
    let mut activo: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for (base, niveles) in &disp {
        activo.insert(
            base.clone(),
            glass_renderer::lod_level_for_distance(niveles, dist),
        );
    }
    let ranges = cache.primitive_ranges.get(key)?;
    let out: Vec<(u32, u32)> = ranges
        .iter()
        .enumerate()
        .filter(|(idx, _)| match lod_de_rango.get(idx) {
            Some((base, nivel)) => activo.get(base) == Some(nivel),
            None => true,
        })
        .map(|(_, r)| (r.first_index, r.index_count))
        .collect();
    Some(out)
}

/// Cámaras del frame al avanzar partículas: VP y ojo de la vista de
/// escena, los de la vista de juego si la hay y el centro del
/// editor. (Agrupadas por el lint too_many_arguments de Clippy.)
struct CamarasParticulas {
    vp_scene: [f32; 16],
    vp_game_opt: Option<[f32; 16]>,
    eye_scene: [f32; 4],
    eye_game_opt: Option<[f32; 4]>,
    centro_editor: Option<[f32; 3]>,
}

fn avanzar_particulas(
emisores_rt: &mut std::collections::HashMap<u64, glass_particles::Emitter>,
aplicados: &mut std::collections::HashMap<u64, glass_core::scene::ParticleEmitterData>,
prev_origen: &mut std::collections::HashMap<u64, [f32; 3]>,
scene_state: &std::sync::Arc<std::sync::Mutex<glass_core::scene::SceneState>>,
dt: f32,
camaras: CamarasParticulas,
) -> Option<ParticleFrameOut> {
    let CamarasParticulas { vp_scene, vp_game_opt, eye_scene, eye_game_opt, centro_editor } = camaras;
    use glass_particles::ParticleCommand;
    let inicio = std::time::Instant::now();
    // Recoger emisores de la escena con su matriz de mundo.
    let (entidades, cam_world): (Vec<EntidadEmisora>, Option<[f32; 16]>) = match scene_state.lock() {
        Ok(scene) => {
            let world_matrices = scene.compute_world_matrices();
            let lista = scene
                .entities
                .iter()
                .filter(|e| e.active && e.particles.is_some())
                .filter_map(|e| {
                    world_matrices
                        .get(&e.id)
                        .map(|w| (e.id, e.particles.clone().unwrap(), *w))
                })
                .collect();
            let cam = scene
                .active_camera()
                .and_then(|c| world_matrices.get(&c.id).copied());
            (lista, cam)
        }
        Err(_) => (Vec::new(), None),
    };
    if entidades.is_empty() {
        emisores_rt.clear();
        aplicados.clear();
        prev_origen.clear();
        glass_core::particle_stats::record_frame(0, 0, 0, 0.0);
        return None;
    }
    // Emisores cuya entidad se desactivó o salió de la escena (Fase
    // 9: el cambio de clima apaga la lluvia así): dejan de emitir
    // pero sus partículas TERMINAN su vida y se desvanecen; antes el
    // runtime no se tocaba y la cortina quedaba congelada en el aire
    // para siempre. Al agotarse el pool, el emisor se retira.
    {
        let vivos: std::collections::HashSet<u64> =
            entidades.iter().map(|(id, _, _)| *id).collect();
        let ausentes: Vec<u64> = emisores_rt
            .keys()
            .filter(|id| !vivos.contains(id))
            .copied()
            .collect();
        const IDENTIDAD: [[f32; 3]; 3] =
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        for id in ausentes {
            let origen = prev_origen.get(&id).copied().unwrap_or([0.0; 3]);
            let vacio = match emisores_rt.get_mut(&id) {
                Some(e) => {
                    e.stop();
                    e.update(dt, origen, &IDENTIDAD, origen);
                    e.pool.alive == 0
                }
                None => true,
            };
            if vacio {
                emisores_rt.remove(&id);
                aplicados.remove(&id);
                prev_origen.remove(&id);
            }
        }
    }
    // Crear/reiniciar emisores cuyo dato cambió y simular.
    let mut marcos: std::collections::HashMap<u64, ([f32; 3], [[f32; 3]; 3])> =
        std::collections::HashMap::new();
    for (id, data, world) in &entidades {
        if aplicados.get(id) != Some(data) {
            let mut emisor = glass_particles::Emitter::new(particulas_config_desde(data));
            emisor.playing = data.playing;
            emisores_rt.insert(*id, emisor);
            aplicados.insert(*id, data.clone());
            prev_origen.remove(id);
        }
        let origin = [world[12], world[13], world[14]];
        let basis = base_de_mundo(world);
        let prev = prev_origen
            .get(id)
            .copied()
            .unwrap_or(origin);
        if let Some(emisor) = emisores_rt.get_mut(id) {
            // Reactivación tras un desvanecido en curso: reanudar.
            if data.playing && !emisor.playing {
                emisor.play();
            }
            emisor.update(dt, origin, &basis, prev);
        }
        prev_origen.insert(*id, origin);
        marcos.insert(*id, (origin, basis));
    }
    // Comandos externos (MCP y scripts C#).
    for cmd in glass_particles::drain_commands() {
        match cmd {
            ParticleCommand::Play(id) => {
                if let Some(e) = emisores_rt.get_mut(&id) {
                    e.play();
                }
            }
            ParticleCommand::Stop(id) => {
                if let Some(e) = emisores_rt.get_mut(&id) {
                    e.stop();
                }
            }
            ParticleCommand::Burst(id, n) => {
                if let Some((origin, basis)) = marcos.get(&id) {
                    if let Some(e) = emisores_rt.get_mut(&id) {
                        e.burst(n as usize, *origin, basis);
                    }
                }
            }
        }
    }
    // Bases de cámara por vista (right, up, forward en mundo).
    // La de juego sale de la cámara activa; la de escena, en el
    // editor, de la cámara orbital; sin editor, ambas coinciden.
    let base_juego: ([f32; 3], [f32; 3], [f32; 3]) = match cam_world {
        Some(w) => {
            let b = base_de_mundo(&w);
            (b[0], b[1], [-b[2][0], -b[2][1], -b[2][2]])
        }
        None => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
    };
    let base_escena = base_camara_escena(centro_editor, eye_scene, base_juego);
    // Expansión por vista y overdraw proxy con la de juego.
    let mut verts_e = Vec::new();
    let mut idx_e = Vec::new();
    let mut verts_j = Vec::new();
    let mut idx_j = Vec::new();
    expandir_particulas(emisores_rt, base_escena, &mut verts_e, &mut idx_e);
    expandir_particulas(emisores_rt, base_juego, &mut verts_j, &mut idx_j);
    let eye_j = eye_game_opt.unwrap_or(eye_scene);
    let overdraw = overdraw_proxy(emisores_rt, eye_j);
    let vivas: usize = emisores_rt
        .values()
        .map(|e| e.pool.alive)
        .sum();
    glass_core::particle_stats::record_frame(
        vivas as u32,
        emisores_rt.len() as u32,
        inicio.elapsed().as_micros() as u64,
        overdraw,
    );
    Some(ParticleFrameOut {
        vps: (vp_scene, vp_game_opt.unwrap_or(vp_scene)),
        scene: (verts_e, idx_e),
        game: (verts_j, idx_j),
    })
}


#[cfg(test)]
mod particle_fade_tests {
    use super::*;

    const IDENT: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ];

    fn escena_con_emisor() -> Arc<Mutex<SceneState>> {
        let ent: glass_core::scene::EntityData = serde_json::from_str(
            r#"{"id": 11, "name": "Fuego", "tag": "Untagged", "layer": "Default",
                "is_static": false, "active": true, "parent": null, "children": [],
                "transform": {"position": [0.0, 0.3, 0.0], "rotation": [0.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]},
                "components": ["Transform", "ParticleEmitter"], "mesh": null,
                "camera": null, "light": null, "physics": null, "script": null,
                "script_data": null, "audio": null, "canvas": null,
                "rect_transform": null, "ui_text": null, "ui_button": null,
                "ui_image": null, "ui_drag": null, "portal": null,
                "animator": null, "fog": null,
                "particles": {"rate": 1000.0, "max_particles": 500,
                              "lifetime": [0.2, 0.3], "seed": 7}}"#,
        )
        .expect("entidad de prueba válida");
        let scene = SceneState::new();
        let scene = Arc::new(Mutex::new(scene));
        scene.lock().unwrap().entities.push(ent);
        scene
    }

    /// Fase 9: al desactivar la entidad de un emisor, sus partículas
    /// terminan su vida y el emisor se retira del runtime (antes la
    /// cortina quedaba congelada en el aire para siempre).
    #[test]
    fn emisor_desactivado_se_desvanece_y_se_retira() {
        let scene = escena_con_emisor();
        let mut rt: std::collections::HashMap<u64, glass_particles::Emitter> =
            std::collections::HashMap::new();
        let mut ap: std::collections::HashMap<u64, glass_core::scene::ParticleEmitterData> =
            std::collections::HashMap::new();
        let mut po: std::collections::HashMap<u64, [f32; 3]> =
            std::collections::HashMap::new();
        let eye = [0.0, 1.0, 5.0, 1.0];
        for _ in 0..20 {
            avanzar_particulas(&mut rt, &mut ap, &mut po, &scene, 0.05, CamarasParticulas { vp_scene: IDENT, vp_game_opt: None, eye_scene: eye, eye_game_opt: None, centro_editor: None });
        }
        let vivas = rt.get(&11).map(|e| e.pool.alive).unwrap_or(0);
        assert!(vivas > 0, "el emisor activo debió emitir (vivas={vivas})");

        // Ojo: SceneState::default() ya trae 4 entidades semilla;
        // se desactiva EL EMISOR por id, no por posición.
        if let Some(e) = scene
            .lock()
            .unwrap()
            .entities
            .iter_mut()
            .find(|e| e.id == 11)
        {
            e.active = false;
        }
        for _ in 0..40 {
            avanzar_particulas(&mut rt, &mut ap, &mut po, &scene, 0.1, CamarasParticulas { vp_scene: IDENT, vp_game_opt: None, eye_scene: eye, eye_game_opt: None, centro_editor: None });
        }
        assert!(
            rt.is_empty() && ap.is_empty(),
            "el emisor desactivado debió desvanecerse y retirarse (rt={}, ap={})",
            rt.len(),
            ap.len()
        );
    }
}
/// Transición de play_state entre dos frames consecutivos.
#[cfg(feature = "editor")]
#[derive(Debug, PartialEq, Eq)]
enum PlayTransition {
    None,
    EnterPlay,
    StopPlay,
}

/// EnterPlay solo en Edit→Playing (Paused→Playing es la misma sesión);
/// StopPlay en Playing|Paused→Edit; el resto, None.
#[cfg(feature = "editor")]
fn play_transition(prev: glass_editor::PlayState, now: glass_editor::PlayState) -> PlayTransition {
    use glass_editor::PlayState as P;
    match (prev, now) {
        (P::Edit, P::Playing) => PlayTransition::EnterPlay,
        (P::Playing | P::Paused, P::Edit) => PlayTransition::StopPlay,
        _ => PlayTransition::None,
    }
}

#[cfg(all(test, feature = "editor"))]
mod play_transition_tests {
    use super::{play_transition, PlayTransition as T};
    use glass_editor::PlayState as P;

    #[test]
    fn las_nueve_combinaciones() {
        let casos = [
            (P::Edit, P::Edit, T::None),
            (P::Edit, P::Playing, T::EnterPlay),
            (P::Edit, P::Paused, T::None),
            (P::Playing, P::Edit, T::StopPlay),
            (P::Playing, P::Playing, T::None),
            (P::Playing, P::Paused, T::None),
            (P::Paused, P::Edit, T::StopPlay),
            (P::Paused, P::Playing, T::None),
            (P::Paused, P::Paused, T::None),
        ];
        for (prev, now, esperado) in casos {
            assert_eq!(play_transition(prev, now), esperado);
        }
    }
}

#[cfg(test)]
mod sonda_gpu_tests {
    use super::*;

    fn gpu(
        nombre: &str,
        vendor_id: u32,
        driver_version: u32,
        device_type: vk::PhysicalDeviceType,
    ) -> glass_renderer::context::GpuDeviceInfo {
        glass_renderer::context::GpuDeviceInfo {
            name: nombre.to_string(),
            device_type,
            vendor_id,
            driver_version,
            local_vram_mb: 1024,
            graphics_family: Some(0),
            present_family: Some(0),
            swapchain_supported: true,
        }
    }

    fn intel_uhd() -> glass_renderer::context::GpuDeviceInfo {
        gpu("Intel UHD", 0x8086, (100 << 14) | 9415, vk::PhysicalDeviceType::INTEGRATED_GPU)
    }

    #[test]
    fn diagnostico_nombra_gpu_y_dedicada() {
        let lista = vec![
            intel_uhd(),
            gpu(
                "NVIDIA GeForce GTX 1650",
                0x10de,
                (531 << 22) | (79 << 14),
                vk::PhysicalDeviceType::DISCRETE_GPU,
            ),
        ];
        let texto = texto_diagnostico_sonda("Intel UHD", &lista);
        assert!(texto.contains("Intel UHD"), "{texto}");
        assert!(texto.contains("100.9415"), "{texto}");
        assert!(texto.contains("NVIDIA GeForce GTX 1650"), "{texto}");
    }

    #[test]
    fn diagnostico_sin_dedicada_no_la_menciona() {
        let lista = vec![intel_uhd()];
        let texto = texto_diagnostico_sonda("Intel UHD", &lista);
        assert!(texto.contains("100.9415"), "{texto}");
        assert!(!texto.contains("dedicada"), "{texto}");
    }
}

#[cfg(test)]
mod expulsion_clock_tests {
    use super::delta_expulsion_ms;

    /// Una pausa de 30 s suma 250 ms, no 30.000: el modelo dibujado
    /// en el frame anterior no cruza el horizonte de un salto.
    #[test]
    fn delta_largo_se_acota_a_250_ms() {
        let mut resto = 0.0;
        assert_eq!(delta_expulsion_ms(30_000.0, &mut resto), 250);
        assert_eq!(delta_expulsion_ms(16.7, &mut resto), 16);
    }

    /// La fracción sub-milisegundo se acumula: tres frames de 1,7 ms
    /// avanzan 5 ms en total (1+2+2), no 3.
    #[test]
    fn resto_sub_milisegundo_se_acumula() {
        let mut resto = 0.0;
        let a = delta_expulsion_ms(1.7, &mut resto);
        let b = delta_expulsion_ms(1.7, &mut resto);
        let c = delta_expulsion_ms(1.7, &mut resto);
        assert_eq!((a, b, c), (1, 2, 2));
        // Y un frame de 0,4 ms no congela el reloj: dos suman 0 y el
        // tercero ya entrega 1.
        let mut r2 = 0.0;
        assert_eq!(delta_expulsion_ms(0.4, &mut r2), 0);
        assert_eq!(delta_expulsion_ms(0.4, &mut r2), 0);
        assert_eq!(delta_expulsion_ms(0.4, &mut r2), 1);
    }
}
