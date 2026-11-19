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

## ARRASTRE .BLEND — «el objeto no carga, solo carga un cubo» y «se separa de la punta» (reporte de D3m3tr10, 2026-11-17, probando la v0.10.17.1-beta)

Reporte suyo con su archivo `campfire_voxel_v3.blend` (hoguera voxel,
8,8 MB): «ya arrastro los modelos .blend a la escena y pasan 2 cosas.
1 cuando lo selecciono el objeto en el mouse se separa de la punta y es
incomodo. 2 el objeto del .blend no carga, solo carga un cubo y es una
hoguera».

**Diagnóstico (tres causas encadenadas para el punto 2):** el puente
convierte el archivo sin problema (corrida manual: .glb de 1.638.184 B,
954 mallas, 957 objetos), pero la vía de soltado solo crea la entidad con
el stem del archivo: el modelo entra en el AssetCache únicamente cuando
el AssetWatcher emite `MeshReloaded`, y (a) el vigía SOLO reaccionaba a
eventos Create/Modify — un .blend copiado a la carpeta ANTES de abrir el
editor nunca generaba evento ni conversión; (b) el vigía solo cubría el
`assets/` del directorio de trabajo, nunca los assets del proyecto
abierto; (c) si la conversión se disparaba sin `GLASS_BLENDER` en el
entorno, el bridge no encontraba Blender (solo buscaba la variable o el
PATH) y fallaba sin reintento hasta el próximo guardado. Reproducción
exacta de su síntoma (Estado A): .blend preexistente sin .glb → entidad
creada renderizando el cubo de respaldo. Con la cadena sana (Estado B,
conversión disparada con el editor en marcha) la misma entidad pasó a
dibujar la hoguera real: la cadena de recarga funciona; lo que faltaba
era dispararla.

**El punto 1 tiene dos mitades:** el arrastre con el gizmo que se separa
de la punta es el mapeo fijo ya arreglado en el bloque anterior
(`f44bcfb`); y el soltado aterrizaba SIEMPRE en el plano y=0, así que al
soltar señalando una superficie elevada el objeto aparecía desplazado a
lo largo del rayo hasta el suelo, lejos de la punta del ratón.

**Arreglo:**
- El vigía, al crearse, REGISTRA los modelos ya convertidos del árbol
  (emite `MeshReloaded` por cada fuente con .glb al día), incluidos los
  de fuera de `assets/models/`, que la precarga no cubre. NO convierte en
  el escaneo (ver «lo que el escaneo masivo enseñó» más abajo).
- Vigía por proyecto: la app mantiene un vigía por raíz de assets (el
  inicial + uno por proyecto abierto, sin duplicados por reapertura), y
  todos alimentan el mismo drenado de recargas. La ruta del bridge solo
  se fija si el bridge existe en esa raíz (un proyecto de usuario no
  trae `tools/` y no debe secuestrar la ruta del motor).
- Conversión al soltar: si la fuente soltada no tiene .glb al día, el
  panel (viewport o jerarquía) anota la petición y la app la reenvía al
  vigía dueño de la carpeta; al terminar, el modelo real sustituye al
  cubo en la misma entidad. Cerrojo global: como mucho UN Blender a la
  vez en todo el proceso.
- El bridge localiza Blender también en las rutas de instalación
  habituales (Program Files y Steam en Windows, /usr/bin, /usr/local/bin,
  snap y /opt en Linux, /Applications en macOS) si ni `GLASS_BLENDER` ni
  el PATH lo dan. Verificado con el entorno vacío: encuentra
  /usr/local/bin/blender y convierte el campfire entero.
- Aterrizaje sobre la superficie: `gizmo_3d::punto_aterrizaje` (la misma
  pasada de mallas del picking, con las cotas reales) da el punto de
  impacto del rayo de soltado; si no golpea nada se cae al plano y=0 de
  siempre. Test nuevo de regresión en `tests_picking`.

**Lo que el escaneo masivo enseñó (dos defectos del primer diseño,
cazados en la verificación física y corregidos antes de cerrar):** la
primera versión del escaneo CONVERTÍA toda fuente sin .glb al arrancar:
en este repo son 493 .fbx del paquete polygon-prototype — 493 procesos
de Blender a la vez agotaron la memoria y mataron el editor. Y aun
registrando sin convertir, emitir los modelos de `assets/models/` en el
arranque los subía a GPU dos veces (la precarga diferida los sube
también) y el proceso moría en el rasterizador por software (segfault
en llvmpipe, reproducido 3 veces y confirmado por bisección contra el
binario sin el cambio). Por eso el escaneo registra sin convertir y el
vigía inicial salta la carpeta `models/` (territorio de la precarga);
los vigías de proyecto sí la incluyen (ninguna precarga cubre un
proyecto abierto).

**Verificación física (editor lanzado SIN `GLASS_BLENDER`, con Blender
solo en una ruta estándar):** arranque limpio, cero conversiones y cero
fallos al iniciar (el tormentón anterior desapareció). Soltado del
campfire (preexistente, sin .glb) sobre el suelo: entidad creada y
«Conversión solicitada al soltar»; ~25 s después la hoguera real (anillo
de piedras voxel, troncos y penacho) sustituía al cubo, posada en
[2,69 · 0,50 · −13,89] (la superficie del suelo bajo el cursor, no el
plano y=0). Segundo soltado sobre Platform Tier 1 (medida con
grounding, no a ojo): aterrizó en [3,02 · **1,50** · 1,75] — la cara
superior exacta de la plataforma — y la hoguera se ve posada sobre ella.
Suite del editor: 103/103 (1 test nuevo). Pipeline local 5/5.

**Nota sobre el defecto 6 del probador («arrastrar un .blend congela el
motor»):** es la misma familia — el .blend que no carga — con síntoma
distinto (congelación frente a cubo). Una conversión lanzada sin control
de concurrencia ni de memoria encaja con una congelación en una máquina
ajustada; con los datos que faltan de su amigo se cruzará.

**Consumo: Opus 34.503** (el paquete de diseño: escaneo, vigía por
proyecto, descubrimiento de Blender y aterrizaje; la corrección del
escaneo tras la verificación física y las integraciones finas son de
Rayo) · DeepSeek 0 · Kimi 0.

---

## SELECCIÓN DE GPU EN EQUIPOS HÍBRIDOS — CERRADO (2026-11-17)

**Caso**: al amigo probador (Windows, v0.10.17.1-beta) se le cerraba el
motor al abrir el proyecto, sin mensaje y sin log. Su Visor de eventos
lo zanjó: access violation (c0000005) dentro de `igvk64.dll`
27.20.100.9415, el ICD Vulkan de su Intel integrada (driver de 2021).
Corrección clave de D3m3tr10: el amigo SÍ tiene una NVIDIA dedicada
con el driver actualizado — el motor estaba corriendo sobre la
integrada, cuyo driver murió compilando los pipelines de grafo de la
precarga diferida.

**Diagnóstico de la selección (context.rs)**: la puntuación ya prefería
la discreta (base 10000 frente a un máximo de 8000 de la integrada) y
es idéntica en el tag v0.10.17.1-beta y en main; el contexto se crea
UNA vez al arrancar (no al abrir). Por tanto la NVIDIA nunca llegó a
puntuar: el filtro previo (cola de gráficos + cola de presentación
sobre la superficie + extensión swapchain) la dejó fuera en silencio —
en un portátil híbrido la dedicada puede no reportar presentación
sobre la superficie de la ventana — o ni siquiera fue enumerada. El
defecto de fondo: el filtro no dejaba NINGÚN rastro (la línea «GPU
detectada» solo se imprimía para las que ya habían pasado), así que el
caso era indiagnosticable desde el log.

**Arreglo** (crates/glass_renderer/src/context.rs):
- La selección se extrae a funciones puras: `GpuDeviceInfo` (datos
  planos ya consultados), `gpu_score` (misma escala histórica:
  discreta 10000 → integrada 5000 → virtual 2000 → CPU 1000, bonus
  VRAM/100 capado a 3000, nombre preferido +50000) y `select_gpu`.
- Se anota TODA GPU enumerada con nombre, tipo, vendor, **versión de
  driver** (cruda y en hex) y VRAM, y el veredicto: candidata con su
  score o el motivo de rechazo («sin cola de presentación para la
  superficie», etc.). La elegida se anota con su driver y su score.
- `aviso_gpu`: si la elegida no es la dedicada habiendo una dedicada
  enumerada, se genera un aviso que nombra a ambas y el motivo; va al
  log (warn) y a una **banda visible en el Hub** (campo `gpu_aviso` de
  VulkanContext → LauncherPanel), con la salida práctica: forzar la
  dedicada en la configuración de gráficos de Windows o actualizar el
  driver de la integrada.
- La política de fondo no cambia: una GPU que no puede presentar no
  se elige a ciegas (la creación de la swapchain fallaría al arrancar,
  peor que avisar). Promoverla con reintento y repliegue queda como
  paso opcional si se pide, con su coste dicho.

**Verificación**: 9 tests nuevos de selección con listas simuladas
(híbrido en ambos órdenes, discreta sin VRAM contra integrada con
mucha, discreta sin presentación + aviso con ambos nombres, orden
integrada→virtual→CPU, solo CPU, lista vacía, sin gráficos, sin
swapchain, nombre preferido) — 9/9 en verde. Pipeline local 5/5 a 0.
Test físico: el editor arranca y el log trae las líneas nuevas con la
llvmpipe de la VM («GPU enumerada: 'llvmpipe (LLVM 20.1.2, 256 bits)'
| Tipo: CPU | Vendor: 0x10005 | Driver: 104865800 (0x06402008) |
candidata (score 1079)» y «GPU seleccionada: … | Score: 1079»); el
Hub pinta normal y sin banda (aquí no hay dedicada: el aviso no
aplica). El caso híbrido real no es reproducible en esta VM (solo
lavapipe): la parte de enumeración en un híbrido queda cubierta por
los tests simulados y se confirmará con el log del amigo en la
siguiente release.

**Consumo: Opus 0 · DeepSeek 0 · Kimi 0** (auditoría, arreglo y tests
de Rayo, en la clase de arreglos quirúrgicos directos con precedente:
dock, B7, gizmo).

---

## PUNTO 5 — PROYECTO FANTASMA ROSADO DEL HUB — CERRADO (2026-11-17)

**Reporte**: con el motor recién instalado salía siempre la tarjeta
rosada «GlassEnginer — Proyecto del motor», aunque se borrara todo, y
no se podía quitar.

**Causa raíz**: `descubrir_proyectos` (launcher.rs) sembraba la
tarjeta del directorio de trabajo INCONDICIONALMENTE como primera
entrada, sin comprobar que ahí hubiera un proyecto. En una
instalación el CWD es la carpeta de instalación: la tarjeta se
fabricaba en cada pintado y, al no estar en ningún registro, era
inamovible por construcción.

**Arreglo** (paquete Opus, integrado por Rayo): la entrada del CWD
solo se incluye si `cwd/project.glass` existe, y entonces como un
proyecto más: nombre real con la nueva `nombre_proyecto` (campo
"name" del project.glass; si no, el de la carpeta), descripción
«Proyecto • En este equipo» y clic uniforme `OpenProject` (fuera el
parámetro `es_del_motor` de `project_card` y su ruta especial
OpenDefaultScene, que queda solo para el botón «Abrir» hasta el
punto 6). La deduplicación por ruta canonizada se mantiene. Estado
vacío diferenciado: sin proyectos → «Todavía no hay proyectos en
este equipo. Crea uno con «➕ Nuevo» o abre uno existente con
«📂 Abrir».»; con proyectos pero filtro sin coincidencias → el
mensaje de búsqueda de siempre.

**Verificación**: tests del descubrimiento 3/3 (el existente
actualizado a la regla nueva + `cwd_sin_proyecto_no_aparece` +
`cwd_con_nombre_en_el_meta_lo_usa`, que cubre también la no
duplicación con registro y escaneo). Pipeline local 5/5 a 0. Test
físico en dos ambientes: (a) HOME vacío y CWD sin project.glass
(assets enlazados) → pestaña Proyectos SIN ninguna tarjeta y con el
mensaje de vacío nuevo (captura p5_a.png); (b) mismo ambiente con un
project.glass `{"name": "ProyectoFisico"}` en el CWD → la tarjeta
aparece una vez, con ese nombre y la descripción normal (captura
p5_b.png). Quitar/renombrar tarjetas es el punto 7 (menú contextual).

**Consumo: Opus 14.225** · DeepSeek 0 · Kimi 0.

---

## BLOQUE WINDOWS — QUE NINGÚN CIERRE SEA MUDO — CERRADO (2026-11-17)

**Origen**: el probador en Windows perdió el motor sin mensaje ni log
(access violation en igvk64.dll 27.20.100.9415, driver Intel de 2021,
al abrir el proyecto). Aprobado por D3m3tr10 como bloque completo,
con su condición: sin conflictos con Vulkan — la secuencia arriesgada
corre SOLO en el hijo y el arranque normal no se toca.

**Tres piezas**:

1. **Log a archivo + hook de panic** (`glass_app/src/logfile.rs`,
   nuevo): todo el registro va a `glass.log` en la carpeta
   GlassEnginer (rotación a `glass.log.old` a los 5 MiB) además de
   stderr, con vaciado en cada escritura (si el proceso muere dentro
   del driver, lo último ya está en disco). El hook de panic anota
   `PANIC: mensaje (en archivo:línea)` reabriendo el archivo, sin
   pasar por el mutex del logger. La ruta de datos duplica la regla
   de `updates::data_root` porque glass_editor es opcional en
   glass_app.
2. **Aviso preventivo Intel 2021** (`context.rs`): si la GPU elegida
   es una integrada Intel con driver de la era, el Hub lo avisa antes
   de abrir nada, nombrando la dedicada si existe. **Corrección de
   integración de Rayo**: los drivers Intel de Windows NO usan el
   empaquetado estándar VK_VERSION — codifican (tercer componente,
   cuarto) como (raw >> 14, raw & 0x3FFF); la 27.20.100.9415 llega
   como (100, 9415). La detección usa `version_intel_windows` y la
   regla «tercer componente < 101» (la rama 101 llega en 2022); con el
   empaquetado estándar la 9415 se leería 0.402.661 y el aviso nunca
   saltaría en el equipo real. La enumeración ligera `enumerar_gpus`
   (solo instancia, sin ventana) alimenta la clave de la sonda.
3. **Sonda Vulkan en proceso hijo** (`main.rs`): en el arranque
   normal del editor (ni standalone ni tests), el padre enumera las
   GPUs y, si la caché `gpu_probe.json` no cubre (versión del motor,
   conjunto nombre+driver), lanza el mismo ejecutable con
   `--gpu-probe`. El hijo crea ventana, contexto, renderer, compila
   los materiales de grafo y sube 10 modelos, anunciando por stdout
   `GLASS_PROBE_GPU` al crear el contexto y `GLASS_PROBE_OK` al
   completar; el padre espera hasta 180 s leyendo su salida en un
   hilo. Si el hijo muere o no completa, el padre sigue vivo, cachea
   el fallo y el Hub muestra el diagnóstico (GPU, controlador legible
   — para Intel en su formato 100.9415 —, la dedicada si existe y la
   salida práctica). La banda del Hub ahora parte el texto en líneas
   (antes se salía de la ventana) y el ⚠ ya no se duplica.

