# Revisión «peces» — fin de módulo V4 (autorado estilo Unity)

Eres un revisor externo e independiente. Recibes SOLO este documento (sin acceso al repo).
Tu trabajo: cazar defectos, supuestos falsos y huecos en el módulo V4 de GlassEnginer.
Devuelve una lista de hallazgos, cada uno con severidad **Bloqueante / Mayor / Menor**,
una frase de qué está mal y la evidencia o razonamiento que lo sostiene.
Si algo te parece correcto, no lo listes. Sé concreto y escéptico: no des nada por bueno.

## Contexto del motor (lo mínimo para revisar)

- GlassEnginer: motor de juegos estilo Unity, Rust + Vulkan 1.3, scripting C# (.NET 8).
- Módulo UI/Canvas: `glass_ui` (parser CSS, layout, pintor) + `glass_editor` (Inspector,
  jerarquía) + `glass_core` (escena, `resolve_ui_rect_in`).
- Canvas: resolución de referencia 1920×1080; `canvas_scale_factor` interpola en log
  según el ancho real (a 1280×720 el factor s = 2/3).
- El pintor emite en orden: Sombra → Fondo → Borde → Imagen → Texto; el orden de
  pintado sigue el orden DFS pre-orden de la lista `children` de cada entidad.
- `StyleProps`: conjunto cerrado de propiedades de estilo (fondo, degradado,
  borde, radio, sombra, letter-spacing, overflow, text_overflow, etc.).
- La derivación de documentos (`derivar_documentos`) convierte la escena en
  documentos de layout; el golden test compara documento derivado contra un
  JSON dorado por NOMBRE de entidad (los ids los asigna el editor).

## Qué hizo V4 (paquetes V4a–V4e)

### V4a — border / letter-spacing / Q4 / anclas + presets + Inspector
- `MODO_BORDE` en el pintor con regla premultiplicada; `border` y `letter-spacing`
  como campos de `StyleProps`; el parser convierte sRGB→lineal en todos los caminos
  y el ángulo de degradado se guarda como CSS − 90 (90deg CSS = izquierda→derecha).
- Campos tipados de estilo en el Inspector; marca «controlado por estilo»;
  editor CSS con avisos en vivo; anclas + 8 presets (los presets ponen
  anclas+pivote juntos al MISMO rincón, no cubren combinaciones mixtas).
- Suite tras V4a: 1.422/0. Consumo: DeepSeek flash útil 5.303.

### V4b — `UIInputFieldData` real + presets Panel/Imagen/InputField
- `TipoGraphic::InputField` (valor o placeholder atenuado; el pintor emite
  imagen+texto en el mismo nodo); sección InputField en el Inspector.
- Hallazgo físico: el layout solo construía el mapa de texto para `Texto`;
  el campo salía blanco sin letras — arreglo de raíz + test de regresión.
- Los presets ponen `background-color: #ffffff;` en el `style_extra` de su imagen.
- Suite tras V4b: 1.431/0. Consumo: 0 (implementado por Lu).

### D-V4-2 (adenda, decisión de Rayo)
- Los px de `StyleProps` NO se escalaban por la escala del canvas: a 1280×720 el
  texto se veía 1,5× grande. Arreglo: el layout escala font_size, letter-spacing,
  border-radius, ancho de borde y sombra por `s` en el mismo camino que usa el mapa.
- Medido a 1280×720 (s=2/3): «B O T Ó N» de 138×27 a 92×18 px. Suite 1.432/0.

### V4c — arrastre en la GameView + arreglo de D1
- D1 (de V2): el botón sacaba su etiqueta de un «Button» genérico. Causa raíz: el
  batch de UI del juego está dentro de `if render_standalone` (main.rs:8118), así
  que la textura del juego en el editor NO tiene UI. Arreglo: la etiqueta del
  botón sale SOLO de su propio `ui_text`, nunca de «Button».
- Arrastre medido: 40 px → 107.4627 en referencia, exacto. Suite 1.441/0.
- Consumo: Opus 25.423.

### V4c adenda — `ascender_a_control` + Guardar sobre `--scene`
- Causa raíz: la carga al arranque nunca ponía `ruta_escena_actual`. Suite 1.452/0.

