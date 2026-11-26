# ANÁLISIS V2 — Núcleo UI/Canvas (S21)

**Sección dueña: S21. Vecinas según inventario §3: S1 (formato de escena /
serialización — V2 NO lo toca), S10 (UI/Canvas clásica), S14 (editor), S3
(MCP), S2 (scripting C#).** Fecha: 2026-11-26. Autora: Lu (segundo agente).
Base: `diseno_fase_ui_canvas.md` (V0 revisado, aprobado por D3m3tr10) +
estado verificado en código en `lu/ui-canvas` (punta `2da24a5`, V1 cerrada).
**Sin código de V2 hasta la orden de trabajo de Rayo.**

## 1. Qué es V2 (criterio del diseño, sin cambiar)

Parser CSS del `style_extra`, cascada, layout completo del «núcleo bueno»,
pintor con el formato de vértice ampliado, paridad de interacción con lo
vivo (hit-test con la semántica actual pulsar/soltar dentro, drag con
`UIDragData`, acción por nombre a C#, texto de bits portado como
rasterizador provisional — se retira en V3). `resolve_scene_ui_rects`, MCP
y C# pasan a leer el documento colocado. **Los tests de `canvas_ui` se
portan ANTES de borrarlo y `canvas_ui` se retira en este sub-bloque: un
solo sistema vivo.**

Criterio de cierre (diseño): rects esperados escritos a mano + snapshot
viejo con diferencia ≤ 0,5 px + imágenes doradas (≤ 2/255 de media, ≤ 1 %
de píxeles fuera de tolerancia); tests de clic y drag portados, incluido el
botón «Jugar», pasan sobre `glass_ui`; editor y runtime dan rects idénticos.

## 2. Estado de partida verificado en código (2026-11-26)

- `crates/glass_ui`: `documento.rs` (68 l., `TipoGraphic`, `NodoDocumento`,
  `Documento`), `estilos.rs` (78 l., `StyleProps` con conjunto cerrado,
  `Dimension`, `Display`, `Sombra`; comentario: «el parser llega en V2»),
  `derivacion.rs` (397 l., `derivar_documentos`, 8 tests), `migracion.rs`
  (300 l., 7 tests). **No hay parser CSS, ni layout (flex/grid), ni
  pintor, ni hit-test en `glass_ui`.** La derivación construye nodos con
  `layout_group` clonado, sin calcular rects.
- `crates/glass_app/src/canvas_ui.rs` (1.005 l., ~26 marcadores de test):
  lote de quads, atlas de bits con `ab_glyph`, hit-test, drag & drop,
  puente de acciones por nombre a C#. Tests a portar, entre otros:
  `escena_canvas_ui_test_genera_lote`, 5 de hit-test
  (`hit_test_acierta_el_centro_del_boton`,
  `hit_test_falla_fuera_del_boton`, bordes, inactivos, solape),
  `escena_real_canvas_ui_test_el_boton_jugar_recibe_el_clic`, y los de
  drag. **Portar antes de borrar: el orden es contrato, no preferencia.**
- Consumidores de `canvas_ui` que deberán leer el documento: `main.rs`
  (migración en 4 vías de carga — esa parte queda, es V1 —,
  `build_ui_scene_images`, `build_canvas_batch`, `drag_press` /
  `drag_drop_target`, `ButtonClickTracker`, `DragSession`), `glass_mcp`
  (`server.rs:2759` usa `resolve_scene_ui_rects`), editor
  (`glass_editor::panels::canvas_ui`, pestaña P10). `glass_core::scene`
  aloja `resolve_scene_ui_rects` y sus tests (5 usos en `scene.rs`).
- Shader actual: `assets/shaders/ui.vert` recibe solo pos/uv/color
  (vec2+vec2+vec4); `ui.frag` muestrea la cobertura del atlas de bits.
  El formato ampliado del diseño (coordenada local, radios vec4, rect de
  clip + softness, modo color/imagen/SDF/sombra, parámetros de degradado)
  **no está construido**. Los `.spv` van comprometidos y se incrustan con
  `include_bytes!`; **en el CI no hay paso que los regenere [SIN
  VERIFICAR cómo se recompilan hoy]** — ver riesgo R1.
- Snapshot de paridad congelado en V1:
  `crates/glass_ui/tests/fixtures/rects_canvasuitest_1280x720.json`
  (generador `examples/snapshot_rects.rs`). Es la referencia numérica de V2.

## 3. Paquetes (orden de dependencias, uno detrás de otro donde se dice)

P0. **Contrato de API + port de tests de `canvas_ui` → `glass_ui`
(DeepSeek flash, con el contrato escrito por Lu directamente).**
Primero el contrato: las firmas que V2 va a exponer (derivar →
colocar → hit-test → drag, tipos de entrada/salida) como stubs en
`glass_ui`, sin lógica. Después, los tests de hit-test, drag, lote y
escena real se reescriben contra ese contrato, en rojo hasta que exista
el núcleo. Sin este paquete no se toca `canvas_ui`. Presupuesto: el
contrato ≤ 60 líneas; cada envío de tests ≤ 150 líneas, máximo 2
iteraciones por envío. Aceptación de P0: el contrato compila y los
tests portados compilan en rojo (fallan por falta de lógica, no por
falta de API).

P1. **Parser CSS + cascada (DeepSeek flash).** Declaraciones y bloques de
pseudo-clase `:hover/:active/:disabled/:focus/:selected`, sin selectores.
Cascada: defecto del tipo → campos del componente → `style_extra`.
Errores: declaración ignorada + aviso en Inspector y log, nunca pánico,
con test de fuzz del parser (500 entradas aleatorias de una gramática
fija, semilla fija; pasa si no hay pánico y ningún `StyleProps`
inválido). Las pseudo-clases se resuelven como función pura dado un
estado sintético (la aplicación del estado en runtime es de P4).
Salida: `StyleProps` resueltos por nodo. Depende de: nada de V2 (puede
ir en paralelo a P0). Presupuesto: cada envío ≤ 150 líneas, máximo 2
iteraciones. Aceptación de P1: el parser pasa el fuzz + los casos de
cascada escritos a mano (incluidas las 5 pseudo-clases con estado
sintético).

P2. **Layout (Opus).** Dos pasadas (medir hijos→padres, colocar
padres→hijos); absoluto por anclas por defecto; flex desde LayoutGroup
(reparto uGUI en tres tramos + tabla de spacing/padding/alineación de 9
posiciones) y grid V2 (columnas fijas, `1fr`, celda fija, auto-fill);
escala del Canvas con la fórmula de Unity (`match` logarítmico) con test;
trait de medida de texto con el rasterizador provisional (tolerancia
±1 px frente a la medida directa con `ab_glyph` de una cadena de prueba
fija, con la fuente por defecto del proyecto); rects redondeados a píxel
físico al pintar. Depende de: P1 (estilos resueltos) **y de la puerta
G0 (R1 resuelto, §3.5)**. **Freno armado (ver R4): paquete partido por
tamaño de salida (cada envío ≤ 300 líneas).** Aceptación de P2: snapshot
viejo ≤ 0,5 px en todos los rects + fixtures nuevos escritos a mano
para flex, grid, degradado y sombra (el snapshot es solo regresión, no
aceptación de lo nuevo).

P3. **Pintor + GPU (Opus).** Formato de vértice ampliado + `ui.vert` /
`ui.frag` nuevos (modos color/imagen/SDF/sombra, degradados multiparada
por subdivisión, alfa premultiplicado, recorte con softness, radios de
esquina), capa superior por documento (`override sorting` + `sort_order`),
invalidación por documento/sub-Canvas. Depende de: P2 (rects colocados).
Incluye regenerar los `.spv` comprometidos (R1, ya resuelto en G0).
Imágenes doradas en backend fijo: lavapipe, 1280×720, escena fija,
semilla fija. Aceptación de P3: doradas dentro de tolerancia en ese
backend + todos los rects de P2 siguen en ≤ 0,5 px.

P4. **Interacción + retirada de `canvas_ui` (Opus con apoyo DeepSeek).**
Hit-test en orden inverso exacto al de pintado (capa superior incluida),
solo `raycast_target`; el evento sube hasta el primer ancestro con
manejador; drag con `UIDragData`; acción por nombre a C#; `resolve_scene_
ui_rects`, MCP y C# leen el documento colocado. Pruebas de integración:
MCP (`tools/list` + lectura de rects por MCP contra la escena viva),
acción por nombre a C# extremo a extremo (clic medido en «Jugar» con
log, como en el cierre de V1) y apertura de la escena en el editor.
**Solo cuando P0–P3 estén en verde: se borra `canvas_ui` y se comprueba
que nada lo referencia.** Rollback: el borrado va en un único commit en
la rama de V2 (la `main` no se toca); revertir ese commit restaura el
sistema viejo. Depende de: P0–P3. Cierre del sub-bloque aquí.
**R5:** P4 congela la interfaz de instancia de prefab tal como esté al
empezar P4 (fixture con la instancia actual); si la Fase B cambia el
formato después, es un paquete nuevo, no retrabajo de V2.

## 3.5 Puertas

- **G0 — R1 resuelto antes de P2.** Antes de arrancar P2: procedimiento
  verificado para regenerar los `.spv` (herramienta disponible en la VM
  o instalable, y paso documentado). Si no existe, se escala a Rayo
  (pregunta Q1) y P2 no arranca. No se acepta «resolverlo en P3».
- **G1 — P0 en rojo compilable antes de P1/P2.** El contrato de API
  existe y los tests portados compilan (en rojo) antes de escribir
  lógica de layout o pintor.

## 4. Riesgos

- **R1 — Regeneración de los `.spv` [SIN VERIFICAR].** El diseño decide
  el formato de vértice pero no dice cómo se recompilan los shaders
  (sin `glslc`/`glslangValidator` en esta VM, sin paso en el CI). Si no
  hay procedimiento, P3 no puede entregar los `.spv` nuevos. Antes de
  P3: verificar el procedimiento real (o declararlo como trabajo de P3).
- **R2 — Retirada de `canvas_ui` antes de tiempo.** `main.rs`, MCP y el
  editor dependen de él. El port de tests (P0) es la red; borrar antes
  de P4 deja el editor sin UI. Mitigación: el borrado es el último paso
  de P4, con `cargo test --workspace` y grep de referencias como puerta.
- **R3 — Paridad de rects ≤ 0,5 px.** El resolutor viejo y el layout
  nuevo deben coincidir en CanvasUiTest y en las escenas de ejemplo.
  Cualquier divergencia (anclas, escala del Canvas, redondeo) rompe el
  criterio. Mitigación: el snapshot congelado se corre desde P2, no al
  final.
- **R4 — Desbocamiento de Claude en paquetes grandes.** El aviso de Rayo
  (buzon `641890c`, Fase D: 54.663 chars de razonamiento, 1 de
  contenido, ~100k tokens facturados) manda: paquetes partidos por
  tamaño de salida y freno `--max-razonamiento` **armado** en llamadas
  grandes (25.000 con contenido cero = aborta y devuelve parcial). P2 y
  P3 van partidos; el freno se arma en cada llamada de Opus.
- **R5 — Prefabs con UI (S22, frente activo de Rayo).** V1 migra
  prefabs; V2 debe derivar layout también para instancias de prefab.
  Coordinar con Rayo: si su Fase B cambia el formato de instancia, el
  layout de V2 lo sigue; no se toca el formato de prefab desde aquí.
- **R6 — Texto provisional.** El trait de medida usa el rasterizador de
  bits; V3 lo retira. Los tests de layout no deben depender de métricas
  del atlas de bits más allá del contrato del trait.

## 5. Test físico de V2 (preparado antes, como manda la regla)

- Escena: CanvasUiTest (la misma del snapshot) + EscenaMigracion.
- Puntos de medición: rects del documento colocado vs. snapshot
  (≤ 0,5 px por rect); editor y runtime dan los mismos rects (mismo
  documento, dos consumidores); imágenes doradas de la GameView
  (≤ 2/255 media, ≤ 1 % fuera de tolerancia); clic medido en «Jugar»
  con bbox por visual grounding (como en el cierre de V1) y log de
  acción; drag con `UIDragData` medido (inicio/fin y destino).
- Barrido: S21 dueña + vecinas S10 (UI clásica renderiza igual), S14
  (editor: jerarquía/Inspector/consola), S3 (MCP lee rects del
  documento), S2 (C# recibe la acción por nombre). S1 no se toca en V2
  (sin cambio de formato de escena: no dispara el barrido completo del
  §3). S22 mínimo: una instancia de prefab con UI (la del formato
  vigente al empezar P4) abre y coloca sus rects con ≤ 0,5 px frente a
  los valores escritos a mano; si no hay tal instancia, se crea una
  escena de prueba mínima con una.

## 6. Cierre de V2

Pipeline exacto del CI en local + CI real de GitHub (regla permanente:
sin CI real no hay cierre) + test físico medido + barrido de la §5 +
informe con consumo por proveedor (caché/no-caché separados) + revisión
cruzada obligatoria (peces: mínimo 2 de familias distintas, sobre este
análisis antes de ejecutar y sobre el plan de P2/P3 si cambian).

## 7. Preguntas para Rayo (decisión suya, no mías)

1. **R1:** ¿cómo se regeneran los `.spv` hoy? ¿Lo resuelvo dentro de P3
   o hay procedimiento que no veo?
2. **Reparto:** el diseño asigna V2 a Opus (layout y pintor) y el parser
   y los ports a DeepSeek flash. ¿Se mantiene, o se mueve algo a
   Sonnet por coste?
3. **R5:** ¿la Fase B de prefabs toca el formato de instancia antes de
   que V2 derive layout, o V2 va primero y Fase B lo sigue?

## 8. Resolución de la revisión peces (2 peces, familias distintas)

Pez 1 (Opus, comprensión): paráfrasis correcta; 0 Bloqueante.
Pez 2 (DeepSeek flash, lente crítica): 2 Bloqueante, 6 Mayor, 2 Menor.
Medición: pez 1 Opus reasoning 0 / contenido 6.309 chars; pez 2
DeepSeek reasoning 7.226 / contenido 1.332 chars (segundo intento; el
primero se cortó con 11.881 de razonamiento y 0 de contenido, por
`max-tokens` corto y sin pedir brevedad — lección: pedir salida escueta
explícita). No se re-audita: máximo una re-auditoría y no se usa.

- **Pez 2 Bloqueante, P0↔P4 circular → APLICAR.** P0 ahora empieza por
  el contrato de API (stubs sin lógica, escritos por Lu) y los tests se
  portan contra él; P4 implementa y borra. Puerta G1.
- **Pez 1 Mayor + Pez 2 Bloqueante, R1 → APLICAR.** R1 se resuelve
  antes de P2 (puerta G0), no antes de P3. Si no hay procedimiento, se
  escala a Rayo y P2 no arranca.
- **Pez 1 Mayor, criterios por paquete → APLICAR.** Cada paquete tiene
  ahora su aceptación explícita en §3 (P0–P3).
- **Pez 1 Mayor + Pez 2 Mayor, R5 sin fallback → APLICAR.** P4 congela
  la interfaz de instancia vigente al empezar P4; un cambio posterior
  de Fase B es paquete nuevo.
- **Pez 2 Mayor, pseudo-clases sin estado → APLICAR.** P1 resuelve
  pseudo-clases como función pura con estado sintético; el cableado en
  runtime es de P4.
- **Pez 2 Mayor, snapshot no cubre lo nuevo → APLICAR.** Fixtures
  nuevos escritos a mano para flex/grid/degradado/sombra; el snapshot
  es solo regresión.
- **Pez 2 Mayor, doradas no deterministas → APLICAR.** Backend fijo en
  P3 (lavapipe, 1280×720, escena y semilla fijas).
- **Pez 2 Mayor, P4 sin integración → APLICAR.** P4 incluye pruebas de
  integración MCP, C# extremo a extremo y editor.
- **Pez 1 Menor + Pez 2 Menor, presupuestos sin número → APLICAR.**
  P0/P1 ≤ 150 líneas por envío (máx. 2 iteraciones); P2/P3 ≤ 300
  líneas por envío; contrato ≤ 60 líneas.
- **Pez 1 Menor, fuzz sin alcance → APLICAR.** 500 entradas, gramática
  fija, semilla fija; pasa sin pánico ni `StyleProps` inválido.
- **Pez 1 Menor, mínimo S22 sin definir → APLICAR.** Definido en §5
  (instancia con UI, ≤ 0,5 px frente a valores escritos a mano).
- **Pez 1 Menor, sin rollback → APLICAR.** El borrado de `canvas_ui`
  va en un único commit en la rama de V2; revertirlo restaura el
  sistema viejo. Sin feature flag: la rama es el mecanismo.

## 9. Addenda al cierre de P0/P1 (2026-11-26)

- **P0 cerrado** (`lu/v2-nucleo` 74f49de): contrato en `api.rs` + 23 tests
  portados en `tests/port_canvas_ui.rs`: 3 pasan, 20 en rojo por `todo!()`
  en `colocar_documento` (verificado por Lu: los 20 panics son en
  `api.rs:104`). Puerta G1 cumplida.
- **P1 cerrado** (`lu/v2-nucleo` 391e977): `parser.rs` con parser CSS,
  cascada y fuzz (500 entradas, semilla fija); lib 24 passed (13 + 11
  nuevos). Los stubs de `parsear_estilo_extra`/`resolver_estilos` se
  borraron de `api.rs` al aplicar lo real.
- **G0 verificado**: `glslang-tools` instalado en la VM de Lu,
  `glslangValidator` 15.1.0 en `/usr/bin/glslangValidator`; compila
  `ui.vert` y `ui.frag` a `/tmp` con RC 0. P2 puede arrancar.
- **[FALTA EN CONTRATO] (del port P0, para P2/P3/P4):** (1) máquina de
  estados pulsar/soltar (`ButtonClickTracker`) — P4; (2) función de
  pintado `pintar_documento -> Vec<Segmento>` para los conteos de
  vértices — P3; (3) `SesionArrastre::follow()` y agarre — P4;
  (4) `DestinoSoltar` incompleto (falta nombre, anchored, drop_action)
  — P4; (5) `preserve_aspect` necesita el tamaño del sprite (el
  contrato no lo recibe) — P2/P3; (6) `hit_test` debe confirmar que
  omite botones con `interactable = false` — P4.
- **Medición de los paquetes:** los informes de P0/P1 no trajeron la
  medición de proveedor (chars); desde P2, cada informe de paquete
  trae su medición como manda la norma.
