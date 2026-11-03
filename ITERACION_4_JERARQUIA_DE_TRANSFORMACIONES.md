# GlassEngine — Reporte de Iteración 4: Jerarquía de Transformaciones Real (Cuaternión Autoritativo, Menús Contextuales Unity y Cierre Ordenado de Señales)

**Fecha:** 2026-11-02
**Estado:** ✅ ITERACIÓN COMPLETA — 56 tests verdes, 0 fallos, 0 warnings
**Commit Asociado:** `feat(engine): real transform hierarchy with authoritative quaternion, Unity context menus and graceful signal shutdown`

---

## 1. Resumen Ejecutivo de la Iteración 4

La Iteración 3 saneó los cimientos: contrato de rotación FFI, iluminación en espacio de mundo y honestidad de las herramientas MCP. Su dictamen externo (Kimi K3.1 Pro) fue tajante sobre el siguiente paso: *"la jerarquía de escena (transform padre-hijo) es más fundacional que el stub de `SceneManager.LoadScene`"*.

La Iteración 4 implementa exactamente eso, y es la primera iteración que **añade una capacidad estructural** en lugar de reparar una existente:

1. **Jerarquía de transformaciones real.** El grafo `parent`/`children` que existía desde la Iteración 1 era puramente organizativo: los hijos se movían con su padre en el editor, pero cada entidad se componía a partir de su transform **local** como si fuera de mundo. Ahora `compute_world_matrices()` recorre el grafo en orden topológico padre→hijo, compone `world = parent_world · local` con cuaterniones y cachea el resultado por entidad. Render, física, picking, gizmos y bounding boxes consumen esa matriz mundial.
2. **Cuaternión autoritativo con encapsulación real.** En la Iteración 3 la conversión Euler↔cuaternión existía, pero ambos campos eran `pub`: nada impedía que un módulo escribiera `transform.rotation` directamente y desincronizara las dos representaciones. Ahora los campos de rotación son **privados** y toda mutación pasa por `set_rotation_euler` / `set_rotation_quat`, que mantienen ambas vistas coherentes por construcción. 28 puntos de escritura en 5 crates migrados.
3. **Compatibilidad de serialización sin migración.** `rotation_quat` se serializa con `#[serde(default)]`; un cuaternión `[0,0,0,0]` (nunca una orientación válida) se detecta y se reconstruye desde Euler, de modo que las escenas `.scene.json` anteriores a esta iteración cargan sin conversión alguna.
4. **Física sincronizada por completo.** La escritura de vuelta de Rapier3D ahora copia el cuaternión completo (antes solo posición y escala, dejando la rotación del cuerpo congelada en el editor).
5. **Menús contextuales estilo Unity** en Scene View (crear objeto 3D/luz/UI bajo el cursor con unprojection real a plano, enfocar selección, crear Main Camera), en Hierarchy (emparentar, desemparentar, agrupar, duplicar) y en el explorador de Proyecto, que dejó de ser una pestaña hardcodeada y ahora lista el disco real con `std::fs::read_dir`.
6. **Defecto de runtime diagnosticado y corregido de raíz.** El motor registraba `Error durante render_frame: A wait operation has not completed in the specified time` seguido de ~1881 errores de `logical device has been lost` al recibir SIGTERM. Causa raíz encontrada y eliminada (sección 2.6).

Métricas de la iteración:

| Métrica | Antes (Iter. 3) | Después (Iter. 4) |
|---|---|---|
| Tests automatizados en el workspace | 30 | **56** (0 fallos) |
| Composición de transform | local como mundo | **`world = parent_world · local` topológico** |
| Fuente de verdad de rotación | Euler `pub` + quat `pub` (podían divergir) | **campos privados, cuaternión autoritativo** |
| Escrituras directas a `transform.rotation` | 28 puntos en 5 crates | **0** (todas vía métodos) |
| Rotación escrita de vuelta por la física | solo posición+escala | **cuaternión completo** |
| `has_light` en el shader | 2 fuentes (duplicación) | **1 sola (`material_params.w`)** |
| Explorador de Proyecto | pestaña hardcodeada | **`std::fs::read_dir` real + menú contextual** |
| Menús contextuales de editor | 0 | **3 superficies (Scene View, Hierarchy, Proyecto)** |
| Cierre por SIGTERM | ~1881 errores + device lost | **0 errores, `Drop` con `device_wait_idle`** |
| Líneas netas | — | **+1416 / −238** en 19 archivos (+ `math.rs` nuevo, 803 LOC) |

