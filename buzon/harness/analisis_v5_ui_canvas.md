# ANÁLISIS V5 — Grupos (LayoutGroup) y recortes/máscaras del Canvas

Rama: `lu/v5-grupos` (desde `dabcb1a`, punta de `lu/v4-autorado`).
Arranque por orden directa del usuario (2026-11-27): la verificación única de
Análisis frena la INTEGRACIÓN de V4 a main, no el trabajo.
Método: el de V4 — este análisis, revisión peces (Opus + DeepSeek flash;
Gemini en pausa por la cuenta), preguntas a Rayo y su orden de trabajo
antes de tocar código.

---

## 1. Estado actual verificado en el código

### 1.1 Grupos

- `LayoutGroupData` existe en glass_core (`scene.rs:912`) desde una fase
  anterior: `direction` ("horizontal"/"vertical"), `spacing: f32`,
  `padding: [f32;4]` [izq, arriba, der, abajo], `alignment`
  ("start"/"center"/"end"). Es un Horizontal/VerticalLayoutGroup
  **reducido**: sin Control Size — cada hijo conserva su tamaño — y la
  alineación es un solo valor que se aplica a la vez al bloque (eje
  principal) y a cada hijo (eje cruzado).
- `layout_group_rects` (`scene.rs:950`) es la función pura que recoloca:
  interior = rect del grupo menos padding; los hijos se encadenan en el
  eje principal con `spacing`; el bloque se desplaza según `alignment`;
  el eje cruzado alinea cada hijo con el mismo valor.
- glass_ui lo aplica en `aplicar_flex` (`layout.rs:813`): un nodo con
  `layout_group` se trata como padre flex y usa `layout_group_rects`
  («uGUI: reparto exacto»); sin componente, `display: flex` del CSS usa
  `flex_css_hijos` (row/column, padding, gap, justify-content,
  align-items con stretch) y `display: grid` usa `aplicar_grid`
  (grid-template-columns, filas por el hijo más alto). Los tres caminos
  conservan el tamaño de los hijos (salvo el stretch transversal del
  flex CSS).
- La derivación lleva el componente (`documento.rs:50`,
  `derivacion.rs:230`) y la huella de prefabs lo incluye
  (`prefab.rs:450,666`): un prefab puede traer LayoutGroup y la huella
  lo distingue.
- Una tool MCP de listado de escena ya reporta el tipo "LayoutGroup"
  (`glass_mcp/src/server.rs:3119`).

### 1.2 Recortes

- El único recorte es CSS: `overflow: hidden` en el `style_extra` de un
  ancestro. El pintor (`pintor.rs`, `calcular_clip_para_nodo`) recorre
  los ancestros, intersecta los rects de los que tienen overflow hidden
  (redondeados a enteros) y emite cada vértice con ese clip; una «capa
  superior» (override sorting) rompe la cadena.
- El shader (`assets/shaders/ui.frag`, paso 6) aplica un clip
  **rectangular** en coordenadas locales del nodo con borde suave;
  la suavidad está **fija a 1,0 px** en el pintor (`pintor.rs:168`).
- El clip NO respeta el `border-radius` del ancestro que recorta: la
  esquina recortada es recta aunque el panel sea redondeado.
- **El hit-test NO respeta el clip** (`api.rs`/`interaccion.rs` no
  mencionan clip ni overflow): un hijo recortado visualmente fuera del
  panel sigue recibiendo clics en la zona invisible.
- No existe ScrollRect en ninguna parte de la UI.

### 1.3 Lo que NO existe

- **Editor**: `glass_editor` no menciona LayoutGroup en ninguna parte —
  sin sección en el Inspector, sin entrada en Añadir Componente, sin
  preset de creación. Hoy un LayoutGroup solo nace editando el JSON de
  la escena a mano.
- **MCP**: `glass_component_get`/`glass_component_set` delegan en
  `glass_core::componentes` (`componente_json`/`aplicar_componente`),
  que conoce 18 nombres (Transform, MeshRenderer, Rigidbody, Collider,
  CharacterController, Camera, Light, Script, AudioSource, Canvas,
  RectTransform, Text, Button, Image, DragDrop, Animator, Fog,
  Particles, Portal). **LayoutGroup no está** — y la decisión MCP ya
  cerrada dice que debe ganar LayoutGroup en este módulo. (Observación:
  **InputField tampoco está** en la lista.)
