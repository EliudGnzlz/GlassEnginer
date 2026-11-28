# ANALISIS V7 — Modos Camera/World del Canvas

Rama base: `lu/v5-grupos` punta `62df48b` (V5 cerrado). V7 arranca sobre
esta punta. **Sin verificación por paquete: una sola verificación de
Análisis al cerrar el módulo, como V2–V5.** Todas las referencias de
línea son contra la punta `62df48b`. Peces del análisis resueltos en
`RESOLUCIONES_PECES_V7.md` (Opus: 3 Bloq/6 May/5 Men; DeepSeek flash:
razonamiento 31.910 / contenido 11.058).

## 1. Estado actual verificado contra el código

- `CanvasData.render_mode: String` (scene.rs:883) acepta
  «ScreenSpaceOverlay», «ScreenSpaceCamera», «WorldSpace» (default
  Overlay). La migración lo valida y corrige valores inválidos a Overlay
  (migracion.rs:23,37-42, con test).
- **NADA en el camino de render lee `render_mode`**: el campo hoy es
  solo metadato de autorado. El editor lo expone en dos ComboBox que
  editan el MISMO campo (canvas_ui.rs:107-122, inspector.rs:3068-3073);
  si un modo se deshabilita (Q11), hay que tocar los dos sitios.
- **Hoy la UI siempre es Overlay**: `render_ui_overlay`
  (renderer.rs:3956) dibuja el lote UI como pase aparte DESPUÉS de la
  escena 3D, solo `if render_standalone` (main.rs:8289, 8421). La
  GameView del editor NO pinta la UI runtime (deuda D-V4-4). **El
  trabajo real de V7 es introducir el primer camino de render no-Overlay**;
  el gate `if render_standalone` deja de valer tal cual.
- **El shader de vértice UI fija el espacio de pantalla**
  (ui.vert: `ndc = (a_pos / pc.screen_size) * 2.0 - 1.0;
  gl_Position = vec4(ndc, 0.0, 1.0)`): un modo Camera/World necesita
  otra transformación de vértice (uniforme MVP o segundo shader) o
  render-a-textura. La escena 3D ya tiene sus matrices de vista/
  proyección (`renderer.render_views`, main.rs ~8271).
- **El pase UI actual no muestra attachment de profundidad** en su
  región (renderer.rs:3986-4060): sin depth no puede respetar la
  profundidad de la escena 3D. (El `enable_depth_test: false` de
  main.rs:9132 es del render de egui, NO del pase UI.) A confirmar en P0
  si el pase dispone del depth de la escena.
- **OffscreenTarget existe** (renderer.rs:691-700, offscreen.rs:15-27)
  con imagen de color + imagen de **profundidad** + sampler + render
  pass + framebuffer; hay 4 instancias. RT+quad es viable con
  infraestructura existente.
- **El lote UI es de un solo canvas**: `build_canvas_batch_v2`
  (ui_v2.rs:301-315) toma `docs.into_iter().next()`. La derivación ya
  produce un Documento por cada raíz Canvas, en orden de entidades
  (derivacion.rs:45-54), pero el render descarta todos menos el primero.
  Multi-canvas es trabajo NUEVO.
- Cámara: `CameraData { fov, near_clip, far_clip, is_main }`
  (scene.rs:702); `EntityData.camera` (scene.rs:2217). **El Canvas NO
  tiene referencia a cámara** (no hay campo `world_camera` ni similar).
  `is_main` no está validado a «exactamente una»: el código toma el
  primer `is_main`, si no la primera cámara, si no nada (main.rs:1493).
- **El Transform 3D del Canvas no lo lee el camino UI**: la derivación
  toma el rect raíz de su `rect_transform` (derivacion.rs:86); la
  posición/rotación/escala 3D de la entidad Canvas no se usa.
- Hit-test y arrastre (interaccion.rs) trabajan en px de pantalla.
- Cobertura de tests del camino UI: dorada `ui_dorada_1280x720`,
  `clip_redondeado_recorta_en_gpu`, suite glass_ui (130 tests tras V5d).
  `render_ui_overlay` no tiene test unitario directo (pase Vulkan; lo
  cubren la dorada y los físicos).

