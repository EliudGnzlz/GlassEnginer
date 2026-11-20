//! Lector FBX: vuelca la escena de ufbx en el IR de la crate
//! (`modelo::EscenaIr`) aplicando el sistema de glTF (ejes diestros
//! con +Y arriba, metros) y las reglas del cargador del motor.

use std::collections::{HashMap, HashSet};
use std::path::Path;

const MAX_JOINTS: usize = 256;

type ClaveEsquina = (u32, [u32; 3], [u32; 2]);

pub fn leer_escena(ruta: &std::path::Path) -> Result<crate::modelo::EscenaIr, crate::FbxError> {
    use crate::modelo::{
        AnimIr, CanalIr, EscenaIr, MaterialIr, MallaIr, NodoIr, PielIr, PrimitivaIr,
    };

    let ruta_texto = ruta.to_str().ok_or_else(|| crate::FbxError::Lectura {
        ruta: ruta.display().to_string(),
        motivo: "la ruta del archivo FBX no es UTF-8 válido".to_string(),
    })?;

    let opciones = ufbx::LoadOpts {
        target_axes: ufbx::CoordinateAxes {
            right: ufbx::CoordinateAxis::PositiveX,
            up: ufbx::CoordinateAxis::PositiveY,
            front: ufbx::CoordinateAxis::PositiveZ,
        },
        generate_missing_normals: true,
        load_external_files: true,
        ..ufbx::LoadOpts::default()
    };

    let escena = ufbx::load_file(ruta_texto, opciones).map_err(|error| {
        crate::FbxError::Lectura {
            ruta: ruta.display().to_string(),
            motivo: format!("{:?}", error),
        }
    })?;
    let escena: &ufbx::Scene = &escena;

    let mut avisos = Vec::new();

    // ufbx aplica la conversión de unidades como escala del nodo raíz,
    // pero el motor hornea la global solo en los modelos sin piel: la
    // escala se hornea AQUÍ (posiciones, traslaciones de nodo y de
    // animación, y la traslación de las inversas de unión), como hace
    // Blender al exportar, y el resultado queda en metros.
    let escala_unidades = {
        let original = escena.settings.original_unit_meters;
        if original > 0.0 && original.is_finite() {
            original as f32
        } else {
            1.0
        }
    };
    if (escala_unidades - 1.0).abs() > 1.0e-6 {
        avisos.push(format!(
            "la fuente viene en unidades de {} m; sus medidas se convierten a metros",
            escena.settings.original_unit_meters
        ));
    }

    let indices_nodos: HashMap<u32, usize> = escena
        .nodes
        .iter()
        .enumerate()
        .map(|(indice, nodo)| (nodo.as_ref().element.element_id, indice))
        .collect();
    let indices_materiales: HashMap<u32, usize> = escena
        .materials
        .iter()
        .enumerate()
        .map(|(indice, material)| (material.as_ref().element.element_id, indice))
        .collect();

    // Primero se reúne una numeración única de huesos para toda la escena.
    let mut huesos = Vec::<u32>::new();
    let mut posiciones_huesos = HashMap::<u32, usize>::new();
    let mut inversas = Vec::<[f32; 16]>::new();
    let mut clusters_por_malla = HashMap::<u32, Vec<Option<usize>>>::new();
    let mut huesos_avisados = HashSet::<u32>::new();

    for malla_ref in escena.meshes.iter() {
        let malla = malla_ref.as_ref();
        let id_malla = malla.element.element_id;

        if malla.skin_deformers.len() > 1 {
            avisos.push(format!(
                "la malla «{}» tiene varios deformadores de piel; solo se usará el primero",
                &*malla.element.name
            ));
        }

        let Some(deformador_ref) = malla.skin_deformers.first() else {
            continue;
        };
        let deformador = deformador_ref.as_ref();
        let mut mapa_local = Vec::with_capacity(deformador.clusters.len());

        for cluster_ref in deformador.clusters.iter() {
            let cluster = cluster_ref.as_ref();
            let Some(hueso_ref) = cluster.bone_node.as_ref() else {
                avisos.push(format!(
                    "se ignora un cluster sin nodo hueso en la malla «{}»",
                    &*malla.element.name
                ));
                mapa_local.push(None);
                continue;
            };

            let id_hueso = hueso_ref.as_ref().element.element_id;
            if let Some(posicion) = posiciones_huesos.get(&id_hueso).copied() {
                // El mismo hueso en otra malla puede traer otra inversa
                // (su espacio de geometría es otro). La piel del motor es
                // única: manda la primera y la diferencia no se esconde.
                let mut inversa = matriz_column_major(&cluster.geometry_to_bone);
                inversa[12] *= escala_unidades;
                inversa[13] *= escala_unidades;
                inversa[14] *= escala_unidades;
                let guardada = inversas[posicion];
                let difiere = inversa
                    .iter()
                    .zip(guardada.iter())
                    .any(|(a, b)| (a - b).abs() > 1.0e-5);
                if difiere && huesos_avisados.insert(id_hueso) {
                    avisos.push(format!(
                        "el hueso «{}» tiene inversas de unión distintas en varias mallas; se usa la de la primera",
                        &*hueso_ref.as_ref().element.name
                    ));
                }
            }
            let posicion = match posiciones_huesos.get(&id_hueso).copied() {
                Some(posicion) => posicion,
                None => {
                    if huesos.len() >= MAX_JOINTS {
                        return Err(crate::FbxError::Lectura {
                            ruta: ruta.display().to_string(),
                            motivo: format!(
                                "la piel supera el máximo de {MAX_JOINTS} articulaciones"
                            ),
                        });
                    }
                    let posicion = huesos.len();
                    huesos.push(id_hueso);
                    posiciones_huesos.insert(id_hueso, posicion);
                    let mut inversa = matriz_column_major(&cluster.geometry_to_bone);
                    // La traslación de una matriz column-major vive en
                    // los índices 12, 13 y 14 (la cuarta columna).
                    inversa[12] *= escala_unidades;
                    inversa[13] *= escala_unidades;
                    inversa[14] *= escala_unidades;
                    inversas.push(inversa);
                    posicion
                }
            };
            mapa_local.push(Some(posicion));
        }

        clusters_por_malla.insert(id_malla, mapa_local);
    }

    // Las mallas se construyen antes que los nodos para conocer el filtrado final.
    let mut mallas_con_id = Vec::<(u32, MallaIr)>::new();

    for malla_ref in escena.meshes.iter() {
        let malla = malla_ref.as_ref();
        let id_malla = malla.element.element_id;
        let nombre_malla = malla.element.name.to_string();
        let deformador = malla.skin_deformers.first().map(|valor| valor.as_ref());
        let con_piel = deformador.is_some();

        if !malla.vertex_position.exists {
            avisos.push(format!(
                "se omite la malla «{nombre_malla}» porque no tiene posiciones"
            ));
            continue;
        }

        let mut nodos_usuarios = escena.nodes.iter().filter_map(|nodo_ref| {
            let nodo = nodo_ref.as_ref();
            let malla_nodo = nodo.mesh.as_ref()?;
            (malla_nodo.as_ref().element.element_id == id_malla).then_some(nodo)
        });

        let primer_usuario = nodos_usuarios.next();
        let mut matrices_usuarios = Vec::<ufbx::Matrix>::new();
        if let Some(nodo) = primer_usuario {
            if nodo.has_geometry_transform {
                matrices_usuarios.push(nodo.geometry_to_node);
            }
        }
        let mut transformadas_distintas = false;
        for nodo in nodos_usuarios {
            if nodo.has_geometry_transform {
                if let Some(primera) = matrices_usuarios.first() {
                    if !matrices_iguales(primera, &nodo.geometry_to_node) {
                        transformadas_distintas = true;
                    }
                }
            }
        }
        if !con_piel && transformadas_distintas {
            avisos.push(format!(
                "la malla «{nombre_malla}» se usa con transformadas de geometría distintas; se pliega la de su primer nodo"
            ));
        }

        let plegado = if con_piel {
            None
        } else {
            matrices_usuarios.first().copied()
        };
        let normales_invertibles = plegado
            .as_ref()
            .map(determinante_matriz_3)
            .map(|determinante| determinante.abs() > 1.0e-20)
            .unwrap_or(true);
        if plegado.is_some() && !normales_invertibles {
            avisos.push(format!(
                "la transformada de geometría de «{nombre_malla}» es singular; sus normales no se transforman"
            ));
        }

        let mut partes = Vec::<(Option<usize>, Vec<u32>)>::new();
        if malla.material_parts.is_empty() {
            let caras = (0..malla.faces.len() as u32).collect::<Vec<_>>();
            let material = material_de_parte(malla, caras.first().copied(), &indices_materiales);
            partes.push((material, caras));
        } else {
            for parte in malla.material_parts.iter() {
                let caras = parte.face_indices.iter().copied().collect::<Vec<_>>();
                if caras.is_empty() {
                    continue;
                }
                let material =
                    material_de_parte(malla, caras.first().copied(), &indices_materiales);
                partes.push((material, caras));
            }
        }

        let mut primitivas = Vec::<PrimitivaIr>::new();
        let mut esquinas = Vec::<u32>::new();

        for (material, caras) in partes {
            let mut primitiva = PrimitivaIr {
                posiciones: Vec::new(),
                normales: Vec::new(),
                uvs: Vec::new(),
                indices: Vec::new(),
                material,
                joints: Vec::new(),
                weights: Vec::new(),
            };
            let mut deduplicacion = HashMap::<ClaveEsquina, u32>::new();

            for indice_cara in caras {
                let Some(cara) = malla.faces.get(indice_cara as usize).copied() else {
                    avisos.push(format!(
                        "se ignora una cara fuera de rango en la malla «{nombre_malla}»"
                    ));
                    continue;
                };

                esquinas.clear();
                ufbx::triangulate_face_vec(&mut esquinas, malla, cara);

                for &esquina in &esquinas {
                    let indice_esquina = esquina as usize;
                    let Some(&vertice) = malla.vertex_indices.get(indice_esquina) else {
                        continue;
                    };
                    // La clave deduplica por los VALORES que se van a
                    // emitir, no por los índices de ufbx: ufbx guarda a
                    // veces un valor de normal por esquina (56.716
                    // valores para 56.716 esquinas en el X Bot) y con
                    // índices no se fusionaría nada; por valores el
                    // conteo coincide con el de Blender (28.374).
                    let clave = clave_esquina(malla, indice_esquina, plegado.as_ref(), normales_invertibles);

                    let indice_salida = match deduplicacion.get(&clave).copied() {
                        Some(indice) => indice,
                        None => {
                            let posicion = malla.vertex_position[indice_esquina];
                            let posicion = match plegado.as_ref() {
                                Some(matriz) => transformar_punto(matriz, &posicion),
                                None => vec3_f32(&posicion),
                            };
                            let posicion = [
                                posicion[0] * escala_unidades,
                                posicion[1] * escala_unidades,
                                posicion[2] * escala_unidades,
                            ];
                            let normal = if malla.vertex_normal.exists {
                                let normal = malla.vertex_normal[indice_esquina];
                                match plegado.as_ref() {
                                    Some(matriz) if normales_invertibles => {
                                        transformar_normal(matriz, &normal)
                                    }
                                    _ => normalizar(vec3_f32(&normal)),
                                }
                            } else {
                                [0.0, 0.0, 0.0]
                            };

                            let indice = primitiva.posiciones.len() as u32;
                            primitiva.posiciones.push(posicion);
                            primitiva.normales.push(normal);
                            if malla.vertex_uv.exists {
                                let uv = malla.vertex_uv[indice_esquina];
                                primitiva.uvs.push([uv.x as f32, 1.0 - uv.y as f32]);
                            }

                            if let Some(deformador) = deformador {
                                let mapa_local = clusters_por_malla.get(&id_malla);
                                let (joints, weights) = pesos_vertice(
                                    deformador,
                                    vertice,
                                    mapa_local,
                                );
                                primitiva.joints.push(joints);
                                primitiva.weights.push(weights);
                            }

                            deduplicacion.insert(clave, indice);
                            indice
                        }
                    };
                    primitiva.indices.push(indice_salida);
                }
            }

            if !primitiva.indices.is_empty() && !primitiva.posiciones.is_empty() {
                primitivas.push(primitiva);
            }
        }

        if primitivas.is_empty() {
            avisos.push(format!(
                "se omite la malla «{nombre_malla}» porque no produjo primitivas con índices"
            ));
            continue;
        }

        mallas_con_id.push((
            id_malla,
            MallaIr {
                nombre: nombre_malla,
                primitivas,
                con_piel,
            },
        ));
    }

    let indices_mallas: HashMap<u32, usize> = mallas_con_id
        .iter()
        .enumerate()
        .map(|(indice, (id, _))| (*id, indice))
        .collect();
    let mallas = mallas_con_id
        .into_iter()
        .map(|(_, malla)| malla)
        .collect::<Vec<_>>();

    let conjunto_huesos = huesos.iter().copied().collect::<HashSet<_>>();
    let mut nodos = Vec::<NodoIr>::with_capacity(escena.nodes.len());
    for nodo_ref in escena.nodes.iter() {
        let nodo = nodo_ref.as_ref();
        let id = nodo.element.element_id;
        let transformada = nodo.local_transform;
        let padre = nodo.parent.as_ref().and_then(|padre| {
            let id_padre = padre.as_ref().element.element_id;
            let indice = indices_nodos.get(&id_padre).copied();
            if indice.is_none() {
                avisos.push(format!(
                    "el nodo «{}» referencia un padre ausente",
                    &*nodo.element.name
                ));
            }
            indice
        });
        let malla = nodo.mesh.as_ref().and_then(|malla| {
            indices_mallas
                .get(&malla.as_ref().element.element_id)
                .copied()
        });

        let traslacion_nodo = vec3_f32(&transformada.translation);
        nodos.push(NodoIr {
            nombre: nodo.element.name.to_string(),
            padre,
            hijos: Vec::new(),
            traslacion: [
                traslacion_nodo[0] * escala_unidades,
                traslacion_nodo[1] * escala_unidades,
                traslacion_nodo[2] * escala_unidades,
            ],
            rotacion: [
                transformada.rotation.x as f32,
                transformada.rotation.y as f32,
                transformada.rotation.z as f32,
                transformada.rotation.w as f32,
            ],
            escala: vec3_f32(&transformada.scale),
            malla,
            es_articulacion: conjunto_huesos.contains(&id),
        });
    }
    for indice in 0..nodos.len() {
        if let Some(padre) = nodos[indice].padre {
            if let Some(nodo_padre) = nodos.get_mut(padre) {
                nodo_padre.hijos.push(indice);
            }
        }
    }

    let mut articulaciones = Vec::with_capacity(huesos.len());
    for id_hueso in &huesos {
        if let Some(indice) = indices_nodos.get(id_hueso).copied() {
            articulaciones.push(indice);
        } else {
            avisos.push(format!(
                "la piel referencia el nodo hueso con element_id {id_hueso}, ausente de la escena"
            ));
        }
    }
    let piel = if articulaciones.is_empty() {
        None
    } else if articulaciones.len() == inversas.len() {
        Some(PielIr {
            articulaciones,
            inversas,
        })
    } else {
        avisos.push("se descarta la piel porque faltan nodos hueso en la escena".to_string());
        None
    };

    let mut texturas = Vec::new();
    let mut texturas_resueltas = HashMap::<u32, Option<usize>>::new();
    let mut materiales = Vec::<MaterialIr>::with_capacity(escena.materials.len());

    for material_ref in escena.materials.iter() {
        let material = material_ref.as_ref();
        let usar_pbr = material.features.pbr.enabled;

        let (mapa_color, mapa_factor, mapa_emision, mapa_factor_emision) = if usar_pbr {
            (
                &material.pbr.base_color,
                &material.pbr.base_factor,
                &material.pbr.emission_color,
                &material.pbr.emission_factor,
            )
        } else {
            (
                &material.fbx.diffuse_color,
                &material.fbx.diffuse_factor,
                &material.fbx.emission_color,
                &material.fbx.emission_factor,
            )
        };

        let mut color_base = if mapa_color.has_value {
            vec4_f32(&mapa_color.value_vec4)
        } else {
            [1.0; 4]
        };
        if mapa_factor.has_value {
            // El factor (difuso o de color base) es un ESCALAR en ufbx:
            // vive en la unión del mapa como value_real, así que su vec4
            // se lee (f, 0, 0, 0). Multiplicar por componentes dejaba el
            // color en (r·f, 0, 0, 0) — materiales rojos sin verde ni
            // azul y con alfa 0 (defecto cazado en el test físico de F3
            // con el edificio Synty y el X Bot). Se escala solo el RGB;
            // el alfa viene del propio color.
            let factor = mapa_factor.value_vec4.x as f32;
            color_base[0] *= factor;
            color_base[1] *= factor;
            color_base[2] *= factor;
        }

        let textura_base = if mapa_color.texture_enabled {
            mapa_color.texture.as_ref().and_then(|textura| {
                resolver_textura(
                    textura.as_ref(),
                    ruta,
                    &mut texturas,
                    &mut texturas_resueltas,
                    &mut avisos,
                )
            })
        } else {
            None
        };

        let rugosidad = if material.pbr.roughness.has_value {
            material.pbr.roughness.value_vec4.x as f32
        } else {
            0.8
        }
        .clamp(0.0, 1.0);
        let metalico = if material.pbr.metalness.has_value {
            material.pbr.metalness.value_vec4.x as f32
        } else {
            0.0
        }
        .clamp(0.0, 1.0);

        let emision = if mapa_emision.has_value {
            let factor = if mapa_factor_emision.has_value {
                mapa_factor_emision.value_vec4.x as f32
            } else {
                1.0
            };
            [
                mapa_emision.value_vec4.x as f32 * factor,
                mapa_emision.value_vec4.y as f32 * factor,
                mapa_emision.value_vec4.z as f32 * factor,
            ]
        } else {
            [0.0; 3]
        };

        materiales.push(MaterialIr {
            nombre: material.element.name.to_string(),
            color_base,
            textura_base,
            metalico,
            rugosidad,
            emision,
            doble_cara: material.features.double_sided.enabled,
        });
    }

    let fps = if escena.settings.frames_per_second > 1.0 {
        escena.settings.frames_per_second
    } else {
        30.0
    };
    let mut animaciones = Vec::<AnimIr>::new();

    for (indice_stack, stack_ref) in escena.anim_stacks.iter().enumerate() {
        let stack = stack_ref.as_ref();
        if stack.time_end <= stack.time_begin {
            continue;
        }

        let mut ids_animados = HashSet::<u32>::new();
        for capa_ref in stack.layers.iter() {
            for propiedad in capa_ref.as_ref().anim_props.iter() {
                ids_animados.insert(propiedad.element.as_ref().element_id);
            }
        }

        let paso = 1.0 / fps;
        let mut instantes = vec![stack.time_begin];
        let mut instante = stack.time_begin;
        loop {
            instante += paso;
            if !instante.is_finite() || instante >= stack.time_end {
                break;
            }
            instantes.push(instante);
        }
        instantes.push(stack.time_end);

        let tiempos = instantes
            .iter()
            .map(|instante| (instante - stack.time_begin) as f32)
            .collect::<Vec<_>>();
        let mut canales = Vec::<CanalIr>::new();

        for nodo_ref in escena.nodes.iter() {
            let nodo = nodo_ref.as_ref();
            if !ids_animados.contains(&nodo.element.element_id) {
                continue;
            }
            let Some(indice_nodo) = indices_nodos.get(&nodo.element.element_id).copied() else {
                continue;
            };

            let mut traslaciones = Vec::with_capacity(instantes.len());
            let mut rotaciones = Vec::with_capacity(instantes.len());
            let mut escalas = Vec::with_capacity(instantes.len());
            for &instante in &instantes {
                let transformada =
                    ufbx::evaluate_transform(stack.anim.as_ref(), nodo, instante);
                let t = vec3_f32(&transformada.translation);
                traslaciones.push([
                    t[0] * escala_unidades,
                    t[1] * escala_unidades,
                    t[2] * escala_unidades,
                ]);
                rotaciones.push([
                    transformada.rotation.x as f32,
                    transformada.rotation.y as f32,
                    transformada.rotation.z as f32,
                    transformada.rotation.w as f32,
                ]);
                escalas.push(vec3_f32(&transformada.scale));
            }

            canales.push(CanalIr {
                nodo: indice_nodo,
                tiempos: tiempos.clone(),
                traslaciones: Some(traslaciones),
                rotaciones: Some(rotaciones),
                escalas: Some(escalas),
            });
        }

        if canales.is_empty() {
            continue;
        }
        let mut nombre = stack.element.name.to_string();
        if nombre.is_empty() {
            nombre = format!("Animación {}", indice_stack + 1);
        }
        animaciones.push(AnimIr { nombre, canales });
    }

    let nombre = ruta
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "escena".to_string());

    Ok(EscenaIr {
        nombre,
        nodos,
        mallas,
        materiales,
        texturas,
        piel,
        animaciones,
        avisos,
    })
}

