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

**Paquete**: Opus 48.064 tokens (14 pares; corrección de integración de
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

**Consumo B9: Opus 48.064** (la corrección del destino contextual y el
test físico, de Rayo: 0 tokens de API).
