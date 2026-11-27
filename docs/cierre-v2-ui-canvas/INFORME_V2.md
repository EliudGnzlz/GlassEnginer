# Informe de módulo — UI/Canvas V2

Rama `lu/v2-nucleo`, punta `355dcb2` (+ arreglos clippy del cierre, commit por
confirmar). Coordinador: Rayo. Segunda agente: Lu. Fecha: 2026-11-26.

## 1. Alcance

Núcleo de la UI nueva sobre el crate `glass_ui` (sin Vulkan/egui): parser CSS
+ cascada, layout (absoluto, flex, grid), pintor CPU + vértices GPU,
interacción (hit-test, clic, arrastre) con paridad con el sistema viejo,
tests del viejo portados antes de borrarlo, imágenes doradas, migración V1
intacta, y retirada del sistema viejo en un único commit revertible. Sin
cambios al formato de escena (S1). P0–P4 cerrados y dados por buenos por
Rayo paquete a paquete (P4a `20d9d22`, P4b1 `7f87db0`, P4b2 `0b28d65`,
P4c `717b9b2`, P4d `355dcb2`); la revisión de fin de módulo es única y
sobre la rama entera (P3 incluido).

## 2. Pipeline exacto del CI, en local

- `cargo build --workspace --locked`: RC 0.
- `cargo build -p glass_app --no-default-features --locked`: RC 0.
- `dotnet build managed/GlassEngine.Core/GlassEngine.Core.csproj`: 0 errores.
- `cargo clippy --workspace --all-targets --locked`: **RC 0** (tras arreglar
  15 lints del código V2 + 1 comentario huérfano dejado por el borrado de
  `resolve_scene_ui_rects`; quedan solo 3 avisos preexistentes de manifiesto:
  `rust-version` sin uso y dependencias sin uso en `glass_audio`/`glass_editor`).
- `xvfb-run -a cargo test --workspace --locked`: **1.390 passed, 0 failed**
  (log `v2_suite_final.log`, tras los arreglos clippy).

## 3. CI real de GitHub (honesto)

| commit | run | resultado |
|---|---|---|
| `20d9d22` P4a | 45854271541 | success |
| `7f87db0` P4b1 | 45859576204 | success |
| `0b28d65` P4b2 | 45864657879 | success |
| `717b9b2` P4c | 45870626519 | **failure** (exit 101) |
| `355dcb2` P4d | 45872299666 | **failure** (exit 101) |

La suite local no reproduce el fallo (1.395/0 en P4c, 1.390/0 en P4d). Los
logs del CI no se pueden leer sin autenticación (403). Diagnóstico en curso:
reproducción con copia limpia de `355dcb2` y los pasos exactos del CI
(`~/workspace/ci-repro-wt`, log `ci_repro_build.log`). **El módulo NO se
declara verde en CI**: la verificación única del lado de Análisis tiene la
última palabra, y este informe deja el rojo por escrito en vez de
esconderlo. La rama se entrega con el pipeline local en verde y el
diagnóstico del CI abierto.

## 4. Test físico (medido, P4c)

- MCP `glass_ui_tree` contra el fixture V1: rects dentro de 0,5 px.
- Mínimo S22: instancia de prefab con UI coloca sus rects ≤0,5 px.
- Clic medido en «Jugar»: bbox [443,473,558,525]/1000 → centro (640,359);
  log `[Canvas UI] Botón 'Start Button Test' clickeado` y C# invocado con
  entidad 12, acción 'StartGame'.
- Arrastre medido panel→casilla: acciones 'SoltarEnCasilla' (entidad 21) y
  'FinArrastre' (entidad 20); el panel queda encajado en la casilla
  (verificado por test unitario paso a paso: rect final [733.33, 320, 80, 80]).
- Editor abierto con la escena: jerarquía, Inspector y consola sin errores;
  GameView idéntica a V1 píxel a píxel en el botón.
- Evidencia: `hidden_files/p4c_fisico/` (logs + capturas).

## 5. Barrido anti-regresión por secciones

Regla aplicada (inventario §3): V2 no toca el formato de escena (S1), así
que no exige barrido completo; el barrido físico cubre la dueña + vecinos
declarados al abrir el módulo (S21, S10, S14, S3, S2) + mínimo S22. El
resto queda en verde de suite, sin cimiento tocado.

