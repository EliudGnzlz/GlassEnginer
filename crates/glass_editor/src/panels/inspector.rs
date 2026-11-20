use egui::{Color32, DragValue, RichText, Slider, Ui};
use glass_core::{
    AudioSourceData, CameraData, CanvasData, CharacterData, EntityData, LightData, MeshData,
    PhysicsData,
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


// ======================================================================
// P6: portapapeles de componentes, cabeceras con menú contextual y ✕,
// y el candado de escala proporcional del Transform.
// ======================================================================

/// Componentes del Inspector que participan en copiar/pegar y en el
/// botón ✕ de la cabecera. Rigidbody y Collider comparten PhysicsData:
/// cada uno copia solo su parte de campos.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TipoComponente {
    Transform,
    MeshRenderer,
    Rigidbody,
    Collider,
    CharacterController,
    Camera,
    Light,
    Script,
    AudioSource,
    Canvas,
    RectTransform,
    Texto,
    Boton,
    Imagen,
    DragDrop,
}

impl TipoComponente {
    /// Nombre tal como aparece en `EntityData::components`.
    pub fn nombre(self) -> &'static str {
        match self {
            TipoComponente::Transform => "Transform",
            TipoComponente::MeshRenderer => "MeshRenderer",
            TipoComponente::Rigidbody => "Rigidbody",
            TipoComponente::Collider => "Collider",
            TipoComponente::CharacterController => "CharacterController",
            TipoComponente::Camera => "Camera",
            TipoComponente::Light => "Light",
            TipoComponente::Script => "Script",
            TipoComponente::AudioSource => "AudioSource",
            TipoComponente::Canvas => "Canvas",
            TipoComponente::RectTransform => "RectTransform",
            TipoComponente::Texto => "Text",
            TipoComponente::Boton => "Button",
            TipoComponente::Imagen => "Image",
            TipoComponente::DragDrop => "DragDrop",
        }
    }
}

/// Lo copiado con «Copiar datos» (es_componente = false: solo se puede
/// pegar sobre el mismo tipo) o con «Copiar componente» (true: se puede
/// pegar como componente nuevo en cualquier entidad).
#[derive(Clone, Debug)]
pub struct PortapapelesComponente {
    pub tipo: TipoComponente,
    pub json: String,
    pub es_componente: bool,
}

/// Lo que el usuario pidió desde la cabecera de un componente: se
/// recoge mientras se pinta y se aplica después de la sección, como el
/// resto de acciones diferidas del Inspector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccionCabecera {
    Nada,
    Quitar,
    CopiarDatos,
    CopiarComponente,
    PegarDatos,
    PegarComoNuevo,
}

fn a_json<T: serde::Serialize>(v: &T) -> Option<String> {
    serde_json::to_string(v).ok()
}

fn registrar_nombre(components: &mut Vec<String>, nombre: &str) {
    if !components.iter().any(|c| c == nombre) {
        components.push(nombre.to_string());
    }
}

fn sustituir<T: serde::de::DeserializeOwned>(slot: &mut Option<T>, json: &str) -> bool {
    if slot.is_none() {
        return false;
    }
    match serde_json::from_str(json) {
        Ok(v) => {
            *slot = Some(v);
            true
        }
        Err(_) => false,
    }
}

fn crear<T: serde::de::DeserializeOwned>(
    slot: &mut Option<T>,
    json: &str,
    components: &mut Vec<String>,
    nombre: &str,
) -> bool {
    if slot.is_some() {
        return false;
    }
    let Ok(v) = serde_json::from_str(json) else {
        return false;
    };
    *slot = Some(v);
    registrar_nombre(components, nombre);
    true
}

/// Deja un PhysicsData sin parte Collider (para extraer solo el RB).
fn neutralizar_collider(p: &mut PhysicsData) {
    p.has_collider = false;
    p.colliders.clear();
    p.mesh_collider = glass_core::MeshColliderMode::None;
}

/// Copia solo la parte Rigidbody de `src` en `dst` y activa el flag.
/// Los campos de capa/colisión son del cuerpo entero y no se tocan.
fn copiar_parte_rb(dst: &mut PhysicsData, src: &PhysicsData) {
    dst.body_type = src.body_type.clone();
    dst.mass = src.mass;
    dst.linear_damping = src.linear_damping;
    dst.angular_damping = src.angular_damping;
    dst.use_gravity = src.use_gravity;
    dst.constraints = src.constraints;
    dst.ccd = src.ccd;
    dst.interpolate = src.interpolate;
    dst.solver_iterations = src.solver_iterations;
    dst.solver_velocity_iterations = src.solver_velocity_iterations;
    dst.max_depenetration_velocity = src.max_depenetration_velocity;
    dst.automatic_center_of_mass = src.automatic_center_of_mass;
    dst.automatic_inertia_tensor = src.automatic_inertia_tensor;
    dst.has_rigidbody = true;
}