- **Componentes de máscara**: no hay RectMask2D ni Mask en `scene.rs`;
  la máscara solo se expresa como CSS en `style_extra`.
- **GridLayoutGroup** como componente uGUI: no hay (la rejilla solo
  existe como `display: grid` CSS).
- **ContentSizeFitter**: no hay; un grupo no se redimensiona a su
  contenido.
- **LayoutElement** (min/preferred/flexible por hijo): no hay; sin
  tamaños preferidos no hay Control Size posible.

---

## 2. Huecos contra Unity (alcance candidato de V5)

### Grupos

- **G1 — Autorado en el editor**: sección LayoutGroup en el Inspector
  (dirección, spacing, padding, alineación) + entrada en Añadir
  Componente + preset de creación (p. ej. «Panel con fila» / «Panel con
  columna»). Sin esto el componente es inusable para un humano.
- **G2 — MCP get/set gana LayoutGroup** (decisión cerrada, obligatorio):
  `"LayoutGroup"` en `componente_json`/`aplicar_componente` con los
  campos del struct; test de ida y vuelta como el existente.
- **G3 — Control Size**: `child_control_width/height` y
  `child_force_expand_width/height` (Unity: el grupo manda el tamaño de
  los hijos; force-expand reparte el espacio sobrante). Es EL
  comportamiento que define a un LayoutGroup de Unity frente al
  reducido actual. Exige tamaños preferidos (G7).
- **G4 — Alineación completa**: pasar de start/center/end único a las 9
  posiciones de TextAnchor (UpperLeft…LowerRight), que en Unity es un
  solo campo `childAlignment` con 9 valores: la alineación del eje
  principal y la del cruzado dejan de estar forzadas a coincidir.
- **G5 — GridLayoutGroup** como componente: cell_size, spacing,
  constraint (columnas fijas / flexible). Hoy la rejilla es solo CSS.
- **G6 — ContentSizeFitter**: el RectTransform del grupo se ajusta al
  contenido (horizontal/vertical: none / min / preferred).
- **G7 — LayoutElement + tamaños preferidos**: min_width/height,
  preferred_width/height, flexible_width/height por hijo; el tamaño
  preferido de un Texto sale del medidor SDF y el de una Imagen de su
  rect. Prerrequisito de G3 y G6.
- **G8 — Menores**: reverse arrangement (orden inverso), grupos
  anidados (verificar que componen: `aplicar_flex` itera los padres en
  orden del documento; hay que probarlo), hijos con anclas stretch
  dentro de un grupo (hoy el reparto sobrescribe el rect entero).

### Recortes / máscaras

- **M1 — RectMask2D como componente**: `UIRectMaskData` (padding
  [izq, arriba, der, abajo] y softness) autorable en el editor y por
  MCP, que alimenta el camino de clip que el pintor ya tiene. Es la
  máscara estándar de Unity para listas y paneles.
- **M2 — El hit-test respeta el clip**: un punto fuera del clip activo
  de un nodo no lo golpea. Paridad con Unity (RectMask2D rechaza el
  raycast fuera) y corrección de un defecto real: hoy se puede clicar
  lo invisible.
- **M3 — Clip con esquinas redondeadas**: el clip heredado respeta el
  border-radius del ancestro (el shader ya calcula un SDF redondeado
  por nodo; el clip necesitaría llevar radio). Un panel redondeado con
  overflow hidden hoy recorta en recto y las esquinas de los hijos
  asoman fuera de la curva.
- **M4 — Mask por alfa (stencil)**: la máscara sigue la forma del
  Graphic de la entidad (alfa de la imagen). En Unity usa stencil; aquí
  exigiría paso nuevo en el renderer. Candidato a quedar fuera de V5.
- **M5 — Softness configurable**: hoy fija a 1,0 px; RectMask2D de Unity
  la expone (softness). Iría dentro de M1.

---

## 3. Riesgos y notas de implementación

- `layout_group_rects` trabaja en unidades de referencia y lo llama
  `aplicar_flex` antes del escalado D-V4-2: Control Size debe repartir
  en el mismo espacio para no romper la dorada ni V4.
- Cambiar la firma de `LayoutGroupData` (G3/G4) toca el formato de
  escena (S1): los campos nuevos deben ser `#[serde(default)]` para que
  las escenas actuales carguen igual; la huella de prefabs ya incluye
  el componente serializado, así que crece sola.
