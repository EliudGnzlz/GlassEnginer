# Registro — Bloque de los 4 defectos del primer usuario externo (2026-11-14)

La 0.10.0-beta la probó un usuario externo (Linux) y reportó 4 defectos.
Por orden de D3m3tr10 todo lo demás se detuvo y este bloque fue el único
activo hasta cerrarlo. Cada arreglo está reproducido contra el código,
probado con tests y **verificado en vivo en el editor** con input
sintético (capturas en `fase1/test-fixes/` fuera del repo).

## Defecto 1 — Todos los objetos tenían física sin agregar Rigidbody

Causa raíz: los menús de creación del editor (Cube/Sphere/Cylinder en
`docking.rs`) nacían con el componente «RigidBody» puesto, y
`EntityData::new` (glass_core/scene.rs) daba cuerpo «Dynamic» a todo lo
que llevara «Collider». Arreglo (solo defectos de autorado; la carga de
escenas no se toca — las escenas autoradas usan el string `body_type`
como verdad):

- Las tres primitivas nacen ahora con `["Transform", "MeshRenderer",
  "Collider"]`, sin RigidBody.
- En `EntityData::new`, el cuerpo solo es «Dynamic» si hay un RigidBody
  explícito; «Collider»/«StaticCollider» sin RigidBody → «Static»
  (choca pero no se mueve), la semántica de Unity.
- Regresión: `regression_physics_tests` en scene.rs (4 tests).

En vivo: un Cube recién creado muestra en el Inspector «Tipo de Cuerpo:
Static»; elevado a Y=6 se queda en Y=6,00 con velocidad (0,0,0) durante
todo el Play; al añadirle el Rigidbody por Component → Physics pasa a
«Dynamic».

Nota observada (fuera del alcance de este bloque): los cuerpos de las
entidades creadas en el editor se construyen al cargar/recargar la
escena, no al entrar en Play; queda registrada como limitación conocida.

## Defecto 2 — Al dar Play no llevaba a la pantalla de juego

Causa raíz: la vista principal es una pestaña del dock de egui
(`EditorTab::GameView`); el campo `viewport.active_tab` es solo un espejo
que cada pestaña reafirma al dibujarse, así que fijarlo no cambiaba lo
visible. Arreglo:

- `EditorDockingLayout::focus_game_tab()` (docking.rs): localiza la
  pestaña GameView en el `DockState` y la trae al frente
  (`find_tab` + `set_active_tab` de egui_dock).
- La app lo llama en la transición Edit→Playing.

En vivo: al dar Play la vista principal pasa sola a la pestaña de Juego
y se ve el juego por su cámara (HUD de Plat3D incluido).

## Defecto 3 — Lo movido en Play no volvía al dar Stop

Causa raíz: la simulación muta el mismo `SceneState` que se edita; el
Stop histórico solo reseteaba al jugador. Arreglo (glass_app/main.rs):

- Detección de transiciones centralizada al FINAL del frame (junto a
  `apply_pending_scene_load`, donde el renderer ya no está prestado):
  función pura `play_transition(prev, now)` con test de las 9
  combinaciones. Las tres vías de UI no se tocan.
- Edit→Playing: foto (`clone`) del `SceneState` + foco a GameView.
- Playing|Paused→Edit: `restore_play_snapshot` sustituye la escena por
  la foto y reconstruye física y scripts espejando el teardown de
  `apply_pending_scene_load` (audio stop, instancias destruidas,
  skinning reseteado, `rebuild_physics_from_scene`,
  `instantiate_scene_scripts` con la lista compuesta desde la escena
  restaurada).

En vivo: el Jugador de Plat3D_L2 pasó de Y=0,60 (autorizado) a Y=0,9989
durante el Play y volvió a **Y=0,60 exactos** al dar Stop; log «Stop:
escena restaurada a su estado previo a Play (N entidades)».

## Defecto 4 — La navegación de la escena no era la de Unity

Verificado contra `viewport.rs`: órbita sin Alt, sin vuelo WASD, sin F.
Esquema implementado (vista de escena):

- Alt + arrastrar izquierdo: orbitar. Alt + arrastrar derecho:
  zoom/dolly por distancia. Botón central: pan (la herramienta Mano con
  izquierdo también sigue haciendo pan). Rueda: dolly hacia el punto
  bajo el cursor (rayo por NDC con el FOV de la vista, 45°).
- Botón derecho mantenido: mirar desde el ojo (el ojo no se mueve) +
  vuelo WASD/QE (Shift = rápido), fuera del bloque de arrastre para que
  baste mantener el botón.
- F: enfocar la entidad seleccionada (reutiliza el camino de
  `frame_selected_request` del menú contextual).
- Guardas: con Alt, el clic no selecciona y no inicia arrastre de eje
  del gizmo; el izquierdo sin Alt ya no orbita (selección y gizmo).

En vivo, gesto a gesto con capturas antes/después: órbita ✓, zoom
Alt+derecho ✓, pan central ✓, rueda con dolly al cursor ✓, vuelo
derecho+W ✓, F enfoca al Jugador ✓.

## Comparador de versiones (decisión B de D3m3tr10)

La versión corregida es **0.10.14.1-beta** (su esquema: 10, día 14,
primera). El `semver` estricto rechazaba el cuarto componente
(verificado: `is_newer("v0.10.14.1-beta", "0.10.0-beta")` daba false).
`updates.rs` tiene ahora parser propio (`VersionKey`: 3 o 4 componentes
+ prerelease con reglas semver) y `is_newer` lo usa en ambos lados:
0.10.14.1 > 0.10.14 > 0.10.0. Su amigo bajará esta una vez a mano (su
Launcher 0.10.0 no la detecta); desde esta versión la auto-actualización
ya acepta el formato de 4 componentes.

## Consumo de APIs del bloque

- Sonnet: fix1 5.829 + fix4 12.627 + fixver 7.834 = **26.290**
- Opus: fix23 (1ª vuelta, parcial) 12.783 + fix23b (completa) 12.797 =
  **25.580**
- Total: **51.870** tokens. La integración, los ajustes quirúrgicos
  (dolly real de la rueda, vuelo fuera del arrastre, `focus_game_tab`,
  reubicación de la detección al final del frame por el préstamo del
  renderer) y toda la verificación en vivo: trabajo directo de Rayo.
