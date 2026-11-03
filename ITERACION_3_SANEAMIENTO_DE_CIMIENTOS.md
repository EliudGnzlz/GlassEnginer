# GlassEngine — Reporte de Iteración 3: Saneamiento de Cimientos (Contrato de Rotación, Iluminación en Espacio de Mundo y Honestidad de Herramientas)

**Fecha:** 2026-11-02
**Estado:** ✅ SANEAMIENTO TÉCNICO COMPLETO — 30 tests verdes, 0 fallos
**Commit Asociado:** `fix(engine): repair rotation FFI contract, real world-space lighting and honest MCP tooling`

---

## 1. Resumen Ejecutivo de la Iteración 3

La Iteración 2 entregó **capacidades**: player standalone, prefabs, `project.json`, barra de menús Unity. La Iteración 3 no añade ni una sola funcionalidad nueva. Hace algo distinto y, en este punto del proyecto, más urgente: **reparar los cimientos que estaban mal y que las capacidades de la Iteración 2 estaban tapando.**

Se corrigieron tres defectos de raíz, cada uno de los cuales invalidaba silenciosamente trabajo ya hecho:

1. **El contrato de rotación entre C# y Rust estaba roto.** No existía conversión alguna entre las dos representaciones que el motor usaba en paralelo, de modo que cualquier rotación que cruzara la frontera FFI se perdía o se corrompía. Toda la API de rotación que un desarrollador C# veía (gizmos, `Transform.rotation`, colliders orientados) era, en la práctica, mentira.
2. **La iluminación era geométricamente falsa.** El vertex shader trataba la posición **local** como si fuera de mundo y el fragment shader tenía el vector de vista **hardcodeado** a `(0,0,1)`. Las normales nunca salían del espacio local. Los objetos rotados se iluminaban mal y ningún objeto podía tener un especular coherente con la cámara.
3. **Dos herramientas MCP mentían.** `glass_capture_viewport` fabricaba un degradado sintético y lo presentaba como "Viewport 3D capturado". `glass_reload_shaders` anunciaba "recargados exitosamente" cuando sólo activaba un flag, con los shaders embebidos en el binario. Un agente de IA que consumiera estas herramientas tomaba decisiones sobre datos falsos.

Además se eliminó un archivo huérfano de **código muerto con respuestas falsas hardcodeadas** (`handlers.rs`) que ni siquiera estaba declarado como módulo, y se construyó por primera vez una **red de seguridad automatizada sobre los contratos de memoria** (12 tests nuevos que anclan offsets Rust↔GLSL y la matemática de rotación).

Métricas de la iteración:

| Métrica | Antes (Iter. 2) | Después (Iter. 3) |
|---|---|---|
| Tests automatizados en el workspace | 18 | **30** (0 fallos) |
| Representaciones de rotación sin conversión | 2 (Euler + Quat) | **1 contrato con conversión bidireccional** |
| Vector de vista en el fragment shader | constante `(0,0,1)` | **posición de cámara real** |
| Normales en el shader | espacio local | **espacio de mundo** |
| Push constants | 128 B (MVP + luz) | **208 B** (MVP + modelo + luz + material + color + cámara) |
| Guarda contra `maxPushConstantsSize` | inexistente | **error ruidoso en tiempo de ejecución** |
| Herramientas MCP que declaran su límite | 0 de 2 | **2 de 2** |
| Archivo huérfano de código muerto | `handlers.rs` (34 LOC) | **eliminado** |
| Líneas netas | — | **+299 / −89** en 12 archivos |

---

## 2. Qué Hizo Exactamente Esta Iteración

### 2.1 Defecto raíz nº1 — El contrato de rotación (el bug más grave del motor)

**El diagnóstico.** El motor usaba dos tipos distintos para el mismo concepto:

