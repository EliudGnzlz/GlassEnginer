# Análisis V3 — Texto SDF (UI/Canvas)

Primera línea: dueña **S21** (UI/Canvas V2) + vecinas **S10** (UI clásica),
**S14** (editor), **S3** (MCP), **S2** (C#), **S1** (formato de escena, no
tocado). Rama de trabajo: `lu/v3-sdf` (desde `origin/main` o desde la punta
de V2 según diga Rayo; nunca `main`). Reparto de la fase: **Opus** hace V3
(peso mayor); DeepSeek flash apoya en lo mediano; Kimi aparcado (sin saldo).

## 1. Criterio del diseño (diseno_fase_ui_canvas.md, V3)

> Atlas SDF con inserción dinámica, layout de texto con mapa carácter →
> posición y modos de desbordamiento; se retira el atlas de bits y su
> rasterizador provisional. Criterio: error máximo de cobertura frente a
> la rasterización directa de `ab_glyph` al mismo tamaño, con umbral
> numérico; ancho de borde ≤ 1,5 px a 12 y a 96 px; los tests de layout
> portados siguen pasando.

Detalle del diseño (§ Texto SDF): conjunto inicial ASCII + Latin-1 con
**inserción dinámica por glifo** (presupuesto por frame) y páginas de atlas
adicionales. Cadena de reserva: fuente del proyecto → fuente por defecto →
glifo de sustitución. Fuentes sin contorno (solo-bitmap, emoji de color) se
rechazan al importar con error visible y reserva al atlas de bits
provisional mientras exista. Clave de caché = hash del fichero + tamaño
base + spread + conjunto de glifos + versión del generador; la caché va
incluida en el build y es regenerable. Spread fijo documentado (limita el
contorno máximo). La API de layout de texto devuelve el **mapa carácter →
posición** (lo necesita InputField) y los modos de desbordamiento (wrap,
truncar, puntos suspensivos). Shaping complejo y bidi quedan como deuda
anotada.

## 2. Estado inicial verificado en código (2026-11-26)

- `crates/glass_renderer/src/ui.rs`: `FontAtlas` es un atlas **de bits**:
  rasteriza una vez a 64 px (`ATLAS_RASTER_PX = 64.0`) con `ab_glyph`,
  ASCII + Latin-1, empaquetado por estantes, bloque sólido 8×8. API:
  `glyph()`, `measure()`, `line_height_at()`, `solid_uv`, `ascent`,
  `line_height`. El shader `ui.frag` (rama MODO_TEXTO) usa
  `texture(atlas_tex, v_uv).a` como **cobertura** directamente — no evalúa
  distancia (la etiqueta «Texto/SDF» del shader es aspiracional).
- `crates/glass_ui/src/layout.rs`: `MedidorTexto` mide con `ab_glyph`
  directo (`MedidaTexto::medir(texto, tamano, ancho_max) -> (w, h)`),
  sin mapa carácter → posición y sin modos de desbordamiento.
- `crates/glass_app/src/ui_v2.rs`: el lote V2 de texto pasa por
  `push_text_extendido` (MODO_TEXTO) con el atlas de bits; `UiFonts`
  resuelve la fuente por nombre.
- El pintor (`glass_ui::pintor`) emite `Segmento.texto` con
  (texto, font_size, align, font); el conversor resuelve el atlas.
- Tests de layout portados: 23/23 en verde (V2); lib `glass_ui` 48 passed
  (antes del cierre clippy de V2). Suite workspace 1.390 passed.

## 3. Paquetes

### V3a — Atlas SDF + inserción dinámica (Opus)
Nuevo tipo de atlas (p. ej. `SdfAtlas`) en `glass_renderer`: por glifo,
campo de distancia con signo generado desde el contorno de la fuente
(`ab_glyph` → outline → distancia; algoritmo acotado por bbox del glifo
ampliada por el spread, nunca fuerza bruta sobre el atlas entero).
Conjunto inicial **ASCII imprimible (95, 32–126) + Latin-1 (96, 160–255)
= 191 glifos**; **inserción dinámica por glifo con presupuesto por frame**
(propuesta: 8 glifos/frame, el resto en cola FIFO; el conjunto inicial
completo se llena en ≤ 24 frames) y páginas adicionales de 1024×1024
cuando se llena una, **con tope de 4 páginas**: si se agotan, los glifos
nuevos resuelven al glifo de sustitución con aviso (sin desalojo en V3;
el desalojo es deuda para V4+). Clave de caché = hash del fichero +
tamaño base + spread + conjunto de glifos + versión del generador; la
caché es **solo de build** (se genera en el build, va incluida en él y es
regenerable; el runtime nunca escribe en disco: si la clave difiere,
regenera en memoria y avisa con `log::warn`). Cadena de reserva: fuente
del proyecto → fuente por defecto → glifo de sustitución ('?'). Glifo
sin contorno → sustitución + aviso (el aviso va a `log::warn` y se cuenta
en las estadísticas del atlas). **Tras V3d no queda reserva al atlas de
bits**: una fuente sin contorno resuelve al glifo de sustitución (el
rechazo en importación con error visible es de la herramienta de import,
deuda D-V3-1, fuera del runtime; en V2 el atlas de bits tampoco
rasterizaba fuentes sin contorno, así que no hay regresión).

