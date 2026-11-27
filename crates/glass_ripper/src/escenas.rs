//! Conversión de escenas y prefabs de Unity (UnityYAML) a entidades de
//! glass_core. Escenas y prefabs comparten el mismo núcleo: GameObjects
//! planos → entidades, jerarquía por Transform y expansión de
//! instancias de prefab (PrefabInstance) sin vínculo vivo.

use std::collections::{HashMap, HashSet};

use glass_core::scene::{
    AnimatorData, CameraData, LightData, MeshData, ScriptComponentData, TransformData,
};
use glass_core::{EntityData, Prefab};
use serde_yaml::Value;

use crate::documentos::{
    booleano, campo, color, entero_de, flotante, partir_documentos, quat, referencia, texto_de,
    vec3, DocumentoUnity,
};
use crate::RipperError;
use crate::escena_fisica::fisica_de_entidad;
use crate::informe::InformeImportacion;
use crate::EstadoInforme;
use crate::instancias::{
    expandir_instancia, AnadidosInstancia, ComponenteAnadido, GameObjectAnadido,
    ResolutoresInstancia,
};

/// Resolutores que el importador proporciona a la conversión de escenas.
pub struct ContextoEscena<'a> {
    /// GUID → ruta importada del asset, relativa a `Assets/` (p. ej.
    /// "Models/X.glb", "Materiales/M.material.json",
    /// "Prefabs/P.prefab.json"); la forma relativa al proyecto
    /// ("Assets/...") solo la construye `fuente_proyecto` para
    /// `prefab_source`.
    pub rutas: &'a dyn Fn(&str) -> Option<String>,
    /// GUID de un .cs → class_name a guardar (el del script ya
    /// convertido por U4); None = no convertido (cuarentena/editor).
    pub clase_de_script: &'a dyn Fn(&str) -> Option<String>,
    /// GUID de un material físico 134 → ruta del .physmat.json
    /// importado, relativa a Assets (p. ej. "Materiales/H.physmat.json").
    pub physmat_de_guid: &'a dyn Fn(&str) -> Option<String>,
    /// (GUID del modelo, fileID de la submalla) → nombre del sub-objeto
    /// en la tabla del `.meta` (el nodo del FBX/GLB al que se enlaza la
    /// entidad). El nombre de la entidad en Unity no siempre coincide
    /// con el nodo (p. ej. «Prop_InfoLaptop» ↔ «DemoFacility_Prop_Laptop»).
    pub objeto_de_submalla: &'a dyn Fn(&str, i64) -> Option<String>,
}

/// Prefab convertido, listo para expandirse en escenas u otros prefabs.
pub struct PrefabConvertido {
    pub prefab: Prefab,
    /// fileID de Unity (de GameObject o de cualquiera de sus
    /// componentes) → id de la entidad convertida.
    pub entidad_por_fileid: HashMap<i64, u64>,
    /// fileID del documento de componente → nombre en `components`.
    pub componente_por_fileid: HashMap<i64, String>,
    pub guid_fuente: String,
}

/// Componentes que se agrupan por `m_GameObject`.
const CLASES_COMPONENTE: [i32; 15] = [
    4, 20, 23, 33, 54, 64, 65, 95, 108, 114, 135, 136, 137, 143, 224,
];

/// Claves estándar de un MonoBehaviour (no son campos del script).
const CLAVES_ESTANDAR: [&str; 10] = [
    "m_ObjectHideFlags",
    "m_CorrespondingSourceObject",
    "m_PrefabInstance",
    "m_PrefabAsset",
    "m_GameObject",
    "m_Enabled",
    "m_EditorHideFlags",
    "m_Script",
    "m_Name",
    "m_EditorClassIdentifier",
];

/// Avisos de un archivo: las cadenas idénticas se cuentan y se vuelcan
/// una sola vez, con «(×N)» si aparecieron más de una vez.
#[derive(Default)]
struct Avisos {
    orden: Vec<String>,
    cuenta: HashMap<String, usize>,
}

impl Avisos {
    fn sumar(&mut self, texto: String) {
        if let Some(n) = self.cuenta.get_mut(&texto) {
            *n += 1;
            return;
        }
        self.cuenta.insert(texto.clone(), 1);
        self.orden.push(texto);
    }

    fn lineas(&self) -> Vec<String> {
        self.orden
            .iter()
            .map(|t| match self.cuenta.get(t) {
                Some(&n) if n > 1 => format!("{t} (×{n})"),
                _ => t.clone(),
            })
            .collect()
    }
}

/// Transform crudo de una entidad, para montar la jerarquía después.
struct Jerarquia {
    entidad: u64,
    transform: i64,
    padre: i64,
    hijos: Vec<i64>,
}

/// Resultado del núcleo compartido.
struct Nucleo {
    entidades: Vec<EntityData>,
    /// Número de entidades planas (las primeras de `entidades`).
    planas: usize,
    /// fileID de los objetos planos → entidad.
    por_fileid: HashMap<i64, u64>,
    /// fileID de cada documento de componente → nombre que escribió en `components`.
    componente_por_fileid: HashMap<i64, String>,
    avisos: Avisos,
    /// Resumen por prefab (guid) de las instancias de la escena, para
    /// el informe de importación; vacío al convertir prefabs.
    resumen: Vec<(String, ResumenPrefab)>,
}

/// Resumen por prefab (guid) de las instancias de un archivo, para el
/// informe de escena.
struct ResumenPrefab {
    nombre: String,
    instancias: usize,
    entidades: usize,
    aplicadas: usize,
}

fn acumular_resumen(
    resumen: &mut Vec<(String, ResumenPrefab)>,
    guid: &str,
    nombre: &str,
    entidades: usize,
    aplicadas: usize,
) {
    match resumen.iter_mut().find(|(g, _)| g.as_str() == guid) {
        Some((_, r)) => {
            r.instancias += 1;
            r.entidades += entidades;
            r.aplicadas += aplicadas;
        }
        None => resumen.push((
            guid.to_string(),
            ResumenPrefab {
                nombre: nombre.to_string(),
                instancias: 1,
                entidades,
                aplicadas,
            },
        )),
    }
}

// Conversión de ejes Unity → motor (la misma que `crate::ejes`):
// posición z → −z, cuaternión (−x, −y, z, w), escala igual.
fn eje_posicion(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[1], -p[2]]
}

fn eje_rotacion(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], q[2], q[3]]
}

fn transform_unity(v: &Value) -> TransformData {
    let mut t = TransformData::default();
    let q = campo(v, "m_LocalRotation")
        .and_then(quat)
        .unwrap_or([0.0, 0.0, 0.0, 1.0]);
    t.set_rotation_quat(eje_rotacion(q));
    if let Some(p) = campo(v, "m_LocalPosition").and_then(vec3) {
        t.position = eje_posicion(p);
    }
    if let Some(s) = campo(v, "m_LocalScale").and_then(vec3) {
        t.scale = s;
    }
    t
}

fn id_ref(v: &Value, clave: &str) -> i64 {
    campo(v, clave).and_then(referencia).map_or(0, |r| r.file_id)
}

fn guid_ref(v: &Value, clave: &str) -> Option<String> {
    campo(v, clave).and_then(referencia).and_then(|r| r.guid)
}

/// Convierte un valor YAML de Unity a JSON (C0b, campos serializados).
/// Devuelve `None` si el valor es o contiene una referencia a un objeto
/// de Unity (`{fileID: ...}`): es un puntero a otro objeto, no un valor,
/// y v1 no lo transporta (el llamador avisa por campo).
fn valor_json(v: &Value) -> Option<serde_json::Value> {
    match v {
        Value::Null => Some(serde_json::Value::Null),
        Value::Bool(b) => Some(serde_json::Value::Bool(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(serde_json::json!(i))
            } else if let Some(u) = n.as_u64() {
                Some(serde_json::json!(u))
            } else {
                n.as_f64().map(|f| serde_json::json!(f))
            }
        }
        Value::String(s) => Some(serde_json::Value::String(s.clone())),
        Value::Sequence(seq) => {
            let mut salida = Vec::with_capacity(seq.len());
            for item in seq {
                salida.push(valor_json(item)?);
            }
            Some(serde_json::Value::Array(salida))
        }
        Value::Mapping(m) => {
            if m.keys().any(|k| k.as_str() == Some("fileID")) {
                return None;
            }
            let mut salida = serde_json::Map::new();
            for (k, val) in m {
                let k = k.as_str()?;
                salida.insert(k.to_string(), valor_json(val)?);
            }
            Some(serde_json::Value::Object(salida))
        }
        Value::Tagged(t) => valor_json(&t.value),
    }
}

/// fileIDs de una lista de `m_Modification` (`m_AddedGameObjects`,
/// `m_AddedComponents`): referencias `{fileID}` o, en versiones
/// recientes, entradas con `addedObject: {fileID}`. Sin ceros.
fn refs_modificacion(m: &Value, clave: &str) -> Vec<i64> {
    campo(m, clave)
        .and_then(Value::as_sequence)
        .map(|s| {
            s.iter()
                .filter_map(|x| {
                    referencia(x)
                        .filter(|r| r.file_id != 0)
                        .or_else(|| campo(x, "addedObject").and_then(referencia))
                })
                .map(|r| r.file_id)
                .filter(|&f| f != 0)
                .collect()
        })
        .unwrap_or_default()
}

/// Nombre que la conversión escribe en `components` para un documento
/// de componente (None: Transform/RectTransform o clase sin conversión).
fn nombre_componente(clase: i32) -> Option<&'static str> {
    Some(match clase {
        20 => "Camera",
        108 => "Light",
        95 => "Animator",
        23 | 137 => "MeshRenderer",
        54 => "RigidBody",
        64 | 65 | 135 | 136 => "Collider",
        143 => "CharacterController",
        114 => "Script",
        _ => return None,
    })
}

fn sin_directorios(ruta: &str) -> &str {
    ruta.rsplit(['/', '\\']).next().unwrap_or(ruta)
}

fn quitar_extension(s: &str) -> &str {
    match s.rfind('.') {
        Some(i) if i > 0 => &s[..i],
        _ => s,
    }
}

/// "Assets/Models/Casa.glb" → "Casa".
fn nombre_modelo(ruta: &str) -> String {
    let base = sin_directorios(ruta);
    base.strip_suffix(".glb")
        .unwrap_or_else(|| quitar_extension(base))
        .to_string()
}

/// "Assets/Materiales/Madera.material.json" → "Madera".
fn nombre_material(ruta: &str) -> String {
    let base = sin_directorios(ruta);
    let base = base.strip_suffix(".json").unwrap_or(base);
    base.strip_suffix(".material").unwrap_or(base).to_string()
}

/// Ruta del plan → `prefab_source` (relativa al PROYECTO, ver
/// `ruta_fuente_prefab` en glass_core): el plan da rutas relativas a
/// `Assets/` (p. ej. "Prefabs/Casa.prefab.json"), así que se antepone
/// el componente si falta; si ya lo trae, se deja tal cual. Sin el
/// componente, el resolver del editor no encuentra el asset y el
/// vínculo nace roto (cazado en el test físico de FiringRange).
fn fuente_proyecto(ruta: &str) -> String {
    let primero = ruta.split('/').next().unwrap_or("");
    if primero.eq_ignore_ascii_case("assets") {
        ruta.to_string()
    } else {
        format!("Assets/{ruta}")
    }
}

/// "Assets/Anim/Jugador.controller.json" → "Jugador".
fn nombre_controlador(ruta: &str) -> String {
    let base = sin_directorios(ruta);
    let base = base.strip_suffix(".json").unwrap_or(base);
    quitar_extension(base).to_string()
}

fn anadir_componente(e: &mut EntityData, nombre: &str) {
    if !e.components.iter().any(|c| c == nombre) {
        e.components.push(nombre.to_string());
    }
}

fn entidad_vacia(id: u64) -> EntityData {
    EntityData {
        id,
        active: true,
        name: String::new(),
        tag: "Untagged".to_string(),
        layer: "Default".to_string(),
        is_static: false,
        components: Vec::new(),
        transform: TransformData::default(),
        mesh: None,
        script: None,
        script_data: None,
        physics: None,
        character: None,
        audio: None,
        particles: None,
        camera: None,
        light: None,
        light_profile: None,
        fog: None,
        ambient: None,
        post: None,
        shadow: None,
        canvas: None,
        rect_transform: None,
        ui_text: None,
        ui_button: None,
        ui_image: None,
        layout_group: None,
        ui_drag: None,
        animator: None,
        portal: None,
        parent: None,
        children: Vec::new(),
        prefab_source: None,
        prefab_hash: None,
        prefab_local_id: None,
        prefab_overrides: None,
    }
}