## 2. Huecos

- **G-V7-1**: ¿A qué cámara se ata ScreenSpaceCamera? (Q1).
- **G-V7-2/8 (fusionados)**: unidad canónica de layout por modo. En
  WorldSpace el tamaño lo manda el Transform; `s` se reinterpreta o se
  depreca. **No permitir dos fuentes de escala (Transform × s) sin
  regla explícita.** ¿Se rehace el layout al cambiar la resolución si
  el canvas ya no está en px de pantalla? (Q8).
- **G-V7-3**: proyección (MVP) vs. render-a-textura + quad. **Es el ADR
  de P0**: la proyección toca shaders y UiVertex (y exige plomería de
  depth, que hoy no existe en el pase UI); RT+quad reutiliza el
  pipeline actual con coste de memoria + resample, y resuelve el depth
  a nivel de plano. (Q3).
- **G-V7-4**: depth. ¿La UI en Camera/World se somete al depth test
  contra la escena 3D? Sin depth en el pase UI, la proyección no puede
  responderlo sin tocar shaders. (Q4).
- **G-V7-5**: hit-test y arrastre en Camera/World: rayo, degenerados
  (sin intersección, canvas de canto, detrás de `near_clip`, arrastre
  fuera de bounds, cara trasera), oclusión por geometría 3D. (Q5).
- **G-V7-6**: GameView / D-V4-4. V7 absorbe un mínimo de D-V4-4 en P1
  (sin él no hay loop de testeo en editor y la verificación única se
  vuelve standalone-only). ¿Se cierra D-V4-4 del todo o solo el mínimo?
  (Q6).
- **G-V7-7**: varios Canvas con modos distintos. Hoy el render toma
  solo el primero; el orden entre canvas y la regla Overlay > Camera /
  World son parte de P4 (con `sorting_order`). (Q7).
- **G-V7-9**: `plane_distance` (nuevo). ScreenSpaceCamera necesita una
  distancia de plano: valor fijo configurable (Unity ~100) o derivado
  para que 1 px canvas = 1 px pantalla (`d = (h/2)/tan(fov/2)`). Sin él
  el canvas «flota» a un tamaño arbitrario. (Q2).
- **G-V7-10**: modos ya autorados sin efecto. Hoy un autor puede elegir
  WorldSpace y obtener Overlay en silencio; las escenas ya guardadas
  con Camera/World cambiarán de comportamiento al implementarse el
  render. (Q11, P6).
- Nota permanente: un cuarto modo futuro exigiría su entrada de
  migración; los campos nuevos llevan default + entrada de migración +
  round-trip (Q9/P6).

## 3. Preguntas para Rayo (decisiones)

**Q1 — Binding de cámara (G-V7-1).** A: campo `camera:
Option<EntityRef>` en `CanvasData` (estilo Unity; el canvas apunta a
una cámara concreta). B: resolver `is_main` en tiempo de render, sin
campo nuevo. Recomendación: A con fallback a main; resolver la cámara
en el llamador y pasarla al pase, no dentro del pase (en el editor hay
varias vistas y «main» no es un singleton global). Sub: ¿sin cámara
(fallback a Overlay + warning, o no renderizar)? ¿cámara borrada en
runtime? ¿viewport no-full → mapear al rect del viewport o a la
ventana?

**Q2 — Distancia del plano (G-V7-9).** (a) valor fijo configurable.
(b) derivado para que 1 px canvas = 1 px pantalla a FOV actual.
Recomendación: (b) como default calculado, con override (a). Si no, el
canvas cambia de tamaño al mover el FOV.