### V3b — Evaluación SDF en el shader (Opus)
Rama MODO_TEXTO de `ui.frag`: `smoothstep` sobre la distancia con
**anti-aliasing en espacio de pantalla vía `fwidth()`** (el semi-ancho del
borde es ~1 px de pantalla, independiente del tamaño: así el borde ≤ 1,5
px se cumple a 12 y a 96 px a la vez). El spread fijo sigue siendo el
**contorno máximo** que el shader puede dibujar (en unidades de atlas);
si un estilo pide más contorno, el pintor lo recorta al spread y avisa
una vez por documento (detección definida, no solo deuda). La cobertura
resultante premultiplica el RGB igual que hoy (lección P4c: con mezcla
premultiplicada el RGB debe bajarse por la cobertura). `.spv` regenerado
y commitado junto a su `.glsl` (gate G0 de V2, sigue vigente). Sin
cambios al formato de vértice (el modo ya existe; la distancia viaja en
el canal que hoy lleva la cobertura).

### V3c — Layout de texto con mapa y desbordamiento (Opus: mapa de layout;
DeepSeek flash: brazo del parser + tests de desbordamiento)
`glass_ui`: la API de layout de texto devuelve el **mapa carácter →
posición**: índice de carácter → **rect de celda de avance** `[x, y,
ancho_avance, alto_línea]` en coordenadas de documento (el avance
incluye el kerning que da `ab_glyph`, igual que hoy; el rect visual del
glifo se lee del atlas). Los modos de desbordamiento **wrap / truncar /
puntos suspensivos** llegan como nuevo campo en `StyleProps` (p. ej.
`text_overflow`, con su brazo en el parser). `MedidorTexto` sigue
midiendo con `ab_glyph` (la métrica no cambia: el avance por glifo es el
mismo); el mapa se construye en el layout. Tests propios de V3c: mapa
contra avances calculados a mano para una cadena conocida; wrap, truncar
y «…» con escenas de prueba; cadena de reserva (fuente de proyecto
ausente → por defecto; glifo ausente → '?'; fuente sin contorno → '?'
+ aviso). Los tests de layout portados (23/23) deben seguir pasando.

### V3d — Retirada del atlas de bits (Opus)
Se borran `FontAtlas` (bits) y su rasterizador provisional; todos los
llamadores pasan al atlas SDF. Inventario completo de llamadores
(verificado por grep 2026-11-26): `glass_renderer/src/ui.rs`
(definición + `UiRenderer` + `ui.font()`), `glass_app/src/main.rs`
(`ui_font_atlases`, `FontAtlas::build`, `renderer.ui_font()`),
`glass_app/src/ui_v2.rs` (`UiFonts`, `atlas_fuente`, tests),
`glass_renderer/tests/ui_dorada.rs` (harness de la dorada).
`glass_editor` y `glass_mcp` **no** usan el atlas de bits (la GameView
dibuja el texto con egui). Un solo commit revertible, como P4d. El test
de sistema único de V7 (aún no) no aplica aquí; la puerta es la suite +
doradas en verde.

**Orden de ejecución**: V3a → V3b → V3c → V3d, cada uno en su commit.
V3b depende del formato/canal SDF que produce V3a; V3c solo depende de
V3a/V3b para su test de integración final; V3d depende de V3a+V3b y de la
migración de todos los llamadores de arriba.

## 4. Puertas

- **G-SDF1 (cobertura)**: error de cobertura del atlas SDF frente a la
  rasterización directa de `ab_glyph` al mismo tamaño, sobre el conjunto
  ASCII + Latin-1 (191 glifos). Método: muestreo de la cobertura evaluada
  por el SDF y del rasterizado directo **sobre la misma rejilla de
  píxeles** (bbox del glifo al tamaño objetivo), diferencia absoluta por
  píxel, agregada como media y p99 sobre todos los píxeles de todos los
  glifos. Umbral (propuesta; §7 Q2): **media ≤ 0,03 y p99 ≤ 0,10**, medido
  a **12, 24, 48 y 96 px**. Ancho de borde medido en un glifo renderizado:
  **≤ 1,5 px a 12 y a 96 px** (del diseño; con el AA por `fwidth()` de
  V3b el borde es ~1 px de pantalla a cualquier tamaño).
