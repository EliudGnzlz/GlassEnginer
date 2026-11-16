# Registro — Defectos del probador, 2ª ronda (2026-11-16)

Siete defectos reportados por el amigo probador (los cuatro últimos
retransmitidos por D3m3tr10 el 2026-11-16 a las 16:24, «desde el
principio»). Protocolo de siempre: causa raíz demostrada, arreglo de
raíz, pipeline local exacto del CI, commit + push, CI real verificado
y verificación física en el editor en vivo.

## 1. La navegación no se mueve como Unity — ARREGLADO, pendiente de la revisión personal de D3m3tr10

Por orden suya, primero el análisis de Opus gesto por gesto
(`viewport.rs` contra Unity real) y el análisis mandó sobre el arreglo.
La equivalencia anunciada en el cierre de los 4 defectos quedó RETIRADA:
la verificación sintética no bastó entonces y no basta ahora — el
veredicto final es su revisión personal con ratón real.

Divergencias corregidas (commit `f69fd18`): órbita y mirada con los dos
ejes invertidos y velocidad 0.005 → signos de Unity (yaw −= dx,
pitch += dy) a 0.003 rad/px, clamp de pitch ±1.54; la rueda sin Alt
hacía DOBLE desplazamiento (distancia + centro por el rayo del cursor)
→ solo hacia el pivote, y Alt+rueda hacia el cursor se conserva; dolly
con Alt+arrastre por el eje dominante (antes solo Y); pan que sigue al
cursor (2·dist·tan(22,5°)/alto del viewport, Shift ×4); vuelo con
velocidad propia `fly_speed` ajustable con la rueda (antes la velocidad
era ∝ distancia y la rueda hacía zoom en vuelo), WASD normalizado,
Q/E por el up de la cámara y sin robar los atajos de herramienta
Q/W/E al volar; el gizmo solo se manipula con el botón primario (un
pan/look que empezaba sobre el gizmo movía el objeto); F encuadra el
centro de los bounds en mundo con distancia según el tamaño (antes: el
origen a distancia fija 8); clic en vacío deselecciona (verificado en
vivo); far del viewport y de la vista Scene 100 → 1000 (el clamp de
distancia era 200 con far 100 y el pivote desaparecía); texto de ayuda
del modo Mano corregido. Punto 7 completado en la integración con la
cuenta de esquinas de `draw_selection_bounding_box`; la fórmula de
`fly_speed` se corrigió en la integración (la de la especificación
podía dar factor negativo y el signo estaba al revés, señalado por el
propio Opus en sus notas).

CI: 43952822149 success.

## 2. Triángulo al quitar el foco de los objetos — ARREGLADO Y VERIFICADO

Reproducido en vivo: al borrar el último objeto con malla (o en
cualquier vista sin mallas) aparecía un triángulo gigante rosa/cian.
Causa raíz en `record_view_pass` (glass_renderer): el «hello triangle»
de respaldo de los primeros hitos se dibujaba siempre que la vista no
tenía mallas (`cmd_draw(3)` con el pipeline del triángulo). Eliminado
el bloque: una vista sin mallas ahora se presenta vacía, como en
Unity. Mismo commit `f69fd18`. Verificado en vivo: escena vacía
(cámara + luz) con la vista Scene limpia, sin triángulo.

## 3. En las demos no sale nada del FPS — ARREGLADO Y VERIFICADO

Verificado desde el zip publicado de v0.10.15.1-beta: los archivos SÍ
viajan (`DemoFpsPolygon.scene.json`, `Fase9Escaparate.scene.json` y los
26 modelos polygon); lo que faltaba era EXPOSICIÓN: la pestaña Ejemplos
del Hub solo ofrecía 4 escenas procedurales. Nueva acción
`LauncherAction::OpenSceneFile` y dos filas nuevas en Ejemplos —
«Demo FPS — POLYGON» y «Escaparate — Shaders y clima» — que cargan los
archivos por la vía probada de `--scene` (`load_from_json` +
bootstrap). Commit `ba94418`, CI 43957359424 success. Verificado en
vivo: la demo FPS se abre desde Ejemplos con su arena completa.

