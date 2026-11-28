# INFORME V5 — Grupos y recortes (módulo UI/Canvas)

Rama `lu/v5-grupos`. Cierre de módulo 2026-11-28.
**V5 NO se declara verde ni se integra hasta la verificación única de Análisis.**

## 1. Alcance

V5 es **grupos de layout y recortes (máscaras) de UI/Canvas**: autorado de
LayoutGroup, LayoutElement, ContentSizeFitter, GridLayoutGroup y
RectMask2D en el editor (secciones del Inspector + Add Component +
presets), registro MCP con round-trip, y el modelo único de clip (A6:
CSS `overflow:hidden` y RectMask2D se intersectan en el mismo acumulador).
No forman parte de V5: ScrollRect (deuda M6), máscara alfa (deuda M4),
InputField dentro de V5 (deuda Q6), los modos Camera/World del Canvas
(V7), ni las tools MCP de UI.

## 2. Paquetes

### V5a — autorado + formato (DeepSeek flash)
- `LayoutGroupData` (direction, spacing f32, padding [f32;4], alignment
  String con 9 valores + 3 alias legacy start/center/end, 12 aceptados)
  en `crates/glass_core/src/scene.rs:912`; `layout_group_rects` pura;
  `EntityData.layout_group` + materialización; registro en
  `componentes.rs` (COMPONENTES + brazos get/set, ejercido por las tools
  EXISTENTES `glass_component_set` / `glass_component_get`, sin tools
  nuevas) + round-trip MCP + sección del Inspector + Add Component +
  presets «Panel con fila/columna».
- Suite 1.462/0. CI #526 Success 3m 4s. Físico exacto (panel blanco,
  hijos rojo/azul, gap 5,33 px = 8×2/3). Consumo: DeepSeek razonamiento
  163.348 / contenido 25.570; Opus 0; Kimi 0.

### V5b-1 — G7-mínimo + G3 Control Size sin flexible (Opus, freno 25.000)
- `LayoutElementData` (min/preferred Option<f32> ×2, ignore_layout) +
  4 flags en el grupo (child_control/force_expand × ancho/alto) +
  pasada bottom-up de preferidos (post-orden) + `layout_group_rects_control`
  (encogido proporcional hacia el mínimo, nunca bajo el mínimo, mínimo
  absoluto 10.0 — suelo preexistente de `resolve_ui_rect_in`,
  scene.rs:1401, no invento de V5). Anidado H-dentro-de-V como dorada
  obligatoria (el encogido se cubre con las doradas unitarias, no con Q7).
- Suite 1.474/0. CI #539 Success 3m 4s con Clippy limpio (tras dos
  incidentes: unused-mut #536 y needless_range_loop, ambos arreglados).
  Físico exacto (5 rects). Consumo: Opus prompt 14.057 B + 20.261 B /
  recibido 16.801 + 39.426 caracteres; DeepSeek 0; Kimi 0.

### V5b-2 — G6 ContentSizeFitter + flexible (Lu directo)
- `ContentSizeFitterData` (horizontal_fit/vertical_fit: none/min/preferred)
  + `LayoutElementData.flexible_width/height` (f32, default 0). El fitter
  usa los tamaños (mín, preferido) del PROPIO nodo (bottom-up) y se aplica
  top-down antes de repartir; el fitter del hijo gana al reparto del padre.
  El sobrante con Force Expand se reparte en proporción a los pesos
  flexibles (sin pesos, a partes iguales).
- Suite 1.486/0. CI #545 Success 2m 30s con Clippy limpio. Físico exacto:
  V [0,0,800,270] (el fitter encoge el RT de 600 a 270), H [20,20,760,120],
  A [30,30,271,100], B [309,30,461,100], C [20,150,760,100]. Aritmética
  del reparto: interior de H 740 (760−padding 20); preferidos
  160+240+spacing 8 = 408; sobrante 332; reparto 1:2 → +110,67/+221,33 →
  270,67→271 y 461,33→461. Consumo 0.

### V5b-3 — G5 GridLayoutGroup (DeepSeek flash)
- `GridLayoutGroupData` (cell_size, spacing, padding, start_corner,
  start_axis, alignment 9 valores, constraint flexible/fixed_column_count/
  fixed_row_count, constraint_count) + `layout_grid_rects` y
  `calcular_columnas_grid` puros; bottom-up (mín == preferido); la rejilla
  gana al grupo cuando ambos están presentes; el CSS `display:grid`
  (aplicar_grid, P2c) NO se toca — es otro sistema. La rejilla IGNORA
  preferred/mín/flexible de los hijos (manda el tamaño de celda, como en
  Unity): decisión de diseño, no deuda.
