use std::collections::{HashMap, HashSet};
use std::path::Path;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::scene::{EntityData, SceneState};

/// Representa un activo de Prefab reutilizable, serializable a `.prefab.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefab {
    pub format_version: u32,
    pub name: String,
    pub root_entity: EntityData,
    pub descendant_entities: Vec<EntityData>,
    /// Fuente (relativa al proyecto) del prefab base si este asset es una variante.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant_of: Option<String>,
    /// Diff de la variante respecto a su base (formato de la Fase D, con snapshot en las añadidas).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant_overrides: Option<PrefabOverrides>,
}

impl Prefab {
    /// Crea un Prefab a partir de una entidad existente y todos sus descendientes en la escena.
    pub fn from_entity(scene: &SceneState, root_id: u64) -> Result<Self, String> {
        let root = scene
            .get_entity(root_id)
            .ok_or_else(|| format!("Entidad #{} no existe en la escena", root_id))?
            .clone();

        let mut descendants = Vec::new();
        let mut queue = root.children.clone();

        while let Some(child_id) = queue.pop() {
            if let Some(child) = scene.get_entity(child_id) {
                descendants.push(child.clone());
                queue.extend(child.children.clone());
            }
        }

        Ok(Self {
            format_version: 1,
            name: root.name.clone(),
            root_entity: root,
            descendant_entities: descendants,
            variant_of: None,
            variant_overrides: None,
        })
    }

