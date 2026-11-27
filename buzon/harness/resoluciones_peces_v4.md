# Resoluciones — peces de fin de módulo V4 (Opus + DeepSeek + Gemini)

Pez 1 (Opus, MWAPI): llamada limpia, exit 0. Prompt 10.369 B, contenido 14.545 car.,
input 3.413 tokens, output 5.009 tokens, freno 25.000.
Pez 2 (DeepSeek flash): llamada limpia, exit 0, paquete entero, sin partir, sin
apagar thinking (norma cerrada 2026-11-27). 9.518 B enviados; contenido 6.460 +
razonamiento 62.799 car. recibidos.
Pez 3 (Gemini, `agy --mode plan --print`, proceso fresco, sin --continue):
~122 s, exit 0. (Un tropiezo inicial de sintaxis CLI, corregido con
`--prompt="..."` adjunto.)

Veredicto conjunto: los tres peces rechazan cerrar V4 con el golden ciego al
orden y la desviación de halos como «documentada». Los tres exigen el reorden
y que el golden mida el orden. H2 lo consideran defecto de diseño (no
convención) y exigen arreglo de raíz o test con colores divergentes.

## APLICAR

### A1. H3 — orden de pintado (Bloqueante pez 1 y 2; Bloqueante pez 3)
- Reordenados los hijos del Canvas SOLO con el editor («Mover Arriba/Abajo»,
  30 movimientos: Verde ↑6, Blanco ↑6, Rojo ↓12, Rosa ↓6). Orden final =
  referencia: Fondo, Verde, Blanco, Halos Neón, Neón, Halos Rosa, Rosa,
  Halos Rojo, Rojo. Verificado en JSON.
- El golden mide ahora el orden: nuevo test `orden_pintado_igual_y_nombres_unicos`
  (lista DFS pre-orden + assert de nombres únicos). Falló antes del reorden
  (prueba de que caza el hueco) y pasa después (3/3).
- Físico: captura standalone de la escena rehecha **byte-idéntica** a la de
  referencia (0 píxeles distintos, sha256 34048450ea5a4761 en ambas). F2 queda
  resuelto: la captura física ahora SÍ es verde.
- D-V4-3 (desviación de halos) se RECHAZA como deuda: no existe; la escena se
  corrigió y el golden la cubre.

### A2. H2 — `target_graphic_color` pisaba el tinte (Mayor pez 1 y 2; Bloqueante pez 3)
- Arreglo de raíz en `capa_componente` (parser.rs): el botón aporta su color
  PRIMERO y la Image lo sobrescribe (prioridad declarada imagen > texto >
  botón). Auditoría: es el ÚNICO campo con la inversión — `color` y
  `font_size` solo los pone el texto; `background_color` era el único que el
  botón pisaba. No hay otros campos afectados.
- Test nuevo `imagen_gana_a_target_graphic_color_del_boton` (parser.rs):
  imagen roja + botón azul → fondo rojo; sin imagen → fondo azul del botón.
- El dorado sigue en verde (en referencia y en editor target == tint).

### A3. H1 — inactivos (Bloqueante pez 1; Menor pez 2; Mayor pez 3)
- Test extendido `nodo_inactivo_y_sus_hijos_rect_cero` (layout.rs): además del
  rect cero, ahora fija `texto_visible` vacío en el nodo inactivo y su hijo.
  Contrato documentado en INFORME_V4: `active=false` = rect [0,0,0,0],
  texto vacío, descendientes también; los estilos quedan sin escalar (RAW)
  en el documento derivado porque el nodo no se pinta (inerte en render).
- El aviso al guardar con inactivos NO se hace: en un motor estilo Unity es
  normal guardar paneles/popups ocultos con `active=false`; un aviso sería
  ruido (argumento del pez 3, aceptado). La Jerarquía atenúa los inactivos
  queda como mejora futura (no bloqueante).
- La deuda D-V4-4 (abajo) explica además por qué no se vieron en el editor:
  la GameView no pinta la UI runtime.

### A4. D-V4-1 — pseudo-clases (Bloqueante para «interactivo», Mayor si no)
- V4 se enmarca como **autorado de UI estática**: botones, InputField (dato +
  render), presets, anclas, puente C#. Los estados visuales `:hover`/`:active`
  NO forman parte del alcance de V4; quedan como deuda D-V4-1 para V6.
  El editor CSS avisará de pseudo-clases muertas cuando se implemente (mejora
  anotada, no bloqueante).

### A5. D-V3-2 — wrap (Bloqueante pez 2; Mayor pez 3; Mayor pez 1)
- Fuera del alcance de V4, declarado explícitamente: V4 es texto de una línea
  (los 5 botones no envuelven). `text_overflow: wrap` no está soportado hasta
  que layout y pintor acuerden el multilínea. No se vende como completo.

## REBATIR (con evidencia de código)

### R1. `canvas_scale_factor` SÍ interpola en log (F3 pez 1; punto 1 pez 3)
- Código (scene.rs:1369): `lw = (screen_w / ref_w).ln(); lh = (screen_h / ref_h).ln();
  (lw + (lh - lw) * m).exp()` — interpola en log ENTRE el factor por ancho y
  el factor por alto según `match_width_or_height`. No es redundante: a
  1280×720 ambos factores son 2/3, así que s = 2/3 para cualquier m; a
  1280×900 con m=0,5, s ≈ 0,745 (no 2/3). La frase del documento era correcta;
  se añade la fórmula al INFORME_V4.

