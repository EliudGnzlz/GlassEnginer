use std::collections::{HashMap, HashSet};
use std::path::Path;
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

    pub fn cargar_desde_proyecto(raiz: &Path, fuente: &str) -> Option<Self> {
        Self::load_from_file(Self::resolver_ruta(raiz, fuente)?).ok()
    }

    /// Huella del asset: la misma que da una instancia intacta en la escena.
    pub fn huella(&self) -> String {
        huella_subarbol(
            std::iter::once(&self.root_entity).chain(&self.descendant_entities),
            self.root_entity.id,
            |e| e.id,
        )
    }
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
pub(crate) struct Nodo<'a> {
    pub e: &'a EntityData,
    /// Padre dentro del subárbol (`None` en la raíz).
    pub padre: Option<&'a EntityData>,
    /// Posición entre los hijos existentes de su padre.
    pub indice: usize,
}

/// Recorre en anchura el subárbol que cuelga de `raiz_id`, siguiendo el orden
/// de `children` y saltando ids que no están en `entidades`.
pub(crate) fn recorrer_subarbol<'a>(
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
pub fn huella_subarbol<'a>(
    entidades: impl IntoIterator<Item = &'a EntityData>,
    raiz_id: u64,
    clave: impl Fn(&EntityData) -> u64,
) -> String {
    fn v<T: Serialize>(x: &T) -> Value {
        serde_json::to_value(x).unwrap_or(Value::Null)
    }
    let mut filas: Vec<(u64, Value)> = recorrer_subarbol(entidades, raiz_id)
        .into_iter()
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
}
