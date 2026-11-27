# CONTRATO — Formato de materiales v2 de GlassEnginer (M0, en papel, sin código)

- **Fecha:** 2026-11-27
- **Estado:** M0 CERRADO en papel. Este documento fija el formato; no hay código en él.
- **Fuentes:** informe de análisis `buzon/harness/analisis_materiales_lu.md` (321 líneas; §2–§5 levantamiento y plan M0–M8; §6 las 9 decisiones planteadas) y veredicto de Rayo que las cierra, `buzon` commit `6d2c9f6` (líneas `DECISION RAYO M0-6.1` a `M0-6.9` + `ORDEN RAYO M0 CERRADO`).
- **Regla de paridad:** todo default nuevo debe dejar la escena actual **idéntica** (los 6 assets existentes y todas las escenas cargan igual que con v1). Donde un default podría cambiar el render, el contrato fija el que preserva el comportamiento de hoy y lo dice.
- **Ejecución:** sigue en cola tras el frente de Canvas completo (V4→V5→V7→tools MCP→faltas del editor), como ordenó el usuario. Este contrato no adelanta la ejecución: solo deja cerrado M0.

---

## 1. Fundamento: las 9 decisiones de Rayo (M0-6.1 a M0-6.9)

1. **6.1 Alcance del formato v2:** entran TODOS los candidatos de M1 (ver §2, grupos C y D). Aparcados en este bloque: detail inputs, workflow especular y modo GI de emisión (ver §3).
2. **6.2 Variantes:** M4 ENTRA en este bloque. Patrón `variant_of`/`variant_overrides` de prefabs, resolución en memoria al cargar.
3. **6.3 Fachada M5:** la hace Lu dentro del frente de materiales. Rayo revisa el diff de fachada antes del cierre del paquete; ventana tras M1 (que fija el formato). No pisa su verificación de reimportación de NeoFPS (verificación, no código de fachada).
4. **6.4 Congelación de `pbr.frag`:** M2 es dueño del shader desde que arranca hasta que cierra; el frente gráfico de Rayo no lo toca en esa ventana (G4b no lo toca; si G5 lo necesitara, se coordina por el buzón). M2 re-mide paridad con rigs físicos antes/después. Fuera de la ventana, el shader se toca con aviso previo por el buzón, como siempre.
5. **6.5 Ubicación:** `MaterialData` SE MUEVE a módulo propio `glass_core::material` en M1, con re-export en `scene.rs` para no romper importadores en la transición. El barrido de cada paquete queda acotado al módulo.
6. **6.6 Identidad:** se mantiene la referencia POR NOMBRE en este bloque; sin GUID (tocaría escenas, Project, MCP e importador: bloque transversal aparte). Mitigación acotada en M3: renombrado seguro en el Project (renombra el archivo y actualiza las referencias en las escenas del proyecto). El GUID queda aparcado para un bloque posterior de identidad de assets.
7. **6.7 Reparto por proveedor:** M1 DeepSeek flash, M2 Opus, M3 DeepSeek flash, M4 DeepSeek flash, M5 Opus, M6 DeepSeek flash, M7 DeepSeek flash, M8 Rayo (medición, sin proveedor). Kimi aparcado por saldo en ambos lados; freno `--max-razonamiento` armado en cada llamada Opus. Peces por paquete SOLO en M2 y M5 (los críticos); el resto va con la revisión única de Análisis al final del bloque, como en V2/V3/V4.
8. **6.8 Unificar (la de fondo): UN SOLO SISTEMA.** `.material.json` gana `shader` (`"pbr"` por defecto; `"graph:<nombre>"` para grafos) y es el único asset de material; las 9 props PBR pasan a ser las propiedades declaradas del shader built-in y M6 les aplica el mismo mecanismo de metadatos que a los grafos; los valores de grafo dejan de vivir por entidad (`shader_properties`) y pasan al asset; los presets se convierten en materiales con vínculo vivo, no copia. `MeshData.shader` queda legacy en migración (M1 define el camino). No conviven dos sistemas al final del bloque; la convivencia es solo transitoria durante M1–M2.
9. **6.9 Multimaterial: SÍ, arrays por rango.** El formato gana `materials: [String]` (opcional; sin él, `material` único como hoy, compatible hacia atrás) y la fachada expone `materials`/`sharedMaterials`. El glTF ya trae materiales por rango y hoy se pierden en la sustitución en bloque.

---

## 2. Lista CERRADA de campos del formato v2

Asset: `assets/materials/<nombre>.material.json`. Identidad = nombre de archivo (6.6); **no hay campo `name` ni `version`**: un asset sin campo `shader` (ni `variant_of`) se carga como v1 con los defaults de abajo (tolerancia por `serde(default)`, como hoy).