---

## 2. Qué Hizo Exactamente Esta Iteración

### 2.1 La jerarquía de transformaciones real

**El diagnóstico.** El grafo existía pero no transformaba. `EntityData` tenía `parent: Option<u64>` y `children: Vec<u64>`, y el editor los usaba para arrastrar nodos en el panel Hierarchy, pero el momento de componer la matriz de modelo hacía esto:

```rust
// ANTES — cada entidad se componía sola, ignorando a su padre
let model = mat4_from_trs_euler(ent.transform.position, ent.transform.rotation, ent.transform.scale);
```

Un hijo arrastrado junto a su padre en el editor **no heredaba** su transform al renderizar: la jerarquía era decorativa. Peor aún, el editor y la física escribían posiciones de mundo directamente en el transform local, corrompiendo la semántica del grafo.

**La solución.** `SceneState::compute_world_matrices()` (crates/glass_core/src/scene.rs:753) resuelve el grafo completo:

```rust
// DESPUÉS — pasada topológica padre→hijo con cuaterniones
let mut pending: Vec<&EntityData> = self.entities.iter().collect();
while !pending.is_empty() && pending.len() < last_len {
    for ent in pending {
        let parent_world = match ent.parent {
            None => Some(mat4_identity()),
            Some(pid) => out.get(&pid).copied(),   // aún no resuelto → vuelta siguiente
        };
        if let Some(pw) = parent_world {
            let local = mat4_from_trs_quat(ent.transform.position,
                                           ent.transform.rotation_quat(),
                                           ent.transform.scale);
            out.insert(ent.id, mat4_mul(pw, local));
        } else {
            still_pending.push(ent);
        }
    }
    pending = still_pending;
}
// Lo que quede sin resolver es un ciclo o un padre inexistente:
// se renderiza como raíz (con warning) para que nada desaparezca.
```

Propiedades de diseño:

- **Orden topológico por rondas**: en cada vuelta se resuelven todas las entidades cuyo padre ya esté resuelto. Coste O(n·profundidad) en el peor caso, O(n) en el caso típico de árboles poco profundos.
- **Ciclos y huérfanos no rompen nada**: lo que no se resuelve se compone como raíz y emite un `log::warn!` explícito. Una escena corrupta degrada a grafo plano en lugar de desaparecer entidades o colgar el bucle.
- **Un solo punto de composición**: los tres sitios que componen matrices (raíz, multi-pasada y fallback de ciclo) usan `mat4_from_trs_quat`, así que no hay dos convenciones de rotación conviviendo.

**Consumidores migrados a espacio de mundo:**

| Consumidor | Antes | Ahora |
|---|---|---|
| Render (`main.rs`) | `mat4_from_trs_euler` local | `world_matrices.get(&id)` + factor 0.5 del mesh |
| Física (writeback Rapier) | posición+escala locales | `set_world_transform` con cuaternión completo |
| Física (edición/pausa) | transform local | `mat4_translation(world)` + `mat4_to_quat(world)` |
| Picking de entidades | transform local | `pick_entity_at_screen(..., &world_matrices, ...)` |
| Gizmos de cámara/luz | transform local | `mat4_translation(*world)` + `quat_to_euler_deg(mat4_to_quat(*world))` |
| Bounding box de selección | transform local | matriz mundial |

**La convención del factor 0.5.** El mesh `cube.glb` tiene vértices en −1..+1, así que el render aplica `model = world · scale(0.5)` **a la derecha**, en espacio local. El factor pertenece al mesh, no al transform: no se propaga a los hijos y no contamina la jerarquía. La física, en cambio, usa el cubo de medio lado como half-extent, que coincide con el render sin factor adicional.

### 2.2 Cuaternión autoritativo con encapsulación real

