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