```rust
// crates/glass_core/src/scene.rs — camino de escena/editor
pub struct TransformData {
    pub position: [f32; 3],
    pub rotation: [f32; 3],   // <-- EULER EN GRADOS
    pub scale:    [f32; 3],
}

// crates/glass_core/src/types.rs — camino FFI hacia C#
#[repr(C)]
pub struct TransformFfi {
    pub position: [f32; 3],
    pub rotation: [f32; 4],   // <-- CUATERNIÓN [i,j,k,w]
    pub scale:    [f32; 3],
}
```

Una búsqueda exhaustiva confirmó que **no existía ni una sola función de conversión entre ambos** en todo el repositorio. Las consecuencias eran graves y silenciosas:
- El puente de scripting C# leía `rotation` como cuaternión desde un campo que contenía grados Euler: cualquier rotación no nula producía basura.
- La física recibía la orientación por una ruta y la devolvía por otra, perdiéndola en el viaje.
- Los gizmos de rotación del editor no orientaban el collider.

**La corrección.** Se creó `crates/glass_core/src/math.rs` (166 LOC) con la matemática de rotación completa y explícita:

| Función | Contrato |
|---|---|
| `euler_deg_to_quat([f32;3]) -> [f32;4]` | convención `M = Rz·Ry·Rx` (extrínseca XYZ / intrínseca ZYX) |
| `quat_to_euler_deg([f32;4]) -> [f32;3]` | inversa, con rama de gimbal lock |
| `quat_rotate_vec3`, `quat_mul`, `quat_normalize`, `quat_to_mat3` | utilidades derivadas |

La convención se **derivó del código existente** (la función `model_matrix_trs_euler` ya construía `M = Rz(γ)·Ry(β)·Rx(α)`) en lugar de imponerla, garantizando compatibilidad con las escenas ya serializadas. La extracción inversa resuelve:

```
y = asin(-M[2][0])
x = atan2(M[2][1], M[2][2])
z = atan2(M[1][0], M[0][0])
```

con rama explícita de gimbal lock (`|M[2][0]| ≥ 0.99999`) que fija `z = 0` preservando la orientación. `quat_normalize` devuelve la identidad `[0,0,0,1]` ante entradas no finitas o degeneradas (`len² < 1e-12`), de modo que un cuaternión corrupto procedente de C# **no puede** propagar NaN a la física o al render.

**Los sitios de frontera corregidos:**