**Condición de no-interferencia, verificada**: el padre solo enumera
(con una instancia efímera que se destruye) y ESPERA a que el hijo
termine antes de crear su propio contexto: nunca hay dos Vulkan
activos a la vez y el arranque normal no cambia. En las tres
arrancadas físicas el motor creó después su contexto y su Hub con
normalidad.

**Verificación**: pipeline local 5/5 a 0 (clippy cazó un
`let_unit_value` en la sonda, corregido). Tests: renderer 75/75
(5 nuevos del aviso Intel, con el valor real 1647815 del driver del
probador), app 66/66 (formateador de pánico + diagnóstico). Test
físico con HOME aislado: (1) primer arranque — el hijo ejecutó la
secuencia entera (10 modelos subidos) dentro de `glass.log`, caché
`ok` escrita, Hub sin banda; (2) segundo arranque — «resultado en
caché correcto; no se repite», sin hijo; (3) caché sembrada en
fallo — el Hub muestra la banda ámbar con el diagnóstico completo
en dos líneas (capturas win_1/win_4.png). El aviso Intel no es
reproducible en la VM (no hay GPU Intel): cubierto por tests con la
codificación real.

**Consumo: Opus 73.209** (AB 26.578 · C intento vacío 11.450 ·
C 35.181) · DeepSeek 0 · Kimi 0.

## PUNTO 6 — EL BOTÓN «ABRIR» DEL HUB ABRE EL SELECTOR DE CARPETAS (CERRADO)

**Defecto (del análisis Hub/Proyecto)**: el botón «📂 Abrir» no abría ningún
selector: disparaba `OpenDefaultScene`, la misma acción que una tarjeta.

**Arreglo** (paquete **Sonnet 5**, 10.337 tokens; integración con dos rescates
de Rayo, ver abajo):
- Nueva dependencia **`rfd = "0.15"`** en glass_editor (portal XDG en Linux,
  diálogo nativo en Windows; sin dependencias nativas de compilación).
- El clic en «Abrir» llama a `rfd::FileDialog::pick_folder()` con el título
  «Abrir proyecto de GlassEnginer». Si el usuario cancela, no pasa nada.
- La decisión vive en la función libre testeable
  **`abrir_carpeta_proyecto(store, dir)`**: sin `project.glass` devuelve el
  error «La carpeta «…» no contiene un proyecto de GlassEnginer (falta
  project.glass).»; con él, registra el proyecto (`nombre_proyecto` +
  `register_project`) y devuelve `OpenProject(dir)`. El guardado lo hace el
  handler del clic tras el Ok; el error se pinta en rojo bajo la cabecera
  (campo nuevo `error_abrir`).
- **`OpenDefaultScene` retirado del todo** (variante del enum, su patrón en
  las acciones diferidas de main.rs y su brazo manejador).

**Rescates de integración (patrones conocidos)**: el par de la función libre
no ancló en silencio y el de los tests supuso un módulo inexistente; la
función se insertó extrayéndola de la respuesta y los 2 tests se añadieron a
mano al módulo `tests_descubrimiento`, ampliando sus imports selectivos.

**Verificación**:
- Suite glass_editor **107/107** (2 nuevos: carpeta sin project.glass → error
  y no registra; con project.glass → registra y devuelve OpenProject).
- Pipeline local completo 5/5 a 0; CI verificado tras el push.
- **Test físico PASS por el camino real completo** (con el portal XDG y el
  backend GTK instalados en la VM y una sesión D-Bus propia): clic en
  «Abrir» → se abre el diálogo nativo del sistema con el título previsto →
  navegando hasta un proyecto externo y eligiendo su carpeta, el proyecto
  quedó **registrado en launcher.json** y **se abrió en el editor** (escena
  vacía, sin escena por defecto en su project.glass). Segunda vuelta:
  eligiendo una carpeta sin project.glass, el Hub muestra el **error en rojo
  con la ruta** y no registra nada; la tarjeta del proyecto bueno sigue en la
  lista. Extra verificado de paso: sin servicio de portal, `pick_folder`
  devuelve None y la app sigue viva sin hacer nada (degradación limpia,
  registrada por rfd en glass.log).

## PUNTO 7 — MENÚ CONTEXTUAL EN LAS TARJETAS DE PROYECTO (CERRADO)

**Capacidad nueva (bloque H2 del análisis Hub/Proyecto)**: las tarjetas no
tenían menú contextual; los proyectos solo se podían abrir.

**Arreglo** (paquete **DeepSeek V4 Flash**, 12.581 tokens; rescates de
integración de Rayo: las tres funciones libres y los 4 tests se insertaron a
mano porque sus pares no anclaron — el ancla de la función del punto 6 se
escribió de memoria en el prompt y no coincidía; clippy cazó además un
`iter().any()` sustituible por `contains()` en los métodos nuevos):
- Clic derecho en una tarjeta → **Abrir · Abrir carpeta contenedora ·
  Renombrar… · Quitar de la lista · Eliminar del disco…**. La acción elegida
  se anota (`AccionTarjeta`) y se ejecuta al terminar la cuadrícula, sin
  mutar el almacén mientras se pintan las tarjetas.
- **Abrir carpeta contenedora**: `abrir_carpeta_contenedora()` lanza el
  explorador del SO (explorer/open/xdg-open) sobre la carpeta padre.
- **Renombrar**: ventana con el nombre actual; `renombrar_proyecto()` mueve
  la carpeta en disco, reescribe el "name" del project.glass y refresca el
  registro (valida vacío, barras y destino existente).
- **Quitar de la lista**: `LauncherStore` gana el campo persistente
  **`ocultos`** (serde default) y `quitar_de_la_lista()` = desregistrar +
  ocultar; `descubrir_proyectos` filtra los ocultos en las TRES fuentes
  (CWD, registro y escaneo), así un proyecto escaneado no resucita.
- **Eliminar del disco…**: ventana de confirmación con la ruta y el aviso
  de irreversibilidad; al confirmar, `eliminar_proyecto_del_disco()` borra
  la carpeta entera y desregistra. Los errores se pintan en la línea de
  error del Hub (la del punto 6).

**Verificación**:
- Suite glass_editor **111/111** (4 nuevos: filtro de ocultos en las tres
  fuentes, quitar desregistra y oculta, renombrar mueve carpeta+meta+
  registro y rechaza barras, eliminar borra y desregistra).
- Pipeline local completo 5/5 a 0; CI verificado tras el push.
- **Test físico PASS completo** (HOME aislado, un proyecto registrado y uno
  escaneado): el menú sale con las 5 entradas; «Abrir carpeta contenedora»
  lanzó xdg-open con la carpeta padre exacta (verificado con un xdg-open
  instrumentado en el PATH); «Renombrar…» renombró la carpeta en disco, el
  project.glass y la tarjeta; «Quitar de la lista» quitó la tarjeta,
  desregistró y dejó la ruta en `ocultos` con la carpeta intacta;
  «Eliminar del disco…» pidió confirmación con la ruta y borró la carpeta
  y el registro.

## PUNTO 8 — LA RUTA DE LA ESCENA FUERA DE LA BARRA SUPERIOR (CERRADO)

**Defecto** (captura del probador en Windows): el selector 🎬 de la barra
superior mostraba la ruta entera de la escena
(`\\?\C:\Users\...\SampleScene`) entre File y Edit.

**Causa raíz**: `glass_core::scenes::short_scene_name` cortaba el nombre
solo por `/`. Con una clave de escena con barras invertidas (la ruta
canonizada de Windows) no hay `/` que cortar y el «nombre corto» era la
ruta completa. La función alimenta el nombre activo del SceneManager, el
guardado y el selector de la barra: un solo arreglo los cura todos.

