# REGISTRO — Bugs reportados por D3m3tr10 (1ª ronda, 2026-11-16)

Lista de 14 puntos entregada por D3m3tr10 el 2026-11-16 tras probar la
v0.10.16.3-beta, ordenada por él de más a menos importante. Cada punto se
cierra con el protocolo de siempre: arreglo de raíz, test físico por el
camino real en la VM, pipeline local exacto del CI, commit, CI de GitHub
en verde y línea de consumo por proveedor.

## B1 — El Launcher no lista los proyectos creados (solo sale 1) — CERRADO

**Reporte:** en la pestaña Proyectos solo salía una tarjeta, que abría el
proyecto anterior; los proyectos que él ya había creado no aparecían.

**Causa raíz (doble, verificada en el código):**
1. `LauncherStore` no tenía ningún registro de proyectos: la lista del Hub
   era la tarjeta del directorio de trabajo (CWD) + un escaneo de
   profundidad 1 de `~/GlassProjects`. `create_project_and_open` escribía
   el proyecto donde dijera el diálogo y no lo anotaba en ningún sitio:
   un proyecto creado en otra ubicación no aparecía jamás, y los de builds
   viejas, en doble carpeta `GlassProjects/<n>/<n>`, tampoco (el escaneo
   no bajaba al segundo nivel).
2. `project_card` ignoraba su parámetro `dir`: TODAS las tarjetas
   disparaban `OpenDefaultScene`. Clicar un proyecto abría siempre el
   proyecto por defecto (el «anterior»), nunca el clicado. Este segundo
   defecto lo destapó el test físico de este cierre, no el reporte.

**Arreglo:**
- `updates.rs`: `ProjectEntry { name, path, last_opened_unix }` y campo
  `projects` en `LauncherStore` (con `serde(default)`: los launcher.json
  instalados siguen cargando) + `register_project()` (upsert por ruta
  canonicalizada, refresca nombre y fecha).
- `launcher.rs`: `descubrir_proyectos()` une tarjeta del CWD + registro
  (más recientes primero, solo si el proyecto sigue en disco) + escaneo
  de la carpeta base con adopción del heredado de doble carpeta; sin
  duplicados (el CWD gana). `render_projects_tab` la usa.
- `launcher.rs`: la tarjeta dispara `OpenProject(dir)`; solo la del CWD
  (proyecto del motor) sigue con `OpenDefaultScene`.
- `main.rs`: `create_project_and_open` y el brazo `OpenProject` registran
  el proyecto en el almacén y lo guardan.
- Tests nuevos: `registrar_proyecto_no_duplica_y_actualiza`,
  `registrar_dos_proyectos_distintos`,
  `descubre_registro_escaneo_y_legado_sin_duplicados`.

**Test físico (VM, HOME aislado, camino real completo):** el Hub mostró
GlassEnginer + Legado (doble carpeta, adoptado) + ProjScan (escaneo);
creado «ProyectoFuera» con el diálogo en una ubicación FUERA de
GlassProjects → quedó registrado en launcher.json y su tarjeta apareció
en el Hub tras volver y tras reiniciar la app; al clicar su tarjeta se
abrió SU escena (`Proyecto abierto: ... cargada: true`) y la fecha del
registro se refrescó.

**Consumo:** Opus 23.781 (paquete de 9 pares). El cableado de la tarjeta
(`OpenProject(dir)`) lo escribió Rayo al destaparlo su test físico.

## B2 — Cerrar no pide guardar + falta autoguardado — CERRADO

**Reporte:** al cerrar el motor no se pide guardar el proyecto; falta el
autoguardado cada 5 min por defecto, modificable en la configuración.

**Causa raíz:** no existía ni el concepto de «escena sucia» ni el de «archivo
de la escena abierta»: nadie comparaba la escena en memoria con lo guardado
y la app no recordaba qué archivo había abierto, así que no había nada que
interceptar al cerrar ni dónde autoguardar.

**Arreglo:**
- Suciedad por contenido: `EditorDockingLayout.hash_guardado` contra
  `undo::scene_hash` (deshacer hasta el estado guardado vuelve a «limpio»
  solo). Se refresca al guardar (Ctrl+S / Save As / guardados de la app), al
  cargar (vigía de escena + marcado directo al abrir, que cubre el caso
  mismo-nombre) y al descartar con «No guardar».
- La app recuerda la ruta de la escena (`ruta_escena_actual`, fijada en
  open_scene_file y al crear proyecto; invalidada si el nombre activo deja
  de coincidir — defensa comprobada también contra New Scene, que ahora
  pasa a llamarse «Untitled» como en Unity).