**Q3 — Proyección vs RT+quad (G-V7-3, la decisión grande).**
Proyección (MVP): UiVertex deja de ser px-pantalla; nuevo pipeline/
variante; depth natural; sin RT. Coste: toca shaders, semántica de
layout, hit-test; multiplica variantes. RT+quad: el pipeline UI actual
queda intacto; el canvas se rinde a textura y se compone con el
Transform. Reutiliza todo; coste memoria + resample; depth a nivel de
plano. Recomendación del análisis: RT+quad para V7 si P0 confirma el
soporte de RT (F1 ya lo confirma: OffscreenTarget existe). Si P0
encuentra que no hay RT/render graph útil, la proyección puede ser más
barata que construir RT. Sub: presupuesto de memoria/perf para N
canvases; política de re-render (dirty vs cada frame); resolución del
RT (px canvas × s vs pantalla).

**Q4 — Depth (G-V7-4).** Recomendación: Camera/World **depth-test on,
depth-write off**, z fijo al plano del canvas. Overlay sin cambios.
Sub: dentro del canvas el orden del pintor debe preservarse (no
reordenar por z); confirmar que el batch UI no se baraja al habilitar
depth-test (z-fighting clásico en UI coplanar). Sub: ¿el pase UI tiene
depth buffer vivo para activar esto sin un segundo pase? (F2, P0).

**Q5 — Hit-test / arrastre (G-V7-5).** Camera: el mapeo pantalla→px
canvas es el mismo que Overlay SOLO si Q2 es la opción (b) derivada;
con (a) fijo, escala con la distancia. World: rayo cámara→plano del
canvas, hit→UV→px, reutilizar el hit-test existente. Recomendación:
la oclusión por geometría 3D se ignora en V7 (se documenta). Sub: plano
oblicuo / de espaldas: ¿doble cara o una? Recomendación: una cara. Sub:
drag continuo si el puntero sale de los bounds o el canvas se mueve
durante el drag.

**Q6 — GameView (G-V7-6 / D-V4-4).** Recomendación: absorber el mínimo
en V7-P1 (pase UI parametrizado `render_ui(camera, viewport, mode)`
llamado desde standalone Y GameView). Pregunta: ¿el pase UI puede
parametrizarse así sin duplicar? ¿V7 cierra D-V4-4 del todo o solo el
mínimo?

**Q7 — Múltiples canvases y orden (G-V7-7).** Añadir `sorting_order:
i32` (default 0) si no existe. Regla propuesta: Overlay siempre por
encima de Camera/World; dentro de Camera/World, `sorting_order` y luego
distancia. Sub: si el batch es por canvas (F5: hoy solo el primero),
P4 obliga a partirlo por canvas/modo.

**Q8 — Factor de escala `s` (G-V7-2/8).** Overlay: sigue como hoy.
Camera: `s` sigue siendo el factor de referencia; se combina con
`plane_distance`. World: el tamaño lo manda el Transform; `s` se
reinterpreta o se depreca. No permitir dos fuentes de escala sin regla
explícita.

**Q9 — Migración.** Los campos nuevos (`camera`, `plane_distance`,
`sorting_order`, `world_units_per_px` si aplica) necesitan default +
entrada de migración + round-trip, con el mismo patrón que
`render_mode` (migracion.rs:23,37-42).

**Q10 — Batching.** ¿`UiVertex` por canvas o global? ¿Un draw call por
canvas en Camera/World es aceptable? (F5 + presupuesto).

**Q11 — Modos autorados sin efecto (G-V7-10).** ¿Deshabilitar los
modos inactivos en el editor, avisar, o permitirlos? Las escenas ya
guardadas con Camera/World cambiarán de comportamiento al
implementarse el render: ¿se marca, se migra, o se acepta?

## 4. Paquetes propuestos

Restricción: sin verificación por paquete, una sola verificación de
Análisis al cierre. Eso sube el coste de una suposición mala → **P0
(ADR) no es opcional** y cada paquete debe ser seguro por construcción
(no romper la salida existente hasta que el modo se activa).

