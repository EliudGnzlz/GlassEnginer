# Análisis V4 — Autorado estilo Unity (UI/Canvas)

Fecha: 2026-11-26. Rama de trabajo: `lu/v4-autorado`, desde la punta de
V3 (`279c9f4`; V2 y V3 siguen sin integrar a `main` a falta de la
verificación única de Análisis). Régimen: paquetes en continuo, una
sola revisión por Análisis al final del módulo (orden del usuario
2026-11-26). Propietario **S21** (UI/Canvas V1) + vecinos S14 (Editor:
sesión, undo, Inspector y Gizmos), S10 (UI/Canvas clásica), S2
(Scripting C# y API estilo Unity), S3 (MCP base). S1 no se toca
(ningún cambio de formato de escena: todo lo nuevo se expresa con
campos que ya existen en `EntityData` o con `style_extra`).

## 1. Criterio del diseño (diseno_fase_ui_canvas.md, V4)

> Inspector: campos de estilo por elemento, marca «controlado por
> estilo», campo CSS extra con avisos de parseo; pestaña Canvas/UI
> crea Panel/Imagen/Texto/Botón/InputField; arrastrar en la vista de
> escena escribe el RectTransform; C# expone las propiedades de
> estilo. Criterio: una pantalla completa creada desde el editor sin
> tocar JSON, y su documento derivado coincide con lo esperado.

## 2. Estado inicial verificado en código (2026-11-26)

- **Pestaña Canvas/UI existe** (`crates/glass_editor/src/panels/canvas_ui.rs`,
  370 líneas): crea Canvas, Texto y Botón; lista los elementos del
  canvas activo; edita modo de render, resolución de referencia y
  match. **No crea Panel/Imagen ni InputField.**
- **Inspector con campos tipados** (`crates/glass_editor/src/panels/inspector.rs`,
  §6.5): Canvas (modo, resolución); RectTransform (**posición anclada,
  tamaño delta, pivote — sin anclas**); Text (texto, tamaño, color,
  fuente, alineación); Button (interactable, color, on-click); Image
  (sprite, tinte, preserve aspect); Drag&Drop. **No hay ningún campo
  de estilo** (border-radius, fondo, degradado, opacidad, overflow,
  box-shadow), **no hay marca «controlado por estilo»** y **el
  `style_extra` no se muestra en ningún sitio** (existe en los datos
  desde V1, pero solo se escribe a mano en el JSON).
- **Presets** (`hierarchy.rs:1009-1032`): Canvas, UIText («Text
  (TMP)»), UIButton («Button» con texto «Button»). **No hay preset
  UIImage ni InputField.**
- **C#** (`managed/GlassEngine.Core/UnityEngine.cs:1058-1099`):
  `RectTransform` expone `anchoredPosition` y `sizeDelta` en vivo por
  el puente dinámico; `anchorMin`/`anchorMax`/`pivot` son **locales
  (no se escriben al motor)**. `Canvas` expone `renderMode`. **No
  existe ninguna propiedad de estilo.** El puente
  (`RuntimeBridge.cs:1145-1182`) tiene `UiSetText/FontSize/TextColor/
  ImageSprite/ImageColor/ButtonColor/AnchoredPos/SizeDelta`. **No hay
  puente de estilos.**
- **GameView** (`viewport.rs:1984`, `render_in_game_canvas_ui`): dibuja
  la UI sobre la textura del juego con el **mismo documento colocado
  que el runtime** (auditoría C7 de V2); los botones son egui Buttons.
  Es donde existen los rects colocados y el hit-test (`rects_colocados`,
  usado también por MCP). La pestaña Scene es solo gizmo 3D.
  Deuda D1 vive en este overlay: dibuja «JUGAR» + «Button».
- **Undo** (`glass_editor/src/undo.rs`): `UndoHistory` por instantáneas
  (`observe` con bandera `editando`): un arrastre debe escribir una
  sola instantánea al soltar, no una por frame.
- **Evidencia de la prueba de botones (2026-11-26, informe en
  `hidden_files/botones_fisico/INFORME_BOTONES.md`)**: el autorado a
  mano en JSON chocó con cuatro cosas que V4 debe decidir: (a) los
  colores CSS se interpretan como lineales, no sRGB — `#0a1531`
  escrito tal cual se ve (56,81,121); (b) el ángulo del degradado va
  girado 90° respecto a CSS (`0deg` = izq→der); (c) no existe la
  propiedad `border`; (d) no existe `letter-spacing`. Ninguna escena
  del repo usa hoy `style_extra` con colores ni degradados, así que
  cambiar la convención del parser no rompería tests ni fixtures.

## 3. Paquetes

El reparto por paquete es una **propuesta**; Q5 lo confirma antes de
arrancar. **Ningún paquete arranca sin la orden de trabajo de Rayo.**

### V4a — Inspector: campos de estilo + marca + CSS extra (DeepSeek flash)

Campos tipados por elemento para el conjunto cerrado de `StyleProps`
que el pintor ya entiende hoy: `background-color`, `background`
(degradado 2 paradas), `border-radius` (4 esquinas), `box-shadow`,
`opacity`, `overflow`, `font-size`, `color`, `width/height/min/max`,
`aspect-ratio`, `text-overflow`. `border` y `letter-spacing` solo
si Q3 dice sí. Cada campo que venga resuelto desde `style_extra` (o
desde un campo de componente que el estilo pisa) lleva la marca
«controlado por estilo» y se edita desactivado (el valor se cambia
en el CSS, no en el campo). Campo CSS extra multilínea con los
avisos del parser en vivo (misma lista que devuelve `analizar_css`:
nunca pánico, el resto de estilos sigue aplicando). RectTransform
gana la edición de `anchor_min`/`anchor_max` + presets de ancla
(esquina, centro, estirar); la aceptación de las anclas se verifica
después de V4d (si V4d se cae, la edición de anclas queda
desactivada). Aceptación: un elemento con `style_extra` válido
muestra sus campos marcados y desactivados; un `style_extra` con
error muestra el aviso y el elemento sigue pintando; matriz de
tests: un test por campo del conjunto cerrado, precedencia
`style_extra` vs componente, CSS parcialmente inválido, edición
desactivada, lista de avisos en vivo y etiqueta «sin efecto» en
pseudo-clases.

### V4b — Creación: Panel/Imagen e InputField (DeepSeek flash)

Preset `UIImage` («Panel» con imagen de color + «Imagen» con sprite)
y creación desde la pestaña Canvas/UI y desde la jerarquía, con
defaults fijos: Panel 200×200, tinte blanco, anclas [0.5,0.5],
pivote [0.5,0.5], nombre «Panel», queda seleccionado y con undo;
Imagen con sprite vacío y los mismos defaults. «Dos clics» = clic en
«➕ Panel» (o «➕ Imagen») + selección automática. InputField según
la respuesta a Q1: si es componente real, su preset nace como
Texto+Imagen con el componente; si es cáscara visual, nace como
Texto+Imagen sin más. Aceptación: cada tipo se crea con dos clics
desde el panel, queda seleccionado, y su documento derivado es el
esperado (rects medidos, documento esperado exacto por tipo).

### V4c — Arrastre en la vista que escribe el RectTransform (Opus)

Arrastrar un elemento UI en la vista (según Q2) mueve su
`anchored_position` (y con el asa de esquina, su `size_delta`);
clic selecciona el elemento por hit-test sobre los rects colocados.
Conversión pantalla→referencia con la escala del canvas y el tamaño
real de la vista; los elementos con anclas de estirado escriben
`size_delta`, no posición. Undo: una instantánea al soltar (nunca
por frame) y solo si el rect cambió; el clic-selección no crea
instantánea; un arrastre cancelado (Esc) no escribe nada. El
overlay de GameView **no se toca** en V4c salvo lo imprescindible
para el arrastre; el doble dibujo de D1 queda como está salvo que
Q6 diga que V4 lo absorbe. Aceptación: arrastrar 40 px de pantalla
mueve el rect medido lo que corresponde en referencia (±0.5 px), la
escena guardada trae el nuevo `RectTransform`, Ctrl+Z revierte el
arrastre completo, y las secuencias clic / arrastre nulo / arrastre
cancelado / arrastre de asa dejan el historial de undo limpio.

### V4d — C# expone las propiedades de estilo (Opus)

Propiedades de estilo en las clases C# de UI (color de fondo,
tamaño de fuente, radio de esquina, opacidad, visibilidad de
overflow — el conjunto que fije V4a) escritas por el puente
dinámico, con el mismo patrón que `UiSetAnchoredPos` (el lote del
Canvas se reconstruye cada frame). Las anclas y el pivote, hoy
locales, se escriben al motor por el mismo puente. La fachada de
scripting vive en su propio contexto de carga (lección D4): el
puente nuevo pasa por el mismo camino que los `UiSet*` existentes,
sin inventar otro. Aceptación: un script que cambia el color de
fondo y el radio por código se refleja en el lote al frame
siguiente; test C# que lo comprueba sin pasos manuales.