**Arreglo** (paquete **DeepSeek V4 Flash**, 4.268 tokens, integrado sin
rescates):
- `short_scene_name` corta por `/` y por `\` (`rsplit(['/', '\\'])`), con
  dos tests nuevos de rutas Windows (normal y verbatim `\\?\`).
- El menú **File** gana **«Abrir carpeta contenedora de la escena»** y
  **«Copiar ruta de la escena»**, tras Open Scene. Como la ruta vive en la
  app (`ruta_escena_actual`), el layout anota la petición
  (`request_abrir_contenedora_escena` / `request_copiar_ruta_escena`,
  patrón de `request_build_settings`) y la app la resuelve: contenedora
  con el helper del punto 7 y copiar con `copy_text`; sin archivo, aviso
  «La escena aún no tiene archivo (guárdala primero)».

**Verificación**:
- glass_core 212 tests (incl. los de rutas Windows); pipeline local 5/5.
- **Test físico PASS** (editor con la escena Escaparate): el selector 🎬
  muestra «Fase9Escaparate»; File trae las dos acciones en su sitio;
  «Abrir carpeta contenedora» lanzó el explorador sobre `assets/scenes`
  (verificado con xdg-open instrumentado); «Copiar ruta» dejó su aviso
  «Ruta de la escena copiada al portapapeles» en la barra. (La lectura del
  portapapeles X11 no se verificó: la VM no tiene xclip/xsel; el copiado
  usa el mecanismo estándar de egui.) El caso Windows del nombre queda
  cubierto por los tests unitarios con rutas reales.

## MENÚ WINDOW CON TODOS LOS PANELES (CERRADO)

**Pedido** (2026-11-17): un botón arriba por si se cierra alguna
herramienta (jerarquía, game, inspector…), «como lo hace Unity».

**Diagnóstico**: ningún panel se podía cerrar a mano (`is_closeable`
false en todas), pero los presets de Layouts reconstruyen el dock y un
panel fuera de la plantilla desaparecía sin retorno individual;
Frametime nacía oculta y MeshForge dependía del portón de arranque — el
mismo defecto del diagnóstico A: el portón se evaluaba una sola vez por
proceso, así que instalar MeshForge desde el Hub y abrir un proyecto en
la misma sesión dejaba el editor sin su pestaña y sin aviso.

**Arreglo** (paquete **DeepSeek V4 Flash**, 4.112 tokens, integrado sin
rescates):
- `set_tab_visible(tab, on)` + `tab_visible(tab)` en el layout:
  generaliza el mecanismo probado de Frametime (find_tab →
  push_to_focused_leaf / remove_tab, idempotente). Frametime reutiliza
  su vía (sincroniza el flag que la restaura tras los presets); la de
  MeshForge sincroniza `mesh_tool_enabled` para los presets sin
  reaplicar ninguno en el momento.
- El menú **Window** lista los 9 paneles con casilla y sus títulos
  reales. La de **MeshForge solo aparece si el portón está activo en
  ese momento**: `plugin_enabled` relee el almacén de disco en cada
  llamada, así que instalar el complemento a mitad de sesión ya ofrece
  la casilla **sin reiniciar la app** (arreglo del diagnóstico A).

**Verificación**: glass_editor 113/113 (2 tests nuevos); pipeline 5/5.
**Test físico PASS con el escenario exacto del diagnóstico A**:
arranque con el complemento desactivado en el registro → editor sin
pestaña MeshForge y Window sin su casilla; activación del registro a
mitad de sesión (lo que escribe una instalación desde el Hub) → Window
ofrece «🧊 MeshForge» sin marcar; al marcarla, la pestaña aparece en el
dock con su panel, sin reiniciar. Además: desmarcar «Consola» la quita
del dock y volver a marcarla la trae de vuelta. El registro de
complementos quedó restaurado a su estado original tras la prueba.

## PUNTO 9 — MENÚ CREAR Y CLIC DERECHO DEL PANEL PROYECTO AMPLIADOS (2026-11-17, CERRADO)

**Qué faltaba:** el panel Proyecto creaba 5 tipos (Carpeta, Script, Grafo, Escena, Material) y sus menús contextuales no tenían las acciones de trabajo diario: crear un prefab vacío, importar un asset desde el disco, abrir la carpeta en el explorador del sistema, copiar la ruta ni reconvertir un .blend/.fbx a demanda.

**Arreglo:**
- **Prefab vacío:** nueva variante `TipoCreacion::Prefab` («🧩 Prefab», nombre por defecto «NuevoPrefab») con `crear_prefab`/`crear_prefab_en`: crea una escena temporal con una entidad raíz del nombre dado y la guarda con `Prefab::from_entity` en la subcarpeta `prefabs`/`Prefabs`; entra en el submenú Crear compartido y en la cabecera del panel. Test: el archivo creado se carga con `Prefab::load_from_file`, la raíz lleva el nombre y un duplicado falla.
- **Menú de fondo:** además de «➕ Crear ▸», «📥 Importar asset…» (diálogo del sistema vía rfd → `importar_archivo` a la carpeta actual) y «↻ Refrescar».
- **Menú de elemento:** «📂 Abrir en el explorador» (helper nuevo `abrir_en_explorador` en launcher.rs: una carpeta se abre ella misma y un archivo abre su contenedora — semántica distinta de `abrir_carpeta_contenedora`, que abre el padre de lo dado), «📋 Copiar ruta», «🔄 Reconvertir» SOLO para .blend/.fbx, «📥 Importar asset…» y «↻ Refrescar», antes del separador de Eliminar.
- **Reconvertir de verdad:** el panel gana la cola pública `pending_conversion` (no la tenía; solo la tenían viewport y jerarquía) y `App::drenar_peticiones_conversion` la drena igual que las otras dos, reenviando la petición al vigía de assets, que convierte con el cerrojo de un solo Blender.

**Verificación:** suite glass_editor 114/114 (test nuevo del prefab incluido); pipeline local exacto 5/5 a 0 (un `collapsible_if` de clippy en el menú nuevo, corregido al integrar). **Test físico PASS por el camino real:** botón «🧩 Prefab» de la cabecera → `assets/prefabs/PruebaP9.prefab.json` creado en disco (con el aviso «Creado: …» en el panel; sin nombre escrito da el error «Escribe un nombre antes de crear el asset», comportamiento previo de la cabecera); menú de fondo con las tres entradas; menú de elemento sobre una carpeta con todas las acciones y «Abrir en el explorador» invocando el explorador con la carpeta misma (`assets/animators`, verificado con un xdg-open instrumentado); menú sobre `fase6_test.blend` con «🔄 Reconvertir» → petición drenada al vigía y, con Blender en el PATH, .glb regenerado y malla reemplazada en VRAM por recarga en caliente. El prefab de prueba se borró tras la verificación.

## CORRECCIÓN AL PUNTO 9 — EL SUBMENÚ CREAR NO TENÍA PREFAB (detectada y corregida con P3a)

Al auditar el panel para P3a se descubrió que la lista de `submenu_crear` (el menú «➕ Crear» compartido por el fondo y los elementos) tenía 5 tipos y NO 6: faltaba Prefab. El par de DeepSeek del punto 9 supuso un orden de la lista que no era el real y el integrador lo saltó en silencio; la cabecera sí tenía el botón «🧩 Prefab» y la creación funcionaba, pero el test físico del punto 9 no abrió el submenú y el hueco se escapó. Corregido insertando `(TipoCreacion::Prefab, "🧩 Prefab")` entre Material y Grafo. Verificado físicamente con P3a: el submenú Crear muestra los 6 tipos.

## PUNTO 10 / P3a — VENTANA PROYECTO ESTILO UNITY: ESTRUCTURA EN DOS ZONAS (2026-11-17, CERRADO)

Primera mitad del rediseño de la ventana Proyecto (P3b hará Favoritos/Quick Access, miniaturas y el slider con modo lista). Todo en `crates/glass_editor/src/panels/project.rs`:

- **Dos zonas:** árbol de carpetas a la izquierda (180 px, con sangría por profundidad y la carpeta actual resaltada) y la lista de assets de siempre a la derecha, separadas por una línea vertical. El árbol se cachea en `refrescar()` (`arbol_carpetas` vía la función existente `listar_carpetas`); no se recorre el disco en cada fotograma.
- **Migas de pan clicables** en la cabecera en vez de la etiqueta estática de ruta: función libre nueva `migas(assets_root, current_dir)` (etiqueta + ruta acumulada por segmento, con test). Cada segmento navega.
- **Buscador** sobre la carpeta actual: campo «Buscar en esta carpeta…» con botón ✖; el filtro se aplica a la copia del listado antes de pintar (alcance declarado: NO recursivo; eso llega con los Favoritos de P3b).
- **Los `.meta.json` dejan de verse:** `listar_directorio` salta los nombres terminados en `.meta.json` (gestión interna de la conversión .blend/.fbx), con test.
- **Lección de egui 0.36 (defecto cazado en el test físico):** la primera versión montaba las dos zonas con `egui::Panel` + `egui::CentralPanel` dentro de la pestaña del dock; al cabo de unas interacciones la primera fila del panel (la cabecera entera) quedaba bajo la barra de pestañas y el contenido se desplazaba ~30 px hacia arriba (la Consola, en la misma hoja, se veía bien: el defecto era de los paneles anidados, no del dock). Sustituidos por columnas de layout ordinario (`horizontal_top` + `allocate_ui_with_layout` + `vertical`), sin estado de panel de por medio. La API además cambió en 0.36: `SidePanel` ya no existe (es `Panel`), `default_width`/`width_range` son `default_size`/`size_range` y `show_inside` está deprecado en favor de `show`.

**Paquete:** Sonnet 5 (14.078 tokens) con los 10 pares A–J anclados sobre el fuente real; los 2 tests nuevos los insertó Rayo a mano desde las NOTAS del paquete. Correcciones de Rayo: la adaptación a la API de paneles de egui 0.36 y, tras el defecto del test físico, la sustitución completa del andamiaje de paneles por layout ordinario.

**Verificación:** suite glass_editor 116/116 (2 tests nuevos: migas y .meta.json); pipeline local exacto 5/5 a 0. **Test físico PASS por el camino real** (Ejemplos → Escaparate → pestaña Proyecto): el árbol navega (raíz → animators; profundo → polygon-prototype/Characters/FixedScale), las migas muestran y recorren rutas de 2 y 4 segmentos y vuelven atrás, el buscador filtra («aud» → solo audio; «mode» → solo models; «fase6» → los 2 archivos fase6), **los .meta.json no aparecen** (con «fase6» se ven `fase6_test.blend` y `fase6_test.glb` pero NO `fase6_test.glb.meta.json`, que sí existe en disco), el submenú Crear del fondo muestra los 6 tipos incluido Prefab (corrección del punto 9) y la cabecera se mantiene en su sitio tras navegar, buscar y cambiar de pestaña (el escenario que rompía la versión con paneles).

## PUNTO 10 / P3b — VENTANA PROYECTO ESTILO UNITY: PRESENTACIÓN (2026-11-17, CERRADO)

Segunda mitad del punto 10 (la estructura, P3a, quedó cerrada en su sección). Alcance: Favoritos/Quick Access por tipo sobre el árbol, rejilla con miniaturas, slider de tamaño con vista de lista al mínimo y barra inferior con el seleccionado.

- **Favoritos / Quick Access:** sobre el árbol, cinco accesos (`TipoRapido`: Escenas, Materiales, Modelos, Prefabs, Scripts). Al pulsarlos, la zona derecha pasa a **vista rápida**: búsqueda recursiva por tipo en todo `assets/` (`buscar_por_tipo`, con tope de profundidad; reutiliza `listar_directorio`, así hereda el filtro de `.meta.json`), cada fila con su carpeta padre en texto débil a la derecha. La cabecera añade la miga «› **tipo**» y la barra inferior muestra «tipo · N elementos». Se sale pulsando una carpeta del árbol o una miga. El renombrado en línea y las colas de apertura/instanciación reutilizan los mecanismos de la lista.
- **Rejilla:** celdas de 40–128 px según el slider; imagen real decodificada para png/jpg/jpeg (`miniatura()`, con caché por ruta, espejo del patrón de miniaturas del Launcher), **muestra de color por albedo** para `.material.json` (`color_material()`, lee `MaterialData.albedo`), icono grande para el resto y nombre truncado con «…». Menú contextual de elemento completo (el mismo de la lista) y de fondo con Crear/Importar/Refrescar; arrastre a la escena con el mismo fantasma y payload que la lista (`publicar_fantasma_arrastre`).
- **Slider y barra inferior:** el slider «Tamaño:» (0–1) vive en la barra inferior; a 0 la vista es la **lista de siempre** (requisito expreso de D3m3tr10). La barra muestra el seleccionado («icono nombre — ruta legible»), o la carpeta actual, o «tipo · N elementos» en vista rápida.
- **Limitación declarada:** el menú contextual de la vista rápida es reducido (Renombrar, Abrir en el explorador, Copiar ruta, Eliminar); el completo vive en lista y rejilla.

**Dos defectos de layout/input cazados en el test físico y corregidos de raíz:**

1. **La barra inferior no se veía en hojas cortas.** Medido con sonda: a la altura por defecto de la hoja (~158 px) quedan ~48 px bajo las filas fijas y `allocate_ui_with_layout` concede a cada columna una altura mínima de ~64 px, así que la barra en flujo caía fuera de la hoja. Además, las etiquetas de Favoritos fuera de su ScrollArea desbordaban la asignación y empujaban todo hacia abajo. Arreglos: Favoritos y árbol comparten **una sola ScrollArea** (lo que debe recortarse va dentro de una), y la barra se pinta en un **rect absoluto al fondo de la hoja, con fondo opaco y después de las zonas** (Ui hija con `UiBuilder`), como una barra de estado: queda por encima del desborde de las zonas y siempre visible.
2. **Favoritos y árbol no recibían clics.** La sonda de acción no saltaba (el clic no llegaba, no era un fallo de estado) y la sonda de rect lo confirmó: el fondo clicable de la lista usaba `ui.clip_rect()` dentro de su ScrollArea, y ese clip heredado **abarca todo el ancho de la hoja** (no se estrecha a la columna derecha); como el fondo se registra después que la columna izquierda, le robaba todos los clics (la rueda sí pasaba, y las filas de la lista, registradas tras el fondo, también: de ahí el cuadro parcial). Arreglo: el fondo de la lista y el de la rejilla se limitan a `ui.max_rect()` de su columna (`viewport_lista` / `viewport_rejilla`). **Lección: dentro de una ScrollArea, `ui.clip_rect()` no es la ventana visible de la columna; un `interact` de fondo debe acotarse al rect de su propia columna.**

**Paquete:** Sonnet 5 (34.000 tokens) con los pares A–L anclados sobre el fuente real y las APIs de egui 0.36 pegadas en el prompt. Rescates de integración de Rayo: Sonnet inventó mecanismos paralelos en los métodos nuevos (campos `pending_*` inexistentes, `ui.close_menu()` por `ui.close()`, un patrón de arrastre con memoria temporal, «Copiar ruta» con la ruta legible) — todos sustituidos por los mecanismos reales de la lista; el fantasma del arrastre faltaba en los métodos nuevos y se extrajo a `publicar_fantasma_arrastre`; los 2 tests nuevos se insertaron a mano corrigiendo la llamada al ayudante de test. Los dos defectos de arriba los diagnosticó y corrigió Rayo en el test físico.

**Verificación:** suite glass_editor 118/118 (2 tests nuevos: `buscar_por_tipo_recursivo_y_sin_meta` y `tipo_rapido_coincide`); pipeline local exacto 5/5 a 0. **Test físico PASS completo por el camino real** (Ejemplos → Escaparate → pestaña Proyecto): la barra inferior se ve a la altura por defecto de la hoja; clic en «🎬 Escenas» → vista rápida con las 80 escenas recursivas, carpeta padre a la derecha, miga «› 🎬 Escenas» y barra «🎬 Escenas · 80 elementos»; clic en el árbol → sale de la vista rápida; clic en una carpeta de la lista → la barra muestra «materials — assets/materials»; slider a la derecha → rejilla con iconos grandes; en `materials`, `oro.material.json` se pinta con su albedo dorado (1.0, 0.766, 0.336, verificado contra el JSON en disco); en `textures`, miniaturas reales de `checker.png` (damero) y `test_icon.png` (magenta); slider al mínimo → vuelve la lista intacta.

**Consumo del punto 10 completo:** Sonnet 48.078 (P3a 14.078 + P3b 34.000) · DeepSeek 0 · Kimi 0 · Opus 0.

## PUNTO 11 — AVISO FALSO DEL INSPECTOR TRAS ADAPTAR UN MESH COLLIDER (2026-11-17, CERRADO)

**Reporte:** tras hornear en MeshForge y responder «Sí, adaptar» a la pregunta del mesh collider, el Inspector decía del nuevo modelo «⚠ Ese modelo no está en assets/: al dibujar se usará el cubo», aunque el .glb horneado estaba en assets/models/ y se dibujaba bien.

**Causa raíz:** el Inspector cachea la lista de modelos de `assets/` (`scan_model_names()`) y solo la reescaneaba al cambiar la entidad seleccionada o con el botón ↻. La adaptación cambia el VALOR del modelo de la entidad (pasa a `<nombre>_edited`) sin cambiar la selección, y el .glb horneado nace en ese mismo instante: la comprobación se hacía contra la lista vieja, donde `<nombre>_edited` aún no existía. El dibujo no usa esa lista (usa el AssetCache, que sí recibe el horneado por el vigilante), de ahí que el modelo se viera bien y el aviso fuera falso por partida doble.

**Arreglo (inspector.rs):** antes de dar el aviso por bueno, si el modelo actual no está en la lista cacheada se reescanea UNA vez para ese valor (campo nuevo `rescan_for_model`): un horneado o una conversión pueden crear el .glb sin cambiar la selección. Si tras reescanear sigue sin estar, el aviso es real y no se vuelve a escanear en cada frame para ese mismo valor (el reescaneo se rearma al cambiar la selección, el modelo o al pulsar ↻). El escaneo se partió en `scan_model_names_in(raíz)` para poder testearlo; `scan_model_names()` conserva firma y comportamiento.

**Paquete:** DeepSeek flash (4.210 tokens) con los 5 pares anclados sobre el fuente real; integración sin rescates.

**Verificación:** suite glass_editor 120/120 (2 tests nuevos del escaneo: stems recursivos con .GLB en mayúsculas, sin duplicados y ordenados; raíz inexistente → lista vacía); pipeline local exacto 5/5 a 0. **Test físico PASS por el camino real, con el flujo exacto del reporte:** escena de prueba con una entidad de modelo «cube» y mesh collider TriMesh → Inspector sin aviso → MeshForge sobre assets/models/cube.glb → Subdividir → Cerrar → «¿Quieres adaptar el mesh collider al nuevo modelo?» → «Sí, adaptar» → la entidad pasa a `cube_edited`, el horneado queda en assets/models/ y **el Inspector no muestra aviso alguno** con la misma entidad seleccionada todo el rato. **Control negativo:** con un modelo inexistente de verdad («no_existe_p11») el aviso SÍ aparece — el arreglo verifica contra disco, no silencia el aviso. (Las escenas y el horneado de la prueba se retiraron del árbol tras el test.)

**Consumo:** DeepSeek flash 4.210 · Sonnet 0 · Kimi 0 · Opus 0.

## ESPACIO DE HERRAMIENTAS / T1 — EL ESPACIO (2026-11-17)

**Pedido:** que las herramientas salgan de la pestaña del editor a un espacio de trabajo propio con barra lateral, tipo Blender (análisis previo en `fase1/diseno_espacio_herramientas.md`, partido en T1/T2/T3). **T1 = el espacio**: conmutador, puerta por el portón y retirada de la pestaña; el panel se reutiliza tal cual, a cuerpo completo. (T2 partirá su render en tres zonas; T3, el registro de herramientas, quedó como decisión suya pendiente.)

**Cambios** (`crates/glass_editor/src/docking.rs`, `crates/glass_app/src/main.rs`):
- Nuevo `EspacioTrabajo { Editor, Herramientas }` y campo `espacio` en `EditorDockingLayout`; `espacio_efectivo(porton)` devuelve Editor si el portón está apagado.
- En `render()`: el portón `plugin_enabled("herramienta-3d")` se lee EN VIVO cada frame; si se apaga estando en Herramientas, el espacio vuelve solo a Editor en ese frame. Conmutador en la barra de menús («Espacio: [🛠 Editor] [🧊 Herramientas]», la segunda solo con el portón activo).
- La barra de herramientas estilo Unity (gizmo, pivote, reproducción) solo se dibuja en el espacio Editor; el área central dibuja el dock en Editor y `mesh_tool.render(...)` a cuerpo completo en Herramientas. Todo lo demás del render (atajos, pendientes del Proyecto, adaptación del collider, diálogos) corre igual en los dos espacios.
- La pestaña deja de existir: fuera la variante `EditorTab::MeshTool` (enum, título y viewer), fuera de los presets y fuera del menú Window; eliminados el campo `mesh_tool_enabled`, su setter y la siembra del portón al construir la app en main.rs (la puerta MCP de las tools `glass_mesh_*` no cambia: se sigue evaluando al arrancar el servidor). El viewer pierde su referencia al panel (campo muerto cazado al compilar con `-D warnings`).
- Tests: fuera los dos de la pestaña/portón; nuevos `espacio_herramientas_exige_porton` y `cambiar_de_espacio_conserva_el_dock_del_editor`.

**Paquete:** DeepSeek flash (8.255 tokens: prompt 3.696 · completion 4.559 · razonamiento 1.018) con 18 pares anclados sobre el fuente real; los 18 anclaron ×1, sin rescates de pares (única corrección de Rayo: el campo muerto del viewer, consecuencia directa de retirar el brazo de la pestaña).

**Verificación:** suite glass_editor 120/120; pipeline local exacto 5/5 a 0 (workspace: 712 tests, 0 fallos). **Test físico PASS completo por el camino real** (Hub → Ejemplos → Escaparate): conmutador visible con los dos espacios; dock inferior SIN pestaña de MeshForge; menú Window sin su casilla; clic en «Herramientas» → el panel manda a cuerpo completo SIN la barra Unity, y su vista wireframe pasa a ocupar media pantalla; sesión real dentro del espacio (cube.glb abierto, Subdividir aplicado dos veces: 8 → 98 vértices); ida y vuelta Editor↔Herramientas con el dock intacto y **la sesión conservada** (98 vértices a la vuelta); **portón apagado en disco estando dentro → retorno automático a Editor y el botón «Herramientas» desaparece**; portón restaurado → el botón vuelve. (Nota de entorno: con lavapipe la app procesa la entrada con retraso; dos clics «perdidos» en la vista resultaron estar encolados y se aplicaron al alcanzar los frames — el panel respondía bien.)

**Consumo:** DeepSeek flash 8.255 · Sonnet 0 · Kimi 0 · Opus 0.

## ESPACIO DE HERRAMIENTAS / T2 — MESFORGE EN TRES ZONAS (2026-11-17)

**Alcance:** partir el render del panel de MeshForge en tres zonas dentro de su espacio, sin tocar su lógica ni su flujo diferido: barra lateral izquierda (identidad de la herramienta + sesión), vista wireframe central a todo el alto y operadores en columna derecha con scroll. Primer bloque con el **reparto en paralelo** (regla 2026-11-17 22:21): los tres paquetes corrieron a la vez, Opus con el mayor peso.

**Fase A (extracción mecánica, de Rayo con script):** el antiguo `render()` monolítico (~420 líneas) se partió en `zona_abrir` (fila de apertura), `zona_sesion` (nombre + Deshacer/Rehacer/Cerrar/Hornear), `zona_operadores` (Dimensiones + Operadores) y `zona_vista` (franja de selección/snap + vista previa con su picking), movidos verbatim; render quedó llamándolos en el mismo orden vertical. Dos ajustes de préstamos: el nombre de la sesión se clona y `receta_len`/`num_verts` se capturan en la cabecera (la etiqueta y «Mover todos» usaban la sesión tras soltarla), y las asignaciones diferidas dentro de las zonas desreferencian (`*action = ...`). Verificado: compila con `-D warnings` y la suite pasa idéntica (120/120).

**Fase B (paquetes en paralelo):**
- **Opus (15.209)**: el nuevo `render()` orquestador — foto de cabecera en ámbito corto (`stats()` exige `&mut`: nombre, estadísticas, receta y nº de vértices), etiqueta de estadísticas con el mismo formato y la pregunta del collider tal cual arriba a lo ancho, y las tres zonas en rects absolutos sobre el espacio restante (izquierda 208 px, derecha 264 px, hueco 6 px, líneas separadoras) con `Ui` hijas (`new_child`); la vista pasa a usar el alto disponible (fuera la constante `VIEWPORT_HEIGHT` de 320 px) y la etiqueta de estado sale del epílogo a la columna izquierda. 4 pares, 4 anclados ×1.
- **DeepSeek flash (2.938)**: los métodos nuevos `zona_identidad` (herramienta activa y «Más herramientas en camino») y `zona_sin_sesion` (aviso central 🧊 «Sin sesión»). 1 par, anclado ×1 (solo retoque mío: las tildes de tres textos).
- **Kimi k3.1-pro (24.329)**: auditoría con el archivo entero — Fase A fiel punto por punto (zonas, `num_verts`, orden del epílogo, overlay con clip absoluto) y dos avisos que resultaron ciertos en el test físico: el separador del epílogo se dibuja ahora en el cursor del padre (queda como divisor bajo la cabecera, aceptable) y, el importante, **las columnas recortan las filas horizontales anchas**.

**Correcciones de integración de Rayo (cazadas en el test físico, tal como avisó la auditoría):** en 208 px la fila de sesión dejaba «Cerrar» cortado y «Hornear» fuera; en 264 px quedaban cortados «Aplicar dimensiones», «Array», «Mover todos» y «Booleana». Arreglo: esas filas pasan a `horizontal_wrapped`, la apertura se apila (etiqueta / campo a lo ancho / botón) y el estado se dibuja con `Label::wrap()` forzado. El retraso de un frame del estado (se dibuja antes de aplicar las acciones del epílogo) queda aceptado: es el comportamiento habitual de un editor y no afecta a nada funcional.

**Verificación:** suite glass_editor 120/120; pipeline local exacto 5/5 a 0 (workspace: 712 tests, 0 fallos). **Test físico PASS completo por el camino real** (Hub → Ejemplos → Escaparate → espacio Herramientas): sin sesión, las tres zonas con la identidad a la izquierda, el aviso central y la derecha vacía; apertura apilada usable; con cube.glb abierto, cabecera de estadísticas a lo ancho, **todos** los botones de sesión y de operadores alcanzables tras las correcciones, la vista wireframe a todo el alto del centro y **Subdividir ejecutado desde su columna (8 → 26 vértices, receta 1)**.

**Consumo:** Opus 15.209 · Kimi k3.1-pro 24.329 · DeepSeek flash 2.938 · Sonnet 0.

## ESPACIO DE HERRAMIENTAS / T3 — REGISTRO DE HERRAMIENTAS (2026-11-17)

**Alcance:** la lista de herramientas del espacio deja de estar escrita a mano en el panel de MeshForge y sale de un registro con portón por herramienta, para que las futuras (vista de huesos/auto-rigging, visor de Ripper) nazcan ya dentro del espacio. Decisión de D3m3tr10: el espacio se completa con T1+T2+T3. Reparto en paralelo, como en T2.

**El registro** (`crates/glass_editor/src/herramientas.rs`, nuevo): `HerramientaInfo { id, nombre, icono, descripcion, plugin_id }`; `registro_herramientas()` declara las herramientas (hoy una: MeshForge, con portón del complemento «herramienta-3d»); `herramientas_disponibles()` filtra por el portón leído en vivo de disco (con su versión testeable `disponibles_con(porton)`); `resolver_activa(disponibles, pedida)` devuelve la pedida si está disponible, si no la primera, y None si no hay ninguna. 5 tests nuevos sobre la lógica pura.

**El cableado:** el espacio existe si hay **al menos una** herramienta disponible (con el registro de hoy el comportamiento es idéntico al de T1: apagar «herramienta-3d» deja la lista vacía y el retorno automático al Editor ocurre igual, verificado en el test físico). El layout guarda `herramienta_activa` y la resuelve contra el registro cada frame; antes de dibujar el panel le pasa la lista de disponibles y la activa, y después drena `peticion_herramienta` (el clic en la lista lateral). La zona de identidad de MeshForge genera su cabecera y su lista desde el registro que recibe; el despacho por herramienta vivirá en la rama del espacio cuando exista un segundo panel (apuntado por la auditoría y dejado indicado en el código).

**Paquetes en paralelo:** **Opus (17.855)** — el módulo del registro y el cableado en docking.rs, 7 pares ×1 más el archivo nuevo; **DeepSeek flash (4.245)** — la zona de identidad generada desde el registro (1 par ×1; sus otros 2 pares, los campos del panel, fallaron el ancla al suponer el tipo de un campo y los añadió Rayo a mano sobre el texto real); **Kimi k3.1-pro (40.754)** — auditoría: APROBADO sin defectos (equivalencia con T1 exacta, préstamos y drenaje correctos, coste de los clones despreciable, tests suficientes).

**Nota de integración:** la línea del filtro del registro costó tres formas hasta dejar contentos a rustc y a clippy a la vez: `map_or(true, |p| porton(p))` (dos lints) → `is_none_or(porton)` (mueve el portón dentro del cierre FnMut: E0507) → `is_none_or(|p| porton(p))` (cierre redundante) → **`is_none_or(&porton)`** (la referencia es Copy e implementa Fn: limpio).

**Verificación:** suite glass_editor 125/125; pipeline local exacto 5/5 a 0 (workspace: 717 tests, 0 fallos). **Test físico PASS completo por el camino real**: la identidad se dibuja desde el registro (cabecera, descripción, lista con la activa marcada y «Más herramientas en camino» con una sola herramienta); el clic en la fila de MeshForge ejercita la petición sin romper el espacio; **portón apagado en disco estando dentro → retorno automático al Editor y el botón «Herramientas» desaparece del conmutador; restaurado → el botón vuelve**.

**Consumo:** Opus 17.855 · Kimi k3.1-pro 40.754 · DeepSeek flash 4.245 · Sonnet 0.

## REJILLA DEL EDITOR ESTILO UNITY (2026-11-17)

**Pedido:** la vista de escena no tenía rejilla propia (auditoría B: lo que se veía como suelo era geometría de la escena). La de Unity es capa del editor: líneas menores cada 1 m, mayores cada 10 m, ejes X/Z en color y desvanecido con la distancia, solo en la vista de escena.

**Construcción (capa del editor, sin tocar el renderer):** la rejilla se dibuja con el painter de egui proyectando segmentos del plano XZ (y = 0) con la misma `view_proj` que los gizmos, debajo de todos ellos. En `gizmo_3d.rs`: `lineas_rejilla(centro, radio)` (pura, con tests) genera las líneas enteras del cuadrado que sigue al centro de la cámara del editor, clasificadas `Menor`/`Mayor`/`EjeX`/`EjeZ`; `draw_editor_grid(...)` las pinta con radio según la distancia de la cámara (2,5×, entre 10 y 100 m), desvanecido por distancia al centro y factor extra para las menores (desaparecen al alejar la cámara a 60 m); si un extremo queda detrás de la cámara, la línea se parte en trozos de 5 m y se pintan los que proyectan. En `viewport.rs`: campo `show_grid` (defecto visible) y botón conmutador «# Rejilla» en la barra de la vista de escena, junto a «Centrar». La vista de juego no la dibuja.

**Paquetes en paralelo:** **Opus (17.356)** — las dos funciones y los 3 tests, 2 pares ×1; **DeepSeek flash (5.177)** — campo, conmutador y llamada de dibujo (3 de 4 pares ×1; la inicialización del campo la añadió Rayo porque el ancla suponía otro valor inicial); **Kimi k3.1-pro (41.044)** — auditoría: apto sin defectos bloqueantes (proyección alineada con los gizmos, troceo sin líneas espurias, coste acotado; su única condición, acotar el desvanecido a [0, 1], ya estaba en el código).

**Verificación:** suite glass_editor 128/128; pipeline local exacto 5/5 a 0 (workspace: 720 tests, 0 fallos). **Test físico PASS por el camino real**: la rejilla se ve sobre el suelo del Escaparate alineada con la escena, con el eje X en rojo y el Z en azul cruzándose en el origen y el desvanecido hacia los bordes; el botón «# Rejilla» la apaga (suelo limpio) y la vuelve a encender.

**Consumo:** Opus 17.356 · Kimi k3.1-pro 41.044 · DeepSeek flash 5.177 · Sonnet 0.

## SEPARACIÓN HUB/MOTOR / S0 — BASES PURAS Y CORRECCIONES PREVIAS

Primero de los sub-bloques S0–S10 del diseño en `fase1/diseno_separacion.md` (v3 + análisis en paralelo; decisiones D1–D6 aprobadas tal cual el 2026-11-18). Sin cambio de comportamiento visible: deja el suelo seguro para la separación.

**En updates.rs:**
- **`HUB_VERSION`** junto a `ENGINE_VERSION`: la identidad propia del Hub; hoy coinciden, desde aquí solo cambia cuando cambie el Hub.
- **Registro del motor en archivo propio `engines.json`** (D1: solo lo escribe el Hub): `EngineStore { en_uso, conservada, pendientes_borrar }` con carga tolerante (ausente/ilegible → default) y **guardado atómico** (temporal `.tmp` + rename); rutas `engines_dir()`/`engines_dir_in(root)` y `motor_exe_in(root, versión)`.
- **`es_cambio_pesado(a, b)`** (difieren major o minor; si alguna no parsea → pesado: ante la duda se conserva) y **`plan_retirada(en_uso, conservada, nueva)`** pura, con las reglas de la política (ligero borra la anterior; pesado conserva la anterior y borra la conservada previa; reinstalar la en uso no toca nada) y dos salvaguardas añadidas por Opus: nunca propone borrar la versión que se está instalando ni la que queda conservada.
- **R1 corregido: `pick_asset` pierde el fallback de «un solo zip»** — instalaba el paquete de otra plataforma (en Windows, el zip de Linux). El test preexistente `pick_asset_prefiere_plataforma` afirmaba ese comportamiento defectuoso y se actualizó a la conducta nueva.
- **`pick_engine_asset` nueva y estricta**: zip con «-engine-» y el slug de la plataforma; sin fallback. El contrato de nombre (`GlassEnginer-Engine-<versión>-<slug>.zip`) lo producirá release.yml en S8; la selección genérica sigue sirviendo al flujo actual hasta que S3 lo sustituya, para no romper el canal publicado a mitad de bloque.
- **R2 corregido: la extracción restaura los permisos unix** de cada entrada (`unix_mode()` del zip) tras copiarla: un motor extraído en `engines/<v>` podrá lanzarse como proceso.
- **R4 (mitad): `LauncherStore::save` atómico** (temporal + rename): un corte a mitad de escritura ya no corrompe `launcher.json`.

**En main.rs:** `es_modo_hub(args)` y `proyecto_de_args(args)` puras junto a `is_standalone_launch`, parseadas en `main()` y aún sin uso (el comportamiento llega en S1/S6), con 3 tests de caracterización.

**Paquetes en paralelo:** **Opus (20.553)** — HUB_VERSION, rutas, EngineStore, clasificación/plan y sus 9 tests, 5 pares ×1; **cazó un error en los «hechos verificados» de mi propio prompt** (`tag_version` devuelve `Result<VersionKey, String>` y `core` es `[u64; 4]`, no lo que yo afirmé) y escribió contra la firma real. **DeepSeek flash (8.599)** — pick_asset sin fallback, pick_engine_asset, bit de ejecución, save atómico y las dos funciones de main.rs: 6 pares ×1 tras corregirle los marcadores (`NEW` por `NUEVO`) y una línea en blanco que rompía el parseo del integrador. **Kimi k3.1-pro: 429 (límite de la API) al lanzar su auditoría** — sin consumo; la auditoría de S0 la asumí yo en la integración (lectura de cada par contra el fuente) y queda registrada como desviación con su causa.

**Rescates de Rayo (declarados):** los tests de las correcciones de DeepSeek (no tenía las anclas literales y su borrador usaba constructores que no existen: `ReleaseInfo`/`AssetInfo` no derivan `Default`) los escribí yo con los constructores reales — regresión R1, tres de pick_engine_asset y el del bit de ejecución con un zip en memoria — y la actualización del test preexistente que afirmaba el fallback.

**Verificación:** suite glass_editor 142/142 (128 + 14 nuevos) · glass_app 69/69 (3 nuevos de argumentos) · **pipeline local exacto 5/5 a 0 (workspace: 737 tests, 0 fallos)** · **test físico PASS**: el binario de editor arrancado con `--hub` abre el Hub de siempre, idéntico (la bandera se reconoce y aún no cambia el comportamiento; el modo combinado intacto).

## SEPARACIÓN HUB/MOTOR / S1 — DOS RAÍCES Y APERTURA COMÚN

**Apertura común:** el cuerpo del brazo `LauncherAction::OpenProject` se extrajo a `GlassEngineApp::abrir_proyecto(&mut self, path: &Path)` (escena desde `project.glass` o por defecto, carga o escena vacía, raíz de assets del proyecto, vigía, registro con el nombre de `project.glass` — `nombre_proyecto` pasa a `pub` en glass_editor —, guardado del registro y entrada al editor). El brazo queda como una sola llamada. **La búsqueda de escena por defecto prueba `Assets/Scenes`, `assets/Scenes` y `assets/scenes`**: un proyecto con los assets en minúsculas abría con la escena vacía.

**`--project <dir>` funciona:** `main()` pasa `proyecto_directo` a `new()`, que **siembra `pending_launcher_action = OpenProject(dir)`** si la carpeta contiene `project.glass` (si no, error en el log y se ignora); la acción fluye por el aplazamiento de precarga de siempre y acaba en `abrir_proyecto`. En `resumed()`, con proyecto directo **no se construye la escena demo**. En el build sin editor, `--project` se ignora (documentado; su vía es `--scene`).

**Raíz del motor explícita:** `resolver_raiz_motor(exe_dir, cwd)` pura (el directorio del ejecutable si trae `managed/GlassEngine.Core` al lado — instalación gestionada —; si no, el directorio de trabajo — build de desarrollo) y `raiz_motor_actual()`. El campo `raiz_motor` lo usan desde ya: la inicialización del **ScriptHost** (`managed/…` bajo la raíz), las lecturas de **preparar_assets** (`assets/shader_graphs`, `assets/shaders`, `assets/models`) y las definiciones de **Input v2**. Quedan con ruta relativa al CWD, acotados para S6/S10: la recarga de grafos del vigía y las búsquedas por escena (materiales, prefabs, fuentes).

**Paquetes en paralelo:** **Opus (18.877)** — la extracción, la siembra, el gate de la demo y el cableado de `new()`/`main()`, 9 pares ×1; avisó en NOTAS de que mi texto del brazo venía cortado y dejó la cola (guardar + entrar) en el brazo: **completé la extracción yo** (declarado), junto con hacer `pub nombre_proyecto`, reponer la doc de `preparar_assets` que el ancla había desplazado y el arreglo del lint de clippy que cazó el pipeline (`to_path_buf` innecesario). **DeepSeek flash (5.718)** — `resolver_raiz_motor`/`raiz_motor_actual`, el re-enraizado del ScriptHost, de preparar_assets y de Input v2, y los 2 tests del resolver: 9 pares ×1 sin rescates. **Kimi: 429 otra vez** (`engine_overloaded`: su motor saturado, ajeno al helper), sin consumo — segunda auditoría consecutiva que no puede correr; la revisión la asumí yo en la integración y queda registrada la desviación.

**Verificación:** glass_app 71/71 · glass_editor 142/142 · **pipeline local exacto 5/5 a 0 (workspace: 739 tests)** — el primer paso de clippy cazó el lint citado y se re-verificó a 0 tras el arreglo. **Test físico PASS por partida doble**: `--project` sobre un proyecto con `Assets/Scenes` y escena sellada abre el editor directo en su escena (cubo rojo, jerarquía con sus 3 entidades) y lo registra con el nombre de `project.glass`; y sobre un proyecto con `assets/scenes` en minúsculas y sin campo `scene`, carga su escena (cubo verde) donde antes abría vacía. Sin escena demo en ninguno de los dos (línea de log nueva presente en ambos).

## SEPARACIÓN HUB/MOTOR / S2 — CREACIÓN SIN RUNTIME Y SELLO REAL

**La creación de proyectos es una función de disco:** `crear_proyecto_en_disco(name, path, template) -> Result<PathBuf, String>` resuelve la carpeta (sin doblar el nombre si la ruta ya lo trae), crea las 7 carpetas de siempre, serializa la escena de la plantilla desde un `SceneState` desechable y escribe `project.glass` **con el sello real del motor** (`ENGINE_VERSION`; desaparece el «GlassEngine 0.1.0» fijo). `create_project_and_open` queda en: crear en disco y `abrir_proyecto` (la vía común de S1). Si la creación falla, error en el log y no se abre nada (antes se continuaba con la escena en vivo). `project.glass` se escribe el último: es la marca de creación completada.

**Constructoras puras:** los bloques de `SceneState` de los `setup_*` se extrajeron (mecánicamente, por script) a `construir_escena_plataformas` / `construir_escena_fisica` / `construir_escena_vacia(state)`, que usan tanto los `setup_*` del editor (que añaden Rapier y el script del jugador encima) como la creación en disco. La paridad está verificada en el código: los pasos de runtime de los `setup_*` escriben en `physics_world` y en el ScriptHost, nunca en el `SceneState` que se guarda, así que el archivo de la plantilla es el mismo que producía el flujo en vivo.

**Sello de proyecto (en updates.rs):** `version_de_sello(meta)` (los sellos viejos «GlassEngine 0.1.0» cuentan como sin sello) y `sellar_proyecto(dir, version)`: reescribe el campo `engine` solo si el sello nuevo es mayor — **un sello nunca rebaja** —, preserva el resto del JSON y escribe atómico (temporal + renombrado). Lo usará S3 al migrar proyectos tras actualizar el motor. Auditoría de Kimi: válida; su defecto (en modo `--hub` el Hub debe registrar el proyecto al crearlo, no esperar a que el hijo lo abra) queda anotado como requisito de S6.

**Paquetes en paralelo:** **Opus (18.499)** — `crear_proyecto_en_disco` + la reducción de `create_project_and_open`, 2 pares ×1; avisó de que mi fragmento del VIEJO venía truncado y retiré yo el resto del cuerpo viejo; sus 3 tests de creación venían en NOTAS y los inserté ajustados a las APIs reales (`GlassEngineApp`, `load_from_json` como método). **DeepSeek flash (4.945)** — `version_de_sello`/`sellar_proyecto` + 2 tests, 2 pares ×1 sin rescates; la escritura atómica del sello la añadí yo tras la auditoría. **Kimi k3.1-pro (3.130)** — auditoría entregada esta vez: orden de escritura OK, formato del sello OK, paridad con la validación que quedó verificada en el código, y el defecto de registro que va a S6.

**Verificación:** glass_app 74/74 · glass_editor 144/144 · **pipeline local exacto 5/5 a 0 (workspace: 744 tests)** — el paso `--no-default-features` cazó que la extracción había desplazado los atributos `#[cfg]`/`#[cfg_attr]` de `setup_empty_scene` y `setup_physics_playground_scene` a las constructoras; repuestos en su sitio y re-verificado. **Test físico PASS**: desde el Hub, «Nuevo» → nombre S2Prueba, plantilla Plataformas 3D → el proyecto se crea en disco (árbol completo, `project.glass` con `"engine": "0.10.17.2-beta"`, escena con sus 6 entidades incluido «Player 3D Cube») y el editor abre en su escena por la vía común, con las plataformas y el jugador renderizados. Proyecto y registro de prueba retirados después.