1. `EngineSceneBridge` (lectura de escena hacia C#) → convierte Euler→quat al exportar.
2. Escritura de la física hacia la escena → `ent.transform.rotation = quat_to_euler_deg(tf.rotation)`.
3. Escritura de la escena hacia la física (modo edición) → `physics.set_transform(bh, pos, euler_deg_to_quat(ent.transform.rotation))`. Antes subía la **identidad hardcodeada**, de modo que rotar con el gizmo no orientaba nada.

Se documentó además la frontera de los sitios que **legítimamente** usan la identidad (creación de cuerpos del arnés `--test-*` en el origen sin rotación), para que no se confundan con el bug corregido.

### 2.2 Defecto raíz nº2 — El bug de la escala borrada

```rust
// ANTES (main.rs, escritura física -> escena)
ent.transform.rotation = glass_core::quat_to_euler_deg(tf.rotation);
ent.transform.scale    = tf.scale;   // <-- BUG
```

`glass_physics::sync_all_transforms()` devuelve el cuaternión real pero **hardcodea `scale: [1.0, 1.0, 1.0]`**, porque Rapier no modela escala. Copiar ese campo significaba que cualquier entidad cuyo cuerpo se volviera dinámico **perdía su escala del Inspector cada frame**. La línea se eliminó con un comentario explicativo: la escala es propiedad de la escena, no de Rapier.

Este bug es un buen recordatorio de por qué la Iteración 3 era necesaria: la Iteración 2 presumía de "escalar objetos en el Inspector", y escalar un objeto con física no funcionaba.

### 2.3 Defecto raíz nº3 — Iluminación en espacio local y vector de vista falso

Los shaders adolecían de dos errores encadenados:

```glsl
// mesh.vert — ANTES
v_world_pos = in_position;   // en realidad es ESPACIO LOCAL, no mundo
v_normal    = in_normal;     // normal en espacio LOCAL

// pbr.frag — ANTES
vec3 V = vec3(0.0, 0.0, 1.0);  // vector de vista INVENTADO
```

Consecuencias: un cubo rotado 45° se iluminaba como si no estuviera rotado; el término especular Cook-Torrance era geométricamente arbitrario; y como el vector de vista era constante, la iluminación **no cambiaba al mover la cámara**, que es exactamente lo que delata a un motor roto ante cualquiera que lo pruebe.

**La corrección** exigió ampliar el bloque `push_constant` porque el fragment shader necesita la matriz de modelo para reconstruir la posición de mundo (el `mvp` ya lleva la vista y la proyección aplicadas, así que la información no es recuperable desde él).

Bloque nuevo: **128 → 208 bytes**

| Offset | Campo | Tipo | Propósito |
|---|---|---|---|
| 0 | `mvp` | `mat4` (64 B) | posición en clip space |
| 64 | `model` | `mat4` (64 B) | posición de mundo + matriz normal |
| 128 | `light_dir` | `vec4` | xyz dirección, w ambiente |
| 144 | `light_color` | `vec4` | xyz color·intensidad, w has_light |
| 160 | `material_params` | `vec4` | metallic, roughness, ao, has_light |
| 176 | `object_color` | `vec4` | rgb albedo, a opacidad |
| 192 | `camera_pos` | `vec4` | xyz cámara en mundo |

Todos los offsets caen en múltiplos de 16 y no hay relleno, así que `bytemuck::Pod` sigue siendo válido. Se añadieron dos aserciones en tiempo de compilación (`size == 208`, `align == 4`).

Shaders corregidos:

```glsl
// mesh.vert / pbr.vert — DESPUÉS
v_world_pos = (u_push.model * vec4(in_position, 1.0)).xyz;
mat3 normal_matrix = transpose(inverse(mat3(u_push.model)));
v_normal = normal_matrix * in_normal;
gl_Position = u_push.mvp * vec4(in_position, 1.0);

// pbr.frag — DESPUÉS
vec3 V = normalize(u_push.camera_pos.xyz - v_world_pos);
```

La matriz normal `transpose(inverse(mat3(model)))` es la forma **correcta** para normales bajo transformaciones no uniformes (la escala no uniforme deforma las normales si se usa el modelo directamente). El vector de vista ahora es real y la iluminación responde a la cámara.

**Guarda de compatibilidad.** 208 B supera el mínimo garantizado por la especificación de Vulkan (128 B). Este equipo reporta **256 B** (`vulkaninfo`), así que funciona aquí, pero hardware Intel/Mesa puede reportar 128 y fallar. Se añadió una comprobación explícita en `context.rs`: se lee `physical_device_properties.limits.max_push_constants_size`, y si es insuficiente se emite `log::error!` y se devuelve el error tipado nuevo:

```rust
VulkanError::InsufficientPushConstantSpace { device, available, required }
```

Es decir: el motor **falla ruidosamente y explicando por qué**, en lugar de producir geometría deformada en silencio. `pipeline.rs` ahora dimensiona el rango con `MESH_PUSH_CONSTANTS_SIZE` en vez del `128` literal, y como `renderer.rs` construye **ambos** pipelines (`mesh_pipeline` y `pbr_pipeline`) a través de la misma `new_mesh_pipeline`, la corrección cubre todo el camino 3D.

### 2.4 Honestidad de las herramientas MCP

Dos herramientas del servidor MCP embebido entregaban resultados falsos:

**`glass_capture_viewport`** — cuando no había snapshot en la escena, generaba un degradado de 1280×720 y lo devolvía como `"Viewport 3D capturado"`, sin ninguna indicación de que era sintético. Un agente de IA podía concluir que el motor renderizaba correctamente basándose en un dibujo fabricado por la propia herramienta de captura. Ahora la respuesta distingue ambos casos y **declara explícitamente** el suyo:

```json
{ "synthetic": true,  "content": [ ... "ADVERTENCIA - imagen sintética: no había snapshot..." ] }
{ "synthetic": false, "content": [ ... "Snapshot real publicado por el motor." ] }
```

**`glass_reload_shaders`** — decía "Shaders SPIR-V recargados exitosamente en tiempo de ejecución". En realidad sólo ponía `shaders_reloaded_flag = true`: los `.spv` están **embebidos con `include_bytes!`** en el binario, así que no existe ninguna ruta que reconstruya el pipeline en caliente. Ahora responde `{"applied": false, "flag_set": true}` y el texto explica que hace falta recompilar `glass_renderer` y reiniciar.

También se actualizaron las **descripciones de esquema** de ambas herramientas, que es lo que un cliente MCP lee antes de invocarlas: la advertencia ahora llega *antes* de la llamada, no sólo después.

**Código muerto eliminado.** `crates/glass_mcp/src/handlers.rs` contenía un `McpRouter` que devolvía una jerarquía de escena hardcodeada (`"Main Camera"`, `"Directional Light"`) y un `entity_id: 3` inventado. No estaba declarado como módulo en `lib.rs`, por lo que **nunca se compilaba ni se ejecutaba**. Se verificó la ausencia total de referencias (0 coincidencias en todo el árbol) y se eliminó.

### 2.5 Red de seguridad: 12 tests nuevos sobre los contratos

La Iteración 3 introdujo contratos frágiles que antes no existían. Se blindaron con tests que fallan **antes** de que el fallo llegue a la pantalla:

**`math.rs` — 8 tests:** identidad, rotación de 90° en cada eje, ida y vuelta Euler→quat→Euler, gimbal lock a ±90° de cabeceo, normalización de cuaterniones degenerados (NaN/Inf/vector nulo → identidad), composición de rotaciones.

**`renderer.rs` — 4 tests que anclan el contrato Rust↔GLSL** (categoría que no existía en el proyecto):

1. `offsets_coinciden_con_glsl` — compara `offset_of!` real de cada campo contra los offsets documentados en los comentarios de los cuatro `.frag`/`.vert`. Si alguien reordena un campo en Rust sin tocar el GLSL, **falla aquí** en vez de producir geometría deformada silenciosa.
2. `tamano_y_alineacion_son_los_esperados` — 208 B, alineación 4, todos los offsets múltiplo de 16, ninguno desborda.
3. `default_es_identidad_y_new_solo_cambia_la_mvp` — un `model` por defecto con basura rompería la iluminación.
4. `bytemuck_serializa_exactamente_208_bytes` — verifica que la serialización real coincide con lo declarado al `pipeline_layout`.

Esto responde directamente a una de las críticas más fuertes de la auditoría de la Iteración 2: *"la frontera C#↔Rust no tiene contrato estable"*. Ahora la frontera **tiene tests**.

---

## 3. Verificación

Toda la iteración está verificada con evidencia ejecutada, no declarada:

```
$ cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.15s
    (0 errores, 0 advertencias)

$ cargo build --release -p glass_app
    Finished `release` profile [optimized] target(s) in 13.96s

$ cargo test --workspace
    TOTAL TESTS VERDES: 30      Fallos detectados: 0
    glass_core      18   glass_mcp       3   glass_renderer  4
    glass_audio      3   glass_physics   1   glass_scripting 1

$ dotnet build managed/GlassEngine.Core/GlassEngine.Core.csproj
    Compilación correcta.  0 Advertencia(s)  0 Errores

$ glslc  (los 4 shaders con push constants)
    mesh.vert.spv 2128 B   mesh.frag.spv 2256 B
    pbr.vert.spv  2472 B   pbr.frag.spv  8368 B
```

**Validación cruzada automatizada del layout** (script Python que parsea el bloque `push_constant` de cada GLSL y suma bytes):

```
  pbr.vert     push_constant = 208 bytes   [OK]
  pbr.frag     push_constant = 208 bytes   [OK]
  mesh.vert    push_constant = 208 bytes   [OK]
  mesh.frag    push_constant = 208 bytes   [OK]
```

Cuatro shaders independientes declaran exactamente 208 bytes, coincidiendo con la aserción de Rust y con los offsets anclados por los tests.

---

## 4. Análisis del Estado Completo del Proyecto (Dictamen Independiente — Kimi K3.1 Pro)

Consultado vía MCP `mirai` (`ask_mirai`, modelo `kimi-k3.1-pro`), con el resumen de la iteración y las preguntas de auditoría.

### 4.1 ¿Ataca causas raíz o son parches cosméticos?

> *"Sí ataca causas raíz reales: el defecto de rotación era un contrato FFI roto (dos representaciones sin conversión), la escala un bug de sincronización física y la iluminación un error de pipeline (espacio local vs. mundo, vista hardcodeada). Son fixes de fundamento, no cosméticos; **pero no añaden capacidades, sólo hacen funcionar lo que ya existía, así que es saneamiento, no avance de producto.**"*

El dictamen es correcto y conviene asumirlo sin matices: **la Iteración 3 no mejora el producto, mejora la ingeniería.** La distinción importa porque significa que la posición competitiva del motor es la misma que tras la Iteración 2, pero ahora las afirmaciones que el motor hace sobre sí mismo son verificables.

### 4.2 ¿Cambia el veredicto de "motor de prototipo técnico, MVP/pre-alfa"?

> *"No cambia el veredicto: sigue siendo pre-alfa/MVP técnico. No sube frente a Flax/Godot/Unity/Unreal/Bevy porque **carece de jerarquía, texturas, sombras, build standalone y ECS**; la honestidad de herramientas es ética, no funcionalidad. Mejora la calidad de ingeniería interna, no la competitividad externa."*

Punto clave: se enumera **jerarquía** en primer lugar, coincidiendo con la recomendación del siguiente paso.

### 4.3 ¿Cuál es EL siguiente pendiente más crítico?

> *"La jerarquía de escena (transform padre-hijo). Es prerequisito para animación esquelética, composición de objetos, prefabs e instanciación; **sin ella el grafo plano no escala y bloquea features de alto nivel.** El stub de `SceneManager.LoadScene` también bloquea flujo real, pero la jerarquía es más fundacional."*

Esto confirma la hoja de ruta: **ITERACIÓN 4 = jerarquía de transformaciones real.**

### 4.4 Riesgos nuevos introducidos por la Iteración 3

> *"Push constants de 208 B superan el mínimo Vulkan garantizado (128 B) y pueden fallar en hardware Intel; la guarda evita crash pero **deshabilita iluminación avanzada en esos equipos, fragmentando compatibilidad.** Además, usar Euler como fuente de verdad con gimbal lock introduce discontinuidades en interpolación, y **la convención `M=Rz·Ry·Rx` debe mantenerse idéntica en C# y Rust o corrompe rotaciones.**"*

Tres riesgos legítimos, y el tercero es especialmente importante porque la Iteración 3 **creó** un contrato que antes simplemente no existía (estaba roto en todas partes, así que no había nada que mantener sincronizado). Ahora sí hay algo que puede desincronizarse: la convención Euler. Debe quedar explícitamente documentada en la capa C#.

El segundo riesgo (Euler como fuente de verdad, gimbal lock) ya se reconoció internamente al elegir la corrección: convertir quat→Euler mantiene `TransformData` consistente y es estable salvo en gimbal lock, donde Z se fija a 0 preservando la orientación. **La corrección arquitectónicamente superior —un `rotation_quat: Option<[f32;4]>` autoritativo o un componente `Transform` dedicado— queda diferida a la iteración de jerarquía**, donde el transform se va a rediseñar de todos modos.

### 4.5 Nota global

> **4/10.** *"Saneamiento técnico sólido, pero sigue siendo un prototipo pre-alfa sin las capacidades mínimas de un motor usable."*

Sube de ~3/10 (implícito tras la Iteración 2) a **4/10**: el punto ganado corresponde exclusivamente a que el motor ya no miente sobre lo que hace.

---

## 5. Conclusión y Siguiente Iteración

La Iteración 3 no hizo crecer el motor; hizo que **lo que ya había sea verdad**. Tres defectos de raíz corregidos (contrato de rotación, escala borrada, iluminación en espacio local con vista falsa), dos herramientas MCP que ahora declaran sus límites, un archivo de código muerto eliminado, y por primera vez una red de tests que protege los contratos de memoria entre Rust, GLSL y C#.

El veredicto externo es duro pero preciso: **sigue siendo pre-alfa, sigue sin ganar a nadie, y el saneamiento no es avance de producto.** Pero la crítica más repetida durante dos iteraciones —"la frontera C#↔Rust no tiene contrato estable"— ya no es cierta: hay 30 tests que lo verifican.

**ITERACIÓN 4** atacará lo que el auditor señala unánimemente como el bloqueante más fundacional: la **jerarquía de transformaciones real** (matrices de mundo cacheadas, resolución topológica padre→hijo), junto con la reescritura del `Transform` que arrastra el riesgo de gimbal lock detectado aquí, y el trabajo de paridad Unity en el panel de jerarquía y menús contextuales.

---

## Anexo A — Archivos Modificados

| Archivo | Δ | Contenido |
|---|---|---|
| `crates/glass_core/src/math.rs` | **NUEVO** (166) | matemática de rotación completa, 8 tests |
| `crates/glass_core/src/lib.rs` | +4 | declaración de módulo y re-exports |
| `crates/glass_app/src/main.rs` | +61/−? | 3 sitios de frontera; eliminación del bug de escala |
| `crates/glass_renderer/src/renderer.rs` | +137 | struct 208 B, `Default`, `new`, aserciones, 4 tests de contrato |
| `crates/glass_renderer/src/context.rs` | +36 | guarda `maxPushConstantsSize` + variante de error |
| `crates/glass_renderer/src/pipeline.rs` | +1/−1 | `.size(MESH_PUSH_CONSTANTS_SIZE)` en vez de `128` |
| `crates/glass_mcp/src/server.rs` | +44/−? | honestidad de 2 herramientas + descripciones de esquema |
| `crates/glass_mcp/src/handlers.rs` | **ELIMINADO** (34) | código muerto huérfano con respuestas falsas |
| `assets/shaders/mesh.vert` | +16/−? | matriz normal, posición de mundo |
| `assets/shaders/mesh.frag` | +12/−? | bloque de 208 B |
| `assets/shaders/pbr.vert` | +24/−? | matriz normal, posición de mundo |
| `assets/shaders/pbr.frag` | +18/−? | vector de vista real |
| **Total** | **+299 / −89** | 12 archivos |

## Anexo B — Cifras del Proyecto Tras la Iteración 3

```
LOC Rust:            13 823   (glass_editor 4 417 · glass_app 2 541 ·
                               glass_renderer 2 457 · glass_core 1 513 ·
                               glass_mcp 1 019 · glass_audio 646 ·
                               glass_physics 558 · glass_scripting 443)
LOC C# gestionado:    2 028
Tests automatizados:     30   (0 fallos)
Crates:                   8
Shaders GLSL:             6   (4 con push constants de 208 B)
```