    /// Guarda el Prefab en un archivo `.prefab.json`.
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)?;
        std::fs::write(p, json)
    }

    /// Carga un Prefab desde un archivo `.prefab.json`.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let prefab: Self = serde_json::from_str(&json)
            .map_err(std::io::Error::other)?;
        Ok(prefab)
    }

    /// Resuelve un `prefab_source` contra la raíz del proyecto y carga el
    /// asset. Si la ruta no existe tal cual, prueba intercambiando la
    /// capitalización del primer componente (`assets` ↔ `Assets`): las
    /// fuentes escritas por distintas vías no siempre coinciden en
    /// mayúsculas y en Linux el sistema de archivos sí distingue.
    /// Ruta real del asset de una fuente relativa al proyecto: la
    /// directa si existe; si no, con la capitalización alterna del primer
    /// componente (`Assets` ↔ `assets`). `None` si no está en ninguna.
    pub fn resolver_ruta(raiz: &Path, fuente: &str) -> Option<std::path::PathBuf> {
        let directa = raiz.join(fuente);
        if directa.is_file() {
            return Some(directa);
        }
        let mut partes = fuente.splitn(2, '/');
        let primero = partes.next().unwrap_or("");
        let resto = partes.next().unwrap_or("");
        let alterno = match primero {
            "assets" => "Assets",
            "Assets" => "assets",
            _ => primero,
        };
        if alterno != primero {
            let alt = raiz.join(alterno).join(resto);
            if alt.is_file() {
                return Some(alt);
            }
        }
        None
    }

    /// Carga el asset SIN resolver variantes: el contenido tal cual está en
    /// disco. Para crear/editar variantes; el resto del motor usa
    /// `cargar_desde_proyecto`, que resuelve.
    pub fn cargar_crudo_desde_proyecto(raiz: &Path, fuente: &str) -> Option<Self> {
        Self::load_from_file(Self::resolver_ruta(raiz, fuente)?).ok()
    }

    /// Carga el asset resolviendo la cadena de variantes: si el archivo es
    /// una variante, materializa base + su diff (y la base puede ser a su vez
    /// variante). Un ciclo, una base ausente o un diff imposible de aplicar
    /// devuelven `None` con el motivo en el log: el vínculo se trata como
    /// roto, nunca se entra en bucle.
    pub fn cargar_desde_proyecto(raiz: &Path, fuente: &str) -> Option<Self> {
        match Self::resolver_variante(raiz, fuente, &mut Vec::new()) {
            Ok(p) => Some(p),
            Err(e) => {
                log::error!("Prefab {fuente}: {e}");
                None
            }
        }
    }

    fn resolver_variante(raiz: &Path, fuente: &str, cadena: &mut Vec<String>) -> Result<Self, String> {
        let clave = fuente.replace('\\', "/");
        let clave = clave.strip_prefix("./").unwrap_or(&clave).to_string();
        if cadena.iter().any(|c| c == &clave) {
            let mut ciclo = cadena.clone();
            ciclo.push(clave);
            return Err(format!("ciclo de variantes detectado: {}", ciclo.join(" → ")));
        }
        let crudo = Self::cargar_crudo_desde_proyecto(raiz, &clave)
            .ok_or_else(|| format!("asset no encontrado: {clave}"))?;
        let Some(fuente_base) = crudo.variant_of.clone() else { return Ok(crudo) };
        cadena.push(clave.clone());
        let mut resuelto = Self::resolver_variante(raiz, &fuente_base, cadena)?;
        if let Some(diff) = &crudo.variant_overrides {
            Self::aplicar_diff_variante(&mut resuelto, diff)
                .map_err(|e| format!("diff de la variante {clave} sobre {fuente_base}: {e}"))?;
        }
        resuelto.name = crudo.name.clone();
        resuelto.variant_of = crudo.variant_of.clone();
        resuelto.variant_overrides = None;
        Ok(resuelto)
    }

    fn aplicar_diff_variante(base: &mut Self, diff: &PrefabOverrides) -> Result<(), String> {
        for q in &diff.quitadas {
            base.eliminar_entidad(*q)?;
        }
        for p in &diff.propiedades {
            base.aplicar_propiedad(p.local_id, &p.path, &p.valor)?;
        }
        for a in &diff.anadidas {
            let sub = a.subarbol.as_ref().ok_or_else(|| {
                format!("la entidad añadida «{}» de la variante no trae snapshot", a.nombre)
            })?;
            if sub.is_empty() {
                return Err(format!("snapshot vacío en la añadida «{}»", a.nombre));
            }
            // Preorden: la primera es la raíz añadida; cada entidad se cuelga
            // de su padre ya insertado, remapeando ids con los asignados.
            let mut mapa: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
            for (i, ent) in sub.iter().enumerate() {
                let padre = if i == 0 {
                    a.padre_local_id
                } else {
                    let padre_snapshot = ent
                        .parent
                        .ok_or_else(|| "snapshot de añadida sin padre".to_string())?;
                    let padre_asignado = mapa.get(&padre_snapshot).copied().ok_or_else(|| {
                        "el padre de una entidad del snapshot no se insertó antes (¿preorden?)".to_string()
                    })?;
                    Some(padre_asignado)
                };
                let asignado = base.anadir_entidad(ent.clone(), padre)?;
                mapa.insert(ent.id, asignado);
            }
        }
        Ok(())
    }

    /// Diff de `editado` contra `base` en el formato de la Fase D, con el
    /// snapshot completo en las añadidas: es el diff que se guarda en un
    /// archivo de variante. `None` si las raíces no coinciden (la vista de
    /// edición conserva siempre la raíz del asset).
    ///
    /// Invariante: los ids nuevos de `editado` deben ser mayores que
    /// cualquier id que la base haya tenido (el asignador de la escena de
    /// edición es monótono y nunca reutiliza); un id reutilizado tras
    /// borrar una entidad se leería como modificación de la borrada.
    pub fn calcular_diff_entre(base: &Self, editado: &Self) -> Option<PrefabOverrides> {
        if base.root_entity.id != editado.root_entity.id {
            return None;
        }
        let mut diff = PrefabOverrides::default();
        let ids_base: std::collections::HashSet<u64> = base.entidades().map(|e| e.id).collect();
        let ids_edit: std::collections::HashSet<u64> = editado.entidades().map(|e| e.id).collect();
        // Propiedades: entidades presentes en ambos.
        for ent_edit in editado.entidades() {
            if !ids_base.contains(&ent_edit.id) {
                continue;
            }
            let Some(ent_base) = base.entidades().find(|e| e.id == ent_edit.id) else { continue };
            let es_raiz = ent_edit.id == base.root_entity.id;
            for path in paths_comparables(es_raiz) {
                let vb = valor_en_path(ent_base, path);
                let ve = valor_en_path(ent_edit, path);
                if vb != ve {
                    if let Some(valor) = ve {
                        diff.propiedades.push(OverridePropiedad {
                            local_id: ent_edit.id,
                            path: path.to_string(),
                            valor,
                            huerfana: false,
                        });
                    }
                }
            }
        }
        // Quitadas: en la base y no en el editado (la raíz nunca). Solo
        // las de arriba de cada rama: `eliminar_entidad` se lleva el
        // subárbol entero, y una quitada cuyo padre también falta ya no
        // existe al aplicar el diff (la resolución fallaría).
        for ent_base in base.entidades() {
            if ent_base.id == base.root_entity.id || ids_edit.contains(&ent_base.id) {
                continue;
            }
            let padre_quitado = ent_base
                .parent
                .is_some_and(|p| p != base.root_entity.id && !ids_edit.contains(&p));
            if !padre_quitado {
                diff.quitadas.push(ent_base.id);
            }
        }
        // Añadidas: raíces de subárbol cuyo padre sí está en la base.
        for ent_edit in editado.entidades() {
            if ids_base.contains(&ent_edit.id) {
                continue;
            }
            let padre_en_base = ent_edit.parent.is_some_and(|p| ids_base.contains(&p));
            if !padre_en_base {
                continue; // su raíz añadida ya lo cubre
            }
            let subarbol: Vec<EntityData> = recorrer_subarbol(editado.entidades(), ent_edit.id)
                .iter()
                .map(|n| n.e.clone())
                .collect();
            diff.anadidas.push(OverrideAnadida {
                padre_local_id: ent_edit.parent,
                nombre: ent_edit.name.clone(),
                indice: 0,
                subarbol: Some(subarbol),
            });
        }
        Some(diff)
    }

    /// Variante recién creada: apunta a la base, sin diff, con la expansión
    /// de la base como snapshot materializado en el propio archivo.
    pub fn variante_vacia(nombre: &str, fuente_base: &str, base: &Self) -> Self {
        Self {
            format_version: 1,
            name: nombre.to_string(),
            root_entity: base.root_entity.clone(),
            descendant_entities: base.descendant_entities.clone(),
            variant_of: Some(fuente_base.to_string()),
            variant_overrides: Some(PrefabOverrides::default()),
        }
    }

    /// Huella del asset: la misma que da una instancia intacta en la escena.
    pub fn huella(&self) -> String {
        huella_subarbol(
            std::iter::once(&self.root_entity).chain(&self.descendant_entities),
            self.root_entity.id,
            |e| e.id,
        )
    }

    /// Todas las entidades del asset: la raíz primero y después los descendientes.
    fn entidades(&self) -> impl Iterator<Item = &EntityData> {
        std::iter::once(&self.root_entity).chain(&self.descendant_entities)
    }

    /// Entidad del asset con ese id local (raíz o descendiente).
    fn entidad_mut(&mut self, local_id: u64) -> Option<&mut EntityData> {
        if self.root_entity.id == local_id {
            return Some(&mut self.root_entity);
        }
        self.descendant_entities.iter_mut().find(|e| e.id == local_id)
    }

    /// Escribe `valor` en `path` de la entidad del asset con id `local_id`.
    /// Falla si la entidad no existe o si el path no es comparable para ella
    /// (por ejemplo, `transform.*`, `name`, `tag` o `layer` en la raíz).
    pub fn aplicar_propiedad(&mut self, local_id: u64, path: &str, valor: &Value) -> Result<(), String> {
        let es_raiz = local_id == self.root_entity.id;
        if !paths_comparables(es_raiz).contains(&path) {
            return Err(format!(
                "El path `{}` no es aplicable a la entidad #{} del prefab",
                path, local_id
            ));
        }
        let ent = self
            .entidad_mut(local_id)
            .ok_or_else(|| format!("La entidad #{} no existe en el prefab", local_id))?;
        escribir_path(ent, path, valor)
    }

    /// Quita del asset la entidad `local_id` y todo su subárbol, y la
    /// desengancha de los `children` de su padre. Devuelve las entidades
    /// quitadas en orden de recorrido en anchura (la indicada primero).
    /// La raíz no se puede eliminar.
    pub fn eliminar_entidad(&mut self, local_id: u64) -> Result<Vec<EntityData>, String> {
        if local_id == self.root_entity.id {
            return Err("No se puede eliminar la raíz del prefab".into());
        }
        if !self.descendant_entities.iter().any(|e| e.id == local_id) {
            return Err(format!("La entidad #{} no existe en el prefab", local_id));
        }
        let orden: Vec<u64> = recorrer_subarbol(self.entidades(), local_id)
            .iter()
            .map(|n| n.e.id)
            .collect();
        let conjunto: HashSet<u64> = orden.iter().copied().collect();

        self.root_entity.children.retain(|c| *c != local_id);
        for e in &mut self.descendant_entities {
            e.children.retain(|c| *c != local_id);
        }

        let (mut quitadas, restantes): (Vec<EntityData>, Vec<EntityData>) =
            std::mem::take(&mut self.descendant_entities)
                .into_iter()
                .partition(|e| conjunto.contains(&e.id));
        self.descendant_entities = restantes;
        quitadas.sort_by_key(|e| orden.iter().position(|id| *id == e.id));
        Ok(quitadas)
    }

    /// Añade UNA entidad al asset como último hijo de `padre_local_id`
    /// (`None` = raíz del asset) y devuelve el id local asignado: el
    /// máximo de los ids del asset + 1.
    ///
    /// La entidad entra sin hijos: su `children` se vacía y sus campos de
    /// vínculo se borran. Para añadir un subárbol hay que llamar sucesivas
    /// veces, cada una con el id devuelto por la anterior como padre.
    pub fn anadir_entidad(&mut self, mut entidad: EntityData, padre_local_id: Option<u64>) -> Result<u64, String> {
        let padre = padre_local_id.unwrap_or(self.root_entity.id);
        let id = self
            .entidades()
            .map(|e| e.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("No quedan ids locales libres en el prefab")?;
        let padre_ent = self
            .entidad_mut(padre)
            .ok_or_else(|| format!("La entidad padre #{} no existe en el prefab", padre))?;
        padre_ent.children.push(id);

        entidad.id = id;
        entidad.parent = Some(padre);
        entidad.children.clear();
        entidad.prefab_source = None;
        entidad.prefab_hash = None;
        entidad.prefab_local_id = None;
        self.descendant_entities.push(entidad);
        Ok(id)
    }
}

// ---------------------------------------------------------------------------
// Diff de instancia frente al asset
// ---------------------------------------------------------------------------