### R2. Clippy `-D warnings` + avisos: NO es contradictorio (Mayor pez 1 y 2; punto 2 pez 3)
- Los 3 avisos son de **cargo** (manifiesto), no de rustc/clippy:
  `unused field 'rust-version'` (Cargo.toml:26), `unused dependency
  'glass_core'` (glass_audio), `unused dependency 'egui-winit'`
  (glass_editor). `RUSTFLAGS="-D warnings"` solo convierte lints de rustc en
  errores; no afecta a los avisos de manifiesto de cargo. Por eso clippy
  sale con RC 0. Verificado en la re-ejecución del pipeline.

### R3. «Sondas 15/16» NO es 15 de 16 (Mayor pez 2 y 3)
- 15 y 16 son los **ids de entidad** de las sondas (`UiDynStyleProbe` 15 y
  `UiDynStyleTextProbe` 16) en `UiDynTest.scene.json`. Las 4 sondas dan código
  0 (cierre V4d). No hay sonda incompleta. El puente se verifica por sondas
  físicas (código 0), no por `dotnet test` en CI — limitación conocida del
  enfoque V4d, anotada, no bloqueante.

### R4. El texto NO escribe en `background_color` (Bloqueante 2 pez 3, punto b)
- `capa_componente` (parser.rs): el texto pone `color` y `font_size`; solo
  imagen y botón tocan `background_color`. La lectura del pez 3 era errónea.
  (El punto a — el `#ffffff` del preset V4b en `style_extra` — SÍ es real:
  ver A6.)

### R5. Arrastre 40 px → 107,4627 (Mayor pez 1; punto 5 pez 3)
- No es un delta con s=2/3: es la **posición absoluta** tras el arrastre
  (inicial [0,0] + delta). El delta usa `delta_referencia(delta, s)` con
  `s = canvas_scale_factor(canvas, game_rect)` — el panel de GameView, no el
  viewport 1280×720. El Canvas V4c tiene `match_width_or_height: 0,5`;
  s = 40/107,4627 ≈ 0,3722 implica un game_rect ≈ 715×402 px, consistente
  con el panel del editor (el propio pez 3 derivó 714,7 px). El valor
  coincidió con el esperado precalculado (107,46 ± 0,5). Hueco de
  documentación: el cierre V4c no registró el game_rect; se anota.

### R6. «Tolerancia >10» en la fila superior (punto 4 pez 3)
- La comparación inicial usó umbral >10 solo para agrupar; la medida final
  tras el reorden es **0 píxeles con diferencia >0** (byte-idéntico). El
  umbral ya no aplica.

## ANOTAR (deudas y notas, no bloqueantes)

- **D-V4-4 (nueva)**: la GameView del editor NO pinta la UI runtime
  (`if render_standalone`, main.rs:8118). El arreglo de D1 (etiqueta solo de
  `ui_text` propio) es correcto para el síntoma del doble dibujo en el
  overlay del editor; el hueco de la GameView es una deuda arquitectónica
  aparte. (F5 pez 1; punto 6 pez 2; Bloqueante 3 pez 3 — parcialmente
  rebatido: el diagnóstico de D1 era correcto para la etiqueta; lo que
  faltaba era declarar (1) como deuda propia.)
- **D-V4-2**: auditoría completa — `padding`/`gap` son entradas de layout en
  unidades de referencia (los rects se escalan al final); solo los valores
  pintados necesitan escalado explícito. La sombra escala dx, dy Y blur
  (layout.rs:652-656, verificado). No hay más campos px sin escalar.
- **Preset V4b `#ffffff`**: el `background-color: #ffffff;` del preset en
  `style_extra` gana por diseño (la cascada es defecto → componente →
  style_extra → pseudo) y deja sin efecto `ui_image.color` en esas
  entidades; el control efectivo es el campo de estilo del Inspector, que
  escribe en el mismo CSS. Se documenta la interacción (no es un defecto:
  es la cascada declarada).
- **`ui_set_*` vs `style_extra`**: definido — `ui_set_background_color` (y
  familia) escribe en `style_extra` vía `establecer_declaracion`
  (main.rs:895); los cambios desde C# persisten y no se sobrescriben al
  re-resolver. (`ui_set_text` escribe directo en `ui_text.text`.)
- **InputField (V4b)**: dato + render completos; la edición interactiva
  (foco, caret, teclado) está fuera del alcance de V4 y choca con D-V3-3
  (kerning 4,97 px) cuando se haga el caret — anotado para ese frente.
- **Presets de anclas**: solo mismo rincón (ancla+pivote juntos); sin
  combinaciones mixtas ni stretch. Limitación conocida de V4a.
- **JSON `"transform": null`**: el deserializador aborta; deuda de robustez.
- **D-V3-3 (kerning)**: bien caracterizada como deuda; el golden no compara
  glifos, así que no lo detecta — correcto por diseño del test (rects, no
  glifos). No bloquea V4.
- **D-V3-1**: sin objeción (Menor).
- **Consumo**: ver INFORME_V4 §8 (por proveedor, con unidades).

## ESCALAR
Nada. Todas las resoluciones están dentro del alcance de Lu (arreglos de raíz
del módulo + tests + documentación). El cambio de `capa_componente` se
reporta en el CIERRE del buzón para que Rayo lo vea.
