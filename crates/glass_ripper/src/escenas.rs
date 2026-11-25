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
use crate::instancias::{expandir_instancia, ResolutoresInstancia};

/// Resolutores que el importador proporciona a la conversión de escenas.
pub struct ContextoEscena<'a> {
    /// GUID → ruta importada del asset (p. ej. "Assets/Models/X.glb",
    /// "Assets/Materiales/M.material.json", "Assets/Prefabs/P.prefab").
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

    /// Aviso que se emite como mucho una vez por archivo, sin conteo.
    fn unico(&mut self, texto: String) {
        if !self.cuenta.contains_key(&texto) {
            self.sumar(texto);
        }
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
    avisos: Avisos,
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
        for &c in comps {
            let v = &c.valor;
            match c.clase {
                4 | 224 => {
                    if c.clase == 224 {
                        self.avisos.unico(
                            "RectTransform presente: la UI se convierte como jerarquía; el Canvas va en la fase de UI"
                                .to_string(),
                        );
                    }
                    self.entidad_de_transform.insert(c.file_id, id);
                    if con_transform {
                        self.avisos
                            .sumar("GameObject con varios Transform: se usa el primero".to_string());
                    } else {
                        con_transform = true;
                        e.transform = transform_unity(v);
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
                20 => {
                    e.camera = Some(self.camara(v, &e.tag));
                    anadir_componente(&mut e, "Camera");
                }
                108 => {
                    e.light = Some(self.luz(v));
                    anadir_componente(&mut e, "Light");
                }
                95 => {
                    e.animator = Some(self.animator(v));
                    anadir_componente(&mut e, "Animator");
                }
                114 => self.monobehaviour(v, &mut e),
                54 | 64 | 65 | 135 | 136 | 143 => fisica.push((c.clase, v)),
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
            }
            if let Some(mut p) = f.physics {
                p.layer = capa.clamp(0, 31) as u8;
                if p.has_collider {
                    anadir_componente(&mut e, "Collider");
                }
                e.physics = Some(p);
            }
            if f.character.is_some() {
                anadir_componente(&mut e, "CharacterController");
            }
            e.character = f.character;
        }
        e
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
        if let Some(m) = v.as_mapping() {
            for (k, _) in m {
                let Some(k) = k.as_str() else { continue };
                if CLAVES_ESTANDAR.contains(&k)
                    || matches!(k, "move_speed" | "jump_velocity" | "auto_demo")
                {
                    continue;
                }
                self.avisos
                    .sumar(format!("campo serializado sin hueco en script_data: {k}"));
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

    // 2. Entidades planas, en orden de documento.
    let mut entidades: Vec<EntityData> = Vec::new();
    let mut pos: HashMap<u64, usize> = HashMap::new();
    let mut por_fileid: HashMap<i64, u64> = HashMap::new();
    let mut gos: HashSet<i64> = HashSet::new();
    let mut siguiente_id: u64 = 1;
    for d in docs.iter().filter(|d| d.clase == 1 && !d.stripped) {
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
    let resolutores = ResolutoresInstancia {
        material: &resolver_material,
        modelo: &resolver_modelo,
    };
    for d in docs.iter().filter(|d| d.clase == 1001 && !d.stripped) {
        if d.nombre == "Prefab" {
            est.avisos
                .sumar("instancia de prefab legado (anterior a 2018.3): omitida".to_string());
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
        let modificacion = campo(v, "m_Modification").unwrap_or(&nulo);
        let exp = expandir_instancia(fuente, modificacion, &mut siguiente_id, &resolutores);
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
            "instancia de {}: {} entidades, {} modificaciones aplicadas, expandida sin vínculo vivo",
            fuente.prefab.name,
            exp.entidades.len(),
            exp.aplicadas
        ));
        for a in exp.avisos {
            est.avisos.sumar(a);
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
        avisos: est.avisos,
    }
}

fn convertir_escena_interno(
    texto: &str,
    ctx: &ContextoEscena,
    prefabs: &HashMap<String, PrefabConvertido>,
) -> (Vec<EntityData>, Vec<String>) {
    let docs = partir_documentos(texto);
    let n = ejecutar_nucleo(&docs, ctx, prefabs);
    let avisos = n.avisos.lineas();
    (n.entidades, avisos)
}

fn convertir_prefab_interno(
    guid: &str,
    texto: &str,
    ctx: &ContextoEscena,
    prefabs: &HashMap<String, PrefabConvertido>,
) -> Result<(PrefabConvertido, Vec<String>), String> {
    let docs = partir_documentos(texto);
    if docs.iter().any(|d| d.nombre == "Prefab") {
        return Err("prefab legado (anterior a 2018.3): no soportado en v1".to_string());
    }
    let mut n = ejecutar_nucleo(&docs, ctx, prefabs);
    let raices: Vec<usize> = n.entidades[..n.planas]
        .iter()
        .enumerate()
        .filter(|(_, e)| e.parent.is_none())
        .map(|(i, _)| i)
        .collect();
    let Some(&iraiz) = raices.first() else {
        return Err("prefab sin GameObject raíz".to_string());
    };
    if raices.len() > 1 {
        n.avisos.sumar(format!(
            "el prefab tiene {} raíces: se usa la primera",
            raices.len()
        ));
    }
    let raiz = n.entidades.remove(iraiz);
    let prefab = Prefab {
        format_version: 1,
        name: raiz.name.clone(),
        root_entity: raiz,
        descendant_entities: n.entidades,
    };
    Ok((
        PrefabConvertido {
            prefab,
            entidad_por_fileid: n.por_fileid,
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
    let (entidades, avisos) = convertir_escena_interno(texto, ctx, prefabs);
    let detalle = format!("escena: {} entidades", entidades.len());
    volcar_informe(informe, guid, ruta, &detalle, &avisos);
    Ok(entidades)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(avisos
            .iter()
            .any(|a| a.contains("sin hueco en script_data: vida")));
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
}
