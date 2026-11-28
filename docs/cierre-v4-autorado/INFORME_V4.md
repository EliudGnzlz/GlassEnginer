# INFORME V4 — Autorado estilo Unity (módulo UI/Canvas)

Rama `lu/v4-autorado`. Cierre de módulo 2026-11-27.
**V4 NO se declara verde ni se integra hasta la verificación única de Análisis.**

## 1. Alcance

V4 es **autorado de UI estática estilo Unity**: border, letter-spacing, InputField
real (dato + render), arrastre en GameView, anclas + presets, puente de estilos
Rust+C# y la pantalla de 5 botones reconstruida solo con el editor (G-AUTOR1).
Los estados visuales `:hover`/`:active` y la edición interactiva de InputField
NO forman parte de V4 (deudas D-V4-1 y frente posterior).

## 2. Paquetes

### V4a — border / letter-spacing / Q4 / anclas + presets + Inspector
- `MODO_BORDE` con regla premultiplicada; `border` y `letter-spacing` en
  `StyleProps`; el parser convierte sRGB→lineal y el ángulo de degradado se
  guarda como CSS − 90 (90deg CSS = izquierda→derecha).
- Campos tipados de estilo en el Inspector, marca «controlado por estilo»,
  editor CSS con avisos en vivo, anclas + 8 presets.
- Suite 1.422/0. Consumo: DeepSeek flash útil 5.303.

### V4b — `UIInputFieldData` real + presets Panel/Imagen/InputField
- `TipoGraphic::InputField` (valor o placeholder atenuado); sección InputField
  en el Inspector. Hallazgo físico: el layout solo construía el mapa de texto
  para `Texto` — arreglo de raíz + test de regresión.
- Los presets ponen `background-color: #ffffff;` en el `style_extra`.
- Suite 1.431/0. Consumo: 0 (Lu).

### D-V4-2 (adenda, decisión de Rayo)
- El layout escala font_size, letter-spacing, border-radius, ancho de borde y
  sombra por `s` en el mismo camino que usa el mapa. Medido a 1280×720
  (s=2/3): «B O T Ó N» de 138×27 a 92×18 px. Suite 1.432/0.

### V4c — arrastre en GameView + D1
- D1 en la raíz: la etiqueta del botón sale SOLO de su propio `ui_text`
  (antes, un botón sin `ui_text` propio dibujaba «Button» en el overlay y el
  hijo dibujaba «JUGAR»: doble dibujo). La etiqueta vive en el overlay del
  editor; el texto renderizado vive en el hijo Texto y no se tocó.
- Arrastre medido: 40 px pantalla → posición 107,4627 en referencia
  (esperado 107,46 ± 0,5). Es posición absoluta: el delta usa
  `delta_referencia(delta, s)` con `s = canvas_scale_factor(canvas,
  game_rect)` — el panel de GameView (≈715×402, m=0,5, s≈0,3722) —, no el
  s=2/3 del viewport 1280×720. El cierre V4c no registró el game_rect
  (hueco de documentación, anotado).
- Suite 1.441/0. Consumo: Opus 25.423.

### V4c adenda — `ascender_a_control` + Guardar sobre `--scene`
- La carga al arranque no ponía `ruta_escena_actual`. Suite 1.452/0.

### V4d — puente de estilos Rust + C#
- 9 métodos `ui_set_*` (Rust + FFI + C#); sondas `UiDynStyleProbe` (entidad
  15) y `UiDynStyleTextProbe` (entidad 16) en `UiDynTest.scene.json`; las 4
  sondas dan código 0. G-AUTOR3 físico en verde.
- `ui_set_*` escribe en `style_extra` vía `establecer_declaracion`
  (main.rs:895): los cambios desde C# persisten y no se sobrescriben al
  re-resolver. `ui_set_text` escribe directo en `ui_text.text`.
- Suite 1.452/0. Consumo: Opus 41.848.

### V4e — pantalla de 5 botones solo con el editor (G-AUTOR1) + cierre
- 40 entidades rehechas con el editor, sin editar el JSON a mano; nombres
  idénticos a la referencia; orden de hijos igualado (ver §3).
- Golden: `dorado_botones.rs`, 3 tests (ver §3). Suite final **1.456/0**.
- Pipeline exacto del CI en local (ver §4).

## 3. G-AUTOR1 — el dorado y el físico

- Golden test `crates/glass_ui/tests/dorado_botones.rs`: deriva el documento
  de la escena del editor y de la referencia y las compara por NOMBRE
  (los ids los asigna el editor): rect tolerancia 0,5 px, estilos 1e-3.
  Dorado versionado: `fixtures/dorado_botones_1280x720.json` (40 nodos).
