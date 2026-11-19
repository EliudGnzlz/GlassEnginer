use glass_fbx::escritor::escribir_glb;
use glass_fbx::modelo::*;

const CUATERNION_IDENTIDAD: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const MATRIZ_IDENTIDAD: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
];

// PNG válido de 1x1 píxel.
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
    0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41,
    0x54, 0x78, 0x9C, 0x63, 0xFC, 0xCF, 0xC0, 0x50,
    0x0F, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xA9,
    0x8C, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn aprox(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-5, "se esperaba {b}, se obtuvo {a}");
}

fn comprobar_vec3(json: &[f32], esperado: [f32; 3]) {
    assert_eq!(json.len(), 3);
    aprox(json[0], esperado[0]);
    aprox(json[1], esperado[1]);
    aprox(json[2], esperado[2]);
}

#[test]
fn glb_estatico_con_material_y_textura() {
    let posiciones = vec![
        [-0.5, -0.5, 0.0],
        [0.5, -0.5, 0.0],
        [0.5, 0.5, 0.0],
        [-0.5, 0.5, 0.0],
    ];
    let normales = vec![[0.0, 0.0, 1.0]; 4];
    let uvs = vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];

    let escena = EscenaIr {
        nombre: "Estatica".to_string(),
        nodos: vec![NodoIr {
            nombre: "Raiz".to_string(),
            padre: None,
            hijos: vec![],
            traslacion: [0.0, 0.0, 0.0],
            rotacion: CUATERNION_IDENTIDAD,
            escala: [1.0, 1.0, 1.0],
            malla: Some(0),
            es_articulacion: false,
        }],
        mallas: vec![MallaIr {
            nombre: "Quad".to_string(),
            primitivas: vec![PrimitivaIr {
                posiciones,
                normales,
                uvs,
                indices,
                material: Some(0),
                joints: vec![],
                weights: vec![],
            }],
            con_piel: false,
        }],
        materiales: vec![MaterialIr {
            nombre: "Material".to_string(),
            color_base: [0.25, 0.5, 0.75, 1.0],
            textura_base: Some(0),
            metalico: 0.1,
            rugosidad: 0.4,
            emision: [0.0, 0.0, 0.0],
            doble_cara: false,
        }],
        texturas: vec![TexturaIr {
            nombre: "Textura".to_string(),
            bytes: PNG_1X1.to_vec(),
            mime: "image/png",
        }],
        piel: None,
        animaciones: vec![],
        avisos: vec![],
    };

    let bytes = escribir_glb(&escena);

    assert_eq!(&bytes[0..4], &b"glTF"[..]);
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(version, 2);

    let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
    let documento = &gltf.document;

    assert_eq!(documento.scenes().count(), 1);
    let escena_gltf = documento.scenes().next().unwrap();
    assert_eq!(escena_gltf.nodes().count(), 1);

    assert_eq!(documento.meshes().count(), 1);
    let malla = documento.meshes().next().unwrap();
    assert_eq!(malla.primitives().count(), 1);
    let primitiva = malla.primitives().next().unwrap();
    assert_eq!(primitiva.material().index(), Some(0));

    assert_eq!(documento.materials().count(), 1);
    let material = documento.materials().next().unwrap();
    let pbr = material.pbr_metallic_roughness();
    let color = pbr.base_color_factor();
    aprox(color[0], 0.25);
    aprox(color[1], 0.5);
    aprox(color[2], 0.75);
    aprox(color[3], 1.0);

    assert_eq!(documento.images().count(), 1);
    assert_eq!(documento.textures().count(), 1);

    let pos = primitiva.get(&gltf::Semantic::Positions).unwrap();
    assert_eq!(pos.count(), 4);

    let min = pos.min().unwrap();
    let min: Vec<f32> = min
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect();
    comprobar_vec3(&min, [-0.5, -0.5, 0.0]);

    let max = pos.max().unwrap();
    let max: Vec<f32> = max
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect();
    comprobar_vec3(&max, [0.5, 0.5, 0.0]);
}