**El diagnóstico.** La Iteración 3 construyó la conversión Euler↔cuaternión, pero ambos campos seguían siendo `pub`. Cualquier módulo podía escribir `transform.rotation = [45.0, 0.0, 0.0]` y dejar el cuaternión almacenado obsoleto: la desincronización que la iteración anterior había cerrado por convención seguía abierta por tipado.

**La solución.** Los campos de rotación son ahora privados y la única vía de mutación son métodos que mantienen ambas vistas coherentes:

```rust
pub struct TransformData {
    pub position: [f32; 3],
    pub scale: [f32; 3],
    rotation: [f32; 3],              // derivado, para Inspector/gizmos
    #[serde(default)]
    rotation_quat: [f32; 4],         // AUTORITATIVO [i,j,k,w]
}

impl TransformData {
    pub fn rotation(&self) -> [f32; 3];                  // vista derivada
    pub fn rotation_quat(&self) -> [f32; 4];             // autoridad
    pub fn set_rotation_euler(&mut self, euler_deg: [f32; 3]);   // recalcula quat
    pub fn set_rotation_quat(&mut self, q: [f32; 4]);            // recalcula euler
    pub fn add_rotation_euler(&mut self, delta_deg: [f32; 3]);
}
```

Reglas del contrato:

- **El cuaternión es la autoridad.** `set_rotation_quat` normaliza, almacena y deriva Euler para las vistas. `set_rotation_euler` convierte a cuaternión y almacena ambos. No existe ruta de escritura que deje las dos vistas en desacuerdo.
- **`[0,0,0,0]` nunca es una orientación.** `rotation_quat()` lo detecta y reconstruye desde Euler. Esto es lo que hace que las escenas antiguas (sin campo `rotation_quat`) carguen sin migración: `#[serde(default)]` produce el cero, y el getter lo reconstruye al vuelo.
- **28 puntos de escritura migrados** en `glass_core`, `glass_app`, `glass_editor` (inspector, hierarchy, gizmo_3d), `glass_physics` y `glass_scripting`. El compilador fue el verificador: al privatizar los campos, cada acceso directo dejó de compilar hasta migrarse.

**Convención congelada y documentada.** `M = Rz(γ)·Ry(β)·Rx(α)` (extrínseco XYZ / intrínseco ZYX), idéntica en Rust y en C#. La extracción usa `y = asin(−M[2][0])`, `x = atan2(M[2][1], M[2][2])`, `z = atan2(M[1][0], M[0][0])`, con rama de gimbal lock (`|M[2][0]| ≥ 0.99999`) que fija `z = 0`. Los tests de `math.rs` anclan la convención para que un cambio accidental rompa la compilación de pruebas, no el runtime.

### 2.3 `math.rs`: la biblioteca matemática del motor

Archivo nuevo (803 LOC, 21 pruebas) que centraliza toda la matemática de transformaciones. Antes estaba dispersa entre `scene.rs` y `main.rs`; ahora es una crate interna re-exportada por `glass_core::lib.rs`:

| Función | Rol |
|---|---|
| `mat4_from_trs_euler` / `mat4_from_trs_quat` | composición TRS (las dos vistas del mismo contrato) |
| `mat4_mul`, `mat4_translation`, `mat4_scale`, `mat4_identity` | álgebra básica column-major |
| `mat4_to_quat` | extracción Shepperd con guardas `1.0e-8` |
| `euler_deg_to_quat` / `quat_to_euler_deg` / `quat_to_mat3` / `quat_mul` / `quat_normalize` / `quat_rotate_vec3` | álgebra de cuaterniones |
| `mat4_inverse_affine` | inversa afín (picking, unprojection); `None` si `|det| < 1e-12` |
| `mat4_transform_point` | transformación de punto (rayos de picking) |
| `mat4_scale` / `mat4_to_euler_deg` | descomposición para gizmos |

Las 21 pruebas anclan: redondeo de identidad, composición TRS contra matrices construidas a mano, ida y vuelta Euler→quat→Euler y quat→mat4→quat, gimbal lock en ±90°, inversa afín contra multiplicación, y el layout column-major (traslación en elementos 12–14).

### 2.4 Compatibilidad de serialización sin migración