/// Valor de una propiedad de una entidad del prefab que la instancia sobrescribe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverridePropiedad {
    /// Id local (en el asset) de la entidad afectada.
    pub local_id: u64,
    /// Path de la propiedad (ver `paths_comparables`).
    pub path: String,
    /// Valor en la instancia. En los paths de transform es un array de 3 números.
    pub valor: Value,
    /// La entidad o el path ya no existen en el asset.
    #[serde(default)]
    pub huerfana: bool,
}

/// Entidad que existe en la instancia y no en el asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideAnadida {
    /// Id local del padre en el asset (`None` si cuelga de la raíz).
    pub padre_local_id: Option<u64>,
    pub nombre: String,
    /// Posición entre las entidades añadidas bajo el mismo padre.
    pub indice: usize,
    /// Snapshot completo del subárbol añadido (raíz primero, preorden) cuando
    /// el diff vive en un ARCHIVO de variante y debe ser autocontenido. En el
    /// diff de una instancia en escena va `None`: la entidad vive en la escena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subarbol: Option<Vec<EntityData>>,
}

impl PartialEq for OverrideAnadida {
    fn eq(&self, other: &Self) -> bool {
        self.padre_local_id == other.padre_local_id
            && self.nombre == other.nombre
            && self.indice == other.indice
            && match (&self.subarbol, &other.subarbol) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    a.len() == b.len()
                        && a.iter()
                            .zip(b.iter())
                            .all(|(x, y)| serde_json::to_value(x).ok() == serde_json::to_value(y).ok())
                }
                _ => false,
            }
    }
}

/// Diferencias de una instancia respecto a su asset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrefabOverrides {
    #[serde(default)]
    pub propiedades: Vec<OverridePropiedad>,
    #[serde(default)]
    pub anadidas: Vec<OverrideAnadida>,
    /// Ids locales del asset que no están en la instancia.
    #[serde(default)]
    pub quitadas: Vec<u64>,
}

impl PrefabOverrides {
    /// `true` si la instancia no difiere en nada del asset.
    pub fn esta_vacia(&self) -> bool {
        self.propiedades.is_empty() && self.anadidas.is_empty() && self.quitadas.is_empty()
    }
}

/// Paths del transform, que se comparan y escriben de forma tipada.
const PATHS_TRANSFORM: [&str; 3] = ["transform.position", "transform.rotation", "transform.scale"];

/// Campos de primer nivel que se comparan y escriben por JSON.
const PATHS_JSON: [&str; 26] = [
    "name", "active", "tag", "layer", "is_static", "components",
    "mesh", "script", "script_data", "physics", "character", "audio",
    "particles", "camera", "light", "light_profile", "fog", "canvas",
    "rect_transform", "ui_text", "ui_button", "ui_image", "layout_group",
    "ui_drag", "animator", "portal",
];

/// Paths que nunca entran en el diff de la raíz, además de `transform.*`.
const EXCLUIDOS_RAIZ: [&str; 3] = ["name", "tag", "layer"];

/// Paths comparables en orden estable: los campos generales, el transform
/// y después los componentes. Con `es_raiz` se omiten `transform.*`,
/// `name`, `tag` y `layer`.
pub fn paths_comparables(es_raiz: bool) -> Vec<&'static str> {
    let (generales, componentes) = PATHS_JSON.split_at(6);
    generales
        .iter()
        .chain(PATHS_TRANSFORM.iter())
        .chain(componentes.iter())
        .copied()
        .filter(|p| !es_raiz || !(p.starts_with("transform.") || EXCLUIDOS_RAIZ.contains(p)))
        .collect()
}

/// Deserializa `v` al mismo tipo que `_muestra`.
fn como_tipo_de<T: DeserializeOwned>(_muestra: &T, v: &Value) -> Result<T, serde_json::Error> {
    serde_json::from_value(v.clone())
}

/// Valor de `path` en la entidad. El transform se lee tipado (la rotación,
/// en grados euler, por `rotation()`); el resto, del JSON de primer nivel.
/// Un campo omitido al serializar (p. ej. un `Option` en `None`) se lee
/// como `null`. Devuelve `None` si el path no existe.
pub fn valor_en_path(ent: &EntityData, path: &str) -> Option<Value> {
    match path {
        "transform.position" => serde_json::to_value(ent.transform.position).ok(),
        "transform.rotation" => serde_json::to_value(ent.transform.rotation()).ok(),
        "transform.scale" => serde_json::to_value(ent.transform.scale).ok(),
        p if PATHS_JSON.contains(&p) => {
            let json = serde_json::to_value(ent).ok()?;
            Some(json.get(p).cloned().unwrap_or(Value::Null))
        }
        _ => None,
    }
}

/// Escribe `valor` en `path`. El transform se escribe tipado: la rotación
/// pasa por `set_rotation`, que recalcula el cuaternión. El resto se escribe
/// serializando la entidad, cambiando la clave y deserializando; el
/// transform previo se conserva tal cual.
pub fn escribir_path(ent: &mut EntityData, path: &str, valor: &Value) -> Result<(), String> {
    let tipo = |e: serde_json::Error| format!("Valor no válido para `{}`: {}", path, e);
    match path {
        "transform.position" => {
            ent.transform.position = serde_json::from_value(valor.clone()).map_err(tipo)?;
        }
        "transform.rotation" => {
            let euler = como_tipo_de(&ent.transform.rotation(), valor).map_err(tipo)?;
            ent.transform.set_rotation_euler(euler);
        }
        "transform.scale" => {
            ent.transform.scale = serde_json::from_value(valor.clone()).map_err(tipo)?;
        }
        p if PATHS_JSON.contains(&p) => {
            let mut json = serde_json::to_value(&*ent)
                .map_err(|e| format!("No se pudo serializar la entidad #{}: {}", ent.id, e))?;
            json.as_object_mut()
                .ok_or_else(|| format!("La entidad #{} no se serializa como objeto", ent.id))?
                .insert(p.to_string(), valor.clone());
            let mut nueva: EntityData = serde_json::from_value(json).map_err(tipo)?;
            nueva.transform = ent.transform.clone();
            *ent = nueva;
        }
        _ => return Err(format!("El path `{}` no existe", path)),
    }
    Ok(())
}

/// Ruta de un asset de prefab tal como se guarda en `prefab_source`:
/// relativa al proyecto, con `/` como separador. Si la ruta es absoluta
/// y contiene una carpeta `assets`, se corta desde ella; si no, se usa
/// tal cual (normalizando separadores).
pub fn ruta_fuente_prefab(ruta: &Path) -> String {
    let texto = ruta.to_string_lossy().replace('\\', "/");
    let partes: Vec<&str> = texto.split('/').collect();
    if ruta.is_absolute() {
        if let Some(i) = partes
            .iter()
            .position(|p| p.eq_ignore_ascii_case("assets"))
        {
            return partes[i..].join("/");
        }
    }
    texto
}

/// Nodo de un recorrido en anchura de un subárbol.
pub struct Nodo<'a> {
    pub e: &'a EntityData,
    /// Padre dentro del subárbol (`None` en la raíz).
    pub padre: Option<&'a EntityData>,
    /// Posición entre los hijos existentes de su padre.
    pub indice: usize,
}