#[test]
fn glb_con_piel_y_animacion() {
    let posiciones = vec![
        [-0.5, -0.5, 0.0],
        [0.5, -0.5, 0.0],
        [0.5, 0.5, 0.0],
        [-0.5, 0.5, 0.0],
    ];
    let normales = vec![[0.0, 0.0, 1.0]; 4];
    let indices = vec![0, 1, 2, 0, 2, 3];

    let escena = EscenaIr {
        nombre: "Piel".to_string(),
        nodos: vec![
            NodoIr {
                nombre: "Armature".to_string(),
                padre: None,
                hijos: vec![1, 3],
                traslacion: [0.0, 0.0, 0.0],
                rotacion: CUATERNION_IDENTIDAD,
                escala: [1.0, 1.0, 1.0],
                malla: None,
                es_articulacion: false,
            },
            NodoIr {
                nombre: "Hueso1".to_string(),
                padre: Some(0),
                hijos: vec![2],
                traslacion: [0.0, 0.0, 0.0],
                rotacion: CUATERNION_IDENTIDAD,
                escala: [1.0, 1.0, 1.0],
                malla: None,
                es_articulacion: true,
            },
            NodoIr {
                nombre: "Hueso2".to_string(),
                padre: Some(1),
                hijos: vec![],
                traslacion: [0.0, 0.0, 0.0],
                rotacion: CUATERNION_IDENTIDAD,
                escala: [1.0, 1.0, 1.0],
                malla: None,
                es_articulacion: true,
            },
            NodoIr {
                nombre: "Cuerpo".to_string(),
                padre: Some(0),
                hijos: vec![],
                traslacion: [0.0, 0.0, 0.0],
                rotacion: CUATERNION_IDENTIDAD,
                escala: [1.0, 1.0, 1.0],
                malla: Some(0),
                es_articulacion: false,
            },
        ],
        mallas: vec![MallaIr {
            nombre: "Cuerpo".to_string(),
            primitivas: vec![PrimitivaIr {
                posiciones,
                normales,
                uvs: vec![],
                indices,
                material: None,
                joints: vec![
                    [0, 0, 0, 0],
                    [1, 0, 0, 0],
                    [0, 0, 0, 0],
                    [1, 0, 0, 0],
                ],
                weights: vec![
                    [1.0, 0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0, 0.0],
                ],
            }],
            con_piel: true,
        }],
        materiales: vec![],
        texturas: vec![],
        piel: Some(PielIr {
            articulaciones: vec![1, 2],
            inversas: vec![MATRIZ_IDENTIDAD, MATRIZ_IDENTIDAD],
        }),
        animaciones: vec![AnimIr {
            nombre: "Andar".to_string(),
            canales: vec![CanalIr {
                nodo: 2,
                tiempos: vec![0.0, 0.5, 1.0],
                traslaciones: None,
                rotaciones: Some(vec![
                    CUATERNION_IDENTIDAD,
                    [0.0, 0.382_683_43, 0.0, 0.923_879_5],
                    [0.0, 0.707_106_77, 0.0, 0.707_106_77],
                ]),
                escalas: None,
            }],
        }],
        avisos: vec![],
    };

    let bytes = escribir_glb(&escena);
    let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
    let documento = &gltf.document;

    assert_eq!(documento.skins().count(), 1);
    let skin = documento.skins().next().unwrap();
    assert_eq!(skin.joints().count(), 2);

    let inversas = skin.inverse_bind_matrices().unwrap();
    assert_eq!(inversas.count(), 2);

    let nodo_cuerpo = documento.nodes().nth(3).unwrap();
    assert!(nodo_cuerpo.skin().is_some());

    let primitiva = documento
        .meshes()
        .next()
        .unwrap()
        .primitives()
        .next()
        .unwrap();
    assert!(primitiva.get(&gltf::Semantic::Joints(0)).is_some());
    assert!(primitiva.get(&gltf::Semantic::Weights(0)).is_some());

    assert_eq!(documento.animations().count(), 1);
    let animacion = documento.animations().next().unwrap();
    assert_eq!(animacion.channels().count(), 1);
    let canal = animacion.channels().next().unwrap();
    assert_eq!(canal.target().node().index(), 2);
    assert!(matches!(
        canal.target().property(),
        gltf::animation::Property::Rotation
    ));
}

#[test]
fn glb_sin_uvs_ni_material_no_rompe() {
    let escena = EscenaIr {
        nombre: "SinUvsNiMaterial".to_string(),
        nodos: vec![NodoIr {
            nombre: "Raiz".to_string(),
            padre: None,
            hijos: vec![],
            traslacion: [0.0, 0.0, 0.0],
            rotacion: CUATERNION_IDENTIDAD,
            escala: [1.0, 1.0, 1.0],
            malla: Some(0),
            es_articulacion: false,
        }],
        mallas: vec![MallaIr {
            nombre: "Triangulo".to_string(),
            primitivas: vec![PrimitivaIr {
                posiciones: vec![
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                ],
                normales: vec![
                    [0.0, 0.0, 1.0],
                    [0.0, 0.0, 1.0],
                    [0.0, 0.0, 1.0],
                ],
                uvs: vec![],
                indices: vec![0, 1, 2],
                material: None,
                joints: vec![],
                weights: vec![],
            }],
            con_piel: false,
        }],
        materiales: vec![],
        texturas: vec![],
        piel: None,
        animaciones: vec![],
        avisos: vec![],
    };

    let bytes = escribir_glb(&escena);
    let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
    let documento = &gltf.document;

    assert_eq!(documento.meshes().count(), 1);
    let primitiva = documento
        .meshes()
        .next()
        .unwrap()
        .primitives()
        .next()
        .unwrap();

    assert!(primitiva.get(&gltf::Semantic::TexCoords(0)).is_none());
    assert!(primitiva.material().index().is_none());
}