`rotation_quat` se declara con `#[serde(default)]`. El flujo de carga de una escena antigua:

1. El JSON no tiene `rotation_quat` → serde lo inicializa a `[0,0,0,0]`.
2. La primera llamada a `rotation_quat()` detecta el cero y lo reconstruye desde `rotation` (Euler).
3. Al guardar, el cuaternión ya reconstruido se serializa junto al Euler.

Resultado: **cero scripts de migración, cero versionado de formato**, y las escenas de las iteraciones 1–3 siguen siendo válidas. Cuatro pruebas anclan el contrato (`transform_json_without_rotation_quat_still_loads`, `transform_json_round_trips_both_representations`, `rotation_quat_survives_a_script_round_trip`, `euler_setter_keeps_quaternion_in_sync`).

### 2.5 Física sincronizada por completo y `has_light` con una sola fuente

**Física.** El writeback de Rapier3D (`main.rs`, ~1099–1127) llamaba `set_world_transform(entity_id, tf.position, tf.rotation)` pero nunca copiaba `tf.scale`; y en edición/pausa se congelaban los cuerpos con `set_linear_velocity([0,0,0])` a partir del transform **local**. Ahora:

- El writeback escribe posición **y cuaternión** de mundo vía `set_world_transform`, que convierte a local cuando la entidad tiene padre.
- La congelación en edición/pausa precomputa `compute_world_matrices()` y alimenta la física con `mat4_translation(world)` + `mat4_to_quat(world)`, de modo que el estado físico coincide con lo que se ve en el viewport.

**`has_light`.** El shader leía la presencia de luz de dos sitios (`light_color.w` y `material_params.w`), una duplicación que la Iteración 3 dejó latente. Ahora `material_params.w` es la **única** fuente (`mesh.frag` documenta el contrato en un comentario), y Rust la deriva de `scene.main_light().is_some()`. `light_color.w` queda reservado y sin usar.

### 2.6 Defecto de runtime: el "device lost" fantasma (diagnóstico y corrección de raíz)

**El síntoma.** Al terminar el editor con SIGTERM (o con `timeout N`, que usa SIGTERM), el log registraba:

```
ERROR glass_app] Error durante render_frame: A wait operation has not completed in the specified time
```

seguido, en la variante con `timeout`, de ~1881 errores de `Vulkan driver error: The logical device has been lost`.

**La investigación.** Matriz de señales ejecutada 3 veces por señal sobre el binario real:

| Señal | Errores en log | Líneas de log |
|---|---|---|
| SIGTERM | **1 en 3/3 ejecuciones** | 59 |
| SIGINT | 0 | 58 |
| SIGHUP | 0 | 58 |
| SIGKILL | 0 | 58 |

El orden del log probó el mecanismo: la línea ERROR aparece ~5 s después del último INFO (`Script PlayerController instanciado exitosamente...`) y es la **última** línea; `Limpiando GlassEngineApp en orden estricto de dependencias...` (el `Drop`) nunca se ejecuta. Es decir: el proceso muere **a mitad de un frame en vuelo**.

**La causa raíz.** Las esperas de Vulkan (`wait_for_fences`, `queue_wait_idle`, `acquire_next_image`) usan `u64::MAX` y terminan en el syscall del kernel `drm_syncobj_timeline_wait` (verificado con `/proc/<pid>/wchan` durante una ejecución viva). Al recibir SIGTERM, el kernel interrumpe ese syscall y el driver lo traduce a `VK_TIMEOUT` — de ahí el mensaje "A wait operation has not completed in the specified time", que es el texto de `VK_TIMEOUT`, no un timeout real del motor. La acción por defecto de SIGTERM mata el proceso en el retorno a espacio de usuario, antes de que el frame termine; el dispositivo queda con trabajo en vuelo y la siguiente espera (o el drenaje del proceso) produce la cascada de `device lost`.