- Diálogo «⚠ Cambios sin guardar» (Guardar / No guardar / Cancelar) al
  pulsar Hub y al cerrar la ventana en el editor; sin ruta, «Guardar» pasa
  por el «Save Scene As» existente y la vuelta al Hub se retoma al
  cerrarlo. Sin cambios, el cierre va directo, como antes.
- Autoguardado: cada `autosave_minutes` (LauncherStore, defecto 5,
  0 = desactivado) si la escena está sucia y tiene archivo; configurable en
  Edit → «⚙ Preferencias...» (persistencia con cargar-modificar-guardar
  fresco para no pisar el registro de proyectos).

**Test físico (VM, HOME aislado, proyecto ProyectoB2 con escena de 3
entidades y autoguardado sembrado a 1 min):** crear una entidad disparó el
autoguardado (log y 4ª entidad escrita en disco); Hub con cambios abrió el
diálogo; Cancelar se quedó en el editor; Guardar volvió al Hub y escribió
la 5ª entidad (sin línea extra de autoguardado: fue el diálogo); cierre de
ventana limpio volvió directo; cierre con cambios quedó interceptado
(«pidiendo decisión») y No guardar descartó (el archivo se quedó en 5);
Preferencias mostró el 1 min sembrado y arrastrar a 0 lo persistió en
launcher.json.

**Consumo:** Opus 59.284 (paquete de 21 pares; el par del test lo añadió
Rayo a mano porque Opus no tenía el final del archivo, y dos `.show(ctx)`
se corrigieron a `ui.ctx()` al integrar).

## B3 — Sin drag & drop del panel de proyecto a la escena/jerarquía — CERRADO

**Reporte:** no se puede arrastrar nada —ni .blend ni .fbx— del panel de
proyecto a la escena ni a la jerarquía.

**Causa raíz:** el panel de proyecto no tenía ningún gesto de arrastre: las
filas solo respondían a clic, doble clic y menú contextual, y ni el viewport
ni la jerarquía aceptaban carga alguna.

**Arreglo:**
- Carga útil `ArrastreAsset { ruta }` (DragAndDrop de egui) e
  `instanciar_asset(state, ruta, pos)` compartido: prefab →
  `load_from_file` + `instantiate_prefab`; modelo (.glb/.gltf/.blend/.fbx)
  → entidad con MeshRenderer cuyo `model` es el stem, que es la clave del
  AssetCache (el .blend/.fbx llega ya convertido a .glb hermano por el
  AssetWatcher, o precargado si vive en `assets/models`).
- Destino viewport (solo en modo Edit): borde verde + «Soltar para
  instanciar: <nombre>»; al soltar, instancia en el punto del plano y=0 bajo
  el cursor (`screen_to_world_on_plane`) y selecciona la entidad.
- Destino jerarquía: resalte verde si el puntero está sobre el panel con
  carga; al soltar, instancia en el origen y selecciona. Sin conflicto con
  el DnD interno de reparentado, que no usa cargas de egui.
- Origen: **la primera versión usaba `ui.dnd_drag_source` y el test físico
  cazó una regresión grave: su interact interno con Sense::drag() se queda
  con la pulsación y las filas arrastrables dejaban de responder a clic,
  doble clic y menú contextual.** Rehecho observando el puntero sobre la
  propia etiqueta, que conserva intacto su comportamiento: el arrastre se
  detecta con `press_origin` dentro del rect de la fila +
  `is_decidedly_dragging()` — `is_pointer_button_down_on` tampoco sirve
  aquí, porque la interacción de arrastre del ScrollArea se queda con los
  ids potenciales de clic/arrastre (diagnóstico en vivo con log temporal:
  down_on=false en todas las filas, origen y decidido correctos) — y la
  carga se publica cada fotograma con su fantasma junto al cursor; el
  plugin DragAndDrop de egui la limpia al soltar.
- Tests nuevos: qué es instanciable, instanciar un modelo crea la entidad
  con su stem, instanciar un prefab desde archivo.

**Test físico (VM, proyecto desechable, assets del propio repo):**
arrastrar `cube.blend` al viewport creó la entidad en el punto del plano
bajo el cursor ([3.89, 0, −6.25], renderizada y seleccionada);
`Gold_Coin.prefab.json` al viewport ([3.77, 0, −5.58], conservando su
rotación, escala y componentes) y a la jerarquía (en el origen); fantasma
y borde verde visibles durante el arrastre. Regresión verificada tras
rehacer el origen: el clic selecciona la fila, el doble clic en un prefab
instancia por la vía antigua y el menú contextual abre en las filas
arrastrables.