/// Recorre en anchura el subárbol que cuelga de `raiz_id`, siguiendo el orden
/// de `children` y saltando ids que no están en `entidades`.
pub fn recorrer_subarbol<'a>(
    entidades: impl IntoIterator<Item = &'a EntityData>,
    raiz_id: u64,
) -> Vec<Nodo<'a>> {
    let por_id: HashMap<u64, &'a EntityData> =
        entidades.into_iter().map(|e| (e.id, e)).collect();
    let mut nodos = Vec::new();
    let Some(raiz) = por_id.get(&raiz_id).copied() else {
        return nodos;
    };
    let mut vistos = HashSet::from([raiz_id]);
    nodos.push(Nodo { e: raiz, padre: None, indice: 0 });
    let mut i = 0;
    while i < nodos.len() {
        let padre = nodos[i].e;
        let mut indice = 0;
        for cid in &padre.children {
            if let Some(hijo) = por_id.get(cid).copied() {
                if vistos.insert(*cid) {
                    nodos.push(Nodo { e: hijo, padre: Some(padre), indice });
                    indice += 1;
                }
            }
        }
        i += 1;
    }
    nodos
}

/// FNV-1a de 64 bits. Implementación propia para que la huella no dependa
/// de la versión de ninguna dependencia.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Huella del subárbol que cuelga de `raiz_id` (id dentro de `entidades`).
///
/// `clave` da el id estable de cada entidad: su id en el asset o su
/// `prefab_local_id` en la escena, de modo que asset e instancia intacta
/// dan la misma huella. Entran los datos de cada entidad, su clave, la de
/// su padre y su posición entre los hijos. No entran los ids de escena,
/// los campos de vínculo ni el transform de la raíz.
///
/// Frontera de anidadas: una raíz de instancia anidada (con
/// `prefab_source`, que no sea la raíz del recorrido) aporta su fila,
/// pero su subárbol NO entra: es territorio de la instancia anidada, con
/// su propia huella. Sin esta frontera la huella del asset (que clavea
/// el subárbol anidado con los ids del asset exterior) y la de la escena
/// (que lo clavea con los ids del asset interior) jamás coincidirían.
pub fn huella_subarbol<'a>(
    entidades: impl IntoIterator<Item = &'a EntityData>,
    raiz_id: u64,
    clave: impl Fn(&EntityData) -> u64,
) -> String {
    fn v<T: Serialize>(x: &T) -> Value {
        serde_json::to_value(x).unwrap_or(Value::Null)
    }
    let nodos = recorrer_subarbol(entidades, raiz_id);
    // Ids bajo una raíz anidada (excluidos): el recorrido es en anchura
    // con el padre siempre antes que el hijo, así que la exclusión se
    // propaga en una sola pasada.
    let mut excluidos: std::collections::HashSet<u64> = std::collections::HashSet::new();
    for n in &nodos {
        if let Some(p) = n.padre {
            if excluidos.contains(&p.id)
                || (p.id != raiz_id && p.prefab_source.is_some())
            {
                excluidos.insert(n.e.id);
            }
        }
    }
    let mut filas: Vec<(u64, Value)> = nodos
        .into_iter()
        .filter(|n| !excluidos.contains(&n.e.id))
        .map(|n| {
            let e = n.e;
            let mut f = vec![
                Value::from(clave(e)),
                n.padre.map_or(Value::Null, |p| Value::from(clave(p))),
                n.padre.map_or(Value::Null, |_| Value::from(n.indice as u64)),
                v(&e.name),
                v(&e.active),
                v(&e.tag),
                v(&e.layer),
                v(&e.is_static),
                v(&e.components),
            ];
            if n.padre.is_some() {
                f.push(v(&e.transform));
            }
            f.extend(vec![
                v(&e.mesh),
                v(&e.script),
                v(&e.script_data),
                v(&e.physics),
                v(&e.character),
                v(&e.audio),
                v(&e.particles),
                v(&e.camera),
                v(&e.light),
                v(&e.light_profile),
                v(&e.fog),
                v(&e.canvas),
                v(&e.rect_transform),
                v(&e.ui_text),
                v(&e.ui_button),
                v(&e.ui_image),
                v(&e.layout_group),
                v(&e.ui_drag),
                v(&e.animator),
                v(&e.portal),
            ]);
            (clave(e), Value::Array(f))
        })
        .collect();
    // Orden por clave; a igualdad, el del recorrido (sort estable).
    filas.sort_by_key(|(k, _)| *k);
    let filas = Value::Array(filas.into_iter().map(|(_, f)| f).collect());
    let mut canonico = String::new();
    escribir_canonico(&filas, &mut canonico);
    format!("{:016x}", fnv1a_64(canonico.as_bytes()))
}

