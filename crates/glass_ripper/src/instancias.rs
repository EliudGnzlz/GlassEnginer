use crate::documentos::{campo, referencia, texto_de};
use crate::escenas::PrefabConvertido;
use crate::ejes::{convertir_cuaternion, convertir_escala, convertir_posicion};
use glass_core::scene::EntityData;
use serde_yaml::Value;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// Resolutores externos usados al aplicar modificaciones de instancia.
pub struct ResolutoresInstancia<'a> {
    /// guid de material → nombre para `MeshData.material`.
    pub material: &'a dyn Fn(&str) -> Option<String>,
    /// guid de malla → nombre para `MeshData.model`.
    pub modelo: &'a dyn Fn(&str) -> Option<String>,
}

/// Resultado de expandir una instancia de prefab.
pub struct Expansion {
    /// Entidades clonadas con ids nuevos; la raíz va primero.
    pub entidades: Vec<EntityData>,
    /// fileID del prefab FUENTE → id de la entidad clonada.
    pub entidad_por_fileid: HashMap<i64, u64>,
    pub aplicadas: usize,
    /// Avisos ya agregados.
    pub avisos: Vec<String>,
}

#[derive(Default, Clone)]
struct AjusteTransform {
    pos: [Option<f32>; 3],
    rot: [Option<f32>; 4],
    esc: [Option<f32>; 3],
}

impl AjusteTransform {
    fn activo(&self) -> bool {
        self.pos.iter().any(|v| v.is_some())
            || self.rot.iter().any(|v| v.is_some())
            || self.esc.iter().any(|v| v.is_some())
    }
}