- Suite 1.495/0. CI #548 Success 3m 3s con Clippy limpio. Físico exacto:
  panel blanco [20,20,1240,250]; celdas A rojo [40,40,200,100], B azul
  [250,40,200,100], C verde [460,40,200,100], D amarillo [40,150,200,100],
  E magenta [250,150,200,100], F cian [460,150,200,100]; barra
  [20,280,1240,100]. Consumo: DeepSeek razonamiento 51.669 / contenido
  36.581; Opus 0; Kimi 0.

### V5c — máscara M1/M2/M3/M5 (spike + DeepSeek flash)
- **Spike M3** (commit 7f88a3b, Lu directo): `UiVertex` 160 B con
  `clip_radios` (loc 11, offset 144); regla de radios del pintor por
  borde ganador; test del pintor + test GPU permanente
  `clip_redondeado_recorta_en_gpu`; **dorada `ui_dorada_1280x720.png`
  regenerada y promovida** (diff **4.488 px** con umbral >2 por canal:
  esquinas del panel rojo 53×4 = 212, esquinas del panel blanco 29×4 =
  116, banda del texto «JUGAR» 4.160, resto 0; el panel degradado no
  cambia porque no tiene radios).
- **Causa raíz del spike: el clip NUNCA recortó nada desde V2.** La mezcla
  es premultiplicada (ONE / ONE_MINUS_SRC_ALPHA, ui.rs:749-750) y los pasos
  4 (rect propio), 5 (sombra) y 6 (clip) de `ui.frag` y `ui_image.frag`
  solo escalaban `base.a`, dejando `base.rgb` entero. Arreglo: los tres
  pasos escalan `base` entero. **Prueba falsable:** captura pre-arreglo
  `v5c_spike_01.png` — (750,200), (720,200), (780,200) y (750,180) =
  **(238,125,155)** rosa plano (sRGB de linear_rojo + linear_cielo, el
  hijo a plena intensidad fuera del clip); captura post-arreglo
  `v5c_spike2_01.png` — los mismos puntos = **(93,120,152)** cielo.
- **Paquete** (commit 76fd1c9): `RectMask2DData` (padding px canvas,
  softness px pantalla); clip único A6; el padding se escala por el
  factor del canvas; hit-test y arrastre usan el mismo clip (SDF de rect
  redondeado, borde duro). El andamiaje de DeepSeek se reescribió contra
  el código real.
- Suite 1.504/0. CI #558 Success 2m 59s con Clippy limpio. Físico exacto
  (panel [100,100,600,400], rojo visible [120,120,560,40], arco y
  softness medidos). Consumo: DeepSeek razonamiento 45.272 / contenido
  25.163 (spike Lu directo); Opus 0; Kimi 0.

### V5d — cierre del módulo (Lu directo)
- Físico Q7 (§3), barrido S1–S23 (§6), revisión peces (§7), este informe.
- Dos tests nuevos en `crates/glass_ui/src/pintor.rs` por los peces:
  `pintor_mascara_padding_escala_con_canvas` (el padding se escala por
  s=2/3: [30,15,30,15] → clip [120,110,560,380]; softness sin escalar) y
  `pintor_interseccion_vacia_descarta_el_nodo` (dos ancestros disjuntos
  → clip cero y el pintor no emite segmentos del hijo). Pipeline local
  exacto del CI en verde (build, clippy, juego puro, dotnet, suite
  **1.506/0**). CI de GitHub del commit de cierre: por verificar.
- Sin cambios de comportamiento: solo tests + informe.

### Precedencias (de mayor a menor, verificadas en código y tests)
1. La rejilla gana al grupo en el mismo nodo (rama grid primera en
   `aplicar_flex_con_preferidos`).
2. El fitter se aplica antes de repartir; el fitter del hijo gana al
   reparto del padre (el nodo anidado se coloca con su rect ajustado y
   luego reparte a sus hijos).
3. Dentro de un grupo mandan preferred/mín/flexible de LayoutElement.
4. La máscara recorta todo lo que tiene debajo, sea cual sea el layout.
5. La capa superior rompe la cadena de clip.

## 3. Q7 — el físico de cierre (V5d)