| Sec. | Nombre | Estado | Evidencia / motivo |
|---|---|---|---|
| S1 | Escena, entidades, transforms | suite | Sin cambio de formato (congelación P4 registrada); suite 1.390/0 |
| S2 | Scripting C# y API Unity | pass | Acción por nombre + arrastre por atómicos, medido en P4c |
| S3 | MCP base y Command Queue | pass | `glass_ui_tree` lee el documento colocado (test + físico) |
| S4 | Física base y colliders | suite | Sin tocar; suite verde |
| S5 | Render base y materiales | suite | Sin tocar; suite verde |
| S6 | Shaders (rediseño 2S) | pass | Shaders UI nuevos compilan; `.spv` verificados 1:1 con regeneración fresca |
| S7 | Iluminación | suite | Sin tocar; suite verde |
| S8 | Audio (Fase 3) | suite | Sin tocar; suite verde |
| S9 | Animación (runtime) | suite | Sin tocar; suite verde |
| S10 | UI/Canvas clásica | pass | GameView del editor idéntica a V1 |
| S11 | Assets e importación | suite | Sin tocar; suite verde |
| S12 | Partículas | suite | Sin tocar; suite verde |
| S13 | Portales y escaparate | suite | Sin tocar; suite verde |
| S14 | Editor: sesión, undo, Inspector, Gizmos | pass | Escena abierta en editor; jerarquía/Inspector/consola OK |
| S15 | Hub/Launcher | suite | Sin tocar; suite verde |
| S16 | Paridad RB/CC y multinúcleo | suite | Sin tocar; suite verde |
| S17 | Demo FPS / integral | suite | Sin tocar; suite verde |
| S18 | MeshForge (PAUSA) | no corre | En pausa; no tocado |
| S19 | Colliders como componentes | suite | Cerrado; sin tocar; suite verde |
| S20 | Importación Unity (PAUSA) | no corre | En pausa; no tocado |
| S21 | UI/Canvas V1 (dueña) | pass | Físico P4c + suite; paridad snapshot 0,0 px |
| S22 | Prefabs vivos | pass | Mínimo: instancia con UI ≤0,5 px; formato congelado al empezar P4 |
| S23 | MCP completitud | suite | Sin tocar; suite verde (el frente MCP-UI es posterior a V2) |

## 6. Consumo total del módulo (por proveedor)

Medición por bytes enviados / caracteres recibidos (el `usage` del gateway
no es fiable). `reasoning_chars` de Opus reportado como 0 en todas las
llamadas (el gateway no expone el razonamiento de Opus).

| Proveedor | Total medido (contenido) | Notas |
|---|---|---|
| Opus (MWAPI) | P2a 18.652; P2b 960 + 23.287; P2c 21.918; P3a 41.610; P3b1 14.505; P3b2 12.191; P4a1 31.535; P4b1 108. **Total ≈ 164.766 caracteres** | Freno `--max-razonamiento` armado (25.000) en todas las llamadas grandes. P4b1 rehusó el delta a ciegas (108). |
| DeepSeek flash | 0 medido | P0/P1 por subagente sin captura de medición (hueco registrado). P2–P4: 0 llamadas. |
| Kimi | 0 | Sin saldo en ambos lados. |

**Hueco reconocido**: P0/P1 no tienen mediciones capturadas (ni Opus ni
DeepSeek). No se pueden reconstruir a posteriori; a partir de P2 todas las
llamadas llevan su medición en el cierre del paquete. Se anota como deuda
de proceso, no como cifra inventada.

## 7. Revisión de fin de módulo (peces)

Dos peces, familias distintas, solo con este informe como contexto
(`REVISION_V2.md`; resultados en `pez1_v2_result.json`,
`pez2_v2_result.json`).

### Pez 1 — Opus, comprensión (razonamiento 0, contenido 3.828)

Paráfrasis correcta (0 bloqueantes de comprensión). Hallazgos:

1. **Bloqueante — CI de P4c en rojo sin diagnóstico** → APLICAR: repro
   limpio en curso (§3); el informe lo deja por escrito y no declara el
   módulo verde en CI.
2. **Mayor — Métricas de consumo incompletas (P0/P1)** → APLICAR (parcial):
   hueco declarado en §6; no reconstruible.