| # | Paquete | Alcance | Depende | Cierra |
|---|---|---|---|---|
| **P0** | Recon + ADR | Confirmar F2, F4, F7; decidir Q1–Q4, Q7; ADR de RT vs proyección, binding, plane distance, depth, orden. Sin código de producción. | — | G-V7-1..4, 7, 8, 9 (diseño) |
| **P1** | Pase UI mode-aware + GameView | Extraer `render_ui(camera, viewport, mode)`; standalone y GameView lo llaman. **Todos los modos siguen = Overlay**. Sin cambio visual. | P0 (F8) | D-V4-4 (mínimo) |
| **P2** | ScreenSpaceCamera (1 canvas) | Mecanismo elegido, binding + fallback, plane distance, depth-test on/write off. Overlay intacto. | P0 | G-V7-1, 4 |
| **P3** | WorldSpace (1 canvas) | Transform, m/px, orientación, una cara, depth. | P0 | G-V7-2, 8 |
| **P4** | Multi-canvas + `sorting_order` | Campo + migración, split de batch, regla Overlay>Camera/World, orden intra-modo. | P2, P3 | G-V7-7 |
| **P5** | Interacción Camera/World | Camera: mapeo + política de oclusión; World: ray→plano, topmost picking, drag continuo, back-face. | P3, P4 | G-V7-5 |
| **P6** | Migración/serialización audit + cierre | Defaults, migraciones de todos los campos, round-trip, corrección de inválidos; **verificación única de Análisis**; actualizar doc y marcar diferidos. | todas | G-V7-6 (cierre), Q9, Q11 |

**Orden:** P1 solo en paralelo a P0 si Rayo lo autoriza; por defecto
secuencial P0→P1 (P1 toca el camino compartido y un ADR distinto lo
haría rehacer). P2 antes que P3 (Camera es plano y mira a cámara: el
único concepto nuevo es depth; World suma transform + interacción). P5
al final (necesita P3 y P4 para testear overlap). P6 es continuo por
campo pero se audita al cierre.

**Criterios de cierre por paquete:**
- P1: standalone con la misma imagen que hoy; GameView pinta UI runtime
  en Overlay.
- P2: escena con modelo entre cámara y canvas → el modelo tapa el
  canvas; sin cámara → fallback documentado.
- P3: canvas rotado/escalado en mundo; oclusión correcta; movimiento de
  cámara estable.
- P4: dos canvases solapados → orden esperado; Overlay por encima.
- P5: drag en canvas rotado; dos canvases solapados → topmost recibe;
  clic tapado por modelo según política.
- P6: round-trip de escena con todos los campos nuevos; migración de
  escena vieja sin ellos; checklist de verificación única (abajo).

**Diferidos explícitos (para no inflar V7):**
- Camino de proyección MVP (si gana RT+quad).
- Oclusión por geometría 3D en el raycast de UI.
- Previsualización de Camera/World en SceneView (solo GameView/standalone).
- Viewports de cámara no-full.
- Per-canvas render scale / AA policies.
- Múltiples `is_main` (regla: exactamente una, validada).

**Riesgos principales:**
- P1 toca camino compartido → regresión standalone (mitigar: salida
  idéntica como criterio).
- RT resample → texto borroso (fijar resolución del RT a px canvas × s).
- Depth test con UI coplanar → z-fighting (depth-write off, z fijo).
- Escenas viejas sin campos nuevos (mitigar: defaults en migración, P6).
- Resolver `is_main` dentro del pase (mitigar: pasar cámara explícita
  — Q1).
- Si P0 elige proyección/MVP, la superficie de shaders crece y la
  verificación de Análisis debe cubrir shaders UI + dorada.

**Verificación única de cierre (esbozo para P6):**
1. Overlay sin cambios (regresión).
2. Camera + oclusor 3D entre cámara y canvas (tapado / no tapado).
3. Camera sin cámara o con cámara borrada → fallback.
4. World rotado + escalado + cámara orbitando.
5. Dos canvases (Overlay + World) superpuestos → orden.
6. Drag en World rotado; clic sobre dos canvases solapados.
7. Round-trip de escena con todos los campos nuevos; migración de
   escena vieja sin ellos.
8. GameView y standalone coinciden en los modos soportados.