### V4e — Cierre del módulo (Lu)

Pantalla completa creada **solo con el editor** (la pantalla de los 5
botones del informe de prueba, rehecha sin tocar JSON): Canvas +
Panel de fondo + 5 botones con estilos y arrastre. Documento
derivado comparado con el esperado (rects ≤0.5 px, estilos
resueltos iguales). Pipeline exacto de CI en local, test físico por
el camino real (standalone + editor), barrido S1–S23, consumo por
proveedor en el CIERRE.

## 4. Puertas

- **G-AUTOR1**: pantalla completa creada solo con el editor, sin
  tocar JSON; documento derivado igual al esperado (rects ≤0.5 px,
  estilos resueltos iguales). El esperado es un **JSON dorado
  versionado en el repo** (rects y estilos resueltos por entidad),
  generado desde la escena de botones escrita a mano
  (`EscenaBotones.scene.json`); el comparador es un test que hace
  diff campo a campo. Si Q8 cambia la pantalla, el dorado cambia
  con ella; si Q4 cambia la convención, el dorado se regenera bajo
  la convención elegida.
- **G-AUTOR2**: arrastrar en la vista escribe el `RectTransform`
  (verificado en la escena guardada) y Ctrl+Z lo revierte.
- **G-AUTOR3**: C# — un script que cambia color/tamaño/fuente por
  código se refleja en el lote al frame siguiente.
