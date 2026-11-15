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
que llevara «Collider».

**Regla exacta de D3m3tr10 (aclaración 2026-11-14 20:53): objeto sin
Rigidbody y sin CharacterController → SIN física.** La física, gravedad
incluida, entra solo por el RB o por el CC. (La primera versión de este
arreglo dejaba un cuerpo estático a los objetos con solo Collider;
verificado contra la aclaración, eso NO cumplía la regla y se ajustó
en el mismo bloque.) Arreglo, solo en la vía de autorado — la carga de
escenas no se toca: el bloque `physics` explícito de una escena es la
declaración de física de su autor y se respeta tal cual:

- Las tres primitivas nacen ahora solo con `["Transform",
  "MeshRenderer"]`: sin bloque de física.
- En `EntityData::new`, el bloque de física se crea SOLO si hay un
  RigidBody explícito; «Collider» o «StaticCollider» sueltos no crean
  nada (ni cuerpo ni collider).
- Añadir el Rigidbody (Inspector o Component → Physics) crea el bloque
  completo (`PhysicsData::default()`), como ya hacía el editor.
- El spawn por código (`RuntimeBridge.SpawnEntity` con tipo STATIC)
  materializa su bloque explícitamente en `spawn_entity`: pedir física
  por código es la vía legítima, y sus tests (`spawn_bridge_tests`)
  distinguen estático/dinámico/sin física como antes.
- Regresión: `regression_physics_tests` en scene.rs (4 tests): con RB →
  Dynamic; Collider solo → sin física; StaticCollider solo → sin
  física; sin componentes → sin física.

En vivo: un Cube recién creado no tiene sección de física en el
Inspector (no hay bloque); al añadirle el Rigidbody por Component →
Physics aparece «Tipo de Cuerpo: Dynamic».

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

## Análisis de causa raíz y auditoría de los arreglos

Pedido por D3m3tr10 (2026-11-14 22:05): nada de parches básicos; cada
arreglo debe eliminar el mecanismo que causa el fallo.

### Desencadenantes (mecanismo, no síntoma)

1. **Física sin Rigidbody.** Dos causas acopladas: (a) los menús de
   creación inyectaban «RigidBody» en la lista de componentes de cada
   primitiva; (b) `EntityData::new` trataba la lista de componentes
   como declaración de física con opt-out: cualquiera de
   RigidBody/Collider/StaticCollider creaba un `PhysicsData`, y el tipo
   por defecto era Dynamic. La física se tenía «a menos que nada la
   pidiera», al revés de Unity.
2. **Play no llevaba a Game.** Existían DOS representaciones de «qué
   pestaña se ve»: el `DockState` de egui_dock (la verdad visible) y
   `ViewportPanel.active_tab` (un espejo legado que cada función de
   render sobrescribe con su propio valor). Ningún camino de Play
   tocaba el DockState.
3. **Stop no restauraba.** Un único `SceneState` mutable compartido
   entre edición y simulación: los scripts y la física lo mutan en el
   sitio durante el Play, y el Stop histórico solo reseteaba al jugador
   porque nunca existió una foto del estado previo.
4. **Navegación distinta de Unity.** La capa de gestos del viewport se
   escribió como esquema propio (órbita con cualquier arrastre) en vez
   del mapa modal de Unity; varios gestos (vuelo WASD, F, dolly al
   cursor) directamente no tenían camino de código.
5. **Clippy durante el ajuste del defecto 1**
   (`field_reassign_with_default`, 1 error real + 2 líneas resumen de
   «could not compile»): mi materialización del bloque STATIC en
   `spawn_entity` construía con `Default::default()` y reasignaba el
   campo después. Y el error de compilación E0499 al integrar
   Play/Stop: la detección de transiciones se insertó dentro del bucle
   de frame, donde `self.renderer` está prestado todo el frame.

### Auditoría arreglo a arreglo

- **Defecto 1 — DE RAÍZ.** La física pasa a ser opt-in en el único
  constructor (`EntityData::new`: bloque solo con RigidBody), los
  menús ya no inyectan componentes de física, y las únicas
  construcciones de `PhysicsData` del árbol son las cuatro vías
  legítimas (constructor con RB, «Añadir Rigidbody» del Inspector y del
  menú Component, y el spawn por código). Verificado por grep: no
  queda ninguna creación implícita.
- **Defecto 2 — DE RAÍZ.** El arreglo actúa sobre el `DockState`
  (`focus_game_tab`), que es donde vive la verdad visible; el espejo
  `active_tab` queda como lo que es (cada pestaña lo reafirma al
  dibujarse) y ya no decide nada en este flujo.
- **Defecto 3 — DE RAÍZ.** Foto al entrar y restauración completa al
  salir reutilizando la MISMA secuencia de teardown/rebuild que la
  carga de escenas (no una segunda implementación que pueda derivar),
  con la detección centralizada al final del frame — lo que además
  resolvió el E0499 sin préstamos parciales ni `unsafe`. **Retirado el
  mecanismo legado** `request_reset_to_spawn` (bandera + 3 emisores +
  bloque consumidor que teletransportaba al jugador a (0, 0,5, 0)):
  quedó 100 % subsumido por la restauración y mantener dos mecanismos
  de Stop era el verdadero riesgo.
- **Defecto 4 — DE RAÍZ.** La capa de gestos se reescribió al mapa
  modal de Unity, con guardas Alt en los otros dos consumidores del
  puntero (selección y arrastre del gizmo) para que los modos no se
  filtren entre sí.
- **Comparador — DE RAÍZ.** El mecanismo era la gramática estricta de
  3 componentes de `semver`; se sustituyó el parser por uno propio con
  la gramática del proyecto y sus tests, y se retiró la dependencia
  `semver` de `glass_editor` (quedaba muerta; el propio clippy la
  señaló).
- **Clippy — DE RAÍZ, sin silencios.** El `field_reassign_with_default`
  se corrigió declarando el campo distinto en el propio inicializador
  (`..Default::default()`). En todo el diff del bloque no hay ni un
  solo `#[allow]` (verificado por grep sobre `1aac729..HEAD`).

## Consumo de APIs del bloque

- Sonnet: fix1 5.829 + fix4 12.627 + fixver 7.834 = **26.290**
- Opus: fix23 (1ª vuelta, parcial) 12.783 + fix23b (completa) 12.797 =
  **25.580**
- Total: **51.870** tokens. La integración, los ajustes quirúrgicos
  (dolly real de la rueda, vuelo fuera del arrastre, `focus_game_tab`,
  reubicación de la detección al final del frame por el préstamo del
  renderer) y toda la verificación en vivo: trabajo directo de Rayo.