/// Expande una instancia clonando el prefab fuente y aplicando las
/// modificaciones de Unity que se puedan mapear en v1.
pub fn expandir_instancia(
    fuente: &PrefabConvertido,
    modificacion: &Value,
    siguiente_id: &mut u64,
    resolutores: &ResolutoresInstancia,
) -> Expansion {
    let mut refs: Vec<&EntityData> =
        Vec::with_capacity(1 + fuente.prefab.descendant_entities.len());
    refs.push(&fuente.prefab.root_entity);
    refs.extend(fuente.prefab.descendant_entities.iter());

    let mut clones: Vec<EntityData> = Vec::with_capacity(refs.len());
    let mut viejo_a_nuevo: HashMap<u64, u64> = HashMap::with_capacity(refs.len());

    for entidad in &refs {
        let nuevo_id = *siguiente_id;
        *siguiente_id += 1;
        viejo_a_nuevo.insert(entidad.id, nuevo_id);
        let mut clon = (*entidad).clone();
        clon.id = nuevo_id;
        clones.push(clon);
    }

    for (i, original) in refs.iter().enumerate() {
        let Some(clon) = clones.get_mut(i) else {
            continue;
        };
        if i == 0 {
            clon.parent = None;
        } else {
            clon.parent = original
                .parent
                .and_then(|padre| viejo_a_nuevo.get(&padre).copied());
        }
        clon.children = original
            .children
            .iter()
            .filter_map(|hijo| viejo_a_nuevo.get(hijo).copied())
            .collect();
    }

    let mut id_a_indice: HashMap<u64, usize> = HashMap::with_capacity(clones.len());
    for (i, clon) in clones.iter().enumerate() {
        id_a_indice.insert(clon.id, i);
    }

    let mut entidad_por_fileid =
        HashMap::with_capacity(fuente.entidad_por_fileid.len());
    for (file_id, viejo_id) in &fuente.entidad_por_fileid {
        if let Some(&nuevo_id) = viejo_a_nuevo.get(viejo_id) {
            entidad_por_fileid.insert(*file_id, nuevo_id);
        }
    }

    let mut aplicadas = 0usize;
    let mut no_resueltas = 0usize;
    let mut layer_no_aplicado = 0usize;
    let mut material_sin_resolver = 0usize;
    let mut malla_sin_resolver = 0usize;
    let mut sin_mapeo: BTreeMap<String, usize> = BTreeMap::new();
    let mut ajustes = vec![AjusteTransform::default(); clones.len()];

    if let Some(Value::Sequence(modificaciones)) = campo(modificacion, "m_Modifications") {
        for item in modificaciones {
            let Some(objetivo) = campo(item, "target").and_then(referencia) else {
                no_resueltas += 1;
                continue;
            };

            let mismo_guid = match objetivo.guid.as_deref() {
                Some(g) => g == fuente.guid_fuente,
                None => true,
            };
            if !mismo_guid {
                no_resueltas += 1;
                continue;
            }

            let Some(&nuevo_id) = entidad_por_fileid.get(&objetivo.file_id) else {
                no_resueltas += 1;
                continue;
            };
            let Some(&ent_idx) = id_a_indice.get(&nuevo_id) else {
                continue;
            };
            let Some(path) = texto_de(item, "propertyPath") else {
                continue;
            };
            let valor = campo(item, "value");

            if path == "m_Name" {
                if let (Some(ent), Some(nombre)) =
                    (clones.get_mut(ent_idx), valor.and_then(texto_valor))
                {
                    ent.name = nombre;
                    aplicadas += 1;
                }
            } else if path == "m_IsActive" {
                if let (Some(ent), Some(activo)) =
                    (clones.get_mut(ent_idx), valor.and_then(valor_bool))
                {
                    ent.active = activo;
                    aplicadas += 1;
                }
            } else if path == "m_Layer" {
                layer_no_aplicado += 1;
            } else if let Some(componente) = componente_xyz(&path, "m_LocalPosition.") {
                if let (Some(v), Some(ajuste)) = (valor.and_then(valor_f32), ajustes.get_mut(ent_idx))
                {
                    ajuste.pos[componente] = Some(v);
                    aplicadas += 1;
                }
            } else if let Some(componente) = componente_quat(&path, "m_LocalRotation.") {
                if let (Some(v), Some(ajuste)) = (valor.and_then(valor_f32), ajustes.get_mut(ent_idx))
                {
                    ajuste.rot[componente] = Some(v);
                    aplicadas += 1;
                }
            } else if let Some(componente) = componente_xyz(&path, "m_LocalScale.") {
                if let (Some(v), Some(ajuste)) = (valor.and_then(valor_f32), ajustes.get_mut(ent_idx))
                {
                    ajuste.esc[componente] = Some(v);
                    aplicadas += 1;
                }
            } else if path.starts_with("m_LocalEulerAnglesHint.") || path == "m_RootOrder" {
                // Redundantes con el cuaternión o con el orden de documento.
            } else if let Some(indice) = indice_material(&path) {
                if indice == 0 {
                    if let Some(guid) = campo(item, "objectReference")
                        .and_then(referencia)
                        .and_then(|r| r.guid)
                    {
                        if let Some(ent) = clones.get_mut(ent_idx) {
                            if let Some(malla) = ent.mesh.as_mut() {
                                match (resolutores.material)(&guid) {
                                    Some(nombre) => {
                                        malla.material = nombre;
                                        aplicadas += 1;
                                    }
                                    None => material_sin_resolver += 1,
                                }
                            } else {
                                material_sin_resolver += 1;
                            }
                        }
                    }
                }
            } else if path == "m_Mesh" {
                if let Some(guid) = campo(item, "objectReference")
                    .and_then(referencia)
                    .and_then(|r| r.guid)
                {
                    if let Some(ent) = clones.get_mut(ent_idx) {
                        if let Some(malla) = ent.mesh.as_mut() {
                            match (resolutores.modelo)(&guid) {
                                Some(nombre) => {
                                    malla.model = nombre;
                                    aplicadas += 1;
                                }
                                None => malla_sin_resolver += 1,
                            }
                        } else {
                            malla_sin_resolver += 1;
                        }
                    }
                }
            } else {
                *sin_mapeo.entry(path).or_insert(0) += 1;
            }
        }
    }

    for (i, ajuste) in ajustes.iter().enumerate() {
        if !ajuste.activo() {
            continue;
        }
        let Some(ent) = clones.get_mut(i) else {
            continue;
        };

        let mut pos_unity = convertir_posicion(ent.transform.position);
        let mut rot_unity = convertir_cuaternion(ent.transform.rotation_quat());
        let mut esc_unity = convertir_escala(ent.transform.scale);

        for (k, v) in ajuste.pos.iter().enumerate() {
            if let Some(x) = v {
                pos_unity[k] = *x;
            }
        }
        for (k, v) in ajuste.rot.iter().enumerate() {
            if let Some(x) = v {
                rot_unity[k] = *x;
            }
        }
        for (k, v) in ajuste.esc.iter().enumerate() {
            if let Some(x) = v {
                esc_unity[k] = *x;
            }
        }

        ent.transform.position = convertir_posicion(pos_unity);
        ent.transform.scale = convertir_escala(esc_unity);
        ent.transform.set_rotation_quat(convertir_cuaternion(rot_unity));
    }

    let mut avisos = Vec::new();
    if no_resueltas > 0 {
        avisos.push(format!(
            "modificaciones con destino no resuelto: {no_resueltas}"
        ));
    }
    if layer_no_aplicado > 0 {
        avisos.push(format!(
            "m_Layer no aplicado (capa por instancia no mapeada en v1): {layer_no_aplicado}"
        ));
    }
    if material_sin_resolver > 0 {
        avisos.push(format!(
            "material de instancia sin resolver: {material_sin_resolver}"
        ));
    }
    if malla_sin_resolver > 0 {
        avisos.push(format!(
            "malla de instancia sin resolver: {malla_sin_resolver}"
        ));
    }
    for (path, veces) in sin_mapeo {
        avisos.push(format!(
            "propertyPath sin mapeo: {path} ({veces} veces)"
        ));
    }

    for clave in [
        "m_RemovedComponents",
        "m_RemovedGameObjects",
        "m_AddedGameObjects",
        "m_AddedComponents",
    ] {
        let n = contar_lista(modificacion, clave);
        if n > 0 {
            avisos.push(format!("{clave}: {n} elementos no aplicados en v1"));
        }
    }

    Expansion {
        entidades: clones,
        entidad_por_fileid,
        aplicadas,
        avisos,
    }
}