/// JSON con las claves de cada objeto ordenadas, para que la huella no
/// dependa del orden de inserción (p. ej. un `HashMap` dentro de un componente).
fn escribir_canonico(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut claves: Vec<&String> = m.keys().collect();
            claves.sort();
            out.push('{');
            for (i, k) in claves.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(k.clone()).to_string());
                out.push(':');
                escribir_canonico(&m[k.as_str()], out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                escribir_canonico(x, out);
            }
            out.push(']');
        }
        otro => out.push_str(&otro.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_prefab_creation_and_instantiation() {
        let mut scene = SceneState::new();
        let parent_id = scene.create_entity("CoinGroup".into(), vec!["Transform".into()]);
        let child_id = scene.create_entity("CoinMesh".into(), vec!["Transform".into(), "MeshRenderer".into()]);
        assert!(scene.reparent(child_id, Some(parent_id)));

        // Crear prefab desde parent
        let prefab = Prefab::from_entity(&scene, parent_id).expect("Error creando prefab");
        assert_eq!(prefab.name, "CoinGroup");
        assert_eq!(prefab.descendant_entities.len(), 1);

        // Guardar y cargar prefab en memoria JSON
        let json = serde_json::to_string(&prefab).unwrap();
        let loaded_prefab: Prefab = serde_json::from_str(&json).unwrap();

        // Instanciar prefab dos veces en la escena
        let inst1_id = scene.instantiate_prefab(&loaded_prefab, None, Some([1.0, 2.0, 3.0]), None);
        let inst2_id = scene.instantiate_prefab(&loaded_prefab, None, Some([4.0, 5.0, 6.0]), None);

        assert_ne!(inst1_id, parent_id);
        assert_ne!(inst2_id, inst1_id);

        let inst1_root = scene.get_entity(inst1_id).unwrap();
        assert_eq!(inst1_root.transform.position, [1.0, 2.0, 3.0]);
        assert_eq!(inst1_root.children.len(), 1);

        let inst1_child = scene.get_entity(inst1_root.children[0]).unwrap();
        assert_eq!(inst1_child.parent, Some(inst1_id));
        assert_ne!(inst1_child.id, child_id);
    }

    const RUTA: &str = "prefabs/coin.prefab.json";

    /// Escena con un prefab "CoinGroup" (raíz + un hijo) y una instancia vinculada.
    fn escena_con_instancia() -> (SceneState, Prefab, u64) {
        let mut scene = SceneState::new();
        let raiz = scene.create_entity("CoinGroup".into(), vec!["Transform".into()]);
        let hijo = scene.create_entity("CoinMesh".into(), vec!["Transform".into(), "MeshRenderer".into()]);
        assert!(scene.reparent(hijo, Some(raiz)));
        let prefab = Prefab::from_entity(&scene, raiz).expect("Error creando prefab");
        let inst = scene.instantiate_prefab(&prefab, None, Some([1.0, 2.0, 3.0]), Some(RUTA));
        (scene, prefab, inst)
    }

    #[test]
    fn instanciar_con_fuente_escribe_vinculo() {
        let (mut scene, prefab, inst) = escena_con_instancia();
        let raiz = scene.get_entity(inst).unwrap();
        assert_eq!(raiz.prefab_source.as_deref(), Some(RUTA));
        assert_eq!(raiz.prefab_hash, Some(prefab.huella()));
        assert_eq!(raiz.prefab_local_id, Some(prefab.root_entity.id));
        let hijo = scene.get_entity(raiz.children[0]).unwrap();
        assert_eq!(hijo.prefab_local_id, Some(prefab.descendant_entities[0].id));
        assert_eq!(hijo.prefab_source, None);
        assert_eq!(hijo.prefab_hash, None);

        // Sin fuente no hay vínculo.
        let suelta = scene.instantiate_prefab(&prefab, None, None, None);
        let suelta = scene.get_entity(suelta).unwrap();
        assert_eq!(suelta.prefab_source, None);
        assert_eq!(suelta.prefab_hash, None);
        assert_eq!(suelta.prefab_local_id, None);

        // Recién creada, la instancia está intacta: refrescar no avisa.
        let resolver = |r: &str| (r == RUTA).then(|| prefab.clone());
        assert!(scene.refrescar_instancias_prefab(&resolver).is_empty());
    }

    #[test]
    fn refrescar_aplica_cambios_del_asset_a_instancia_intacta() {
        let (mut scene, prefab, inst) = escena_con_instancia();
        let hijo_escena = scene.get_entity(inst).unwrap().children[0];

        // Editar el asset: cambia un dato del hijo, añade otro hijo y mueve la raíz.
        let mut editado = prefab.clone();
        editado.descendant_entities[0].tag = "Recogible".into();
        let nuevo_id = 10_000;
        let mut nuevo = EntityData::new(nuevo_id, "CoinShine".into(), vec!["Transform".into()]);
        nuevo.parent = Some(editado.root_entity.id);
        editado.descendant_entities.push(nuevo);
        editado.root_entity.children.push(nuevo_id);
        editado.root_entity.transform.position = [9.0, 9.0, 9.0];

        let resolver = |r: &str| (r == RUTA).then(|| editado.clone());
        assert!(scene.refrescar_instancias_prefab(&resolver).is_empty());

        let raiz = scene.get_entity(inst).unwrap();
        assert_eq!(raiz.transform.position, [1.0, 2.0, 3.0]);
        assert_eq!(raiz.prefab_hash, Some(editado.huella()));
        assert_eq!(raiz.children.len(), 2);
        assert_eq!(raiz.children[0], hijo_escena);
        let hijo = scene.get_entity(hijo_escena).unwrap();
        assert_eq!(hijo.tag, "Recogible");
        assert_eq!(hijo.parent, Some(inst));
        let creado = scene.get_entity(raiz.children[1]).unwrap();
        assert_eq!(creado.name, "CoinShine");
        assert_eq!(creado.prefab_local_id, Some(nuevo_id));
        assert_eq!(creado.parent, Some(inst));

        // Tras actualizar, la instancia queda intacta respecto al asset nuevo.
        assert!(scene.refrescar_instancias_prefab(&resolver).is_empty());
    }

    #[test]
    fn refrescar_no_pisa_instancia_modificada() {
        let (mut scene, prefab, inst) = escena_con_instancia();
        let hijo_escena = scene.get_entity(inst).unwrap().children[0];
        let hash_previo = scene.get_entity(inst).unwrap().prefab_hash.clone();
        scene.get_entity_mut(hijo_escena).unwrap().tag = "Tocado".into();

        let mut editado = prefab.clone();
        editado.descendant_entities[0].tag = "Recogible".into();
        let resolver = |r: &str| (r == RUTA).then(|| editado.clone());
        let avisos = scene.refrescar_instancias_prefab(&resolver);

        assert_eq!(avisos.len(), 1);
        assert!(avisos[0].contains("instancia modificada: no actualizada"));
        assert_eq!(scene.get_entity(hijo_escena).unwrap().tag, "Tocado");
        assert_eq!(scene.get_entity(inst).unwrap().prefab_hash, hash_previo);
    }

    #[test]
    fn refrescar_con_asset_ausente_avisa_vinculo_roto() {
        let (mut scene, _prefab, inst) = escena_con_instancia();
        let hijo_escena = scene.get_entity(inst).unwrap().children[0];
        let raiz_antes = serde_json::to_string(scene.get_entity(inst).unwrap()).unwrap();
        let hijo_antes = serde_json::to_string(scene.get_entity(hijo_escena).unwrap()).unwrap();

        let resolver = |_: &str| -> Option<Prefab> { None };
        let avisos = scene.refrescar_instancias_prefab(&resolver);

        assert_eq!(avisos.len(), 1);
        assert!(avisos[0].contains("vínculo roto"));
        assert_eq!(serde_json::to_string(scene.get_entity(inst).unwrap()).unwrap(), raiz_antes);
        assert_eq!(serde_json::to_string(scene.get_entity(hijo_escena).unwrap()).unwrap(), hijo_antes);
    }

    #[test]
    fn revertir_descarta_lo_tocado_y_deja_la_instancia_intacta() {
        let (mut scene, prefab, inst) = escena_con_instancia();
        let hijo_escena = scene.get_entity(inst).unwrap().children[0];

        // Tocar la instancia: cambiar un dato del hijo y añadir una entidad.
        scene.get_entity_mut(hijo_escena).unwrap().tag = "Tocado".into();
        let extra = scene.create_entity("Extra".into(), vec!["Transform".into()]);
        assert!(scene.reparent(extra, Some(inst)));

        // Revertir contra el asset original.
        scene.revertir_instancia_prefab(inst, &prefab).unwrap();

        // Lo tocado se descartó y lo añadido desapareció.
        assert_eq!(scene.get_entity(hijo_escena).unwrap().tag, "Untagged");
        assert!(scene.get_entity(extra).is_none());
        let raiz = scene.get_entity(inst).unwrap();
        assert_eq!(raiz.children.len(), 1);
        // La raíz conserva su transform de instancia y su vínculo, intacta.
        assert_eq!(raiz.transform.position, [1.0, 2.0, 3.0]);
        assert_eq!(raiz.prefab_source.as_deref(), Some(RUTA));
        assert_eq!(raiz.prefab_hash, Some(prefab.huella()));
        let resolver = |r: &str| (r == RUTA).then(|| prefab.clone());
        assert!(scene.refrescar_instancias_prefab(&resolver).is_empty());

        // Una entidad que no es raíz de instancia no se puede revertir.
        assert!(scene.revertir_instancia_prefab(hijo_escena, &prefab).is_err());
        assert!(scene.revertir_instancia_prefab(999_999, &prefab).is_err());
    }

    #[test]
    fn desempaquetar_corta_el_vinculo_y_conserva_el_estado() {
        let (mut scene, _prefab, inst) = escena_con_instancia();
        let hijo_escena = scene.get_entity(inst).unwrap().children[0];
        scene.get_entity_mut(hijo_escena).unwrap().tag = "Tocado".into();

        scene.desempaquetar_instancia(inst).unwrap();

        // El estado actual se hornea; el vínculo desaparece de todo el subárbol.
        let raiz = scene.get_entity(inst).unwrap();
        assert_eq!(raiz.prefab_source, None);
        assert_eq!(raiz.prefab_hash, None);
        assert_eq!(raiz.prefab_local_id, None);
        assert_eq!(raiz.transform.position, [1.0, 2.0, 3.0]);
        let hijo = scene.get_entity(hijo_escena).unwrap();
        assert_eq!(hijo.prefab_local_id, None);
        assert_eq!(hijo.tag, "Tocado");
        assert_eq!(hijo.parent, Some(inst));

        // Sin vínculo ya no se puede desempaquetar ni cuenta para el refresco.
        assert!(scene.desempaquetar_instancia(inst).is_err());
        let resolver = |_: &str| -> Option<Prefab> { None };
        assert!(scene.refrescar_instancias_prefab(&resolver).is_empty());
    }

    #[test]
    fn cargar_desde_proyecto_resuelve_y_tolera_capitalizacion() {
        let (_scene, prefab, _inst) = escena_con_instancia();
        let base = std::env::temp_dir().join(format!(
            "glass_prefab_resolver_{}",
            std::process::id()
        ));
        let assets = base.join("Assets").join("prefabs");
        std::fs::create_dir_all(&assets).unwrap();
        prefab.save_to_file(assets.join("coin.prefab.json")).unwrap();

        // Ruta tal cual y con el primer componente en minúsculas.
        let directa = Prefab::cargar_desde_proyecto(&base, "Assets/prefabs/coin.prefab.json");
        assert_eq!(directa.map(|p| p.name), Some("CoinGroup".into()));
        let alterna = Prefab::cargar_desde_proyecto(&base, "assets/prefabs/coin.prefab.json");
        assert_eq!(alterna.map(|p| p.name), Some("CoinGroup".into()));
        // Ausente: None, sin pánico.
        assert!(Prefab::cargar_desde_proyecto(&base, "Assets/prefabs/no.prefab.json").is_none());

        // resolver_ruta devuelve la ruta REAL (con la capitalización del
        // disco), que es la que necesita «Abrir» para guardar encima.
        let real = Prefab::resolver_ruta(&base, "assets/prefabs/coin.prefab.json");
        assert_eq!(real, Some(assets.join("coin.prefab.json")));
        assert!(Prefab::resolver_ruta(&base, "Assets/prefabs/no.prefab.json").is_none());

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn entidad_sin_campos_de_vinculo_deserializa_sin_vinculo() {
        let json = serde_json::to_value(EntityData::new(7, "Vieja".into(), vec!["Transform".into()])).unwrap();
        let obj = json.as_object().unwrap();
        assert!(!obj.contains_key("prefab_source"));
        assert!(!obj.contains_key("prefab_hash"));
        assert!(!obj.contains_key("prefab_local_id"));

        let leida: EntityData = serde_json::from_value(json).unwrap();
        assert_eq!(leida.prefab_source, None);
        assert_eq!(leida.prefab_hash, None);
        assert_eq!(leida.prefab_local_id, None);
    }

    // ---- Fase D1a: paths, operaciones sobre el asset y serde del diff ----

    /// Prefab con raíz → (hijo → nieto, otro). Devuelve (prefab, raíz, hijo, nieto, otro).
    fn prefab_de_prueba() -> (Prefab, u64, u64, u64, u64) {
        let mut scene = SceneState::new();
        let raiz = scene.create_entity("Raiz".into(), vec!["Transform".into()]);
        let hijo = scene.create_entity("Hijo".into(), vec!["Transform".into()]);
        let nieto = scene.create_entity("Nieto".into(), vec!["Transform".into()]);
        let otro = scene.create_entity("Otro".into(), vec!["Transform".into()]);
        assert!(scene.reparent(hijo, Some(raiz)));
        assert!(scene.reparent(nieto, Some(hijo)));
        assert!(scene.reparent(otro, Some(raiz)));
        let prefab = Prefab::from_entity(&scene, raiz).unwrap();
        (prefab, raiz, hijo, nieto, otro)
    }

    fn descendiente(p: &Prefab, id: u64) -> &EntityData {
        p.descendant_entities.iter().find(|e| e.id == id).unwrap()
    }

    fn como_f64(v: &Value) -> Vec<f64> {
        v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
    }

    #[test]
    fn paths_comparables_excluye_lo_de_la_raiz() {
        let todos = paths_comparables(false);
        assert_eq!(todos.len(), 29);
        assert!(todos.contains(&"transform.rotation"));
        assert!(todos.contains(&"name"));
        let raiz = paths_comparables(true);
        assert_eq!(raiz.len(), 23);
        for p in ["transform.position", "transform.rotation", "transform.scale", "name", "tag", "layer"] {
            assert!(!raiz.contains(&p));
        }
        for p in ["id", "parent", "children", "prefab_source", "prefab_hash", "prefab_local_id", "prefab_overrides", "transform"] {
            assert!(!todos.contains(&p));
        }
    }

    #[test]
    fn valor_en_path_y_escribir_path_ida_y_vuelta() {
        let mut e = EntityData::new(1, "E".into(), vec!["Transform".into()]);

        // Posición: tipada.
        escribir_path(&mut e, "transform.position", &json!([1.0, 2.0, 3.0])).unwrap();
        assert_eq!(e.transform.position, [1.0, 2.0, 3.0]);
        assert_eq!(como_f64(&valor_en_path(&e, "transform.position").unwrap()), vec![1.0, 2.0, 3.0]);

        // Rotación: vía set_rotation; rotation() devuelve lo escrito.
        escribir_path(&mut e, "transform.rotation", &json!([10.0, 20.0, 30.0])).unwrap();
        let leida = como_f64(&valor_en_path(&e, "transform.rotation").unwrap());
        for (a, b) in leida.iter().zip([10.0, 20.0, 30.0]) {
            assert!((a - b).abs() < 1e-4, "rotación {a} != {b}");
        }
        // Y sobrevive a una escritura por JSON de otro campo.
        escribir_path(&mut e, "tag", &json!("Enemigo")).unwrap();
        assert_eq!(e.tag, "Enemigo");
        let tras_json = como_f64(&valor_en_path(&e, "transform.rotation").unwrap());
        assert_eq!(tras_json, leida);

        // Campo por JSON (mesh): se copia de una entidad con MeshRenderer.
        let mut scene = SceneState::new();
        let con_mesh = scene.create_entity("M".into(), vec!["Transform".into(), "MeshRenderer".into()]);
        let mesh = valor_en_path(scene.get_entity(con_mesh).unwrap(), "mesh").unwrap();
        escribir_path(&mut e, "mesh", &mesh).unwrap();
        assert_eq!(valor_en_path(&e, "mesh").unwrap(), mesh);
        assert_eq!(serde_json::to_value(&e.mesh).unwrap(), mesh);

        // Path inexistente o excluido, y tipo que no encaja.
        assert!(valor_en_path(&e, "no_existe").is_none());
        assert!(valor_en_path(&e, "id").is_none());
        assert!(escribir_path(&mut e, "no_existe", &json!(1)).is_err());
        assert!(escribir_path(&mut e, "children", &json!([])).is_err());
        assert!(escribir_path(&mut e, "transform.position", &json!("hola")).is_err());
        assert_eq!(e.transform.position, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn aplicar_propiedad_en_descendiente_y_raiz() {
        let (mut p, raiz, hijo, _, _) = prefab_de_prueba();

        p.aplicar_propiedad(hijo, "tag", &json!("Recogible")).unwrap();
        assert_eq!(descendiente(&p, hijo).tag, "Recogible");
        p.aplicar_propiedad(hijo, "transform.scale", &json!([2.0, 2.0, 2.0])).unwrap();
        assert_eq!(descendiente(&p, hijo).transform.scale, [2.0, 2.0, 2.0]);

        p.aplicar_propiedad(raiz, "active", &json!(false)).unwrap();
        assert!(!p.root_entity.active);

        // En la raíz, tag y transform no son aplicables.
        assert!(p.aplicar_propiedad(raiz, "tag", &json!("X")).is_err());
        assert!(p.aplicar_propiedad(raiz, "transform.position", &json!([0.0, 0.0, 0.0])).is_err());
        // Id inexistente.
        assert!(p.aplicar_propiedad(999_999, "tag", &json!("X")).is_err());
    }

    #[test]
    fn eliminar_entidad_quita_el_subarbol() {
        let (mut p, raiz, hijo, nieto, otro) = prefab_de_prueba();

        let quitadas = p.eliminar_entidad(hijo).unwrap();
        let ids: Vec<u64> = quitadas.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![hijo, nieto]);
        assert_eq!(p.descendant_entities.len(), 1);
        assert_eq!(p.descendant_entities[0].id, otro);
        assert!(!p.root_entity.children.contains(&hijo));
        assert!(p.root_entity.children.contains(&otro));

        assert!(p.eliminar_entidad(raiz).is_err());
        assert!(p.eliminar_entidad(hijo).is_err());
    }

    #[test]
    fn anadir_entidad_asigna_max_mas_uno() {
        let (mut p, raiz, hijo, nieto, otro) = prefab_de_prueba();
        let max = [raiz, hijo, nieto, otro].into_iter().max().unwrap();

        let mut nueva = EntityData::new(0, "Nueva".into(), vec!["Transform".into()]);
        nueva.children.push(12_345);
        let id = p.anadir_entidad(nueva, Some(hijo)).unwrap();
        assert_eq!(id, max + 1);
        let e = descendiente(&p, id);
        assert_eq!(e.parent, Some(hijo));
        assert!(e.children.is_empty());
        assert_eq!(e.name, "Nueva");
        assert_eq!(descendiente(&p, hijo).children.last(), Some(&id));

        // Sin padre: cuelga de la raíz.
        let id2 = p
            .anadir_entidad(EntityData::new(0, "Otra".into(), vec!["Transform".into()]), None)
            .unwrap();
        assert_eq!(id2, id + 1);
        assert_eq!(descendiente(&p, id2).parent, Some(raiz));
        assert_eq!(p.root_entity.children.last(), Some(&id2));

        // Padre inexistente: error y el asset no cambia.
        let antes = p.descendant_entities.len();
        assert!(p
            .anadir_entidad(EntityData::new(0, "X".into(), vec![]), Some(999_999))
            .is_err());
        assert_eq!(p.descendant_entities.len(), antes);
    }

    #[test]
    fn prefab_overrides_serde_ida_y_vuelta() {
        assert!(PrefabOverrides::default().esta_vacia());
        let o = PrefabOverrides {
            propiedades: vec![OverridePropiedad {
                local_id: 3,
                path: "transform.position".into(),
                valor: json!([1.0, 2.0, 3.0]),
                huerfana: false,
            }],
            anadidas: vec![OverrideAnadida { padre_local_id: Some(2), nombre: "Extra".into(), indice: 0, subarbol: None }],
            quitadas: vec![5],
        };
        assert!(!o.esta_vacia());
        let texto = serde_json::to_string(&o).unwrap();
        let leido: PrefabOverrides = serde_json::from_str(&texto).unwrap();
        assert_eq!(leido, o);
        // Un objeto vacío deserializa como diff vacío.
        let vacio: PrefabOverrides = serde_json::from_str("{}").unwrap();
        assert!(vacio.esta_vacia());
    }
}

#[cfg(test)]
mod prefab_variantes_tests {
    use super::*;
    use serde_json::json;
    use std::path::{Path, PathBuf};

    const POS: &str = "transform.position";
    const BASE: &str = "prefabs/base.prefab";

    /// Carpeta temporal única por test; se borra al salir (también si el test falla).
    struct DirTemporal(PathBuf);

    impl DirTemporal {
        fn nuevo(nombre: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let ruta = std::env::temp_dir().join(format!(
                "glass_core_variantes_{nombre}_{}_{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&ruta).expect("no se pudo crear la carpeta temporal");
            Self(ruta)
        }
    }

    impl Drop for DirTemporal {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn ent(id: u64, nombre: &str, parent: Option<u64>, children: Vec<u64>) -> EntityData {
        let mut e = EntityData::new(id, nombre.to_string(), vec!["Transform".to_string()]);
        e.parent = parent;
        e.children = children;
        e
    }

    /// Raíz 1 con hijos 2 «Hijo» y 3 «Otro».
    fn fixture_base() -> Prefab {
        Prefab {
            format_version: 1,
            name: "Base".to_string(),
            root_entity: ent(1, "Raiz", None, vec![2, 3]),
            descendant_entities: vec![
                ent(2, "Hijo", Some(1), vec![]),
                ent(3, "Otro", Some(1), vec![]),
            ],
            variant_of: None,
            variant_overrides: None,
        }
    }

    fn variante(nombre: &str, fuente_base: &str, diff: PrefabOverrides) -> Prefab {
        let mut v = Prefab::variante_vacia(nombre, fuente_base, &fixture_base());
        v.variant_overrides = Some(diff);
        v
    }

    /// Diff de V1: posición del 2 a [9,0,0], quita el 3, añade «Nueva» (con hija) bajo el 2.
    fn diff_v1() -> PrefabOverrides {
        let mut d = PrefabOverrides::default();
        d.propiedades.push(OverridePropiedad {
            local_id: 2,
            path: POS.to_string(),
            valor: json!([9.0, 0.0, 0.0]),
            huerfana: false,
        });
        d.quitadas.push(3);
        d.anadidas.push(OverrideAnadida {
            padre_local_id: Some(2),
            nombre: "Nueva".to_string(),
            indice: 0,
            subarbol: Some(vec![
                ent(10, "Nueva", Some(2), vec![11]),
                ent(11, "NuevaHija", Some(10), vec![]),
            ]),
        });
        d
    }

    fn escribir(raiz: &Path, fuente: &str, p: &Prefab) {
        let ruta = raiz.join(fuente);
        if let Some(dir) = ruta.parent() {
            std::fs::create_dir_all(dir).expect("no se pudo crear la carpeta del asset");
        }
        let texto = serde_json::to_string_pretty(p).expect("no se pudo serializar el prefab");
        std::fs::write(&ruta, texto).expect("no se pudo escribir el asset");
    }

    fn todas(p: &Prefab) -> Vec<&EntityData> {
        std::iter::once(&p.root_entity).chain(p.descendant_entities.iter()).collect()
    }

    fn por_nombre<'a>(p: &'a Prefab, nombre: &str) -> Option<&'a EntityData> {
        todas(p).into_iter().find(|e| e.name == nombre)
    }

    fn por_id(p: &Prefab, id: u64) -> Option<&EntityData> {
        todas(p).into_iter().find(|e| e.id == id)
    }

    #[test]
    fn resolucion_simple() {
        let dir = DirTemporal::nuevo("simple");
        escribir(&dir.0, BASE, &fixture_base());
        escribir(&dir.0, "prefabs/v1.prefab", &variante("V1", BASE, diff_v1()));

        let p = Prefab::cargar_desde_proyecto(&dir.0, "prefabs/v1.prefab")
            .expect("la variante debe resolverse");

        assert_eq!(p.name, "V1");
        assert_eq!(p.variant_of.as_deref(), Some(BASE));
        assert!(p.variant_overrides.is_none());

        let hijo = por_id(&p, 2).expect("falta la entidad 2");
        assert_eq!(valor_en_path(hijo, POS), Some(json!([9.0, 0.0, 0.0])));
        assert!(por_nombre(&p, "Otro").is_none(), "la entidad 3 debía quitarse");

        let nueva = por_nombre(&p, "Nueva").expect("falta la añadida");
        assert_eq!(nueva.parent, Some(2));
        assert!(hijo.children.contains(&nueva.id));

        let hija = por_nombre(&p, "NuevaHija").expect("falta la hija de la añadida");
        assert_eq!(hija.parent, Some(nueva.id));
        assert!(nueva.children.contains(&hija.id));

        assert_eq!(todas(&p).len(), 4);
    }

    #[test]
    fn cadena_de_dos_variantes() {
        let dir = DirTemporal::nuevo("cadena");
        escribir(&dir.0, BASE, &fixture_base());
        escribir(&dir.0, "prefabs/v1.prefab", &variante("V1", BASE, diff_v1()));

        // El id de «NuevaHija» solo existe tras resolver V1: V2 depende del orden.
        let v1 = Prefab::cargar_desde_proyecto(&dir.0, "prefabs/v1.prefab").expect("V1 debe resolverse");
        let id_hija = por_nombre(&v1, "NuevaHija").expect("falta NuevaHija en V1").id;

        let mut d2 = PrefabOverrides::default();
        d2.propiedades.push(OverridePropiedad {
            local_id: 2,
            path: POS.to_string(),
            valor: json!([5.0, 0.0, 0.0]),
            huerfana: false,
        });
        d2.quitadas.push(id_hija);
        escribir(&dir.0, "prefabs/v2.prefab", &variante("V2", "prefabs/v1.prefab", d2));

        let p = Prefab::cargar_desde_proyecto(&dir.0, "prefabs/v2.prefab").expect("V2 debe resolverse");

        assert_eq!(p.name, "V2");
        assert_eq!(p.variant_of.as_deref(), Some("prefabs/v1.prefab"));
        assert!(p.variant_overrides.is_none());
        assert_eq!(valor_en_path(por_id(&p, 2).unwrap(), POS), Some(json!([5.0, 0.0, 0.0])));
        assert!(por_nombre(&p, "Otro").is_none());
        assert!(por_nombre(&p, "Nueva").is_some());
        assert!(por_nombre(&p, "NuevaHija").is_none());
    }

    #[test]
    fn ciclo_devuelve_none() {
        let dir = DirTemporal::nuevo("ciclo");
        escribir(&dir.0, "prefabs/a.prefab", &variante("A", "prefabs/b.prefab", PrefabOverrides::default()));
        escribir(&dir.0, "prefabs/b.prefab", &variante("B", "prefabs/a.prefab", PrefabOverrides::default()));

        assert!(Prefab::cargar_desde_proyecto(&dir.0, "prefabs/a.prefab").is_none());
    }

    #[test]
    fn base_ausente_devuelve_none() {
        let dir = DirTemporal::nuevo("ausente");
        escribir(
            &dir.0,
            "prefabs/v.prefab",
            &variante("V", "prefabs/no_existe.prefab", PrefabOverrides::default()),
        );

        assert!(Prefab::cargar_desde_proyecto(&dir.0, "prefabs/v.prefab").is_none());
        // El crudo sí carga: el archivo existe.
        assert!(Prefab::cargar_crudo_desde_proyecto(&dir.0, "prefabs/v.prefab").is_some());
    }

    #[test]
    fn diff_entre_prefabs() {
        let base = fixture_base();
        let mut editado = base.clone();
        editado.aplicar_propiedad(2, POS, &json!([9.0, 0.0, 0.0])).unwrap();
        editado.eliminar_entidad(3).unwrap();
        // Las añadidas entran con ids por encima del máximo que la base
        // haya tenido (10 y 11): es el invariante del asignador monótono
        // de la escena de edición. Reutilizar el id 3 recién borrado se
        // leería como «modificar Otro», no como añadir.
        editado
            .descendant_entities
            .iter_mut()
            .find(|e| e.id == 2)
            .unwrap()
            .children
            .push(10);
        editado.descendant_entities.push(ent(10, "Nueva", Some(2), vec![11]));
        editado.descendant_entities.push(ent(11, "NuevaHija", Some(10), vec![]));

        let diff = Prefab::calcular_diff_entre(&base, &editado).expect("las raíces coinciden");

        assert_eq!(diff.propiedades.len(), 1);
        assert_eq!(diff.propiedades[0].local_id, 2);
        assert_eq!(diff.propiedades[0].path, POS);
        assert_eq!(diff.quitadas, vec![3]);
        assert_eq!(diff.anadidas.len(), 1);

        let a = &diff.anadidas[0];
        assert_eq!(a.padre_local_id, Some(2));
        assert_eq!(a.nombre, "Nueva");
        let sub = a.subarbol.as_ref().expect("la añadida debe traer snapshot");
        assert_eq!(sub.len(), 2);
        assert_eq!(sub[0].name, "Nueva");
        assert_eq!(sub[1].name, "NuevaHija");

        let mut otra = editado.clone();
        otra.root_entity.id = 99;
        assert!(Prefab::calcular_diff_entre(&base, &otra).is_none());
    }

    #[test]
    fn diff_entre_quitadas_solo_la_cima_del_subarbol() {
        // Quitar un subárbol entero: el diff trae solo la quitada de
        // arriba; la de dentro ya no existe al aplicar (eliminar la
        // cima se lleva el subárbol) y la resolución fallaría con ella.
        let base = Prefab {
            format_version: 1,
            name: "Base".to_string(),
            root_entity: ent(1, "Raiz", None, vec![2]),
            descendant_entities: vec![
                ent(2, "Hijo", Some(1), vec![4]),
                ent(4, "Nieto", Some(2), vec![]),
            ],
            variant_of: None,
            variant_overrides: None,
        };
        let mut editado = base.clone();
        editado.eliminar_entidad(2).unwrap();
        let diff = Prefab::calcular_diff_entre(&base, &editado).expect("las raíces coinciden");
        assert_eq!(diff.quitadas, vec![2]);
        let mut resuelto = base.clone();
        Prefab::aplicar_diff_variante(&mut resuelto, &diff)
            .expect("el diff se aplica sin quitadas ya inexistentes");
        assert_eq!(resuelto.entidades().count(), 1);
    }

    #[test]
    fn serde_variante() {
        let v = Prefab::variante_vacia("V", BASE, &fixture_base());
        let json_v = serde_json::to_string(&v).unwrap();
        assert!(json_v.contains("\"variant_of\""));
        assert!(json_v.contains("\"variant_overrides\""));

        let leido: Prefab = serde_json::from_str(&json_v).unwrap();
        assert_eq!(leido.variant_of.as_deref(), Some(BASE));
        assert!(leido.variant_overrides.as_ref().is_some_and(|d| d.esta_vacia()));

        let json_normal = serde_json::to_string(&fixture_base()).unwrap();
        assert!(!json_normal.contains("variant_of"));
        assert!(!json_normal.contains("variant_overrides"));

        let anadida_escena = OverrideAnadida {
            padre_local_id: None,
            nombre: "X".to_string(),
            indice: 0,
            subarbol: None,
        };
        assert!(!serde_json::to_string(&anadida_escena).unwrap().contains("subarbol"));
    }
}