## SEPARACIÓN HUB/MOTOR / S3 — INSTALACIÓN DEL MOTOR GESTIONADO

**Instalar una versión ya no es «descargar y registrar»: es dejarla en uso.** `instalar_motor_desde_zip(root, zip, version, proyectos)` extrae a `engines/.staging-<version>`, valida que el paquete trae el ejecutable en su raíz (un paquete sin `glass_app[.exe]` se rechaza sin tocar nada: ni staging, ni destino, ni el registro), renombra el staging a `engines/<version>` (todo dentro de `engines/`: mismo disco, renombrado atómico), aplica `plan_retirada` de S0 sobre el `EngineStore` —la anterior se borra si el cambio es ligero; si es pesado pasa a `conservada` y se borra la conservada anterior; lo que no se pueda borrar queda en `pendientes_borrar` y la instalación NO falla—, fija `en_uso` y sella los proyectos registrados con la versión nueva (`sellar_proyecto` de S2; cuenta los migrados). Devuelve un `ResumenInstalacionMotor` que la pestaña muestra: versión en uso, proyectos sellados, retiradas, conservada y diferidas.

**`install_release`** descarga en streaming a un temporal DENTRO de `engines/` (`.descarga-<versión>-<pid>.zip`), verifica el SHA-256 antes de extraer y delega en el núcleo. El mecanismo viejo (`versions/` + `store.installations`) deja de alimentarse; la estructura `Installation` queda solo por compatibilidad de lectura. `is_installed(tag)` pasa a mirar el ejecutable gestionado. Antes de cada instalación se barren los residuos de instalaciones interrumpidas (`.descarga-*` / `.staging-*`).