**Consumo:** Opus 23.881 (paquete de 6 pares). La corrección del origen
del arrastre (de `dnd_drag_source` a observación del puntero con
`press_origin`) la escribió Rayo directamente tras cazarla el test físico.

## B4-6 — Picking y colliders: plano no seleccionable, collider del plano enorme, enemigo no seleccionable — CERRADO

**Reportes:** (B4) el plano no se puede seleccionar clicando en la escena ·
(B5) el «collider» del plano se ve enorme, mucho más grande que el plano ·
(B6) el enemigo de la demo FPS no se selecciona al clicarlo.

**Causa raíz única (picking y caja de selección):** `pick_entity_at_screen` y
`draw_selection_bounding_box` (gizmo_3d.rs) suponían que toda malla es un cubo
unidad centrado en el origen, con medias extensiones = escala × 0,5, y DESPUÉS
transformaban esas esquinas por la matriz de mundo completa: la escala se
aplicaba DOS VECES y se ignoraba la geometría real. Plano a escala [10,1,10] →
caja de picking de 100×100 m imposible de clicar en su superficie (B4) y caja
de selección dibujada 10× más grande (B5: el «contorno enorme» de la captura
era la caja de selección). Enemigo xbot (escala 1): la caja supuesta era un
cubo de 1 m en los pies, pero el modelo mide ~1,8 m con el origen en los pies
→ el torso y la cabeza quedaban fuera (B6). El picking además era una prueba
de AABB en pantalla con desempate por distancia del CENTRO, no por superficie.

**Causa raíz adicional (lectura literal de B5):** el preset «Plano / Suelo»
de la jerarquía no creaba física alguna: `EntityData::new` solo materializa
`PhysicsData` si los componentes incluyen "RigidBody", y el preset trae
[Transform, MeshRenderer, StaticCollider] → el suelo no colisionaba con nada.

**Hallazgo al integrar (raíz segunda):** el primer `screen_ray` de Opus
reutilizaba `mat4_inverse_affine` de glass_core, que solo invierte la parte
afín 3×3 — no invierte la perspectiva de un view-projection — y
`mat4_transform_point` no divide por w. Los tests de picking lo cazaron
(fallaban torso y vacío; centro y plano pasaban por simetría). Arreglo de
raíz: inversa general 4×4 por adjunta y desproyección homogénea dentro de
gizmo_3d, con la Y en la convención de `world_to_screen` (la proyección
invierte Y para Vulkan) y NDC z ∈ [0,1]; test de invariante nuevo
(proyectar → lanzar el rayo → el rayo pasa por el punto). De propina, la
misma corrección arregla la precisión de `screen_to_world_on_plane`
(menú Crear y drops de B3), que usaba la misma inversa afín.

**Arreglo:**
- `AssetCache::bounds_snapshot()`; el panel de viewport guarda las cotas
  locales reales de cada malla (la app las refresca desde la caché cada
  frame antes de renderizar el layout).
- Picking nuevo: pasada de iconos de cámara/luz (≤24 px, prioridad absoluta,
  como antes) y después rayo contra la AABB de mundo de las cotas REALES de
  cada malla; gana el impacto más cercano al rayo. La caja de selección
  dibuja las esquinas de esas mismas cotas. Entidad sin malla conocida →
  cubo unidad de respaldo; `model == ""` → cotas de "cube" (el respaldo del
  render).
- Preset Plano: materializa `PhysicsData` estática con `box_extents`
  [5, 0.5, 5] y un collider explícito con `offset [0, −0.5, 0]`, para que la
  cara superior del collider quede en y = 0, la superficie visible (sin el
  desplazamiento los objetos reposarían flotando 0,5 m).
- 6 tests nuevos en gizmo_3d (plano escalado clicable en su superficie,
  modelo con origen en los pies clicable en el torso, gana el más cercano
  al rayo, clic en vacío, invariante del rayo, cotas reales).

**Test físico (VM, escena de prueba con el Plane del preset y el Enemigo
clonado de DemoFpsPolygon):** clic en la superficie del plano lejos del
centro → selecciona Plane (en la reproducción previa NO seleccionaba en
ningún punto); la caja naranja abraza el quad 10×10 exacto; clic en el
torso del enemigo → selecciona Enemigo y la caja envuelve la figura entera;
clic en el cielo → deselecciona; el log confirma «Objeto seleccionado en
Scene View por Raycasting: #1 / #2». Preset nuevo creado en vivo: el
Inspector muestra RigidBody Static con half-extents 5,00 / 0,50 / 5,00.
Regresión B3: arrastrar Gold_Coin al viewport cae en [4.41, 0, 1.36], el
punto del suelo bajo el cursor (con la matemática vieja habría caído
reflejado al otro lado).

