# REGISTRO — Fase 5: UI, layout, anclas y Canvas Scaler

**Cierre: 2026-11-12.** Diseño propio: `fase1/DISENO_FASE5.md`. Especificación: §Fase 5 + anexo §8 del consolidado.

**Verificación de cierre: 340 tests, 0 fallos** (`cargo test --workspace` con `RUSTFLAGS="-D warnings"`). Tools MCP: 37 → **39** (`glass_ui_tree`, `glass_ui_stats`).

## Punto de partida (verificado en código)

`RectTransformData` ya tenía `anchor_min`/`anchor_max`/`pivot`, pero el resolver solo usaba anchor_min (sin estirado ni jerarquía), y `CanvasData` ya declaraba `reference_resolution` y `match_width_or_height` sin que nada los aplicara. La fase hace realidad lo que los datos ya prometían.

## Entregado

- **Anclas completas** (`glass_core::scene`): `resolve_ui_rect_in(rt, padre)` — el span entre anclas se estira con el padre y `size_delta` se suma al span. `resolve_ui_rect` queda como envoltorio contra la pantalla y reproduce la fórmula histórica al píxel con anclas puntuales (los 3 tests previos pasan sin tocarlos, incluida la rareza de posición con tamaño sin clamar y salida clamada a ≥10 px, que es paridad con el editor).
- **Canvas Scaler**: `canvas_scale_factor` con la fórmula logarítmica de Unity (Scale With Screen Size). El layout se calcula en unidades de referencia y se multiplica por el factor al final. La entidad **Canvas es el espacio raíz** (su rect es la pantalla en unidades de referencia, no su rect autorado — decisión tomada al ver el primer test de jerarquía fallar por componer contra el RectTransform por defecto del Canvas).
- **Jerarquía**: `resolve_scene_ui_rects(scene, w, h)` resuelve cada elemento contra el rect de su ancestro UI más cercano, con memoización y guarda anti-ciclos, y devuelve el mapa id → rect en píxeles.
- **Layout groups**: `LayoutGroupData` (dirección, spacing, padding, alineación) + `layout_group_rects` pura: recoloca los hijos UI directos en fila/columna conservando sus tamaños, por profundidad ascendente (grupos anidados correctos). Cableado en `EntityData::new` con el componente "LayoutGroup".
- **App**: `canvas_ui` (batch de dibujo, hit-test de botones y las dos funciones de drag & drop) consume el mapa resuelto: una sola resolución para pintar y pulsar, como antes pero ya jerárquica.
- **MCP**: `glass_ui_tree` (factor + rect de cada elemento a la resolución pedida) y `glass_ui_stats` (conteos + profundidad máxima).
- **Contenido**: escena `Fase5Layout` (banda superior estirada con título hijo, panel inferior estirado con LayoutGroup horizontal centrado de 3 botones) y `docs/UI.md` (Kimi).

## Test físico (VM; driver `fase1/driver_f5.py`, captura en `test-fisico5/`)

Los rects devueltos por `glass_ui_tree` coinciden **al píxel** con el cálculo a mano en las dos resoluciones:

- A 1920×1080 (factor 1,0): banda [0, 0, 1920, 64]; título (hijo de la banda) [760, 12, 400, 40]; panel [0, 1000, 1920, 80]; botones a x = 674 / 870 / 1066, y = 1015 (bloque centrado de 572 px con espaciado 16 y padding 15).
- A 1280×720 (factor 0,667): todo escalado por 2/3 (banda 42,7 de alto; botones de 120×33,3 a x = 449,3 / 580 / 710,7).
- `glass_ui_stats`: 1 canvas, 7 elementos, 5 botones, 6 textos, 1 grupo, profundidad 2.
- La captura muestra la banda azul a todo ancho con el título centrado y la fila de botones (rojo/verde/azul) centrada y equiespaciada en el panel inferior.

Efecto esperado en contenido existente: las escenas con Canvas 1920×1080 (Plat3D) ahora aplican el factor 0,667 a 1280×720 — la UI se ve proporcionada en vez de a tamaño de autoría; es el comportamiento que el dato del Canvas ya declaraba.

## Deudas anotadas

- La previsualización del editor no compone jerarquía ni aplica el scaler (el runtime sí).
- LayoutGroup sin Control Size (los hijos conservan su tamaño) y sin autoría desde Inspector/C# (se escribe en el JSON de la escena).

## Consumo (de los `out*.json` de la fase)

- **DeepSeek: 14.367 tokens** (núcleo completo en scene.rs a la primera, con sus tests).
- **Kimi: 2.818 tokens** (docs/UI.md).
- Claude: 0 (la integración de la app y las tools fueron quirúrgicas de Rayo).