- **G-SDF2 (regresión)**: los 23 tests de layout portados siguen en verde;
  lib `glass_ui` en verde; suite workspace en verde; doradas en verde
  (incluido el test blanco-sobre-negro de P4c).
- **G-SDF3 (paridad visual)**: la dorada V2 y una captura de texto SDF a
  12/96 px comparadas con la rasterización directa (misma tolerancia que
  V2: media ≤ 2/255, ≤ 1 % fuera). S10 se valida con este criterio sobre
  la escena dorada V2 (el cambio es visual y «igual» significa: misma
  tolerancia numérica, no apreciación a ojo).

## 5. Riesgos

- **R1 — Coste de generación SDF**: el campo de distancia por glifo puede
  ser caro si se hace por fuerza bruta. Mitigación: distancia exacta solo
  dentro de la bbox del glifo ampliada por el spread; fuera, distancia al
  borde de la bbox (aproximación documentada); el test mide el tiempo por
  glifo: **p95 < 1 ms a 64 px sobre los 191 glifos del conjunto inicial,
  en esta VM** (criterio de fallo definido).
- **R2 — Inserción dinámica y presupuesto por frame**: si el presupuesto
  se agota, los glifos que faltan se quedan en cola y el texto sale con
  sustitutos un frame. Mitigación: el presupuesto es por frame y la cola
  es FIFO; el test mide que el conjunto inicial (191 glifos) se llena en
  ≤ 24 frames a 8/frame.
- **R3 — Spread fijo**: limita el contorno máximo (outline) que el shader
  puede dibujar. Mitigación: spread documentado en el código y en el
  informe; si un estilo pide más contorno, el pintor lo recorta al spread
  y avisa una vez por documento (detección definida en V3b).
- **R4 — Fuentes sin contorno**: un .ttf solo-bitmap o emoji de color no
  tiene de dónde sacar distancia. Mitigación: sustitución + aviso en el
  atlas; el rechazo en importación es de herramienta, fuera de V3.
- **R5 — Regresión de layout**: el cambio de atlas no debe mover ni un
  rect. Mitigación: las métricas (avance, ascenso, altura de línea) se
  siguen leyendo de `ab_glyph`, no del atlas; G-SDF2 lo cubre.

## 6. Plan físico