## 4. Pantalla de carga al abrir proyecto (CMD + ventana blanca) — ARREGLADO Y VERIFICADO

Causa raíz: `resumed()` precargaba TODOS los .glb a VRAM antes del
primer frame (en esta VM, ~2,5 min de ventana en blanco), y en Windows
la app abría consola por no declarar el subsistema de ventanas.

- `windows_subsystem = "windows"` (commit `ba94418`): sin CMD.
- Precarga diferida (commit `283f372`, CI 43958813361 success): el
  calentamiento vive en `App::preparar_assets` con el mismo cuerpo; el
  standalone lo llama al arrancar y el editor lo difiere a la primera
  entrada, presentando el frame anterior la pantalla de carga del
  Launcher («GlassEnginer — Cargando el motor… / Preparando modelos y
  materiales»). El Hub aparece en cuanto Vulkan y egui están listos.

Verificado en vivo: Hub en ~14 s tras lanzar (antes ~2 min); al abrir
la demo FPS desde Ejemplos se presenta la pantalla de carga y después
el editor con la demo cargada.

## 5. Un proyecto limpio nuevo no abre — ARREGLADO Y VERIFICADO

El «sistema de proyectos» era una fachada: (a) la lista del Hub estaba
hardcodeada a 1 entrada; (b) `OpenProject(_path)` ignoraba la ruta y
cargaba la demo del plataformero; (c) la creación no guardaba ninguna
escena en disco; (d) la plantilla «Escena vacía» cargaba el Physics
Playground; (e) hallado en la verificación: la Ubicación del Hub ya es
la carpeta del proyecto y `create_project_and_open` unía el nombre
otra vez → `GlassProjects/<nombre>/<nombre>`, y el proyecto nunca
aparecía en la lista.

Arreglo (commit `ba94418`): el Hub escanea `~/GlassProjects` buscando
`project.glass`; `OpenProject` real (campo «scene» de project.glass o
el primer `.scene.json` de `Assets/Scenes`; si no hay, escena vacía);
creación tipo Unity (`Assets/{Scenes,Scripts,Materials,Models,
Textures}`, `ProjectSettings/`, `Packages/`), project.glass con la
escena principal y la escena de la plantilla GUARDADA en
`Assets/Scenes/SampleScene.scene.json`; `setup_empty_scene()` nuevo
(solo cámara y luz solar); ruta de creación corregida (sin doble join).

Verificado en vivo de punta a punta: crear «PruebaVacia» (Escena
vacía) → carpetas y escena en disco con solo Main Camera y Sun → su
tarjeta aparece en el Hub → al reabrirlo carga SU escena, no la demo.

## 6. Arrastrar un .blend congela el motor — NO REPRODUCIDO, pendiente de datos del probador

Investigado por el camino real en el editor en vivo, con Blender
4.2.3 configurado (GLASS_BLENDER): (a) .blend pequeño → conversión en
hilo aparte en ~9 s y recarga en caliente sin congelar; (b) .blend con
armadura y piel → igual, sin bloqueo; (c) .blend de 45 MB / 640k
vértices → conversión ~45 s en segundo plano y recarga ~1 s, el editor
siguió respondiendo (capturas seriadas). La importación por arrastre
(`importar_soltados`) solo copia el archivo; la conversión ya va en un
hilo separado con guarda de una sola exportación por ruta.

Sin reproducción no se aplica ningún arreglo especulativo. Para
cerrarlo hacen falta los datos del probador: tamaño y contenido del
.blend (¿texturas externas, librerías enlazadas?), si tiene Blender
instalado y en qué versión, cuánto tiempo esperó antes de cerrarlo y
con qué build (¿la Release publicada?). Sospechas ordenadas con lo
visto: una recarga en caliente de un .glb muy pesado bloquea el hilo
principal lo que dure la subida a VRAM; varias exportaciones de
Blender a la vez si se sueltan muchos .blend juntos (la guarda es por
ruta, no global).

