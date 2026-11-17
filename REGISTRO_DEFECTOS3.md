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

## B7-8+10 — Gizmo de cámara «desorientado», anillos de rotación en un plano, selección blanca y líneas −25% — CERRADO

**Reportes**: (7) el frustum de la cámara se dibuja hacia otro lado (ya
reportado antes y seguía igual); (8) los tres anillos de rotación están
en el mismo plano; (10) la selección debe ser blanca y sus líneas un
25% más finas.

**B7 — causa raíz demostrada (el gizmo NO era el culpable)**: `draw_camera_gizmo`
extrae right/up/back de las columnas de la matriz de mundo, la misma
convención que `camera_view_matrix` (fwd = −columna 2); verificado
numéricamente con `mat4_from_trs_euler` y en vivo (una cámara con yaw
+90° dibuja su frustum hacia −X, que ES el forward del motor: los signos
Euler del motor son el espejo de Unity en pitch y en yaw). Lo que sí
estaba sin migrar eran las DOS funciones que crean la escena por
defecto/plantilla: `setup_empty_scene` y `setup_audio_lighting_scene`
conservaban las rotaciones de ANTES de la migración de signos
documentada en `camera_view_matrix` — cámara `[14, 0, 0]` (con la
convención actual mira ARRIBA 14°) y sol `[55, −40, 0]` (forward
(0.37, +0.82, −0.44): el sol apuntaba hacia arriba, luz desde abajo).
Demostrado en vivo con la configuración exacta de plantilla: la vista
de la cámara mostraba un cubo situado ARRIBA centrado y el suelo
reducido a una franja inferior. Todas las escenas .scene.json del repo
ya estaban migradas; solo quedaban las plantillas en código.
**Arreglo**: en ambos sitios, cámara → `[−14, 0, 0]` y sol →
`[−55, 40, 0]` (el sol migrado da forward (−0.37, −0.82, −0.44), hacia
abajo). Notas laterales NO incluidas en este sub-bloque: el PiP
«Camera Preview» pinta la textura de la cámara principal con el nombre
de la seleccionada; la tapa lejana del frustum se clampa a [10, 25] m
(geométricamente correcta, visualmente grande).

**B8 — causa raíz**: en `render_3d_transform_gizmo` los «anillos» eran
tres círculos 2D de pantalla concéntricos (radios 0.85/1.0/1.15 × 45 px)
con hit-test por bandas de distancia al centro (38/45/52 px).
**Arreglo**: anillos 3D reales — helper `anillo_rotacion_puntos` que
genera cada anillo en el plano perpendicular a su eje (X→YZ, Y→XZ,
Z→XY), radio de mundo calculado para ≈45 px de pantalla (mediana de
px/unidad de las puntas de eje), 64 segmentos proyectados con
`world_to_screen`; hit-test por distancia mínima a las polilíneas
(<7 px). El arrastre de rotación no depende de la geometría dibujada
(sigue usando `active_drag_axis` + delta de ratón, sin cambios).

**B10 — arreglo** en `draw_selection_bounding_box`: línea principal
naranja 2.0 → **blanca 1.5**; halo 3.5 → 2.6; esquinas doradas radio
3.0 → **blancas radio 2.25** (borde 1.0 → 0.75); borde de la etiqueta
naranja → blanco.

**Paquete**: Opus 21.065 tokens (12 pares; correcciones de integración
de Rayo: nombres reales de parámetros `vp`/`origin_3d` en el par de
polilíneas, ruta completa de `GizmoMode`, y el helper recolocado antes
del doc-comment de la función). Tests: `cargo test -p glass_editor`
**91/91** (88 + 3 nuevos de anillos: plano YZ del anillo X,
perpendicularidad entre anillos, nº de segmentos).

**Test físico PASS (VM, binario de editor reconstruido)**:
- B10: suelo y plataforma seleccionados muestran la caja **blanca** y
  fina, sin rastro de naranja.