fn material_de_parte(
    malla: &ufbx::Mesh,
    primera_cara: Option<u32>,
    indices_globales: &HashMap<u32, usize>,
) -> Option<usize> {
    let cara = primera_cara?;
    let material_local = *malla.face_material.get(cara as usize)? as usize;
    let material = malla.materials.get(material_local)?;
    indices_globales
        .get(&material.as_ref().element.element_id)
        .copied()
}

fn pesos_vertice(
    deformador: &ufbx::SkinDeformer,
    vertice: u32,
    mapa_local: Option<&Vec<Option<usize>>>,
) -> ([u16; 4], [f32; 4]) {
    let mut joints = [0_u16; 4];
    let mut weights = [0.0_f32; 4];
    let Some(mapa_local) = mapa_local else {
        weights[0] = 1.0;
        return (joints, weights);
    };
    let Some(datos) = deformador.vertices.get(vertice as usize) else {
        weights[0] = 1.0;
        return (joints, weights);
    };

    let inicio = datos.weight_begin as usize;
    let fin = inicio.saturating_add(datos.num_weights as usize);
    let mut pesos = Vec::<(usize, f64)>::new();
    for indice in inicio..fin {
        let Some(peso) = deformador.weights.get(indice) else {
            continue;
        };
        if !peso.weight.is_finite() || peso.weight <= 0.0 {
            continue;
        }
        let Some(Some(conjunto)) = mapa_local.get(peso.cluster_index as usize) else {
            continue;
        };
        pesos.push((*conjunto, peso.weight));
    }
    pesos.sort_by(|a, b| b.1.total_cmp(&a.1));
    pesos.truncate(4);

    let suma = pesos.iter().map(|(_, peso)| *peso).sum::<f64>();
    if suma <= 0.0 || !suma.is_finite() {
        if let Some(conjunto) = mapa_local.iter().flatten().next() {
            joints[0] = *conjunto as u16;
        }
        weights[0] = 1.0;
        return (joints, weights);
    }

    for (salida, (conjunto, peso)) in pesos.into_iter().enumerate() {
        joints[salida] = conjunto as u16;
        weights[salida] = (peso / suma) as f32;
    }
    (joints, weights)
}