/// Extrae los GUIDs de los bloques `m_SourcePrefab` de un texto Unity.
pub fn dependencias_prefab(texto: &str) -> Vec<String> {
    let mut salida = Vec::new();
    let mut vistos = HashSet::new();

    for linea in texto.lines() {
        let Some(pos) = linea.find("m_SourcePrefab:") else {
            continue;
        };
        let resto = &linea[pos + "m_SourcePrefab:".len()..];
        if let Some(guid) = guid_en_linea(resto) {
            if vistos.insert(guid.clone()) {
                salida.push(guid);
            }
        }
    }

    salida
}

/// Orden topológico estable: cada guid aparece después de sus dependencias.
pub fn ordenar_prefabs(
    dependencias: &[(String, Vec<String>)],
) -> Result<Vec<String>, Vec<String>> {
    let nombres: Vec<String> = dependencias.iter().map(|(g, _)| g.clone()).collect();
    let mut indice = HashMap::with_capacity(nombres.len());
    for (i, nombre) in nombres.iter().enumerate() {
        indice.insert(nombre.clone(), i);
    }

    let mut adyacentes = vec![Vec::new(); nombres.len()];
    let mut grados = vec![0usize; nombres.len()];

    for (i, (_, deps)) in dependencias.iter().enumerate() {
        let mut vistos = HashSet::new();
        for dep in deps {
            if let Some(&j) = indice.get(dep) {
                if vistos.insert(j) {
                    adyacentes[j].push(i);
                    grados[i] += 1;
                }
            }
        }
    }

    let mut cola = VecDeque::new();
    for (i, &grado) in grados.iter().enumerate() {
        if grado == 0 {
            cola.push_back(i);
        }
    }

    let mut resultado = Vec::with_capacity(nombres.len());
    let mut procesado = vec![false; nombres.len()];

    while let Some(i) = cola.pop_front() {
        resultado.push(nombres[i].clone());
        procesado[i] = true;

        for &j in &adyacentes[i] {
            grados[j] = grados[j].saturating_sub(1);
            if grados[j] == 0 {
                cola.push_back(j);
            }
        }
    }

    if resultado.len() == nombres.len() {
        Ok(resultado)
    } else {
        let err = nombres
            .iter()
            .enumerate()
            .filter(|(i, _)| !procesado[*i])
            .map(|(_, nombre)| nombre.clone())
            .collect();
        Err(err)
    }
}

fn contar_lista(modificacion: &Value, clave: &str) -> usize {
    match campo(modificacion, clave) {
        Some(Value::Sequence(seq)) => seq.len(),
        Some(Value::Null) | None => 0,
        Some(_) => 1,
    }
}