- B8: con una plataforma en modo Rotar (E), los tres anillos se ven
  como elipses en **tres planos distintos** que se cruzan en 3D
  (verde horizontal, rojo vertical de canto, azul de frente); el hover
  sobre el anillo X lo pinta **amarillo y grueso** con la etiqueta
  «Rotar X (Pitch)»; el arrastre sobre ese anillo llevó la Rotación X
  del Inspector de 0.0° a **57.6°**.
- B7: proyecto sin escena (cae a `setup_empty_scene`, log «cargada:
  false») → Main Camera con **Rotación X = −14.0°** en el Inspector y
  Sun con **Rotación [−55.0°, 40.0°]**, con las flechas del gizmo de
  luz apuntando hacia abajo.

**Consumo B7-8+10: Opus 21.065** (las correcciones de integración y el
test físico, de Rayo: 0 tokens de API).

## B9 — Estructura tipo Unity y crear con clic derecho — CERRADO

**Reporte**: al abrir su proyecto, el panel Proyecto mostraba las
carpetas y archivos del motor mezclados, no los assets del proyecto; y
con clic derecho no se podía crear nada (pidió «verifica»).

**Verificación del reporte (era cierto en lo esencial)**:
- El menú contextual de cada elemento solo tenía Renombrar / Mover a… /
  Eliminar: **ninguna creación con clic derecho**. Sí existía una barra
  superior de creación (Carpeta/Script/Grafo/Escena con campo de
  nombre), pero no es la vía que un usuario de Unity busca, y **no
  incluía Material** pese a que el motor sí tiene assets de material
  (`MaterialData` en `assets/materials/<nombre>.material.json`).
- El panel estaba enraizado SIEMPRE en la carpeta `assets` del CWD del
  proceso: nada lo re-enraizaba al abrir un proyecto, así que mostraba
  los assets de la instalación (los del motor), no los del proyecto
  abierto. La estructura de carpetas tipo Unity al crear el proyecto
  (Assets/Scenes, Scripts, Materials, Models, Textures) ya existía
  desde B1/B2; lo que faltaba era que el panel la mirara.

**Arreglo**:
- `ProjectPanel::set_assets_root(raiz)`: re-enraíza el panel (crea la
  carpeta si falta, vuelve a la raíz y refresca). La App lo llama al
  abrir un proyecto (`OpenProject`, respetando la convención que exista
  en disco: `Assets` o `assets`) y al crear uno nuevo (su `Assets`).
  El proyecto del CWD (el del motor) no cambia: sigue en `assets`.
- Submenú **«➕ Crear»** en el menú contextual de cada elemento Y en el
  del fondo del panel (clic derecho en hueco vacío, registrado antes
  que los elementos para no robarles su menú): Carpeta, Escena,
  Material, Grafo de shaders y Script C#. El destino de Carpeta/Escena
  es la carpeta clicada (o la visible si se clicó un archivo o el
  fondo); Material/Grafo/Script van a su carpeta canónica. El nombre
  sale del campo «Nombre:» si hay algo escrito; si no, nombre por
  defecto del tipo (NuevaCarpeta, NuevaEscena, NuevoMaterial…).
- Tipo **Material** nuevo en todo el flujo: `crear_material` escribe un
  `MaterialData::default()` serializado en la carpeta de materiales, la
  barra de creación gana el botón «🖌 Material» y el icono 🖌 para
  `.material.json`.
- `subcarpeta(root, minúscula, Mayúscula)`: las creaciones de escenas,
  materiales y grafos respetan la convención del proyecto abierto
  (`Scenes`/`Materials` si existen; si no, minúsculas como en el repo).
- `crear_escena_en(dir, nombre)`: la escena contextual se crea en la
  carpeta pedida; `crear_escena` delega en ella.

**Límite honesto (adyacente, fuera de B9)**: la RESOLUCIÓN de contenido
de assets (materiales por nombre en el Inspector/renderer, precarga de
modelos, AssetWatcher) sigue resolviendo contra el `assets` del CWD;
el panel (navegación, creación, apertura de escenas y prefabs por ruta)
ya sigue al proyecto abierto. Hacer todo el motor relativo al proyecto
es un cambio mayor que no es lo que pedía este punto.