- M3 toca el shader UI: hay que regenerar el `.spv` y el UiVertex
  (144 B) puede crecer si el clip gana radio — afecta a glass_renderer
  y al batch de glass_app.
- M2 toca `hit_test`/`ascender_a_control`: el clip activo por nodo ya
  lo calcula el pintor; la interacción necesitaría el mismo cálculo
  (moverlo a un sitio común de glass_ui, no duplicarlo).
- Secciones del barrido: S10/S21 (UI), S3 (MCP, por G2), S1 (formato,
  si G3/G4/M1 añaden campos), S6 (shaders, si M3), S14 (editor, por G1).

## 4. Preguntas a Rayo

- **Q1 (alcance de grupos)**: ¿V5 incluye Control Size completo
  (G3+G7, con force-expand) o se queda en el reducido actual + autorado
  (G1/G2/G4)? Mi recomendación: completo — sin Control Size el
  LayoutGroup de Unity no está, solo un encadenador de posiciones.
- **Q2 (rejilla)**: ¿GridLayoutGroup como componente (G5) entra en V5 o
  basta el `display: grid` CSS que ya existe?
- **Q3 (fitter)**: ¿ContentSizeFitter (G6) entra en V5? Sin él los
  menús dinámicos obligan a dimensionar el panel a mano.
- **Q4 (máscara)**: ¿V5 es RectMask2D (M1+M5) + hit-test (M2) + esquinas
  (M3), y la Mask por alfa (M4) queda fuera/deuda? Mi recomendación: sí.
- **Q5 (alineación)**: ¿G4 (9 posiciones) entra, o se mantiene
  start/center/end por simplicidad?
- **Q6 (InputField en MCP)**: `glass_component_get/set` tampoco conoce
  InputField (quedó fuera en V4). ¿Se añade en V5 aprovechando G2 o se
  anota como deuda aparte?
- **Q7 (dorada/físico de V5)**: propongo una escena de prueba construida
  con el editor: una ventana con RectMask2D y un LayoutGroup vertical
  con Control Size (botones que se reparten el alto), medida por dorada
  de rects + captura standalone comparada por píxeles, como G-AUTOR1.
  ¿Vale como físico del módulo o pides otra escena?
- **Q8 (paquetes y proveedores)**: propongo V5a autorado+formato
  (G1/G2/G4), V5b Control Size (G3/G7/G6), V5c máscara (M1/M2/M3/M5),
  V5d cierre (dorada + físico + peces). ¿Reparto de proveedores como en
  V4 (DeepSeek flash los mecánicos, Opus los de diseño con freno
  25.000, Lu el cierre)?

- **Q9 (ScrollRect)**: no existe y es el consumidor principal de
  RectMask2D. Mi recomendación: **fuera de V5** como deuda con nombre;
  V5 entrega sus prerrequisitos (M1+M2+G3) y el ScrollRect se monta en
  el módulo que lo necesite. ¿Lo confirmas o entra como paquete propio?
- **Q10 (reparto revisado)**: V5a autorado+formato y V5c máscara,
  mecánicos → DeepSeek flash; V5b-1 (medición + Control Size) es diseño
  algorítmico → Opus con freno 25.000; V5b-2 y V5d, Lu. ¿Vale?

---

## 5. Revisión peces (2026-11-27) y resoluciones

Peces: Opus (prompt 11.007 B, contenido 10.215 caracteres) y DeepSeek
flash (prompt 11.007 B, razonamiento 76.278, contenido 11.646, paquete
entero, sin corte). Gemini en pausa por la cuenta. Ambos coinciden en
lo esencial: ScrollRect sin decidir, orden de rebuild, migración de
formato de G4, V5b monolítico y matriz de pruebas corta.

### APLICAR

- **A1 — V5b se parte en dos.** V5b-1: G7-mínimo (min/preferred por
  hijo + `ignore_layout`; medición del preferido de Texto con el
  medidor SDF y de Imagen con su rect) + G3 Control Size sin flexible.
  V5b-2: G6 ContentSizeFitter + tamaños flexible. Cada paquete cierra
  con la suite completa y sus pruebas de aceptación antes del siguiente.