**Consumo:** Opus 48.673 (paquete de 14 pares). Las correcciones de
integración (campo de cotas en el panel en vez de parámetro, por los dos
llamantes internos; inversa general y test de invariante; offset del
collider del preset) las escribió Rayo directamente.

## AUDITORÍA DE ACTUALIZACIONES (orden de D3m3tr10, 2026-11-16) — SIN APLICACIÓN AUTOMÁTICA

Regla suya: nada de actualizaciones automáticas; la release se ofrece y
solo se descarga/instala con su acción explícita. Auditoría del código de
red completo (updates.rs, plugins.rs, launcher.rs y llamantes en la app):

- Comprobar actualizaciones del motor: SOLO el botón «⬇ Buscar
  actualizaciones» de Instalaciones.
- Instalar una versión: SOLO el botón «⬇ Descargar e instalar {versión}».
- Catálogo de complementos: SOLO el botón «🔄 Buscar complementos».
- Instalar/actualizar un complemento: SOLO sus botones «⬇ Instalar» /
  «⬆ Actualizar a v…».
- Al arrancar no se lanza ningún hilo de red; no hay temporizadores ni
  polling; la pestaña Comunidad es texto estático; el editor no hace
  llamadas de red (AssetWatcher = archivos locales, MCP = stdio local).

**Veredicto: no existe ningún canal que descargue o instale sin su clic;
no hubo nada que eliminar.**

### Auditoría afinada, canal por canal (corrección suya del 2026-11-16 21:59)

Su precisión: los cambios aparecieron en la versión nueva DESPUÉS de
instalarla, no venían «acumulados» en el paquete; su lista de 14 salió de
ir usándola. Se re-auditó buscando TODO canal por el que la copia
instalada pueda cambiar sin un clic suyo, incluidos los datos remotos en
vivo. Barrido global: el ÚNICO código de red de todo el producto está en
`updates.rs` y `plugins.rs` (ni el C# ni el resto de crates abren red).

**Canales remotos (la petición nace SOLO de su clic; el contenido es el
del repo en ese instante):**
1. **Lista de releases de GitHub** → la oferta de actualización de
   Instalaciones. Solo se pide con «⬇ Buscar actualizaciones». Lo que se
   ofrece depende del estado del repo al clicar: publicar una release o
   marcarla «Latest» cambia la próxima oferta sin tocar su copia.
2. **Catálogo de complementos**, leído en vivo de
   `raw.githubusercontent.com/{repo}/main/plugins/catalog.json` al pulsar
   «🔄 Buscar complementos». Su contenido es el de la rama main en ese
   momento: así apareció «MeshForge» v1.0.1 en su copia instalada cuando
   se publicó el renombrado, sin que su copia cambiara.
3. **Las descargas** (paquete del motor y de cada complemento): solo tras
   su clic de instalar, con SHA-256 verificado antes de extraer.

Las ofertas y el catálogo viven SOLO en memoria durante la sesión: el
store persistido (`launcher.json`) guarda repo, instalaciones, proyectos
y el intervalo de autoguardado — ninguna oferta ni catálogo se guarda ni
se muestra al arrancar. Al abrir el Launcher no se lanza ningún hilo de
red, no hay temporizadores ni polling, y ningún chequeo de arranque muta
el store ni la vista (los únicos guardados del store nacen de sus clics:
instalar, crear/abrir proyecto, «Guardar» del repo).

**Canales locales sin clic en el momento (sin red):**
4. El Hub re-escanea el disco en cada frame: un proyecto que aparezca en
   `~/GlassProjects` o en una ruta registrada se lista solo.
5. El AssetWatcher del editor convierte .blend/.fbx a .glb y recarga
   mallas/shaders en caliente cuando un archivo cambia en el disco.
6. El autoguardado del editor (B2, pedido por él): escribe su escena cada
   N minutos solo si hay cambios sin guardar.
7. La caché de render expulsa modelos ociosos de VRAM (interno, sin
   cambio de contenido).

**Conclusión frente a su regla** («la release lista solo se ofrece; nada
se aplica ni cambia sin mi acción»): nada se APLICA sin su clic en ningún
canal, y en reposo su copia solo muestra estado local. Lo que «fue
apareciendo» con el tiempo se explica por (a) comportamientos de la
build .3 que solo se ejercen al usarla — su lista de 14 salió de ahí — y
(b) el contenido en vivo de los canales 1 y 2, que cambió porque el repo
cambió (releases publicadas/marcadas y el catálogo del renombrado)
mientras su copia seguía intacta. La explicación anterior de este
registro («la .3 traía todo acumulado») queda corregida por esta sección.
