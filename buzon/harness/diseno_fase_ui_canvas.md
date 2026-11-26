# Diseño de la fase UI/Canvas — V0 revisado (2026-11-25)

Decisiones formales de D3m3tr10 (2026-11-25, SÍ a todo en su forma recomendada,
§4/§5 de `EXPEDIENTE_UI_CANVAS_HTML_2026-11-24.md`):

1. CSS v1 = «núcleo bueno»: flex y grid básicos, posición absoluta, tamaños en
   % y automáticos, `aspect-ratio`, recortes (overflow + formas simples),
   fondos con degradado, esquinas redondeadas, 9-trozos, sombras, opacidad y
   transiciones. Fuera de v1 solo lo exótico (keyframes complejos, filtros).
2. Autorado = componentes Unity que generan el documento + campo de «estilo
   extra» CSS por elemento. Sin ficheros .html/.css como assets en v1.
3. InputField por partes: escribir, borrar, cursor y texto por código primero;
   selección con ratón, copiar/pegar después, como deuda registrada.
4. Overlay completo primero; Camera/World como documento-sobre-textura al
   cierre de la misma fase.
5. Texto SDF sencillo (atlas de distancias por fuente, estilo TMP básico).
6. Migración automática al abrir la escena, con copia de seguridad; el
   formato viejo se lee solo para convertir. Un solo sistema vivo.

Revisión cruzada (Opus, contexto fresco, 2026-11-25): NO CONFORME inicial con
25 defectos; **todos aplicados** en esta versión (marcados donde cambian el
planteamiento original).

## Estado de partida (verificado en código el 2026-11-25)

- Datos en `glass_core::scene`: `CanvasData` (render_mode como cadena —
  Camera/World son etiqueta sin implementar —, reference_resolution,
  match_width_or_height), `RectTransformData` (anclas, pivot, size_delta,
  anchored_position), `LayoutGroupData` (H/V reducido), `UITextData`,
  `UIButtonData`, `UIImageData`, `UIDragData`.
- Runtime en `glass_app::canvas_ui` (965 líneas): lote de quads, atlas de
  bits con `ab_glyph`, hit-test de botones (pulsar/soltar dentro), drag &
  drop en partida y puente de acciones por nombre a C#. El editor usa
  `resolve_scene_ui_rects` para los rects de la vista; MCP y C# (Text,
  Image, Button, RectTransform, Canvas) leen del mismo sistema.
- Editor: pestaña «🖼 Canvas / UI» (P10). El texto hoy es atlas de bits.

## Arquitectura

- **Crate nuevo `glass_ui`**: el motor de documentos. Sin dependencias de
  Vulkan ni de egui: entra el árbol derivado + el viewport, salen quads en
  el formato de vértice ampliado (abajo). Editor, runtime y MCP consumen
  el MISMO documento colocado: lo que se ve en el editor es lo que corre.
- **El documento se DERIVA de la escena**: un nodo por cada entidad con
  `RectTransformData` bajo un Canvas (también contenedores sin Graphic:
  son el padre de anclas de sus hijos). Como máximo **un Graphic por
  nodo** (Image, Text o RawImage): si una entidad trae dos, la migración
  crea un hijo sintético para el texto y el Inspector lo marca como error
  de validación. «Panel» no es tipo propio: es un nodo con Image.
  Entidades inactivas o con el componente desactivado quedan fuera de
  layout, pintado y hit-test. Un Canvas anidado es un nodo del documento
  padre: hereda rect y escala, ignora su propia reference_resolution y
  solo aporta frontera de invalidación y `override sorting`; documento
  propio solo para Canvas raíz.
- **Autorado = fuente**: el documento es una vista calculada, no un
  segundo formato. El campo «estilo extra» es `style_extra: String` en
  los datos UI (serde, defecto vacío) y `raycast_target: bool` (defecto
  `true` en Image y Text). Ambos aditivos.
- **Estilos**: `StyleProps` tipado con el conjunto cerrado del «núcleo
  bueno»; el texto CSS es entrada, nunca formato guardado. Cascada:
  defecto del tipo de elemento → campos del componente → `style_extra`.
  Sintaxis del extra: declaraciones + bloques de pseudo-clase `:hover{}`,
  `:active{}`, `:disabled{}`, `:focus{}`, `:selected{}`, sin selectores.
  Errores de parseo: la declaración se ignora, se avisa en Inspector y
  log, nunca pánico (con test de fuzz del parser). Los campos que el
  extra sobrescribe se marcan en el Inspector como «controlado por
  estilo» y dejan de ser editables (equivalente al «driven by» de Unity).