**Paquete**: Opus 39.441 tokens (14 pares; corrección de integración de
Rayo: el destino contextual usa la ruta y el tipo reales de la entrada
en vez de reconstruirlos por nombre). Tests: `cargo test -p
glass_editor` **94/94** (91 + 3 nuevos: material escribe JSON válido y
relee como MaterialData, subcarpeta respeta la convención,
set_assets_root re-enraíza y refresca).

**Test físico PASS (VM, proyecto de usuario con estructura de
plantilla)**: al abrirlo, el panel muestra SOLO sus carpetas
(Materials, Models, Scenes, Scripts, Textures), sin un solo archivo
del motor; clic derecho en el fondo → Crear → Carpeta crea
`NuevaCarpeta` en su Assets (verificado en disco); clic derecho sobre
Materials → Crear → Material crea `Materials/NuevoMaterial.material.json`
con los 9 campos de MaterialData (JSON validado); clic derecho sobre
la carpeta Scenes → Crear → Escena crea `Scenes/NuevaEscena.scene.json`
DENTRO de Scenes (plantilla válida).

**Consumo B9: Opus 39.441** (la corrección del destino contextual y el
test físico, de Rayo: 0 tokens de API). *(Cifra rectificada el
2026-11-17 contra `resp_b9_proyecto.json`: el cierre anunció 48.064
por un error de lectura; el `usage` del archivo es prompt 30.011 +
completion 9.430 = 39.441, de la única llamada del paquete.)*

## B11-13 — Demo FPS: mouse invertido, sin HUD de balas/arma, impacto antes que el proyectil — CERRADO

**B11 — Causa raíz**: `JugadorPersonaje.ProcesarRaton()` sumaba el
delta vertical de `Input.mousePosition` (cuya Y crece hacia abajo) al
pitch que crece hacia arriba: ratón arriba → mirar abajo. El yaw era
correcto. Confirmado físicamente con la build defectuosa (ratón abajo
+27 px → la vista subió). **Arreglo**: `_pitch - ddy * Sensibilidad`
en `JugadorPersonaje.cs` y en el gemelo `JugadorFps.cs` (mismo modo de
fallo en la demo de la Fase 10).

**B12 — Causa raíz**: el estado ya existía (`ArmaActualNombre`,
`MunicionEnCargador`) pero nada lo presentaba. **Arreglo**: script
nuevo `HudArmas.cs` + entidades 230 «HudArma» (Text anclado abajo a la
derecha del Canvas existente) y 231 «Hud» en `DemoFpsPolygon`;
getter nuevo `CargadorActual` en `JugadorPersonaje`. El texto solo se
reasigna cuando cambia.

**B13 — Causa raíz DOBLE**: (1) `ProyectilF10.OnCollisionEnter`
ignoraba el punto de contacto real (`Collision.point`, que llega con
el evento durante el paso de física) y registraba el impacto con la
posición del transform del proyectil, que en ese instante aún es la
del frame anterior (hasta ~4,3 m por detrás a 260 m/s): el número de
daño aparecía en el aire, antes de que el proyectil llegara al blanco.
(2) `Lanzar` ponía `Scale = (1,1,1)` descartando la escala autorada
0,12: el proyectil se veía como una bola de ~3 m con un collider de
0,06. **Arreglo**: registrar con `c.point` (con la posición propia
como respaldo si el punto viene a cero) y capturar/restaurar la
escala autorada, a la par del patrón ya correcto de `ProyectilRpg`.

**Paquete**: Opus 12.001 tokens (7 pares + `HudArmas.cs` nuevo;
integración 7/7; la edición de escena —entidades 230/231— la hizo
Rayo con round-trip exacto del JSON). `cargo test -p glass_core
--test scene_assets` 3/3; build gestionado 0 errores.