fn resolver_textura(
    textura: &ufbx::Texture,
    ruta_fuente: &Path,
    texturas: &mut Vec<crate::modelo::TexturaIr>,
    resueltas: &mut HashMap<u32, Option<usize>>,
    avisos: &mut Vec<String>,
) -> Option<usize> {
    let id = textura.element.element_id;
    if let Some(resultado) = resueltas.get(&id) {
        return *resultado;
    }

    let nombre = if textura.element.name.is_empty() {
        if textura.filename.length == 0 {
            format!("textura_{id}")
        } else {
            textura.filename.to_string()
        }
    } else {
        textura.element.name.to_string()
    };

    // OJO: un Blob vacío de ufbx puede traer el puntero a null y su
    // Deref (from_raw_parts) es UB que el build de depuración caza:
    // se mira `size` antes de tocar los datos.
    let mut bytes = if textura.content.size > 0 {
        textura.content.to_vec()
    } else {
        Vec::new()
    };
    if bytes.is_empty() && textura.absolute_filename.length > 0 {
        if let Ok(leidos) = std::fs::read(&*textura.absolute_filename) {
            bytes = leidos;
        }
    }
    if bytes.is_empty() && textura.relative_filename.length > 0 {
        if let Some(directorio) = ruta_fuente.parent() {
            if let Ok(leidos) = std::fs::read(directorio.join(&*textura.relative_filename)) {
                bytes = leidos;
            }
        }
    }
    if bytes.is_empty() {
        avisos.push(format!("textura no encontrada: «{nombre}»"));
        resueltas.insert(id, None);
        return None;
    }

    let nombre_archivo = if textura.filename.length > 0 {
        &textura.filename
    } else if textura.relative_filename.length > 0 {
        &textura.relative_filename
    } else {
        &textura.absolute_filename
    };
    let mime = mime_textura(&bytes, nombre_archivo);
    let Some(mime) = mime else {
        avisos.push(format!(
            "se ignora la textura «{nombre}»: solo se admiten PNG y JPEG"
        ));
        resueltas.insert(id, None);
        return None;
    };

    let indice = texturas.len();
    texturas.push(crate::modelo::TexturaIr { nombre, bytes, mime });
    resueltas.insert(id, Some(indice));
    Some(indice)
}