- **A2 — Orden de rebuild: verificado en el código, es top-down.**
  `derivar_documentos` construye los nodos en DFS pre-orden
  (`derivacion.rs:105`) y `aplicar_flex` itera en ese orden. Para el
  grupo reducido actual basta; para Control Size con grupos anidados
  no: V5b-1 incluye como prerrequisito de diseño una **pasada de
  tamaños preferidos bottom-up** (post-orden) que alimenta el reparto
  top-down existente. Grupos anidados = caso de aceptación obligatorio
  (dorada H dentro de V, ambos con Control Size), no nota menor.
- **A3 — ScrollRect es hueco explícito (M6) + Q9**, con recomendación
  de dejarlo fuera como deuda con nombre.
- **A4 — G4 decidido aquí: mismo campo, sin renombrar.** `alignment`
  sigue siendo el string de siempre y gana los 9 valores de posición
  (upper_left, upper_center, upper_right, middle_left, center,
  middle_right, lower_left, lower_center, lower_right); los valores
  actuales quedan como alias permanentes: start = esquina de inicio de
  bloque e inicio cruzado según dirección, end = la opuesta, center =
  center. Compatibilidad por construcción; aceptación: test de carga
  de escena antigua + huella de prefab.
- **A5 — Matriz de pruebas del módulo** (por paquete y en V5d):
  doradas de rects (horizontal, vertical, force-expand, 9 alineaciones,
  anidados), hit-test con clip (clic en zona recortada no golpea),
  máscaras anidadas, clip redondeado, carga de escenas viejas,
  ida-y-vuelta MCP de cada componente nuevo, compilación de shader.
  La regresión V2–V4 la cubre la suite completa de cargo, que ya
  contiene sus doradas y se ejecuta entera en cada cierre (pipeline
  permanente, no hace falta preguntarlo).
- **A6 — Modelo único de clip** (vale para pintor y hit-test): las
  fuentes son los ancestros con `overflow: hidden` CSS y los
  componentes RectMask2D; se intersectan bajando por la cadena de
  ancestros; la «capa superior» rompe la cadena (regla actual, se
  mantiene); RectMask2D recorta a los descendientes, no a sí mismo, con
  su padding encogiendo el rect y su softness propia; el radio de
  esquina del nodo que recorta se aplica al clip (M3) en ambas fuentes;
  el hit-test usa el mismo clip calculado (rect + radio; la softness
  no afecta al hit).
- **A7 — Matriz de componentes**: todo componente que V5 cree o
  extienda (LayoutGroup, RectMask2D y, si entran, GridLayoutGroup,
  ContentSizeFitter, LayoutElement) lleva registro en
  `glass_core::componentes`, test de ida y vuelta MCP, sección en el
  Inspector y entrada en Añadir Componente.
- **A8 — M5 no es opcional**: la softness es campo de M1 desde el
  principio.
- **A9 — Estrategia de rama**: si la verificación de Análisis obliga a
  corregir `lu/v4-autorado`, `lu/v5-grupos` se rebasea sobre la punta
  corregida antes de cerrar su primer paquete.
- **A10 — M3 con estimación y puerta**: el clip gana el radio del nodo
  que recorta (hasta +16 B por vértice si son 4 esquinas; se medirá en
  un spike al abrir V5c antes de comprometer el formato de vértice).
  Puerta de regresión de V5c: suite completa + dorada de V4 + captura
  standalone comparada.

### ANOTAR (deudas con nombre si Rayo no las mete)

- `childScaleWidth/Height` y `layoutPriority`: fuera de la paridad
  mínima de V5; deuda anotada.
- Opt-out de máscara por Graphic (`Maskable` de Unity): la «capa
  superior» ya rompe la cadena a nivel de nodo; el flag por gráfico
  queda como deuda anotada.
- Interacción Animator↔layout (animar el tamaño de un hijo dentro de
  un grupo): limitación conocida; el frente de animación es post-1.0 y
  está dormido.
- Q6 (InputField en MCP): la recomendación por defecto pasa a ser
  deuda aparte; solo entra en V5 si Rayo lo ordena expresamente.

### REBATIR

- Nada de fondo: las dos revisiones se sostienen en el documento o en
  el código verificado (A2). La comparación por píxeles del físico se
  mantiene con la barra de byte-idéntico (en esta máquina y driver,
  V4e lo logró) reportando el % de diferencia si no se alcanza.

*Fin del análisis. Pendiente: orden de trabajo de Rayo (Q1–Q10).*