- **Layout**: dos pasadas (medir hijos→padres, colocar padres→hijos).
  **Modo por defecto de todo nodo: absoluto por anclas**, traducido del
  RectTransform. Flex y grid solo si hay LayoutGroup o `display`
  explícito en el extra. El flex que viene de un LayoutGroup implementa
  el reparto de uGUI en tres tramos (mínimo → preferido ∝ (pref−min) →
  flexible), con tabla explícita para spacing, padding, alineación de 9
  posiciones, `childControlSize`, `childForceExpand` y orden inverso.
  Grid: en V2 columnas fijas, `1fr`, celda fija y auto-fill; en V5
  FixedRowCount, flujo por columnas, esquina de inicio y alineación del
  bloque. Unidades: px de referencia previos a la escala del Canvas; %
  sobre padre auto se resuelve como auto. Tamaños auto dependen de un
  **trait de medida de texto** (ancho máximo → tamaño) que V2 implementa
  con el rasterizador provisional y V3 reimplementa sin tocar el layout.
  La escala del Canvas usa la fórmula de Unity (interpolación logarítmica
  con `match`) escrita y con test. La rotación y la escala del Transform
  no afectan al layout; la matriz local se aplica al pintar y su inversa
  en el hit-test. Los rects finales se redondean a píxel físico al pintar.
- **Pintor y GPU (decisión tomada en V1, no en V2)**: una única ampliación
  del formato de vértice y del shader de UI, sin bindless: coordenada
  local al rect, radios de esquina (vec4), rect de clip + softness, modo
  (color / imagen / SDF / sombra) y parámetros de degradado. Degradados
  multiparada por subdivisión del quad. **Alpha premultiplicado en todo
  el lote.** `opacity` y CanvasGroup multiplican el alfa por elemento,
  como uGUI (sin opacidad de grupo con capa intermedia). Interpolación de
  color en el mismo espacio que la UI actual.
- **Orden y capas**: orden del pintor por jerarquía; **capa superior por
  documento**: nodos con `override sorting` + `sort_order` que no heredan
  el clip del padre y se pintan y se prueban al final, en el mismo orden
  (para el desplegable del Dropdown, tooltips y el objeto arrastrado,
  que se promueve a esta capa mientras dura el arrastre). Orden entre
  Canvas raíz: `sort_order` y, a igualdad, índice de hermano.
- **Invalidación**: bandera de sucio por documento y por sub-Canvas; sin
  dirty-tracking fino en v1. Las transiciones activas marcan sucio solo
  mientras corren. Un documento limpio no se recalcula.
- **Interacción**: hit-test en orden inverso exacto al de pintado (capa
  superior incluida), solo nodos con `raycast_target`; el evento **sube
  hasta el primer ancestro con manejador** (el clic en el Text hijo llega
  al Button). CanvasGroup (`blocksRaycasts`, `interactable`) se aplica
  en ese mismo recorrido. Los estados del botón se aplican al nodo de su
  `targetGraphic`, esté donde esté.
- **Texto SDF**: conjunto inicial ASCII + Latin-1 con **inserción
  dinámica por glifo** (presupuesto por frame) y páginas de atlas
  adicionales. Cadena de reserva: fuente del proyecto → fuente por
  defecto → glifo de sustitución. Fuentes sin contorno (solo-bitmap,
  emoji de color) se rechazan al importar con error visible y reserva al
  atlas de bits provisional mientras exista. Clave de caché = hash del
  fichero + tamaño base + spread + conjunto de glifos + versión del
  generador; la caché va incluida en el build y es regenerable. Spread
  fijo documentado (limita el contorno máximo). La API de layout de texto
  devuelve el **mapa carácter → posición** (lo necesita InputField) y los
  modos de desbordamiento (wrap, truncar, puntos suspensivos). Shaping
  complejo y bidi quedan como deuda anotada.
- **Camera/World (V7)**: tamaño del RT = rect × píxeles-por-unidad
  configurable (World, con mipmaps) o tamaño de pantalla (Camera, se
  reasigna al redimensionar). El RT solo se repinta si el documento está
  sucio. Pase insertado antes del pase principal, con barreras
  documentadas; composición premultiplicada; depth test activado y
  escritura según el modo. Hit-test: rayo → plano → UV → coordenadas del
  documento; se descarta si un collider 3D está más cerca y entre varios
  Canvas gana el más cercano. Presupuesto de memoria de RT medido.