Escena `hidden_files/v5d_fisico/EscenaV5d.scene.json` (7 entidades, canvas
[1280,720], s=1). Ventana (id 2) [100,100,600,400] blanca radios 40 +
RectMask2D padding 20 softness 2.5 → máscara [120,120,560,360], arco
sup-izq centro (160,160) r=40. **La Ventana NO lleva `overflow:hidden`:
su radio 40 es solo su propio borde pintado (`border-radius: 40px` en
su `style_extra`); el único clip de los hijos es la máscara, que toma
sus radios del `border_radius` del estilo del nodo máscara.** Grupo V
(id 3) hijo de la Ventana, relativo [20,20,560,360] → absoluto
[120,120,560,360]; vertical, spacing 10, padding 10, control W+H,
expandir ancho SÍ, expandir alto NO. Hijos con LayoutElement pref_h: A
naranja #ff8000 80 → [130,130,540,80]; B azul #2020e0 120 →
[130,220,540,120]; C verde #20a020 60 → [130,350,540,60] (total 260 +
spacing 20 = 280 ≤ 340 interior, sin encogido). Hijo Rojo (id 7) hijo
de la Ventana DESPUÉS del grupo (pinta encima), relativo [-10,-30,300,80]
→ absoluto [90,70,300,80]; visible = hijo ∩ máscara menos el arco
sup-izq.

**Convención de medida:** los bboxes son índices de píxel INCLUSIVOS
(ancho = x1−x0+1); los rects son exactos sin tolerancia; los colores
llevan **±2** por el redondeo sRGB→lineal→sRGB del pintor (canales bajos
32→34).

**Editor:** jerarquía correcta; Inspector Grupo V: Vertical, Espaciado
10.0, Padding 10×4, start, Controlar ancho/alto SÍ, Expandir ancho SÍ,
Expandir alto NO; Inspector Ventana: RectMask2D Padding 20×4, Softness
2.5; File → Save Scene conserva las 7 entidades, la jerarquía y los 5
componentes nuevos en el JSON.

**Standalone** (captura `v5d_standalone_a.png`, sha256
`f56fc882031a2210160489ee80db9d95de961cb618ac1ace95bd4308db074984`):
10/10 puntos dorados y 5/5 bboxes exactos por píxel:
- (400,140) naranja (255,128,0); (200,140) rojo (224,34,34); (125,125)
  blanco (fuera del arco, dist 49,5>40 a (160,160)); (170,125) rojo
  (banda superior recta, x>160: dentro del rect sin tocar el arco);
  (200,180) naranja (dentro de A, por debajo de la zona que cubre el
  rojo); (600,250) azul (34,34,224); (600,380) verde (34,160,34) — R y
  B 32→34 por el redondeo, G=160 exacto; (650,460) blanco; (750,300)
  negro; (300,105) blanco.
- Bboxes: naranja [130,130,669,209] (540×80); azul [130,220,669,339]
  (540×120); verde [130,350,669,409] (540×60); rojo x0=121
  (arco+softness, rango aceptado 118–124), x1=389, y0=120, y1=149
  (269×30, el área VISIBLE = hijo ∩ máscara, no el rect del hijo);
  blanco [100,100,699,499] (600×400).
- **Determinismo:** segunda captura standalone `v5d_standalone_b.png`
  con el MISMO sha256, 0 píxeles distintos.

## 4. Hallazgo de raíz del módulo

**H-V5-1. El clip nunca recortó nada desde V2 (spike M3).** Con mezcla
premultiplicada, los pasos 4/5/6 de los shaders solo atenuaban `base.a`.
El recorte rectangular de CSS y las esquinas redondeadas eran decorativos:
todo lo que debía quedar fuera se pintaba a plena intensidad. La
reproducción en el harness dorado lo probó (el hijo pintaba rosa plano
(238,125,155) = sRGB(linear_rojo + linear_cielo) sobre el cielo). Arreglo
de raíz en `ui.frag` y `ui_image.frag` (los tres pasos escalan `base`
entero) + test GPU permanente + dorada regenerada. Lección: con mezcla
premultiplicada, cualquier desvanecido SDF debe escalar `base` entero;
escalar solo el alfa no recorta nada. Prueba falsable (§2 V5c): captura
pre-arreglo con el hijo en rosa (238,125,155) fuera del clip, captura
post-arreglo con cielo (93,120,152) en los mismos puntos.

## 5. Pipeline (exacto del CI, en local para 76fd1c9; CI real #558)

- `cargo build --workspace --locked` con `RUSTFLAGS="-D warnings"`: OK.
- `cargo clippy --workspace --all-targets` con `RUSTFLAGS="-D warnings"`: OK
  (RC 0). Solo los 3 avisos de manifiesto de cargo (rust-version,
  glass_core en glass_audio, egui-winit en glass_editor).