**La corrección.** Manejadores propios para SIGTERM/SIGINT/SIGHUP que solo hacen `store` sobre un `AtomicBool` (lo único async-signal-safe que se necesita), instalados **antes** de arrancar el bucle y **reinstalados tras inicializar CoreCLR** — el runtime .NET instala los suyos durante el arranque y pisa los nuestros, detalle que la primera versión de la corrección pasó por alto y que la verificación reveló. El bucle consulta la bandera en `about_to_wait` y en `RedrawRequested`; al detectarla sale por `event_loop.exit()`, lo que permite que `Drop` ejecute `device_wait_idle()` y destruya los recursos en orden estricto de dependencias.

**Verificación posterior:**

```
$ timeout 12 env DISPLAY=:0 ./target/release/glass_app
EXIT=124 (timeout, como antes)   ERRORES: 0   device lost: 0
[INFO] Limpiando GlassEngineApp en orden estricto de dependencias...
[INFO] Contexto Vulkan liberado correctamente.

$ ./target/release/glass_app --test-m7   EXIT=0  ERRORES=0  HITO 7 COMPLETADO
$ ./target/release/glass_app --test-m8   EXIT=0  ERRORES=0
```

**Riesgo latente declarado (dictamen externo, sección 4.2):** la lógica actual trata `VK_TIMEOUT` bajo bandera de apagado como benigno. Un `VK_TIMEOUT` real durante ejecución (GPU colgada, TDR) seguiría reportándose como error — correcto — pero no hay ruta de recuperación de dispositivo. Es una limitación conocida, no un defecto de esta corrección.

### 2.7 Editor estilo Unity: menús contextuales reales

**Scene View** (`viewport.rs::render_scene_tab`). Clic derecho sobre el viewport abre un menú anclado al cursor:

- **Crear bajo el cursor**: `gizmo_3d::screen_to_world_on_plane` unproyecta el clic (NDC cerca/lejos → `mat4_inverse_affine` → `mat4_transform_point` → intersección rayo-plano y=0) y muestra `⌖ Crear en (x, y, z)` con las coordenadas reales; sin intersección muestra `⌖ Crear en el origen`.
- **Submenús** `⬡ Objeto 3D` (Cubo/Esfera/Cápsula/Plano), `☼ Luz` (Direccional/Puntual), `🖼 UI` (Canvas/Texto/Botón) y `📦 Vacío`, todos vía `HierarchyPanel::instantiate_preset` sobre el estado de escena real, con selección automática del nuevo id y log `Creado por menú contextual del Scene View: #N`.
- **🎯 Enfocar seleccionado**: centra la cámara de escena en `mat4_translation(world)` de la entidad seleccionada y restablece la distancia a 8.0.
- **📷 Crear Main Camera**.

**Hierarchy** (`hierarchy.rs`). Menú por entidad con Mover Arriba/Abajo, Agrupar Selección, Crear Hijo Vacío, Desemparentar, `🔗 Emparentar con... ▾` (lista de candidatos), Duplicar, Eliminar y `➕ Añadir:`; menú de fondo vacío con `➕ Crear GameObject`; arrastrar-soltar con píldora fantasma y posiciones Before/Inside/After.

**Proyecto** (`console.rs`). La pestaña Project dejó de ser una lista hardcodeada y ahora es un explorador real: `ConsolePanel` recorre `assets/` con `std::fs::read_dir`, distingue archivo/carpeta, ordena y refresca bajo demanda (`🔄 Refrescar`, `⬆ Subir`), navega con doble clic, y ofrece menú contextual por entrada (`📂 Abrir carpeta`, `📋 Copiar ruta al portapapeles` vía `ui.ctx().copy_text`, `ℹ Mostrar en consola`, `🗑 Eliminar del disco` — esta última **guardada** para que sólo opere dentro de `assets/`) y menú de fondo (`🔄 Refrescar`, `➕ Nueva carpeta`).

---

## 3. Verificación

Toda la iteración está verificada con evidencia ejecutada, no declarada:

```
$ cargo check --workspace --all-targets
    0 errores, 0 advertencias

$ cargo build --release -p glass_app
    Finished `release` profile [optimized] target(s) in 5.15s

$ cargo test --workspace
    TOTAL TESTS VERDES: 56      Fallos detectados: 0
    glass_core      44   glass_renderer   4   glass_mcp       3
    glass_audio      3   glass_physics    1   glass_scripting 1

$ dotnet build managed/GlassEngine.Core/GlassEngine.Core.csproj
    Compilación correcta.  0 Advertencia(s)  0 Errores

$ glslc assets/shaders/mesh.frag -o assets/shaders/mesh.frag.spv
    mesh.frag.spv 2272 B  (has_light desde material_params.w, única fuente)

$ timeout 12 env DISPLAY=:0 ./target/release/glass_app
    ERRORES: 0 · device lost: 0 · cierre ordenado con device_wait_idle
```

Las 26 pruebas nuevas de esta iteración anclan: la biblioteca matemática completa (21 en `math.rs`), la sincronización Euler↔cuaternión y la compatibilidad de serialización (4 en `scene.rs`), y la conversión mundo→local de `set_world_transform` para hijos.

---

## 4. Análisis del Estado Completo del Proyecto (Dictamen Independiente — Kimi K3.1 Pro)

Consultado vía MCP `mirai` (`ask_mirai`, modelo `kimi-k3.1-pro`), con el resumen de la iteración y las preguntas de auditoría.

### 4.1 ¿La jerarquía resuelve el problema estructural señalado?

> *"Resuelve el problema estructural crítico: la autoridad única del cuaternión elimina la desincronización Euler/cuaternión, y el orden topológico con detección de ciclos/huérfanos es la base correcta para un grafo de escena. Sin embargo, persisten dos agujeros de diseño que limitan la escalabilidad: **falta de dirty-flagging/incrementalidad** — recorrer todo el grafo cada frame es O(n) incluso para escenas estáticas; en escenas complejas (10k+ nodos) esto consume CPU innecesariamente, necesitas propagación de suciedad: solo recalcular subárboles cuyo `local_transform` o `parent` cambió. Y **descomposición TRS incompleta** — si un padre tiene escala no uniforme (ej. `[2,1,1]`) y el hijo rota, la matriz mundial del hijo contiene shear; al descomponer para física o `set_world_transform`, la escala y rotación extraídas serán incorrectas a menos que implementes descomposición polar o manejo explícito de shear (como hace Unity con `lossyScale`)."*

Ambas observaciones son correctas y se aceptan como deuda técnica declarada, no como defecto de esta iteración: el recorrido completo es el comportamiento correcto para escenas de cientos de entidades (el caso actual), y el shear por escala no uniforme heredada es una limitación que Unity también documenta (`lossyScale` no es invertible). Quedan registradas para una iteración de optimización posterior.

### 4.2 ¿La corrección de señales es de raíz o un parche?

> *"Es una **solución de raíz para la terminación ordenada**, pero un **parche para la robustez del driver**. Manejar señales POSIX y drenar la cola de Vulkan (`device_wait_idle`) antes de destruir el dispositivo es la única forma correcta de cerrar una app Vulkan. CoreCLR pisa los manejadores por diseño, así que reinstalarlos es correcto. Riesgo latente: el driver puede devolver `VK_TIMEOUT` por otras causas (GPU colgada, TDR, underrun de comandos). Si tu lógica asume que `VK_TIMEOUT` solo ocurre en shutdown, un timeout real durante ejecución causará un crash o comportamiento indefinido. Debes distinguir `VK_TIMEOUT` por señal (salir) de `VK_TIMEOUT` por GPU hang (reiniciar dispositivo o abortar)."*

Coincide con el riesgo declarado en 2.6. La distinción señal-vs-hang queda en el backlog.

### 4.3 Nueva nota global

> ***"5/10"*** — *"Has pasado de 'demo técnica con transformaciones rotas' a 'núcleo de motor coherente', pero sigue siendo un pre-alfa sin pipeline de contenido real ni capacidad de producción."*

Progresión externa: ~3/10 (Iteración 2) → 4/10 (Iteración 3) → **5/10 (Iteración 4)**. Por primera vez la nota sube por una **capacidad estructural**, no por saneamiento.

### 4.4 Los 3 siguientes ítems priorizados