**Auditoría de Kimi, por puntos:** (1) atomicidad — el guardado de `engines.json` ya era atómico desde S0; la carpeta huérfana tras un corte entre renombrar y guardar es recuperable (la siguiente instalación de esa versión la sustituye) y su reconciliación al arrancar queda anotada para S10. (2) reinstalar la conservada — el plan ya la intercambia bien (pasa a en uso y la otra queda conservada, sin borrar ninguna); faltaba la prueba y se añadió. (3) borrado bloqueado en Windows → diferido persistido: OK tal cual; el lock de sesión para ni siquiera intentarlo llega en S4. (4) sellar con un editor abierto — ningún camino del editor mantiene `project.glass` en memoria ni lo reescribe al guardar (el renombrado lo relee fresco): no hay vector de actualización perdida hoy. (5) la limpieza de temporales entra en este paquete, no en S10: hecho (barrido al instalar).

**Paquetes en paralelo:** **Opus (20.083)** — núcleo + `install_release` + `is_installed` + 5 tests, 3 pares ×1 (P1/P2 fusionados por solape de anclas, avisado en NOTAS). **DeepSeek flash (2.365)** — la UI de la pestaña (tipo del mensaje, resumen en el mensaje de estado, texto al pie nuevo), 3 pares ×1 sin rescates. **Kimi k3.1-pro (3.505)** — auditoría de arriba. Rescates míos: el barrido de residuos con su test y el test de la conservada reinstalada.

**Verificación:** glass_editor 151/151 · **pipeline local exacto 5/5 a 0 (751 tests)** · **test físico PASS por la vía real completa**, con servidor local y raíz de datos aislada: Buscar actualizaciones ofrece v0.10.99 → Descargar e instalar → `engines/0.10.99/` con su payload, `engines.json` con `en_uso: 0.10.99` y el proyecto de prueba sellado de 0.9.0 a 0.10.99; segunda vuelta con v0.10.100 → la 0.10.99 **desaparece** (cambio ligero), `en_uso: 0.10.100` y el sello sube a 0.10.100. Sin residuos de staging ni de descarga.

## SEPARACIÓN HUB/MOTOR / S4 — LOCK DE SESIÓN, BORRADO DIFERIDO CON AVISO Y SALIDA ORDENADA DEL HIJO

**El Hub ya no borra a ciegas una versión que un editor está usando.** Cada motor gestionado toma al arrancar un lock compartido (`File::lock_shared`, std) sobre `engines/<versión>/.sesion.lock`, que vive en un guarda de `main()` hasta que el bucle de eventos vuelve (`bloquear_sesion_si_gestionado`: solo si el exe vive bajo `engines/<versión>/`). La sonda del Hub (`version_con_sesion_activa`) intenta el lock exclusivo sin esperar y es **fail-closed** (auditoría de Kimi: solo un `try_lock` con éxito significa «sin sesión»; WouldBlock o cualquier otro error difieren el borrado).

**Integración:** `instalar_motor_desde_zip` consulta la sonda antes de borrar cada versión del plan: con sesión viva la difiere sin tocar su carpeta (a `pendientes_borrar` y al resumen). `procesar_pendientes_borrar` completa los diferidos y lo llama `main()` al arrancar **solo desde el proceso del Hub** (ni hijos `--project` ni standalone: engines.json lo escribe el Hub, D1). La pestaña Instalaciones pinta las pendientes con su aviso («Se retirarán cuando cierres el motor que las está usando») — D5: el diferido nunca es silencioso.

**«Volver al Hub» en un editor hijo es salir, no cambiar de vista.** Los cuatro aterrizajes del flujo de vuelta (botón Hub sin suciedad, Guardar, No guardar, fin del Save As) y el cierre de ventana resuelven igual en ambos modos: en el combinado pasan a la vista Launcher; en el hijo marcan `salir_del_proceso` (o salen directos si la ventana se cierra limpia) y el bucle de eventos ejecuta la salida tras el frame, con la teardown normal — nunca un Hub dentro del hijo. La resolución vive como asignaciones de campos en el render (un método `&mut self` no compila ahí por los préstamos del renderer: rescate declarado).

**Auditoría de Kimi:** sonda fail-closed aplicada; vida del guarda OK tal como está (`run_app(...)?` propaga y main termina); salida tras el frame OK; queda como limitación conocida que dos procesos combinados simultáneos no se protegen entre sí (fuera del modelo soportado: un solo Hub; el combinado no corre desde engines/).

**Paquetes en paralelo:** **Opus (14.524)** — campo e init, 4 aterrizajes, CloseRequested, RedrawRequested, arranque de main() y aviso en la pestaña, 10 pares ×1 (el método `volver_al_hub` de su diseño no pasó por el integrador y los préstamos lo hicieron inviable: sustituido por la bifurcación en campos, declarado). **DeepSeek flash (9.786)** — la API de locks, la sonda en la retirada y `procesar_pendientes_borrar` ×1; sus 3 tests venían contra una firma inventada (`instalar_motor_desde_zip_en` de 3 argumentos) y con un `#[test]` duplicado que dejó sin atributo al test vecino: reescritos por Rayo contra la API real. **Kimi k3.1-pro (4.387)** — auditoría de arriba.

**Verificación:** glass_editor 154/154 (3 tests nuevos de lock: detección/liberación, pendientes con y sin sesión, instalación que difiere) · **pipeline local exacto 5/5 a 0 (754 tests)** · **test físico PASS en cuatro actos** con raíz aislada y un hijo real copiado a `engines/0.10.99/`: (1) el hijo arranca con `--project`, toma su lock (log + sonda flock externa: sesión activa) y abre el proyecto; (2) el Hub instala la 0.10.100 con el hijo vivo → la 0.10.99 queda en `pendientes_borrar` con su carpeta intacta y el aviso visible en la pestaña; (3) cerrar la ventana del hijo lo hace SALIR del todo («Cierre de ventana en el editor hijo: salida ordenada») y el lock se libera; (4) al rearrancar el Hub, el barrido completa el borrado («Borrados diferidos completados al arrancar: [0.10.99]») y la lista queda vacía.

## SEPARACIÓN HUB/MOTOR / S5 — HUB LIGERO EN --hub Y SONDA GPU CONTRA EL MOTOR GESTIONADO

**El Hub ya no arranca el runtime del editor.** `main()` pasa `es_hub` a `GlassEngineApp` (campo nuevo) y en ese modo: no se construye la escena demo (rama propia en `resumed`, con su log), no arranca el AssetWatcher inicial sobre `assets/`, el MCP en vivo no arranca ni con `--mcp-live`, y **el ScriptHost/CoreCLR no se inicializa** (la inicialización era incondicional en `resumed` si existía `managed/`: quedó tras la misma puerta; la afirmación de que era perezoso era falsa y la cazó el test físico, no el papel). La precarga de assets en builds de editor ya era diferida y no se tocó. El renderer Vulkan/egui sigue (es la presentación del propio Hub) y la física queda como mundo vacío. Deuda declarada: en S5 el Hub aún puede abrir un proyecto en su proceso desde la UI (con todo su runtime); S6 lo sustituye por procesos hijo. `--hub` + `--project` en la misma invocación se **rechaza en main()** con error claro (DEFECTO de la auditoría de Kimi: no basta con que S6 nunca lo produzca, el usuario puede escribirlo).

**La sonda de GPU sondea al motor gestionado, no al Hub.** `updates::objetivo_sonda()` devuelve (exe, versión) del motor en uso si su ejecutable está en disco; `comprobar_sonda_gpu` usa ese exe para el hijo `--gpu-probe` y esa versión como clave de la caché `gpu_probe.json` (comparación Y escritura: la escritura seguía con ENGINE_VERSION y la cazó también el test físico). Sin motor gestionado, se sondea el propio binario como hasta ahora. La rama `--gpu-probe` retorna en main() antes de la toma del lock de sesión (verificado por líneas), así que el hijo de la sonda nunca marca sesión: el reparo de la auditoría no aplicaba.