- `cargo build -p glass_app --no-default-features --locked`: OK.
- `dotnet build managed/GlassEngine.Core`: 0 errores.
- `xvfb-run -a cargo test --workspace --locked`: **1.504 passed / 0 failed**.
- CI real #558 (46114521811) Success 2m 59s, job 2m 57s, todos los pasos
  en verde; por crate: glass_app 87, glass_audio 13, glass_core 334,
  glass_editor 265, glass_fbx 5, glass_hub 12, glass_launcher 112,
  glass_mcp 40, glass_mesh_edit 51, glass_particles 8, glass_physics 169,
  glass_renderer 127, glass_ripper 125, glass_scripting 28, glass_ui 128.

## 6. Barrido anti-regresión S1–S23

Sección dueña S21 (UI/Canvas). V5 toca formato de escena (S1: 5 campos
nuevos en EntityData) → barrido completo por la fila 1 del mapa
(`INVENTARIO_BARRIDO_SECCIONES_2026-11-25.md`, 2026-11-25). Evidencia:
suite por crate (CI #558) + físico Q7 (S21/S10/S14) + carga legada (S1)
+ dorada (S5/S6) + round-trips MCP (S3/S23). Las filas sin conteo propio
se cubren por la suite completa del crate que las implementa (glass_core
334, glass_physics 169, glass_app 87, glass_scripting 28, glass_ripper
125); los tests no se agrupan por sección, así que no existe un conteo
por sección.

| Sec. | Estado | Evidencia |
|---|---|---|
| S1 escena/serialización | pasa | Carga legada V5a (§5 de PECES_V5); suite glass_core 334/0; Save Scene Q7 conserva los 5 campos nuevos |
| S2 scripting C# | pasa | Suite glass_scripting 28/0; dotnet 0 errores; sondas UiDynTest en verde por suite |
| S3 MCP base | pasa | Suite glass_mcp 40/0; tools/list responde; 5 round-trips nuevos en verde |
| S4 física | pasa | Suite glass_physics 169/0 |
| S5 render/materiales | pasa (condicional) | Suite glass_renderer 127/0; dorada ui_dorada en verde con la fixture regenerada — la independencia la da la re-verificación de Análisis, no el propio cambio |
| S6 shaders | pasa (condicional) | .spv de ui.vert/ui.frag/ui_image.frag regenerados en el spike; dorada en verde; sin cambios en el grafo; misma condición que S5 |
| S7 iluminación | pasa | Suite en verde (sin cambios de V5 en luces) |
| S8 audio | pasa | Suite glass_audio 13/0 |
| S9 animación | pasa | Suite en verde (V5 no toca Animator) |
| S10 UI clásica | pasa | Q7 físico (§3); suite glass_ui 128/0 |
| S11 assets/glTF | pasa | Suite glass_fbx 5/0 + glass_ripper 125/0 |
| S12 partículas | pasa | Suite glass_particles 8/0 |
| S13 portales | pasa | Suite en verde (V5 no toca portales) |
| S14 editor/Inspector | pasa | 5 secciones nuevas, cada una verificada en el físico de su paquete (V5a LayoutGroup, V5b-1 LayoutElement, V5b-2 ContentSizeFitter, V5b-3 GridLayoutGroup, V5c RectMask2D) + Q7 editor (§3: LayoutGroup y RectMask2D) + Save Scene; suite glass_editor 265/0 |
| S15 Hub | pasa | Suite glass_hub 12/0 + glass_launcher 112/0 |
| S16 paridad RB/CC | pasa | Suite en verde (V5 no toca física) |
| S17 Demo FPS | pasa | Suite en verde (V5 no toca demos) |
| S18 MeshForge (PAUSA) | no-regresión | Suite glass_mesh_edit 51/0 |
| S19 colliders componentes | pasa | Suite en verde (V5 no toca colliders) |
| S20 importación Unity (PAUSA) | no-regresión | Suite glass_ripper 125/0 (el importador vive en glass_ripper); la reimportación limpia de NeoFPS es prueba manual y queda pendiente en el frente pausado, no es regresión de V5 |
| S21 UI/Canvas (dueña) | pasa | Q7 (§3) + suite 1.504/0 |
| S22 prefabs vivos | pasa | Suite en verde; los campos nuevos conviven con prefab_source/hash/local_id/overrides |
| S23 MCP completitud | pasa | tools/list responde; 58 tools sin cambios (V5 no añade ni quita tools); 5 round-trips nuevos en verde |

## 7. Peces de fin de módulo (Opus + DeepSeek)

Documento de revisión: `hidden_files/v5d_fisico/PECES_V5.md`. Resoluciones
completas en `hidden_files/v5d_fisico/RESOLUCIONES_PECES_V5.md`.

- **Pez 1 (Opus)**: 2 Bloqueantes (B1 aritmética Q7, B2 etiqueta de
  punto), 5 Mayores, 10 Menores. La primera llamada devolvió solo un
  preámbulo de 125 caracteres (incidente; reintentada una vez con
  instrucción de no-preámbulo; la segunda entregó 11.921 caracteres).
  Resuelto: B1 rebatido punto por punto (convención de bbox inclusiva,
  spacing 10, +2 solo en canales bajos, rect del hijo sí contiene el
  punto) con tres precisiones aplicadas al informe; B2 reetiquetado;
  M2 descompuesto el diff de la dorada (4.488 px: 212 esquinas del panel
  rojo + 116 del blanco + 4.160 banda del texto); M3 con prueba falsable
  (rosa pre-arreglo / cielo post-arreglo); el resto aplicado (tabla de
  precedencias, línea de grid sin flexible, nota de cobertura por crate,
  alcance del determinismo, radio 40, 12 valores, versión del inventario)
  o anotado (freno 25.000 no observable por el gateway).
- **Pez 2 (DeepSeek flash)**: 1 Bloqueante (B1: la máscara «no es la
  intersección A6»), 5 Mayores, 7 Menores. Resuelto: B1 rebatido (la
  Ventana no lleva `overflow:hidden`; la medida (125,125) blanco lo
  prueba) con la frase explícita añadida; M1 con dos tests nuevos en
  V5d (padding escalado con s=2/3 e intersección vacía); M2 con la fila
  S14 corregida por paquete; M5 con S5/S6 en condicional; el resto
  aplicado (tolerancia ±2, justificación de (170,125), aritmética V5b-2,
  tablas de rects de V5b-2/V5b-3, consumo V5d).
- **Sin re-auditoría pedida por ningún pez.**

## 8. Consumo del módulo V5

| Paquete | Opus | DeepSeek flash | Kimi |
|---|---|---|---|
| V5a | 0 | razonamiento 163.348 / contenido 25.570 | 0 |
| V5b-1 | prompt 14.057 B + 20.261 B / recibido 16.801 + 39.426 car. | 0 | 0 |
| V5b-2 | 0 | 0 | 0 |
| V5b-3 | 0 | razonamiento 51.669 / contenido 36.581 | 0 |
| V5c (spike Lu + paquete) | 0 | razonamiento 45.272 / contenido 25.163 | 0 |
| V5d (peces + 2 tests) | prompt 13.591 B + 14.237 B (1ª llamada Opus vacía, reintentada) / recibido 11.921 + 125 car. (Opus) | razonamiento 133.512 / contenido 6.801 | 0 |
| **Total** | **62.146 B enviados / 68.273 car. recibidos** | **razonamiento 393.801 / contenido 94.115** | **0** |

La tabla cuenta consumo de APIs externas; el trabajo directo de Lu no
pasa por ninguna API y no tiene coste medible (convención igual que en
V4). Unidades: B de prompt enviado; caracteres recibidos (Opus) y
caracteres de razonamiento/contenido (DeepSeek).

## 9. Deudas

**Ninguna deuda de esta lista bloquea el cierre de V5**: son limitaciones
conocidas para frentes posteriores, como en V4 (que cerró con D-V3-1/2/3
abiertas).
- **M4** (máscara alfa) fuera. **M6** (ScrollRect) fuera. **Q6** InputField
  fuera de V5 (deuda con nombre propio).
- **D-V3-1/2/3** siguen abiertas (importador rechaza fuentes sin outline;
  el pintor emite el texto envuelto como una sola línea; sin kerning).
- **D-V4-1**: `:hover`/`:active` se parsean pero nunca se aplican.
- **D-V4-4**: la GameView del editor no pinta la UI runtime.
- **Filete del clip redondeado anidado** (~4–5 px cuando un ancestro
  cuadrado gana la esquina; medido en el spike V5c en (811,111)). Sin
  dueño: su arreglo exigiría un clip verdadero de varios rects
  redondeados.
- ANOTAR: `childScaleWidth/Height`, `layoutPriority`, `Maskable` por
  Graphic, interacción Animator↔layout.
- **La dorada `ui_dorada_1280x720.png` se regeneró en el spike**: Análisis
  la re-verifica al cerrar el módulo (aviso de Rayo, buzon 4ff0b0d).

## 10. Estado

Módulo V5 CERRADO por Lu. Rama `lu/v5-grupos` (punta 76fd1c9 + 2 tests de
V5d + informe).
**Pendiente la verificación única de Análisis antes de integrar en `main`.**
La salida es el veredicto de Análisis en el buzón, como en V2/V3/V4.
Sigue V7 (modos Camera/World), después las tools MCP de UI y el frente de
faltas de la UI del editor; después la tarea de materiales (en cola).