**Test físico PASS (standalone, build de juego + DLL nueva)**:
- B12: el HUD muestra «Pistola 9mm 12 / 12» y descuenta en vivo
  (11 / 12, 10 / 12) tras cada disparo.
- B11: ratón arriba → la vista sube (contrario al comportamiento con
  el defecto).
- B13: el proyectil vuela como bolita a su escala autorada; dos
  impactos registrados en el Enemigo (`ENEMIGO impacto dano=10
  vida=90`, luego `vida=80`, con su retardo de vuelo, no instantáneo)
  y el número de daño «10» aparece SOBRE el enemigo, a la altura del
  torso, con la barra de vida reducida en la captura.
- **Nota de diagnóstico (falsa alarma resuelta)**: una primera
  medición a ojo de las capturas reducidas sugirió que los disparos
  pasaban ~90 px por encima del enemigo. La medición con
  `visual_grounding` y una sonda temporal de ida y vuelta por el
  puente de transforms demostraron que la cadena completa (ratón →
  pitch → cuaternión almacenado → cámara renderizada → dirección de
  disparo) es exacta: a pitch −2° el enemigo aparece a 375 px
  (predicho 374) y la respuesta de la cámara es a tasa completa
  (11,9 px/° medidos). El error era de la medición a ojo, no del
  motor; la sonda se retiró antes del commit.

**Consumo B11-13: Opus 12.001** (la edición de escena, el diagnóstico
de la falsa alarma y el test físico, de Rayo: 0 tokens de API).

## B14 — Frametime dentro del editor como ventana propia — CERRADO

**Reporte**: el Frametime era un overlay flotante dibujado desde
main.rs por fuera del editor, siempre visible y sin forma de
quitarlo; debía ser una ventana propia del editor, mucho más pequeña
y solo con datos relevantes, ocultable y recuperable desde el menú
de ventanas.

**Arreglo**: el overlay se elimina de main.rs (−49 líneas) y el
Frametime pasa a ser una pestaña más del dock (`EditorTab::Frametime`,
título «⏱ Frametime»), oculta por defecto. El menú Window gana la
casilla «⏱ Frametime» que la muestra/oculta
(`set_frametime_visible`, idempotente: añade con
`push_to_focused_leaf`, quita con `find_tab` + `remove_tab`);
`apply_preset` la repone si estaba visible, porque reconstruye el
dock. Contenido compacto (6 líneas, sin p95 ni Present): `FPS ·
Total ms`, Update, Physics, Render, GPU y la línea de memoria
`RSS · GPU a/b MiB`, leídos del mismo registro de profiling que
usaba el overlay.

**Paquete**: Opus 16.472 tokens (9 pares, todos en docking.rs;
integración 9/9; el borrado del overlay en main.rs lo hizo Rayo).
Tests: `cargo test -p glass_editor` **96/96** (94 + 2 nuevos:
mostrar/ocultar sin duplicar y supervivencia a apply_preset).

**Test físico PASS (editor en vivo, SampleScene)**: al entrar no hay
overlay ni pestaña de Frametime; Window → «⏱ Frametime» abre la
pestaña con datos reales de esta VM (`FPS 4 · Total 248.47 ms`,
Render 161.48, GPU 114.60, `RSS 644.9 MiB · GPU 160/256 MiB`) y los
valores se refrescan en vivo (Total 248.47 → 245.44 s segundos
después); desmarcar la casilla quita la pestaña del dock.

**Consumo B14: Opus 16.472** (el borrado del overlay y el test
físico, de Rayo: 0 tokens de API).

---

**BLOQUE DE 14 PUNTOS COMPLETO** (B1, B2, B3, B4-6, B7-8+10, B9,
B11-13, B14 cerrados; la auditoría de actualizaciones, conforme sin
cambios). Quedan fuera del bloque, como ya estaban: el veredicto
personal de D3m3tr10 sobre la navegación (defecto 1 del probador) y
los datos del probador para el .blend que congela (defecto 6).
Consumo total del bloque en Opus: 23.781 (B1) + 59.284 (B2) + 23.881
(B3) + 48.673 (B4-6) + 21.065 (B7-8+10) + 39.441 (B9) + 12.001
(B11-13) + 16.472 (B14) = **244.598 tokens**.

