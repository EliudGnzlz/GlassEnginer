# ADR V7 — Modos Camera/World del Canvas

Estado: **ACEPTADO por Rayo** (respuestas Q1–Q11, buzon `110b38f`,
2026-11-28). Base: `lu/v5-grupos` punta `62df48b`. Análisis:
`ANALISIS_V7.md`; peces resueltos en `RESOLUCIONES_PECES_V7.md`.
**P0 no escribe código de producción.** La puerta P0→P1 queda abierta
con este ADR; si P0 hubiera encontrado RT inviable, se volvía al buzón
antes de cambiar a MVP — **no ocurrió: RT confirmado viable** (ver
§2).

## 1. Contexto

`CanvasData.render_mode` existe desde V2 pero ningún camino de render
lo lee: hoy toda la UI es ScreenSpaceOverlay, dibujada como pase aparte
después de la escena 3D, solo `if render_standalone`. V7 introduce los
dos primeros modos no-Overlay. Verificación única de Análisis al cerrar
el módulo, como V2–V5; V7 no se integra hasta la suya (V5 sigue
pendiente de la suya).

## 2. Recon de P0 (hechos verificados en el código, punta 62df48b)

- **F1 — RT viable: CONFIRMADO.** `OffscreenTarget`
  (offscreen.rs:15-27) con color + depth (D32_SFLOAT) + sampler + render
  pass + framebuffer; `new(device, allocator, width, height)` con
  extents arbitrarios; el color es R8G8B8A8_UNORM y está documentado
  como «Muestreable por UI / egui». 4 instancias ya existen.
- **F2 — Depth en el pase UI: CONFIRMADO con matiz.** El pase de render
  UI (ui.rs ~595-615) SÍ liga un attachment de profundidad (DONT_CARE,
  DEPTH_STENCIL_ATTACHMENT_OPTIMAL), pero el pipeline UI (ui.rs:740-741)
  tiene `depth_test_enable(false)`: nunca testea. Además la UI se graba
  en los objetivos FINALES (`final_offscreen` / `final_game_offscreen`,
  renderer.rs:3996-4000), cuya profundidad NO contiene la escena (la
  profundidad de la escena vive en `offscreen.depth_image`). **Consecuencia:
  activar depth-test en el pase UI actual no puede responder Q4.** El
  depth contra la escena solo es posible si el canvas se compone DENTRO
  del pase 3D, como quad con la profundidad de la escena.
- **F3 — Transform del Canvas: CONFIRMADO.** La derivación no lee el
  `transform` 3D del Canvas (solo su `rect_transform`, derivacion.rs:86).
  WorldSpace lo leerá por primera vez.
- **F4 — Referencias a entidades: NO existe `EntityRef`.** No hay
  `EntityRef` / `entity_ref` / `ref_entidad` en ningún crate. El único
  patrón de referencia entre entidades son ids u64 (`parent:
  Option<u64>`, `children: Vec<u64>`, scene.rs:2271-2272).
- **F5 — Lote por canvas: CONFIRMADO.** `build_canvas_batch_v2`
  (ui_v2.rs:301-315) toma solo el primer Documento; la derivación ya
  produce uno por canvas (derivacion.rs:45-54). Multi-canvas es nuevo.
- **F6 — `is_main`: CONFIRMADO.** Primer `is_main` gana, si no la
  primera cámara, si no nada (main.rs:1493). Sin validación de
  «exactamente una».
- **F7 — Viewport por cámara: CONFIRMADO que no existe.** `FrameDraws`
  (renderer.rs:49-61) separa vista de escena y vista de juego en dos
  objetivos completos; el viewport es uno por dibujo (renderer.rs:3326).
  Cada cámara es ventana completa (standalone) o panel de GameView
  (editor). V7 mapea a ventana completa; viewports no-full, diferidos.
- **F8 — GameView: CONFIRMADO.** Nunca llama a `render_ui_overlay`
  (D-V4-4); la vista de juego se presenta muestreando
  `game_offscreen` como textura de egui
  (`create_game_offscreen_descriptor_set`, renderer.rs:2850-2867;
  main.rs:9161-9174). Muestrear un offscreen como textura es un patrón
  establecido.
- **Precedente del quad en escena: CONFIRMADO.** Los portales dibujan
  quads texturizados DENTRO del pase 3D (`PortalDrawItem { mesh, push,
  texture_index, range }`, renderer.rs:248-253, 3285): malla + textura
  con el depth test de la escena. Es el mecanismo que reutiliza V7.

## 3. Decisiones (Rayo, Q1–Q11)

- **D1 (Q1) — Binding de cámara.** Campo nuevo `camera:
  Option<EntityRef>` en `CanvasData`, con fallback a la cámara `main`.
  La cámara se resuelve EN EL LLAMADOR y se pasa al pase; nunca dentro
  del pase. Sin cámara válida (ni campo, ni main, ni cámara borrada):
  fallback a Overlay + warning visible (log + aviso en
  Inspector/GameView), nunca no-renderizar en silencio. Viewport no-full:
  diferido; V7 mapea a ventana completa.
- **D2 (Q2) — Distancia del plano.** Default derivado
  `d = (h/2) / tan(fov/2)` con h = alto del viewport; override con
  `plane_distance: Option<f32>` (None = derivada). Con override fijo, el
  canvas cambia de tamaño en pantalla al mover el FOV — documentado.
- **D3 (Q3) — RT+quad para V7.** Cada canvas Camera/World se rinde con
  el pipeline UI actual a su propio `OffscreenTarget` (resolución = px
  canvas × s) y se compone como quad texturizado DENTRO del pase 3D
  (patrón PortalDrawItem). Re-render cada frame (como Overlay hoy);
  dirty, diferido. Sin tope duro de memoria en V7; se mide en P2/P3.
  Si P0 hubiera encontrado RT inviable, se volvía al buzón — no ocurrió.