Convenciones de tipos: `String` ruta de asset (vacío = sin mapa); `f32` escalar; `[f32;N]` vector; `bool`; `enum` escrito como string con el conjunto cerrado que se indica; `Option<T>` = campo ausente o `null`.

### A. Identidad, shader y variante (nuevo en v2)

| Campo | Tipo | Default | Desde | Usado en |
| :--- | :--- | :--- | :--- | :--- |
| `shader` | `String` | `"pbr"` | M1 | M1–M8. Valores cerrados en este bloque: `"pbr"` (built-in) o `"graph:<nombre>"`. Es el único eje que elige el shader; `MeshData.shader` queda legacy (ver §4). |
| `variant_of` | `Option<String>` | ausente | M1 (campo) / M4 (resolución) | M4. Nombre del material padre. Cadena base+diff con detección de ciclos, patrón prefabs. |
| `variant_overrides` | `Option<Map<String, Value>>` | ausente | M1 (campo) / M4 (resolución) | M4. Objeto con los mismos nombres de campo de este contrato; solo los campos presentes pisan al padre. |

### B. Propiedades PBR (las 9 de v1, sin cambio de nombre, tipo ni default)

| Campo | Tipo | Default | Desde | Usado en |
| :--- | :--- | :--- | :--- | :--- |
| `albedo` | `[f32;4]` | `[1, 1, 1, 1]` | v1 | M1–M8. Equivale a `_BaseColor`. |
| `texture` | `String` | `""` (blanco 1×1) | v1 | M1–M8. Mapa base (`_BaseMap`). |
| `metallic` | `f32` | `0.0` | v1 | M1–M8. Escalar; multiplica el canal G del mapa MR cuando existe (ver §2.C). |
| `roughness` | `f32` | `0.45` | v1 | M1–M8. Escalar (inverso de `_Smoothness`); multiplica el canal B del mapa MR cuando existe. |
| `emissive` | `[f32;3]` | `[0, 0, 0]` | v1 | M1–M8. `_EmissionColor`. |
| `emissive_intensity` | `f32` | `0.0` | v1 | M1–M8. Intensidad HDR; alimenta el bloom. |
| `tiling` | `[f32;2]` | `[1, 1]` | v1 | M1–M8. Transformación UV (scale) del mapa base. En este bloque es la ÚNICA transformación UV: los mapas nuevos comparten el mismo UV (sin ST por mapa). |
| `offset` | `[f32;2]` | `[0, 0]` | v1 | M1–M8. Transformación UV (offset) del mapa base. |
| `transparent` | `bool` | `false` | v1 | M1–M8. Surface Type binario; elige el pipeline transparente. |

M6 aplica a estas 9 propiedades el mismo mecanismo de metadatos que a los grafos (`PropertyDef`: etiqueta, rango, HDR…), **sin renombrarlas** (6.8).

### C. Mapas de superficie (nuevos, M1 formato + importador, M2 GPU)

| Campo | Tipo | Default | Desde | Usado en |
| :--- | :--- | :--- | :--- | :--- |
| `normal_texture` | `String` | `""` (sin mapa) | M1 | M1, M2. Mapa de normales en espacio tangente; el marco tangente se calcula por derivadas en el fragment shader (M2), porque el formato de vértice no lleva tangentes. |
| `normal_scale` | `f32` | `1.0` | M1 | M1, M2. Intensidad del normal map. |
| `metallic_roughness_texture` | `String` | `""` (sin mapa) | M1 | M1, M2. **Un solo mapa MR, no dos separados** (6.1). Convención de canales fijada: **G = metallic, B = roughness** (convención glTF, la fuente del importador). Los valores del mapa multiplican a los escalares `metallic`/`roughness`. |
| `occlusion_texture` | `String` | `""` (sin mapa) | M1 | M1, M2. Mapa de oclusión. |
| `occlusion_strength` | `f32` | `1.0` | M1 | M1, M2. El escalar de AO queda cubierto por este campo: hoy el canal `ao` del shader va fijo a 1.0, así que el default 1.0 preserva la paridad. Cuando hay mapa, multiplica su valor. |
| `emissive_texture` | `String` | `""` (sin mapa) | M1 | M1, M2. Mapa de emisión; multiplica a `emissive` × `emissive_intensity`. |

M1 incluye el frente importador: `MaterialIr` (`glass_fbx`) y el cargador glTF deben **capturar** estos mapas (hoy el IR los pierde) y mapear `doble_cara` al nuevo `cull` (ver §2.D). Sin esto, M2 no tiene datos que mostrar en modelos importados.