/// Copia solo la parte Collider de `src` en `dst` y activa el flag.
fn copiar_parte_collider(dst: &mut PhysicsData, src: &PhysicsData) {
    dst.colliders = src.colliders.clone();
    dst.mesh_collider = src.mesh_collider;
    dst.box_extents = src.box_extents;
    dst.box_offset = src.box_offset;
    dst.is_trigger = src.is_trigger;
    dst.friction = src.friction;
    dst.restitution = src.restitution;
    dst.has_collider = true;
}

/// Foto serializable del componente Script (nombre + datos): la entidad
/// lo guarda en dos campos y el portapapeles lo trata como uno solo.
#[derive(serde::Serialize, serde::Deserialize)]
struct ScriptFoto {
    script: Option<String>,
    data: Option<ScriptComponentData>,
}

/// JSON del componente `tipo` de la entidad, o None si no lo tiene.
pub fn extraer_componente(ent: &EntityData, tipo: TipoComponente) -> Option<String> {
    match tipo {
        TipoComponente::Transform => a_json(&ent.transform),
        TipoComponente::MeshRenderer => ent.mesh.as_ref().and_then(a_json),
        TipoComponente::CharacterController => ent.character.as_ref().and_then(a_json),
        TipoComponente::Camera => ent.camera.as_ref().and_then(a_json),
        TipoComponente::Light => ent.light.as_ref().and_then(a_json),
        TipoComponente::AudioSource => ent.audio.as_ref().and_then(a_json),
        TipoComponente::Canvas => ent.canvas.as_ref().and_then(a_json),
        TipoComponente::RectTransform => ent.rect_transform.as_ref().and_then(a_json),
        TipoComponente::Texto => ent.ui_text.as_ref().and_then(a_json),
        TipoComponente::Boton => ent.ui_button.as_ref().and_then(a_json),
        TipoComponente::Imagen => ent.ui_image.as_ref().and_then(a_json),
        TipoComponente::DragDrop => ent.ui_drag.as_ref().and_then(a_json),
        TipoComponente::Script => {
            if ent.script.is_none() && ent.script_data.is_none() {
                None
            } else {
                a_json(&ScriptFoto {
                    script: ent.script.clone(),
                    data: ent.script_data.clone(),
                })
            }
        }
        TipoComponente::Rigidbody => ent
            .physics
            .as_ref()
            .filter(|p| p.has_rigidbody)
            .and_then(|p| {
                let mut c = p.clone();
                neutralizar_collider(&mut c);
                a_json(&c)
            }),
        TipoComponente::Collider => ent
            .physics
            .as_ref()
            .filter(|p| p.has_collider)
            .and_then(|p| {
                let mut c = p.clone();
                c.has_rigidbody = false;
                a_json(&c)
            }),
    }
}

/// Pega VALORES sobre el componente existente del mismo tipo. False si
/// la entidad no tiene ese componente o el JSON no casa.
pub fn aplicar_datos(ent: &mut EntityData, tipo: TipoComponente, json: &str) -> bool {
    match tipo {
        TipoComponente::Transform => match serde_json::from_str(json) {
            Ok(t) => {
                ent.transform = t;
                true
            }
            Err(_) => false,
        },
        TipoComponente::MeshRenderer => sustituir(&mut ent.mesh, json),
        TipoComponente::CharacterController => sustituir(&mut ent.character, json),
        TipoComponente::Camera => sustituir(&mut ent.camera, json),
        TipoComponente::Light => sustituir(&mut ent.light, json),
        TipoComponente::AudioSource => sustituir(&mut ent.audio, json),
        TipoComponente::Canvas => sustituir(&mut ent.canvas, json),
        TipoComponente::RectTransform => sustituir(&mut ent.rect_transform, json),
        TipoComponente::Texto => sustituir(&mut ent.ui_text, json),
        TipoComponente::Boton => sustituir(&mut ent.ui_button, json),
        TipoComponente::Imagen => sustituir(&mut ent.ui_image, json),
        TipoComponente::DragDrop => sustituir(&mut ent.ui_drag, json),
        TipoComponente::Script => {
            if ent.script.is_none() && ent.script_data.is_none() {
                return false;
            }
            match serde_json::from_str::<ScriptFoto>(json) {
                Ok(f) => {
                    ent.script = f.script;
                    ent.script_data = f.data;
                    true
                }
                Err(_) => false,
            }
        }
        TipoComponente::Rigidbody => {
            let Some(p) = ent.physics.as_mut().filter(|p| p.has_rigidbody) else {
                return false;
            };
            let Ok(srcp) = serde_json::from_str::<PhysicsData>(json) else {
                return false;
            };
            copiar_parte_rb(p, &srcp);
            true
        }
        TipoComponente::Collider => {
            let Some(p) = ent.physics.as_mut().filter(|p| p.has_collider) else {
                return false;
            };
            let Ok(srcp) = serde_json::from_str::<PhysicsData>(json) else {
                return false;
            };
            copiar_parte_collider(p, &srcp);
            true
        }
    }
}