Escena de texto con «JUGAR» y un párrafo a 12/24/96 px (ASCII + Latin-1
con tildes y eñe): captura en editor (GameView) y en standalone; medida
de cobertura y de ancho de borde contra la rasterización directa;
desbordamiento wrap/truncar/«…» con escenas de prueba; inserción dinámica
observada (glifos nuevos aparecen sin parpadeo visible más allá de un
frame). Barrido: S21 dueña + S10 (UI clásica renderiza igual), S14
(editor: la pestaña de texto no cambia), S3 (MCP lee rects, sin cambio),
S2 (C# sin cambio). S1 no tocado → sin barrido completo.

## 7. Preguntas para Rayo

- **Q1 — Spread**: propuesta **4,0 px a 64 px base** (1/16 del em). ¿Lo
  fijamos así?
- **Q2 — Umbral de cobertura**: propuesta **media ≤ 0,03 y p99 ≤ 0,10**
  (12/24/48 px, ASCII + Latin-1). ¿Lo fijamos así?
- **Q3 — Páginas del atlas**: ¿1024×1024 como el atlas de bits, o
  2048×2048 para dejar más margen?
- **Q4 — Presupuesto de inserción**: propuesta **8 glifos/frame**. ¿Lo
  fijamos así?
- **Q5 — Rama**: ¿`lu/v3-sdf` desde `origin/main` (con V2 aún sin
  integrar) o espero a que integres V2?

## 8. Cierre

Pipeline exacto del CI en local + CI real + test físico medido + barrido
§6 + consumo por proveedor + revisión de peces única al final del módulo
(sobre la rama entera). Cierre y consumo en el buzón por paquete, como en
V2. Presupuestos: paquetes Opus ≤ 300 líneas por envío, máx. 2
iteraciones; freno `--max-razonamiento` armado (25.000) en llamadas
grandes; paquetes partidos por tamaño de salida.

## 9. Resoluciones de peces (2026-11-26)

Dos peces, familias distintas, solo con este análisis como contexto
(resultados en `pez1_v3_result.json` — Opus, comprensión, razonamiento 0 /
contenido 5.324 — y `pez2_v3_result.json` — DeepSeek flash, lente crítica,
razonamiento 18.776 / contenido 2.434). Paráfrasis del pez 1 correcta.

### Pez 1 (Opus)
- **B1 — Q5 sin cerrar y excluyente** → ESCALAR a Rayo (§7 Q5): la base
  de la rama V3 (desde `origin/main` sin V2 o desde la punta de V2) la
  decide él; sin su respuesta no se ejecuta V3a/V3d.
- **B2 — Incoherencia de umbral (12/24/48 vs 12/96)** → APLICAR: G-SDF1
  mide cobertura a 12, 24, 48 **y 96** px; el borde se mide a 12 y 96.
- **B3 — R1 sin umbral verificable** → APLICAR: p95 < 1 ms/glifo a 64 px
  sobre los 191 glifos, en esta VM.
- **M1 — Método de comparación de G-SDF1 sin fijar** → APLICAR: misma
  rejilla de píxeles, diferencia absoluta por píxel, media y p99.
- **M2 — Contrato del mapa V3c sin especificar** → APLICAR: rect de celda
  de avance en coordenadas de documento, kerning de `ab_glyph` incluido.
- **M3 — Sin presupuesto para página llena** → APLICAR: tope de 4 páginas;
  agotadas, sustitución + aviso; sin desalojo en V3 (deuda V4+).
- **M4 — Ciclo de vida de la caché** → APLICAR: solo de build; el runtime
  nunca escribe; si la clave difiere, regenera en memoria + aviso.
- **M5 — Contradicción aritmética spread/borde** → APLICAR: el borde se
  evalúa con AA por `fwidth()` en espacio de pantalla (~1 px a cualquier
  tamaño); el spread queda solo como contorno máximo.
- **M6 — V3d sin inventario completo** → APLICAR: inventario por grep en
  §3 V3d (`glass_editor` y `glass_mcp` no usan el atlas).
- **m1 — Canal del aviso** → APLICAR: `log::warn` + contador en las
  estadísticas del atlas.
- **m2 — Rechazo en importación como nota** → APLICAR: deuda explícita
  D-V3-1 (herramienta de import, fuera del runtime).
- **m3 — N=12 mal calculado** → APLICAR: 24 frames para 191 glifos.
- **m4 — Criterio de «igual» para S10** → APLICAR: G-SDF3 con tolerancia
  numérica sobre la dorada V2.
- **m5 — Reparto V3c sin delimitar** → APLICAR: Opus hace el mapa de
  layout; DeepSeek, el brazo del parser + tests de desbordamiento.
- **m6 — §9 vacío** → APLICAR: esta sección.

### Pez 2 (DeepSeek)
- **B1 — Q1/Q2 abiertas** → ESCALAR a Rayo (§7 Q1/Q2): spread y umbral
  los fija él; las propuestas (4,0 px; media ≤0,03/p99 ≤0,10) quedan en
  el análisis.
- **B2 — Q5 sin resolver** → ESCALAR: ídem pez 1 B1.
- **B3 — Contradicción fuentes sin contorno vs V3d** → APLICAR: tras V3d
  no hay reserva al atlas de bits; fuente sin contorno → '?' + aviso
  (sin regresión: en V2 tampoco se rasterizaban).
- **M4 — Dependencias entre paquetes** → APLICAR: orden V3a→V3b→V3c→V3d
  declarado en §3, un commit por paquete.
- **M5 — Q3/Q4 abiertas + R2** → ESCALAR (Q3/Q4) + APLICAR (N=24).
- **M6 — G-SDF1 incompleta** → APLICAR: ídem pez 1 M1 (+ medida del
  borde sobre glifo renderizado).
- **M7 — V3c sin aceptación propia** → APLICAR: tests propios de V3c
  declarados en §3 (mapa, wrap/truncar/«…», cadena de reserva).
- **M8 — R3 sin mitigación verificable** → APLICAR: recorte al spread +
  aviso una vez por documento (V3b).
- **M9 — Cadena de reserva sin prueba** → APLICAR: tests de reserva en
  §3 V3c.
- **M10 — Caché ambigua** → APLICAR: ídem pez 1 M4.
- **M11 — Barrido sin criterio medible** → APLICAR: S10 por G-SDF3;
  S14/S3/S2 por suite + tests V2 que los cubren.
- **m12 — Corpus sin concretar** → APLICAR: 191 glifos (95 + 96); 1
  página esperada, máx. 4.
- **m13 — Canal del aviso** → APLICAR: ídem pez 1 m1.

No se usó re-auditoría. Las preguntas Q1–Q5 van a Rayo por el buzón; sin
su respuesta a Q5 no se ejecuta ningún paquete (B1/B2 de ambos peces).

## 10. Deuda registrada

- **D-V3-1 — Rechazo de fuentes sin contorno en importación**: la
  herramienta de import debe rechazarlas con error visible. Frente de
  herramienta, fuera del runtime de V3.