/// Enlaza `hijo` bajo `padre` (manda el último enlace; sin duplicados).
fn enlazar(entidades: &mut [EntityData], pos: &HashMap<u64, usize>, hijo: u64, padre: u64) {
    if hijo == padre {
        return;
    }
    let (Some(&ih), Some(&ip)) = (pos.get(&hijo), pos.get(&padre)) else {
        return;
    };
    if let Some(anterior) = entidades[ih].parent {
        if anterior == padre {
            return;
        }
        if let Some(&ia) = pos.get(&anterior) {
            entidades[ia].children.retain(|&c| c != hijo);
        }
    }
    entidades[ih].parent = Some(padre);
    if !entidades[ip].children.contains(&hijo) {
        entidades[ip].children.push(hijo);
    }
}

/// Estado mutable de la conversión de las entidades planas.
struct Estado<'c, 'a> {
    ctx: &'c ContextoEscena<'a>,
    avisos: Avisos,
    componente_por_fileid: HashMap<i64, String>,
    entidad_de_transform: HashMap<i64, u64>,
    jerarquia: Vec<Jerarquia>,
    submallas: HashMap<String, HashSet<i64>>,
    orden_submallas: Vec<String>,
}

impl<'c, 'a> Estado<'c, 'a> {
    fn new(ctx: &'c ContextoEscena<'a>) -> Self {
        Estado {
            ctx,
            avisos: Avisos::default(),
            componente_por_fileid: HashMap::new(),
            entidad_de_transform: HashMap::new(),
            jerarquia: Vec::new(),
            submallas: HashMap::new(),
            orden_submallas: Vec::new(),
        }
    }

    fn nombre_capa(&mut self, n: i64) -> String {
        match n {
            0 => "Default".to_string(),
            1 => "TransparentFX".to_string(),
            2 => "Ignore Raycast".to_string(),
            4 => "Water".to_string(),
            5 => "UI".to_string(),
            otro => {
                self.avisos
                    .sumar(format!("capa de Unity sin nombre en el motor: Layer {otro}"));
                format!("Layer {otro}")
            }
        }
    }

    /// Convierte un GameObject plano y sus componentes en una entidad.
    fn entidad(&mut self, id: u64, go: &Value, comps: &[&DocumentoUnity]) -> EntityData {
        let mut e = entidad_vacia(id);
        e.name = texto_de(go, "m_Name").unwrap_or_default();
        e.active = campo(go, "m_IsActive").is_none() || booleano(go, "m_IsActive");
        e.tag = texto_de(go, "m_TagString")
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Untagged".to_string());
        let capa = entero_de(go, "m_Layer").unwrap_or(0);
        e.layer = self.nombre_capa(capa);
        if entero_de(go, "m_StaticEditorFlags").unwrap_or(0) != 0 {
            e.is_static = true;
            anadir_componente(&mut e, "StaticCollider");
        }

        let mut con_transform = false;
        let mut filtro: Option<&Value> = None;
        let mut renderer: Option<&DocumentoUnity> = None;
        let mut fisica: Vec<(i32, &Value)> = Vec::new();
        let mut fisica_ids: Vec<(i32, i64)> = Vec::new();
        for &c in comps {
            let v = &c.valor;
            match c.clase {
                4 | 224 => {
                    self.entidad_de_transform.insert(c.file_id, id);
                    if con_transform {
                        self.avisos
                            .sumar("GameObject con varios Transform: se usa el primero".to_string());
                    } else {
                        con_transform = true;
                        e.transform = transform_unity(v);
                        if c.clase == 224 {
                            self.aplicar_componente_doc(&mut e, c);
                        }
                        let hijos = campo(v, "m_Children")
                            .and_then(Value::as_sequence)
                            .map(|s| {
                                s.iter()
                                    .filter_map(referencia)
                                    .map(|r| r.file_id)
                                    .filter(|&f| f != 0)
                                    .collect()
                            })
                            .unwrap_or_default();
                        self.jerarquia.push(Jerarquia {
                            entidad: id,
                            transform: c.file_id,
                            padre: id_ref(v, "m_Father"),
                            hijos,
                        });
                    }
                }
                33 => filtro = campo(v, "m_Mesh"),
                23 | 137 => {
                    if renderer.is_some() {
                        self.avisos
                            .sumar("GameObject con varios renderers: se usa el primero".to_string());
                    } else {
                        renderer = Some(c);
                    }
                }
                20 | 95 | 108 => {
                    self.aplicar_componente_doc(&mut e, c);
                    if let Some(nombre) = nombre_componente(c.clase) {
                        anadir_componente(&mut e, nombre);
                        self.componente_por_fileid.insert(c.file_id, nombre.to_string());
                    }
                }
                114 => {
                    let tenia_script = e.script.is_some();
                    self.aplicar_componente_doc(&mut e, c);
                    if !tenia_script && e.script.is_some() {
                        anadir_componente(&mut e, "Script");
                        self.componente_por_fileid.insert(c.file_id, "Script".to_string());
                    }
                }
                54 | 64 | 65 | 135 | 136 | 143 => {
                    fisica.push((c.clase, v));
                    fisica_ids.push((c.clase, c.file_id));
                }
                _ => {}
            }
        }

        if !con_transform {
            self.avisos.sumar(format!(
                "GameObject sin Transform: {} usa el transform por defecto",
                e.name
            ));
        }
        if let Some(r) = renderer {
            e.mesh = Some(self.malla(r, filtro));
            anadir_componente(&mut e, "MeshRenderer");
            self.componente_por_fileid.insert(r.file_id, "MeshRenderer".to_string());
        }
        if !fisica.is_empty() {
            let ctx = self.ctx;
            let f = fisica_de_entidad(&fisica, &|g: Option<&str>| {
                g.and_then(|g| (ctx.physmat_de_guid)(g))
            });
            for a in f.avisos {
                self.avisos.sumar(a);
            }
            if fisica.iter().any(|(c, _)| *c == 54) {
                anadir_componente(&mut e, "RigidBody");
                for &(_, fid) in fisica_ids.iter().filter(|(c, _)| *c == 54) {
                    self.componente_por_fileid.insert(fid, "RigidBody".to_string());
                }
            }
            if let Some(mut p) = f.physics {
                p.layer = capa.clamp(0, 31) as u8;
                if p.has_collider {
                    anadir_componente(&mut e, "Collider");
                    for &(_, fid) in fisica_ids
                        .iter()
                        .filter(|(c, _)| matches!(*c, 64 | 65 | 135 | 136))
                    {
                        self.componente_por_fileid.insert(fid, "Collider".to_string());
                    }
                }
                e.physics = Some(p);
            }
            if f.character.is_some() {
                anadir_componente(&mut e, "CharacterController");
                for &(_, fid) in fisica_ids.iter().filter(|(c, _)| *c == 143) {
                    self.componente_por_fileid.insert(fid, "CharacterController".to_string());
                }
            }
            e.character = f.character;
        }
        e
    }

    /// Aplica los datos de un documento de componente sobre `e` (directos:
    /// Camera, Light, Animator, MonoBehaviour, RectTransform; MeshRenderer
    /// sin MeshFilter; física de un solo documento). No toca `components`:
    /// el nombre lo pone quien llama (`entidad` o la expansión).
    fn aplicar_componente_doc(&mut self, e: &mut EntityData, doc: &DocumentoUnity) {
        let v = &doc.valor;
        match doc.clase {
            20 => {
                let tag = e.tag.clone();
                e.camera = Some(self.camara(v, &tag));
            }
            108 => e.light = Some(self.luz(v)),
            95 => e.animator = Some(self.animator(v)),
            114 => {
                // `monobehaviour` añade "Script"; se deshace si no estaba.
                let habia = e.components.iter().any(|c| c == "Script");
                self.monobehaviour(v, e);
                if !habia {
                    e.components.retain(|c| c != "Script");
                }
            }
            224 => {
                let r = e.rect_transform.get_or_insert_with(Default::default);
                r.anchored_position = vec2(v, "m_AnchoredPosition").unwrap_or(r.anchored_position);
                r.size_delta = vec2(v, "m_SizeDelta").unwrap_or(r.size_delta);
                r.anchor_min = vec2(v, "m_AnchorMin").unwrap_or(r.anchor_min);
                r.anchor_max = vec2(v, "m_AnchorMax").unwrap_or(r.anchor_max);
                r.pivot = vec2(v, "m_Pivot").unwrap_or(r.pivot);
            }
            23 | 137 => e.mesh = Some(self.malla(doc, None)),
            54 | 64 | 65 | 135 | 136 | 143 => {
                let ctx = self.ctx;
                let f = fisica_de_entidad(&[(doc.clase, v)], &|g: Option<&str>| {
                    g.and_then(|g| (ctx.physmat_de_guid)(g))
                });
                for a in f.avisos {
                    self.avisos.sumar(a);
                }
                if let Some(mut p) = f.physics {
                    if let Some(previa) = &e.physics {
                        p.layer = previa.layer;
                    }
                    e.physics = Some(p);
                }
                if f.character.is_some() {
                    e.character = f.character;
                }
            }
            _ => {}
        }
    }

    fn malla(&mut self, r: &DocumentoUnity, filtro: Option<&Value>) -> MeshData {
        let rv = &r.valor;
        let ref_malla = if r.clase == 137 {
            self.avisos.sumar(
                "SkinnedMeshRenderer: se importa como malla estática; los huesos van en la fase de animación"
                    .to_string(),
            );
            campo(rv, "m_Mesh")
        } else {
            filtro
        };
        let model = self.modelo_de(ref_malla);
        let object = self.objeto_de(ref_malla);
        let material = self.material_de(campo(rv, "m_Materials"));
        if campo(rv, "m_Enabled").is_some() && !booleano(rv, "m_Enabled") {
            self.avisos
                .sumar("MeshRenderer deshabilitado: la entidad conserva la malla".to_string());
        }
        MeshData {
            model,
            object,
            material,
            ..Default::default()
        }
    }

    /// Nombre del sub-objeto (nodo del GLB) al que apunta la referencia
    /// de malla, desde la tabla de sub-objetos del `.meta` del modelo.
    /// Vacío si la referencia no trae GUID o la tabla no lo cubre: el
    /// motor cae entonces al enlace por nombre de entidad.
    fn objeto_de(&mut self, ref_malla: Option<&Value>) -> String {
        let Some((guid, fid)) = ref_malla.and_then(referencia).and_then(|r| {
            let fid = r.file_id;
            r.guid.map(|g| (g, fid))
        }) else {
            return String::new();
        };
        (self.ctx.objeto_de_submalla)(&guid, fid).unwrap_or_default()
    }

    fn modelo_de(&mut self, ref_malla: Option<&Value>) -> String {
        let con_guid = ref_malla.and_then(referencia).and_then(|r| {
            let fid = r.file_id;
            r.guid.map(|g| (g, fid))
        });
        let Some((guid, fid)) = con_guid else {
            self.avisos
                .sumar("malla no resuelta (sin guid): model vacío".to_string());
            return String::new();
        };
        if !self.submallas.contains_key(&guid) {
            self.orden_submallas.push(guid.clone());
        }
        self.submallas.entry(guid.clone()).or_default().insert(fid);
        match (self.ctx.rutas)(&guid) {
            Some(ruta) => nombre_modelo(&ruta),
            None => {
                self.avisos
                    .sumar(format!("malla no resuelta (guid {guid}): model vacío"));
                String::new()
            }
        }
    }

    fn material_de(&mut self, lista: Option<&Value>) -> String {
        let Some(seq) = lista.and_then(Value::as_sequence) else {
            return String::new();
        };
        if seq.len() > 1 {
            self.avisos.sumar(
                "materiales extra del renderer no aplicados (solo el primero)".to_string(),
            );
        }
        let Some(r) = seq.first().and_then(referencia) else {
            return String::new();
        };
        match r.guid {
            Some(g) => match (self.ctx.rutas)(&g) {
                Some(ruta) => nombre_material(&ruta),
                None => {
                    self.avisos
                        .sumar(format!("material no resuelto (guid {g}): sin material"));
                    String::new()
                }
            },
            None => {
                if r.file_id != 0 {
                    self.avisos
                        .sumar("material local del archivo no resuelto: sin material".to_string());
                }
                String::new()
            }
        }
    }