/// Pega el componente como NUEVO en la entidad. False si ya lo tiene
/// (para los Option) o si es el Transform, que no se puede añadir.
pub fn instalar_componente(ent: &mut EntityData, tipo: TipoComponente, json: &str) -> bool {
    let nombre = tipo.nombre();
    match tipo {
        TipoComponente::Transform => false,
        TipoComponente::MeshRenderer => crear(&mut ent.mesh, json, &mut ent.components, nombre),
        TipoComponente::CharacterController => {
            crear(&mut ent.character, json, &mut ent.components, nombre)
        }
        TipoComponente::Camera => crear(&mut ent.camera, json, &mut ent.components, nombre),
        TipoComponente::Light => crear(&mut ent.light, json, &mut ent.components, nombre),
        TipoComponente::AudioSource => crear(&mut ent.audio, json, &mut ent.components, nombre),
        TipoComponente::Canvas => crear(&mut ent.canvas, json, &mut ent.components, nombre),
        TipoComponente::RectTransform => {
            crear(&mut ent.rect_transform, json, &mut ent.components, nombre)
        }
        TipoComponente::Texto => crear(&mut ent.ui_text, json, &mut ent.components, nombre),
        TipoComponente::Boton => crear(&mut ent.ui_button, json, &mut ent.components, nombre),
        TipoComponente::Imagen => crear(&mut ent.ui_image, json, &mut ent.components, nombre),
        TipoComponente::DragDrop => crear(&mut ent.ui_drag, json, &mut ent.components, nombre),
        TipoComponente::Script => {
            if ent.script.is_some() || ent.script_data.is_some() {
                return false;
            }
            match serde_json::from_str::<ScriptFoto>(json) {
                Ok(f) => {
                    ent.script = f.script;
                    ent.script_data = f.data;
                    registrar_nombre(&mut ent.components, nombre);
                    true
                }
                Err(_) => false,
            }
        }
        TipoComponente::Rigidbody => {
            let Ok(mut srcp) = serde_json::from_str::<PhysicsData>(json) else {
                return false;
            };
            match ent.physics.as_mut() {
                None => {
                    neutralizar_collider(&mut srcp);
                    srcp.has_rigidbody = true;
                    ent.physics = Some(srcp);
                }
                Some(p) => copiar_parte_rb(p, &srcp),
            }
            registrar_nombre(&mut ent.components, nombre);
            true
        }
        TipoComponente::Collider => {
            let Ok(mut srcp) = serde_json::from_str::<PhysicsData>(json) else {
                return false;
            };
            match ent.physics.as_mut() {
                None => {
                    srcp.has_rigidbody = false;
                    srcp.has_collider = true;
                    ent.physics = Some(srcp);
                }
                Some(p) => copiar_parte_collider(p, &srcp),
            }
            registrar_nombre(&mut ent.components, nombre);
            true
        }
    }
}

/// El menú contextual de la cabecera de un componente: copiar datos,
/// copiar componente, pegar datos (solo datos del mismo tipo) y pegar
/// componente como nuevo (solo si el portapapeles trae un componente).
pub fn menu_componente(
    ui: &mut Ui,
    tipo: TipoComponente,
    portapapeles: Option<&PortapapelesComponente>,
) -> AccionCabecera {
    let mut accion = AccionCabecera::Nada;
    if ui.button("📋 Copiar datos").clicked() {
        accion = AccionCabecera::CopiarDatos;
        ui.close();
    }
    if ui.button("📋 Copiar componente").clicked() {
        accion = AccionCabecera::CopiarComponente;
        ui.close();
    }
    ui.separator();
    let puede_datos = portapapeles.is_some_and(|p| !p.es_componente && p.tipo == tipo);
    if ui
        .add_enabled(puede_datos, egui::Button::new("📥 Pegar datos"))
        .clicked()
    {
        accion = AccionCabecera::PegarDatos;
        ui.close();
    }
    let puede_nuevo = tipo != TipoComponente::Transform
        && portapapeles.is_some_and(|p| p.es_componente);
    if ui
        .add_enabled(puede_nuevo, egui::Button::new("📥 Pegar componente como nuevo"))
        .clicked()
    {
        accion = AccionCabecera::PegarComoNuevo;
        ui.close();
    }
    accion
}