### D. Opciones de superficie (nuevas, M1 formato, M2 GPU)

| Campo | Tipo | Default | Desde | Usado en |
| :--- | :--- | :--- | :--- | :--- |
| `alpha_clip` | `bool` | `false` | M1 | M1, M2. Activa el descarte por alfa (`discard`) en el pase PBR y en el pase de sombras/profundidad (precedente `depth_clip.frag`). |
| `alpha_cutoff` | `f32` | `0.5` | M1 | M1, M2. Umbral de descarte; solo aplica con `alpha_clip = true`. Default 0.5 (convención Unity). |
| `cull` | `enum` (`"front"` \| `"back"` \| `"both"`) | `"both"` | M1 | M1, M2. **Default `"both"` por paridad**: hoy todo se dibuja a doble cara (`CullMode NONE` en ambos pipelines); `"back"` cambiaría el render de los 6 assets existentes. El importador mapea `MaterialIr.doble_cara = true` → `"both"`, `false` → `"back"`. |
| `blend_mode` | `enum` (`"alpha"` \| `"premultiply"` \| `"additive"` \| `"multiply"`) | `"alpha"` | M1 | M1, M2. Solo aplica con `transparent = true`. `"alpha"` es el blend actual (`SRC_ALPHA / ONE_MINUS_SRC_ALPHA`). |
| `preserve_specular` | `bool` | `true` | M1 | M1, M2. Solo aplica con `transparent = true` y `blend_mode != "alpha"` (convención URP). |
| `receive_shadows` | `bool` | `true` | M1 | M1, M2. Default `true` por paridad: hoy el shader muestrea el shadow map sin consultar flags del material. |
| `render_queue` | `i32` | `2000` | M1 | M1, M2. Cola de render (convención Unity: 2000 = opaco). La clave de ordenación gana el término `render_queue` por delante de la actual `(transparente al final, shader, textura)`; con todos los valores en 2000 el orden de las escenas actuales no cambia. La selección del pipeline transparente sigue el `transparent` del material en este bloque. |
| `specular_highlights` | `bool` | `true` | M1 | M1, M2. Toggle de highlights especulares. |
| `env_reflections` | `bool` | `true` | M1 | M1, M2. Toggle de reflexiones de entorno (IBL). |

### E. Multimaterial (lado entidad, no del asset)

| Campo | Tipo | Default | Desde | Usado en |
| :--- | :--- | :--- | :--- | :--- |
| `materials` | `[String]` (opcional) | ausente | M1 | M1, M5. Campo de `MeshData` (lado entidad), **no** del `.material.json`. Lista de materiales por rango de malla, en orden. Ausente = `material` único como hoy (compatible hacia atrás). Cuando está presente, sustituye a la sustitución en bloque: los materiales por rango del glTF se conservan en vez de perderse. La fachada expone `materials`/`sharedMaterials` (6.9, M5). |

---

## 3. Lo aparcado (explícito; no entra en este bloque)

| Aparcado | Motivo / destino |
| :--- | :--- |
| Detail inputs (mask, detail base/normal, 2º tiling) | Fuera de 6.1. No hay campo en v2; un asset que los trajera no los carga. |
| Workflow especular | El motor es metálico GGX; no hay workflow especular (6.1). |
| Modo GI de emisión (Realtime/Baked/None) | Sin GI horneada en el motor (6.1). |
| GUID de identidad de assets | Aparcado a un bloque transversal posterior de identidad de assets (6.6). Mitigación en este bloque: renombrado seguro en el Project, en M3. |
| Bindless real (*descriptor indexing* de texturas) | Fuera del bloque salvo necesidad demostrada; M8 mide y decide (el SSBO indexado de parámetros y el bindless son dos decisiones distintas). |
| Interacción en runtime por MCP (pulsar botón, escribir en InputField, onClick) | Territorio V6, no de materiales. Las tools MCP de M7 cubren crear/editar el asset y leer/escribir sus propiedades, con validación. |
| Preview 3D dedicado del material | No es campo de formato; entra como pieza de M3 (preview simple reutilizando la miniatura de modelos; la fidelidad depende de M2). |

---

## 4. Reglas de migración y compatibilidad