    fn camara(&mut self, v: &Value, tag: &str) -> CameraData {
        let d = CameraData::default();
        let fov = flotante(v, "field of view")
            .or_else(|| flotante(v, "m_FieldOfView"))
            .unwrap_or(d.fov);
        let near_clip = flotante(v, "near clip plane")
            .or_else(|| flotante(v, "m_NearClipPlane"))
            .unwrap_or(d.near_clip);
        let far_clip = flotante(v, "far clip plane")
            .or_else(|| flotante(v, "m_FarClipPlane"))
            .unwrap_or(d.far_clip);
        let fov = if booleano(v, "orthographic") {
            self.avisos.sumar(
                "cámara ortográfica importada como perspectiva (fov por defecto)".to_string(),
            );
            60.0
        } else {
            fov
        };
        CameraData {
            fov,
            near_clip,
            far_clip,
            is_main: tag == "MainCamera",
        }
    }

    fn luz(&mut self, v: &Value) -> LightData {
        let base = LightData::default();
        let light_type = match entero_de(v, "m_Type") {
            Some(0) => "Spot",
            Some(1) => "Directional",
            Some(2) => "Point",
            Some(3) => "Area",
            otro => {
                self.avisos.sumar(format!(
                    "tipo de luz desconocido ({}): se importa como Point",
                    otro.map_or_else(|| "ausente".to_string(), |t| t.to_string())
                ));
                "Point"
            }
        }
        .to_string();
        let color_rgb = campo(v, "m_Color")
            .and_then(color)
            .map_or(base.color, |c| [c[0], c[1], c[2]]);
        let (spot_outer_deg, spot_inner_deg) = if light_type == "Spot" {
            let outer = flotante(v, "m_SpotAngle").unwrap_or(base.spot_outer_deg);
            (outer, flotante(v, "m_InnerSpotAngle").unwrap_or(outer * 0.8))
        } else {
            (base.spot_outer_deg, base.spot_inner_deg)
        };
        let cast_shadow = campo(v, "m_Shadows")
            .and_then(|s| entero_de(s, "m_Type"))
            .unwrap_or(0)
            != 0;
        LightData {
            light_type,
            color: color_rgb,
            intensity: flotante(v, "m_Intensity").unwrap_or(base.intensity),
            range: flotante(v, "m_Range").unwrap_or(base.range),
            cast_shadow,
            spot_outer_deg,
            spot_inner_deg,
            ..base
        }
    }

    fn animator(&mut self, v: &Value) -> AnimatorData {
        let Some(guid) = guid_ref(v, "m_Controller") else {
            return AnimatorData::default();
        };
        self.avisos.sumar(
            "el controlador de Animator no se convierte en v1; queda la referencia por nombre"
                .to_string(),
        );
        let controller = match (self.ctx.rutas)(&guid) {
            Some(ruta) => nombre_controlador(&ruta),
            None => {
                self.avisos
                    .sumar(format!("controlador de Animator no resuelto (guid {guid})"));
                String::new()
            }
        };
        AnimatorData {
            controller,
            ..Default::default()
        }
    }

    fn monobehaviour(&mut self, v: &Value, e: &mut EntityData) {
        if campo(v, "m_Enabled").is_some() && !booleano(v, "m_Enabled") {
            self.avisos
                .sumar("MonoBehaviour deshabilitado: se importa activo".to_string());
        }
        let Some(guid) = guid_ref(v, "m_Script") else {
            self.avisos
                .sumar("MonoBehaviour sin script referenciado: se omite".to_string());
            return;
        };
        let Some(clase) = (self.ctx.clase_de_script)(&guid) else {
            self.avisos.sumar(format!(
                "script no convertido (guid {guid}): el MonoBehaviour se omite"
            ));
            return;
        };
        if e.script.is_some() {
            self.avisos.sumar(format!(
                "varios scripts en {}: solo se conserva el primero",
                e.name
            ));
            return;
        }
        // C0b: los campos serializados del script ya tienen hueco
        // (ScriptComponentData.campos) y viajan a la escena convertida.
        // Solo quedan fuera las referencias a objetos de Unity
        // ({fileID: ...}), que v1 no transporta: un aviso por campo.
        let mut campos = std::collections::BTreeMap::new();
        if let Some(m) = v.as_mapping() {
            for (k, valor) in m {
                let Some(k) = k.as_str() else { continue };
                if CLAVES_ESTANDAR.contains(&k)
                    || matches!(k, "move_speed" | "jump_velocity" | "auto_demo")
                {
                    continue;
                }
                match valor_json(valor) {
                    Some(j) => {
                        campos.insert(k.to_string(), j);
                    }
                    None => {
                        self.avisos.sumar(format!(
                            "campo serializado con referencia a objeto (fuera de C0b v1): {k}"
                        ));
                    }
                }
            }
        }
        let base = ScriptComponentData::default();
        let corto = clase.rsplit('.').next().unwrap_or(&clase).to_string();
        let datos = ScriptComponentData {
            move_speed: flotante(v, "move_speed").unwrap_or(base.move_speed),
            jump_velocity: flotante(v, "jump_velocity").unwrap_or(base.jump_velocity),
            auto_demo: if campo(v, "auto_demo").is_some() {
                booleano(v, "auto_demo")
            } else {
                base.auto_demo
            },
            class_name: clase,
            campos,
        };
        e.script = Some(corto);
        e.script_data = Some(datos);
        anadir_componente(e, "Script");
    }
}