/// Sección de componente con la cabecera común de P6: flecha y título
/// (el título también alterna), menú contextual con clic derecho sobre
/// el título y, si es quitable, el botón ✕ a la derecha de la cabecera.
/// Sustituye a `ui.collapsing` conservando su estado por defecto
/// (cerrada) y su id por sección dentro del `push_id` de la entidad.
/// Devuelve la acción pedida y el resultado del cuerpo, si se pintó.
#[allow(clippy::too_many_arguments)]
pub fn seccion_componente<R>(
    ui: &mut Ui,
    id_salt: &str,
    titulo: &str,
    tipo: TipoComponente,
    quitable: bool,
    portapapeles: Option<&PortapapelesComponente>,
    accion_out: &mut AccionCabecera,
    cuerpo: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let id = ui.make_persistent_id(("seccion_componente", id_salt));
    let estado = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let mut alternar = false;
    let mut cabecera = estado.show_header(ui, |ui| {
        let resp_titulo = ui.add(
            egui::Button::new(RichText::new(titulo).strong())
                .frame(false)
                .wrap_mode(egui::TextWrapMode::Extend),
        );
        if resp_titulo.clicked() {
            alternar = true;
        }
        resp_titulo.context_menu(|ui| {
            let a = menu_componente(ui, tipo, portapapeles);
            if a != AccionCabecera::Nada {
                *accion_out = a;
            }
        });
        if quitable {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("✕")
                    .on_hover_text("Quitar componente")
                    .clicked()
                {
                    *accion_out = AccionCabecera::Quitar;
                }
            });
        }
    });
    if alternar {
        cabecera.toggle();
    }
    let (_, _, cuerpo_resp) = cabecera.body(cuerpo);
    cuerpo_resp.map(|r| r.inner)
}