> *"1. **Carga real de escenas (`SceneManager.LoadScene`)**: es el cuello de botella para todo lo demás; sin instanciación dinámica de escenas no puedes probar jerarquías complejas ni flujo de juego real. 2. **VFS de assets (Virtual File System)**: prerequisito crítico para texturas, shaders y builds standalone; necesitas abstracción de rutas y hot-reload antes de cargar recursos gráficos complejos. 3. **Build/standalone funcional**: validación temprana del despliegue (embebido de CoreCLR, resolución de rutas relativas, empaquetado de assets); sin esto, el motor solo existe en tu máquina de desarrollo."*

Nota: el VFS se interpone antes de lo previsto en el roadmap (estaba detrás del build/standalone). La Iteración 5 se planificará con ese orden: `LoadScene` real + VFS mínimo.

---

## 5. Conclusión y Siguiente Iteración

La Iteración 4 convierte la jerarquía de entidades de un adorno de editor en una transformación real, cierra la última vía de desincronización de rotación por tipado, y elimina el único defecto de runtime reproducible que quedaba. El motor pasa de "demo técnica con transformaciones rotas" a "núcleo de motor coherente" (dictamen externo, 5/10).

**ITERACIÓN 5 (planificada): carga real de escenas + VFS mínimo**, siguiendo la priorización externa: `SceneManager.LoadScene` con instanciación dinámica y ciclo de escenas, y la abstracción de rutas que texturas, shaders y el build standalone necesitan como prerequisito.

---

## Anexo A — Archivos Modificados

| Archivo | Cambio | Rol en la iteración |
|---|---|---|
| `crates/glass_core/src/math.rs` | **NUEVO** (803 LOC, 21 tests) | biblioteca matemática: TRS, cuaterniones, inversa afín |
| `crates/glass_core/src/scene.rs` | +533 | `compute_world_matrices` topológico, rotación privada, `set_world_transform` |
| `crates/glass_core/src/lib.rs` | +6 | re-exportación de la API matemática |
| `crates/glass_app/src/main.rs` | +231/−? | consumo de matrices de mundo, señales ordenadas, física completa |
| `crates/glass_app/Cargo.toml` | +1 | dependencia `libc` para `sigaction` |
| `crates/glass_editor/src/panels/viewport.rs` | +179 | menú contextual del Scene View con unprojection |
| `crates/glass_editor/src/panels/console.rs` | +223 | explorador de Proyecto real con menú contextual |
| `crates/glass_editor/src/panels/gizmo_3d.rs` | +121/−? | `screen_to_world_on_plane`, picking y gizmos en espacio de mundo |
| `crates/glass_editor/src/panels/inspector.rs` | +16/−? | edición de rotación vía `set_rotation_euler` |
| `crates/glass_editor/src/panels/hierarchy.rs` | +4/−? | presets alineados con la jerarquía |
| `crates/glass_editor/src/panels/mod.rs` | +2/−? | re-exportación |
| `crates/glass_physics` (world.rs) | sincronización | cuaternión completo en el writeback |
| `crates/glass_scripting` (bindings.rs) | sincronización | getter/setter vía métodos encapsulados |
| `assets/shaders/mesh.frag` | +17/−? | `has_light` desde `material_params.w` (única fuente) |
| `assets/shaders/mesh.vert` | +16/−? | alineación del bloque de push constants |
| `assets/shaders/pbr.frag` / `pbr.vert` | +18/+24 | contrato de luz coherente con `mesh.*` |
| `crates/glass_mcp/src/handlers.rs` | **ELIMINADO** (Iter. 3) | código muerto con respuestas falsas |
| **Total** | **+1416 / −238** | 19 archivos + `math.rs` nuevo |

## Anexo B — Cifras del Proyecto Tras la Iteración 4

```
LOC Rust:            15 277   (glass_editor 4 782 · glass_core 2 514 ·
                               glass_app 2 621 · glass_renderer 2 465 ·
                               glass_mcp 1 019 · glass_audio 646 ·
                               glass_physics 558 · glass_scripting 443)
LOC C# gestionado:    2 028
Tests automatizados:     56   (0 fallos)  — antes: 30
Crates:                   8
Shaders GLSL:             6   (4 con push constants de 208 B)
SPIR-V:  mesh.vert 2128 B · mesh.frag 2272 B · pbr.vert 2472 B · pbr.frag 8368 B
```