fn mime_textura(bytes: &[u8], nombre: &str) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        return Some("image/jpeg");
    }
    match Path::new(nombre)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        _ => None,
    }
}

fn vec3_f32(valor: &ufbx::Vec3) -> [f32; 3] {
    [valor.x as f32, valor.y as f32, valor.z as f32]
}

fn vec4_f32(valor: &ufbx::Vec4) -> [f32; 4] {
    [
        valor.x as f32,
        valor.y as f32,
        valor.z as f32,
        valor.w as f32,
    ]
}

fn matriz_column_major(matriz: &ufbx::Matrix) -> [f32; 16] {
    [
        matriz.m00 as f32,
        matriz.m10 as f32,
        matriz.m20 as f32,
        0.0,
        matriz.m01 as f32,
        matriz.m11 as f32,
        matriz.m21 as f32,
        0.0,
        matriz.m02 as f32,
        matriz.m12 as f32,
        matriz.m22 as f32,
        0.0,
        matriz.m03 as f32,
        matriz.m13 as f32,
        matriz.m23 as f32,
        1.0,
    ]
}

fn matrices_iguales(a: &ufbx::Matrix, b: &ufbx::Matrix) -> bool {
    a.m00 == b.m00
        && a.m10 == b.m10
        && a.m20 == b.m20
        && a.m01 == b.m01
        && a.m11 == b.m11
        && a.m21 == b.m21
        && a.m02 == b.m02
        && a.m12 == b.m12
        && a.m22 == b.m22
        && a.m03 == b.m03
        && a.m13 == b.m13
        && a.m23 == b.m23
}