## Migración (V1)

- `ui_schema_version: u32` nuevo en la escena; ausente = versión 0, que
  dispara la migración. La conversión es **siempre en memoria** en el
  cargador (editor y runtime); **solo el editor escribe** la copia de
  seguridad y marca la escena como sucia; el runtime nunca escribe.
- Copia con nombre `<escena>.json.pre-ui.bak` (fuera del escáner de
  escenas). Migración **idempotente**: abrir dos veces produce una sola
  copia y ningún cambio; guardar y cargar no altera la escena.
- Tabla campo viejo → `StyleProps`/enum nuevo con defecto y error para
  cada valor no representable: `render_mode` pasa a enum con serde
  (desconocido → Overlay con aviso); correspondencia explícita de
  `LayoutGroupData` (espaciado, márgenes, alineación, control de tamaño);
  `preserve_aspect` de la imagen; la acción por nombre del botón queda
  como evento declarado.
- La migración se aplica también a **prefabs** con UI, y el **importador
  de Unity emite ya la versión actual** (no genera escenas por migrar).
- V1 guarda un **snapshot de los rects del resolutor viejo** por escena
  de ejemplo; V2 compara su layout contra ese snapshot (≤ 0,5 px).

## Sub-bloques (cada uno cierra completo)

- **V1 Frontera y migración.** Tipos del documento y de estilos en
  `glass_ui`; derivación escena → documento con las reglas de arriba;
  decisión del formato de vértice y del shader de UI (documentada, sin
  construirla aún); migración con versión, copia y tabla de campos;
  snapshot de rects viejos. Criterio: conteos, tipos, jerarquía y
  propiedades derivadas sin pérdida en las escenas de ejemplo;
  idempotencia verificada; copia creada una sola vez.
- **V2 Núcleo: estilos, layout, pintor e interacción mínima (Overlay).**
  Parser CSS del extra, cascada, layout completo del núcleo bueno,
  pintor con el formato de vértice ampliado, **paridad de interacción
  con lo vivo**: hit-test con la semántica actual (pulsar/soltar
  dentro), drag con `UIDragData`, acción por nombre a C# y el texto de
  bits **portado** como rasterizador provisional (se retira en V3).
  `resolve_scene_ui_rects`, MCP y C# pasan a leer el documento colocado.
  **Los tests de `canvas_ui` se portan ANTES de borrarlo, y `canvas_ui`
  se retira en este sub-bloque**: un solo sistema vivo. Criterio: rects
  esperados escritos a mano + snapshot viejo con diferencia ≤ 0,5 px +
  imágenes doradas (≤ 2/255 de media, ≤ 1 % de píxeles fuera de
  tolerancia); los tests de clic y drag portados, incluido el botón
  «Jugar», pasan sobre `glass_ui`; editor y runtime dan rects idénticos.
- **V3 Texto SDF.** Atlas SDF con inserción dinámica, layout de texto
  con mapa carácter → posición y modos de desbordamiento; se retira el
  atlas de bits y su rasterizador provisional. Criterio: error máximo de
  cobertura frente a la rasterización directa de `ab_glyph` al mismo
  tamaño, con umbral numérico; ancho de borde ≤ 1,5 px a 12 y a 96 px;
  los tests de layout portados siguen pasando.
- **V4 Autorado Unity.** Inspector: campos de estilo por elemento, marca
  «controlado por estilo», campo CSS extra con avisos de parseo;
  pestaña Canvas/UI crea Panel/Imagen/Texto/Botón/InputField; arrastrar
  en la vista de escena escribe el RectTransform; C# expone las
  propiedades de estilo. Criterio: una pantalla completa creada desde el
  editor sin tocar JSON, y su documento derivado coincide con lo
  esperado.
- **V5 Grupo y recortes.** `CanvasGroup` en cascada, recorte por
  rectángulo con softness y por forma (Mask simple), Grid completo,
  ajustadores (ContentSizeFitter, AspectRatioFitter con FitInParent y
  EnvelopeParent como restricciones propias) y `LayoutElement`.
  Criterio: escenas de prueba por componente con rects y estados
  esperados.
