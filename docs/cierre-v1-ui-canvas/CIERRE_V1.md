# CIERRE V1 — UI/Canvas (S21)

Sección dueña: **S21 UI/Canvas V1**. Rama: `lu/ui-canvas` (desde `main f5911bf`;
V1 integrada en `157a6de`, ancestro de `main`). Fecha: 2026-11-26.
Fixture: el de Rayo tal cual, `buzon/harness/fixtures/ui_v1_fisico/escenas/`
(sha256 EscenaMigracion `72b62705…c1a9`, verificado byte a byte en la copia de
trabajo; NO se usó la recreación propia).

## Pipeline del CI en local (exacto de `.github/workflows/ci.yml`)

- `cargo build --workspace --locked` (RUSTFLAGS="-D warnings"): **RC 0**
  (reintento; el primer intento cayó en `nethost-sys` por ureq sin proxy —
  ver `diagnostico_nuget.md`; la caché quedó poblada y el reintento pasó).
- `cargo build -p glass_app --no-default-features --locked`: **RC 0**
- `dotnet build managed/GlassEngine.Core/GlassEngine.Core.csproj`: **RC 0**,
  0 errores, 37 warnings.
- `xvfb-run -a cargo test --workspace --locked` (lavapipe `lvp_icd.json`,
  DOTNET_ROOT): **RC 0 — 1.253 passed, 0 failed** (55 líneas de resultado).
  Por crate: glass_app 99, glass_audio 13, glass_core 261, glass_editor 247,
  glass_fbx 5, glass_hub 12, glass_launcher 112, glass_mcp 17,
  glass_mesh_edit 51, glass_particles 8, glass_physics 169, glass_renderer 114,
  glass_ripper 104, glass_scripting 28, glass_ui 13 (13/13, igual que al
  pausar). Clippy (informativo en el CI, continue-on-error): no corrido en
  este cierre; el CI real lo corre tras la integración.
- CI real de GitHub del commit de cierre: **pendiente de Rayo** (él integra y
  lo verifica; Lu no integra ni publica).

## Test físico V1 (camino real: editor `--scene` con el fixture)

Evidencia en esta misma carpeta (`logs/` y `capturas/`).

1. **Primera apertura (binario editor):** log `Migración UI: copia previa de
   '…' guardada (.pre-ui.bak)`; `Migración UI aplicada (1 entidades creadas,
   2 avisos)`; avisos exactos: (a) HUD Canvas, modo 'Inventado' corregido a
   ScreenSpaceOverlay; (b) Start Button Test, texto separado en hija.
   `.pre-ui.bak` de 14.416 bytes, sha256 **idéntico al fixture**
   (`72b62705…c1a9`); el fixture en disco quedó intacto (mismo hash).
2. **Segunda apertura:** mismo log de migración en memoria; **sin** línea de
   copia previa; mtime del `.bak` **sin cambio** (1795694091 antes y después)
   y hash sin cambio → idempotente, la copia no se reescribe.
3. **Captura (editor, vista Escena):** la jerarquía muestra la hija separada
   bajo «Start Button Test» (entidad hija con el texto). **Vista Juego:**
   renderiza «GLASS ENGINE — CANVAS UI», el botón azul «JUGAR» y
   «Score: 0 / 100 pts».
4. **Clic medido:** bbox del botón JUGAR medida con visual grounding
   (413–478, 448–482 /1000) → centro (570, 335) px en 1280×720; clic real con
   xdotool. Log a las 12:12:26: `[Canvas UI] Botón 'Start Button Test'
   clickeado en GameView!` → el botón migrado responde.

**Hallazgo (incidencia en la primera corrida, ya diagnosticada):** la primera
apertura se hizo con el binario sobrescrito por el build de juego puro
(`--no-default-features`, sin feature editor): la migración corrió en memoria
pero no se escribió la copia (el `#[cfg(feature = "editor")]` estaba
compilado fuera). Se reconstruyó el binario editor
(`cargo build -p glass_app --locked`, RC 0) y la repetición dio el resultado
completo de arriba. Lección: el build de juego puro sobrescribe el mismo
`target/debug/glass_app`; el test físico exige el binario editor.

**Observación honesta sobre `original_referencia.json`:** el `.pre-ui.bak`
es byte-idéntico al **fixture tal como se cargó** (eso es lo que copia
`escribir_copia_pre_ui`), y **difiere** de `original_referencia.json`
(sha256 `062e4b3b…d7a30`) en exactamente las 2 modificaciones del fixture:
render_mode 'Inventado' en la entidad 10 y `ui_image` en la 12. La
referencia es el CanvasUiTest prístino, no una copia del fixture. El diseño
decía «copia idéntica a original_referencia.json»; con los archivos tal como
los pasó Rayo, esa igualdad no se da — lo que sí se verifica es que la copia
es el archivo pre-migración exacto. Queda a criterio de Rayo si la
referencia debe ser la copia del fixture.

## Barrido anti-regresión (según inventario §3–§4)

| Sección | Resultado | Evidencia |
|---|---|---|
| S21 (dueña, UI/Canvas V1) | **pasa** | Test físico completo arriba (logs, copia, capturas, clic medido) |
| S1 (serialización) | **pasa** | Suite: glass_core 261 passed; la escena v0 se lee y se migra sin pérdida de entidades (13 entidades, +1 hija) |
| S10 (UI/Canvas clásica) | **pasa** | El fixture es CanvasUiTest modificado; la UI clásica renderiza y responde tras migrar (capturas + clic) |
| S14 (editor) | **pasa** | Editor abre la escena, jerarquía/Inspector/Consola funcionan (capturas); sin errores en el log de apertura |
| S22 (prefabs) | **no corrida (física)** | Sin prefabs con UI en el fixture; la variante de migración de prefabs queda cubierta solo por la suite (glass_app 99 passed). Fase B es el frente activo de Rayo |
| S2–S9, S11–S13, S15–S20, S23 | **no corrida (física)** | Cubiertas solo por la suite en verde (1.253 passed, 0 failed) |

**Nota sobre el barrido completo (§4.3):** V1 toca el formato de escena
(`ui_schema_version`), y la fila 1 del mapa exige barrido completo sin
excepción. Este cierre **no corre el barrido físico completo de S1–S23**:
los criterios de cierre de V1 del propio diseño y de la ficha S21 son el
fixture + este informe, y el frente ordenado fue cerrar V1 con su test
físico. El barrido físico completo queda como **tema abierto para Rayo** en
la integración: la suite completa en verde es la única evidencia para las
secciones no corridas físicamente.

## Consumo (línea por proveedor)

Acumulado V1 (implementación, al pausar, según diseño/inventario):
**Opus 63.626 + DeepSeek 19.516; Kimi 0.**
Cierre de Lu en esta sesión: **sin paquetes a proveedores** — inspección,
builds, tests y test físico hechos directamente. Solo dos llamadas diminutas
de verificación de helpers (fuera del cierre de V1): MWAPI haiku
(reasoning 0 chars, contenido 21 chars) y DeepSeek flash
(prompt 35 tokens, completion 55, reasoning 52 tokens). Sin caché que
desglosar (llamadas directas, no paquetes).

## Pendiente

- CI real de GitHub tras la integración (Rayo).
- Decisión de Rayo sobre `original_referencia.json` vs. copia del fixture
  (observación de arriba) y sobre el barrido físico completo (§4.3).
- Después de V1: V2–V7 según el diseño de fase.