fn transformar_punto(matriz: &ufbx::Matrix, punto: &ufbx::Vec3) -> [f32; 3] {
    [
        (matriz.m00 * punto.x + matriz.m01 * punto.y + matriz.m02 * punto.z + matriz.m03)
            as f32,
        (matriz.m10 * punto.x + matriz.m11 * punto.y + matriz.m12 * punto.z + matriz.m13)
            as f32,
        (matriz.m20 * punto.x + matriz.m21 * punto.y + matriz.m22 * punto.z + matriz.m23)
            as f32,
    ]
}

fn determinante_matriz_3(m: &ufbx::Matrix) -> f64 {
    m.m00 * (m.m11 * m.m22 - m.m12 * m.m21)
        - m.m01 * (m.m10 * m.m22 - m.m12 * m.m20)
        + m.m02 * (m.m10 * m.m21 - m.m11 * m.m20)
}

fn transformar_normal(m: &ufbx::Matrix, normal: &ufbx::Vec3) -> [f32; 3] {
    let determinante = determinante_matriz_3(m);
    let c00 = m.m11 * m.m22 - m.m12 * m.m21;
    let c01 = m.m02 * m.m21 - m.m01 * m.m22;
    let c02 = m.m01 * m.m12 - m.m02 * m.m11;
    let c10 = m.m12 * m.m20 - m.m10 * m.m22;
    let c11 = m.m00 * m.m22 - m.m02 * m.m20;
    let c12 = m.m02 * m.m10 - m.m00 * m.m12;
    let c20 = m.m10 * m.m21 - m.m11 * m.m20;
    let c21 = m.m01 * m.m20 - m.m00 * m.m21;
    let c22 = m.m00 * m.m11 - m.m01 * m.m10;

    normalizar([
        ((c00 * normal.x + c01 * normal.y + c02 * normal.z) / determinante) as f32,
        ((c10 * normal.x + c11 * normal.y + c12 * normal.z) / determinante) as f32,
        ((c20 * normal.x + c21 * normal.y + c22 * normal.z) / determinante) as f32,
    ])
}