fn guid_en_linea(texto: &str) -> Option<String> {
    let pos = texto.find("guid:")?;
    let resto = &texto[pos + "guid:".len()..];
    let inicio = resto.find(|c: char| !c.is_whitespace() && c != '"' && c != '\'')?;
    let sub = &resto[inicio..];
    let fin = sub
        .find(|c: char| c.is_whitespace() || c == ',' || c == '}' || c == ']' || c == '"')
        .unwrap_or(sub.len());
    if fin == 0 {
        return None;
    }
    Some(
        sub[..fin]
            .trim_matches('"')
            .trim_matches('\'')
            .to_string(),
    )
}

fn texto_valor(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => n
            .as_i64()
            .map(|i| i.to_string())
            .or_else(|| n.as_f64().map(|f| f.to_string())),
        _ => None,
    }
}

fn valor_f32(v: &Value) -> Option<f32> {
    match v {
        Value::String(s) => s.trim().parse().ok(),
        Value::Number(n) => n.as_f64().map(|x| x as f32),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn valor_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|i| i != 0),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Some(true),
            "0" | "false" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn componente_xyz(path: &str, prefijo: &str) -> Option<usize> {
    match path.strip_prefix(prefijo)? {
        "x" => Some(0),
        "y" => Some(1),
        "z" => Some(2),
        _ => None,
    }
}

fn componente_quat(path: &str, prefijo: &str) -> Option<usize> {
    match path.strip_prefix(prefijo)? {
        "x" => Some(0),
        "y" => Some(1),
        "z" => Some(2),
        "w" => Some(3),
        _ => None,
    }
}

fn indice_material(path: &str) -> Option<usize> {
    let resto = path.strip_prefix("m_Materials.Array.data[")?;
    let fin = resto.find(']')?;
    resto[..fin].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glass_core::scene::TransformData;

    fn entidad(id: u64, parent: Option<u64>, children: Vec<u64>) -> EntityData {
        EntityData {
            id,
            active: true,
            name: format!("E{id}"),
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
            parent,
            children,
        }
    }

    fn prefab_fabricado() -> PrefabConvertido {
        let mut mapa = HashMap::new();
        mapa.insert(100, 10);
        mapa.insert(200, 11);
        PrefabConvertido {
            prefab: glass_core::Prefab {
                format_version: 1,
                name: "P".to_string(),
                root_entity: entidad(10, None, vec![11]),
                descendant_entities: vec![entidad(11, Some(10), Vec::new())],
            },
            entidad_por_fileid: mapa,
            guid_fuente: "gfuente".to_string(),
        }
    }

    fn modificacion(yaml: &str) -> Value {
        serde_yaml::from_str(yaml).unwrap()
    }

    type ResolutorPrueba = fn(&str) -> Option<String>;

    fn sin_resolutores() -> (ResolutorPrueba, ResolutorPrueba) {
        (|_| None, |_| None)
    }

    #[test]
    fn expansion_remapea_ids_y_jerarquia() {
        let material = |_: &str| -> Option<String> { None };
        let modelo = |_: &str| -> Option<String> { None };
        let resolutores = ResolutoresInstancia {
            material: &material,
            modelo: &modelo,
        };
        let mut siguiente = 5;
        let exp = expandir_instancia(
            &prefab_fabricado(),
            &Value::Null,
            &mut siguiente,
            &resolutores,
        );

        assert_eq!(siguiente, 7);
        assert_eq!(exp.entidades.len(), 2);
        assert_eq!(exp.entidades[0].id, 5);
        assert_eq!(exp.entidades[1].id, 6);
        assert_eq!(exp.entidades[0].parent, None);
        assert_eq!(exp.entidades[0].children, vec![6]);
        assert_eq!(exp.entidades[1].parent, Some(5));
        assert_eq!(exp.entidad_por_fileid.get(&100), Some(&5));
        assert_eq!(exp.entidad_por_fileid.get(&200), Some(&6));
    }

    #[test]
    fn aplica_nombre_posicion_y_rotacion() {
        let material = |_: &str| -> Option<String> { None };
        let modelo = |_: &str| -> Option<String> { None };
        let resolutores = ResolutoresInstancia {
            material: &material,
            modelo: &modelo,
        };
        let modv = modificacion(
            r#"
m_Modifications:
- target: {fileID: 100}
  propertyPath: m_Name
  value: Nuevo
- target: {fileID: 100}
  propertyPath: m_LocalPosition.z
  value: "2"
- target: {fileID: 100}
  propertyPath: m_LocalRotation.x
  value: "0.5"
"#,
        );
        let mut siguiente = 1;
        let exp = expandir_instancia(
            &prefab_fabricado(),
            &modv,
            &mut siguiente,
            &resolutores,
        );

        assert_eq!(exp.aplicadas, 3);
        let raiz = &exp.entidades[0];
        assert_eq!(raiz.name, "Nuevo");
        assert!((raiz.transform.position[2] + 2.0).abs() < 1e-6);

        let esperado = normalizar(convertir_cuaternion([0.5, 0.0, 0.0, 1.0]));
        let actual = raiz.transform.rotation_quat();
        for (a, b) in actual.iter().zip(esperado.iter()) {
            assert!((*a - *b).abs() < 1e-5);
        }
    }

    #[test]
    fn aviso_agregado_para_path_desconocido() {
        let (material, modelo) = sin_resolutores();
        let resolutores = ResolutoresInstancia {
            material: &material,
            modelo: &modelo,
        };
        let modv = modificacion(
            r#"
m_Modifications:
- target: {fileID: 100}
  propertyPath: m_Algo
  value: 1
- target: {fileID: 100}
  propertyPath: m_Algo
  value: 2
- target: {fileID: 100}
  propertyPath: m_LocalEulerAnglesHint.x
  value: 10
"#,
        );
        let mut siguiente = 1;
        let exp = expandir_instancia(
            &prefab_fabricado(),
            &modv,
            &mut siguiente,
            &resolutores,
        );

        assert_eq!(exp.aplicadas, 0);
        let avisos_algo: Vec<_> = exp
            .avisos
            .iter()
            .filter(|a| a.contains("m_Algo"))
            .collect();
        assert_eq!(avisos_algo.len(), 1);
        assert!(avisos_algo[0].contains("(2 veces)"));
        assert!(!exp
            .avisos
            .iter()
            .any(|a| a.contains("m_LocalEulerAnglesHint")));
    }

    #[test]
    fn avisa_lista_removed_components() {
        let (material, modelo) = sin_resolutores();
        let resolutores = ResolutoresInstancia {
            material: &material,
            modelo: &modelo,
        };
        let modv = modificacion(
            r#"
m_RemovedComponents:
- {fileID: 123}
"#,
        );
        let mut siguiente = 1;
        let exp = expandir_instancia(
            &prefab_fabricado(),
            &modv,
            &mut siguiente,
            &resolutores,
        );
        assert!(exp
            .avisos
            .iter()
            .any(|a| a.contains("m_RemovedComponents: 1 elementos")));
    }

    #[test]
    fn orden_topologico_y_ciclos() {
        let s = |v: &str| v.to_string();
        let cadena = vec![
            (s("A"), vec![s("B")]),
            (s("B"), vec![s("C")]),
            (s("C"), Vec::new()),
        ];
        assert_eq!(
            ordenar_prefabs(&cadena).unwrap(),
            vec![s("C"), s("B"), s("A")]
        );

        let ciclo = vec![(s("X"), vec![s("Y")]), (s("Y"), vec![s("X")])];
        let err = ordenar_prefabs(&ciclo).unwrap_err();
        assert!(err.contains(&s("X")));
        assert!(err.contains(&s("Y")));

        let ausente = vec![(s("A"), vec![s("Z")]), (s("B"), Vec::new())];
        let res = ordenar_prefabs(&ausente).unwrap();
        assert_eq!(res.len(), 2);
        assert!(res.contains(&s("A")));
        assert!(res.contains(&s("B")));
    }

    #[test]
    fn extrae_guids_de_source_prefab() {
        let texto = r#"
  m_SourcePrefab: {fileID: 100100000, guid: aaaa, type: 3}
  m_SourcePrefab: {fileID: 100100000, guid: bbbb, type: 3}
"#;
        assert_eq!(
            dependencias_prefab(texto),
            vec!["aaaa".to_string(), "bbbb".to_string()]
        );
    }

    fn normalizar(q: [f32; 4]) -> [f32; 4] {
        let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
    }
}