- Nuevo test `orden_pintado_igual_y_nombres_unicos`: compara la lista DFS
  pre-orden de nombres (orden de pintado) y exige nombres únicos. Se añadió
  porque el golden por nombre no veía el orden (H3).
- Resultado: **3 passed** (`dorado_desde_referencia`,
  `editor_igual_a_dorado`, `orden_pintado_igual_y_nombres_unicos`).
- Físico: standalone con la escena del editor y con la de referencia,
  capturas 1280×900. Comparación píxel a píxel: **0 píxeles distintos,
  byte-idéntico** (sha256 `34048450ea5a4761` en ambas). Capturas:
  `hidden_files/v4e_fisico/standalone_editor3.png` (editor) y
  `standalone_ref.png` (referencia).
- El dorado se mide a 1280×720 (misma proporción que la referencia
  1920×1080, s=2/3 para cualquier `match_width_or_height`); el físico a
  1280×900 (ambas escenas al mismo viewport, comparación válida).

## 4. Hallazgos de raíz de V4e

### H1. Tres botones inactivos (22 de 24 diferencias del dorado)
Blanco (10), Neón (11) y Rosa (13) tenían `active=false` en el JSON guardado.
Un nodo inactivo → rect [0,0,0,0] para sí y descendientes, `MapaTexto`
vacío y estilos RAW sin escalar. Se reactivaron con el checkbox del
Inspector; JSON final `inactive: []`. Test de regresión extendido
(`nodo_inactivo_y_sus_hijos_rect_cero`: rect cero + `texto_visible` vacío).
Contrato: `active=false` = rect cero, texto vacío, descendientes también;
los estilos quedan sin escalar en el documento derivado porque el nodo no
se pinta (inerte en render). No se avisa al guardar con inactivos: en Unity
es normal guardar paneles ocultos así; sería ruido.

### H2. `capa_componente`: `target_graphic_color` pisaba el tinte de la Image
La prioridad imagen > texto > botón se aplicaba a `style_extra`, no a
`background_color`. Arreglo de raíz: el botón aporta su color primero y la
Image lo sobrescribe (parser.rs). Auditoría: es el único campo con la
inversión (`color`/`font_size` solo los pone el texto). Test nuevo con
colores divergentes (`imagen_gana_a_target_graphic_color_del_boton`).
En la referencia `target_graphic_color == ui_image.color` en los 5 botones;
el dorado sigue en verde.

### H3. Orden de pintado de los halos
Los halos de Rosa/Rojo (y los de Neón) se crearon después de sus botones y
se pintaban encima. Reordenados con «Mover Arriba/Abajo» solo con el editor
(30 movimientos: Verde ↑6, Blanco ↑6, Rojo ↓12, Rosa ↓6). El orden final de
hijos del Canvas iguala la referencia. El golden ahora mide el orden (§3) y
el físico es byte-idéntico. No queda como deuda: la escena se corrigió.

### D1 (V4c) — precisión
El arreglo (etiqueta solo de `ui_text` propio) es correcto para el doble
dibujo en el overlay del editor. La GameView sin UI runtime es una deuda
aparte: ver D-V4-4.

## 5. Pipeline (exacto del CI, en local)

- `cargo build --workspace --locked`: OK.
- `cargo clippy --workspace --all-targets` con `RUSTFLAGS="-D warnings"`: OK
  (RC 0). Los 3 avisos son de **cargo** (manifiesto), no de rustc:
  `unused field 'rust-version'`, `unused dependency 'glass_core'`
  (glass_audio), `unused dependency 'egui-winit'` (glass_editor).
  `RUSTFLAGS` no afecta a los avisos de manifiesto de cargo.
- `cargo build -p glass_app --no-default-features --locked`: OK.
- `dotnet build managed/GlassEngine.Core`: 0 errores.
- `xvfb-run -a cargo test --workspace --locked`: **1.456 passed / 0 failed**
  (log `/tmp/v4e_suite2.log`).
- `canvas_scale_factor` (scene.rs:1369): `lw=(screen_w/ref_w).ln()`,
  `lh=(screen_h/ref_h).ln()`, `(lw+(lh-lw)*m).exp()` — interpola en log
  ENTRE el factor por ancho y el factor por alto según
  `match_width_or_height`. A 1280×720 ambos son 2/3 → s=2/3.

## 6. Barrido anti-regresión S1–S23