---

## DEFECTO DEL DOCK INFERIOR — las pestañas «abrían el panel equivocado» (2026-11-17)

**Reporte** (pasada física de MeshForge, sobre `32bdd81`): las
etiquetas del dock inferior no coincidían con sus zonas clicables —
«Proyecto / Assets» (centro) abría Grafo de Shaders, «Grafo de
Shaders» abría MeshForge y «MeshForge» no hacía nada. Hipótesis
sugerida: una pestaña invisible (la Frametime oculta de B14)
ocupando un hueco de hit-test tras «Consola».

**Investigación (la hipótesis NO se confirmó)**: la construcción del
DockState está limpia (ningún preset mete pestañas extra; Frametime
solo entra con `show_frametime`) y en egui_dock 0.21.1 cada pestaña
se dibuja y se clica con el MISMO widget (`tabs()` en
`widgets/dock_area/show/leaf.rs`: `response.clicked()` →
`leaf.active = tab_index`), así que no existe vía de código para un
desfase etiqueta↔zona. Verificación física previa al arreglo, con
las etiquetas medidas por grounding y clic en cada centro: **las
cuatro pestañas abrían su panel** en los tres estados (sin
Frametime, con Frametime en la hoja inferior y con Frametime en la
hoja central).

**Causa raíz confirmada en código y medida en vivo**: egui 0.36.2
trae `InputOptions::max_click_duration = 0,8 s` por defecto
(`input_state/mod.rs:114`): si una pulsación dura más de 0,8 s,
`could_any_button_be_click()` pasa a false y la pulsación **deja de
ser un clic**. En las pestañas del dock (sentido `click_and_drag`)
la pulsación larga se convierte en **arrastre de pestaña**: no
activa nada y, si el puntero se mueve, **reordena las pestañas en
silencio**. Medido en la VM (~4-5 fps): pulsación de 0,05/0,3/0,7 s
→ la pestaña se activa; 0,9/1,0/2,0 s → no se activa (la frontera
cae exactamente en los 0,8 s de la constante); un arrastre de 68 px
llevó «Proyecto» del 2º al último puesto sin ningún aviso. La
pasada que reportó el defecto clicaba con pulsaciones largas
(patrón de agente a bajo fps), así que sus clics o no existían o
arrastraban pestañas entre medición y medición: de ahí el mapa
«desplazado» observado. En un equipo lento, una pulsación
deliberada de un usuario real puede superar igualmente los 0,8 s.

**Arreglo de raíz** (main.rs, al crear el contexto egui):
`input_options.max_click_duration = f64::INFINITY` — los clics no
caducan por duración; el arrastre sigue naciendo solo del
movimiento (`max_click_dist`, intacto), así que arrastrar
pestañas para reordenar sigue funcionando igual que en Unity.

**Test físico PASS tras el arreglo** (editor en vivo, HEAD con el
arreglo, MeshForge activo): estado 1 sin Frametime — barrido
rápido Consola/Proyecto/Grafo/MeshForge, cada una abre su panel, y
pulsaciones lentas de 2 s (Grafo) y 1,5 s (MeshForge) activan su
pestaña (antes: nada). Estado 2 con Frametime visible desde
Window — barrido correcto y pulsación lenta de 2 s en Proyecto
activa Proyecto. Estado 3 con Frametime oculta de nuevo — la
pestaña desaparece de su hoja, barrido correcto y pulsación lenta
de 2 s en MeshForge activa MeshForge.

**Consumo: Opus 0** (investigación y arreglo mecánico de una línea,
de Rayo).

## NOTA MENOR REGISTRADA (sin arreglar, 2026-11-17)