3. **Mayor — Clippy no ejecutado** → APLICAR: clippy corrido en el cierre,
   15 lints arreglados, RC 0 (§2).
4. **Menor — P0 «23 tests portados» ambiguo** → APLICAR: aclarado — P0
   definió los 23 tests (puerta G1 = definidos); verdes 23/23 por primera
   vez en P4a.
5. **Menor — Doble etiqueta «JUGAR»/«Button» sin rastreo** → APLICAR:
   registrado como deuda en §8 (frente de faltas de la UI del editor).
6. **Menor — Dorada no cubría texto claro sobre fondo oscuro** → APLICAR:
   P4c añadió el test de regresión `texto_extendido_pinta_glifos`
   (blanco sobre negro, cobertura 0,426→0,132).

### Pez 2 — DeepSeek flash, lente crítica (razonamiento 16.073, contenido 1.946)

1. **Bloqueante — CI de GitHub en rojo** → APLICAR: ídem pez 1.1.
2. **Mayor — Dorada P3c insuficiente** → APLICAR: ídem pez 1.6.
3. **Mayor — Standalone sin bisect** → REBATIR (con evidencia): la
   captura de V1 ya muestra el mismo 3D negro en GameView; la estela
   sigue del pase UI con LOAD + objetivo sin refrescar, ambos
   preexistentes a V2. ANOTAR: triado por Rayo a su frente.
4. **Mayor — Paridad G1 debilitada por las 2 correcciones** → REBATIR
   (con evidencia): ambas correcciones se probaron contra el fixture
   congelado (factor 2/3 con referencia 1920×1080; ancla [0,0] =
   esquina inferior, según el snapshot V1) y P4b2 midió paridad 0,0 px
   en la escena real. Las correcciones acercan los tests a V1, no los
   alejan.
5. **Mayor — `.spv` sin verificación de correspondencia** → APLICAR:
   verificados en el cierre — los 3 `.spv` commiteados son idénticos a
   una regeneración fresca con `glslangValidator -V` (sha256).
6. **Menor — Clippy** → APLICAR: ídem pez 1.3.
7. **Menor — Consumo incompleto** → APLICAR: ídem pez 1.2.
8. **Menor — Cobertura borrada (`resolve_scene_ui_rects`)** → REBATIR:
   los 5 tests borrados probaban solo la función borrada; la cobertura
   equivalente vive en `glass_ui` (tests de layout + 23 portados +
   paridad de snapshot 0,0 px + test MCP).
9. **Menor — Fuzz 500 entradas** → ANOTAR: alcance aceptado en el
   análisis revisado por peces; limitación conocida, no un fallo.
10. **Menor — Atribución de clic en otros anidamientos** → ANOTAR: la
    corrección es genérica (burbuja hasta el botón que aporta la
    acción); el caso del hijo de texto está cubierto por test portado.
11. **Menor — GameView «JUGAR»/«Button»** → APLICAR: ídem pez 1.5.

No se usó re-auditoría (queda disponible). Ningún hallazgo se escaló al
usuario (ninguno es una decisión de diseño solo suya).

## 8. Deuda registrada (para frentes posteriores, no bloquea V2)

- **D1 — GameView dibuja «JUGAR» + «Button»**: el overlay del editor
  dibuja la etiqueta del hijo de texto y además el botón con su etiqueta
  de reserva. Idéntico a V1. Frente: faltas de la UI del editor (Lu,
  tras V7 + tools MCP de UI).
- **D2 — Standalone: 3D no pinta, UI deja estela**: preexistente, fuera
  de V2. Frente: Rayo (triado por él, buzon `ff9f5b4`).
- **D3 — CI de GitHub en rojo en P4c/P4d**: diagnóstico abierto (§3).
- **D4 — Consumo P0/P1 sin medir**: deuda de proceso (§6).

## 9. Estado final

P0–P4 en `lu/v2-nucleo` (punta `355dcb2` + cierre clippy). `canvas_ui`
borrado en un único commit revertible. Suite local 1.390/0 (confirmada
tras los arreglos clippy, log `v2_suite_final.log`); clippy RC 0. CI de
GitHub en rojo en los dos últimos commits, diagnóstico abierto. Revisión
de peces hecha (2 peces, 2 bloqueantes = el mismo: el CI). El módulo se
entrega a la verificación única del lado de Análisis con este informe.