## 7. Al cerrar el motor se cierra también el Launcher — ARREGLADO Y VERIFICADO

Launcher y editor son el mismo proceso y `CloseRequested` salía sin
más. Ahora, con la feature de editor y en modo Editor (no
standalone), cerrar la ventana vuelve al Launcher — queda vivo en
segundo plano, igual que con el botón «Hub» —; cerrar desde el propio
Launcher sí sale. Commit `ba94418`. Verificado en vivo con un
WM_DELETE_WINDOW real (ClientMessage X11): en el editor → Hub con el
proceso vivo; en el Hub → el proceso sale. (Nota: `xdotool
windowclose` en Xvfb sin WM destruye la ventana sin generar
CloseRequested; no sirve para probar esto.)

## Consumo de tokens del bloque (por proveedor)

- Claude Opus (`claude-opus-5-5`): análisis de navegación 63.453 ·
  arreglo de navegación 59.162 · defectos 3+5 20.812 · defecto 4
  69.693 → **213.120** en total.
- Sonnet, DeepSeek, Kimi: 0 (no se usaron en este bloque).

## Estado al cerrar el registro

- Cerrados y verificados: 2, 3, 4, 5, 7.
- Arreglado y con CI en verde, a la espera del veredicto humano: 1
  (navegación — la revisión personal de D3m3tr10).
- Abierto por falta de datos: 6 (.blend — datos del probador).
- Commits: `f69fd18` (1+2) · `ba94418` (3+5+7+CMD) · `283f372` (4).
  CIs: 43952822149 · 43957359424 · 43958813361, todos success.

## 8. El Launcher no ve las actualizaciones del motor — ARREGLADO Y VERIFICADO (2026-11-16, hallado por D3m3tr10)

Desde una instalación v0.10.14.1-beta, «Buscar actualizaciones» decía
«Estás en la última versión publicada» existiendo v0.10.15.1-beta y
v0.10.16.1-beta. Causa raíz en `updates.rs`: `check_update` pedía
`GET /repos/{repo}/releases/latest`, y el «latest» de GitHub es la
release NO-prerelease más reciente — en este repo, las del motor son
todas prerelease y las del complemento no, así que el endpoint
devolvía `herramienta-3d-v1.0.1`; su tag no parsea como versión del
motor, `is_newer` daba falso y el Launcher se declaraba al día para
siempre.

Arreglo (publicado como **v0.10.16.2-beta**): `fetch_latest` pide la
LISTA (`/releases?per_page=100`) y `select_latest_engine_release`
elige: fuera borradores, solo tags del motor (`is_engine_tag`: 'v' +
dígito + parseable por `tag_version` — los de complementos llevan
nombre y quedan fuera), prereleases incluidas, y la mayor según el
comparador `VersionKey` existente. `ReleaseInfo` gana `draft` y
`prerelease` por serde. `ENGINE_VERSION` sube a 0.10.16.2-beta (la
constante seguía en 0.10.14.1: también había que ponerla al día).
Tests nuevos (6): selección con la mezcla real del repo, borradores
fuera, solo complementos → None, orden VersionKey (sin-prerelease >
prerelease; el core manda), parseo del JSON de la API y is_engine_tag.

Verificación física por el camino real: binario con la identidad de
una instalación vieja (v0.10.14.1-beta) y la selección nueva, contra
la lista real de releases del repo (servida por el gancho
GLASS_UPDATE_API porque el proxy TLS de esta VM corta el TLS directo
de ureq; en máquinas reales la API responde directa, como demuestra
el propio uso del probador): la pestaña Instalaciones ofreció
«⬆ Hay una versión nueva: v0.10.16.1-beta (esta copia es
v0.10.14.1-beta)» con su botón de descarga. Nota de arranque en frío:
las instalaciones anteriores a la .2 tienen el buscador roto y no la
verán — su paso a la .2 es por descarga manual; a partir de la .2
las actualizaciones fluyen solas.

Consumo del defecto 8: Opus 13.897. Acumulado del bloque de
defectos (2ª ronda + defecto 8): **Opus 227.017**.