Tras responder «Sí, adaptar» a un mesh collider en MeshForge, el
Inspector muestra «⚠ Ese modelo no está en assets/: al dibujar se
usará el cubo» aunque el .glb horneado está en `assets/models/`, el
AssetWatcher lo registra en el AssetCache y el viewport dibuja la
forma correcta. La comprobación del Inspector usa otra lista que
no incluye el modelo recién horneado. Queda registrada a la espera
de que D3m3tr10 decida si entra en un bloque.

## B7 REABIERTO — el frustum seguía torcido (2026-11-17)

**Reporte de D3m3tr10** probando la v0.10.17.1-beta, con captura: con la Main
Camera seleccionada, el frustum (líneas cian) salía deformado, con el
rectángulo lejano disparado hacia la esquina superior derecha, fuera de la
vista de escena, y apuntando a un lado distinto del que muestra la Camera
Preview. La corrección de plantillas de B7 no era la causa: el fallo estaba
en el dibujo del frustum mismo. **Tenía razón en todo.**

**Causa raíz (confirmada numéricamente, no a ojo):** en `draw_camera_gizmo`
(gizmo_3d.rs), las esquinas del frustum se calculaban a mano, componente a
componente, y la **componente Y de las dos esquinas derechas del plano
lejano llevaba los índices cambiados**: `f_tr` usaba `right[0]` en vez de
`right[1]`, y `f_br` usaba `right[0]` y `up[0]` en vez de `right[1]` y
`up[1]`. Con la cámara mirando a −Z (`right = (1,0,0)`), la Y de esas
esquinas recibía `+w_far` (≈ +25,7 m con fov 60 y el tope de 25 m): el
rectángulo lejano se levantaba por la derecha y la pirámide entera se leía
torcida hacia arriba a la derecha. Las demás componentes y las esquinas del
plano cercano estaban bien; por eso el frustum «casi» se reconocía.

**Diagnóstico:** sonda temporal de diagnóstico dentro de `draw_camera_gizmo`
que imprimía base, esquinas proyectadas y VP; las esquinas izquierdas
coincidían al píxel con el modelo analítico y las derechas no; las cifras de
la sonda reproducen exactamente el error de índices (p. ej. f_tr proyectada
a ndc_y −2,851 frente a −0,858 correcta). La sonda se retiró antes del
cierre. Nota: la verificación física de B7 en su día miró el frustum a ojo
desde un punto donde dominaba el ápex y no midió las esquinas lejanas; por
eso el defecto sobrevivió al primer cierre.

**Arreglo de raíz:** las 8 esquinas se calculan ahora con un único helper
`esquina_frustum(...)` que aplica `pos + fwd*d + right*(sr*w) + up*(su*h)`
componente a componente con un solo bucle indexado: la clase de error
(índice cambiado en una componente) ya no puede colarse en una esquina
suelta. Tests de regresión nuevos con los valores exactos del caso medido
en vivo y con una base rotada (yaw 90°: perpendicularidad y centrado).

**Verificación física (antes/después desde la misma cámara de escena):**
(1) escena de control con frustum pequeño (fov 30, far 10): antes, una sola
diagonal torcida cruzando la vista; después, el rectángulo lejano cae al
píxel donde lo predice el modelo (cobertura medida sobre la captura:
arriba 60/65, derecha 81/81, abajo 69/74). (2) La configuración de su
captura (cámara [0, 3.5, 9], rotación −14°, fov 60): antes, la diagonal
torcida; después, la banda superior nivelada del rectángulo lejano y la
pirámide apuntando hacia delante y abajo sobre el suelo, de acuerdo con la
Camera Preview. (3) SampleScene: antes, dos diagonales gigantes cruzando
el cielo; después, las aristas laterales simétricas convergiendo sobre el
eje de la cámara. El tope del plano lejano dibujado (25 m) se mantiene: con
las esquinas correctas es el tamaño de siempre del gizmo; si se quiere más
corto, es decisión aparte.

**Consumo: Opus 0** (diagnóstico, arreglo y tests directos de Rayo).

---

## GIZMO DE MOVER INVERTIDO (reporte de D3m3tr10, 2026-11-17, probando la v0.10.17.1-beta)