/// Núcleo compartido por escenas y prefabs.
fn ejecutar_nucleo(
    docs: &[DocumentoUnity],
    ctx: &ContextoEscena,
    prefabs: &HashMap<String, PrefabConvertido>,
    vincular: bool,
    informar_resumen: bool,
) -> Nucleo {
    let mut est = Estado::new(ctx);

    // 1. Índices: componentes por GameObject, stripped por instancia y
    //    clases omitidas (ClassID, nombre de la primera clave, cuenta).
    let mut omitidas: Vec<(i32, String, usize)> = Vec::new();
    let mut componentes: HashMap<i64, Vec<&DocumentoUnity>> = HashMap::new();
    let mut stripped_de: HashMap<i64, Vec<&DocumentoUnity>> = HashMap::new();
    for d in docs {
        if d.stripped {
            stripped_de
                .entry(id_ref(&d.valor, "m_PrefabInstance"))
                .or_default()
                .push(d);
        } else if CLASES_COMPONENTE.contains(&d.clase) {
            componentes
                .entry(id_ref(&d.valor, "m_GameObject"))
                .or_default()
                .push(d);
        } else if d.clase != 1 && d.clase != 1001 {
            match omitidas.iter_mut().find(|o| o.0 == d.clase) {
                Some(o) => o.2 += 1,
                None => omitidas.push((d.clase, d.nombre.clone(), 1)),
            }
        }
    }

    // Pre-escaneo de añadidos de instancias (m_AddedGameObjects y
    // m_AddedComponents) de prefabs convertidos: sus entradas salen de
    // `componentes`; el paso 4 los convierte sobre la expansión.
    let doc_por_fileid: HashMap<i64, &DocumentoUnity> =
        docs.iter().map(|d| (d.file_id, d)).collect();
    let mut added_gos: HashSet<i64> = HashSet::new();
    let mut added_comps: HashSet<i64> = HashSet::new();
    for d in docs
        .iter()
        .filter(|d| d.clase == 1001 && !d.stripped && d.nombre != "Prefab")
    {
        let convertido = guid_ref(&d.valor, "m_SourcePrefab")
            .is_some_and(|g| prefabs.contains_key(&g));
        if !convertido {
            continue;
        }
        let Some(m) = campo(&d.valor, "m_Modification") else {
            continue;
        };
        added_gos.extend(refs_modificacion(m, "m_AddedGameObjects"));
        added_comps.extend(refs_modificacion(m, "m_AddedComponents"));
    }
    let mut added_componentes: HashMap<i64, Vec<&DocumentoUnity>> = HashMap::new();
    for go in &added_gos {
        if let Some(lista) = componentes.remove(go) {
            added_componentes.insert(*go, lista);
        }
    }
    for lista in componentes.values_mut() {
        lista.retain(|c| !added_comps.contains(&c.file_id));
    }

    // 2. Entidades planas, en orden de documento.
    let mut entidades: Vec<EntityData> = Vec::new();
    let mut pos: HashMap<u64, usize> = HashMap::new();
    let mut por_fileid: HashMap<i64, u64> = HashMap::new();
    let mut gos: HashSet<i64> = HashSet::new();
    let mut siguiente_id: u64 = 1;
    for d in docs
        .iter()
        .filter(|d| d.clase == 1 && !d.stripped && !added_gos.contains(&d.file_id))
    {
        let id = siguiente_id;
        siguiente_id += 1;
        let comps = componentes
            .get(&d.file_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let e = est.entidad(id, &d.valor, comps);
        gos.insert(d.file_id);
        por_fileid.insert(d.file_id, id);
        for c in comps {
            por_fileid.insert(c.file_id, id);
        }
        pos.insert(id, entidades.len());
        entidades.push(e);
    }
    let planas = entidades.len();
    for (go, lista) in &componentes {
        if !gos.contains(go) {
            for _ in lista {
                est.avisos.sumar(
                    "componente sobre un objeto de instancia o sin GameObject: no se aplica en v1"
                        .to_string(),
                );
            }
        }
    }

    // 3. Jerarquía entre entidades planas: primero el orden de
    //    m_Children, después los padres declarados (manda el hijo).
    let padre_declarado: HashMap<i64, i64> = est
        .jerarquia
        .iter()
        .map(|j| (j.transform, j.padre))
        .collect();
    for j in &est.jerarquia {
        for h in &j.hijos {
            let (Some(&hid), Some(&decl)) =
                (est.entidad_de_transform.get(h), padre_declarado.get(h))
            else {
                continue;
            };
            if decl == j.transform || decl == 0 {
                enlazar(&mut entidades, &pos, hid, j.entidad);
            }
        }
    }
    let mut pendientes: Vec<(u64, i64)> = Vec::new();
    for j in est.jerarquia.iter().filter(|j| j.padre != 0) {
        match est.entidad_de_transform.get(&j.padre) {
            Some(&p) => enlazar(&mut entidades, &pos, j.entidad, p),
            None => pendientes.push((j.entidad, j.padre)),
        }
    }

    // 4. Instancias de prefab: expandir todas y registrar anclajes;
    //    los padres se resuelven después (pueden apuntar a stripped de
    //    instancias posteriores).
    let nulo = Value::Null;
    let mut anclajes: HashMap<i64, u64> = HashMap::new();
    let mut raices: Vec<(u64, i64)> = Vec::new();
    let resolver_material = |g: &str| (ctx.rutas)(g).map(|r| nombre_material(&r));
    let resolver_modelo = |g: &str| (ctx.rutas)(g).map(|r| nombre_modelo(&r));
    let mut resumen: Vec<(String, ResumenPrefab)> = Vec::new();
    for d in docs.iter().filter(|d| d.clase == 1001 && !d.stripped) {
        if d.nombre == "Prefab" {
            // Instancia legada (anterior a 2018.3): Unity ya serializó la
            // copia entera como objetos planos (paso 2). No se re-expande;
            // solo se escribe el vínculo con el prefab de origen.
            let v = &d.valor;
            let resuelta = guid_ref(v, "m_ParentPrefab")
                .and_then(|g| prefabs.get(&g).map(|f| (g, f)));
            let Some((guid, fuente)) = resuelta.filter(|_| vincular) else {
                est.avisos
                    .sumar("instancia de prefab legado (anterior a 2018.3): omitida".to_string());
                continue;
            };
            let nombre = &fuente.prefab.name;
            let raiz_fid = id_ref(v, "m_RootGameObject");
            let Some(&raiz) = por_fileid.get(&raiz_fid) else {
                est.avisos.sumar(format!(
                    "instancia legada de {nombre}: raíz no resuelta (m_RootGameObject {raiz_fid}), la copia queda plana sin vínculo"
                ));
                continue;
            };
            let Some(ruta) = (ctx.rutas)(&guid) else {
                est.avisos.sumar(format!(
                    "instancia legada de {nombre}: prefab sin ruta importada (guid {guid}), la copia queda plana sin vínculo"
                ));
                continue;
            };
            let huella = fuente.prefab.huella();
            // Copia: la raíz y cada GameObject plano cuyo m_PrefabInternal
            // apunta a este documento «Prefab».
            let copia: Vec<i64> = std::iter::once(raiz_fid)
                .chain(
                    docs.iter()
                        .filter(|g| {
                            g.clase == 1
                                && !g.stripped
                                && g.file_id != raiz_fid
                                && id_ref(&g.valor, "m_PrefabInternal") == d.file_id
                        })
                        .map(|g| g.file_id),
                )
                .collect();
            let mut vinculadas = 0;
            for fid in copia {
                let Some(i) = por_fileid.get(&fid).and_then(|id| pos.get(id)).copied() else {
                    continue;
                };
                let origen = doc_por_fileid
                    .get(&fid)
                    .map_or(0, |g| id_ref(&g.valor, "m_PrefabParentObject"));
                let local = (origen != 0)
                    .then(|| fuente.entidad_por_fileid.get(&origen).copied())
                    .flatten();
                let e = &mut entidades[i];
                if e.id == raiz {
                    e.prefab_source = Some(ruta.clone());
                    e.prefab_hash = Some(huella.clone());
                }
                e.prefab_local_id = local;
                if local.is_none() {
                    est.avisos.sumar(format!(
                        "instancia de {nombre}: {} sin objeto de origen en el prefab (m_PrefabParentObject {origen}), prefab_local_id vacío",
                        e.name
                    ));
                }
                vinculadas += 1;
            }
            acumular_resumen(&mut resumen, &guid, nombre, vinculadas, 0);
            continue;
        }
        let v = &d.valor;
        let Some(guid) = guid_ref(v, "m_SourcePrefab") else {
            est.avisos
                .sumar("PrefabInstance sin m_SourcePrefab: omitida".to_string());
            continue;
        };
        let Some(fuente) = prefabs.get(&guid) else {
            est.avisos
                .sumar(format!("instancia de prefab no convertido (guid {guid}): omitida"));
            continue;
        };
        let resolver_componente = |f: i64| {
            fuente
                .entidad_por_fileid
                .get(&f)
                .zip(fuente.componente_por_fileid.get(&f))
                .map(|(id, nombre)| (*id, nombre.clone()))
        };
        let resolutores = ResolutoresInstancia {
            material: &resolver_material,
            modelo: &resolver_modelo,
            componente_de_fileid: Some(&resolver_componente),
        };
        let fuente_ruta = vincular
            .then(|| (ctx.rutas)(&guid))
            .flatten()
            .map(|r| fuente_proyecto(&r));
        let modificacion = campo(v, "m_Modification").unwrap_or(&nulo);
        // Añadidos de la instancia: GameObjects nuevos (ids antes de los
        // clones) y componentes nuevos sobre clones o sobre añadidos.
        let mut informe_anadidos: Vec<String> = Vec::new();
        let mut anadidos = AnadidosInstancia::default();
        let mut id_de_anadido: HashMap<i64, u64> = HashMap::new();
        let mut objetos: Vec<(EntityData, i64)> = Vec::new();
        for f in refs_modificacion(modificacion, "m_AddedGameObjects") {
            let Some(go) = doc_por_fileid
                .get(&f)
                .filter(|g| g.clase == 1 && !g.stripped)
            else {
                informe_anadidos.push(format!(
                    "GameObject añadido (fileID {f}) no está en el archivo: omitido"
                ));
                continue;
            };
            let id = siguiente_id;
            siguiente_id += 1;
            let comps: Vec<&DocumentoUnity> = added_componentes
                .get(&f)
                .map(|l| {
                    l.iter()
                        .copied()
                        .filter(|c| !added_comps.contains(&c.file_id))
                        .collect()
                })
                .unwrap_or_default();
            let padre_fid = comps
                .iter()
                .find(|c| matches!(c.clase, 4 | 224))
                .map_or(0, |t| id_ref(&t.valor, "m_Father"));
            objetos.push((est.entidad(id, &go.valor, &comps), padre_fid));
            id_de_anadido.insert(f, id);
            for c in &comps {
                id_de_anadido.insert(c.file_id, id);
            }
        }

        // Id de clon PREVISTO: `expandir_instancia` numera los clones
        // desde `*siguiente_id` (ya avanzado por los añadidos) en el orden
        // raíz + `descendant_entities` del asset, así que la entidad del
        // asset en la posición i (raíz = 0) se clona con `base + i`. Si
        // instancias.rs cambia ese orden, la comprobación tras la
        // expansión lo avisa.
        let base = siguiente_id;
        let posicion: HashMap<u64, u64> = std::iter::once(&fuente.prefab.root_entity)
            .chain(&fuente.prefab.descendant_entities)
            .enumerate()
            .map(|(i, e)| (e.id, i as u64))
            .collect();
        // fileID de escena → (id previsto, fileID de origen en el prefab
        // si es un objeto del prefab). Añadido → su id; stripped de esta
        // instancia → clon previsto; 0 o irresoluble → None.
        let destino = |f: i64| -> Option<(u64, Option<i64>)> {
            if f == 0 {
                return None;
            }
            if let Some(&id) = id_de_anadido.get(&f) {
                return Some((id, None));
            }
            let s = doc_por_fileid.get(&f)?;
            if !s.stripped || id_ref(&s.valor, "m_PrefabInstance") != d.file_id {
                return None;
            }
            let origen = id_ref(&s.valor, "m_CorrespondingSourceObject");
            let asset = fuente.entidad_por_fileid.get(&origen)?;
            posicion.get(asset).map(|&i| (base + i, Some(origen)))
        };
        for (entidad, padre_fid) in objetos {
            let padre = destino(padre_fid).map(|(id, _)| id);
            if padre.is_none() && padre_fid != 0 {
                informe_anadidos.push(format!(
                    "padre del GameObject añadido {} no resuelto (Transform {padre_fid}): queda en la raíz",
                    entidad.name
                ));
            }
            anadidos.objetos.push(GameObjectAnadido { entidad, padre });
        }
        let mut datos_anadidos: Vec<(&DocumentoUnity, u64, Option<i64>)> = Vec::new();
        for f in refs_modificacion(modificacion, "m_AddedComponents") {
            let Some(&doc) = doc_por_fileid.get(&f) else {
                informe_anadidos.push(format!(
                    "componente añadido (fileID {f}) no está en el archivo: omitido"
                ));
                continue;
            };
            let Some(nombre) = nombre_componente(doc.clase) else {
                informe_anadidos.push(format!(
                    "componente añadido de ClassID {} (fileID {f}) sin conversión: omitido",
                    doc.clase
                ));
                continue;
            };
            let Some((entidad, origen)) = destino(id_ref(&doc.valor, "m_GameObject")) else {
                informe_anadidos.push(format!(
                    "componente añadido {nombre} (fileID {f}): GameObject destino no resuelto, omitido"
                ));
                continue;
            };
            anadidos.componentes.push(ComponenteAnadido {
                entidad,
                nombre: nombre.to_string(),
            });
            datos_anadidos.push((doc, entidad, origen));
        }

        let mut exp = expandir_instancia(
            fuente,
            modificacion,
            &mut siguiente_id,
            &resolutores,
            fuente_ruta.as_deref(),
            &anadidos,
        );
        exp.informe.extend(informe_anadidos);
        // Datos de cada componente añadido sobre su entidad en la
        // expansión (el nombre ya lo puso `expandir_instancia`).
        for (doc, previsto, origen) in datos_anadidos {
            let real = origen
                .and_then(|o| exp.entidad_por_fileid.get(&o).copied())
                .unwrap_or(previsto);
            if real != previsto {
                exp.informe.push(format!(
                    "componente añadido (fileID {}): clon previsto {previsto}, la expansión dio {real}",
                    doc.file_id
                ));
            }
            match exp.entidades.iter_mut().find(|e| e.id == real) {
                Some(e) => est.aplicar_componente_doc(e, doc),
                None => exp.informe.push(format!(
                    "componente añadido (fileID {}): entidad {real} fuera de la expansión, datos sin aplicar",
                    doc.file_id
                )),
            }
        }
        let anclas = stripped_de
            .get(&d.file_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for s in anclas {
            let origen = id_ref(&s.valor, "m_CorrespondingSourceObject");
            if let Some(&ent) = exp.entidad_por_fileid.get(&origen) {
                anclajes.insert(s.file_id, ent);
            }
        }
        if let Some(raiz) = exp.entidades.first() {
            raices.push((raiz.id, id_ref(modificacion, "m_TransformParent")));
        }
        est.avisos.sumar(format!(
            "instancia de {}: {} entidades, {} modificaciones aplicadas, expandida {} vínculo vivo",
            fuente.prefab.name,
            exp.entidades.len(),
            exp.aplicadas,
            if fuente_ruta.is_some() { "con" } else { "sin" }
        ));
        acumular_resumen(
            &mut resumen,
            &guid,
            &fuente.prefab.name,
            exp.entidades.len(),
            exp.aplicadas,
        );
        for a in exp.avisos {
            est.avisos.sumar(a);
        }
        for l in exp.informe {
            est.avisos.sumar(format!("instancia de {}: {l}", fuente.prefab.name));
        }
        for e in exp.entidades {
            pos.insert(e.id, entidades.len());
            entidades.push(e);
        }
    }
    for (raiz, padre_fid) in raices.into_iter().filter(|r| r.1 != 0) {
        let padre = est
            .entidad_de_transform
            .get(&padre_fid)
            .or_else(|| anclajes.get(&padre_fid))
            .copied();
        match padre {
            Some(p) => enlazar(&mut entidades, &pos, raiz, p),
            None => est.avisos.sumar(format!(
                "padre de instancia no resuelto (fileID {padre_fid}): la raíz queda sin padre"
            )),
        }
    }
    for (hijo, padre_fid) in pendientes {
        match anclajes.get(&padre_fid) {
            Some(&p) => enlazar(&mut entidades, &pos, hijo, p),
            None => est.avisos.sumar(format!(
                "padre no resuelto (Transform {padre_fid}): la entidad queda sin padre"
            )),
        }
    }

    // Informe por prefab, solo en escenas (no en la conversión de prefabs).
    if informar_resumen {
        for (_, r) in &resumen {
            est.avisos.sumar(format!(
                "Prefab {}: {} instancia(s), {} entidades, {} modificaciones aplicadas",
                r.nombre, r.instancias, r.entidades, r.aplicadas
            ));
        }
    }

    // 5. Avisos de cierre por archivo.
    for guid in &est.orden_submallas {
        let n = est.submallas.get(guid).map_or(0, HashSet::len);
        if n > 1 {
            est.avisos.sumar(format!(
                "el FBX {guid} se referencia por sub-mallas ({n} fileID): el motor usa el modelo entero por archivo"
            ));
        }
    }
    for (clase, nombre, n) in &omitidas {
        let nombre = if nombre.is_empty() { "desconocida" } else { nombre };
        est.avisos
            .sumar(format!("ClassID {clase} ({nombre}): {n} documentos omitidos"));
    }

    Nucleo {
        entidades,
        planas,
        por_fileid,
        componente_por_fileid: est.componente_por_fileid,
        avisos: est.avisos,
        resumen,
    }
}

#[cfg(test)]
fn convertir_escena_interno(
    texto: &str,
    ctx: &ContextoEscena,
    prefabs: &HashMap<String, PrefabConvertido>,
) -> (Vec<EntityData>, Vec<String>) {
    let (entidades, avisos, _) = convertir_escena_con_resumen(texto, ctx, prefabs);
    (entidades, avisos)
}

/// Como `convertir_escena_interno`, más el resumen por prefab (guid)
/// de las instancias de la escena.
fn convertir_escena_con_resumen(
    texto: &str,
    ctx: &ContextoEscena,
    prefabs: &HashMap<String, PrefabConvertido>,
) -> (Vec<EntityData>, Vec<String>, Vec<(String, ResumenPrefab)>) {
    let docs = partir_documentos(texto);
    let n = ejecutar_nucleo(&docs, ctx, prefabs, true, true);
    let avisos = n.avisos.lineas();
    (n.entidades, avisos, n.resumen)
}

/// Vector 2D de Unity (`{x, y}`), sin conversión de ejes.
fn vec2(v: &Value, clave: &str) -> Option<[f32; 2]> {
    let c = campo(v, clave)?;
    let eje = |k: &str| campo(c, k).and_then(Value::as_f64).map(|x| x as f32);
    Some([eje("x")?, eje("y")?])
}

/// Cabecera de un prefab legado (anterior a 2018.3): documento
/// `!u!1001` «Prefab» con `m_RootGameObject` y `m_IsPrefabParent`.
fn es_cabecera_legada(d: &DocumentoUnity) -> bool {
    d.clase == 1001 && d.nombre == "Prefab" && !d.stripped
}

fn convertir_prefab_interno(
    guid: &str,
    texto: &str,
    ctx: &ContextoEscena,
    prefabs: &HashMap<String, PrefabConvertido>,
) -> Result<(PrefabConvertido, Vec<String>), String> {
    // La cabecera legada se aparta antes del núcleo: dentro del propio
    // prefab no es una instancia (el aviso de instancia legada queda
    // para las escenas) y solo aporta la raíz señalada.
    let (cabeceras, docs): (Vec<_>, Vec<_>) = partir_documentos(texto)
        .into_iter()
        .partition(es_cabecera_legada);
    let raiz_legada = cabeceras
        .first()
        .map(|c| id_ref(&c.valor, "m_RootGameObject"))
        .filter(|&f| f != 0);
    let mut n = ejecutar_nucleo(&docs, ctx, prefabs, true, false);
// Fase E: si el archivo es una variante de Unity, se guarda COMO
    // variante (base + diff), no aplanada.
    if let Some((guid_base, fuente_base)) = detectar_variante(&docs, ctx, prefabs) {
        if let Some(base) = prefabs.get(&guid_base) {
            if let Some((prefab, diff)) = derivar_variante(&n, base, &fuente_base) {
                let mut avisos = n.avisos.lineas();
                avisos.push(format!(
                    "variante de {fuente_base}: {} propiedades, {} añadidas, {} quitadas",
                    diff.propiedades.len(),
                    diff.anadidas.len(),
                    diff.quitadas.len()
                ));
                let convertido = PrefabConvertido {
                    prefab,
                    entidad_por_fileid: n.por_fileid,
                    componente_por_fileid: n.componente_por_fileid,
                    guid_fuente: guid.to_string(),
                };
                return Ok((convertido, avisos));
            }
        }
    }
    let raices: Vec<usize> = n.entidades[..n.planas]
        .iter()
        .enumerate()
        .filter(|(_, e)| e.parent.is_none())
        .map(|(i, _)| i)
        .collect();
    let senalada = raiz_legada
        .and_then(|f| n.por_fileid.get(&f))
        .and_then(|&id| raices.iter().copied().find(|&i| n.entidades[i].id == id));
    let Some(iraiz) = senalada.or_else(|| raices.first().copied()) else {
        return Err("prefab sin GameObject raíz".to_string());
    };
    if raices.len() > 1 {
        let criterio = if senalada.is_some() {
            "la señalada por m_RootGameObject"
        } else {
            "la primera"
        };
        n.avisos.sumar(format!(
            "el prefab tiene {} raíces: se usa {criterio}",
            raices.len()
        ));
    }
    let raiz = n.entidades.remove(iraiz);
    let prefab = Prefab {
        format_version: 1,
        name: raiz.name.clone(),
        root_entity: raiz,
        descendant_entities: n.entidades,
        variant_of: None,
        variant_overrides: None,
    };
    Ok((
        PrefabConvertido {
            prefab,
            entidad_por_fileid: n.por_fileid,
            componente_por_fileid: n.componente_por_fileid,
            guid_fuente: guid.to_string(),
        },
        n.avisos.lineas(),
    ))
}

// Construcción del error de conversión de RipperError (variante con
// ruta y motivo).
fn error_conversion(ruta: &str, motivo: &str) -> RipperError {
    RipperError::Conversion {
        ruta: ruta.to_string(),
        motivo: motivo.to_string(),
    }
}

fn volcar_informe(
    informe: &mut InformeImportacion,
    guid: &str,
    ruta: &str,
    detalle: &str,
    avisos: &[String],
) {
    let estado = if avisos.is_empty() {
        EstadoInforme::Convertido
    } else {
        EstadoInforme::ConvertidoConAvisos
    };
    informe.anadir(guid, Some(ruta), estado, detalle);
    for a in avisos {
        informe.anadir(guid, Some(ruta), EstadoInforme::ConvertidoConAvisos, a.as_str());
    }
}

/// Convierte un `.prefab` de Unity en un `glass_core::Prefab`.
/// Los prefabs anidados deben estar ya en `prefabs` (por guid).
pub fn convertir_prefab(
    guid: &str,
    ruta: &str,
    texto: &str,
    ctx: &ContextoEscena,
    prefabs: &HashMap<String, PrefabConvertido>,
    informe: &mut InformeImportacion,
) -> Result<PrefabConvertido, RipperError> {
    match convertir_prefab_interno(guid, texto, ctx, prefabs) {
        Ok((convertido, avisos)) => {
            let detalle = format!(
                "prefab: {} entidades, raíz {}",
                1 + convertido.prefab.descendant_entities.len(),
                convertido.prefab.name
            );
            volcar_informe(informe, guid, ruta, &detalle, &avisos);
            Ok(convertido)
        }
        Err(motivo) => {
            informe.anadir(guid, Some(ruta), EstadoInforme::NoConvertido, motivo.as_str());
            Err(error_conversion(ruta, &motivo))
        }
    }
}

/// Convierte un `.unity` en la lista de entidades de la escena del motor
/// (raíces e instancias expandidas en la misma lista).
pub fn convertir_escena(
    guid: &str,
    ruta: &str,
    texto: &str,
    ctx: &ContextoEscena,
    prefabs: &HashMap<String, PrefabConvertido>,
    informe: &mut InformeImportacion,
) -> Result<Vec<EntityData>, RipperError> {
    let (entidades, avisos, resumen) = convertir_escena_con_resumen(texto, ctx, prefabs);
    let detalle = format!("escena: {} entidades", entidades.len());
    volcar_informe(informe, guid, ruta, &detalle, &avisos);
    for (guid_prefab, r) in &resumen {
        informe.acumular_prefab(
            guid,
            guid_prefab,
            &r.nombre,
            r.instancias,
            r.entidades,
            r.aplicadas,
        );
    }
    Ok(entidades)
}

#[cfg(test)]
mod tests_prefab_legado {
    use super::*;

    const GUID_MODELO: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90";
    const GUID_MATERIAL: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";

    /// Prefab legado mínimo: la cabecera señala «RaizLegada», pero antes
    /// aparece «Suelto», otra entidad sin padre. «Hijo» lleva malla y
    /// material.
    const PREFAB_LEGADO: &str = r#"%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!1001 &100100000
Prefab:
  m_ObjectHideFlags: 1
  serializedVersion: 2
  m_Modification:
    m_TransformParent: {fileID: 0}
    m_Modifications: []
    m_RemovedComponents: []
  m_ParentPrefab: {fileID: 0}
  m_RootGameObject: {fileID: 1000011}
  m_IsPrefabParent: 1
--- !u!1 &1000010
GameObject:
  m_ObjectHideFlags: 0
  m_PrefabParentObject: {fileID: 0}
  m_PrefabInternal: {fileID: 100100000}
  serializedVersion: 5
  m_Component:
  - component: {fileID: 4000010}
  m_Layer: 0
  m_Name: Suelto
  m_TagString: Untagged
  m_IsActive: 1
--- !u!4 &4000010
Transform:
  m_ObjectHideFlags: 1
  m_PrefabParentObject: {fileID: 0}
  m_PrefabInternal: {fileID: 100100000}
  m_GameObject: {fileID: 1000010}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 0}
  m_RootOrder: 0
--- !u!1 &1000011
GameObject:
  m_ObjectHideFlags: 0
  m_PrefabParentObject: {fileID: 0}
  m_PrefabInternal: {fileID: 100100000}
  serializedVersion: 5
  m_Component:
  - component: {fileID: 4000011}
  m_Layer: 0
  m_Name: RaizLegada
  m_TagString: Untagged
  m_IsActive: 1
--- !u!4 &4000011
Transform:
  m_ObjectHideFlags: 1
  m_PrefabParentObject: {fileID: 0}
  m_PrefabInternal: {fileID: 100100000}
  m_GameObject: {fileID: 1000011}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children:
  - {fileID: 4000012}
  m_Father: {fileID: 0}
  m_RootOrder: 1
--- !u!1 &1000012
GameObject:
  m_ObjectHideFlags: 0
  m_PrefabParentObject: {fileID: 0}
  m_PrefabInternal: {fileID: 100100000}
  serializedVersion: 5
  m_Component:
  - component: {fileID: 4000012}
  - component: {fileID: 33000012}
  - component: {fileID: 23000012}
  m_Layer: 0
  m_Name: Hijo
  m_TagString: Untagged
  m_IsActive: 1
--- !u!4 &4000012
Transform:
  m_ObjectHideFlags: 1
  m_PrefabParentObject: {fileID: 0}
  m_PrefabInternal: {fileID: 100100000}
  m_GameObject: {fileID: 1000012}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 4000011}
  m_RootOrder: 0
--- !u!33 &33000012
MeshFilter:
  m_ObjectHideFlags: 1
  m_PrefabParentObject: {fileID: 0}
  m_PrefabInternal: {fileID: 100100000}
  m_GameObject: {fileID: 1000012}
  m_Mesh: {fileID: 4300000, guid: a1b2c3d4e5f60718293a4b5c6d7e8f90, type: 3}
--- !u!23 &23000012
MeshRenderer:
  m_ObjectHideFlags: 1
  m_PrefabParentObject: {fileID: 0}
  m_PrefabInternal: {fileID: 100100000}
  m_GameObject: {fileID: 1000012}
  m_Enabled: 1
  m_CastShadows: 1
  m_ReceiveShadows: 1
  m_Materials:
  - {fileID: 2100000, guid: 0f1e2d3c4b5a69788796a5b4c3d2e1f0, type: 2}
"#;

    fn convertir(texto: &str) -> Result<(PrefabConvertido, Vec<String>), String> {
        let rutas = |g: &str| match g {
            GUID_MODELO => Some("Assets/Models/ModeloPlano.glb".to_string()),
            GUID_MATERIAL => Some("Assets/Materiales/MatOscuro.material.json".to_string()),
            _ => None,
        };
        let sin_script = |_: &str| -> Option<String> { None };
        let sin_physmat = |_: &str| -> Option<String> { None };
        let submalla = |_: &str, _: i64| Some("NodoPlano".to_string());
        let ctx = ContextoEscena {
            rutas: &rutas,
            clase_de_script: &sin_script,
            physmat_de_guid: &sin_physmat,
            objeto_de_submalla: &submalla,
        };
        convertir_prefab_interno("guid-prefab-legado", texto, &ctx, &HashMap::new())
    }

    #[test]
    fn legado_usa_la_raiz_de_la_cabecera() {
        let (conv, avisos) = convertir(PREFAB_LEGADO).expect("el prefab legado debe convertirse");
        let prefab = &conv.prefab;
        assert_eq!(prefab.name, "RaizLegada");
        assert_eq!(prefab.root_entity.name, "RaizLegada");
        assert_eq!(
            conv.entidad_por_fileid.get(&1000011),
            Some(&prefab.root_entity.id)
        );
        let hijo = prefab
            .descendant_entities
            .iter()
            .find(|e| e.name == "Hijo")
            .expect("falta la entidad Hijo");
        assert_eq!(hijo.parent, Some(prefab.root_entity.id));
        let detalle = format!("{hijo:?}");
        assert!(detalle.contains("ModeloPlano"), "Hijo sin malla: {detalle}");
        assert!(detalle.contains("MatOscuro"), "Hijo sin material: {detalle}");
        assert!(
            !avisos.iter().any(|a| a.contains("prefab legado")),
            "aviso de instancia legada en el propio prefab: {avisos:?}"
        );
        assert!(
            avisos.iter().any(|a| a.contains("2 raíces: se usa la señalada")),
            "{avisos:?}"
        );
    }

    #[test]
    fn legado_sin_raiz_resoluble_usa_la_primera() {
        let texto = PREFAB_LEGADO.replace(
            "m_RootGameObject: {fileID: 1000011}",
            "m_RootGameObject: {fileID: 999}",
        );
        let (conv, avisos) = convertir(&texto).expect("el prefab legado debe convertirse");
        assert_eq!(conv.prefab.root_entity.name, "Suelto");
        assert!(
            avisos.iter().any(|a| a.contains("2 raíces: se usa la primera")),
            "{avisos:?}"
        );
        assert!(!avisos.iter().any(|a| a.contains("prefab legado")), "{avisos:?}");
    }
}