- **D4 (Q4) — Depth.** Camera/World: depth-test on, depth-write off, z
  fijo al plano del canvas. Overlay sin cambios. El orden del pintor
  dentro del canvas se preserva (el RT se rinde plano primero, sin
  reordenar); P2 verifica que el batch no se baraja.
- **D5 (Q5) — Hit-test / arrastre.** Camera: mapeo pantalla→px igual
  que Overlay con D2 derivada; con override fijo la escala es
  `d_derivada / d_fija` (documentar la fórmula). World: rayo
  cámara→plano, una cara, hit→UV→px con el hit-test existente. Oclusión
  por geometría 3D ignorada en V7 (documentado). Drag capturado hasta
  soltar aunque el puntero salga de bounds; si el canvas se mueve
  durante el drag, se recalcula cada movimiento en espacio canvas.
- **D6 (Q6) — GameView / D-V4-4.** V7 cierra D-V4-4 con el mínimo de P1:
  un único pase parametrizado `render_ui(camera, viewport, mode)` llamado
  desde standalone Y GameView. Si picando código duplicar fuera
  inevitable, se trae al buzón antes de duplicar. SceneView y viewports
  no-full quedan diferidos explícitos, fuera de D-V4-4.
- **D7 (Q7) — Orden entre canvas.** Campo nuevo `sorting_order: i32`,
  default 0, con migración. Overlay siempre por encima de Camera/World.
  Dentro de cada modo: `sorting_order` mayor por encima; a igualdad, el
  canvas más cercano a la cámara por encima. (La métrica de distancia
  por modo se define en P4.)
- **D8 (Q8) — Escala.** Overlay: `s` como hoy. Camera: `s` se combina
  con `plane_distance` (con D2 derivada, s=1 → 1 px canvas = 1 px
  pantalla). World: `s` NO se aplica al tamaño — lo manda la escala del
  Transform (tamaño mundo = layout px × escala del Transform; escala no
  uniforme admitida con su distorsión); el Inspector muestra `s`
  deshabilitado o con aviso en ese modo. NO se crea `world_units_per_px`
  en V7. El layout NO se rehace al cambiar la resolución en ningún modo;
  en Camera la distancia derivada se recalcula.
- **D9 (Q9) — Migración.** `camera`, `plane_distance` y
  `sorting_order` con default + entrada de migración + round-trip
  (patrón `render_mode`, migracion.rs:23,37-42). P6 audita.
- **D10 (Q10) — Batching.** Un draw call por canvas en Camera/World,
  aceptable. Batch por canvas (cada canvas su lote/RT).
- **D11 (Q11) — Autorado.** Se permite autorar (no se deshabilitan los
  ComboBox) con aviso visible en el Inspector mientras el modo no tenga
  efecto (hasta P2/P3). Las escenas ya guardadas con Camera/World
  CONSERVAN su modo al llegar V7 (no se migran a Overlay); el cambio de
  comportamiento se documenta en el cierre.

## 4. Flujo de render por modo (diseño)

- **Overlay (sin cambios):** layout → pintor → lote → `render_ui_overlay`
  al objetivo final, como hoy.
- **Camera:** por canvas: layout (con `s` y D2) → pintor → lote UI →
  `UiRenderer::record` al RT propio del canvas (cada frame) → quad
  texturizado con esa RT, colocado a `plane_distance` frente a la
  cámara resuelta (D1), dibujado DENTRO del pase 3D con depth-test on /
  depth-write off (D4). El quad mira a la cámara.
- **World:** igual, pero el quad es una malla en el mundo: posición /
  rotación / escala del Transform de la entidad Canvas (D8); una cara
  (D5).
- **Multi-canvas (P4):** un lote + un RT por canvas; orden de dibujo
  según D7 (Overlay encima de Camera/World; intra-modo por
  `sorting_order` y distancia). Hoy el render toma solo el primer
  canvas (F5): P4 lo cambia.
- **GameView (P1):** el mismo pase parametrizado sirve a standalone y a
  GameView; la GameView presenta su objetivo como hoy (muestreo egui).

## 5. Campo `camera`: forma concreta (para P2)

No existe `EntityRef`: se define como tipo nuevo (newtype sobre u64,
serde, default None) o como `Option<u64>` directo — lo decide P2 con el
patrón de ids de `parent`/`children` como referencia. Validación en
carga (id desconocido o sin componente cámara → None + warning) y en
render (cámara borrada → fallback D1). Migración: campo ausente → None.

## 6. Riesgos abiertos (para P2/P3/P4)

- Resample del RT → texto borroso (resolución = px canvas × s; medir).
- Memoria de N RTs (sin tope en V7; medir en P2/P3).
- El quad Camera debe seguir a la cámara cada frame (posición/rotación
  derivadas de la cámara resuelta, no del Transform del Canvas).
- Métrica de distancia del desempate D7 por modo (P4).
- El mapeo UV→px del hit-test en World con escala no uniforme (P5).
- Si la proyección MVP llegara a ser necesaria más adelante, es un
  cambio de arquitectura fuera de V7 (diferido explícito).

## 7. Paquetes (de ANALISIS_V7 §4, con las decisiones aplicadas)

P0 (este ADR, sin código) → P1 (pase mode-aware + GameView, todo sigue
Overlay; D-V4-4 mínimo) → P2 (Camera, 1 canvas) → P3 (World, 1 canvas)
→ P4 (multi-canvas + `sorting_order`) → P5 (interacción Camera/World)
→ P6 (migración audit + cierre, verificación única de Análisis).
Secuencial P0→P1 (orden de Rayo).