1. **Carga tolerante:** todos los campos nuevos llevan `serde(default)`. Los 6 assets existentes (`cristal`, `damero`, `emisor`, `espejo`, `mate`, `oro`) y todas las escenas cargan sin reescritura y sin cambio visual.
2. **Paridad por defecto:** cada default nuevo reproduce el comportamiento actual (ver la columna de notas del §2: `occlusion_strength` 1.0 = el 1.0 fijo de hoy; `cull` `"both"` = el `NONE` de hoy; `blend_mode` `"alpha"` = el blend actual; `receive_shadows`/`specular_highlights`/`env_reflections` `true` = el comportamiento incondicional de hoy; `render_queue` 2000 con todos iguales no reordena nada).
3. **Detección v1/v2:** un asset sin `shader` ni `variant_of` es v1 y carga con defaults; no se reescribe en disco al cargar.
4. **Variantes (M4):** `variant_of` nombra al padre por nombre (identidad por nombre, 6.6). La resolución recorre la cadena base+diff **en memoria al cargar** y el resultado aplanado queda en la misma caché de materiales; no se reescriben assets en disco ni hay coste por frame. Detección de ciclos (patrón prefabs). Los overrides solo pisan los campos presentes en `variant_overrides`.
5. **Un solo sistema (6.8):** al final del bloque no conviven dos sistemas. La convivencia es solo transitoria durante M1–M2:
   - `MeshData.shader` queda **legacy** en migración; **M1 define el camino** (cómo el shader de la entidad pasa al campo `shader` del asset y cómo los valores de grafo de `shader_properties` pasan al asset). Este contrato fija solo el destino, no el camino.
   - Los presets de grafo se convierten en **materiales con vínculo vivo** (no copia): el preset deja de ser una copia de valores aplicada a la entidad.
   - La vía «crear desde shader/grafo» existe al final del bloque (crear un material eligiendo shader `pbr` o `graph:<nombre>`).
6. **Multimaterial (6.9):** `materials` (lado entidad) es opcional; ausente = `material` único como hoy. Cuando está presente, sus entradas se aplican por rango de malla en orden; la fachada (M5) expone `materials`/`sharedMaterials` con la misma semántica de clonación al primer acceso que el material físico (`Collider`/`CharacterController`).
7. **Formato de vértice:** sin cambio en este bloque. El normal mapping usa marco cotangente por derivadas en el fragment shader (M2); si M2 demuestra que no basta, el cambio de formato de vértice + importadores se decide en M2 con medición, no se anticipa aquí.
8. **Push constants y descriptores (M2):** el análisis de presupuesto (el push actual ya declara offsets hasta 224+16 B, por encima del mínimo garantizado de 128 B de Vulkan; el set 0 tiene un solo binding) se hace **en M2**, no en M8: añadir mapas exige ampliar el set 0 o un set nuevo.
9. **Pase de sombras (M2):** el pase de profundidad/sombras aplica el mismo `alpha_clip` que el pase PBR (precedente `depth_clip.frag`).

---

## 5. Reglas de ejecución del bloque

- **Orden:** M0 (este contrato) → M1 → M2 → M3 → M4 → M5 → M6 → M7 → M8. Dependencias: M1 ← M0; M2 ← M1; M3 ← M1 (el preview fiel ← M2); M4 ← M1 (la UI de override ← M3); M5 ← M1 (la fachada trabaja sobre el asset ya resuelto; ventana tras M1, 6.3); M6 independiente; M7 ← M1; M8 medición posterior a M2.
- **Reparto (6.7):** M1 DeepSeek flash · M2 Opus · M3 DeepSeek flash · M4 DeepSeek flash · M5 Opus · M6 DeepSeek flash · M7 DeepSeek flash · M8 Rayo (medición). Kimi aparcado. Freno `--max-razonamiento` armado en cada llamada Opus.
- **Revisiones:** peces por paquete SOLO en M2 y M5 (los críticos). El resto va con la revisión única de Análisis al final del bloque, como en V2/V3/V4.
- **Cierre por paquete:** pipeline exacto del CI en local, físico por el camino real con evidencia medible, barrido anti-regresión, consumo por proveedor y CIERRE en el buzón. M2 re-mide paridad con rigs físicos antes/después (6.4).
- **Congelación (6.4):** M2 es dueño de `pbr.frag` de arranque a cierre; el frente gráfico no lo toca en esa ventana. Fuera de ella, aviso previo por el buzón.
- **Ubicación del código (6.5):** `MaterialData` se mueve a `glass_core::material` en M1, con re-export en `scene.rs`.
- **Fachada (6.3):** M5 la hace Lu; Rayo revisa el diff de fachada antes del cierre del paquete.
- **Qué NO es este contrato:** no fija el diseño interno de M2 (pipelines por combinación vs estado dinámico), ni la UI de M3, ni los metadatos concretos de M6 (etiquetas y rangos por propiedad), ni la medición de M8. Eso se decide en cada paquete, sobre este contrato.