- **V6 Interacción completa.** Estados y transiciones, Toggle, Slider,
  ScrollRect (inercia y elástico), Dropdown (sobre la capa superior),
  **InputField parte 1** (foco, escribir, borrar, cursor, texto por
  código; eventos de texto/IME, no keycodes), eventos generales a C# y
  navegación básica por foco. La deuda de InputField (selección con
  ratón, copiar/pegar) entra en el registro de deudas con identificador.
  Criterio: escenas de prueba por componente; ScrollRect con
  desplazamiento tras inercia dentro de tolerancia; InputField con
  secuencia de teclas → texto y cursor esperados.
- **V7 Cierre de fase.** Camera/World como documento-sobre-textura con
  las especificaciones de arriba; **re-ejecución de la migración desde
  las `.bak` como test de regresión**; fotos doradas de UI para el porte
  nº 2; medición de coste (escena de 500 nodos: recálculo completo en ms
  p95 y memoria de RT; límite: no peor que `canvas_ui` en las escenas de
  ejemplo — dato para el porte nº 9); test de sistema único en CI (falla
  si existe el módulo `canvas_ui`, el atlas de bits o un lector del
  formato viejo fuera del migrador); release de la fase.

## Reparto y reglas de la fase

- **Claude Opus**: V2 (layout y pintor), V3 (SDF) y V7 (RT) — el peso
  mayor. **DeepSeek flash**: parser CSS, migrador, ports de tests y los
  componentes de V5 y V6 sobre el núcleo cerrado. Kimi aparcado por
  saldo (su carga repartida así, según la orden del 2026-11-25 08:33).
  Revisión cruzada obligatoria en V1 y V2.
- Cierre por sub-bloque: pipeline local exacto de ci.yml, CI real, test
  físico medido (escena y puntos de medición preparados antes;
  inspeccionar Inspector, archivo de escena y números), barrido
  anti-regresión e informe al principal con consumo por proveedor.
- Nada de bindless/render graph (lista de «No» de Jasper). Nada de modo
  2D: esto es Canvas/UI, no un sistema 2D.

---

## PAUSA ORDENADA POR D3m3tr10 (2026-11-25 12:54) — punto de retoma

La fase UI/Canvas queda PAUSADA aquí para cerrar primero el importador de
`.unitypackage`. Estado exacto al pausar:

- **V1 implementado e integrado en el árbol** (sin commitear al pausar; el
  commit entra con el cierre del importador porque el árbol es uno):
  - Crate nuevo `glass_ui`: `documento.rs`, `estilos.rs` (StyleProps),
    `derivacion.rs` (Opus: escena → documentos), `migracion.rs` (DeepSeek:
    migración en memoria + copia `.pre-ui.bak` idempotente, solo editor).
  - `glass_core::scene`: `style_extra` en UITextData/UIButtonData/UIImageData,
    `raycast_target` en UITextData/UIImageData, `UI_SCHEMA_VERSION_ACTUAL = 1`,
    `parsear_escena`/`serializar_escena` (raíz array v0 u objeto v1).
  - Cableado en `glass_app::canvas_ui::migrar_ui_escena_cargada` en las 4 vías
    de carga + migración de prefabs (main.rs y 2 sitios de docking.rs).
  - Snapshot de paridad para V2 congelado:
    `crates/glass_ui/tests/fixtures/rects_canvasuitest_1280x720.json`
    (generador: `crates/glass_ui/examples/snapshot_rects.rs`).
  - Tests de glass_ui: 13/13 en verde al pausar.
- **Pendiente de V1 al retomar** (no rehacer nada de lo anterior):
  1. Test físico medido con el fixture ya preparado:
     `~/workspace/.tmp/ui_v1_fisico/escenas/EscenaMigracion.scene.json`
     (CanvasUiTest con render_mode «Inventado» y ui_image en la entidad 12;
     esperado: log «Migración UI aplicada (1 entidades creadas, 2 avisos)»,
     `.pre-ui.bak` idéntica a `original_referencia.json`, segunda apertura
     sin reescribir la copia).
  2. Informe de cierre de V1 al principal con el consumo acumulado:
     Opus diseño 43.393 + Opus derivación 20.233 + DeepSeek migración
     17.868 + continuación 1.648 (la parte de V1; el consumo del convertidor
     y de materiales se declara en el cierre del importador).
- **Siguen V2–V7** según este mismo diseño (núcleo/pintor con paridad contra
  el snapshot, SDF, autorado, grupo/recortes, interacción, Camera/World y
  cierre con release).