### V4d — puente de estilos Rust + C#
- 9 métodos `ui_set_*` (Rust + FFI + C#); sondas 15/16; físico G-AUTOR3 en verde
  (panel naranja radio 20 borde blanco, «ESTILO C#» con espaciado). Suite 1.452/0.
- Consumo: Opus 41.848.
- Lecciones: `style_extra` es un `String` plano, NO Option; el JSON de escena con
  `"transform": null` FALLA (siempre objeto transform completo); las entidades no
  colgadas del Canvas no se pintan.

### V4e — pantalla de 5 botones solo con el editor (G-AUTOR1) + cierre de módulo
- La pantalla de referencia (5 botones animados del vídeo, recreados en una tarea
  previa) se rehízo ENTERA con el editor, sin editar el JSON a mano: 40 entidades,
  nombres idénticos a la referencia, verificadas una a una contra el JSON dorado.
- Golden test `dorado_botones.rs`: deriva el documento de la escena del editor y
  de la referencia y las compara por nombre (rect tolerancia 0,5 px, estilos 1e-3).
  Resultado: **2 passed** (`editor_igual_a_dorado`, `dorado_desde_referencia`).
- Pipeline exacto del CI en local: build workspace OK; clippy con
  `RUSTFLAGS="-D warnings"` OK (solo avisos preexistentes de dependencias no usadas);
  build `glass_app --no-default-features` OK; `dotnet build` 0 errores;
  suite **`cargo test --workspace --locked`: 1.454 passed / 0 failed**.
- Prueba física: standalone con la escena del editor y con la escena de referencia,
  capturas 1280×900, comparación píxel a píxel.

## Hallazgos de V4e (los dos de raíz + una desviación)

### H1. Tres botones estaban INACTIVOS (causa de 22 de 24 diferencias del dorado)
- Blanco (10), Neón (11) y Rosa (13) tenían `active=false` en el JSON guardado
  (se desactivaron en algún momento sin advertirlo; Verde/Rojo estaban activos).
- En `colocar_documento_con_medidor` (layout.rs) un nodo inactivo → rect [0,0,0,0]
  para sí Y sus descendientes, `MapaTexto::default()` (texto_visible vacío) y
  estilos RAW sin escalar (sin `estilos_pintado`). Eso explicaba 22 de 24
  diferencias (14 rects cero, 3 textos vacíos, font_size 34 vs 22,67, sombra 26/22
  vs 17,33/14,67). Se reactivaron con el checkbox del Inspector. JSON final:
  `inactive: []` (los 40 activos). El dorado pasó a 2/2.
- Pregunta para el pez: ¿debería el editor avisar de algún modo cuando se guarda
  una escena con entidades inactivas, o es comportamiento aceptable y documentado?

### H2. `capa_componente` (parser.rs:649): `target_graphic_color` del Button SOBRESCRIBE el tinte de la Image
- La prioridad imagen > texto > botón se aplica a `style_extra`, NO a este campo:
  `background_color` = color de texto/font_size, luego `imagen.color`, luego
  `boton.target_graphic_color` LO SOBRESCRIBE.
- En la referencia, `target_graphic_color == ui_image.color` en los 5 botones (por
  eso el dorado background_color = tinte). El editor crea botones con el azul por
  defecto [0.25, 0.55, 0.85]. Solo Rosa y Rojo se vieron afectados (su CSS tiene
  degradado pero NO `background-color`, así que el valor de la cascada queda);
  Verde/Blanco/Neón estaban bien (su CSS `background-color` gana en la cascada).
- Arreglo: se puso el «Color Botón» de cada uno a su tinte (Rojo #ffcf00,
  Rosa #f953c6). JSON final verificado: `target_graphic == tint` en ambos.
- Pregunta para el pez: ¿es este comportamiento del motor un defecto que debería
  arreglarse de raíz (que el tinte de la imagen gane siempre), o una convención
  válida que basta documentar? ¿Hay algún otro campo con la misma inversión?

### H3 (desviación documentada, NO bloqueante según el autor). Orden de pintado de los halos
- En la escena del editor, los halos de Rosa y Rojo se crearon DESPUÉS de sus
  botones, así que en la lista `children` del Canvas van después y se pintan
  ENCIMA (en la referencia van antes, debajo). El orden de hijos DENTRO de cada
  botón sí se corrigió con «Mover Arriba» (Cara antes que Texto, si no la Cara
  opaca tapaba el texto de Blanco/Neón — eso se cazó en la captura física).
- Medida: comparación píxel a píxel editor vs referencia: **2,667 % de píxeles
  difieren, diferencia máxima de canal 52**, todo concentrado en la banda y=400–479
  (fila inferior, zona de halos). La fila superior es idéntica a tolerancia >10.
- El autor lo deja como desviación documentada (el dorado formal pasa; la
  diferencia es solo el tinte del halo sobre el borde del botón). Pregunta para
  el pez: ¿aceptas dejarlo como desviación documentada o exiges reordenar los
  hijos del Canvas con «Mover Arriba/Abajo» hasta igualar la referencia?

## Deudas abiertas del módulo V4 (para tu valoración)

- **D-V4-1**: `resolver_estilos` usa siempre `EstadoPseudo::default()` — las
  pseudo-clases (`:hover`, `:active`) se parsean pero NUNCA se aplican (capturas
  byte-idénticas con y sin hover). Es un hueco funcional conocido desde la tarea
  de botones.
- **D-V4-3 (nueva, propuesta)**: el orden de pintado de los halos (H3 arriba).
- **D-V3-2** (heredada): el mapa parte el texto envuelto en líneas, pero el pintor
  lo emite en una sola línea.
- **D-V3-3** (heredada): el pintor no aplica kerning (deriva medida 4,97 px).
- **D-V3-1** (heredada): el importador rechaza fuentes sin outline.

## Consumo total del módulo V4
- Opus: 67.271 (V4c 25.423 + V4d 41.848; V4a/V4b/V4e 0).
- DeepSeek flash útil: 5.303 (V4a). Kimi: 0.
- V4e: 0 (implementado por Lu; sin llamadas a modelos).

## Lo que se te pide
1. Lista de hallazgos con severidad (Bloqueante / Mayor / Menor) y evidencia.
2. Valora explícitamente H1, H2 y H3: ¿la resolución es correcta? ¿Falta algo?
3. Valora si las deudas D-V4-1, D-V3-2, D-V3-3 están bien caracterizadas o si alguna
   debería ser Bloqueante para cerrar el módulo.
4. Si ves un supuesto falso en el documento (un dato, una premisa, una medición),
   dilo con su corrección.
Responde directamente, sin herramientas.
