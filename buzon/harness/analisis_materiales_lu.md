# INFORME — Análisis de materiales: el documento de Unity contra GlassEnginer hoy

- **Fecha:** 2026-11-27 · **Rama revisada:** `lu/v4-autorado` @ `d839708` (solo lectura; sin cambios de código).
- **Documento de referencia:** `~/workspace/user/files/Analisis_Completo_Materiales_Unity_0_56k3.md` (análisis exhaustivo de materiales en Unity 6 / URP Lit: creación, Inspector, variantes, variables y drawers, serialización `.mat`, runtime y mapeo a motor Rust+Vulkan+C#).
- **Alcance:** análisis adelantado de la tarea de materiales ordenada por el usuario. La fase de arreglar/crear espera a que termine el frente UI/Canvas; este informe deja el terreno medido y un plan propuesto.
- **Método:** lectura completa del documento y levantamiento del código real del motor, con evidencia citada como `archivo:línea` en cada afirmación.

---

## 1. Resumen ejecutivo

**Estado en una línea:** GlassEnginer tiene un sistema de materiales PBR **funcional pero mínimo** («Materiales v1»): un asset JSON con **9 propiedades fijas** sobre un único shader PBR propio, sin mapas más allá de la textura base, sin variantes, sin editor de asset y sin acceso desde C#; en paralelo convive un segundo sistema, los **materiales de grafo** (shader graph con propiedades declaradas y presets), que sí tiene propiedades abiertas pero sin metadatos de UI.

Cobertura frente al documento (detalle en §3):

| Bloque del documento | Estado |
| :--- | :--- |
| §1 Vías de creación | Parcial: crear desde Project sí; desde shader, variantes y `new Material` en runtime, no |
| §2 Surface Options | Casi todo falta: solo hay `transparent` (bool); sin blending modes, sin cull, sin alpha clip, sin receive shadows |
| §2 Surface Inputs | Base + emisivo + tiling sí; faltan normal, metálico/roughness por mapa, height, occlusion por mapa, detail |
| §2 Advanced Options | Falta (sin render queue ni sorting priority ni toggles; y en esta rama tampoco hay instancing en el pase PBR, §3.4) |
| §2 Preview 3D del material | No hay preview dedicado de material |
| §3 Material Variants | No existe (hay precedente reutilizable: variantes de prefab) |
| §4 Variables y drawers | En grafos: propiedades sin metadatos (ni label, ni rango, ni HDR); en el PBR: nada, es fijo |
| §5 Serialización | JSON propio razonable, pero referencia por nombre (sin GUID) y sin versión de formato |
| §6 Runtime C# | Nada: no existe `UnityEngine.Material` ni `Renderer.material/sharedMaterial` para materiales visuales |
| §7 Mapeo propuesto por el documento | Archivo estructurado sí; UI por reflexión no; StorageBuffer bindless no (push constants); variantes aplanadas no |

---

## 2. Cómo son HOY los materiales en GlassEnginer (con evidencia)

### 2.1 Conviven dos sistemas

1. **Material PBR fijo** (`MaterialData`): un asset por material, sobre el shader PBR del motor. Es el sistema «de producción» para mallas 3D.
2. **Materiales de grafo**: un shader graph (`.graph.json`) con propiedades declaradas; los valores viven por entidad o en presets. Es el sistema abierto/extensible (toon, agua, fuego…), pero no es un «material» como asset reutilizable con nombre: la entidad referencia el grafo por `MeshData.shader = "graph:<nombre>"`.

### 2.2 El asset: `MaterialData` y `.material.json`

`crates/glass_core/src/scene.rs:1172-1208` — doc del propio código: «Activo de material estilo Unity (… materiales 3D, bloque «Materiales v1»): vive en `assets/materials/<nombre>.material.json` y una entidad lo referencia con `MeshData.material`. Sustituye por completo a los parámetros por defecto (y a los del glTF) de la entidad».

Las **9 propiedades completas** de `MaterialData` (el asset no tiene más; el eje «qué shader» no vive en el material sino en `MeshData.shader`, §2.1):

| Campo | Tipo | Default | Equivalente Unity |
| :--- | :--- | :--- | :--- |
| `albedo` | `[f32;4]` | `[1,1,1,1]` | `_BaseColor` |
| `texture` | ruta `String` | `""` (blanco 1×1) | `_BaseMap` |
| `metallic` | `f32` | `0.0` | `_Metallic` (solo escalar) |
| `roughness` | `f32` | `0.45` | inverso de `_Smoothness` (solo escalar) |
| `emissive` | `[f32;3]` | `[0,0,0]` | `_EmissionColor` |
| `emissive_intensity` | `f32` | `0.0` | intensidad HDR (alimenta el bloom) |
| `tiling` | `[f32;2]` | `[1,1]` | `_BaseMap_ST` (scale) |
| `offset` | `[f32;2]` | `[0,0]` | `_BaseMap_ST` (offset) |
| `transparent` | `bool` | `false` | Surface Type (binario) |

No hay: referencia a shader (el PBR es implícito y único), nombre interno (manda el nombre de archivo), padre/variante, render queue, cull, alpha clip, mapas secundarios, keywords ni flags. Assets reales en el repo: `assets/materials/{cristal,damero,emisor,espejo,mate,oro}.material.json` (6).

La entidad lo referencia por **nombre** en `MeshData.material` (`crates/glass_core/src/scene.rs:110-114`); `MeshData` (`scene.rs:93-120`) además trae `color [f32;3]`, `wireframe`, `shader String`, `model`, `object` y `shader_properties: HashMap<String, serde_json::Value>` (valores de grafo por entidad).

### 2.3 Serialización

JSON plano con `serde` y `#[serde(default)]` por campo: un archivo viejo sin un campo nuevo carga con el default (tolerancia buena para evolucionar el formato). Sin campo de versión de formato, sin GUID: **la identidad del material es su nombre de archivo** — renombrar el archivo rompe las referencias de las escenas que lo nombran. La carga en runtime es bajo demanda desde las raíces del proyecto a un `HashMap<String, MaterialData>` (`crates/glass_app/src/main.rs:2259`, `:6683-6768`, parse en `:6705`).

### 2.4 Camino de render (GPU)

- Los datos del material viajan en **push constants por draw**: `assets/shaders/pbr.frag:9-25` — `material_params` (x=metallic, y=roughness, z=ao escalar, w=has_light) y `emissive` (rgb + intensidad en w). **No** hay `StorageBuffer<GpuMaterialData>` ni índice de material por instancia (lo que el §7 del documento propone como destino bindless).
- El `ao` (z) se rellena con **1.0 fijo** en todos los sitios de dibujo (`crates/glass_app/src/main.rs:7084`, `:7099`, `:7182`, `:7196`): el canal existe en el shader pero hoy no modula nada — no hay oclusión configurable, ni por mapa ni por escalar, y `MaterialData` ni siquiera tiene campo para ello.
- **Una sola textura de material**: `base_color_tex` (set 0, binding 0, `pbr.frag:25`). Sin normal map (el shader usa la normal interpolada del vértice; `pbr.frag:164-170` calcula además una normal geométrica por derivadas), sin mapa metálico/roughness, sin mapa de oclusión (AO es un escalar), sin height/parallax, sin detail.
- El PBR en sí es completo en iluminación: GGX (`pbr.frag:124`), GeometrySmith (`:149`), IBL por cubemap (set 6), shadow maps (set 2), lightmaps (sets 4/5) y luces puntuales (set 3). El hueco no es la luz: son los **datos de superficie** que el material puede aportar.
- Pipelines: opaco y transparente (`crates/glass_renderer/src/pipeline.rs:110-134` y `:198`/`:226-345`). **Cull siempre `NONE`** (`pipeline.rs:110` y `:315`): todo se dibuja a doble cara, no existe Render Face. Transparente = blend `SRC_ALPHA / ONE_MINUS_SRC_ALPHA` y sin escritura de profundidad (`pipeline.rs:331-345`); la selección del pipeline transparente se hace por el bool (`crates/glass_app/src/main.rs:7241-7247`) — los materiales de grafo usan otro mecanismo, el archivo marcador de §2.5.
- **Ordenación de draws**: clave `(transparente al final, nombre de shader, índice de textura)` con orden estable, pensada para agrupar enlaces de pipeline/descriptores (`main.rs:7777-7794`). El «shader» de la clave es el nombre del pipeline/grafo del draw (p. ej. `material_transparent` o el nombre del grafo), no un campo del material. No hay término de distancia: los transparentes **no** se ordenan back-to-front como en Unity, ni existe sorting priority/render queue por material.
- Los modelos glTF traen sus propios materiales por rango (`AssetCache.materials` / `primary_materials`, `main.rs:7133-7195`); el asset `.material.json` de la entidad los sustituye por completo.

### 2.5 Materiales de grafo (el segundo sistema)

- `ShaderGraph { properties: Vec<PropertyDef> }` (`crates/glass_core/src/shader_graph.rs:432`, `:448`); `PropertyDef { name, kind, default }` (`:374-378`) con `PropertyKind { Float, Color, Vec2, Vec3, Vec4, Texture2D }` (`:340-347`). Las propiedades se compilan a un UBO (16 B por propiedad no-textura, `:554`, `:1310-1311`).
- **Sin metadatos de Inspector**: `PropertyDef` no tiene etiqueta visible, rango min/max, flag HDR, header ni modo slider/enum/toggle — el equivalente a los drawers de Unity (§4.1 del documento) no existe ni para grafos.
- **Presets**: `assets/shader_graphs/<grafo>.<preset>.preset.json` = `{name, graph, properties{…}}` (p. ej. `toon_demo.suave.preset.json`), con módulo `presets` en `glass_core::shader_graph` y panel de grafos en el editor (`crates/glass_editor/src/panels/shader_graph.rs:16`, `:141`). Un preset es lo más parecido a un «material de grafo» reutilizable, pero no se asigna como asset a la entidad: se aplica copiando valores a sus `shader_properties` (tool MCP `glass_shader_preset_apply`, §2.8).
- Transparencia de grafo por archivo marcador `assets/shaders/graph_<nombre>.transparent` (`main.rs:2398-2401`, `:3956-3961`) — un mecanismo por convención, fuera del asset.
- `alpha_clip` existe solo como **convención de nombre** en `shader_properties` de grafos (leído en `main.rs:7512`), igual que `outline_width`/`outline_color` (`:7678-7681`): no es una propiedad del sistema de materiales PBR.

### 2.6 Editor: crear sí, editar el asset no

- **Crear**: el panel Project crea `<assets>/materials/<Nombre>.material.json` con `MaterialData::default()` serializado (`crates/glass_editor/src/panels/project.rs:331-346`). En el Project, el archivo se muestra con icono 🖌 y **swatch del color albedo** con caché (`project.rs:1023`, `:1234`, `:2429`).
- **Asignar**: en el Inspector de la entidad, el material es un **campo de texto libre** donde se escribe el nombre, con un aviso de si el asset existe y un botón «➕ Crear material con valores por defecto» que escribe el JSON **a mano**, con un comentario que afirma que `glass_editor` no depende de `serde_json` — afirmación desactualizada: el crate sí lo declara (`crates/glass_editor/Cargo.toml:27`) y `project.rs` serializa con él; es una inconsistencia local del editor, no una limitación real — `crates/glass_editor/src/panels/inspector.rs:2170-2195`. El shader se muestra como etiqueta de solo lectura (`Shader: {}`).
- **Editar el asset**: no hay formulario de edición para `.material.json`. El contraste interno es fuerte: el **material físico** (`PhysicsMaterial`) sí tiene editor completo en el Inspector — copia editable, serialización y guardado (`inspector.rs:1478`, `:1590`, `:1655-1680`) y campo-objeto con desplegable para asignarlo (`campo_material_physmat`, `inspector.rs:892`). El patrón de la casa existe; el material visual no lo tiene.
- **Preview**: hay panel de miniatura de modelos (`crates/glass_editor/src/panels/miniatura_modelo.rs`), pero no un preview 3D dedicado del material (esfera con luz rotable, canales) como el bloque 6 del documento.

### 2.7 Fachada C#: los scripts no pueden tocar materiales visuales

- `managed/GlassEngine.Core/UnityEngine.cs` **no contiene** clase `Material` ni `Renderer`/`MeshRenderer`, y `RuntimeBridge.cs` no exporta delegates de material visual. Alcance de la búsqueda (reproducible): grep de `class Material`, `MeshRenderer`, `MaterialPropertyBlock` y `SetColor`/`SetFloat`/`SetTexture` sobre `managed/**/*.cs` — los únicos resultados son los del material físico y `LightSetColorDelegate` (luces, `Interop/NativeDelegates.cs:165`). Un script C# no puede leer, crear ni modificar el material visual de una entidad en runtime: ninguno de los tres mecanismos de Unity (§6 del documento) existe — ni `.material` con clonación, ni `sharedMaterial`, ni `MaterialPropertyBlock`.
- El patrón sí existe en la casa para física, con las dos semánticas de Unity bien separadas: `Collider.sharedMaterial` (el asset compartido, sin clonar) y `CharacterController.material` (**clona al primer acceso** y trabaja sobre la copia) — `managed/GlassEngine.Core/Physics/Collider.cs:15-36`, `CharacterController.cs:47-91`, `PhysicsMaterial.cs`/`PhysicMaterial.cs`.

### 2.8 MCP

Tools existentes (`crates/glass_mcp/src/server.rs`): `glass_list_materials` (`:826`, impl. `:4596`), `glass_set_entity_material` (`:834`, impl. `:4636` — asigna por nombre), `glass_material_property_set` (`:1102`, impl. `:1981` — escribe propiedades **de grafo** en la entidad), `glass_shader_preset_apply` (`:1168`, impl. `:2294`), `glass_shader_graph_validate` (`:1114`, impl. `:2020`) y la consulta del esquema de propiedades de un grafo (`:1093`). **No** hay tool que cree o edite el asset `.material.json` ni que lea/escriba las propiedades PBR de un material.

### 2.9 Importación de modelos

`MaterialIr` del importador FBX (`crates/glass_fbx/src/modelo.rs:81-91`): `nombre`, `color_base [f32;4]`, `textura_base Option<usize>`, `metalico`, `rugosidad`, `emision [f32;3]`, `doble_cara bool`. Es decir, la importación ya reduce todo material a un conjunto parecido al de `MaterialData` (y pierde por el camino mapas de normales y demás canales del archivo original). Detalle: `doble_cara` existe en el IR pero **no tiene campo en `MaterialData`**; en la práctica el pipeline no recorta caras nunca (§2.4), así que todo rinde doble cara siempre.

### 2.10 Precedente reutilizable: variantes de prefab

El motor ya resolvió herencia con overrides para prefabs: `variant_of: Option<String>` y `variant_overrides: Option<PrefabOverrides>` (`crates/glass_core/src/prefab.rs:17-20`), con resolución de la cadena base+diff al cargar (`prefab.rs:132-135`) y detección de ciclos. Es el patrón natural a reutilizar si se hacen variantes de material.

---

## 3. Comparación punto por punto (documento vs motor)

Leyenda: ✅ tenemos · 🟡 a medias · ❌ falta · 🔀 distinto (equivalente por otra vía).

### 3.1 §1 del documento — Qué es un material y sus 5 vías de creación

| Vía Unity | GlassEnginer | Estado |
| :--- | :--- | :--- |
| 1. Create > Material en Project | `crear_material` en el panel Project, con defaults | ✅ |
| 2. Crear desde un shader/grafo | No: el grafo se asigna escribiendo `graph:<nombre>` en `MeshData.shader`; no se crea un asset de material desde él | ❌ |
| 3. Material Variant | No existe herencia de materiales | ❌ |
| 4. Extracción desde modelos | El importador reduce a `MaterialIr` horneado en el modelo; no hay extracción a `.material.json` editables | 🟡 |
| 5. `new Material(...)` en runtime | Sin clase `Material` en C# | ❌ |
| Shader compartido por cientos de materiales | El PBR es único e implícito: todos los `.material.json` usan el mismo shader; los grafos son el «segundo shader» por entidad | 🔀 |

### 3.2 §2 del documento — Inspector: Surface Options

| Propiedad URP/Lit | GlassEnginer | Estado |
| :--- | :--- | :--- |
| Workflow Mode (Metallic/Specular) | Metallic ✅ (GGX + F0 mezclado por metallic); Specular ❌ (sin parámetro especular) | 🟡 |
| Surface Type (Opaque/Transparent) | `transparent: bool` | 🟡 |
| Blending Mode (Alpha/Premultiply/Additive/Multiply) | Solo Alpha normal; sin modos | ❌ |
| Preserve Specular (transparente) | No | ❌ |
| Render Face (Front/Back/Both) | Siempre doble cara (`CullMode NONE` en ambos pipelines) | ❌ |
| Alpha Clipping + Threshold | Solo convención `alpha_clip` en grafos; nada en el material PBR | ❌ |
| Receive Shadows | Sin opción por material: ningún campo de `MaterialData` lo controla y el shader muestrea el shadow map sin consultar flags del material | ❌ |

### 3.3 §2 del documento — Surface Inputs y Detail Inputs

| Propiedad URP/Lit | GlassEnginer | Estado |
| :--- | :--- | :--- |
| Base Map + Base Color | `texture` + `albedo` | ✅ |
| Metallic Map / Specular Map | Solo escalar `metallic` | ❌ |
| Smoothness (+ fuente por canal) | Solo escalar `roughness` (concepto inverso) | 🟡 |
| Normal Map + intensidad | No (normal de vértice/geométrica) | ❌ |
| Height Map (parallax) | No | ❌ |
| Occlusion Map | El canal `ao` del shader se alimenta con 1.0 fijo (`main.rs:7084`): sin mapa y sin escalar configurable | ❌ |
| Emission + color HDR + intensidad | `emissive` + `emissive_intensity` (alimenta bloom) | ✅ |
| GI de emisión (Realtime/Baked/None) | No | ❌ |
| Tiling y Offset | `tiling` + `offset`, una sola pareja (solo hay una textura) | ✅ |
| Detail Inputs (mask, detail base/normal, tiling 2º) | Nada | ❌ |

### 3.4 §2 del documento — Advanced Options y Preview

| Propiedad | GlassEnginer | Estado |
| :--- | :--- | :--- |
| Specular Highlights (toggle) | No | ❌ |
| Environment Reflections (toggle) | No (IBL siempre que exista cubemap) | ❌ |
| Sorting Priority / Render Queue | No; ordenación por (transparente, shader, textura), sin distancia ni prioridad | ❌ |
| GPU Instancing | En esta rama, no: los draws del pase PBR van con `instance_count = 1` y no hay `gl_InstanceIndex` ni entrada de vértice por instancia en los shaders del pase (verificado en `crates/glass_renderer/src/renderer.rs` y `assets/shaders/pbr.vert`). El bloque de render instanciado de Rayo vive en `main` y llegará a esta línea al integrar; además, con push constants por draw, un draw instanciado compartiría un único material | ❌ (en esta rama) |
| Preview 3D del material en el Inspector | No hay (sí miniaturas de modelo en Project) | ❌ |
| Cabecera: selector de Shader, Edit, Reset, menú ⋮ | El shader es etiqueta de solo lectura; sin reset ni menú del asset | ❌ |

### 3.5 §3 del documento — Material Variants

Todo falta para materiales: sin padre, sin propagación, sin indicador de override, sin revert/apply, sin `Lock in children`. El motor sí tiene el patrón resuelto en prefabs (`variant_of`/`variant_overrides`, §2.10) y un pariente débil en los presets de grafo (valores copiados, sin vínculo vivo). Estado: ❌ con precedente reutilizable.

### 3.6 §4 del documento — Variables, drawers y ShaderGUI

| Mecanismo Unity | GlassEnginer | Estado |
| :--- | :--- | :--- |
| Bloque `Properties` que define el Inspector | El PBR no declara propiedades: son los 9 campos fijos de `MaterialData`. Los grafos sí declaran (`PropertyDef`) | 🟡 |
| Drawers/atributos (`[Range]`, `[HDR]`, `[Toggle]`, `[Enum]`, `[Header]`, `[Normal]`, `[NoScaleOffset]`…) | `PropertyDef` = nombre + tipo + default; sin ningún metadato de presentación | ❌ |
| Blackboard de Shader Graph (Exposed, Reference, Mode) | El editor de grafos edita nodos y propiedades; `Exposed` equivale a estar en `properties`; sin Mode/rangos | 🟡 |
| `CustomEditor`/`ShaderGUI` (UI a medida por shader) | No hay UI de material que personalizar: ni siquiera hay formulario del asset (§2.6) | ❌ |
| UI automática por reflexión SPIR-V (propuesta §7 del doc) | Sin reflexión: búsqueda de `spirv`/`naga`/`rspirv` en `Cargo.toml` y `crates/*/Cargo.toml` sin resultados; la compilación de shaders es por GLSL (fuentes `.glsl`/generadas por el compilador de grafos) a `.spv` con glslangValidator, sin paso de reflexión | ❌ |

### 3.7 §5 del documento — Serialización `.mat`

| Aspecto UnityYAML | GlassEnginer | Estado |
| :--- | :--- | :--- |
| Archivo de texto con propiedades tipadas | JSON con campos tipados y defaults por campo | ✅ |
| Referencia al shader por GUID | Sin shader (implícito) y sin GUID: identidad = nombre de archivo | 🔀 |
| `m_Parent` (variante) | No | ❌ |
| Keywords válidas/inválidas guardadas | No hay keywords | ❌ |
| `m_CustomRenderQueue` | No | ❌ |
| `m_EnableInstancingVariants` | No es dato del material | ❌ |
| TexEnvs con scale/offset por textura | Una sola textura con tiling/offset globales | 🟡 |
| Versión de serialización | Sin campo de versión (la tolerancia la dan los `serde(default)`) | 🟡 |

### 3.8 §6 del documento — Runtime C# y rendimiento

| Mecanismo | GlassEnginer | Estado |
| :--- | :--- | :--- |
| `renderer.material` (clona) | No existe para visuales (sí para el material físico del collider) | ❌ |
| `renderer.sharedMaterial` | No existe para visuales | ❌ |
| `MaterialPropertyBlock` | No existe | ❌ |
| `Shader.PropertyToID` / acceso por id | No hay API de propiedades por nombre/id para scripts | ❌ |
| Edición en runtime por cualquier vía | Solo carga bajo demanda con caché en memoria al abrir la escena; un script no puede cambiar ni el albedo de una entidad | ❌ |

### 3.9 §7 del documento — El mapeo que el propio documento propone

| Propuesta del documento | GlassEnginer hoy | Estado |
| :--- | :--- | :--- |
| Archivo estructurado con `shader_guid`, `parent`, `render_queue`, `flags`, mapa tipado | JSON con 9 campos fijos; sin shader/parent/queue/flags | 🟡 |
| UI automática desde metadatos/reflexión | Ni metadatos ni reflexión | ❌ |
| `StorageBuffer<GpuMaterialData>` bindless + `material_index` por objeto | Push constants por draw (correcto y simple a esta escala, pero es otro diseño). Ojo terminológico: un SSBO indexado por objeto no es «bindless» (eso es *descriptor indexing* de texturas); son dos decisiones distintas (ver M8) | 🔀 |
| Variantes resueltas en el editor y aplanadas en el cooker | Sin variantes y sin cooker de materiales (carga directa) | ❌ |

---

## 4. Los huecos más grandes (ordenados por impacto)

1. **Sin mapas de superficie**: ni normal map, ni metallic/roughness por textura, ni occlusion por mapa. Es el hueco visual nº 1: el PBR ilumina bien pero «come» solo color base + dos escalares; cualquier asset importado pierde sus mapas (el IR ni los captura).
2. **Sin acceso desde C#**: ningún script puede tocar un material visual. Para un motor «estilo Unity» es el hueco funcional nº 1: efectos básicos (parpadeo al recibir daño, disolver, cambiar color por equipo) son imposibles sin tocar el motor.
3. **Sin editor del asset**: asignar es escribir el nombre a mano y editar es abrir el JSON en un editor de texto. El patrón para arreglarlo ya existe en la casa (editor del material físico).
4. **Sin variantes**: cada variación de un material obliga a duplicar el JSON completo y mantenerlo a mano; el patrón `variant_of` de prefabs está probado y sin usar aquí.
5. **Sin opciones de superficie**: cull (todo es doble cara siempre, con su coste — y el `doble_cara` que el importador captura en `MaterialIr` se pierde al no existir campo equivalente), alpha clipping, modos de blending, render queue/sorting y ordenación de transparentes sin distancia — un paquete de corrección visual y de coste.
6. **Sin metadatos de propiedades en grafos**: el Inspector de grafos no puede ofrecer sliders con rango, HDR ni agrupación; limita el autorado de materiales de grafo, que es el sistema extensible del motor.

Fuera de la lista, pero diagnosticado en el cuerpo: la identidad del asset por **nombre de archivo** (sin GUID, §2.3) queda como riesgo transversal del plan (§5) y como decisión §6.6.

---

## 5. Plan de trabajo propuesto (para ejecutar tras el frente UI/Canvas)

Paquetes ordenados; cada uno con el cierre habitual de la casa (pipeline local exacto del CI, físico por el camino real, barrido, consumo por proveedor y CIERRE en el buzón).

- **M0 — Contrato y decisiones** (sin código): cerrar con Rayo el alcance del formato v2 y las decisiones de §6. *Dependencias:* ninguna. *Salida:* lista cerrada de campos y de qué queda aparcado.
- **M1 — Formato `MaterialData` v2 + migración**: añadir los campos acordados (candidatos: `normal_texture` + `normal_scale`, `metallic_roughness_texture` o mapas separados, `occlusion_texture` + `occlusion_strength`, `alpha_clip` + `alpha_cutoff`, `cull`, `blend_mode` (Alpha/Premultiply/Additive/Multiply, con `preserve_specular` en transparente), `receive_shadows`, `render_queue`/offset de ordenación, flags `specular_highlights`/`env_reflections`, `emissive_texture`) todos con `serde(default)` para no romper los 6 assets ni ninguna escena; tests de carga de assets viejos. Incluye el frente importador: `MaterialIr` (`glass_fbx`) y el cargador glTF deben **capturar** los mapas nuevos y mapear `doble_cara` al nuevo `cull` — sin esto, M2 no tiene datos que mostrar en modelos importados. *Dependencias:* M0. *Nota:* `MaterialData` vive en `glass_core::scene` — tocarlo puede disparar el barrido completo; valorar moverlo a módulo propio dentro del mismo paquete.
- **M2 — GPU: shader PBR v2**: muestreo de los mapas nuevos (MR/AO y normales), alpha clip con `discard`, cull y blending por material (estrategia a decidir: pipelines por combinación acotada o estado dinámico), prioridad en la clave de ordenación y **decisión medida** sobre ordenación de transparentes back-to-front por distancia (implementar o descartar con números, no dejarlo en «valoración»). Tres frentes que el borrador no traía y la revisión cazó: (a) **marco tangente** — el formato de vértice no lleva tangentes (verificado: ni en `pbr.vert` ni en el layout de malla), así que el normal mapping exige marco cotangente por derivadas en el fragment (el shader ya calcula la normal geométrica así) o cambio de formato de vértice + importadores; (b) **pase de sombras/profundidad**: debe aplicar el mismo alpha clip al proyectar (precedente en la casa: `assets/shaders/depth_clip.frag`); (c) **presupuesto de push constants y descriptores**: el push actual ya declara offsets hasta 224+16 B (`pbr.frag:14-18`), por encima del mínimo garantizado de 128 B de Vulkan, y el set 0 tiene un solo binding — añadir mapas exige ampliar el set 0 o un set nuevo, y ese análisis va **aquí**, no en M8. Regenerar `.spv` y re-medir paridad con rigs físicos antes/después. *Dependencias:* M1. *Riesgo alto de paridad visual: es el mismo `pbr.frag` que el frente gráfico de Rayo acaba de tocar (G3).*
- **M3 — Editor de material**: formulario completo del asset siguiendo el patrón del editor de `PhysicsMaterial` (formulario manual con copia editable + guardar — es el patrón de la casa, no espera a M6), campo-objeto con desplegable en el Inspector en vez de texto libre, y preview simple (esfera) reutilizando la miniatura de modelos. *Dependencias:* M1; el preview fiel depende de M2 (dependencia dura solo para esa pieza).
- **M4 — Variantes de material**: `parent` + overrides en el asset, resolución de cadena al cargar (patrón `variant_of` de prefabs, con detección de ciclos) **en memoria**: se recorre la cadena una vez por carga y el resultado aplanado queda en la misma caché de materiales, sin reescribir assets en disco y sin coste por frame. UI mínima de override en el editor de M3. *Dependencias:* M1 para la resolución; M3 solo para la UI de override.
- **M5 — Fachada C# `Material` + `Renderer`/`MeshRenderer`**: crear las clases `Material` y `Renderer`/`MeshRenderer` (hoy no existen) expuestas a `GetComponent`, con acceso `sharedMaterial`/`material` con clonación al primer acceso (patrón ya probado en `Collider`/`PhysicMaterial`), `Set/Get` por nombre con ids precalculados y puente Rust para aplicar a la entidad viva. Incluye decidir el multimaterial por rango: hoy `MeshData.material` sustituye en bloque los materiales glTF por rango (§2.4); los arrays `materials`/`sharedMaterials` de Unity quedan como decisión §6.9. *Dependencias:* M1 (las variantes de M4 no son dependencia dura: la fachada trabaja sobre el asset ya resuelto). *Coordinación:* la fachada es territorio que el frente del importador de Rayo ha estado tocando (C0-C3); cerrar dueño y ventana con él.
- **M6 — Metadatos de propiedades**: extender `PropertyDef` (etiqueta, rango, HDR, header, toggle/enum donde aplique) y generar los controles del Inspector de grafos desde esos metadatos; si M1 deja las propiedades del PBR declaradas como datos, aplicar el mismo mecanismo. *Dependencias:* ninguna dura (beneficia a los grafos desde ya). Los controles de grafo son independientes del formulario del asset de M3 (patrón manual), así que M6 no rehace M3.
- **M7 — MCP de materiales**: tools para crear/editar el asset `.material.json` y leer/escribir sus propiedades, con validación (hoy un agente solo puede asignar y tocar propiedades de grafo). *Dependencias:* M1 (trabaja sobre el formato; no depende de M4).
- **M8 — Evaluación de buffers (sin compromiso)**: medir si los push constants por draw limitan cuando el render instanciado de `main` se integre en esta línea. Son **dos decisiones distintas** que no deben mezclarse: (a) un `StorageBuffer<GpuMaterialData>` indexado por objeto/instancia para los parámetros, como propone el §7 del documento, y (b) bindless real (*descriptor indexing*) para texturas. Solo si la medición lo pide se diseña (a); (b) queda fuera salvo necesidad demostrada. *Dependencias:* medición posterior a M2.

**Riesgos transversales:** (a) formato — `MaterialData` está en `glass_core::scene`; su cambio exige el barrido anti-regresión completo si toca formato de escena; (b) explosión de pipelines si cull/blend se resuelven por pipeline sin acotar combinaciones; (c) paridad visual — todo cambio en `pbr.frag` invalida doradas y exige re-medición física; (d) referencias por nombre — cualquier renombre de asset rompe escenas (valorar GUID o herramienta de renombrado seguro en M1/M3); (e) solape con el frente gráfico de Rayo (G1-G4 han tocado ambiente, post y el propio `pbr.frag` en G3): hace falta ventana de congelación del shader.

---

## 6. Decisiones a cerrar con Rayo antes de ejecutar

1. **Alcance del formato v2**: qué campos de la lista candidata de M1 entran en este bloque y cuáles se aparcan (la candidata con más impacto visual es el normal map; la de más impacto de sistema, el cull).
2. **¿Variantes (M4) en este bloque o en uno posterior?** El usuario pidió «rehacer» los materiales siguiendo el documento, donde las variantes son un bloque entero (§3).
3. **Dueño de la fachada C# (M5)**: ¿entra en este frente (Lu) o en el del importador/fachada (Rayo), que acaba de cerrar C0-C3 sobre la misma fachada?
4. **Ventana de congelación de `pbr.frag`** frente a su parte gráfica (G1-G4): quién lo toca y cuándo, para no pisar paridades.
5. **Ubicación de `MaterialData`**: ¿se queda en `glass_core::scene` o se mueve a un módulo `material` propio? Afecta al alcance del barrido de cada paquete.
6. **Identidad del asset**: ¿se mantiene la referencia por nombre o se introduce GUID con migración? (Unity usa GUID; el coste aquí es tocar escenas y el Project.)
7. **Reparto por proveedor** de los paquetes (patrón de orquestación de la casa) y si la revisión «peces» es por paquete o solo al cierre del bloque.
8. **La bifurcación de fondo** (la cazó la revisión «peces»): ¿`.material.json` gana referencia a shader y **unifica** PBR y grafos en un solo sistema de materiales, o **conviven** dos sistemas? Es previa al contrato de M0/M1, decide si existe la vía «crear desde shader/grafo» (§3.1) y define qué cubre exactamente la fachada de M5.
9. **Multimaterial por rango**: ¿la fachada y el formato soportan arrays de materiales por rango de malla (como `Renderer.materials` de Unity y como ya trae un glTF), o se mantiene la sustitución en bloque de `MeshData.material`?

---

## 7. Revisión «peces» del informe

Tres revisores de familias distintas, solo lectura, sobre el borrador (sin acceso al código: juzgaron también si la evidencia citada sostiene cada afirmación). Los hallazgos que se repiten entre peces se resuelven una sola vez. Toda corrección marcada APLICAR ya está incorporada en el texto de arriba; las verificaciones de código que las respaldan se hicieron sobre el clon tras la revisión.

### Pez 1 — Opus (`claude-opus-4-6`, vía MWAPI, freno `--max-razonamiento 25000`): 12 hallazgos

| # | Hallazgo (resumen) | Resolución |
| :--- | :--- | :--- |
| 1 | «9 propiedades, no hay más» contradice que `MeshData` sí tenga `shader`; y «sustituye a los del glTF» sin evidencia propia | **REBATIR en parte**: la frase del glTF es literal del doc comment citado (`scene.rs:1172-1176`). **APLICAR** la precisión: los 9 campos son los de `MaterialData` y el eje shader vive en `MeshData` (§2.2) |
| 2 | Receive Shadows y Cull se afirman con evidencia que solo prueba el pipeline | **APLICAR** la precisión de sombras (§3.2: ningún campo lo controla y el shader no consulta flags del material). **REBATIR** lo del cull: `pipeline.rs:110`/`:315` prueban exactamente el estado de cull afirmado |
| 3 | Workflow marcado 🟡 sin parámetro especular que lo justifique | **APLICAR**: veredicto dividido en la celda (Metallic ✅ / Specular ❌) |
| 4 | «Hay render instanciado» es más fuerte que su evidencia (ordenar draws ≠ instancing) | **APLICAR** — y la verificación posterior le dio la razón contra el borrador: en esta rama los draws van con `instance_count = 1`, sin `gl_InstanceIndex` ni entrada por instancia; el instanciado vive en `main`. Corregido en §1 y §3.4 |
| 5 | La negativa de C# («no existe Material») se apoya en «búsqueda sin resultados» sin alcance | **APLICAR**: §2.7 declara el alcance reproducible de la búsqueda |
| 6 | §2.3 y §3.8 se contradicen («bajo demanda» vs «recarga al cargar escena») | **APLICAR**: redacción unificada (§3.8) |
| 7 | «Sin crates de reflexión» es una negación sin evidencia; no se dice qué herramienta de shaders se usa | **APLICAR**: §3.6 declara la búsqueda en los manifiestos y que la compilación es por GLSL/glslang, sin reflexión SPIR-V |
| 8 | El 🔀 de §3.9 disfraza un ❌ (bindless) | **APLICAR** la precisión terminológica (§3.9 y M8); se mantiene 🔀 porque los push constants son un diseño válido y vigente, no una ausencia |
| 9 | M2 deja la ordenación por distancia como «valoración» en vez de requisito | **APLICAR**: M2 la convierte en decisión medida con entregable |
| 10 | Ciclo blando M2→M3 y solape M6/M3 sin declarar | **APLICAR**: dependencias declaradas en M3 (preview ← M2) y M6 (independiente del formulario manual de M3) |
| 11 | M4: el «aplanado sin coste» no dice si es en disco o en memoria | **APLICAR**: M4 especifica resolución en memoria al cargar, sin reescribir assets |
| 12 | §4 omite dos diagnósticos ya hechos: identidad por nombre y `doble_cara` perdido | **APLICAR**: `doble_cara` entra en el hueco 5 e identidad queda señalada al cierre de §4 |

### Pez 2 — DeepSeek (`deepseek-flash`, vía su helper): 8 hallazgos útiles

Circunstancia: el primer intento con el paquete completo murió como ya le pasó a Rayo una vez — 45.897 caracteres de razonamiento y **0 de contenido** (parcial, exit 3). Siguiendo la norma de la casa (achicar la demanda, nunca agrandarla), el reintento fue con paquete reducido (§1–§2) y entregó 8 hallazgos.

| # | Hallazgo (resumen) | Resolución |
| :--- | :--- | :--- |
| 1 | «El informe termina en §2.10: no hay plan» | **REBATIR**: artefacto del paquete reducido — solo recibió §1–§2; el informe completo trae §3–§8. **ANOTAR** la circunstancia (esta nota) |
| 2 | El `ao` de los push constants no corresponde a ningún campo de `MaterialData`: origen sin explicar | **APLICAR** — caza real: verificado en `main.rs:7084`/`:7099`/`:7182`/`:7196`, el `ao` se rellena con **1.0 fijo**. Añadido a §2.4 y corregido el veredicto de Occlusion en §3.3 (🟡 → ❌) |
| 3 | «Sin receive shadows» choca con que haya shadow maps | **APLICAR** (= Opus 2) |
| 4 | §2.7 confunde `sharedMaterial` (no clona) con `.material` (clona) | **APLICAR**: §2.7 reescrito separando las dos semánticas |
| 5 | §2.4 atribuye lo transparente solo al bool; los grafos usan marcador | **APLICAR**: cláusula en §2.4 que remite a §2.5 |
| 6 | Instancing afirmado sin evidencia | **APLICAR** (= Opus 4) |
| 7 | Evidencia negativa sin búsqueda reproducible | **APLICAR** (= Opus 5) |
| 8 | La clave de ordenación usa «nombre de shader» pero el material no tiene shader: incoherencia | **APLICAR** la precisión en §2.4: el nombre es el del pipeline/grafo del draw, no un campo del material |

### Pez 3 — Gemini (Antigravity `agy`, modo plan, proceso fresco): 12 hallazgos

| # | Hallazgo (resumen) | Resolución |
| :--- | :--- | :--- |
| 1 | Instancing 🔀 sin evidencia; con push constants por draw, un draw instanciado compartiría material | **APLICAR** (= Opus 4; su argumento técnico queda incorporado en §3.4) |
| 2 | M2 asume normales en espacio tangente sin verificar si existen tangentes | **APLICAR** — verificado: no hay tangentes en el formato de vértice ni en `pbr.vert`. M2 gana el frente de marco cotangente por derivadas o cambio de formato |
| 3 | Los importadores (`MaterialIr`, cargador glTF) quedan fuera del plan | **APLICAR**: M1 gana el sub-entregable de captura en el IR/glTF y el mapeo `doble_cara`→`cull` |
| 4 | Bifurcación MaterialData vs grafos sin resolver en el plan ni en §6 | **APLICAR**: nueva decisión §6.8, previa a M0/M1 (es la vía de escalado a Rayo que este informe usa) |
| 5 | Orden M3→M6 invertido: la UI por metadatos rehacería el editor | **APLICAR** la aclaración (= Opus 10): M3 sigue el patrón manual del physmat y M6 cubre los controles de grafo; no hay rehacer |
| 6 | M1/M2 omiten `blend_mode`, `preserve_specular`, `receive_shadows`; y el pase de sombras ignora el alpha clip | **APLICAR**: campos añadidos a M1 y el pase de profundidad/sombras entra en M2 (con el precedente `depth_clip.frag`) |
| 7 | Contradicción del `ao` escalar | **APLICAR** (= DeepSeek 2) |
| 8 | Presupuesto de push constants y bindings sin analizar; no debe esperar a M8 | **APLICAR**: el análisis de presupuesto y descriptores entra en M2 |
| 9 | Contradicción: `glass_editor` «no depende de serde_json» pero `project.rs` serializa con él | **APLICAR** — caza real: `crates/glass_editor/Cargo.toml:27` sí declara `serde_json`; el comentario de `inspector.rs` está desactualizado. Corregido en §2.6 |
| 10 | Dependencias mal acotadas (M3←M2, M4 no depende de M3, M5/M7) | **APLICAR** (= Opus 10/11): dependencias re-declaradas en M3, M4, M5 y M7 |
| 11 | M5 incompleto: faltan `Renderer`/`MeshRenderer`, `GetComponent` y multimaterial | **APLICAR**: M5 se amplía a la fachada completa y el multimaterial queda como decisión §6.9 |
| 12 | «Bindless» mal usado: SSBO indexado ≠ descriptor indexing | **APLICAR**: terminología corregida en §3.9 y M8 (dos decisiones separadas) |

**Balance:** 32 hallazgos (12 + 8 + 12). APLICAR 29 (varios compartidos entre peces), REBATIR 2 (Opus 1 en parte y DeepSeek 1), ESCALAR 0 como resolución suelta — la única cuestión que excede al informe, la bifurcación de sistemas, quedó **aplicada como decisión §6.8** para que la cierre Rayo. Sin re-auditoría: las correcciones son de precisión y alcance, no de criterio.

## 8. Consumo por proveedor

Medición por bytes enviados y caracteres recibidos (el `usage` de los gateways no es fiable, norma de la casa). La exploración y la redacción fueron locales, sin llamadas a proveedores.

| Proveedor | Uso | Prompt (bytes) | Razonamiento (chars) | Contenido (chars) |
| :--- | :--- | ---: | ---: | ---: |
| Claude Opus 4.6 (MWAPI) | Pez 1, paquete completo, freno 25.000 (no saltó) | 29.483 | 0 | 4.169 |
| DeepSeek flash | Pez 2, intento 1 (paquete completo): parcial exit 3, **0 útil** | 29.483 | 45.897 | 0 |
| DeepSeek flash | Pez 2, intento 2 (paquete reducido §1–§2): útil | 14.732 | 33.670 | 1.490 |
| Gemini (agy, modo plan) | Pez 3, paquete completo | ≈27.600 | sin desglose en esta ruta | ≈6.049 (salida total) |
| Kimi | Sin saldo en ambos lados: no usado | — | — | 0 |

**Totales útiles:** Opus 4.169 · DeepSeek 1.490 (con 79.567 de razonamiento entre ambos intentos) · Gemini ≈6.049 · Kimi 0.