/// Si los documentos forman una variante de Unity (exactamente una
/// PrefabInstance cuyo origen ya está convertido y ningún GameObject
/// propio salvo los añadidos por esa instancia), devuelve el guid de la
/// base y la fuente que el núcleo escribirá en la raíz expandida.
fn detectar_variante(
    docs: &[DocumentoUnity],
    ctx: &ContextoEscena,
    prefabs: &HashMap<String, PrefabConvertido>,
) -> Option<(String, String)> {
    let instancias: Vec<&DocumentoUnity> = docs
        .iter()
        .filter(|d| d.clase == 1001 && !d.stripped && d.nombre == "PrefabInstance")
        .collect();
    if instancias.len() != 1 {
        return None;
    }
    let instancia = instancias[0];
    let guid_base = guid_ref(&instancia.valor, "m_SourcePrefab")?;
    if !prefabs.contains_key(&guid_base) {
        return None;
    }
    let modificacion = campo(&instancia.valor, "m_Modification")?;
    let anadidos: std::collections::HashSet<i64> =
        refs_modificacion(modificacion, "m_AddedGameObjects").into_iter().collect();
    let propios = docs
        .iter()
        .filter(|d| d.clase == 1 && !d.stripped)
        .all(|d| anadidos.contains(&d.file_id));
    if !propios {
        return None;
    }
    let ruta = (ctx.rutas)(guid_base.as_str())?;
    let fuente = fuente_proyecto(&ruta);
    Some((guid_base, fuente))
}