- **G-AUTOR4**: `style_extra` con error muestra el aviso en el
  Inspector y el elemento sigue pintando (nunca pánico).

## 5. Riesgos

- **El overlay de GameView es también la deuda D1.** V4c lo toca
  para el arrastre; si D1 queda fuera de V4, el trabajo debe no
  empeorarla (el botón seguiría dibujándose dos veces).
- **El arrastre no debe pelearse con el gizmo 3D.** La vista de
  escena es 3D; si el arrastre vive ahí, el hit-test de UI debe
  ganar solo cuando el puntero cae sobre un rect UI.
- **Undo por instantánea.** Un arrastre que escriba cada frame
  inundaría el historial; la instantánea va al soltar, y solo si el
  rect cambió.
- **La fachada C# vive en su propio contexto de carga.** El puente
  nuevo de estilos no puede asumir el contexto por defecto (lección
  D4).
- **Cambiar la convención de color/ángulo del CSS cambia el
  significado de los `style_extra` ya escritos.** Hoy no hay ninguno
  con colores ni degradados en el repo, pero un usuario con escenas
  propias lo notaría; la decisión es de Rayo (Q4).
- **`:hover`/`:active` no están cableados en runtime** (evidencia:
  capturas de hover/clic byte-idénticas, informe de botones). El
  Inspector de V4a mostrará los bloques de pseudo-clase como «sin
  efecto en runtime hasta V6» para no vender un estilo muerto.
- **InputField es V6.** Si V4 lo crea como componente real, V6 lo
  hereda; si es cáscara, V6 lo sustituye. La decisión es Q1.

## 6. Plan físico

1. Crear la pantalla de los 5 botones **solo con el editor**:
   Canvas, Panel de fondo, 5 botones (verde, blanco, neón,
   degradado rosa, degradado rojo) con sus estilos desde el
   Inspector, un botón recolocado arrastrándolo en la vista.
2. Guardar la escena, reabrirla, comparar el documento derivado con
   el esperado y con la escena de botones escrita a mano (rects y
   estilos).
3. Standalone: captura de la pantalla; clic medido en un botón y
   acción despachada en el log.
4. Script C# mínimo que cambia un estilo por código; captura antes
   y después.

## 7. Preguntas para Rayo

- **Q1 — InputField en la creación:** ¿V4 crea un componente real
  `UIInputFieldData` (cuya conducta llega en V6) o una cáscara
  visual Texto+Imagen hasta V6?
- **Q2 — Dónde se arrastra:** ¿el arrastre vive en la GameView
  (donde hoy están los rects colocados y el hit-test) o en la pestaña
  Scene (donde habría que añadir overlay de UI al gizmo 3D)?
- **Q3 — Propiedades nuevas:** ¿V4 añade `border` (ancho/color) y
  `letter-spacing` a `StyleProps`/parser? La prueba de botones las
  echó en falta; sin ellas el autorado de botones reales sigue
  necesitando paneles anidados y espacios literales.
- **Q4 — Convenciones CSS:** ¿el parser convierte sRGB→lineal para
  que el color escrito sea el color que se ve, y el ángulo del
  degradado sigue CSS (`90deg` = izq→der)? Cambia el significado de
  los `style_extra` con colores ya escritos (hoy ninguno en el repo).
- **Q5 — Reparto:** ¿V4a/V4b en DeepSeek flash, V4c/V4d en Opus
  (freno `--max-razonamiento` armado en 25.000), V4e en Lu? El
  diseño reparte Opus a V2/V3/V7 y DeepSeek a V5/V6; V4 no tiene
  asignación escrita.