fn normalizar(normal: [f32; 3]) -> [f32; 3] {
    let longitud = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if longitud > 0.0 && longitud.is_finite() {
        [
            normal[0] / longitud,
            normal[1] / longitud,
            normal[2] / longitud,
        ]
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// Clave de deduplicación de una esquina: el vértice de la malla (que
/// fija la posición y los pesos) más los bits de la normal y la UV YA
/// transformadas, que son los valores que se emitirán.
fn clave_esquina(
    malla: &ufbx::Mesh,
    indice_esquina: usize,
    plegado: Option<&ufbx::Matrix>,
    normales_invertibles: bool,
) -> ClaveEsquina {
    let vertice = malla
        .vertex_indices
        .get(indice_esquina)
        .copied()
        .unwrap_or(u32::MAX);
    let normal = if malla.vertex_normal.exists {
        let n = malla.vertex_normal[indice_esquina];
        let v = match plegado {
            Some(matriz) if normales_invertibles => transformar_normal(matriz, &n),
            _ => normalizar(vec3_f32(&n)),
        };
        [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()]
    } else {
        [0, 0, 0]
    };
    let uv = if malla.vertex_uv.exists {
        let u = malla.vertex_uv[indice_esquina];
        [(u.x as f32).to_bits(), (1.0 - u.y as f32).to_bits()]
    } else {
        [u32::MAX, u32::MAX]
    };
    (vertice, normal, uv)
}