/// Construye el `Prefab` de una variante a partir de la expansión del
/// núcleo: el snapshot es la expansión materializada (sin el vínculo de
/// la raíz, que lo expresa `variant_of`) y el diff sale de comparar la
/// expansión, remapeada a los ids locales de la base, contra la base.
/// `None` si la expansión no trae la raíz vinculada esperada o el diff
/// no se puede derivar (el llamante sigue entonces el camino normal).
fn derivar_variante(
    n: &Nucleo,
    base: &PrefabConvertido,
    fuente_base: &str,
) -> Option<(Prefab, glass_core::prefab::PrefabOverrides)> {
    // Criterio fuerte: la raíz expandida es la entidad vinculada a la base
    // cuyo id local es la raíz de la base.
    let id_raiz_base = base.prefab.root_entity.id;
    let raiz_exp = n.entidades.iter().find(|e| {
        e.prefab_source.as_deref() == Some(fuente_base)
            && e.prefab_local_id == Some(id_raiz_base)
    })?;
    // Remapeo: id de la expansión -> id para el diff. Los vinculados usan
    // su id local en la base; los añadidos (sin local) reciben ids por
    // encima del máximo de la base, en orden de aparición.
    let max_base = id_raiz_base.max(
        base.prefab
            .descendant_entities
            .iter()
            .map(|e| e.id)
            .max()
            .unwrap_or(0),
    );
    let mut mapa: HashMap<u64, u64> = HashMap::new();
    let mut siguiente = max_base + 1;
    for e in &n.entidades {
        let nuevo = match e.prefab_local_id {
            Some(l) => l,
            None => {
                let v = siguiente;
                siguiente += 1;
                v
            }
        };
        mapa.insert(e.id, nuevo);
    }
    let remapear = |e: &EntityData| -> EntityData {
        let mut c = e.clone();
        c.id = mapa[&e.id];
        c.parent = e.parent.and_then(|p| mapa.get(&p).copied());
        c.children = e.children.iter().filter_map(|h| mapa.get(h).copied()).collect();
        c.prefab_source = None;
        c.prefab_hash = None;
        c.prefab_local_id = None;
        c.prefab_overrides = None;
        c
    };
    let mut raiz_diff = remapear(raiz_exp);
    raiz_diff.parent = None;
    let editado = glass_core::Prefab {
        format_version: 1,
        name: raiz_diff.name.clone(),
        root_entity: raiz_diff,
        descendant_entities: n
            .entidades
            .iter()
            .filter(|e| e.id != raiz_exp.id)
            .map(remapear)
            .collect(),
        variant_of: None,
        variant_overrides: None,
    };
    let diff = glass_core::Prefab::calcular_diff_entre(&base.prefab, &editado)?;
    // Snapshot: la expansión con sus propios ids; la raíz pierde el
    // vínculo (lo expresa variant_of) y todas pierden el local_id, como
    // en un prefab normal. Las raíces anidadas de la base conservan SU
    // vínculo (su fuente es otra).
    let mut raiz_snap = raiz_exp.clone();
    raiz_snap.parent = None;
    raiz_snap.prefab_source = None;
    raiz_snap.prefab_hash = None;
    raiz_snap.prefab_local_id = None;
    raiz_snap.prefab_overrides = None;
    let descendants: Vec<EntityData> = n
        .entidades
        .iter()
        .filter(|e| e.id != raiz_exp.id)
        .map(|e| {
            let mut c = e.clone();
            c.prefab_local_id = None;
            c
        })
        .collect();
    let prefab = glass_core::Prefab {
        format_version: 1,
        name: raiz_snap.name.clone(),
        root_entity: raiz_snap,
        descendant_entities: descendants,
        variant_of: Some(fuente_base.to_string()),
        variant_overrides: Some(diff.clone()),
    };
    Some((prefab, diff))
}

#[cfg(test)]
mod tests {
    use super::*;

/// Variante de la Casa: mueve la Puerta a x=7 y añade «Extra» bajo la raíz.
    const VARIANTE: &str = "%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!1 &700
GameObject:
  m_ObjectHideFlags: 0
  m_CorrespondingSourceObject: {fileID: 0}
  m_PrefabInstance: {fileID: 0}
  m_PrefabAsset: {fileID: 0}
  serializedVersion: 6
  m_Component:
  - component: {fileID: 701}
  m_Layer: 0
  m_Name: Extra
  m_TagString: Untagged
  m_IsActive: 1
--- !u!4 &701
Transform:
  m_ObjectHideFlags: 0
  m_CorrespondingSourceObject: {fileID: 0}
  m_PrefabInstance: {fileID: 0}
  m_PrefabAsset: {fileID: 0}
  m_GameObject: {fileID: 700}
  serializedVersion: 2
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 2, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 5001}
--- !u!1001 &5000
PrefabInstance:
  m_ObjectHideFlags: 0
  serializedVersion: 2
  m_Modification:
    serializedVersion: 3
    m_TransformParent: {fileID: 0}
    m_Modifications:
    - target: {fileID: 401, guid: 11112222333344445555666677778888, type: 3}
      propertyPath: m_LocalPosition.x
      value: 7
      objectReference: {fileID: 0}
    m_RemovedComponents: []
    m_RemovedGameObjects: []
    m_AddedGameObjects:
    - targetCorrespondingSourceObject: {fileID: 400, guid: 11112222333344445555666677778888, type: 3}
      insertIndex: -1
      addedObject: {fileID: 700}
    m_AddedComponents: []
  m_SourcePrefab: {fileID: 100100000, guid: 11112222333344445555666677778888, type: 3}
--- !u!4 &5001 stripped
Transform:
  m_CorrespondingSourceObject: {fileID: 400, guid: 11112222333344445555666677778888, type: 3}
  m_PrefabInstance: {fileID: 5000}
  m_PrefabAsset: {fileID: 0}
";

    /// Un GameObject propio (no añadido) que contiene una instancia de la Casa.
    const PROPIO_E_INSTANCIA: &str = "%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!1 &800
GameObject:
  m_ObjectHideFlags: 0
  m_CorrespondingSourceObject: {fileID: 0}
  m_PrefabInstance: {fileID: 0}
  m_PrefabAsset: {fileID: 0}
  serializedVersion: 6
  m_Component:
  - component: {fileID: 801}
  m_Layer: 0
  m_Name: Propio
  m_TagString: Untagged
  m_IsActive: 1
--- !u!4 &801
Transform:
  m_ObjectHideFlags: 0
  m_CorrespondingSourceObject: {fileID: 0}
  m_PrefabInstance: {fileID: 0}
  m_PrefabAsset: {fileID: 0}
  m_GameObject: {fileID: 800}
  serializedVersion: 2
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children:
  - {fileID: 5001}
  m_Father: {fileID: 0}
--- !u!1001 &5000
PrefabInstance:
  m_ObjectHideFlags: 0
  serializedVersion: 2
  m_Modification:
    serializedVersion: 3
    m_TransformParent: {fileID: 801}
    m_Modifications: []
    m_RemovedComponents: []
    m_RemovedGameObjects: []
    m_AddedGameObjects: []
    m_AddedComponents: []
  m_SourcePrefab: {fileID: 100100000, guid: 11112222333344445555666677778888, type: 3}
--- !u!4 &5001 stripped
Transform:
  m_CorrespondingSourceObject: {fileID: 400, guid: 11112222333344445555666677778888, type: 3}
  m_PrefabInstance: {fileID: 5000}
  m_PrefabAsset: {fileID: 0}
";

    fn como_f64s(v: &serde_json::Value) -> Vec<f64> {
        v.as_array()
            .expect("se esperaba un array")
            .iter()
            .map(|x| x.as_f64().expect("se esperaba un número"))
            .collect()
    }

    #[test]
    fn variante_se_convierte_como_variante() {
        let (convertido, avisos) = convertir_prefab_interno(
            "guid-variante",
            VARIANTE,
            &ctx_con_prefab(),
            &prefabs_casa(),
        )
        .expect("la variante se convierte");
        let prefab = &convertido.prefab;
        assert_eq!(prefab.variant_of.as_deref(), Some("Assets/Prefabs/Casa.prefab"));
        let diff = prefab.variant_overrides.as_ref().expect("la variante trae su diff");
        assert_eq!(diff.propiedades.len(), 1);
        let p = &diff.propiedades[0];
        assert_eq!(p.local_id, 2);
        assert_eq!(p.path, "transform.position");
        // En espacio del motor (la conversión voltea la Z de Unity):
        // la Puerta de la base está en [0, 0, -1] y la variante la mueve en X.
        assert_eq!(como_f64s(&p.valor), vec![7.0, 0.0, -1.0]);
        assert_eq!(diff.anadidas.len(), 1);
        let a = &diff.anadidas[0];
        let sub = a.subarbol.as_ref().expect("la añadida trae snapshot");
        assert_eq!(sub.len(), 1);
        assert_eq!(sub[0].name, "Extra");
        assert!(diff.quitadas.is_empty());
        assert_eq!(1 + prefab.descendant_entities.len(), 3);
        assert!(prefab.root_entity.prefab_source.is_none());
        assert!(avisos.iter().any(|l| l.contains("variante de")));
    }

    #[test]
    fn variante_resuelta_contra_la_base_da_la_materializacion() {
        let (convertido, _) = convertir_prefab_interno(
            "guid-variante",
            VARIANTE,
            &ctx_con_prefab(),
            &prefabs_casa(),
        )
        .expect("la variante se convierte");
        let dir = std::env::temp_dir()
            .join(format!("glass_ripper_variante_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let casa = prefabs_casa();
        let base = &casa[G_PREFAB].prefab;
        std::fs::write(
            dir.join("Casa.prefab.json"),
            serde_json::to_string_pretty(base).unwrap(),
        )
        .unwrap();
        let mut variante = convertido.prefab.clone();
        variante.variant_of = Some("Casa.prefab.json".to_string());
        std::fs::write(
            dir.join("Variante.prefab.json"),
            serde_json::to_string_pretty(&variante).unwrap(),
        )
        .unwrap();
        let cargado = glass_core::Prefab::cargar_desde_proyecto(&dir, "Variante.prefab.json")
            .expect("la variante se resuelve contra la base");
        let todas: Vec<&EntityData> = std::iter::once(&cargado.root_entity)
            .chain(cargado.descendant_entities.iter())
            .collect();
        let puerta = todas.iter().find(|e| e.name == "Puerta").expect("falta la Puerta");
        assert_eq!(
            como_f64s(&serde_json::to_value(puerta.transform.position).unwrap()),
            vec![7.0, 0.0, -1.0]
        );
        let extra = todas.iter().find(|e| e.name == "Extra").expect("falta Extra");
        assert_eq!(
            como_f64s(&serde_json::to_value(extra.transform.position).unwrap()),
            vec![0.0, 2.0, 0.0]
        );
        assert_eq!(extra.parent, Some(cargado.root_entity.id));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefab_con_gameobject_propio_no_es_variante() {
        let (convertido, _) = convertir_prefab_interno(
            "guid-propio",
            PROPIO_E_INSTANCIA,
            &ctx_con_prefab(),
            &prefabs_casa(),
        )
        .expect("se convierte por el camino normal");
        assert!(convertido.prefab.variant_of.is_none());
    }

    const G_MALLA: &str = "0123456789abcdef0123456789abcdef";
    const G_MAT: &str = "fedcba9876543210fedcba9876543210";
    const G_SCRIPT: &str = "aaaabbbbccccddddeeeeffff00001111";
    const G_PREFAB: &str = "11112222333344445555666677778888";

    fn rutas(g: &str) -> Option<String> {
        match g {
            G_MALLA => Some("Assets/Models/Casa.glb".to_string()),
            G_MAT => Some("Assets/Materiales/Madera.material.json".to_string()),
            _ => None,
        }
    }

    fn clases(g: &str) -> Option<String> {
        (g == G_SCRIPT).then(|| "Juego.Jugador".to_string())
    }

    fn sin_physmat(_: &str) -> Option<String> {
        None
    }

    fn sin_objeto(_: &str, _: i64) -> Option<String> {
        None
    }

    fn ctx() -> ContextoEscena<'static> {
        ContextoEscena {
            rutas: &rutas,
            clase_de_script: &clases,
            physmat_de_guid: &sin_physmat,
            objeto_de_submalla: &sin_objeto,
        }
    }

    fn casi(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    const PREFAB: &str = r#"%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!1 &100
GameObject:
  m_ObjectHideFlags: 0
  m_Component:
  - component: {fileID: 400}
  - component: {fileID: 3300}
  - component: {fileID: 2300}
  m_Layer: 0
  m_Name: Casa
  m_TagString: Untagged
  m_IsActive: 1
  m_StaticEditorFlags: 0
--- !u!4 &400
Transform:
  m_GameObject: {fileID: 100}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 1, y: 2, z: 3}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children:
  - {fileID: 401}
  m_Father: {fileID: 0}
--- !u!33 &3300
MeshFilter:
  m_GameObject: {fileID: 100}
  m_Mesh: {fileID: 4300000, guid: 0123456789abcdef0123456789abcdef, type: 3}
--- !u!23 &2300
MeshRenderer:
  m_GameObject: {fileID: 100}
  m_Enabled: 1
  m_Materials:
  - {fileID: 2100000, guid: fedcba9876543210fedcba9876543210, type: 2}
--- !u!1 &101
GameObject:
  m_Component:
  - component: {fileID: 401}
  - component: {fileID: 6500}
  m_Layer: 0
  m_Name: Puerta
  m_TagString: Untagged
  m_IsActive: 1
--- !u!4 &401
Transform:
  m_GameObject: {fileID: 101}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 1}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 400}