Sus palabras: «cuando selecciono un objeto para mover, ejemplo el Z si
muevo el mouse para donde esta mirando la flecha, se va a la direccion
contraria, al igual con los otros ejes». Captura suya: Main Camera
seleccionada con la flecha Z azul apuntando a la izquierda en pantalla.

**¿Comparte raíz con B7? No.** En B7 el dibujo del frustum tenía dos
índices cambiados; aquí el dibujo del gizmo es correcto (los ejes se
pintan desde el origen de mundo de la entidad con los ejes de mundo) y lo
que fallaba era la matemática del ARRASTRE.

**Causa raíz:** el arrastre de traslación no proyectaba nada: aplicaba un
mapeo fijo pantalla→eje pensado para la vista por defecto del editor
(`X += delta.x·0,02`, `Y -= delta.y·0,02`, `Z += (delta.x+delta.y)·0,02`,
y mapeos análogos en escala y en el modo combinado). Ese mapeo solo
coincide con la realidad cuando la cámara mira desde +Z con X a la
derecha e Y arriba; al orbitar, cada eje se proyecta en otra dirección de
pantalla y el objeto se movía al contrario de su flecha (o en diagonal).

**Reproducción física del defecto (binario sin el arreglo):** SampleScene,
Platform Tier 2 en (6.00, 1.50, 0.00); órbita de +90° (la flecha Z apunta
a la izquierda, como en su captura); arrastre real de −120 px HACIA la
punta de la flecha Z: el Inspector marcó Z = −2,40 (exactamente
−120 × 0,02 del mapeo fijo) y el objeto se desplazó en pantalla en
dirección contraria al arrastre.

**Arreglo de raíz:** `gizmo_3d::avance_arrastre_eje(origen, dir, vp, rect,
delta)` proyecta el eje a pantalla y devuelve el avance firmado en
unidades de mundo que sigue al cursor 1:1 sobre la dirección proyectada
del eje (0 si el eje apunta a la cámara: su proyección mide < 1 px). La
constante `GIZMO_EJE_LEN` (1,4) es ahora la única fuente de la longitud
del eje para dibujo, anillos y arrastre. El viewport aplica el avance en
el eje de MUNDO y convierte el resultado a local con la inversa de la
matriz de mundo del padre cuando la entidad tiene padre. El arrastre por
el centro pasa a mover en el plano de la vista (derecha/arriba de la
cámara, mundo-por-píxel calculado a la distancia del objeto; con Shift,
profundidad a lo largo de la mirada), y el de escala usa el mismo avance
por eje. Además, la captura del eje se decide al PULSAR el botón
(`press_axis`), no en `drag_started()`: con un eje muy escorzado
(≈25 px), un primer movimiento brusco sacaba el puntero del segmento
antes del frame de inicio del arrastre y la captura se perdía en
silencio (medido con sonda temporal en el binario; la sonda se retiró).

**Tests:** 4 nuevos sobre `avance_arrastre_eje` (vista por defecto 1:1
con el cursor; vista orbitada 180° sin inversión — la regresión; eje Z
desde el lado avanzando hacia la punta; eje apuntando a la cámara = 0).
Suite del editor: 102/102.

**Verificación física (binario con el arreglo, misma coreografía que la
reproducción; vista orbitada +90°):** Z: 0,00 → +1,8935 arrastrando
120 px hacia la punta (predicho por la proyección 1:1: +1,91); el objeto
sigue al cursor hacia la izquierda. Y: 1,50 → 3,0450 arrastrando ~100 px
hacia arriba (predicho +1,57). X (eje casi hacia la cámara, ~25-30 px en
pantalla): 6,00 → 8,8189 con arrastre continuo hacia la punta y
6,00 → 9,4439 repitiendo el primer paso brusco que antes no capturaba.
En los tres casos los otros dos ejes quedan intactos.

**Consumo: Opus 0 · DeepSeek 0 · Kimi 0** (diagnóstico, reproducción,
arreglo, tests y verificación directos de Rayo).
