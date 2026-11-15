# Registro — Input en el editor: «ningún control sirve» en Plat3D (2026-11-14)

Reporte del amigo probador sobre la 0.10.14.1-beta: probó el
plataforma (Plat3D) y «ningún control servía»; precisó después que
presionó todo el teclado y no funcionó nada. Del FPS no reportó
fallos. Orden de D3m3tr10 (22:52/22:54): causa raíz demostrada,
arreglo de raíz en el mecanismo (no en Plat3D), verificación en vivo
por el camino del usuario real (Launcher → editor → escena → Play).

## Causa raíz (demostrada en código y en vivo)

**Los clics sobre botones del Canvas se pierden en la vista Game del
editor, por dos capas acopladas:**

1. La ruta runtime de clics del Canvas
   (`button_click_tracker` → `script_host.invoke_button_click`, la
   misma que usa el standalone) está desactivada en el editor:
   `let ui_clicks_active = self.is_standalone;`
   (`crates/glass_app/src/main.rs`). El comentario afirmaba que «la
   superposición de egui ya gestiona sus clics»: es falso.
2. La vista Game del editor dibuja su PROPIA copia egui del Canvas
   (`render_in_game_canvas_ui` en
   `crates/glass_editor/src/panels/viewport.rs`) con botones egui de
   verdad que, al pulsarse, SOLO escriben un log
   («[Canvas UI] Botón … clickeado en GameView!»): la acción
   `on_click_action` de la entidad jamás se despacha al script.

A eso se sumaba una tercera inexactitud del mismo mecanismo: en el
editor, la posición del ratón se convertía a coordenadas del
offscreen usando la ventana entera, cuando la vista Game es un
sub-rectángulo de la ventana — ni siquiera alimentando el tracker
habrían coincidido los clics.

**Por qué el amigo sintió un fallo total de teclado:** quedó atrapado
en la escena de menú. El menú de Plat3D se maneja SOLO con el botón
JUGAR (Canvas); la escena de menú no tiene ningún control de teclado
(ni jugador, ni navegación por teclas). Con el botón muerto, presionar
todo el teclado no hace literalmente nada.

## Lo que NO estaba roto (verificado en vivo, editor + Play)

- **El teclado en partida funciona.** En `Plat3D_L1` con Play, la W
  movió al Jugador: el Inspector marcó su posición en
  (−5.50, −4.26, 3.87) tras andar y caer del borde, y el ciclo de
  muerte/reaparición lo devolvió al spawn (Y = 0.9989). La cadena
  winit → `keys_pressed`/egui → banco de ejes → snapshot → C# está
  sana en modo editor (capturas `fase1/test-input/l04–l07`).
- **La demo FPS funciona en editor + Play**: la W mueve la vista y
  los clics disparan (`[DEMO] DISPARO arma=Pistola 9mm` en el log).
  Su gameplay usa el snapshot de input (teclas y botón de ratón), no
  botones del Canvas (capturas `fase1/test-input/f01–f03`).
- **La física nueva (sin RB/CC no hay física) NO rompe Plat3D**: sus
  entidades llevan los bloques de física explícitos en las escenas y
  sus scripts crean los cuerpos por código
  (`Plat3DPlayerController` hace `RigidBody.Initialize`); nada depende
  de los cuerpos que antes auto-creaba `EntityData::new`.
- **Reproducción del fallo**: editor con `Plat3D_Menu`, Play, clic
  real sobre JUGAR → nada: ni línea `[Canvas UI]` de despacho, ni
  carga de escena, el menú queda fijo (capturas
  `fase1/test-input/m05–m06`).

## Arreglo (de raíz, en el mecanismo de input del editor)

- `ViewportPanel` publica cada frame el rectángulo donde se pinta el
  juego (`game_view_rect`, en puntos egui) desde `render_game_tab`.
- En Play, la superposición egui dibuja los botones del Canvas como
  **pintura no interactiva** (rect + texto), para no consumir el clic;
  fuera de Play conserva el botón egui de vista previa.
- En `main.rs`, la conversión del ratón usa el rect publicado en modo
  editor (con `pixels_per_point`), sin clampar: fuera del rect el
  hit-test falla solo, como en el standalone. El mismo mapeo alimenta
  el snapshot de ratón que leen los scripts.
- `ui_clicks_active` pasa a ser verdadero también cuando el editor
  está en `Playing`: el tracker runtime (clics y drag & drop del
  Canvas) despacha por la MISMA ruta que el juego publicado. Un solo
  mecanismo para editor y standalone.

Generación: Claude Opus (`claude-opus-5-5`), 13.053 tokens
(10.303 prompt + 2.750 completion), 8 pares VIEJO/NUEVO; integración,
verificación y este registro: Rayo.

## Verificación en vivo del arreglo

- **Pipeline local exacto al CI: VERDE** — build workspace 0,
  clippy exacto 0, build `--no-default-features` 0, dotnet 0 errores,
  tests 509/509.
- **Camino completo en el editor** (binario de editor, escena
  `Plat3D_Menu`): Play → **clic en JUGAR → el log registra el despacho
  por la ruta runtime** («Botón de la entidad 13: método 'PlayGame'
  invocado en 'Plat3DMenuButtons'») → `Plat3D_Loading_L1` activa
  (8 entidades) → `Plat3D_L1` activa (47 entidades) → ya en L1, la
  **W mueve al Jugador** (Inspector: posición (−5.50, 0.8938, 3.5921)
  tras andar hasta el borde). Capturas `fase1/test-input/v01–v04`,
  log `fase1/test-input/fix_editor.log`. El primer clic cayó a la
  primera: el mapeo por el rect de la vista Game es correcto.
- **Control standalone** (binario de juego, mismo menú): clic en
  JUGAR → mismo despacho → Loading → L1 activa. La ruta de siempre
  queda intacta (captura `fase1/test-input/s02`, log
  `fix_standalone.log`).
- Nota cosmética: el mensaje de log del tracker dice «clickeado en el
  standalone» también cuando despacha desde el editor, porque la ruta
  ahora es compartida; el texto es anterior al arreglo y no afecta al
  comportamiento.