- **Q6 — Deuda D1:** ¿V4 se traga la D1 (el overlay dibuja el botón
  dos veces) al tocar el overlay para el arrastre, o queda para el
  frente de faltas de UI del editor?
- **Q7 — Anclas en el Inspector:** ¿V4a incluye edición de anclas +
  presets de ancla (esquina, centro, estirar) en el RectTransform?
  Hoy no se pueden editar sin tocar el JSON.
- **Q8 — Pantalla de aceptación:** ¿la pantalla de los 5 botones del
  informe de prueba sirve como pantalla de aceptación de G-AUTOR1
  (rehecha solo con el editor), o prefieres otra?

## 8. Cierre

Cierre por paquete en el buzón con consumo por proveedor; revisión
única por Análisis al final del módulo; sin integración a `main`
hasta esa verificación. Base: punta de V3 (`279c9f4`). **Ningún
paquete arranca hasta que Rayo responda §7 y dé la orden de
trabajo**; V4e queda bloqueado a la resolución de Q4 y Q8 (su
esperado depende de ambas).

## 9. Resoluciones de peces (2026-11-26)

Pez 1 Opus (razonamiento 0, contenido 5.104): paráfrasis correcta;
0 Bloqueante, 3 Mayor, 4 Menor. Pez 2 DeepSeek flash (razonamiento
21.525, contenido útil 1.957, parcial exit 3; reintento con demanda
reducida también cortado, 13.804/696, 3 hallazgos que confirman los
3 primeros del parcial): 3 Bloqueante, 2 Mayor (+1 truncado). Sin
re-auditoría; nada escalado al usuario.

- **Pez1 Bloqueante (alcance de V4a depende de Q3) → ESCALAR (Q3)
  + APLICAR:** V4a lista ahora el conjunto cerrado exacto que el
  pintor entiende hoy; `border`/`letter-spacing` solo si Q3 dice sí.
- **Pez1 Mayor (anclas: V4a acepta, V4d escribe) → APLICAR:** la
  aceptación de anclas de V4a se verifica después de V4d; si V4d se
  cae, la edición de anclas queda desactivada.
- **Pez1 Mayor + Pez2 Bloqueante (G-AUTOR1 sin oráculo) → APLICAR:**
  el esperado es un JSON dorado versionado (rects y estilos
  resueltos por entidad) generado desde la escena de botones escrita
  a mano, con comparador test campo a campo.
- **Pez1 Mayor + Pez2 Bloqueante (Q4 cambia semántica) →
  ESCALAR (Q4) + APLICAR:** ningún paquete arranca hasta que Rayo
  responda §7; V4e queda bloqueado a Q4 y Q8, y el dorado se
  regenera bajo la convención elegida.
- **Pez2 Bloqueante (aceptación fijada sobre Q1/Q2/Q6/Q8 abiertas)
  → APLICAR:** mismo régimen (§8): las respuestas de Rayo preceden
  a la orden de trabajo; cada paquete reescribe su aceptación por
  variante si la respuesta la cambia.
- **Pez1 Menor (D1: V4c «reordena» el overlay) → APLICAR:** V4c no
  toca el overlay salvo lo imprescindible para el arrastre; D1 queda
  como está salvo que Q6 diga que V4 lo absorbe.
- **Pez1 Menor (undo sin criterio para clic/arrastre nulo) →
  APLICAR:** V4c cubre ahora clic-selección sin instantánea,
  arrastre sin cambio sin instantánea y arrastre cancelado sin
  escritura, con secuencias de test.
- **Pez1 Menor (reparto incoherente) → APLICAR:** §3 declara el
  reparto como propuesta; Q5 lo confirma antes de arrancar.
- **Pez1 Menor (`:hover` como promesa) → ANOTAR:** el Inspector los
  mostrará como «sin efecto en runtime (V6, sin fecha)»; D-V4-1
  queda registrada como deuda real (§10).
- **Pez2 Mayor (V4a: aceptación pobre) → APLICAR:** matriz de tests
  por campo, precedencia, CSS parcial inválido, edición desactivada,
  avisos en vivo y etiqueta de pseudo-clase.
- **Pez2 Mayor (V4b: defaults sin cuantificar) → APLICAR:** defaults
  fijos por tipo (Panel 200×200, tinte blanco, anclas/pivote
  [0.5,0.5], nombre, selección + undo) y «dos clics» definido.
- **Pez2 Mayor truncado (V4c: tolerancia/undo/anclas estiradas) →
  APLICAR:** cubierto por las resoluciones de undo y por «anclas de
  estirado escriben `size_delta`» en V4c.

## 10. Deuda registrada

- **D-V4-1:** `:hover`/`:active` sin cablear en runtime (V6). El
  Inspector los mostrará como sin efecto hasta entonces.