/// Escala proporcional del candado del Transform: cambiar un eje mueve
/// los otros dos guardando la proporción. Si el eje editado valía ~0 no
/// hay proporción que guardar y los demás reciben el mismo delta. El
/// valor nuevo se acota a ≥ 0 (con actual > 0, un nuevo negativo da
/// factor 0 y todo queda en 0) y ningún resultado es negativo ni NaN.
pub fn escala_proporcional(actual: [f32; 3], eje: usize, nuevo: f32) -> [f32; 3] {
    if eje >= 3 {
        return actual;
    }
    let nuevo = if nuevo.is_finite() { nuevo.max(0.0) } else { 0.0 };
    let actual = actual.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    let anterior = actual[eje];
    let mut resultado = actual;
    if anterior.abs() > 1.0e-6 {
        let factor = nuevo / anterior;
        for valor in &mut resultado {
            *valor = (*valor * factor).max(0.0);
        }
        resultado[eje] = nuevo;
    } else {
        let delta = nuevo - anterior;
        for (i, valor) in resultado.iter_mut().enumerate() {
            *valor = if i == eje {
                nuevo
            } else {
                (actual[i] + delta).max(0.0)
            };
        }
    }
    resultado
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
    /// Portapapeles de componentes de P6 (copiar datos / componente).
    portapapeles: Option<PortapapelesComponente>,
    /// Candado de escala proporcional del Transform y a qué entidad
    /// pertenece (se suelta al cambiar la selección).
    escala_bloqueada: bool,
    escala_bloqueada_para: Option<u64>,
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
            portapapeles: None,
            escala_bloqueada: false,
            escala_bloqueada_para: None,
        }
    }

    /// Aplica una acción de cabecera de P6 sobre la entidad. «Quitar»
    /// no pasa por aquí: cada sección lo traduce a su flag diferido de
    /// siempre, con su semántica exacta de retirada.
    fn ejecutar_accion_cabecera(
        &mut self,
        entity: &mut EntityData,
        tipo: TipoComponente,
        accion: AccionCabecera,
    ) {
        match accion {
            AccionCabecera::Nada | AccionCabecera::Quitar => {}
            AccionCabecera::CopiarDatos | AccionCabecera::CopiarComponente => {
                if let Some(json) = extraer_componente(entity, tipo) {
                    self.portapapeles = Some(PortapapelesComponente {
                        tipo,
                        json,
                        es_componente: accion == AccionCabecera::CopiarComponente,
                    });
                }
            }
            AccionCabecera::PegarDatos => {
                if let Some(pp) = self.portapapeles.as_ref() {
                    if pp.tipo == tipo {
                        let json = pp.json.clone();
                        aplicar_datos(entity, tipo, &json);
                    }
                }
            }
            AccionCabecera::PegarComoNuevo => {
                if let Some(pp) = self.portapapeles.as_ref() {
                    let (tipo_pp, json) = (pp.tipo, pp.json.clone());
                    instalar_componente(entity, tipo_pp, &json);
                }
            }
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
        // El candado de escala es por entidad: al cambiar la selección
        // se suelta, como el resto de estado por selección del panel.
        if self.escala_bloqueada_para != Some(entity.id) {
            self.escala_bloqueada = false;
            self.escala_bloqueada_para = Some(entity.id);
        }

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
                let mut accion_transform = AccionCabecera::Nada;
                ui.horizontal(|ui| {
                    let resp_titulo = ui.add(
                        egui::Label::new(RichText::new("📐 Transform").strong())
                            .sense(egui::Sense::click()),
                    );
                    resp_titulo.context_menu(|ui| {
                        let a = menu_componente(
                            ui,
                            TipoComponente::Transform,
                            self.portapapeles.as_ref(),
                        );
                        if a != AccionCabecera::Nada {
                            accion_transform = a;
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("⟲ Reset").clicked() {
                            reset_transform = true;
                        }
                    });
                });
                self.ejecutar_accion_cabecera(
                    entity,
                    TipoComponente::Transform,
                    accion_transform,
                );

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

                        // Escala, con el candado de proporción de P6:
                        // activado, cambiar un eje mueve los otros dos
                        // guardando la proporción (escala_proporcional).
                        let texto_candado = if self.escala_bloqueada {
                            "🔒 Escala"
                        } else {
                            "🔓 Escala"
                        };
                        if ui.small_button(texto_candado).clicked() {
                            self.escala_bloqueada = !self.escala_bloqueada;
                        }
                        let escala_antes = entity.transform.scale;
                        ui.horizontal(|ui| {
                            ui.colored_label(red, "X");
                            if ui
                                .add(DragValue::new(&mut entity.transform.scale[0]).speed(0.02))
                                .changed()
                                && self.escala_bloqueada
                            {
                                let n = entity.transform.scale[0];
                                entity.transform.scale = escala_proporcional(escala_antes, 0, n);
                            }
                            ui.colored_label(green, "Y");
                            if ui
                                .add(DragValue::new(&mut entity.transform.scale[1]).speed(0.02))
                                .changed()
                                && self.escala_bloqueada
                            {
                                let n = entity.transform.scale[1];
                                entity.transform.scale = escala_proporcional(escala_antes, 1, n);
                            }
                            ui.colored_label(blue, "Z");
                            if ui
                                .add(DragValue::new(&mut entity.transform.scale[2]).speed(0.02))
                                .changed()
                                && self.escala_bloqueada
                            {
                                let n = entity.transform.scale[2];
                                entity.transform.scale = escala_proporcional(escala_antes, 2, n);
                            }
                        });
                        ui.end_row();
                    });

                ui.separator();

                // ==========================================
                // 3. COMPONENTE MESH RENDERER & MATERIAL
                // ==========================================
                let mut accion_mesh = AccionCabecera::Nada;
                let mut remove_mesh = false;
                if let Some(ref mut mesh) = entity.mesh {
                    seccion_componente(ui, "mesh", "◈ Mesh Renderer & Material", TipoComponente::MeshRenderer, true, self.portapapeles.as_ref(), &mut accion_mesh, |ui| {
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

                    });
                    ui.separator();
                }
                if accion_mesh == AccionCabecera::Quitar {
                    remove_mesh = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::MeshRenderer, accion_mesh);
                }
                if remove_mesh {
                    entity.mesh = None;
                    entity.components.retain(|c| c != "MeshRenderer" && c != "Material");
                }

                // ==========================================
                // 3.5. COMPONENTE CÁMARA (CAMERA ESTILO UNITY)
                // ==========================================
                let mut accion_camera = AccionCabecera::Nada;
                let mut remove_camera = false;
                if let Some(ref mut cam) = entity.camera {
                    seccion_componente(ui, "camera", "🎥 Cámara (Camera)", TipoComponente::Camera, true, self.portapapeles.as_ref(), &mut accion_camera, |ui| {
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
                    });
                    ui.separator();
                }
                if accion_camera == AccionCabecera::Quitar {
                    remove_camera = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::Camera, accion_camera);
                }
                if remove_camera {
                    entity.camera = None;
                    entity.components.retain(|c| c != "Camera");
                }

                // ==========================================
                // 3.6. COMPONENTE LUZ (LIGHT ESTILO UNITY)
                // ==========================================
                let mut accion_light = AccionCabecera::Nada;
                let mut remove_light = false;
                if let Some(ref mut light) = entity.light {
                    seccion_componente(ui, "light", "☼ Luz (Light)", TipoComponente::Light, true, self.portapapeles.as_ref(), &mut accion_light, |ui| {
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
                    });
                    ui.separator();
                }
                if accion_light == AccionCabecera::Quitar {
                    remove_light = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::Light, accion_light);
                }
                if remove_light {
                    entity.light = None;
                    entity.components.retain(|c| c != "Light");
                }

                // ==========================================
                // 4. COMPONENTES RIGIDBODY Y COLLIDER (RAPIER3D)
                // Separados como en Unity: un Collider sin Rigidbody es estático.
                // ==========================================
                let mut accion_rb = AccionCabecera::Nada;
                let mut accion_col = AccionCabecera::Nada;
                let mut accion_cols = AccionCabecera::Nada;
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
                        seccion_componente(ui, "rigidbody", "⚙ Rigidbody", TipoComponente::Rigidbody, true, self.portapapeles.as_ref(), &mut accion_rb, |ui| {
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

                        });
                    }

                    if phys.has_collider
                        && phys.colliders.is_empty()
                        && phys.mesh_collider == glass_core::MeshColliderMode::None
                    {
                        seccion_componente(ui, "collider", "🧊 Collider", TipoComponente::Collider, true, self.portapapeles.as_ref(), &mut accion_col, |ui| {
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

                        });
                    } else if phys.has_collider {
                        // P3: cada collider es su propio componente, como
                        // en Unity. Fricción y elasticidad son del cuerpo
                        // (compartidos) y van una sola vez, arriba.
                        seccion_componente(ui, "colliders", "🧊 Colliders", TipoComponente::Collider, true, self.portapapeles.as_ref(), &mut accion_cols, |ui| {
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
                if accion_rb == AccionCabecera::Quitar {
                    remove_rigidbody = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::Rigidbody, accion_rb);
                }
                if accion_col == AccionCabecera::Quitar {
                    remove_collider = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::Collider, accion_col);
                }
                if accion_cols == AccionCabecera::Quitar {
                    remove_collider = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::Collider, accion_cols);
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
                let mut accion_character = AccionCabecera::Nada;
                let mut remove_character = false;
                if let Some(ref mut cc) = entity.character {
                    seccion_componente(ui, "character", "🚶 Character Controller", TipoComponente::CharacterController, true, self.portapapeles.as_ref(), &mut accion_character, |ui| {
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
                    });
                }
                if accion_character == AccionCabecera::Quitar {
                    remove_character = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::CharacterController, accion_character);
                }
                if remove_character {
                    entity.character = None;
                    entity.components.retain(|c| c != "CharacterController");
                }

                // ==========================================
                // 5. COMPONENTE SCRIPT C# (.NET CORECLR)
                // ==========================================
                let mut accion_script = AccionCabecera::Nada;
                let mut remove_script = false;
                if let Some(ref mut script_name) = entity.script {
                    seccion_componente(ui, "script", "§ Script C# (.NET CoreCLR)", TipoComponente::Script, true, self.portapapeles.as_ref(), &mut accion_script, |ui| {
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

                    });
                    ui.separator();
                }
                if accion_script == AccionCabecera::Quitar {
                    remove_script = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::Script, accion_script);
                }
                if remove_script {
                    entity.script = None;
                    entity.script_data = None;
                    entity.components.retain(|c| c != "Script");
                }

                // ==========================================
                // 6. COMPONENTE AUDIOSOURCE (KIRA 2D/3D)
                // ==========================================
                let mut accion_audio = AccionCabecera::Nada;
                let mut remove_audio = false;
                if let Some(ref mut audio) = entity.audio {
                    seccion_componente(ui, "audio", "♫ AudioSource (Kira CPAL)", TipoComponente::AudioSource, true, self.portapapeles.as_ref(), &mut accion_audio, |ui| {
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
                        });
                    });
                    ui.separator();
                }
                if accion_audio == AccionCabecera::Quitar {
                    remove_audio = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::AudioSource, accion_audio);
                }
                if remove_audio {
                    entity.audio = None;
                    entity.components.retain(|c| c != "AudioSource");
                }

                // ==========================================
                // 6.5. COMPONENTES DE UI (CANVAS, RECTTRANSFORM, TEXT, BUTTON)
                // ==========================================
                let mut accion_canvas = AccionCabecera::Nada;
                let mut remove_canvas = false;
                if let Some(ref mut canvas) = entity.canvas {
                    seccion_componente(ui, "canvas", "🖼 Canvas (Sistema UI)", TipoComponente::Canvas, true, self.portapapeles.as_ref(), &mut accion_canvas, |ui| {
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
                    });
                    ui.separator();
                }
                if accion_canvas == AccionCabecera::Quitar {
                    remove_canvas = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::Canvas, accion_canvas);
                }
                if remove_canvas {
                    entity.canvas = None;
                    entity.components.retain(|c| c != "Canvas");
                }

                let mut accion_rect = AccionCabecera::Nada;

                let mut remove_rect = false;
                if let Some(ref mut rt) = entity.rect_transform {
                    seccion_componente(ui, "rect", "⊡ RectTransform (Geometría UI 2D)", TipoComponente::RectTransform, true, self.portapapeles.as_ref(), &mut accion_rect, |ui| {
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
                    });
                    ui.separator();
                }
                if accion_rect == AccionCabecera::Quitar {
                    remove_rect = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::RectTransform, accion_rect);
                }
                if remove_rect {
                    entity.rect_transform = None;
                    entity.components.retain(|c| c != "RectTransform");
                }

                let mut accion_text = AccionCabecera::Nada;

                let mut remove_text = false;
                if let Some(ref mut txt) = entity.ui_text {
                    seccion_componente(ui, "text", "🔤 Text (TMP / UI Text)", TipoComponente::Texto, true, self.portapapeles.as_ref(), &mut accion_text, |ui| {
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
                    });
                    ui.separator();
                }
                if accion_text == AccionCabecera::Quitar {
                    remove_text = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::Texto, accion_text);
                }
                if remove_text {
                    entity.ui_text = None;
                    entity.components.retain(|c| c != "Text");
                }

                let mut accion_btn = AccionCabecera::Nada;

                let mut remove_btn = false;
                if let Some(ref mut btn) = entity.ui_button {
                    seccion_componente(ui, "button", "🔘 Button (Interactable)", TipoComponente::Boton, true, self.portapapeles.as_ref(), &mut accion_btn, |ui| {
                        ui.checkbox(&mut btn.interactable, "Interactable (Habilitado)");
                        ui.horizontal(|ui| {
                            ui.label("Color Botón:");
                            ui.color_edit_button_rgba_unmultiplied(&mut btn.target_graphic_color);
                        });
                        ui.horizontal(|ui| {
                            ui.label("On Click Event:");
                            ui.text_edit_singleline(&mut btn.on_click_action);
                        });
                    });
                    ui.separator();
                }
                if accion_btn == AccionCabecera::Quitar {
                    remove_btn = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::Boton, accion_btn);
                }
                if remove_btn {
                    entity.ui_button = None;
                    entity.components.retain(|c| c != "Button");
                }

                let mut accion_img = AccionCabecera::Nada;

                let mut remove_img = false;
                if let Some(ref mut img) = entity.ui_image {
                    seccion_componente(ui, "image", "🖼 Image", TipoComponente::Imagen, true, self.portapapeles.as_ref(), &mut accion_img, |ui| {
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
                    });
                    ui.separator();
                }
                if accion_img == AccionCabecera::Quitar {
                    remove_img = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::Imagen, accion_img);
                }
                if remove_img {
                    entity.ui_image = None;
                    entity.components.retain(|c| c != "Image");
                }

                let mut accion_drag = AccionCabecera::Nada;

                let mut remove_drag = false;
                if let Some(ref mut drag) = entity.ui_drag {
                    seccion_componente(ui, "drag", "✋ Drag & Drop (UI)", TipoComponente::DragDrop, true, self.portapapeles.as_ref(), &mut accion_drag, |ui| {
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
                    });
                    ui.separator();
                }
                if accion_drag == AccionCabecera::Quitar {
                    remove_drag = true;
                } else {
                    self.ejecutar_accion_cabecera(entity, TipoComponente::DragDrop, accion_drag);
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
    use super::{
        aplicar_datos, escala_proporcional, extraer_componente, instalar_componente,
        scan_model_names_in, TipoComponente,
    };
    use glass_core::{CameraData, ColliderSpec, EntityData, PhysicsData};
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

    // ---- P6: portapapeles de componentes ----

    fn ent_p6() -> EntityData {
        EntityData::new(1, "E".to_string(), vec!["Transform".to_string()])
    }

    #[test]
    fn p6_transform_extraer_y_aplicar() {
        let mut a = ent_p6();
        a.transform.position = [1.0, 2.0, 3.0];
        let mut b = ent_p6();
        let json = extraer_componente(&a, TipoComponente::Transform).unwrap();
        assert!(aplicar_datos(&mut b, TipoComponente::Transform, &json));
        assert_eq!(b.transform.position, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn p6_extraer_rigidbody_no_arrastra_colliders() {
        let mut a = ent_p6();
        let mut p = PhysicsData {
            has_rigidbody: true,
            has_collider: true,
            ..Default::default()
        };
        p.colliders.push(ColliderSpec {
            shape: "Box".to_string(),
            size: [0.5, 0.5, 0.5],
            offset: [0.0, 0.0, 0.0],
            is_trigger: false,
        });
        a.physics = Some(p);
        let json = extraer_componente(&a, TipoComponente::Rigidbody).unwrap();
        let copia: PhysicsData = serde_json::from_str(&json).unwrap();
        assert!(copia.has_rigidbody);
        assert!(!copia.has_collider);
        assert!(copia.colliders.is_empty());
        assert_eq!(copia.mesh_collider, glass_core::MeshColliderMode::None);
    }

    #[test]
    fn p6_aplicar_datos_collider_no_toca_la_masa() {
        let mut origen = ent_p6();
        let po = PhysicsData {
            has_collider: true,
            is_trigger: true,
            mass: 1.0,
            ..Default::default()
        };
        origen.physics = Some(po);
        let mut destino = ent_p6();
        let pd = PhysicsData {
            has_rigidbody: true,
            has_collider: true,
            is_trigger: false,
            mass: 5.0,
            ..Default::default()
        };
        destino.physics = Some(pd);
        let json = extraer_componente(&origen, TipoComponente::Collider).unwrap();
        assert!(aplicar_datos(&mut destino, TipoComponente::Collider, &json));
        let p = destino.physics.as_ref().unwrap();
        assert_eq!(p.mass, 5.0);
        assert!(p.has_rigidbody);
        assert!(p.is_trigger);
    }

    #[test]
    fn p6_instalar_camera_crea_y_registra_y_segunda_vez_falla() {
        let mut e = ent_p6();
        assert!(e.camera.is_none());
        let json = serde_json::to_string(&CameraData::default()).unwrap();
        assert!(instalar_componente(&mut e, TipoComponente::Camera, &json));
        assert!(e.camera.is_some());
        assert_eq!(e.components.iter().filter(|c| *c == "Camera").count(), 1);
        assert!(!instalar_componente(&mut e, TipoComponente::Camera, &json));
        assert_eq!(e.components.iter().filter(|c| *c == "Camera").count(), 1);
    }

    // ---- P6: candado de escala proporcional ----

    fn assert_escala(actual: [f32; 3], esperada: [f32; 3]) {
        for i in 0..3 {
            assert!(
                (actual[i] - esperada[i]).abs() < 1.0e-6,
                "eje {i}: esperado {}, obtenido {}",
                esperada[i],
                actual[i]
            );
        }
    }

    #[test]
    fn p6_escala_uniforme() {
        assert_escala(escala_proporcional([2.0, 2.0, 2.0], 0, 3.0), [3.0, 3.0, 3.0]);
    }

    #[test]
    fn p6_escala_no_uniforme() {
        assert_escala(escala_proporcional([1.0, 2.0, 4.0], 1, 1.0), [0.5, 1.0, 2.0]);
    }

    #[test]
    fn p6_escala_editar_a_cero_sin_nan() {
        let r = escala_proporcional([1.0, 2.0, 4.0], 1, 0.0);
        assert_escala(r, [0.0, 0.0, 0.0]);
        assert!(r.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn p6_escala_eje_en_cero_aplica_delta() {
        assert_escala(escala_proporcional([0.0, 2.0, 2.0], 0, 1.0), [1.0, 3.0, 3.0]);
        // Delta 0 (editar 0 a 0): los demás ejes no se tocan.
        assert_escala(escala_proporcional([0.0, 0.5, 2.0], 0, 0.0), [0.0, 0.5, 2.0]);
    }

    #[test]
    fn p6_escala_nuevo_negativo_anula_la_proporcion() {
        assert_escala(escala_proporcional([1.0, 2.0, 4.0], 1, -1.0), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn p6_escala_eje_fuera_de_rango_no_toca() {
        assert_eq!(escala_proporcional([1.0, 2.0, 4.0], 3, 8.0), [1.0, 2.0, 4.0]);
    }
}