--- !u!65 &6500
BoxCollider:
  m_GameObject: {fileID: 101}
  m_IsTrigger: 0
  m_Enabled: 1
  m_Size: {x: 2, y: 4, z: 6}
  m_Center: {x: 0, y: 0, z: 0}
"#;

    const ESCENA: &str = r#"%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!1 &6000
GameObject:
  m_Component:
  - component: {fileID: 6001}
  m_Layer: 0
  m_Name: Suelo
  m_TagString: Untagged
  m_IsActive: 1
--- !u!4 &6001
Transform:
  m_GameObject: {fileID: 6000}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 0}
--- !u!1001 &5000
PrefabInstance:
  m_ObjectHideFlags: 0
  serializedVersion: 2
  m_Modification:
    m_TransformParent: {fileID: 0}
    m_Modifications:
    - target: {fileID: 100, guid: 11112222333344445555666677778888, type: 3}
      propertyPath: m_Name
      value: CasaNueva
      objectReference: {fileID: 0}
    - target: {fileID: 400, guid: 11112222333344445555666677778888, type: 3}
      propertyPath: m_LocalPosition.z
      value: 5
      objectReference: {fileID: 0}
    m_RemovedComponents: []
  m_SourcePrefab: {fileID: 100100000, guid: 11112222333344445555666677778888, type: 3}
--- !u!4 &5001 stripped
Transform:
  m_CorrespondingSourceObject: {fileID: 400, guid: 11112222333344445555666677778888, type: 3}
  m_PrefabInstance: {fileID: 5000}
  m_PrefabAsset: {fileID: 0}
"#;

    #[test]
    fn prefab_con_jerarquia_malla_y_collider() {
        let (conv, _) = convertir_prefab_interno(G_PREFAB, PREFAB, &ctx(), &HashMap::new()).unwrap();
        let raiz = &conv.prefab.root_entity;
        assert_eq!(raiz.id, 1);
        assert_eq!(raiz.name, "Casa");
        assert_eq!(conv.prefab.name, "Casa");
        assert_eq!(raiz.children, vec![2]);
        assert!(casi(raiz.transform.position[2], -3.0));
        let mesh = raiz.mesh.as_ref().unwrap();
        assert_eq!(mesh.model, "Casa");
        assert_eq!(mesh.material, "Madera");
        assert!(raiz.components.iter().any(|c| c == "MeshRenderer"));

        assert_eq!(conv.prefab.descendant_entities.len(), 1);
        let hijo = &conv.prefab.descendant_entities[0];
        assert_eq!(hijo.parent, Some(1));
        let p = hijo.physics.as_ref().unwrap();
        assert_eq!(p.body_type, "Static");
        assert!(!p.has_rigidbody);
        assert_eq!(p.colliders.len(), 1);
        assert_eq!(p.colliders[0].shape, "Box");
        let s = p.colliders[0].size;
        assert!(casi(s[0], 1.0) && casi(s[1], 2.0) && casi(s[2], 3.0));
        assert!(hijo.components.iter().any(|c| c == "Collider"));

        assert_eq!(conv.entidad_por_fileid.get(&2300), Some(&1));
        assert_eq!(conv.entidad_por_fileid.get(&6500), Some(&2));
        assert_eq!(conv.guid_fuente, G_PREFAB);
    }

    #[test]
    fn escena_con_instancia_expandida() {
        let (conv, _) = convertir_prefab_interno(G_PREFAB, PREFAB, &ctx(), &HashMap::new()).unwrap();
        let mut prefabs = HashMap::new();
        prefabs.insert(G_PREFAB.to_string(), conv);
        let (ents, avisos) = convertir_escena_interno(ESCENA, &ctx(), &prefabs);
        assert_eq!(ents.len(), 3);
        assert_eq!(ents[0].name, "Suelo");
        assert_eq!(ents[0].id, 1);
        let raiz = &ents[1];
        assert_eq!(raiz.id, 2);
        assert_eq!(raiz.name, "CasaNueva");
        assert!(raiz.parent.is_none());
        let p = raiz.transform.position;
        assert!(casi(p[0], 1.0) && casi(p[1], 2.0) && casi(p[2], -5.0));
        assert_eq!(ents[2].parent, Some(2));
        assert!(avisos.iter().any(|a| a.contains("expandida sin vínculo vivo")));
    }

    #[test]
    fn camara_con_espacios_y_luz_puntual() {
        let texto = r#"--- !u!1 &1
GameObject:
  m_Component:
  - component: {fileID: 3}
  m_Layer: 0
  m_Name: Camara
  m_TagString: MainCamera
  m_IsActive: 1
--- !u!20 &3
Camera:
  m_GameObject: {fileID: 1}
  m_Enabled: 1
  near clip plane: 0.3
  far clip plane: 500
  field of view: 70
  orthographic: 0
--- !u!1 &10
GameObject:
  m_Component:
  - component: {fileID: 11}
  m_Layer: 0
  m_Name: Luz
  m_TagString: Untagged
  m_IsActive: 1
--- !u!108 &11
Light:
  m_GameObject: {fileID: 10}
  m_Type: 2
  m_Color: {r: 1, g: 0.5, b: 0.25, a: 1}
  m_Intensity: 2
  m_Range: 15
  m_Shadows:
    m_Type: 1
"#;
        let (ents, _) = convertir_escena_interno(texto, &ctx(), &HashMap::new());
        let cam = ents[0].camera.as_ref().unwrap();
        assert!(casi(cam.fov, 70.0) && casi(cam.near_clip, 0.3) && casi(cam.far_clip, 500.0));
        assert!(cam.is_main);
        let luz = ents[1].light.as_ref().unwrap();
        assert_eq!(luz.light_type, "Point");
        assert!(casi(luz.color[1], 0.5) && casi(luz.color[2], 0.25));
        assert!(casi(luz.intensity, 2.0) && casi(luz.range, 15.0));
        assert!(luz.cast_shadow);
    }

    #[test]
    fn monobehaviour_resuelto_y_no_resuelto() {
        let texto = r#"--- !u!1 &1
GameObject:
  m_Component:
  - component: {fileID: 2}
  m_Layer: 0
  m_Name: Jugador
  m_IsActive: 1
--- !u!114 &2
MonoBehaviour:
  m_GameObject: {fileID: 1}
  m_Enabled: 1
  m_Script: {fileID: 11500000, guid: aaaabbbbccccddddeeeeffff00001111, type: 3}
  m_EditorClassIdentifier: 
  move_speed: 3
  vida: 10
--- !u!1 &5
GameObject:
  m_Component:
  - component: {fileID: 6}
  m_Layer: 0
  m_Name: Otro
  m_IsActive: 1
--- !u!114 &6
MonoBehaviour:
  m_GameObject: {fileID: 5}
  m_Enabled: 1
  m_Script: {fileID: 11500000, guid: 99998888777766665555444433332222, type: 3}
"#;
        let (ents, avisos) = convertir_escena_interno(texto, &ctx(), &HashMap::new());
        assert_eq!(ents[0].script.as_deref(), Some("Jugador"));
        let datos = ents[0].script_data.as_ref().unwrap();
        assert_eq!(datos.class_name, "Juego.Jugador");
        assert!(casi(datos.move_speed, 3.0));
        assert!(ents[0].components.iter().any(|c| c == "Script"));
        // C0b: `vida: 10` ya no se pierde ni avisa «sin hueco»: viaja en campos.
        assert_eq!(datos.campos.get("vida"), Some(&serde_json::json!(10)));
        assert!(!avisos.iter().any(|a| a.contains("sin hueco en script_data")));
        assert!(ents[1].script.is_none());
        assert!(!ents[1].components.iter().any(|c| c == "Script"));
        assert!(avisos.iter().any(|a| a.contains("script no convertido")));
    }

    #[test]
    fn clase_desconocida_contada() {
        let texto = r#"--- !u!104 &2
RenderSettings:
  m_Fog: 0
--- !u!104 &3
RenderSettings:
  m_Fog: 1
--- !u!1 &4
GameObject:
  m_Name: Vacio
  m_IsActive: 1
"#;
        let (ents, avisos) = convertir_escena_interno(texto, &ctx(), &HashMap::new());
        assert_eq!(ents.len(), 1);
        assert!(avisos
            .iter()
            .any(|a| a == "ClassID 104 (RenderSettings): 2 documentos omitidos"));
    }

    #[test]
    fn prefab_legado_es_error() {
        let texto = "--- !u!1001 &100100000\nPrefab:\n  m_ObjectHideFlags: 1\n";
        assert!(convertir_prefab_interno(G_PREFAB, texto, &ctx(), &HashMap::new()).is_err());
    }

    fn rutas_con_prefab(g: &str) -> Option<String> {
        if g == G_PREFAB {
            Some("Assets/Prefabs/Casa.prefab".to_string())
        } else {
            rutas(g)
        }
    }

    fn ctx_con_prefab() -> ContextoEscena<'static> {
        ContextoEscena {
            rutas: &rutas_con_prefab,
            clase_de_script: &clases,
            physmat_de_guid: &sin_physmat,
            objeto_de_submalla: &sin_objeto,
        }
    }

    fn prefabs_casa() -> HashMap<String, PrefabConvertido> {
        let (conv, _) =
            convertir_prefab_interno(G_PREFAB, PREFAB, &ctx(), &HashMap::new()).unwrap();
        let mut prefabs = HashMap::new();
        prefabs.insert(G_PREFAB.to_string(), conv);
        prefabs
    }

    #[test]
    fn rect_transform_en_base_convierte_valores() {
        let texto = r#"--- !u!1 &1
GameObject:
  m_Component:
  - component: {fileID: 2}
  m_Layer: 5
  m_Name: Panel
  m_TagString: Untagged
  m_IsActive: 1
--- !u!224 &2
RectTransform:
  m_GameObject: {fileID: 1}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 1, y: 2, z: 3}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 0}
  m_AnchorMin: {x: 0, y: 0}
  m_AnchorMax: {x: 1, y: 1}
  m_AnchoredPosition: {x: 10, y: 20}
  m_SizeDelta: {x: 100, y: 50}
  m_Pivot: {x: 0.5, y: 0.5}