V4e no tocó código del motor salvo `capa_componente` (H2) y tests; el resto
de cambios de código del módulo (V4a–V4d) tuvieron su cierre con suite en
verde. Evidencia a nivel de módulo: suite completa **1.456/0**; dorado 3/3;
físico byte-idéntico. S21 (dueña) verificada; S1 sin cambios de formato
(no hace falta barrido completo); S14/S10/S2/S3 sin cambios en V4e y en
verde por la suite. Sin regresiones detectadas.

## 7. Peces de fin de módulo (Opus + DeepSeek + Gemini)

Documento de revisión: `hidden_files/v4e_fisico/PECES_V4.md`. Resoluciones
completas en `hidden_files/v4e_fisico/RESOLUCIONES_PECES_V4.md`.

- **Pez 1 (Opus)**: 3 Bloqueantes (F1 golden ciego al orden, F2 cabecera
  vendía verde una captura con 2,667 %, H1 sin decisión de raíz), 6 Mayores,
  4 Menores. No aceptaba cerrar con F1/F2 sin resolver.
- **Pez 2 (DeepSeek flash)**: 2 Bloqueantes (H3 orden, D-V3-2 wrap),
  5 Mayores, 6 Menores. Exigía reordenar halos y no aceptaba H2 como
  convención.
- **Pez 3 (Gemini)**: 4 Bloqueantes (golden ciego, cascada de color, D1 mal
  diagnosticada, H3 no aceptable), 6 Mayores, 3 Menores.
- **Resuelto**: H3 (reorden + golden mide orden + físico byte-idéntico),
  H2 (arreglo de raíz + test), H1 (test de regresión + contrato), F1/F2
  (el golden mide el orden y el físico es verde). **Rebatidos con código**:
  `canvas_scale_factor` sí interpola en log (ancho↔alto); clippy (avisos
  de cargo, no de rustc); «sondas 15/16» son ids de entidad, no 15 de 16
  (las 4 dan código 0); el texto no escribe en `background_color`; el
  arrastre usa el s del panel de GameView (≈0,3722), no 2/3.
- Sin re-auditoría pedida por ningún pez.

## 8. Consumo del módulo V4

| Paquete | Opus | DeepSeek flash | Kimi |
|---|---|---|---|
| V4a | 0 | 5.303 (útil) | 0 |
| V4b | 0 | 0 | 0 |
| V4c | 25.423 | 0 | 0 |
| V4d | 41.848 | 0 | 0 |
| V4e (peces) | 5.009 out / 3.413 in (tokens, pez 1) | 6.460 contenido + 62.799 razonamiento (pez 2) | 0 |
| **Total** | **72.280** | **11.763 útil/contenido** | **0** |

Gemini (pez 3): ~122 s, sin conteo de tokens. V4e sin peces: 0 (Lu).
El freno `--max-razonamiento` (25.000) estuvo armado en las llamadas Opus.

## 9. Deudas

- **D-V4-1**: `:hover`/`:active` se parsean pero nunca se aplican
  (`resolver_estilos` usa siempre `EstadoPseudo::default()`). V4 es autorado
  de UI estática; los estados visuales van en V6. El editor CSS avisará de
  pseudo-clases muertas (mejora anotada).
- **D-V4-4 (nueva)**: la GameView del editor no pinta la UI runtime
  (`if render_standalone`, main.rs:8118). Deuda arquitectónica.
- **D-V3-2**: el mapa envuelve en líneas pero el pintor emite una sola línea.
  Fuera del alcance de V4 (texto de una línea); `text_overflow: wrap` no está
  soportado hasta acordar el multilínea.
- **D-V3-3**: el pintor no aplica kerning (deriva 4,97 px). Bloqueará un
  caret preciso en InputField si no se unifican los avances.
- **D-V3-1**: el importador rechaza fuentes sin outline (Menor).
- **D-V4-2**: cerrada. Auditoría: `padding`/`gap` son entradas de layout en
  unidades de referencia (los rects se escalan al final); la sombra escala
  dx, dy y blur. Sin más campos px sin escalar.
- Presets de anclas: solo mismo rincón; sin mixtas ni stretch.
- JSON `"transform": null` aborta la carga (robustez).
- InputField: edición interactiva (foco, caret, teclado) fuera de V4.
- El `#ffffff` del preset V4b en `style_extra` gana por diseño y deja sin
  efecto `ui_image.color`; el control efectivo es el campo de estilo del
  Inspector (misma CSS).

## 10. Estado

Módulo V4 CERRADO por Lu. Rama `lu/v4-autorado` (punta tras este informe).
**Pendiente la verificación única de Análisis antes de integrar en `main`.**
Sigue V5 (grupos/recortes) por orden de Rayo, tras la verificación.
