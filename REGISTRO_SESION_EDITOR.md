# Registro — Sesión del editor (2026-11-13)

Sesión ordenada tras la Fase 10 y la escalera de esferas, con cuatro
puntos: (1) el defecto del FOV en el standalone con editor, (2) la
verificación del panel Project, (3) el «desfase» del fantasma al
arrastrar en Hierarchy y (4) la importación por arrastre desde el SO.
Todo verificado en vivo en la VM (Xvfb + lavapipe), con capturas en
`fase1/test-editor/` (fuera del repo).

## 1. FOV en el standalone con editor — ARREGLADO (commit `8ee510b`)

**Causa raíz (distinta de la hipótesis inicial):** el puente
`SetCameraFov` y el dato siempre fueron correctos. El binario CON
editor en `--standalone` presentaba en pantalla el *post* de la
**vista de escena del editor** (`final_offscreen`, cámara orbital de
FOV fijo), no la vista de juego. En el binario puro no pasaba porque
allí `vp_scene == vp_game`. Por eso la vista «seguía» al contenido
pero la mira del francotirador (FOV 18) nunca se presentaba
(medido ×1,13 en la Fase 10).

**Arreglo:** `Renderer.present_game` (defecto `false`): cuando está
activo, el blit al swapchain y el overlay del Canvas usan
`final_game_offscreen`. `glass_app` lo activa solo en el binario con
editor cuando `is_standalone`. El modo editor no cambia.

**Verificación:** la sesión de mira repetida pasa de plano general a
primer plano telefoto con `fov=18` (capturas
`campo_mira_pre_fov.png` / `campo_mira.png` en `fase1/test-f10/`),
con el HUD del Canvas componiendo sobre la vista de juego. Suite
390/390 en el momento del arreglo.

## 2. Panel Project — VERIFICADO EN VIVO (sin cambios)

Ejercitado con el editor abierto sobre el propio proyecto:

- Crear **Script C#** con nombre → archivo en
  `managed/GlassEngine.Core/Scripts/` ✓
- Crear **Carpeta** → directorio en `assets/` ✓ (sin nombre, el panel
  avisa «Escribe un nombre antes de crear el asset»; con nombre
  repetido, «Ya existe …»)
- **Renombrar** inline desde el menú contextual (✓ o Enter) →
  renombrado en disco ✓
- **Mover a…** (submenú con las carpetas de assets) → movido en
  disco y desaparecido del origen ✓
- **Eliminar** → va a la papelera segura `.project_trash/` (nada se
  borra) ✓
- Navegación por doble clic entre carpetas ✓; las carpetas se
  ordenan antes que los archivos ✓

Los artefactos de prueba se retiraron al terminar; el árbol queda
limpio.

## 3. Hierarchy: el fantasma «despegado» — YA ESTABA ARREGLADO; verificado

El arreglo es de la pasada del 2026-11-06 y sigue vigente en el
código: al `drag_started` se guarda `drag_grab_offset = ratón −
esquina de la fila` y el fantasma se dibuja en la capa Foreground en
`ratón − drag_grab_offset`, conservando el ancho de la fila.

Verificación con dos arrastres medidos píxel a píxel sobre las
capturas: la esquina del fantasma coincide con
`puntero − offset_de_agarre` (agarre al centro y al borde izquierdo
de la fila). No queda desfase sistemático.

El reparentado por arrastre (soltar en la zona central de una fila →
`Inside`) también funciona: verificado en vivo con
«High Platform Step 2» pasando a hija de «Static Floor» (indentada,
con flecha de despliegue en el padre). Nota honesta: con entrada
sintética a ~5 fps algunas repeticiones del gesto no cuajaron (la
zona de soltado se evalúa por frames); con ratón real el gesto es
continuo.

## 4. Importación por arrastre desde el SO — IMPLEMENTADA

Antes: cero manejo de `dropped_files` en todo el editor.

Ahora (`EditorLayout::render` + `ProjectPanel`):

- Al soltar archivos sobre la ventana del editor se copian a la
  **carpeta visible del panel Project**, con nombre libre si hay
  colisión (`ruta_libre_en`: sufijo « (2)», « (3)»…), el panel
  refresca, selecciona lo importado y lo anota en su línea de estado
  («Importado: assets/…» / «Importados N archivos en …»).
- Mientras se sobrevuela arrastrando, una banda translúcida anuncia
  «Suelta para importar en <carpeta visible>».
- Los directorios soltados se rechazan con error en el estado (la
  importación es de archivos); un error no detiene el resto.
- Lógica en `project.rs`: `importar_archivo` (función libre) e
  `importar_soltados` (método). Test nuevo
  `importar_archivo_copia_y_resuelve_colision`: copia, colisión con
  sufijo, rechazo de directorio y de origen inexistente.

**Verificación de extremo a extremo:** cliente XDND propio
(`fase1/xdnd_drop.py`, python-xlib) que ejecuta el protocolo real
contra la ventana del editor — hallazgo: winit pide y parsea la
selección en el `XdndPosition`, no en el `XdndDrop`; sirviendo la
selección en ese momento, el editor importó el archivo soltado y el
panel mostró «Importado: assets/ArrastrePrueba.txt». Suite: **391/391**.

Código del paquete: Sonnet 5 (14.922 tokens medidos; un primer
intento con prompt más largo murió por el timeout del cliente sin
medición). La adaptación a la API de egui 0.36 (`DroppedFile::path()`
método, `content_rect()`) y la integración fueron de Rayo.

## Consumo de la sesión

- FOV (punto 1): sin APIs (investigación y arreglo propios de Rayo).
- Panel Project y Hierarchy (puntos 2–3): sin APIs (verificación).
- Arrastre (punto 4): **Sonnet 5: 14.922 tokens** (+ 1 intento sin
  medición por timeout del cliente). Opus 0 · DeepSeek 0 · Kimi 0.

## Estado al cierre

Cola siguiente según la orden del 2026-11-13: **integración de
iluminación** (Spot que emite, asignación por objeto con histéresis,
`PerfilDeLuces`, IBL de skybox; cierre con GIF obligatorio).