"#;
        let (ents, _) = convertir_escena_interno(texto, &ctx(), &HashMap::new());
        assert_eq!(ents.len(), 1);
        let r = ents[0].rect_transform.as_ref().expect("falta rect_transform");
        assert_eq!(r.anchor_min, [0.0, 0.0]);
        assert_eq!(r.anchor_max, [1.0, 1.0]);
        assert_eq!(r.anchored_position, [10.0, 20.0]);
        assert_eq!(r.size_delta, [100.0, 50.0]);
        assert_eq!(r.pivot, [0.5, 0.5]);
        let p = ents[0].transform.position;
        assert!(casi(p[0], 1.0) && casi(p[1], 2.0) && casi(p[2], -3.0));
    }

    #[test]
    fn vinculo_en_escenas_y_en_prefabs() {
        // Fase E: el anidado dentro de un prefab convertido conserva su
        // vínculo vivo (antes de la Fase E se aplanaba sin vínculo, la
        // decisión C de la Fase B; la escena ya lo escribía).
        let prefabs = prefabs_casa();
        let huella = prefabs[G_PREFAB].prefab.huella();

        let (como_prefab, _) =
            convertir_prefab_interno("guid-contenedor", ESCENA, &ctx_con_prefab(), &prefabs)
                .unwrap();
        let expandidas = &como_prefab.prefab.descendant_entities;
        assert_eq!(expandidas.len(), 2);
        let raiz_anidada = expandidas
            .iter()
            .find(|e| e.name == "CasaNueva")
            .expect("falta la raíz anidada");
        assert_eq!(raiz_anidada.prefab_source.as_deref(), Some("Assets/Prefabs/Casa.prefab"));
        assert_eq!(raiz_anidada.prefab_hash, Some(huella.clone()));
        assert_eq!(raiz_anidada.prefab_local_id, Some(1));
        let puerta_anidada = expandidas
            .iter()
            .find(|e| e.name == "Puerta")
            .expect("falta Puerta");
        assert_eq!(puerta_anidada.prefab_local_id, Some(2));
        assert!(puerta_anidada.prefab_source.is_none());

        let (ents, _) = convertir_escena_interno(ESCENA, &ctx_con_prefab(), &prefabs);
        assert_eq!(ents.len(), 3);
        let raiz = ents
            .iter()
            .find(|e| e.name == "CasaNueva")
            .expect("falta la raíz expandida");
        assert_eq!(raiz.prefab_source.as_deref(), Some("Assets/Prefabs/Casa.prefab"));
        assert_eq!(raiz.prefab_hash, Some(huella));
        assert_eq!(raiz.prefab_local_id, Some(1));
        let puerta = ents.iter().find(|e| e.name == "Puerta").expect("falta Puerta");
        assert_eq!(puerta.parent, Some(raiz.id));
        assert_eq!(puerta.prefab_local_id, Some(2));
    }

    #[test]
    fn fuente_proyecto_antepone_assets_si_falta() {
        assert_eq!(
            fuente_proyecto("Prefabs/Casa.prefab.json"),
            "Assets/Prefabs/Casa.prefab.json"
        );
        assert_eq!(
            fuente_proyecto("Assets/Prefabs/Casa.prefab.json"),
            "Assets/Prefabs/Casa.prefab.json"
        );
        assert_eq!(
            fuente_proyecto("assets/Prefabs/Casa.prefab.json"),
            "assets/Prefabs/Casa.prefab.json"
        );
    }

    #[test]
    fn vinculo_con_ruta_de_plan_corta() {
        // El plan de producción da rutas relativas a `Assets/` (sin el
        // componente, porque `espejo` lo quita): la fuente escrita debe
        // ser igualmente la relativa al proyecto. Antes del arreglo, la
        // instancia nacía con "Prefabs/..." y el editor la dejaba como
        // vínculo roto.
        fn rutas_cortas(g: &str) -> Option<String> {
            if g == G_PREFAB {
                Some("Prefabs/Casa.prefab.json".to_string())
            } else {
                rutas(g)
            }
        }
        let ctx_corto = ContextoEscena {
            rutas: &rutas_cortas,
            clase_de_script: &clases,
            physmat_de_guid: &sin_physmat,
            objeto_de_submalla: &sin_objeto,
        };
        let prefabs = prefabs_casa();
        let (ents, _) = convertir_escena_interno(ESCENA, &ctx_corto, &prefabs);
        let raiz = ents
            .iter()
            .find(|e| e.name == "CasaNueva")
            .expect("falta la raíz expandida");
        assert_eq!(
            raiz.prefab_source.as_deref(),
            Some("Assets/Prefabs/Casa.prefab.json")
        );
    }

    #[test]
    fn instancia_legada_escribe_vinculo_sin_reexpandir() {
        let texto = r#"%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!1001 &900
Prefab:
  m_ObjectHideFlags: 1
  serializedVersion: 2
  m_Modification:
    m_TransformParent: {fileID: 0}
    m_Modifications: []
    m_RemovedComponents: []
  m_ParentPrefab: {fileID: 100100000, guid: 11112222333344445555666677778888, type: 3}
  m_RootGameObject: {fileID: 7000}
  m_IsPrefabParent: 0
--- !u!1 &7000
GameObject:
  m_ObjectHideFlags: 0
  m_PrefabParentObject: {fileID: 100, guid: 11112222333344445555666677778888, type: 3}
  m_PrefabInternal: {fileID: 900}
  m_Component:
  - component: {fileID: 7001}
  m_Layer: 0
  m_Name: Casa
  m_TagString: Untagged
  m_IsActive: 1
--- !u!4 &7001
Transform:
  m_PrefabParentObject: {fileID: 400, guid: 11112222333344445555666677778888, type: 3}
  m_PrefabInternal: {fileID: 900}
  m_GameObject: {fileID: 7000}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 1, y: 2, z: 3}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children:
  - {fileID: 7011}
  m_Father: {fileID: 0}
--- !u!1 &7010
GameObject:
  m_ObjectHideFlags: 0
  m_PrefabParentObject: {fileID: 101, guid: 11112222333344445555666677778888, type: 3}
  m_PrefabInternal: {fileID: 900}
  m_Component:
  - component: {fileID: 7011}
  m_Layer: 0
  m_Name: Puerta
  m_TagString: Untagged
  m_IsActive: 1
--- !u!4 &7011
Transform:
  m_PrefabParentObject: {fileID: 401, guid: 11112222333344445555666677778888, type: 3}
  m_PrefabInternal: {fileID: 900}
  m_GameObject: {fileID: 7010}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0, z: 1}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 7001}
"#;
        let prefabs = prefabs_casa();
        let huella = prefabs[G_PREFAB].prefab.huella();
        let (ents, avisos) = convertir_escena_interno(texto, &ctx_con_prefab(), &prefabs);
        assert_eq!(ents.len(), 2, "{ents:?}");
        assert!(
            !avisos.iter().any(|a| a.contains("omitida") || a.contains("no resuelta") || a.contains("vacío")),
            "{avisos:?}"
        );
        let (casa, puerta) = (&ents[0], &ents[1]);
        assert_eq!(casa.name, "Casa");
        assert_eq!(casa.prefab_source.as_deref(), Some("Assets/Prefabs/Casa.prefab"));
        assert_eq!(casa.prefab_hash, Some(huella));
        assert_eq!(casa.prefab_local_id, Some(1));
        assert_eq!(puerta.name, "Puerta");
        assert_eq!(puerta.prefab_source, None);
        assert_eq!(puerta.prefab_local_id, Some(2));
        assert_eq!(casa.parent, None);
        assert_eq!(casa.children, vec![puerta.id]);
        assert_eq!(puerta.parent, Some(casa.id));
    }

    #[test]
    fn added_gameobject_y_componente_aplicados() {
        let texto = r#"%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!1001 &5000
PrefabInstance:
  m_ObjectHideFlags: 0
  serializedVersion: 2
  m_Modification:
    m_TransformParent: {fileID: 0}
    m_Modifications: []
    m_RemovedComponents: []
    m_AddedGameObjects:
    - {fileID: 8000}
    m_AddedComponents:
    - {fileID: 8100}
  m_SourcePrefab: {fileID: 100100000, guid: 11112222333344445555666677778888, type: 3}
--- !u!4 &5001 stripped
Transform:
  m_CorrespondingSourceObject: {fileID: 400, guid: 11112222333344445555666677778888, type: 3}
  m_PrefabInstance: {fileID: 5000}
  m_PrefabAsset: {fileID: 0}
--- !u!1 &5002 stripped
GameObject:
  m_CorrespondingSourceObject: {fileID: 100, guid: 11112222333344445555666677778888, type: 3}
  m_PrefabInstance: {fileID: 5000}
  m_PrefabAsset: {fileID: 0}
--- !u!1 &8000
GameObject:
  m_Component:
  - component: {fileID: 8001}
  m_Layer: 0
  m_Name: Anadido
  m_TagString: Untagged
  m_IsActive: 1
--- !u!4 &8001
Transform:
  m_GameObject: {fileID: 8000}
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 1, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Children: []
  m_Father: {fileID: 5001}
--- !u!108 &8100
Light:
  m_GameObject: {fileID: 5002}
  m_Enabled: 1
  m_Type: 2
  m_Color: {r: 1, g: 1, b: 1, a: 1}
  m_Intensity: 3
  m_Range: 10
"#;
        let (ents, avisos) = convertir_escena_interno(texto, &ctx_con_prefab(), &prefabs_casa());
        assert_eq!(ents.len(), 3, "{ents:?}");
        let raiz = ents
            .iter()
            .find(|e| e.name == "Casa")
            .expect("falta la raíz expandida");
        let anadido = ents
            .iter()
            .find(|e| e.name == "Anadido")
            .expect("falta el GameObject añadido");
        assert_eq!(anadido.parent, Some(raiz.id));
        assert_eq!(anadido.prefab_local_id, None);
        assert!(raiz.components.iter().any(|c| c == "Light"), "{:?}", raiz.components);
        let luz = raiz.light.as_ref().expect("la luz añadida no se aplicó");
        assert!(casi(luz.intensity, 3.0));
        assert!(
            !avisos
                .iter()
                .any(|a| a.contains("omitido") || a.contains("no resuelto") || a.contains("no se aplica")),
            "{avisos:?}"
        );
    }

    #[test]
    fn resumen_llega_a_convertir_escena() {
        let prefabs = prefabs_casa();
        let mut informe = InformeImportacion::default();
        let Ok(ents) = convertir_escena(
            "99990000aaaabbbbccccddddeeeeffff",
            "Assets/Scenes/Pueblo.unity",
            ESCENA,
            &ctx_con_prefab(),
            &prefabs,
            &mut informe,
        ) else {
            panic!("convertir_escena falló");
        };
        let expandidas = ents.iter().filter(|e| e.name != "Suelo").count();
        assert_eq!(expandidas, 2);
        assert_eq!(informe.prefabs.len(), 1);
        let resumen = format!("{:?}", informe.prefabs);
        for esperado in [
            G_PREFAB.to_string(),
            "\"Casa\"".to_string(),
            "instancias: 1".to_string(),
            format!("entidades: {expandidas}"),
            "aplicadas: 2".to_string(),
        ] {
            assert!(resumen.contains(&esperado), "falta {esperado} en {resumen}");
        }
        let texto = informe.a_texto();
        assert!(
            texto.contains(
                "Prefab Casa (guid 11112222333344445555666677778888): 1 instancia(s) en 1 escena(s), "
            ),
            "{texto}"
        );
    }
}