**Paquetes en paralelo:** **Opus (9.485)** — campo/parámetro/init, parseo y MCP en main(), puerta del vigía, rama sin-demo y la mitad de la sonda (9 pares ×1; dos sub-pares de la sonda quedaron fuera por fragmentos cortos y los aplicó Rayo). **DeepSeek flash (2.107)** — `objetivo_sonda_en/objetivo_sonda` (su ancla de inserción coincidía ×3 y el integrador lo saltó: insertado por Rayo) y los 3 tests, que llegaron con el vicio ya visto (#[test] duplicado que dejó sin atributo al test vecino): reescritos por Rayo. **Kimi k3.1-pro (3.448)** — auditoría: el DEFECTO --hub+--project aplicado; el resto, OK o no aplicable con evidencia.

**Verificación:** glass_editor 157/157 (+3 de objetivo_sonda) · **pipeline local exacto 5/5 a 0 (757 tests), corrido dos veces (antes y después de los rescates)** · **test físico PASS** con raíz aislada y motores reales copiados a engines/: arranque `--hub` con log «sin escena demo ni runtime de editor», **cero líneas CoreCLR**, sin AssetWatcher, y la sonda ejecutada contra el gestionado con la caché escrita como `"motor": "0.10.100"`; segundo arranque con la sonda servida de la caché («resultado en caché correcto; no se repite»); al cambiar el motor en uso a 0.10.101 la sonda se repite y la caché pasa a 0.10.101; `--hub --project` sale con el error de incompatibilidad sin abrir nada.

## SEPARACIÓN HUB/MOTOR / S6 — EL HUB LANZA EL MOTOR COMO PROCESO HIJO

**El Hub ya no abre nada en su propio proceso.** Con `es_hub`, las cuatro acciones del Launcher (abrir proyecto, crear proyecto, ejemplo, escena suelta) se interceptan antes de la lógica de precarga y se ejecutan con `ejecutar_accion_hub`: todo se lanza como proceso del motor gestionado en uso (`lanzar_motor_hijo` sobre `objetivo_sonda()`), con `--project <dir absoluto>`, creando antes en disco en el caso de crear (`crear_proyecto_en_disco`, sin runtime) o con `--sample <clave>` en los ejemplos (D6; el receptor siembra la misma acción `LoadSampleScene` en el hijo). Las rutas se absolutizan contra el CWD del Hub. Dos correcciones de la auditoría de Kimi aplicadas al integrar: (1) **el Hub no registra la apertura** — el registro de proyectos de `launcher.json` tiene un único escritor, el hijo en su `abrir_proyecto`; si el Hub también registrara al lanzar, los dos guardados se pisarían y quedaría una apertura falsa si el spawn falla; (2) **el hijo no hereda los `GLASS_*` de prueba/desarrollo** del entorno del Hub (p. ej. `GLASS_UPDATE_API`): se retiran todos menos `GLASS_LAUNCHER_HOME`. Un hilo recoge al hijo al salir para no dejar zombis. El punto 4 de la auditoría (parser de versiones contra «0.0.0» y sufijos `-beta`) quedó verificado sin cambios: es el parser ya en uso contra tags reales.

**Dos versiones a la vista.** El pie del Launcher en modo Hub muestra «Hub v<HUB_VERSION>» y «Motor v<en uso>» (o «Sin motor instalado») en dos líneas — en una sola se cortaba contra el borde de la barra lateral, cazado en el test físico — y la cabecera de crear proyecto se identifica como Hub. La pestaña Instalaciones se parte en dos secciones: **Motor** (el gestionado en uso; la conservada por cambio pesado con su botón Borrar, que no difiere si tiene sesión — devuelve error —; los restos del mecanismo viejo en `versions/` con su botón Borrar restos; los pendientes de S4 se quedan donde estaban) y **Hub** (su versión y la nota de que su comprobación llega en S9). La comprobación de novedades del motor se hace contra la versión gestionada (`check_update_against`, con `check_update` como envoltorio sobre ENGINE_VERSION): base = en uso, o «0.0.0» sin motor, y los textos dicen «esta copia es v<en uso>» o «aún no hay motor instalado». Sin motor, Proyectos muestra el aviso y deshabilita Nuevo/Abrir/tarjetas, y Ejemplos muestra solo el aviso. «Complementos usando el motor en uso» queda diferido a S7, que es su sub-bloque.

**Paquetes en paralelo:** **Opus (36.182)** — campo `es_hub` y helpers en el panel, pie de dos versiones, cabecera Hub, Instalaciones en dos secciones, comprobación contra la gestionada, interceptación en main (18 pares esperados; M3/M4 y la sección Hub llegaron como código suelto en sus notas y los insertó Rayo con las correcciones de Kimi). **DeepSeek flash (6.033)** — `check_update_against`, `borrar_conservada_en`, `restos_versions_en`, `borrar_versions_en` y 4 tests, otra vez con el vicio del `#[test]` duplicado: reescritos por Rayo contra la API real. **Kimi k3.1-pro (4.304)** — auditoría con veredicto NO APTO por la carrera del registro y el riesgo del entorno: ambos aplicados; puntos 3 y 5 OK, punto 4 verificado.

**Verificación:** glass_editor 161/161 (+4) · glass_app 74/74 · **pipeline local exacto 5/5 a 0 (761 tests)** · **test físico PASS** con raíz aislada y el binario real copiado a `engines/0.10.100`: doble clic en la tarjeta → hijo del gestionado con `--project`, su lock de sesión tomado y «Proyecto abierto» en su log, con el Hub vivo en su vista; Ejemplos → hijo con `--sample fisica`; Crear proyecto desde el Hub → creado en disco (`project.glass`) y hijo lanzado, registrado por el propio hijo; Instalaciones con las dos secciones y la comprobación contra la gestionada («Estás en la última versión publicada (v0.10.100)», no contra la versión del binario); Borrar restos y Borrar conservada a clic, verificados en disco y en `engines.json`; y con una segunda raíz sin motor: aviso y acciones deshabilitadas en Proyectos, solo el aviso en Ejemplos, aviso en Instalaciones y la comprobación con base «0.0.0» ofreciendo «v0.10.100 (aún no hay motor instalado)». La VM se reemplazó a mitad del test (procesos y Xvfb/xdotool perdidos): se reinstaló el tooling y se repitieron las comprobaciones restantes sobre la misma fixture.

## Separación S7 — complementos contra el motor en uso (2026-11-18)

- **D3 hecho código**: el juicio de compatibilidad de complementos ya no usa
  una constante global: `engine_incompatibility`/`engine_compatible`/
  `check_engine`/`install_plugin`/`install_plugin_from_bytes` reciben la
  **versión explícita** contra la que juzgar. En el Hub, la pestaña
  Complementos juzga contra el **motor gestionado en uso** (`engines.json`);
  en modo combinado, contra el propio binario. Sin motor instalado: aviso
  «⚠ Sin motor instalado: instala el motor para usar complementos.», todo
  complemento marcado incompatible («sin motor instalado») y Activar
  bloqueado — Desactivar sigue permitido. La instalación también juzga
  contra esa versión (la del Hub en modo Hub).
- **Puerta de activación versionada** `plugin_enabled_for(id, version)`:
  lee el manifiesto de la instalación en disco y exige compatibilidad con
  la versión dada. Las tres puertas de MeshForge la usan ya con la versión
  del proceso (MCP en vivo, MCP stdio y `herramientas_disponibles`).
  Semántica de borde (auditoría Kimi aplicada): sin manifiesto en la
  carpeta (instalación heredada) no bloquea; manifiesto presente pero
  corrupto SÍ bloquea (metadatos no fiables). La semántica de instalación
  se mantiene de rechazo: un incompatible nunca llega a instalarse porque
  `check_engine` corre antes de persistir.
- **Test físico PASS** por la vía real del MCP stdio (`--mcp`,
  tools/list) con raíz aislada y la herramienta-3d registrada y activada:
  manifiesto con `min_engine 99.0.0` → 50 tools, **0 glass_mesh_***;
  manifiesto corrupto → 0; sin manifiesto → 57 tools, **7 glass_mesh_***;
  manifiesto compatible → 7. La puerta responde a la versión tal como se
  pidió (D3: incompatible, no se activa).
- Pipeline local exacto: 5/5 a 0, **764 tests** (glass_editor 164).
  Consumo: **Opus 9.285 · DeepSeek flash 14.725 · Kimi k3.1-pro 2.987**.

## Separación S8 — releases e instalador (2026-11-18)

- **release.yml**: dos familias de tags — motor `v*` y Hub `hub-v*` (la
  versión se desnuda quitando el prefijo que toque, en pwsh, bash y en el
  publish). Cada job monta **dos stagings** con el contenido del payload de
  siempre: `dist/hub` (setup, AppImage y zip del Launcher) y `dist/engine`
  (payload del motor gestionado). Nuevos artefactos: los zips del motor
  `GlassEnginer-Engine-<versión>-windows-x64.zip` y `…-linux-x64.zip` con el
  payload **en la raíz** (contrato exacto de `pick_engine_asset`; ambos jobs
  verifican que el ejecutable queda en la raíz del zip). El zip del Launcher
  de Windows sigue sin existir: manda el setup.
- **Publicación separada por familia** (contaminación cazada en la
  auditoría): un release del Hub publica SOLO el setup y el AppImage — los
  zips del motor llevarían la versión del Hub y el selector del motor
  podría confundirlos con un motor de verdad — y su setup se compila **sin
  semilla** (la carpeta sembrada llevaría también la versión equivocada).
  El release del motor publica todo, como hasta ahora más los dos zips.
  `checksums.txt` cubre exactamente lo publicado en cada familia.
- **GlassEnginer.iss**: define opcional `EngineSeedDir`; con él, el setup
  copia el payload del motor a `%USERPROFILE%\GlassEnginer\engines\<versión>\`
  (la `data_root()` real de Windows, vía `GetEnv('USERPROFILE')`) con
  `uninsneveruninstall` (desinstalar el Hub no toca la zona de datos) y
  escribe `engines.json` con `{"en_uso": "<versión>"}` SOLO si no existe
  (una reinstalación no cambia el motor en uso), avisando con un error
  visible si la escritura falla. Sin el define, compila y se comporta como
  siempre.
- **Verificación**: YAML validado; los pasos de zip de Linux ejecutados tal
  cual contra un payload de prueba — ejecutable en la raíz y nombre que
  `pick_engine_asset` acepta para Linux y rechaza para la otra plataforma.
  **Pendiente declarado** (como fijaba el diseño): la compilación de ISCC
  con los parámetros nuevos y la instalación real en Windows se verifican
  en el job de Windows de la primera release que se lance con este
  workflow; el comportamiento de `workflow_dispatch` (compila el ref desde
  el que se lanza y etiqueta ese commit) es el histórico de todas las
  releases y no se ha cambiado.
- Consumo: **Opus 19.436 · DeepSeek flash 7.966 · Kimi k3.1-pro 9.714**.
  Nota: la reescritura completa de Opus no se pudo usar tal cual (su
  prompt no incluía el release.yml real y lo reconstruyó con rutas y
  comandos inventados); aportó la resolución por familias y el aviso de
  contaminación entre familias, y la integración final se hizo a mano
  sobre el archivo real. La auditoría de Kimi (NO APTO sobre el borrador)
  dejó dos correcciones aplicadas: sin semilla en releases del Hub y el
  fallo al escribir `engines.json` ya no es silencioso.

### Separación S9 — actualización del propio Hub (2026-11-18)

Diseño §S9 con D2: el Hub se comprueba a mano, se ofrece y SOLO se descarga con el clic del usuario; nada automático en ningún paso. Reparto: DeepSeek flash (selectores del Hub en updates.rs), Opus (aplicación + UI + cableado), Kimi (auditoría del diff), Rayo integra y corrige.

- **Selectores (DeepSeek, con dos correcciones de Rayo)**: `is_hub_tag`/`hub_tag_version` (desnudan `hub-v`), `select_latest_hub_release` (sin borradores, prerelease sí), `pick_hub_asset`. La `fetch_latest_hub` de DeepSeek llegó con la API de ureq 2 inventada y su `check_hub_update` usaba `is_newer` con VersionKey (firma incompatible: `is_newer` parsea tags y con `hub-v` devolvería siempre «al día»): ambas reescritas por Rayo — fetch espejo del real (ureq 3) y comparación directa entre VersionKey. Su tercer test llegó truncado (el razonamiento se comió el presupuesto) y lo completó Rayo; el helper de releases de los tests fijos no aceptaba tag y hubo que crear `rel_con_tag`.
- **Aplicación (Opus)**: `AccionHub::{Relanzar, LanzarInstalador}`; `descargar_actualizacion_hub` (pick_hub_asset + descarga a `data_root()/hub-update/` + SHA-256 obligatorio: sin hash o sin coincidencia no se instala nada); `aplicar_actualizacion_hub` (Windows → instalador; Linux → destino = env `APPIMAGE` o current_exe, renombrar a `.viejo`, copiar con 0o755, restaurar el anterior si la copia falla); `limpiar_respaldo_hub` al arrancar el Hub. Sus campos supuestos (`release.tag`, `asset.url`, sha2 propio) se corrigieron a los reales (`tag_name`, `browser_download_url`, `sha256_hex` del módulo).
- **UI y cableado (Opus + Rayo)**: sección Hub de Instalaciones con máquina propia (`hub_update_state`/`hub_update_rx`/`HubUpdateMsg`): «Buscar actualizaciones del Hub» → oferta «Hay una versión nueva del Hub: <tag>.» → «Descargar e instalar» (la descarga empieza SOLO ahí). El panel deja `accion_hub_pendiente` y main.rs la consume al pintar el frame (patrón `salir_del_proceso`): lanza el Hub nuevo con `--hub --tras-actualizar-hub` o el instalador, y sale; si el lanzamiento falla, no sale y el error se ve en la sección. El diseño de Opus encadenaba un método sobre el campo equivocado (`pending_launcher_action` es del app, no del panel): el consumo lo escribió Rayo contra el código real. El flag `--tras-actualizar-hub` llega al panel por un AtomicBool que su primer `tick_hub_update` consume y convierte en splash.
- **Auditoría de Kimi: APTO CON CORRECCIONES (2, aplicadas)**: (1) `pick_hub_asset` demasiado permisivo → selección estricta: nombre con el slug + «launcher» (+ «setup» y .exe en Windows, .AppImage en Linux) y candidato ÚNICO, si hay varios no se instala nada. (2) El splash apagaba `mostrar_carga` al expirar y podía matar una carga del motor concurrente → estados separados: el splash vive en `splash_hub_hasta`/`texto_carga`, la visibilidad de la pantalla es `mostrar_carga || splash activo` y al expirar solo se limpia el estado propio.
- **Verificación**: pipeline local 5/5, **771 tests**. **Test físico PASS por la vía real**: stub HTTP (puerto 8798) con lista mezclada (v0.10.99 del motor, hub-v99.0.0, hub-v98.0.0 y un tag de complemento) y el binario real corriendo como «AppImage» (env APPIMAGE) en `--hub` con raíz aislada: clic en buscar → oferta exacta de hub-v99.0.0 (selector correcto entre familias) y NADA descargado antes del clic (D2); clic en «Descargar e instalar» → el AppImage quedó reemplazado por el paquete descargado (script marcador, modo 755), el anterior preservado en `.viejo` (513 MB), el temporal de descarga borrado y el Hub nuevo lanzado con los args exactos `--hub --tras-actualizar-hub` (marcador en disco) mientras el proceso viejo salía. Segundo acto: un `.viejo` obsoleto junto a otro AppImage desapareció al arrancar ese Hub (limpieza verificada en disco). **Pendiente declarado**: el splash de 2 s no se pudo fotografiar en esta VM (los 2 s transcurren durante el calentamiento del primer frame con lavapipe, antes de que la ventana presente; en hardware real los frames se presentan al instante) y la prueba del ejecutable bloqueado en Windows queda para la primera release `hub-v*` real.
- **Consumo S9: Opus 59.227 · DeepSeek flash 17.818 · Kimi 12.793.**

## Separación S10 — regresión integral y endurecimiento (CIERRE de la separación)

- **Reconciliación de huérfanas (DeepSeek)**: `reconciliar_huerfanas_en`/`reconciliar_huerfanas` en updates.rs — al arrancar el Hub, las carpetas de `engines/` con nombre de versión que el store no conoce (ni en uso, ni conservada, ni ya pendientes) pasan a `pendientes_borrar` persistido; no borra nada (el borrado sigue siendo de `procesar_pendientes_borrar`, que respeta sesiones). Carpetas ajenas, archivos y residuos `.descarga-*`/`.staging-*` quedan fuera. Es la deuda anotada en S3 (corte entre el renombrado y el guardado del store).
- **Contrato de argumentos y entorno (Opus)**: `validar_argumentos` pura en main.rs — `--project`/`--scene`/`--sample` exigen valor (error que nombra el flag); `--hub` incompatible con los tres; `--project`+`--scene` incompatible. Sustituye al chequeo puntual --hub/--project de S5. En el arranque del Hub: reconciliar ANTES de procesar. El motor hijo ya no hereda `APPIMAGE` (apuntaba al ejecutable del Hub).
- **Auditoría de Kimi: APTO CON CORRECCIONES (6 aplicadas, 2 obligatorias)**: (1) **rollback peligroso**: una lista de pendientes vieja podía nombrar a la versión que volvió a estar en uso → purga de en_uso/conservada en reconciliar Y en procesar, con tests del escenario y de la integración reconciliar→procesar con sesión viva. (2) La guarda del arranque no incluía `es_hub` y el modo combinado también reconciliaba/procesaba, contra D1 y contra la doc → guarda corregida. (3) Un valor que es otro flag cuenta como valor ausente y se validan TODAS las apariciones del flag. (4) `APPDIR` también se retira del entorno del hijo. (5) Si el guardado tras reconciliar falla, se registra `log::warn!` en vez de un rastro falso. (6) El riesgo aceptado de la ventana de instalación interrumpida queda documentado en la doc.
- **Doc (Rayo)**: `docs/HUB_Y_MOTOR.md` — los dos modos (separado y combinado de desarrollo), el contrato de argumentos, locks, diferidos, reconciliación y la regla de que nada se actualiza solo.
- **BUG 1 cazado por la regresión — el guardado del editor escribía en el árbol equivocado**: File → Save Scene / Ctrl+S (`save_scene_to` en docking.rs) resolvía la ruta por la clave VFS del SceneManager (anclado al árbol descubierto desde el CWD) e ignoraba la ruta absoluta registrada al cargar (`ruta_escena_actual`, que sí usaban el autoguardado y el diálogo de salida). Físicamente: editar un proyecto en el hijo y guardar escribía en `assets/scenes/SampleScene.scene.json` DEL MOTOR y dejaba el archivo del proyecto intacto. **Arreglo (Opus)**: el layout recibe cada frame el espejo de la ruta y el nombre registrados, y el guardado simple escribe en la ruta absoluta si la escena activa sigue siendo esa (helper puro `ruta_guardado_directa` con tests); Save As no cambia.
- **BUG 2 cazado por la regresión — Build & Run relativo al CWD y lanzamiento imposible**: `execute_build_and_run` escribía `project.json` y la escena del build en el árbol del CWD (en el hijo, el del Hub), copiaba los assets y el managed del CWD, y lanzaba el paquete con ruta relativa + `current_dir`, que Rust resuelve contra el directorio nuevo → «No such file or directory» en cualquier modo. **Arreglo (Opus)**: la app registra la raíz del proyecto abierto (`raiz_proyecto_actual`, fijada en `abrir_proyecto` y limpiada al cargar un ejemplo); el build se construye sobre ella (sin proyecto, el CWD de siempre): assets desde la raíz del proyecto con su convención, escena y `project.json` nacen dentro del paquete, managed desde `raiz_motor_actual()` (el payload de este proceso) y lanzamiento con rutas absolutas.
- **Verificación**: pipeline local 5/5, **781 tests**. **REGRESIÓN FÍSICA COMPLETA PASS (10 actos, raíz aislada + stub de releases en el 8797)**: (1) carpeta huérfana 0.10.50 reconciliada y borrada al arrancar el Hub; (2) crear proyecto desde el Hub → hijo lanzado con el exe gestionado 0.10.99 y `--project` absoluto; (3) en el hijo, borrar «Static Floor» y Save Scene → el archivo DEL PROYECTO pasa a 5 entidades y el del repo queda intacto (tras el arreglo del bug 1; antes escribía en el repo); (4) cerrar la ventana del hijo → el proceso sale y el Hub sigue; (5) instalar v0.11.0 (pesado) con el editor cerrado → en uso 0.11.0, conservada 0.10.99 y sello del proyecto a 0.11.0; (6) reabrir el proyecto → hijo con el exe de engines/0.11.0; (7) instalar v0.11.1 (ligero) con el editor de 0.11.0 ABIERTO → en uso 0.11.1 y 0.11.0 en pendientes_borrar con el aviso D5 visible («Se retirarán cuando cierres el motor que las está usando»); (8) cerrar el hijo y rearrancar el Hub → 0.11.0 borrada en el arranque; (9) «Borrar» de la conservada en Instalaciones → carpeta fuera y conservada a null; (10) Build & Run desde el hijo → paquete en `<proyecto>/build/standalone` (binario copiado del exe del motor, assets del proyecto, managed del payload, escena editada con 5 entidades) y proceso standalone lanzado desde el paquete (tras el arreglo del bug 2; antes contaminaba el CWD y el spawn moría). La VM se reemplazó a mitad de la regresión (se perdieron Xvfb/xdotool/lavapipe y los procesos); el entorno se restauró y la regresión se completó entera. Sin pendientes declarados.
- **Consumo S10: Opus 35.847 (8.743 base + 12.174 arreglo guardado + 14.930 arreglo Build & Run) · DeepSeek flash 7.306 · Kimi 8.899.**

## Ronda post-release v0.10.18.1-beta — modelos que no cargaban, rejilla 3D, miniaturas, fuentes y barra única (2026-11-18)

Seis puntos reportados por D3m3tr10 tras probar la release publicada; un solo cierre. Reparto en paralelo: Opus (rejilla y rasterizador de miniaturas, el mayor peso), DeepSeek flash (función de rejilla troceada, cableado de miniaturas, tests del bridge), Kimi k3.1-pro (tres auditorías), Rayo integra, corrige y verifica físicamente.

- **1. REGRESIÓN PRIORIDAD 1 — .blend/.fbx arrastrados quedaban en cubo en la build separada.** Reproducida por el camino de la release (instalación falsa sin `tools/`, hijo desde CWD neutro): el log del hijo daba la causa — «Bridge de Blender no encontrado en 'tools/headless_blender_bridge.py'». **Causa raíz doble**: (a) `BRIDGE_PATH` solo se fijaba desde el padre de una carpeta vigilada que contuviera el bridge; en la build instalada ninguna carpeta vigilada lo tiene y se caía al fallback relativo contra el CWD del hijo; (b) el payload de la release ni siquiera incluía `tools/` (release.yml montaba solo binario + managed + assets en ambos payloads). La prueba del .blend de la regresión de S10 no repitió el arrastre en la build separada: esa es la causa de proceso asumida. **Arreglo**: `ruta_bridge_en_raiz`/`fijar_bridge_desde_raiz` puros en watcher.rs (fijan el OnceLock desde una raíz si el archivo existe), `AssetWatcher::new` los reutiliza, y main.rs fija el bridge desde `raiz_motor_actual()` al arrancar todo proceso no-Hub, antes del vigía inicial; release.yml copia `tools/headless_blender_bridge.py` a los payloads de Windows y Linux (motor y Hub; el .iss copia el payload recursivo). Tests de la función pura de DeepSeek (4: sin tools, tools vacía, con archivo, directorio con ese nombre).
- **2. Rejilla sobre los modelos → rejilla 3D con oclusión.** La rejilla se pintaba con el Painter 2D de egui sobre la textura del viewport, sin test de profundidad. Ahora vive en el pase 3D de la vista de escena: pipeline LINE_LIST propio (`editor_line.vert/.frag`, patrón de las partículas de la Fase 8P), depth test LESS_OR_EQUAL sin escritura de profundidad, dibujo gateado por vista (escena sí; juego y portales no). La geometría se trocea en tramos de 2 m (sin troceo la atenuación por distancia dejaba la rejilla vacía). Auditoría de Kimi cazó el orden de publicación del setter de buffers (commit-on-success aplicado).
- **3. UI — fuera la barra de ayuda del gizmo y la fila de creación + «Listo» del panel Proyecto.** Retirados ambos elementos. Limpieza: `ejecutar_creacion` y `nuevo_nombre` eliminados; la creación queda solo por el menú contextual «Crear ▸». **Corrección de Kimi aplicada**: al retirar la línea de estado, los errores del panel quedaban invisibles → `set_info`/`set_error` ahora publican también en el log, y los mensajes viven en la Consola (verificado en el log del hijo: «Proyecto: Creado: …»).
- **4. Deslizador «Tamaño:» + miniaturas de modelo.** El deslizador funcionaba; el defecto era que agrandar solo agrandaba un emoji genérico. **Rasterizador por software (Opus)**: `miniatura_modelo.rs` — vista 3/4 ortográfica (azimut −35°, elevación 22°), z-buffer, sombreado plano con luz fija, color por primitiva/material (arcilla si no hay material), fondo transparente, 192×192, con sus tests. **Cableado (DeepSeek, en dos paquetes pequeños tras un intento único que quemó 14.036 tokens en razonamiento sin contenido)**: la fuente es el .glb directo o el .glb hermano de un .blend/.fbx; sin fuente o con render vacío, emoji de respaldo. **Auditoría de Kimi: APTO CON CORRECCIONES (3 aplicadas)**: (1) `.gltf` fuera de la resolución (el cargador solo acepta GLB binario); (2) presupuesto de 2 rasterizaciones por frame en la rejilla; (3) invalidación de la caché por mtime del .glb — la miniatura de un .blend/.fbx aparece sola cuando la conversión crea el .glb.
- **5. Iconos como cuadrados vacíos en Windows.** Causa raíz: `setup_editor_fonts` solo buscaba fuentes en rutas de sistema Linux; en Windows ninguna existe y egui caía a sus fuentes por defecto (tofu). **Arreglo**: Noto Emoji (monocroma) y Noto Sans Symbols 2 se incrustan con `include_bytes!` (`crates/glass_editor/assets/fonts/`, licencia OFL incluida) y cierran SIEMPRE la cadena de respaldo de Proportional y Monospace, tras las cargas de sistema. Cobertura verificada con fontTools sobre los glifos que usa la UI. Auditoría de Kimi: la variable es segura con ab_glyph (instancia por defecto) y el orden se corrigió para que las incrustadas queden tras Meslo, siempre al final.
- **6. Barra de la vista de escena duplicada.** Los 6 botones de herramienta duplicaban los de la barra principal: fila entera retirada de la vista (la escena gana la franja) y «⌖ Centrar» + «# Rejilla» movidos a la barra principal, actuando sobre el mismo `viewport` que consume el render (confirmado por Kimi). Los atajos Q/W/E/R/T/Y siguen en la vista.
- **Verificación**: pipeline local exacto 5/5 sobre el árbol final. **Test físico integral por el camino de la release separada** (instalación falsa actualizada con el binario nuevo y `tools/` dentro de `engines/0.10.99/`, hijo desde CWD neutro): (a) arrastrar `campfire_voxel_v3.blend` → «Conversión headless exitosa» en ~20 s, .glb hermano en disco y la entidad con la forma real del modelo (losa ancha, no el cubo), ocluyendo la rejilla; (b) arrastrar `xbot_walking.fbx` → conversión en ~8 s y el X Bot de pie en la escena; (c) arrastrar `serpiente.glb` → modelo directo sin conversión; (d) rejilla solo en la vista de escena (la de juego sin rejilla), «# Rejilla» de la barra principal apaga/enciende, «Centrar» de la barra principal restaura la cámara tras orbitar, y la vista de escena ya no tiene barra propia; (e) iconos reales en barra principal, paneles y menús (🏠 ✋ ↻ ▶ ⏸ 🗑 🧩 🎬…), sin un solo cuadrado — en la VM, que tampoco tiene fuentes de emoji del sistema: las fuentes viajan con la app; (f) panel Proyecto sin fila de creación ni «Listo», creación contextual verificada en los 6 tipos (Carpeta, Escena, Material, Prefab vacío, Grafo — que copia el primer .graph.json de `shader_graphs/`, requisito preexistente — y Script C#), deslizador de mínimo (lista) a máximo con miniaturas reales: imagen (checker), material (oro), modelo .glb (serpiente), y .blend/.fbx que pasan del emoji de respaldo a su modelo al convertirse (campfire verde con su fuego; X Bot rojo en T). El renombrado no se tocó en esta ronda (código idéntico, dentro del alcance de ambas auditorías sin hallazgos); su editor en línea no se pudo operar con clics sintéticos en Xvfb por el foco, sin relación con la ronda. Los modelos se ven oscuros en la escena de prueba porque está vacía (sin luces): es la iluminación de la escena, no la carga — la forma y las miniaturas con color lo confirman.
- **Consumo de la ronda: Opus 48.980 · DeepSeek flash 36.775 (incluye 14.036 de un paquete fallido por longitud, sin contenido) · Kimi k3.1-pro 60.937. Total ≈ 146.692.**
## Ronda de 16 puntos post-release — CIERRE COMPLETO (2026-11-18)

Segunda mitad de la ronda (puntos 2, 3, 5, 7, 8, 9, 10, 15, 16) más los hallazgos del barrido de regresión que la regla anti-regresión («no es arreglar algo y dañar otra cosa») hizo correr sobre los flujos viejos. Reparto en paralelo: Opus (Inspector RB/Collider, anillo de rotación, recarga bajo demanda — el mayor peso), DeepSeek flash (placa de nombre, caja del cubo, X de pestañas, arreglo de masa), Kimi k3.1-pro (paquete Gizmos, re-anclaje de escenas, selector, cuatro auditorías), Rayo integra, corrige y verifica físicamente.

- **Punto 3 — la caja de selección no abrazaba al cubo primitivo.** Causa raíz: la malla cruda de `assets/models/cube.glb` mide 2 m y el render la dibuja con media escala (el cubo a escala 1 mide 1 m); la selección usaba las cotas reales sin esa media escala y la caja salía el doble. Arreglo en `entity_local_bounds`: si el modelo no se nombra (primitiva), las cotas del mapa «cube» se reducen a la mitad; si se nombra, van tal cual. Cubre selección, picking y punto de aterrizaje. Verificado en físico: caja exacta de 1 m sobre el cubo y cajas exactas en los modelos importados.
- **Punto 5 — Rigidbody y Collider como componentes separados (diseño con fuentes de Unity).** `PhysicsData` gana `has_rigidbody`/`has_collider` (serde default true: las escenas viejas no cambian). Sin RB el cuerpo se fuerza estático; sin collider no se crean formas ni malla de colisión; el Inspector muestra dos cabeceras con Quitar/Añadir independientes, y quitar el último elimina la física. El CharacterController no se toca (su cápsula es suya). Refinamiento de la regla de física vigente: un Collider sin Rigidbody es un cuerpo estático (como en Unity); sin ninguno de los dos no hay física. Verificado en físico con escena dedicada: cubo completo cae y reposa; solo-collider queda fijo; RB sin collider cae (ver hallazgo de masa).
- **Punto 7 — cerrar paneles por la X.** Causa raíz: `is_closeable` devolvía false para toda pestaña. Ahora devuelve true, `on_close` registra la pestaña cerrada y un drenado la aplica (cerrar Frametime apaga su flag). Window reabre lo cerrado. Verificado en físico. Corrección de integración: en egui_dock 0.21.1 `on_close` devuelve `OnCloseResponse`, no bool como traía el paquete.
- **Punto 8 — paneles flotantes: YA FUNCIONABA, sin cambio de código.** egui_dock renderiza las superficies ventana como ventanas DENTRO de la ventana del editor (no ventanas nativas del sistema operativo): arrastrar una pestaña fuera del área de acople la convierte en flotante y se re-acopla arrastrándola de vuelta, verificado en físico con el Grafo de Shaders operativo dentro de la flotante. Declarado tal cual: arrastrar la pestaña fuera de la ventana de la aplicación no hace nada (es la conducta de Unity).
- **Punto 9 — botón «Gizmos».** Menú «Gizmos ▾» en la vista de escena con «Rejilla» y «Nombres de objetos»; es el ÚNICO gobierno de la rejilla (el botón «# Rejilla» salió de la barra principal; «⌖ Centrar» se queda). Verificado en físico: ambos interruptores apagan y encienden.
- **Punto 10 — fuera el «(#id)» de las placas.** La placa muestra solo el nombre de la entidad.
- **Punto 15 — el gizmo mantiene tamaño en pantalla.** La longitud de los ejes y el radio de los anillos se calculan desde los píxeles por unidad en pantalla (`longitud_eje_gizmo_mundo`, `radio_anillo_mundo`), con el mismo valor para dibujo y para hit-test. Verificado en físico: flecha de ≈78 px de cerca y ≈80 px de lejos.
- **Punto 16 — la rotación con los anillos iba invertida.** Tres diseños: (1) tangente proyectada — degenera en lo alto del anillo; (2) parámetro θ sobre la polilínea proyectada — tests en verde pero FALLÓ el físico: con la cámara casi frontal la elipse es estrecha, los arcos casi coinciden y el punto más cercano saltaba de arco (−87° arrastrando hacia abajo); (3) **intersección rayo-plano (el método de Unity)**: el rayo del cursor perfora el plano del anillo en un punto 3D único y el giro es la diferencia de su ángulo entre frames (el eje Y lleva signo invertido por la parametrización zurda (X,Z) de su anillo; saltos >60° se descartan). Verificado en físico con el gesto exacto del reporte: arrastrando hacia abajo SOBRE el arco medido por píxeles, Rotación X = +104,7° siguiendo a la mano (antes −87°); desde atrás, +78°. Lección registrada: tres veces en esta ronda un «fallo» era un agarre sintético mal medido, no el código — para elementos finos se extraen los píxeles reales de la captura. Corrección post-auditoría: los 10 argumentos de la función se agruparon en `CamaraAnillo` por el lint `too_many_arguments` de clippy (mismo cálculo; los 6 tests del anillo, incluido el caso físico exacto, siguen en verde).
- **Punto 2 — el selector ofrecía las escenas de desarrollo y abrir una cerraba el motor.** Parte 1 (selector): la puerta va en la enumeración, no en la carga — `available_scenes` devuelve vacío si la raíz no es proyecto (`project.glass` o `project.json`); en un proyecto solo se ofrecen sus escenas (verificado: exactamente «FisicaP2» y «Principal»). Parte 2 (el cierre): se reprodujo un segfault por la vía `--scene` (hilo llvmpipe tras cargar una escena con modelos) y se determinó su origen con la base pre-ronda `a19062a` compilada en un worktree: la base SOBREVIVE la misma escena y el binario de la ronda caía en 1,6 s — **era una regresión de la propia ronda**: la recarga bajo demanda (hallazgo siguiente) disparaba en frío en arranques sin escaneo inicial de modelos y la subida a GPU a mitad del primer frame segfaultaba el driver. Arreglo de raíz en el punto siguiente. Tras el arreglo: `--scene` sobrevive 45 s con 0 recargas, y BakeGhost, CalibTest, MicroAnim, MicroMcp e InstantiateRegenTest sobreviven 30 s cada una. El cierre exacto que vio D3m3tr10 en Windows no se reprodujo en esta VM con la build anterior; con la puerta del selector, la vía que lo produjo (abrir una escena de desarrollo desde una build instalada) ya no existe.
- **HALLAZGO del barrido 1 — las escenas del proyecto no cargaban desde el panel («No se pudo cargar»).** Causa raíz: el SceneManager se ancla una vez al arrancar subiendo desde el CWD; en el hijo separado el CWD es el del Hub y las cargas por nombre resolvían contra las escenas del motor. Arreglo: `abrir_proyecto` re-ancla el gestor a la raíz del proyecto (punto único, con log y error visible si el mutex está envenenado), y el VFS lista/resuelve también en `Assets/Scenes`. Auditoría de Kimi: el posible autodeadlock se verificó no alcanzable en el grafo de llamadas. Verificado en físico.
- **HALLAZGO del barrido 2 — un Rigidbody sin Collider no caía.** Causa raíz: `PhysicsWorld::set_mass` reescala densidades de colliders; sin colliders devolvía false sin fijar nada y el cuerpo quedaba con masa inversa cero. Arreglo: sin colliders se aplican propiedades de masa de un cubo de 1 m con la masa pedida. Test nuevo en verde y retest físico: el cubo sin collider cayó (Y de 3,00 a −7,46 en vivo) y al Detener la escena se restauró por snapshot.
- **HALLAZGO del barrido 3 — modelos expulsados por ociosos que volvían como cubos para siempre.** `evict_idle` (600 ticks) borra todo el modelo de la caché y nada lo recargaba al volver a una escena que lo referenciaba. Arreglo: recarga bajo demanda (máx. 1 por frame) que resuelve el .glb por stem entre las raíces del proyecto y del motor y lo carga por el mismo camino que la recarga del vigilante. **Y su propia regresión, cazada por el punto 2**: sin puerta, la recarga trataba «nunca cargado» como «expulsado» y cargaba en frío durante el arranque standalone → segfault. Puerta final: la recarga SOLO deshace expulsiones reales de `evict_idle` en este proceso (set `modelos_expulsados`, alimentado en el bloque de expulsión). Verificado en ambos sentidos: en el editor, expulsados los 3 modelos del proyecto, al volver a su escena se recargaron y se ven; en `--scene`, 0 recargas y sin cierre.
- **Observación preexistente (no es de la ronda)**: los nombres por defecto del menú «Crear ▸» no se hacen únicos; si el nombre ya existe, la creación falla con aviso en Consola.
- **Auditorías**: Kimi auditó el diff de la parte 2 («APTO»), el arreglo de escenas (con la disposición de arriba) y el DIFF FINAL COMPLETO de la ronda («APTO», 23.312 tokens). **Verificación**: pipeline local exacto 5/5 sobre el árbol definitivo — 807 tests, 36 suites, 0 fallos. **Test físico**: el de cada punto, citado en su línea, más el barrido anti-regresión (undo, modos W/E/R, picking, vista de juego sin rejilla, selector, creación contextual).
- **Consumo total de la ronda (partes 1 + 2 + barrido + cierre): Opus ≈ 133.449 · DeepSeek flash ≈ 57.249 · Kimi k3.1-pro ≈ 119.967. Total ≈ 310.665 tokens.** (La corrección de `CamaraAnillo` y la puerta `modelos_expulsados` son de Rayo, sin consumo de API.)

---

# Separación verdadera — V1: el Hub como programa propio (2026-11-18)

Diseño aprobado por D3m3tr10 con las opciones recomendadas (D-V1a Vulkan+egui sin escena · D-V2 nombre `glass_hub` · D-V3a sin argumentos = error claro · D-V4 shim solo en hub-v0.2.0). Documento: `fase1/diseno_separacion_verdadera.md`.

**Hecho en V1:**
- **Crate nuevo `glass_launcher`** (infraestructura compartida del lanzador): `updates.rs` y `plugins.rs` MOVIDOS desde glass_editor (git los detecta como renames puros, 0 cambios de línea); `archivos.rs` con los ayudantes de archivos/proyectos extraídos del panel (`nombre_proyecto`, `abrir_carpeta_*`, `abrir_en_explorador`, `renombrar_proyecto`, `eliminar_proyecto_del_disco`, `canon`, `default_projects_dir`); `setup_editor_fonts` movida con las fuentes incrustadas (los assets/fonts del editor se eliminaron; viven en glass_launcher). `abrir_carpeta_proyecto` NO fue a archivos: devuelve `LauncherAction` y vive en el panel del Hub.
- **Crate nuevo `glass_hub`**: lib con el panel (`launcher.rs` movido desde glass_editor, solo cambian las rutas de módulo) y binario con cáscara propia (main.rs, 731 líneas): ventana winit vía `glass_core::GlassWindow`, `VulkanContext` + `SwapchainContext` de glass_renderer, render pass/framebuffers espejo de los de GlassRenderer, `egui-ash-renderer` con las mismas opciones (srgb_framebuffer: true), despacho de las 4 acciones del panel (abrir/crear/ejemplo/escena) lanzando el motor hijo con la maquinaria S6, y aplicación de la actualización del Hub (AccionHub) con salida del proceso. SIN GlassRenderer, sin escena, sin C#, sin física. Acepta `--hub` como no-op (compatibilidad) y `--tras-actualizar-hub` anunciando el arranque antes de crear el panel.
- **glass_editor** re-exporta `updates`, `plugins` y `setup_editor_fonts` desde glass_launcher: todas las rutas internas y las de glass_app (`glass_editor::updates::…`) siguen resolviendo sin tocarlas. **glass_app** conserva su Hub en proceso en V1, importando el panel desde `glass_hub::launcher` (V2 retirará ese modo).
- **Auditoría de Kimi (12.870 tokens): 2 hallazgos medios, misma raíz** — un error a mitad del frame (submit o grabación) dejaba la sincronización en estado desconocido y el bucle continuaba: corregido de raíz haciendo el error de render FATAL (log + salida), la misma semántica que glass_app.
- **Test físico PASS**: el `glass_hub` autónomo abre su ventana (tema claro, tarjeta «Repro Blend», pie con versión) como proceso propio; un clic en la tarjeta (la tarjeta abre con UN clic, como en el Hub combinado) lanza el motor instalado 0.10.99 como hijo y el editor abre el proyecto con su escena. Regresión: el Hub en proceso de glass_app renderiza idéntico (misma captura) con el panel importado. Los tests se conservan: 807 en total, reubicados (glass_editor 72 + glass_launcher 114 + glass_hub 9 = los 195 de antes).
- **Consumo V1**: Opus 43.893 (la cáscara) · Kimi k3.1-pro 12.870 (auditoría) · DeepSeek flash 1.137 (una llamada mal planteada por Rayo — se le pidió editar archivos y la API no tiene sistema de archivos; el barrido mecánico lo hizo Rayo con scripts; se declara la desviación). Total V1 ≈ 57.900 tokens.

---

# Defecto de campo (2026-11-18): «Buscar actualizaciones» → «the response body is larger than request limit: 10485760»

**Veredicto: el Hub publicado (hub-v0.1.1) NO tiene el defecto; el usuario ejecuta un lanzador anterior al arreglo.** Causa raíz: en la build de su época (su motor registrado es v0.10.16.2-beta), `install_release` descargaba el zip del motor con `get_bytes` → `read_to_vec()` SIN elevar el límite de ureq: cualquier zip (23–29 MB) supera el tope por defecto de 10 MiB (10485760) y la instalación muere con ese mensaje. La comprobación en sí no puede producirlo: la lista real de releases del repo mide 155 KB (15 releases, medido el 2026-11-18). Ese lanzador tampoco puede autocurarse: su actualización del propio Hub descargaba el setup por la misma vía capada.

**Verificación contra la build publicada y el repo real (2026-11-18):** AppImage oficial de hub-v0.1.1 extraído y ejecutado con un hogar falso (motor en uso antiguo). La red de la VM sale por un proxy con MITM TLS que el binario no acepta (webpki), así que se sirvió la API real a través de un espejo local (`GLASS_UPDATE_API`): misma lista real de releases capturada en vivo y las URLs de descarga reescritas al espejo, que baja los assets reales de GitHub. Resultado: la comprobación ofreció «Hay una versión nueva: v0.10.18.2-beta», y «Descargar e instalar» bajó el zip real (28,8 MB), verificó su SHA-256, lo instaló en `engines/0.10.18.2-beta/` (83 MB, con `tools/`) y dejó `en_uso` en la nueva versión. **Sin error de límite en ningún paso.**

**Salida para el usuario:** instalar a mano UNA vez un setup actual — el del Hub hub-v0.1.1 (recomendado: es la release del Hub) o el de la release v0.10.18.2-beta (trae el mismo código ya arreglado y siembra el motor nuevo) —; su registro de motores y proyectos se conserva. **No hace falta hotfix**: la build publicada está sana. Nota para V6: el shim de migración debe contar con lanzadores pre-arreglo que no pueden autodescargarse nada de más de 10 MB.

## Separación verdadera V2 — glass_app sin modo Hub (2026-11-18)

- Fuera de `glass_app`: el panel del lanzador en proceso, `AppMode`, `es_hub`, `pending_launcher_action`, el despacho `ejecutar_accion_hub`/`lanzar_motor_hijo`, `es_modo_hub`, la rama `--hub` de `validar_argumentos` y la dependencia `glass_hub` (también la entrada huérfana de workspace.dependencies). Borrado el `glass_editor/src/launcher.rs` residual de V1.
- El motor conserva su **pantalla de carga propia** (`cargando`/`carga_dibujada` en App; réplica de la antigua splash: fondo claro, «GlassEnginer», línea de acento, «Cargando el motor…») durante la precarga, y un tipo interno **`AccionApertura`** (LoadSampleScene/CreateProject/OpenProject/OpenSceneFile) para las aperturas sembradas por la CLI.
- **Puerta D-V3a**: sin destino (ni `--project`, `--scene`, `--sample`, standalone ni modo de prueba/MCP) o con `--hub`, glass_app sale con el error claro que apunta al Hub. Auditoría: también salen con error el `--project` sin `project.glass` y el `--sample` con clave desconocida; los flags MCP se comprueban exactos (`--mcp`, `--mcp-stdio`, `--mcp-live`).
- «Volver al Hub» y el cierre de ventana ahora **cierran el proceso siempre** (con escena sucia sigue pidiendo Guardar/No guardar/Cancelar); el Hub es otro programa (el reencuentro es V3). El registro del proyecto abierto lo hace `abrir_proyecto` con un `LauncherStore` cargado al vuelo.
- El mantenimiento S4 vive en el binario del Hub con el orden contractual (reconciliar → procesar → anunciar tras-actualizar → limpiar respaldo), y la cáscara del Hub alimenta la banda de aviso GPU de su panel desde su propio contexto. El diagnóstico de la sonda GPU del editor queda en su log.
- Auditoría Kimi: NO APTO inicial con 4 hallazgos — 3 corregidos de raíz (puerta, orden S4, aviso GPU) y 1 rechazado con motivo (en la build pura sin editor, `--hub` es un flag desconocido en un binario de juego cuyo arranque sin argumentos es la escena por defecto por diseño; la puerta D-V3a es del binario del editor).
- Pipeline local completo 5/5; test físico: los 4 errores de la puerta por CLI con su mensaje exacto; el Hub autónomo lanzó el motor nuevo, que mostró su pantalla de carga propia y abrió el proyecto «Repro Blend» en el editor.
